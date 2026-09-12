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
# H: adds a vault, serves every hosted vault, and prints the new Base32 code.
./target/debug/qfsd --create-vault --data-dir .qfs-host --listen-addr 127.0.0.1:7447 --directory-addr 127.0.0.1:7440
```

```sh
# Paste the host's uppercase, unpadded Base32 code after running read.
read -r QFS_JOIN_CODE
./target/debug/qfsd --data-dir .qfs-member --listen-addr 127.0.0.1:7448 --directory-addr 127.0.0.1:7440 --join-code "$QFS_JOIN_CODE"
```

Restart H with the same data directory and without `--create-vault` to reload
its existing vaults. Supplying the directory address re-advertises their codes
at the newly bound address; adding `--create-vault` creates one more vault.

```sh
./target/debug/qfsd --data-dir .qfs-host --listen-addr 127.0.0.1:7447 --directory-addr 127.0.0.1:7440
```

Successful admission logs `accepted vault member; pair live` on H and
`joined vault` on the member. Ctrl-C or SIGTERM shuts down gracefully.

### Demo terminal

Every `qfsd` startup, including directory mode and existing-vault restarts,
automatically displays the color-coded demo log in its launching terminal.
No debug flag or environment opt-in is required. Events cover verified peer
sessions, signed directory routes, file changes, encrypted piece delivery,
file availability with per-source piece counts, offline recovery, revocation,
and cache eviction. Heartbeats and have queries produce no log events.
Labels name the algorithms actually used; partial pulls never claim a complete file.

Display updates group activity every 750 ms, show up to four event blocks plus a
burst summary, and never delay protocol processing to pace the display. Each
process also appends complete event blocks to `--data-dir/demo-events.log`.
Colors follow the terminal automatically; `FORCE_COLOR=1` forces ANSI and
`NO_COLOR=1` requests plain text. Timestamps use UTC; peer/file labels show
the first 12 hexadecimal digits of their public identifiers.

For one corner terminal showing all VMs, run the included Python 3 monitor:

```sh
python3 backend/demo_monitor.py \
  central=demo@192.168.64.2:/home/demo/.qfs-directory/demo-events.log \
  vault=demo@192.168.64.3:/home/demo/.qfs-host/demo-events.log \
  client1=demo@192.168.64.4:/home/demo/.qfs-member/demo-events.log
```

Replace the example SSH users, VM addresses, and absolute data paths; add two
more `label=user@host:/path/demo-events.log` arguments for five sources. Local
`label=/absolute/path/demo-events.log` arguments also work. SSH must already
work noninteractively. The monitor keeps each event and its indented sources
together, labels the originating VM, and summarizes bursts across all nodes.
Server logs start automatically; this optional combined view is started once
in the corner terminal. Ctrl-C stops only the monitor. No new network protocol
or daemon port is introduced. Library-only client processes can enable the same
events with `demo_log::start("VAULT CLIENT")` and `demo_log::set_log_file(path)`.

The runtime is Tokio current-thread with `LocalSet` and `spawn_local`.
The peer handshake deadline is five seconds, idle timeout is 30 seconds,
and H permits 32 simultaneous provisional handshakes and 128 TCP connections.
Members poll H every **100 ms** for presence and pending controls. Missed ticks
are skipped and each reply drains before the next heartbeat. This reduces demo
control latency; online file bytes still require a pull, and throughput depends
on the link, storage and processing time.
Presence expiry remains 30 seconds, so a silent connection failure can leave a
peer marked online until that timeout; a detected disconnect clears presence.

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
The `vault` file persists the vault id, current code and ad timestamp (with a
compatibility member list); `replica.bin` is authoritative for replica membership.
Joined members and hosts load their durable replica before serving file operations.
The identity file must be in the configured data directory so its existing lock
protects that replica. A second open of the same identity fails.
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
Durable tests cover process-state loss and reopen, metadata-only snapshot size,
complete-file corruption, corrupt chunk discard, mailbox pins, orphan cleanup,
write-failure atomicity, acknowledgments and root binding. Tree tests cover member
writers, restart, strict UTF-8 paths, byte-exact names, cycle/uniqueness checks,
serialized renames and unlink during an in-flight pull. Kick tests cover historical
writer reload, atomic code/membership rollback, pair revocation and TCP closure.
Multi-vault tests cover disjoint trees, retained bindings, process reload, migration
recovery and lost-WrapAck ciphertext retry. Eviction tests cover host/pin/refusal,
local availability bits, in-flight responses and empty-holder fallback. Locate tests cover H-first
selection, member fallback, bounded concurrent discovery and exact have replies.
TCP tests also cover member path operations, atomic failed saves, have queries,
trusted pulls and more than 1024 queued online controls before a fresh reply.

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
- `src/store/{chunks,durable,replica,tree}.rs`: fallible plaintext put/get/has,
  request-order have-bitsets, immutable chunk files, authenticated metadata
  snapshots and directory entries. Staging shares immutable chunk records rather
  than copying their plaintext.
- `src/sync/pull.rs`: in-process bounded pull, encrypted control requests and one
  AES-GCM chunk body per response. Holders skip missing IDs. Acceptance uses a
  pre-trusted manifest from H, checks GCM and the plaintext chunk hash, then
  rechecks the current manifest and live file link under the store lock. Accepted
  batches persist before becoming visible; unlink cannot resurrect via a stale pull.
- `src/sync/locate.rs`, `src/protocol/locate.rs`: prefer a live, ungated pair to H;
  otherwise query live member pairs for exactly the requested 1–32 IDs. Async
  queries run concurrently with a five-second overall deadline. The `qfs/v1/have/`
  prefix is separate from instruction kinds; replies never supply manifest trust
  and locations are never stored in the directory or replica snapshot.
- `src/sync/host.rs` and `src/sync/host/{persistence,filesystem}.rs`: durable
  vault membership, heartbeat/TTL presence, member-writer
  signature verification, online control-only fan-out, and immediate offline
  chunk encryption. Bodies coalesce by file/index; all control instructions
  remain ordered. Recipient-only ML-DSA challenge authentication drains the queue.
  H is TCB for all shared plaintext; live cursors go direct pairwise GCM,
  without DSA or H. A stopped host rejects commits.

- `src/net/{frame,directory,session,join,locate}.rs`: bounded versioned TCP frames,
  signed directory, peer handshake, vault-specific admission, member commits,
  have queries, pulls and adapters for the existing atomic mailbox staging. Frame bodies
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
Public writer history travels in the flush offer and is bound to that encrypted
end digest; it stays staged until the entire batch authenticates. A member can
therefore verify files from a writer who joined and was kicked while it was offline.
Both pull and host use the same encrypt-at-send helper and the existing packet
and chunk counter domains on K_ab.

H stores each vault at `data_dir/vaults/<64 lowercase hex vault_id>/`, containing
`vault`, `replica.bin` and `chunks/`. Member processes retain one replica in their
data directory. Identity, keys and the sole keystore lock stay at the data root.
Within a replica, `chunks/<64 lowercase hex chunk_id>` holds immutable plaintext
only, up to 1 MiB per chunk. `replica.bin` holds the root, tree, members,
writer-signed manifests, ordered instructions, mailbox content references,
acknowledgment watermarks and chunk index; it never contains chunk bodies or GCM
envelopes. Its versioned header, generation, length and SHA-256 reject complete-file
corruption before body parsing. Atomic replacement uses the keystore's existing
file/parent fsync protocol and 16 MiB metadata cap. A metadata operation rewrites
metadata, not the vault's plaintext. Chunks remain unencrypted at rest.

`HostService::open_durable` and `MemberReplica::open_durable` recover those files
under the live keystore lock. The root is `FileId(vault_id.0)`, or zero without a
vault; another root is rejected. Changing a zero-root store to a vault requires an
explicit one-time metadata migration. Invalid chunk hashes are discarded; missing
mailbox-pinned chunks make reopen fail closed. Successful snapshots sweep orphans
and unreferenced chunks. Clear/unlink/remove release their queued chunk pins.
All current members, including H, must acknowledge a log prefix before it is
truncated; a missing acknowledgment pins it. Online apply watermarks use the
existing GCM heartbeat, and flush uses its existing batch acknowledgment.

H exposes `mkdir`, `link_file`, `save_file`, `unlink` and `rename`. Every current
member may mutate any path; NewManifest still binds its writer to the authenticated
sender. H serializes arrivals, so a later operation resolves against the earlier
result and can fail if its source moved. Names are byte-exact and case-sensitive,
with no Unicode normalization. Paths use the vault root (an optional leading `/`),
reject dot/dot-dot, NUL, backslashes, drive prefixes and empty components, and are
limited to 255 bytes per component and 4096 bytes overall. Nonempty directory
unlink and moves into descendants fail. Deprecated Add remains a no-op.

New-file saves prepare their manifest and Link together and publish one durable
snapshot before returning controls. `JoinedPeer::{mkdir,save_file,unlink,rename}`
send those operations to H over the existing GCM frames. Uploads are bounded to
64 MiB of staged plaintext per connection; individual wire frames also include
headers and the GCM tag within the 1 MiB frame cap. Use smaller chunks, such as
512 KiB, for TCP uploads. `JoinedPeer::{have_query,pull}` support host discovery
and trusted pulls; `net::locate::{query_over_stream,serve_have_once}` supports
queries on already-established member pairs. Peer session establishment still
uses the existing handshake. There is no CLI filesystem command or OS mount.

Queued ciphertext is rebuilt from durable control records and pinned plaintext
after restart; active pair keys and counters are never reloaded for reuse.
Finish the live wrap, refresh/re-seal, and flush before live traffic. Online
control packets are also authenticated in counter order before newer membership
or have replies, preserving W=1024. `into_state`/`resume` remains available for
memory-only hand-offs; durable handles hold the keystore lock and must be dropped
before reopening from disk.

`HostService::kick` accepts only a current member other than H. H removes the
member from fan-out and mailboxes, logs Kick for the remaining members, persists
denial plus a fresh code, discards the pair and requests TCP disconnection.
`VaultHost::kick` then publishes the new code and forgets the old one; directory
errors propagate while the old code stays invalid locally. Recipients discard
the kicked pair after applying Kick. Offline recipients apply behind the flush
gate; online mesh access can continue until the recipient applies that control.

Kick preserves accepted files and directories. Archived public identity documents
let restart verify historical signatures independently of current membership;
new commits still require current membership. Kick cannot erase remote copies or
old keys. The denylist stops the same identity rejoining accidentally; a newly
minted identity with the current code can join. Code rotation controls entry.
Admission and replica changes use an undo journal and the existing atomic/fsync
writer. An interrupted two-file update rolls back before either file is served.

`VaultSet` dispatches encrypted Join by vault ID on one listener. A confirmed
pair stays bound to one vault even between TCP connections. Another-vault Join
returns an encrypted, distinct conflict and preserves that pair and mailbox.
Only authenticated refusal discards a new joiner's provisional pair; ambiguous
EOF or lost WrapAck retains cached ciphertext and counters for retry. Legacy
single-vault layouts still open. Creating a second vault migrates the first with
`.migrate-vaults` (raw vault ID and phase), synced renames and verified, idempotent
resume after interruption. Directory existence alone never proves completion.

`MemberReplica::evict_file` removes local chunk plaintext, files and index rows,
while retaining the tree and signed manifest. It requires a live, ungated pair
to H and refuses mailbox-pinned chunks. H always refuses eviction. Have bits
come directly from local chunk presence. `pull_from_holders` and its async form
try the next holder after an empty response; inspect `PullReport::is_complete`
(or `missing`) for partial delivery. Existing trusted pull restores evicted
bytes. Async fetch operations must discard their stream if canceled on timeout.

Still left: successor election, client GUI integration, unkick and same-peer
multi-vault multiplexing. Static ek rotation does not provide forward secrecy.
There is no per-packet DSA, second content key, per-peer ciphertext chunk replica,
or random stored chunk nonce.
