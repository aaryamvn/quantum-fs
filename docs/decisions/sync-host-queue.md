# Designated-member host queue
status: accepted
date: 2026-09-12      scope: sync
decision: >
  The queue is a role on one appointed group member H, not an outside
  service. H uses the same ML-KEM/ML-DSA identity and pairwise keys as that
  member. Committed updates (save, mkdir, membership, new manifest) MUST go
  writer → H under K_wH. H verifies DSA, applies locally (immediate), fans
  out under K_Hpeer to online members, and enqueues catch-up control packets
  for offline members under K_Hoffline. H keeps a full plaintext replica of
  shared files so a returning peer can flush and pull from H without the
  writer. Mailbox blobs are { recipient_id, sender_id, epoch, seq, queued_at,
  ciphertext }; ciphertext is a normal pairwise packet. The mailbox holds
  control only (manifests, membership, “file F is vN”), never chunk bodies.
  Flush is ordered; apply is idempotent; only the recipient may flush, via
  ML-DSA over a challenge. Presence (online iff live heartbeat/TTL) is
  answered by H. Live cursors and other ephemeral UI events MUST go
  direct pairwise among online peers, not through H. If H is down: online
  peers may still mesh on data they already hold; new committed updates and
  offline mailboxes wait until H returns. v1 does not elect a successor.
why:
- Human: locally owned by a member so H can store data and commits feel immediate
- One send to H avoids wrapping a distinct ciphertext per offline peer
- Queue as file store would make H a cloud; bodies stay pull/fan-out
rejected:
- Non-member opaque mailbox — cannot store a replica or re-encrypt for catch-up
- Hub for live cursors — extra hop; VISION wants super-real-time among online peers
- Auto-failover of H in v1 — needs replicated log; out of scope
