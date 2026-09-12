import { motion, useReducedMotion } from "motion/react";
import { useEffect, useState } from "react";

import { FileIcon, FolderIcon, FOLDER_COLOR_HEX, iconCategoryForName, iconSpecForName } from "@/components/icons";
import type { FsNode } from "@/lib/backend";
import { withAlpha } from "@/lib/color";

import { EASE } from "../layout";
import { useWorkspace } from "../store";

/** How many bytes of a text file the hero block asks for. Enough to fill it twice over. */
const PREVIEW_BYTES = 1200;

/** The categories whose bytes are worth showing as characters. */
const TEXTUAL = new Set(["document", "code", "data"]);

export interface PreviewProps {
  node: FsNode;
}

/**
 * The hero at the top of the inspector: the thing itself, big.
 *
 * A details pane that opens with a table of fields makes you read before you
 * recognize. The icon at 128px is the fastest possible answer to "what am I
 * looking at", and the radial wash behind it is taken from the icon's own hue —
 * so a violet folder and a red PDF each tint their own pocket of the panel and
 * the eye lands on the right block without reading a word.
 *
 * Text files get their first lines underneath, because for a note or a config
 * the content *is* the identity. The fetch is canceled on every node change:
 * clicking down a list of files must never let a slow read paint the previous
 * file's contents under the current file's icon.
 */
export function Preview({ node }: PreviewProps) {
  const client = useWorkspace((s) => s.client);
  const reduced = useReducedMotion() ?? false;
  const [text, setText] = useState<string | null>(null);
  const [reading, setReading] = useState(false);

  const isFolder = node.kind === "folder";
  const hue = isFolder
    ? FOLDER_COLOR_HEX[node.color ?? "graphite"].hue
    : iconSpecForName(node.name).hue;

  const previewable =
    !isFolder && node.availability === "local" && TEXTUAL.has(iconCategoryForName(node.name));

  useEffect(() => {
    setText(null);
    setReading(false);
    if (!client || !previewable) return;

    let live = true;
    setReading(true);
    void client.readTextPreview(node.vaultId, node.id, PREVIEW_BYTES).then(
      (value) => {
        if (!live) return;
        setText(value);
        setReading(false);
      },
      () => {
        if (!live) return;
        setText(null);
        setReading(false);
      },
    );
    return () => {
      live = false;
    };
  }, [client, previewable, node.vaultId, node.id, node.modifiedAt]);

  return (
    <div data-testid="inspector-preview" data-node-id={node.id}>
      <div className="relative mt-[16px] grid h-[200px] place-items-center overflow-hidden rounded-[12px] border border-line bg-bg p-[16px]">
        <div
          aria-hidden
          className="absolute inset-0"
          style={{
            background: `radial-gradient(60% 60% at 50% 40%, ${withAlpha(hue, 0.18)}, transparent)`,
          }}
        />
        <div className="relative">
          {isFolder ? (
            <FolderIcon color={node.color ?? "graphite"} size={128} open />
          ) : (
            <FileIcon name={node.name} size={128} />
          )}
        </div>
      </div>

      {reading ? (
        /* The bytes may have to be pulled from another peer before they can be
           read, so the wait is named rather than left as a gap under the icon. */
        <p data-testid="inspector-preview-loading" className="mt-[10px] text-[11.5px] leading-[16px] text-fg-3">
          Loading preview…
        </p>
      ) : null}

      {text !== null && text !== "" ? (
        <motion.pre
          data-selectable
          data-testid="inspector-preview-text"
          initial={reduced ? false : { opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ duration: reduced ? 0 : 0.24, ease: EASE }}
          className="mt-[10px] max-h-[160px] overflow-hidden rounded-[10px] border border-line bg-surface-2 px-[10px] py-[8px] font-[ui-monospace,SFMono-Regular,Menlo,monospace] text-[11px] leading-[16px] whitespace-pre-wrap text-fg-2"
          style={{
            maskImage: "linear-gradient(to bottom, #000 62%, transparent 100%)",
            WebkitMaskImage: "linear-gradient(to bottom, #000 62%, transparent 100%)",
          }}
        >
          {text}
        </motion.pre>
      ) : null}
    </div>
  );
}
