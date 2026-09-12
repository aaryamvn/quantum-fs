import { Sparkles, X } from "lucide-react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { useEffect, useRef } from "react";

import { Avatar } from "@/components/ui/Avatar";
import { IconButton } from "@/components/ui/IconButton";

import type { Member } from "@/lib/backend";

import { EASE, Z } from "../layout";
import { useWorkspace } from "../store";
import { CHAT_RIGHT, CHAT_WIDTH, useChat } from "./chatStore";
import type { ChatMessage } from "./chatStore";

/** The three dots, so the delays are written once. */
const DOTS = [0, 1, 2];

/**
 * The conversation, only while there is one.
 *
 * It rises out of the bar rather than opening a pane: the answer is about the
 * folder behind it, so covering the grid with a sidebar would hide the subject
 * mid-sentence. Nothing here persists — closing clears, because a stale thread
 * about a folder you have since left is worse than no thread at all.
 *
 * Positioned against the center column exactly like {@link ChatBar} and sharing
 * its width constant, so the two read as one object that grew rather than two
 * panels that happen to be stacked.
 */
export function ChatThread() {
  const reduced = useReducedMotion() ?? false;
  const scroller = useRef<HTMLDivElement | null>(null);

  const messages = useChat((s) => s.messages);
  const pending = useChat((s) => s.pending);
  const open = useChat((s) => s.open);
  const clear = useChat((s) => s.clear);
  const me = useWorkspace((s) => s.me);

  const visible = open && (messages.length > 0 || pending);

  // The newest line is the one you asked for; the thread is always pinned to it.
  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
  }, [messages.length, pending, visible]);

  const enter = reduced
    ? {
        initial: { opacity: 0 },
        animate: { opacity: 1, transition: { duration: 0.16 } },
        exit: { opacity: 0, transition: { duration: 0.12 } },
      }
    : {
        initial: { opacity: 0, y: 12 },
        animate: { opacity: 1, y: 0, transition: { duration: 0.22, ease: EASE } },
        exit: { opacity: 0, y: 8, transition: { duration: 0.14, ease: EASE } },
      };

  const line = reduced
    ? { initial: { opacity: 0 }, animate: { opacity: 1 }, transition: { duration: 0.16 } }
    : {
        initial: { opacity: 0, y: 6 },
        animate: { opacity: 1, y: 0 },
        transition: { duration: 0.2, ease: EASE },
      };

  return (
    <div
      data-testid="chat-thread"
      className="pointer-events-none absolute bottom-[76px]"
      style={{ width: CHAT_WIDTH, right: CHAT_RIGHT, zIndex: Z.chrome }}
    >
      <AnimatePresence>
        {visible ? (
          <motion.div
            ref={scroller}
            {...enter}
            className="glass scroll-thin pointer-events-auto max-h-[42vh] overflow-y-auto
              rounded-[14px] border border-line-strong p-[14px]
              shadow-[0_18px_50px_rgba(0,0,0,0.45)]"
          >
            <div
              className="sticky top-0 z-[1] -mx-[14px] -mt-[14px] mb-[10px] flex items-center
                justify-between bg-surface-2/85 px-[14px] pt-[14px] pb-[8px] backdrop-blur-[6px]"
            >
              <span className="text-[12.5px] leading-[16px] text-fg-3">Conversation</span>
              <IconButton
                icon={<X size={16} strokeWidth={1.75} />}
                label="Close conversation"
                tooltip={false}
                size={24}
                onClick={() => clear()}
              />
            </div>

            <ul className="flex flex-col gap-[10px]">
              {messages.map((message) => (
                <motion.li key={message.id} {...line}>
                  <Row message={message} me={me} />
                </motion.li>
              ))}

              {pending ? (
                <motion.li key="typing" {...line} className="flex items-center gap-[8px]">
                  <AgentBadge />
                  <span className="flex items-center gap-[4px] rounded-[12px] bg-white/[0.05] px-[12px] py-[9px]">
                    {DOTS.map((i) => (
                      <motion.span
                        key={i}
                        className="block h-[5px] w-[5px] rounded-full bg-fg-3"
                        animate={reduced ? { opacity: [0.4, 1, 0.4] } : { y: [0, -4, 0] }}
                        transition={{
                          duration: 1,
                          repeat: Infinity,
                          ease: "easeInOut",
                          delay: i * 0.15,
                        }}
                      />
                    ))}
                  </span>
                </motion.li>
              ) : null}
            </ul>
          </motion.div>
        ) : null}
      </AnimatePresence>
    </div>
  );
}

/** The agent's face: the same mark the bar wears, at half the weight. */
function AgentBadge() {
  return (
    <span
      aria-hidden
      className="grid h-[20px] w-[20px] shrink-0 place-items-center rounded-full bg-white/[0.06] text-fg-2"
    >
      <Sparkles size={14} strokeWidth={1.75} />
    </span>
  );
}

/**
 * One line of the conversation.
 *
 * Side, color and shape carry the role — a bubble on the right is you, a mark
 * on the left is the agent, centered gray is the app apologizing — so the thread
 * needs no "You:"/"Claude:" labels eating a line each.
 */
function Row({ message, me }: { message: ChatMessage; me: Member | null }) {
  if (message.role === "system") {
    return (
      <p data-selectable className="px-[24px] text-center text-[12px] leading-[17px] text-fg-3">
        {message.text}
      </p>
    );
  }

  if (message.role === "agent") {
    return (
      <div className="flex items-start gap-[8px]">
        <AgentBadge />
        <p
          data-selectable
          className="max-w-[85%] pt-[1px] text-[13.5px] leading-[19px] whitespace-pre-wrap text-fg-2"
        >
          {message.text}
        </p>
      </div>
    );
  }

  return (
    <div className="flex items-end justify-end gap-[8px]">
      <p
        data-selectable
        className="max-w-[80%] rounded-[12px] bg-white/[0.08] px-[12px] py-[8px]
          text-[13.5px] leading-[19px] whitespace-pre-wrap text-fg"
      >
        {message.text}
      </p>
      <Avatar
        peerId={me?.peerId ?? "me"}
        name={me?.name ?? "You"}
        initials={me?.initials}
        size={20}
      />
    </div>
  );
}

export default ChatThread;
