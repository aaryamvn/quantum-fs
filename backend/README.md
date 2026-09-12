# backend
Protocol core: peers, sync, storage, crypto, offline queue. Owner: teammate. Rules: `docs/agents/PROTOCOL.md`.

Stack: Rust — `docs/decisions/backend-stack-xwing.md`.
Binding crypto/sync: `docs/decisions/crypto-encoding.md`, `crypto-pq-hybrid.md`, `crypto-identity-selfcert.md`, `crypto-pairwise-aead.md`, `sync-plaintext-chunks.md`, `sync-host-tcb.md`.

One crate: library `quantam_fs` and daemon `qfsd`. Runtime:
[`backend-runtime.md`](../docs/decisions/backend-runtime.md).

## Run

Requires a stable Rust toolchain with Cargo. From the repository root:

```sh
cd backend
cargo build
./target/debug/qfsd --help
./target/debug/qfsd --data-dir .qfs --listen-addr 127.0.0.1:7447 --peer-identity-path identity
```

The daemon creates the data directory and an empty identity placeholder, logs
`crypto pending`, and waits until Ctrl-C or SIGTERM (Unix) for graceful shutdown.
Existing identity bytes are preserved and marked unverified. Relative identity
paths resolve under `--data-dir`; absolute paths are used directly. The configured
listen address is parsed but no network socket is opened yet.

Optional `--host-id /path/to/host-id.bin` reads exactly 32 raw bytes identifying
appointed member H. Host selection uses the same binary and identity as every
member; activation awaits authenticated local identity loading. IDs have no text
or hex wire representation. The scaffold creates no public/private keys or K_ab.

## Test

From the repository root:

```sh
cd backend
cargo test
cargo build
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

Tests cover unsigned peer ordering, SHA-256 identity/chunk vectors, canonical
encodings and signing payloads, nonce bits, the 32-chunk pull cap, plaintext
storage/have-bitset, CLI parsing/help, and identity-path preservation.

## Layout and implementation boundary

- `src/{config,daemon,keystore,error,ids,encoding}.rs`: startup/shutdown, pending
  identity path, host selection, errors/newtypes, canonical bytes and SHA-256.
- `src/crypto/{identity,wrap,aead,sign}.rs`: wire-shaped identity/wrap/session
  types and typed `NotImplemented` traits. One opaque K_ab handle serves both
  packets and file bytes; no real cryptographic operation is implemented.
- `src/protocol/{packet,manifest,pull}.rs`: headers, manifests, bounded pull
  requests, and replay-window state (W=1024; acceptance logic pending).
- `src/store/chunks.rs`: in-memory plaintext put/get/has and have-bitset; no
  durable storage, per-peer ciphertext replicas, or stored random chunk nonce.
- `src/sync/{pull,host}.rs`: stub pull and appointed-host presence TTL, ordered
  mailbox/challenge/flush, and control fan-out interfaces. H is TCB for all shared
  plaintext; live cursors go direct pairwise GCM, without DSA or H.

Later agents still owe X-Wing Construction B wrap with generated K_ab and a
single-use HKDF-SHA256 wrap_key (retry resends the same ciphertext); Pure ML-DSA-65
identity/wrap/manifest/flush authentication; AES-256-GCM, fresh epochs/counters
and sliding-window acceptance; key generation, verification, zeroization and
persistence; encrypt-at-send pull validation/orchestration; host presence,
ordered mailbox behavior and control fan-out; and networking. Static ek rotation
does not provide forward secrecy. No client GUI or separate host program exists
in this crate.
