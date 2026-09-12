use crate::{
    config::Config,
    crypto::{
        identity::IdentityManager,
        wrap::{RustCryptoConstructionBWrap, ROTATION_INTERVAL_SECS},
    },
    ids::PeerId,
    keystore::KeyStore,
    store::chunks::shared_chunk_store,
    sync::host::HostService,
    Result,
};
use std::time::Duration;

/// H is a role of an authenticated member, never a separate process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberRole {
    Member,
    Host,
}

pub fn member_role(local_id: &PeerId, host_id: Option<&PeerId>) -> MemberRole {
    if host_id == Some(local_id) {
        MemberRole::Host
    } else {
        MemberRole::Member
    }
}

pub async fn run(config: Config) -> Result<()> {
    // Install handlers before announcing readiness, including on Unix SIGTERM.
    #[cfg(unix)]
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    #[cfg(unix)]
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;

    let host_id = config.load_host_id()?;
    std::fs::create_dir_all(&config.data_dir)?;
    let store = KeyStore::open(&config.identity_path())?;
    let identity = store.load_or_create()?;
    let wraps = RustCryptoConstructionBWrap::new(store.clone());
    let mut host = if member_role(&identity.peer_id, host_id.as_ref()) == MemberRole::Host {
        Some(HostService::new(
            store.clone(),
            [identity.peer_id].into(),
            shared_chunk_store(),
        )?)
    } else {
        None
    };
    eprintln!(
        "qfsd: identity verified; crypto ready; role {:?}; configured listen address {} (inactive); {} pending wraps",
        member_role(&identity.peer_id, host_id.as_ref()),
        config.listen_addr,
        store.pending_wraps()?.len()
    );
    // Startup prepared fresh epochs; transport will later deliver pending wraps.
    // A running daemon rotates at least weekly without changing its static ek.
    #[cfg(unix)]
    loop {
        tokio::select! {
            _ = interrupt.recv() => break,
            _ = terminate.recv() => break,
            _ = tokio::time::sleep(Duration::from_secs(ROTATION_INTERVAL_SECS)) => {
                eprintln!("qfsd: prepared {} rotated pair wraps", wraps.rotate_all()?);
                if let Some(host) = &mut host { host.refresh_mailboxes()?; }
            }
        }
    }
    #[cfg(not(unix))]
    loop {
        tokio::select! {
            result = tokio::signal::ctrl_c() => { result?; break; },
            _ = tokio::time::sleep(Duration::from_secs(ROTATION_INTERVAL_SECS)) => {
                eprintln!("qfsd: prepared {} rotated pair wraps", wraps.rotate_all()?);
                if let Some(host) = &mut host { host.refresh_mailboxes()?; }
            }
        }
    }
    if let Some(host) = &mut host {
        host.stop();
    }
    eprintln!("qfsd: shutdown complete");
    Ok(())
}
