# Pairwise AES via X-Wing wrap (Construction B)
status: accepted
date: 2026-09-12      scope: crypto
decision: >
  Each pair has one AES-256-GCM key K_ab for every packet and file byte on
  that pair. Construction B: initiator generates 32 random bytes K_ab;
  (ss, kem_ct) ← X-Wing-Encaps(other.ek); wrap_key ← HKDF-SHA256(ikm=ss,
  salt=empty, info as crypto-encoding.md, L=32); wrap_ct ← AES-256-GCM(
  wrap_key, nonce=12 zero bytes, AAD=wrap AAD, pt=K_ab). wrap_key is
  single-use: never persist; one Encaps per wrap; retry resends the same
  ciphertext. Both discard ss and wrap_key; persist K_ab. First contact:
  smaller peer_id initiates. Later, either peer may initiate; epoch u64
  strictly increases (next is last+1). Duplicate epoch from two initiators:
  keep the smaller initiator’s wrap, the other retries at last+2. On every
  process start, initiate a new epoch per known pair (do not reuse K_ab or
  counters across restarts). Rotate at least weekly the same way. Rotation
  is not forward secrecy: static ek, so stolen dk unwraps stored kem_ct.
  Rotation bounds GCM volume and a leaked K_ab once the old epoch key is
  dropped. v1 does not use ephemeral ek. Nonce (96 bits) is type_bit
  (0=packet, 1=chunk body) || dir_bit (1 iff sender_id > receiver_id as
  unsigned 32-byte compare) || 30 zero bits || u64be(counter). seq in AAD
  is that counter. Independent sliding window W=1024 per (peer, epoch,
  direction, type): H = highest accepted counter; reject if counter ≤ H−W
  or already seen; otherwise accept and if counter > H set H to counter.
  Packet and chunk AAD both include sender and receiver. Keep previous
  epoch key until in-flight drains, then drop.
  Kick: delete the pair; already-held K and plaintext are not erased.
why:
- Crash restore of a counter under a live K_ab is GCM nonce reuse / forgery
- Mixed random and counter nonces on one key collide in the same space
- in-order seq reject drops valid reordered traffic; no transport was named
rejected:
- Construction A; two AES planes; group AES key
- Random chunk nonces; seq ≤ last_seq as the only replay rule
- Ephemeral KEM per epoch in v1 — not accepted; FS limitation is documented instead
