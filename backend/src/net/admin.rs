//! Line-oriented local admin port for the desktop app. It reads host-local
//! state and drives the existing admission helpers; no frame kind, sealed
//! control, manifest, or chunk path is involved.

use std::{
    cell::{Cell, RefCell},
    net::SocketAddr,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
};

use crate::{
    demo_log::{self, Kind},
    ids::PeerId,
    keystore::{atomic_private_write, KeyStore},
    net::{
        directory::{DirForget, DirectoryClient},
        join::{unix_time, VaultHost},
        short_code,
        vaults::VaultSet,
        VaultId,
    },
    store::vaults::{create_vault_dir_with_code, vault_directory},
    Error, Result,
};

pub const ADMIN_IDLE_TIMEOUT: Duration = Duration::from_secs(5);
pub const MAX_ADMIN_CONNECTIONS: usize = 16;
const MAX_ADMIN_LINE: usize = 4096;
/// Rejected connections wait before the error, so guessing costs wall time.
const REJECT_DELAY: Duration = Duration::from_millis(500);

/// Everything the admin commands need; all of it host-local.
pub struct AdminContext {
    pub token: String,
    pub vaults: VaultSet,
    pub keys: KeyStore,
    pub data_dir: PathBuf,
    pub directory: Option<DirectoryClient>,
    pub directory_addr: Option<SocketAddr>,
    pub advertise_addr: SocketAddr,
    pub capacity_bytes: u64,
}

/// Accept loop for the admin port. Runs on the same LocalSet as serve_host.
pub async fn serve_admin(listener: TcpListener, ctx: AdminContext) -> Result<()> {
    let ctx = Rc::new(ctx);
    let connections = Rc::new(Cell::new(0usize));
    loop {
        let (stream, _) = listener.accept().await?;
        let Some(permit) = ConnectionPermit::acquire(&connections, MAX_ADMIN_CONNECTIONS) else {
            drop(stream);
            continue;
        };
        let ctx = ctx.clone();
        tokio::task::spawn_local(async move {
            let _permit = permit;
            if let Err(error) = serve_connection(stream, ctx).await {
                report_close(&error);
            }
        });
    }
}

/// The desktop app opens a fresh admin connection roughly every second and
/// drops it, so ordinary hang-ups are not incidents: an idle timeout or an EOF
/// says nothing at all, a reset says it quietly, and only a real protocol or
/// storage failure — a bad token included — earns a red ATTENTION line.
fn report_close(error: &Error) {
    match error {
        Error::State("admin idle timeout") => {}
        Error::Io(io)
            if matches!(
                io.kind(),
                std::io::ErrorKind::UnexpectedEof
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::BrokenPipe
            ) =>
        {
            demo_log::event(
                Kind::Sync,
                "LOCAL",
                "admin: app disconnected mid-reply",
                &[format!("reason  {error}")],
            );
        }
        Error::State("admin write timeout") => {
            demo_log::event(
                Kind::Sync,
                "LOCAL",
                "admin: app disconnected mid-reply",
                &[format!("reason  {error}")],
            );
        }
        _ => demo_log::event(
            Kind::Warning,
            "LOCAL",
            "admin: connection closed",
            &[format!("reason  {error}")],
        ),
    }
}

struct ConnectionPermit(Rc<Cell<usize>>);

impl ConnectionPermit {
    fn acquire(counter: &Rc<Cell<usize>>, limit: usize) -> Option<Self> {
        if counter.get() >= limit {
            return None;
        }
        counter.set(counter.get() + 1);
        Some(Self(counter.clone()))
    }
}

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        self.0.set(self.0.get().saturating_sub(1));
    }
}

async fn serve_connection(stream: TcpStream, ctx: Rc<AdminContext>) -> Result<()> {
    let mut reader = BufReader::new(stream);
    let mut authorized = false;
    while let Some(line) = read_line(&mut reader).await? {
        if line.is_empty() {
            continue;
        }
        // A wrong token still costs wall time and the connection; the
        // read-only commands below simply never need one.
        if let Some(token) = line.strip_prefix("AUTH ") {
            if !constant_time_eq(token.as_bytes(), ctx.token.as_bytes()) {
                let peer_addr = reader
                    .get_ref()
                    .peer_addr()
                    .map_or_else(|_| "unknown".to_owned(), |addr| addr.to_string());
                demo_log::event(
                    Kind::Warning,
                    "LOCAL",
                    "admin: rejected connection (bad token)",
                    &[format!("from  {peer_addr}")],
                );
                tokio::time::sleep(REJECT_DELAY).await;
                return write_line(reader.get_mut(), "ERR unauthorized").await;
            }
            authorized = true;
            write_line(reader.get_mut(), "OK").await?;
            continue;
        }
        let reply = match dispatch(&ctx, &line, authorized).await {
            Ok(reply) => reply,
            Err(error) => format!("ERR {}", one_line(&error.to_string())),
        };
        write_line(reader.get_mut(), &reply).await?;
    }
    Ok(())
}

/// Folds over every shared byte with no early return, then adds the length
/// check, so a wrong token never reveals how long its correct prefix was.
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut diff = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        diff |= a ^ b;
    }
    (left.len() == right.len()) & (diff == 0)
}

async fn dispatch(ctx: &AdminContext, line: &str, authorized: bool) -> Result<String> {
    let mut words = line.split_whitespace();
    let command = words.next().unwrap_or_default();
    if !authorized && !is_read_only(command) {
        return Ok("ERR unauthorized".to_owned());
    }
    match command {
        "PING" => Ok("OK".to_owned()),
        "STATUS" => status(ctx, authorized),
        "CREATE_VAULT" => create_vault(ctx, argument(&mut words)?, argument(&mut words)?).await,
        "KICK" => kick(ctx, argument(&mut words)?, argument(&mut words)?).await,
        "ROTATE_CODE" => rotate_code(ctx, argument(&mut words)?).await,
        "FORGET_VAULT" => forget_vault(ctx, argument(&mut words)?).await,
        "OPS" => ops(ctx, argument(&mut words)?, argument(&mut words)?),
        _ => Ok("ERR unknown command".to_owned()),
    }
}

/// Read-only commands answer before, and without, AUTH. Everything that
/// mutates admission or storage still requires the token.
fn is_read_only(command: &str) -> bool {
    matches!(command, "PING" | "STATUS" | "OPS")
}

fn argument<'a>(words: &mut impl Iterator<Item = &'a str>) -> Result<&'a str> {
    words.next().ok_or(Error::InvalidInput("missing argument"))
}

fn status(ctx: &AdminContext, authorized: bool) -> Result<String> {
    let host_id = ctx.keys.peer_id()?;
    let mut lines = vec![format!(
        "SERVER {} {} {} {} {}",
        hex(&host_id.0),
        ctx.directory_addr
            .map_or_else(|| "-".to_owned(), |addr| addr.to_string()),
        ctx.advertise_addr,
        ctx.capacity_bytes,
        env!("CARGO_PKG_VERSION"),
    )];
    for vault in ctx.vaults.vaults() {
        let state = vault.borrow();
        let vault_id = state.vault_id();
        let label = hex(&vault_id.0);
        let meta = AppMeta::read(&ctx.data_dir, vault_id);
        let members: Vec<PeerId> = state.host.members().iter().copied().collect();
        let online: Vec<bool> = members
            .iter()
            .map(|&peer| peer == host_id || state.host.presence(peer).ok().flatten().is_some())
            .collect();
        lines.push(format!(
            "VAULT {} {} {} {} {} {} {} {}",
            label,
            vault_code(&meta, &state, authorized),
            meta.quota,
            state.host.used_bytes(),
            members.len(),
            online.iter().filter(|live| **live).count(),
            meta.created_ms,
            meta.creator,
        ));
        for (&peer, &live) in members.iter().zip(online.iter()) {
            lines.push(format!(
                "MEMBER {} {} {} {} {}",
                label,
                hex(&peer.0),
                u8::from(live),
                state.host.last_seen(peer).unwrap_or(0),
                state.host.queued_ops(peer),
            ));
        }
    }
    lines.push("END".to_owned());
    Ok(lines.join("\n"))
}

async fn create_vault(ctx: &AdminContext, quota: &str, creator: &str) -> Result<String> {
    let quota: u64 = quota
        .parse()
        .map_err(|_| Error::InvalidInput("quota must be a byte count"))?;
    let creator = if creator == "-" {
        "-".to_owned()
    } else {
        hex(&parse_peer(creator)?.0)
    };
    let minted = short_code::generate()?;
    let (vault_id, directory) =
        create_vault_dir_with_code(&ctx.data_dir, &ctx.keys, short_code::derive(&minted)?)?;
    let mut vault =
        VaultHost::open_durable(ctx.keys.clone(), &directory.join("vault"), &directory)?;
    if let Some(client) = &ctx.directory {
        vault.publish(client, ctx.advertise_addr).await?;
    }
    AppMeta {
        quota,
        created_ms: unix_millis(),
        creator,
        short: minted.clone(),
    }
    .write(&directory)?;
    ctx.vaults.insert(Rc::new(RefCell::new(vault)))?;
    demo_log::event(
        Kind::Lifecycle,
        "LOCAL",
        "admin: created vault",
        &[
            format!("vault  {}", short(&vault_id.0)),
            format!("quota  {quota} bytes"),
        ],
    );
    Ok(format!("OK {} {minted}", hex(&vault_id.0)))
}

async fn kick(ctx: &AdminContext, vault: &str, peer: &str) -> Result<String> {
    let vault_id = parse_vault(vault)?;
    let target = parse_peer(peer)?;
    let hosted = hosted_vault(ctx, vault_id)?;
    let directory = require_directory(ctx)?;
    let minted = short_code::generate()?;
    VaultHost::kick_with_code(
        &hosted,
        directory,
        ctx.advertise_addr,
        target,
        short_code::derive(&minted)?,
    )
    .await?;
    store_short(ctx, vault_id, &minted)?;
    demo_log::event(
        Kind::Membership,
        "LOCAL",
        "admin: removed vault member",
        &[
            format!("vault  {}", short(&vault_id.0)),
            format!("peer  {}", demo_log::peer(target)),
        ],
    );
    Ok(format!("OK {minted}"))
}

async fn rotate_code(ctx: &AdminContext, vault: &str) -> Result<String> {
    let vault_id = parse_vault(vault)?;
    let hosted = hosted_vault(ctx, vault_id)?;
    let directory = require_directory(ctx)?;
    let rotated = short_code::generate()?;
    VaultHost::rotate_code_with(
        &hosted,
        directory,
        ctx.advertise_addr,
        short_code::derive(&rotated)?,
    )
    .await?;
    store_short(ctx, vault_id, &rotated)?;
    demo_log::event(
        Kind::Membership,
        "LOCAL",
        "admin: rotated join code",
        &[format!("vault  {}", short(&vault_id.0))],
    );
    Ok(format!("OK {rotated}"))
}

async fn forget_vault(ctx: &AdminContext, vault: &str) -> Result<String> {
    let vault_id = parse_vault(vault)?;
    let hosted = hosted_vault(ctx, vault_id)?;
    let (code, forgotten_at) = {
        let mut state = hosted.borrow_mut();
        let code = state.join_code();
        let forgotten_at = unix_time()?.max(
            state
                .host
                .admission()
                .map_or(0, |metadata| metadata.issued_at)
                .saturating_add(1),
        );
        state.host.stop();
        (code, forgotten_at)
    };
    if let Some(client) = &ctx.directory {
        if let Ok(forget) = DirForget::sign(&ctx.keys, vault_id, code, forgotten_at) {
            // Best effort: the vault stops serving locally either way.
            let _ = client.forget(&forget).await;
        }
    }
    ctx.vaults.remove(vault_id);
    let directory = vault_directory(&ctx.data_dir, vault_id);
    if directory.try_exists()? {
        let removed =
            directory.with_file_name(format!(".removed-{}-{}", hex(&vault_id.0), unix_time()?));
        std::fs::rename(&directory, &removed)?;
    }
    demo_log::event(
        Kind::Lifecycle,
        "LOCAL",
        "admin: forgot vault",
        &[format!("vault  {}", short(&vault_id.0))],
    );
    Ok("OK".to_owned())
}

fn ops(ctx: &AdminContext, vault: &str, since: &str) -> Result<String> {
    let vault_id = parse_vault(vault)?;
    let since: u64 = since
        .parse()
        .map_err(|_| Error::InvalidInput("since must be a record id"))?;
    let hosted = hosted_vault(ctx, vault_id)?;
    let mut lines: Vec<String> = hosted
        .borrow()
        .host
        .recent_ops(since)
        .into_iter()
        .map(|(id, actor, at)| format!("OP {id} {} {at}", hex(&actor.0)))
        .collect();
    lines.push("END".to_owned());
    Ok(lines.join("\n"))
}

fn hosted_vault(ctx: &AdminContext, vault_id: VaultId) -> Result<Rc<RefCell<VaultHost>>> {
    ctx.vaults
        .get(vault_id)
        .ok_or(Error::InvalidInput("unknown vault"))
}

fn require_directory(ctx: &AdminContext) -> Result<&DirectoryClient> {
    ctx.directory
        .as_ref()
        .ok_or(Error::InvalidInput("host has no directory address"))
}

/// The join code a STATUS line prints: the short code this host minted when it
/// has one, the 26-character protocol code otherwise, and `-` to callers that
/// have not authenticated.
fn vault_code(meta: &AppMeta, state: &VaultHost, authorized: bool) -> String {
    if !authorized {
        return "-".to_owned();
    }
    if meta.short.is_empty() {
        return state.join_code().to_string();
    }
    meta.short.clone()
}

/// Records the short code a rotation installed. Vaults created before the
/// per-vault directory layout have none, and keep printing the long code.
fn store_short(ctx: &AdminContext, vault_id: VaultId, short: &str) -> Result<()> {
    let directory = vault_directory(&ctx.data_dir, vault_id);
    if !directory.try_exists()? {
        return Ok(());
    }
    let mut meta = AppMeta::read(&ctx.data_dir, vault_id);
    meta.short = short.to_owned();
    meta.write(&directory)
}

/// Cosmetic per-vault fields the wire protocol has no room for.
struct AppMeta {
    quota: u64,
    created_ms: u64,
    creator: String,
    short: String,
}

impl AppMeta {
    fn read(data_dir: &Path, vault_id: VaultId) -> Self {
        let mut meta = Self {
            quota: 0,
            created_ms: 0,
            creator: "-".to_owned(),
            short: String::new(),
        };
        let Ok(text) =
            std::fs::read_to_string(vault_directory(data_dir, vault_id).join("app-meta"))
        else {
            return meta;
        };
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key {
                "quota" => meta.quota = value.trim().parse().unwrap_or(0),
                "created_ms" => meta.created_ms = value.trim().parse().unwrap_or(0),
                "creator" => meta.creator = one_line(value.trim()),
                "short" => {
                    meta.short = short_code::normalize(value.trim()).unwrap_or_default();
                }
                _ => {}
            }
        }
        meta
    }

    fn write(&self, directory: &Path) -> Result<()> {
        let text = format!(
            "quota={}\ncreated_ms={}\ncreator={}\nshort={}\n",
            self.quota, self.created_ms, self.creator, self.short
        );
        atomic_private_write(&directory.join("app-meta"), text.as_bytes())
    }
}

async fn read_line(reader: &mut BufReader<TcpStream>) -> Result<Option<String>> {
    let mut bytes: Vec<u8> = Vec::new();
    loop {
        let available = tokio::time::timeout(ADMIN_IDLE_TIMEOUT, reader.fill_buf())
            .await
            .map_err(|_| Error::State("admin idle timeout"))??;
        if available.is_empty() {
            return Ok(None);
        }
        let (chunk, complete) = match available.iter().position(|byte| *byte == b'\n') {
            Some(index) => (&available[..index], true),
            None => (available, false),
        };
        let taken = chunk.len();
        if bytes.len().saturating_add(taken) > MAX_ADMIN_LINE {
            return Err(Error::InvalidInput("admin line is too long"));
        }
        bytes.extend_from_slice(chunk);
        reader.consume(taken + usize::from(complete));
        if complete {
            let text = String::from_utf8(bytes)
                .map_err(|_| Error::InvalidInput("admin line is not UTF-8"))?;
            return Ok(Some(text.trim().to_owned()));
        }
    }
}

async fn write_line(stream: &mut TcpStream, text: &str) -> Result<()> {
    let mut line = text.to_owned();
    line.push('\n');
    tokio::time::timeout(ADMIN_IDLE_TIMEOUT, stream.write_all(line.as_bytes()))
        .await
        .map_err(|_| Error::State("admin write timeout"))??;
    Ok(())
}

fn one_line(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character == '\n' || character == '\r' {
                ' '
            } else {
                character
            }
        })
        .collect()
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok())
        .unwrap_or(0)
}

pub(crate) fn hex(bytes: &[u8; 32]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(64);
    for byte in bytes {
        value.push(char::from(DIGITS[usize::from(byte >> 4)]));
        value.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    value
}

fn short(bytes: &[u8; 32]) -> String {
    hex(bytes).chars().take(12).collect()
}

fn parse_hex32(value: &str) -> Result<[u8; 32]> {
    if value.len() != 64 {
        return Err(Error::InvalidInput("identifier must be 64 hex characters"));
    }
    let mut bytes = [0u8; 32];
    let text = value.as_bytes();
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = (hex_digit(text[index * 2])? << 4) | hex_digit(text[index * 2 + 1])?;
    }
    Ok(bytes)
}

fn hex_digit(byte: u8) -> Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(Error::InvalidInput("identifier must be lowercase hex")),
    }
}

fn parse_vault(value: &str) -> Result<VaultId> {
    Ok(VaultId(parse_hex32(value)?))
}

fn parse_peer(value: &str) -> Result<PeerId> {
    Ok(PeerId(parse_hex32(value)?))
}

#[cfg(test)]
mod tests {
    use super::{hex, one_line, parse_hex32};

    #[test]
    fn hex_round_trips_and_rejects_uppercase() {
        let bytes = [0xab; 32];
        assert_eq!(hex(&bytes).len(), 64);
        assert_eq!(parse_hex32(&hex(&bytes)).unwrap(), bytes);
        assert!(parse_hex32(&hex(&bytes).to_uppercase()).is_err());
        assert!(parse_hex32("abcd").is_err());
    }

    #[test]
    fn error_text_stays_on_one_line() {
        assert_eq!(one_line("bad\ninput\r"), "bad input ");
    }
}
