# Serialized vault tree operations
status: accepted
date: 2026-09-12      scope: sync
decision: >
  Root the directory tree at the replica's expected_root. Paths are relative to
  that root; path APIs also accept one leading slash as an explicit vault-root
  spelling. Reject NUL, backslash, drive-letter prefixes, empty components,
  dot and dot-dot, components over 255 UTF-8 bytes and paths over 4096 bytes.
  Names are byte-exact and case-sensitive without Unicode normalization;
  NFC and NFD spellings are separate dirents. Require unique names per parent,
  an existing directory parent, empty directories on unlink, and acyclic moves.
  Preserve control kinds 0–3 unchanged: Add (1) remains a deprecated no-op;
  Clear drops manifest/chunks but retains a link; Remove also removes its link.
  Add kind 4 Link(parent,name,child,is_dir), 5 Unlink(parent,name), and
  6 Rename(src_parent,src_name,dst_parent,dst_name), with canonical UTF-8
  u32-length-prefixed names. Persist dirents and ordered instructions together.
  Any current vault member may submit these path operations for any path in v1.
  NewManifest requires writer_id equal to the authenticated sender and membership;
  no per-path ACL is introduced. H is the mandatory commit hub and serializes
  arrivals. The first valid arrival changes the tree; later operations resolve
  against that resulting tree and may fail if their source no longer exists.
  Members converge by applying H's ordered log, without a CRDT. Existing-file
  saves replace manifests; new-file saves stage a manifest and Link together
  and publish them with one durable snapshot. On TCP, send the signed manifest
  and GCM bodies, then a matching Link for a new file or the existing heartbeat
  for a saved file to finalize the prepared upload. No new frame type or
  plaintext class byte is introduced.
why:
- Vault members already share plaintext trust; explicit flat authorization matches v1.
- Metadata-only directory operations preserve the chunk/replica split and deterministic replay.
rejected:
- Empty inodes from deprecated Add, case folding, Unicode normalization and coalescing instructions.
