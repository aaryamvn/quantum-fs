# Post-quantum suite and AEAD
status: accepted
date: 2026-09-12      scope: crypto
decision: >
  v1 uses ML-KEM-768 (FIPS 203), ML-DSA-65 (FIPS 204), AES-256-GCM, and
  HKDF-SHA256. Each peer generates independent long-term ML-KEM (ek, dk) and
  ML-DSA (vk, sk) keypairs; do not share seeds across algorithms. Publish only
  ek and vk. Zeroize dk, sk, AES keys, KEM shared secrets, and HKDF output on
  drop. Raise to ML-KEM-1024 / ML-DSA-87 only in a superseding decision.
why:
- Category 3 is the usual pair with AES-256
- FIPS parameter sets; no home-grown PQ
rejected:
- ML-KEM-1024 + ML-DSA-87 for v1 — larger keys and signatures, not required yet
- Classic DH/ECDH hybrid for v1 — VISION asks for post-quantum key exchange
