use super::*;
use crate::demo_log::{self, Kind};

impl HostService {
    /// Admission is part of the same staged snapshot as membership. On legacy
    /// opens the admission file seeds it; a newer replica is authoritative.
    pub fn attach_admission(&mut self, path: &Path, metadata: VaultMetadata) -> Result<()> {
        if FileId(metadata.vault_id.0) != self.state.expected_root {
            return Err(Error::InvalidInput("vault admission root mismatch"));
        }
        let old_path = self.admission_path.replace(path.to_owned());
        let result = (|| {
            let mut staged = self.stage_state()?;
            if staged
                .admission
                .as_ref()
                .is_some_and(|stored| FileId(stored.vault_id.0) != staged.expected_root)
            {
                return Err(Error::InvalidInput("stored admission root mismatch"));
            }
            if staged.admission.is_none() {
                staged.denied.extend(metadata.denied.iter().copied());
                staged.admission = Some(metadata);
            }
            self.publish_state(staged)
        })();
        if result.is_err() {
            self.admission_path = old_path;
        }
        result
    }

    pub fn admission(&self) -> Option<&VaultMetadata> {
        self.state.admission.as_ref()
    }
    pub fn denied(&self) -> &BTreeSet<PeerId> {
        &self.state.denied
    }

    pub fn set_admission(&mut self, metadata: VaultMetadata) -> Result<()> {
        self.require_running()?;
        if FileId(metadata.vault_id.0) != self.state.expected_root
            || metadata.members.iter().copied().collect::<BTreeSet<_>>() != self.state.members
            || metadata.denied != self.state.denied
        {
            return Err(Error::InvalidInput(
                "admission membership differs from replica",
            ));
        }
        let mut staged = self.stage_state()?;
        staged.admission = Some(metadata);
        self.publish_state(staged)
    }

    /// H alone invokes this API. fan_out_control checks the authenticated
    /// sender before routing kind 7 here. File history deliberately survives.
    pub fn kick(&mut self, target: PeerId) -> Result<PeerId> {
        self.kick_with_code(target, None)
    }

    /// Same removal with a caller-chosen replacement join code, so a host can
    /// install the code a short code derives into. `None` generates one.
    pub fn kick_with_code(&mut self, target: PeerId, code: Option<JoinCode>) -> Result<PeerId> {
        self.require_running()?;
        self.require_member(target)?;
        if target == self.state.host_id {
            return Err(Error::InvalidInput("cannot kick H"));
        }
        let id_before = self.state.next_control;
        self.commit_verified_with_code(ControlUpdate::Kick(target), None, code)?;
        self.note_recent_op(self.state.host_id, id_before);
        self.presence.remove(&target);
        self.challenges.remove(&target);
        self.disconnects.push(target);
        self.keys.discard_pair(target)?;
        demo_log::event(
            Kind::Membership,
            "ML-DSA-65 + AES-256-GCM",
            format!("member revoked  {}", demo_log::peer(target)),
            &[
                "membership snapshot committed".to_owned(),
                "pair key discarded · join code rotated".to_owned(),
            ],
        );
        Ok(target)
    }

    pub fn take_disconnects(&mut self) -> Vec<PeerId> {
        std::mem::take(&mut self.disconnects)
    }

    pub fn historical_documents(&self) -> Vec<IdentityDocument> {
        self.state.identity_documents.values().cloned().collect()
    }
}

impl MemberReplica {
    /// H's authenticated welcome carries public history separately from its
    /// current membership list. Do not restore a kicked peer's key slots.
    pub fn learn_history(&mut self, documents: &[IdentityDocument]) -> Result<()> {
        if documents
            .iter()
            .all(|document| self.identity_documents.get(&document.peer_id) == Some(document))
        {
            return Ok(());
        }
        let mut staged = self.stage()?;
        for document in documents {
            document.verify()?;
            staged.historical_members.insert(document.peer_id);
            staged
                .identity_documents
                .insert(document.peer_id, document.clone());
        }
        self.apply_staged(staged)
    }

    /// Apply older queued controls first. This also handles a freshly joined
    /// replica when a Kick has already been folded into H's truncated snapshot.
    pub fn reconcile_members(
        &mut self,
        members: &BTreeSet<PeerId>,
        denied: &BTreeSet<PeerId>,
    ) -> Result<()> {
        if !members.contains(&self.host_id)
            || !members.contains(&self.keys.peer_id()?)
            || !members.is_disjoint(denied)
            || denied.contains(&self.host_id)
        {
            return Err(Error::AuthenticationFailed);
        }
        if &self.members == members && &self.denied == denied {
            return Ok(());
        }
        let mut staged = self.stage()?;
        if !staged.denied.is_subset(denied) {
            return Err(Error::AuthenticationFailed);
        }
        staged.members = members.clone();
        staged.historical_members.extend(members.iter().copied());
        staged.denied = denied.clone();
        self.apply_staged(staged)
    }
}

/// Public documents survive pair discard in metadata, without restoring a
/// revoked pair or requiring the writer to remain in the current live set.
fn archive_documents(
    keys: &KeyStore,
    members: &BTreeSet<PeerId>,
    archive: &mut BTreeMap<PeerId, IdentityDocument>,
) -> Result<()> {
    for &peer in members {
        // Entries were verified at insertion or durable load; their signing
        // principal remains fixed across EK rotation. Avoid repeated DSA work.
        if archive.contains_key(&peer) {
            continue;
        }
        let document = if peer == keys.peer_id()? {
            Some(keys.identity()?)
        } else {
            match keys.load_verified_peer(&peer) {
                Ok(document) => Some(document),
                Err(Error::KeyUnavailable) => None,
                Err(error) => return Err(error),
            }
        };
        if let Some(document) = document {
            document.verify()?;
            archive.insert(peer, document);
        }
    }
    Ok(())
}

pub(super) fn archive_host(keys: &KeyStore, staged: &mut HostState) -> Result<()> {
    staged
        .historical_members
        .extend(staged.members.iter().copied());
    archive_documents(
        keys,
        &staged.historical_members,
        &mut staged.identity_documents,
    )?;
    require_writer_documents(&staged.manifests, &staged.log, &staged.identity_documents)
}

pub(super) fn archive_replica(keys: &KeyStore, staged: &mut StagedReplica) -> Result<()> {
    staged
        .historical_members
        .extend(staged.members.iter().copied());
    archive_documents(
        keys,
        &staged.historical_members,
        &mut staged.identity_documents,
    )?;
    require_writer_documents(&staged.manifests, &staged.log, &staged.identity_documents)
}

fn require_writer_documents(
    manifests: &BTreeMap<FileId, TrustedManifest>,
    log: &[ControlRecord],
    documents: &BTreeMap<PeerId, IdentityDocument>,
) -> Result<()> {
    let writers = manifests
        .values()
        .map(|manifest| manifest.manifest().writer_id)
        .chain(log.iter().filter_map(|record| match &record.update {
            ControlUpdate::NewManifest(manifest) => Some(manifest.writer_id),
            _ => None,
        }));
    for writer in writers {
        if !documents.contains_key(&writer) {
            return Err(Error::KeyUnavailable);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        crypto::wrap::{ConstructionBWrap, RustCryptoConstructionBWrap},
        net::VaultId,
    };

    #[test]
    fn failed_kick_keeps_code_members_and_packets_unpublished() -> Result<()> {
        let dir = std::env::temp_dir().join(format!(
            "qfs-kick-failure-{}",
            u64::from_be_bytes(random_bytes()?)
        ));
        std::fs::create_dir(&dir)?;
        let keys = KeyStore::open(&dir.join("identity"))?;
        let member = KeyStore::open(&dir.join("member"))?;
        let h = keys.peer_id()?;
        let b = member.peer_id()?;
        keys.import_peer(member.identity()?)?;
        member.import_peer(keys.identity()?)?;
        let (lo, hi) = if h < b {
            (&keys, &member)
        } else {
            (&member, &keys)
        };
        let (_, wrap) = RustCryptoConstructionBWrap::new(lo.clone()).create(
            hi.peer_id()?,
            &hi.identity()?.ek,
            Epoch(1),
        )?;
        RustCryptoConstructionBWrap::new(hi.clone()).unwrap(lo.peer_id()?, &wrap)?;
        let root = FileId([41; 32]);
        let mut host = HostService::open_durable(keys.clone(), &dir, root, BTreeSet::from([h, b]))?;
        let code = JoinCode::generate()?;
        host.attach_admission(
            &dir.join("vault"),
            VaultMetadata {
                vault_id: VaultId(root.0),
                join_code: code,
                issued_at: 0,
                members: vec![h, b],
                denied: BTreeSet::new(),
            },
        )?;
        let old_replica = std::fs::read(dir.join("replica.bin"))?;
        let old_admission = std::fs::read(dir.join("vault"))?;
        host.chunks()
            .lock()
            .map_err(|_| Error::State("lock"))?
            .fail_next_persist();
        assert!(host.kick(b).is_err());
        assert!(host.has_member(&b));
        assert_eq!(
            host.admission().ok_or(Error::State("admission"))?.join_code,
            code
        );
        assert!(host.take_disconnects().is_empty());
        assert!(host.instruction_log().is_empty());
        assert!(host.take_online_control(b)?.is_empty());
        assert!(keys.current_session(b).is_ok());
        assert_eq!(std::fs::read(dir.join("vault"))?, old_admission);
        // take_online_control can produce a newer equal-state snapshot, so check
        // the failed mutation left the earlier snapshot semantically identical.
        let (_, before) = encoding::decode_replica(&old_replica, root)?;
        let (_, after) = encoding::decode_replica(&std::fs::read(dir.join("replica.bin"))?, root)?;
        assert!(before == after);
        drop(host);
        drop(keys);
        drop(member);
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }
}
