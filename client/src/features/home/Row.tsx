import type { CSSProperties, ReactNode } from "react";

import { Chip } from "@/components/ui/Chip";
import { Tooltip } from "@/components/ui/Tooltip";
import { formatBytes } from "@/lib/format";
import type { Vault } from "@/lib/backend";

import { ICON_COL, ICON_SIZE, ICON_STROKE, Plus, Server, Shield, VaultPlusGlyph } from "./icons";

const EASE = "ease-[cubic-bezier(0.2,0.8,0.2,1)]";

/**
 * Rows live inside a card, whose `overflow: hidden` would clip a ring drawn
 * outside them; the offset is pulled in so focus stays visible on the top row.
 */
const RING = "focus-visible:[outline-offset:-2px]";

const ROW =
  `flex w-full gap-[10px] px-[14px] py-[12px] text-left ` +
  `bg-surface hover:bg-surface-hover active:bg-surface-active ` +
  `[--cut:var(--color-surface)] hover:[--cut:var(--color-surface-hover)] active:[--cut:var(--color-surface-active)] ` +
  `transition-colors duration-[160ms] ${EASE} ${RING}`;

/** The card: a bordered slab that the rows sit inside, hairlines between them. */
export function Card({
  className = "",
  style,
  children,
}: {
  className?: string;
  style?: CSSProperties;
  children: ReactNode;
}) {
  return (
    <div
      className={`overflow-hidden rounded-[12px] border border-line bg-surface ${className}`}
      style={style}
    >
      {children}
    </div>
  );
}

/**
 * A server is a heading, not a row: it names the group of vaults beneath it, so
 * the card below can be pure content.
 *
 * The address IS the name. A server has no human name worth inventing — an
 * auto-generated "Server 2" tells you nothing and, on a demo where three
 * machines are on screen at once, actively lies about which host you are
 * looking at. The `ip:port` it answers on is the only label that identifies it.
 *
 * A server the daemon cannot reach keeps its vaults on screen — they exist, and
 * hiding them would read as data loss — but goes quiet: the identity dims, an
 * Offline chip says why, and the one action it offers stops pretending it works.
 */
export function ServerHeading({
  address,
  online = true,
  onAdd,
}: {
  address: string;
  online?: boolean;
  onAdd(): void;
}) {
  /* The chip is the one thing on an offline heading that must stay readable, so
     the dimming is applied per element rather than to the row that contains it. */
  const dim = online ? "" : "opacity-45";

  const plus = (
    <button
      type="button"
      data-plus=""
      // aria-disabled, not disabled: a disabled button takes no pointer events,
      // so the tooltip explaining why it cannot be used would never appear —
      // and an unexplained dead control is the thing this is here to avoid.
      aria-disabled={online ? undefined : true}
      onClick={online ? onAdd : undefined}
      aria-label={
        online ? `Add vault to ${address}` : `Add vault to ${address} — server unreachable`
      }
      className={`-mr-[3px] grid h-[24px] w-[24px] shrink-0 place-items-center rounded-full
        transition-colors duration-[160ms] ${EASE}
        ${
          online
            ? "text-fg-3 hover:bg-surface-hover hover:text-fg focus-visible:text-fg"
            : "cursor-default text-fg-3 opacity-40"
        }`}
    >
      <Plus size={15} strokeWidth={ICON_STROKE} aria-hidden />
    </button>
  );

  return (
    <div className="mb-[10px] flex h-[28px] items-center">
      <Server
        size={ICON_SIZE}
        strokeWidth={ICON_STROKE}
        className={`shrink-0 text-fg-2 ${dim}`}
        aria-hidden
      />
      <span
        className={`ml-[8px] truncate text-[15px] leading-none font-medium tracking-[-0.005em] text-fg ${dim}`}
      >
        {address}
      </span>
      {online ? null : (
        <Chip tone="warning" size="xs" className="ml-[8px]">
          Offline
        </Chip>
      )}
      <span className="flex-1" />
      {online ? plus : <Tooltip label="Server unreachable">{plus}</Tooltip>}
    </div>
  );
}

/**
 * One vault: name on top, membership and footprint underneath.
 *
 * `onOpen` is handed the row element, not just the vault: the dive's aperture
 * starts on this row's exact rect, and only the row itself knows where it is.
 */
export function VaultRow({
  vault,
  first,
  onOpen,
}: {
  vault: Vault;
  first: boolean;
  onOpen?(vault: Vault, el: HTMLElement): void;
}) {
  const members = `${vault.memberCount} member${vault.memberCount === 1 ? "" : "s"}`;

  return (
    <button
      type="button"
      data-row="vault"
      onClick={(e) => onOpen?.(vault, e.currentTarget)}
      className={`${ROW} min-h-[58px] items-start ${first ? "" : "border-t border-line"}`}
    >
      <span
        className="flex shrink-0 items-center justify-center text-fg-2"
        style={{ width: ICON_COL, height: 20 }}
      >
        <Shield size={ICON_SIZE} strokeWidth={ICON_STROKE} aria-hidden />
      </span>
      <span className="flex min-w-0 flex-col">
        <span className="truncate text-[15px] leading-[20px] font-medium tracking-[-0.005em] text-fg">
          {vault.name}
        </span>
        <span className="truncate text-[12.5px] leading-[16px] text-fg-3 tabular-nums">
          {members}
          <span className="mx-[6px]">·</span>
          {formatBytes(vault.usedBytes)}
        </span>
      </span>
    </button>
  );
}

/** A server with nothing in it yet — the card stays, so the group keeps its shape. */
export function EmptyVaultRow() {
  return (
    <div className="flex min-h-[48px] items-center px-[14px] py-[12px] text-[13px] leading-[18px] text-fg-3">
      No vaults yet
    </div>
  );
}

/** The one action that lives in the list: joining a vault you were invited to. */
export function JoinVaultRow({ onClick }: { onClick(): void }) {
  return (
    <button
      type="button"
      data-row="join-vault"
      onClick={onClick}
      className={`${ROW} min-h-[48px] items-center`}
    >
      <span
        className="flex shrink-0 items-center justify-center text-fg-2"
        style={{ width: ICON_COL, height: 20 }}
      >
        <VaultPlusGlyph />
      </span>
      <span className="text-[15px] leading-[20px] font-medium tracking-[-0.005em] text-fg">
        Join a Vault
      </span>
    </button>
  );
}
