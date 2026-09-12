/**
 * The one line the shell can say when the screen that would have said it is gone.
 *
 * Toasts live in the workspace store and are drawn by the workspace screen, so
 * anything that *ends* the workspace — being removed from the open vault — has
 * nowhere to put its own explanation: `ToastStack` unmounts with the screen and
 * the sentence goes with it. This store outlives both worlds, so an event
 * handler can leave a notice behind and let the home screen find it on mount.
 *
 * One notice at a time, on purpose: this is the channel for facts that change
 * which screen you are on, and there is never a queue of those worth reading.
 * The timer is module state rather than store state, because a countdown in a
 * store is a re-render per tick for a thing nothing renders.
 */

import { create } from "zustand";

export type ShellNoticeTone = "info" | "error";

export interface ShellNotice {
  id: string;
  text: string;
  tone: ShellNoticeTone;
}

/** Long enough to read twice — it is usually the only account of why you are here. */
const NOTICE_MS = 6000;

let seq = 0;
let timer: ReturnType<typeof setTimeout> | null = null;

export interface ShellNoticeState {
  /** Null when there is nothing to say. */
  notice: ShellNotice | null;
  /** Replaces whatever stood before and returns the new notice's id. */
  show(text: string, tone?: ShellNoticeTone): string;
  /** With an id, a no-op unless that notice is still the one showing. */
  dismiss(id?: string): void;
}

export const useShellNotice = create<ShellNoticeState>((set, get) => ({
  notice: null,

  show(text, tone = "info") {
    seq += 1;
    const id = `notice_${seq}`;
    if (timer !== null) clearTimeout(timer);
    set({ notice: { id, text, tone } });
    // Keyed on the id so a later notice's timer cannot clear an earlier one's
    // replacement out from under it.
    timer = setTimeout(() => {
      timer = null;
      get().dismiss(id);
    }, NOTICE_MS);
    return id;
  },

  dismiss(id) {
    const current = get().notice;
    if (current === null) return;
    if (id !== undefined && current.id !== id) return;
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
    set({ notice: null });
  },
}));
