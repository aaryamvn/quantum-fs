# Vault capacity = server allocation + the sum of member contributions
status: accepted
date: 2026-09-12      scope: client
decision: >
  Every client contributes a fixed amount of storage (`contributionBytes` in `profile.json`, default
  8 GiB, chosen in Settings → Storage). Each member replicates its `clientId` and `contributionBytes`
  in its `.qfs-meta.json` member entry. A vault's capacity is the server allocation chosen at creation
  (`quotaBytes`, the host's `app-meta` quota) plus the contributions of its current members, deduplicated
  by client id and excluding the host; a server's capacity is what it reports plus the distinct client
  contributions seen across this client's vaults on it. The flat per-online-member credit is removed.
  Settings → Storage shows a thin segmented ring (server + one segment per contributor) with used bytes
  as an inner arc. Nothing in the protocol changes: the host's quota and used bytes are unchanged and
  the sum is computed by every client from replicated data.
why:
- The human's model is that capacity is the sum of what each connected client allocates and grows as clients join; the previous figure was a per-server flag plus an invented constant.
- `.qfs-meta.json` is the only member-visible channel (MEMBER status rows are host-local and carry no bytes).
rejected:
- A new admin verb to raise the host quota per member — the host neither enforces nor stores member contributions; the sum is a client-side view.
- Counting contributions per online member only — capacity would flicker with presence.
