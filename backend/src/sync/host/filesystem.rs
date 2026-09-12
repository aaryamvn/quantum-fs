use super::*;
use crate::crypto::sign::MANIFEST_CONTEXT;
use crate::demo_log::{self, Kind};

fn delivery_details(
    host: &HostService,
    since_control: u64,
    file_id: Option<FileId>,
) -> Vec<String> {
    let online = host
        .state
        .online
        .iter()
        .filter(|(_, pending)| pending.iter().any(|entry| entry.record.id >= since_control))
        .map(|(&peer, _)| demo_log::peer(peer))
        .collect::<Vec<_>>();
    let queued = host
        .state
        .mailboxes
        .iter()
        .filter_map(|(&peer, pending)| {
            let count = pending
                .iter()
                .filter(|entry| match &entry.content {
                    QueueContent::Control(record) => record.id >= since_control,
                    QueueContent::Chunk {
                        file_id: queued, ..
                    } => Some(*queued) == file_id,
                })
                .count();
            (count != 0).then(|| format!("{} — {count} encrypted item(s)", demo_log::peer(peer)))
        })
        .collect::<Vec<_>>();
    let mut details = Vec::new();
    if !online.is_empty() {
        details.push(format!("live notify → {}", online.join(", ")));
    }
    if !queued.is_empty() {
        details.push(format!("offline queue → {}", queued.join(", ")));
    }
    details
}

fn filesystem_details(host: &HostService, since_control: u64, sender: PeerId) -> Vec<String> {
    let mut details = vec![format!("authorized member {}", demo_log::peer(sender))];
    details.extend(delivery_details(host, since_control, None));
    details
}

pub(super) fn validate_link_kind(
    manifests: &BTreeMap<FileId, TrustedManifest>,
    update: &ControlUpdate,
) -> Result<()> {
    if matches!(update, ControlUpdate::Link { child, is_dir: true, .. } if manifests.contains_key(child))
    {
        return Err(Error::InvalidInput(
            "file manifest cannot become a directory",
        ));
    }
    Ok(())
}

pub(super) fn apply_tree_update(
    tree: &DirectoryTree,
    update: &ControlUpdate,
) -> Result<(DirectoryTree, Option<FileId>)> {
    let mut next = tree.clone();
    let removed = match update {
        ControlUpdate::NewManifest(manifest) => {
            if next.is_dir(&manifest.file_id) {
                return Err(Error::InvalidInput("cannot save a directory as a file"));
            }
            None
        }
        ControlUpdate::Kick(_) => None,
        ControlUpdate::Add(_) => None, // Deprecated v1 no-op; never creates an inode.
        ControlUpdate::Clear(file_id) => Some(*file_id),
        ControlUpdate::Remove(file_id) => {
            next.remove_id(file_id)?;
            Some(*file_id)
        }
        ControlUpdate::Link {
            parent,
            name,
            child,
            is_dir,
        } => {
            next.link(*parent, name, *child, *is_dir)?;
            None
        }
        ControlUpdate::Unlink { parent, name } => Some(next.unlink(*parent, name)?),
        ControlUpdate::Rename {
            src_parent,
            src_name,
            dst_parent,
            dst_name,
        } => {
            next.rename(*src_parent, src_name, *dst_parent, dst_name)?;
            None
        }
    };
    Ok((next, removed))
}

impl HostService {
    pub fn tree(&self) -> &DirectoryTree {
        &self.state.tree
    }

    /// Flat v1 trust: every current member can change any path. H's arrival
    /// order determines the result; operations never bypass the commit hub.
    pub fn mkdir(&mut self, sender: PeerId, path: &str) -> Result<FileId> {
        self.require_running()?;
        self.require_member(sender)?;
        let (parent, name) = self.state.tree.resolve_parent(path)?;
        let child = FileId(random_bytes()?);
        let since_control = self.state.next_control;
        self.fan_out_control(
            sender,
            &ControlUpdate::Link {
                parent,
                name,
                child,
                is_dir: true,
            },
        )?;
        demo_log::event(
            Kind::File,
            "AES-256-GCM",
            format!("directory committed  {path}"),
            &filesystem_details(self, since_control, sender),
        );
        Ok(child)
    }

    pub fn link_file(&mut self, sender: PeerId, path: &str, child: FileId) -> Result<()> {
        self.require_running()?;
        self.require_member(sender)?;
        let (parent, name) = self.state.tree.resolve_parent(path)?;
        self.fan_out_control(
            sender,
            &ControlUpdate::Link {
                parent,
                name,
                child,
                is_dir: false,
            },
        )
    }

    pub fn unlink(&mut self, sender: PeerId, path: &str) -> Result<()> {
        self.require_running()?;
        self.require_member(sender)?;
        let (parent, name) = self.state.tree.resolve_parent(path)?;
        let since_control = self.state.next_control;
        self.fan_out_control(sender, &ControlUpdate::Unlink { parent, name })?;
        demo_log::event(
            Kind::File,
            "AES-256-GCM",
            format!("path unlinked  {path}"),
            &filesystem_details(self, since_control, sender),
        );
        Ok(())
    }

    pub fn rename(&mut self, sender: PeerId, source: &str, destination: &str) -> Result<()> {
        self.require_running()?;
        self.require_member(sender)?;
        let (src_parent, src_name) = self.state.tree.resolve_parent(source)?;
        let (dst_parent, dst_name) = self.state.tree.resolve_parent(destination)?;
        let since_control = self.state.next_control;
        self.fan_out_control(
            sender,
            &ControlUpdate::Rename {
                src_parent,
                src_name,
                dst_parent,
                dst_name,
            },
        )?;
        demo_log::event(
            Kind::File,
            "AES-256-GCM",
            format!("path renamed  {source} → {destination}"),
            &filesystem_details(self, since_control, sender),
        );
        Ok(())
    }

    /// In-process writer-to-H API. TCP sends the same signed manifest and
    /// encrypt-at-send bodies; file bytes are placed on H before commit.
    pub fn save_file(
        &mut self,
        writer: &KeyStore,
        path: &str,
        bodies: &[Vec<u8>],
    ) -> Result<FileId> {
        self.require_running()?;
        let sender = writer.peer_id()?;
        self.require_member(sender)?;
        self.state.tree.resolve_parent(path)?;
        let existing = self.state.tree.resolve(path).ok();
        if existing.is_some_and(|id| self.state.tree.is_dir(&id)) {
            return Err(Error::InvalidInput("cannot save a directory"));
        }
        let file_id = match existing {
            Some(id) => id,
            None => FileId(random_bytes()?),
        };
        let version = self.state.manifests.get(&file_id).map_or(Ok(1), |m| {
            m.manifest()
                .version
                .checked_add(1)
                .ok_or(Error::State("manifest version exhausted"))
        })?;
        let mut manifest = Manifest {
            file_id,
            chunk_ids: Vec::new(),
            size: 0,
            writer_id: sender,
            version,
            signature: Vec::new(),
        };
        for (index, plaintext) in bodies.iter().enumerate() {
            if plaintext.len() as u64 > crate::store::durable::MAX_CHUNK_BYTES {
                return Err(Error::InvalidInput("chunk exceeds 1 MiB"));
            }
            manifest.size = manifest
                .size
                .checked_add(plaintext.len() as u64)
                .ok_or(Error::InvalidInput("file size overflow"))?;
            manifest
                .chunk_ids
                .push(encoding::chunk_id(&file_id, index as u64, plaintext));
        }
        manifest.signature = RustCryptoPureMlDsa.sign(
            &writer.signing_key()?,
            MANIFEST_CONTEXT,
            &encoding::manifest_m(&manifest)?,
        )?;
        let link = if existing.is_none() {
            let (parent, name) = self.state.tree.resolve_parent(path)?;
            Some(ControlUpdate::Link {
                parent,
                name,
                child: file_id,
                is_dir: false,
            })
        } else {
            None
        };
        let chunk_count = manifest.chunk_ids.len();
        let byte_count = manifest.size;
        let since_control = self.state.next_control;
        self.commit_writer_upload(
            sender,
            manifest,
            bodies
                .iter()
                .enumerate()
                .map(|(i, body)| (i as u64, body.clone()))
                .collect(),
            link,
        )?;
        let mut details = vec![
            format!("file {}", demo_log::file(file_id)),
            format!("{chunk_count} chunk(s) · {byte_count} bytes"),
            format!("writer {} · signature verified", demo_log::peer(sender)),
        ];
        details.extend(delivery_details(self, since_control, Some(file_id)));
        demo_log::event(
            Kind::File,
            "ML-DSA-65 + AES-256-GCM",
            format!("file committed  {path}"),
            &details,
        );
        Ok(file_id)
    }

    /// Reuses the ordinary commit preparation for every instruction, but
    /// publishes all records of one save only after one durable snapshot.
    pub(crate) fn commit_writer_upload(
        &mut self,
        sender: PeerId,
        manifest: Manifest,
        bodies: BTreeMap<u64, Vec<u8>>,
        link: Option<ControlUpdate>,
    ) -> Result<()> {
        self.verify_writer_manifest(sender, manifest.clone())?;
        if let Some(update) = &link {
            if !matches!(update, ControlUpdate::Link { child, is_dir: false, .. } if *child == manifest.file_id)
            {
                return Err(Error::InvalidInput("upload link does not match file"));
            }
        }
        self.refresh_mailboxes()?;
        let gate_owners: Vec<_> = self
            .state
            .members
            .iter()
            .copied()
            .map(|peer| Ok((peer, self.keys.owns_live_gate(peer, self.state.gate_owner)?)))
            .collect::<Result<_>>()?;
        let id_before = self.state.next_control;
        let mut transaction = Self {
            keys: self.keys.clone(),
            state: self.stage_state()?,
            presence: self.presence.clone(),
            challenges: self.challenges.clone(),
            running: self.running,
            defer_persistence: true,
            admission_path: self.admission_path.clone(),
            disconnects: Vec::new(),
            recent_ops: VecDeque::new(),
            last_seen: BTreeMap::new(),
        };
        let prepared = (|| -> Result<()> {
            {
                let mut chunks = transaction
                    .state
                    .chunks
                    .lock()
                    .map_err(|_| Error::State("chunk store poisoned"))?;
                for (index, plaintext) in bodies {
                    chunks.put(&manifest.file_id, index, plaintext)?;
                }
            }
            transaction.fan_out_control(sender, &ControlUpdate::NewManifest(manifest))?;
            if let Some(link) = link {
                transaction.fan_out_control(sender, &link)?;
            }
            self.publish_state(transaction.state)
        })();
        if prepared.is_err() {
            for (peer, owned_before) in gate_owners {
                if !owned_before {
                    self.keys
                        .unblock_live_traffic(peer, self.state.gate_owner)?;
                }
            }
        } else {
            // The staged transaction published the record ids; mirror them into
            // the operator ring now that the snapshot is durable.
            for id in id_before..self.state.next_control {
                self.note_recent_op(sender, id);
            }
        }
        prepared
    }

    pub(crate) fn verify_writer_manifest(
        &self,
        sender: PeerId,
        manifest: Manifest,
    ) -> Result<TrustedManifest> {
        self.require_running()?;
        self.require_member(sender)?;
        if manifest.writer_id != sender {
            return Err(Error::AuthenticationFailed);
        }
        TrustedManifest::verify(manifest, &self.keys, &self.state.members)
    }
}

impl MemberReplica {
    pub fn tree(&self) -> &DirectoryTree {
        &self.tree
    }

    /// Called by the transport only after the packet authenticates as H in the
    /// current live epoch. Avoids opening the same AES counter a second time.
    pub(crate) fn apply_authenticated_control(&mut self, record: ControlRecord) -> Result<()> {
        self.keys.require_live_traffic(self.host_id)?;
        let mut staged = self.stage()?;
        self.stage_control(&mut staged, &record)?;
        self.apply_staged(staged)
    }
}
