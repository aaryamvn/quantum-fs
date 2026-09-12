import { Check, Cloud } from "lucide-react";
import type { ReactNode } from "react";

import { iconSpecForName } from "@/components/icons";
import { Avatar } from "@/components/ui/Avatar";
import { ProgressRing } from "@/components/ui/ProgressRing";
import type { FsNode, PeerId } from "@/lib/backend";
import { formatBytes } from "@/lib/format";
import { formatPath, pathOf } from "@/lib/path";
import { formatDateTime, formatRelative } from "@/lib/time";

import { useMember, useWorkspace } from "../store";

export interface DetailsListProps {
  node: FsNode;
}

/** "3 items" / "1 item" — the count is read far more often than it is counted. */
function items(n: number): string {
  return `${n} ${n === 1 ? "item" : "items"}`;
}

/** "HDR file", "Folder". The registry already knows every extension's short name. */
function kindLabel(node: FsNode): string {
  if (node.kind === "folder") return "Folder";
  const label = iconSpecForName(node.name).label;
  return label === "" ? "File" : `${label} file`;
}

/**
 * One row of the definition list. A div wrapper is legal inside `dl` and keeps
 * the pair aligned. The 6px of vertical padding is what turns six stamps into
 * six readable lines rather than a block of small type.
 */
function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex items-baseline gap-[8px] py-[6px] leading-[20px]">
      <dt className="w-[84px] shrink-0 text-[12px] text-fg-3">{label}</dt>
      <dd className="min-w-0 flex-1 text-[12.5px] break-words text-fg tabular-nums">{children}</dd>
    </div>
  );
}

/**
 * Avatar + name + when. Used twice, for the two people a node remembers.
 *
 * The name wraps instead of truncating and the relative time moves under it:
 * in a 288px pane a truncated name is the one field that is actively wrong —
 * "Aaryaman V…" and "Aaryaman W…" are different people — while a stamp that
 * costs a second line costs nothing.
 */
function Person({ label, peerId, at }: { label: string; peerId: PeerId; at: number }) {
  const member = useMember(peerId);
  const name = member?.name ?? "Unknown member";

  return (
    <div className="flex items-start gap-[8px] py-[6px]">
      <span className="w-[84px] shrink-0 text-[12px] leading-[20px] text-fg-3">{label}</span>
      <Avatar peerId={peerId} name={name} initials={member?.initials} size={20} />
      <span className="min-w-0 flex-1">
        <span className="block text-[12.5px] leading-[20px] break-words text-fg">{name}</span>
        <span className="block text-[11px] leading-[15px] text-fg-3">{formatRelative(at)}</span>
      </span>
    </div>
  );
}

/**
 * Everything the inspector knows about one node, in the order people ask for it.
 *
 * Identity first (what it is called, what kind of thing it is), then the one
 * fact that changes what you can *do* — is this file actually on this Mac —
 * and only then the stamps. Availability sits that high because it is the one
 * line that changes how the file behaves: there is no download control anywhere
 * in the app, so this line, and the hint under it, are what tell you that a
 * double-click on a remote file fetches it first.
 *
 * The definition list is a fixed 84px label column so the values line up as a
 * single scannable edge, and every number is tabular so a size does not shift
 * as a transfer ticks.
 */
export function DetailsList({ node }: DetailsListProps) {
  const nodes = useWorkspace((s) => s.nodes);
  const vaultName = useWorkspace((s) => s.vaultName);

  const isFolder = node.kind === "folder";
  const subtitle = isFolder
    ? `Folder · ${items(node.childCount)}`
    : `${kindLabel(node)} · ${formatBytes(node.sizeBytes)}`;

  // The chain minus the node itself; the root carries the vault's name, not its own.
  const ancestors = pathOf(nodes, node.id).slice(0, -1);
  const where =
    ancestors.length === 0
      ? vaultName
      : formatPath(ancestors.map((entry, index) => (index === 0 ? vaultName : entry.name)));

  return (
    <div data-testid="inspector-details" data-node-id={node.id}>
      <p
        data-selectable
        className="font-heading text-[17px] leading-[22px] font-medium tracking-[-0.01em] break-words text-fg"
      >
        {node.name}
      </p>
      <p className="mt-[2px] text-[12.5px] text-fg-2">{subtitle}</p>

      <div className="mt-[10px] flex items-center gap-[8px]">
        {node.availability === "local" ? (
          <>
            <Check size={14} strokeWidth={1.75} className="shrink-0 text-success" />
            <span className="text-[12.5px] text-fg-2">Available offline</span>
            {node.holders.length > 0 ? (
              <span className="text-[11px] text-fg-3">on {node.holders.length} peers</span>
            ) : null}
          </>
        ) : node.availability === "downloading" ? (
          <>
            <ProgressRing value={node.progress ?? undefined} size={14} className="shrink-0 text-violet" />
            <span className="text-[12.5px] text-fg-2">
              Downloading · {Math.round((node.progress ?? 0) * 100)}%
            </span>
          </>
        ) : (
          <>
            <Cloud size={14} strokeWidth={1.75} className="shrink-0 text-fg-3" />
            <span className="min-w-0 flex-1 text-[12.5px] text-fg-2">
              Not on this Mac · on {node.holders.length} peers
            </span>
          </>
        )}
      </div>

      {node.availability === "remote" && !isFolder ? (
        <p className="mt-[4px] text-[11px] leading-[15px] text-fg-3">Double-click to download</p>
      ) : null}

      <dl className="mt-[10px] flex flex-col">
        <Row label="Size">{formatBytes(node.sizeBytes)}</Row>
        {isFolder ? <Row label="Items">{items(node.childCount)}</Row> : null}
        <Row label="Created">{formatDateTime(node.createdAt)}</Row>
        <Row label="Modified">{`${formatDateTime(node.modifiedAt)} · ${formatRelative(node.modifiedAt)}`}</Row>
        <Row label="Where">{where}</Row>
        <Row label="Vault">{vaultName}</Row>
      </dl>

      <div className="mt-[10px] flex flex-col">
        <Person label="Created by" peerId={node.createdBy} at={node.createdAt} />
        <Person label="Last edit" peerId={node.modifiedBy} at={node.modifiedAt} />
      </div>
    </div>
  );
}
