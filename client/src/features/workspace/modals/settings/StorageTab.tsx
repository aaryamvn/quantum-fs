import { useEffect, useRef, useState } from "react";

import { ContributionRing } from "@/components/ui/ContributionRing";
import type { ContributionSegment } from "@/components/ui/ContributionRing";
import { Segmented } from "@/components/ui/Segmented";
import { Toggle } from "@/components/ui/Toggle";
import { useWorkspace } from "@/features/workspace/store";
import { useBackend } from "@/lib/backend";
import type { Profile, VaultMetaPatch } from "@/lib/backend";
import { BRAND } from "@/lib/color";
import { formatBytes } from "@/lib/format";

import { useSettingsVault } from "./SettingsVaultContext";

/** Long enough that a toggle and a threshold set in one go travel as one write. */
const SAVE_DEBOUNCE_MS = 400;

const THRESHOLDS = ["80", "90", "95"] as const;
type Threshold = (typeof THRESHOLDS)[number];

const THRESHOLD_OPTIONS = THRESHOLDS.map((value) => ({ value, label: `${value}%` }));

const GIB = 1024 * 1024 * 1024;

/** What one machine can sensibly pledge, from "barely" to "this is a spare drive". */
const CONTRIBUTIONS = ["4", "8", "16", "32", "64"] as const;
type Contribution = (typeof CONTRIBUTIONS)[number];

const CONTRIBUTION_OPTIONS = CONTRIBUTIONS.map((value) => ({ value, label: `${value} GB` }));

/** The server's slice is not a person, so it takes the brand stop rather than a peer colour. */
const SERVER_COLOR = BRAND.violet;

/** The nearest offered threshold, so a value the daemon set by hand still shows as chosen. */
function nearestThreshold(pct: number): Threshold {
  return THRESHOLDS.reduce((best, value) =>
    Math.abs(Number(value) - pct) < Math.abs(Number(best) - pct) ? value : best,
  );
}

/** Same idea for the pledge: a profile written by another build still lights a segment. */
function nearestContribution(bytes: number): Contribution {
  const gb = bytes / GIB;
  return CONTRIBUTIONS.reduce((best, value) =>
    Math.abs(Number(value) - gb) < Math.abs(Number(best) - gb) ? value : best,
  );
}

/**
 * What the vault costs — on the server, on every member's disk, and on this Mac.
 *
 * Three different numbers live here and are deliberately kept apart. The quota is
 * the host's allocation; each member's contribution raises the ceiling above it,
 * which is what the ring shows and what a single bar could never say. The local
 * footprint at the bottom is only what this machine happens to be holding, and
 * auto-cleanup is the bridge between it and the vault — which is why its caption
 * says plainly that a file removed locally is still in the vault.
 *
 * Every control here saves itself: there is nothing to get wrong and nothing to
 * confirm, so a Save button would only add a step.
 */
export function StorageTab() {
  const client = useWorkspace((s) => s.client);
  const toast = useWorkspace((s) => s.toast);
  const { vaultId, meta, members, isOpenVault, refresh } = useSettingsVault();
  // A number, not the tree: the sum only changes when a file's availability does.
  // It is the open vault's tree, so it is only shown when that is this vault.
  const localBytes = useWorkspace((s) => {
    let total = 0;
    for (const id in s.nodes) {
      const node = s.nodes[id];
      if (node.kind === "file" && node.availability === "local") total += node.sizeBytes;
    }
    return total;
  });

  const { client: backend, servers, refresh: refreshServers } = useBackend();
  const vault = servers.flatMap((server) => server.vaults).find((v) => v.id === vaultId);

  const [autoCleanup, setAutoCleanup] = useState(meta?.autoCleanup ?? false);
  const [threshold, setThreshold] = useState<Threshold>(
    nearestThreshold(meta?.cleanupThresholdPct ?? 90),
  );
  const [profile, setProfile] = useState<Profile | null>(null);

  const timer = useRef<number | null>(null);
  const pending = useRef<VaultMetaPatch>({});
  const contributionTimer = useRef<number | null>(null);

  useEffect(() => {
    if (!meta) return;
    setAutoCleanup(meta.autoCleanup);
    setThreshold(nearestThreshold(meta.cleanupThresholdPct));
  }, [meta]);

  // The pledge is the client's, not the vault's, so it is read from the profile and
  // kept live off `profile-changed` — another surface may set it while this is open.
  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const next = await backend.getProfile();
        if (alive) setProfile(next);
      } catch {
        // A profile that cannot be read simply leaves the control unset; the
        // surrounding pane is still worth showing.
      }
    })();
    const unsubscribe = backend.subscribe((e) => {
      if (e.type === "profile-changed") setProfile(e.profile);
    });
    return () => {
      alive = false;
      unsubscribe();
    };
  }, [backend]);

  useEffect(
    () => () => {
      if (timer.current !== null) window.clearTimeout(timer.current);
      if (contributionTimer.current !== null) window.clearTimeout(contributionTimer.current);
    },
    [],
  );

  // One coalesced write: flipping the switch and then picking a threshold inside
  // the window sends a single patch, so the row toasts "Saved" once, not twice.
  const schedule = (patch: VaultMetaPatch) => {
    if (!client || !vaultId) return;
    pending.current = { ...pending.current, ...patch };
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      timer.current = null;
      const body = pending.current;
      pending.current = {};
      void (async () => {
        try {
          await client.updateVaultMeta(vaultId, body);
          await refresh();
          toast("Saved", "success");
        } catch (e) {
          toast(e instanceof Error ? e.message : String(e), "error");
          await refresh();
        }
      })();
    }, SAVE_DEBOUNCE_MS);
  };

  // Same debounce, its own timer: stepping 4 → 8 → 16 while making up your mind is
  // one decision, and it should be one write and one "Saved".
  const scheduleContribution = (bytes: number) => {
    if (contributionTimer.current !== null) window.clearTimeout(contributionTimer.current);
    contributionTimer.current = window.setTimeout(() => {
      contributionTimer.current = null;
      void (async () => {
        try {
          const next = await backend.setProfile({ contributionBytes: bytes });
          setProfile(next);
          // The pledge changes the vault's ceiling, so both the member list and the
          // server list have to be re-read before the ring can be right.
          await Promise.all([refresh(), refreshServers()]);
          toast("Saved", "success");
        } catch (e) {
          toast(e instanceof Error ? e.message : String(e), "error");
          const current = await backend.getProfile().catch(() => null);
          if (current) setProfile(current);
        }
      })();
    }, SAVE_DEBOUNCE_MS);
  };

  const used = vault?.usedBytes ?? 0;
  const quota = vault?.quotaBytes ?? 0;

  const contributors = members.filter((member) => member.contributionBytes > 0);
  const pledged = contributors.reduce((sum, member) => sum + member.contributionBytes, 0);
  // `capacityBytes` is the backend's own answer; the sum is only a floor for a
  // daemon that predates the field.
  const capacity = vault?.capacityBytes || quota + pledged;

  const memberSegments: ContributionSegment[] = [...contributors]
    .sort((a, b) => b.contributionBytes - a.contributionBytes)
    .map((member) => ({
      id: member.peerId,
      label: member.name,
      bytes: member.contributionBytes,
      color: member.color,
    }));

  const segments: ContributionSegment[] = [
    { id: "server", label: "Server", bytes: quota, color: SERVER_COLOR },
    ...memberSegments,
  ];

  // Everyone on the list gets a legend row, including the members whose pledge is
  // unknown — an absent row would read as an absent member.
  const silent = members.filter((member) => member.contributionBytes <= 0);
  const selfPeerId = members.find((member) => member.isSelf)?.peerId ?? null;

  const pct = (bytes: number) => (capacity > 0 ? Math.round((bytes / capacity) * 100) : 0);

  const contribution = profile ? nearestContribution(profile.contributionBytes) : null;

  return (
    <div data-testid="vault-settings-storage" className="flex flex-col gap-[24px]">
      <div className="flex flex-col gap-[12px]">
        <div className="flex items-center gap-[20px]">
          <ContributionRing segments={segments} totalBytes={capacity} usedBytes={used} />

          <ul className="flex min-w-0 flex-1 flex-col gap-[8px]">
            {segments.map((segment) => (
              <li key={segment.id} className="flex items-center gap-[8px]">
                <span
                  aria-hidden
                  className="h-[10px] w-[10px] shrink-0 rounded-full"
                  style={{ background: segment.color }}
                />
                <span className="min-w-0 flex-1 truncate text-[12.5px] leading-none text-fg-2">
                  {segment.label}
                  {segment.id === selfPeerId ? <span className="text-fg-3"> (you)</span> : null}
                </span>
                <span className="shrink-0 text-[12.5px] leading-none text-fg tabular-nums">
                  {formatBytes(segment.bytes)}
                </span>
                <span className="w-[36px] shrink-0 text-right text-[12.5px] leading-none text-fg-3 tabular-nums">
                  {pct(segment.bytes)}%
                </span>
              </li>
            ))}

            {silent.map((member) => (
              <li key={member.peerId} className="flex items-center gap-[8px]">
                <span
                  aria-hidden
                  className="h-[10px] w-[10px] shrink-0 rounded-full border border-line"
                />
                <span className="min-w-0 flex-1 truncate text-[12.5px] leading-none text-fg-2">
                  {member.name}
                  {member.isSelf ? <span className="text-fg-3"> (you)</span> : null}
                </span>
                <span className="shrink-0 text-[12.5px] leading-none text-fg-3 tabular-nums">
                  &mdash;
                </span>
                <span className="w-[36px] shrink-0" />
              </li>
            ))}
          </ul>
        </div>

        <p className="text-[12.5px] leading-none text-fg-2 tabular-nums">
          {formatBytes(used)} of {formatBytes(capacity)} used &middot; {contributors.length}{" "}
          {contributors.length === 1 ? "contributor" : "contributors"}
        </p>
      </div>

      <div className="flex flex-col gap-[10px]">
        <span className="text-[12.5px] leading-none text-fg-3">Your contribution</span>
        <Segmented
          value={contribution ?? CONTRIBUTIONS[0]}
          options={CONTRIBUTION_OPTIONS}
          onChange={(next) => {
            setProfile((current) =>
              current ? { ...current, contributionBytes: Number(next) * GIB } : current,
            );
            scheduleContribution(Number(next) * GIB);
          }}
        />
        <span className="text-[12.5px] leading-[18px] text-fg-3">
          Storage this computer adds to every vault it belongs to.
        </span>
      </div>

      <div className="flex flex-col gap-[12px]">
        <div className="flex items-start justify-between gap-[16px]">
          <span className="flex min-w-0 flex-col gap-[4px]">
            <span className="text-[13px] leading-none text-fg">Free up space automatically</span>
            <span className="text-[11px] leading-[16px] text-fg-3">
              When this Mac&rsquo;s disk passes {threshold}% usage, files that other peers hold are
              removed locally (they stay in the vault).
            </span>
          </span>
          <Toggle
            checked={autoCleanup}
            label="Free up space automatically"
            onChange={(next) => {
              setAutoCleanup(next);
              schedule({ autoCleanup: next });
            }}
          />
        </div>

        <fieldset
          disabled={!autoCleanup}
          aria-label="Cleanup threshold"
          className={`m-0 border-0 p-0 ${autoCleanup ? "" : "opacity-50"}`}
        >
          <Segmented
            value={threshold}
            options={THRESHOLD_OPTIONS}
            onChange={(next) => {
              setThreshold(next);
              schedule({ cleanupThresholdPct: Number(next) });
            }}
          />
        </fieldset>
      </div>

      <div className="flex h-[44px] items-center gap-[10px] border-t border-line">
        <span className="w-[140px] shrink-0 text-[12.5px] text-fg-3">Local footprint</span>
        <span className="text-[12.5px] text-fg-2 tabular-nums">
          {isOpenVault ? formatBytes(localBytes) : "—"}
        </span>
        <span className="truncate text-[11px] text-fg-3">
          {isOpenVault ? "on this Mac" : "open this vault to measure it"}
        </span>
      </div>
    </div>
  );
}
