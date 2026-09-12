use std::{net::SocketAddr, path::PathBuf};

use clap::Parser;

use crate::{ids::PeerId, Error, Result};

/// CLI configuration; relative identity paths are resolved inside data_dir.
#[derive(Parser, Debug)]
#[command(
    name = "qfsd",
    version,
    about = "Local-first file-system daemon (crypto pending)"
)]
pub struct Config {
    #[arg(long, default_value = ".qfs")]
    pub data_dir: PathBuf,
    /// Reserved address; the scaffold does not open a network listener.
    #[arg(long, default_value = "127.0.0.1:7447")]
    pub listen_addr: SocketAddr,
    /// Local member identity path (created as an empty placeholder).
    #[arg(long, default_value = "identity")]
    pub peer_identity_path: PathBuf,
    /// Appointed H's raw 32-byte peer-id file; no text/hex ID encoding.
    #[arg(long, value_name = "RAW_32_BYTE_FILE")]
    pub host_id: Option<PathBuf>,
}

impl Config {
    pub fn identity_path(&self) -> PathBuf {
        if self.peer_identity_path.is_absolute() {
            self.peer_identity_path.clone()
        } else {
            self.data_dir.join(&self.peer_identity_path)
        }
    }

    pub fn load_host_id(&self) -> Result<Option<PeerId>> {
        let Some(path) = &self.host_id else {
            return Ok(None);
        };
        let bytes = std::fs::read(path)?;
        let bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|_| Error::InvalidInput("host-id file must contain exactly 32 raw bytes"))?;
        Ok(Some(PeerId(bytes)))
    }
}
