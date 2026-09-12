# Canonical byte encoding
status: accepted
date: 2026-09-12      scope: crypto
decision: >
  One encoding is used for HKDF info, AAD, ML-DSA M, and wrap fields:
  fixed-width big-endian; peer_id, FileId, ChunkId, vk-hash inputs as raw
  bytes (peer_id is 32 bytes); Epoch and Seq are u64be; protocol version is
  u8 and is 1 in v1. Length-prefix every variable field as u32be || bytes.
  Do not hex- or Debug-format ids. One serialize function per object; those
  bytes are AAD and DSA M (no second layout). Wrap HKDF info is ASCII
  qfs/v1/wrap/pair/ || min_id || 0x3a || max_id || 0x2f || u64be(epoch)
  with min_id < max_id as unsigned 32-byte strings. Packet AAD is u8
  version || sender_id || receiver_id || u64be(epoch) || u64be(seq). Chunk
  AAD is packet AAD || file_id || u64be(index). Wrap GCM AAD is min_id ||
  max_id || u64be(epoch). Identity M is peer_id || u32be(len(ek)) || ek ||
  u32be(len(vk)) || vk || u64be(created_at Unix seconds). Wrap M is
  u32be(len(kem_ct)) || kem_ct || u32be(len(wrap_ct)) || wrap_ct ||
  u64be(epoch) || min_id || max_id. Manifest M is file_id || u64be(version)
  || u64be(size) || writer_id || u32be(n) || n × ChunkId. Flush M is the
  32-byte challenge from H.
why:
- Prior crypto/sync files deferred layout; AAD/sign/HKDF mismatch is an interop and auth hole
rejected:
- UTF-8 interpolation of ids into HKDF info — width and endian not defined
- Separate sign-bytes vs AAD-bytes — they drift
