# Client profile: first-launch name screen, machine-bound client id, admin = host, creator or token holder
status: accepted
date: 2026-09-12      scope: client
decision: >
  The app keeps one client-wide profile in `<data dir>/profile.json` (`name`, `nameSet`, `color`,
  `contributionBytes`). Until `nameSet` is true the app shows an onboarding screen after the splash —
  the wordmark, "Hey there, what is your name?", one field and Next — then slides into the home list.
  The name is saved locally, written into every joined vault's `.qfs-meta.json` member entry, and
  mirrored to the directory's profile port under the machine-derived client id (net-directory-profile-port.md);
  a fresh install with no local name asks the directory once before onboarding. A vault's admin is the
  host, the sidecar's `createdBy` peer, or any client holding that server's admin token; only admins
  can remove members or delete the vault, and the client refuses those calls for everyone else.
  Kicked clients remove the vault on the first authenticated admission denial that a STATUS probe
  confirms, or on the second consecutive denial when no probe is possible, and announce it as
  `You were removed from "<vault>"` before returning home.
why:
- The human asked for a name typed once, tied to the computer, stored centrally, and for onboarding to replace the home screen on first launch.
- The connect-string token is the server's administrative credential; whoever pasted it administers its vaults, which is what makes seeded vaults deletable.
- AdmissionDenied is host-authenticated; requiring an admin-port confirmation left an offline-kicked member retrying forever when its admin port was unreachable.
rejected:
- Per-vault names — the human wants one name per computer.
- Deriving identity from MAC/hardware for the protocol — still forbidden (net-vault-join-directory.md); the machine id only keys the cosmetic name.
