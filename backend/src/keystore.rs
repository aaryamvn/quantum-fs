//! Only identity-path lifecycle exists. No real key material is generated or persisted.

use std::{
    fs::{self, File, OpenOptions},
    io::ErrorKind,
    path::{Path, PathBuf},
};

use crate::{crypto::identity::IdentityDocument, ids::PeerId, Error, Result};

pub struct IdentityPath {
    pub path: PathBuf,
    pub state: IdentityState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityState {
    Pending,
    /// Existing bytes are preserved; they are not parsed, trusted, or used yet.
    ExistingUnverified,
}

/// Create an empty placeholder atomically, or load its existing state without truncation.
pub fn load_or_create_identity_path(path: &Path) -> Result<IdentityPath> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::AlreadyExists => File::open(path)?,
        Err(error) => return Err(error.into()),
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(Error::InvalidInput("identity path must be a regular file"));
    }
    let state = if metadata.len() == 0 {
        IdentityState::Pending
    } else {
        IdentityState::ExistingUnverified
    };
    Ok(IdentityPath {
        path: path.to_owned(),
        state,
    })
}

/// A future implementation must verify the signature and self-certifying peer_id
/// before returning any identity as authenticated. Secrets must zeroize on drop.
pub trait IdentityKeyStore {
    fn load_or_create_identity(&self, path: &Path) -> Result<IdentityDocument>;
    fn load_verified_peer(&self, peer_id: &PeerId) -> Result<IdentityDocument>;
}

pub struct PendingKeyStore;

impl IdentityKeyStore for PendingKeyStore {
    fn load_or_create_identity(&self, _path: &Path) -> Result<IdentityDocument> {
        Err(Error::NotImplemented(
            "identity generation, verification, and key persistence",
        ))
    }

    fn load_verified_peer(&self, _peer_id: &PeerId) -> Result<IdentityDocument> {
        Err(Error::NotImplemented("verified peer identity persistence"))
    }
}
