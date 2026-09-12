# Durable replica, filesystem tree and holder locate
area: backend      status: done      opened: 2026-09-12      by: human
prompt: |
  You are continuing the backend. Do not scaffold. Do not reimplement crypto, pull encrypt-at-send, host commit/flush/re-seal internals, or the TCP/join handshake. Language: Rust. Follow docs/agents/PROTOCOL.md in full. Stay in backend/ plus your task/decision files. Do not touch client/.
  
  Create docs/tasks/active/backend-260912-durable-fs-locate.md from docs/agents/templates/task.md; prompt: this message verbatim.
  
  CHECKPOINT: before starting each Plan checkbox, overwrite that task file's Checkpoint in place (≤10 lines) with done / in-flight / next / open from the template. Write it before the next step, never only at the end. If you skip a checkbox, put the reason in open:.
  
  If docs/tasks/active/backend-260912-net-join.md still exists, stop and tell the human. Do not share-edit encoding.rs with that task.
  
  READ FIRST:
  - docs/agents/PROTOCOL.md
  - docs/VISION.md (Napster: nearby holders first; location gossip is not the directory)
  - backend/README.md
  - backend/src/store/chunks.rs
  - backend/src/sync/{host,pull}.rs
  - backend/src/keystore.rs (atomic_private_write, File::try_lock on sibling .lock)
  - backend/src/encoding.rs (control kinds 0–3; encode_pull_request; encode_net_control; no GCM plaintext class byte)
  - backend/src/net/session.rs if present (do not change TCP frame type numbers 1–14)
  - docs/decisions/sync-plaintext-chunks.md
  - docs/decisions/sync-host-tcb.md
  - docs/decisions/net-vault-join-directory.md
  - docs/decisions/crypto-keystore.md
  - docs/decisions/crypto-encoding.md
  - docs/agents/templates/decision.md
  
  Skip superseded-by files.
  
  GOAL: (1) Chunks, tree, members, instruction log, mailbox content, and last-applied watermarks survive process exit, with per-op I/O proportional to metadata not vault size. (2) Real path operations: mkdir, create/save file, unlink, rename; writer→H commit of those ops. (3) Locate holders for a pull: H if the requester has a live session to H, else members with live local sessions who have the chunks. Keep plaintext-at-rest. Keep encrypt-at-send on the wire.
  
  Write two accepted decision files (new files; do not edit other decision bodies):
  - docs/decisions/storage-replica.md — chunk files vs replica.bin, header/hash, persist-before-visible, pin/unlink, log truncation, lock, root FileId
  - docs/decisions/sync-tree.md — path rules, byte-exact case-sensitive names with no Unicode normalization, kind 1 Add deprecated no-op, v1 any member may mutate any path, H serializes, concurrent ops: H arrival order wins
  
  Implement in this order and do not skip ahead: durable layout + persist hook + accept-gate-on-live-manifest → tree kinds 4–6 and dirents in replica.bin → locate. Tree and locate both sit on the chunk/replica split; do not put chunk plaintext in replica.bin even as a temporary hack.
  
  ALREADY DONE — call, do not rewrite:
  - MemoryChunkStore; ChunkRecord {file_id, index, plaintext}; chunk_id = SHA-256(file_id || u64be(index) || plaintext); have_bitset in request order, LSB first
  - HostService commit/fan-out/offline encrypt-at-send, coalesce chunks by (file_id, index) not the instruction log, refresh_mailboxes, flush-before-live, TrustedManifest, into_state/resume (memory only)
  - ControlUpdate::{NewManifest, Add, Clear, Remove} encodings kinds 0–3. Add is a no-op — leave it a no-op; do not invent empty inodes
  - Clear/Remove already drop mailbox QueueContent::Chunk for that file_id, then remove_file + drop manifest
  - InProcessPullCoordinator serve/accept; accept() takes a pre-trusted manifest only
  - Keystore atomic_private_write: temp, write, file.sync_all, rename, parent directory sync_all, owner-only; MAX_STORE_BYTES = 16 MiB. Reuse it for replica.bin. Do not invent a second fsync protocol
  - Keystore exclusive lock: sibling .lock, File::try_lock (advisory; kernel releases on process death). No PID file
  - Restart drops live K_ab slots and prepares new wraps. Resume must refresh_mailboxes so queued bodies re-seal under the live epoch. Do not persist GCM envelopes, wrap_key, or PairKeyHandle
  - TCP/join may exist under src/net/; do not change handshake order or frame types
  
  NORMS (do not pick a silent alternative):
  
  STORAGE
  1. Split storage. Chunks are content-addressed and immutable; they are not in the atomic snapshot.
     data_dir/chunks/<64 lowercase hex chunk_id> — plaintext bytes only. Same temp-fsync-rename-parent-sync sequence as atomic_private_write, size cap = 1 MiB (TCP frame body). Never rewrite a chunk file. put() of an existing chunk_id is a no-op. On load, recompute chunk_id from (file_id, index, bytes) recorded in replica.bin; hash mismatch or torn file → discard that file; that is not a successful load of that chunk.
     data_dir/replica.bin — metadata only. Rewrite via atomic_private_write (16 MiB cap) on every successful mutation. A 2 GB vault must not pay 2 GB I/O to mkdir or to commit a manifest.
  
  2. replica.bin header. Reject on any mismatch; do not parse a bad body:
     magic b"qfs/local/replica/" || u8 format_version=1 || u64be(generation) || u32be(body_len) || body || sha256(magic||version||generation||body_len||body).
     A bit-flipped complete file must fail closed. generation is monotonic per successful write; it is not a substitute for the hash.
  
  3. replica.bin body (canonical BE, u32be length prefixes): expected_root FileId || dirents (after tree lands; empty map until then) || member PeerIds || manifests (existing manifest_m+sig) || instruction log (encode_control_record each) || next_control u64be || per-peer mailbox QueueContent only (Control records and/or file_id||u64be(index)||chunk_id — never GCM) || per-peer acked_through u64be || chunk index: n × (chunk_id || file_id || u64be(index)). No plaintext in replica.bin.
  
  4. expected_root: if HostService has a VaultId, expected_root = FileId(vault_id.0). Else FileId([0;32]). On open, if the file's root ≠ expected_root → Error, do not mix. A later VaultId wiring is a documented one-time rewrite of replica.bin, not an in-place dual-root.
  
  5. Persist-before-visible: HostService::commit_verified (and every other mutation that today assigns self.state) must build staged state including mailbox content and tree → write replica.bin from staged → only then assign self.state and return Ok. If the write fails, in-memory state is unchanged, live-traffic gates revert as they do today on prepare failure, and take_online_control / flush must not observe the commit. Never persist after packets have been handed to a caller. Do not put the write at the end of commit after fan-out has already mutated self.state.
  
  6. Chunk files vs Clear/Remove: write chunk files in put() before commit. After replica.bin is durable, unlink chunk files whose chunk_id is in neither any live manifest nor any undrained mailbox QueueContent::Chunk. Pin mailbox-referenced chunks until that commit's retain() drops them (Clear/Remove already drop those queue entries in the same commit). refresh_mailboxes / flush must not see a missing pinned chunk; if they would, that is a bug, not a skip-and-log. Orphans (put then failed persist) are swept on open and after successful persist.
  
  7. Instruction log: do not coalesce (order matters for tree ops). Truncate prefix < min(acked_through over current members including H). Missing acked_through treats as 0 and pins the whole log. A member who never returns pins the log until kick (out of this task; do not invent expiry). Persist acked_through. MemberReplica, after successful apply of record id N, reports N; HostService stores it before the next replica.bin write. No new wire ack message.
  
  8. Lock: DurableStore opens only under a live KeyStore for this data_dir. Use that keystore .lock. Do not add replica.lock or a PID file. Second process: KeyStore::open fails as today.
  
  9. accept() in pull.rs: after GCM + trusted-manifest membership + hash, write only if this replica still has a live manifest for body.file_id AND (once the tree exists) a dirent pointing at that file_id. A pull in flight when Remove/Unlink lands must not resurrect chunks. Idempotent if the file is still live.
  
  10. Disk encryption of chunks is out of v1. Zeroize is for keys, not file bytes.
  
  TREE
  11. Paths: UTF-8, `/` separator, relative to vault root, no NUL, no empty component, no `.` or `..`, component max 255 bytes, full path max 4096. Reject `\\` and drive letters. Names are byte-exact and case-sensitive; no Unicode normalization (NFC and NFD of the same letters are two dirents). State that in sync-tree.md.
  
  12. Keep control kinds 0–3 encodings unchanged.
     Kind 1 Add: remains a no-op, deprecated pending removal. Do not define “create empty inode if linked next.”
     Kind 2 Clear(file_id): drop chunks+manifest, keep dirent.
     Kind 3 Remove(file_id): drop chunks+manifest+dirent that points at that FileId.
     New kinds (unknown kind already fail-closed):
     4 Link: parent FileId || u32be(len(name)) || name_utf8 || child FileId || u8 is_dir (1=dir, 0=file)
     5 Unlink: parent FileId || u32be(len(name)) || name_utf8
     6 Rename: src_parent || src_name || dst_parent || dst_name (same name encoding)
     Tree ops are control instructions, not chunk bodies. Do not coalesce the log.
  
  13. Authorization (sync-tree.md, not a silent widening): v1 any current vault member may submit kinds 4–6 for any path (flat trust; members already share plaintext). NewManifest still requires writer_id == sender_id && writer ∈ members. Non-members rejected. H is the mandatory hub and serializes; concurrent renames: H arrival order is the conflict resolution; no CRDT. Extend fan_out_control so kinds 4–6 accept any member, not only sender==H. Do not add per-path ACL.
  
  14. H APIs: mkdir, link_file, unlink, rename, save_file (put chunks, sign manifest, commit NewManifest, Link if new). Unlink directory only if empty. Cycle-detect rename into descendant. Name uniqueness per parent. Root dirent exists at expected_root.
  
  15. Writer who is not H sends tree ops and NewManifest to H via existing fan_out_control / GcmPacket once membership exists. Persist-before-visible still applies on H.
  
  LOCATE
  16. HaveQuery / HaveReply are not ControlRecords and MUST NOT use kinds 0–6. They are not logged, not snapshotted, not coalesced. Never pass them to decode_control_record. Putting them in the control kind space would make 1 mean both Add and HaveQuery.
  
  17. Locate GCM plaintext starts with magic b"qfs/v1/have/" (12 bytes). That prefix makes a mis-dispatch into decode_net_control / decode_pull_request / decode_control_record fail closed (first byte 0x71; pull count would exceed 32; control kind would be unknown or finish() fail).
     HaveQuery: magic || u8 1 || file_id || u32be(n) || n × ChunkId, 1 ≤ n ≤ 32.
     HaveReply: magic || u8 2 || file_id || u32be(n) || n × ChunkId (echo the query's ids in the same order) || have_bitset.
     have_bitset is ChunkStore::have_bitset over those echoed ids (request order, LSB first, length = ceil(n/8)). It is not the file's full chunk list and not relative to a manifest version. Reject n mismatch, id-list mismatch, or bitset length ≠ ceil(n/8).
  
  18. Locator::holders on the requester uses the requester's live pair sessions (KeyStore::current_session + require_live_traffic), not HostService::presence (that is H's TTL table). If a live session to H exists, holders = [H] first (full replica). Else query members with live local sessions; skip H if that session is dead. Return who is known now; do not block forever. Directory has no membership and must not store locations.
  
  19. Pull still uses InProcessPullCoordinator / existing PullRequest. Ignore holder-supplied manifests. accept() still needs the pre-trusted fan-out manifest plus live dirent. If net session APIs exist, send HaveQuery as GcmPacket after flush-before-live; do not add TCP frame types. If net-join is unfinished, in-process locator tests are enough.
  
  DO NOT:
  - Put chunk plaintext in replica.bin; pin retired K_ab to decrypt old envelopes; durable per-peer GCM replicas
  - Ciphertext-hashed chunk ids; double AEAD; disk encryption; per-packet DSA
  - Coalesce the instruction log; invent log expiry instead of kick
  - Change kind 1 Add into an inode; use control kinds 7/8 for locate
  - PID file / second replica.lock; a second fsync protocol
  - Kick, multi-vault, storage eviction, successor election, client/
  - Rewrite handshake/Join-in-GCM or TCP frame type numbers
  - git add -A / commit unless the human asks
  
  TESTS:
  Storage
  - Commit two chunks, drop process, reopen KeyStore+HostService+store: same plaintext, members, log, mailbox QueueContent; refresh_mailboxes does not need old epoch keys.
  - replica.bin size after a large-chunk commit is metadata-sized, not proportional to plaintext.
  - Bit-flip any byte of a complete replica.bin → open fails closed; no partial apply.
  - Persist failure after staged apply (inject write error): HostState unchanged; take_online_control empty for that commit.
  - Torn/corrupt chunk file: discarded; replica still opens.
  - Two members: only A acks → log prefix remains; both ack through N → replica.bin log starts after N.
  - Second KeyStore::open on the same identity fails.
  - Clear/Remove then refresh_mailboxes/flush: no missing-chunk error; chunk files for that file gone after persist; mailbox keeps the control record, not dangling chunk refs.
  
  Tree
  - mkdir /a, save /a/f with two chunks, restart: tree + plaintext + members reload.
  - `..`, NUL, slash-in-name rejected; NFC vs NFD are distinct names.
  - Unlink non-empty dir fails; unlink/Remove file then accept() writes 0 chunks (no resurrection).
  - Two members rename the same child to different names; H order wins; both converge on H's log; replica.bin matches.
  - Member (not H) mkdir/save: H has bytes+dirent; if replica write fails, no fan-out.
  
  Locate
  - Live session to H: holders starts with H; pull from H works.
  - H session gone, member B has_bitset the queried ids: holders contains B not H; pull from B with the earlier trusted manifest works.
  - HaveReply n / id list / bitset-length mismatch rejected.
  - Holder on an older manifest: bits answer the queried ids only; requester does not treat the reply as a new manifest.
  - Existing host/pull/crypto/durable tests stay green.
  
  VERIFY: cd backend && cargo test && cargo build && cargo clippy --all-targets -- -D warnings
  
  At completion:
  1. What survives restart; chunk dir vs replica.bin; which tree ops exist and who may issue them; how locate chooses H vs members.
  2. Update backend/README.md.
  Still left: kick, multi-vault, storage eviction, successor election, client GUI; networking/join only if that task is unfinished.

## Plan
- [x] Durable chunk/metadata split, persist-before-visible hooks, live-manifest pull gate, recovery and failure tests.
- [x] Tree kinds 4–6, persisted dirents, member path operations and writer-to-H transport, tree tests.
- [x] Holder locate with separate have encoding and gated GCM transport, locate tests.
- [x] Update README, run full tests/build/strict Clippy, close task.

## Checkpoint
done:       All four plan steps complete; 134 tests, build, strict all-target Clippy, formatting and diff checks pass.
in-flight:  none
next:       none; implementation and README complete.
open:       No skipped steps; remaining product features are listed below.

## Outcome
changed:    backend/{src,tests,README.md}; docs/decisions/{storage-replica,sync-tree}.md; this task.
verified:   cd backend && cargo test → 134 passed; cargo build → qfsd built; cargo clippy --all-targets -- -D warnings → passed; cargo fmt --check and git diff --check → passed.
not-done:   Kick, multi-vault hosting, storage eviction, successor election and client GUI (outside requested scope).
gotchas:    Durable handles retain the existing keystore lock; drop them before reopen. Metadata cap 16 MiB; chunks cap 1 MiB; TCP headers/tag reduce usable body size. New-file manifest+Link publish in one snapshot; counters still advance on failed preparation. No staged files or commit.
