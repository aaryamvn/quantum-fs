//! Pure naming, time and PRNG helpers shared by the node runtime and the command layer.
//!
//! WHY this exists: the webview was built against `client/src/lib/backend/mock/engine.ts`, and
//! `npm run dev` and `npm run app` must stay indistinguishable. The state itself now lives in the
//! embedded backend node (`src/node/`, docs/decisions/client-backend-embed.md), but the *cosmetic*
//! rules the UI was screenshotted against — Finder's copy naming, the rename field's validation
//! messages, the join-code alphabet — are still this file's job, mirrored line for line from
//! `client/src/lib/path.ts` and the mock engine. Where the two could drift the mock wins.
//!
//! Nothing here touches state, so every function is `pub` and free of locks. The surface is
//! deliberately wider than any single caller needs — it is the shared vocabulary of the node
//! runtime and the command layer — so an unused helper is not a defect here.
#![allow(dead_code)]

use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

/// Extensions whose last two segments belong together — splitting them loses meaning.
pub const COMPOUND_EXTENSIONS: [&str; 7] = [
    "tar.gz", "tar.bz2", "tar.xz", "d.ts", "min.js", "min.css", "min.map",
];
/// Same ceiling `client/src/lib/path.ts` enforces under the rename field.
pub const MAX_NAME_LENGTH: usize = 255;
/// Characters that cannot survive a round trip through a real file system.
pub const ILLEGAL_NAME_CHARS: [char; 3] = ['/', ':', '\\'];
/// Same ceiling the home screen's create-vault field enforces.
pub const MAX_VAULT_NAME: usize = 40;

/// The sidebar shows a short list; more than this and it stops being "recent".
pub const MAX_RECENTS: usize = 8;
/// Crockford-ish base32 minus the ambiguous glyphs: what a join code is spelled with.
pub const JOIN_CODE_ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
/// Length of a code the UI shows and the directory resolves.
pub const JOIN_CODE_LENGTH: usize = 6;

/// Wall clock in epoch milliseconds — the unit every timestamp in `types.ts` uses.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// `validateName` from `lib/path.ts`: the message to show, or `None` when the name is fine.
/// Leading dots stay legal — `.env` and `.gitignore` are real files people keep in vaults.
pub fn name_error(name: &str) -> Option<&'static str> {
    if name.trim().is_empty() {
        return Some("Name can't be empty");
    }
    if name.contains(ILLEGAL_NAME_CHARS) {
        return Some("Names can't contain / : or \\");
    }
    if name.chars().count() > MAX_NAME_LENGTH {
        return Some("Name is too long");
    }
    None
}

/// `splitName` from `lib/path.ts`: the part a rename edits, and the extension without its dot.
/// Compound extensions (`archive.tar.gz`, `types.d.ts`) and dotfiles (`.env`) stay whole.
pub fn split_name(name: &str) -> (String, String) {
    let lower = name.to_lowercase();
    for compound in COMPOUND_EXTENSIONS {
        let cut = name.len() as isize - compound.len() as isize - 1;
        if cut > 0 && lower.ends_with(&format!(".{compound}")) {
            let cut = cut as usize;
            return (name[..cut].to_string(), name[cut + 1..].to_string());
        }
    }
    match name.rfind('.') {
        Some(dot) if dot > 0 && dot != name.len() - 1 => {
            (name[..dot].to_string(), name[dot + 1..].to_string())
        }
        _ => (name.to_string(), String::new()),
    }
}

/// Inverse of [`split_name`]; an empty extension joins to nothing, never a trailing dot.
pub fn join_name(base: &str, ext: &str) -> String {
    if ext.is_empty() {
        base.to_string()
    } else {
        format!("{base}.{ext}")
    }
}

/// The `/^(.*?)\s+copy(?:\s+(\d+))?$/i` of `lib/path.ts`, hand-rolled so no regex crate is needed.
/// Returns the stem before the suffix and the number it carried, so "x copy 2" counts up to 3.
pub fn parse_copy_suffix(base: &str) -> Option<(String, Option<u32>)> {
    let trailing_digits: String = base.chars().rev().take_while(|c| c.is_ascii_digit()).collect();
    if !trailing_digits.is_empty() {
        let head = &base[..base.len() - trailing_digits.len()];
        if head.ends_with(char::is_whitespace) {
            let head = head.trim_end();
            if let Some(stem) = strip_copy_word(head) {
                let n: String = trailing_digits.chars().rev().collect();
                return Some((stem, n.parse::<u32>().ok()));
            }
        }
    }
    strip_copy_word(base).map(|stem| (stem, None))
}

/// "`<stem>` copy" -> `<stem>`; the whitespace before "copy" is required, so "copy" alone is a name.
///
/// Matched over `text`'s own characters rather than over a lowercased copy: `to_lowercase` can
/// change a string's byte length (`İ` becomes two chars, `K` one byte instead of three), so a
/// byte offset taken from the lowercase form can land mid-character in the original and panic.
pub fn strip_copy_word(text: &str) -> Option<String> {
    // Walk back exactly four characters; `cut` ends up at the byte offset of the first of them.
    let mut cut = text.len();
    let mut tail = String::with_capacity(4);
    let mut back = text.char_indices().rev();
    for _ in 0..4 {
        let (index, ch) = back.next()?;
        tail.insert(0, ch);
        cut = index;
    }
    if !tail.eq_ignore_ascii_case("copy") {
        return None;
    }
    let head = &text[..cut];
    if !head.ends_with(char::is_whitespace) {
        return None;
    }
    Some(head.trim_end().to_string())
}

/// `uniqueName` from `lib/path.ts`: Finder's scheme, case-insensitive, suffix on the base.
pub fn unique_name(existing: &[String], desired: &str) -> String {
    let taken: HashSet<String> = existing.iter().map(|n| n.to_lowercase()).collect();
    if !taken.contains(&desired.to_lowercase()) {
        return desired.to_string();
    }
    let (base, ext) = split_name(desired);
    let (root, mut n) = match parse_copy_suffix(&base) {
        Some((root, number)) => (root, number.unwrap_or(1)),
        None => (base, 0),
    };
    loop {
        n += 1;
        let stem = if n == 1 {
            format!("{root} copy")
        } else {
            format!("{root} copy {n}")
        };
        let candidate = join_name(&stem, &ext);
        if !taken.contains(&candidate.to_lowercase()) {
            return candidate;
        }
    }
}

/// mulberry32, bit-for-bit with the mock's PRNG, so a rotated join code is the same
/// string in both runtimes. `Math.imul` is a wrapping 32-bit multiply.
pub fn mulberry32(state: &mut u32) -> f64 {
    *state = state.wrapping_add(0x6d2b79f5);
    let mut t = *state;
    t = (t ^ (t >> 15)).wrapping_mul(t | 1);
    t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
    ((t ^ (t >> 14)) as f64) / 4_294_967_296.0
}

/// A fresh join code: [`JOIN_CODE_LENGTH`] draws from [`JOIN_CODE_ALPHABET`], mock-identical.
pub fn join_code(state: &mut u32) -> String {
    (0..JOIN_CODE_LENGTH)
        .map(|_| {
            let pick = (mulberry32(state) * JOIN_CODE_ALPHABET.len() as f64) as usize;
            JOIN_CODE_ALPHABET[pick.min(JOIN_CODE_ALPHABET.len() - 1)] as char
        })
        .collect()
}

#[cfg(test)]
mod helpers {
    use super::*;

    #[test]
    fn naming_rules_match_lib_path() {
        assert_eq!(split_name("archive.tar.gz"), ("archive".into(), "tar.gz".into()));
        assert_eq!(split_name("types.d.ts"), ("types".into(), "d.ts".into()));
        assert_eq!(split_name(".env"), (".env".into(), "".into()));
        assert_eq!(split_name("poster.hdr"), ("poster".into(), "hdr".into()));

        let taken = vec!["poster.hdr".to_string(), "poster copy.hdr".to_string()];
        assert_eq!(unique_name(&taken, "poster.hdr"), "poster copy 2.hdr");
        assert_eq!(unique_name(&taken, "hero.mp4"), "hero.mp4");
        assert_eq!(unique_name(&["x copy".to_string()], "x copy"), "x copy 2");
        assert_eq!(name_error("a/b"), Some("Names can't contain / : or \\"));
        assert_eq!(name_error("   "), Some("Name can't be empty"));
        assert_eq!(name_error(&"x".repeat(256)), Some("Name is too long"));
        assert_eq!(name_error(".env"), None);
    }

    #[test]
    fn copy_suffixes_survive_names_whose_case_changes_length() {
        // U+212A KELVIN SIGN is three bytes and lowercases to a one-byte `k`, so any offset
        // taken from the lowercased form is a wrong — and here panicking — index into the
        // original. Same story for `İ`, which lowercases into two characters.
        assert_eq!(strip_copy_word("\u{212A} Copy"), Some("\u{212A}".to_string()));
        assert_eq!(strip_copy_word("\u{130} copy"), Some("\u{130}".to_string()));
        assert_eq!(strip_copy_word("copy"), None, "no whitespace before the word");
        assert_eq!(strip_copy_word("cop"), None, "shorter than the word");
        assert_eq!(strip_copy_word("Ünïcödé copy"), Some("Ünïcödé".to_string()));

        assert_eq!(
            unique_name(&["\u{212A} Copy".to_string()], "\u{212A} Copy"),
            "\u{212A} copy 2"
        );
        assert_eq!(
            unique_name(&["Ünïcödé".to_string()], "Ünïcödé"),
            "Ünïcödé copy"
        );
    }

    #[test]
    fn copy_suffix_numbers_round_trip() {
        assert_eq!(parse_copy_suffix("x copy 2"), Some(("x".into(), Some(2))));
        assert_eq!(parse_copy_suffix("x copy"), Some(("x".into(), None)));
        assert_eq!(parse_copy_suffix("x"), None);
        assert_eq!(join_name("x copy 2", "hdr"), "x copy 2.hdr");
        assert_eq!(join_name("x", ""), "x");
    }

    #[test]
    fn join_codes_are_six_base32_characters() {
        let mut state = 1_234_567_u32;
        let code = join_code(&mut state);
        assert_eq!(code.len(), JOIN_CODE_LENGTH);
        assert!(code.bytes().all(|b| JOIN_CODE_ALPHABET.contains(&b)));
        // Same seed, same code: the mock and this build must agree character for character.
        let mut again = 1_234_567_u32;
        assert_eq!(join_code(&mut again), code);
    }
}
