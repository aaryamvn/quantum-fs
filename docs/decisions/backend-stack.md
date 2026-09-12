# Backend language and crypto libraries
status: accepted
date: 2026-09-12      scope: backend
decision: >
  The protocol core is Rust, one crate in backend/ until compile time forces
  a split. Use RustCrypto ml-kem, ml-dsa, aes-gcm, hkdf, sha2, plus zeroize
  and rand. No unwrap/expect on crypto, wrap, packet, or pull paths. liboqs
  / pqcrypto FFI only if those crates block; record that in a superseding
  decision. Layout when implemented: crypto (identity, wrap, aead, sign),
  protocol (packet, manifest, pull), store (chunks), sync (pull, host
  presence/mailbox/fan-out).
why:
- Human specified Rust
- Pure-Rust FIPS-oriented crates match crypto-pq-suite.md
rejected:
- Multiple crates from day one — extra ceremony for an empty tree
- liboqs as default — FFI surface; keep as last resort
