import { useEffect, useRef, useState } from "react";

import { Segmented } from "@/components/ui/Segmented";
import { Toggle } from "@/components/ui/Toggle";
import { useWorkspace } from "@/features/workspace/store";
import { useBackend } from "@/lib/backend";
import type { VaultMetaPatch } from "@/lib/backend";
import { formatBytes } from "@/lib/format";

import { useSettingsVault } from "./SettingsVaultContext";

/** Long enough that a toggle and a threshold set in one go travel as one write. */
const SAVE_DEBOUNCE_MS = 400;

const THRESHOLDS = ["80", "90", "95"] as const;
type Threshold = (typeof THRESHOLDS)[number];

const THRESHOLD_OPTIONS = THRESHOLDS.map((value) => ({ value, label: `${value}%` }));

/** The nearest offered threshold, so a value the daemon set by hand still shows as chosen. */
function nearestThreshold(pct: number): Threshold {
  return THRESHOLDS.reduce((best, value) =>
    Math.abs(Number(value) - pct) < Math.abs(Number(best) - pct) ? value : best,
  );
}

/**
 * What the vault costs — on the server, and on this Mac.
 *
 * Two different numbers live here and are deliberately kept apart: the quota is
 * the vault's slice of its server and is the same for every member, while the
 * local footprint is only what this machine happens to be holding. Auto-cleanup
 * is the bridge between them, which is why its caption says plainly that a file
 * removed locally is still in the vault.
 *
 * These two switches save themselves — there is nothing to get wrong and nothing
 * to confirm, so a Save button would only add a step.
 */
export function StorageTab() {
  const client = useWorkspace((s) => s.client);
  const toast = useWorkspace((s) => s.toast);
  const { vaultId, meta, isOpenVault, refresh } = useSettingsVault();
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

  const { servers } = useBackend();
  const vault = servers.flatMap((server) => server.vaults).find((v) => v.id === vaultId);

  const [autoCleanup, setAutoCleanup] = useState(meta?.autoCleanup ?? false);
  const [threshold, setThreshold] = useState<Threshold>(
    nearestThreshold(meta?.cleanupThresholdPct ?? 90),
  );

  const timer = useRef<number | null>(null);
  const pending = useRef<VaultMetaPatch>({});

  useEffect(() => {
    if (!meta) return;
    setAutoCleanup(meta.autoCleanup);
    setThreshold(nearestThreshold(meta.cleanupThresholdPct));
  }, [meta]);

  useEffect(
    () => () => {
      if (timer.current !== null) window.clearTimeout(timer.current);
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

  const used = vault?.usedBytes ?? 0;
  const quota = vault?.quotaBytes ?? 0;
  const fraction = quota > 0 ? Math.max(0, Math.min(1, used / quota)) : 0;

  return (
    <div data-testid="vault-settings-storage" className="flex flex-col gap-[24px]">
      <div>
        <div className="h-[8px] w-full overflow-hidden rounded-full bg-white/[0.06]">
          <span
            className="brand-gradient block h-full rounded-full"
            style={{ width: `${fraction * 100}%` }}
            aria-hidden
          />
        </div>
        <p className="mt-[10px] text-[12.5px] leading-none text-fg-2 tabular-nums">
          {formatBytes(used)} of {formatBytes(quota)} used
        </p>
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
