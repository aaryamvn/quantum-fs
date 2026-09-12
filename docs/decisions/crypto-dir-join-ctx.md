# Directory and vault-admission signing contexts
status: accepted
date: 2026-09-12      scope: crypto
decision: >
  Extend the existing Pure ML-DSA context allow-list with qfs/v1/dir for
  directory advertisements/deletions and qfs/v1/join for vault admission.
  Preserve qfs/v1/id, qfs/v1/wrap, qfs/v1/manifest and qfs/v1/flush unchanged.
  There is no qfs/v1/pkt. DirectoryAd M is peer_id || vault_id ||
  u32be(len(addr)) || addr_utf8 || u32be(len(ek)) || ek ||
  u32be(len(vk)) || vk || u64be(issued_at). JoinRequest M is vault_id ||
  raw 16-byte join_code || canonical identity_m(document). DirForget M is
  peer_id || vault_id || raw join_code || u64be(issued_at). Signatures are
  detached from M. Keep all serializers in encoding.rs. Directory addresses
  are canonical numeric SocketAddr display strings, including bracketed IPv6;
  hostnames/DNS, unspecified addresses and port zero are not join targets.
  Sign after binding to the actual listener address or explicit advertise_addr.
why:
- Directory routing and admission are separate signed objects, not ordinary signed packets.
- Numeric addresses bind the advertised endpoint without DNS resolution ambiguity.
rejected:
- Reusing identity or packet signing contexts for directory/admission objects.
- Base32 or hexadecimal identifiers inside canonical signed messages.
