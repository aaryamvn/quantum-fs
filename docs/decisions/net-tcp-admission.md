# TCP admission and mailbox transport
status: accepted
date: 2026-09-12      scope: net
decision: >
  Use the user-confirmed Types table: Identity=1, EpochHint=2, Wrap=3,
  WrapAck=4, DirLookup=5, DirAd=6, DirPut=7, DirForget=8, Join=9,
  GcmPacket=10, GcmChunk=11, FlushChal=12, FlushSig=13, Heartbeat=14.
  A peer sends signed Join M only inside GcmPacket after final WrapAck;
  its encrypted envelope is u32be(M length) || M || u32be(signature length)
  || signature. Match its identity M to the already verified exchange.
  The user-confirmed directory exception permits raw codes in directory
  requests; the directory is not a pairwise crypto member. Directory rows
  contain signed ads only, with bounded timestamp/vk replay metadata that
  survives deletion until the seven-day acceptance horizon expires.
  WrapAck carries u64be(epoch) and a one-byte collision-retry flag. Final
  GCM readiness waits for flag zero; a flagged ack precedes the loser's
  existing retry_collision wrap. Retry lost acknowledgments with cached wraps.
  Persist rejected-pair epoch watermarks while discarding their keys, cached
  wraps and peer documents. Hints can raise a floor but never create a session.
  Keep single-vault metadata (id, code, members, advertisement timestamp)
  separately from volatile plaintext/mailboxes. Use numeric bound or explicit
  advertised addresses and current-thread Tokio with LocalSet/spawn_local.
  Use thin prepare/ack adapters around existing host and replica algorithms.
  Flush sends the queued packet/chunk frames in FIFO order, followed by a GCM
  end marker committing to their canonical ordered digest. Prepare opens old
  counters into existing staging before authenticating that newer marker;
  commit only after it verifies. H removes the unchanged snapshot only after
  the recipient's GCM apply acknowledgment. Retain receipts/gates on failure.
why:
- Explicit user clarifications resolve contradictory tags, directory code privacy and local-only flush APIs.
- A new control counter opened ahead of queued controls can expire their replay window.
rejected:
- Bare peer Join; directory pair keys; resetting counters or keeping restart-ineligible keys.
- Replacing host commit/coalescing/re-sealing logic with a separate transport implementation.
