import { Copy, Files, Info, Trash2 } from "lucide-react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import type { ReactNode } from "react";

import { FileIcon, FolderIcon } from "@/components/icons";
import { Avatar } from "@/components/ui/Avatar";
import { EmptyState } from "@/components/ui/EmptyState";
import { GhostButton } from "@/components/ui/GhostButton";
import type { FsNode } from "@/lib/backend";
import { formatBytes } from "@/lib/format";
import { formatRelative } from "@/lib/time";

import { EASE, INSPECTOR_W } from "../layout";
import { useCurrentFolder, useMember, useSelectionNodes, useWorkspace } from "../store";
import { AccessSummary } from "./AccessSummary";
import { ActivityList } from "./ActivityList";
import { ColorRow } from "./ColorRow";
import { DetailsList } from "./DetailsList";
import { Preview } from "./Preview";
import { Section } from "./Section";

/** How many of a multi-selection are drawn in the fanned stack. Four stops reading as a stack. */
const FAN_MAX = 3;
/** Degrees between the fanned icons. Enough to read as loose paper, not as a mistake. */
const FAN_STEP = 6;

function count(n: number, one: string, many: string): string {
  return `${n} ${n === 1 ? one : many}`;
}

/** "3 files, 1 folder" — zero-count halves are dropped rather than written as "0 folders". */
function breakdown(nodes: FsNode[]): string {
  const folders = nodes.filter((node) => node.kind === "folder").length;
  const files = nodes.length - folders;
  return [
    files > 0 ? count(files, "file", "files") : null,
    folders > 0 ? count(folders, "folder", "folders") : null,
  ]
    .filter(Boolean)
    .join(", ");
}

/** The icon a node shows anywhere outside the canvas grid. */
function NodeGlyph({ node, size }: { node: FsNode; size: number }) {
  return node.kind === "folder" ? (
    <FolderIcon color={node.color ?? "graphite"} size={size} open />
  ) : (
    <FileIcon name={node.name} size={size} />
  );
}

/**
 * The right-hand pane: what is selected, described.
 *
 * Always visible, never a toggle. A details pane that has to be summoned is a
 * pane nobody reads, and half of what this app has to say about a file — is it
 * actually on this Mac, who else can edit it, who touched it last — is
 * invisible in the grid. Keeping the column permanent also keeps the canvas
 * width constant, so tiles never reflow because you clicked something.
 *
 * Three modes, one pane. Exactly one node gets the full treatment; a
 * multi-selection gets a stack, the totals, and the three actions that make
 * sense on many things at once; nothing selected falls back to the folder you
 * are standing in, which is the honest answer to "tell me about here". The
 * crossfade between modes is short and opacity-only: the pane's job is to be
 * already-read by the time you look at it.
 */
export interface InspectorProps {
  /** Seconds to hold the entrance for, so the shell can stagger rail → bar → pane. */
  delay?: number;
}

export function Inspector({ delay = 0 }: InspectorProps = {}) {
  const reduced = useReducedMotion() ?? false;

  const selection = useSelectionNodes();
  const folder = useCurrentFolder();
  const folderCreator = useMember(folder?.createdBy);

  const duplicateNodes = useWorkspace((s) => s.duplicateNodes);
  const copy = useWorkspace((s) => s.copy);
  const requestDelete = useWorkspace((s) => s.requestDelete);
  const openModal = useWorkspace((s) => s.openModal);

  const single = selection.length === 1 ? selection[0] : null;
  const multi = selection.length > 1;

  let mode: string;
  let content: ReactNode;

  if (single) {
    // Keyed on the mode, never on the node id: arrow-keying across the grid must
    // update this pane in place. Keying per node would make every neighbouring
    // selection a full crossfade — 140ms of blank pane — and would remount
    // Preview, restarting its text read on a file you are only passing through.
    mode = "item";
    content = (
      <>
        <Preview node={single} />

        <Section
          title="Details"
          action={
            <GhostButton
              className="-mr-[8px]"
              icon={<Info size={14} strokeWidth={1.75} />}
              onClick={() => openModal({ kind: "info", nodeId: single.id })}
            >
              Info
            </GhostButton>
          }
        >
          <DetailsList node={single} />
        </Section>

        <Section title="Access">
          <AccessSummary node={single} />
        </Section>

        {single.kind === "folder" ? (
          <Section title="Color">
            <ColorRow node={single} />
          </Section>
        ) : null}

        <Section title="Activity">
          <ActivityList nodeId={single.id} />
        </Section>
      </>
    );
  } else if (multi) {
    const ids = selection.map((node) => node.id);
    const total = selection.reduce((sum, node) => sum + node.sizeBytes, 0);

    mode = "multi";
    content = (
      <div data-testid="inspector-multi">
        <div className="mt-[16px] grid h-[200px] place-items-center overflow-hidden rounded-[12px] border border-line bg-bg p-[16px]">
          <div className="flex items-center">
            {selection.slice(0, FAN_MAX).map((node, index) => (
              <div
                key={node.id}
                className={index === 0 ? "" : "-ml-[30px]"}
                style={{
                  transform: `rotate(${(index - 1) * FAN_STEP}deg)`,
                  zIndex: FAN_MAX - index,
                }}
              >
                <NodeGlyph node={node} size={88} />
              </div>
            ))}
          </div>
        </div>

        <p className="mt-[16px] font-heading text-[17px] leading-[22px] font-medium tracking-[-0.01em] text-fg">
          {count(selection.length, "item", "items")} selected
        </p>
        <p className="mt-[2px] text-[12.5px] text-fg-2">
          {breakdown(selection)} · {formatBytes(total)}
        </p>

        <div className="mt-[16px] -ml-[8px] flex flex-wrap items-center gap-[2px]">
          <GhostButton
            icon={<Files size={14} strokeWidth={1.75} />}
            onClick={() => void duplicateNodes(ids)}
          >
            Duplicate
          </GhostButton>
          <GhostButton icon={<Copy size={14} strokeWidth={1.75} />} onClick={() => copy(ids)}>
            Copy
          </GhostButton>
          <GhostButton
            variant="danger"
            icon={<Trash2 size={14} strokeWidth={1.75} />}
            onClick={() => requestDelete(ids)}
          >
            Delete
          </GhostButton>
        </div>
      </div>
    );
  } else if (folder) {
    mode = "folder";
    content = (
      <div data-testid="inspector-folder" data-node-id={folder.id}>
        <Preview node={folder} />

        <p className="mt-[16px] font-heading text-[17px] leading-[22px] font-medium tracking-[-0.01em] break-words text-fg">
          {folder.name}
        </p>
        <p className="mt-[2px] text-[12.5px] text-fg-2">
          {count(folder.childCount, "item", "items")} · {formatBytes(folder.sizeBytes)}
        </p>

        {/*
          The creator line wraps rather than truncates: a half-shown name is the
          one fact in this pane that can be read as the wrong person, and the
          stamp underneath costs a line nobody misses.
        */}
        <div className="mt-[12px] flex items-start gap-[8px]">
          <Avatar
            peerId={folder.createdBy}
            name={folderCreator?.name ?? "Unknown member"}
            initials={folderCreator?.initials}
            size={20}
          />
          <span className="min-w-0 flex-1">
            <span className="block text-[12.5px] leading-[20px] break-words text-fg-2">
              Created by <span className="text-fg">{folderCreator?.name ?? "Unknown member"}</span>
            </span>
            <span className="block text-[11px] leading-[15px] text-fg-3">
              {formatRelative(folder.createdAt)}
            </span>
          </span>
        </div>

        <Section title="Access">
          <AccessSummary node={folder} />
        </Section>

        <Section title="Activity">
          <ActivityList nodeId={folder.id} />
        </Section>
      </div>
    );
  } else {
    mode = "empty";
    content = (
      <EmptyState
        className="mt-[64px]"
        icon={<Info size={16} strokeWidth={1.75} />}
        title="Nothing to show"
        detail="Open a vault to see what is inside it."
      />
    );
  }

  return (
    <motion.aside
      data-testid="inspector"
      style={{ width: INSPECTOR_W }}
      className="scroll-thin h-full shrink-0 overflow-x-hidden overflow-y-auto border-l border-line bg-surface px-[18px] pt-[16px] pb-[28px]"
      initial={reduced ? { opacity: 0 } : { x: 24, opacity: 0 }}
      animate={reduced ? { opacity: 1 } : { x: 0, opacity: 1 }}
      transition={{ duration: reduced ? 0 : 0.42, delay: reduced ? 0 : delay, ease: EASE }}
    >
      <AnimatePresence mode="wait" initial={false}>
        <motion.div
          key={mode}
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: reduced ? 0 : 0.14, ease: EASE }}
        >
          {content}
        </motion.div>
      </AnimatePresence>
    </motion.aside>
  );
}
