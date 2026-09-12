//! Helpers the node runtime needs on top of the shared naming rules.
//!
//! The UI naming rules themselves live in [`crate::fs_state`] (they mirror
//! `client/src/lib/path.ts`) and are re-exported here so the node reads one source of truth.
//! The backend tree is case-sensitive and would happily hold `Report` next to `report`, which
//! the UI forbids, so *this* layer is where that rule gets enforced before a path is submitted.

use std::str::FromStr;

use quantam_fs::net::JoinCode;

pub use crate::fs_state::{name_error, now_ms, unique_name};

/* ------------------------------------------------------------------- hex */

/// 64 lowercase hex characters of the raw 32 bytes — the id form every unit shares.
pub fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from_digit((byte >> 4) as u32, 16).unwrap_or('0'));
        out.push(char::from_digit((byte & 0x0f) as u32, 16).unwrap_or('0'));
    }
    out
}

/// Inverse of [`hex`] for the 32-byte ids; anything else is not an id.
pub fn unhex32(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    let bytes = text.as_bytes();
    let mut out = [0u8; 32];
    for (index, slot) in out.iter_mut().enumerate() {
        let high = (bytes[index * 2] as char).to_digit(16)?;
        let low = (bytes[index * 2 + 1] as char).to_digit(16)?;
        *slot = ((high << 4) | low) as u8;
    }
    Some(out)
}

/* ------------------------------------------------------------- join codes */

/// A join code is either the bare 26-character Base32 text or `"<ip>:<port>/<CODE>"`.
/// Returns the directory address when the caller supplied one.
pub fn parse_join_input(raw: &str) -> Result<(Option<String>, JoinCode), String> {
    let trimmed = raw.trim();
    let (addr, code) = match trimmed.rsplit_once('/') {
        Some((addr, code)) if !addr.is_empty() => (Some(addr.trim().to_string()), code.trim()),
        _ => (None, trimmed),
    };
    let code = JoinCode::from_str(&code.to_ascii_uppercase())
        .map_err(|_| "Invalid join code".to_string())?;
    Ok((addr, code))
}

/* ----------------------------------------------------------- presentation */

/// The "AB" a contact card draws when there is no avatar.
pub fn initials(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().filter(|w| !w.is_empty()).collect();
    let letters: String = match words.as_slice() {
        [] => String::new(),
        [single] => single.chars().take(2).collect(),
        [first, .., last] => first
            .chars()
            .take(1)
            .chain(last.chars().take(1))
            .collect::<String>(),
    };
    letters.to_uppercase()
}

/// Stable presence color for an id with no cosmetic sidecar entry yet.
pub fn color_for(seed: &str) -> String {
    // FNV-1a over the id, then one of nine hues the vault palette already uses.
    let mut hash: u32 = 0x811c_9dc5;
    for byte in seed.as_bytes() {
        hash ^= *byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    const PALETTE: [&str; 9] = [
        "#6f8cff", "#ff7a6b", "#a98bff", "#4fb3ff", "#3fc7b4", "#57c47a", "#ffb04f", "#ff7ab8",
        "#ff5f5f",
    ];
    PALETTE[(hash % PALETTE.len() as u32) as usize].to_string()
}

/// `srv_` + host:port with `:` and `.` replaced by `_`, e.g. `srv_172_26_28_115_8447`.
pub fn server_id_for(address: &str) -> String {
    format!("srv_{}", address.replace([':', '.'], "_"))
}

/// The vault root's node id; the UI derives it exactly this way.
pub fn root_node_id(vault_hex: &str) -> String {
    format!("root_{vault_hex}")
}

/// A `/a/b/c` vault path from the component chain; the root is `/`.
pub fn vault_path(components: &[String]) -> String {
    if components.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", components.join("/"))
    }
}

/// Present a backend error to the user without leaking protocol wording into dialogs.
pub fn friendly(error: &quantam_fs::Error) -> String {
    match error {
        quantam_fs::Error::AuthenticationFailed => {
            "The vault server rejected this client".to_string()
        }
        quantam_fs::Error::Io(inner) => format!("Could not reach the vault server: {inner}"),
        other => other.to_string(),
    }
}
