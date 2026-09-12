//! Private local persistence. Handles expose no key bytes and expire with the
//! keystore instance. Restart discards old AES slots and prepares fresh wraps.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

use rand::TryRng;
use zeroize::Zeroizing;

use crate::{
    crypto::{
        aead::{PairKeyHandle, PairKeyState},
        identity::{IdentityDocument, IdentityManager, LocalIdentity},
        sign::{PureMlDsa, RustCryptoPureMlDsa, SigningKeyHandle, WRAP_CONTEXT},
        wrap::{ConstructionBWrap, PairSession, RustCryptoConstructionBWrap, WrapMessage},
    },
    encoding,
    ids::{Epoch, PeerId, Seq},
    protocol::packet::PayloadType,
    Error, Result,
};

pub(crate) const MAX_STORE_BYTES: u64 = 16 * 1024 * 1024;
pub(crate) const MAX_ADMISSION_CANDIDATES: usize = 32;

#[derive(Clone)]
pub struct KeyStore {
    inner: Arc<Mutex<StoreInner>>,
}

pub(crate) struct StoreInner {
    pub(crate) local: LocalIdentity,
    pub(crate) peers: BTreeMap<PeerId, IdentityDocument>,
    pub(crate) pairs: BTreeMap<PeerId, PersistedPair>,
    pub(crate) candidates: BTreeMap<PeerId, PersistedPair>,
    pub(crate) keys: BTreeMap<u64, PairKeyState>,
    pub(crate) retry_epochs: BTreeMap<PeerId, Epoch>,
    pub(crate) candidate_retry_epochs: BTreeMap<PeerId, Epoch>,
    pub(crate) epoch_watermarks: BTreeMap<PeerId, Epoch>,
    pub(crate) instance_id: [u8; 32],
    pub(crate) next_slot: u64,
    pub(crate) mailbox_gates: BTreeMap<PeerId, BTreeSet<u64>>,
    admitted_pairs: BTreeMap<PeerId, Epoch>,
    identity_path: PathBuf,
    state_path: PathBuf,
    _lock: File,
    failed: bool,
}

pub(crate) struct PersistedState {
    pub(crate) local_id: PeerId,
    pub(crate) next_slot: u64,
    pub(crate) peers: Vec<IdentityDocument>,
    pub(crate) pairs: Vec<PersistedPair>,
    pub(crate) candidates: Vec<PersistedPair>,
    pub(crate) keys: Vec<PersistedKey>,
    pub(crate) epoch_watermarks: Vec<(PeerId, Epoch)>,
}

#[derive(Clone)]
pub(crate) struct PersistedPair {
    pub(crate) peer_id: PeerId,
    pub(crate) epoch: Epoch,
    pub(crate) initiator: PeerId,
    pub(crate) wrap: WrapMessage,
}

pub(crate) struct PersistedKey {
    pub(crate) slot: u64,
    pub(crate) peer_id: PeerId,
    pub(crate) epoch: Epoch,
    pub(crate) key: Zeroizing<[u8; 32]>,
}

impl KeyStore {
    /// Confirms that this live keystore owns the advisory lock for `data_dir`.
    /// Replica persistence deliberately reuses that lock instead of creating a
    /// second lock domain.
    pub(crate) fn require_data_dir_lock(&self, data_dir: &Path) -> Result<()> {
        let identity_path = self.lock()?.identity_path.clone();
        let identity_parent = identity_path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let lock_root = fs::canonicalize(identity_parent)?;
        let candidate = fs::canonicalize(data_dir)?;
        if candidate == lock_root {
            return Ok(());
        }
        let name =
            data_dir
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or(Error::InvalidInput(
                    "replica data directory differs from keystore lock directory",
                ))?;
        let is_vault_id = name.len() == 64
            && name
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        let lexical_parent = data_dir.parent();
        let is_vault_path = is_vault_id
            && lexical_parent
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                == Some("vaults")
            && lexical_parent
                .and_then(Path::parent)
                .is_some_and(|root| fs::canonicalize(root).ok().as_ref() == Some(&lock_root))
            && candidate.starts_with(&lock_root);
        if !is_vault_path {
            return Err(Error::InvalidInput(
                "replica data directory differs from keystore lock directory",
            ));
        }
        Ok(())
    }

    /// Exclusively load/create this member and prepare fresh epochs for known
    /// pairs. Canonical signed wraps are available via pending_wraps; no I/O to peers.
    pub fn open(path: &Path) -> Result<Self> {
        ensure_parent(path)?;
        let lock = private_options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(sibling(path, ".lock"))?;
        lock.try_lock()
            .map_err(|_| Error::State("identity keystore is already open"))?;
        let bytes = read_private(path)?;
        let state_path = sibling(path, ".keys");
        if bytes.as_ref().is_none_or(|bytes| bytes.is_empty()) && state_path.try_exists()? {
            return Err(Error::State("identity is missing while pair state exists"));
        }
        let local = match bytes {
            Some(bytes) if !bytes.is_empty() => encoding::decode_local_identity(&bytes)?,
            _ => {
                let local = LocalIdentity::generate()?;
                atomic_private_write(path, &encoding::encode_local_identity(&local)?)?;
                local
            }
        };
        let mut inner = StoreInner {
            local,
            peers: BTreeMap::new(),
            pairs: BTreeMap::new(),
            candidates: BTreeMap::new(),
            keys: BTreeMap::new(),
            retry_epochs: BTreeMap::new(),
            candidate_retry_epochs: BTreeMap::new(),
            epoch_watermarks: BTreeMap::new(),
            instance_id: random_bytes()?,
            next_slot: 0,
            mailbox_gates: BTreeMap::new(),
            admitted_pairs: BTreeMap::new(),
            identity_path: path.to_owned(),
            state_path,
            _lock: lock,
            failed: false,
        };
        if let Some(bytes) = read_private(&inner.state_path)? {
            let persisted = encoding::decode_store_state(&bytes)?;
            if persisted.local_id != inner.local.document.peer_id {
                return Err(Error::AuthenticationFailed);
            }
            inner.next_slot = persisted.next_slot;
            for (peer_id, epoch) in persisted.epoch_watermarks {
                if epoch == Epoch(0) || inner.epoch_watermarks.insert(peer_id, epoch).is_some() {
                    return Err(Error::InvalidInput("invalid persisted epoch watermark"));
                }
            }
            for peer in persisted.peers {
                peer.verify()?;
                if peer.peer_id == inner.local.document.peer_id
                    || inner.peers.insert(peer.peer_id, peer).is_some()
                {
                    return Err(Error::InvalidInput("duplicate or local persisted peer"));
                }
            }
            for pair in persisted.pairs {
                let peer = inner
                    .peers
                    .get(&pair.peer_id)
                    .ok_or(Error::AuthenticationFailed)?;
                let local_id = inner.local.document.peer_id;
                let signer = if pair.initiator == local_id {
                    &inner.local.document
                } else if pair.initiator == peer.peer_id {
                    peer
                } else {
                    return Err(Error::AuthenticationFailed);
                };
                if pair.epoch != pair.wrap.epoch
                    || pair.wrap.min_id != local_id.min(peer.peer_id)
                    || pair.wrap.max_id != local_id.max(peer.peer_id)
                {
                    return Err(Error::AuthenticationFailed);
                }
                RustCryptoPureMlDsa.verify(
                    &signer.vk,
                    WRAP_CONTEXT,
                    &encoding::wrap_m(&pair.wrap)?,
                    &pair.wrap.signature,
                )?;
                let pair_peer_id = pair.peer_id;
                let pair_epoch = pair.epoch;
                if inner.pairs.insert(pair_peer_id, pair).is_some() {
                    return Err(Error::InvalidInput("duplicate persisted pair"));
                }
                let watermark = inner
                    .epoch_watermarks
                    .entry(pair_peer_id)
                    .or_insert(Epoch(0));
                *watermark = (*watermark).max(pair_epoch);
            }
            if persisted.candidates.len() > MAX_ADMISSION_CANDIDATES {
                return Err(Error::InvalidInput(
                    "too many persisted admission candidates",
                ));
            }
            // Admission candidates never survive a process restart, but only
            // a canonical authenticated candidate may raise the durable floor.
            let mut candidate_peers = BTreeSet::new();
            for candidate in persisted.candidates {
                let peer = inner
                    .peers
                    .get(&candidate.peer_id)
                    .ok_or(Error::AuthenticationFailed)?;
                let local_id = inner.local.document.peer_id;
                let signer = if candidate.initiator == local_id {
                    &inner.local.document
                } else if candidate.initiator == peer.peer_id {
                    peer
                } else {
                    return Err(Error::AuthenticationFailed);
                };
                if candidate.epoch != candidate.wrap.epoch
                    || candidate.wrap.min_id != local_id.min(peer.peer_id)
                    || candidate.wrap.max_id != local_id.max(peer.peer_id)
                    || !candidate_peers.insert(candidate.peer_id)
                {
                    return Err(Error::AuthenticationFailed);
                }
                RustCryptoPureMlDsa.verify(
                    &signer.vk,
                    WRAP_CONTEXT,
                    &encoding::wrap_m(&candidate.wrap)?,
                    &candidate.wrap.signature,
                )?;
                let watermark = inner
                    .epoch_watermarks
                    .entry(candidate.peer_id)
                    .or_insert(Epoch(0));
                *watermark = (*watermark).max(candidate.epoch);
            }
            // Decoded prior K_ab bytes are zeroized here and never activated.
            drop(persisted.keys);
        }
        // Commit removal of restart-ineligible keys before preparing fresh epochs.
        inner.persist()?;
        let store = Self {
            inner: Arc::new(Mutex::new(inner)),
        };
        let pairs: Vec<_> = {
            let state = store.lock()?;
            state
                .peers
                .values()
                .filter_map(|peer| match state.pairs.get(&peer.peer_id) {
                    Some(pair) => Some((peer.clone(), Some(pair.epoch))),
                    None if state.local.document.peer_id < peer.peer_id => {
                        Some((peer.clone(), None))
                    }
                    None => None,
                })
                .collect()
        };
        let wraps = RustCryptoConstructionBWrap::new(store.clone());
        for (peer, _previous) in pairs {
            // A discarded restart candidate can be newer than the confirmed
            // pair. Always prepare above the durable maximum.
            let next = store.next_epoch(&peer.peer_id)?;
            wraps.create(peer.peer_id, &peer.ek, next)?;
        }
        Ok(store)
    }

    pub(crate) fn lock(&self) -> Result<MutexGuard<'_, StoreInner>> {
        let inner = self
            .inner
            .lock()
            .map_err(|_| Error::State("keystore lock poisoned"))?;
        if inner.failed {
            return Err(Error::State("keystore persistence failed; reopen required"));
        }
        Ok(inner)
    }

    pub fn peer_id(&self) -> Result<PeerId> {
        Ok(self.lock()?.local.document.peer_id)
    }

    pub fn identity(&self) -> Result<IdentityDocument> {
        Ok(self.lock()?.local.document.clone())
    }

    pub fn signing_key(&self) -> Result<SigningKeyHandle> {
        Ok(self.lock()?.local.signing_key.clone())
    }

    /// Only a verified identity may become a source of an encapsulation key.
    pub fn import_peer(&self, document: IdentityDocument) -> Result<()> {
        document.verify()?;
        let mut inner = self.lock()?;
        if document.peer_id == inner.local.document.peer_id {
            return Err(Error::InvalidInput(
                "cannot import the local member as a peer",
            ));
        }
        let mut rewrap = false;
        if let Some(previous) = inner.peers.get(&document.peer_id) {
            if document != *previous && document.created_at <= previous.created_at {
                return Err(Error::AuthenticationFailed);
            }
            rewrap = previous.ek != document.ek && inner.pairs.contains_key(&document.peer_id);
        }
        let peer_id = document.peer_id;
        let ek = document.ek.clone();
        inner.peers.insert(document.peer_id, document);
        inner.persist()?;
        drop(inner);
        if rewrap {
            // A cached ciphertext addressed to a retired ek cannot be retried.
            // Keep its live K_ab until drain, but prepare a new epoch for the new ek.
            let result = RustCryptoConstructionBWrap::new(self.clone()).create(
                peer_id,
                &ek,
                self.next_epoch(&peer_id)?,
            );
            if let Err(error) = result {
                self.lock()?.failed = true;
                return Err(error);
            }
        }
        Ok(())
    }

    pub fn rotate_identity_ek(&self) -> Result<IdentityDocument> {
        let mut inner = self.lock()?;
        let rotated = inner.local.rotate_ek()?;
        if let Err(error) = atomic_private_write(
            &inner.identity_path,
            &encoding::encode_local_identity(&rotated)?,
        ) {
            inner.failed = true;
            return Err(error);
        }
        inner.local = rotated;
        Ok(inner.local.document.clone())
    }

    pub fn pending_wraps(&self) -> Result<Vec<WrapMessage>> {
        let inner = self.lock()?;
        Ok(inner
            .pairs
            .values()
            .chain(inner.candidates.values())
            .filter(|pair| pair.initiator == inner.local.document.peer_id)
            .map(|pair| pair.wrap.clone())
            .collect())
    }

    /// Returns this store's durable epoch watermark for a peer, or zero when unknown.
    pub fn peer_epoch(&self, peer_id: PeerId) -> Result<Epoch> {
        Ok(self
            .lock()?
            .epoch_watermarks
            .get(&peer_id)
            .copied()
            .unwrap_or(Epoch(0)))
    }

    /// Returns the initiator and exact cached signed wrap for handshake retry.
    pub fn cached_pair(&self, peer_id: PeerId) -> Result<Option<(PeerId, WrapMessage)>> {
        Ok(self
            .lock()?
            .pairs
            .get(&peer_id)
            .map(|pair| (pair.initiator, pair.wrap.clone())))
    }

    pub fn cached_candidate(&self, peer_id: PeerId) -> Result<Option<(PeerId, WrapMessage)>> {
        Ok(self
            .lock()?
            .candidates
            .get(&peer_id)
            .map(|pair| (pair.initiator, pair.wrap.clone())))
    }

    pub fn candidate_session(&self, peer_id: PeerId) -> Result<PairSession> {
        let inner = self.lock()?;
        let epoch = inner
            .candidates
            .get(&peer_id)
            .map(|pair| pair.epoch)
            .ok_or(Error::KeyUnavailable)?;
        inner.session(peer_id, epoch)
    }

    pub(crate) fn mark_admitted(&self, peer_id: PeerId) -> Result<()> {
        let mut inner = self.lock()?;
        let epoch = inner
            .pairs
            .get(&peer_id)
            .map(|pair| pair.epoch)
            .ok_or(Error::KeyUnavailable)?;
        inner.session(peer_id, epoch)?;
        inner.admitted_pairs.insert(peer_id, epoch);
        Ok(())
    }

    pub(crate) fn has_admitted_pair(&self, peer_id: PeerId) -> Result<bool> {
        let inner = self.lock()?;
        Ok(inner.admitted_pairs.get(&peer_id).is_some_and(|epoch| {
            inner
                .pairs
                .get(&peer_id)
                .is_some_and(|pair| pair.epoch == *epoch)
                && inner.session(peer_id, *epoch).is_ok()
        }))
    }

    pub fn promote_candidate(&self, peer_id: PeerId, epoch: Epoch) -> Result<PairSession> {
        let mut inner = self.lock()?;
        let candidate = inner
            .candidates
            .remove(&peer_id)
            .ok_or(Error::KeyUnavailable)?;
        if candidate.epoch != epoch {
            inner.candidates.insert(peer_id, candidate);
            return Err(Error::State("candidate epoch changed before admission"));
        }
        if inner
            .pairs
            .get(&peer_id)
            .is_some_and(|confirmed| candidate.epoch <= confirmed.epoch)
        {
            inner.candidates.insert(peer_id, candidate);
            return Err(Error::State("candidate is not newer than confirmed pair"));
        }
        inner.pairs.insert(peer_id, candidate);
        inner.candidate_retry_epochs.remove(&peer_id);
        inner.persist()?;
        inner.session(peer_id, epoch)
    }

    pub fn discard_candidate(&self, peer_id: PeerId) -> Result<()> {
        let mut inner = self.lock()?;
        if let Some(candidate) = inner.candidates.remove(&peer_id) {
            inner
                .keys
                .retain(|_, key| key.peer_id != peer_id || key.epoch != candidate.epoch);
            inner.candidate_retry_epochs.remove(&peer_id);
            inner.persist()?;
        }
        Ok(())
    }

    /// Erases all provisional pair and peer state after admission fails.
    /// WrapAck timeouts must retain the pair instead so retry can resend the
    /// identical cached ciphertext.
    pub fn discard_pair(&self, peer_id: PeerId) -> Result<()> {
        let mut inner = self.lock()?;
        inner.keys.retain(|_, state| state.peer_id != peer_id);
        inner.pairs.remove(&peer_id);
        inner.candidates.remove(&peer_id);
        inner.retry_epochs.remove(&peer_id);
        inner.candidate_retry_epochs.remove(&peer_id);
        inner.mailbox_gates.remove(&peer_id);
        inner.peers.remove(&peer_id);
        inner.admitted_pairs.remove(&peer_id);
        inner.persist()
    }

    /// Raises the durable coordination floor learned from an authenticated peer's
    /// epoch hint. This never installs a key, wrap, or active session.
    pub fn observe_remote_epoch(&self, peer_id: PeerId, epoch: Epoch) -> Result<()> {
        let mut inner = self.lock()?;
        if !inner.peers.contains_key(&peer_id) {
            return Err(Error::KeyUnavailable);
        }
        if epoch
            > inner
                .epoch_watermarks
                .get(&peer_id)
                .copied()
                .unwrap_or(Epoch(0))
        {
            inner.epoch_watermarks.insert(peer_id, epoch);
            inner.persist()?;
        }
        Ok(())
    }

    /// A collision loser must initiate at the next epoch (last-before-collision + 2).
    pub fn retry_epoch(&self, peer_id: &PeerId) -> Result<Option<Epoch>> {
        Ok(self.lock()?.retry_epochs.get(peer_id).copied())
    }

    pub(crate) fn candidate_retry_epoch(&self, peer_id: &PeerId) -> Result<Option<Epoch>> {
        Ok(self.lock()?.candidate_retry_epochs.get(peer_id).copied())
    }

    pub fn next_epoch(&self, peer_id: &PeerId) -> Result<Epoch> {
        let inner = self.lock()?;
        let floor = inner
            .epoch_watermarks
            .get(peer_id)
            .copied()
            .unwrap_or(Epoch(0));
        Ok(Epoch(
            floor
                .0
                .checked_add(1)
                .ok_or(Error::State("epoch exhausted"))?,
        ))
    }

    pub fn session(&self, peer_id: PeerId, epoch: Epoch) -> Result<PairSession> {
        let inner = self.lock()?;
        inner.session(peer_id, epoch)
    }

    /// Returns the active slot for the pair's currently persisted epoch.
    pub fn current_session(&self, peer_id: PeerId) -> Result<PairSession> {
        let inner = self.lock()?;
        let epoch = inner
            .pairs
            .get(&peer_id)
            .map(|pair| pair.epoch)
            .ok_or(Error::KeyUnavailable)?;
        inner.session(peer_id, epoch)
    }

    /// Peeks at the next counter. `seal` remains the atomic reuse guard when
    /// concurrent callers race after receiving the same value.
    pub fn next_outbound_seq(
        &self,
        handle: &PairKeyHandle,
        payload_type: PayloadType,
    ) -> Result<Seq> {
        self.with_pair_state(handle, |state| state.next_outbound_seq(payload_type))
    }

    pub(crate) fn block_live_traffic(&self, peer_id: PeerId, owner: u64) -> Result<()> {
        self.lock()?
            .mailbox_gates
            .entry(peer_id)
            .or_default()
            .insert(owner);
        Ok(())
    }

    pub(crate) fn unblock_live_traffic(&self, peer_id: PeerId, owner: u64) -> Result<()> {
        let mut inner = self.lock()?;
        if let Some(owners) = inner.mailbox_gates.get_mut(&peer_id) {
            owners.remove(&owner);
            if owners.is_empty() {
                inner.mailbox_gates.remove(&peer_id);
            }
        }
        Ok(())
    }

    pub(crate) fn owns_live_gate(&self, peer: PeerId, owner: u64) -> Result<bool> {
        Ok(self
            .lock()?
            .mailbox_gates
            .get(&peer)
            .is_some_and(|owners| owners.contains(&owner)))
    }

    pub fn require_live_traffic(&self, peer_id: PeerId) -> Result<()> {
        if self
            .lock()?
            .mailbox_gates
            .get(&peer_id)
            .is_some_and(|owners| !owners.is_empty())
        {
            return Err(Error::State("live traffic blocked until mailbox drain"));
        }
        Ok(())
    }

    /// Call after an old epoch's in-flight work drains. All clones of this slot
    /// become unusable, and its key bytes are zeroized when removed.
    pub fn retire(&self, handle: &PairKeyHandle) -> Result<()> {
        let mut inner = self.lock()?;
        if handle.store_id != inner.instance_id || inner.keys.remove(&handle.slot).is_none() {
            return Err(Error::KeyUnavailable);
        }
        inner.persist()
    }

    pub(crate) fn with_pair_state<T>(
        &self,
        handle: &PairKeyHandle,
        operation: impl FnOnce(&mut PairKeyState) -> Result<T>,
    ) -> Result<T> {
        let mut inner = self.lock()?;
        if handle.store_id != inner.instance_id {
            return Err(Error::KeyUnavailable);
        }
        let key = inner
            .keys
            .get_mut(&handle.slot)
            .ok_or(Error::KeyUnavailable)?;
        operation(key)
    }
}

impl StoreInner {
    pub(crate) fn session(&self, peer_id: PeerId, epoch: Epoch) -> Result<PairSession> {
        let slot = self
            .keys
            .iter()
            .find_map(|(slot, state)| {
                (state.peer_id == peer_id && state.epoch == epoch).then_some(*slot)
            })
            .ok_or(Error::KeyUnavailable)?;
        Ok(PairSession::new(
            peer_id,
            epoch,
            PairKeyHandle {
                slot,
                store_id: self.instance_id,
            },
        ))
    }

    pub(crate) fn install(
        &mut self,
        peer_id: PeerId,
        initiator: PeerId,
        message: WrapMessage,
        key: Zeroizing<[u8; 32]>,
    ) -> Result<PairSession> {
        let epoch = message.epoch;
        let slot = self.next_slot;
        self.next_slot = slot
            .checked_add(1)
            .ok_or(Error::State("keystore slots exhausted"))?;
        // Competing wraps for one epoch cannot leave two active keys/counter domains.
        self.keys
            .retain(|_, state| state.peer_id != peer_id || state.epoch != epoch);
        self.keys
            .insert(slot, PairKeyState::new(key, peer_id, epoch));
        let watermark = self.epoch_watermarks.entry(peer_id).or_insert(Epoch(0));
        *watermark = (*watermark).max(epoch);
        self.pairs.insert(
            peer_id,
            PersistedPair {
                peer_id,
                epoch,
                initiator,
                wrap: message,
            },
        );
        self.persist()?;
        self.session(peer_id, epoch)
    }

    pub(crate) fn install_candidate(
        &mut self,
        peer_id: PeerId,
        initiator: PeerId,
        message: WrapMessage,
        key: Zeroizing<[u8; 32]>,
    ) -> Result<PairSession> {
        if self.candidates.contains_key(&peer_id) {
            return Err(Error::State("peer admission candidate already exists"));
        }
        if self.candidates.len() >= MAX_ADMISSION_CANDIDATES {
            return Err(Error::State("too many retained admission candidates"));
        }
        let epoch = message.epoch;
        let slot = self.next_slot;
        self.next_slot = slot
            .checked_add(1)
            .ok_or(Error::State("keystore slots exhausted"))?;
        self.keys
            .insert(slot, PairKeyState::new(key, peer_id, epoch));
        let watermark = self.epoch_watermarks.entry(peer_id).or_insert(Epoch(0));
        *watermark = (*watermark).max(epoch);
        self.candidates.insert(
            peer_id,
            PersistedPair {
                peer_id,
                epoch,
                initiator,
                wrap: message,
            },
        );
        self.persist()?;
        self.session(peer_id, epoch)
    }

    pub(crate) fn persist(&mut self) -> Result<()> {
        let state = PersistedState {
            local_id: self.local.document.peer_id,
            next_slot: self.next_slot,
            peers: self.peers.values().cloned().collect(),
            pairs: self.pairs.values().cloned().collect(),
            candidates: self.candidates.values().cloned().collect(),
            keys: self
                .keys
                .iter()
                .map(|(slot, state)| PersistedKey {
                    slot: *slot,
                    peer_id: state.peer_id,
                    epoch: state.epoch,
                    key: Zeroizing::new(*state.key),
                })
                .collect(),
            epoch_watermarks: self
                .epoch_watermarks
                .iter()
                .map(|(peer_id, epoch)| (*peer_id, *epoch))
                .collect(),
        };
        let result = encoding::encode_store_state(&state)
            .and_then(|bytes| atomic_private_write(&self.state_path, &bytes));
        if result.is_err() {
            self.failed = true;
        }
        result
    }
}

pub trait IdentityKeyStore {
    fn load_or_create_identity(&self, path: &Path) -> Result<IdentityDocument>;
    fn load_verified_peer(&self, peer_id: &PeerId) -> Result<IdentityDocument>;
}

impl IdentityManager for KeyStore {
    fn load_or_create(&self) -> Result<IdentityDocument> {
        self.identity()
    }
}

impl IdentityKeyStore for KeyStore {
    fn load_or_create_identity(&self, path: &Path) -> Result<IdentityDocument> {
        let inner = self.lock()?;
        if inner.identity_path != path {
            return Err(Error::InvalidInput("identity path differs from open store"));
        }
        Ok(inner.local.document.clone())
    }

    fn load_verified_peer(&self, peer_id: &PeerId) -> Result<IdentityDocument> {
        self.lock()?
            .peers
            .get(peer_id)
            .cloned()
            .ok_or(Error::KeyUnavailable)
    }
}

pub(crate) fn random_bytes<const N: usize>() -> Result<[u8; N]> {
    let mut value = Zeroizing::new([0; N]);
    rand::rngs::SysRng
        .try_fill_bytes(value.as_mut())
        .map_err(|_| Error::State("OS entropy unavailable"))?;
    Ok(*value)
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

pub(crate) fn read_private(path: &Path) -> Result<Option<Zeroizing<Vec<u8>>>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file() {
        return Err(Error::InvalidInput("keystore path must be a regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.len() != 0 && metadata.permissions().mode() & 0o077 != 0 {
            return Err(Error::InvalidInput(
                "keystore file permissions must be owner-only (0600)",
            ));
        }
    }
    if metadata.len() > MAX_STORE_BYTES {
        return Err(Error::InvalidInput("keystore file is too large"));
    }
    let mut bytes = Zeroizing::new(Vec::new());
    File::open(path)?
        .take(MAX_STORE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_STORE_BYTES {
        return Err(Error::InvalidInput("keystore file is too large"));
    }
    Ok(Some(bytes))
}

pub(crate) fn atomic_private_write(path: &Path, bytes: &[u8]) -> Result<()> {
    atomic_private_write_bounded(path, bytes, MAX_STORE_BYTES)
}

pub(crate) fn atomic_private_write_bounded(path: &Path, bytes: &[u8], maximum: u64) -> Result<()> {
    if bytes.len() as u64 > maximum {
        return Err(Error::InvalidInput("keystore file is too large"));
    }
    let temporary = sibling(
        path,
        &format!(
            ".tmp-{}-{}",
            std::process::id(),
            u64::from_be_bytes(random_bytes()?)
        ),
    );
    let result = (|| -> Result<()> {
        let mut file = private_options()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::KeyStore;
    use crate::{
        crypto::wrap::{ConstructionBWrap, RustCryptoConstructionBWrap},
        ids::{Epoch, PeerId},
        Error, Result,
    };

    #[test]
    fn mailbox_gate_is_shared_by_clones_and_owner_scoped() -> Result<()> {
        let directory = std::env::temp_dir().join(format!(
            "qfs-keystore-gate-{}-{}",
            std::process::id(),
            u64::from_be_bytes(super::random_bytes()?)
        ));
        std::fs::create_dir(&directory)?;
        let result = (|| -> Result<()> {
            let store = KeyStore::open(&directory.join("identity"))?;
            let clone = store.clone();
            let peer = PeerId([7; 32]);
            store.block_live_traffic(peer, 10)?;
            clone.block_live_traffic(peer, 20)?;
            assert!(matches!(
                clone.require_live_traffic(peer),
                Err(Error::State(_))
            ));
            store.unblock_live_traffic(peer, 10)?;
            assert!(matches!(
                store.require_live_traffic(peer),
                Err(Error::State(_))
            ));
            clone.unblock_live_traffic(peer, 20)?;
            store.require_live_traffic(peer)
        })();
        let _ = std::fs::remove_dir_all(directory);
        result
    }

    #[test]
    fn discarded_pair_retains_only_monotonic_epoch_watermark() -> Result<()> {
        let directory = std::env::temp_dir().join(format!(
            "qfs-keystore-discard-{}-{}",
            std::process::id(),
            u64::from_be_bytes(super::random_bytes()?)
        ));
        std::fs::create_dir(&directory)?;
        let low_path = directory.join("low");
        let result = (|| -> Result<()> {
            let first = KeyStore::open(&low_path)?;
            let second = KeyStore::open(&directory.join("high"))?;
            let first_identity = first.identity()?;
            let second_identity = second.identity()?;
            let first_id = first_identity.peer_id;
            let (low, low_identity, high, high_identity) =
                if first_identity.peer_id < second_identity.peer_id {
                    (first, first_identity, second, second_identity)
                } else {
                    (second, second_identity, first, first_identity)
                };
            let low_path = if low.identity()?.peer_id == first_id {
                low_path.clone()
            } else {
                directory.join("high")
            };
            low.import_peer(high_identity.clone())?;
            high.import_peer(low_identity.clone())?;
            let (low_session, wrap) = RustCryptoConstructionBWrap::new(low.clone()).create(
                high_identity.peer_id,
                &high_identity.ek,
                Epoch(1),
            )?;
            RustCryptoConstructionBWrap::new(high.clone()).unwrap(low_identity.peer_id, &wrap)?;
            assert!(low.cached_pair(high_identity.peer_id)?.map(|pair| pair.1) == Some(wrap));

            low.discard_pair(high_identity.peer_id)?;
            high.discard_pair(low_identity.peer_id)?;
            assert!(matches!(
                low.session(high_identity.peer_id, low_session.epoch),
                Err(Error::KeyUnavailable)
            ));
            low.import_peer(high_identity.clone())?;
            high.import_peer(low_identity.clone())?;
            low.observe_remote_epoch(high_identity.peer_id, Epoch(7))?;
            high.observe_remote_epoch(low_identity.peer_id, Epoch(7))?;
            let (_, wrap) = RustCryptoConstructionBWrap::new(low.clone()).create(
                high_identity.peer_id,
                &high_identity.ek,
                Epoch(8),
            )?;
            RustCryptoConstructionBWrap::new(high.clone()).unwrap(low_identity.peer_id, &wrap)?;
            low.discard_pair(high_identity.peer_id)?;
            drop(low);

            let reopened = KeyStore::open(&low_path)?;
            assert_eq!(reopened.peer_epoch(high_identity.peer_id)?, Epoch(8));
            assert!(matches!(
                reopened.current_session(high_identity.peer_id),
                Err(Error::KeyUnavailable)
            ));
            reopened.import_peer(high_identity.clone())?;
            assert_eq!(reopened.next_epoch(&high_identity.peer_id)?, Epoch(9));
            Ok(())
        })();
        let _ = std::fs::remove_dir_all(directory);
        result
    }

    #[test]
    fn admission_candidate_does_not_replace_confirmed_pair() -> Result<()> {
        let directory = std::env::temp_dir().join(format!(
            "qfs-keystore-candidate-{}-{}",
            std::process::id(),
            u64::from_be_bytes(super::random_bytes()?)
        ));
        std::fs::create_dir(&directory)?;
        let result = (|| -> Result<()> {
            let first_path = directory.join("first");
            let second_path = directory.join("second");
            let first = KeyStore::open(&first_path)?;
            let second = KeyStore::open(&second_path)?;
            let first_identity = first.identity()?;
            let second_identity = second.identity()?;
            let first_is_low = first_identity.peer_id < second_identity.peer_id;
            let (low, low_identity, high, high_identity) = if first_is_low {
                (first, first_identity, second, second_identity)
            } else {
                (second, second_identity, first, first_identity)
            };
            let low_path = if first_is_low {
                first_path
            } else {
                second_path
            };
            low.import_peer(high_identity.clone())?;
            high.import_peer(low_identity.clone())?;
            let low_wrap = RustCryptoConstructionBWrap::new(low.clone());
            let high_wrap = RustCryptoConstructionBWrap::new(high.clone());
            let (_, confirmed_wrap) =
                low_wrap.create(high_identity.peer_id, &high_identity.ek, Epoch(1))?;
            high_wrap.unwrap(low_identity.peer_id, &confirmed_wrap)?;

            let (_, candidate_wrap) =
                low_wrap.create_candidate(high_identity.peer_id, &high_identity.ek, Epoch(2))?;
            high_wrap.unwrap_candidate(low_identity.peer_id, &candidate_wrap)?;
            assert!(low_wrap.retry(&candidate_wrap)? == candidate_wrap);
            assert_eq!(low.current_session(high_identity.peer_id)?.epoch, Epoch(1));
            assert_eq!(high.current_session(low_identity.peer_id)?.epoch, Epoch(1));
            assert_eq!(
                low.candidate_session(high_identity.peer_id)?.epoch,
                Epoch(2)
            );
            assert_eq!(
                high.candidate_session(low_identity.peer_id)?.epoch,
                Epoch(2)
            );
            assert!(high_wrap
                .unwrap_candidate(low_identity.peer_id, &confirmed_wrap)
                .is_err());
            assert_eq!(
                high.candidate_session(low_identity.peer_id)?.epoch,
                Epoch(2)
            );

            low.discard_candidate(high_identity.peer_id)?;
            let (_, newer_wrap) =
                low_wrap.create_candidate(high_identity.peer_id, &high_identity.ek, Epoch(3))?;
            high_wrap.unwrap_candidate(low_identity.peer_id, &newer_wrap)?;
            assert_eq!(high.current_session(low_identity.peer_id)?.epoch, Epoch(1));
            assert_eq!(
                high.candidate_session(low_identity.peer_id)?.epoch,
                Epoch(3)
            );

            low.discard_candidate(high_identity.peer_id)?;
            high.discard_candidate(low_identity.peer_id)?;
            let (_, high_candidate) =
                high_wrap.create_candidate(low_identity.peer_id, &low_identity.ek, Epoch(4))?;
            low_wrap.unwrap_candidate(high_identity.peer_id, &high_candidate)?;
            assert_eq!(low.current_session(high_identity.peer_id)?.epoch, Epoch(1));
            assert_eq!(high.current_session(low_identity.peer_id)?.epoch, Epoch(1));
            low.discard_candidate(high_identity.peer_id)?;
            high.discard_candidate(low_identity.peer_id)?;
            assert_eq!(low.current_session(high_identity.peer_id)?.epoch, Epoch(1));
            assert_eq!(high.current_session(low_identity.peer_id)?.epoch, Epoch(1));
            low_wrap.create_candidate(high_identity.peer_id, &high_identity.ek, Epoch(5))?;
            low_wrap.create(high_identity.peer_id, &high_identity.ek, Epoch(6))?;
            assert!(low
                .promote_candidate(high_identity.peer_id, Epoch(5))
                .is_err());
            assert_eq!(low.current_session(high_identity.peer_id)?.epoch, Epoch(6));
            assert_eq!(
                low.candidate_session(high_identity.peer_id)?.epoch,
                Epoch(5)
            );
            drop(low_wrap);
            drop(high_wrap);
            drop(high);
            drop(low);
            let reopened = KeyStore::open(&low_path)?;
            assert_eq!(
                reopened.current_session(high_identity.peer_id)?.epoch,
                Epoch(7)
            );
            assert!(reopened.cached_candidate(high_identity.peer_id)?.is_none());
            Ok(())
        })();
        let _ = std::fs::remove_dir_all(directory);
        result
    }
}
