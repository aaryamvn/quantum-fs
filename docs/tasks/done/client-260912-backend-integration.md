# Integrate the Tauri client with the real backend (cross-area: client + backend)
area: shared      status: active      opened: 2026-09-12      by: Aaryaman
prompt: >
  This is going to be a complicated, long-winded task which you must execute precisely and perfectly using dynamic workflows and sending out hundreds of agents. We need to actually integrate our extremely complicated backend with this tauri client. I can not stress this enough - we MUST execute this super perfectly. Every single little feature that exists on the frontend must not be static anymore and must perfectly work using the backend.
  Queuing actions must work perfectly and precisely using the vault server
  Files must actually open once they are clicked on, regardless of whether they are downloaded or not, and this must always happen as close to instantly as possible
  These files must be intelligently assembled by finding the relevant chunks and bringing them together using the daemon extremely quickly
  Kicking users must work precisely using the backend features implemented
  Joining a vault or adding a new vault to a server must work perfectly
  Adding a new server must work perfectly - we connect to servers via IP as previously discussed, so in the terminal or whatever opens up the actual vaults server please indicate the IP address of the server in the terminal so we can copy and paste it into the GUI to add/join the server, and later add vaults etc
  Ensure vault addition and calculation of storage availability based on clients connected works perfectly (with a small buffer for when clients might be offline maybe)
  Adding files, folders, all of the information rendered for these files and folders, all of the history rendered, etc - EVERYTHING must be dynamic
  While I would hope and assume that the backend already supports everything that the frontend requires of it, there may be circumstances where you have to modify certain parts of the backend to accommodate for these gaps. This is fine, and please exhibit as much autonomy as possible, but be extremely careful not to screw up any core algorithms, methods, encryption standards, novelty, or anything else that seems important. Be super meticulous and always double check your work.
  This has to be so real time and well built, despite all the chunking and peer to peer usage, that the SECOND i modify a file or folder on one client, it will immediately render on ALL other connected clients. Extremely and exceptionally important.
  Ensure you implement loading states and/or skeletons as they are required.
  We have only about 2 hours to get this done PERFECTLY so be extremely quick, precise, thorough, and make sure EVERYTHING is perfectly integrated.
  Ensure that all of the events are logged perfectly as they are expected to in the terminal so we can demonstrate it during the demo.
  Once we are done with this integration perfectly, open up THREE windows of virtual machines all running macos, with the quantumfs app open in each of them. These will all be my clients. Then, run TWO orchestration servers and ONE main central server on my own hardware. All the clients will connect to these two servers. Ensure the IP is sufficiently exposed. Make sure this is all setup smoothly and perfectly so that I can record a smooth demo.
  Ensure that no matter the file type, if i click it it will open eg word doc python file etc in the appropriate application (even for recent files displayed in the sidebar) - u no longer navigate to the location of the file but instead just open said file. same with from the command k switcher
  Ensure that the files are assembled without error based on how files are chunked in the backend across connected clients, and this is done efficiently and quickly.

## Plan
- [x] Pull teammate's backend commits (rebased; tree verified identical to a clean merge; no conflicts)
- [ ] Understand: map backend library API vs client seam (`client/src/lib/backend/client.ts`); list gaps
- [ ] Decide + record: client embeds `quantam_fs` as an in-process member node (own thread, current-thread runtime + LocalSet); servers stay `qfsd` processes; decision file `docs/decisions/client-backend-embed.md`
- [ ] Backend additions for gaps (member→H admin controls: create vault, kick, rotate code; vault/node metadata) without touching crypto/admission invariants
- [ ] Replace `client/src-tauri/src/{bridge,fs_state,fs_commands}.rs` fake with the embedded node; emit `BackendEvent`s to the webview
- [ ] Client UI: open files in OS default app (tile, recents, cmd-k); loading states; remove static data
- [ ] Verify: backend `cargo test`, client `npm run typecheck` + `cargo build`; 2-client end-to-end propagation
- [ ] Demo: qfsd directory + 2 hosts on host machine (LAN IP printed); macOS guest VMs via tart with the app

## Checkpoint   (overwrite in place · ≤10 lines · write BEFORE starting the next step)
done:       all six units landed and compile (backend: 33 suites + tests/admin.rs pass, clippy/fmt clean; client: typecheck+build clean; src-tauri: cargo build clean); T1/T2 reviewed + fixed; servers running via backend/scripts/demo_servers.sh (dir 172.26.28.115:7440, hosts :7447/:7448, admin :8447/:8448); tart VMs qfs-client-1/2 booted (192.168.64.4/.3), ssh keyed, share /tmp/qfs-demo/app mounted, admin port reachable from guests
in-flight:  all code units landed (backend 33 suites + tests/admin.rs + tests/net_rejoin.rs green; src-tauri builds with 0 warnings; client typecheck/build clean); servers restarted fresh on the new qfsd (short codes verified live: CREATE_VAULT → 6-char code); app bundle rebuilding; headless e2e test client/src-tauri/tests/e2e_node.rs still being finished
next:       stage bundle → vm-demo.sh redeploy (guests get QFS_DIRECTORY_ADDR=172.26.28.115:7440) → host-client → run e2e test against final code → human smoke; then commit (explicit paths) with Task trailer
open:       only 2 macOS guests can run → third client on the host; live cursors off (no member-to-member channel); e2e test still running against the pre-fix runtime

## Outcome   (fill at completion · ≤10 lines · facts only)
changed:    backend (admin port net/admin.rs, net/short_code.rs, heartbeat_once, re-admission with stale code, ops ring, tests/admin.rs, tests/net_rejoin.rs, README); client/src-tauri (node/* embedded runtime, bridge/fs_commands/fs_types/lib rewired, tests/e2e_node.rs); client/src (seam openFile/importFiles/vault-removed, 6-box join code, import action, loading states, shell notice); scripts demo_servers.sh + vm-demo.sh; docs/decisions/client-backend-embed.md
verified:   backend `cargo test` 41 result lines all ok + clippy -D warnings + fmt; client `npm run typecheck` + `npm run build` clean; src-tauri `cargo build` 0 warnings; `cargo test --test e2e_node` 6/6 consecutive passes (two nodes + real qfsd: create, join by 6-char code in ~0.5–1 s, folder/rename/color/file seen on the peer in 0–411 ms, 3 MiB import pulled + assembled byte-identical, kick notified in ~1 s, leave)
not-done:   live cursors/hover/drag presence (no member-to-member channel; sync-host-tcb forbids H relay); per-node access lists are advisory sidecar data (no ACL in protocol); setMemberRole rejects (host-vs-member only); daemon --create-vault CLI path still mints 26-char codes; host tears down a live session on a benign Link name collision instead of replying (backend follow-up); bootstrap mailbox is not restaged for a re-admitted member (backend follow-up)
gotchas:    admin port = listen port + 1000, LAN-reachable, token printed once as `qfsd: app connect string ip:port/TOKEN` (tokens change on every --fresh restart → wipe client state or re-add servers); clients need QFS_DIRECTORY_ADDR (or <data_dir>/directory.txt or a known server) to resolve 6-char codes; QFS_DATA_DIR runs several clients on one machine; macOS bash 3.2 treats a UTF-8 "…" after `$VAR` as part of the name under set -u; Apple Virtualization runs at most 2 macOS guests (third client on the host); screencapture is blocked for the agent's terminal
