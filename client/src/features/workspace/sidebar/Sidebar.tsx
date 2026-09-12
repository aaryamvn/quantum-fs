import { motion, useReducedMotion } from "motion/react";

import { Wordmark } from "@/components/brand/Wordmark";
import { Caption } from "@/components/ui/Caption";

import { EASE, SIDEBAR_W, TITLEBAR_INSET, Z } from "../layout";
import { useWorkspace } from "../store";
import { ProfileFooter } from "./ProfileFooter";
import { RecentsSection } from "./RecentsSection";
import { ServerTree } from "./ServerTree";

/**
 * The one rail: who you are, where you have just been, and every vault you can
 * reach — in that order of permanence, top to bottom.
 *
 * There is deliberately no second navigator anywhere in the app. A file manager
 * that puts favorites in one place, servers in another and the account in a
 * menu makes "where am I" a three-surface question; keeping the whole answer in
 * one 240px column means the canvas never has to explain its own context.
 *
 * The wordmark at the top is a button, not a logo: clicking it leaves the vault
 * and returns to the overview. It does that in two steps — `closeVault()` drops
 * the workspace's tree and presence, and a `qfs:home` window event tells the app
 * shell to show the overview screen. The event exists because the shell owns
 * routing and the workspace store does not know the screen above it; a custom
 * event keeps that dependency pointing one way instead of importing the shell.
 *
 * It is the same wordmark the splash and home screens set, left-aligned on the
 * rail's own 12px text edge: a centered icon would be a second brand mark, and
 * the name read in the same face in all three places is what makes them one app.
 */
export function Sidebar() {
  const reduced = useReducedMotion() ?? false;

  function goHome() {
    useWorkspace.getState().closeVault();
    window.dispatchEvent(new CustomEvent("qfs:home"));
  }

  return (
    <motion.aside
      data-testid="sidebar"
      initial={reduced ? { opacity: 0 } : { opacity: 0, x: -24 }}
      animate={{ opacity: 1, x: 0 }}
      transition={{ duration: reduced ? 0 : 0.42, ease: EASE }}
      style={{ width: SIDEBAR_W, zIndex: Z.chrome }}
      className="relative flex h-full shrink-0 flex-col border-r border-line bg-surface"
    >
      {/* Clears the macOS traffic lights and gives the frameless window something to drag by. */}
      <div data-tauri-drag-region style={{ height: TITLEBAR_INSET + 12 }} className="shrink-0" />

      <div className="flex shrink-0 px-[12px] pb-[14px]">
        <button
          type="button"
          aria-label="Overview"
          onClick={goHome}
          className="rounded-[8px] leading-none opacity-100 transition-opacity duration-[160ms] ease-[cubic-bezier(0.2,0.8,0.2,1)] hover:opacity-80"
        >
          <Wordmark size={22} />
        </button>
      </div>

      <RecentsSection />

      <div className="scroll-thin min-h-0 flex-1 overflow-y-auto px-[8px] pb-[8px]">
        <Caption className="px-[4px] pt-[16px] pb-[6px]">Servers</Caption>
        <ServerTree />
      </div>

      <ProfileFooter />
    </motion.aside>
  );
}

export default Sidebar;
