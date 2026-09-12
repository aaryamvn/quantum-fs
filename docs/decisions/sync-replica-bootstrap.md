# Initial replica delivery through the encrypted mailbox
status: accepted
date: 2026-09-12      scope: sync
decision: >
  Admission of a new member stages a recipient-specific snapshot of H's current
  manifests and topologically ordered tree links, followed by referenced chunk
  bodies, in the existing encrypted mailbox. Snapshot instructions use existing
  control encodings and increasing synthetic ids ending at the pre-admission
  high-water mark N; they are not new global mutations. Empty current state after earlier mutations
  uses an encrypted deprecated Add no-op at N as its completion record. Reject inconsistent
  state if its snapshot requires more positive ids than the history through N.
  Persist a separate per-recipient bootstrap_through=N with the mailbox and
  admission before visibility. This coverage excludes pre-baseline log records
  from mailbox reconstruction; it is not an acknowledgment and cannot advance
  the log truncation floor. New commits append normally after N; existing
  coalescing and Clear/Remove rules determine the final bodies to deliver.
  Bind optional bootstrap coverage in a versioned flush offer to the encrypted
  flush-end digest. Tree names, manifests and file bytes remain inside GCM;
  only existing public identity history and the coverage number accompany the
  challenge. Verify archived writer documents and signatures, including accepted
  files from kicked writers. Stage the complete baseline, subsequent controls
  and final bodies atomically. Failed or tampered drains publish no replica.
  Advance actual acknowledgment and clear coverage only after successful apply
  and the existing authenticated flush acknowledgment. Persist coverage in a
  versioned replica extension; read legacy metadata with empty coverage.
  Re-seal after epoch changes without old keys. Preserve control kinds and TCP
  frame numbers, plaintext chunk files, metadata-only snapshots and sole lock.
why:
- A truncated instruction log cannot initialize a newly admitted member.
- Snapshot coverage and successful recipient acknowledgment have different meanings.
rejected:
- False acknowledgments, plaintext tree snapshots on the wire, and replaying stale log entries over a current baseline.
