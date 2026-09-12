import { useMemo, useRef } from "react";
import {
  ClipboardPaste,
  CloudDownload,
  Copy,
  FilePlus,
  Files,
  FolderOpen,
  FolderPlus,
  History,
  Import,
  Info,
  Lock,
  Palette,
  Pencil,
  Share2,
  Trash2,
} from "lucide-react";

import { FileIcon } from "@/components/icons";
import { MenuItem, MenuList, MenuSection, MenuSeparator } from "@/components/ui/Menu";
import { Popover } from "@/components/ui/Popover";
import { Tooltip } from "@/components/ui/Tooltip";
import type { NodeId } from "@/lib/backend";

import { useChildren, useIsCreator, useNode, useWorkspace } from "../store";
import type { ContextMenuState } from "../store";
import { ColorSwatches } from "./ColorSwatches";
import { OwnerBlock } from "./OwnerBlock";
import { QuickActions } from "./QuickActions";

/**
 * The right-click menu, for an item and for the empty canvas behind it.
 *
 * One component for both because they are the same object seen from two angles:
 * right-clicking a file asks about that file, right-clicking the background asks
 * about the folder you are standing in — and a user who learns where "History"
 * or "Manage Access" lives should find it in the same place either way.
 *
 * The shape is deliberate and repeats top to bottom: the five hottest actions as
 * icons, the full labeled inventory, the destructive one alone below a rule, and
 * provenance at the foot. Actions apply to the whole selection when the click
 * landed inside one, which is what makes right-click-after-marquee work.
 *
 * Positioning, flipping and dismissal belong to {@link Popover}; roving focus and
 * typeahead belong to {@link MenuList} — this file only decides what is in the
 * list and what each row does. `MenuList onClose` is what closes the panel after
 * an item runs, so no row has to remember to.
 */

/** House icon metrics for every glyph in the list. */
const ICON = { size: 16, strokeWidth: 1.75 } as const;

/**
 * Access is the one action that is not yours to take unless you made the node.
 * Hidden would be worse than disabled: people would hunt for it and conclude the
 * app forgot, so it stays in place and says why.
 */
function AccessItem({ nodeId }: { nodeId: NodeId }) {
  const isCreator = useIsCreator(nodeId);
  const openModal = useWorkspace((s) => s.openModal);

  if (isCreator) {
    return (
      <MenuItem
        icon={<Lock {...ICON} />}
        onSelect={() => openModal({ kind: "access", nodeId })}
      >
        Manage Access
      </MenuItem>
    );
  }

  return (
    <Tooltip label="Only the creator can change access" placement="left">
      <span className="block">
        <MenuItem icon={<Lock {...ICON} />} disabled>
          Manage Access
        </MenuItem>
      </span>
    </Tooltip>
  );
}

/** The color row: label above, swatches beneath, aligned to the label's left edge. */
function ColorRow({ nodeId }: { nodeId: NodeId }) {
  return (
    <MenuSection className="pb-[2px]">
      <div className="flex h-[30px] w-full items-center gap-[10px] px-[10px] text-[13px] leading-none text-fg">
        <span
          className="grid shrink-0 place-items-center text-fg-2"
          style={{ width: 16, height: 16, lineHeight: 0 }}
          aria-hidden
        >
          <Palette {...ICON} />
        </span>
        <span className="min-w-0 flex-1 truncate text-left">Color</span>
      </div>
      <div className="flex items-center pb-[4px] pl-[36px]">
        <ColorSwatches nodeId={nodeId} />
      </div>
    </MenuSection>
  );
}

function ItemMenu({ nodeId }: { nodeId: NodeId }) {
  const node = useNode(nodeId);
  const selection = useWorkspace((s) => s.selection);
  const openModal = useWorkspace((s) => s.openModal);
  const openNode = useWorkspace((s) => s.openNode);
  const download = useWorkspace((s) => s.download);
  const importFiles = useWorkspace((s) => s.importFiles);
  const startRename = useWorkspace((s) => s.startRename);
  const duplicateNodes = useWorkspace((s) => s.duplicateNodes);
  const copy = useWorkspace((s) => s.copy);
  const requestDelete = useWorkspace((s) => s.requestDelete);

  if (!node) return null;

  // A right-click inside a multi-selection acts on all of it; one outside a
  // selection acts on the node under the pointer alone.
  const targets = selection.length > 1 && selection.includes(nodeId) ? selection : [nodeId];
  const multi = targets.length > 1;
  const isFolder = node.kind === "folder";

  return (
    <>
      <QuickActions nodeId={nodeId} />
      <MenuSeparator />

      <MenuItem
        icon={<Info {...ICON} />}
        shortcut="mod+i"
        onSelect={() => openModal({ kind: "info", nodeId })}
      >
        Get Info
      </MenuItem>

      {/* Open is one instruction for a local file and a remote one alike: pulling
          a file down is what opening something you do not have yet means, so the
          row never branches on availability. */}
      <MenuItem
        icon={isFolder ? <FolderOpen {...ICON} /> : <FileIcon name={node.name} size={16} />}
        onSelect={() => openNode(nodeId)}
      >
        Open
      </MenuItem>

      {/* Fetch without open, for the one case where the distinction is real: a
          file that is not on this Mac, that you want on this Mac before you are
          somewhere with no network. One file only — a selection-wide fetch is a
          different feature — and gone entirely once the bytes are here. */}
      {!multi && !isFolder && node.availability === "remote" ? (
        <MenuItem
          data-menu-item="download"
          icon={<CloudDownload {...ICON} />}
          onSelect={() => void download(nodeId)}
        >
          Download
        </MenuItem>
      ) : null}

      {/* A folder's own menu can fill it: right-clicking the tile is how you
          import into a folder without entering it first, which is the same row
          the background menu offers for the folder you are standing in. */}
      {isFolder ? (
        <MenuItem
          data-menu-item="import"
          icon={<Import {...ICON} />}
          onSelect={() => void importFiles(nodeId)}
        >
          Import files…
        </MenuItem>
      ) : null}

      <MenuItem
        icon={<Pencil {...ICON} />}
        shortcut="enter"
        disabled={multi}
        onSelect={() => startRename(nodeId)}
      >
        Rename
      </MenuItem>

      <MenuItem
        icon={<Files {...ICON} />}
        shortcut="mod+d"
        onSelect={() => void duplicateNodes(targets)}
      >
        Duplicate
      </MenuItem>

      <MenuItem icon={<Copy {...ICON} />} shortcut="mod+c" onSelect={() => copy(targets)}>
        Copy
      </MenuItem>

      <MenuItem
        icon={<Share2 {...ICON} />}
        onSelect={() => openModal({ kind: "share", nodeId })}
      >
        Share
      </MenuItem>

      {isFolder ? <ColorRow nodeId={nodeId} /> : null}

      <MenuItem
        icon={<History {...ICON} />}
        onSelect={() => openModal({ kind: "history", nodeId })}
      >
        History
      </MenuItem>

      <AccessItem nodeId={nodeId} />

      <MenuSeparator />

      <MenuItem
        icon={<Trash2 {...ICON} />}
        shortcut="mod+backspace"
        danger
        onSelect={() => requestDelete(targets)}
      >
        Delete
      </MenuItem>

      <MenuSeparator />
      <OwnerBlock nodeId={nodeId} />
    </>
  );
}

function BackgroundMenu() {
  const folderId = useWorkspace((s) => s.folderId);
  const clipboard = useWorkspace((s) => s.clipboard);
  const createNode = useWorkspace((s) => s.createNode);
  const importFiles = useWorkspace((s) => s.importFiles);
  const paste = useWorkspace((s) => s.paste);
  const selectAll = useWorkspace((s) => s.selectAll);
  const openModal = useWorkspace((s) => s.openModal);
  const children = useChildren(folderId);

  if (folderId === null) return null;

  const canPaste = clipboard !== null && clipboard.nodeIds.length > 0;

  return (
    <>
      <MenuItem
        icon={<FolderPlus {...ICON} />}
        shortcut="mod+shift+n"
        onSelect={() => void createNode("folder")}
      >
        New Folder
      </MenuItem>

      <MenuItem
        icon={<FilePlus {...ICON} />}
        shortcut="mod+n"
        onSelect={() => void createNode("file")}
      >
        New File
      </MenuItem>

      {/* Import is the only way real bytes enter a vault, so it sits with the two
          "new" rows rather than under a rule: from the user's side it is the
          third way to put something in this folder. */}
      <MenuItem
        data-menu-item="import"
        icon={<Import {...ICON} />}
        onSelect={() => void importFiles(folderId)}
      >
        Import files…
      </MenuItem>

      <MenuSeparator />

      <MenuItem
        icon={<ClipboardPaste {...ICON} />}
        shortcut="mod+v"
        disabled={!canPaste}
        onSelect={() => void paste()}
      >
        Paste
      </MenuItem>

      <MenuItem
        /* No glyph of its own, but the slot is kept so the label column stays straight. */
        icon={<span aria-hidden />}
        shortcut="mod+a"
        onSelect={() => selectAll(children.map((child) => child.id))}
      >
        Select All
      </MenuItem>

      <MenuSeparator />

      <MenuItem
        icon={<Info {...ICON} />}
        shortcut="mod+i"
        onSelect={() => openModal({ kind: "info", nodeId: folderId })}
      >
        Get Info
      </MenuItem>

      <MenuItem
        icon={<Share2 {...ICON} />}
        onSelect={() => openModal({ kind: "share", nodeId: folderId })}
      >
        Share
      </MenuItem>

      <AccessItem nodeId={folderId} />

      <MenuItem
        icon={<History {...ICON} />}
        onSelect={() => openModal({ kind: "history", nodeId: folderId })}
      >
        History
      </MenuItem>

      <MenuSeparator />
      <OwnerBlock nodeId={folderId} />
    </>
  );
}

export function ContextMenu() {
  const contextMenu = useWorkspace((s) => s.contextMenu);
  const closeContextMenu = useWorkspace((s) => s.closeContextMenu);

  // The last position is kept so the panel can animate *out* after the store has
  // already forgotten where it was; without it a dismissal is a hard cut.
  const last = useRef<ContextMenuState | null>(null);
  if (contextMenu !== null) last.current = contextMenu;
  const shown = contextMenu ?? last.current;

  // A fresh object literal every render would re-run the Popover's measure pass
  // on every store tick, so the anchor point is memoized on its coordinates.
  const placed = shown !== null;
  const x = shown?.x ?? 0;
  const y = shown?.y ?? 0;
  const anchor = useMemo(() => (placed ? { x, y } : null), [placed, x, y]);

  return (
    <Popover
      open={contextMenu !== null}
      onClose={closeContextMenu}
      anchor={anchor}
      placement="bottom-start"
      offset={2}
      kind="menu"
      className="w-[248px]"
    >
      <div data-testid="context-menu">
        <MenuList className="min-w-0 w-full" onClose={closeContextMenu}>
          {shown === null ? null : shown.nodeId === null ? (
            <BackgroundMenu />
          ) : (
            <ItemMenu nodeId={shown.nodeId} />
          )}
        </MenuList>
      </div>
    </Popover>
  );
}

export default ContextMenu;
