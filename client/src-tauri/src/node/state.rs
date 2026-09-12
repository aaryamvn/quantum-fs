//! Everything the node keeps on disk outside the backend's own files.
//!
//! Layout under the app data dir:
//! ```text
//! profile.json                     one canonical identity for the whole UI
//! servers.json                     added hosts and their admin tokens
//! recents.json                     the sidebar's newest-first list, max 8
//! demo-events.log                  demo_log mirror
//! directory.txt                    one line `ip:port`: the central directory we know
//! vaults/<vault_hex>/vault.json    our membership record for one vault
//! vaults/<vault_hex>/identity*     KeyStore (identity, .keys, .lock)
//! vaults/<vault_hex>/replica.bin   MemberReplica snapshot
//! vaults/<vault_hex>/chunks/       plaintext chunk store
//! vaults/<vault_hex>/history.json  append-only HistoryEvent list, max 2000
//! vaults/<vault_hex>/nodes.json    per-node dates and authors, so they survive a restart
//! vaults/<vault_hex>/host.json     the host's last verified ad + local fallback numbers
//! vaults/<vault_hex>/open/         files assembled for the OS to open
//! ```
//! Every file is written atomically (temp + rename) so a crash mid-write cannot leave the
//! app with half a JSON document — the backend applies the same rule to its own state.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use quantam_fs::net::{short_code, JoinCode};

use crate::fs_types::HistoryEvent;

use super::names::{color_for, hex};

/// The sidebar shows a short list; more than this and it stops being "recent".
pub const MAX_RECENTS: usize = 8;
/// History is a demo surface, not an audit log; keep the newest 2000 entries.
pub const MAX_HISTORY: usize = 2000;

/// The one identity the UI ever sees for this client. The protocol identity is per vault
/// (docs/decisions/client-backend-embed.md), so every per-vault peer id is mapped onto this
/// string before it reaches the webview.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub peer_id: String,
    pub name: String,
    pub color: String,
    /// Has a human actually named themselves? A `profile.json` written before this field
    /// existed has no `nameSet`, so it defaults to false and that client re-onboards once —
    /// deterministic, and the old value was only ever a `$USER` guess anyway.
    #[serde(default)]
    pub name_set: bool,
    /// How much local disk this client lends every vault it is a member of.
    #[serde(default = "default_contribution")]
    pub contribution_bytes: u64,
}

/// What a client contributes until the human says otherwise: 8 GiB.
pub fn default_contribution() -> u64 {
    8 * 1024 * 1024 * 1024
}

/// The smallest and largest contribution `set_profile` accepts: 1 GiB .. 256 GiB.
pub const MIN_CONTRIBUTION_BYTES: u64 = 1024 * 1024 * 1024;
pub const MAX_CONTRIBUTION_BYTES: u64 = 256 * 1024 * 1024 * 1024;

impl Profile {
    fn generate() -> Self {
        // 12 random bytes -> 24 hex characters, the `me_` form every unit expects.
        let peer_id = format!("me_{}", hex(&random_12()));
        let color = color_for(&peer_id);
        Profile {
            peer_id,
            name: String::new(),
            color,
            name_set: false,
            contribution_bytes: default_contribution(),
        }
    }
}

/// One host we can talk to. `token` is absent for a server we only learned about from a
/// join code: it serves vaults but we cannot run admin commands against it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerRecord {
    pub id: String,
    pub name: String,
    /// `ip:port` of the admin listener (the UI shows this as the server address).
    pub address: String,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub host_peer_id: Option<String>,
    #[serde(default)]
    pub directory_addr: Option<String>,
    #[serde(default)]
    pub capacity_bytes: u64,
    #[serde(default)]
    pub online: bool,
}

/// One entry of the recents list, newest first.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentRecord {
    pub vault_id: String,
    pub node_id: String,
    pub at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VaultRole {
    Owner,
    Member,
}

/// Our membership of one vault, enough to rejoin it after a restart.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultRecord {
    /// 64-hex of the backend `VaultId`; also the directory name.
    pub vault_id: String,
    pub server_id: String,
    /// Kept current from admin `STATUS`, so a rotation cannot lock us out on reconnect.
    pub join_code: String,
    pub name: String,
    pub role: VaultRole,
    /// `ip:port` of the central directory that resolves this vault's join code.
    pub directory_addr: String,
    /// The vault task has been admitted at least once. Persisted so a later restart can tell
    /// "never got in" from "was in and is not any more".
    #[serde(default)]
    pub joined_once: bool,
}

/// Dates and authors for one node. The protocol carries neither, so first sight counts as
/// creation and the answer is kept on disk: a restart must not reset every date to "now".
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeRecord {
    pub created_at: u64,
    pub modified_at: u64,
    #[serde(default)]
    pub created_by: String,
    #[serde(default)]
    pub modified_by: String,
}

/// What one vault task caches about its host, so a reconnect does not depend on the directory
/// still carrying our join code, plus the two numbers the home screen falls back to when we
/// hold no admin token for the host.
///
/// WHY a file of its own rather than two more fields in `vault.json`: `VaultRecord` is built
/// with struct literals in `node/mod.rs`, which this unit may not edit.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct VaultCache {
    /// The six characters a human types for this vault. `joinCode` in `vault.json` stays the
    /// 26-character text the protocol actually admits with; this is only the typing surface
    /// (`backend/src/net/short_code.rs`), and `STATUS` prints it for an authenticated admin.
    pub short_code: String,
    /// The host's advertised `ip:port` from the last `DirectoryAd` that verified.
    pub host_addr: String,
    /// 64-hex of the host's peer id from that ad.
    pub host_peer_id: String,
    pub host_ek: Vec<u8>,
    pub host_vk: Vec<u8>,
    pub ad_issued_at: u64,
    pub ad_signature: Vec<u8>,
    /// `members().len()` of the replica, and the manifest bytes the tree adds up to.
    pub member_count: u32,
    pub used_bytes: u64,
}

/// 12 bytes of OS entropy for the one cosmetic id in the app. The keystore's own
/// `random_bytes` is crate-private and nothing cryptographic depends on this value,
/// so `/dev/urandom` with a clock fallback is the right amount of machinery.
fn random_12() -> [u8; 12] {
    let mut out = [0u8; 12];
    if let Ok(mut device) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        if device.read_exact(&mut out).is_ok() {
            return out;
        }
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    out[..8].copy_from_slice(&nanos.to_le_bytes());
    out[8..].copy_from_slice(&std::process::id().to_le_bytes());
    out
}

/* ------------------------------------------------------------------ paths */

pub fn profile_path(data_dir: &Path) -> PathBuf {
    data_dir.join("profile.json")
}
pub fn servers_path(data_dir: &Path) -> PathBuf {
    data_dir.join("servers.json")
}
pub fn recents_path(data_dir: &Path) -> PathBuf {
    data_dir.join("recents.json")
}
pub fn vaults_root(data_dir: &Path) -> PathBuf {
    data_dir.join("vaults")
}
pub fn vault_dir(data_dir: &Path, vault_hex: &str) -> PathBuf {
    vaults_root(data_dir).join(vault_hex)
}
pub fn vault_record_path(data_dir: &Path, vault_hex: &str) -> PathBuf {
    vault_dir(data_dir, vault_hex).join("vault.json")
}
pub fn history_path(data_dir: &Path, vault_hex: &str) -> PathBuf {
    vault_dir(data_dir, vault_hex).join("history.json")
}
pub fn nodes_path(data_dir: &Path, vault_hex: &str) -> PathBuf {
    vault_dir(data_dir, vault_hex).join("nodes.json")
}
pub fn cache_path(data_dir: &Path, vault_hex: &str) -> PathBuf {
    vault_dir(data_dir, vault_hex).join("host.json")
}
/// The root loop (`node/mod.rs`, another unit's file) is the caller of the four helpers
/// below: a code-only join has to find a directory before any vault task exists.
#[allow(dead_code)]
pub fn directory_path(data_dir: &Path) -> PathBuf {
    data_dir.join("directory.txt")
}
pub fn open_dir(data_dir: &Path, vault_hex: &str) -> PathBuf {
    vault_dir(data_dir, vault_hex).join("open")
}

/* ---------------------------------------------------------- join codes */

/// What the user typed, as both forms: the canonical short code (when it is one) and the
/// `JoinCode` the protocol admits with. A 6-character code derives into its join code
/// deterministically; a 26-character one is the join code itself.
pub fn resolve_join_code(text: &str) -> Option<(Option<String>, JoinCode)> {
    if let Some(short) = short_code::normalize(text) {
        let code = short_code::derive(&short).ok()?;
        return Some((Some(short), code));
    }
    JoinCode::from_str(text.trim()).ok().map(|code| (None, code))
}

/// One place in the node module for "is this a short code?".
pub fn normalize_short(text: &str) -> Option<String> {
    short_code::normalize(text)
}

/// The central directory to resolve a join code against, in order of authority: the
/// environment, the address we wrote down last time, then any server we already know.
/// A code-only join has no server yet, which is the whole point of the file.
#[allow(dead_code)]
pub fn resolve_directory_addr(data_dir: &Path, known: &[SocketAddr]) -> Option<SocketAddr> {
    if let Ok(value) = std::env::var("QFS_DIRECTORY_ADDR") {
        if let Ok(addr) = value.trim().parse::<SocketAddr>() {
            return Some(addr);
        }
    }
    if let Ok(text) = std::fs::read_to_string(directory_path(data_dir)) {
        if let Some(addr) = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .find_map(|line| line.parse::<SocketAddr>().ok())
        {
            return Some(addr);
        }
    }
    known.first().copied()
}

/// Write down a directory address that worked, so the next code-only join needs no server.
#[allow(dead_code)]
pub fn remember_directory_addr(data_dir: &Path, addr: SocketAddr) {
    let path = directory_path(data_dir);
    let line = addr.to_string();
    if std::fs::read_to_string(&path)
        .map(|text| text.trim() == line)
        .unwrap_or(false)
    {
        return;
    }
    let _ = std::fs::create_dir_all(data_dir);
    let temp = path.with_extension("txt.tmp");
    if std::fs::write(&temp, format!("{line}\n")).is_ok() {
        let _ = std::fs::rename(&temp, &path);
    }
}

/* -------------------------------------------------------------- json i/o */

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Temp + rename, so a reader never sees a partial document.
fn write_json<T: Serialize>(path: &Path, value: &T) {
    let Ok(bytes) = serde_json::to_vec_pretty(value) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let temp = path.with_extension("json.tmp");
    if std::fs::write(&temp, &bytes).is_ok() {
        let _ = std::fs::rename(&temp, path);
    }
}

/* ------------------------------------------------------------------ load */

/// Read the profile, creating (and persisting) one on first run.
pub fn load_profile(data_dir: &Path) -> Profile {
    let path = profile_path(data_dir);
    if let Some(profile) = read_json::<Profile>(&path) {
        if profile.peer_id.starts_with("me_") {
            return profile;
        }
    }
    let profile = Profile::generate();
    let _ = save_profile(data_dir, &profile);
    profile
}

/// Write `profile.json` atomically. Unlike the other savers this one reports failure: the
/// onboarding sheet must not tell a human their name is stored when the disk said no.
pub fn save_profile(data_dir: &Path, profile: &Profile) -> std::io::Result<()> {
    let path = profile_path(data_dir);
    let bytes = serde_json::to_vec_pretty(profile)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::create_dir_all(data_dir)?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, &bytes)?;
    std::fs::rename(&temp, &path)
}

/// A stable id for this *client* — this machine plus this data directory — as 32 lowercase
/// hex characters. Never written down: it is recomputed at every start, so a data directory
/// copied to another Mac becomes a different client rather than a duplicate of the first.
///
/// `fallback` is the profile's `peer_id`, used when the platform has no machine id to give.
pub fn client_id(data_dir: &Path, fallback: &str) -> String {
    let machine = platform_uuid().unwrap_or_else(|| fallback.to_string());
    let canonical = std::fs::canonicalize(data_dir).unwrap_or_else(|_| data_dir.to_path_buf());
    let mut hash = Sha256::new();
    hash.update(b"qfs/v1/client-id/");
    hash.update(machine.as_bytes());
    hash.update(b"\0");
    hash.update(canonical.as_os_str().as_encoded_bytes());
    let digest: [u8; 32] = hash.finalize().into();
    hex(&digest[..16])
}

/// The machine's own identifier, as the OS states it. `None` when neither source answers.
fn platform_uuid() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("ioreg")
            .args(["-rd1", "-c", "IOPlatformExpertDevice"])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            if !line.contains("IOPlatformUUID") {
                continue;
            }
            // `    "IOPlatformUUID" = "0A1B..."`
            let Some((_, raw)) = line.rsplit_once('=') else {
                continue;
            };
            let value = raw.trim().trim_matches('"').to_string();
            if !value.is_empty() {
                return Some(value);
            }
        }
        None
    }
    #[cfg(not(target_os = "macos"))]
    {
        let text = std::fs::read_to_string("/etc/machine-id").ok()?;
        let value = text.trim().to_string();
        if value.is_empty() {
            None
        } else {
            Some(value)
        }
    }
}

pub fn load_servers(data_dir: &Path) -> Vec<ServerRecord> {
    read_json::<Vec<ServerRecord>>(&servers_path(data_dir)).unwrap_or_default()
}

pub fn save_servers(data_dir: &Path, servers: &[ServerRecord]) {
    write_json(&servers_path(data_dir), &servers);
}

pub fn load_recents(data_dir: &Path) -> Vec<RecentRecord> {
    let mut recents = read_json::<Vec<RecentRecord>>(&recents_path(data_dir)).unwrap_or_default();
    recents.sort_by_key(|record| std::cmp::Reverse(record.at));
    recents.truncate(MAX_RECENTS);
    recents
}

pub fn save_recents(data_dir: &Path, recents: &[RecentRecord]) {
    write_json(&recents_path(data_dir), &recents);
}

/// Every vault directory that still carries a `vault.json`; `.removed-*` directories are
/// left behind by `leave`/`delete` and are skipped by the `starts_with` filter.
pub fn load_vaults(data_dir: &Path) -> Vec<VaultRecord> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(vaults_root(data_dir)) else {
        return out;
    };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if name.len() != 64 || !name.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        if let Some(record) = read_json::<VaultRecord>(&vault_record_path(data_dir, &name)) {
            if record.vault_id == name {
                out.push(record);
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn save_vault(data_dir: &Path, record: &VaultRecord) {
    write_json(&vault_record_path(data_dir, &record.vault_id), record);
}

pub fn load_history(data_dir: &Path, vault_hex: &str) -> Vec<HistoryEvent> {
    read_json::<Vec<HistoryEvent>>(&history_path(data_dir, vault_hex)).unwrap_or_default()
}

pub fn save_history(data_dir: &Path, vault_hex: &str, history: &[HistoryEvent]) {
    let start = history.len().saturating_sub(MAX_HISTORY);
    write_json(&history_path(data_dir, vault_hex), &&history[start..]);
}

pub fn load_records(data_dir: &Path, vault_hex: &str) -> HashMap<String, NodeRecord> {
    read_json::<HashMap<String, NodeRecord>>(&nodes_path(data_dir, vault_hex)).unwrap_or_default()
}

pub fn save_records(data_dir: &Path, vault_hex: &str, records: &HashMap<String, NodeRecord>) {
    write_json(&nodes_path(data_dir, vault_hex), records);
}

pub fn load_cache(data_dir: &Path, vault_hex: &str) -> VaultCache {
    read_json::<VaultCache>(&cache_path(data_dir, vault_hex)).unwrap_or_default()
}

pub fn save_cache(data_dir: &Path, vault_hex: &str, cache: &VaultCache) {
    write_json(&cache_path(data_dir, vault_hex), cache);
}

/// Remember the short code a `CREATE_VAULT`, `ROTATE_CODE` or `KICK` reply carried, for the
/// callers (the root loop) that hold no vault task handle. Nothing else in the cache moves.
#[allow(dead_code)]
pub fn remember_short_code(data_dir: &Path, vault_hex: &str, short: &str) {
    let Some(short) = short_code::normalize(short) else {
        return;
    };
    let mut cache = load_cache(data_dir, vault_hex);
    if cache.short_code == short {
        return;
    }
    cache.short_code = short;
    save_cache(data_dir, vault_hex, &cache);
}
