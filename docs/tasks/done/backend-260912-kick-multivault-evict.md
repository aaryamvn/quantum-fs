# Kick, multi-vault hosting, and local eviction
area: backend      status: done      opened: 2026-09-12      by: human
prompt: |
  You are continuing the backend. Do not scaffold. Do not reimplement crypto, pull encrypt-at-send, host commit/flush/re-seal internals, tree ops, locate encodings, or the TCP/join handshake. Language: Rust. Follow docs/agents/PROTOCOL.md in full. Stay in backend/ plus your task/decision files. Do not touch client/.

  Create docs/tasks/active/backend-260912-kick-multivault-evict.md from docs/agents/templates/task.md; prompt: this message verbatim.

  CHECKPOINT: before starting each Plan checkbox, overwrite that task file's Checkpoint in place (≤10 lines) with done / in-flight / next / open from the template. Write it before the next step, never only at the end. If you skip a checkbox, put the reason in open:.

  READ FIRST:
  - docs/agents/PROTOCOL.md
  - docs/VISION.md (kick leaves old keys/copies; eviction is storage pressure, not unlink)
  - backend/README.md
  - backend/src/protocol/manifest.rs (TrustedManifest::verify requires writer_id ∈ the members set it is given)
  - backend/src/sync/host.rs (ControlUpdate kinds 0–6, fan_out_control, add_member, publish_state)
  - backend/src/sync/host/persistence.rs (verified_manifests uses metadata.members — the live set)
  - backend/src/sync/host/filesystem.rs
  - backend/src/sync/pull.rs (serve skips missing ids with no error)
  - backend/src/sync/locate.rs
  - backend/src/encoding.rs (encode_control_record, encode_vault_metadata / decode_vault_metadata finish())
  - backend/src/net/join.rs (VaultHost, admit, serve_host discard_pair on Join failure, rotate_code, JoinRequest.vault_id)
  - backend/src/net/session.rs
  - backend/src/daemon.rs
  - backend/src/config.rs
  - backend/src/store/{durable,chunks}.rs
  - backend/src/keystore.rs (retire / discard_pair — peer-keyed, not (peer, vault))
  - docs/decisions/sync-host-tcb.md
  - docs/decisions/net-vault-join-directory.md
  - docs/decisions/storage-replica.md
  - docs/decisions/sync-tree.md
  - docs/decisions/crypto-keystore.md
  - docs/agents/templates/decision.md

  Skip superseded-by files. If any docs/tasks/active/* besides the file you just created exists, stop and tell the human.

  GOAL: (1) H can kick a member. (2) One H process can host several vaults. (3) Members can evict local chunk plaintext without unlinking. Keep persist-before-visible. Keep encrypt-at-send. Do not elect a successor. Do not start the GUI.

  Write three accepted decision files (new files; do not edit other decision bodies):
  - docs/decisions/sync-kick.md — include historical membership on replay (below), the A↔B window, denylist vs rotation, and discard_pair on remaining members
  - docs/decisions/net-multi-vault.md — include Join rejection when the peer already has a live pair bound to another vault, and discard-only-provisional
  - docs/decisions/storage-eviction.md — include have_bitset derivation, pull fallback on empty holder, refuse evict unless H session is live, and that chunk_ids are not shared across files so evict_file needs no refcount

  Implement in this order and do not skip ahead: kick + historical membership + denylist → multi-vault layout/Join dispatch/provisional discard → member-local eviction.

  ALREADY DONE — call, do not rewrite:
  - Construction B, AES-GCM, Pure ML-DSA, keystore lock, atomic_private_write (file + parent fsync), replica.bin header/hash, chunks/<hex> write-once
  - HostService commit/fan-out/offline mailboxes, kinds 0–6, Add is a deprecated no-op, any member may tree-mutate, H serializes
  - persist-before-visible via publish_state; log truncate by min acked_through over current members
  - TrustedManifest::verify(manifest, keys, members) rejects writer_id ∉ members. Live commits must keep that check against the membership set as of that record. Snapshot reload must not pass the post-kick live set into that check for historical manifests (that is the bug this task exists to avoid)
  - Join Identity→EpochHint→Wrap→WrapAck→Join-in-GCM; TCP frame types 1–14; JoinRequest M has vault_id || join_code || identity_m
  - Directory map is join_code → DirectoryAd only; ads keyed by (peer_id, vault_id); rotate_code already DirPut + DirForget
  - Locator qfs/v1/have/; holders require at least one matching bit; accept() requires live dirent + current manifest
  - serve() skips missing chunk ids with no error — pull callers must not treat an empty response from a nominated holder as success
  - VaultHost::rotate_code; KeyStore::retire / discard_pair (drops that peer's pair entirely)
  - Existing tests must stay green

  NORMS (do not pick a silent alternative):

  KICK
  1. Only H may kick. fan_out_control: Kick requires sender_id == host_id. Tree's any-member trust does not extend to membership. Cannot kick H. Cannot kick a non-member. No unkick in this task.

  2. Keep kinds 0–6 bytes unchanged. Kind 7 Kick: peer_id (32 bytes). Unknown kind stays fail-closed. Kick is a logged control instruction (u64be id || u8 7 || peer_id), not a new TCP frame.

  3. Membership is historical, not live-set retroactive. State this in sync-kick.md: kick is a membership change, not a history purge.
     - Live commit of NewManifest / kinds 4–6: check sender/writer against membership as of this new record (current members, before Kick is applied).
     - Catch-up / log apply: walk records in id order; add_member and Kick mutate a running membership set; each manifest/tree record is authorized against that set at its id.
     - Snapshot load (verified_manifests / open_durable): re-verify DSA and peer documents; do not require writer_id ∈ the post-kick live members. Reconstruct authorization by walking the retained log from snapshot.members-as-of-the-truncation-floor (see 4) or by treating snapshotted manifests as already-accepted (DSA still required). Either way, restart after kicking B must keep B's files and directories.
     - TrustedManifest::verify's members argument must be the historical set for that record, never a blindly substituted live set on reload.

  4. Truncation floor (min acked_through over current members including H) may drop after Kick because B no longer pins. Only truncate records already folded into snapshot fields (dirents, manifests, members, mailboxes, chunk_index). After truncate, snapshot.members is membership after the last removed id; any later log replay starts from that set. Do not truncate a Kick until it has been applied into snapshot.members.

  5. HostService::kick(target) -> Result<PeerId>:
     Stage: remove target from members, acked_through, presence, online, mailbox; append to denied; assign a new join_code in the same staged vault/replica metadata as the membership change (so a crash cannot leave B removed while the old code still admits); push Kick to remaining members only (never to the kicked peer); publish_state (replica.bin AND vault admission file in one persist-before-visible sequence: if either write fails, neither is visible); return Ok(target).
     After Ok, the caller tears that TCP down. Do not invent a global session registry: serve_live / VaultSet drain HostService/VaultHost::take_disconnects() -> Vec<PeerId> populated by kick's return value. In-process tests without TCP still get Ok(target) and discard_pair.

  6. Remaining members, when they apply Kick, must discard_pair(target) too (not only H). Application-layer require_member is not enough: B's packets would still GCM-authenticate on A.

  7. A↔B window: membership is eventually consistent. Offline members are gated by existing flush-before-live (Kick is in the mailbox; they apply it before live traffic). Online members may still mesh with B until they apply the Kick control. Accept that window; bound it in sync-kick.md; do not require draining H before serving every pair (that would block the H-down mesh). After Kick is applied, discard_pair closes it.

  8. Denylist: persist denied peer_ids for this vault. admit() rejects them even with the current code. This is accidental/lazy rejoin only. peer_id is self-certifying and free to mint; a kicked user with a new identity and the current code gets in. Code rotation is the actual control. State that in sync-kick.md; do not describe denied as a security boundary.

  9. Vault admission encoding: after the existing members list, if no bytes remain, denied is empty and there is no extension (legacy faf7bdd files). If bytes remain: u32be ext_len || ext. Inside ext: u8 ext_version=1 || u32be denied_n || denied_n × PeerId || u32be code_len || join_code bytes if you must store a not-yet-directory-published code, otherwise just denied. reader.finish() after ext. Future fields go inside a bumped ext_version, not another "if remaining" fork. Do not break existing vault files.

  10. After local persist of the new code, DirPut the new DirectoryAd and DirForget the old one (existing rotate_code helpers). If directory I/O fails: return the error (do not swallow); local admit already uses the new code so the old code does not admit. Test that.

  11. Kick does not wipe the kicked peer's disk, does not make historical plaintext unreadable, and does not revoke old K_ab they already hold (VISION). Stopping new access is: not a member, not in fan-out, pairs discarded, old join code dead.

  MULTI-VAULT
  12. One H process, many vaults, one KeyStore and one peer_id. Each HostService/replica is one vault. expected_root remains FileId(vault_id.0). Directory is unchanged.

  13. On-disk: data_dir/vaults/<64 lowercase hex vault_id>/{vault, replica.bin, chunks/}. Identity, .keys, .lock stay at data_dir. No replica.lock.

  14. Legacy migration (crash-safe; same class of failure storage-replica.md forbids):
      Detection is not "vaults/ exists." Write data_dir/.migrate-vaults marker (atomic_private_write) BEFORE touching replica/chunks. Marker body: vault_id || phase u8.
      Move replica.bin, chunks/, and vault by rename on the same filesystem (not copy), each with parent-directory fsync as atomic_private_write already does for a single file; for directories use rename of the chunks dir.
      Remove legacy paths, then clear the marker.
      Startup: if the marker is present, resume the recorded phase or roll back to the pre-move layout; do not guess from directory existence. State resume-vs-rollback in net-multi-vault.md and implement one of them (prefer resume if vaults/<id>/replica.bin verifies, else rollback).
      Creating a second vault runs this migration if the legacy single-vault layout is still in use.

  15. Each live TCP session binds to exactly one vault_id at Join. serve_host takes a vault map. After Join plaintext, dispatch by request.vault_id. Track peer_id → bound vault_id for live sessions.

  16. If the joining peer already has a live pair bound to a different vault on this H: fail with a distinct error (not AuthenticationFailed-generic), leave the existing session and mailbox untouched, do not discard_pair. Same-peer two vaults on one H remains out of this task.

  17. "Discard provisional pair" means only a pair minted during THIS handshake. An existing member who presents the wrong vault_id or the other vault's code must not lose their good session. Today's serve_host discard_pair on any admit failure is the bug; fix it. A brand-new peer whose Join fails still discard_pair as today.

  18. Member qfsd still joins one vault (--join-code). --create-vault on a data_dir that already hosts vaults adds a vault, publishes its code, prints the new Base32 code, keeps serving the others. One listener. Weekly wrap rotation refreshes mailboxes for every hosted vault.

  EVICTION
  19. Eviction is local cache, not Unlink/Remove/Clear, not a control record, not logged. Keep dirents and manifests. Drop chunk files + in-memory plaintext (so have_bitset, which is ChunkStore::has over that map, clears those bits) + chunk_index rows. Then persist replica.bin. Do not keep a separate advertisement map.

  20. chunk_id = SHA-256(file_id || index || plaintext) is not shared across files. evict_file deletes that file's chunk set with no refcount. State this in storage-eviction.md.

  21. HostService::evict_file errors (H is the availability copy). MemberReplica::evict_file(file_id) errors unless KeyStore has a live, ungated current_session to H (H-down mesh must not lose the last reachable copy). Also refuse if any undrained mailbox QueueContent::Chunk names those ids. storage-eviction.md: eviction is safe against permanent loss only while H can restore; it is refused when H is unreachable rather than silently degrading Locator fallback.

  22. Pull: a nominated holder returning zero of the requested ids is not success. Try the next Locator holder. Do not change serve() skipping missing ids; change the caller. Locator have-replies already require at least one matching bit — after eviction those bits must be false so the evicted member is not nominated.

  DO NOT:
  - Touch client/; successor election; auto-failover of H; unkick
  - Wipe kicked disks; pin retired K_ab; durable GCM envelopes
  - New TCP frame types; change kinds 0–6; vault_id on every GCM packet
  - Re-verify historical manifests against the post-kick live member set
  - discard_pair of a pre-existing live session on a failed Join to a second vault
  - Guess migration completion from vaults/ existing
  - Evict on H; evict when H session is dead; treat eviction as unlink
  - Describe denylist as preventing a new self-certified identity
  - git add -A / commit unless the human asks

  TESTS:
  Kick
  - B writes a file, H kicks B, drop process, reopen H: B's dirent+manifest+plaintext still load; verified_manifests must not fail.
  - H kicks B: A and H no longer list B; B cannot pull/mkdir; admit(B) fails with old and new codes; Kick never in B's mailbox; take_disconnects contains B.
  - Persist failure: members and join code unchanged; take_online_control empty for that Kick.
  - Directory DirPut fails after local persist: old code does not admit; error is returned.
  - Kick H or self or outsider rejected.
  - After kick, log prefix truncates once remaining members ack (B no longer pins).
  - A applies Kick: A's discard_pair(B) so B→A GCM is not session-valid.
  - Online window: document-only assertion that A may still have a session with B until Kick is applied; after apply, it is gone.
  - Live TCP to B closes.

  Multi-vault
  - One H, two vaults, two join codes, two members: disjoint trees.
  - Member of vault A presents vault B's valid code (or wrong vault_id): distinct error; A's session and mailbox intact (no discard_pair of A's pair).
  - Brand-new peer wrong code: provisional pair discarded; no member inserted.
  - Restart H: both replicas and both codes reload.
  - Legacy single-vault data_dir still opens.
  - Second create-vault migrates the first; kill between chunks rename and marker clear: reopen resumes or rolls back to a consistent replica (no silent half-move).

  Eviction
  - Member evict_file with live H session: chunk files gone, have_bitset false for those ids, dirent+manifest remain, Locator does not nominate that member, pull from H restores.
  - evict_file while H session dead: error; bits unchanged.
  - Nominated holder (before bit update would have lied) returning 0 ids: caller tries next holder.
  - H evict_file errors; H chunk files remain.
  - Evict racing an in-flight serve: persist-before-visible still; leftover serve may skip ids; requester falls back.
  - Existing host/pull/tree/locate/net tests stay green.

  VERIFY: cd backend && cargo test && cargo build && cargo clippy --all-targets -- -D warnings

  At completion:
  1. Historical vs live membership; what kick does not do; denylist vs rotation; Join/provisional discard; migration marker; eviction vs unlink and H-down.
  2. Update backend/README.md.
  Still left: successor election, client GUI, unkick, same-peer multi-vault mux.

## Additional prompt
> for the H server, make heartbeat checking ever 0.1 seconds to decrease the latency for demos, so that information transfer appears near instant. If any of that's not possible, tell me and tell me why

## Plan
- [x] Implement kick, historical membership verification, atomic admission rotation, denylist, disconnects, and focused tests.
- [x] Implement multi-vault layout, recoverable migration, Join dispatch and provisional-only discard, with tests.
- [x] Implement member-local eviction and pull fallback, with tests.
- [x] Set H heartbeat polling to 100 ms for demos and verify online propagation.
- [x] Update README, run all required verification, and archive the completed task.

## Checkpoint
done: Kick/history, atomic admission, multi-vault/migration, eviction/fallback, 100 ms heartbeat, README and full verification.
in-flight: None; archiving the completed task without staging or committing.
next: Human review; successor election, client GUI, unkick and same-peer multi-vault multiplexing remain separate work.
open: No skipped steps. Bulk transfer remains pull-driven; silent-loss presence timeout remains 30 seconds.

## Outcome
changed: Backend host membership/history, coordinated admission persistence, vault dispatch/migration, local eviction/pull fallback, 100 ms control polling, tests and README; three accepted decision files.
verified: cargo test → 171 passed, 0 failed, 0 ignored; cargo build → qfsd produced; cargo clippy --all-targets -- -D warnings → passed; cargo fmt --check → passed; scoped git diff --check → passed.
verified: Restart preserves kicked-writer history; failed writes roll back; two vaults remain separate; bad second-vault Join preserves the existing pair; interrupted migration resumes; eviction restores through fallback; flush-history tampering retries atomically.
not-done: Successor election, client GUI, unkick and same-peer multi-vault multiplexing.
gotchas: Kick cannot erase copied bytes or old keys; new identities can bypass denial with the current code. Eviction requires a live ungated H pair. Heartbeat speeds controls, not automatic file-body transfer; silent-loss TTL remains 30 seconds. No staging or commits.
