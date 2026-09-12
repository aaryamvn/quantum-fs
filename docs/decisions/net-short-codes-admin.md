# Six-character join codes, read-only admin status, and re-admission of current members
status: accepted
date: 2026-09-12      scope: net
decision: >
  Vaults created through a host's admin port get a six-character code (A-Z, 2-7; 30 bits) shown to
  humans; the protocol's 16-byte JoinCode is derived from it as SHA-256("qfs/v1/short-code/" || code)
  truncated to 16 bytes, on the host and in the app. The directory, admission, and handshake are
  unchanged; only the entropy behind admission is reduced, and only for admin-created vaults
  (`qfsd --create-vault` still mints random 128-bit codes). A host's admin port answers PING, STATUS and
  OPS without the admin token but prints `-` in place of every join code; CREATE_VAULT, KICK,
  ROTATE_CODE and FORGET_VAULT require the token. A peer that is already a current member (present in
  the host's member set and not denied) is re-admitted even when it presents a code that has since
  rotated; a kicked peer stays denied and a new identity still needs the current code.
why:
- The human wants codes people can read aloud and type into six boxes; 128-bit codes are 26 characters.
- Kicks and rotations otherwise lock out every innocent member the next time it reconnects; the code controls entry of new identities, not the standing of existing ones (`sync-kick.md`).
- Read-only status lets a client that joined by code alone show presence, usage and quota without holding the server's admin token.
rejected:
- Keeping 128-bit codes in the UI — rejected by the human for the demo.
- Pushing rotated codes to members over the sealed channel — a new control kind in the replicated log; too invasive for now.
- Full AUTH for STATUS — would force every joiner to be handed the admin token.
This partially supersedes the "old codes die at once" clause of docs/decisions/net-vault-join-directory.md for members that are already admitted; the ≥128-bit rule there now applies only to CLI-created vaults.
