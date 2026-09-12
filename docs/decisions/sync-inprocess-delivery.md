# In-process transfer and mailbox drain
status: accepted
date: 2026-09-12      scope: sync
decision: >
  Use one chunk encrypt/open helper for online pull and offline host queues.
  Accept bodies only against an opaque trusted manifest installed by H's
  authenticated control path after writer membership and ML-DSA verification.
  Store plaintext with file/index metadata. Append every control instruction;
  coalesce bodies by file/index by removing the older queued body and appending
  its replacement, preserving ascending sequence order within each AEAD type.
  Keep per-pair live traffic blocked while any mailbox owner has a pending drain.
  Complete the current wrap and re-seal stale queue frames before ordered flush;
  controls also need re-sealing when their epoch is stale. Never reset counters
  on an active key. Open frames in FIFO order, stage authenticated contents,
  apply the complete instruction log, then write bodies matching final manifests.
  Cache exact authenticated drain receipts in memory so retries after a failed
  body do not reopen accepted counters. Clear gates only after successful apply.
  Control records have monotonically increasing u64 instruction ids for
  idempotent apply; their codec wraps canonical signed-manifest bytes unchanged.
  Mailbox/replica state can be handed to a replacement in-process host without
  key handles; the replacement re-seals with the reopened keystore. This does
  not add durable chunk or queue storage and cannot recover lost process memory.
why:
- FIFO authentication and staged writes satisfy both replay-window ordering and instructions-first apply.
- Re-sealing uses H's plaintext replica without retaining restart-ineligible keys.
rejected:
- Opening live traffic before drain — can push queued counters outside W=1024.
- Coalescing instructions or replacing bodies in their old queue position — loses operations or reorders counters.
