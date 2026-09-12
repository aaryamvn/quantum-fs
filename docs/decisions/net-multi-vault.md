# One host identity with separate vault replicas
status: accepted
date: 2026-09-12      scope: net
decision: >
  Host multiple vaults on one listener and KeyStore identity. Place each vault at
  vaults/<64 lowercase hex vault_id>/{vault,replica.bin,chunks}; retain identity,
  keys and the sole advisory lock at the data directory root. Each replica root
  remains FileId(vault_id.0). A member process joins one vault.
  Dispatch the encrypted Join by vault_id and bind each live peer session to one
  vault. Reject another-vault Join for a peer with an existing binding using a
  distinct vault-session-conflict error; preserve that pair, mailbox and binding.
  Failed admission discards only a pair created by that handshake, never a
  pre-existing valid pair. Return conflicts as encrypted network control kind 6
  (bound VaultId then requested VaultId), separate from logged control kinds.
  A generic authenticated admission denial uses encrypted network control kind 7
  with no payload; EOF/timeouts cannot prove refusal and retain pending wraps.
  Only an authenticated denial discards a new joiner's provisional pair, avoiding
  nonce-counter reuse after a lost WrapAck. Preserve TCP frame types 1–14.
  A confirmed live pair uses
  only its cached wrap during another Join; it cannot be rotated before admission
  resolves. Restart-prepared epochs remain eligible for ordinary coordination.
  Creating another vault retains existing vaults and publishes only the new code.
  Refresh mailboxes for every vault after ordinary pair rotation.
  Migrate a legacy layout only with a durable .migrate-vaults marker containing
  raw vault_id and phase, written before moves. Rename on the same filesystem,
  syncing both parents after each move. On restart resume the recorded migration
  when the replica verifies at its source or destination; fail closed on corrupt
  metadata. Do not infer completion from vaults/ existing. Clear the marker only
  after all three moves and directory syncs complete. A retained marker therefore
  always permits idempotent resume, including death after chunks moved.
why:
- Peer-keyed pair keys cannot safely multiplex the same peer across vaults yet.
- A durable migration marker prevents an incomplete move from looking like an empty vault.
rejected:
- Per-vault identities or locks, copying vault plaintext, and guessing migration from directories.
