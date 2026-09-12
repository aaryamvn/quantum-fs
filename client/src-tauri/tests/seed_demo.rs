#![cfg(unix)]
//! Headless demo seeder: creates vaults on already-running `qfsd` servers and fills them with
//! realistic folders and files through the same [`Node`] the app embeds.
//!
//! Inert unless `QFS_SEED_SERVERS` is set (comma-separated connect strings, `ADDRESS/TOKEN`), so a
//! plain `cargo test` never touches a live server. The setup mirrors `tests/e2e_node.rs`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;

use quantamfs_lib::fs_types::{Availability, CreateNodeInput, CreateVaultInput, FsNode, NodeKind};
use quantamfs_lib::node::Node;

type Outcome = Result<(), Box<dyn std::error::Error>>;

/// The directory the demo servers register with.
const DIRECTORY_ADDR: &str = "172.26.28.115:7440";
/// 1 GiB per vault: small enough that the host's default 32 GiB capacity gate passes.
const QUOTA_BYTES: u64 = 1_073_741_824;
const SEED_ROOT: &str = "/tmp/qfs-demo";

/* ----------------------------------------------------------------- specs */

struct FileSpec {
    /// `None` = the vault root.
    folder: Option<&'static str>,
    name: &'static str,
    body: String,
}

struct VaultSpec {
    name: &'static str,
    folders: Vec<&'static str>,
    files: Vec<FileSpec>,
}

fn file(folder: Option<&'static str>, name: &'static str, body: &str) -> FileSpec {
    FileSpec {
        folder,
        name,
        body: body.to_string(),
    }
}

/* ------------------------------------------------------------------ test */

#[test]
fn seed_demo() -> Outcome {
    let Ok(servers) = std::env::var("QFS_SEED_SERVERS") else {
        return Ok(());
    };
    let connects: Vec<String> = servers
        .split(',')
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect();
    if connects.is_empty() {
        return Ok(());
    }
    if std::env::var("QFS_DIRECTORY_ADDR").is_err() {
        std::env::set_var("QFS_DIRECTORY_ADDR", DIRECTORY_ADDR);
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(seed(connects))
}

async fn seed(connects: Vec<String>) -> Outcome {
    let data_dir = PathBuf::from(SEED_ROOT).join("seeder");
    let _ = std::fs::remove_dir_all(&data_dir);
    std::fs::create_dir_all(&data_dir)?;
    let source_root = PathBuf::from(SEED_ROOT).join("seed-src");
    let _ = std::fs::remove_dir_all(&source_root);
    std::fs::create_dir_all(&source_root)?;

    let emit: Arc<dyn Fn(&str, Value) + Send + Sync> = Arc::new(|_name: &str, _payload: Value| {});
    let node = Node::start(data_dir, emit);

    for (index, connect) in connects.iter().enumerate() {
        let server_name = format!("Server {}", (b'A' + index as u8) as char);
        let server = match node.add_server(server_name.clone(), connect.clone()).await {
            Ok(server) => server,
            Err(error) => {
                println!("SEED ERROR {server_name} - add_server: {error}");
                continue;
            }
        };
        for spec in vault_specs(index) {
            match seed_vault(&node, &server.id, &source_root, &spec).await {
                Ok(line) => println!("VAULT {server_name} | {line}"),
                Err((step, error)) => {
                    println!("SEED ERROR {server_name} {} {step}: {error}", spec.name)
                }
            }
        }
    }
    println!("SEED DONE");
    Ok(())
}

/// One vault, end to end. `Err((step, message))` names the step that failed so the caller can
/// report it and carry on with the next vault.
async fn seed_vault(
    node: &Node,
    server_id: &str,
    source_root: &PathBuf,
    spec: &VaultSpec,
) -> Result<String, (String, String)> {
    let vault = node
        .create_vault(CreateVaultInput {
            server_id: server_id.to_string(),
            name: spec.name.to_string(),
            quota_bytes: QUOTA_BYTES,
        })
        .await
        .map_err(|error| ("create_vault".to_string(), error))?;
    let vault_id = vault.id.clone();
    let root_id = format!("root_{vault_id}");

    wait_for(Duration::from_secs(30), || async {
        tree(node, &vault_id)
            .await
            .iter()
            .any(|item| item.id == root_id && item.parent_id.is_none())
    })
    .await
    .map_err(|error| ("hydrate".to_string(), error))?;

    // Folders first: every file needs its parent id.
    let mut parents: Vec<(&'static str, String)> = Vec::new();
    for folder in &spec.folders {
        let made = node
            .create_node(CreateNodeInput {
                vault_id: vault_id.clone(),
                parent_id: root_id.clone(),
                kind: NodeKind::Folder,
                name: folder.to_string(),
                actor: None,
            })
            .await
            .map_err(|error| (format!("create_node({folder})"), error))?;
        parents.push((folder, made.id));
    }

    // Write the bytes to disk, then import them folder by folder.
    let vault_dir = source_root.join(spec.name);
    let mut bytes = 0u64;
    let mut batches: Vec<(String, Vec<PathBuf>)> = vec![(root_id.clone(), Vec::new())];
    for (_, id) in &parents {
        batches.push((id.clone(), Vec::new()));
    }
    for entry in &spec.files {
        let directory = match entry.folder {
            Some(folder) => vault_dir.join(folder),
            None => vault_dir.clone(),
        };
        std::fs::create_dir_all(&directory)
            .map_err(|error| ("write_source".to_string(), error.to_string()))?;
        let path = directory.join(entry.name);
        std::fs::write(&path, entry.body.as_bytes())
            .map_err(|error| ("write_source".to_string(), error.to_string()))?;
        bytes += entry.body.len() as u64;
        let parent_id = match entry.folder {
            None => root_id.clone(),
            Some(folder) => parents
                .iter()
                .find(|(name, _)| *name == folder)
                .map(|(_, id)| id.clone())
                .ok_or_else(|| ("import".to_string(), format!("no folder {folder}")))?,
        };
        if let Some(batch) = batches.iter_mut().find(|(id, _)| *id == parent_id) {
            batch.1.push(path);
        }
    }
    for (parent_id, paths) in batches {
        if paths.is_empty() {
            continue;
        }
        node.import_files(vault_id.clone(), parent_id, paths)
            .await
            .map_err(|error| ("import_files".to_string(), error))?;
    }

    let want_files = spec.files.len();
    let want_nodes = 1 + spec.folders.len() + want_files;
    wait_for(Duration::from_secs(60), || async {
        let items = tree(node, &vault_id).await;
        let files: Vec<&FsNode> = items
            .iter()
            .filter(|item| item.kind == NodeKind::File)
            .collect();
        items.len() >= want_nodes
            && files.len() >= want_files
            && files
                .iter()
                .all(|item| item.availability == Availability::Local)
    })
    .await
    .map_err(|error| ("settle".to_string(), error))?;

    let meta = node
        .get_vault_meta(vault_id.clone())
        .await
        .map_err(|error| ("get_vault_meta".to_string(), error))?;
    Ok(format!(
        "{} | code {} | {} folders {want_files} files {bytes} bytes",
        spec.name,
        meta.join_code,
        spec.folders.len()
    ))
}

async fn tree(node: &Node, vault_id: &str) -> Vec<FsNode> {
    node.list_tree(vault_id.to_string())
        .await
        .unwrap_or_default()
}

/// Poll until `test` is true or `timeout` elapses. Never panics: a timeout is an error the
/// caller reports and moves past.
async fn wait_for<F, Fut>(timeout: Duration, mut test: F) -> Result<Duration, String>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let started = Instant::now();
    loop {
        if test().await {
            return Ok(started.elapsed());
        }
        if started.elapsed() >= timeout {
            return Err(format!("timed out after {timeout:?}"));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/* ------------------------------------------------------------- generators */

/// Deterministic pseudo-random stream, so every run produces byte-identical demo data.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 11
    }

    fn range(&mut self, low: u64, high: u64) -> u64 {
        low + self.next() % (high - low + 1)
    }
}

fn interviews_csv() -> String {
    let personas = [
        "Studio lead",
        "Solo designer",
        "IT admin",
        "Research scientist",
        "Producer",
        "Founder",
        "Archivist",
        "Video editor",
    ];
    let quotes = [
        "I stopped trusting the shared drive after the second silent overwrite.",
        "If it needs a login page before I can open a file, my team will not use it.",
        "The part I care about is seeing who else is in the folder right now.",
        "We move 40 GB of raw footage a week and the upload is the bottleneck.",
        "Security asked us for post-quantum answers and we had none.",
        "Every sync tool I have used lies about what finished uploading.",
        "I want the folder to look like a folder, not like a web app.",
        "Our compliance team needs the audit trail more than the storage.",
        "Onboarding a freelancer takes a day of IT tickets. That is the whole problem.",
        "Offline access is not a feature for us, it is the default working condition.",
    ];
    let mut out = String::from("id,date,persona,quote,score\n");
    let mut rng = Rng(0x5EED_0001);
    for row in 1..=200u32 {
        let day = 1 + (row % 28);
        let month = 1 + (row / 28) % 9;
        let persona = personas[(row as usize) % personas.len()];
        let quote = quotes[(rng.next() as usize) % quotes.len()];
        let score = rng.range(3, 10);
        out.push_str(&format!(
            "INT-{row:03},2026-{month:02}-{day:02},{persona},\"{quote}\",{score}\n"
        ));
    }
    out
}

fn budget_csv() -> String {
    let rows = [
        ("Engineering", "Salaries"),
        ("Engineering", "Cloud compute"),
        ("Engineering", "Tooling licences"),
        ("Engineering", "Hardware refresh"),
        ("Engineering", "Contractors"),
        ("Design", "Salaries"),
        ("Design", "Software licences"),
        ("Design", "User research incentives"),
        ("Design", "Print and production"),
        ("Research", "Salaries"),
        ("Research", "Lab equipment"),
        ("Research", "Conference travel"),
        ("Research", "Publication fees"),
        ("Sales", "Salaries"),
        ("Sales", "Commissions"),
        ("Sales", "Travel and events"),
        ("Sales", "CRM licences"),
        ("Marketing", "Paid acquisition"),
        ("Marketing", "Content production"),
        ("Marketing", "Brand campaign"),
        ("Operations", "Office lease"),
        ("Operations", "Insurance"),
        ("Operations", "Legal counsel"),
        ("Operations", "Accounting"),
        ("Operations", "Recruiting fees"),
        ("Support", "Salaries"),
        ("Support", "Helpdesk tooling"),
        ("Support", "On-call stipends"),
        ("Security", "Penetration testing"),
        ("Security", "Audit and certification"),
    ];
    let mut out = String::from("department,line,planned,actual\n");
    let mut rng = Rng(0x5EED_0002);
    for (department, line) in rows.iter() {
        for month in ["Jul", "Aug"] {
            let planned = rng.range(8, 240) * 1_000;
            let drift = rng.range(0, 24) as i64 - 12;
            let actual = (planned as i64 + planned as i64 * drift / 100).max(0);
            out.push_str(&format!("{department},{line} ({month}),{planned},{actual}\n"));
        }
    }
    out
}

fn forecast_csv() -> String {
    let mut out = String::from("month,new_arr,expansion_arr,churn_arr,net_arr\n");
    let mut rng = Rng(0x5EED_0003);
    let mut base = 412_000i64;
    for index in 0..36u32 {
        let year = 2026 + index / 12;
        let month = 1 + index % 12;
        let new = base + rng.range(0, 60_000) as i64;
        let expansion = new / 4 + rng.range(0, 12_000) as i64;
        let churn = new / 9 + rng.range(0, 7_000) as i64;
        out.push_str(&format!(
            "{year}-{month:02},{new},{expansion},{churn},{}\n",
            new + expansion - churn
        ));
        base += 9_500;
    }
    out
}

fn benchmark_csv() -> String {
    let algorithms = [
        ("ML-KEM-512", 9u64, 1_632u64),
        ("ML-KEM-768", 13, 2_400),
        ("ML-KEM-1024", 19, 3_168),
        ("X25519", 27, 32),
        ("X-Wing-X25519-ML-KEM-768", 34, 2_432),
        ("ML-DSA-44", 62, 2_560),
        ("ML-DSA-65", 94, 4_032),
        ("Ed25519", 21, 64),
        ("RSA-3072", 41_000, 384),
        ("Falcon-512", 3_900, 1_281),
    ];
    let mut out = String::from("run,algorithm,keygen_us,encaps_us,decaps_us,bytes\n");
    let mut rng = Rng(0x5EED_0004);
    for run in 1..=500u32 {
        let (name, base, bytes) = algorithms[(run as usize) % algorithms.len()];
        let keygen = base + rng.range(0, base.max(5) / 5);
        let encaps = base * 3 / 4 + 2 + rng.range(0, base.max(5) / 6);
        let decaps = base * 9 / 10 + 2 + rng.range(0, base.max(5) / 6);
        out.push_str(&format!("{run},{name},{keygen},{encaps},{decaps},{bytes}\n"));
    }
    out
}

/// ~30 000 rows of plausible lab telemetry. Deliberately large: it is the file that proves a
/// multi-chunk transfer in the demo.
fn sensor_csv() -> String {
    let mut out = String::with_capacity(1_700_000);
    out.push_str("timestamp,sensor_id,temp_c,humidity,voltage\n");
    let mut rng = Rng(0x5EED_0005);
    let mut stamp = 1_772_000_000u64;
    for index in 0..30_000u32 {
        let sensor = 1 + index % 6;
        let wave = ((index % 720) as f64) / 720.0 * 6.283_185_307;
        let temp = 20.5 + 3.4 * wave.sin() + (rng.range(0, 40) as f64) / 100.0;
        let humidity = 41.0 + 6.0 * wave.cos() + (rng.range(0, 60) as f64) / 100.0;
        let voltage = 3.30 - (rng.range(0, 45) as f64) / 1000.0;
        out.push_str(&format!(
            "{stamp},SENS-{sensor:02},{temp:.2},{humidity:.2},{voltage:.3}\n"
        ));
        stamp += 2;
    }
    out
}

/* ------------------------------------------------------------ vault specs */

fn vault_specs(server_index: usize) -> Vec<VaultSpec> {
    match server_index {
        0 => vec![design_team(), engineering()],
        1 => vec![finance(), research()],
        _ => Vec::new(),
    }
}

fn design_team() -> VaultSpec {
    VaultSpec {
        name: "Design Team",
        folders: vec!["Brand", "Mockups", "Research"],
        files: vec![
            file(Some("Brand"), "brand-guidelines.md", BRAND_GUIDELINES),
            file(Some("Brand"), "color-palette.json", COLOR_PALETTE),
            file(Some("Mockups"), "homepage-v3-notes.md", HOMEPAGE_NOTES),
            file(Some("Mockups"), "mobile-onboarding-flow.md", ONBOARDING_FLOW),
            file(
                Some("Research"),
                "user-interviews.csv",
                &interviews_csv(),
            ),
            file(Some("Research"), "survey-summary.md", SURVEY_SUMMARY),
            file(None, "launch-checklist.md", LAUNCH_CHECKLIST),
        ],
    }
}

fn engineering() -> VaultSpec {
    VaultSpec {
        name: "Engineering",
        folders: vec!["src", "docs", "ops"],
        files: vec![
            file(Some("src"), "main.py", MAIN_PY),
            file(Some("src"), "sync_engine.py", SYNC_ENGINE_PY),
            file(Some("src"), "chunker.py", CHUNKER_PY),
            file(Some("docs"), "ARCHITECTURE.md", ARCHITECTURE_MD),
            file(Some("docs"), "API.md", API_MD),
            file(Some("ops"), "runbook.md", RUNBOOK_MD),
            file(Some("ops"), "deploy.sh", DEPLOY_SH),
            file(Some("ops"), "config.yaml", CONFIG_YAML),
            file(None, "README.md", README_MD),
        ],
    }
}

fn finance() -> VaultSpec {
    VaultSpec {
        name: "Finance",
        folders: vec!["Q3-2026", "Invoices", "Policies"],
        files: vec![
            file(Some("Q3-2026"), "q3-budget.csv", &budget_csv()),
            file(Some("Q3-2026"), "revenue-forecast.csv", &forecast_csv()),
            file(Some("Q3-2026"), "board-summary.md", BOARD_SUMMARY),
            file(Some("Invoices"), "invoice-2026-091.txt", INVOICE_091),
            file(Some("Invoices"), "invoice-2026-092.txt", INVOICE_092),
            file(Some("Policies"), "expense-policy.md", EXPENSE_POLICY),
            file(None, "finance-overview.md", FINANCE_OVERVIEW),
        ],
    }
}

fn research() -> VaultSpec {
    VaultSpec {
        name: "Research",
        folders: vec!["Papers", "Notes", "Data"],
        files: vec![
            file(Some("Papers"), "pq-crypto-survey.md", PQ_SURVEY),
            file(Some("Papers"), "ml-kem-vs-rsa.md", ML_KEM_VS_RSA),
            file(Some("Notes"), "lab-notebook.md", LAB_NOTEBOOK),
            file(Some("Notes"), "weekly-sync.md", WEEKLY_SYNC),
            file(Some("Data"), "benchmark-results.csv", &benchmark_csv()),
            file(Some("Data"), "sensor-readings.csv", &sensor_csv()),
            file(None, "research-roadmap.md", RESEARCH_ROADMAP),
        ],
    }
}

/* ------------------------------------------------------------- Design Team */

const BRAND_GUIDELINES: &str = r##"# Quantum Labs - Brand Guidelines
Version 4.2 - owned by the Design Team - last revised 4 August 2026

## 1. Who we sound like
Quantum Labs writes the way a good engineer explains something to a colleague who is
smart but busy. Plain sentences. Concrete nouns. No hedging.

- Say "your files stay on your machine", not "leveraging edge-first persistence".
- Name the mechanism. "Encrypted with ML-KEM-768" beats "military-grade security".
- Never promise what the product does not do yet. Roadmap language belongs on the roadmap.
- Second person for the reader, first person plural for us. Avoid "users" in product copy.

Words we use: vault, host, peer, replica, join code, local-first.
Words we avoid: cloud (unless literal), synergy, seamless, revolutionary, blazing fast.

## 2. Logo
The mark is the split-orbit Q. It is one piece of artwork; do not rebuild it from the
typeface.

- Clear space: one full Q-height on every side. Nothing crosses it, including page edges.
- Minimum size: 24 px tall on screen, 9 mm in print.
- Colour: Ink on light surfaces, Paper on dark surfaces, single-colour only. The two-tone
  version exists for the app icon and nowhere else.
- Never: rotate it, add a drop shadow, outline it, place it on a photograph without the
  Ink scrim, stretch it, or animate the orbit faster than 1.2 s per revolution.
- The wordmark "Quantum Labs" is set in GT Walsheim Medium with -1.5% tracking. It is
  locked artwork. Do not retype it.

## 3. Colour
Core palette (tokens live in color-palette.json, which is the source of truth):

- Ink - near-black, every body text on light surfaces.
- Paper - warm off-white, the default app background.
- Quantum Violet - the primary action colour. One per screen, never two.
- Signal Teal - success, presence, "this peer is online".
- Amber Flag - warning and quota pressure.
- Coral Alert - destructive actions and failed transfers only.
- Slate 300/500/700 - borders, secondary text, disabled states.

Rules: Quantum Violet never sits on Signal Teal. Coral Alert is never decorative.
Body text must hold 4.5:1 against its background; large display text may drop to 3:1.

## 4. Typography
- Display and headings: GT Walsheim (Medium for H1-H2, Regular for H3).
- UI, body and tables: Inter, with -0.011em tracking at 14 px and below.
- Numerals in tables and file sizes: Inter with tabular figures enabled.
- Code, hashes and join codes: Berkeley Mono, letter-spaced 0.04em, always uppercase for
  join codes.
- Scale: 40 / 32 / 24 / 18 / 16 / 14 / 13 / 11. Nothing in between, nothing below 11.
- Line length caps at 72 characters for prose.

## 5. Product surfaces
File and folder icons are the 3D isometric set at 2x; they carry the folder colour on the
front face only. Presence dots are 8 px, Signal Teal for online, Slate 500 for away.
Empty states get one sentence and one action, never an illustration alone.

## 6. Approvals
Anything with the logo on it ships past design review. Post the artboard link in
#brand-review and tag the design lead. Turnaround is one working day.
"##;

const COLOR_PALETTE: &str = r##"{
  "$schema": "https://quantumlabs.dev/schemas/tokens-1.json",
  "name": "Quantum Labs Core",
  "version": "4.2.0",
  "updated": "2026-08-04",
  "color": {
    "ink": { "value": "#101014", "type": "color", "use": "body text on light" },
    "paper": { "value": "#FBFAF7", "type": "color", "use": "app background" },
    "quantum-violet-600": { "value": "#5B3DF5", "type": "color", "use": "primary action" },
    "quantum-violet-500": { "value": "#7358F7", "type": "color", "use": "hover" },
    "quantum-violet-100": { "value": "#E7E2FE", "type": "color", "use": "selected row" },
    "signal-teal-600": { "value": "#0E9E8F", "type": "color", "use": "success, presence" },
    "signal-teal-100": { "value": "#D6F2EE", "type": "color", "use": "success surface" },
    "amber-flag-600": { "value": "#C97A08", "type": "color", "use": "quota warning" },
    "amber-flag-100": { "value": "#FBEBD2", "type": "color", "use": "warning surface" },
    "coral-alert-600": { "value": "#D9483B", "type": "color", "use": "destructive" },
    "coral-alert-100": { "value": "#FBE1DE", "type": "color", "use": "error surface" },
    "slate-700": { "value": "#3A3F46", "type": "color", "use": "secondary text" },
    "slate-500": { "value": "#6C737D", "type": "color", "use": "muted text" },
    "slate-300": { "value": "#C7CCD3", "type": "color", "use": "borders" },
    "slate-100": { "value": "#EEF0F3", "type": "color", "use": "hairlines" }
  },
  "folder": {
    "graphite": { "value": "#5A6270" },
    "coral": { "value": "#E86A5C" },
    "amber": { "value": "#E3A13A" },
    "moss": { "value": "#5C8F52" },
    "teal": { "value": "#2FA79A" },
    "violet": { "value": "#7358F7" },
    "plum": { "value": "#9B5AA8" }
  },
  "radius": { "sm": "6px", "md": "10px", "lg": "16px", "pill": "999px" },
  "shadow": {
    "raise": "0 1px 2px rgba(16,16,20,0.08), 0 8px 24px rgba(16,16,20,0.06)",
    "modal": "0 24px 64px rgba(16,16,20,0.22)"
  }
}
"##;

const HOMEPAGE_NOTES: &str = r##"# Homepage v3 - review notes
Mockup: Homepage v3 (artboards 01-07) - review held 11 August 2026
Present: design lead, two engineers, head of growth

## What changed from v2
- The hero now shows the app window with a real vault open instead of an abstract render.
  Three peer avatars sit in the toolbar with live presence dots.
- Headline: "Your files. Your machines. Encrypted for the next thirty years."
  Subhead names the mechanism in one line: local-first replicas, post-quantum key exchange.
- The pricing table moved below the fold. Nobody in the five-second test read it in v2.
- Added the "join with a six-character code" strip, because it is the single moment in the
  demo that makes people lean forward.

## Decisions
1. Hero screenshot is a real capture at 2x, not a Figma mock. Approved.
2. The animated orbit runs once on load and then rests. No loop. Approved.
3. Drop the logo wall. We have four named customers and a wall of six looks thin.
4. The security section gets three cards, not a table: key exchange, signatures, transport.
   Copy comes from docs/ARCHITECTURE.md so the claims stay accurate.
5. Growth wants an email capture in the hero. Design pushed back; compromise is a sticky
   footer bar that appears after 40% scroll.

## Open questions
- Does the hero screenshot need a light and dark variant? Engineering says the app already
  themes; capture both and decide at implementation.
- The mobile hero currently stacks to 780 px tall. Needs a shorter crop of the window.
- Waiting on legal for the wording of "post-quantum" in the headline.

## Next
Homepage v3.1 with the footer bar and the dark hero by Thursday. Handoff to web the
following Monday, with the video export for the launch tweet.
"##;

const ONBOARDING_FLOW: &str = r##"# Mobile onboarding - flow notes
Screens 01-09, portrait, 393 x 852. Prototype link lives in the Mockups board.

## The one thing
A new person must reach a file they can open in under 60 seconds, without creating an
account. The join code is the whole onboarding.

## Flow
01 Welcome - one line of copy, one primary button ("I have a join code"), one text link
   ("Set up a new vault"). No carousel. No sign-up.
02 Code entry - six large monospaced cells, auto-advance, paste fills all six. Uppercase
   is forced. Backspace steps back a cell.
03 Resolving - the orbit mark spins while the directory is queried. Copy: "Finding the
   host for QX7K2M". Times out at 8 s into screen 04.
04 Cannot find it - three named causes (typo, host offline, code rotated) and a retry.
   Never a raw error string.
05 Name yourself - display name and an avatar colour. Prefilled from the device name.
   This is the only data we ask for and it never leaves the vault.
06 Hydrating - the tree fills in live, folder by folder, with a progress line. This is the
   moment that sells the product; do not replace it with a spinner.
07 Vault - the file list, with the first folder already expanded.
08 First file - tapping a remote file shows the pull progress inline on the row.
09 Done nudge - a one-time tip pointing at the presence row: "Maya is in this vault now."

## Interaction rules
- Nothing blocks on the network for more than 8 seconds without an explanation.
- Remote files are never hidden. They show with a cloud-arrow and pull on tap.
- Back from screen 06 leaves the vault properly; it does not strand a half-joined replica.
- Haptic tick on each code cell, one success haptic at screen 07. Nothing else.

## Accessibility
Code cells expose a single combined text field to VoiceOver. Presence dots carry labels,
not colour alone. Minimum target 44 x 44. Reduced motion drops the hydration animation to
a plain progress bar.
"##;

const SURVEY_SUMMARY: &str = r##"# Q3 user survey - summary
Fielded 14-28 July 2026. 412 responses from the beta list, 200 of them followed up with
an interview (see user-interviews.csv). Margin of error roughly 4.6%.

## Headline numbers
- 68% call file sharing their most painful weekly task.
- 74% have lost work to a sync conflict or a silent overwrite at least once.
- 31% currently pay for two or more storage products at the same time.
- 52% cannot name the encryption their current tool uses. Of those who can, 9% mention
  anything post-quantum.
- Net satisfaction with the current tool: -12. Nobody is happy; everyone is resigned.

## What people ask for, in order
1. "Show me who else is in this folder right now." (61%)
2. Files that work offline and reconcile without a merge dialog. (57%)
3. Sharing with someone outside the company without an account for them. (49%)
4. A real audit trail of who changed what. (38%)
5. Storage they host themselves. (33%, but 71% among regulated industries)

## Segments worth noting
- Studios and agencies: largest files, least tolerance for upload waits. They churn on
  speed, not on price.
- Regulated (health, legal, defence adjacent): they will accept a slower product for a
  self-hosted one with signatures they can inspect.
- Solo designers: price sensitive, but they are the ones who evangelise. The free tier
  should never feel crippled.

## What this changes
- The presence row moves from a nice-to-have to a launch requirement.
- "No account needed to join" becomes the headline claim on the pricing page.
- Post-quantum is not a demand-generator on its own; it is a trust closer late in the
  evaluation. Keep it in the security section, out of the hero subhead.
"##;

const LAUNCH_CHECKLIST: &str = r##"# Launch checklist - Quantum Labs 1.0
Target: 29 September 2026, 09:00 PT. Owner: design lead. Update this file, not Slack.

## Brand and assets
- [x] Logo lockups exported (SVG, PNG 1x/2x/3x, app icon set)
- [x] color-palette.json v4.2 published to the token pipeline
- [x] GT Walsheim and Inter licences cleared for web embedding
- [ ] Social cards for the five launch posts
- [ ] Press kit zip (logo, three screenshots, one-paragraph description)

## Product surfaces
- [x] Empty states written for vault, folder and search
- [x] Error copy for offline host, rotated code, quota exceeded
- [ ] Dark theme pass on the join-code modal
- [ ] 3D folder icon set re-exported at 3x for the retina capture
- [ ] Onboarding tip copy final (waiting on screen 09)

## Marketing site
- [x] Homepage v3 approved
- [ ] Homepage v3.1 with the sticky footer bar
- [ ] Security section copy checked against docs/ARCHITECTURE.md
- [ ] Pricing page numbers confirmed with Finance
- [ ] Accessibility audit on the marketing site (contrast, focus order, reduced motion)

## Demo
- [ ] Two hosts seeded with the demo vaults
- [ ] Screen recording at 2560 x 1440, 60 fps, no notifications
- [ ] Backup recording in case the venue network is hostile

## Sign-off
- [ ] Design lead
- [ ] Engineering lead
- [ ] Legal on the "post-quantum" wording
- [ ] Founder
"##;

/* ------------------------------------------------------------- Engineering */

const MAIN_PY: &str = r##"#!/usr/bin/env python3
"""qfs-chunk - split a file into content-defined chunks and print a manifest.

Used by the import path's reference implementation and by the ops team when a transfer
needs to be explained to a customer. Pure standard library on purpose: it runs on a
locked-down laptop with no wheels installed.
"""

import argparse
import hashlib
import json
import os
import sys
from pathlib import Path

from chunker import Chunker, DEFAULT_AVG, DEFAULT_MAX, DEFAULT_MIN


def build_manifest(path: Path, avg: int, low: int, high: int) -> dict:
    """Chunk `path` and return the manifest the host stores next to the file."""
    chunker = Chunker(avg=avg, minimum=low, maximum=high)
    digest = hashlib.blake2b(digest_size=32)
    chunks = []
    offset = 0
    with path.open("rb") as handle:
        for piece in chunker.split(handle):
            chunk_id = hashlib.blake2b(piece, digest_size=32).hexdigest()
            digest.update(piece)
            chunks.append({"id": chunk_id, "offset": offset, "len": len(piece)})
            offset += len(piece)
    return {
        "name": path.name,
        "size": offset,
        "file_id": digest.hexdigest(),
        "chunk_count": len(chunks),
        "chunks": chunks,
    }


def human(count: int) -> str:
    for unit in ("B", "KiB", "MiB", "GiB"):
        if count < 1024 or unit == "GiB":
            return f"{count:.1f} {unit}" if unit != "B" else f"{count} B"
        count /= 1024.0
    return f"{count} B"


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(prog="qfs-chunk", description=__doc__)
    parser.add_argument("path", type=Path, help="file to chunk")
    parser.add_argument("--avg", type=int, default=DEFAULT_AVG,
                        help="target average chunk size in bytes")
    parser.add_argument("--min", dest="low", type=int, default=DEFAULT_MIN)
    parser.add_argument("--max", dest="high", type=int, default=DEFAULT_MAX)
    parser.add_argument("--json", action="store_true", help="emit the manifest as JSON")
    parser.add_argument("--out", type=Path, help="write the manifest here instead of stdout")
    args = parser.parse_args(argv)

    if not args.path.is_file():
        print(f"qfs-chunk: {args.path} is not a file", file=sys.stderr)
        return 2
    if not args.low <= args.avg <= args.high:
        print("qfs-chunk: expected min <= avg <= max", file=sys.stderr)
        return 2

    manifest = build_manifest(args.path, args.avg, args.low, args.high)

    if args.json or args.out:
        text = json.dumps(manifest, indent=2)
        if args.out:
            args.out.write_text(text + os.linesep)
        else:
            print(text)
        return 0

    print(f"{manifest['name']}  {human(manifest['size'])}")
    print(f"file id  {manifest['file_id']}")
    print(f"chunks   {manifest['chunk_count']}")
    sizes = [chunk["len"] for chunk in manifest["chunks"]]
    if sizes:
        print(f"smallest {human(min(sizes))}   largest {human(max(sizes))}"
              f"   mean {human(sum(sizes) // len(sizes))}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
"##;

const SYNC_ENGINE_PY: &str = r##""""Reference model of the replica loop, the heartbeat and the host control log.

This is the executable description the Rust runtime is checked against. It is deliberately
single-threaded and in-memory: the point is the ordering rules, not the I/O.
"""

import time
from dataclasses import dataclass, field
from typing import Dict, Iterable, List, Optional

HEARTBEAT_MS = 100
PULL_BATCH = 32
LEASE_MS = 3_000


class ControlLogError(Exception):
    """Raised when an entry cannot be appended in the position it claims."""


@dataclass(frozen=True)
class Entry:
    """One appended fact. The host assigns `seq`; nobody else may."""
    seq: int
    actor: str
    kind: str
    node_id: str
    payload: dict
    at_ms: int

    def summary(self) -> str:
        return f"#{self.seq} {self.kind} {self.node_id[:8]} by {self.actor[:8]}"


class ControlLog:
    """Append-only, totally ordered, host-owned. Every replica converges by replaying it."""

    def __init__(self) -> None:
        self._entries: List[Entry] = []

    def __len__(self) -> int:
        return len(self._entries)

    def append(self, actor: str, kind: str, node_id: str, payload: dict,
               expect_seq: Optional[int] = None) -> Entry:
        seq = len(self._entries)
        if expect_seq is not None and expect_seq != seq:
            raise ControlLogError(f"stale write: expected {expect_seq}, log is at {seq}")
        entry = Entry(seq, actor, kind, node_id, dict(payload), int(time.time() * 1000))
        self._entries.append(entry)
        return entry

    def since(self, seq: int) -> List[Entry]:
        return self._entries[seq:]


@dataclass
class Replica:
    """One peer's local view. `applied` is how far it has replayed the control log."""

    peer_id: str
    applied: int = 0
    tree: Dict[str, dict] = field(default_factory=dict)
    have_chunks: set = field(default_factory=set)
    wanted: List[str] = field(default_factory=list)
    last_seen_ms: int = 0

    def apply(self, entries: Iterable[Entry]) -> List[str]:
        """Replay entries in order. Returns the node ids that changed, for the UI."""
        touched = []
        for entry in entries:
            if entry.seq != self.applied:
                raise ControlLogError(
                    f"{self.peer_id}: gap at {entry.seq}, applied {self.applied}")
            handler = getattr(self, f"_on_{entry.kind}", None)
            if handler is not None:
                handler(entry)
            self.applied = entry.seq + 1
            touched.append(entry.node_id)
        return touched

    def _on_upsert(self, entry: Entry) -> None:
        node = self.tree.setdefault(entry.node_id, {"id": entry.node_id})
        node.update(entry.payload)

    def _on_remove(self, entry: Entry) -> None:
        self.tree.pop(entry.node_id, None)

    def _on_manifest(self, entry: Entry) -> None:
        node = self.tree.setdefault(entry.node_id, {"id": entry.node_id})
        node["chunks"] = list(entry.payload.get("chunks", []))
        missing = [c for c in node["chunks"] if c not in self.have_chunks]
        node["availability"] = "local" if not missing else "remote"
        self.wanted.extend(missing)

    def next_pull(self) -> List[str]:
        """One heartbeat's worth of chunk requests, oldest first."""
        batch, self.wanted = self.wanted[:PULL_BATCH], self.wanted[PULL_BATCH:]
        return batch

    def receive(self, chunk_id: str) -> None:
        self.have_chunks.add(chunk_id)
        for node in self.tree.values():
            chunks = node.get("chunks")
            if chunks and all(c in self.have_chunks for c in chunks):
                node["availability"] = "local"


class Host:
    """Owns the log and the presence table; serves chunks it has been given."""

    def __init__(self) -> None:
        self.log = ControlLog()
        self.replicas: Dict[str, Replica] = {}
        self.chunks: Dict[str, bytes] = {}

    def join(self, peer_id: str) -> Replica:
        replica = self.replicas.setdefault(peer_id, Replica(peer_id))
        replica.apply(self.log.since(replica.applied))
        return replica

    def save_chunk(self, chunk_id: str, body: bytes) -> None:
        self.chunks[chunk_id] = body

    def heartbeat(self, now_ms: Optional[int] = None) -> Dict[str, int]:
        """Fan the log out to every live replica and serve one batch of chunks each."""
        now_ms = now_ms if now_ms is not None else int(time.time() * 1000)
        served = {}
        for peer_id, replica in self.replicas.items():
            replica.apply(self.log.since(replica.applied))
            count = 0
            for chunk_id in replica.next_pull():
                body = self.chunks.get(chunk_id)
                if body is not None:
                    replica.receive(chunk_id)
                    count += 1
            replica.last_seen_ms = now_ms
            served[peer_id] = count
        return served

    def online(self, now_ms: Optional[int] = None) -> List[str]:
        now_ms = now_ms if now_ms is not None else int(time.time() * 1000)
        return [peer for peer, r in self.replicas.items()
                if now_ms - r.last_seen_ms <= LEASE_MS]
"##;

const CHUNKER_PY: &str = r##""""Content-defined chunking with a Rabin-style rolling hash.

Boundaries depend on the bytes, not on the offset, so inserting a byte near the start of a
file rewrites one chunk instead of every chunk after it. That is the whole reason the
sync engine can send a 4 GiB edit as a few hundred KiB.
"""

DEFAULT_MIN = 128 * 1024
DEFAULT_AVG = 512 * 1024
DEFAULT_MAX = 1024 * 1024

WINDOW = 48
PRIME = 0x3B9ACA07
MASK64 = (1 << 64) - 1


class Chunker:
    """Splits a byte stream at content-defined boundaries."""

    def __init__(self, avg=DEFAULT_AVG, minimum=DEFAULT_MIN, maximum=DEFAULT_MAX):
        if not minimum <= avg <= maximum:
            raise ValueError("expected minimum <= avg <= maximum")
        self.minimum = minimum
        self.maximum = maximum
        # One boundary every `avg` bytes on average: match the low bits of the hash.
        bits = max(1, (avg - 1).bit_length())
        self.mask = (1 << bits) - 1
        self._pow = pow(PRIME, WINDOW - 1, 1 << 64)

    def split(self, stream, read_size=1 << 20):
        """Yield chunks from a binary file object."""
        buffer = bytearray()
        digest = 0
        while True:
            block = stream.read(read_size)
            if not block:
                break
            for byte in block:
                buffer.append(byte)
                digest = (digest * PRIME + byte) & MASK64
                if len(buffer) > WINDOW:
                    stale = buffer[-WINDOW - 1]
                    digest = (digest - stale * self._pow * PRIME) & MASK64
                if len(buffer) < self.minimum:
                    continue
                if len(buffer) >= self.maximum or (digest & self.mask) == self.mask:
                    yield bytes(buffer)
                    buffer.clear()
                    digest = 0
        if buffer:
            yield bytes(buffer)

    def split_bytes(self, data: bytes):
        """Convenience wrapper for data already in memory."""
        import io
        return list(self.split(io.BytesIO(data)))
"##;

const ARCHITECTURE_MD: &str = r##"# QuantumFS architecture

## The shape of the thing
QuantumFS is a local-first file system that several machines share. Every participant keeps
a full replica of a vault's tree on its own disk and works from that replica, online or
not. One machine per vault is the **host**: it owns the ordering of changes and stores the
bytes so that a peer that is asleep does not stall everyone else. The host is not a
gatekeeper for reads - a peer that already has a chunk never asks for it again.

There is no server-side account system. Identity is a keypair generated on first run. A
vault is joined with a six-character code that resolves through a small **directory**
process to a host address; the directory learns nothing about the vault beyond that
mapping.

## Replicas and the control log
Every mutation - create, rename, move, delete, colour, manifest - is appended to the host's
**control log** as a numbered entry. The log is append-only and totally ordered. A replica
is exactly a fold over the log: replay entries 0..n and you have the tree. Two replicas
that have applied the same prefix are byte-identical in what they show.

Consequences worth stating plainly:

- Conflicts cannot produce divergence. Two peers that rename the same folder both append;
  the second entry wins and both replicas agree on which one that was.
- Recovery is trivial. A replica that has been offline for a week sends its applied
  sequence number and receives the tail.
- History is free. The audit trail in the app is the log, filtered by node id.

Writes are optimistic locally and confirmed by the host. A write that the host rejects
(quota, permission, stale sequence) is rolled back in the UI with the host's reason.

## The heartbeat
Every connected peer exchanges a frame with the host every **100 ms**. One frame carries:
the peer's applied sequence, its presence (name, colour, the node it is looking at), up to
32 chunk requests, and any chunk bodies the host asked it for. The host answers with the
log tail, the presence table, and up to 32 chunk bodies.

100 ms is the number that makes presence feel live and keeps a 32-peer vault under a
megabit of idle traffic. A peer missed for three heartbeats is shown as away; at 3 s its
lease expires and it drops out of the presence row.

## Storage
Files are split by content-defined chunking (see `src/chunker.py`) with a 512 KiB average,
128 KiB floor and 1 MiB ceiling. A file is a manifest: an ordered list of chunk ids, each
id a BLAKE2b-256 hash of the plaintext chunk. Identical chunks are stored once per vault.

A peer holds whatever chunks it has pulled. The UI shows a file as local, downloading or
remote, and a remote file becomes local by requesting its missing chunks - in manifest
order, 32 per heartbeat, so a multi-gigabyte pull never blocks a click.

## Cryptography
- **Key exchange: X-Wing**, the hybrid KEM combining X25519 with ML-KEM-768. Hybrid is the
  point: a break of either component leaves the session key protected by the other. This
  is what makes today's captured traffic useless to a future quantum adversary.
- **Signatures: ML-DSA-65.** Every control-log entry is signed by the peer that authored
  it and counter-signed by the host when it assigns a sequence number. A replica verifies
  before it applies, so a compromised transport cannot forge history.
- **Transport: AES-256-GCM**, keyed from the X-Wing shared secret through HKDF-SHA-512,
  with a per-direction nonce counter. Frames are sealed individually; a truncated stream is
  detectable and never silently accepted.
- **At rest:** chunk bodies are encrypted with the vault key before they leave the machine
  that imported them. The host stores ciphertext and cannot read the files it serves.
- Vault keys rotate weekly. Rotation appends a key epoch to the control log; old chunks
  stay readable under their epoch key.

## Failure behaviour
- Host offline: peers keep working on their replica. Local mutations queue, ordered by
  local time, and are appended when the host returns.
- Peer removed: the host drops it from the member list and stops serving it. Its local
  replica is sealed - the app removes the vault rather than leaving a stale copy open.
- Quota exceeded: the append is rejected with `QUOTA`, and the import is undone locally.
- Corrupted chunk: the hash check fails on receipt, the chunk is discarded and re-requested
  from the host. Three failures in a row surface as a transfer error.

## What is deliberately not here
No central metadata service, no cloud account, no server-side search index, no plaintext
on the host. Anything that would require trusting infrastructure the user does not own is
out of scope by design.
"##;

const API_MD: &str = r##"# Node API

The desktop app talks to an embedded node over an in-process command channel; the same
surface is exposed to integration tests. Every call is async and returns either a value or
a human-readable error string suitable for display.

## Servers

### `add_server(name, connect) -> Server`
`connect` is `ADDRESS/TOKEN`, printed by `qfsd` on startup. Contacts the admin port,
authenticates with the token and remembers the host. Returns the server with its peer id,
capacity and current vault list. Errors: `unreachable`, `bad token`, `version mismatch`.

### `list_servers() -> [Server]`
Everything this node knows, with a liveness flag refreshed by the heartbeat.

## Vaults

### `create_vault({server_id, name, quota_bytes}) -> Vault`
Provisions a vault on the host. `quota_bytes` is taken out of the server's capacity; the
call fails with `CAPACITY` if the sum of quotas would exceed it. The caller becomes owner.

### `join_vault(code) -> Vault`
Resolves a six-character code through the directory (or through a host this node already
knows), joins as a member and starts hydration. Errors: `unknown code`, `host offline`,
`revoked`.

### `get_vault_meta(vault_id) -> VaultMeta`
Name, description, join code, creation time, key rotation time, cleanup settings.

### `rotate_join_code(vault_id) -> String`
Owner and admins only. The previous code stops resolving immediately.

### `leave_vault(vault_id)` / `delete_vault(vault_id)`
Leaving removes the local replica and the membership. Deleting is owner-only and also
removes the vault from the host.

## Tree

### `list_tree(vault_id) -> [FsNode]`
The whole projected tree. `FsNode` carries id, parent id, kind, name, size, availability,
optional folder colour and optional download progress.

### `create_node({vault_id, parent_id, kind, name})`
### `rename_node({vault_id, node_id, name})`
### `move_nodes({vault_id, node_ids, parent_id})`
### `duplicate_nodes({vault_id, node_ids})`
### `delete_nodes({vault_id, node_ids})`
### `set_node_color({vault_id, node_id, color})`
Each appends one control-log entry per affected node and returns the updated nodes.
Name collisions are resolved by suffixing, never by overwriting.

## Files

### `import_files(vault_id, parent_id, paths) -> [FsNode]`
Reads each path, chunks it, encrypts and stores the chunks, appends a manifest entry. The
returned nodes are already `local`.

### `request_download(vault_id, node_id)`
Queues the file's missing chunks. Progress arrives as `backend://fs-changed` events with a
`progress` field; the node becomes `local` when the last chunk verifies.

### `read_text_preview(vault_id, node_id, max_bytes) -> Option<String>`
Assembles a local file and returns up to `max_bytes` of it, or `None` if the file is not
local or is not text.

### `open_node(vault_id, node_id)`
Assembles the file to a temporary path and hands it to the OS.

## Members and presence

### `list_members(vault_id) -> [Member]`
### `set_member_role({vault_id, peer_id, role})`
### `remove_member(vault_id, peer_id)`
### `get_presence(vault_id) -> [PeerPresence]`
### `publish_presence({vault_id, node_id})`

## Events
- `backend://fs-changed` - upserts and removals for one vault.
- `backend://presence` - the presence table after a heartbeat.
- `backend://vault-removed` - this node lost access; drop the UI for that vault.
- `backend://server-status` - a host went offline or came back.
"##;

const RUNBOOK_MD: &str = r##"# Host runbook

Audience: whoever is on call. Every command assumes the `qfsd` data directory is
`/var/lib/qfsd` unless the unit file says otherwise.

## Health in one look
    systemctl status qfsd
    journalctl -u qfsd -n 200 --no-pager
    qfsctl status            # peers, vaults, log sequence, capacity

A healthy host prints a control-log sequence that advances whenever anyone is working, and
a peer list whose `last_seen` is under 300 ms for every online member.

## Common alarms

### `qfsd_peers_online == 0` for more than 5 minutes during working hours
Usually the network, not the daemon. Check the listen address is still bound
(`ss -lntp | grep qfsd`), then check the directory registration
(`qfsctl directory ping`). If the directory lost us, restart is safe: replicas reconnect
without losing state because the control log is on disk.

### `qfsd_capacity_used > 90%`
Find the greedy vault with `qfsctl vaults --by-usage`. Raising a quota is instant and safe.
Deleting a vault is not reversible - get written confirmation from the vault owner first.

### `qfsd_log_append_rejected` climbing
Nearly always a stale-sequence storm from one peer with a clock or version problem. Its
peer id is in the log line. `qfsctl peer <id> --describe` shows its client version; ask it
to update before removing it.

### Chunk serve latency over 400 ms
Check disk first (`iostat -x 5`). The chunk store is small random reads; a host on spinning
disk under load will do this. Moving the chunk store to SSD is the fix, not more RAM.

## Routine work

### Rotate a join code
    qfsctl vault <vault-id> rotate-code
Tell the owner before you do it: every outstanding invitation dies immediately.

### Take a backup
    systemctl stop qfsd
    tar -C /var/lib -czf /backup/qfsd-$(date +%F).tgz qfsd
    systemctl start qfsd
The stop matters: the control log and the chunk index must be consistent with each other.
Expect under 60 seconds of downtime; peers keep working offline through it.

### Restore
Stop the daemon, replace the data directory, start it. Peers that are ahead of the restored
log will re-append their queued mutations. Peers that are behind simply catch up.

### Upgrade
Drain is unnecessary - this is a single process with a durable log. Install, restart, watch
`journalctl` for the version line and for the first `heartbeat` after it. Roll back by
reinstalling the previous package; the on-disk format is stable within a major version.

## Escalation
Page the platform lead if the control log refuses to open (`log: corrupt entry at N`) or if
signature verification starts failing for more than one peer. Both mean stop and
investigate, not restart in a loop.
"##;

const DEPLOY_SH: &str = r##"#!/usr/bin/env bash
# Deploy qfsd to a host. Idempotent: safe to re-run, and it refuses to half-finish.
set -euo pipefail

HOST="${1:-}"
VERSION="${2:-latest}"
DATA_DIR="${QFSD_DATA_DIR:-/var/lib/qfsd}"
UNIT="qfsd.service"
ARTIFACTS="${QFSD_ARTIFACTS:-https://builds.quantumlabs.dev/qfsd}"

usage() {
  echo "usage: $(basename "$0") <host> [version]" >&2
  echo "  host     ssh target, e.g. ops@host-a.internal" >&2
  echo "  version  release tag or 'latest' (default)" >&2
  exit 2
}

log()  { printf '\033[1;35m==>\033[0m %s\n' "$*"; }
fail() { printf '\033[1;31mfail:\033[0m %s\n' "$*" >&2; exit 1; }

[ -n "$HOST" ] || usage
command -v ssh >/dev/null || fail "ssh is not on PATH"

log "checking $HOST is reachable"
ssh -o BatchMode=yes -o ConnectTimeout=8 "$HOST" true || fail "cannot ssh to $HOST"

log "resolving version"
if [ "$VERSION" = latest ]; then
  VERSION="$(curl -fsSL "$ARTIFACTS/LATEST" | tr -d '[:space:]')"
  [ -n "$VERSION" ] || fail "could not resolve the latest version"
fi
log "deploying qfsd $VERSION to $HOST"

CURRENT="$(ssh "$HOST" 'qfsd --version 2>/dev/null | awk "{print \$2}"' || true)"
if [ "$CURRENT" = "$VERSION" ]; then
  log "already at $VERSION, nothing to do"
  exit 0
fi

log "staging the binary"
ssh "$HOST" bash -s -- "$ARTIFACTS" "$VERSION" <<'REMOTE'
set -euo pipefail
artifacts="$1"; version="$2"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
curl -fsSL "$artifacts/$version/qfsd-linux-amd64" -o "$tmp/qfsd"
curl -fsSL "$artifacts/$version/qfsd-linux-amd64.sha256" -o "$tmp/qfsd.sha256"
( cd "$tmp" && sha256sum -c --status <(sed "s|qfsd-linux-amd64|qfsd|" qfsd.sha256) )
chmod 0755 "$tmp/qfsd"
sudo install -o root -g root -m 0755 "$tmp/qfsd" /usr/local/bin/qfsd.new
REMOTE

log "backing up the data directory"
ssh "$HOST" "sudo tar -C \$(dirname $DATA_DIR) -czf /var/backups/qfsd-pre-$VERSION.tgz \$(basename $DATA_DIR)"

log "swapping the binary and restarting"
ssh "$HOST" "sudo systemctl stop $UNIT \
  && sudo mv /usr/local/bin/qfsd.new /usr/local/bin/qfsd \
  && sudo systemctl start $UNIT"

log "waiting for the health check"
for attempt in $(seq 1 20); do
  if ssh "$HOST" 'qfsctl status >/dev/null 2>&1'; then
    log "healthy after ${attempt}s"
    ssh "$HOST" 'qfsctl status | head -20'
    exit 0
  fi
  sleep 1
done

fail "qfsd did not become healthy; check 'journalctl -u $UNIT -n 200' on $HOST"
"##;

const CONFIG_YAML: &str = r##"# qfsd host configuration. Reloaded on SIGHUP except for listen addresses.
version: 3

node:
  name: host-a
  data_dir: /var/lib/qfsd
  # Total storage this host will hand out across all vaults.
  capacity: 32GiB
  # Refuse a new vault whose quota would push allocation past this fraction.
  allocation_ceiling: 0.92

listen:
  peer_addr: 0.0.0.0:8447
  # Token-gated; bind it to the management interface only.
  admin_addr: 127.0.0.1:8547
  advertise_addr: host-a.internal:8447

directory:
  addr: directory.internal:7440
  register: true
  # How often we re-announce the vault short codes we host.
  announce_interval: 30s

sync:
  heartbeat: 100ms
  peer_lease: 3s
  pull_batch: 32
  max_frame: 2MiB
  # Backpressure: stop accepting appends when the log is this far ahead of fsync.
  append_watermark: 4096

storage:
  chunk:
    average: 512KiB
    minimum: 128KiB
    maximum: 1MiB
  fsync: batched
  # Chunks nobody references are collected after this long.
  orphan_grace: 24h

crypto:
  kem: x-wing            # X25519 + ML-KEM-768
  signature: ml-dsa-65
  transport: aes-256-gcm
  kdf: hkdf-sha512
  vault_key_rotation: 7d
  # Reject peers that cannot negotiate the hybrid KEM.
  require_hybrid: true

limits:
  max_vaults: 64
  max_members_per_vault: 128
  max_file_size: 64GiB

log:
  level: info
  format: text
  # Presence spam is noisy and useless in aggregate.
  suppress: [presence.tick]

metrics:
  addr: 127.0.0.1:9847
  path: /metrics
"##;

const README_MD: &str = r##"# QuantumFS

A local-first, post-quantum-encrypted file system that several machines share in real time.
Files live on your disks. One machine per vault hosts the ordering and the bytes. Joining
takes a six-character code and no account.

## Repository layout

    src/        the reference implementation of the sync model
      main.py         qfs-chunk, the manifest tool
      sync_engine.py  replica, heartbeat and control-log model
      chunker.py      content-defined chunking
    docs/       ARCHITECTURE.md (read this first), API.md
    ops/        runbook.md, deploy.sh, config.yaml

## Getting a vault running

    # 1. a directory, so codes resolve
    qfsd --directory --listen-addr 0.0.0.0:7440

    # 2. a host
    qfsd --listen-addr 0.0.0.0:8447 \
         --admin-addr 127.0.0.1:8547 \
         --directory-addr directory.internal:7440

The host prints an app connect string, `ADDRESS/TOKEN`. Paste it into the desktop app to
add the server, create a vault, and hand the six-character join code to whoever needs it.

## Development

    python -m pytest tests/          # the reference model
    cargo test                       # the runtime
    cargo test --test e2e_node -- --nocapture   # real daemons, two nodes, real bytes

The end-to-end test is the one that matters: it starts real daemons, joins by code, edits
the tree from one node, and pulls the bytes on the other.

## Conventions
- Errors that reach a person are sentences, not enum names.
- Nothing blocks the heartbeat. Anything slow goes to a blocking task.
- The control log is the source of truth. If a bug can be explained by "the log said
  otherwise", fix the replay, not the UI.
"##;

/* ----------------------------------------------------------------- Finance */

const BOARD_SUMMARY: &str = r##"# Q3 2026 board summary
Quantum Labs, Inc. - prepared by Finance, 6 October 2026 - unaudited

## Headline
Revenue of $1.48M against a $1.39M plan, up 23% on Q2. Operating burn of $1.02M, $84K
under plan, mostly because two engineering hires slipped into Q4. Cash at quarter end
$14.2M, which is 20 months of runway at the current burn and 16 months at the Q4 plan.

## Revenue
- New ARR booked: $612K across 34 accounts. Average deal $18K, up from $14.2K.
- Expansion ARR: $196K, led by three studio accounts moving from 10 to 50 seats.
- Gross churn: $71K, of which $48K is one customer that was acquired and consolidated.
- Net revenue retention: 118%.
- Self-hosted deployments are now 41% of new bookings and carry a 31% higher average deal.

## Costs
- Headcount 38, plan was 40. Engineering 19, Design 5, Research 4, GTM 7, Ops 3.
- Cloud and infrastructure $71K, 12% under plan; the shift to customer-hosted vaults is
  visibly reducing our own storage spend.
- Security audit and penetration test $88K, on plan, completed 19 September with two
  medium findings, both closed.
- Marketing $214K, $31K over plan, from pulling the launch campaign forward.

## Watch items
1. Sales cycle for regulated buyers is 94 days, against 41 days elsewhere. The pipeline
   coverage is fine; the timing is not. Q4 forecast assumes 60% of regulated deals slip.
2. One customer is 14% of ARR. Concentration is the single largest forecast risk.
3. The two deferred engineering hires are now blocking the mobile client. Finance supports
   an above-band offer if it closes in October.

## Asks of the board
- Approve the $450K Q4 infrastructure envelope for the launch capacity buffer.
- Approve the revised option pool refresh of 3.5%.
- Guidance for Q4: $1.72M revenue, $1.15M burn, cash $13.1M at year end.
"##;

const INVOICE_091: &str = r##"===============================================================================
                        QUANTUM LABS, INC.
              1180 Foothill Parkway, Suite 400, Palo Alto, CA 94303
                  finance@quantumlabs.dev  -  +1 650 555 0142
===============================================================================

INVOICE                                            No.      2026-091
                                                   Issued   14 September 2026
                                                   Due      14 October 2026
                                                   Terms    Net 30

BILL TO
  Northwind Studios LLC
  Attn: Accounts Payable
  221 Harbour Road, Floor 6
  Seattle, WA 98104
  PO 4471-B

-------------------------------------------------------------------------------
DESCRIPTION                              QTY     UNIT PRICE        AMOUNT
-------------------------------------------------------------------------------
QuantumFS Team plan, annual              50       $  228.00     $ 11,400.00
  seats, 1 Oct 2026 - 30 Sep 2027
Self-hosted host licence, per host        3       $1,200.00     $  3,600.00
Priority support, annual                  1       $2,400.00     $  2,400.00
Onboarding and migration services         16 h    $  185.00     $  2,960.00
Volume discount, 50+ seats                                      $ -1,140.00
-------------------------------------------------------------------------------
                                              Subtotal          $ 19,220.00
                                              Sales tax (0%)    $      0.00
                                              TOTAL DUE         $ 19,220.00
-------------------------------------------------------------------------------

PAYMENT
  ACH        Pacific Commerce Bank  -  Routing 121000358  -  Account 8814-207755
  Wire       SWIFT PCBKUS66  -  same account
  Reference  INV 2026-091

NOTES
  Seat count true-up occurs at the six-month mark; additional seats are billed
  pro rata at the same unit price. Late balances accrue 1.5% per month.
  Questions: finance@quantumlabs.dev

                     Thank you for working with Quantum Labs.
===============================================================================
"##;

const INVOICE_092: &str = r##"===============================================================================
                        QUANTUM LABS, INC.
              1180 Foothill Parkway, Suite 400, Palo Alto, CA 94303
                  finance@quantumlabs.dev  -  +1 650 555 0142
===============================================================================

INVOICE                                            No.      2026-092
                                                   Issued   18 September 2026
                                                   Due       2 October 2026
                                                   Terms    Net 14

BILL TO
  Meridian Health Research Institute
  Attn: Procurement, Building C
  4400 Cascade Avenue
  Portland, OR 97213
  PO MHRI-2026-0884

-------------------------------------------------------------------------------
DESCRIPTION                              QTY     UNIT PRICE        AMOUNT
-------------------------------------------------------------------------------
QuantumFS Regulated plan, annual         24       $  396.00     $  9,504.00
  seats, 1 Oct 2026 - 30 Sep 2027
Self-hosted host licence, per host         2      $1,200.00     $  2,400.00
Compliance package                         1      $4,800.00     $  4,800.00
  (audit export, retention policy, BAA)
Security questionnaire review              6 h    $  185.00     $  1,110.00
On-site installation, two days             1      $3,200.00     $  3,200.00
Multi-year commitment credit (3 yr)                             $ -2,100.00
-------------------------------------------------------------------------------
                                              Subtotal          $ 18,914.00
                                              Sales tax (0%)    $      0.00
                                              TOTAL DUE         $ 18,914.00
-------------------------------------------------------------------------------

PAYMENT
  ACH        Pacific Commerce Bank  -  Routing 121000358  -  Account 8814-207755
  Wire       SWIFT PCBKUS66  -  same account
  Reference  INV 2026-092

NOTES
  Includes the executed Business Associate Agreement dated 11 September 2026.
  The on-site installation is scheduled for 6-7 October; travel is included.
  Questions: finance@quantumlabs.dev

                     Thank you for working with Quantum Labs.
===============================================================================
"##;

const EXPENSE_POLICY: &str = r##"# Expense policy
Effective 1 July 2026. Applies to everyone, including founders and contractors.
Owner: Finance. Questions: finance@quantumlabs.dev

## The principle
Spend the company's money as carefully as you would spend your own, and as freely as the
work requires. If you would be comfortable explaining the expense in an all-hands, it is
fine. If you would not, ask first.

## Submitting
- Receipt required for anything over $25. A photo is fine; a card statement is not.
- Submit within 30 days. After 60 days approval moves to the CFO and is not guaranteed.
- Code the expense to your team and a project. "General" is not a project.
- Reimbursement runs twice monthly, on the 5th and the 20th.

## Travel
- Book economy on flights under six hours, premium economy above six. Business class needs
  written approval before booking.
- Hotels up to $260 a night in tier-one cities, $180 elsewhere. Conference rate always
  beats the cap.
- Ground transport: rideshare or transit. Rental cars only when the itinerary needs one.
- Personal days attached to a business trip are fine; the company pays the flight it would
  have paid anyway and nothing else.

## Meals
- Travelling: $85 a day, all in. No per-diem cash.
- Team meals: $60 a head. Include the attendee list in the note.
- Client entertainment: pre-approve anything over $400.
- Alcohol is reimbursable with a meal and never on its own.

## Equipment and software
- Laptop, monitor, keyboard, mouse, chair: standard kit, ordered through Ops, not expensed.
- Home office stipend: $800 on joining, $400 every two years after.
- Software under $30 a month: expense it. Above that, ask Ops, because we probably have a
  licence already and because security reviews new vendors.
- Anything that touches customer data goes through a security review first. No exceptions,
  including free tiers.

## Not reimbursable
Parking and traffic fines, personal subscriptions, gifts to family members, travel loyalty
upgrades bought with company money, and anything paid to a vendor not in the approved list
without a purchase order.

## Approvals
Under $500 self-approved. $500-$5,000 your manager. Above $5,000 the CFO. Above $25,000 a
purchase order and the CEO. Splitting an expense to stay under a threshold is a
terminable offence.
"##;

const FINANCE_OVERVIEW: &str = r##"# Finance - how this vault is organised

This vault is the working set for the Finance team. It is not the system of record; the
ledger lives in the accounting system and the signed contracts live with Legal. What is
here is what we build, review and circulate.

## Folders
- **Q3-2026** - the current quarter's working models. `q3-budget.csv` is planned against
  actual by department and line. `revenue-forecast.csv` is the rolling 36-month model that
  feeds the board deck. `board-summary.md` is the narrative that goes with them.
- **Invoices** - issued invoices as sent. Never edit an issued invoice; void it and issue
  the next number. Numbering is `YYYY-NNN`, sequential, no gaps.
- **Policies** - the policies people actually have to follow. Changes go through the CFO
  and are announced before they take effect.

## Working rules
1. One number, one source. If a figure appears in two files, one of them links to the
   other rather than restating it.
2. Models are CSV so they diff and so anyone can open them. Presentation lives in the deck,
   not in the model.
3. Quarter-end close is the fifth working day. Nothing in Q3-2026 changes after close; the
   quarter's folder is frozen and Q4 opens.
4. Anything shared outside Finance is exported, not linked. Membership of this vault is
   Finance plus the CEO.

## Calendar
- Close: fifth working day after quarter end.
- Board pack: ten days before the meeting, drafts circulated five days before.
- Annual budget cycle opens 1 November.
- Audit fieldwork: February.

## Who to ask
Bookings and invoices: AR. Vendor payments and POs: AP. Model changes and forecast
assumptions: FP&A. Policy: CFO.
"##;

/* ---------------------------------------------------------------- Research */

const PQ_SURVEY: &str = r##"# Post-quantum cryptography: a working survey
Quantum Labs Research - revision 7, 2 September 2026
Scope: what we deploy, why, and what we are watching.

## 1. The problem, stated precisely
Shor's algorithm solves integer factorisation and discrete logarithms in polynomial time on
a sufficiently large fault-tolerant quantum computer. That retires RSA, finite-field
Diffie-Hellman and every elliptic-curve scheme in use today. Grover's algorithm halves the
effective security of symmetric primitives, which is handled by doubling key length -
AES-256 remains comfortable; AES-128 does not.

The timeline is contested and largely beside the point. The operative risk is **harvest
now, decrypt later**: traffic captured today is decrypted whenever the capability arrives.
Any data with a confidentiality lifetime longer than the remaining time to a
cryptographically relevant quantum computer is already exposed. For the file systems our
customers run - research data, legal archives, medical records - that lifetime is decades.

## 2. The standardised set
NIST completed the first selection round in August 2024:

- **ML-KEM** (FIPS 203), formerly CRYSTALS-Kyber. A module-lattice key-encapsulation
  mechanism. Parameter sets 512, 768 and 1024, targeting categories 1, 3 and 5.
- **ML-DSA** (FIPS 204), formerly CRYSTALS-Dilithium. Module-lattice signatures based on
  Fiat-Shamir with aborts. Parameter sets 44, 65 and 87.
- **SLH-DSA** (FIPS 205), formerly SPHINCS+. Stateless hash-based signatures. Large and
  slow, but its security rests only on the hash function, which makes it the conservative
  fallback if lattices ever fall.
- **FN-DSA** (Falcon), draft FIPS 206. NTRU-lattice signatures with very small signatures
  and a floating-point sampler that is genuinely difficult to implement without leaking.

Sizes matter more than speed in practice. ML-KEM-768: 1,184-byte public key, 1,088-byte
ciphertext, 2,400-byte secret key. ML-DSA-65: 1,952-byte public key, 3,309-byte signature.
Compare X25519's 32 bytes and Ed25519's 64. The cost of the migration is mostly bandwidth
and protocol framing, not CPU - ML-KEM is in fact faster than X25519 on modern hardware.

## 3. Why hybrid, and why X-Wing
Lattice assumptions are younger than factoring. A structural break would be catastrophic
and is not impossible; the SIKE collapse in 2022 - a decade-old isogeny scheme broken in an
afternoon on a laptop - is the cautionary case.

A hybrid KEM combines a post-quantum KEM with a classical one and derives the session key
from both shared secrets, so the result stays secure if **either** component holds.
**X-Wing** is the concrete construction we use: X25519 and ML-KEM-768, combined through a
SHA3-256 based KDF that binds both ciphertexts and both public keys into the output. Its
security proof holds in the standard model given ML-KEM's IND-CCA security or the
Diffie-Hellman assumption on Curve25519.

Cost: 1,216 bytes of extra key material and 1,120 bytes of extra ciphertext per handshake,
about a millisecond of CPU. That is the entire price of not betting the product on one
assumption.

## 4. Signatures in our stack
Control-log entries are signed with **ML-DSA-65**. Category 3, deterministic by default,
and no floating-point arithmetic - which is what ruled out Falcon for us, since a
side-channel in the sampler would be invisible in review and fatal in deployment.

We do not hybridise signatures. The reasoning differs from KEMs: a signature forgery
requires the adversary to act *now*, so there is no harvest-now risk, and a future break can
be answered by re-signing under a new scheme. The log format carries an algorithm
identifier per entry precisely so that migration is a version bump, not a rewrite.

## 5. Migration lessons
Three things go wrong repeatedly:

1. **Framing.** Protocols with 16-bit length fields or single-packet handshake assumptions
   break on 1-2 KB keys. Fix the framing before the cryptography.
2. **Certificate chains.** A chain with three ML-DSA-65 signatures is over 10 KB. Anything
   that assumed a handshake fits in one round trip needs re-measuring.
3. **Downgrade.** A negotiated hybrid mode that falls back silently to classical is worth
   nothing. Our hosts refuse a peer that cannot do X-Wing; `require_hybrid: true` is not
   configurable to false in production builds.

## 6. What we are watching
- FIPS 206 (FN-DSA) finalisation and whether constant-time samplers become dependable.
- The performance of ML-KEM on the embedded targets our mobile client will use.
- Key-encapsulation in group settings, for the multi-member vault rekey path - today we
  re-encapsulate per member, which is linear and fine at 128 members and not at 10,000.
- Any cryptanalytic progress on module-LWE with structured errors. Nothing concerning has
  appeared, which is what one would expect either way.

## 7. Our position
Hybrid key exchange with X-Wing, ML-DSA-65 signatures, AES-256-GCM transport, HKDF-SHA-512
derivation, weekly vault key rotation. Conservative, standardised, and honest about the
assumptions it rests on. See `ml-kem-vs-rsa.md` for the measured comparison and
`../Data/benchmark-results.csv` for the raw runs.
"##;

const ML_KEM_VS_RSA: &str = r##"# ML-KEM-768 against RSA-3072: what the numbers say
Research note - 21 August 2026. Raw runs in ../Data/benchmark-results.csv.

## Setup
Apple M3 Pro, 36 GB, macOS 15.4, performance cores pinned, 500 iterations per operation,
median reported. ML-KEM from the reference implementation with AVX2 disabled for parity;
RSA from OpenSSL 3.3. Both at roughly NIST category 3 / 128-bit classical security.

## Speed
| Operation      | ML-KEM-768 | RSA-3072   | Ratio        |
|----------------|-----------:|-----------:|-------------:|
| Key generation |      13 us |  41,000 us | 3,150x faster |
| Encapsulate    |      11 us |      30 us | 2.7x faster   |
| Decapsulate    |      13 us |   1,900 us | 146x faster   |

The key generation number is the one that changes system design. RSA key generation is a
prime search, so nobody generates RSA keys per session. ML-KEM keygen is cheap enough that
an ephemeral keypair per handshake is free, which is what actually delivers forward secrecy.

## Size
| Artefact          | ML-KEM-768 | RSA-3072 |
|-------------------|-----------:|---------:|
| Public key        |  1,184 B   |    398 B |
| Secret key        |  2,400 B   |  1,704 B |
| Ciphertext        |  1,088 B   |    384 B |
| Handshake added   | ~2,272 B   |   ~782 B |

This is the whole cost. About 1.5 KB more per handshake. On our 100 ms heartbeat that is
invisible; on a protocol that handshakes per request it would not be.

## X-Wing, the thing we actually ship
X25519 plus ML-KEM-768: 34 us keygen, 31 us encapsulate, 36 us decapsulate, 2,432 bytes of
combined public material. Roughly 2.5x the cost of ML-KEM alone and still 1,200x cheaper
than RSA keygen, in exchange for security that survives a break of either component.

## Conclusions
1. Post-quantum key exchange is not a performance problem. It is a bandwidth and framing
   problem, and a modest one.
2. Cheap keygen is a feature, not an accident: per-session ephemeral keys become the
   default rather than an optimisation.
3. There is no performance argument left for staying on RSA. The remaining arguments are
   about certificate ecosystems and vendor support, which is a procurement problem.
4. Hybrid costs about 1 ms and 1.2 KB. Nobody should be skipping it to save that.
"##;

const LAB_NOTEBOOK: &str = r##"# Lab notebook - sync and crypto bench
Keeper: Research. One entry per working session. Append only; corrections get their own
dated entry.

## 2026-08-03
Rebuilt the bench rig after the firmware update. Sensor array SENS-01 through SENS-06 back
online at 0.5 Hz. Baseline drift on SENS-04 is still 0.3 C high against the reference
thermometer; recorded, not corrected, so the raw file stays raw.
Ran the first clean ML-KEM-768 sweep, 500 iterations. Median keygen 13 us, which matches
the reference paper within noise.

## 2026-08-05
Chunker boundary study. Inserted one byte at offset 0 of a 1.4 GB file and re-chunked.
Content-defined chunking rewrote 1 chunk; fixed-size chunking rewrote 2,847. This is the
argument for CDC in one line and it goes in the architecture doc.
Side finding: our 128 KiB minimum produces a long tail of exactly-minimum chunks on highly
compressible data. Not harmful, slightly wasteful. Left alone for now.

## 2026-08-11
Heartbeat load test. 32 simulated peers against one host, all idle. Steady state 780 kbit/s
aggregate, host CPU 4%. At 128 peers, 3.1 Mbit/s and 14% CPU - linear, as expected, since
the presence table is broadcast to everyone.
Above roughly 200 peers the presence broadcast will dominate. Fix when we need it: send
deltas, not the table.

## 2026-08-14
Pull-path measurement with the 1.5 MB sensor file. Three chunks, all served in the first
heartbeat after the request. End-to-end from request to local: 118 ms, of which 96 ms is
waiting for the next heartbeat tick. The transfer is not the cost; the tick is.
Worth considering an immediate flush on a user-initiated download. Filed as an idea, not a
change - the 100 ms tick is what keeps the traffic predictable.

## 2026-08-19
ML-DSA-65 verification under replay. Replaying a 400,000-entry control log takes 41 s if
every signature is verified individually, 6 s with batch verification. Batch verification
is safe here because a batch failure falls back to individual verification to find the
bad entry. Recommending batch verify for cold replay only, never for live append.

## 2026-08-26
Chunk store on spinning disk, as a customer will have. Serve latency went from 4 ms median
to 380 ms p99 under a 16-peer pull storm. Confirms the runbook note. The access pattern is
small random reads and no amount of RAM fixes the tail.

## 2026-09-01
Temperature-correlated voltage dip on SENS-02 at the 720-sample mark, every cycle. It is
the bench heater, not the sensor. Noted so nobody reads it as a hardware fault in the
data. Kept in the dataset because it is a nice example of a real artefact for the demo.

## 2026-09-08
Re-ran the full benchmark matrix for the survey revision. No regressions. RSA-3072 keygen
still the slowest thing in the table by three orders of magnitude.
"##;

const WEEKLY_SYNC: &str = r##"# Research weekly sync

## 8 September 2026
Present: all four. 30 minutes.

**Done**
- Benchmark matrix re-run for survey revision 7; no regressions.
- Batch signature verification landed behind a flag for cold replay.
- Sensor dataset frozen at 30,000 rows for the demo vault.

**In flight**
- Group rekey design. Current per-member re-encapsulation is O(members); sketching a tree
  construction. Nothing to review yet.
- Mobile ML-KEM numbers on the A17 and on a mid-range Android. Waiting on the test device.

**Decisions**
- We are not hybridising signatures. Rationale written up in pq-crypto-survey.md section 4
  so the question stops coming back.
- Batch verification stays off for live append. The latency win is not worth the
  fallback complexity on the hot path.

**Blocked**
- Legal review of the survey before it goes on the marketing site. Chased twice.

## 1 September 2026
Present: three, one at a conference.

**Done**
- Survey revision 7 drafted, including the X-Wing rationale.
- Heartbeat load test written up; presence broadcast identified as the scaling wall.

**Discussion**
Long argument about whether to send presence deltas now or when it hurts. Landed on later:
the code is simple today and the wall is at 200 peers, which no customer is near. Recorded
so that when someone hits it, they know it was a choice.

**Next**
- Mobile numbers.
- Decide whether the demo vault ships with the sensor artefact in it. (It should. It makes
  the data look real, because it is.)
"##;

const RESEARCH_ROADMAP: &str = r##"# Research roadmap

## What this team is for
Answer the questions that would otherwise be settled by guessing: which primitives we
deploy, how the sync model behaves at scale, and what the numbers actually are. Everything
we publish internally has to be reproducible from the files in this vault.

## Now (Q4 2026)
1. **Group rekey.** Per-member re-encapsulation is linear in membership. Design and
   measure a tree-based rekey so a 1,000-member vault rotates in bounded work.
   Owner: crypto. Exit: design note plus a benchmark at 128, 1,024 and 8,192 members.
2. **Mobile cost of post-quantum.** ML-KEM and ML-DSA on the phones our client will target,
   including battery cost of a 100 ms heartbeat. Exit: numbers in benchmark-results.csv
   and a recommendation on heartbeat interval for mobile.
3. **Presence at scale.** Replace the broadcast presence table with deltas, and measure the
   crossover point. Exit: a patch behind a flag and a load test to 512 peers.

## Next (H1 2027)
4. **Cold replay performance.** A 5M-entry control log currently replays in minutes.
   Snapshotting plus batch verification should make it seconds. Exit: snapshot format
   proposal and a measured replay.
5. **Chunker tuning on real corpora.** Our 512 KiB average is a reasonable default chosen
   from first principles, not from data. Measure deduplication and rewrite amplification
   across video, RAW photography, code and document corpora.
6. **SLH-DSA fallback path.** Prove we can migrate signatures without a format break by
   shipping a second algorithm identifier end to end.

## Watching
- FIPS 206 finalisation and constant-time Falcon samplers.
- Cryptanalysis of module-LWE with structured errors.
- Threshold and distributed KEM constructions, for the multi-host vault we will eventually
  want.
- Formal verification tooling mature enough to model the control-log replay.

## Not doing
- Our own primitives. We deploy standardised algorithms with public analysis, full stop.
- A distributed consensus protocol. One host per vault with a durable ordered log is the
  design; multi-host belongs in the watching list until a customer problem demands it.
- Compression in the sync path. Measured twice, both times dominated by the fact that the
  interesting files are already compressed.
"##;
