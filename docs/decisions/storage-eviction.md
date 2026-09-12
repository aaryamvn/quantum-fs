# Local member cache eviction
status: accepted
date: 2026-09-12      scope: storage
decision: >
  Eviction removes local plaintext and chunk-index rows while retaining dirents
  and signed manifests. It is not Unlink, Clear, Remove or a logged instruction.
  Persist a staged metadata snapshot before making the removal visible; unlink
  unreferenced chunk files after that durable boundary using existing cleanup.
  H refuses eviction because its full replica is the availability copy. Members
  require a live, ungated current pair to H and refuse removal of mailbox-pinned
  chunks. This protects against permanent loss only while H can restore; refuse
  eviction when H is unreachable rather than weakening H-down mesh availability.
  Chunk IDs include file_id and index, so different files do not share chunks;
  evict_file needs no cross-file reference count. Derive have_bitset directly
  from ChunkStore::has over retained plaintext, without an advertisement cache.
  A stale nominated holder can return no requested chunks: pull callers must
  try the next locator holder rather than reporting successful delivery. An
  idempotent receive of an already-held chunk remains successful.
why:
- Storage pressure must not change the shared filesystem namespace or its history.
- Live H availability and honest have bits preserve restore and fallback behavior.
rejected:
- Eviction on H or while H is unreachable, control records for cache changes, and refcounts across files.
