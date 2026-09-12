# Demo terminal event log
area: backend      status: done      opened: 2026-09-12      by: human
prompt: >
  For the purposes of our demo, we will have 5 VMs displayed on my two monitors. In the corner, I will have a terminal running, on which we will display a LOG of all of the events taking place. Strictly for demo purposes it is imperative we log all of the complicated backend events happening for each action taken on the apps by the clients in a terminal in a very clean, presentable manner. It should be very intuitive, color coded, and render thoughtfully selected events. Some things we do want to render to showcase the novelty in our solution include when file pieces are picked up from multiple clients and reassembled at the requester - that would be ONE log event and it will span maybe one primary line with a few indents made underneath to show which clients we picked up pieces from by their unique identifier or whatever. During these transmissions you can potentially log just for show the encryption standard we are using at the start like [encryption standard] [event name]. Whatever else you think is worth logging that showcases our novelty extremely clearly and intuitively, please do so. But we do not want to be logging things like polling all of the clients every 0.1 seconds or whatever, so be smart about it. The idea is that it should render events GENERALLY at a readable and processable frequency so it shouldnt be ridiculously fast but shouldnt necessarily be slow either. Every single time the central and vault servers are ever started up must this logging tool pop up. Always, without exception In the terminal. Do it quickly

## Plan
- [x] Implement a shared color-coded terminal event renderer and automatic daemon startup.
- [x] Instrument meaningful admission, file, transfer, offline recovery, and membership boundaries; suppress routine polling.
- [x] Verify formatting and backend regressions; document demo usage and commit scoped changes.

## Checkpoint
done: Automatic logger, verified-source aggregation, semantic hooks and multi-node monitor implemented; regression and three-process smoke checks pass.
in-flight: None.
next: Integrate branch backend/demo-event-log with concurrent backend fixes when that session is ready.
open: User confirmed concurrent backend bug fixes; leave original worktree untouched. No client changes planned.

## Outcome
changed: backend/src demo_log/startup/network/filesystem/pull hooks; backend/demo_monitor.py; backend tests and README.
verified: cargo test --offline (full suite), cargo build --offline, cargo fmt --check, cargo clippy --offline --all-targets -- -D warnings; four Python monitor tests pass.
verified: Three disposable localhost daemons automatically log/mirror; idle heartbeats stay silent; combined monitor and vault restart banner pass. Real encrypted two-holder test verifies grouped sources and partial/idempotent accuracy.
not-done: Original worktree and existing VM binaries were deliberately left untouched to isolate the concurrent bug-fix session.
gotchas: Worktree /private/tmp/qfs-demo-event-log at base b295e34; localhost tests require sandbox escalation. Rust toolchain/cache reused from /private/tmp/qfs-rust-toolchain.KiGtuj; build outputs isolated. Combined monitor uses existing noninteractive SSH, no added server ports.
