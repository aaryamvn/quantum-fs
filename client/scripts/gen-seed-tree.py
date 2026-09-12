#!/usr/bin/env python3
"""Build the demo seed tree: six vaults of real, plausible files.

    python3 client/scripts/gen-seed-tree.py --out /tmp/qfs-demo/seed

Layout (the seeder in client/src-tauri/tests/seed_demo.rs reads exactly this):

    <out>/A/01 Product Design      -> vault "Product Design" on server A
    <out>/A/02 Engineering
    <out>/A/03 Marketing Launch
    <out>/B/01 Research Lab        -> vault "Research Lab" on server B
    <out>/B/02 Finance Q3
    <out>/B/03 Personal Archive

The `NN ` prefix only fixes the order; the vault name is what follows it.

Every vault holds at least one of each of the 22 extensions the app draws a
distinct icon for, and the office formats are genuine containers: the .docx,
.xlsx and .pptx are real OOXML zips, the .pdf is a hand-built one-page PDF and
the .png is a real deflate-compressed image. Everything is generated from a
seeded RNG, so two runs produce the same tree byte for byte.

Standard library only.
"""

from __future__ import annotations

import argparse
import json
import random
import shutil
import struct
import sys
import zipfile
import zlib
from pathlib import Path

# ---------------------------------------------------------------- constants

# One of each of these lands in every vault's first folder, so the file grid
# always shows the full icon set.
REQUIRED_EXTS = [
    "docx", "xlsx", "pptx", "pdf", "png", "svg", "md", "txt", "py", "ts",
    "tsx", "js", "json", "csv", "yaml", "toml", "sh", "rs", "html", "css",
    "sql", "ipynb",
]

# Drawn at random for every file beyond the required set. Weighted towards the
# formats a real working folder is full of.
FILLER_EXTS = (
    ["md"] * 4 + ["txt"] * 3 + ["pdf"] * 3 + ["png"] * 3 + ["csv"] * 2 +
    ["docx"] * 2 + ["xlsx"] * 2 + ["json"] * 2 + ["py"] * 2 + ["ts"] * 2 +
    ["js", "tsx", "rs", "sh", "yaml", "toml", "svg", "html", "css", "sql",
     "ipynb", "pptx"]
)

# Anything under ~8 KB reads as "empty" in the size column; anything over 20 KB
# makes a vault expensive to sync live. One outsized file per vault gives the
# byte counts something to show.
SMALL_MIN, SMALL_MAX = 1_200, 20_000
BIG_MIN, BIG_MAX = 300_000, 800_000
VAULT_BYTE_BUDGET = 6 * 1024 * 1024

ZIP_DATE = (2026, 9, 1, 9, 0, 0)   # fixed, so the office zips are reproducible

# ------------------------------------------------------------------ vaults

VAULTS = [
    {
        "server": "A",
        "dirname": "01 Product Design",
        "project": "QuantumFS",
        "team": "Product Design",
        "folders": ["Design System", "Research", "Shipped Screens"],
        "nested": ["Drafts", "Exports"],
        "people": ["Aaryaman", "Noor", "Elias", "Priya", "Tomas"],
        "terms": [
            "vault card", "sidebar", "file grid", "onboarding", "join code",
            "peer avatar", "storage ring", "context menu", "toast", "splash",
        ],
        "slugs": [
            "brand-guidelines", "color-tokens", "type-scale", "icon-audit",
            "sidebar-spec", "file-grid-spec", "vault-card", "onboarding-flow",
            "empty-states", "toast-patterns", "storage-ring", "avatar-stack",
            "context-menu", "motion-notes", "grid-baseline", "dark-mode",
            "accessibility-review", "copy-deck", "handoff-notes", "usability-round-3",
            "interview-notes", "persona-map", "journey-map", "survey-results",
            "competitive-teardown", "wireframes-v2", "hi-fi-v4", "splash-screen",
            "settings-panel", "share-sheet", "download-states", "offline-badge",
            "review-checklist", "spacing-audit", "component-inventory",
            "figma-export", "redline-sheet", "shipped-log", "changelog-design",
            "design-debt",
        ],
    },
    {
        "server": "A",
        "dirname": "02 Engineering",
        "project": "qfsd",
        "team": "Engineering",
        "folders": ["Architecture", "Runbooks", "Services", "Tooling"],
        "nested": ["Diagrams", "RFC Drafts"],
        "people": ["Marek", "Sofia", "Dan", "Ingrid", "Hassan"],
        "terms": [
            "replica loop", "chunk store", "manifest", "heartbeat", "admin port",
            "directory ad", "ML-KEM handshake", "piece scheduler", "admission",
            "join code", "eviction", "backpressure",
        ],
        "slugs": [
            "architecture", "protocol-notes", "wire-format", "chunker",
            "manifest-format", "replica-loop", "heartbeat", "admission-flow",
            "key-rotation", "admin-port", "directory-service", "piece-scheduler",
            "cache-eviction", "backpressure", "error-taxonomy", "metrics",
            "tracing", "load-test", "bench-results", "profiling-notes",
            "runbook-host", "runbook-directory", "incident-2026-08-14",
            "oncall-handbook", "deploy", "rollback", "healthcheck", "config",
            "service-map", "dependency-audit", "build-matrix", "ci-pipeline",
            "release-checklist", "api-surface", "node-api", "fs-types",
            "storage-model", "migration-007", "flags", "todo-tech-debt",
        ],
    },
    {
        "server": "A",
        "dirname": "03 Marketing Launch",
        "project": "QuantumFS 1.0",
        "team": "Marketing",
        "folders": ["Campaign Assets", "Press", "Web"],
        "nested": ["Social Cuts", "Stills"],
        "people": ["Bea", "Jonah", "Amara", "Luca", "Sinead"],
        "terms": [
            "launch day", "waitlist", "press embargo", "landing page", "hero shot",
            "demo video", "pricing page", "newsletter", "keynote", "founder letter",
        ],
        "slugs": [
            "launch-plan", "messaging-house", "positioning", "tagline-options",
            "press-release", "press-kit-notes", "embargo-list", "media-targets",
            "founder-letter", "keynote-outline", "demo-script", "faq",
            "landing-copy", "pricing-copy", "hero-banner", "og-image",
            "social-thread", "linkedin-post", "newsletter-draft", "email-sequence",
            "waitlist-report", "paid-plan", "budget-launch", "channel-mix",
            "brand-lockup", "logo-usage", "screenshot-spec", "video-storyboard",
            "caption-sheet", "utm-conventions", "analytics-plan", "post-mortem",
            "influencer-list", "event-run-of-show", "swag-order", "webinar-deck",
            "case-study-draft", "testimonials", "asset-index", "launch-checklist",
        ],
    },
    {
        "server": "B",
        "dirname": "01 Research Lab",
        "project": "Post-quantum sync",
        "team": "Research",
        "folders": ["Benchmarks", "Notebooks", "Papers"],
        "nested": ["Raw Runs", "Plots"],
        "people": ["Dr. Okafor", "Wen", "Ana", "Petra", "Yusuf"],
        "terms": [
            "ML-KEM-768", "ML-DSA-65", "lattice", "key encapsulation", "throughput",
            "handshake latency", "entropy pool", "side channel", "constant time",
            "hybrid mode",
        ],
        "slugs": [
            "ml-kem-vs-rsa", "handshake-latency", "throughput-matrix",
            "entropy-audit", "constant-time-review", "hybrid-mode", "kem-sizes",
            "signature-costs", "lattice-primer", "pq-survey", "threat-model",
            "attack-surface", "side-channel-notes", "bench-harness", "run-log",
            "plot-latency", "plot-throughput", "dataset-notes", "methodology",
            "reproducibility", "lab-notebook", "weekly-sync", "roadmap-research",
            "paper-draft", "related-work", "citations", "reviewer-comments",
            "experiment-042", "experiment-043", "seed-corpus", "fuzzing-notes",
            "memory-profile", "cpu-profile", "hardware-inventory", "calibration",
            "error-bars", "statistics", "abstract", "poster", "open-questions",
        ],
    },
    {
        "server": "B",
        "dirname": "02 Finance Q3",
        "project": "FY26 Q3",
        "team": "Finance",
        "folders": ["Budgets", "Invoices", "Reports"],
        "nested": ["Approved", "Pending"],
        "people": ["Renata", "Kofi", "Mei", "Thomas", "Ilse"],
        "terms": [
            "burn rate", "runway", "accrual", "purchase order", "headcount",
            "cloud spend", "reconciliation", "forecast", "variance", "vendor",
        ],
        "slugs": [
            "budget-q3", "budget-q4-draft", "forecast-model", "burn-rate",
            "runway-scenarios", "headcount-plan", "cloud-spend", "vendor-list",
            "purchase-orders", "invoice-2026-091", "invoice-2026-092",
            "invoice-2026-093", "expense-policy", "travel-policy", "reimbursements",
            "payroll-summary", "tax-notes", "audit-prep", "reconciliation",
            "variance-analysis", "board-summary", "cap-table-notes", "runway-chart",
            "cost-centres", "amortisation", "accruals", "contract-review",
            "renewal-calendar", "insurance", "banking-notes", "fx-exposure",
            "monthly-close", "q3-report", "q2-report", "kpi-sheet",
            "unit-economics", "pricing-model", "discount-approvals",
            "vendor-scorecard", "finance-readme",
        ],
    },
    {
        "server": "B",
        "dirname": "03 Personal Archive",
        "project": "Personal",
        "team": "Archive",
        "folders": ["Archive 2024", "Photos", "Scripts"],
        "nested": ["Letters", "Receipts"],
        "people": ["Sam", "Ravi", "Nina", "Otto", "Clara"],
        "terms": [
            "backup", "scan", "old laptop", "shoebox", "family", "recipe",
            "trip", "spare keys", "warranty", "notebook",
        ],
        "slugs": [
            "backup-index", "old-laptop-notes", "scan-batch-01", "scan-batch-02",
            "letters-2019", "postcards", "recipes", "reading-list", "trip-kyoto",
            "trip-lisbon", "packing-list", "warranty-camera", "warranty-laptop",
            "insurance-renters", "apartment-inventory", "moving-checklist",
            "receipts-2024", "tax-2024", "bank-statements-notes", "passwords-readme",
            "dotfiles", "backup-script", "photo-rename", "resize-batch",
            "duplicate-finder", "old-website", "cv-2023", "cover-letter",
            "portfolio-notes", "guitar-tabs", "running-log", "garden-plan",
            "bike-maintenance", "car-service", "family-tree", "birthday-list",
            "gift-ideas", "wishlist", "notes-to-self", "archive-readme",
        ],
    },
]

# ------------------------------------------------------------ text builders
#
# Every text generator grows its own body until it reaches `target` bytes, so a
# 2 KB file and an 18 KB file of the same type are both structurally sensible
# rather than one padded with filler.


def sentences(rng, ctx, count):
    subject = rng.choice(ctx["people"])
    out = []
    for _ in range(count):
        term = rng.choice(ctx["terms"])
        other = rng.choice(ctx["terms"])
        out.append(rng.choice([
            f"The {term} still assumes a single writer; {subject} is rewriting that path.",
            f"We measured the {term} twice and the second run matched the first to within 3%.",
            f"Nothing about the {other} changes here, which is the point of keeping it separate.",
            f"{subject} flagged the {term} in review: it is correct but very hard to read.",
            f"Open question: does the {term} survive a restart with the {other} half-written?",
            f"Decision: keep the {term} as it is for 1.0 and revisit after the launch.",
            f"The {term} is the part people notice first, so it gets the careful pass.",
            f"If the {other} is unavailable we fall back to the {term} and log it once.",
        ]))
    return " ".join(out)


def grow(head, make_block, target):
    parts = [head]
    size = len(head)
    index = 0
    while size < target:
        block = make_block(index)
        parts.append(block)
        size += len(block)
        index += 1
    return "".join(parts)


def gen_md(ctx, rng, target):
    head = (
        f"# {ctx['title']}\n\n"
        f"_{ctx['team']} - {ctx['project']} - {ctx['folder']}_\n\n"
        f"{sentences(rng, ctx, 3)}\n\n"
    )

    def block(i):
        term = rng.choice(ctx["terms"])
        rows = "\n".join(
            f"| {rng.choice(ctx['terms'])} | {rng.choice(ctx['people'])} | "
            f"{rng.choice(['open', 'done', 'blocked', 'in review'])} |"
            for _ in range(rng.randint(3, 6))
        )
        return (
            f"## {i + 1}. {term.capitalize()}\n\n"
            f"{sentences(rng, ctx, rng.randint(2, 5))}\n\n"
            f"- {sentences(rng, ctx, 1)}\n"
            f"- {sentences(rng, ctx, 1)}\n"
            f"- {sentences(rng, ctx, 1)}\n\n"
            f"| item | owner | state |\n| --- | --- | --- |\n{rows}\n\n"
        )

    return grow(head, block, target)


def gen_txt(ctx, rng, target):
    head = f"{ctx['title']}\n{'=' * len(ctx['title'])}\n\n"

    def block(i):
        day = 1 + (i % 28)
        return (
            f"2026-09-{day:02d}  {rng.choice(ctx['people'])}\n"
            f"    {sentences(rng, ctx, rng.randint(2, 4))}\n\n"
        )

    return grow(head, block, target)


def gen_py(ctx, rng, target):
    head = (
        '"""' + ctx["title"] + ".\n\n"
        + sentences(rng, ctx, 2) + '\n"""\n\n'
        "from __future__ import annotations\n\n"
        "import json\nimport logging\nfrom dataclasses import dataclass\n\n"
        "log = logging.getLogger(__name__)\n\n\n"
        "@dataclass(frozen=True)\nclass Record:\n"
        "    name: str\n    owner: str\n    bytes_used: int\n\n\n"
    )

    def block(i):
        name = ctx["slug"].replace("-", "_")
        return (
            f"def {name}_step_{i}(records: list[Record], *, strict: bool = True) -> dict:\n"
            f'    """{sentences(rng, ctx, 1)}"""\n'
            f"    total = sum(record.bytes_used for record in records)\n"
            f"    if strict and total > {rng.randint(1, 64)} * 1024 ** 3:\n"
            f'        raise ValueError("quota exceeded")\n'
            f"    log.debug(\"step {i}: %d records, %d bytes\", len(records), total)\n"
            f"    return {{\"step\": {i}, \"count\": len(records), \"bytes\": total}}\n\n\n"
        )

    return grow(head, block, target)


def gen_ts(ctx, rng, target):
    head = (
        f"// {ctx['title']}\n// {sentences(rng, ctx, 1)}\n\n"
        "export interface VaultSummary {\n"
        "  id: string;\n  name: string;\n  usedBytes: number;\n"
        "  quotaBytes: number;\n  members: number;\n}\n\n"
    )

    def block(i):
        name = "".join(part.capitalize() for part in ctx["slug"].split("-"))
        return (
            f"/** {sentences(rng, ctx, 1)} */\n"
            f"export function {name[0].lower()}{name[1:]}Step{i}(vaults: VaultSummary[]): VaultSummary[] {{\n"
            f"  return vaults\n"
            f"    .filter((vault) => vault.usedBytes < vault.quotaBytes)\n"
            f"    .sort((a, b) => b.usedBytes - a.usedBytes)\n"
            f"    .slice(0, {rng.randint(4, 24)});\n}}\n\n"
        )

    return grow(head, block, target)


def gen_tsx(ctx, rng, target):
    head = (
        f"// {ctx['title']}\nimport {{ useMemo, useState }} from \"react\";\n\n"
        "type Props = { vaultId: string; onOpen: (id: string) => void };\n\n"
    )

    def block(i):
        name = "".join(part.capitalize() for part in ctx["slug"].split("-"))
        return (
            f"export function {name}Panel{i}({{ vaultId, onOpen }}: Props) {{\n"
            f"  const [query, setQuery] = useState(\"\");\n"
            f"  const label = useMemo(() => `${{vaultId}} - {rng.choice(ctx['terms'])}`, [vaultId]);\n"
            f"  return (\n"
            f"    <section className=\"panel\" aria-label={{label}}>\n"
            f"      <input value={{query}} onChange={{(e) => setQuery(e.target.value)}} />\n"
            f"      <button type=\"button\" onClick={{() => onOpen(vaultId)}}>Open</button>\n"
            f"    </section>\n  );\n}}\n\n"
        )

    return grow(head, block, target)


def gen_js(ctx, rng, target):
    head = f"// {ctx['title']}\n'use strict';\n\nconst DEFAULTS = {{ retries: 3, timeoutMs: 15000 }};\n\n"

    def block(i):
        return (
            f"function {ctx['slug'].replace('-', '_')}_{i}(input, options = DEFAULTS) {{\n"
            f"  // {sentences(rng, ctx, 1)}\n"
            f"  const rows = Array.isArray(input) ? input : [input];\n"
            f"  return rows.map((row, index) => ({{ ...row, index, retries: options.retries }}));\n"
            f"}}\n\nmodule.exports.{ctx['slug'].replace('-', '_')}_{i} = "
            f"{ctx['slug'].replace('-', '_')}_{i};\n\n"
        )

    return grow(head, block, target)


def gen_json(ctx, rng, target):
    entries = []
    size = 0
    i = 0
    while size < target:
        entry = {
            "id": f"{ctx['slug']}-{i:03d}",
            "label": rng.choice(ctx["terms"]),
            "owner": rng.choice(ctx["people"]),
            "bytes": rng.randint(1024, 8 * 1024 ** 3),
            "state": rng.choice(["local", "remote", "syncing", "evicted"]),
            "note": sentences(rng, ctx, 1),
        }
        entries.append(entry)
        size += len(json.dumps(entry))
        i += 1
    document = {
        "schema": 3,
        "project": ctx["project"],
        "generated": "2026-09-12T09:00:00Z",
        "entries": entries,
    }
    return json.dumps(document, indent=2, sort_keys=True) + "\n"


def gen_csv(ctx, rng, target):
    head = "id,date,owner,item,state,bytes,note\n"

    def block(i):
        return (
            f"{ctx['slug'][:4].upper()}-{i:05d},2026-{1 + i % 9:02d}-{1 + i % 28:02d},"
            f"{rng.choice(ctx['people'])},{rng.choice(ctx['terms'])},"
            f"{rng.choice(['open', 'closed', 'pending', 'verified'])},"
            f"{rng.randint(1024, 900 * 1024 ** 2)},"
            f"\"{sentences(rng, ctx, 1)}\"\n"
        )

    return grow(head, block, target)


def gen_yaml(ctx, rng, target):
    head = (
        f"# {ctx['title']}\n"
        f"project: {ctx['project']}\nteam: {ctx['team']}\nschema: 3\n\n"
        "defaults:\n  retries: 3\n  timeout_ms: 15000\n  capacity_bytes: 34359738368\n\n"
        "entries:\n"
    )

    def block(i):
        return (
            f"  - id: {ctx['slug']}-{i:03d}\n"
            f"    label: \"{rng.choice(ctx['terms'])}\"\n"
            f"    owner: {rng.choice(ctx['people'])}\n"
            f"    bytes: {rng.randint(1024, 8 * 1024 ** 3)}\n"
            f"    note: \"{sentences(rng, ctx, 1)}\"\n"
        )

    return grow(head, block, target)


def gen_toml(ctx, rng, target):
    head = (
        f"# {ctx['title']}\n\n[project]\nname = \"{ctx['project']}\"\n"
        f"team = \"{ctx['team']}\"\nschema = 3\n\n"
        "[defaults]\nretries = 3\ntimeout_ms = 15000\n\n"
    )

    def block(i):
        return (
            f"[[entry]]\nid = \"{ctx['slug']}-{i:03d}\"\n"
            f"label = \"{rng.choice(ctx['terms'])}\"\n"
            f"owner = \"{rng.choice(ctx['people'])}\"\n"
            f"bytes = {rng.randint(1024, 8 * 1024 ** 3)}\n"
            f"note = \"{sentences(rng, ctx, 1)}\"\n\n"
        )

    return grow(head, block, target)


def gen_sh(ctx, rng, target):
    head = (
        f"#!/usr/bin/env bash\n# {ctx['title']}\n# {sentences(rng, ctx, 1)}\n"
        "set -euo pipefail\n\nROOT=\"${1:-$PWD}\"\nLOG=\"${LOG:-/tmp/" + ctx["slug"] + ".log}\"\n\n"
    )

    def block(i):
        return (
            f"step_{i}() {{\n"
            f"  echo \"[{ctx['slug']}] step {i}: {rng.choice(ctx['terms'])}\" | tee -a \"$LOG\"\n"
            f"  find \"$ROOT\" -maxdepth {rng.randint(1, 4)} -type f -name '*.json' -print0 |\n"
            f"    xargs -0 -n1 basename >>\"$LOG\" 2>/dev/null || true\n"
            f"}}\nstep_{i}\n\n"
        )

    return grow(head, block, target)


def gen_rs(ctx, rng, target):
    head = (
        f"//! {ctx['title']}\n//! {sentences(rng, ctx, 1)}\n\n"
        "use std::collections::BTreeMap;\n\n"
        "#[derive(Debug, Clone, PartialEq, Eq)]\npub struct Entry {\n"
        "    pub id: String,\n    pub owner: String,\n    pub bytes: u64,\n}\n\n"
    )

    def block(i):
        name = ctx["slug"].replace("-", "_")
        return (
            f"/// {sentences(rng, ctx, 1)}\n"
            f"pub fn {name}_step_{i}(entries: &[Entry]) -> BTreeMap<String, u64> {{\n"
            f"    let mut out = BTreeMap::new();\n"
            f"    for entry in entries {{\n"
            f"        *out.entry(entry.owner.clone()).or_insert(0) += entry.bytes;\n"
            f"    }}\n"
            f"    out.retain(|_, bytes| *bytes > {rng.randint(1, 512)} * 1024);\n"
            f"    out\n}}\n\n"
        )

    return grow(head, block, target)


def gen_html(ctx, rng, target):
    head = (
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n"
        f"<title>{ctx['title']}</title>\n<link rel=\"stylesheet\" href=\"style.css\">\n"
        "</head>\n<body>\n"
        f"<h1>{ctx['title']}</h1>\n<p>{sentences(rng, ctx, 2)}</p>\n"
    )

    def block(i):
        return (
            f"<section id=\"s{i}\">\n  <h2>{rng.choice(ctx['terms']).capitalize()}</h2>\n"
            f"  <p>{sentences(rng, ctx, rng.randint(2, 4))}</p>\n"
            f"  <ul>\n    <li>{rng.choice(ctx['people'])}</li>\n"
            f"    <li>{rng.choice(ctx['terms'])}</li>\n  </ul>\n</section>\n"
        )

    return grow(head, block, target) + "</body>\n</html>\n"


def gen_css(ctx, rng, target):
    head = (
        f"/* {ctx['title']} */\n:root {{\n  --bg: #0e1014;\n  --surface: #171a21;\n"
        "  --text: #e6e9ef;\n  --accent: #6f7cff;\n  --radius: 12px;\n}\n\n"
    )

    def block(i):
        return (
            f".{ctx['slug']}-{i} {{\n"
            f"  display: flex;\n  gap: {rng.randint(4, 24)}px;\n"
            f"  padding: {rng.randint(4, 20)}px {rng.randint(8, 28)}px;\n"
            f"  background: var(--surface);\n  border-radius: var(--radius);\n"
            f"  color: var(--text);\n}}\n\n"
        )

    return grow(head, block, target)


def gen_sql(ctx, rng, target):
    head = (
        f"-- {ctx['title']}\n-- {sentences(rng, ctx, 1)}\n\n"
        "CREATE TABLE IF NOT EXISTS entry (\n"
        "  id         TEXT PRIMARY KEY,\n  owner      TEXT NOT NULL,\n"
        "  label      TEXT NOT NULL,\n  bytes      INTEGER NOT NULL,\n"
        "  created_at TEXT NOT NULL\n);\n\n"
    )

    def block(i):
        return (
            f"INSERT INTO entry (id, owner, label, bytes, created_at) VALUES\n"
            f"  ('{ctx['slug']}-{i:04d}', '{rng.choice(ctx['people'])}', "
            f"'{rng.choice(ctx['terms'])}', {rng.randint(1024, 10 ** 9)}, "
            f"'2026-0{1 + i % 9}-{1 + i % 28:02d}');\n"
        )

    return grow(head, block, target) + (
        "\nSELECT owner, count(*) AS files, sum(bytes) AS total\n"
        "  FROM entry GROUP BY owner ORDER BY total DESC;\n"
    )


def gen_svg(ctx, rng, target):
    palette = ["#6f7cff", "#3ddc97", "#ffb454", "#ff6b81", "#4fc3f7", "#b388ff"]
    head = (
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 640 400\" "
        "width=\"640\" height=\"400\" role=\"img\">\n"
        f"  <title>{ctx['title']}</title>\n"
        "  <rect width=\"640\" height=\"400\" fill=\"#0e1014\"/>\n"
    )

    def block(i):
        x = rng.randint(10, 580)
        y = rng.randint(10, 340)
        return (
            f"  <rect x=\"{x}\" y=\"{y}\" width=\"{rng.randint(20, 60)}\" "
            f"height=\"{rng.randint(10, 50)}\" rx=\"6\" fill=\"{rng.choice(palette)}\" "
            f"opacity=\"0.{rng.randint(4, 9)}\"/>\n"
            f"  <circle cx=\"{x + 12}\" cy=\"{y + 12}\" r=\"{rng.randint(3, 9)}\" "
            f"fill=\"{rng.choice(palette)}\"/>\n"
        )

    body = grow(head, block, max(target, 1200))
    return body + (
        "  <text x=\"24\" y=\"376\" fill=\"#e6e9ef\" font-family=\"sans-serif\" "
        f"font-size=\"18\">{ctx['title']}</text>\n</svg>\n"
    )


def gen_ipynb(ctx, rng, target):
    cells = [{
        "cell_type": "markdown",
        "metadata": {},
        "source": [f"# {ctx['title']}\n", "\n", sentences(rng, ctx, 2) + "\n"],
    }]
    size = 0
    i = 0
    while size < target:
        code = [
            "import json\n",
            "import statistics\n",
            "\n",
            f"rows = [r for r in load(\"{ctx['slug']}-{i:02d}.json\") if r[\"bytes\"] > 0]\n",
            "print(len(rows), statistics.median(r[\"bytes\"] for r in rows))\n",
        ]
        cells.append({
            "cell_type": "code",
            "execution_count": i + 1,
            "metadata": {},
            "outputs": [{
                "name": "stdout",
                "output_type": "stream",
                "text": [f"{rng.randint(40, 9000)} {rng.randint(1024, 10 ** 7)}\n"],
            }],
            "source": code,
        })
        cells.append({
            "cell_type": "markdown",
            "metadata": {},
            "source": [f"## Run {i + 1}\n", "\n", sentences(rng, ctx, 3) + "\n"],
        })
        size += sum(len(line) for cell in cells[-2:] for line in cell["source"]) + 200
        i += 1
    notebook = {
        "cells": cells,
        "metadata": {
            "kernelspec": {"display_name": "Python 3", "language": "python", "name": "python3"},
            "language_info": {"name": "python", "version": "3.12.2"},
        },
        "nbformat": 4,
        "nbformat_minor": 5,
    }
    return json.dumps(notebook, indent=1) + "\n"


TEXT_GENERATORS = {
    "md": gen_md, "txt": gen_txt, "py": gen_py, "ts": gen_ts, "tsx": gen_tsx,
    "js": gen_js, "json": gen_json, "csv": gen_csv, "yaml": gen_yaml,
    "toml": gen_toml, "sh": gen_sh, "rs": gen_rs, "html": gen_html,
    "css": gen_css, "sql": gen_sql, "svg": gen_svg, "ipynb": gen_ipynb,
}

# ---------------------------------------------------------- binary builders


def png_bytes(rng, size=64):
    """A real 8-bit RGB PNG: IHDR + deflated IDAT + IEND, CRCs and all."""
    base = (rng.randint(30, 210), rng.randint(30, 210), rng.randint(30, 210))
    raw = bytearray()
    for y in range(size):
        raw.append(0)   # filter type 0 (None) for every scanline
        for x in range(size):
            raw.append((base[0] + x * 2) % 256)
            raw.append((base[1] + y * 2) % 256)
            raw.append((base[2] + (x + y)) % 256)

    def chunk(kind, payload):
        return (struct.pack(">I", len(payload)) + kind + payload
                + struct.pack(">I", zlib.crc32(kind + payload) & 0xFFFFFFFF))

    header = struct.pack(">IIBBBBB", size, size, 8, 2, 0, 0, 0)
    return (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", header)
            + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
            + chunk(b"IEND", b""))


def pdf_bytes(ctx, rng):
    """A one-page PDF written by hand: 4 objects and a correct xref table."""
    lines = [ctx["title"], "", f"{ctx['team']} - {ctx['project']}", ""]
    for _ in range(rng.randint(6, 14)):
        text = sentences(rng, ctx, 1)
        # PDF string literals escape these three characters and nothing else.
        text = text.replace("\\", r"\\").replace("(", r"\(").replace(")", r"\)")
        lines.append(text[:92])

    parts = ["BT", "/F1 16 Tf", "72 720 Td", "18 TL"]
    for index, line in enumerate(lines):
        parts.append(f"({line}) Tj" if line else "()Tj")
        parts.append("T*")
        if index == 0:
            parts.append("/F1 11 Tf")
    parts.append("ET")
    stream = "\n".join(parts).encode("latin-1", "replace")

    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
        b"/Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        b"<< /Length " + str(len(stream)).encode() + b" >>\nstream\n" + stream + b"\nendstream",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ]

    out = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
    offsets = []
    for number, body in enumerate(objects, start=1):
        offsets.append(len(out))
        out += f"{number} 0 obj\n".encode() + body + b"\nendobj\n"
    xref_at = len(out)
    out += f"xref\n0 {len(objects) + 1}\n".encode()
    out += b"0000000000 65535 f \n"
    for offset in offsets:
        out += f"{offset:010d} 00000 n \n".encode()
    out += (f"trailer\n<< /Size {len(objects) + 1} /Root 1 0 R >>\n"
            f"startxref\n{xref_at}\n%%EOF\n").encode()
    return bytes(out)


def _zip(parts):
    """Deterministic zip: fixed timestamps, deflate, stable member order."""
    import io
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w", zipfile.ZIP_DEFLATED) as archive:
        for name, payload in parts:
            info = zipfile.ZipInfo(name, date_time=ZIP_DATE)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o600 << 16
            archive.writestr(info, payload)
    return buffer.getvalue()


def xml_escape(text):
    return (text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
            .replace('"', "&quot;"))


XML_HEAD = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'

RELS_NS = "http://schemas.openxmlformats.org/package/2006/relationships"
OFFICE_REL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"


def docx_bytes(ctx, rng):
    paragraphs = [(ctx["title"], True), (f"{ctx['team']} - {ctx['project']}", False)]
    for _ in range(rng.randint(8, 20)):
        paragraphs.append((sentences(rng, ctx, rng.randint(1, 3)), False))

    body = []
    for text, heading in paragraphs:
        style = '<w:pPr><w:pStyle w:val="Heading1"/></w:pPr>' if heading else ""
        body.append(
            f"<w:p>{style}<w:r><w:t xml:space=\"preserve\">{xml_escape(text)}</w:t></w:r></w:p>"
        )

    document = (
        XML_HEAD
        + '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">'
        + "<w:body>" + "".join(body)
        + '<w:sectPr><w:pgSz w:w="12240" w:h="15840"/>'
          '<w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr>'
        + "</w:body></w:document>"
    )
    content_types = (
        XML_HEAD
        + '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
        '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
        '<Default Extension="xml" ContentType="application/xml"/>'
        '<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'
        "</Types>"
    )
    rels = (
        XML_HEAD + f'<Relationships xmlns="{RELS_NS}">'
        f'<Relationship Id="rId1" Type="{OFFICE_REL}/officeDocument" Target="word/document.xml"/>'
        "</Relationships>"
    )
    return _zip([
        ("[Content_Types].xml", content_types),
        ("_rels/.rels", rels),
        ("word/document.xml", document),
    ])


def xlsx_bytes(ctx, rng):
    header = ["Item", "Owner", "State", "Bytes", "Note"]
    rows = [header]
    for _ in range(rng.randint(12, 40)):
        rows.append([
            rng.choice(ctx["terms"]),
            rng.choice(ctx["people"]),
            rng.choice(["open", "closed", "pending", "verified"]),
            rng.randint(1024, 10 ** 9),
            sentences(rng, ctx, 1)[:110],
        ])

    def column(index):
        name = ""
        index += 1
        while index:
            index, remainder = divmod(index - 1, 26)
            name = chr(65 + remainder) + name
        return name

    sheet_rows = []
    for row_index, row in enumerate(rows, start=1):
        cells = []
        for col_index, value in enumerate(row):
            ref = f"{column(col_index)}{row_index}"
            if isinstance(value, int):
                cells.append(f'<c r="{ref}"><v>{value}</v></c>')
            else:
                cells.append(
                    f'<c r="{ref}" t="inlineStr"><is><t xml:space="preserve">'
                    f"{xml_escape(str(value))}</t></is></c>"
                )
        sheet_rows.append(f'<row r="{row_index}">' + "".join(cells) + "</row>")

    ns = "http://schemas.openxmlformats.org/spreadsheetml/2006/main"
    sheet = (XML_HEAD + f'<worksheet xmlns="{ns}"><sheetData>'
             + "".join(sheet_rows) + "</sheetData></worksheet>")
    workbook = (
        XML_HEAD + f'<workbook xmlns="{ns}" xmlns:r="{OFFICE_REL}">'
        '<sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets></workbook>'
    )
    content_types = (
        XML_HEAD
        + '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
        '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
        '<Default Extension="xml" ContentType="application/xml"/>'
        '<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>'
        '<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>'
        "</Types>"
    )
    rels = (
        XML_HEAD + f'<Relationships xmlns="{RELS_NS}">'
        f'<Relationship Id="rId1" Type="{OFFICE_REL}/officeDocument" Target="xl/workbook.xml"/>'
        "</Relationships>"
    )
    workbook_rels = (
        XML_HEAD + f'<Relationships xmlns="{RELS_NS}">'
        f'<Relationship Id="rId1" Type="{OFFICE_REL}/worksheet" Target="worksheets/sheet1.xml"/>'
        "</Relationships>"
    )
    return _zip([
        ("[Content_Types].xml", content_types),
        ("_rels/.rels", rels),
        ("xl/workbook.xml", workbook),
        ("xl/_rels/workbook.xml.rels", workbook_rels),
        ("xl/worksheets/sheet1.xml", sheet),
    ])


A_NS = "http://schemas.openxmlformats.org/drawingml/2006/main"
P_NS = "http://schemas.openxmlformats.org/presentationml/2006/main"


def _theme_xml():
    def scheme_color(tag, value):
        return f'<a:{tag}><a:srgbClr val="{value}"/></a:{tag}>'

    colors = "".join(scheme_color(tag, value) for tag, value in [
        ("dk1", "000000"), ("lt1", "FFFFFF"), ("dk2", "1F2430"), ("lt2", "EEF1F6"),
        ("accent1", "6F7CFF"), ("accent2", "3DDC97"), ("accent3", "FFB454"),
        ("accent4", "FF6B81"), ("accent5", "4FC3F7"), ("accent6", "B388FF"),
        ("hlink", "0563C1"), ("folHlink", "954F72"),
    ])
    fill = ('<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>'
            '<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>'
            '<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>')
    line = ('<a:ln w="6350"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>'
            '<a:ln w="12700"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>'
            '<a:ln w="19050"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>')
    effects = "<a:effectStyle><a:effectLst/></a:effectStyle>" * 3
    return (
        XML_HEAD + f'<a:theme xmlns:a="{A_NS}" name="QuantumFS"><a:themeElements>'
        f"<a:clrScheme name=\"QuantumFS\">{colors}</a:clrScheme>"
        '<a:fontScheme name="QuantumFS">'
        '<a:majorFont><a:latin typeface="Helvetica"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont>'
        '<a:minorFont><a:latin typeface="Helvetica"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont>'
        "</a:fontScheme>"
        '<a:fmtScheme name="QuantumFS">'
        f"<a:fillStyleLst>{fill}</a:fillStyleLst>"
        f"<a:lnStyleLst>{line}</a:lnStyleLst>"
        f"<a:effectStyleLst>{effects}</a:effectStyleLst>"
        f"<a:bgFillStyleLst>{fill}</a:bgFillStyleLst>"
        "</a:fmtScheme></a:themeElements>"
        "<a:objectDefaults/><a:extraClrSchemeLst/></a:theme>"
    )


def _shape(shape_id, name, place, x, y, cx, cy, paragraphs, size):
    runs = "".join(
        f'<a:p><a:r><a:rPr lang="en-US" sz="{size}" dirty="0"/>'
        f"<a:t>{xml_escape(text)}</a:t></a:r></a:p>"
        for text in paragraphs
    ) or "<a:p/>"
    return (
        f'<p:sp><p:nvSpPr><p:cNvPr id="{shape_id}" name="{name}"/>'
        f'<p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr>'
        f'<p:nvPr><p:ph type="{place}"/></p:nvPr></p:nvSpPr>'
        f'<p:spPr><a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm>'
        f'<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr>'
        f"<p:txBody><a:bodyPr/><a:lstStyle/>{runs}</p:txBody></p:sp>"
    )


def pptx_bytes(ctx, rng):
    bullets = [sentences(rng, ctx, 1)[:120] for _ in range(rng.randint(3, 6))]
    slide_body = (
        _shape(2, "Title 1", "ctrTitle", 838200, 1825625, 10515600, 1325563,
               [ctx["title"]], 4000)
        + _shape(3, "Subtitle 2", "subTitle", 838200, 3375025, 10515600, 2360613,
                 [f"{ctx['team']} - {ctx['project']}"] + bullets, 1800)
    )
    slide = (
        XML_HEAD + f'<p:sld xmlns:a="{A_NS}" xmlns:r="{OFFICE_REL}" xmlns:p="{P_NS}">'
        "<p:cSld><p:spTree>"
        '<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>'
        '<p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/>'
        '<a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>'
        + slide_body +
        "</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"
    )

    layout_body = (
        _shape(2, "Title 1", "ctrTitle", 838200, 1825625, 10515600, 1325563, [], 4000)
        + _shape(3, "Subtitle 2", "subTitle", 838200, 3375025, 10515600, 2360613, [], 1800)
    )
    layout = (
        XML_HEAD + f'<p:sldLayout xmlns:a="{A_NS}" xmlns:r="{OFFICE_REL}" xmlns:p="{P_NS}" '
        'type="title" preserve="1"><p:cSld name="Title Slide"><p:spTree>'
        '<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>'
        '<p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/>'
        '<a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>'
        + layout_body +
        "</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"
    )

    master = (
        XML_HEAD + f'<p:sldMaster xmlns:a="{A_NS}" xmlns:r="{OFFICE_REL}" xmlns:p="{P_NS}">'
        "<p:cSld><p:spTree>"
        '<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>'
        '<p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/>'
        '<a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>'
        + _shape(2, "Title Placeholder 1", "title", 838200, 365125, 10515600, 1325563, [], 4400)
        + "</p:spTree></p:cSld>"
        '<p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" '
        'accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" '
        'accent6="accent6" hlink="hlink" folHlink="folHlink"/>'
        '<p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst>'
        "</p:sldMaster>"
    )

    presentation = (
        XML_HEAD + f'<p:presentation xmlns:a="{A_NS}" xmlns:r="{OFFICE_REL}" xmlns:p="{P_NS}">'
        '<p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst>'
        '<p:sldIdLst><p:sldId id="256" r:id="rId2"/></p:sldIdLst>'
        '<p:sldSz cx="12192000" cy="6858000"/><p:notesSz cx="6858000" cy="9144000"/>'
        "</p:presentation>"
    )

    def relationships(*items):
        body = "".join(
            f'<Relationship Id="{rid}" Type="{OFFICE_REL}/{kind}" Target="{target}"/>'
            for rid, kind, target in items
        )
        return XML_HEAD + f'<Relationships xmlns="{RELS_NS}">{body}</Relationships>'

    content_types = (
        XML_HEAD
        + '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
        '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
        '<Default Extension="xml" ContentType="application/xml"/>'
        '<Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>'
        '<Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/>'
        '<Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/>'
        '<Override PartName="/ppt/slides/slide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>'
        '<Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>'
        "</Types>"
    )

    return _zip([
        ("[Content_Types].xml", content_types),
        ("_rels/.rels", relationships(("rId1", "officeDocument", "ppt/presentation.xml"))),
        ("ppt/presentation.xml", presentation),
        ("ppt/_rels/presentation.xml.rels", relationships(
            ("rId1", "slideMaster", "slideMasters/slideMaster1.xml"),
            ("rId2", "slide", "slides/slide1.xml"),
            ("rId3", "theme", "theme/theme1.xml"),
        )),
        ("ppt/slideMasters/slideMaster1.xml", master),
        ("ppt/slideMasters/_rels/slideMaster1.xml.rels", relationships(
            ("rId1", "slideLayout", "../slideLayouts/slideLayout1.xml"),
            ("rId2", "theme", "../theme/theme1.xml"),
        )),
        ("ppt/slideLayouts/slideLayout1.xml", layout),
        ("ppt/slideLayouts/_rels/slideLayout1.xml.rels", relationships(
            ("rId1", "slideMaster", "../slideMasters/slideMaster1.xml"),
        )),
        ("ppt/slides/slide1.xml", slide),
        ("ppt/slides/_rels/slide1.xml.rels", relationships(
            ("rId1", "slideLayout", "../slideLayouts/slideLayout1.xml"),
        )),
        ("ppt/theme/theme1.xml", _theme_xml()),
    ])


# ----------------------------------------------------------------- writing


def write_file(path, ext, ctx, rng, target):
    if ext in TEXT_GENERATORS:
        payload = TEXT_GENERATORS[ext](ctx, rng, target).encode("utf-8")
    elif ext == "png":
        payload = png_bytes(rng, 64)
    elif ext == "pdf":
        payload = pdf_bytes(ctx, rng)
    elif ext == "docx":
        payload = docx_bytes(ctx, rng)
    elif ext == "xlsx":
        payload = xlsx_bytes(ctx, rng)
    elif ext == "pptx":
        payload = pptx_bytes(ctx, rng)
    else:
        raise ValueError(f"no generator for .{ext}")
    path.write_bytes(payload)
    return len(payload)


def make_ctx(vault, folder, slug):
    return {
        "vault": vault["name"],
        "team": vault["team"],
        "project": vault["project"],
        "people": vault["people"],
        "terms": vault["terms"],
        "folder": folder,
        "slug": slug,
        "title": slug.replace("-", " ").title(),
    }


def fill_dir(directory, vault, folder_label, count, rng, taken, exts=None):
    """Write `count` files into `directory`. `exts` pins the leading extensions."""
    directory.mkdir(parents=True, exist_ok=True)
    files = 0
    total = 0
    for index in range(count):
        ext = exts[index] if exts and index < len(exts) else rng.choice(FILLER_EXTS)
        slug = rng.choice(vault["slugs"])
        name = f"{slug}.{ext}"
        bump = 2
        while name in taken:
            name = f"{slug}-{bump}.{ext}"
            bump += 1
        taken.add(name)
        ctx = make_ctx(vault, folder_label, name[: -len(ext) - 1])
        total += write_file(directory / name, ext, ctx, rng, rng.randint(SMALL_MIN, SMALL_MAX))
        files += 1
    return files, total


def build_vault(root, vault, rng):
    vault_dir = root / vault["server"] / vault["dirname"]
    if vault_dir.exists():
        shutil.rmtree(vault_dir)
    vault_dir.mkdir(parents=True)

    folders = list(vault["folders"])
    total_files = 0
    total_bytes = 0

    # Root: 5-15 entries, of which the folders are 2-4.
    root_files = rng.randint(5, 15) - len(folders)
    root_files = max(2, root_files)
    taken = set()
    files, written = fill_dir(vault_dir, vault, "root", root_files, rng, taken)
    total_files += files
    total_bytes += written
    root_items = len(folders) + files

    # The alphabetically first folder is the deep one: 22-35 files, one of every
    # required type, plus two nested subfolders.
    deep = sorted(folders)[0]
    deep_count = rng.randint(22, 35)
    order = list(REQUIRED_EXTS)
    rng.shuffle(order)
    deep_dir = vault_dir / deep
    taken = set()
    files, written = fill_dir(deep_dir, vault, deep, deep_count, rng, taken, exts=order)
    total_files += files
    total_bytes += written

    for nested in vault["nested"]:
        nested_taken = set()
        files, written = fill_dir(
            deep_dir / nested, vault, f"{deep}/{nested}", rng.randint(4, 8), rng, nested_taken
        )
        total_files += files
        total_bytes += written

    for folder in folders:
        if folder == deep:
            continue
        folder_taken = set()
        files, written = fill_dir(
            vault_dir / folder, vault, folder, rng.randint(3, 8), rng, folder_taken
        )
        total_files += files
        total_bytes += written

    # One deliberately outsized file per vault, so the size column and the
    # transfer bars have something real to show. It replaces the deep folder's
    # first .csv rather than adding a file, which keeps the counts above exact.
    big = sorted(deep_dir.glob("*.csv"))[0]
    total_bytes -= big.stat().st_size
    renamed = deep_dir / f"{vault['name'].lower().replace(' ', '-')}-export.csv"
    if renamed != big and not renamed.exists():
        big.rename(renamed)
        big = renamed
    ctx = make_ctx(vault, deep, big.stem)
    total_bytes += write_file(big, "csv", ctx, rng, rng.randint(BIG_MIN, BIG_MAX))

    return root_items, total_files, total_bytes


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", default="/tmp/qfs-demo/seed",
                        help="root of the generated tree (default: /tmp/qfs-demo/seed)")
    parser.add_argument("--seed", type=int, default=20260912,
                        help="RNG seed; the same seed always yields the same tree")
    args = parser.parse_args(argv)

    root = Path(args.out).expanduser()
    root.mkdir(parents=True, exist_ok=True)

    rows = []
    grand_files = 0
    grand_bytes = 0
    for index, vault in enumerate(VAULTS):
        vault = dict(vault)
        vault["name"] = vault["dirname"].split(" ", 1)[1]
        rng = random.Random(args.seed + index * 1009)
        root_items, files, size = build_vault(root, vault, rng)
        if size > VAULT_BYTE_BUDGET:
            print(f"warning: {vault['name']} is {size} bytes, over the "
                  f"{VAULT_BYTE_BUDGET} byte budget", file=sys.stderr)
        rows.append((vault["server"], vault["name"], root_items, files, size))
        grand_files += files
        grand_bytes += size

    width = max(len(row[1]) for row in rows)
    print(f"seed tree: {root}")
    print()
    print(f"  {'srv':<4}{'vault':<{width + 2}}{'root':>6}{'files':>8}{'bytes':>12}")
    print(f"  {'-' * (4 + width + 2 + 6 + 8 + 12)}")
    for server, name, root_items, files, size in rows:
        print(f"  {server:<4}{name:<{width + 2}}{root_items:>6}{files:>8}{size:>12,}")
    print(f"  {'-' * (4 + width + 2 + 6 + 8 + 12)}")
    print(f"  {'':<4}{'total':<{width + 2}}{'':>6}{grand_files:>8}{grand_bytes:>12,}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
