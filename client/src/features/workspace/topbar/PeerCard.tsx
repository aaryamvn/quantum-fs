import { Avatar } from "@/components/ui/Avatar";
import { Caption } from "@/components/ui/Caption";
import { Chip } from "@/components/ui/Chip";
import { Divider } from "@/components/ui/Divider";
import { FileIcon, FolderIcon } from "@/components/icons";
import type { PeerId } from "@/lib/backend";
import { formatRelative } from "@/lib/time";

import { actorLabel, useMember, useNode, usePeerPresence, useWorkspace } from "../store";

/** Icon size inside the "last edited" row — big enough to read the family, small enough to stay a line. */
const ROW_ICON = 20;

/**
 * The contact card behind a presence avatar: who this member is, where they are
 * right now, and the one thing they touched last.
 *
 * It answers the two questions an avatar row provokes and nothing else — "who is
 * that?" and "what did they just do?" — because the moment a hover card starts
 * carrying controls it becomes a menu that closes when you reach for it. The
 * single affordance is the last-edited row, which navigates: seeing a peer's
 * edit and getting to it should not cost a search.
 *
 * Offline members keep their card rather than disappearing from it: "last seen"
 * is the answer you came for when someone has gone dark, and a face that opens
 * nothing is worse than a face that opens a short answer. A peer the member list
 * cannot name — one who has left, or whose record has not landed yet — keeps the
 * card too, saying the one thing that is known about them and nothing it would
 * have to invent (no role, no last edit).
 */
export function PeerCard({ peerId }: { peerId: PeerId }) {
  const member = useMember(peerId);
  const presence = usePeerPresence(peerId);
  const vaultName = useWorkspace((s) => s.vaultName);
  const navigateTo = useWorkspace((s) => s.navigateTo);
  const select = useWorkspace((s) => s.select);

  const online = presence ? presence.online : (member?.online ?? false);
  const folder = useNode(presence?.folderId ?? null);
  const edited = useNode(member?.lastEdited?.nodeId ?? null);

  const { name, initials } = actorLabel(member);

  // The root folder is named after the vault already, but a peer sitting at the
  // top should read as "in <vault>", not "in <a folder that happens to match>".
  const where = folder ? (folder.parentId === null ? vaultName : folder.name) : vaultName;

  const status = !member
    ? online
      ? `Online · in ${where}`
      : "Offline"
    : member.isSelf
      ? `You · in ${where}`
      : !online
        ? `Offline · last seen ${formatRelative(member.lastSeenAt)}`
        : presence?.idle
          ? "Idle"
          : `Online · in ${where}`;

  const openEdited = (): void => {
    if (!edited) return;
    if (edited.kind === "folder") {
      navigateTo(edited.id);
      return;
    }
    if (edited.parentId !== null) navigateTo(edited.parentId);
    select([edited.id]);
  };

  return (
    <div data-testid="peer-card" className="w-[264px] p-[14px]">
      <div className="flex items-center gap-[10px]">
        <Avatar peerId={peerId} name={name} initials={initials} size={40} dim={!online} />
        <div className="min-w-0 flex-1">
          <div className="truncate text-[14px] font-medium text-fg">{name}</div>
          <div className="mt-[2px] truncate text-[12px] text-fg-2">{status}</div>
        </div>
      </div>

      <Divider className="mt-[12px] mb-[14px]" />

      <Caption>Last edited</Caption>

      {member && edited && member.lastEdited ? (
        <button
          type="button"
          onClick={openEdited}
          data-node-id={edited.id}
          className="mt-[7px] flex w-full items-center gap-[9px] rounded-[8px] px-[6px] py-[5px] text-left
            transition-colors duration-[160ms] ease-[cubic-bezier(0.2,0.8,0.2,1)]
            hover:bg-surface-hover focus-visible:bg-surface-hover focus-visible:outline-none"
        >
          <span className="grid shrink-0 place-items-center" style={{ width: ROW_ICON, height: ROW_ICON }}>
            {edited.kind === "folder" ? (
              <FolderIcon color={edited.color ?? "graphite"} size={ROW_ICON} />
            ) : (
              <FileIcon name={edited.name} size={ROW_ICON} />
            )}
          </span>
          <span className="min-w-0 flex-1 truncate text-[13px] text-fg">{edited.name}</span>
          <span className="shrink-0 text-[11.5px] text-fg-3">
            {formatRelative(member.lastEdited.at)}
          </span>
        </button>
      ) : (
        <div className="mt-[7px] px-[6px] text-[13px] text-fg-3">No edits yet</div>
      )}

      {member ? (
        <div className="mt-[14px]">
          <Chip tone={member.role === "admin" ? "violet" : "neutral"} size="xs">
            {member.role === "admin" ? "Admin" : "Member"}
          </Chip>
        </div>
      ) : null}
    </div>
  );
}

export default PeerCard;
