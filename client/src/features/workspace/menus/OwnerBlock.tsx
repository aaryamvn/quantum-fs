import { Avatar } from "@/components/ui/Avatar";
import type { NodeId, PeerId } from "@/lib/backend";
import { formatDateTime } from "@/lib/time";

import { useMember, useNode } from "../store";

/**
 * Who made this, and when — the quiet footer of the context menu.
 *
 * In a vault that several people write to at once, "who put this here" is the
 * question asked right after "what is this", and every other answer to it costs
 * a modal. Parking it at the foot of the menu means the answer is already on
 * screen by the time you have finished reading the actions, and it reads as
 * provenance rather than as another thing to click: no hover state, no target.
 *
 * The creator is resolved against the live member list, not stored on the node,
 * so a rename of a peer updates everywhere at once. A peer who has since left
 * the vault has no record left to resolve, so the id itself is shown, shortened
 * — an honest "someone who is gone" beats a blank line.
 */

/** Long enough to recognize, short enough not to wrap: "peer_a…f3c1". */
function shortPeer(peerId: PeerId): string {
  return peerId.length > 14 ? `${peerId.slice(0, 7)}…${peerId.slice(-4)}` : peerId;
}

export interface OwnerBlockProps {
  nodeId: NodeId;
}

export function OwnerBlock({ nodeId }: OwnerBlockProps) {
  const node = useNode(nodeId);
  const creator = useMember(node?.createdBy);

  if (!node) return null;

  const name = creator ? (creator.isSelf ? "you" : creator.name) : shortPeer(node.createdBy);

  return (
    <div
      data-testid="owner-block"
      data-node-id={nodeId}
      className="flex items-center gap-[10px] px-[10px] pt-[8px] pb-[6px]"
    >
      <Avatar peerId={node.createdBy} name={name} initials={creator?.initials} size={28} />
      <span className="flex min-w-0 flex-col gap-[2px]">
        <span className="truncate text-[12.5px] leading-none text-fg">Created by {name}</span>
        <span className="truncate text-[11.5px] leading-none text-fg-3 tabular-nums">
          {formatDateTime(node.createdAt)}
        </span>
      </span>
    </div>
  );
}

export default OwnerBlock;
