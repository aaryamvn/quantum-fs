# Durable plaintext replica
status: accepted
date: 2026-09-12      scope: storage
decision: >
  Store immutable plaintext chunks (at most 1 MiB each) at chunks/<lowercase
  hex chunk_id>; store metadata only in replica.bin (at most 16 MiB). Use
  encoding.rs for its canonical body: root, dirents, members, signed manifests,
  ordered instructions, next instruction id, mailbox content references,
  acknowledgment watermarks and chunk index. Prefix with qfs/local/replica/,
  version 1, increasing u64 generation and u32 body length; append SHA-256
  of the entire preceding file. Validate the complete header/hash before parsing.
  Reuse atomic_private_write for both layouts. Persist staged metadata before
  publishing state or returning outgoing packets; failed persistence leaves
  visible state unchanged. Chunk puts precede metadata writes and may leave
  orphans after failure. Sweep orphans on open and after successful persistence;
  retain every chunk referenced by a live manifest or undrained mailbox.
  Verify indexed chunk hashes on load and discard corrupt chunks. Never persist
  GCM envelopes or key handles; rebuild mailboxes using the fresh live epoch.
  Keep the live KeyStore and its sibling advisory lock as the sole process lock.
  The expected root is FileId(vault_id.0), or zero when no vault id exists;
  reject another root. Migrating a zero-root replica later requires an explicit
  one-time metadata rewrite, never accepting two roots in one store.
  Acknowledgments report the inclusive last successfully applied instruction N;
  retain instructions after the minimum acknowledged N over all current members
  including H. A missing acknowledgment is zero and pins the entire prefix.
  Carry online watermarks on existing GCM heartbeat kind 5 plus u64be(N);
  preserve the legacy one-byte heartbeat. Flush acknowledgments infer N from
  the acknowledged control batch, without a new wire acknowledgment type.
  No expiry substitutes for kick. Disk encryption is outside v1; zeroize keys,
  not file bytes. Pull acceptance checks the currently applied manifest and
  linked file immediately before persisting received chunks.
why:
- Metadata mutations must not rewrite a vault's plaintext; mailbox references pin required bytes.
- Hashes reject complete-file corruption; durable publication and kernel locking prevent partial visibility.
rejected:
- Plaintext in replica.bin, durable ciphertext, old key pinning, PID files and a second replica lock.
