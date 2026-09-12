# Pairwise AES via ML-KEM wrap (Construction B)
status: superseded-by: docs/decisions/crypto-pairwise-aead.md
date: 2026-09-12      scope: crypto
decision: >
  Each pair (A, B) has one AES-256-GCM key K_ab. That key encrypts every
  packet on the pair and every file byte sent on the pair. No per-file
  content key; no network-wide AES key. Establish K_ab with Construction B:
  the lexicographically smaller peer_id generates 32 random bytes K_ab;
  (ss, kem_ct) ← Encaps(other.ek); wrap_key ← HKDF-SHA256(ikm=ss, salt=empty,
  info="qfs/v1/wrap/pair/{min_id}:{max_id}/{epoch}", L=32); wrap_ct ←
  AES-256-GCM(wrap_key, K_ab); send (kem_ct, wrap_ct) under the wrap
  signature rules in crypto-identity-bind.md; both discard ss and wrap_key
  and persist K_ab. wrap_key never encrypts packets or chunks. Rotate at
  least weekly: new K_ab, new wrap, bump epoch (u64). Keep only the previous
  epoch key until in-flight packets drain, then drop it. Packet nonces are
  96-bit directional counters (high bit = sender is the larger peer_id);
  reject seq ≤ last_seq for that epoch. Chunk-body nonces are random 96-bit
  and stored beside the ciphertext. Packet AAD is version || sender_id ||
  receiver_id || epoch || seq. Kick: delete the pair, stop wrapping new
  epochs to that peer; already-held K and plaintext are not erased.
why:
- VISION: pairwise AES for communication, PQ KEM to exchange the key, weekly rotate
- Human: Construction B, one AES role for packets and file bytes
rejected:
- Construction A (KEM-derived K_ab) — human chose generated-then-wrapped
- Separate transport key and content key — human collapsed the two planes
- One group AES key — any member could decrypt every packet; worse on kick
