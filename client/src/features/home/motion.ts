import type { Variants } from "motion/react";

/** The house ease — matches --ease-out-expo in globals.css. */
export const EASE: [number, number, number, number] = [0.16, 1, 0.3, 1];

export interface HomeVariants {
  container: Variants;
  lockup: Variants;
  row: Variants;
}

/**
 * Entrance choreography. One container drives everything: the lockup and every
 * row are variant children of it, so `staggerChildren` alone produces the
 * 45 ms cascade — no per-row timers.
 *
 * Reduced motion keeps the same variant graph but drops transforms and time.
 */
export function homeVariants(reduced: boolean): HomeVariants {
  if (reduced) {
    const instant: Variants = {
      hidden: { opacity: 0 },
      visible: { opacity: 1, transition: { duration: 0 } },
    };
    return {
      container: { hidden: {}, visible: { transition: { staggerChildren: 0 } } },
      lockup: instant,
      row: instant,
    };
  }

  return {
    container: {
      hidden: {},
      visible: { transition: { staggerChildren: 0.045, delayChildren: 0.02 } },
    },
    lockup: {
      hidden: { opacity: 0, y: 12 },
      visible: { opacity: 1, y: 0, transition: { duration: 0.6, ease: EASE } },
    },
    row: {
      hidden: { opacity: 0, y: 10 },
      visible: { opacity: 1, y: 0, transition: { duration: 0.5, ease: EASE } },
    },
  };
}
