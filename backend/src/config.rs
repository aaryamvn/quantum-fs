use std::{net::SocketAddr, path::PathBuf};

use clap::Parser;

use crate::{ids::PeerId, Error, Result};

/// CLI configuration; relative identity paths are resolved inside data_dir.
#[derive(Parser, Debug)]
#[command(name = "qfsd", version, about = "Local-first file-system daemon")]
pub struct Config {
    #[arg(long, default_value = ".qfs")]
    pub data_dir: PathBuf,
    /// Numeric TCP bind address; use port zero for an ephemeral listener.
    #[arg(long, default_value = "127.0.0.1:7447")]
    pub listen_addr: SocketAddr,
    /// Private local member identity and independent key seeds.
    #[arg(long, default_value = "identity")]
    pub peer_identity_path: PathBuf,
    /// Appointed H's raw 32-byte peer-id file; no text/hex ID encoding.
    #[arg(long, value_name = "RAW_32_BYTE_FILE")]
    pub host_id: Option<PathBuf>,
    /// Run the signed-ad directory without a member identity or file replica.
    #[arg(long, conflicts_with_all = ["create_vault", "join_code", "host_id"])]
    pub directory: bool,
    #[arg(long, value_name = "SOCKET_ADDR")]
    pub directory_addr: Option<SocketAddr>,
    /// Vault capability encoded as uppercase RFC 4648 Base32 without padding.
    #[arg(
        long,
        value_name = "BASE32",
        requires = "directory_addr",
        conflicts_with = "create_vault"
    )]
    pub join_code: Option<crate::net::JoinCode>,
    #[arg(long, requires = "directory_addr")]
    pub create_vault: bool,
    /// Numeric address advertised after bind (for NAT/container forwarding).
    #[arg(long)]
    pub advertise_addr: Option<SocketAddr>,
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
