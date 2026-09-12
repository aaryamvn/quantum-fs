# Identity bind and signatures
status: accepted
date: 2026-09-12      scope: crypto
decision: >
  Each peer’s identity document is (peer_id, ek, vk, created_at) signed by
  that peer’s ML-DSA sk. Recipients persist it and refuse any ML-KEM ek that
  is not under a valid signature for that peer_id. Packets are
  encrypt-then-sign: ML-DSA over the concatenation of the clear header,
  ciphertext, and GCM tag. The wrap message (kem_ct, wrap_ct, epoch, min_id,
  max_id) is likewise ML-DSA-signed by the generator. File manifests are
  ML-DSA-signed by the writer; do not attach a signature per chunk. Hash
  payloads with SHA-256 and domain-separate with UTF-8 prefixes before
  signing: "qfs/v1/id", "qfs/v1/wrap", "qfs/v1/pkt", "qfs/v1/manifest".
  AES-GCM integrity is not origin: both ends hold K_ab, so DSA is what
  names the sender and the writer.
why:
- Unbound ek lets a MITM encapsulate K_ab to themselves
- Encrypt-then-sign rejects junk before decrypt
- Per-chunk ML-DSA-65 is large; a signed manifest of chunk ids is enough
rejected:
- Sign-then-encrypt packets — must decrypt to authenticate
- Unsigned KEM keys — identity misbinding
- Per-chunk signatures in v1 — cost; GCM plus signed chunk_id list suffices
