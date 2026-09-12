# Backend language and crypto libraries
status: accepted
date: 2026-09-12      scope: backend
decision: >
  The protocol core is Rust, one crate in backend/ until compile time forces
  a split. Use RustCrypto x-wing (X-Wing KEM), ml-dsa, aes-gcm, hkdf, sha2,
  plus zeroize and rand. ml-kem may appear only as an x-wing dependency;
  protocol code must not call raw ML-KEM Encaps. No unwrap/expect on crypto,
  wrap, packet, or pull paths. liboqs / pqcrypto FFI only if those crates
  block; record that in a superseding decision. Layout when implemented:
  crypto (identity, wrap, aead, sign), protocol (packet, manifest, pull),
  store (chunks), sync (pull, host presence/mailbox/fan-out).
why:
- Human specified Rust; KEM is X-Wing per crypto-pq-hybrid.md
rejected:
- Multiple crates from day one — extra ceremony for an empty tree
- liboqs as default — FFI surface; last resort
- Direct ml-kem Encaps in protocol code — would skip the hybrid floor
