//! Client for the host's token-gated admin listener (docs/decisions/client-backend-embed.md).
//!
//! The protocol is UTF-8 lines terminated by `\n`. A connection that carries a token opens
//! with `AUTH <token>`; after `OK` the same connection accepts many commands. One connection
//! per call is fine and keeps failure handling trivial: a dropped socket is just a failed poll.
//!
//! `PING`, `STATUS` and `OPS` are read-only and the host answers them without `AUTH`, so a
//! server we only learned about from a join code still reports its own numbers. Everything
//! that changes the host's state (`CREATE_VAULT`, `KICK`, `ROTATE_CODE`, `FORGET_VAULT`) needs
//! the token and is refused here rather than sent to be rejected.
//!
//! Nothing here touches the sealed peer protocol — that is the whole point of the separate
//! port (docs/decisions/client-backend-embed.md, `net-tcp-admission.md`).

use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

/// A cold TCP connect on a LAN either answers fast or not at all.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
/// The host answers `STATUS` from memory; anything slower than this is a dead host.
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);
/// A `STATUS` block for a busy host: bounded so a hostile peer cannot stream forever.
const MAX_BLOCK_LINES: usize = 4096;

/// Where admin commands go, and the token that authorizes them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminTarget {
    /// `ip:port` of the admin listener.
    pub addr: String,
    /// The host's connect-string token, or empty for a server added from a join code: then
    /// `AUTH` is skipped and only the read-only commands are available.
    pub token: String,
}

/// `SERVER <host_peer> <directory_addr> <advertise_addr> <capacity_bytes> <version>`
#[derive(Debug, Clone, Default)]
pub struct AdminServer {
    pub host_peer: String,
    pub directory_addr: String,
    /// Where members reach the host itself. Read from the directory ad instead, so this is
    /// only kept for parity with the printed `STATUS` block.
    #[allow(dead_code)]
    pub advertise_addr: String,
    pub capacity_bytes: u64,
    /// The host's build. Shown nowhere yet; part of the block the protocol defines.
    #[allow(dead_code)]
    pub version: String,
}

/// `VAULT <vault> <code|-> <quota> <used> <members> <online> <created_ms> <creator|->`;
/// `code` is the 6-character short code the host prints, `-` while it has none.
#[derive(Debug, Clone)]
pub struct AdminVault {
    pub vault: String,
    /// The short code as the host prints it, or empty when the column was `-`.
    pub join_code: String,
    pub quota_bytes: u64,
    pub used_bytes: u64,
    pub member_count: u32,
    pub online_count: u32,
    pub created_ms: u64,
    pub creator: Option<String>,
}

/// `MEMBER <vault> <peer> <online> <last_seen_ms> <queued_ops>`
#[derive(Debug, Clone)]
pub struct AdminMember {
    pub vault: String,
    pub peer: String,
    pub online: bool,
    pub last_seen_at: u64,
    pub queued_ops: u32,
}

/// One `OPS` row: which control record a peer committed, and when the host saw it.
#[derive(Debug, Clone)]
pub struct AdminOp {
    pub record_id: u64,
    pub actor: String,
    pub at: u64,
}

/// A whole `STATUS` block, already split by row kind.
#[derive(Debug, Clone, Default)]
pub struct AdminStatus {
    pub server: AdminServer,
    pub vaults: Vec<AdminVault>,
    pub members: Vec<AdminMember>,
}

impl AdminStatus {
    pub fn vault(&self, vault_hex: &str) -> Option<&AdminVault> {
        self.vaults.iter().find(|v| v.vault == vault_hex)
    }

    pub fn members_of(&self, vault_hex: &str) -> Vec<&AdminMember> {
        self.members.iter().filter(|m| m.vault == vault_hex).collect()
    }

    /// Online members of every vault on this host, H itself excluded — the term the
    /// server-capacity formula adds to the host's own advertised capacity.
    pub fn online_members_excluding_host(&self) -> u64 {
        self.members
            .iter()
            .filter(|member| member.online && member.peer != self.server.host_peer)
            .count() as u64
    }
}

/// What `AUTH` answering `ERR unauthorized` becomes. The sentence is user-facing; the root loop
/// also matches on it to throw the dead token away instead of re-sending it every second (each
/// rejected `AUTH` prints an ATTENTION line on the host's console).
pub const UNAUTHORIZED: &str = "That server rejected the connect string";

/// One admin connection, authenticated when the target carried a token.
pub struct AdminConn {
    lines: BufReader<TcpStream>,
    /// `AUTH` succeeded: the state-changing commands are allowed on this connection.
    authed: bool,
}

impl AdminConn {
    /// Connect and authenticate. The error strings are already user-facing.
    pub async fn connect(target: &AdminTarget) -> Result<Self, String> {
        let stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(&target.addr))
            .await
            .map_err(|_| "Could not reach that server".to_string())?
            .map_err(|_| "Could not reach that server".to_string())?;
        let mut conn = AdminConn {
            lines: BufReader::new(stream),
            authed: false,
        };
        if target.token.is_empty() {
            // No connect string: the read-only commands still work, so this is a usable
            // connection for `PING`, `STATUS` and `OPS` and nothing else.
            return Ok(conn);
        }
        match conn.one(&format!("AUTH {}", target.token)).await {
            Ok(_) => {
                conn.authed = true;
                Ok(conn)
            }
            Err(message) if message.contains("unauthorized") => Err(UNAUTHORIZED.to_string()),
            Err(message) => Err(message),
        }
    }

    /// Refuse a state-changing command we know the host will reject.
    fn require_token(&self) -> Result<(), String> {
        if self.authed {
            Ok(())
        } else {
            Err("This server was added without a connect string".to_string())
        }
    }

    /// Send one command and read a single-line reply. `ERR <message>` becomes the error.
    async fn one(&mut self, command: &str) -> Result<String, String> {
        self.send(command).await?;
        let line = self.read_line().await?;
        parse_reply(&line)
    }

    /// Send one command and read lines until `END`.
    async fn block(&mut self, command: &str) -> Result<Vec<String>, String> {
        self.send(command).await?;
        let mut out = Vec::new();
        loop {
            let line = self.read_line().await?;
            let trimmed = line.trim_end();
            if trimmed == "END" {
                return Ok(out);
            }
            if let Some(message) = trimmed.strip_prefix("ERR ") {
                return Err(message.to_string());
            }
            if trimmed == "ERR" {
                return Err("The server rejected that command".to_string());
            }
            if out.len() >= MAX_BLOCK_LINES {
                return Err("The server sent an oversized reply".to_string());
            }
            if !trimmed.is_empty() {
                out.push(trimmed.to_string());
            }
        }
    }

    async fn send(&mut self, command: &str) -> Result<(), String> {
        let payload = format!("{command}\n");
        tokio::time::timeout(REPLY_TIMEOUT, async {
            self.lines.get_mut().write_all(payload.as_bytes()).await?;
            self.lines.get_mut().flush().await
        })
        .await
        .map_err(|_| "The server stopped responding".to_string())?
        .map_err(|_| "The server closed the connection".to_string())
    }

    async fn read_line(&mut self) -> Result<String, String> {
        let mut line = String::new();
        let read = tokio::time::timeout(REPLY_TIMEOUT, self.lines.read_line(&mut line))
            .await
            .map_err(|_| "The server stopped responding".to_string())?
            .map_err(|_| "The server closed the connection".to_string())?;
        if read == 0 {
            return Err("The server closed the connection".to_string());
        }
        Ok(line)
    }

    /* -------------------------------------------------------- commands */

    pub async fn ping(&mut self) -> Result<(), String> {
        self.one("PING").await.map(|_| ())
    }

    pub async fn status(&mut self) -> Result<AdminStatus, String> {
        let lines = self.block("STATUS").await?;
        let mut status = AdminStatus::default();
        for line in lines {
            let fields: Vec<&str> = line.split_whitespace().collect();
            match fields.as_slice() {
                ["SERVER", peer, directory, advertise, capacity, version @ ..] => {
                    status.server = AdminServer {
                        host_peer: (*peer).to_string(),
                        directory_addr: (*directory).to_string(),
                        advertise_addr: (*advertise).to_string(),
                        capacity_bytes: capacity.parse().unwrap_or(0),
                        version: version.first().map(|v| (*v).to_string()).unwrap_or_default(),
                    };
                }
                ["VAULT", vault, code, quota, used, members, online, created, rest @ ..] => {
                    status.vaults.push(AdminVault {
                        vault: (*vault).to_string(),
                        join_code: optional(code).unwrap_or_default(),
                        quota_bytes: quota.parse().unwrap_or(0),
                        used_bytes: used.parse().unwrap_or(0),
                        member_count: members.parse().unwrap_or(0),
                        online_count: online.parse().unwrap_or(0),
                        created_ms: created.parse().unwrap_or(0),
                        creator: rest.first().and_then(|value| optional(value)),
                    });
                }
                ["MEMBER", vault, peer, online, last_seen, queued] => {
                    status.members.push(AdminMember {
                        vault: (*vault).to_string(),
                        peer: (*peer).to_string(),
                        online: *online == "1",
                        last_seen_at: last_seen.parse().unwrap_or(0),
                        queued_ops: queued.parse().unwrap_or(0),
                    });
                }
                // Forward compatibility: a newer host may add row kinds we do not read.
                _ => {}
            }
        }
        Ok(status)
    }

    /// `CREATE_VAULT <quota> <creator|->` -> `(vault_hex, join_code)`.
    pub async fn create_vault(
        &mut self,
        quota_bytes: u64,
        creator: Option<&str>,
    ) -> Result<(String, String), String> {
        self.require_token()?;
        let creator = creator.unwrap_or("-");
        let reply = self
            .one(&format!("CREATE_VAULT {quota_bytes} {creator}"))
            .await?;
        let mut fields = reply.split_whitespace();
        match (fields.next(), fields.next()) {
            (Some(vault), Some(code)) => Ok((vault.to_string(), code.to_string())),
            _ => Err("The server sent an unreadable reply".to_string()),
        }
    }

    /// `KICK <vault> <peer>` -> the join code the rotation produced.
    pub async fn kick(&mut self, vault_hex: &str, peer_hex: &str) -> Result<String, String> {
        self.require_token()?;
        self.first_field(&format!("KICK {vault_hex} {peer_hex}")).await
    }

    /// `ROTATE_CODE <vault>` -> the new join code.
    pub async fn rotate_code(&mut self, vault_hex: &str) -> Result<String, String> {
        self.require_token()?;
        self.first_field(&format!("ROTATE_CODE {vault_hex}")).await
    }

    /// `FORGET_VAULT <vault>` — the host stops serving it and forgets the directory entry.
    pub async fn forget_vault(&mut self, vault_hex: &str) -> Result<(), String> {
        self.require_token()?;
        self.one(&format!("FORGET_VAULT {vault_hex}")).await.map(|_| ())
    }

    /// `OPS <vault> <since>` -> the committed control records newer than `since`.
    pub async fn ops(&mut self, vault_hex: &str, since: u64) -> Result<Vec<AdminOp>, String> {
        let lines = self.block(&format!("OPS {vault_hex} {since}")).await?;
        Ok(lines
            .iter()
            .filter_map(|line| {
                let fields: Vec<&str> = line.split_whitespace().collect();
                match fields.as_slice() {
                    ["OP", id, actor, at] => Some(AdminOp {
                        record_id: id.parse().ok()?,
                        actor: (*actor).to_string(),
                        at: at.parse().unwrap_or(0),
                    }),
                    _ => None,
                }
            })
            .collect())
    }

    async fn first_field(&mut self, command: &str) -> Result<String, String> {
        let reply = self.one(command).await?;
        reply
            .split_whitespace()
            .next()
            .map(str::to_string)
            .ok_or_else(|| "The server sent an unreadable reply".to_string())
    }
}

/// `OK`, `OK <rest>` or `ERR <message>`; anything else is a protocol mismatch.
fn parse_reply(line: &str) -> Result<String, String> {
    let trimmed = line.trim_end();
    if trimmed == "OK" {
        return Ok(String::new());
    }
    if let Some(rest) = trimmed.strip_prefix("OK ") {
        return Ok(rest.trim().to_string());
    }
    if let Some(message) = trimmed.strip_prefix("ERR ") {
        return Err(message.trim().to_string());
    }
    if trimmed == "ERR" {
        return Err("The server rejected that command".to_string());
    }
    Err("The server sent an unreadable reply".to_string())
}

/// `-` is the protocol's "absent" marker for optional hex fields.
fn optional(value: &str) -> Option<String> {
    if value == "-" || value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}
