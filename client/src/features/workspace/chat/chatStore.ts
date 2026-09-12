/**
 * The agent bar's own tiny store.
 *
 * Deliberately NOT part of the workspace store: a conversation is scratch state
 * that belongs to one surface, it never arrives from a peer, and nothing else in
 * the app renders from it — folding it into the workspace store would make every
 * keystroke in the bar a notification to tiles and the inspector.
 *
 * It owns no transport of its own either. The question goes out through the same
 * `BackendClient` every other mutation uses, read from the workspace store at
 * send time rather than held here, so the chat cannot outlive a vault swap with
 * a stale client in hand. The reply is a stub today (the mock answers with the
 * folder's item count); when a real local model is wired up, only the daemon
 * side changes and this file does not.
 */

import { create } from "zustand";

import { useWorkspace } from "../store";

/** Who said it. `system` is the bar talking about itself — errors, not answers. */
export type ChatRole = "user" | "agent" | "system";

/**
 * The local model the bar is pointed at.
 *
 * One member on purpose: the bar no longer offers a picker, because there is one
 * model that can actually answer. The field stays so the daemon call has a name
 * to carry the day a second one exists, and adding it is a member here plus a
 * control — not a re-plumbing.
 */
export type ChatModel = "claude";

export interface ChatMessage {
  id: string;
  role: ChatRole;
  text: string;
  /** epoch ms */
  at: number;
}

export interface ChatState {
  messages: ChatMessage[];
  /** A question is in flight: the send button waits and the thread types. */
  pending: boolean;
  model: ChatModel;
  /** Thread expanded above the bar. */
  open: boolean;
}

export interface ChatActions {
  send(text: string): Promise<void>;
  setOpen(open: boolean): void;
  clear(): void;
}

export type ChatStore = ChatState & ChatActions;

/**
 * Kept clear on EACH side of the bar.
 *
 * Symmetric because the bar is centered in the center column: a one-sided
 * reservation would slide it off the middle of the pane. It is only breathing
 * room from the column's edges now — nothing floats in that corner for the bar
 * to collide with since the create button was removed.
 */
const CHAT_GUTTER = 24;

/** Widest the bar ever gets, however much room the column has. */
export const CHAT_MAX_W = 640;
/**
 * Narrowest it may get before it stops shrinking.
 *
 * Below this the bar has no typing line left worth the name, so it holds this
 * width and gives up dead center instead (see `CHAT_RIGHT`). Only reachable near
 * the 900px minimum window.
 */
const CHAT_MIN_W = 240;

/**
 * Width shared by the bar and the thread.
 *
 * Both are positioned against the center column independently, so the number
 * lives once — a thread one pixel wider than its bar reads as a misalignment
 * before anyone can say why.
 */
export const CHAT_WIDTH = `max(${CHAT_MIN_W}px, min(${CHAT_MAX_W}px, calc(100% - ${CHAT_GUTTER * 2}px)))`;

/**
 * Distance from the center column's right edge, for both surfaces.
 *
 * Centering expressed as an offset rather than `left: 50%` so it can be clamped:
 * the offset is exactly `CHAT_GUTTER` for every width where the bar is still
 * shrinking, so this IS dead center until the width floor bites, and past that
 * the bar keeps its gutter instead of touching the column's edge.
 */
export const CHAT_RIGHT = `max(calc((100% - ${CHAT_WIDTH}) / 2), ${CHAT_GUTTER}px)`;

/** Message ids are local and disposable; a counter beats a uuid dependency. */
let seq = 0;
function nextId(prefix: string): string {
  seq += 1;
  return `${prefix}_${seq}`;
}

/**
 * Bumped every time the thread is cleared.
 *
 * A question already in flight cannot be recalled from the daemon, so closing
 * the thread has to be able to disown its answer: the reply is dropped unless
 * the conversation it belongs to is still the one on screen. Without it, closing
 * mid-question re-opens the thread a second later with a stranded answer in it.
 */
let conversation = 0;

/** Errors reach the thread as prose, whatever was thrown. */
function messageOf(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string" && error.trim() !== "") return error;
  return "Something went wrong";
}

export const useChat = create<ChatStore>()((set, get) => ({
  messages: [],
  pending: false,
  model: "claude",
  open: false,

  async send(text) {
    const prompt = text.trim();
    if (prompt === "" || get().pending) return;

    const token = conversation;
    const append = (role: ChatRole, body: string) => {
      if (token !== conversation) return;
      set((s) => ({
        messages: [...s.messages, { id: nextId(role), role, text: body, at: Date.now() }],
      }));
    };

    append("user", prompt);
    set({ pending: true, open: true });

    // Read the seam at send time: the vault (and with it the client) can change
    // between two questions, and a captured client would answer about the old one.
    const { client, vaultId, folderId } = useWorkspace.getState();
    if (!client || !vaultId || !folderId) {
      append("system", "Open a vault first — the agent answers about the folder you are in.");
      if (token === conversation) set({ pending: false });
      return;
    }

    try {
      const reply = await client.askAgent({ vaultId, folderId, prompt });
      append("agent", reply.text);
    } catch (error) {
      append("system", messageOf(error));
    } finally {
      // Only this conversation's own spinner: a reply that landed after a clear
      // must not reach back into the empty thread and switch anything on.
      if (token === conversation) set({ pending: false });
    }
  },

  setOpen(open) {
    set({ open });
  },

  clear() {
    conversation += 1;
    set({ messages: [], pending: false, open: false });
  },
}));
