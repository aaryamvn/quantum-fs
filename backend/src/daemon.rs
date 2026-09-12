use std::{cell::RefCell, collections::BTreeSet, rc::Rc, time::Duration};

use tokio::{net::TcpListener, task::LocalSet};

use crate::{
    config::Config,
    crypto::{
        identity::IdentityManager,
        wrap::{RustCryptoConstructionBWrap, ROTATION_INTERVAL_SECS},
    },
    ids::PeerId,
    keystore::{atomic_private_write, KeyStore},
    net::{
        directory::{serve as serve_directory, DirectoryClient, DirectoryStore},
        join::{join_host, serve_host, unix_time, VaultHost, VaultMetadata},
        vaults::VaultSet,
        JoinCode, VaultId,
    },
    store::vaults::{discover_vaults, migrate_legacy, prepare_vault_directory},
    sync::host::{HostService, MemberReplica},
    Error, Result,
};

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
    LocalSet::new().run_until(run_inner(config)).await
}

async fn run_inner(config: Config) -> Result<()> {
    let mut shutdown = Shutdown::install()?;
    std::fs::create_dir_all(&config.data_dir)?;
    if config.directory {
        return run_directory(&config, &mut shutdown).await;
    }

    let host_id = config.load_host_id()?;
    let keys = KeyStore::open(&config.identity_path())?;
    let identity = keys.load_or_create()?;
    let listener = TcpListener::bind(config.listen_addr).await?;
    let local_addr = listener.local_addr()?;
    let wraps = RustCryptoConstructionBWrap::new(keys.clone());

    if config.join_code.is_none() {
        // Migration runs before opening any replica handles, so every durable
        // path and the one shared keystore lock still refer to the same layout.
        if config.create_vault && config.data_dir.join("vault").try_exists()? {
            migrate_legacy(&keys, &config.data_dir)?;
        }
        let mut directories = discover_vaults(&keys, &config.data_dir)?;
        let new_id = if config.create_vault {
            let metadata = VaultMetadata {
                vault_id: VaultId::generate()?,
                join_code: JoinCode::generate()?,
                issued_at: 0,
                members: vec![keys.peer_id()?],
                denied: BTreeSet::new(),
            };
            let directory = prepare_vault_directory(&keys, &config.data_dir, metadata.vault_id)?;
            if directory.join("vault").try_exists()?
                || directory.join("replica.bin").try_exists()?
            {
                return Err(Error::State("generated vault already exists"));
            }
            atomic_private_write(
                &directory.join("vault"),
                &crate::encoding::encode_vault_metadata(&metadata)?,
            )?;
            directories.push((metadata.vault_id, directory));
            Some(metadata.vault_id)
        } else {
            None
        };
        if !directories.is_empty() {
            let vaults = VaultSet::new(keys.clone())?;
            for (id, directory) in directories {
                let mut vault =
                    VaultHost::open_durable(keys.clone(), &directory.join("vault"), &directory)?;
                if let Some(directory_addr) = config.directory_addr {
                    // Re-advertise existing codes after bind too: an ephemeral
                    // port or explicit NAT address can change across restarts.
                    vault
                        .publish(
                            &DirectoryClient::new(directory_addr),
                            config.advertise_addr.unwrap_or(local_addr),
                        )
                        .await?;
                }
                if Some(id) == new_id {
                    eprintln!("qfsd: join code {}", vault.join_code());
                }
                vaults.insert(Rc::new(RefCell::new(vault)))?;
            }
            eprintln!("qfsd: vault listening {local_addr}");
            return run_vaults(listener, vaults, wraps, &mut shutdown).await;
        }
    }

    if let Some(code) = config.join_code {
        let directory_addr = config
            .directory_addr
            .ok_or(Error::InvalidInput("--join-code requires --directory-addr"))?;
        eprintln!("qfsd: member listening {local_addr}");
        tokio::task::spawn_local(reject_inbound(listener));
        return run_member(
            keys,
            DirectoryClient::new(directory_addr),
            code,
            &config.data_dir,
            &mut shutdown,
        )
        .await;
    }

    let role = member_role(&identity.peer_id, host_id.as_ref());
    let host = if role == MemberRole::Host {
        Some(HostService::open_durable(
            keys.clone(),
            &config.data_dir,
            crate::ids::FileId([0; 32]),
            [identity.peer_id].into(),
        )?)
    } else {
        None
    };
    eprintln!(
        "qfsd: identity verified; crypto ready; role {role:?}; listening {local_addr}; {} pending wraps",
        keys.pending_wraps()?.len()
    );
    run_idle_member(listener, wraps, host, &mut shutdown).await
}

async fn run_directory(config: &Config, shutdown: &mut Shutdown) -> Result<()> {
    let listener = TcpListener::bind(config.listen_addr).await?;
    let local_addr = listener.local_addr()?;
    let store = Rc::new(RefCell::new(DirectoryStore::open(
        &config.data_dir.join("directory.bin"),
    )?));
    eprintln!("qfsd: directory listening {local_addr}");
    tokio::select! {
        result = serve_directory(listener, store) => result?,
        result = shutdown.wait() => result?,
    }
    eprintln!("qfsd: shutdown complete");
    Ok(())
}

async fn run_vaults(
    listener: TcpListener,
    vaults: VaultSet,
    wraps: RustCryptoConstructionBWrap,
    shutdown: &mut Shutdown,
) -> Result<()> {
    let server = serve_host(listener, vaults.clone());
    tokio::pin!(server);
    loop {
        tokio::select! {
            result = shutdown.wait() => { result?; break; }
            result = &mut server => { result?; break; }
            _ = tokio::time::sleep(Duration::from_secs(ROTATION_INTERVAL_SECS)) => {
                let count = wraps.rotate_all()?;
                vaults.refresh_mailboxes()?;
                eprintln!("qfsd: prepared {count} rotated pair wraps");
            }
        }
    }
    vaults.stop();
    eprintln!("qfsd: shutdown complete");
    Ok(())
}

async fn run_member(
    keys: KeyStore,
    directory: DirectoryClient,
    code: crate::net::JoinCode,
    data_dir: &std::path::Path,
    shutdown: &mut Shutdown,
) -> Result<()> {
    let mut replica: Option<Rc<RefCell<MemberReplica>>> = None;
    loop {
        let attempt = async {
            let ad = directory
                .lookup(code)
                .await?
                .ok_or(Error::InvalidInput("join code not found"))?;
            ad.verify(unix_time()?)?;
            if replica.is_none() {
                replica = Some(Rc::new(RefCell::new(MemberReplica::open_durable(
                    keys.clone(),
                    data_dir,
                    crate::ids::FileId(ad.vault_id.0),
                    ad.peer_id,
                    BTreeSet::from([keys.peer_id()?, ad.peer_id]),
                )?)));
            }
            join_host(keys.clone(), &ad, code, replica.clone()).await
        };
        let mut joined = tokio::select! {
            result = shutdown.wait() => { result?; break; }
            result = attempt => match result {
                Ok(joined) => joined,
                Err(error) => {
                    eprintln!("qfsd: join retry: {error}");
                    wait_retry().await;
                    continue;
                }
            }
        };
        replica = Some(Rc::clone(&joined.replica));
        eprintln!("qfsd: joined vault");
        tokio::select! {
            result = shutdown.wait() => { result?; break; }
            result = joined.run() => {
                if let Err(error) = result {
                    eprintln!("qfsd: host disconnected; retrying: {error}");
                }
            }
        }
        wait_retry().await;
    }
    eprintln!("qfsd: shutdown complete");
    Ok(())
}

async fn wait_retry() {
    tokio::time::sleep(Duration::from_secs(1)).await;
}

async fn run_idle_member(
    listener: TcpListener,
    wraps: RustCryptoConstructionBWrap,
    mut host: Option<HostService>,
    shutdown: &mut Shutdown,
) -> Result<()> {
    let inbound = reject_inbound(listener);
    tokio::pin!(inbound);
    loop {
        tokio::select! {
            result = shutdown.wait() => { result?; break; }
            result = &mut inbound => { result?; break; }
            _ = tokio::time::sleep(Duration::from_secs(ROTATION_INTERVAL_SECS)) => {
                let count = wraps.rotate_all()?;
                if let Some(service) = &mut host { service.refresh_mailboxes()?; }
                eprintln!("qfsd: prepared {count} rotated pair wraps");
            }
        }
    }
    if let Some(service) = &mut host {
        service.stop();
    }
    eprintln!("qfsd: shutdown complete");
    Ok(())
}

async fn reject_inbound(listener: TcpListener) -> Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        drop(stream);
    }
}

struct Shutdown {
    #[cfg(unix)]
    terminate: tokio::signal::unix::Signal,
    #[cfg(unix)]
    interrupt: tokio::signal::unix::Signal,
}

impl Shutdown {
    fn install() -> Result<Self> {
        #[cfg(unix)]
        {
            Ok(Self {
                terminate: tokio::signal::unix::signal(
                    tokio::signal::unix::SignalKind::terminate(),
                )?,
                interrupt: tokio::signal::unix::signal(
                    tokio::signal::unix::SignalKind::interrupt(),
                )?,
            })
        }
        #[cfg(not(unix))]
        {
            Ok(Self {})
        }
    }

    async fn wait(&mut self) -> Result<()> {
        #[cfg(unix)]
        {
            tokio::select! {
                _ = self.interrupt.recv() => Ok(()),
                _ = self.terminate.recv() => Ok(()),
            }
        }
        #[cfg(not(unix))]
        {
            tokio::signal::ctrl_c().await?;
            Ok(())
        }
    }
}
