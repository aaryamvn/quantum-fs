use std::sync::Arc;

use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm as Gcm, Nonce,
};
use hkdf::Hkdf;
use sha2::Sha256;
use x_wing::{Decapsulate, Decapsulator, KeyExport};
use zeroize::{Zeroize, Zeroizing};

use crate::{
    crypto::{
        aead::PairKeyHandle,
        sign::{PureMlDsa, RustCryptoPureMlDsa, WRAP_CONTEXT},
    },
    encoding,
    ids::{Epoch, PeerId},
    keystore::{random_bytes, KeyStore},
    Error, Result,
};

pub const ROTATION_INTERVAL_SECS: u64 = 7 * 24 * 60 * 60;

#[derive(Clone, PartialEq, Eq)]
pub struct WrapMessage {
    pub kem_ct: Vec<u8>,
    pub wrap_ct: Vec<u8>,
    pub epoch: Epoch,
    pub min_id: PeerId,
    pub max_id: PeerId,
    pub signature: Vec<u8>,
}

pub struct PairSession {
    pub peer_id: PeerId,
    pub epoch: Epoch,
    key_handle: PairKeyHandle,
}

impl PairSession {
    pub fn new(peer_id: PeerId, epoch: Epoch, key_handle: PairKeyHandle) -> Self {
        Self {
            peer_id,
            epoch,
            key_handle,
        }
    }
    pub fn key_handle(&self) -> &PairKeyHandle {
        &self.key_handle
    }
}

/// X-Wing only. Valid-size ciphertexts under a wrong dk implicitly reject by
/// producing a different secret; Construction B's GCM tag detects that failure.
pub trait XWing {
    fn encapsulate(&self, peer_ek: &[u8]) -> Result<(SharedSecretHandle, Vec<u8>)>;
    fn decapsulate(
        &self,
        private_key: &XWingPrivateKeyHandle,
        kem_ct: &[u8],
    ) -> Result<SharedSecretHandle>;
}

pub struct SharedSecretHandle {
    secret: Zeroizing<[u8; 32]>,
}

#[derive(Clone)]
pub struct XWingPrivateKeyHandle {
    seed: Arc<Zeroizing<[u8; 32]>>,
}

impl XWingPrivateKeyHandle {
    pub fn generate() -> Result<Self> {
        Ok(Self::from_seed(random_bytes()?))
    }

    pub(crate) fn from_seed(mut seed: [u8; 32]) -> Self {
        let handle = Self {
            seed: Arc::new(Zeroizing::new(seed)),
        };
        seed.zeroize();
        handle
    }

    pub(crate) fn seed(&self) -> &[u8; 32] {
        self.seed.as_ref()
    }

    fn expanded(&self) -> x_wing::DecapsulationKey {
        // Both the borrowed seed copy and expanded key zeroize on drop.
        let copy = Zeroizing::new(*self.seed());
        x_wing::DecapsulationKey::from(*copy)
    }

    pub fn public_key(&self) -> Vec<u8> {
        self.expanded().encapsulation_key().to_bytes().to_vec()
    }
}

pub fn validate_public_key(bytes: &[u8]) -> Result<()> {
    x_wing::EncapsulationKey::try_from(bytes)
        .map(|_| ())
        .map_err(|_| Error::AuthenticationFailed)
}

pub struct RustCryptoXWing;

impl XWing for RustCryptoXWing {
    fn encapsulate(&self, peer_ek: &[u8]) -> Result<(SharedSecretHandle, Vec<u8>)> {
        let ek =
            x_wing::EncapsulationKey::try_from(peer_ek).map_err(|_| Error::AuthenticationFailed)?;
        // All 64 bytes are freshly sampled from the OS with checked entropy errors.
        // This is the X-Wing API, never a direct invocation of its component KEM.
        let random = Zeroizing::new(random_bytes::<64>()?);
        let randomness = Zeroizing::new((*random).into());
        let (ciphertext, mut secret) = ek.encapsulate_deterministic(&randomness);
        let shared = SharedSecretHandle {
            secret: Zeroizing::new(
                secret
                    .as_slice()
                    .try_into()
                    .map_err(|_| Error::State("invalid X-Wing shared secret length"))?,
            ),
        };
        secret.zeroize();
        Ok((shared, ciphertext.to_vec()))
    }

    fn decapsulate(
        &self,
        private_key: &XWingPrivateKeyHandle,
        kem_ct: &[u8],
    ) -> Result<SharedSecretHandle> {
        let ciphertext =
            x_wing::Ciphertext::try_from(kem_ct).map_err(|_| Error::AuthenticationFailed)?;
        let mut secret = private_key.expanded().decapsulate(&ciphertext);
        let shared = SharedSecretHandle {
            secret: Zeroizing::new(
                secret
                    .as_slice()
                    .try_into()
                    .map_err(|_| Error::State("invalid X-Wing shared secret length"))?,
            ),
        };
        secret.zeroize();
        Ok(shared)
    }
}

/// One independent random K_ab protects both packets and chunks. HKDF derives
/// only a single-use wrapping key; neither the KEM secret nor wrapping key is
/// persisted. Retries copy the cached signed wrap without another Encaps.
/// Static ek rotation is not forward secrecy.
pub trait ConstructionBWrap {
    fn create(
        &self,
        peer_id: PeerId,
        peer_ek: &[u8],
        epoch: Epoch,
    ) -> Result<(PairSession, WrapMessage)>;
    fn unwrap(&self, sender_id: PeerId, message: &WrapMessage) -> Result<PairSession>;
    fn retry(&self, message: &WrapMessage) -> Result<WrapMessage>;
}

pub struct RustCryptoConstructionBWrap {
    store: KeyStore,
    kem: Arc<dyn XWing + Send + Sync>,
}

impl RustCryptoConstructionBWrap {
    pub fn new(store: KeyStore) -> Self {
        Self::with_kem(store, Arc::new(RustCryptoXWing))
    }

    pub fn with_kem(store: KeyStore, kem: Arc<dyn XWing + Send + Sync>) -> Self {
        Self { store, kem }
    }

    pub fn create_candidate(
        &self,
        peer_id: PeerId,
        peer_ek: &[u8],
        epoch: Epoch,
    ) -> Result<(PairSession, WrapMessage)> {
        self.create_inner(peer_id, peer_ek, epoch, true)
    }

    pub fn unwrap_candidate(
        &self,
        sender_id: PeerId,
        message: &WrapMessage,
    ) -> Result<PairSession> {
        self.unwrap_inner(sender_id, message, true)
    }

    pub fn retry_candidate_collision(&self, peer_id: PeerId) -> Result<(PairSession, WrapMessage)> {
        let (epoch, ek) = {
            let inner = self.store.lock()?;
            (
                inner
                    .candidate_retry_epochs
                    .get(&peer_id)
                    .copied()
                    .ok_or(Error::State("no losing candidate wrap to retry"))?,
                inner
                    .peers
                    .get(&peer_id)
                    .ok_or(Error::KeyUnavailable)?
                    .ek
                    .clone(),
            )
        };
        self.store.discard_candidate(peer_id)?;
        self.create_candidate(peer_id, &ek, epoch)
    }

    fn create_inner(
        &self,
        peer_id: PeerId,
        peer_ek: &[u8],
        epoch: Epoch,
        candidate: bool,
    ) -> Result<(PairSession, WrapMessage)> {
        let mut inner = self.store.lock()?;
        let local_id = inner.local.document.peer_id;
        let peer = inner.peers.get(&peer_id).ok_or(Error::KeyUnavailable)?;
        if peer.ek != peer_ek {
            return Err(Error::AuthenticationFailed);
        }
        if candidate && inner.candidates.contains_key(&peer_id) {
            return Err(Error::State("peer admission candidate already exists"));
        }
        if !inner.pairs.contains_key(&peer_id) && local_id >= peer_id {
            return Err(Error::State("smaller peer initiates first contact"));
        }
        let next = inner
            .epoch_watermarks
            .get(&peer_id)
            .copied()
            .unwrap_or(Epoch(0))
            .0
            .checked_add(1)
            .ok_or(Error::State("epoch exhausted"))?;
        if epoch.0 != next {
            return Err(Error::InvalidInput("wrap must use the next epoch"));
        }
        let min_id = local_id.min(peer_id);
        let max_id = local_id.max(peer_id);
        let pair_key = Zeroizing::new(random_bytes::<32>()?);
        let (secret, kem_ct) = self.kem.encapsulate(peer_ek)?;
        let wrap_key = derive_wrap_key(&secret, min_id, max_id, epoch)?;
        let cipher =
            Gcm::new_from_slice(wrap_key.as_ref()).map_err(|_| Error::State("invalid wrap key"))?;
        let wrap_ct = cipher
            .encrypt(
                &Nonce::from(encoding::WRAP_GCM_NONCE),
                Payload {
                    msg: pair_key.as_ref(),
                    aad: &encoding::wrap_aad(&min_id, &max_id, epoch)?,
                },
            )
            .map_err(|_| Error::AuthenticationFailed)?;
        let mut message = WrapMessage {
            kem_ct,
            wrap_ct,
            epoch,
            min_id,
            max_id,
            signature: Vec::new(),
        };
        message.signature = RustCryptoPureMlDsa.sign(
            &inner.local.signing_key,
            WRAP_CONTEXT,
            &encoding::wrap_m(&message)?,
        )?;
        if candidate {
            inner.candidate_retry_epochs.remove(&peer_id);
        } else {
            inner.retry_epochs.remove(&peer_id);
        }
        let session = if candidate {
            inner.install_candidate(peer_id, local_id, message.clone(), pair_key)?
        } else {
            inner.install(peer_id, local_id, message.clone(), pair_key)?
        };
        Ok((session, message))
    }

    fn unwrap_inner(
        &self,
        sender_id: PeerId,
        message: &WrapMessage,
        candidate: bool,
    ) -> Result<PairSession> {
        let mut inner = self.store.lock()?;
        let local_id = inner.local.document.peer_id;
        let sender = inner.peers.get(&sender_id).ok_or(Error::KeyUnavailable)?;
        if message.min_id != local_id.min(sender_id) || message.max_id != local_id.max(sender_id) {
            return Err(Error::AuthenticationFailed);
        }
        RustCryptoPureMlDsa.verify(
            &sender.vk,
            WRAP_CONTEXT,
            &encoding::wrap_m(message)?,
            &message.signature,
        )?;
        let existing = if candidate {
            inner.candidates.get(&sender_id)
        } else {
            inner.pairs.get(&sender_id)
        };
        let mut collision = false;
        let mut supersede = false;
        let expected = Epoch(
            inner
                .epoch_watermarks
                .get(&sender_id)
                .copied()
                .unwrap_or(Epoch(0))
                .0
                .checked_add(1)
                .ok_or(Error::State("epoch exhausted"))?,
        );
        match existing {
            None if !candidate && (sender_id >= local_id || message.epoch != expected) => {
                return Err(Error::State("invalid first-contact initiator or epoch"));
            }
            Some(previous) if message.epoch < previous.epoch => {
                return Err(Error::State("stale wrap epoch"));
            }
            Some(previous) if message.epoch == previous.epoch => {
                if previous.initiator == sender_id && previous.wrap == *message {
                    return inner.session(sender_id, message.epoch);
                }
                if previous.initiator <= sender_id {
                    return Err(Error::EpochConflict {
                        retry_epoch: Epoch(
                            message
                                .epoch
                                .0
                                .checked_add(1)
                                .ok_or(Error::State("epoch exhausted"))?,
                        ),
                    });
                }
                collision = true;
            }
            Some(_) if candidate => supersede = true,
            _ => {}
        }
        if candidate && !collision && message.epoch < expected {
            return Err(Error::State("invalid candidate initiator or epoch"));
        }
        let secret = self
            .kem
            .decapsulate(&inner.local.decapsulation_key, &message.kem_ct)?;
        let wrap_key = derive_wrap_key(&secret, message.min_id, message.max_id, message.epoch)?;
        let cipher =
            Gcm::new_from_slice(wrap_key.as_ref()).map_err(|_| Error::State("invalid wrap key"))?;
        let plaintext = Zeroizing::new(
            cipher
                .decrypt(
                    &Nonce::from(encoding::WRAP_GCM_NONCE),
                    Payload {
                        msg: &message.wrap_ct,
                        aad: &encoding::wrap_aad(&message.min_id, &message.max_id, message.epoch)?,
                    },
                )
                .map_err(|_| Error::AuthenticationFailed)?,
        );
        let key = Zeroizing::new(
            plaintext
                .as_slice()
                .try_into()
                .map_err(|_| Error::AuthenticationFailed)?,
        );
        if collision {
            let retry = Epoch(
                message
                    .epoch
                    .0
                    .checked_add(1)
                    .ok_or(Error::State("epoch exhausted"))?,
            );
            if candidate {
                inner.candidate_retry_epochs.insert(sender_id, retry);
            } else {
                inner.retry_epochs.insert(sender_id, retry);
            }
        }
        if candidate {
            if supersede || collision {
                if let Some(previous) = inner.candidates.remove(&sender_id) {
                    inner
                        .keys
                        .retain(|_, key| key.peer_id != sender_id || key.epoch != previous.epoch);
                }
                if supersede {
                    inner.candidate_retry_epochs.remove(&sender_id);
                }
            }
            inner.install_candidate(sender_id, sender_id, message.clone(), key)
        } else {
            inner.install(sender_id, sender_id, message.clone(), key)
        }
    }

    /// Explicitly carry out the losing initiator's retry at last-before-collision+2.
    pub fn retry_collision(&self, peer_id: PeerId) -> Result<(PairSession, WrapMessage)> {
        let (epoch, ek) = {
            let inner = self.store.lock()?;
            (
                inner
                    .retry_epochs
                    .get(&peer_id)
                    .copied()
                    .ok_or(Error::State("no losing wrap to retry"))?,
                inner
                    .peers
                    .get(&peer_id)
                    .ok_or(Error::KeyUnavailable)?
                    .ek
                    .clone(),
            )
        };
        self.create(peer_id, &ek, epoch)
    }

    /// Used at least weekly by the daemon. Transport later sends pending_wraps.
    pub fn rotate_all(&self) -> Result<usize> {
        let peers: Vec<_> = {
            let inner = self.store.lock()?;
            inner
                .pairs
                .keys()
                .map(|id| {
                    let peer = inner.peers.get(id).ok_or(Error::KeyUnavailable)?;
                    Ok((*id, peer.ek.clone()))
                })
                .collect::<Result<_>>()?
        };
        for (peer_id, ek) in &peers {
            self.create(*peer_id, ek, self.store.next_epoch(peer_id)?)?;
        }
        Ok(peers.len())
    }
}

impl ConstructionBWrap for RustCryptoConstructionBWrap {
    fn create(
        &self,
        peer_id: PeerId,
        peer_ek: &[u8],
        epoch: Epoch,
    ) -> Result<(PairSession, WrapMessage)> {
        self.create_inner(peer_id, peer_ek, epoch, false)
    }

    fn unwrap(&self, sender_id: PeerId, message: &WrapMessage) -> Result<PairSession> {
        self.unwrap_inner(sender_id, message, false)
    }

    fn retry(&self, message: &WrapMessage) -> Result<WrapMessage> {
        let inner = self.store.lock()?;
        if inner
            .pairs
            .values()
            .chain(inner.candidates.values())
            .any(|pair| pair.initiator == inner.local.document.peer_id && pair.wrap == *message)
        {
            Ok(message.clone())
        } else {
            Err(Error::State("wrap is not a cached local ciphertext"))
        }
    }
}

fn derive_wrap_key(
    secret: &SharedSecretHandle,
    min_id: PeerId,
    max_id: PeerId,
    epoch: Epoch,
) -> Result<Zeroizing<[u8; 32]>> {
    let mut key = Zeroizing::new([0; 32]);
    Hkdf::<Sha256>::new(Some(&[]), secret.secret.as_ref())
        .expand(
            &encoding::wrap_hkdf_info(&min_id, &max_id, epoch)?,
            key.as_mut(),
        )
        .map_err(|_| Error::State("HKDF output length rejected"))?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    };

    #[test]
    fn kem_round_trip_and_wrong_dk_cannot_authenticate_the_wrap() -> Result<()> {
        let receiver = XWingPrivateKeyHandle::generate()?;
        let wrong = XWingPrivateKeyHandle::generate()?;
        let (secret, ciphertext) = RustCryptoXWing.encapsulate(&receiver.public_key())?;
        let received = RustCryptoXWing.decapsulate(&receiver, &ciphertext)?;
        let rejected = RustCryptoXWing.decapsulate(&wrong, &ciphertext)?;
        assert!(secret.secret.as_ref() == received.secret.as_ref());
        assert!(secret.secret.as_ref() != rejected.secret.as_ref());
        assert!(RustCryptoXWing
            .decapsulate(&receiver, &ciphertext[..100])
            .is_err());
        let min = PeerId([1; 32]);
        let max = PeerId([2; 32]);
        let aad = encoding::wrap_aad(&min, &max, Epoch(1))?;
        let correct_key = derive_wrap_key(&secret, min, max, Epoch(1))?;
        let wrong_key = derive_wrap_key(&rejected, min, max, Epoch(1))?;
        let plaintext = Zeroizing::new(random_bytes::<32>()?);
        let ciphertext = Gcm::new_from_slice(correct_key.as_ref())
            .map_err(|_| Error::State("test cipher"))?
            .encrypt(
                &Nonce::from(encoding::WRAP_GCM_NONCE),
                Payload {
                    msg: plaintext.as_ref(),
                    aad: &aad,
                },
            )
            .map_err(|_| Error::AuthenticationFailed)?;
        assert!(Gcm::new_from_slice(wrong_key.as_ref())
            .map_err(|_| Error::State("test cipher"))?
            .decrypt(
                &Nonce::from(encoding::WRAP_GCM_NONCE),
                Payload {
                    msg: &ciphertext,
                    aad: &aad
                }
            )
            .is_err());
        Ok(())
    }

    struct CountingKem {
        calls: AtomicUsize,
        secret: Mutex<Option<SharedSecretHandle>>,
    }
    impl XWing for CountingKem {
        fn encapsulate(&self, ek: &[u8]) -> Result<(SharedSecretHandle, Vec<u8>)> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let (secret, ciphertext) = RustCryptoXWing.encapsulate(ek)?;
            *self.secret.lock().map_err(|_| Error::State("test lock"))? =
                Some(SharedSecretHandle {
                    secret: Zeroizing::new(*secret.secret),
                });
            Ok((secret, ciphertext))
        }
        fn decapsulate(&self, dk: &XWingPrivateKeyHandle, ct: &[u8]) -> Result<SharedSecretHandle> {
            RustCryptoXWing.decapsulate(dk, ct)
        }
    }

    #[test]
    fn construction_b_has_independent_pair_key_and_retry_does_not_encapsulate() -> Result<()> {
        let directory = std::env::temp_dir().join(format!(
            "qfs-wrap-unit-{}",
            u64::from_be_bytes(random_bytes()?)
        ));
        std::fs::create_dir(&directory)?;
        let result = (|| -> Result<()> {
            let a = KeyStore::open(&directory.join("a"))?;
            let b = KeyStore::open(&directory.join("b"))?;
            a.import_peer(b.identity()?)?;
            b.import_peer(a.identity()?)?;
            let (low, high, state_path) = if a.peer_id()? < b.peer_id()? {
                (a, b, directory.join("a.keys"))
            } else {
                (b, a, directory.join("b.keys"))
            };
            let kem = Arc::new(CountingKem {
                calls: AtomicUsize::new(0),
                secret: Mutex::new(None),
            });
            let sender = RustCryptoConstructionBWrap::with_kem(low.clone(), kem.clone());
            let receiver = RustCryptoConstructionBWrap::new(high.clone());
            let peer = high.identity()?;
            let (local_session, message) = sender.create(peer.peer_id, &peer.ek, Epoch(1))?;
            let remote_session = receiver.unwrap(low.peer_id()?, &message)?;
            let captured = kem.secret.lock().map_err(|_| Error::State("test lock"))?;
            let wrap_key = derive_wrap_key(
                captured
                    .as_ref()
                    .ok_or(Error::State("missing test secret"))?,
                message.min_id,
                message.max_id,
                message.epoch,
            )?;
            let key = low.with_pair_state(local_session.key_handle(), |state| {
                Ok(Zeroizing::new(*state.key))
            })?;
            assert!(wrap_key.as_ref() != key.as_ref());
            high.with_pair_state(remote_session.key_handle(), |state| {
                assert!(state.key.as_ref() == key.as_ref());
                Ok(())
            })?;
            for _ in 0..3 {
                assert!(sender.retry(&message)? == message);
            }
            assert_eq!(kem.calls.load(Ordering::SeqCst), 1);
            assert_eq!(encoding::WRAP_GCM_NONCE, [0; 12]);
            // K_ab is durably stored, but no ss or wrap_key field exists in the codec.
            let state_bytes = Zeroizing::new(std::fs::read(state_path)?);
            let persisted = encoding::decode_store_state(&state_bytes)?;
            assert_eq!(persisted.keys.len(), 1);
            assert!(persisted.keys[0].key.as_ref() == key.as_ref());
            Ok(())
        })();
        let cleanup = std::fs::remove_dir_all(directory);
        result?;
        cleanup?;
        Ok(())
    }
}
