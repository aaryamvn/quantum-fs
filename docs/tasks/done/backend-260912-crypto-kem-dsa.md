# Backend identity, KEM wrap, and AEAD
area: backend      status: complete      opened: 2026-09-12      by: Justin
prompt: |
  You are continuing the backend, not scaffolding. Language: Rust. Follow docs/agents/PROTOCOL.md in full. Stay in backend/plus your task/decision files. Do not touch client/.
  
  Create docs/tasks/active/backend-260912-crypto-kem-dsa.md from docs/agents/templates/task.md; prompt: this message verbatim. Checkpoint before each step.
  
  READ FIRST (accepted only — do not implement superseded files):
  - backend/README.md
  - backend/src/crypto/{identity,wrap,aead,sign}.rs
  - backend/src/keystore.rs
  - backend/src/encoding.rs
  - docs/decisions/crypto-encoding.md
  - docs/decisions/crypto-pq-hybrid.md
  - docs/decisions/crypto-identity-selfcert.md
  - docs/decisions/crypto-pairwise-aead.md
  - docs/decisions/backend-stack-xwing.md
  
  Skip (superseded): crypto-pq-suite.md, crypto-pairwise-aes.md, crypto-identity-bind.md, backend-stack.md.
  
  GOAL: Implement identity, Construction B pairwise wrap, and AES-256-GCM on K_ab. Fill the existing traits. Do not rename modules, PeerId, encodings, ctx strings, or AAD layouts.
  
  IMPLEMENT (only these):
  1. IdentityManager + PureMlDsa
     - Independent X-Wing (ek, dk) and ML-DSA-65 (vk, sk); do not share seeds.
     - peer_id := SHA-256(ASCII qfs/v1/peer || vk_bytes) using encoding::peer_id.
     - Identity document signed with FIPS 204 ctx qfs/v1/id; M = encoding identity bytes.
     - Refuse ek unless signature verifies under vk AND peer_id equals that hash.
     - ek may rotate under the same vk; rotating vk is a new principal.
     - Daemon load_or_create: generate or load a real identity; still no network socket.
  2. XWing + ConstructionBWrap
     - Encaps/Decaps are X-Wing only. Protocol code must not call raw ML-KEM Encaps.
     - Initiator generates 32 random bytes K_ab; wrap_key = HKDF-SHA256(ikm=ss, salt=empty, info=encoding wrap HKDF info, L=32).
     - wrap_ct = AES-256-GCM(wrap_key, nonce=12 zero bytes, AAD=wrap AAD, pt=K_ab).
     - wrap_key is single-use: never persist; one Encaps per wrap; retry() returns the same ciphertext.
     - Sign wrap M with ctx qfs/v1/wrap. First contact: smaller PeerId initiates. Later either peer may initiate; epoch strictly increases. Duplicate epoch: keep smaller initiator, other retries at last+2. Process start: new epoch per known pair (do not reuse K_ab or counters).
     - Persist K_ab in the keystore as opaque PairKeyHandle slots; zeroize ss, wrap_key, dk, sk, retired K_ab.
     - Static ek: rotation is not forward secrecy. Do not implement ephemeral KEM per epoch.
  3. Aes256Gcm on that same K_ab (packets and chunk bodies; one AES plane).
     - Nonce from encoding: type_bit || dir_bit || 30 zero bits || u64be(counter). seq in AAD is that counter.
     - Implement sliding-window acceptance W=1024 per (peer, epoch, direction, type) as specified in crypto-pairwise-aead.md.
  4. Cargo.toml: x-wing, ml-dsa, aes-gcm, hkdf, zeroize, rand. ml-kem only as an x-wing dependency, never used directly. No unwrap/expect on crypto/wrap/packet/pull paths.
  
  TESTS:
  - KEM round-trip; wrong dk fails.
  - Swapped ek / wrong peer_id fails identity bind.
  - Two peers agree on K_ab; wrap_key ≠ K_ab; retry() does not Encaps again.
  - Directional nonce unique; replay in-window rejected; seq beyond H−W rejected; holes in the window accepted.
  - Epoch rotate; wrap nonce remains 12 zeros.
  - Manifest/flush sign+verify with the right ctx (pkt ctx must not exist).
  
  DO NOT:
  - Re-scaffold, rename traits, or invent a second encoding.
  - Pull orchestration, host mailbox/fan-out, network listen, client/.
  - A second content AES key, double AEAD, ciphertext-hashed chunk ids, per-packet DSA, qfs/v1/pkt.
  - Ephemeral ek per epoch.
  - git add -A / commit unless I ask.
  
  VERIFY: cd backend && cargo test && cargo build && cargo clippy --all-targets -- -D warnings
  
  At completion tell me:
  1. What is done (identity, wrap, AEAD, signatures, window, tests).
  2. Update backend/README.md so “done vs left” is accurate. Expect still left: encrypt-at-send pull, host presence/mailbox/fan-out, networking.

## Plan
- [x] Verify library APIs and define keystore/provider integration without changing canonical layouts.
- [x] Implement identity/signatures, X-Wing Construction B, durable key slots/epochs, and AEAD/window.
- [x] Test adversarial cases, daemon restart, build/lint; update README and complete hand-off.

## Checkpoint   (overwrite in place · ≤10 lines · write BEFORE starting the next step)
done:       47 tests passed; cargo build, strict Clippy, fmt, daemon signals/reload, dependency tree, and final read-only audit passed.
in-flight:  None.
next:       Complete and moved to done without staging/commit; next sync/network work starts a separate task.
open:       Verification used RUSTUP_HOME=/private/tmp/qfs-rust-toolchain.KiGtuj/rustup CARGO_HOME=/private/tmp/qfs-rust-toolchain.KiGtuj/cargo PATH=/private/tmp/qfs-rust-toolchain.KiGtuj/cargo/bin:$PATH

## Outcome   (fill at completion · ≤10 lines · facts only)
changed:    backend/{Cargo.toml,Cargo.lock,README.md,src/{config,daemon,encoding,error,keystore}.rs,src/crypto/*.rs,src/protocol/packet.rs,tests/{daemon,aead_window,identity_sign,wrap_keystore}.rs}; docs/decisions/crypto-keystore.md; this task.
verified:   cargo test → 47 passed, 0 failed; cargo build → Finished dev; cargo clippy --all-targets -- -D warnings → exit 0; cargo fmt --check → exit 0; git diff --check → exit 0.
verified:   qfsd --help → exit 0; process smoke → real identity generated/reloaded unchanged, 0600 permissions, SIGINT/SIGTERM exit 0; no listener.
verified:   cargo tree -i ml-kem → ml-kem 0.3.2 only through x-wing 0.1.0; final crypto audit → no remaining concrete defect.
not-done:   Encrypt-at-send pull, host presence/mailbox/fan-out, networking/wrap delivery and acknowledgments, durable chunk storage remain outside this task.
gotchas:    X-Wing wrong dk implicitly rejects to a different ss; wrap GCM authentication fails. Collision loser uses retry_collision at previous epoch+2. Old live epochs require explicit retirement after drain.
gotchas:    Private local seed/slot files are not encrypted at rest; restart discards prior AES slots and prepares fresh epochs. Peer ek rotation replaces pending wraps addressed to old ek. Canonical wire encodings and trait names unchanged.
gotchas:    No client edits, staging, or commit. A test-only Clippy reference warning was corrected. Verbatim prompt exceeds usual task line budget.
