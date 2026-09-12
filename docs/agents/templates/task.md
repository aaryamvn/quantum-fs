# <title>
area: client | backend | shared      status: active      opened: YYYY-MM-DD      by: <human>
prompt: >
  <the human's request, verbatim. A resuming agent matches its prompt against this line.>

## Plan
- [ ] <step>
- [ ] <step>

## Checkpoint   (overwrite in place · ≤10 lines · write BEFORE starting the next step)
done:       <steps finished, one line>
in-flight:  <file — what is half-written or failing; "none" if clean>
next:       <the exact next action>
open:       <questions for the human / blockers; "none">

## Outcome   (fill at completion · ≤10 lines · facts only)
changed:    <paths>
verified:   <command → result>
not-done:   <what was left out and why>
gotchas:    <non-obvious facts the next person in this area needs>
