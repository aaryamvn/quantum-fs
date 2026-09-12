# Fix failures reproduced by VM validation
area: backend      status: done      opened: 2026-09-12      by: human
prompt: >
  fix all found bugs during the VM simulation and test.

## Plan
- [x] Review current code and accepted decisions; define safe candidate-session admission and durable initial-replica synchronization.
- [x] Implement member restart without losing a confirmed pair on failed admission; add regression coverage.
- [x] Implement authenticated, durable bootstrap for new members, including historical writers and failure/restart recovery.
- [x] Run Rust regression/build/lint checks and repeat the cross-VM and network-fault scenarios; update README and archive.

## Checkpoint
done: Both VM defects fixed and all verification completed; README updated. Three VMs stopped, binaries/disks retained, fault-injection rules removed, no test processes left.
in-flight: None.
next: None.
open: Frontend IPC and automatic online body/mesh orchestration remain unwired capabilities. No client/stage/commit.

## Outcome
changed: Candidate admission/keystore/session handling; durable encrypted bootstrap and canonical metadata; regression/VM harness fixes; backend README; net-candidate-admission and sync-replica-bootstrap decisions.
verified: cargo test — 182 passed on macOS and Linux; cargo build and cargo clippy --all-targets -- -D warnings passed on both; cargo fmt --all -- --check passed; Python monitor 4/4 and VM harness syntax checks passed.
verified: Three Linux VMs — 18/18 practical scenarios and 11/11 network-fault checks passed; demo control observations 99–157 ms including SSH/inspection overhead; partition detection 30.1 s, reconnect 1.46 s.
verified: Bounded lost-ack test helper rerun — 5/5 multi_vault tests and strict Clippy passed on both platforms. Identical qfsd/probe hashes on all guests; iptables clean and fq_codel restored.
not-done: No frontend IPC, automatic online body pulls/mesh orchestration, GUI, stage, or commit.
gotchas: Reports/logs under /private/tmp/qfs-vm-validation: bugfix-final-20260912.json, network-bugfix-final-20260912.json, mac-bugfix-tests.log, linux-bugfix-verify.log. Lima guests and rebuilt tools remain available there.
