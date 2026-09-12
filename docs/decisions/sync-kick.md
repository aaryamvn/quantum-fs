# Membership removal preserves accepted history
status: accepted
date: 2026-09-12      scope: sync
decision: >
  Only H can append Kick (control kind 7, raw PeerId); reject H and outsiders.
  Kick is a membership change, not a history purge. Authorize new commits using
  the current members at that record. Apply catch-up records in order using
  running membership; H authenticates tree authorization. Preserve admitted
  public identity documents and historical membership in the hashed snapshot.
  Snapshotted manifests are already-accepted records: re-verify their identity
  documents and ML-DSA signatures, without applying the post-kick live set.
  Fold membership and tree effects into snapshot fields before truncating the
  acknowledged log prefix. Remaining members determine the acknowledgment floor.
  Carry public history separately from current membership in H's welcome;
  process older controls before reconciling the authoritative current set.
  Flush offers can carry public identity history in a versioned extension. Verify
  those documents into staging only; bind their canonical bytes to the encrypted
  flush end digest before publishing the batch. This supplies missing writers
  without opening a newer GCM counter before queued controls.
  Stage removal from membership, acknowledgments, presence, outboxes and mailbox,
  persistent denial, a fresh join code, and Kick for remaining recipients only.
  Persist admission and replica together with an undo journal using the existing
  atomic_private_write protocol. Recover a surviving journal by rolling both
  files back before opening either; publish no memory or packets on write failure.
  After successful local publication discard the target pair and request TCP
  disconnection. Remaining recipients discard their target pair after atomic
  control application. Directory publication then puts the new code and forgets
  the old; errors propagate and old-code admission stays disabled locally.
  Offline recipients apply Kick behind flush-before-live. Online A may still mesh
  with B until A applies Kick; that application closes the window by discarding
  B's pair. Do not require H contact before every mesh operation.
  A persistent denylist prevents accidental rejoin by the same principal only:
  identities are free to mint, and a new identity with the current code can join.
  Code rotation controls entry. Kick cannot erase copied plaintext, remote disks,
  old keys or historical access. No unkick or successor election is introduced.
why:
- Live membership must not retroactively invalidate accepted files or directories.
- Atomic local rotation prevents a crash from retaining the old admission code.
rejected:
- Denylist as a security boundary, history purges, and forced H polling for mesh traffic.
