# Backend deep bug audit: crypto, security, networking
area: backend      status: active      opened: 2026-09-12      by: Aaryaman
prompt: >
  You are doing a deep bug audit of the backend of this system. You need to look at the cryptography
  implementation, security practices, and networking and related implementations. If there is a bug,
  rigorously audit and fix it, and then document that you have done so, also indicate to me in your
  response to my prompt that you have done so. This system requires extremely high security and
  precision, so triple check before executing any fixes or patches and consult multiple agents and
  perspectives before deeming something is faulty. Your criteria for checking for bugs and gaps should
  not exclusively be the existence of a bug or gap but rather how something does not fit into the
  context declared within the internal documentation for agents. e.g. if some feature is supposed to
  be built in a certain way but the current implementation does not account for it, that would be
  worth patching and making airtight. Ensure that you do not fix what is not broken however.

## Plan
- [ ] Audit backend/ against the accepted decisions: parallel finders per subsystem and lens, then three
      independent refuters per finding (code trace, reproduction, decision/impact); repeat until dry.
- [ ] Fix only confirmed defects and decision gaps, one unit per file group, each reviewed; add regression tests.
- [ ] Full verify (cargo test, build, clippy -D warnings, fmt --check); update README boundary text if changed.

## Checkpoint   (overwrite in place · ≤10 lines · write BEFORE starting the next step)
done:       baseline: cargo build clean, cargo test 100 passed; decisions and done tasks read.
in-flight:  audit (read-only; no backend files changed yet). Temporary tests/audit_*.rs may appear and are removed after.
next:       classify verified findings; write fix specs; dispatch implementers one per file group.
open:       backend/ is the teammate's area; the human (Aaryaman) explicitly asked for this audit.

## Outcome   (fill at completion · ≤10 lines · facts only)
changed:
verified:
not-done:
gotchas:
