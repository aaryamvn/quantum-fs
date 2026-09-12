# Record backend crypto and host-queue schema
area: backend      status: done      opened: 2026-09-12      by: Justin
prompt: >
  write this encryption plan/schema into the docs for agents to build it later on.

## Plan
- [x] crypto suite, Construction B pairwise AES, identity bind
- [x] encrypt-at-send chunks, designated-member host queue
- [x] backend stack (Rust); point README at the stack decision

## Checkpoint
done:       six accepted decisions + README stack pointer
in-flight:  none
next:       none
open:       none

## Outcome
changed:    docs/decisions/crypto-pq-suite.md crypto-pairwise-aes.md crypto-identity-bind.md sync-encrypt-at-send.md sync-host-queue.md backend-stack.md; backend/README.md
verified:   wc -l on those decision files → 15–28 lines each (cap ~40)
not-done:   no Rust crate; no commit (not asked)
gotchas:    only these decisions bind; implement from them, do not re-derive from this task
