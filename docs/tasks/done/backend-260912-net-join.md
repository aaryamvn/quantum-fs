# Backend TCP vault join and signed directory
area: backend      status: done      opened: 2026-09-12      by: Justin
prompt: |
  You are continuing the backend. Do not scaffold. Do not reimplement crypto, pull, or host commit/flush/re-seal. Language: Rust. Follow docs/agents/PROTOCOL.md in full. Stay in backend/ plus your task/decision files. Do not touch client/.

  Create docs/tasks/active/backend-260912-net-join.md from docs/agents/templates/task.md; prompt: this message verbatim. Checkpoint before each step.

  READ FIRST:
  - backend/README.md
  - backend/src/daemon.rs
  - backend/src/config.rs
  - backend/src/sync/host.rs
  - backend/src/crypto/{identity,wrap,sign}.rs
  - backend/src/keystore.rs
  - backend/src/encoding.rs
  - docs/decisions/net-vault-join-directory.md
  - docs/decisions/sync-host-tcb.md
  - docs/decisions/crypto-identity-selfcert.md
  - docs/decisions/crypto-encoding.md
  - docs/decisions/crypto-pairwise-aead.md
  - docs/decisions/crypto-keystore.md
  - docs/decisions/backend-runtime.md
  - docs/decisions/backend-stack-xwing.md

  Skip superseded-by files.

  GOAL: Two qfsd processes on localhost TCP can create a vault, publish a join code to a directory that stores only a signed ad, look up the code, complete Identity→epoch→wrap→Join-in-GCM, and end with a live pair session. Wire existing HostService, pending_wraps, refresh_mailboxes, flush-before-live. No durable chunk store, no path/tree FS, no GUI.

  ALREADY DONE — call, do not rewrite:
  - KeyStore, Construction B, pending_wraps, current_session, retire, block_live_traffic / require_live_traffic, refresh_mailboxes
  - HostService::{new, commit, heartbeat, take_online_control, issue_flush_challenge, flush_mailbox, refresh_mailboxes, mailbox}
  - MemberReplica, encrypt_at_send
  - Identity bind; PureMlDsa ctx qfs/v1/id|wrap|manifest|flush
  - listen_addr parsed and idle; tokio current_thread (macros, rt, signal, time)

  JOIN AND THE CODE (normative — do not pick plaintext):
  The 16-byte join code MUST NOT appear in any pre-GCM byte. Join (type 7) is a GcmPacket after a pair session exists.
  Handshake order, total:
    (1) TCP
    (2) IdentityDocument exchange; verify peer_id := SHA-256(qfs/v1/peer || vk) and signature
    (3) EpochHint: each side sends its current epoch for this pair, or 0 if none. If they disagree, the lexicographically smaller PeerId initiates the wrap at max(local, remote, 1) or next_epoch as already specified; duplicate-epoch conflict: smaller initiator wins, loser retry_collision. Do not let both sides independently bump into divergent epochs.
    (4) Construction B wrap + WrapAck (see WrapAck rules)
    (5) Join as GcmPacket (JoinRequest M, ctx qfs/v1/join)
    (6) On bad or rotated code: tear down TCP, HostService must not add_member, retire/discard the pair slot created for this handshake, assert no residual session with that peer. These sessions are provisional until Join succeeds. Cap concurrent provisional handshakes (default 32). Handshake deadline 5s. Idle timeout 30s. Concurrent TCP cap 128.
    (7) On good Join: promote session (no longer provisional), append peer_id on H
    (8) If mailbox non-empty: H refresh_mailboxes (re-seal to the live epoch) THEN FlushChal/FlushSig THEN flush. Only then live pull/fan-out. refresh_mailboxes is mandatory between WrapAck and FlushChal whenever wrap created a new epoch (restart). Do not pin retired K_ab to open old mailbox frames.

  WrapAck:
  - Type 13. Receiver sends it after unwrap() succeeds.
  - Initiator persists K_ab in create() as today; do not change that.
  - WrapAck means the receiver installed the session. Duplicate WrapAck: no-op.
  - If WrapAck never arrives: handshake deadline fires; tear down TCP; do not Encaps again; resend the same pending wrap on the next connection via retry()/pending_wraps.
  - Do not start Join or GCM application data before WrapAck (receiver) / before sending WrapAck (receiver side). Initiator waits for WrapAck before sending Join.

  Flush-gate abort:
  - If drain fails midway, gate stays closed, mailbox is not cleared, live traffic stays blocked, replica apply stays atomic (existing HostService staging). Retry flush from the remaining queue; do not open live ChunkBody/Packet until flush_mailbox returns Ok.

  NORMS:
  1. Directory is not a crypto member and not a second H. Same crate: `qfsd --directory` only. No K_ab, no replica, no file bytes.
  2. Map is join_code → DirectoryAd only.
  3. Joiner verifies DirectoryAd (ctx qfs/v1/dir, peer_id bind) BEFORE connecting to the advertised addr.
  4. H admits only its current code for that vault_id. A directory row is not enough.
  5. PeerId identity only. No MAC/email/password.
  6. Leaked code admits until rotate. No join-approval in v1.
  7. tokio current_thread + LocalSet + spawn_local. Add net and io-util only. No multi-thread runtime.
  8. Do not change HostService commit/coalesce/re-seal/flush internals; transport calls them.

  ENCODING (encoding.rs; one serialize path; BE; u32be length prefixes; no hex IDs on the wire):
  - JoinCode: 16 CSPRNG bytes. CLI: RFC 4648 Base32, no padding, uppercase. Map keys and Join M use raw 16 bytes, never Base32 inside M/AAD.
  - VaultId: 32 CSPRNG bytes.
  - DirectoryAd M (ctx qfs/v1/dir): peer_id || vault_id || u32be(len(addr)) || addr_utf8 || u32be(len(ek)) || ek || u32be(len(vk)) || vk || u64be(issued_at_unix). Signature not in M.
  - addr_utf8 is std SocketAddr display: IPv4 a.b.c.d:port or bracketed IPv6 [::1]:port. No hostnames/DNS in v1 (state that as a comment + crypto-dir-join-ctx.md).
  - JoinRequest M (ctx qfs/v1/join): vault_id || join_code_raw16 || identity_m(document).
  - EpochHint M: peer_id || u64be(epoch) with 0 = none.
  - TCP frame: u8 frame_version=1 || u32be(body_len) || u8 type || payload.
    Validate body_len ≤ 1 MiB BEFORE allocating. Unknown type or frame_version ≠ 1: log and close (fail-closed). Do not ignore.
  - Types: Identity=1, EpochHint=2, Wrap=3, WrapAck=4, DirLookup=5, DirAd=6, DirPut=7, DirForget=8, Join=9, GcmPacket=10, GcmChunk=11, FlushChal=12, FlushSig=13, Heartbeat=14.
  - GcmPacket / GcmChunk reuse existing ControlPacket / ChunkBodyFrame layouts. No second AEAD. Join payload is GcmPacket whose plaintext is JoinRequest M.
  Add qfs/v1/dir and qfs/v1/join to require_protocol_context. New decision docs/decisions/crypto-dir-join-ctx.md; do not edit crypto-identity-selfcert.md body.

  DIRECTORY:
  - DirPut: verify DSA + peer_id bind. Accept only if issued_at_unix is strictly greater than any stored ad for the same (peer_id, vault_id). Replay of an older valid ad must fail.
  - Joiner rejects ads with issued_at older than 7 days or more than 120s in the future.
  - DirForget: M = peer_id || vault_id || join_code_raw16 || u64be(issued_at); same monotonic rule; only that peer_id may delete/replace.
  - Caps: max 32 ads per peer_id; max 10_000 ads total; expire ads with issued_at older than 7 days. Reject DirPut when full.
  - Persist with atomic replace + fsync (write temp, fsync, rename), owner-only perms, same style as keystore. Not append-only. Crash-mid-write must not serve a truncated map (keep previous file).

  ADVERTISE ADDR:
  Sign DirectoryAd AFTER bind. listen_addr in the ad is SocketAddr from TcpListener::local_addr(), unless --advertise-addr is set (NAT/container). Never sign :0 or 0.0.0.0 as the join target. Tests use ephemeral bind then advertise local_addr().

  CLI:
  Keep --listen-addr, --data-dir, --peer-identity-path, --host-id.
  Add --directory, --directory-addr, --join-code BASE32, --create-vault, --advertise-addr <SocketAddr>.

  Host --create-vault: mint VaultId + JoinCode, bind, sign ad with advertise/local_addr, DirPut, HostService::new({local_id}). Print Base32 code once on stderr. Rotate: new code, DirPut, DirForget old; H accepts only the new code immediately.

  IMPLEMENT:
  backend/src/net/{mod,frame,directory,session,join}.rs. Activate listen_addr. Directory mode does not construct HostService.

  TESTS (localhost TCP, ephemeral ports):
  - Directory + host --create-vault + member --join-code: member on H; wrap live; raw 16-byte join code never appears in bytes captured before the first GcmPacket.
  - Tampered DirAd (addr/ek changed, old sig) fails before connect.
  - DirPut with valid sig but peer_id != peer_id(vk) rejected.
  - Replayed older DirPut after listen_addr/ek change rejected; joiners do not get the stale addr.
  - Attacker directory ad (attacker identity, attacker listen_addr) + real code: real H rejects; joiner keystore has no residual session/slot with the attacker after the code check fails (retire/discard).
  - Rotate code: old lookup/join fails; new succeeds.
  - Directory store has no member lists and no chunk plaintext.
  - Frame > 1 MiB closed; unknown type closed; frame_version≠1 closed.
  - Lost WrapAck: deadline tears down; next connect resends the same wrap (retry/pending), no second Encaps for that epoch.
  - Reconnect after host restart: wrap new epoch, refresh_mailboxes/re-seal, then flush; bodies open; live GcmChunk before flush is not applied.
  - Flush abort then retry: gate stays closed until Ok; no partial committed replica.
  - Existing host/pull/crypto tests still pass.
  - Directory mode does not start HostService.

  DO NOT:
  - Put Join or join_code in a bare (non-GCM) frame
  - Keep provisional pair slots after a failed Join
  - Pin retired K_ab across restarts instead of refresh_mailboxes
  - Reimplement X-Wing/AES/ML-DSA; raw ML-KEM Encaps; rewrite HostService commit path
  - Hostnames in ads; multi-thread runtime; durable chunks; path/tree FS; join approval; client/
  - git add -A / commit unless I ask

  VERIFY: cd backend && cargo test && cargo build && cargo clippy --all-targets -- -D warnings

  At completion:
  1. Handshake order, how codes stay out of pre-GCM bytes, advertise-addr, WrapAck, re-seal, directory replay/caps.
  2. Update backend/README.md. Still left: durable chunks, path/tree FS, kick, multi-vault if not done, client GUI.

## Plan
- [x] Resolve conflicting wire requirements; implement signed directory types/codecs and bounded TCP frames.
- [x] Implement coordinated provisional handshake, admission and transport adapters using existing crypto/host logic.
- [x] Wire CLI/daemon modes, exercise real localhost processes/reconnects, update README and verify all targets.

## Checkpoint   (overwrite in place · ≤10 lines · write BEFORE starting the next step)
done:       TCP directory/admission/reconnect implemented; 100 tests pass, build/strict Clippy/format pass; README and decisions updated.
in-flight:  Recording outcome and moving this task to done without staging or committing.
next:       Report completed transport behavior and remaining filesystem/durability scope.
open:       None. Single-vault daemon; restart queue tests retain memory state because durable chunks/queues are excluded.

## Outcome   (fill at completion · ≤10 lines · facts only)
changed:    backend/src/net, daemon/config, canonical codecs, context/epoch helpers, approved host transport adapters, README, 36 new tests and two decisions.
verified:   cargo test → 100 passed, 0 failed; cargo build → success; cargo clippy --all-targets -- -D warnings → success; cargo fmt --check → success; git diff --check → clean.
not-done:   Durable chunks/mailboxes, path/tree FS, kick, multi-vault hosting and GUI; pull/commit orchestration remains in-process.
gotchas:    User confirmed frame tags, directory raw-code exception, signed encrypted Join envelope and thin host adapters. Test temp-directory clock collisions fixed with atomic counters. No staging/commit performed.
