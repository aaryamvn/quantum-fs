import { motion, useReducedMotion } from "motion/react";
import { useEffect, useRef, useState } from "react";
import type { CSSProperties, ReactNode } from "react";

import { Iconmark } from "@/components/brand/Iconmark";
import { useBackend } from "@/lib/backend";
import { APP_NAME } from "@/lib/brand";

import type { HomeScreenProps } from "./types";
import {
  ServerGlyph,
  ServerPlusGlyph,
  VaultGlyph,
  VaultPlusGlyph,
} from "./icons";
import { homeVariants } from "./motion";
import {
  INNER_R,
  Row,
  SERVER_H,
  STANDALONE_H,
  THREAD_X,
  VAULT_H,
  VAULT_PAD_L,
} from "./Row";

/** Server rows sit one step brighter than their vaults — the monolith's capstone. */
const SERVER_SURFACE = "#0F0F11";
/** Server hover keeps the same perceived step as a vault's surface → surface-hover. */
const SERVER_SURFACE_HOVER = "var(--color-surface-active)";
/**
 * The iconmark's artwork carries ~17% padding inside its viewBox, so the box is
 * sized up until the visible mark reads ~26px tall next to the 26px wordmark,
 * then pulled left so the ink — not the box — aligns with the list edge.
 */
const MARK_SIZE = 42;
const MARK_BLEED = -6;
/** Reserve for the trailing plus so a long server name never runs under it. */
const PLUS_GUTTER = 48;
const EASE = "ease-[cubic-bezier(0.2,0.8,0.2,1)]";

/** Hairline dropping out of the server icon, dissolving past the last vault. */
const THREAD =
  "linear-gradient(180deg, rgba(245,245,247,0.2) 0%, rgba(245,245,247,0.105) 58%, rgba(245,245,247,0) 100%)";

/**
 * Bottom fade, shown only while there is list left below the fold. Short
 * windows scroll internally with hidden scrollbars, and a hard cut at the
 * bottom edge reads as a broken layout rather than as more content.
 */
const FADE =
  "linear-gradient(to bottom, #000 calc(100% - 64px), transparent 100%)";

/**
 * Vertical scale, one 8px ladder with no near-duplicate steps:
 * 16 between server groups · 24 around the actions and the "or" divider ·
 * 32 from the lockup down to the list.
 */
const GAP_GROUP = 16;
const GAP_SECTION = 24;
const GAP_LOCKUP = 32;

const TOP_R = `${INNER_R}px ${INNER_R}px 0 0`;
const BOTTOM_R = `0 0 ${INNER_R}px ${INNER_R}px`;
const ALL_R = `${INNER_R}px`;

/** The monolith: one bordered block, hairlines inside it, nothing between rows. */
function Shell({
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
      className={`rounded-[14px] border border-line transition-colors duration-[160ms] ${EASE} hover:border-line-strong ${className}`}
      style={style}
    >
      {children}
    </div>
  );
}

/**
 * Home screen — "Monolith".
 *
 * Each server and its vaults are one continuous slab; vault rows hang off a
 * hairline thread that drops out of the server's icon column. Entrance is a
 * single variant cascade driven by `revealed`.
 */
export function HomeScreen({ revealed }: HomeScreenProps) {
  const { servers } = useBackend();
  const reduced = useReducedMotion() ?? false;
  const v = homeVariants(reduced);

  const scroller = useRef<HTMLDivElement | null>(null);
  const content = useRef<HTMLDivElement | null>(null);
  const [overflowing, setOverflowing] = useState(false);

  useEffect(() => {
    const el = scroller.current;
    const inner = content.current;
    if (!el || !inner) return;
    // The content's own offsetHeight, never the scroller's scrollHeight: the
    // entrance translates every row downward, and a transformed descendant
    // inflates the scroll area, which would fade a list that in fact fits.
    const update = () =>
      setOverflowing(inner.offsetHeight - el.clientHeight - el.scrollTop > 1);
    update();
    const ro = new ResizeObserver(update);
    ro.observe(el);
    ro.observe(inner);
    el.addEventListener("scroll", update, { passive: true });
    return () => {
      ro.disconnect();
      el.removeEventListener("scroll", update);
    };
  }, []);

  return (
    <section
      className="fixed inset-y-0 right-0 z-10 flex w-[60%] items-center justify-end pt-[28px] pr-[clamp(40px,9vw,128px)]"
      style={{
        opacity: revealed ? 1 : 0,
        pointerEvents: revealed ? "auto" : "none",
      }}
    >
      <motion.div
        ref={scroller}
        variants={v.container}
        initial="hidden"
        animate={revealed ? "visible" : "hidden"}
        style={
          overflowing ? { maskImage: FADE, WebkitMaskImage: FADE } : undefined
        }
        className="box-content -mx-1 max-h-[calc(100vh-96px)] w-[min(400px,calc(100%-48px))] overflow-y-auto px-1 text-left [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
      >
        <div ref={content}>
          <motion.div
            variants={v.lockup}
            className="flex items-center gap-0"
            style={{ marginLeft: MARK_BLEED }}
          >
            <Iconmark size={MARK_SIZE} />
            <h1 className="text-[26px] leading-none font-medium tracking-[-0.015em] text-fg">
              {APP_NAME}
            </h1>
          </motion.div>

          <div className="flex flex-col" style={{ gap: GAP_GROUP, marginTop: GAP_LOCKUP }}>
            {servers.map((server) => (
              <Shell key={server.id}>
                <Row
                  kind="server"
                  label={server.name}
                  glyph={<ServerGlyph />}
                  height={SERVER_H}
                  padRight={PLUS_GUTTER}
                  radius={server.vaults.length > 0 ? TOP_R : ALL_R}
                  surface={SERVER_SURFACE}
                  surfaceHover={SERVER_SURFACE_HOVER}
                  topLight
                  plusLabel={`Add vault to ${server.name}`}
                  variants={v.row}
                />

                {server.vaults.length > 0 ? (
                  <div className="relative">
                    <span
                      aria-hidden
                      className="pointer-events-none absolute top-0 bottom-0 z-10 w-px"
                      style={{ left: THREAD_X, background: THREAD }}
                    />
                    {server.vaults.map((vault, i) => (
                      <Row
                        key={vault.id}
                        kind="vault"
                        label={vault.name}
                        glyph={<VaultGlyph />}
                        height={VAULT_H}
                        padLeft={VAULT_PAD_L}
                        radius={
                          i === server.vaults.length - 1 ? BOTTOM_R : undefined
                        }
                        divider={i === 0 ? "strong" : "line"}
                        variants={v.row}
                      />
                    ))}
                  </div>
                ) : null}
              </Shell>
            ))}
          </div>

          <Shell style={{ marginTop: GAP_SECTION }}>
            <Row
              kind="add-server"
              label="Add New Server"
              glyph={<ServerPlusGlyph />}
              height={STANDALONE_H}
              radius={ALL_R}
              variants={v.row}
            />
          </Shell>

          <motion.div
            variants={v.row}
            className="flex items-center"
            style={{ marginTop: GAP_SECTION, marginBottom: GAP_SECTION }}
          >
            <span className="h-px flex-1 bg-line-strong" />
            <span className="px-[16px] text-[13px] leading-none text-fg-3">
              or
            </span>
            <span className="h-px flex-1 bg-line-strong" />
          </motion.div>

          <Shell>
            <Row
              kind="join-vault"
              label="Join a Vault"
              glyph={<VaultPlusGlyph />}
              height={STANDALONE_H}
              radius={ALL_R}
              variants={v.row}
            />
          </Shell>
        </div>
      </motion.div>
    </section>
  );
}
