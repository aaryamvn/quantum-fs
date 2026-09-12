# backend
Protocol core: peers, sync, storage, crypto, offline queue. Owner: teammate. Rules: `docs/agents/PROTOCOL.md`.

Stack: Rust — `docs/decisions/backend-stack-xwing.md`.
Binding crypto/sync: `docs/decisions/crypto-encoding.md`, `crypto-pq-hybrid.md`, `crypto-identity-selfcert.md`, `crypto-pairwise-aead.md`, `sync-plaintext-chunks.md`, `sync-host-tcb.md`.

One crate: library `quantam_fs` and daemon `qfsd`. Runtime:
[`backend-runtime.md`](../docs/decisions/backend-runtime.md).

## Run

Requires Rust 1.89 or newer with Cargo. Build from the repository root:

```sh
cd backend
cargo build
./target/debug/qfsd --help
```

Run these in three terminals, each with `backend/` as its working directory:

```sh
# Directory: signed ads and replay metadata only; no member identity or replica.
./target/debug/qfsd --directory --data-dir .qfs-directory --listen-addr 127.0.0.1:7440
```

```sh
# H: creates or reloads one vault and prints its Base32 join code on stderr.
./target/debug/qfsd --create-vault --data-dir .qfs-host --listen-addr 127.0.0.1:7447 --directory-addr 127.0.0.1:7440
```

```sh
# Paste the host's uppercase, unpadded Base32 code after running read.
read -r QFS_JOIN_CODE
./target/debug/qfsd --data-dir .qfs-member --listen-addr 127.0.0.1:7448 --directory-addr 127.0.0.1:7440 --join-code "$QFS_JOIN_CODE"
```

Successful admission logs `accepted vault member; pair live` on H and
`joined vault` on the member. Ctrl-C or SIGTERM shuts down gracefully.
The runtime is Tokio current-thread with `LocalSet` and `spawn_local`.
The peer handshake deadline is five seconds, idle timeout is 30 seconds,
and H permits 32 simultaneous provisional handshakes and 128 TCP connections.

The listener binds before H signs its directory ad. Port zero is supported for
binding; the resulting actual port is advertised. `--advertise-addr IP:PORT`
overrides that target for NAT/container forwarding. Targets must be numeric
`SocketAddr` values (IPv4 or bracketed IPv6), with a nonzero port and a specified
IP address; there are no hostnames or DNS. Unspecified binds such as `0.0.0.0`
need an explicit usable advertisement address.

`--peer-identity-path identity` remains the default; relative paths resolve
inside `--data-dir`. Optional `--host-id /path/to/host-id.bin` reads exactly 32
raw bytes identifying appointed H. `--create-vault` appoints the local identity;
plain member/host-role startup without a vault binds but rejects inbound joins.
There is one `qfsd` binary, including directory mode.

A member creates or verifies independent X-Wing and ML-DSA-65 identity keys.
The identity file contains private seeds and its signed public document;
`.keys` and `.lock` siblings hold pair state and an exclusive process lock.
The `vault` file persists the vault id, current code, membership and ad timestamp.
Directory mode writes `directory.bin` and its lock, without constructing a
`KeyStore` or `HostService`. Private files use owner-only Unix permissions and
atomic replacement; keys are not encrypted at rest. See
[`crypto-keystore.md`](../docs/decisions/crypto-keystore.md).

H rotates pair epochs weekly; disconnected members reconnect and finish the
current wrap before mailbox traffic. Startup discards prior active AES slots
and prepares fresh signed wraps. `VaultHost::rotate_code` rotates admission
immediately, then publishes the new code and forgets the old directory row.
Code rotation is currently a library API, not an additional CLI command.

## Handshake and directory

Peer TCP order is identity verification, epoch hints, coordinated Construction B
wrap, final WrapAck, then the signed Join request inside one GCM packet. Lost
WrapAck retries reuse the cached ciphertext. Bad or rotated codes cause rejection
and provisional key-slot removal; durable epoch watermarks prevent regression.
Join M remains raw vault/code bytes plus canonical identity M. Its detached
ML-DSA signature uses `qfs/v1/join`; both M and signature are inside GCM.

Join codes appear in directory lookup/publication requests, as explicitly allowed
for the non-member directory. They never appear in pre-GCM peer handshake frames.
Joiners verify the ad's `qfs/v1/dir` signature, peer-id binding and age before
connecting. H checks its own current code; a directory row does not grant admission.

The directory rejects non-monotonic updates for each peer/vault, including replay
after forget, and caps ads at 32 per peer and 10,000 total. Ads expire after seven
days; timestamps more than 120 seconds ahead are rejected. Bounded replay
watermarks expire only after the acceptance horizon. Persistence uses file fsync,
atomic rename and parent-directory fsync; temporary partial writes are not served.

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
TCP tests cover actual daemon processes, directory replay/tampering, code rotation,
provisional cleanup, cached WrapAck retry, pre-GCM wire capture, corrupted queued
frames and reconnect re-sealing across a keystore reopen with retained memory state.

## Layout and implementation boundary

- `src/{config,daemon,keystore,error,ids,encoding}.rs`: identity loading,
  startup/shutdown, host selection, opaque durable key slots, canonical bytes,
  bounded local persistence decoding, errors/newtypes, and SHA-256.
- `src/crypto/{identity,wrap,aead,sign}.rs`: verified self-certifying identities,
  Pure ML-DSA-65 signatures with identity/wrap/manifest/flush and directory/join contexts, X-Wing
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

- `src/net/{frame,directory,session,join}.rs`: bounded versioned TCP frames,
  signed directory, peer handshake, single-vault admission, heartbeat/control
  delivery and adapters for the existing atomic mailbox staging. Frame bodies
  over 1 MiB, unknown types and unknown versions close the connection.

Crypto callers import verified peer documents before creating wraps. First
contact starts at epoch 1 from the smaller PeerId; later creation requires the
next epoch. `retry()` returns the cached ciphertext. A simultaneous-wrap loser
uses `retry_collision()` at the epoch before collision plus two; the winner's
key remains available for in-flight data. Retire old session handles
with `KeyStore::retire()` after in-flight work drains. AES send counters must
strictly increase per type; use the same header for canonical AAD and nonce.

Callers establish the live pair wrap before `flush_mailbox`; TCP calls thin
prepare/acknowledge adapters using the same staging and re-sealing code. Stale
mailbox controls and bodies are re-sealed under the current epoch; chunk counters
begin at 1 on its fresh key. Flush authenticates frames in queued order, stages
the complete instruction log, then writes bodies matching the final manifests.
Live pull/control entry points remain gated until flush succeeds. Exact receipts
allow retry after a later corrupt frame without reopening accepted counters.
On TCP, an authenticated end marker commits to the ordered batch; H keeps its
snapshot until it receives the recipient's encrypted apply acknowledgment.
The transport prepares old counters before opening that newer end marker.
Both pull and host use the same encrypt-at-send helper and the existing packet
and chunk counter domains on K_ab.

`HostService::into_state` / `resume` support a memory-state hand-off across a
keystore reopen without retaining old keys. Mailboxes and plaintext are volatile:
an actual process exit loses them. Durable recovery is not implemented.

Still left: durable chunks/mailboxes, path/tree filesystem behavior, kick,
multi-vault hosting and client GUI integration. Pull and commit orchestration
remain available in-process; the current TCP application loop handles admission,
mailbox catch-up, heartbeat and host control delivery. There is no CLI file-transfer
or filesystem interface yet. Static ek rotation does not provide forward secrecy.
There is no per-packet DSA, second content key, per-peer ciphertext chunk replica,
or random stored chunk nonce.
