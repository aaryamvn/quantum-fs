# Local identity and pair key persistence
status: accepted
date: 2026-09-12      scope: crypto
decision: >
  Keep the identity's independently random X-Wing and ML-DSA seeds in a
  local private file, together with the canonical signed identity. Store
  verified peers, pair epoch watermarks, canonical wraps, and K_ab slots
  in a sibling private state file. Use fixed-width big-endian fields and
  u32-length-prefixed records in encoding.rs; reuse canonical object bytes.
  Protect local files with owner-only permissions and exclusive process
  locking; atomically replace and sync state before returning a new session.
  Runtime handles bind to a keystore instance, not reusable slot numbers alone.
  On restart remove prior K_ab slots from active use, retain epoch watermarks,
  and prepare fresh signed wraps for known established pairs before any send.
  Keep old live-process epochs until explicitly retired after in-flight drain.
why:
- Durable epoch watermarks prevent restart reuse of K_ab and GCM counters.
- Opaque revocable slots let packets and chunk bodies use one key without exporting it.
- Private local files are the v1 keystore boundary; at-rest key encryption is not specified.
rejected:
- Reloading active keys and reset counters — reuses GCM nonces.
- Persisting KEM shared secrets or HKDF wrap keys — Construction B requires single use.
