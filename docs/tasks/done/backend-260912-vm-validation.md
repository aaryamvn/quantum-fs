# Cross-VM backend validation
area: backend      status: done      opened: 2026-09-12      by: human
prompt: >
  test the existing backend with VMs on my computer. We must ensure that the practical parts of this software work before we wire the frontend.

## Plan
- [x] Inspect VM availability and backend entry points; prepare isolated Linux VMs and a concrete test matrix.
- [x] Build the existing backend and a thin test driver; run cross-VM admission, file operations, transfer and 100 ms control propagation.
- [x] Exercise restart/offline recovery, kick, multi-vault isolation and eviction/restoration; identify unwired mesh fallback and preserve reproducible defects.
- [x] Run regression checks, document repeatable commands and measured limits, clean up test processes and archive the task.

## Checkpoint
done: Linux 171 tests; VM matrix 16 passed/2 failed; network checks 11 passed; README/repro tools complete; network rules/processes cleaned and all VMs stopped.
in-flight: None; archive without staging or committing.
next: Fix candidate-session admission for member restart and durable initial replica bootstrap before frontend integration.
open: Mesh fallback was not tested end-to-end because the daemon lacks member pairing/serving orchestration. Powered-off guest disks (~6.8 GiB) and logs are retained for reproduction.

## Outcome
changed: backend/examples/vm_probe.rs, backend/scripts/{vm_validate,vm_network_validate}.py, backend/README.md; no production code, client changes, staging or commits.
verified: Ubuntu 24.04 ARM64 native cargo test → 171 passed; native release daemon/probe build, Clippy -D warnings and format checks passed; macOS example build/Clippy/format and Python syntax checks passed.
verified: Three actual Linux VMs: main matrix 16 passed/2 failed; 1.2 MB exact TCP transfer, tree edits, eviction/restore, forced H crash/offline coalescing, kick/history and multi-vault reload passed. Control visibility 122–180 ms (median 162; command/SSH/inspection overhead included).
verified: Network matrix 11 passed: malformed frames rejected; 1 MiB exact restoration under 50 ms delay/5 ms jitter/1% loss; partition detected in 29.9 s and daemon rejoined 1.1 s after restoration. Processes/rules removed; all three VMs stopped.
not-done: Member-only durable restart fails at confirmed-pair epoch guard; fresh admission lacks existing tree/files. Safe fixes need candidate pair promotion and durable bootstrap coverage separate from acknowledgment. Filesystem IPC, automatic online pulls and mesh orchestration remain unwired.
gotchas: Main report /private/tmp/qfs-vm-validation/final-20260912.json; network report /private/tmp/qfs-vm-validation/network-final-20260912.json; sibling log directories. Runtime/LIMA_HOME under the same root; user-v2 IPs H=.104.1, A=.104.3, B=.104.4; guest data/tools live in guest home because Ubuntu clears /tmp on reboot.
