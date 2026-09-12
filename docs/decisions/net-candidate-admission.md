# Candidate epochs during admission
status: accepted
date: 2026-09-12      scope: net
decision: >
  Distinguish a confirmed pair from an unapproved candidate epoch. Preserve
  the confirmed pair, handles, counters and vault binding while an incoming
  restart negotiates a candidate using the existing Identity, EpochHint,
  Construction B wrap and WrapAck sequence. Public identity documents and
  unsigned hints cannot replace a confirmed session. Encrypt Join and admission
  responses using the explicitly selected handshake session; ordinary live
  traffic continues to select the confirmed session until admission succeeds.
  On the joining side, only the authenticated encrypted flush-end marker may
  authorize promotion; a bare FlushOffer cannot. Mark admitted epochs only in
  live runtime state; restart-prepared wraps are not confirmed admissions.
  Promote the host candidate only after authenticated Join verifies the current
  code, identity and existing vault binding. On authenticated refusal discard
  only that candidate; preserve a prior confirmed pair. Retain cached candidate
  ciphertext and live counters after ambiguous disconnect or missing WrapAck.
  Serialize candidate handshakes per peer and bound pending candidates. Record
  candidate wraps with a versioned extension in the existing private keystore;
  reuse its atomic writer and process lock. On process restart discard all old
  active/candidate key slots and prepare fresh epochs above the persisted floor.
  Keep the confirmed-pair and multi-vault constraints in net-multi-vault.md;
  a candidate is not an additional admitted vault session. Preserve TCP frame
  numbers, canonical crypto encodings and Join-in-GCM. Do not introduce a
  pre-admission key replacement based solely on a higher EpochHint.
why:
- Restarted members prepare a new epoch while H may retain the prior live pair.
- Failed admission must not destroy a valid existing session or reset its counters.
rejected:
- Accepting unsigned hints as authority to replace confirmed sessions.
- Reusing restart-ineligible keys or repeating encapsulation for a cached retry.
