use crate::{config::Config, ids::PeerId, keystore, Result};

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
    let identity = keystore::load_or_create_identity_path(&config.identity_path())?;
    eprintln!(
        "qfsd: crypto pending; identity {:?}; configured listen address {} (inactive); host {}",
        identity.state,
        config.listen_addr,
        if host_id.is_some() {
            "configured; role pending identity verification"
        } else {
            "unconfigured"
        }
    );
    // No crypto operation is invoked until a real identity and fresh pair epochs
    // can be established. Stored K_ab/counters must never resume across restart.
    #[cfg(unix)]
    tokio::select! {
        _ = interrupt.recv() => {},
        _ = terminate.recv() => {},
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await?;
    eprintln!("qfsd: shutdown complete");
    Ok(())
}
