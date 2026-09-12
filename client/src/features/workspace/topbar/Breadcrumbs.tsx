import { AnimatePresence, LayoutGroup, motion, useReducedMotion } from "motion/react";
import { useCallback, useEffect, useRef, useState } from "react";
import type { MouseEvent as ReactMouseEvent } from "react";
import { ChevronRight, Shield } from "lucide-react";

import { FolderIcon } from "@/components/icons";
import { MenuItem, MenuList } from "@/components/ui/Menu";
import { Popover } from "@/components/ui/Popover";
import type { FsNode } from "@/lib/backend";

import { EASE } from "../layout";
import { registerDropTarget, usePath, useWorkspace } from "../store";

/** Beyond this, the middle of the path collapses into one "…" button. */
const MAX_CRUMBS = 4;

/**
 * Where you are, and every folder you can get back to in one press.
 *
 * Two things make this more than a label. It is a *drop target row*: dragging a
 * file onto an ancestor crumb moves it up the tree, which is the only gesture
 * that goes upward, so every crumb but the current one registers itself with the
 * drag hit test. And it is animated by layout rather than by re-render — crumbs
 * are keyed by folder id inside a `LayoutGroup`, so diving in slides the new
 * crumb into place beside its parent instead of repainting the whole path, and
 * the vault name flying in from the home screen lands on the root crumb through
 * the shared `vault-title` layout id. That group is deliberately *unnamed*: a
 * `LayoutGroup id` prefixes every descendant layoutId ("breadcrumbs-vault-title"),
 * which would leave the root crumb with nothing on the home screen to pair with
 * and kill the dive transition.
 *
 * The middle collapses rather than the path scrolling: a scrolled breadcrumb
 * hides exactly the ancestors it exists to offer, whereas "first … parent
 * current" keeps both ends of the journey visible at any depth.
 */
export function Breadcrumbs() {
  const folderId = useWorkspace((s) => s.folderId);
  const vaultName = useWorkspace((s) => s.vaultName);
  const crumbs = usePath(folderId);
  const reduced = useReducedMotion() ?? false;

  const overflowRef = useRef<HTMLButtonElement>(null);
  const [overflowOpen, setOverflowOpen] = useState(false);
  const closeOverflow = useCallback(() => setOverflowOpen(false), []);

  // A menu of ancestors that outlived the path it belonged to would anchor to a
  // crumb that has since moved, so arriving anywhere dismisses it.
  useEffect(() => setOverflowOpen(false), [folderId]);

  if (crumbs.length === 0) return <div data-testid="breadcrumbs" className="min-w-0 flex-1" />;

  const collapsed = crumbs.length > MAX_CRUMBS;
  const hidden = collapsed ? crumbs.slice(1, crumbs.length - 2) : [];
  const shown = collapsed ? [crumbs[0], ...crumbs.slice(crumbs.length - 2)] : crumbs;
  const lastId = crumbs[crumbs.length - 1].id;

  return (
    <nav
      data-testid="breadcrumbs"
      aria-label="Breadcrumb"
      className="flex min-w-0 flex-1 items-center"
    >
      <LayoutGroup>
        <AnimatePresence initial={false}>
          {shown.map((node, i) => {
            const showEllipsis = collapsed && i === 1;
            return (
              <motion.div
                key={node.id}
                layout={!reduced}
                initial={reduced ? { opacity: 0 } : { opacity: 0, x: -4 }}
                animate={reduced ? { opacity: 1 } : { opacity: 1, x: 0 }}
                exit={{ opacity: 0, transition: { duration: reduced ? 0 : 0.12, ease: EASE } }}
                transition={{ duration: reduced ? 0 : 0.18, ease: EASE }}
                className="flex min-w-0 items-center"
              >
                {i > 0 ? <Separator /> : null}
                {showEllipsis ? (
                  <>
                    <button
                      type="button"
                      ref={overflowRef}
                      aria-label="Show hidden folders"
                      onClick={() => setOverflowOpen(true)}
                      className="h-[26px] shrink-0 rounded-[6px] px-[6px] text-[13px] leading-none
                        text-fg-3 transition-colors duration-[160ms] ease-[cubic-bezier(0.2,0.8,0.2,1)]
                        hover:bg-white/[0.05] hover:text-fg"
                    >
                      …
                    </button>
                    <Separator />
                  </>
                ) : null}
                <Crumb
                  node={node}
                  isRoot={node.parentId === null}
                  isCurrent={node.id === lastId}
                  vaultName={vaultName}
                />
              </motion.div>
            );
          })}
        </AnimatePresence>
      </LayoutGroup>

      <Popover
        open={overflowOpen && hidden.length > 0}
        onClose={closeOverflow}
        anchor={overflowRef.current}
        placement="bottom-start"
        kind="menu"
      >
        <HiddenCrumbsMenu nodes={hidden} onClose={closeOverflow} />
      </Popover>
    </nav>
  );
}

function Separator() {
  return (
    <ChevronRight size={12} strokeWidth={1.75} className="mx-[1px] shrink-0 text-fg-3" aria-hidden />
  );
}

/**
 * One folder in the path.
 *
 * The current crumb is brighter and drops out of the drop registry: dropping a
 * file on the folder you are already in is a no-op the highlight should never
 * promise. Its right-click opens the folder's own context menu, so the crumb is
 * the handle for the folder you are standing in, which otherwise has no tile.
 */
function Crumb({
  node,
  isRoot,
  isCurrent,
  vaultName,
}: {
  node: FsNode;
  isRoot: boolean;
  isCurrent: boolean;
  vaultName: string;
}) {
  const ref = useRef<HTMLButtonElement>(null);
  // kind as well as id: the vault root is registered twice (this crumb and the
  // sidebar row), so an id-only match would light both from either hover.
  const over = useWorkspace(
    (s) => s.drag.overTargetId === node.id && s.drag.overKind === "crumb",
  );
  const navigateTo = useWorkspace((s) => s.navigateTo);
  const openContextMenu = useWorkspace((s) => s.openContextMenu);

  useEffect(() => {
    const el = ref.current;
    if (isCurrent || !el) return;
    return registerDropTarget(node.id, el, "crumb");
  }, [node.id, isCurrent]);

  function onContextMenu(e: ReactMouseEvent) {
    if (!isCurrent) return;
    e.preventDefault();
    e.stopPropagation();
    openContextMenu(e.clientX, e.clientY, node.id);
  }

  const label = isRoot ? vaultName || node.name : node.name;

  return (
    <button
      type="button"
      ref={ref}
      data-node-id={node.id}
      aria-current={isCurrent ? "page" : undefined}
      onClick={() => navigateTo(node.id)}
      onContextMenu={onContextMenu}
      className={[
        "flex h-[26px] max-w-[180px] min-w-0 items-center gap-[5px] rounded-[6px] px-[6px]",
        "text-[13px] leading-none transition-colors duration-[160ms] ease-[cubic-bezier(0.2,0.8,0.2,1)]",
        isCurrent ? "text-fg" : "text-fg-2 hover:bg-white/[0.05] hover:text-fg",
        over ? "bg-violet/20 ring-1 ring-violet/50" : "",
      ]
        .filter(Boolean)
        .join(" ")}
    >
      {isRoot ? <Shield size={14} strokeWidth={1.75} className="shrink-0 text-fg-3" aria-hidden /> : null}
      {isRoot && isCurrent ? (
        <motion.span layoutId="vault-title" className="truncate">
          {label}
        </motion.span>
      ) : (
        <span className="truncate">{label}</span>
      )}
    </button>
  );
}

/** The ancestors the collapse swallowed, in path order — top of the list is nearest the root. */
function HiddenCrumbsMenu({ nodes, onClose }: { nodes: FsNode[]; onClose(): void }) {
  const navigateTo = useWorkspace((s) => s.navigateTo);

  return (
    <MenuList onClose={onClose}>
      {nodes.map((node) => (
        <MenuItem
          key={node.id}
          icon={<FolderIcon size={16} color={node.color ?? undefined} />}
          onSelect={() => navigateTo(node.id)}
        >
          {node.name}
        </MenuItem>
      ))}
    </MenuList>
  );
}

export default Breadcrumbs;
