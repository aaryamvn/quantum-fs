import { ServerOff } from "lucide-react";
import { motion, useReducedMotion } from "motion/react";
import { useEffect, useRef, useState } from "react";

import { EmptyState } from "@/components/ui/EmptyState";
import { ToastStack } from "@/components/ui/Toast";
import type { ToastData } from "@/components/ui/Toast";
import { useWorkspace } from "@/features/workspace/store";
import { useBackend } from "@/lib/backend";
import type { OrchestrationServer, Vault, VaultId } from "@/lib/backend";
import { APP_NAME } from "@/lib/brand";

import type { HomeScreenProps } from "./types";
import { CreateVaultModal } from "./CreateVaultModal";
import { ServerPlusGlyph } from "./icons";
import { JoinVaultModal } from "./JoinVaultModal";
import { homeVariants } from "./motion";
import { Card, EmptyVaultRow, JoinVaultRow, ServerHeading, VaultRow } from "./Row";
import { SetupServerModal } from "./SetupServerModal";
import { useShellNotice } from "./shellNotice";

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

/** The removal detail carried by the `qfs:vault-removed` window event. */
interface VaultRemovedDetail {
  vaultId?: VaultId;
  reason?: string;
}

/**
 * Two server groups, drawn as bars, while the daemon is still being asked.
 *
 * Two rather than one: a single placeholder reads as "here is your one server",
 * and a list whose length is a guess should look like a list, not like an answer.
 * The metrics are the real ones — a 28px heading, a 48px row inside a card — so
 * the column does not jump when the answer lands.
 */
function ServerSkeleton({ reduced }: { reduced: boolean }) {
  return (
    <div data-testid="home-skeleton" aria-hidden className="flex flex-col" style={{ gap: GAP_GROUP }}>
      {[0, 1].map((i) => (
        <motion.div
          key={i}
          initial={{ opacity: 0.5 }}
          animate={reduced ? { opacity: 0.5 } : { opacity: [0.5, 1, 0.5] }}
          transition={
            reduced
              ? { duration: 0 }
              : { duration: 1.2, repeat: Infinity, ease: "easeInOut", delay: i * 0.12 }
          }
        >
          <div className="mb-[10px] flex h-[28px] items-center gap-[8px]">
            <span className="h-[16px] w-[16px] shrink-0 rounded-[4px] bg-white/[0.06]" />
            <span className="h-[11px] w-[112px] rounded-full bg-white/[0.06]" />
            <span className="h-[10px] w-[86px] rounded-full bg-white/[0.035]" />
          </div>
          <Card>
            <div className="flex min-h-[48px] items-center gap-[12px] px-[14px] py-[12px]">
              <span className="h-[20px] w-[20px] shrink-0 rounded-[5px] bg-white/[0.06]" />
              <span className="h-[10px] w-[142px] rounded-full bg-white/[0.05]" />
            </div>
          </Card>
        </motion.div>
      ))}
    </div>
  );
}

/**
 * What the shell needs from a vault row beyond the vault itself.
 *
 * Declared here rather than in `types.ts` because it is a shell concern, not a
 * home one: the home does not know what opening a vault does, only that the
 * element that was clicked is the rect the dive has to start from.
 */
interface Props extends HomeScreenProps {
  onOpenVault?(vault: Vault, el: HTMLElement): void;
}

/**
 * Home screen.
 *
 * A server is a heading, its vaults are one card underneath it, and the only
 * two things you can start from here — joining a vault, setting up a server —
 * sit at the two ends of the column: one in the list, one pinned to the corner
 * of the window where it stays out of the content's way.
 */
export function HomeScreen({ revealed, onOpenVault }: Props) {
  const { servers, loading, error } = useBackend();
  const reduced = useReducedMotion() ?? false;
  const v = homeVariants(reduced);

  const [setupOpen, setSetupOpen] = useState(false);
  const [joinOpen, setJoinOpen] = useState(false);
  /** The server whose plus was pressed — the New Vault dialog's only input. */
  const [vaultTarget, setVaultTarget] = useState<OrchestrationServer | null>(null);

  /**
   * The shell's one line, drawn here because this is the screen that exists when
   * the workspace does not: being removed from the open vault lands the user on
   * this list, and this is where the reason has to be readable.
   */
  const notice = useShellNotice((s) => s.notice);
  const dismissNotice = useShellNotice((s) => s.dismiss);
  const noticeToasts: ToastData[] = notice
    ? [{ id: notice.id, text: notice.text, kind: notice.tone }]
    : [];

  // A vault removed while this list is what's on screen. The workspace's event
  // handler says why for the vault it has open — and only while it is mounted —
  // so the backend seam re-announces every removal as a window event and the
  // notice for the ones nobody else speaks for is posted here. The open vault is
  // skipped on purpose: that one is the workspace's to explain (as an error, and
  // alongside the teardown), and saying it twice would overwrite its line.
  useEffect(() => {
    const onRemoved = (event: Event) => {
      const detail = (event as CustomEvent<VaultRemovedDetail>).detail;
      if (!detail?.reason) return;
      if (detail.vaultId !== undefined && useWorkspace.getState().vaultId === detail.vaultId) {
        return;
      }
      useShellNotice.getState().show(detail.reason, "info");
    };

    window.addEventListener("qfs:vault-removed", onRemoved);
    return () => window.removeEventListener("qfs:vault-removed", onRemoved);
  }, []);

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
              className="font-heading text-[34px] leading-none font-medium tracking-[-0.02em] text-fg"
              style={{ marginBottom: GAP_LOCKUP }}
            >
              {APP_NAME}
            </motion.h1>

            {/* Above the list rather than over it: the notice explains why the
                list looks different from how it was left, so it reads as part of
                the column and scrolls with it. Clicking it dismisses; it also
                goes on its own after six seconds. */}
            {notice ? (
              <div style={{ marginBottom: GAP_JOIN }}>
                <ToastStack toasts={noticeToasts} onDismiss={dismissNotice} wrap />
              </div>
            ) : null}

            {/* The daemon is the only thing this screen reads, so its two bad
                states are shown in the column rather than behind a dialog: a
                blank list would read as "you belong to nothing". */}
            {error !== null && servers.length === 0 ? (
              <Card>
                <div className="py-[28px]">
                  <EmptyState
                    icon={<ServerOff size={16} strokeWidth={1.75} />}
                    title="Can't reach the local daemon"
                    detail={error}
                  />
                </div>
              </Card>
            ) : loading && servers.length === 0 ? (
              <ServerSkeleton reduced={reduced} />
            ) : null}

            <div className="flex flex-col" style={{ gap: GAP_GROUP }}>
              {servers.map((server) => (
                <motion.div key={server.id} variants={v.row}>
                  <ServerHeading
                    address={server.address}
                    online={server.online}
                    onAdd={() => setVaultTarget(server)}
                  />
                  <Card>
                    {server.vaults.length > 0 ? (
                      server.vaults.map((vault, i) => (
                        <VaultRow
                          key={vault.id}
                          vault={vault}
                          first={i === 0}
                          onOpen={onOpenVault}
                        />
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
        <span className="text-[13px] leading-none font-medium">Add a Server</span>
      </motion.button>

      <SetupServerModal open={setupOpen} onClose={() => setSetupOpen(false)} />
      <JoinVaultModal open={joinOpen} onClose={() => setJoinOpen(false)} />
      <CreateVaultModal server={vaultTarget} onClose={() => setVaultTarget(null)} />
    </>
  );
}
