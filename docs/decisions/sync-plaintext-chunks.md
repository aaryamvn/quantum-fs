# Chunk identity, local store, pull
status: accepted
date: 2026-09-12      scope: sync
decision: >
  Because K_ab is per pair, ciphertext is not reusable. After decrypt, each
  member stores plaintext chunks plus the writer-signed manifest plus a
  have-bitset. chunk_id = SHA-256(file_id || u64be(index) || plaintext)
  with file_id 32 bytes. Manifest is { file_id, ordered chunk_ids, size,
  writer_id, version } signed per crypto-identity-selfcert.md. On send or
  pull, re-encrypt under the recipient’s current-epoch K_ab using chunk-body
  nonces in crypto-pairwise-aead.md (type_bit=1). Do not store a random
  nonce beside the body. PullRequest / PullResponse are pairwise GCM packets
  with no per-packet DSA. The body is one AES-256-GCM(K_ab, plaintext_chunk).
  Do not AEAD that body twice. Chunk AAD is in crypto-encoding.md (includes
  sender_id and receiver_id). PullRequest lists at most 32 chunk_ids. Verify
  GCM, then chunk_id∈manifest, then hash(plaintext), before any local write.
  Pulls are idempotent. Notify-then-pull; host fan-out in sync-host-tcb.md.
  v1: each member is TCB for plaintext they store; zeroize-on-drop applies
  to keys, not file bytes. Disk encryption of the chunk store is out of v1.
why:
- Ciphertext hashes are pair-specific; ids must be over plaintext
- Counter nonces remove a second nonce discipline on K_ab
rejected:
- Ciphertext-hashed chunk ids; durable per-peer ciphertext replicas
- Double AEAD of chunk bodies
- Random chunk nonces stored beside ciphertext — superseded with type_bit counters
