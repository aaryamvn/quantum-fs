//! Human-readable demo telemetry. Protocol work never waits for display pacing.
//! Only semantic operation boundaries call this module; polling stays silent.

use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{self, IsTerminal, Write},
    path::Path,
    sync::{Mutex, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::ids::{FileId, PeerId};

const MAX_PENDING: usize = 256;
const BLOCKS_PER_TICK: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Lifecycle,
    Discovery,
    Security,
    Membership,
    File,
    Transfer,
    Sync,
    Warning,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Self::Lifecycle => "SYSTEM",
            Self::Discovery => "DISCOVERY",
            Self::Security => "SECURE",
            Self::Membership => "PEERS",
            Self::File => "FILE",
            Self::Transfer => "TRANSFER",
            Self::Sync => "SYNC",
            Self::Warning => "ATTENTION",
        }
    }

    fn color(self) -> &'static str {
        match self {
            Self::Lifecycle => "\x1b[1;37m",
            Self::Discovery => "\x1b[34m",
            Self::Security => "\x1b[35m",
            Self::Membership => "\x1b[36m",
            Self::File => "\x1b[32m",
            Self::Transfer => "\x1b[1;36m",
            Self::Sync => "\x1b[33m",
            Self::Warning => "\x1b[1;31m",
        }
    }
}

#[derive(Clone)]
struct Event {
    timestamp: String,
    kind: Kind,
    standard: String,
    headline: String,
    details: Vec<String>,
    count: usize,
}

struct State {
    color: bool,
    mirror: Option<File>,
    pending: Vec<Event>,
    overflow: BTreeMap<Kind, usize>,
    transfers: BTreeMap<(PeerId, FileId, u64), Transfer>,
}

struct Transfer {
    total: usize,
    available: usize,
    sources: BTreeMap<PeerId, (usize, usize)>,
    changed: bool,
}

static LOG: OnceLock<Mutex<State>> = OnceLock::new();

/// Start automatically for every daemon role, including a directory or restart.
/// Library embedders can call this once to display their local operations too.
pub fn start(role: &str) {
    LOG.get_or_init(|| {
        std::thread::spawn(|| loop {
            std::thread::sleep(Duration::from_millis(750));
            flush();
        });
        Mutex::new(State {
            color: std::env::var_os("NO_COLOR").is_none()
                && (io::stderr().is_terminal()
                    || std::env::var("FORCE_COLOR").is_ok_and(|value| value != "0")),
            mirror: None,
            pending: Vec::new(),
            overflow: BTreeMap::new(),
            transfers: BTreeMap::new(),
        })
    });
    event(
        Kind::Lifecycle,
        "QFS DEMO",
        format!("{role} | live backend events"),
        &[
            "X-Wing = X25519 + ML-KEM-768 | signatures ML-DSA-65 | transport AES-256-GCM".into(),
            "PEERS cyan | FILE green | SECURE magenta | SYNC amber | ATTENTION red".into(),
            "Completed actions only; heartbeats and availability polling stay silent".into(),
        ],
    );
}

/// Plain-text, complete event blocks for the optional multi-VM corner monitor.
pub fn set_log_file(path: &Path) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    // `mode` only applies when the file is created, so tighten an existing one.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    if let Some(log) = LOG.get() {
        if let Ok(mut state) = log.lock() {
            state.mirror = Some(file);
        }
    }
    Ok(())
}

/// Record a real outcome. Never pass key material, contents, or admission codes
/// except the deliberately displayed, locally generated startup join code.
pub fn event(kind: Kind, standard: &str, headline: impl AsRef<str>, details: &[String]) {
    let Some(log) = LOG.get() else { return };
    let Ok(mut state) = log.lock() else { return };
    let event = Event {
        timestamp: timestamp(),
        kind,
        standard: safe_text(standard, 64),
        headline: safe_text(headline.as_ref(), 240),
        details: details.iter().take(16).map(|s| safe_text(s, 240)).collect(),
        count: 1,
    };
    // The mirror retains every selected event even when the display summarizes
    // a burst. Its blank-line framing keeps different VM blocks intact.
    if let Some(mirror) = &mut state.mirror {
        if mirror.write_all(render(&event, false).as_bytes()).is_err() {
            state.mirror = None;
            let _ = writeln!(
                io::stderr().lock(),
                "qfsd: demo log mirror unavailable; terminal logging continues"
            );
        }
    }
    if kind == Kind::Lifecycle {
        drain(&mut state);
        write_terminal(&event, state.color);
    } else if let Some(previous) = state.pending.iter_mut().find(|previous| {
        previous.kind == event.kind
            && previous.standard == event.standard
            && previous.headline == event.headline
            && previous.details == event.details
    }) {
        previous.count += 1;
        previous.details = event.details;
    } else if state.pending.len() < MAX_PENDING {
        state.pending.push(event);
    } else {
        *state.overflow.entry(kind).or_default() += 1;
    }
}

/// Flush on graceful shutdown and before returning an error from the daemon.
pub fn flush() {
    if let Some(log) = LOG.get() {
        if let Ok(mut state) = log.lock() {
            drain(&mut state);
        }
    }
}

fn drain(state: &mut State) {
    let mut transfers = Vec::new();
    state
        .transfers
        .retain(|&(requester, file_id, _), progress| {
            if progress.changed {
                transfers.push(transfer_event(requester, file_id, progress));
                progress.changed = false;
            }
            progress.available < progress.total
        });
    for event in transfers {
        if let Some(mirror) = &mut state.mirror {
            let _ = mirror.write_all(render(&event, false).as_bytes());
        }
        state.pending.push(event);
    }
    let mut pending = std::mem::take(&mut state.pending);
    if pending.len() > BLOCKS_PER_TICK {
        // In a burst, keep the demo's principal transfer outcomes visible.
        pending.sort_by_key(|event| (event.kind != Kind::Transfer, event.kind != Kind::Warning));
    }
    let mut summarized = std::mem::take(&mut state.overflow);
    for (index, event) in pending.into_iter().enumerate() {
        if index < BLOCKS_PER_TICK {
            write_terminal(&event, state.color);
        } else {
            *summarized.entry(event.kind).or_default() += event.count;
        }
    }
    if !summarized.is_empty() {
        let details = summarized
            .into_iter()
            .map(|(kind, count)| format!("{}: {count} additional events", kind.label()))
            .collect();
        write_terminal(
            &Event {
                timestamp: timestamp(),
                kind: Kind::Sync,
                standard: "BURST".into(),
                headline: if state.mirror.is_some() {
                    "Activity burst summarized; complete events in demo-events.log"
                } else {
                    "Activity burst summarized"
                }
                .into(),
                details,
                count: 1,
            },
            state.color,
        );
    }
}

/// Sources count only newly authenticated, persisted pieces. Calls across
/// holders and bounded pull batches become one file-level block per display
/// interval; a complete file is never inferred from just a complete batch.
pub(crate) fn transfer(
    requester: PeerId,
    file_id: FileId,
    version: u64,
    total: usize,
    available: usize,
    sources: BTreeMap<PeerId, (usize, usize)>,
) {
    let Some(log) = LOG.get() else { return };
    let Ok(mut state) = log.lock() else { return };
    let key = (requester, file_id, version);
    if !state.transfers.contains_key(&key) && state.transfers.len() >= MAX_PENDING {
        // Bound observation state without affecting a transfer. Already emitted
        // partial blocks remain in the mirror; any future block starts afresh.
        drain(&mut state);
        state.transfers.clear();
    }
    let progress = state.transfers.entry(key).or_insert_with(|| Transfer {
        total,
        available,
        sources: BTreeMap::new(),
        changed: false,
    });
    // An intervening cache eviction starts a new reconstruction.
    if available < progress.available {
        progress.sources.clear();
    }
    progress.available = available;
    progress.changed = true;
    for (holder, (pieces, bytes)) in sources {
        let entry = progress.sources.entry(holder).or_default();
        entry.0 += pieces;
        entry.1 += bytes;
    }
}

fn transfer_event(requester: PeerId, file_id: FileId, progress: &Transfer) -> Event {
    let complete = progress.available == progress.total;
    let mut details = vec![format!(
        "{} / {} pieces local | {} contributing peers{}",
        progress.available,
        progress.total,
        progress.sources.len(),
        if complete {
            " | all pieces available in manifest order"
        } else {
            " | more pieces needed"
        }
    )];
    for (holder, (pieces, bytes)) in &progress.sources {
        details.push(format!(
            "{} -> {pieces} verified pieces ({bytes} bytes)",
            peer(*holder)
        ));
    }
    details.push(
        "GCM authentication + SHA-256 integrity checked against writer-signed manifest".into(),
    );
    Event {
        timestamp: timestamp(),
        kind: Kind::Transfer,
        standard: "AES-256-GCM".into(),
        headline: format!(
            "{} | {} -> {}",
            if complete {
                "File ready locally"
            } else {
                "File transfer in progress"
            },
            file(file_id),
            peer(requester)
        ),
        details,
        count: 1,
    }
}

fn write_terminal(event: &Event, color: bool) {
    let _ = io::stderr()
        .lock()
        .write_all(render(event, color).as_bytes());
}

fn render(event: &Event, color: bool) -> String {
    let (paint, reset) = if color {
        (event.kind.color(), "\x1b[0m")
    } else {
        ("", "")
    };
    let standard = if event.standard.is_empty() {
        String::new()
    } else {
        format!("[{}] ", event.standard)
    };
    let mut output = format!(
        "{} {paint}{standard}[{}]{reset} {}\n",
        event.timestamp,
        event.kind.label(),
        event.headline
    );
    for detail in &event.details {
        output.push_str(&format!("    {paint}|{reset} {detail}\n"));
    }
    if event.count > 1 {
        output.push_str(&format!(
            "    | {} similar events grouped; latest details shown\n",
            event.count
        ));
    }
    output.push('\n');
    output
}

fn timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let seconds = now.as_secs() % 86_400;
    format!(
        "{:02}:{:02}:{:02}.{:03}Z",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60,
        now.subsec_millis()
    )
}

// Names and network errors are untrusted terminal text; prevent escape/control
// injection, forged event rows, bidi overrides, and excessively wide records.
fn safe_text(value: &str, limit: usize) -> String {
    value
        .chars()
        .take(limit)
        .map(|c| {
            if c.is_control() || matches!(c, '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}') {
                ' '
            } else {
                c
            }
        })
        .collect()
}

pub fn peer(id: PeerId) -> String {
    format!("peer {}", short_id(&id.0))
}

pub fn file(id: FileId) -> String {
    format!("file {}", short_id(&id.0))
}

fn short_id(bytes: &[u8; 32]) -> String {
    bytes[..6]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untrusted_names_cannot_forge_terminal_events() {
        assert_eq!(
            safe_text("name\n\x1b[31m\t\r\u{202e}x", 100),
            "name  [31m   x"
        );
        assert_eq!(safe_text("abcdef", 3), "abc");
    }

    #[test]
    fn transfer_is_one_framed_block_with_indented_sources() {
        let event = Event {
            timestamp: "12:30:00.000Z".into(),
            kind: Kind::Transfer,
            standard: "AES-256-GCM".into(),
            headline: "File available locally".into(),
            details: vec!["peer aaaa: 2 chunks".into(), "peer bbbb: 3 chunks".into()],
            count: 1,
        };
        let plain = render(&event, false);
        assert_eq!(plain.matches("[TRANSFER]").count(), 1);
        assert!(plain.contains("\n    | peer aaaa: 2 chunks\n    | peer bbbb: 3 chunks\n\n"));
        assert!(!plain.contains('\x1b'));
        assert!(render(&event, true).contains("\x1b[1;36m"));
    }

    #[test]
    fn partial_batch_does_not_claim_a_complete_file() {
        let mut progress = Transfer {
            total: 40,
            available: 32,
            sources: [(PeerId([1; 32]), (16, 1024)), (PeerId([2; 32]), (16, 2048))].into(),
            changed: true,
        };
        let partial = render(
            &transfer_event(PeerId([3; 32]), FileId([4; 32]), &progress),
            false,
        );
        assert!(partial.contains("File transfer in progress"));
        assert!(partial.contains("32 / 40 pieces local | 2 contributing peers"));
        assert!(partial.contains("peer 010101010101 -> 16 verified pieces (1024 bytes)"));
        assert!(partial.contains("peer 020202020202 -> 16 verified pieces (2048 bytes)"));
        progress.available = 40;
        assert!(transfer_event(PeerId([3; 32]), FileId([4; 32]), &progress)
            .headline
            .starts_with("File ready locally"));
    }
}
