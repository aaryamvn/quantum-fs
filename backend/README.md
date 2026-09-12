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
member, selected by the verified local identity. A matching identity starts an
in-memory `HostService`; weekly rotation refreshes its queued ciphertext. The
standalone daemon currently initializes a singleton vault containing H. Tests
and library callers supply the vault member set and drive commits in-process;
there is no network or CLI transfer interface. IDs have no text or hex wire
representation.

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
Transfer tests cover two holders, encrypted pull requests, missing/idempotent
chunks, tampering and manifest membership, offline coalescing, delete/recreate
instructions, epoch re-sealing, keystore restart with a retained memory replica,
flush authentication, more than 1024 queued bodies, and atomic failed-drain retry.

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
- `src/store/chunks.rs`: in-memory plaintext put/get/has and have-bitset, retaining
  file/index metadata for canonical chunk AAD. No durable chunk storage.
- `src/sync/pull.rs`: in-process bounded pull, encrypted control requests and one
  AES-GCM chunk body per response. Holders skip missing IDs. Acceptance uses a
  pre-trusted manifest from H, checks GCM and the plaintext chunk hash, and writes
  idempotently; a failed response batch writes no chunks.
- `src/sync/host.rs`: vault membership, heartbeat/TTL presence, member-writer
  signature verification, online control-only fan-out, and immediate offline
  chunk encryption. Bodies coalesce by file/index; all control instructions
  remain ordered. Recipient-only ML-DSA challenge authentication drains the queue.
  H is TCB for all shared plaintext; live cursors go direct pairwise GCM,
  without DSA or H. A stopped host rejects commits.

Crypto callers import verified peer documents before creating wraps. First
contact starts at epoch 1 from the smaller PeerId; later creation requires the
next epoch. `retry()` returns the cached ciphertext. A simultaneous-wrap loser
uses `retry_collision()` at the epoch before collision plus two; the winner's
key remains available for in-flight data. Retire old session handles
with `KeyStore::retire()` after in-flight work drains. AES send counters must
strictly increase per type; use the same header for canonical AAD and nonce.

In-process callers establish the live pair wrap before `flush_mailbox`. Stale
mailbox controls and bodies are re-sealed under the current epoch; chunk counters
begin at 1 on its fresh key. Flush authenticates frames in queued order, stages
the complete instruction log, then writes bodies matching the final manifests.
Live pull/control entry points remain gated until flush succeeds. Exact receipts
allow retry after a later corrupt frame without reopening accepted counters.
Both pull and host use the same encrypt-at-send helper and the existing packet
and chunk counter domains on K_ab.

`HostService::into_state` / `resume` support a memory-state hand-off across a
keystore reopen without retaining old keys. Mailboxes and plaintext are volatile:
an actual process exit loses them. Durable recovery is not implemented.

Still left: networking and wrap delivery/acknowledgment, join-code directory,
durable chunk/mailbox storage, and client GUI integration. Static ek rotation
does not provide forward secrecy. There is no per-packet DSA, second content key,
per-peer ciphertext chunk replica, or random stored chunk nonce.
