import { History as HistoryGlyph } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { FileIcon, FolderIcon } from "@/components/icons";
import { EmptyState } from "@/components/ui/EmptyState";
import { Modal } from "@/components/ui/Modal";
import { Segmented } from "@/components/ui/Segmented";
import type { SegmentedOption } from "@/components/ui/Segmented";
import type { HistoryEvent, HistoryKind, NodeId } from "@/lib/backend";

import { useNode, useWorkspace } from "../store";
import { HistoryTimeline } from "./HistoryTimeline";

type Filter = "all" | "changes" | "access" | "moves";

/**
 * The filter is three coarse questions — what changed, who could see it, where
 * it went — rather than one switch per `HistoryKind`. Nine toggles would be a
 * settings pane; three are a glance.
 */
const KINDS: Record<Exclude<Filter, "all">, ReadonlySet<HistoryKind>> = {
  changes: new Set<HistoryKind>([
    "created",
    "renamed",
    "modified",
    "duplicated",
    "colored",
    "downloaded",
    "deleted",
  ]),
  access: new Set<HistoryKind>(["access"]),
  moves: new Set<HistoryKind>(["moved"]),
};

const OPTIONS: SegmentedOption<Filter>[] = [
  { value: "all", label: "All" },
  { value: "changes", label: "Changes" },
  { value: "access", label: "Access" },
  { value: "moves", label: "Moves" },
];

/** Widths that vary row to row, so the wait reads as content arriving, not a bar. */
const SKELETON = [0.82, 0.64, 0.9, 0.55, 0.74, 0.6];

/**
 * The evolution of one node, as a grouped timeline.
 *
 * History is the surface that makes a multiplayer file system legible: in a
 * shared vault "who renamed this, and when" is asked far more often than any
 * property of the file itself, so it gets a full-size stage rather than a
 * section of Get Info. The dialog draws its own chrome (`padded={false}`) so the
 * day captions can stick to the top of a scroller that runs edge to edge.
 *
 * Filtering is client-side over the already-fetched list: the whole history of
 * one node is small, and a round trip per toggle would make the filter feel
 * like a query.
 */
export function HistoryModal() {
  const modal = useWorkspace((s) => s.modal);
  const closeModal = useWorkspace((s) => s.closeModal);
  const client = useWorkspace((s) => s.client);
  const vaultId = useWorkspace((s) => s.vaultId);
  const members = useWorkspace((s) => s.members);

  const open = modal !== null && modal.kind === "history";

  // Shadowed so the panel keeps its node while it animates out; the store's
  // `modal` is already null on the first frame of the exit.
  const [shownId, setShownId] = useState<NodeId | null>(null);
  if (modal !== null && modal.kind === "history" && modal.nodeId !== shownId) {
    setShownId(modal.nodeId);
  }

  const node = useNode(shownId);
  const panel = useRef<HTMLDivElement | null>(null);

  const [filter, setFilter] = useState<Filter>("all");
  const [events, setEvents] = useState<HistoryEvent[]>([]);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!open || !client || vaultId === null || shownId === null) return;
    let alive = true;
    setLoading(true);
    setEvents([]);
    setFilter("all");
    void client
      .getHistory(vaultId, shownId)
      .then((next) => {
        if (!alive) return;
        setEvents(next);
        setLoading(false);
      })
      .catch(() => {
        // A failed read is an empty history here: this dialog only ever reads,
        // and an error panel would say less than "nothing recorded yet".
        if (!alive) return;
        setEvents([]);
        setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, [open, client, vaultId, shownId]);

  const shown = useMemo(
    () => (filter === "all" ? events : events.filter((e) => KINDS[filter].has(e.kind))),
    [events, filter],
  );

  /**
   * This dialog draws its own header, so the shared `Modal` has no title to
   * label it with and a nameless dialog announces as "dialog". The name is
   * written onto the dialog element the panel sits in rather than added as
   * another prop on a primitive every other surface uses titled.
   */
  useEffect(() => {
    if (!open) return;
    const dialog = panel.current?.closest('[role="dialog"]');
    dialog?.setAttribute("aria-label", node ? `History of ${node.name}` : "History");
  }, [open, node]);

  return (
    <Modal open={open} onClose={closeModal} size="lg" padded={false}>
      <div
        ref={panel}
        data-testid="history-modal"
        data-node-id={shownId ?? undefined}
        className="flex min-h-0 flex-1 flex-col"
      >
        <header className="flex shrink-0 items-center gap-[10px] border-b border-line px-[24px] pt-[22px] pr-[56px] pb-[14px]">
          <span className="grid shrink-0 place-items-center" style={{ width: 24, height: 24 }}>
            {node === undefined ? null : node.kind === "folder" ? (
              <FolderIcon color={node.color ?? "graphite"} size={24} />
            ) : (
              <FileIcon name={node.name} size={24} />
            )}
          </span>
          <h2 className="shrink-0 font-heading text-[20px] leading-[26px] font-medium tracking-[-0.015em] text-fg">
            History
          </h2>
          <span className="min-w-0 truncate text-[13px] leading-[26px] text-fg-3">
            {node?.name ?? ""}
          </span>
          <Segmented
            className="ml-auto shrink-0"
            value={filter}
            onChange={(next) => setFilter(next)}
            options={OPTIONS}
          />
        </header>

        <div className="scroll-thin min-h-0 flex-1 overflow-y-auto px-[24px] py-[16px]">
          {loading ? (
            <div className="flex flex-col gap-[16px] pt-[6px]" aria-hidden>
              {SKELETON.map((width, i) => (
                <div key={i} className="flex items-center gap-[10px]">
                  <span className="h-[28px] w-[28px] shrink-0 rounded-full bg-white/[0.05]" />
                  <span
                    className="h-[10px] rounded-full bg-white/[0.04]"
                    style={{ width: `${Math.round(width * 100)}%` }}
                  />
                </div>
              ))}
            </div>
          ) : shown.length === 0 ? (
            <div className="grid h-full place-items-center">
              <EmptyState
                icon={<HistoryGlyph size={16} strokeWidth={1.75} />}
                title="No history yet"
                detail={
                  filter === "all"
                    ? "Renames, moves and edits will show up here."
                    : "Nothing of this kind has happened to it."
                }
              />
            </div>
          ) : (
            <HistoryTimeline events={shown} members={members} />
          )}
        </div>
      </div>
    </Modal>
  );
}

export default HistoryModal;
