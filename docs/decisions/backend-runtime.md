# Backend daemon runtime
status: accepted
date: 2026-09-12      scope: backend
decision: >
  Use Tokio 1 with a current-thread runtime for the single qfsd process;
  enable macros, rt, and signal for startup and graceful SIGINT/SIGTERM
  shutdown. Parse CLI configuration with clap 4. The appointed host H
  remains a role in this same process, selected by configured host_id
  matching the authenticated local peer_id once identity crypto exists.
  The scaffold creates an identity-path placeholder, never fake keys or a
  valid identity, and starts in crypto-pending state without opening a
  network listener. Add I/O capabilities when their protocols are implemented.
why:
- The long-running daemon needs portable shutdown and a future async I/O entry point.
- A pending identity lets the daemon boot before real key generation is implemented.
rejected:
- A second host service or binary — H is an appointed group member.
- Generating placeholder cryptographic keys — they could be mistaken for real credentials.
