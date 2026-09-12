# Chunk identity, local store, pull
status: accepted
date: 2026-09-12      scope: sync
decision: >
  Because K_ab is per pair, ciphertext is not reusable. After decrypt, each
  member stores plaintext chunks plus the writer-signed manifest plus a
  have-bitset. chunk_id = SHA-256(file_id || u64be(index) || plaintext).
  Manifest plaintext is { file_id, ordered chunk_ids, size, writer_id,
  version } and is signed as in crypto-identity-bind.md. On send or pull,
  re-encrypt the plaintext under the recipient pair key (current epoch).
  PullRequest / PullResponse are pairwise packets; the body is one
  AES-256-GCM(K_ab, plaintext_chunk) with a random nonce next to the bytes.
  Do not AEAD that body a second time. Chunk AAD is file_id || u64be(index)
  || sender_id || epoch. A PullRequest lists at most 32 chunk_ids. Verify
  GCM then chunk_id∈manifest then hash(plaintext) before any local write.
  Pulls are idempotent. Notify-then-pull: committed saves publish a new
  manifest; bytes move on pull (or host fan-out; sync-host-queue.md).
why:
- Collapsed AES planes make ciphertext hashes pair-specific; ids must be over plaintext
- Encrypt-at-send matches Construction B without N stored copies per file
rejected:
- Ciphertext-hashed chunk ids — ids would differ per recipient
- Durable per-peer ciphertext replicas — rotation and storage blow up
- Double AEAD of chunk bodies — no extra confidentiality among the pair
