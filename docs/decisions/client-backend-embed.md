# Client embeds the backend library; hosts expose a token-gated admin port
status: accepted
date: 2026-09-12      scope: client
decision: >
  The desktop app links `backend/` (crate `quantam_fs`) and runs one member node per joined vault
  inside the app process, on one dedicated OS thread with a Tokio current-thread runtime and LocalSet.
  Each vault membership has its own identity and data dir under the app data directory
  (`vaults/<vault_id_hex>/`), so one client may belong to several vaults on the same host without
  same-peer multiplexing; the bridge presents all of its own identities to the UI as one `me.peerId`.
  Servers stay `qfsd` processes: one directory ("central server") and N hosts ("orchestration servers").
  A host additionally serves a line-oriented admin protocol on `--admin-addr`, authenticated by a
  random token it prints as one copy-pasteable connect string `IP:PORT/TOKEN`; the app's "Add server"
  takes that string. Admin commands are STATUS (vaults, members, presence, quota/usage, directory
  address, current join codes), CREATE_VAULT, KICK, ROTATE_CODE, FORGET_VAULT. Nothing about the
  peer handshake, admission, control log, manifests, or chunk crypto changes. Per-node cosmetics the
  protocol has no field for (folder color, advisory access lists, vault name/description, creator)
  live in one replicated JSON file at the vault root, `.qfs-meta.json`, hidden from listings.
  Tree change notification is the member's own 100 ms heartbeat: the bridge diffs the replica after
  every tick and emits whole-node `fs-changed` deltas. File bytes are pulled on open (host first, then
  member holders) and small files are prefetched in the background; opening hands an assembled copy
  under the app data dir to the OS default application.
why:
- The daemon has no IPC and cannot be stopped programmatically; the library API (`join_host`, `JoinedPeer`, `VaultHost`) is the documented embedding path (`backend/examples/vm_probe.rs`).
- `net-multi-vault.md` binds one identity to one vault per host; per-vault identities keep that decision intact.
- A separate admin listener keeps frame kinds 1–14 and the sealed channel untouched (`net-tcp-admission.md`).
- Presence is host-local by design (`sync-host-tcb.md`); reading it over the admin port needs no new sealed control kind.
rejected:
- Spawning `qfsd` as a sidecar per vault — no command surface, exclusive identity lock, extra process supervision, slower events.
- New sealed control kinds for kick/create/presence — touches the replicated log, bootstrap snapshot and encoding; too risky for the time available.
- Live cursors through H — forbidden by `sync-host-tcb.md`; member-to-member pairs are not orchestrated by the backend yet, so cursors stay off until that exists.
