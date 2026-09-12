use super::{join::VaultHost, VaultId};
use crate::{
    ids::{Epoch, PeerId},
    keystore::KeyStore,
    Error, Result,
};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

#[derive(Clone)]
pub struct VaultSet {
    inner: Rc<RefCell<VaultSetState>>,
}

struct VaultSetState {
    keys: KeyStore,
    vaults: BTreeMap<VaultId, Rc<RefCell<VaultHost>>>,
    bindings: BTreeMap<PeerId, PairBinding>,
}

#[derive(Clone, Copy)]
struct PairBinding {
    vault_id: VaultId,
    confirmed_epoch: Option<Epoch>,
}

impl VaultSet {
    pub fn new(keys: KeyStore) -> Result<Self> {
        keys.peer_id()?;
        Ok(Self {
            inner: Rc::new(RefCell::new(VaultSetState {
                keys,
                vaults: BTreeMap::new(),
                bindings: BTreeMap::new(),
            })),
        })
    }

    pub fn insert(&self, vault: Rc<RefCell<VaultHost>>) -> Result<()> {
        let vault_id = vault.borrow().vault_id();
        let peer_id = vault.borrow().keys.peer_id()?;
        let mut state = self.inner.borrow_mut();
        if peer_id != state.keys.peer_id()? {
            return Err(Error::InvalidInput(
                "vault host identity differs from vault set",
            ));
        }
        if state.vaults.contains_key(&vault_id) {
            return Err(Error::InvalidInput("vault is already hosted"));
        }
        let mut bindings = state.bindings.clone();
        for &member in vault.borrow().host.members() {
            if member == peer_id || state.keys.current_session(member).is_err() {
                continue;
            }
            if let Some(bound) = bindings.get(&member) {
                if bound.vault_id != vault_id {
                    return Err(Error::VaultSessionConflict {
                        peer_id: member,
                        bound: bound.vault_id,
                        requested: vault_id,
                    });
                }
            }
            bindings.insert(
                member,
                PairBinding {
                    vault_id,
                    confirmed_epoch: None,
                },
            );
        }
        state.bindings = bindings;
        state.vaults.insert(vault_id, vault);
        Ok(())
    }

    pub fn vaults(&self) -> Vec<Rc<RefCell<VaultHost>>> {
        self.inner.borrow().vaults.values().cloned().collect()
    }

    pub fn get(&self, vault_id: VaultId) -> Option<Rc<RefCell<VaultHost>>> {
        self.inner.borrow().vaults.get(&vault_id).cloned()
    }

    pub fn bound_vault(&self, peer_id: PeerId) -> Option<VaultId> {
        self.inner
            .borrow()
            .bindings
            .get(&peer_id)
            .map(|binding| binding.vault_id)
    }

    pub(crate) fn keys(&self) -> KeyStore {
        self.inner.borrow().keys.clone()
    }

    pub(crate) fn has_confirmed_pair(&self, peer_id: PeerId) -> bool {
        let state = self.inner.borrow();
        let Some(binding) = state.bindings.get(&peer_id) else {
            return false;
        };
        binding.confirmed_epoch.is_some_and(|confirmed| {
            state
                .keys
                .current_session(peer_id)
                .is_ok_and(|session| session.epoch == confirmed)
        })
    }

    pub fn check_binding(&self, peer_id: PeerId, requested: VaultId) -> Result<()> {
        if let Some(bound) = self.bound_vault(peer_id) {
            if bound != requested {
                return Err(Error::VaultSessionConflict {
                    peer_id,
                    bound,
                    requested,
                });
            }
        }
        Ok(())
    }

    pub fn refresh_mailboxes(&self) -> Result<()> {
        for vault in self.vaults() {
            vault.borrow_mut().host.refresh_mailboxes()?;
        }
        Ok(())
    }

    pub fn stop(&self) {
        for vault in self.vaults() {
            vault.borrow_mut().host.stop();
        }
    }

    pub fn take_disconnects(&self) -> Vec<PeerId> {
        let vaults = self.vaults();
        let mut disconnected = Vec::new();
        for vault in vaults {
            let vault_id = vault.borrow().vault_id();
            for peer_id in vault.borrow_mut().take_disconnects() {
                self.unbind(peer_id, vault_id);
                disconnected.push(peer_id);
            }
        }
        disconnected
    }

    pub(crate) fn bind(&self, peer_id: PeerId, vault_id: VaultId) -> Result<()> {
        self.check_binding(peer_id, vault_id)?;
        let epoch = self.inner.borrow().keys.current_session(peer_id)?.epoch;
        self.inner.borrow_mut().bindings.insert(
            peer_id,
            PairBinding {
                vault_id,
                confirmed_epoch: Some(epoch),
            },
        );
        Ok(())
    }

    pub(crate) fn unbind(&self, peer_id: PeerId, vault_id: VaultId) {
        let mut state = self.inner.borrow_mut();
        if state
            .bindings
            .get(&peer_id)
            .is_some_and(|binding| binding.vault_id == vault_id)
        {
            state.bindings.remove(&peer_id);
        }
    }
}

impl From<Rc<RefCell<VaultHost>>> for VaultSet {
    fn from(vault: Rc<RefCell<VaultHost>>) -> Self {
        let keys = vault.borrow().keys.clone();
        let vault_id = vault.borrow().vault_id();
        let bindings = vault
            .borrow()
            .host
            .members()
            .iter()
            .filter(|&&peer_id| keys.current_session(peer_id).is_ok())
            .map(|&peer_id| {
                (
                    peer_id,
                    PairBinding {
                        vault_id,
                        confirmed_epoch: None,
                    },
                )
            })
            .collect();
        Self {
            inner: Rc::new(RefCell::new(VaultSetState {
                keys,
                vaults: BTreeMap::from([(vault_id, vault)]),
                bindings,
            })),
        }
    }
}
