# Directory serves a cosmetic display-name store on a side port
status: accepted
date: 2026-09-12      scope: net
decision: >
  The central directory process additionally serves a token-less, line-oriented "profile port" at its
  listen IP and listen port + 1000 (7440 → 8440; `--profile-addr` overrides). Commands: `PING` → `OK`,
  `PROFILE_PUT <client_id> <name_b64>` → `OK`, `PROFILE_GET <client_id>` → `OK <name_b64>` | `ERR not found`.
  `client_id` is 32 lowercase hex characters the client derives from its machine (SHA-256 of a fixed
  prefix, the platform UUID and the data-dir path); `name_b64` is standard base64 of a 1–64 character
  display name. Records persist in `<data_dir>/profiles.json` (owner-only, atomic write, 10 000 cap).
  Nothing about the signed directory frames (kinds 5–8), admission, or the sealed channel changes; the
  store is cosmetic and unauthenticated, exactly like the display name it holds.
why:
- The human wants a name typed once at first launch to follow the computer forever and to live on the central server; per-vault peer identities cannot key that (one per vault, minted per join).
- A side listener follows the admin-port precedent (client-backend-embed.md) and keeps frame kinds 1–14 untouched.
rejected:
- New directory frame kinds — touches frame.rs limits, encoding.rs state format and the replay rules for a cosmetic field.
- Signing profile records — there is no client-wide keypair, and net-vault-join-directory.md already makes display names cosmetic.
