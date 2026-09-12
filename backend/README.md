# backend
Protocol core: peers, sync, storage, crypto, offline queue. Owner: teammate. Rules: `docs/agents/PROTOCOL.md`.

Stack: Rust — `docs/decisions/backend-stack-xwing.md`.
Binding crypto/sync: `docs/decisions/crypto-encoding.md`, `crypto-pq-hybrid.md`, `crypto-identity-selfcert.md`, `crypto-pairwise-aead.md`, `sync-plaintext-chunks.md`, `sync-host-tcb.md`.

One crate: library `quantam_fs` and daemon `qfsd`. Runtime:
[`backend-runtime.md`](../docs/decisions/backend-runtime.md).

## Run

Requires Rust 1.89 or newer with Cargo. From the repository root:

```sh
cd backend
cargo build
./target/debug/qfsd --help
./target/debug/qfsd --data-dir .qfs --listen-addr 127.0.0.1:7447 --peer-identity-path identity
```

The daemon creates or verifies a real identity with independently generated
X-Wing and ML-DSA-65 keys, logs `identity verified; crypto ready`, and waits until
Ctrl-C or SIGTERM (Unix) for graceful shutdown. Empty scaffold placeholders are
upgraded; invalid nonempty identities are rejected without replacement. Relative
identity paths resolve under `--data-dir`; absolute paths are used directly.
The configured listen address is parsed; no network socket is opened yet.

The identity file contains private key seeds and the signed public document.
Sibling `.keys` and `.lock` files hold verified peers/pair state and an exclusive
process lock. Files use owner-only permissions on Unix; keys are not encrypted
at rest. See [`crypto-keystore.md`](../docs/decisions/crypto-keystore.md).
On restart, prior AES slots are discarded and fresh signed wraps are prepared
for established pairs. The daemon also prepares new epochs weekly. Networking
will deliver these through `KeyStore::pending_wraps()`; current startup sends nothing.

Optional `--host-id /path/to/host-id.bin` reads exactly 32 raw bytes identifying
appointed member H. Host selection uses the same binary and identity as every
member, selected by the verified local identity. Host behavior remains pending.
IDs have no text or hex wire representation.

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
encodings and signing payloads, real KEM/signature/AEAD round trips, identity
tampering, context separation, wrap retries and epoch collisions, restart/key
retirement, replay windows and nonce domains, the 32-chunk pull cap, plaintext
storage/have-bitset, and CLI/identity persistence. X-Wing uses implicit rejection:
a wrong dk yields a different shared secret and fails wrap GCM authentication.

## Layout and implementation boundary

- `src/{config,daemon,keystore,error,ids,encoding}.rs`: identity loading,
  startup/shutdown, host selection, opaque durable key slots, canonical bytes,
  bounded local persistence decoding, errors/newtypes, and SHA-256.
- `src/crypto/{identity,wrap,aead,sign}.rs`: verified self-certifying identities,
  Pure ML-DSA-65 signatures with identity/wrap/manifest/flush contexts, X-Wing
  Construction B, single-use HKDF-SHA256 wrap keys, and AES-256-GCM. One opaque
  K_ab handle serves packets and file bytes. Secret buffers and retired slots
  zeroize; imported EK rotation prepares a fresh wrap for the same principal.
- `src/protocol/{packet,manifest,pull}.rs`: headers, manifests, bounded pull
  requests, and per-peer/epoch/direction/type sliding-window acceptance (W=1024).
  Authentication must succeed before a receive counter is marked accepted.
- `src/store/chunks.rs`: in-memory plaintext put/get/has and have-bitset; no
  durable storage, per-peer ciphertext replicas, or stored random chunk nonce.
- `src/sync/{pull,host}.rs`: stub pull and appointed-host presence TTL, ordered
  mailbox/challenge/flush, and control fan-out interfaces. H is TCB for all shared
  plaintext; live cursors go direct pairwise GCM, without DSA or H.

Crypto callers import verified peer documents before creating wraps. First
contact starts at epoch 1 from the smaller PeerId; later creation requires the
next epoch. `retry()` returns the cached ciphertext. A simultaneous-wrap loser
uses `retry_collision()` at the epoch before collision plus two; the winner's
key remains available for in-flight data. Retire old session handles
with `KeyStore::retire()` after in-flight work drains. AES send counters must
strictly increase per type; use the same header for canonical AAD and nonce.

Still left: encrypt-at-send pull validation/orchestration; host presence,
ordered mailbox/challenge/flush behavior and control fan-out; networking and
delivery/acknowledgment of wraps; durable chunk storage. Static ek rotation does
not provide forward secrecy. There is no per-packet DSA or second content key.
