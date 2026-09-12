# PQ hybrid suite and AEAD
status: accepted
date: 2026-09-12      scope: crypto
decision: >
  v1 KEM is X-Wing (X25519 + ML-KEM-768), not raw ML-KEM Encaps. Signatures
  are Pure ML-DSA-65 (FIPS 204). Bulk is AES-256-GCM. KDF is HKDF-SHA256.
  Each peer generates independent long-term ML-DSA (vk, sk) and X-Wing
  (ek, dk) keypairs; do not share seeds. Publish only ek and vk. Zeroize
  dk, sk, AES keys, KEM shared secrets, and HKDF output on drop. Construction
  B Encaps/Decaps are X-Wing; ek is the X-Wing public key. ML-KEM-768 is
  NIST Category 3 (AES-192 analog); AES-256-GCM is the bulk cipher and is
  not claimed to match KEM category. Do not encapsulate each epoch to an
  ephemeral ek in v1 (static ek). Raise 1024/87 or drop hybrid only by a
  superseding decision.
why:
- Hybrid keeps a classical floor if lattices or an ML-KEM implementation fail
- VISION requires post-quantum exchange; X-Wing is still a PQ KEM
- AES-256-GCM is cheap; Category 3 is the common interop KEM default
rejected:
- Raw ML-KEM-768 only — no classical floor (docs/decisions/crypto-pq-suite.md)
- Classic ECDH without ML-KEM — not post-quantum
- ML-KEM-1024 in v1 to match AES-256 on paper — size; keep 768 + AES-256
- Ephemeral KEM per epoch in v1 — deferred; see crypto-pairwise-aead.md on FS
