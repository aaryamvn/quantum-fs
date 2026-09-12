use std::{
    cell::RefCell,
    collections::BTreeSet,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    rc::Rc,
    time::Duration,
};

use tokio::{net::TcpListener, task::LocalSet};

use crate::{
    config::Config,
    crypto::{
        identity::IdentityManager,
        wrap::{RustCryptoConstructionBWrap, ROTATION_INTERVAL_SECS},
    },
    demo_log::{self, Kind},
    ids::PeerId,
    keystore::{atomic_private_write, read_private, KeyStore},
    net::{
        admin::{serve_admin, AdminContext},
        directory::{serve as serve_directory, DirectoryClient, DirectoryStore},
        join::{join_host, serve_host, unix_time, VaultHost},
        vaults::VaultSet,
        JoinCode,
    },
    store::vaults::{create_vault_dir, discover_vaults, migrate_legacy},
    sync::host::{HostService, MemberReplica},
    Error, Result,
};

/// The address other peers can reach on this machine. No packet is sent; the
/// kernel only resolves which local interface would route to a public address.
pub fn detect_lan_ip() -> Option<IpAddr> {
    let socket = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(8, 8, 8, 8), 53)).ok()?;
    Some(socket.local_addr().ok()?.ip())
}

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
    let role = if config.directory {
        "CENTRAL DIRECTORY"
    } else if config.join_code.is_some() {
        "VAULT MEMBER"
    } else {
        "VAULT SERVER"
    };
    demo_log::start(role);
    let result = LocalSet::new().run_until(run_inner(config)).await;
    demo_log::flush();
    result
}

async fn run_inner(config: Config) -> Result<()> {
    let mut shutdown = Shutdown::install()?;
    std::fs::create_dir_all(&config.data_dir)?;
    if let Err(error) = demo_log::set_log_file(&config.data_dir.join("demo-events.log")) {
        demo_log::event(
            Kind::Warning,
            "LOCAL",
            "Demo log mirror unavailable",
            &[format!("reason  {error}")],
        );
    }
    if config.directory {
        return run_directory(&config, &mut shutdown).await;
    }

    let host_id = config.load_host_id()?;
    let keys = KeyStore::open(&config.identity_path())?;
    let identity = keys.load_or_create()?;
    let listener = TcpListener::bind(config.listen_addr).await?;
    let local_addr = listener.local_addr()?;
    let wraps = RustCryptoConstructionBWrap::new(keys.clone());
    demo_log::event(
        Kind::Security,
        "X-Wing + ML-DSA-65",
        "Post-quantum identity verified",
        &[format!("local peer  {}", demo_log::peer(identity.peer_id))],
    );

    if config.join_code.is_none() {
        // Migration runs before opening any replica handles, so every durable
        // path and the one shared keystore lock still refer to the same layout.
        if config.create_vault && config.data_dir.join("vault").try_exists()? {
            migrate_legacy(&keys, &config.data_dir)?;
        }
        let mut directories = discover_vaults(&keys, &config.data_dir)?;
        let new_id = if config.create_vault {
            let (vault_id, _, directory) = create_vault_dir(&config.data_dir, &keys)?;
            directories.push((vault_id, directory));
            Some(vault_id)
        } else {
            None
        };
        let advertise = advertise_addr(&config, local_addr);
        if !directories.is_empty() || config.directory_addr.is_some() {
            let vaults = VaultSet::new(keys.clone())?;
            for (id, directory) in directories {
                let mut vault =
                    VaultHost::open_durable(keys.clone(), &directory.join("vault"), &directory)?;
                if let Some(directory_addr) = config.directory_addr {
                    // Re-advertise existing codes after bind too: an ephemeral
                    // port or explicit NAT address can change across restarts.
                    vault
                        .publish(&DirectoryClient::new(directory_addr), advertise)
                        .await?;
                }
                if Some(id) == new_id {
                    demo_log::event(
                        Kind::Lifecycle,
                        "LOCAL",
                        format!("qfsd: join code {}", vault.join_code()),
                        &[],
                    );
                }
                vaults.insert(Rc::new(RefCell::new(vault)))?;
            }
            demo_log::event(
                Kind::Lifecycle,
                "TCP",
                format!("qfsd: vault listening {local_addr}"),
                &[format!("vaults  {}", vaults.vaults().len())],
            );
            start_admin(&config, &keys, vaults.clone(), local_addr, advertise).await?;
            return run_vaults(listener, vaults, wraps, &mut shutdown).await;
        }
    }

    if let Some(code) = config.join_code {
        let directory_addr = config
            .directory_addr
            .ok_or(Error::InvalidInput("--join-code requires --directory-addr"))?;
        demo_log::event(
            Kind::Lifecycle,
            "TCP",
            format!("qfsd: member listening {local_addr}"),
            &[],
        );
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
    demo_log::event(
        Kind::Lifecycle,
        "TCP",
        format!(
            "qfsd: identity verified; crypto ready; role {role:?}; listening {local_addr}; {} pending wraps",
            keys.pending_wraps()?.len()
        ),
        &[],
    );
    run_idle_member(listener, wraps, host, &mut shutdown).await
}

/// The address hosted vaults advertise: the explicit flag, else the LAN address
/// behind an unspecified bind, else the bound address exactly as today.
fn advertise_addr(config: &Config, local_addr: SocketAddr) -> SocketAddr {
    match config.advertise_addr {
        Some(addr) => addr,
        None if local_addr.ip().is_unspecified() => match detect_lan_ip() {
            Some(ip) => SocketAddr::new(ip, local_addr.port()),
            None => local_addr,
        },
        None => local_addr,
    }
}

/// Owner-only token file so a restart keeps the app's saved connect string.
fn load_or_create_admin_token(config: &Config) -> Result<String> {
    if let Some(token) = &config.admin_token {
        return Ok(token.clone());
    }
    let path = config.data_dir.join("admin-token");
    if let Some(bytes) = read_private(&path)? {
        let stored = String::from_utf8(bytes.to_vec())
            .map_err(|_| Error::State("admin token file is not UTF-8"))?
            .trim()
            .to_owned();
        if !stored.is_empty() {
            return Ok(stored);
        }
    }
    // 20 characters of the join-code Base32 alphabet: 100 bits of entropy.
    let token: String = JoinCode::generate()?.to_string().chars().take(20).collect();
    atomic_private_write(&path, token.as_bytes())?;
    Ok(token)
}

async fn start_admin(
    config: &Config,
    keys: &KeyStore,
    vaults: VaultSet,
    local_addr: SocketAddr,
    advertise: SocketAddr,
) -> Result<()> {
    let token = load_or_create_admin_token(config)?;
    let bind = match config.admin_addr {
        Some(addr) => addr,
        None => {
            let ip = if config.listen_addr.ip().is_unspecified() {
                IpAddr::V4(Ipv4Addr::UNSPECIFIED)
            } else {
                config.listen_addr.ip()
            };
            let port = local_addr
                .port()
                .checked_add(1000)
                .ok_or(Error::InvalidInput(
                    "listen port too high for the default admin port; pass --admin-addr",
                ))?;
            SocketAddr::new(ip, port)
        }
    };
    let listener = TcpListener::bind(bind).await?;
    let admin_addr = listener.local_addr()?;
    let connect = format!("{}:{}/{token}", advertise.ip(), admin_addr.port());
    demo_log::event(
        Kind::Lifecycle,
        "LOCAL",
        "qfsd: app connect string",
        &[
            "connect  printed below (contains the admin token)".to_owned(),
            format!("address  {advertise}"),
        ],
    );
    eprintln!("qfsd: app connect string {connect}");
    let context = AdminContext {
        token,
        vaults,
        keys: keys.clone(),
        data_dir: config.data_dir.clone(),
        directory: config.directory_addr.map(DirectoryClient::new),
        directory_addr: config.directory_addr,
        advertise_addr: advertise,
        capacity_bytes: config.capacity_bytes,
    };
    tokio::task::spawn_local(async move {
        if let Err(error) = serve_admin(listener, context).await {
            demo_log::event(
                Kind::Warning,
                "LOCAL",
                "qfsd: admin port stopped",
                &[format!("reason  {error}")],
            );
        }
    });
    Ok(())
}

async fn run_directory(config: &Config, shutdown: &mut Shutdown) -> Result<()> {
    let listener = TcpListener::bind(config.listen_addr).await?;
    let local_addr = listener.local_addr()?;
    let store = Rc::new(RefCell::new(DirectoryStore::open(
        &config.data_dir.join("directory.bin"),
    )?));
    demo_log::event(
        Kind::Lifecycle,
        "TCP",
        format!("qfsd: directory listening {local_addr}"),
        &["stores signed routing advertisements only".to_owned()],
    );
    let reachable = if local_addr.ip().is_unspecified() {
        SocketAddr::new(
            detect_lan_ip().unwrap_or_else(|| local_addr.ip()),
            local_addr.port(),
        )
    } else {
        local_addr
    };
    demo_log::event(
        Kind::Lifecycle,
        "LOCAL",
        format!("qfsd: directory address {reachable}"),
        &["paste this as the app's central server".to_owned()],
    );
    eprintln!("qfsd: directory address {reachable}");
    tokio::select! {
        result = serve_directory(listener, store) => result?,
        result = shutdown.wait() => result?,
    }
    demo_log::event(Kind::Lifecycle, "LOCAL", "qfsd: shutdown complete", &[]);
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
                if count != 0 {
                    demo_log::event(
                        Kind::Security,
                        "X-Wing",
                        format!("qfsd: prepared {count} rotated pair wraps"),
                        &["fresh pair epochs staged for authenticated peers".to_owned()],
                    );
                }
            }
        }
    }
    vaults.stop();
    demo_log::event(Kind::Lifecycle, "LOCAL", "qfsd: shutdown complete", &[]);
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
                    demo_log::event(
                        Kind::Warning,
                        "TCP",
                        "qfsd: join retry scheduled",
                        &[format!("reason  {error}")],
                    );
                    wait_retry().await;
                    continue;
                }
            }
        };
        replica = Some(Rc::clone(&joined.replica));
        demo_log::event(
            Kind::Membership,
            "X-Wing + ML-DSA-65",
            "qfsd: joined vault",
            &[format!("host  {}", demo_log::peer(joined.peer_id()))],
        );
        tokio::select! {
            result = shutdown.wait() => { result?; break; }
            result = joined.run() => {
                if let Err(error) = result {
                    demo_log::event(
                        Kind::Warning,
                        "TCP",
                        "Host disconnected; reconnecting",
                        &[format!("reason  {error}")],
                    );
                }
            }
        }
        wait_retry().await;
    }
    demo_log::event(Kind::Lifecycle, "LOCAL", "qfsd: shutdown complete", &[]);
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
                if count != 0 {
                    demo_log::event(
                        Kind::Security,
                        "X-Wing",
                        format!("qfsd: prepared {count} rotated pair wraps"),
                        &["fresh pair epochs staged for authenticated peers".to_owned()],
                    );
                }
            }
        }
    }
    if let Some(service) = &mut host {
        service.stop();
    }
    demo_log::event(Kind::Lifecycle, "LOCAL", "qfsd: shutdown complete", &[]);
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
