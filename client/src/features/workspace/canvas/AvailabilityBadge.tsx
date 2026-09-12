import { CloudDownload } from "lucide-react";

import { ProgressRing } from "@/components/ui/ProgressRing";
import { Tooltip } from "@/components/ui/Tooltip";
import type { FsNode } from "@/lib/backend";

export interface AvailabilityBadgeProps {
  node: FsNode;
  /** Diameter of the badge in px; the caller positions it. */
  size?: number;
}

/**
 * Whether this file is actually *here*, said the way Finder says it about iCloud.
 *
 * The loud case is the quiet one: a file that is on this machine gets no badge at
 * all. A grid where every tile carries a green tick teaches you to stop reading
 * ticks, and then the one file that is missing reads exactly like the rest. So
 * the badge exists only to mark the exception — not downloaded, or downloading
 * right now — which makes "has a cloud on it" a fact you can take in without
 * inspecting anything.
 *
 * Folders never get one: the tree is replicated to every member, so a folder is
 * local by definition and a cloud on it would be a lie.
 *
 * The pixel count stays in the tooltip rather than under the icon: at 16px there
 * is no room for a number, and a percentage that is only true for one second is
 * worth less than a ring you can read the length of at a glance.
 */
export function AvailabilityBadge({ node, size = 16 }: AvailabilityBadgeProps) {
  if (node.kind !== "file") return null;
  if (node.availability === "local") return null;

  const circle =
    "inline-flex items-center justify-center rounded-full border border-line-strong bg-surface-2";

  if (node.availability === "downloading") {
    const pct = Math.round(Math.max(0, Math.min(1, node.progress ?? 0)) * 100);
    return (
      <Tooltip label={`Downloading · ${pct}%`}>
        <span
          data-testid="availability-badge"
          data-availability="downloading"
          className={`${circle} text-violet`}
          style={{ width: size, height: size }}
        >
          {/* Inset by the badge's own border so the arc reads as one ring, not two. */}
          <ProgressRing value={node.progress ?? undefined} size={size - 4} />
        </span>
      </Tooltip>
    );
  }

  return (
    <Tooltip label="Not on this Mac · double-click to download">
      <span
        data-testid="availability-badge"
        data-availability="remote"
        className={`${circle} text-fg-2`}
        style={{ width: size, height: size }}
      >
        <CloudDownload size={10} strokeWidth={1.75} />
      </span>
    </Tooltip>
  );
}
