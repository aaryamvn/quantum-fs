import { ChevronDown, Server, Settings, Shield } from "lucide-react";
import { useReducedMotion } from "motion/react";
import { useCallback, useEffect, useRef, useState } from "react";
import type { MouseEvent as ReactMouseEvent } from "react";

import { IconButton } from "@/components/ui/IconButton";
import { useBackend } from "@/lib/backend";
import type { OrchestrationServer, Vault } from "@/lib/backend";

import { registerDropTarget, useWorkspace } from "../store";

/** The root folder's id is derived, never looked up: the backend guarantees this shape. */
const rootIdOf = (vaultId: string) => `root_${vaultId}`;

function openVault(vaultId: string, active: boolean): void {
  if (active) {
    useWorkspace.getState().navigateTo(rootIdOf(vaultId));
    return;
  }
  // Switching vaults tears down a tree, a subscription and a presence session —
  // the app shell owns that; the rail only names the destination.
  window.dispatchEvent(new CustomEvent("qfs:open-vault", { detail: { vaultId } }));
}

/**
 * One vault, and the only row in the app that is both a navigation and a drop
 * target.
 *
 * The row is a `div` wrapping two sibling buttons rather than one button with a
 * cog inside it: a button inside a button is invalid HTML and the inner one
 * stops being reachable by keyboard in some engines. Siblings keep DOM order —
 * row, then cog — so Tab walks the rail the way it reads.
 *
 * It registers as a drop target only for the vault that is open. Dropping onto
 * a vault you are not in would mean a cross-vault move, which is a copy plus a
 * delete across two key sets, not a move; until that has a real design, the
 * other rows stay inert so a drag cannot silently do the wrong thing.
 *
 * The vault root is registered twice across the app — here as `"sidebar"` and
 * in the top bar as the root `"crumb"` — so the registry is keyed by kind+id
 * (`store/geometry.ts`) and both entries coexist. The highlight still matches on
 * `overKind === "sidebar"` as well as the id, so hovering the root crumb lights
 * the crumb and not this row.
 */
function VaultRow({ vault }: { vault: Vault }) {
  const reduced = useReducedMotion() ?? false;
  const rootId = rootIdOf(vault.id);
  const active = useWorkspace((s) => s.vaultId === vault.id);
  const over = useWorkspace((s) => s.drag.overTargetId === rootId && s.drag.overKind === "sidebar");
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = ref.current;
    if (!active || !el) return;
    return registerDropTarget(rootId, el, "sidebar");
  }, [active, rootId]);

  const onCog = useCallback(
    (e: ReactMouseEvent) => {
      e.stopPropagation();
      useWorkspace
        .getState()
        .openModal({ kind: "vault-settings", vaultId: vault.id, tab: "general" });
    },
    [vault.id],
  );

  return (
    <div
      ref={ref}
      data-vault-id={vault.id}
      className={[
        "group relative mx-[4px] h-[30px] rounded-[7px] transition-colors duration-[160ms] ease-[cubic-bezier(0.2,0.8,0.2,1)]",
        active ? "bg-white/[0.07]" : "hover:bg-surface-hover",
        over ? "bg-violet/15 ring-1 ring-violet/60" : "",
      ]
        .filter(Boolean)
        .join(" ")}
    >
      <button
        type="button"
        aria-current={active ? "true" : undefined}
        onClick={() => openVault(vault.id, active)}
        title={vault.name}
        className="flex h-full w-full items-center gap-[8px] pr-[30px] pl-[26px] text-left"
      >
        <Shield
          size={14}
          strokeWidth={1.75}
          className={`shrink-0 ${active ? "text-fg" : "text-fg-3"}`}
        />
        <span className={`min-w-0 flex-1 truncate text-[13px] ${active ? "text-fg" : "text-fg-2"}`}>
          {vault.name}
        </span>
      </button>

      {/* Always half-visible on the open vault: its settings are the one thing
          you come back to, and hiding them entirely makes the cog a secret. */}
      <span
        className={[
          "absolute top-1/2 right-[3px] -translate-y-1/2",
          reduced ? "" : "transition-opacity duration-[120ms] ease-[cubic-bezier(0.2,0.8,0.2,1)]",
          active ? "opacity-50" : "opacity-0",
          "group-hover:opacity-100 focus-within:opacity-100",
        ]
          .filter(Boolean)
          .join(" ")}
      >
        <IconButton
          icon={<Settings size={14} strokeWidth={1.75} />}
          label="Vault settings"
          size={24}
          onClick={onCog}
        />
      </span>
    </div>
  );
}

function ServerGroup({ server }: { server: OrchestrationServer }) {
  const reduced = useReducedMotion() ?? false;
  // Open by default: a collapsed tree hides the only way into a vault, and the
  // list is short enough that collapsing is a preference, not a necessity.
  const [open, setOpen] = useState(true);

  return (
    <div data-server-id={server.id} className="pb-[4px]">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((was) => !was)}
        className="group flex h-[28px] w-full items-center gap-[8px] px-[8px] text-left"
      >
        {/*
          One leading slot, two glyphs stacked in the same grid cell: the server
          mark is what the row *is*, and the caret is what the row *does*, so the
          caret only exists while you are pointing at it. Cross-fading in place
          (rather than swapping nodes) keeps the name from shifting a pixel.
        */}
        <span aria-hidden className="grid h-[20px] w-[20px] shrink-0 place-items-center">
          <Server
            size={15}
            strokeWidth={1.75}
            className={[
              "col-start-1 row-start-1 text-fg",
              reduced
                ? ""
                : "transition-opacity duration-[160ms] ease-[cubic-bezier(0.2,0.8,0.2,1)]",
              "group-hover:opacity-0 group-focus-visible:opacity-0",
            ]
              .filter(Boolean)
              .join(" ")}
          />
          <ChevronDown
            size={15}
            strokeWidth={1.75}
            className={[
              "col-start-1 row-start-1 text-fg opacity-0",
              reduced
                ? ""
                : "transition-[opacity,transform] duration-[160ms] ease-[cubic-bezier(0.2,0.8,0.2,1)]",
              open ? "" : "-rotate-90",
              "group-hover:opacity-100 group-focus-visible:opacity-100",
            ]
              .filter(Boolean)
              .join(" ")}
          />
        </span>
        <span className="min-w-0 flex-1 truncate text-[13px] font-[600] text-fg">
          {server.address}
        </span>
        {/* Presence is white, not green: a colored dot in a monochrome rail reads
            as a warning, and reachability is a state, not an alert. */}
        <span
          aria-hidden
          title={server.online ? "Online" : "Offline"}
          className="shrink-0 rounded-full"
          style={{
            width: 6,
            height: 6,
            background: server.online ? "var(--color-fg)" : "var(--color-fg-3)",
          }}
        />
      </button>

      {open ? server.vaults.map((vault) => <VaultRow key={vault.id} vault={vault} />) : null}
    </div>
  );
}

/**
 * Every server this peer knows, and the vaults it holds.
 *
 * Servers come from the backend context rather than the workspace store on
 * purpose: the store describes the *open* vault, and this list has to keep
 * standing when no vault is open at all (and while one is being switched).
 */
export function ServerTree() {
  const { servers } = useBackend();

  return (
    <div data-testid="server-tree">
      {servers.map((server) => (
        <ServerGroup key={server.id} server={server} />
      ))}
    </div>
  );
}

export default ServerTree;
