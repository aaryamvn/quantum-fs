# Backend encrypt-at-send pull and host mailboxes
area: backend      status: done      opened: 2026-09-12      by: Justin
prompt: |
  You are continuing the backend. Do not scaffold. Do not reimplement crypto. Language: Rust. Follow docs/agents/PROTOCOL.md in full. Stay in backend/ plus your task/decision files. Do not touch client/.
  
  Create docs/tasks/active/backend-260912-encrypt-at-send.md from docs/agents/templates/task.md; put this prompt in prompt:. Checkpoint before each step.
  
  READ FIRST (accepted + current code — do not implement superseded files):
  - backend/README.md
  - backend/src/sync/{pull,host}.rs
  - backend/src/protocol/{pull,manifest,packet}.rs
  - backend/src/store/chunks.rs
  - backend/src/crypto/{aead,sign}.rs
  - backend/src/encoding.rs
  - backend/src/keystore.rs
  - backend/tests/aead_window.rs
  - docs/decisions/sync-plaintext-chunks.md
  - docs/decisions/sync-host-tcb.md
  - docs/decisions/net-vault-join-directory.md
  - docs/decisions/crypto-encoding.md
  - docs/decisions/crypto-pairwise-aead.md
  - docs/decisions/crypto-identity-selfcert.md
  - docs/decisions/crypto-keystore.md
  
  Skip (superseded): sync-encrypt-at-send.md, sync-host-queue.md, crypto-pq-suite.md, crypto-pairwise-aes.md, crypto-identity-bind.md, backend-stack.md.
  
  GOAL: In-process encrypt-at-send for (1) online pull and (2) H pushing file bytes into offline mailboxes as commits happen. Reuse existing AES-GCM, encodings, keystore slots, and W=1024 windows. No listen socket. No join-code directory.
  
  ALREADY DONE — do not rename or reimplement:
  - RustCryptoAes256Gcm::seal/open: AAD must match nonce; packet vs chunk send counters are independent; outbound seq must strictly increase; open authenticates before ReplayWindowState::accept
  - encoding::{chunk_id, chunk_aad, packet_aad, nonce, manifest_m, flush_m}
  - PayloadType::{Packet, ChunkBody}, PROTOCOL_VERSION, ReplayWindowState
  - KeyStore identity/wrap/session/retire; Construction B; restart drops active K_ab slots and prepares a new epoch
  - PureMlDsa contexts: qfs/v1/id, qfs/v1/wrap, qfs/v1/manifest, qfs/v1/flush. There is no qfs/v1/pkt
  - PullRequest cap 32; MemoryChunkStore plaintext put/get/has
  - Existing tests (currently 47) must stay green
  
  NORMS (do not pick a silent alternative):
  
  A. Epoch vs mailbox
     Chunk frames sealed for an offline member use the then-current K_Hoffline.
     When that pair’s epoch advances (daemon restart or weekly rotate), H MUST re-seal every still-queued chunk body for that member under the new epoch, with ChunkBody seqs restarting at 1. H has the plaintext replica. Do not pin retired K_ab across restarts (crypto-keystore.md). Do not expect an offline peer who restarted to open old-epoch ciphertext.
  
  B. Replay window vs delayed delivery
     ChunkBody seqs share one window per (pair, epoch, direction).
     On reconnect, this order is mandatory and total:
       (1) finish live-epoch wrap
       (2) re-seal mailbox frames if their epoch is stale
       (3) flush_mailbox strictly in queued order (oldest first)
       (4) only then any live pull or fan-out on that pair
     Never open a later mailbox seq before an earlier one. Flush must complete before live ChunkBody/Packet traffic on that pair.
  
  C. Coalesce
     Offline chunk frames coalesce by (file_id, index), keeping the latest plaintext/version.
     Do not coalesce the instruction log (add / clear / remove / new manifest). A delete then recreate must remain two instructions.
  
  D. Manifest trust
     Verify Pure ML-DSA (ctx qfs/v1/manifest, encoding::manifest_m, signature not in M) BEFORE reading chunk_ids.
     Then open GCM. Then require encoding::chunk_id(file_id, index, plaintext) == manifest.chunk_ids[index]. Then put. Idempotent if already have that id. Any failure → no write.
     accept() takes a pre-trusted manifest already applied from H’s control path. Ignore manifest bytes from the chunk holder.
     On commit, H rejects unless writer_id is in that vault’s member list.
  
  IMPLEMENT:
  
  1. Store
     MemoryChunkStore::put already takes (file_id, index, plaintext) but only keys by ChunkId. Persist (file_id, index) with the plaintext so a holder can rebuild chunk_aad. Do not store ciphertext. Do not add on-disk chunk durability.
  
  2. Seq helper
     If missing, add KeyStore::next_outbound_seq(handle, PayloadType) -> Seq that returns last+1 for that type (or 1 if none). Pull and host must use this. Do not reuse packet seqs for chunk bodies.
  
  3. Encrypt-at-send helper (used by pull AND host)
     Inputs: local KeyStore, recipient PairSession, file_id, index, plaintext.
     header = {version: 1, sender: local peer_id, receiver: peer, epoch: session.epoch, seq: next_outbound_seq(ChunkBody)}
     aad = encoding::chunk_aad(&header, file_id, index)
     nonce = encoding::nonce(&header, PayloadType::ChunkBody)
     ciphertext = Aes256Gcm::seal(handle, nonce, aad, plaintext)
     Return ChunkBodyFrame { header, file_id, index, ciphertext }.
     GCM output is the body. No second AEAD. No random nonce. No ML-DSA on the body.
  
  4. Receive helper
     Open with the recipient handle using nonce/aad derived from the frame (same functions as seal). Then apply NORMS D (DSA already done on the pre-trusted manifest). Then ChunkStore::put.
  
  5. Pull (online members)
     Add PullResponse in protocol/pull.rs: one ChunkBodyFrame per response (one AES-256-GCM body). PullRequest stays ≤32 ids.
     Replace PendingPullCoordinator with an in-process coordinator:
     - serve(request, requester_id): for each requested id the holder has, encrypt-at-send to that requester; skip missing ids (not an error).
     - accept(responses, trusted_manifest): receive helper only; do not take a holder-supplied manifest.
     The pull request itself is pairwise GCM control (PayloadType::Packet). Bodies are ChunkBody. No per-packet DSA.
  
  6. Host
     Same qfsd process; --host-id already selects H. H’s replica is MemoryChunkStore.
     Presence: heartbeat + TTL; online iff unexpired. Live cursors are not hosted.
     Vault member list: in-memory set of PeerId for v1 tests (join-code directory is out of scope).
     On committed update (chunks already on H as plaintext + writer-signed manifest):
     - Verify manifest DSA. Reject if writer_id ∉ member list. Apply to H’s store.
     - Online members: fan_out_control only (encoded signed manifest). They pull bodies. Do not push chunk bodies to online members.
     - Offline members: immediately encrypt-at-send each new/changed chunk under K_Hoffline and append_mailbox in recipient order as the change happens. Coalesce per NORM C. Re-seal per NORM A if epoch advances while they stay offline.
     MailboxEnvelope keeps existing fields. Add a typed inner frame in encoding.rs: u8 kind (0=control, 1=chunk-body) || (if 1: file_id || u64be(index)) || u32be(len) || gcm_ciphertext. That frame is NOT a second AEAD.
     Flush: 32-byte challenge; recipient signs encoding::flush_m with ctx qfs/v1/flush; only that recipient drains. Apply on drain: instruction/control log first, then chunk bodies (net-vault-join-directory.md). Idempotent. After flush, live traffic may start (NORM B).
     If HostService is not running, commits fail.
  
  7. Tests (in-process, no sockets)
     - Two holders, one requester, signed member writer: serve/accept; plaintext matches; same chunk to B vs C is different ciphertext.
     - Tampered GCM, wrong index, DSA-failing manifest, holder-supplied other validly signed manifest from a non-member writer_id → no store write.
     - PullRequest of 33 ids rejected; pulling an id already in store is a no-op.
     - Host: one online + one offline; commit; online received control only and must pull; offline mailbox already has (coalesced) chunk bodies before reconnect.
     - Three saves to the same (file_id, index) while offline → one body equal to the last plaintext; instruction log still has three commits if you record them.
     - Queue bodies, rotate pair epoch on H, flush; open succeeds under the new epoch, fails if forced to use the old handle.
     - Queue seqs 1..N; reconnect; a live pull attempted before flush is rejected or blocked; flush then pull succeeds.
     - Failed GCM on a mailbox body does not mark the replay window accepted (same rule as aead_window.rs).
     - Existing crypto/encoding/daemon tests still pass.
  
  DO NOT:
  - Bind --listen-addr; join codes; central directory; client/
  - Reimplement X-Wing/ML-DSA/AES; raw ML-KEM Encaps; ephemeral KEM per epoch
  - Pin retired pair keys across restarts to decrypt old mailbox frames
  - Second content AES key; double AEAD; ciphertext-hashed chunk ids; random chunk nonces; per-packet DSA; qfs/v1/pkt
  - Trust a manifest from the chunk holder; verify DSA after using chunk_ids
  - Durable disk chunk store; H successor election; live cursors through H
  - git add -A / commit unless I ask
  
  VERIFY: cd backend && cargo test && cargo build && cargo clippy --all-targets -- -D warnings
  
  At completion tell me:
  1. What pull/host can do in-process, including epoch re-seal and flush-before-live.
  2. Update backend/README.md done vs left. Still left: networking/wrap delivery, join-code directory, durable chunk storage, client GUI.

## Plan
- [ ] Define transfer/trust/queue contracts and add storage metadata, sequence/gate helpers, and codecs.
- [ ] Implement online pull, host commits/coalescing/re-seal, ordered authenticated flush, and daemon role integration.
- [ ] Exercise adversarial/in-process ordering tests, run full checks, update README and hand-off.

## Checkpoint   (overwrite in place · ≤10 lines · write BEFORE starting the next step)
done:       Implementation, README, review fixes and full verification complete: 64 tests pass, build/Clippy/format pass.
in-flight:  Recording outcome and moving the task to done without staging or committing.
next:       Report implemented in-process behavior and remaining transport/durability boundary.
open:       None. Plaintext/mailboxes remain volatile; memory-state restart hand-off does not add disk recovery.

## Outcome   (fill at completion · ≤10 lines · facts only)
changed:    Plaintext metadata, sequence/gate helpers, trusted pull, typed codecs, H commit/coalesce/reseal/flush, daemon H role, README and 17 added tests.
verified:   cargo test → 64 passed, 0 failed; cargo build → success; cargo clippy --all-targets -- -D warnings → success; cargo fmt --check → success; git diff --check → clean.
not-done:   Networking/wrap delivery, join-code directory, durable chunks/mailboxes and client GUI integration.
gotchas:    Real process exit loses memory chunks/queues. Prompt exceeds task line budget as explicitly requested. No staging or commit performed.
