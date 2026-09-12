# Designated-member host queue
status: accepted
date: 2026-09-12      scope: sync
decision: >
  The queue is a role on one appointed group member H, not an outside
  service. H uses that member’s X-Wing/ML-DSA identity and pairwise keys.
  Committed updates (save, mkdir, membership, new manifest) MUST go writer
  → H under K_wH. H verifies manifest DSA, applies locally, fans out under
  K_Hpeer to online members, and for each offline member encrypts the
  updated file under K_Hoffline and appends it to that mailbox as the
  change happens (net-vault-join-directory.md). Fan-out to online members
  is GCM-only control; writer authorship is the signed manifest. Online
  members pull chunk bodies (encrypt-at-send). Offline mailboxes contain
  control packets AND encrypt-at-send chunk bodies for those updates. H
  keeps a full plaintext replica per hosted vault. Threat: compromising H
  compromises all shared plaintext. H is TCB for all shared files; each
  other member is TCB for files they hold. Mailbox blobs are
  { recipient_id, sender_id, epoch, seq, queued_at, ciphertext } with
  ciphertext a pairwise packet. Flush is ordered; apply is idempotent; only the recipient may
  flush, via ML-DSA over a challenge (ctx qfs/v1/id is not used; challenge
  M is 32 random bytes from H, signed under the recipient vk with ctx
  qfs/v1/flush). Presence (online iff live heartbeat/TTL) is answered by H.
  Live cursors MUST go direct pairwise, not through H. If H is down: online
  peers may mesh on data they already hold; new commits and offline
  mailboxes wait. v1 does not elect a successor.
why:
- Local member host can store a replica and re-encrypt catch-up
- One send to H avoids wrapping per offline peer
- Availability-only notes hid that H is a confidentiality TCB
rejected:
- Non-member opaque mailbox; hub for live cursors; auto-failover of H in v1
- Mailbox holds control only, never chunk bodies — blocks offline file push
  (net-vault-join-directory.md)
