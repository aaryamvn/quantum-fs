# Self-certifying identity and what is signed
status: accepted
date: 2026-09-12      scope: crypto
decision: >
  peer_id := SHA-256(ASCII qfs/v1/peer || vk_bytes) (32 bytes); vk_bytes is
  the FIPS 204 ML-DSA-65 public-key encoding. A document cannot assert
  another peer’s id without that vk. Identity document is (peer_id, ek, vk,
  created_at) signed by sk; created_at is u64be Unix seconds. Recipients
  persist it and refuse ek unless the signature verifies under vk and
  peer_id equals that hash. ek may rotate under the same vk (same peer_id).
  Rotating vk changes peer_id (new principal). Domain-separate with FIPS 204
  ctx (≤255 bytes): qfs/v1/id, qfs/v1/wrap, qfs/v1/manifest, qfs/v1/flush. M is the
  canonical encoding in crypto-encoding.md. Use Pure ML-DSA for those
  objects. Do not ML-DSA-sign ordinary pairwise packets or live cursors;
  AES-GCM with directional nonces authenticates the pair against network
  attackers. Keep DSA on identity, wraps, writer manifests, and mailbox
  flush (authorship that must survive H re-encryption).
why:
- A chosen peer_id plus a self-signature allowed identity substitution
- FIPS 204 ctx is length-delimited; DIY prefix||hash is not HashML-DSA
- Per-packet ML-DSA-65 is ~3.3KB and worse flood resistance than GCM
rejected:
- Self-signed chosen peer_id — MITM mints a new (ek, vk) under the victim’s name
- SHA-256(prefix||payload) then sign — prefix hazards; not FIPS HashML-DSA
- Encrypt-then-sign on every packet — cost; pair GCM already stops off-path forgery
