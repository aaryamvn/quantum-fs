//! Client for the directory's profile store: the one place a display name outlives this
//! machine's `profile.json`.
//!
//! WHY it exists: display names are cosmetic and members are identified by peer id
//! (docs/decisions/net-vault-join-directory.md), so the protocol carries no name at all. A
//! client that has been wiped, or a second client on a fresh data dir, would therefore have to
//! be re-named by hand every time. The directory keeps `client_id -> name` so it does not.
//!
//! The wire protocol is `node/admin.rs`'s in miniature: UTF-8 lines ending `\n`, one
//! connection per request, closed after. It listens on the directory's IP at the directory
//! port plus 1000 (7440 -> 8440).
//!
//! ```text
//! PING                              -> OK
//! PROFILE_PUT <client_id> <name_b64> -> OK
//! PROFILE_GET <client_id>            -> OK <name_b64>   |  ERR not found
//! ```
//! `name_b64` is standard base64 (with padding) of the UTF-8 name; `client_id` is 32
//! lowercase hex characters. Nothing here is authenticated and nothing here is trusted: a
//! name that comes back is a cosmetic string, bounded by the reader below.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

/// A cold TCP connect on a LAN either answers fast or not at all.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// The store answers from memory; anything slower than this is a dead directory.
const REPLY_TIMEOUT: Duration = Duration::from_secs(3);
/// A reply line is `OK ` plus base64 of a 40-character name; this is generous and bounded.
const MAX_LINE_BYTES: u64 = 4096;
/// The same ceiling `set_profile` enforces, applied to whatever the store hands back.
const MAX_NAME_CHARS: usize = 40;

/// Store this client's display name under its id. `Ok(())` only when the store said `OK`.
pub async fn put(addr: SocketAddr, client_id: &str, name: &str) -> Result<(), String> {
    let line = format!("PROFILE_PUT {} {}", client_id, b64_encode(name.as_bytes()));
    let reply = round_trip(addr, &line).await?;
    match reply.strip_prefix("OK") {
        Some(_) => Ok(()),
        None => Err(reply),
    }
}

/// The name this client was last known by, or `None` when the store has never seen it.
pub async fn get(addr: SocketAddr, client_id: &str) -> Result<Option<String>, String> {
    let reply = round_trip(addr, &format!("PROFILE_GET {client_id}")).await?;
    let Some(rest) = reply.strip_prefix("OK") else {
        // `ERR not found` is the ordinary answer for a client nobody has named yet.
        return Ok(None);
    };
    let encoded = rest.trim();
    if encoded.is_empty() {
        return Ok(None);
    }
    let bytes = b64_decode(encoded).ok_or_else(|| "unreadable reply".to_string())?;
    let name = String::from_utf8(bytes).map_err(|_| "unreadable reply".to_string())?;
    let name = name.trim().to_string();
    if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
        return Ok(None);
    }
    Ok(Some(name))
}

/// One connection, one command, one line back. Every failure is a plain sentence: nothing
/// here is user-facing, it only ever reaches a one-line stderr note.
async fn round_trip(addr: SocketAddr, command: &str) -> Result<String, String> {
    let stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
        .await
        .map_err(|_| "profile store did not answer".to_string())?
        .map_err(|_| "profile store is not reachable".to_string())?;
    let mut lines = BufReader::new(stream);
    let payload = format!("{command}\n");
    tokio::time::timeout(REPLY_TIMEOUT, async {
        lines.get_mut().write_all(payload.as_bytes()).await?;
        lines.get_mut().flush().await
    })
    .await
    .map_err(|_| "profile store stopped responding".to_string())?
    .map_err(|_| "profile store closed the connection".to_string())?;

    let mut line = String::new();
    let read = tokio::time::timeout(REPLY_TIMEOUT, lines.read_line(&mut line))
        .await
        .map_err(|_| "profile store stopped responding".to_string())?
        .map_err(|_| "profile store closed the connection".to_string())?;
    if read == 0 {
        return Err("profile store closed the connection".to_string());
    }
    // Nothing here trusts the peer: an oversized line is a bad answer, not a buffer to grow.
    if read as u64 > MAX_LINE_BYTES {
        return Err("profile store sent an oversized reply".to_string());
    }
    let line = line.trim_end().to_string();
    if line.is_empty() {
        return Err("profile store sent nothing".to_string());
    }
    Ok(line)
}

/* ------------------------------------------------------------------ base64 */

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding. Hand-written for the same reason the node writes its own hex:
/// one wire field is not worth a dependency.
fn b64_encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for group in input.chunks(3) {
        let b0 = group[0] as u32;
        let b1 = *group.get(1).unwrap_or(&0) as u32;
        let b2 = *group.get(2).unwrap_or(&0) as u32;
        let packed = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[(packed >> 18) as usize & 63] as char);
        out.push(ALPHABET[(packed >> 12) as usize & 63] as char);
        out.push(if group.len() > 1 {
            ALPHABET[(packed >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if group.len() > 2 {
            ALPHABET[packed as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// The inverse. `None` for anything that is not well-formed standard base64.
fn b64_decode(input: &str) -> Option<Vec<u8>> {
    let bytes: &[u8] = input.as_bytes();
    if bytes.is_empty() || bytes.len() % 4 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for group in bytes.chunks(4) {
        let mut packed = 0u32;
        let mut real = 0usize;
        for (index, byte) in group.iter().enumerate() {
            if *byte == b'=' {
                // Padding is only ever the last one or two characters.
                if index < 2 || group.iter().skip(index).any(|b| *b != b'=') {
                    return None;
                }
                packed <<= 6;
                continue;
            }
            let value = ALPHABET.iter().position(|c| c == byte)? as u32;
            packed = (packed << 6) | value;
            real += 1;
        }
        match real {
            4 => {
                out.push((packed >> 16) as u8);
                out.push((packed >> 8) as u8);
                out.push(packed as u8);
            }
            3 => {
                out.push((packed >> 16) as u8);
                out.push((packed >> 8) as u8);
            }
            2 => out.push((packed >> 16) as u8),
            _ => return None,
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{b64_decode, b64_encode};

    #[test]
    fn base64_round_trips() {
        for text in ["", "a", "ab", "abc", "abcd", "Ada Lovelace", "naïve ☃"] {
            let encoded = b64_encode(text.as_bytes());
            if text.is_empty() {
                assert!(encoded.is_empty());
                continue;
            }
            assert_eq!(
                b64_decode(&encoded).as_deref(),
                Some(text.as_bytes()),
                "round trip failed for {text:?}"
            );
        }
    }

    #[test]
    fn base64_matches_the_standard_alphabet() {
        assert_eq!(b64_encode(b"Ma"), "TWE=");
        assert_eq!(b64_encode(b"Man"), "TWFu");
        assert_eq!(b64_decode("TWFu").as_deref(), Some(&b"Man"[..]));
        assert!(b64_decode("TWF").is_none());
        assert!(b64_decode("T=Fu").is_none());
    }
}
