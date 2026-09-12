# Backend daemon scaffold
area: backend      status: complete      opened: 2026-09-12      by: Justin
prompt: |
  You are scaffolding the backend daemon only. Language: Rust. Do not implement X-Wing, ML-KEM, ML-DSA, AES-GCM, HKDF, or any real key exchange yet.
  
  Follow docs/agents/PROTOCOL.md in full. Stay in backend/ plus your task/decision files. Do not touch client/. Create docs/tasks/active/backend-260912-daemon-scaffold.md from docs/agents/templates/task.md; put this prompt in prompt:. Checkpoint before each step. If you need a runtime choice (e.g. tokio), record it in docs/decisions/backend-runtime.md; do not silently violate accepted docs/decisions/*.md.
  
  READ FIRST (accepted only — do not implement superseded files):
  - docs/decisions/backend-stack-xwing.md
  - docs/decisions/crypto-encoding.md
  - docs/decisions/crypto-pq-hybrid.md
  - docs/decisions/crypto-identity-selfcert.md
  - docs/decisions/crypto-pairwise-aead.md
  - docs/decisions/sync-plaintext-chunks.md
  - docs/decisions/sync-host-tcb.md
  - backend/README.md
  - docs/VISION.md (product constraints only)
  
  Skip (superseded; status superseded-by only): crypto-pq-suite.md, crypto-pairwise-aes.md, crypto-identity-bind.md, sync-encrypt-at-send.md, sync-host-queue.md, backend-stack.md.
  
  GOAL: A compiling Rust crate + long-running daemon binary whose types, modules, constants, encodings, and traits match the accepted decisions, so a later agent can fill in X-Wing wrap / ML-DSA / AES-GCM without renaming the world.
  
  REQUIREMENTS:
  1. One crate in backend/ (binary + lib). Binary name is the daemon, e.g. qfsd. Same binary for every member; host is a config role on that process (appointed H in sync-host-tcb.md), not a second program. Comment in host code: compromising H compromises all shared plaintext; H is TCB for all shared files.
  2. Modules exactly as backend-stack-xwing.md: crypto/{identity,wrap,aead,sign}, protocol/{packet,manifest,pull}, store/chunks, sync/{pull,host}. Add config, error, ids, encoding, keystore, daemon entry (load config, load/create identity path, graceful shutdown). CLI/config: data dir, listen addr, peer identity path, host_id.
  3. Newtypes and wire-shaped structs now, even if crypto is stubbed:
     - PeerId: [u8; 32], Ord/Eq as unsigned byte compare (Construction B min/max and dir_bit)
     - Epoch(u64), Seq(u64), FileId([u8; 32]), ChunkId([u8; 32])
     - Protocol version: u8 = 1
     - Identity document: (peer_id, ek, vk, created_at: u64 Unix seconds) + signature bytes. peer_id is defined as SHA-256(ASCII qfs/v1/peer || vk_bytes); implement that hash now (no ML-DSA yet). ek is “X-Wing public key bytes” (opaque Vec<u8> until the crypto agent).
     - Wrap message: (kem_ct, wrap_ct, epoch, min_id, max_id) + signature bytes
     - PairSession: (peer_id, epoch) + opaque key handle. One AES plane only: that handle is K_ab for packets and file bytes.
     - Packet header: version, sender_id, receiver_id, epoch, seq. No per-packet DSA field.
     - Manifest: file_id, ordered chunk_ids, size, writer_id, version + signature bytes
     - Mailbox envelope: recipient_id, sender_id, epoch, seq, queued_at, ciphertext
     - Flush challenge: 32-byte challenge from H (M for ctx qfs/v1/flush)
     - PullRequest with a hard cap of 32 chunk_ids (constant)
     - Replay window state shape per (peer, epoch, direction, type), W=1024 (logic can be stubbed; types and constant now)
  4. Encoding lives in one module and is the only byte layout (crypto-encoding.md). One serialize function per object; those bytes are both AAD and later DSA M. No hex/Debug of ids. Implement now:
     - Wrap HKDF info: ASCII qfs/v1/wrap/pair/ || min_id || 0x3a || max_id || 0x2f || u64be(epoch), min_id < max_id as unsigned 32-byte strings
     - Packet AAD: u8 version || sender_id || receiver_id || u64be(epoch) || u64be(seq)
     - Chunk AAD: packet AAD || file_id || u64be(index)
     - Wrap GCM AAD: min_id || max_id || u64be(epoch)
     - Identity / wrap / manifest / flush M as specified in crypto-encoding.md
     - Nonce builder (96 bits): type_bit (0=packet, 1=chunk body) || dir_bit (1 iff sender_id > receiver_id) || 30 zero bits || u64be(counter). seq in AAD is that counter.
     - Wrap GCM nonce constant: 12 zero bytes (do not AES-GCM yet)
     - FIPS 204 ctx constants only: qfs/v1/id, qfs/v1/wrap, qfs/v1/manifest, qfs/v1/flush. There is no qfs/v1/pkt.
     - chunk_id = SHA-256(file_id || u64be(index) || plaintext)  (hashing plaintext is allowed; encrypting it is not yet)
  5. crypto/* must be traits + Result stubs that return a typed error (e.g. NotImplemented). No unwrap/expect on those paths. Do not call unimplemented crypto from main in a way that crashes boot: daemon starts, logs “crypto pending”, waits on shutdown. Trait names should match the later work: X-Wing encaps/decaps (not raw ML-KEM), Pure ML-DSA with ctx, AES-256-GCM, Construction B wrap (generated K_ab, HKDF wrap_key, single-use wrap_key, retry = same ciphertext). Protocol code must never be sketched as calling raw ML-KEM Encaps.
  6. store/chunks: put/get/has for plaintext chunks + have-bitset API. Empty in-memory or on-disk is fine. Do not store per-peer ciphertext. Do not store a random nonce beside chunk bodies. Comment: v1 member is TCB for plaintext they store; zeroize is for keys, not file bytes.
  7. sync/host: types and method signatures for presence TTL, ordered mailbox append/flush (flush auth is ML-DSA over the 32-byte challenge), fan-out of control not chunk bodies. Bodies empty/stub. Live cursor messages are not hosted here (comment: direct pairwise GCM, no DSA, not through H).
  8. Tests that compile today: PeerId unsigned ordering; peer_id hash from a fake vk; wrap HKDF info bytes (raw 32-byte ids, not UTF-8 interpolation); packet and chunk AAD encodings; wrap AAD; nonce bit layout; PullRequest rejects >32 ids; chunk_id with known file_id/index/plaintext; daemon --help or config parse. Do not add tests that require real X-Wing/ML-DSA/AEAD.
  9. Update backend/README.md Run and Test with the exact commands. List what is scaffold vs not implemented. Cargo.toml may list x-wing, ml-dsa, aes-gcm, hkdf, sha2, zeroize, rand unused, or omit them until the crypto agent — either is fine; do not implement them. If ml-kem appears, it must only be as an x-wing dependency, never used directly.
  
  DO NOT:
  - Implement X-Wing Encaps/Decaps, ML-DSA sign/verify, AES-GCM, HKDF-derived wrap_key, persist of real K_ab bytes
  - Implement ephemeral KEM per epoch (v1 uses static ek; rotation ≠ forward secrecy — a comment is enough)
  - Implement pull orchestration, host fan-out logic, network protocol, client GUI
  - A second AES key for files vs packets
  - Ciphertext-hashed chunk ids; random chunk nonces; per-packet DSA; sign prefixes qfs/v1/pkt
  - A non-member queue server or a second host binary
  - git add -A / commit unless I ask
  - Follow superseded decision files if they contradict the accepted ones
  
  VERIFY: cargo test in backend/ passes; cargo build produces the daemon; README Run/Test are accurate.
  
  REPORT: CHANGED paths; VERIFIED command output; README: what the daemon can do now vs what later agents still owe (X-Wing Construction B wrap, ML-DSA identity/wrap/manifest/flush, AES-GCM + sliding window, encrypt-at-send pull, host mailbox behavior).

## Plan
- [x] Establish accepted decisions and record runtime; scaffold crate and interfaces.
- [x] Implement specified encodings, plaintext storage, daemon lifecycle, and tests.
- [x] Verify build/tests and daemon startup/shutdown; update README and hand-off.

## Checkpoint   (overwrite in place · ≤10 lines · write BEFORE starting the next step)
done:       cargo test (19 passed), build, fmt --check, clippy -D warnings, daemon help/SIGINT/SIGTERM, final decision audit passed.
in-flight:  None.
next:       Scaffold complete; future crypto/sync work starts a separate task. Moved to done without staging or commit.
open:       Toolchain: RUSTUP_HOME=/private/tmp/qfs-rust-toolchain.KiGtuj/rustup CARGO_HOME=/private/tmp/qfs-rust-toolchain.KiGtuj/cargo PATH=/private/tmp/qfs-rust-toolchain.KiGtuj/cargo/bin:$PATH

## Outcome   (fill at completion · ≤10 lines · facts only)
changed:    backend/{.gitignore,Cargo.toml,Cargo.lock,README.md,src/,tests/}; docs/decisions/backend-runtime.md; this task.
verified:   cargo test → 19 passed, 0 failed; cargo build → Finished dev; cargo fmt --check → exit 0; cargo clippy --all-targets -- -D warnings → exit 0.
verified:   qfsd --help → exit 0; process smoke test → crypto pending, stays running, SIGINT/SIGTERM exit 0 with shutdown complete, existing identity preserved.
verified:   git diff --check → exit 0; read-only audit against seven accepted decisions → no mismatches found.
not-done:   X-Wing Construction B/HKDF wrap, ML-DSA identity/wrap/manifest/flush, AES-GCM/sliding window, real key persistence, encrypt-at-send pull, host behavior, networking (requested exclusions).
gotchas:    Identity path is empty placeholder or existing unverified bytes; no keys/network listener. --host-id reads exactly 32 raw bytes from a file. No client edits, staging, or commit.
gotchas:    Initial fetch failed restricted DNS; authorized fetch succeeded. Verified with isolated Rust 1.98.1; no global/profile changes. Verbatim prompt exceeds usual line budget.
