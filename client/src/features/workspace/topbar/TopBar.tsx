import { motion, useReducedMotion } from "motion/react";
import type { ReactNode } from "react";

import { EASE, TOPBAR_H } from "../layout";
import { Breadcrumbs } from "./Breadcrumbs";
import { NavCarets } from "./NavCarets";
import { SearchTrigger } from "./SearchTrigger";

export interface TopBarProps {
  /** Presence avatars, injected by the integrator so the bar owns no peer state. */
  trailing?: ReactNode;
  /** Seconds to hold the entrance for, so the shell can stagger rail → bar → pane. */
  delay?: number;
}

/**
 * The window's one strip of chrome: where you are on the left, who is here and
 * what you are looking for on the right.
 *
 * It doubles as the window's drag handle (`data-tauri-drag-region`), which is
 * the reason it is this skinny and this empty — a title bar you can grab has to
 * have somewhere to grab, so the bar keeps a wide expanse of nothing between the
 * crumbs and the search field rather than filling itself with controls. The
 * attribute stays on the header alone: Tauri only begins a drag on the element
 * that carries it, so the buttons nested inside still receive their own clicks
 * and need no opt-out of their own.
 */
export function TopBar({ trailing, delay = 0 }: TopBarProps) {
  const reduced = useReducedMotion() ?? false;

  return (
    <motion.header
      data-testid="topbar"
      data-tauri-drag-region
      initial={reduced ? { opacity: 0 } : { opacity: 0, y: -12 }}
      animate={reduced ? { opacity: 1 } : { opacity: 1, y: 0 }}
      transition={{ duration: reduced ? 0 : 0.38, delay: reduced ? 0 : delay, ease: EASE }}
      style={{ height: TOPBAR_H }}
      className="flex shrink-0 items-center gap-[8px] border-b border-line bg-bg px-[12px]"
    >
      <NavCarets />
      <Breadcrumbs />
      {trailing}
      <SearchTrigger />
    </motion.header>
  );
}

export default TopBar;
