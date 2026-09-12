import { motion, useReducedMotion } from "motion/react";
import { useEffect, useRef, useState } from "react";

import { useBackend } from "@/lib/backend";
import type { OrchestrationServer } from "@/lib/backend";
import { APP_NAME } from "@/lib/brand";

import type { HomeScreenProps } from "./types";
import { CreateVaultModal } from "./CreateVaultModal";
import { ServerPlusGlyph } from "./icons";
import { JoinVaultModal } from "./JoinVaultModal";
import { homeVariants } from "./motion";
import { Card, EmptyVaultRow, JoinVaultRow, ServerHeading, VaultRow } from "./Row";
import { SetupServerModal } from "./SetupServerModal";

/**
 * Bottom fade, shown only while there is list left below the fold. Short
 * windows scroll internally with hidden scrollbars, and a hard cut at the
 * bottom edge reads as a broken layout rather than as more content.
 */
const FADE =
  "linear-gradient(to bottom, #000 calc(100% - 64px), transparent 100%)";

/**
 * The column stops 140px short of the window rather than 96px: the corner link
 * is fixed at the bottom-right, and at short window heights a taller allowance
 * runs the last card straight through it.
 */
/** Vertical scale: 32 under the wordmark, 28 between server groups, 20 to the join row. */
const GAP_LOCKUP = 32;
const GAP_GROUP = 28;
const GAP_JOIN = 20;

const EASE = "ease-[cubic-bezier(0.2,0.8,0.2,1)]";

/**
 * Home screen.
 *
 * A server is a heading, its vaults are one card underneath it, and the only
 * two things you can start from here — joining a vault, setting up a server —
 * sit at the two ends of the column: one in the list, one pinned to the corner
 * of the window where it stays out of the content's way.
 */
export function HomeScreen({ revealed }: HomeScreenProps) {
  const { servers } = useBackend();
  const reduced = useReducedMotion() ?? false;
  const v = homeVariants(reduced);

  const [setupOpen, setSetupOpen] = useState(false);
  const [joinOpen, setJoinOpen] = useState(false);
  /** The server whose plus was pressed — the New Vault dialog's only input. */
  const [vaultTarget, setVaultTarget] = useState<OrchestrationServer | null>(null);

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
    <>
      <section
        className="fixed inset-y-0 right-0 z-10 flex w-[60%] items-center justify-end pt-7 pr-[clamp(40px,9vw,128px)]"
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
          className="box-content -mx-1 max-h-[calc(100vh-140px)] w-[min(460px,calc(100%-48px))] overflow-y-auto px-1 text-left [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
        >
          <div ref={content}>
            <motion.h1
              variants={v.lockup}
              className="text-[34px] leading-none font-medium tracking-[-0.02em] text-fg"
              style={{ marginBottom: GAP_LOCKUP }}
            >
              {APP_NAME}
            </motion.h1>

            <div className="flex flex-col" style={{ gap: GAP_GROUP }}>
              {servers.map((server) => (
                <motion.div key={server.id} variants={v.row}>
                  <ServerHeading
                    name={server.name}
                    address={server.address}
                    onAdd={() => setVaultTarget(server)}
                  />
                  <Card>
                    {server.vaults.length > 0 ? (
                      server.vaults.map((vault, i) => (
                        <VaultRow key={vault.id} vault={vault} first={i === 0} />
                      ))
                    ) : (
                      <EmptyVaultRow />
                    )}
                  </Card>
                </motion.div>
              ))}
            </div>

            <motion.div variants={v.row} style={{ marginTop: GAP_JOIN }}>
              <Card>
                <JoinVaultRow onClick={() => setJoinOpen(true)} />
              </Card>
            </motion.div>
          </div>
        </motion.div>
      </section>

      {/*
        Pinned to the window, not to the list: setting up a server is the one
        thing you do before there is a list at all, and parking it in the corner
        keeps the column reading as pure content.
      */}
      <motion.button
        type="button"
        data-action="setup-server"
        onClick={() => setSetupOpen(true)}
        initial={false}
        animate={{ opacity: revealed ? 1 : 0 }}
        transition={
          reduced
            ? { duration: 0 }
            : { duration: 0.5, delay: revealed ? 0.32 : 0, ease: [0.16, 1, 0.3, 1] }
        }
        className={`fixed right-[24px] bottom-[20px] z-20 flex items-center gap-[6px] rounded-[8px]
          px-[8px] py-[6px] text-fg-3 transition-colors duration-[160ms] ${EASE}
          hover:text-fg focus-visible:text-fg`}
        style={{
          pointerEvents: revealed ? "auto" : "none",
          ["--cut" as string]: "var(--color-bg)",
        }}
      >
        <ServerPlusGlyph />
        <span className="text-[13px] leading-none font-medium">Setup New Server</span>
      </motion.button>

      <SetupServerModal open={setupOpen} onClose={() => setSetupOpen(false)} />
      <JoinVaultModal open={joinOpen} onClose={() => setJoinOpen(false)} />
      <CreateVaultModal server={vaultTarget} onClose={() => setVaultTarget(null)} />
    </>
  );
}
