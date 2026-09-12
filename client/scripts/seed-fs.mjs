/**
 * Deterministic seed generator for the quantam-fs mock file system.
 *
 * WHY: the browser mock (`src/lib/backend/mock/`) and the Rust core
 * (`src-tauri/src/fs_*.rs`, via `include_str!`) must show pixel-identical data,
 * so both read one generated artefact instead of two hand-maintained seeds.
 * Everything here is driven by a fixed PRNG and a fixed clock (SEED_NOW), so
 * re-running produces a byte-identical `src/lib/backend/seed/fs.json`.
 *
 * REGENERATE:
 *   cd client && node scripts/seed-fs.mjs
 *
 * Never introduce Math.random(), Date.now(), locale formatting or Map/Set
 * iteration over non-deterministic input here — it would break reproducibility
 * and make the JSON churn on every run.
 */

import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const OUT = resolve(HERE, "../src/lib/backend/seed/fs.json");

/* ------------------------------------------------------------------ clock */

/** 2026-09-12T12:00:00Z — the one "now" every timestamp is measured back from. */
const SEED_NOW = Date.UTC(2026, 8, 12, 12, 0, 0);
const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;
const KB = 1024;
const MB = 1024 * KB;
const GB = 1024 * MB;

/* -------------------------------------------------------------------- rng */

/** mulberry32 — tiny, stable, dependency-free; the only source of variation. */
function mulberry32(a) {
  return function next() {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const rnd = mulberry32(20260912);
const ri = (min, max) => min + Math.floor(rnd() * (max - min + 1));
const pick = (arr) => arr[Math.floor(rnd() * arr.length)];
const chance = (p) => rnd() < p;

/** Deterministic subset of `arr` of size `n`, order preserved. */
function pickN(arr, n) {
  const pool = arr.slice();
  const out = [];
  const take = Math.min(n, pool.length);
  for (let i = 0; i < take; i += 1) out.push(pool.splice(Math.floor(rnd() * pool.length), 1)[0]);
  return out;
}

const B32 = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
const joinCode = () => Array.from({ length: 6 }, () => B32[Math.floor(rnd() * B32.length)]).join("");

/* ---------------------------------------------------------------- identity */

const SELF = "peer_aaryaman";

const PEERS = {
  peer_aaryaman: { peerId: "peer_aaryaman", name: "Aaryaman Maheshwari", color: "#FF7B7B" },
  peer_justin: { peerId: "peer_justin", name: "Justin Park", color: "#4E0EFF" },
  peer_maya: { peerId: "peer_maya", name: "Maya Okafor", color: "#7BFFD6" },
  peer_kenji: { peerId: "peer_kenji", name: "Kenji Watanabe", color: "#FFC27B" },
  peer_priya: { peerId: "peer_priya", name: "Priya Nair", color: "#C084FC" },
  peer_lena: { peerId: "peer_lena", name: "Lena Fischer", color: "#60A5FA" },
  peer_omar: { peerId: "peer_omar", name: "Omar Haddad", color: "#F472B6" },
  peer_sofia: { peerId: "peer_sofia", name: "Sofia Rossi", color: "#34D399" },
};

const initialsOf = (name) =>
  name
    .split(" ")
    .slice(0, 2)
    .map((w) => w[0])
    .join("")
    .toUpperCase();

const firstNameOf = (peerId) => PEERS[peerId].name.split(" ")[0];

/* ------------------------------------------------------------------ vaults */

const VAULTS = [
  {
    id: "vlt_1_1",
    serverId: "srv_1",
    name: "Design Assets",
    description: "Brand, campaign and film source files for the studio.",
    createdBy: "peer_aaryaman",
    ageDays: 392,
    autoCleanup: true,
    roster: [
      ["peer_aaryaman", "admin"],
      ["peer_justin", "member"],
      ["peer_maya", "member"],
      ["peer_kenji", "member"],
      ["peer_priya", "member"],
    ],
  },
  {
    id: "vlt_1_2",
    serverId: "srv_1",
    name: "Hackathon Build",
    description: "Source tree for the post-quantum file system prototype.",
    createdBy: "peer_justin",
    ageDays: 61,
    autoCleanup: false,
    roster: [
      ["peer_justin", "admin"],
      ["peer_aaryaman", "member"],
      ["peer_kenji", "member"],
      ["peer_lena", "member"],
      ["peer_omar", "member"],
    ],
  },
  {
    id: "vlt_1_3",
    serverId: "srv_1",
    name: "Family Photos",
    description: "Shared camera roll, originals kept on the home server.",
    createdBy: "peer_maya",
    ageDays: 380,
    autoCleanup: true,
    roster: [
      ["peer_maya", "admin"],
      ["peer_aaryaman", "member"],
    ],
  },
  {
    id: "vlt_2_1",
    serverId: "srv_2",
    name: "Research Papers",
    description: "Drafts, published work and the datasets behind them.",
    createdBy: "peer_lena",
    ageDays: 310,
    autoCleanup: false,
    roster: [
      ["peer_lena", "admin"],
      ["peer_aaryaman", "member"],
      ["peer_sofia", "member"],
      ["peer_omar", "member"],
    ],
  },
  {
    id: "vlt_2_2",
    serverId: "srv_2",
    name: "Backups",
    description: "Rolling machine images and database snapshots.",
    createdBy: "peer_aaryaman",
    ageDays: 345,
    autoCleanup: true,
    roster: [
      ["peer_aaryaman", "admin"],
      ["peer_kenji", "member"],
    ],
  },
];

/* -------------------------------------------------------------------- size */

/** Extension → rough size band, so a .mp4 never reads as 4 KB in the UI. */
const CATEGORY = {
  video: ["mp4", "mov", "mkv", "avi", "webm", "m4v", "wmv", "ogv", "prores"],
  audio: ["wav", "aiff", "mp3", "flac", "aac", "ogg", "m4a", "mid"],
  image: [
    "jpg", "jpeg", "png", "heic", "webp", "tiff", "bmp", "avif", "exr", "tga",
    "raw", "dng", "cr2", "arw", "nef", "gif",
  ],
  design: [
    "psd", "psb", "ai", "fig", "sketch", "indd", "xd", "afdesign", "c4d",
    "blend", "glb", "obj", "fbx", "nk", "aep", "prproj", "drp", "hdr", "cube",
    "usdz", "stl", "gltf", "key", "procreate",
  ],
  doc: ["pdf", "docx", "pptx", "xlsx", "odt", "ods", "odp", "rtf", "epub", "djvu"],
  archive: [
    "zip", "gz", "xz", "tar", "7z", "rar", "dmg", "iso", "bak", "vmdk",
    "qcow2", "img", "bin", "pkg", "deb", "rpm", "apk", "exe", "msi", "app",
    "dll", "so", "zst", "lz4", "ipa",
  ],
  data: [
    "csv", "tsv", "parquet", "h5", "mat", "npy", "rds", "sav", "sqlite", "db",
    "sql", "feather", "arrow", "jsonl", "ndjson",
  ],
  font: ["woff2", "woff", "ttf", "otf", "eot"],
};

const BAND = {
  video: [200 * MB, 4 * GB],
  audio: [3 * MB, 120 * MB],
  image: [1 * MB, 12 * MB],
  design: [5 * MB, 400 * MB],
  doc: [200 * KB, 20 * MB],
  archive: [20 * MB, 8 * GB],
  data: [500 * KB, 500 * MB],
  font: [20 * KB, 400 * KB],
  code: [1 * KB, 80 * KB],
};

const EXT_CATEGORY = (() => {
  const map = Object.create(null);
  for (const [cat, exts] of Object.entries(CATEGORY)) for (const e of exts) map[e] = cat;
  return map;
})();

const extOf = (name) => (name.includes(".") ? name.slice(name.lastIndexOf(".") + 1).toLowerCase() : "");

function sizeFor(name) {
  const cat = EXT_CATEGORY[extOf(name)] ?? "code";
  const [lo, hi] = BAND[cat];
  return ri(lo, hi);
}

/* ---------------------------------------------------------------- tree DSL */

/** Folder spec. `k` registers a lookup key so previews/recents can find it. */
const D = (name, opts, children = []) => ({ kind: "folder", name, ...opts, children });
/** File spec. */
const F = (name, opts = {}) => ({ kind: "file", name, ...opts });
/** Several files sharing the same options. */
const Fs = (names, opts = {}) => names.map((n) => F(n, opts));

const TREES = {
  vlt_1_1: [
    D("Projects", { slug: "projects", color: "violet" }, [
      D("Website", { color: "blue" }, [
        F("index.html"),
        F("about.tsx"),
        F("sitemap.xml"),
        F("robots.txt"),
        F("analytics.tsv"),
        F("hero.jpeg"),
        F("wireframe.xd"),
        F("copy.rtf"),
      ]),
      D("Mobile App", { color: "green" }, [
        F("onboarding.mp4"),
        F("design.afdesign"),
        F("splash.psd"),
        F("store-listing.odt"),
        F("metrics.jsonl"),
        F("flow.drawio"),
        F("build.ipa"),
      ]),
      D("Q4 Campaign", { color: "amber" }, [
        F("spots.mp3"),
        F("radio.aac"),
        F("banner.avif"),
        F("print.eps"),
        F("slides.odp"),
        F("sheet.ods"),
        F("captions.vtt"),
        F("master.mkv"),
        F("promo.webm"),
      ]),
      F("roadmap.md"),
      F("timeline.xlsx"),
      F("brief.docx"),
    ]),
    D("Brand", { slug: "brand", color: "coral" }, [
      F("logo.ai"),
      F("wordmark.svg"),
      F("guidelines.pdf"),
      F("colors.json", { k: "v11_colors" }),
      F("type-specimen.indd"),
      F("brand.psd"),
      F("favicon.ico"),
      F("social-kit.zip"),
    ]),
    D("Archive", { slug: "archive", color: null }, [
      F("2019-site.zip"),
      F("old-logos.tar.gz"),
      F("backup-2024.dmg"),
      F("legacy.iso"),
      F("archive.7z"),
      F("misc.rar"),
      F("retired.bak"),
    ]),
    D("Film", { slug: "film", color: "blue" }, [
      F("intro.mov", { availability: "remote" }),
      F("teaser.mp4", { availability: "remote" }),
      F("master.prores", { availability: "remote" }),
      F("captions.srt"),
      F("edit.prproj"),
      F("motion.aep"),
      F("vo.aiff"),
      F("mix.wav"),
      F("storyboard.pdf"),
      F("color-lut.cube"),
    ]),
    D("Research", { slug: "research", color: "teal" }, [
      F("survey.csv", { k: "v11_survey" }),
      F("analysis.ipynb"),
      F("model.py"),
      F("report.tex"),
      F("refs.bib"),
      F("results.parquet"),
      F("plots.r"),
      F("summary.md", { k: "v11_summary" }),
      F("data.sqlite"),
      F("findings.pdf"),
    ]),
    F("poster.hdr", {
      slug: "poster",
      availability: "remote",
      sizeBytes: 412 * MB,
      holders: ["peer_maya", "peer_kenji"],
    }),
    F("hero.mp4", { slug: "hero", availability: "remote", sizeBytes: Math.round(1.9 * GB) }),
    F("moodboard.fig", { slug: "moodboard", availability: "local", k: "v11_moodboard", recent: true }),
    F("palette.ase", { slug: "palette", availability: "local" }),
    F("launch-plan.docx"),
    F("budget.xlsx"),
    F("pitch.pptx"),
    F("README.md", { availability: "local", k: "v11_readme", recent: true }),
    F("logo.svg"),
    F("icons.sketch"),
    F("fonts.zip", { availability: "remote" }),
    F("type-scale.json", { k: "v11_typescale" }),
    F("brand-voice.txt", { k: "v11_voice" }),
    F("campaign.key"),
    F("site.html"),
    F("styles.css"),
    F("app.tsx"),
    F("build.py", { k: "v11_buildpy" }),
    F("notes.md", { k: "v11_notes" }),
    F("photo-001.heic"),
    F("render.blend"),
    F("model.glb"),
    F("soundtrack.wav"),
    F("invoice.pdf"),
    F("backup.dmg"),
  ],

  vlt_1_2: [
    D("src", { color: "violet" }, [
      F("index.ts"),
      F("App.tsx", { k: "v12_app", recent: true }),
      F("main.rs"),
      F("lib.rs"),
      F("store.ts"),
      F("styles.css"),
      F("types.d.ts"),
      F("worker.js"),
      F("utils.mjs"),
      F("config.cjs"),
      F("schema.graphql"),
      F("query.sql"),
      F("handler.go"),
      F("service.java"),
      F("view.kt"),
      F("model.swift"),
      F("script.rb"),
      F("app.php"),
      F("main.c"),
      F("engine.cpp"),
      F("header.h"),
      F("program.cs"),
      F("main.dart"),
      F("script.lua"),
      F("module.scala"),
      F("notebook.ipynb"),
      F("tool.pl"),
      F("shell.sh"),
      F("task.ps1"),
      F("Makefile"),
      F("Dockerfile"),
    ]),
    D("docs", { color: "teal" }, [
      F("README.md", { k: "v12_readme" }),
      F("CHANGELOG.md"),
      F("LICENSE"),
      F("api.yaml"),
      F("openapi.json"),
    ]),
    D("assets", { color: "amber" }, [
      F("icon.png"),
      F("hero.webp"),
      F("logo.svg"),
      F("loader.gif"),
      F("favicon.ico"),
      F("font.woff2"),
      F("font.ttf"),
      F("font.otf"),
    ]),
    F("package.json", { k: "v12_pkg", recent: true }),
    F("Cargo.toml"),
    F("tsconfig.json"),
    F(".env", { availability: "remote" }),
    F(".gitignore"),
    F("pnpm-lock.yaml"),
    F("vite.config.ts"),
    F("tauri.conf.json"),
    F("build.zip", { availability: "remote" }),
  ],

  vlt_1_3: [
    D("2024", { color: "pink" }, [
      ...Fs(
        [
          "IMG_2401.heic", "IMG_2402.jpg", "IMG_2403.png", "IMG_2404.raw",
          "IMG_2405.dng", "IMG_2406.cr2", "IMG_2407.mov", "IMG_2408.mp4",
          "IMG_2409.gif", "IMG_2410.tiff", "IMG_2411.arw", "IMG_2412.nef",
        ],
        { availability: "remote" },
      ),
    ]),
    D("2025", { color: "amber" }, [
      ...Fs(
        [
          "IMG_2501.heic", "IMG_2502.jpg", "IMG_2503.png", "IMG_2504.dng",
          "IMG_2505.mov", "IMG_2506.mp4", "IMG_2507.avi", "IMG_2508.m4v",
          "IMG_2509.gif", "IMG_2510.heic", "IMG_2511.jpg", "IMG_2512.raw",
        ],
        { availability: "remote" },
      ),
    ]),
    D("2026", { color: "teal" }, [
      F("IMG_2601.heic", { availability: "local" }),
      F("IMG_2602.jpg", { availability: "local" }),
      F("IMG_2603.png", { availability: "local" }),
      F("IMG_2604.dng", { availability: "remote" }),
      F("IMG_2605.cr2", { availability: "remote" }),
      F("IMG_2606.mov", { availability: "remote" }),
      F("IMG_2607.mp4", { availability: "remote" }),
      F("IMG_2608.gif", { availability: "local" }),
      F("IMG_2609.tiff", { availability: "remote" }),
      F("IMG_2610.webp", { availability: "local" }),
      F("IMG_2611.avif", { availability: "remote" }),
    ]),
    F("album.md", { availability: "local", k: "v13_album", recent: true }),
  ],

  vlt_2_1: [
    D("Drafts", { color: "coral" }, [
      F("draft-alpha.tex"),
      F("draft-alpha.pdf"),
      F("abstract.md", { k: "v21_abstract", recent: true }),
      F("refs.bib"),
      F("revision.docx"),
      F("latex-build.log"),
      F("reviewer.diff"),
      F("thesis.sty"),
      F("ieee.cls"),
    ]),
    D("Published", { color: "green" }, [
      F("paper-2025.pdf"),
      F("paper-2025.tex"),
      F("bibliography.bib"),
      F("camera-ready.docx"),
      F("poster-icml.pdf"),
      F("supplement.md"),
      F("book-chapter.epub"),
      F("scan.djvu"),
      F("ieee.bst"),
    ]),
    D("Datasets", { color: "blue" }, [
      F("responses.csv"),
      F("cohort.xlsx"),
      F("events.parquet", { availability: "remote" }),
      F("signals.h5", { availability: "remote" }),
      F("matrix.mat"),
      F("embeddings.npy", { availability: "remote" }),
      F("model.rds"),
      F("survey.sav"),
      F("frames.feather"),
      F("index.arrow"),
    ]),
  ],

  vlt_2_2: [
    D("Weekly", { color: "graphite" }, [
      F("weekly-2026-09-06.dmg", { availability: "remote" }),
      F("weekly-2026-08-30.iso", { availability: "remote" }),
      F("weekly-2026-08-23.zip"),
      F("weekly-2026-08-16.tar"),
      F("weekly-2026-08-09.tar.gz"),
      F("weekly-2026-08-02.tar.xz"),
      F("home-index.bak"),
      F("postgres-dump.sql"),
      F("ledger.db", { availability: "remote" }),
      F("notes.sqlite"),
      F("workstation.vmdk", { availability: "remote" }),
      F("sandbox.qcow2", { availability: "remote" }),
    ]),
    D("Monthly", { color: "red" }, [
      F("disk.img", { availability: "remote" }),
      F("firmware.bin"),
      F("daemon.pkg"),
      F("agent_1.4.2_amd64.deb"),
      F("agent-1.4.2.x86_64.rpm"),
      F("mobile-1.4.2.apk"),
      F("installer.exe"),
      F("setup.msi"),
      F("Toolbox.app"),
      F("runtime.dll"),
      F("libcore.so"),
      F("snapshot.tar.zst", { availability: "remote" }),
      F("journal.lz4"),
    ]),
  ],
};

/* --------------------------------------------------------------- building */

const nodes = [];
const byKey = Object.create(null);
const byId = Object.create(null);
const vaultRosters = Object.create(null);

/** Interpolate a timestamp between a node's birth and SEED_NOW. */
const at2 = (node, frac) => Math.round(node.createdAt + (SEED_NOW - node.createdAt) * frac);

function buildVault(vault) {
  const memberIds = vault.roster.map(([id]) => id);
  vaultRosters[vault.id] = memberIds;
  const others = memberIds.filter((id) => id !== SELF);
  const short = vault.id.replace(/^vlt_/, "");
  let counter = 0;
  const nextId = (slug) => (slug ? `n_${short}_${slug}` : `n_${short}_${String(++counter).padStart(3, "0")}`);

  const vaultCreatedAt = SEED_NOW - vault.ageDays * DAY - ri(0, 23) * HOUR - ri(0, 59) * MIN;

  const root = {
    id: `root_${vault.id}`,
    vaultId: vault.id,
    parentId: null,
    kind: "folder",
    name: vault.name,
    sizeBytes: 0,
    createdAt: vaultCreatedAt,
    modifiedAt: vaultCreatedAt,
    createdBy: vault.createdBy,
    modifiedBy: vault.createdBy,
    color: null,
    availability: "local",
    progress: null,
    holders: memberIds.slice(),
    childCount: 0,
  };
  nodes.push(root);
  byId[root.id] = root;

  /** Recurse the spec, giving each node a birth strictly after its parent's. */
  function walk(spec, parent) {
    for (const item of spec) {
      const room = SEED_NOW - 2 * DAY - parent.createdAt;
      const createdAt =
        room > DAY
          ? parent.createdAt + ri(1, Math.floor(room / DAY)) * DAY + ri(0, 23) * HOUR + ri(0, 59) * MIN
          : parent.createdAt + ri(1, 22) * HOUR;

      const modifiedAt = item.recent
        ? SEED_NOW - ri(3, 115) * MIN
        : createdAt + Math.floor(rnd() * rnd() * (SEED_NOW - createdAt));

      const createdBy = pick(memberIds);
      const modifiedBy = modifiedAt > createdAt ? pick(memberIds) : createdBy;

      let availability = "local";
      if (item.kind === "file") {
        availability = item.availability ?? (chance(0.35) ? "remote" : "local");
      }

      let holders;
      if (item.kind === "folder") holders = memberIds.slice();
      else if (availability === "remote") holders = item.holders ?? pickN(others, ri(1, Math.min(3, others.length)));
      else holders = [SELF, ...pickN(others, ri(0, Math.min(2, others.length)))];

      const node = {
        id: nextId(item.slug),
        vaultId: vault.id,
        parentId: parent.id,
        kind: item.kind,
        name: item.name,
        sizeBytes: item.kind === "folder" ? 0 : (item.sizeBytes ?? sizeFor(item.name)),
        createdAt,
        modifiedAt: Math.max(createdAt, modifiedAt),
        createdBy,
        modifiedBy,
        color: item.color ?? null,
        availability,
        progress: null,
        holders,
        childCount: 0,
      };

      nodes.push(node);
      byId[node.id] = node;
      if (item.k) byKey[item.k] = node;

      if (item.kind === "folder") walk(item.children, node);
    }
  }

  walk(TREES[vault.id], root);
}

for (const v of VAULTS) buildVault(v);

/* Roll folder size / childCount / modifiedAt up from the leaves. */
const childrenOf = Object.create(null);
for (const n of nodes) {
  if (!n.parentId) continue;
  (childrenOf[n.parentId] ??= []).push(n);
}

function rollUp(node) {
  const kids = childrenOf[node.id] ?? [];
  node.childCount = kids.length;
  if (node.kind !== "folder") return node.sizeBytes;
  let total = 0;
  let newest = node.modifiedAt;
  for (const kid of kids) {
    total += rollUp(kid);
    if (kid.modifiedAt > newest) newest = kid.modifiedAt;
  }
  node.sizeBytes = total;
  node.modifiedAt = newest;
  return total;
}
for (const v of VAULTS) rollUp(byId[`root_${v.id}`]);

/* -------------------------------------------------------------- vault meta */

const vaults = {};
for (const v of VAULTS) {
  const root = byId[`root_${v.id}`];
  vaults[v.id] = {
    id: v.id,
    serverId: v.serverId,
    name: v.name,
    description: v.description,
    joinCode: joinCode(),
    createdAt: root.createdAt,
    createdBy: v.createdBy,
    keyRotatedAt: SEED_NOW - ri(2, 166) * HOUR,
    autoCleanup: v.autoCleanup,
    cleanupThresholdPct: 90,
  };
}

/* ----------------------------------------------------------------- members */

const members = {};
for (const v of VAULTS) {
  const vaultFiles = nodes.filter((n) => n.vaultId === v.id && n.kind === "file");
  members[v.id] = v.roster.map(([peerId, role]) => {
    const offline = peerId === "peer_priya";
    const edited = pick(vaultFiles);
    return {
      peerId,
      name: PEERS[peerId].name,
      initials: initialsOf(PEERS[peerId].name),
      color: PEERS[peerId].color,
      role,
      online: !offline,
      lastSeenAt: offline ? SEED_NOW - 3 * HOUR : SEED_NOW - ri(0, 5) * MIN,
      lastEdited: offline ? null : { nodeId: edited.id, at: edited.modifiedAt },
      queuedOps: offline ? 7 : 0,
      isSelf: peerId === SELF,
    };
  });
}

/* ------------------------------------------------------------------ access */

const nameToId = (vaultId, name) => nodes.find((n) => n.vaultId === vaultId && n.name === name).id;

const access = [
  {
    nodeId: "n_1_1_brand",
    inherit: false,
    entries: [
      { peerId: "peer_justin", level: "editor" },
      { peerId: "peer_maya", level: "editor" },
      { peerId: "peer_kenji", level: "viewer" },
      { peerId: "peer_priya", level: "viewer" },
    ],
  },
  {
    nodeId: "n_1_1_archive",
    inherit: false,
    entries: [
      { peerId: "peer_justin", level: "viewer" },
      { peerId: "peer_maya", level: "viewer" },
      { peerId: "peer_kenji", level: "viewer" },
      { peerId: "peer_priya", level: "viewer" },
    ],
  },
  {
    nodeId: "n_1_1_film",
    inherit: false,
    entries: [
      { peerId: "peer_maya", level: "editor" },
      { peerId: "peer_kenji", level: "editor" },
    ],
  },
  {
    nodeId: byKey.v12_pkg.id,
    inherit: false,
    entries: [
      { peerId: "peer_kenji", level: "editor" },
      { peerId: "peer_lena", level: "viewer" },
      { peerId: "peer_omar", level: "viewer" },
    ],
  },
  {
    nodeId: nameToId("vlt_1_3", "2026"),
    inherit: false,
    entries: [{ peerId: "peer_aaryaman", level: "editor" }],
  },
  {
    nodeId: nameToId("vlt_2_1", "Datasets"),
    inherit: false,
    entries: [
      { peerId: "peer_sofia", level: "editor" },
      { peerId: "peer_omar", level: "viewer" },
      { peerId: "peer_aaryaman", level: "viewer" },
    ],
  },
  {
    nodeId: nameToId("vlt_2_2", "Monthly"),
    inherit: false,
    entries: [{ peerId: "peer_kenji", level: "viewer" }],
  },
];

/* ----------------------------------------------------------------- history */

const rawHistory = [];
const push = (node, kind, at, by, from, to, summary) => {
  rawHistory.push({
    vaultId: node.vaultId,
    nodeId: node.id,
    kind,
    at: Math.max(node.createdAt, Math.min(at, SEED_NOW - MIN)),
    by,
    from,
    to,
    summary,
  });
};

const N = (id) => byId[id];

/* vlt_1_1 — hand-written so the demo's required nodes carry the required kinds. */
{
  const projects = N("n_1_1_projects");
  const brand = N("n_1_1_brand");
  const archive = N("n_1_1_archive");
  const film = N("n_1_1_film");
  const research = N("n_1_1_research");
  const poster = N("n_1_1_poster");
  const hero = N("n_1_1_hero");
  const moodboard = N("n_1_1_moodboard");
  const palette = N("n_1_1_palette");
  const readme = byKey.v11_readme;

  push(projects, "created", projects.createdAt, projects.createdBy, null, null, "created");
  push(projects, "colored", at2(projects, 0.2), "peer_aaryaman", null, "violet", "set colour to violet");
  push(projects, "renamed", at2(projects, 0.45), "peer_justin", "Client Work", "Projects", "renamed from Client Work");

  push(brand, "created", brand.createdAt, brand.createdBy, null, null, "created");
  push(brand, "colored", at2(brand, 0.3), "peer_aaryaman", null, "coral", "set colour to coral");
  push(brand, "access", at2(brand, 0.6), "peer_aaryaman", null, "peer_kenji", "gave Kenji view access");
  push(brand, "access", at2(brand, 0.62), "peer_aaryaman", null, "peer_justin", "gave Justin edit access");

  push(archive, "created", archive.createdAt, archive.createdBy, null, null, "created");
  push(archive, "access", at2(archive, 0.5), "peer_aaryaman", null, "peer_maya", "gave Maya view access");
  push(archive, "deleted", at2(archive, 0.74), "peer_justin", "old-draft.psd", null, "deleted old-draft.psd");

  push(film, "created", film.createdAt, film.createdBy, null, null, "created");
  push(film, "colored", at2(film, 0.35), "peer_maya", null, "blue", "set colour to blue");
  push(film, "access", at2(film, 0.55), "peer_aaryaman", null, "peer_maya", "gave Maya edit access");

  push(research, "created", research.createdAt, research.createdBy, null, null, "created");
  push(research, "colored", at2(research, 0.4), "peer_kenji", null, "teal", "set colour to teal");

  push(poster, "created", poster.createdAt, poster.createdBy, null, null, "created");
  push(poster, "renamed", at2(poster, 0.4), "peer_maya", "poster-v1.hdr", "poster.hdr", "renamed from poster-v1.hdr");
  push(poster, "moved", at2(poster, 0.6), "peer_kenji", "Archive", "Design Assets", "moved from Archive to Design Assets");
  push(poster, "downloaded", at2(poster, 0.92), "peer_aaryaman", null, null, "downloaded to this Mac");

  push(hero, "created", hero.createdAt, hero.createdBy, null, null, "created");
  push(hero, "modified", hero.modifiedAt, hero.modifiedBy, null, null, "modified");
  push(hero, "duplicated", at2(hero, 0.8), "peer_justin", "hero.mp4", "hero copy.mp4", "duplicated from hero.mp4");

  push(moodboard, "created", moodboard.createdAt, moodboard.createdBy, null, null, "created");
  push(moodboard, "downloaded", at2(moodboard, 0.85), "peer_aaryaman", null, null, "downloaded to this Mac");
  push(moodboard, "modified", moodboard.modifiedAt, moodboard.modifiedBy, null, null, "modified");

  push(palette, "created", palette.createdAt, palette.createdBy, null, null, "created");
  push(palette, "duplicated", at2(palette, 0.5), "peer_maya", "palette.ase", "palette copy.ase", "duplicated from palette.ase");
  push(palette, "moved", at2(palette, 0.7), "peer_justin", "Brand", "Design Assets", "moved from Brand to Design Assets");

  push(readme, "created", readme.createdAt, readme.createdBy, null, null, "created");
  push(readme, "modified", readme.modifiedAt, readme.modifiedBy, null, null, "modified");
}

/* Other vaults — generated, two events per sampled node. */
const EXTRA_KINDS = ["modified", "renamed", "moved", "colored", "downloaded", "duplicated"];

for (const v of VAULTS) {
  if (v.id === "vlt_1_1") continue;
  const pool = nodes.filter((n) => n.vaultId === v.id && n.parentId);
  const picks = pickN(pool, 6);
  for (const node of picks) {
    push(node, "created", node.createdAt, node.createdBy, null, null, "created");
    const kind = pick(EXTRA_KINDS);
    const who = pick(vaultRosters[v.id]);
    const when = at2(node, 0.3 + rnd() * 0.65);
    if (kind === "modified") push(node, kind, node.modifiedAt, node.modifiedBy, null, null, "modified");
    else if (kind === "renamed") {
      const old = `old-${node.name}`;
      push(node, kind, when, who, old, node.name, `renamed from ${old}`);
    } else if (kind === "moved") {
      const parent = byId[node.parentId];
      push(node, kind, when, who, "Inbox", parent.name, `moved from Inbox to ${parent.name}`);
    } else if (kind === "colored") {
      const colour = node.color ?? "amber";
      push(node, kind, when, who, null, colour, `set colour to ${colour}`);
    } else if (kind === "downloaded") {
      push(node, kind, when, "peer_aaryaman", null, null, "downloaded to this Mac");
    } else {
      push(node, kind, when, who, node.name, `${node.name} copy`, `duplicated from ${node.name}`);
    }
  }
}

rawHistory.sort((a, b) => a.at - b.at || a.nodeId.localeCompare(b.nodeId) || a.kind.localeCompare(b.kind));
const history = rawHistory.map((e, i) => ({
  id: `h_${i + 1}`,
  vaultId: e.vaultId,
  nodeId: e.nodeId,
  kind: e.kind,
  at: e.at,
  by: e.by,
  from: e.from,
  to: e.to,
  summary: e.summary,
}));

/* ----------------------------------------------------------------- recents */

const recents = [
  byKey.v11_readme,
  byKey.v11_moodboard,
  byKey.v12_app,
  byKey.v12_pkg,
  byKey.v13_album,
  byKey.v21_abstract,
]
  .slice()
  .sort((a, b) => b.modifiedAt - a.modifiedAt)
  .map((n) => ({ vaultId: n.vaultId, nodeId: n.id, at: n.modifiedAt }));

/* ---------------------------------------------------------------- previews */

const PREVIEW_TEXT = {
  v11_readme: `# Design Assets

Everything the studio ships from. Source lives here, exports go to Projects/.

- Brand/ — marks, type, colour. Ask before editing guidelines.pdf.
- Film/ — masters are remote by default; download before scrubbing.
- Archive/ — frozen. Read-only for everyone but the admins.

Naming: kebab-case, no dates in filenames, versions live in history.
`,
  v11_notes: `Notes — week 37

- Poster is finally locked. Kenji has the 412 MB HDR pinned locally.
- Hero cut runs 8s long; Justin trimming the title card.
- Priya is offline again (7 ops queued) — she has the type specimen.
- Move the 2019 site zip out of Archive before the key rotation.
`,
  v11_typescale: `{
  "base": 15,
  "ratio": 1.2,
  "steps": {
    "caption": 12,
    "body": 15,
    "lead": 18,
    "title": 22,
    "display": 32
  },
  "family": "GT Walsheim",
  "weights": [400, 500]
}
`,
  v11_voice: `Brand voice

Plain. Precise. Never breathless.

Say "your files stay on your machine", not "revolutionary local-first paradigm".
Numbers beat adjectives. One idea per sentence.
Never use exclamation marks in product copy.
`,
  v11_buildpy: `#!/usr/bin/env python3
"""Export every board in the working file to 2x PNG."""

from pathlib import Path

OUT = Path("exports")
SCALE = 2


def export(boards):
    OUT.mkdir(exist_ok=True)
    for name, board in boards.items():
        target = OUT / f"{name}@{SCALE}x.png"
        board.render(scale=SCALE).save(target)
        print(f"wrote {target}")


if __name__ == "__main__":
    export(load_boards())
`,
  v11_colors: `{
  "coral": "#FF7B7B",
  "violet": "#4E0EFF",
  "mint": "#7BFFD6",
  "amber": "#FFC27B",
  "ink": "#0A0C12",
  "surface": "#11141C",
  "line": "#1E2430"
}
`,
  v11_survey: `respondent,role,years,vault_size_gb,sync_rating,offline_days
r001,designer,4,38,4,2
r002,engineer,9,120,5,0
r003,producer,6,12,3,5
r004,engineer,2,64,4,1
r005,designer,11,210,5,0
r006,researcher,3,9,2,8
r007,engineer,7,95,4,1
r008,producer,1,4,3,3
r009,designer,5,47,5,0
r010,researcher,8,150,4,2
`,
  v12_readme: `# quantam-fs

Local-first, serverless, post-quantum-encrypted file system with real-time
multiplayer editing.

## Run

    pnpm install
    pnpm tauri dev

## Layout

    src/      React client
    src-tauri/ Rust core: transport, crypto, replicated tree
    docs/     protocol notes

Keys are ML-KEM-768; every vault rotates weekly.
`,
  v12_pkg: `{
  "name": "quantam-fs",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc && vite build",
    "tauri": "tauri"
  },
  "dependencies": {
    "react": "19.0.0",
    "react-dom": "19.0.0",
    "motion": "13.0.0",
    "lucide-react": "0.460.0"
  }
}
`,
  v11_summary: `# Summary

Across 10 respondents, sync confidence tracks vault size inversely once a peer
passes ~100 GB. Offline days cluster on producers, who hold the fewest bytes.

Next: instrument queued-op depth per peer; the current proxy (offline_days)
overstates how far behind a peer actually is.
`,
  v21_abstract: `# Abstract

We describe a replicated file tree in which every operation is encrypted under a
lattice-based scheme before it leaves the device, and in which conflict
resolution is total: any two peers that have seen the same set of operations
agree on the same tree, regardless of order.

Our prototype sustains 4,000 ops/s per peer on commodity hardware with a median
convergence latency of 38 ms across a LAN of eight peers.
`,
};

const previews = {};
for (const [key, text] of Object.entries(PREVIEW_TEXT)) {
  const node = byKey[key];
  if (!node) throw new Error(`preview key without a node: ${key}`);
  if (text.length > 900) throw new Error(`preview too long: ${key} (${text.length})`);
  previews[node.id] = text;
}

/* ------------------------------------------------------------------- write */

const out = {
  generatedAt: SEED_NOW,
  self: SELF,
  vaults,
  members,
  nodes,
  access,
  history,
  recents,
  previews,
};

mkdirSync(dirname(OUT), { recursive: true });
writeFileSync(OUT, `${JSON.stringify(out, null, 2)}\n`, "utf8");

const exts = new Set(nodes.filter((n) => n.kind === "file").map((n) => extOf(n.name)));
process.stdout.write(
  `seed-fs: ${nodes.length} nodes · ${exts.size} extensions · ${Object.keys(vaults).length} vaults · ` +
    `${history.length} history · ${access.length} access · ${Object.keys(previews).length} previews → ${OUT}\n`,
);
