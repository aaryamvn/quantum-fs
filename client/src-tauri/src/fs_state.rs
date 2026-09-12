//! The in-memory file system the Tauri build serves to the webview.
//!
//! WHY this exists at all: the daemon (`qfsd`) is not written yet, but the workspace UI must be
//! finished and demoable against the real seam (docs/decisions/client-workspace.md). So Rust owns
//! the same engine the browser mock owns, seeded from the same generated `fs.json`.
//!
//! This file is a line-by-line mirror of `client/src/lib/backend/mock/engine.ts`: the same error
//! strings, the same history summaries, the same id scheme, the same delta sets. Where the two
//! could drift the mock wins, because that is what the UI was built and screenshotted against —
//! `npm run dev` and `npm run app` must be indistinguishable. Naming rules (`validateName`,
//! `uniqueName`, `splitName`) mirror `client/src/lib/path.ts`.
//!
//! Every mutating method returns `(result, Vec<FsChange>)` rather than emitting itself: the caller
//! (`fs_commands`) must drop the `MutexGuard` before it emits, otherwise a listener that calls back
//! into a command would deadlock the app.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::fs_types::{
    AccessEntry, Availability, CreateNodeInput, DeleteNodesInput, DuplicateNodesInput, FsChange,
    FsNode, FsSeed, HistoryEvent, HistoryKind, Member, MemberRole, MoveNodesInput, NodeAccess,
    NodeKind, PeerPresence, PresenceInput, Recent, RenameNodeInput, SetAccessInput,
    SetNodeColorInput, VaultMeta, VaultMetaPatch,
};

/// Extensions whose last two segments belong together — splitting them loses meaning.
const COMPOUND_EXTENSIONS: [&str; 7] = [
    "tar.gz", "tar.bz2", "tar.xz", "d.ts", "min.js", "min.css", "min.map",
];
/// Same ceiling `client/src/lib/path.ts` enforces under the rename field.
const MAX_NAME_LENGTH: usize = 255;
/// Characters that cannot survive a round trip through a real file system.
const ILLEGAL_NAME_CHARS: [char; 3] = ['/', ':', '\\'];
/// Same ceiling the home screen's create-vault field enforces.
const MAX_VAULT_NAME: usize = 40;

/// The sidebar shows a short list; more than this and it stops being "recent".
const MAX_RECENTS: usize = 8;
/// Crockford-ish base32 minus the ambiguous glyphs: what a join code is spelled with.
const JOIN_CODE_ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
const JOIN_CODE_LENGTH: usize = 6;

/// Even a tiny file takes this long, so the progress ring is legible rather than a flash.
const DOWNLOAD_MIN_MS: f64 = 1200.0;
/// And even a 40 GB file finishes within a demo beat.
const DOWNLOAD_MAX_MS: f64 = 4200.0;
/// Pretend LAN throughput: 50 MiB/s. Only used to make big files feel bigger.
const DOWNLOAD_BYTES_PER_SECOND: f64 = 52_428_800.0;

/// Wall clock in epoch milliseconds — the unit every timestamp in `types.ts` uses.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// How long a simulated transfer of `size_bytes` should take, in ms.
pub fn download_ms(size_bytes: u64) -> f64 {
    (DOWNLOAD_MIN_MS + (size_bytes as f64 / DOWNLOAD_BYTES_PER_SECOND) * 1000.0)
        .clamp(DOWNLOAD_MIN_MS, DOWNLOAD_MAX_MS)
}

/// `validateName` from `lib/path.ts`: the message to show, or `None` when the name is fine.
/// Leading dots stay legal — `.env` and `.gitignore` are real files people keep in vaults.
fn name_error(name: &str) -> Option<&'static str> {
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
fn split_name(name: &str) -> (String, String) {
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
fn join_name(base: &str, ext: &str) -> String {
    if ext.is_empty() {
        base.to_string()
    } else {
        format!("{base}.{ext}")
    }
}

/// The `/^(.*?)\s+copy(?:\s+(\d+))?$/i` of `lib/path.ts`, hand-rolled so no regex crate is needed.
/// Returns the stem before the suffix and the number it carried, so "x copy 2" counts up to 3.
fn parse_copy_suffix(base: &str) -> Option<(String, Option<u32>)> {
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
fn strip_copy_word(text: &str) -> Option<String> {
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
fn unique_name(existing: &[String], desired: &str) -> String {
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
fn mulberry32(state: &mut u32) -> f64 {
    *state = state.wrapping_add(0x6d2b79f5);
    let mut t = *state;
    t = (t ^ (t >> 15)).wrapping_mul(t | 1);
    t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
    ((t ^ (t >> 14)) as f64) / 4_294_967_296.0
}

/// Managed state: the whole replicated tree of every vault this client belongs to.
pub struct FsState(pub Mutex<FsDb>);

/// The engine itself. Always reached through [`FsState`]'s mutex.
pub struct FsDb {
    nodes: HashMap<String, FsNode>,
    /// Insertion order of `nodes` — the mock iterates a `Map`, so `list_tree` must match it.
    order: Vec<String>,
    /// Only *explicit* lists. An absent key means "inherit", which is the common case.
    access: HashMap<String, Vec<AccessEntry>>,
    history: Vec<HistoryEvent>,
    /// `(vault_id, node_id, at)`, newest first, capped at [`MAX_RECENTS`].
    recents: Vec<(String, String, u64)>,
    vaults: HashMap<String, VaultMeta>,
    members: HashMap<String, Vec<Member>>,
    /// vault id -> peer id -> that peer's last published presence.
    presence: HashMap<String, HashMap<String, PeerPresence>>,
    previews: HashMap<String, String>,
    self_id: String,
    /// Per-vault counter behind the `n_<vault>_c<n>` ids new nodes get.
    node_counters: HashMap<String, u64>,
    history_counter: u64,
    agent_counter: u64,
    rng: u32,
}

impl FsDb {
    /// Boot from the one generated seed both runtimes share.
    ///
    /// The JSON is compiled in: it is a build-time asset, so a parse failure is a build mistake
    /// and must be loud rather than leave the app running with an empty tree.
    pub fn seeded() -> Self {
        const SEED_JSON: &str = include_str!("../../src/lib/backend/seed/fs.json");
        let seed: FsSeed = serde_json::from_str(SEED_JSON).unwrap_or_else(|e| {
            panic!(
                "client/src/lib/backend/seed/fs.json does not match fs_types.rs ({e}). \
                 Regenerate it with `cd client && node scripts/seed-fs.mjs`."
            )
        });

        let mut nodes = HashMap::new();
        let mut order = Vec::with_capacity(seed.nodes.len());
        for node in seed.nodes {
            order.push(node.id.clone());
            nodes.insert(node.id.clone(), node);
        }

        let mut access = HashMap::new();
        for entry in seed.access {
            if !entry.inherit {
                access.insert(entry.node_id, entry.entries);
            }
        }

        let recents: Vec<(String, String, u64)> = seed
            .recents
            .into_iter()
            .map(|r| (r.vault_id, r.node_id, r.at))
            .collect();

        // The highest seeded `h_<n>` decides where appended history starts.
        let history_counter = seed
            .history
            .iter()
            .filter_map(|h| h.id.strip_prefix("h_").and_then(|n| n.parse::<u64>().ok()))
            .max()
            .unwrap_or(0);

        // Everyone the fixture says is online is already standing in the root of their vault,
        // so the avatar row and the folder badges are populated on first paint.
        let mut presence: HashMap<String, HashMap<String, PeerPresence>> = HashMap::new();
        for (vault_id, list) in &seed.members {
            let root = nodes
                .values()
                .find(|n| &n.vault_id == vault_id && n.parent_id.is_none())
                .map(|n| n.id.clone());
            let mut by_peer = HashMap::new();
            for member in list {
                if !member.online {
                    continue;
                }
                by_peer.insert(
                    member.peer_id.clone(),
                    PeerPresence {
                        peer_id: member.peer_id.clone(),
                        online: true,
                        idle: false,
                        folder_id: root.clone(),
                        cursor: None,
                        hovering_node_id: None,
                        dragging_node_ids: Vec::new(),
                        updated_at: seed.generated_at,
                    },
                );
            }
            presence.insert(vault_id.clone(), by_peer);
        }

        FsDb {
            nodes,
            order,
            access,
            history: seed.history,
            recents,
            vaults: seed.vaults,
            members: seed.members,
            presence,
            previews: seed.previews,
            self_id: seed.self_,
            node_counters: HashMap::new(),
            history_counter,
            agent_counter: 0,
            rng: (seed.generated_at % 4_294_967_296) as u32,
        }
    }

    /* --------------------------------------------------------- internals */

    /// Demo ops may claim to be another member; everything else is this client.
    fn actor(&self, actor: Option<String>) -> String {
        actor.unwrap_or_else(|| self.self_id.clone())
    }

    fn node(&self, node_id: &str) -> Result<FsNode, String> {
        self.nodes
            .get(node_id)
            .cloned()
            .ok_or_else(|| format!("Unknown node: {node_id}"))
    }

    fn folder(&self, node_id: &str) -> Result<FsNode, String> {
        let node = self.node(node_id)?;
        if node.kind != NodeKind::Folder {
            return Err("Can't put things inside a file".to_string());
        }
        Ok(node)
    }

    fn meta(&self, vault_id: &str) -> Result<VaultMeta, String> {
        self.vaults
            .get(vault_id)
            .cloned()
            .ok_or_else(|| format!("Unknown vault: {vault_id}"))
    }

    fn root_of(&self, vault_id: &str) -> Option<String> {
        self.order
            .iter()
            .filter_map(|id| self.nodes.get(id))
            .find(|n| n.vault_id == vault_id && n.parent_id.is_none())
            .map(|n| n.id.clone())
    }

    /// Direct children of `parent_id`, in tree order.
    fn child_ids(&self, parent_id: &str) -> Vec<String> {
        self.order
            .iter()
            .filter(|id| {
                self.nodes
                    .get(*id)
                    .is_some_and(|n| n.parent_id.as_deref() == Some(parent_id))
            })
            .cloned()
            .collect()
    }

    fn child_names(&self, parent_id: &str) -> Vec<String> {
        self.child_ids(parent_id)
            .iter()
            .filter_map(|id| self.nodes.get(id).map(|n| n.name.clone()))
            .collect()
    }

    /// `root_id` plus every node beneath it, parents before children.
    fn subtree(&self, root_id: &str) -> Vec<String> {
        let mut out = vec![root_id.to_string()];
        let mut i = 0;
        while i < out.len() {
            let cur = out[i].clone();
            out.extend(self.child_ids(&cur));
            i += 1;
        }
        out
    }

    /// True when `ancestor_id` is a *proper* ancestor of `id`; a node is not its own ancestor.
    fn is_descendant(&self, id: &str, ancestor_id: &str) -> bool {
        if id == ancestor_id {
            return false;
        }
        let mut seen: HashSet<String> = HashSet::from([id.to_string()]);
        let mut parent = self.nodes.get(id).and_then(|n| n.parent_id.clone());
        while let Some(p) = parent {
            if p == ancestor_id {
                return true;
            }
            if !seen.insert(p.clone()) {
                break;
            }
            parent = self.nodes.get(&p).and_then(|n| n.parent_id.clone());
        }
        false
    }

    fn next_node_id(&mut self, vault_id: &str) -> String {
        let n = self.node_counters.entry(vault_id.to_string()).or_insert(0);
        *n += 1;
        let short = vault_id.strip_prefix("vlt_").unwrap_or(vault_id);
        format!("n_{short}_c{n}")
    }

    /// Reject a name a file system would, before anything is written. The collision message
    /// names the *existing* sibling's kind, because that is the thing in the user's way.
    fn assert_name_free(
        &self,
        parent_id: &str,
        name: &str,
        exclude: Option<&str>,
    ) -> Result<(), String> {
        if let Some(message) = name_error(name) {
            return Err(message.to_string());
        }
        let lower = name.to_lowercase();
        for id in self.child_ids(parent_id) {
            if Some(id.as_str()) == exclude {
                continue;
            }
            let Some(sibling) = self.nodes.get(&id) else {
                continue;
            };
            if sibling.name.to_lowercase() != lower {
                continue;
            }
            return Err(match sibling.kind {
                NodeKind::File => "A file with that name already exists".to_string(),
                NodeKind::Folder => "A folder with that name already exists".to_string(),
            });
        }
        Ok(())
    }

    /// Roll a size delta up the ancestor chain; every node it touches goes in `touched`.
    fn bump_sizes(&mut self, from_parent: Option<String>, delta: i64, touched: &mut Vec<String>) {
        let mut parent_id = from_parent;
        let mut seen: HashSet<String> = HashSet::new();
        while let Some(id) = parent_id {
            if !seen.insert(id.clone()) {
                break;
            }
            let Some(parent) = self.nodes.get_mut(&id) else {
                break;
            };
            if delta > 0 {
                parent.size_bytes = parent.size_bytes.saturating_add(delta as u64);
            } else if delta < 0 {
                parent.size_bytes = parent.size_bytes.saturating_sub(delta.unsigned_abs());
            }
            touched.push(id.clone());
            parent_id = parent.parent_id.clone();
        }
    }

    fn touch(&mut self, node_id: &str, actor: &str, at: u64) {
        if let Some(node) = self.nodes.get_mut(node_id) {
            node.modified_at = at;
            node.modified_by = actor.to_string();
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn record(
        &mut self,
        vault_id: &str,
        node_id: &str,
        kind: HistoryKind,
        by: &str,
        at: u64,
        from: Option<String>,
        to: Option<String>,
        summary: String,
    ) {
        self.history_counter += 1;
        self.history.push(HistoryEvent {
            id: format!("h_{}", self.history_counter),
            vault_id: vault_id.to_string(),
            node_id: node_id.to_string(),
            kind,
            at,
            by: by.to_string(),
            from,
            to,
            summary,
        });
    }

    /// The upserts for a set of ids, dropping any that vanished in the same op.
    fn upserts(&self, ids: &[String]) -> Vec<FsChange> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for id in ids {
            if !seen.insert(id.clone()) {
                continue;
            }
            if let Some(node) = self.nodes.get(id) {
                out.push(FsChange::Upsert { node: node.clone() });
            }
        }
        out
    }

    /// Drop every trace of a vault this client no longer has: tree, lists, recents, previews.
    fn forget_vault(&mut self, vault_id: &str) {
        let ids: Vec<String> = self
            .order
            .iter()
            .filter(|id| self.nodes.get(*id).is_some_and(|n| n.vault_id == vault_id))
            .cloned()
            .collect();
        let gone: HashSet<&String> = ids.iter().collect();
        for id in &ids {
            self.nodes.remove(id);
            self.access.remove(id);
            self.previews.remove(id);
        }
        self.order.retain(|id| !gone.contains(id));
        self.presence.remove(vault_id);
        self.members.remove(vault_id);
        self.vaults.remove(vault_id);
        self.history.retain(|event| event.vault_id != vault_id);
        self.recents.retain(|(v, _, _)| v != vault_id);
    }

    /* ------------------------------------------------------------ reads */

    /// The local user, as a `Member`, so avatars and presence share one shape.
    pub fn me(&self) -> Result<Member, String> {
        if let Some(primary) = self
            .members
            .get("vlt_1_1")
            .and_then(|list| list.iter().find(|m| m.is_self))
        {
            return Ok(primary.clone());
        }
        self.members
            .values()
            .flatten()
            .find(|m| m.is_self)
            .cloned()
            .ok_or_else(|| "No local member in the seed".to_string())
    }

    pub fn self_id(&self) -> String {
        self.self_id.clone()
    }

    /// Every node of a vault, root included; the tree is small because it is replicated.
    pub fn list_tree(&self, vault_id: &str) -> Vec<FsNode> {
        self.order
            .iter()
            .filter_map(|id| self.nodes.get(id))
            .filter(|n| n.vault_id == vault_id)
            .cloned()
            .collect()
    }

    /// First `max_bytes` of a text-ish file; `None` when the file has no preview.
    pub fn preview(&self, node_id: &str, max_bytes: usize) -> Option<String> {
        let body = self.previews.get(node_id)?;
        Some(body.chars().take(max_bytes).collect())
    }

    /// The effective permission list for a node: its own entries, or the nearest ancestor's.
    pub fn get_access(&self, node_id: &str) -> Result<NodeAccess, String> {
        let node = self.node(node_id)?;
        if let Some(entries) = self.access.get(&node.id) {
            return Ok(NodeAccess {
                node_id: node.id,
                inherit: false,
                entries: entries.clone(),
            });
        }
        let mut parent = node.parent_id.clone();
        let mut seen: HashSet<String> = HashSet::from([node.id.clone()]);
        while let Some(id) = parent {
            if !seen.insert(id.clone()) {
                break;
            }
            if let Some(entries) = self.access.get(&id) {
                return Ok(NodeAccess {
                    node_id: node.id,
                    inherit: true,
                    entries: entries.clone(),
                });
            }
            parent = self.nodes.get(&id).and_then(|n| n.parent_id.clone());
        }
        // Nothing explicit anywhere above: the vault default, where every member is an editor.
        Ok(NodeAccess {
            node_id: node.id,
            inherit: true,
            entries: Vec::new(),
        })
    }

    /// A folder's history is its own events plus its direct children's. Newest first.
    pub fn get_history(&self, vault_id: &str, node_id: &str) -> Vec<HistoryEvent> {
        let mut out: Vec<HistoryEvent> = self
            .history
            .iter()
            .filter(|event| {
                if event.vault_id != vault_id {
                    return false;
                }
                if event.node_id == node_id {
                    return true;
                }
                self.nodes
                    .get(&event.node_id)
                    .is_some_and(|subject| subject.parent_id.as_deref() == Some(node_id))
            })
            .cloned()
            .collect();
        out.sort_by(|a, b| b.at.cmp(&a.at));
        out
    }

    pub fn list_recents(&self) -> Vec<Recent> {
        self.recents
            .iter()
            .filter_map(|(vault_id, node_id, at)| {
                let node = self.nodes.get(node_id)?.clone();
                Some(Recent {
                    node,
                    vault_name: self
                        .vaults
                        .get(vault_id)
                        .map(|v| v.name.clone())
                        .unwrap_or_default(),
                    at: *at,
                })
            })
            .collect()
    }

    pub fn get_vault_meta(&self, vault_id: &str) -> Result<VaultMeta, String> {
        self.meta(vault_id)
    }

    pub fn list_members(&self, vault_id: &str) -> Vec<Member> {
        self.members.get(vault_id).cloned().unwrap_or_default()
    }

    fn member_index(&self, vault_id: &str, peer_id: &str) -> Result<usize, String> {
        self.members
            .get(vault_id)
            .and_then(|list| list.iter().position(|m| m.peer_id == peer_id))
            .ok_or_else(|| "That member is not in this vault".to_string())
    }

    pub fn member_count(&self, vault_id: &str) -> u32 {
        self.members.get(vault_id).map(|m| m.len()).unwrap_or(0) as u32
    }

    /// The vault's presence as the rest of the app may see it. The local cursor is blanked on
    /// the way out: a client that echoed its own pointer back would draw a second, laggier one.
    pub fn get_presence(&self, vault_id: &str) -> Vec<PeerPresence> {
        let Some(peers) = self.presence.get(vault_id) else {
            return Vec::new();
        };
        let mut out: Vec<PeerPresence> = peers
            .values()
            .map(|peer| {
                let mut copy = peer.clone();
                if copy.peer_id == self.self_id {
                    copy.cursor = None;
                }
                copy
            })
            .collect();
        out.sort_by(|a, b| a.peer_id.cmp(&b.peer_id));
        out
    }

    /* --------------------------------------------------------- mutations */

    /// Create an empty folder or file directly under `parent_id`.
    pub fn create_node(
        &mut self,
        input: CreateNodeInput,
    ) -> Result<(FsNode, Vec<FsChange>), String> {
        let actor = self.actor(input.actor.clone());
        let parent = self.folder(&input.parent_id)?;
        let name = input.name.trim().to_string();
        self.assert_name_free(&parent.id, &name, None)?;

        let at = now_ms();
        let id = self.next_node_id(&input.vault_id);
        let node = FsNode {
            id: id.clone(),
            vault_id: input.vault_id.clone(),
            parent_id: Some(parent.id.clone()),
            kind: input.kind,
            name,
            size_bytes: 0,
            created_at: at,
            modified_at: at,
            created_by: actor.clone(),
            modified_by: actor.clone(),
            color: None,
            availability: Availability::Local,
            progress: None,
            holders: match input.kind {
                NodeKind::File => vec![actor.clone()],
                NodeKind::Folder => Vec::new(),
            },
            child_count: 0,
        };
        self.nodes.insert(id.clone(), node);
        self.order.push(id.clone());
        if let Some(p) = self.nodes.get_mut(&parent.id) {
            p.child_count += 1;
        }
        self.touch(&parent.id, &actor, at);

        self.record(
            &input.vault_id,
            &id,
            HistoryKind::Created,
            &actor,
            at,
            None,
            None,
            "created".to_string(),
        );
        // A new node is empty, so no ancestor's size moved: the parent is the only
        // other row on screen that changed.
        let changes = self.upserts(&[id.clone(), parent.id.clone()]);
        let created = self.nodes.get(&id).cloned().expect("just inserted");
        Ok((created, changes))
    }

    /// Rename one node in place; its parent and children are untouched.
    pub fn rename_node(
        &mut self,
        input: RenameNodeInput,
    ) -> Result<(FsNode, Vec<FsChange>), String> {
        let actor = self.actor(input.actor.clone());
        let node = self.node(&input.node_id)?;
        let name = input.name.trim().to_string();
        let previous = node.name.clone();
        match node.parent_id.clone() {
            Some(parent_id) => self.assert_name_free(&parent_id, &name, Some(&node.id))?,
            None => {
                if let Some(message) = name_error(&name) {
                    return Err(message.to_string());
                }
            }
        }

        let at = now_ms();
        if let Some(n) = self.nodes.get_mut(&node.id) {
            n.name = name.clone();
        }
        self.touch(&node.id, &actor, at);
        if let Some(parent_id) = node.parent_id.clone() {
            self.touch(&parent_id, &actor, at);
        }

        self.record(
            &input.vault_id,
            &node.id,
            HistoryKind::Renamed,
            &actor,
            at,
            Some(previous.clone()),
            Some(name),
            format!("renamed from {previous}"),
        );
        let ids = match node.parent_id.clone() {
            Some(parent_id) => vec![node.id.clone(), parent_id],
            None => vec![node.id.clone()],
        };
        let changes = self.upserts(&ids);
        let renamed = self.nodes.get(&node.id).cloned().expect("just renamed");
        Ok((renamed, changes))
    }

    /// Reparent a selection into one folder.
    ///
    /// Validated as a whole before anything moves: a drag of five tiles where the third is the
    /// destination's own parent must fail with nothing half-applied. Nodes already in the
    /// destination are skipped rather than rejected, because dropping a mixed selection onto
    /// the folder some of it already lives in is a normal gesture.
    pub fn move_nodes(
        &mut self,
        input: MoveNodesInput,
    ) -> Result<(Vec<FsNode>, Vec<FsChange>), String> {
        let actor = self.actor(input.actor.clone());
        let target = self.folder(&input.to_parent_id)?;

        let mut moving = Vec::new();
        for node_id in &input.node_ids {
            let node = self.node(node_id)?;
            if node.id == target.id || self.is_descendant(&target.id, &node.id) {
                return Err("Can't move a folder into itself".to_string());
            }
            if node.parent_id.as_deref() == Some(target.id.as_str()) {
                continue;
            }
            moving.push(node);
        }
        if moving.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }
        for node in &moving {
            self.assert_name_free(&target.id, &node.name, Some(&node.id))?;
        }

        let at = now_ms();
        let mut touched: Vec<String> = Vec::new();
        let mut moved = Vec::new();
        for node in moving {
            let from_parent = node
                .parent_id
                .clone()
                .and_then(|id| self.nodes.get(&id).cloned());
            if let Some(parent) = &from_parent {
                if let Some(p) = self.nodes.get_mut(&parent.id) {
                    p.child_count = p.child_count.saturating_sub(1);
                }
                self.touch(&parent.id, &actor, at);
                self.bump_sizes(Some(parent.id.clone()), -(node.size_bytes as i64), &mut touched);
            }
            if let Some(n) = self.nodes.get_mut(&node.id) {
                n.parent_id = Some(target.id.clone());
            }
            self.touch(&node.id, &actor, at);
            if let Some(t) = self.nodes.get_mut(&target.id) {
                t.child_count += 1;
            }
            self.bump_sizes(Some(target.id.clone()), node.size_bytes as i64, &mut touched);

            let summary = match &from_parent {
                Some(parent) => format!("moved from {} to {}", parent.name, target.name),
                None => format!("moved to {}", target.name),
            };
            self.record(
                &input.vault_id,
                &node.id,
                HistoryKind::Moved,
                &actor,
                at,
                from_parent.as_ref().map(|p| p.name.clone()),
                Some(target.name.clone()),
                summary,
            );
            touched.push(node.id.clone());
            if let Some(n) = self.nodes.get(&node.id) {
                moved.push(n.clone());
            }
        }
        self.touch(&target.id, &actor, at);
        touched.push(target.id.clone());

        let changes = self.upserts(&touched);
        Ok((moved, changes))
    }

    /// Delete a selection and, for folders, everything beneath it.
    /// Returns the deltas plus whether the recents list lost an entry.
    pub fn delete_nodes(
        &mut self,
        input: DeleteNodesInput,
    ) -> Result<(Vec<FsChange>, bool), String> {
        let actor = self.actor(input.actor.clone());
        let at = now_ms();
        let mut removed: Vec<String> = Vec::new();
        let mut touched: Vec<String> = Vec::new();

        for node_id in &input.node_ids {
            let Some(node) = self.nodes.get(node_id).cloned() else {
                continue;
            };
            let Some(parent_id) = node.parent_id.clone() else {
                return Err("Can't delete the vault root".to_string());
            };
            let parent = self.nodes.get(&parent_id).cloned();

            let subtree = self.subtree(&node.id);
            let gone: HashSet<String> = subtree.iter().cloned().collect();
            for id in &subtree {
                self.nodes.remove(id);
                self.access.remove(id);
                self.previews.remove(id);
            }
            self.order.retain(|id| !gone.contains(id));
            removed.extend(subtree);

            if let Some(parent) = parent {
                if let Some(p) = self.nodes.get_mut(&parent.id) {
                    p.child_count = p.child_count.saturating_sub(1);
                }
                self.touch(&parent.id, &actor, at);
                self.bump_sizes(Some(parent.id.clone()), -(node.size_bytes as i64), &mut touched);
                self.record(
                    &input.vault_id,
                    &parent.id,
                    HistoryKind::Deleted,
                    &actor,
                    at,
                    Some(node.name.clone()),
                    None,
                    format!("deleted {}", node.name),
                );
            }
        }
        if removed.is_empty() {
            return Ok((Vec::new(), false));
        }

        let gone: HashSet<String> = removed.iter().cloned().collect();
        let mut changes: Vec<FsChange> = removed
            .into_iter()
            .map(|node_id| FsChange::Remove { node_id })
            .collect();
        let survivors: Vec<String> = touched.into_iter().filter(|id| !gone.contains(id)).collect();
        changes.extend(self.upserts(&survivors));

        // A recents row pointing at a deleted node is a dead link; drop it now rather than
        // letting `list_recents` quietly shrink the list on the next read.
        let before = self.recents.len();
        self.recents.retain(|(_, node_id, _)| !gone.contains(node_id));
        Ok((changes, self.recents.len() != before))
    }

    /// Copy a selection, subtrees and all.
    ///
    /// Only the top of each copied tree is renamed — the children keep their names, because
    /// they are unique within their own new folder. That is what Finder does, and it is why
    /// "Brand copy" does not contain "logo copy.svg".
    pub fn duplicate_nodes(
        &mut self,
        input: DuplicateNodesInput,
    ) -> Result<(Vec<FsNode>, Vec<FsChange>), String> {
        let actor = self.actor(input.actor.clone());
        let at = now_ms();
        let mut touched: Vec<String> = Vec::new();
        let mut tops = Vec::new();

        for node_id in &input.node_ids {
            let source = self.node(node_id)?;
            let parent_id = match input.to_parent_id.clone().or_else(|| source.parent_id.clone()) {
                Some(id) => id,
                None => return Err("Can't duplicate the vault root".to_string()),
            };
            let parent = self.folder(&parent_id)?;
            // A folder cannot receive its own copy: the walk would keep finding the nodes it has
            // just written and never terminate. Rejected up front, with the wording `move_nodes`
            // uses for the same gesture.
            if parent.id == source.id || self.is_descendant(&parent.id, &source.id) {
                return Err("Can't copy a folder into itself".to_string());
            }

            let name = unique_name(&self.child_names(&parent.id), &source.name);
            let copy_id = self.copy_subtree(&source.id, &parent.id, &name, &actor, at, &mut touched);
            let copy = self.nodes.get(&copy_id).cloned().expect("just copied");

            if let Some(p) = self.nodes.get_mut(&parent.id) {
                p.child_count += 1;
            }
            self.touch(&parent.id, &actor, at);
            self.bump_sizes(Some(parent.id.clone()), copy.size_bytes as i64, &mut touched);

            self.record(
                &input.vault_id,
                &copy.id,
                HistoryKind::Duplicated,
                &actor,
                at,
                Some(source.name.clone()),
                Some(copy.name.clone()),
                format!("duplicated from {}", source.name),
            );
            tops.push(copy);
        }

        let changes = self.upserts(&touched);
        Ok((tops, changes))
    }

    /// Depth-first copy; every new node lands in `touched` so one event covers the tree.
    fn copy_subtree(
        &mut self,
        source_id: &str,
        parent_id: &str,
        name: &str,
        actor: &str,
        at: u64,
        touched: &mut Vec<String>,
    ) -> String {
        let source = self.nodes.get(source_id).cloned().expect("checked by caller");
        // Snapshot the children *before* the copy exists. Duplicating a folder into itself would
        // otherwise put the copy in this very list and the walk would recurse forever;
        // `duplicate_nodes` refuses that gesture, and this keeps the walk finite regardless.
        let children = self.child_ids(source_id);
        let copy_id = self.next_node_id(&source.vault_id);
        let copy = FsNode {
            id: copy_id.clone(),
            parent_id: Some(parent_id.to_string()),
            name: name.to_string(),
            created_at: at,
            modified_at: at,
            created_by: actor.to_string(),
            modified_by: actor.to_string(),
            ..source.clone()
        };
        self.nodes.insert(copy_id.clone(), copy);
        self.order.push(copy_id.clone());
        touched.push(copy_id.clone());

        for child_id in children {
            let child_name = self
                .nodes
                .get(&child_id)
                .map(|n| n.name.clone())
                .unwrap_or_default();
            self.copy_subtree(&child_id, &copy_id, &child_name, actor, at, touched);
        }
        copy_id
    }

    /// Tint one folder, or clear the tint back to the default graphite.
    pub fn set_node_color(
        &mut self,
        input: SetNodeColorInput,
    ) -> Result<(FsNode, Vec<FsChange>), String> {
        let actor = self.actor(input.actor.clone());
        let node = self.node(&input.node_id)?;
        if node.kind != NodeKind::Folder {
            return Err("Only folders have colors".to_string());
        }

        let at = now_ms();
        if let Some(n) = self.nodes.get_mut(&node.id) {
            n.color = input.color;
        }
        self.touch(&node.id, &actor, at);

        let (to, summary) = match input.color {
            Some(color) => (
                Some(color.as_str().to_string()),
                format!("set color to {}", color.as_str()),
            ),
            None => (None, "cleared the color".to_string()),
        };
        self.record(
            &input.vault_id,
            &node.id,
            HistoryKind::Colored,
            &actor,
            at,
            None,
            to,
            summary,
        );
        let changes = self.upserts(std::slice::from_ref(&node.id));
        let tinted = self.nodes.get(&node.id).cloned().expect("just tinted");
        Ok((tinted, changes))
    }

    /// Flip a remote file to `downloading`. `None` = nothing to do (already here, or in flight).
    /// The second element is how long the simulated transfer should take, in ms.
    pub fn begin_download(
        &mut self,
        node_id: &str,
    ) -> Result<Option<(FsChange, f64)>, String> {
        let node = self.node(node_id)?;
        if node.availability != Availability::Remote {
            return Ok(None);
        }
        let duration = download_ms(node.size_bytes);
        let n = self.nodes.get_mut(node_id).expect("checked above");
        n.availability = Availability::Downloading;
        n.progress = Some(0.0);
        Ok(Some((FsChange::Upsert { node: n.clone() }, duration)))
    }

    /// Advance a simulated transfer by `step` (0..1). `None` once the node is gone or finished.
    pub fn tick_download(&mut self, node_id: &str, step: f64) -> Option<(FsChange, bool)> {
        let me = self.self_id.clone();
        let (node, done) = {
            let node = self.nodes.get_mut(node_id)?;
            if node.availability != Availability::Downloading {
                return None;
            }
            let next = node.progress.unwrap_or(0.0) + step;
            if next < 1.0 {
                node.progress = Some(next);
                (node.clone(), false)
            } else {
                node.availability = Availability::Local;
                node.progress = None;
                if !node.holders.iter().any(|h| h == &me) {
                    node.holders.push(me.clone());
                }
                (node.clone(), true)
            }
        };
        if done {
            self.record(
                &node.vault_id,
                &node.id,
                HistoryKind::Downloaded,
                &me,
                now_ms(),
                None,
                None,
                "downloaded to this Mac".to_string(),
            );
        }
        Some((FsChange::Upsert { node }, done))
    }

    /// Replace one node's access list, or hand it back to inheritance.
    ///
    /// `modifiedAt` deliberately does not move — a permission change is not an edit — but the
    /// inspector still has to re-read, so the node goes out as an upsert.
    pub fn set_access(
        &mut self,
        input: SetAccessInput,
    ) -> Result<(NodeAccess, Vec<FsChange>), String> {
        let node = self.node(&input.node_id)?;
        let meta = self.meta(&input.vault_id)?;
        let me = self.self_id.clone();
        let at = now_ms();

        let summary = if input.inherit {
            self.access.remove(&node.id);
            "reset access to inherited".to_string()
        } else {
            // The vault's creator is an editor by construction and can never be removed, so
            // listing them would offer a control that does nothing.
            let mut seen: HashSet<String> = HashSet::new();
            let mut entries: Vec<AccessEntry> = Vec::new();
            for entry in input.entries {
                if entry.peer_id == meta.created_by || !seen.insert(entry.peer_id.clone()) {
                    continue;
                }
                entries.push(entry);
            }
            let count = entries.len();
            self.access.insert(node.id.clone(), entries);
            format!(
                "changed access for {count} {}",
                if count == 1 { "member" } else { "members" }
            )
        };
        self.record(
            &input.vault_id,
            &node.id,
            HistoryKind::Access,
            &me,
            at,
            None,
            None,
            summary,
        );
        let changes = self.upserts(std::slice::from_ref(&node.id));
        Ok((self.get_access(&node.id)?, changes))
    }

    /// Newest first, at most [`MAX_RECENTS`] entries.
    pub fn touch_recent(&mut self, vault_id: &str, node_id: &str) {
        self.recents.retain(|(_, id, _)| id != node_id);
        self.recents
            .insert(0, (vault_id.to_string(), node_id.to_string(), now_ms()));
        self.recents.truncate(MAX_RECENTS);
    }

    /// Apply a settings patch.
    ///
    /// A vault's name is denormalized in two other places the user is looking at — the home
    /// screen's card and the tree's root folder — so renaming fans out to both. `known_servers`
    /// is the id list of `bridge.rs`'s server list, which is where the vault row actually lives;
    /// the caller applies the row move once this returns.
    pub fn update_vault_meta(
        &mut self,
        vault_id: &str,
        patch: VaultMetaPatch,
        known_servers: &[String],
    ) -> Result<VaultMetaUpdate, String> {
        let mut meta = self.meta(vault_id)?;
        let mut renamed = false;
        let mut rehomed = false;

        if let Some(name) = patch.name {
            let name = name.trim().to_string();
            if name.is_empty() || name.chars().count() > MAX_VAULT_NAME {
                return Err("Invalid vault name".to_string());
            }
            renamed = name != meta.name;
            meta.name = name;
        }
        if let Some(description) = patch.description {
            meta.description = description;
        }
        if let Some(auto_cleanup) = patch.auto_cleanup {
            meta.auto_cleanup = auto_cleanup;
        }
        if let Some(pct) = patch.cleanup_threshold_pct {
            meta.cleanup_threshold_pct = pct.clamp(1, 100);
        }
        if let Some(server_id) = patch.server_id {
            if server_id != meta.server_id {
                if !known_servers.iter().any(|id| id == &server_id) {
                    return Err(format!("Unknown server: {server_id}"));
                }
                meta.server_id = server_id;
                rehomed = true;
            }
        }
        self.vaults.insert(vault_id.to_string(), meta.clone());

        let mut changes = Vec::new();
        if renamed {
            if let Some(root_id) = self.root_of(vault_id) {
                if let Some(root) = self.nodes.get_mut(&root_id) {
                    root.name = meta.name.clone();
                }
                let me = self.self_id.clone();
                self.touch(&root_id, &me, now_ms());
                changes = self.upserts(std::slice::from_ref(&root_id));
            }
        }
        Ok(VaultMetaUpdate {
            meta,
            changes,
            renamed,
            rehomed,
        })
    }

    /// Mint a fresh code; the old one stops resolving the moment this returns.
    pub fn rotate_join_code(&mut self, vault_id: &str) -> Result<String, String> {
        let mut meta = self.meta(vault_id)?;
        let mut code = String::with_capacity(JOIN_CODE_LENGTH);
        for _ in 0..JOIN_CODE_LENGTH {
            let r = mulberry32(&mut self.rng);
            let index = (r * JOIN_CODE_ALPHABET.len() as f64).floor() as usize;
            code.push(JOIN_CODE_ALPHABET[index.min(JOIN_CODE_ALPHABET.len() - 1)] as char);
        }
        meta.join_code = code.clone();
        self.vaults.insert(vault_id.to_string(), meta);
        Ok(code)
    }

    /// Demoting the last admin would lock everyone out of the join code and settings.
    pub fn set_member_role(
        &mut self,
        vault_id: &str,
        peer_id: &str,
        role: MemberRole,
    ) -> Result<Member, String> {
        let index = self.member_index(vault_id, peer_id)?;
        let list = self.members.get_mut(vault_id).expect("checked above");
        if list[index].role == MemberRole::Admin && role != MemberRole::Admin {
            let admins = list.iter().filter(|m| m.role == MemberRole::Admin).count();
            if admins <= 1 {
                return Err("A vault needs at least one admin".to_string());
            }
        }
        list[index].role = role;
        Ok(list[index].clone())
    }

    /// Drop a member, their presence and their access entries. Returns the new member count.
    pub fn remove_member(&mut self, vault_id: &str, peer_id: &str) -> Result<u32, String> {
        if peer_id == self.self_id {
            return Err("You can't remove yourself".to_string());
        }
        let index = self.member_index(vault_id, peer_id)?;
        if let Some(list) = self.members.get_mut(vault_id) {
            list.remove(index);
        }
        if let Some(peers) = self.presence.get_mut(vault_id) {
            peers.remove(peer_id);
        }
        let node_ids: Vec<String> = self
            .order
            .iter()
            .filter(|id| self.nodes.get(*id).is_some_and(|n| n.vault_id == vault_id))
            .cloned()
            .collect();
        for id in node_ids {
            if let Some(entries) = self.access.get_mut(&id) {
                entries.retain(|entry| entry.peer_id != peer_id);
            }
        }
        Ok(self.member_count(vault_id))
    }

    /// Destroy a vault for everyone. Admin-only, because it cannot be undone.
    pub fn delete_vault(&mut self, vault_id: &str) -> Result<(), String> {
        let index = self.member_index(vault_id, &self.self_id.clone())?;
        let me = &self.members[vault_id][index];
        if me.role != MemberRole::Admin {
            return Err("Only an admin can delete a vault".to_string());
        }
        self.forget_vault(vault_id);
        Ok(())
    }

    /// Leave a vault the others keep. The last admin has to hand the role over first.
    pub fn leave_vault(&mut self, vault_id: &str) -> Result<(), String> {
        let index = self.member_index(vault_id, &self.self_id.clone())?;
        let list = &self.members[vault_id];
        if list[index].role == MemberRole::Admin
            && list.iter().filter(|m| m.role == MemberRole::Admin).count() <= 1
        {
            return Err("Make someone else an admin first".to_string());
        }
        self.forget_vault(vault_id);
        Ok(())
    }

    /// Publish this client's own presence and return the vault's peer list.
    pub fn publish_presence(&mut self, input: PresenceInput) -> Vec<PeerPresence> {
        let me = self.self_id.clone();
        let at = now_ms();
        let peers = self.presence.entry(input.vault_id.clone()).or_default();
        peers.insert(
            me.clone(),
            PeerPresence {
                peer_id: me,
                online: true,
                idle: false,
                folder_id: input.folder_id,
                // Stored so a future "follow me" can read it back; never echoed to this client.
                cursor: input.cursor,
                hovering_node_id: input.hovering_node_id,
                dragging_node_ids: input.dragging_node_ids,
                updated_at: at,
            },
        );
        self.get_presence(&input.vault_id)
    }

    /// The agent bar's stand-in reply: the one thing only a real reader of the folder
    /// could know — how much is in it — and then the plain truth about the model.
    pub fn agent_reply(&mut self, folder_id: &str) -> Result<(String, String), String> {
        let folder = self.node(folder_id)?;
        let count = self.child_ids(&folder.id).len();
        self.agent_counter += 1;
        Ok((
            format!("agent_{}", self.agent_counter),
            format!(
                "I can see {count} items in \u{201c}{}\u{201d}. Once your local Claude or Codex is \
                 connected, I can read them and act on your behalf.",
                folder.name
            ),
        ))
    }
}

/// What `update_vault_meta` changed, so the caller knows which events to emit and whether
/// the server list has to move the vault's row.
#[derive(Debug)]
pub struct VaultMetaUpdate {
    pub meta: VaultMeta,
    pub changes: Vec<FsChange>,
    pub renamed: bool,
    pub rehomed: bool,
}

#[cfg(test)]
mod parity {
    use super::*;
    use crate::fs_types::FolderColor;

    fn seeded() -> FsDb {
        FsDb::seeded()
    }

    #[test]
    fn seed_parses_and_indexes() {
        let db = seeded();
        assert_eq!(db.list_tree("vlt_1_1").len(), 96);
        assert_eq!(db.me().unwrap().peer_id, "peer_aaryaman");
        assert_eq!(db.list_recents().len(), 6);
        assert_eq!(db.member_count("vlt_1_1"), 5);
        assert!(!db.get_access("n_1_1_brand").unwrap().entries.is_empty());
        assert!(db.preview("n_1_1_069", 32).is_some());
        // Online members start standing in their vault root.
        assert!(!db.get_presence("vlt_1_1").is_empty());
    }

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
    fn create_rename_and_collisions() {
        let mut db = seeded();
        let (node, changes) = db
            .create_node(CreateNodeInput {
                vault_id: "vlt_1_1".into(),
                parent_id: "root_vlt_1_1".into(),
                kind: NodeKind::Folder,
                name: "  Launch  ".into(),
                actor: Some("peer_justin".into()),
            })
            .unwrap();
        assert_eq!(node.name, "Launch");
        assert_eq!(node.id, "n_1_1_c1");
        assert_eq!(changes.len(), 2, "the node and its parent");

        let err = db
            .create_node(CreateNodeInput {
                vault_id: "vlt_1_1".into(),
                parent_id: "root_vlt_1_1".into(),
                kind: NodeKind::File,
                name: "launch".into(),
                actor: None,
            })
            .unwrap_err();
        assert_eq!(err, "A folder with that name already exists");

        let (renamed, _) = db
            .rename_node(RenameNodeInput {
                vault_id: "vlt_1_1".into(),
                node_id: node.id.clone(),
                name: "Launch 26".into(),
                actor: None,
            })
            .unwrap();
        assert_eq!(renamed.name, "Launch 26");
        // Both events land in the same millisecond and the sort is stable by `at`, exactly as
        // the mock's is, so assert the event exists rather than that it sorts first.
        assert!(db
            .get_history("vlt_1_1", &node.id)
            .iter()
            .any(|e| e.summary == "renamed from Launch"));
    }

    #[test]
    fn duplicate_uses_finder_naming_and_copies_subtrees() {
        let mut db = seeded();
        let (copies, _) = db
            .duplicate_nodes(DuplicateNodesInput {
                vault_id: "vlt_1_1".into(),
                node_ids: vec!["n_1_1_brand".into()],
                to_parent_id: None,
                actor: None,
            })
            .unwrap();
        assert_eq!(copies[0].name, "Brand copy");
        assert_eq!(copies[0].child_count, db.node("n_1_1_brand").unwrap().child_count);
        assert_eq!(
            db.child_ids(&copies[0].id).len(),
            db.child_ids("n_1_1_brand").len()
        );

        let (again, _) = db
            .duplicate_nodes(DuplicateNodesInput {
                vault_id: "vlt_1_1".into(),
                node_ids: vec!["n_1_1_brand".into()],
                to_parent_id: None,
                actor: None,
            })
            .unwrap();
        assert_eq!(again[0].name, "Brand copy 2");
    }

    #[test]
    fn duplicating_a_folder_into_its_own_subtree_is_refused() {
        let mut db = seeded();
        for destination in ["n_1_1_projects", "n_1_1_001"] {
            assert_eq!(
                db.duplicate_nodes(DuplicateNodesInput {
                    vault_id: "vlt_1_1".into(),
                    node_ids: vec!["n_1_1_projects".into()],
                    to_parent_id: Some(destination.into()),
                    actor: None,
                })
                .unwrap_err(),
                "Can't copy a folder into itself",
                "destination {destination}"
            );
        }
        // The tree is untouched: nothing was half-copied before the refusal.
        assert_eq!(db.list_tree("vlt_1_1").len(), 96);

        // A copy into an unrelated folder still walks the whole subtree exactly once.
        let (copies, _) = db
            .duplicate_nodes(DuplicateNodesInput {
                vault_id: "vlt_1_1".into(),
                node_ids: vec!["n_1_1_projects".into()],
                to_parent_id: Some("n_1_1_archive".into()),
                actor: None,
            })
            .unwrap();
        assert_eq!(
            db.subtree(&copies[0].id).len(),
            db.subtree("n_1_1_projects").len()
        );
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
    fn moves_are_validated_as_a_whole() {
        let mut db = seeded();
        assert_eq!(
            db.move_nodes(MoveNodesInput {
                vault_id: "vlt_1_1".into(),
                node_ids: vec!["n_1_1_brand".into()],
                to_parent_id: "n_1_1_brand".into(),
                actor: None,
            })
            .unwrap_err(),
            "Can't move a folder into itself"
        );

        let root_before = db.node("root_vlt_1_1").unwrap().size_bytes;
        let file = db
            .list_tree("vlt_1_1")
            .into_iter()
            .find(|n| n.kind == NodeKind::File && n.parent_id.as_deref() == Some("n_1_1_brand"))
            .unwrap();
        let (moved, _) = db
            .move_nodes(MoveNodesInput {
                vault_id: "vlt_1_1".into(),
                node_ids: vec![file.id.clone()],
                to_parent_id: "n_1_1_archive".into(),
                actor: None,
            })
            .unwrap();
        assert_eq!(moved[0].parent_id.as_deref(), Some("n_1_1_archive"));
        // The vault root is an ancestor of both sides, so its total must not move.
        assert_eq!(db.node("root_vlt_1_1").unwrap().size_bytes, root_before);
        assert!(db.get_history("vlt_1_1", &file.id)[0]
            .summary
            .starts_with("moved from Brand to Archive"));
    }

    #[test]
    fn deletes_roll_sizes_up_and_refuse_the_root() {
        let mut db = seeded();
        assert_eq!(
            db.delete_nodes(DeleteNodesInput {
                vault_id: "vlt_1_1".into(),
                node_ids: vec!["root_vlt_1_1".into()],
                actor: None,
            })
            .unwrap_err(),
            "Can't delete the vault root"
        );

        let before = db.node("root_vlt_1_1").unwrap().size_bytes;
        let folder = db.node("n_1_1_brand").unwrap();
        let (changes, _) = db
            .delete_nodes(DeleteNodesInput {
                vault_id: "vlt_1_1".into(),
                node_ids: vec![folder.id.clone()],
                actor: None,
            })
            .unwrap();
        assert!(matches!(changes[0], FsChange::Remove { .. }));
        assert_eq!(db.node("root_vlt_1_1").unwrap().size_bytes, before - folder.size_bytes);
        assert!(db.node(&folder.id).is_err());
    }

    #[test]
    fn color_is_folders_only() {
        let mut db = seeded();
        assert_eq!(
            db.set_node_color(SetNodeColorInput {
                vault_id: "vlt_1_1".into(),
                node_id: "n_1_1_poster".into(),
                color: None,
                actor: None,
            })
            .unwrap_err(),
            "Only folders have colors"
        );
        let (node, _) = db
            .set_node_color(SetNodeColorInput {
                vault_id: "vlt_1_1".into(),
                node_id: "n_1_1_brand".into(),
                color: Some(FolderColor::Teal),
                actor: None,
            })
            .unwrap();
        assert!(matches!(node.color, Some(FolderColor::Teal)));
        assert_eq!(db.get_history("vlt_1_1", "n_1_1_brand")[0].summary, "set color to teal");
    }

    #[test]
    fn downloads_tick_to_local() {
        let mut db = seeded();
        let remote = db
            .list_tree("vlt_1_1")
            .into_iter()
            .find(|n| n.availability == Availability::Remote)
            .unwrap();
        let (_, duration) = db.begin_download(&remote.id).unwrap().unwrap();
        assert!((1200.0..=4200.0).contains(&duration));
        assert!(db.begin_download(&remote.id).unwrap().is_none(), "already in flight");

        let step = 100.0 / duration;
        let mut done = false;
        for _ in 0..100 {
            match db.tick_download(&remote.id, step) {
                Some((_, finished)) => {
                    done = finished;
                    if finished {
                        break;
                    }
                }
                None => break,
            }
        }
        assert!(done);
        let after = db.node(&remote.id).unwrap();
        assert_eq!(after.availability, Availability::Local);
        assert!(after.holders.iter().any(|h| h == "peer_aaryaman"));
        assert_eq!(db.get_history("vlt_1_1", &remote.id)[0].summary, "downloaded to this Mac");
    }

    #[test]
    fn access_inherits_and_strips_the_vault_creator() {
        let mut db = seeded();
        let (access, changes) = db
            .set_access(SetAccessInput {
                vault_id: "vlt_1_1".into(),
                node_id: "n_1_1_film".into(),
                inherit: false,
                entries: vec![
                    AccessEntry {
                        peer_id: "peer_aaryaman".into(),
                        level: crate::fs_types::AccessLevel::Editor,
                    },
                    AccessEntry {
                        peer_id: "peer_maya".into(),
                        level: crate::fs_types::AccessLevel::Viewer,
                    },
                ],
            })
            .unwrap();
        assert_eq!(access.entries.len(), 1, "the vault creator is stripped");
        assert!(!access.inherit);
        assert_eq!(changes.len(), 1);
        assert_eq!(
            db.get_history("vlt_1_1", "n_1_1_film")[0].summary,
            "changed access for 1 member"
        );

        let child = db
            .child_ids("n_1_1_film")
            .into_iter()
            .next()
            .expect("Film has children");
        let inherited = db.get_access(&child).unwrap();
        assert!(inherited.inherit);
        assert_eq!(inherited.entries.len(), 1);
    }

    #[test]
    fn membership_rules_hold() {
        let mut db = seeded();
        assert_eq!(
            db.remove_member("vlt_1_1", "peer_aaryaman").unwrap_err(),
            "You can't remove yourself"
        );
        assert_eq!(
            db.remove_member("vlt_1_1", "peer_nobody").unwrap_err(),
            "That member is not in this vault"
        );
        assert_eq!(
            db.set_member_role("vlt_1_1", "peer_aaryaman", MemberRole::Member)
                .unwrap_err(),
            "A vault needs at least one admin"
        );
        assert_eq!(db.leave_vault("vlt_1_1").unwrap_err(), "Make someone else an admin first");
        assert_eq!(db.remove_member("vlt_1_1", "peer_justin").unwrap(), 4);
        assert!(db.delete_vault("vlt_1_1").is_ok());
        assert!(db.list_tree("vlt_1_1").is_empty());
        assert!(db.get_vault_meta("vlt_1_1").is_err());
    }

    #[test]
    fn vault_settings_fan_out_to_the_root_folder() {
        let mut db = seeded();
        let servers = vec!["srv_1".to_string(), "srv_2".to_string()];
        let update = db
            .update_vault_meta(
                "vlt_1_1",
                VaultMetaPatch {
                    name: Some("  Studio  ".into()),
                    server_id: Some("srv_2".into()),
                    cleanup_threshold_pct: Some(140),
                    ..VaultMetaPatch::default()
                },
                &servers,
            )
            .unwrap();
        assert_eq!(update.meta.name, "Studio");
        assert_eq!(update.meta.cleanup_threshold_pct, 100);
        assert!(update.renamed && update.rehomed);
        assert_eq!(update.changes.len(), 1, "the root folder was renamed");
        assert_eq!(db.node("root_vlt_1_1").unwrap().name, "Studio");

        assert_eq!(
            db.update_vault_meta(
                "vlt_1_1",
                VaultMetaPatch {
                    name: Some("".into()),
                    ..VaultMetaPatch::default()
                },
                &servers,
            )
            .unwrap_err(),
            "Invalid vault name"
        );
        assert_eq!(
            db.update_vault_meta(
                "vlt_1_1",
                VaultMetaPatch {
                    server_id: Some("srv_9".into()),
                    ..VaultMetaPatch::default()
                },
                &servers,
            )
            .unwrap_err(),
            "Unknown server: srv_9"
        );
    }

    #[test]
    fn join_codes_are_six_base32_characters() {
        let mut db = seeded();
        let code = db.rotate_join_code("vlt_1_1").unwrap();
        assert!(code.bytes().all(|b| JOIN_CODE_ALPHABET.contains(&b)));
        assert_eq!(db.get_vault_meta("vlt_1_1").unwrap().join_code, code);
        // Bit-for-bit with the mock: `makeRandom(seed.generatedAt)` in engine.ts draws these.
        assert_eq!(code, "GSALF2");
        assert_eq!(db.rotate_join_code("vlt_1_2").unwrap(), "444ITU");
    }

    #[test]
    fn recents_stay_capped_and_newest_first() {
        let mut db = seeded();
        db.touch_recent("vlt_1_1", "n_1_1_poster");
        let recents = db.list_recents();
        assert_eq!(recents[0].node.id, "n_1_1_poster");
        assert_eq!(recents[0].vault_name, "Design Assets");
        for i in 0..12 {
            let id = db
                .list_tree("vlt_1_2")
                .get(i)
                .map(|n| n.id.clone())
                .unwrap_or_default();
            db.touch_recent("vlt_1_2", &id);
        }
        assert!(db.list_recents().len() <= MAX_RECENTS);
    }
}
