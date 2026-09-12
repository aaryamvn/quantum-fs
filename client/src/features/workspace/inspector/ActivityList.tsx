import { History } from "lucide-react";
import { useEffect, useState } from "react";

import { Avatar } from "@/components/ui/Avatar";
import { GhostButton } from "@/components/ui/GhostButton";
import type { HistoryEvent, NodeId } from "@/lib/backend";
import { formatRelative } from "@/lib/time";

import { actorLabel, useNode, useWorkspace } from "../store";

/** Fired by the history modal (and the demo) when an append happened out of band. */
const HISTORY_CHANGED = "qfs:history-changed";

export interface ActivityListProps {
  nodeId: NodeId;
  /** How many events the inspector shows before deferring to the history modal. */
  limit?: number;
}

/**
 * The last few things that happened here, newest first.
 *
 * In a multiplayer file system the question "who touched this, and when" is
 * asked constantly, and making it a modal-only answer means it is never asked
 * at all. Four rows is the deliberate ceiling: enough to see the shape of
 * recent activity, short enough that it never pushes the details above it off
 * the pane. The full trail is one button away.
 *
 * The read re-runs on the node's modification stamp, which is the store's own
 * signal that something was appended — so a peer's rename lands in this list on
 * the same `fs-changed` event that repaints the tile, with no polling.
 */
export function ActivityList({ nodeId, limit = 4 }: ActivityListProps) {
  const client = useWorkspace((s) => s.client);
  const vaultId = useWorkspace((s) => s.vaultId);
  const members = useWorkspace((s) => s.members);
  const openModal = useWorkspace((s) => s.openModal);
  const modifiedAt = useNode(nodeId)?.modifiedAt;

  const [events, setEvents] = useState<HistoryEvent[]>([]);
  const [tick, setTick] = useState(0);

  useEffect(() => {
    const onChanged = () => setTick((n) => n + 1);
    window.addEventListener(HISTORY_CHANGED, onChanged);
    return () => window.removeEventListener(HISTORY_CHANGED, onChanged);
  }, []);

  useEffect(() => {
    if (!client || !vaultId) return;
    let live = true;
    void client.getHistory(vaultId, nodeId).then(
      (list) => {
        if (live) setEvents(list);
      },
      () => {
        if (live) setEvents([]);
      },
    );
    return () => {
      live = false;
    };
  }, [client, vaultId, nodeId, modifiedAt, tick]);

  const shown = events.slice(0, limit);

  return (
    <div data-testid="inspector-activity" data-node-id={nodeId}>
      {shown.length === 0 ? (
        <p className="text-[12.5px] text-fg-3">Nothing has happened here yet</p>
      ) : (
        <ul className="flex flex-col">
          {shown.map((event) => {
            const member = event.by ? members.find((entry) => entry.peerId === event.by) : undefined;
            const { name, initials } = actorLabel(member);

            return (
              <li key={event.id} className="flex items-start gap-[8px] py-[6px]">
                <Avatar
                  peerId={event.by}
                  name={name}
                  initials={initials}
                  size={18}
                  className="mt-[1px]"
                />
                <p className="min-w-0 flex-1 text-[12.5px] leading-[20px] text-fg-2">
                  <span className="text-fg">{name}</span> {event.summary}
                </p>
                <span className="shrink-0 text-[11px] leading-[20px] text-fg-3">
                  {formatRelative(event.at)}
                </span>
              </li>
            );
          })}
        </ul>
      )}

      <GhostButton
        variant="secondary"
        className="mt-[10px]"
        icon={<History size={14} strokeWidth={1.75} />}
        onClick={() => openModal({ kind: "history", nodeId })}
      >
        View history
      </GhostButton>
    </div>
  );
}
