# Vaults, orchestration servers, join codes, central directory
status: accepted
date: 2026-09-12      scope: net
decision: >
  A "vault" is what VISION.md calls a network: one shared file system with a member list.
  Vaults are hosted by an "orchestration server" — the designated host H of sync-host-tcb — which
  may host several vaults. Per vault, H stores only metadata: the member list and the per-member
  offline queue. On a member's reconnect, H immediately pushes that member's queued instructions
  (adds, clears, removals) and the client executes them before anything else.
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
rejected:
- MAC address as identity — spoofable/unstable (above); also useless for encryption keying.
- Central server holding member lists or queues — makes it a subpoena/leak target; keep it a lookup table.
open:
- A leaked join code grants entry until rotated; consider requiring approval by H or any member.
- Queued "clear" on reconnect cannot stop a malicious member copying files while offline (known limit of kick).
