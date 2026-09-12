# Vaults, orchestration servers, join codes, central directory
status: accepted
date: 2026-09-12      scope: net
decision: >
  A "vault" is what VISION.md calls a network: one shared file system with a member list.
  Vaults are hosted by an "orchestration server" — the designated host H of sync-host-tcb — which
  may host several vaults. Per vault, H stores the member list, the per-member offline queue, and
  the current shared file bytes (plaintext replica) so it can encrypt-at-send to offline members.
  When a committed change happens, H immediately encrypts the updated file under that offline
  member's current-epoch pairwise key and appends those packets to that member's queue — not only
  at reconnect. On reconnect, H pushes queued instructions (adds, clears, removals) and queued
  encrypted file updates; the member applies instructions first, then decrypts and stores chunks.
  Creating a vault mints a permanent, high-entropy join code (≥128 bits, base32). The owner may
  rotate it; old codes stop working at once. ONE central directory service maps
  join_code → (orchestration server address/peer_id, vault_id) and nothing else. To join, a client
  submits the code to the directory, learns the server, and the server appends the joiner's identity
  to the vault member list.
  Members are identified exclusively by peer_id (crypto-identity-selfcert), never by MAC address,
  email, or password. Display name is chosen at onboarding and is cosmetic.
why:
- Directory holds zero file data and zero membership — only code→server; a leak reveals nothing.
- peer_id is bound to the member's signing key, so membership and queue delivery are unforgeable.
- MAC addresses are spoofable, randomized per network by modern OSes, and differ per interface.
- Offline members receive encrypted file bytes as commits happen, so catch-up is not instructions-only.
rejected:
- MAC address as identity — spoofable/unstable (above); also useless for encryption keying.
- Central server holding member lists or queues — makes it a subpoena/leak target; keep it a lookup table.
- Metadata-only H (member list + instruction queue, no file bytes) — cannot encrypt-at-send updates while a member is offline.
open:
- A leaked join code grants entry until rotated; consider requiring approval by H or any member.
- Queued "clear" on reconnect cannot stop a malicious member copying files while offline (known limit of kick).
