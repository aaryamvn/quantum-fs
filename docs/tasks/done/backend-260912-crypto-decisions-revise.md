# Revise crypto/sync decisions from review
area: backend      status: done      opened: 2026-09-12      by: Justin
prompt: >
  Accept and fix documents for all except KEM per epoch, and note all changes made to docs

## Plan
- [x] Supersede crypto suite, identity, pairwise AES; add encoding
- [x] Supersede sync chunk/host and backend-stack; leave ephemeral KEM out
- [x] Point README at the new decisions

## Checkpoint
done:       seven accepted decisions; six superseded; README pointers
in-flight:  none
next:       none
open:       none

## Outcome
changed:    docs/decisions/* (six superseded, seven accepted); backend/README.md
verified:   accepted files exist and old files status is superseded-by
not-done:   ephemeral KEM per epoch (explicitly excluded); no commit
gotchas:    implement from the seven accepted files only, not the superseded ones
