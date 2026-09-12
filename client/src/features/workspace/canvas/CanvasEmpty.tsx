/**
 * What the canvas shows when it has no tiles to show.
 *
 * Three states, one component, because they are the same slot and the grid must
 * not decide how each one is dressed. An empty folder offers the two things you
 * came to do rather than apologising; loading draws the grid it is about to fill,
 * so the layout never jumps when the tree lands; an error says which layer failed
 * and keeps the daemon's own sentence, which is the only thing that tells anyone
 * what to fix.
 *
 * The skeleton uses the real tile metrics from `layout.ts` — a placeholder that
 * is not exactly tile-shaped is worse than no placeholder at all.
 */

import { FilePlus, FolderPlus, Sparkles } from "lucide-react";
import { motion, useReducedMotion } from "motion/react";

import { EmptyState } from "@/components/ui/EmptyState";
import { GhostButton } from "@/components/ui/GhostButton";

import { TILE_GAP_X, TILE_GAP_Y, TILE_H, TILE_W } from "../layout";
import { useWorkspace } from "../store";

export interface CanvasEmptyProps {
  kind: "empty-folder" | "loading" | "error";
  /** The failure's own words; shown under the title of `error`. */
  message?: string;
}

/** Enough rows to fill a normal window without implying a count. */
const SKELETONS = 12;

export function CanvasEmpty({ kind, message }: CanvasEmptyProps) {
  const reduced = useReducedMotion() ?? false;
  const createNode = useWorkspace((s) => s.createNode);

  if (kind === "loading") {
    return (
      <div
        data-testid="canvas-empty"
        data-kind="loading"
        aria-busy
        className="grid"
        style={{
          gridTemplateColumns: `repeat(auto-fill, ${TILE_W}px)`,
          columnGap: TILE_GAP_X,
          rowGap: TILE_GAP_Y,
          justifyContent: "start",
        }}
      >
        {Array.from({ length: SKELETONS }, (_, i) => (
          <motion.div
            key={i}
            aria-hidden
            className="rounded-[10px] bg-white/[0.04]"
            style={{ width: TILE_W, height: TILE_H }}
            initial={{ opacity: 0.5 }}
            animate={reduced ? { opacity: 0.5 } : { opacity: [0.5, 1, 0.5] }}
            transition={
              reduced
                ? { duration: 0 }
                : { duration: 1.2, repeat: Infinity, ease: "easeInOut", delay: i * 0.04 }
            }
          />
        ))}
      </div>
    );
  }

  if (kind === "error") {
    return (
      <div data-testid="canvas-empty" data-kind="error" className="grid min-h-[280px] place-items-center">
        <EmptyState title="Couldn't load this folder" detail={message} />
      </div>
    );
  }

  return (
    <div
      data-testid="canvas-empty"
      data-kind="empty-folder"
      className="grid min-h-[280px] place-items-center"
    >
      <EmptyState
        icon={<Sparkles size={16} strokeWidth={1.75} />}
        title="This folder is empty"
        detail="Drop files here or create something new"
        action={
          <div className="flex items-center gap-[8px]">
            <GhostButton
              icon={<FolderPlus size={14} strokeWidth={1.75} />}
              onClick={() => void createNode("folder")}
            >
              New folder
            </GhostButton>
            <GhostButton
              icon={<FilePlus size={14} strokeWidth={1.75} />}
              onClick={() => void createNode("file")}
            >
              New file
            </GhostButton>
          </div>
        }
      />
    </div>
  );
}
