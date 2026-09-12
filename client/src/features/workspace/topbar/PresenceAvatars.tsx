import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { Avatar } from "@/components/ui/Avatar";
import { Popover } from "@/components/ui/Popover";
import type { Member, NodeId, PeerId } from "@/lib/backend";

import { useNode, useWorkspace } from "../store";
import { PeerCard } from "./PeerCard";

/** Hover has to be deliberate: a card that opens on a pass-through is noise. */
const OPEN_DELAY_MS = 250;
/** Long enough to cross the gap from avatar to card, short enough not to linger. */
const CLOSE_DELAY_MS = 120;

/** One avatar's worth of everything the row needs, resolved once for the whole cluster. */
interface Entry {
  member: Member;
  online: boolean;
  idle: boolean;
  folderId: NodeId | null;
}

type OpenState =
  | { kind: "peer"; peerId: PeerId; el: HTMLElement }
  | { kind: "overflow"; el: HTMLElement }
  | null;

function byName(a: Entry, b: Entry): number {
  return a.member.name.localeCompare(b.member.name, undefined, { sensitivity: "base" });
}

/**
 * Who is in the vault, as a row of faces.
 *
 * Ordering is the whole design: other people first, because the question the row
 * answers is "who else is here", you last so your own face never displaces a
 * peer's, and the people who have gone dark at the end, dimmed — present on the
 * member list, absent from the room. Live presence outranks the member record
 * throughout; `member.online` is what the list said when it was fetched, a
 * `PeerPresence` is what is true this second.
 *
 * Overlap rather than a spaced row so the cluster stays one object at a glance,
 * and an arriving peer pops in on a spring instead of shoving the row sideways —
 * a join is worth noticing, and a silent reflow is not a notification.
 *
 * The faces are identical discs, so the row reads as a cluster and not a paint
 * chart; presence is carried by opacity alone — full for here, 70% for idle,
 * dimmed for gone. The "+N" disc is painted last and above the row: it is the
 * handle for everyone the row could not fit, and a handle half-covered by the
 * face in front of it is a target people miss.
 */
export function PresenceAvatars({ max = 4 }: { max?: number }) {
  const reduced = useReducedMotion() ?? false;
  const members = useWorkspace((s) => s.members);
  const presence = useWorkspace((s) => s.presence);

  const [open, setOpen] = useState<OpenState>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const cancel = useCallback(() => {
    if (timer.current !== null) {
      clearTimeout(timer.current);
      timer.current = null;
    }
  }, []);

  const schedule = useCallback(
    (next: OpenState, delay: number) => {
      cancel();
      timer.current = setTimeout(() => {
        timer.current = null;
        setOpen(next);
      }, delay);
    },
    [cancel],
  );

  const closeSoon = useCallback(() => schedule(null, CLOSE_DELAY_MS), [schedule]);
  const closeNow = useCallback(() => {
    cancel();
    setOpen(null);
  }, [cancel]);

  useEffect(() => cancel, [cancel]);

  const ordered = useMemo<Entry[]>(() => {
    const entries = members.map((member): Entry => {
      const live = presence.find((peer) => peer.peerId === member.peerId);
      return {
        member,
        online: live ? live.online : member.online,
        idle: live ? live.idle : false,
        folderId: live ? live.folderId : null,
      };
    });
    const others = entries.filter((e) => e.online && !e.member.isSelf).sort(byName);
    const self = entries.filter((e) => e.member.isSelf);
    const offline = entries.filter((e) => !e.online && !e.member.isSelf).sort(byName);
    return [...others, ...self, ...offline];
  }, [members, presence]);

  const visible = ordered.slice(0, max);
  const hidden = ordered.slice(max);

  if (ordered.length === 0) return null;

  return (
    <div data-testid="presence-avatars" className="flex items-center">
      <AnimatePresence initial={false}>
        {visible.map((entry, i) => {
          const { member, online } = entry;
          // Only the entrance and exit use opacity. Idle used to sit at 0.7, but a
          // translucent face in a stack shows the face beneath it through itself;
          // presence is carried by the dot and the popover line instead.
          const anim = reduced
            ? {
                initial: { opacity: 0 },
                animate: { opacity: 1, transition: { duration: 0 } },
                exit: { opacity: 0, transition: { duration: 0 } },
              }
            : {
                initial: { opacity: 0, scale: 0.6 },
                animate: {
                  opacity: 1,
                  scale: 1,
                  transition: { type: "spring" as const, stiffness: 500, damping: 30 },
                },
                exit: { opacity: 0, scale: 0.8, transition: { duration: 0.16 } },
              };

          return (
            <motion.button
              key={member.peerId}
              type="button"
              layout={!reduced}
              {...anim}
              data-peer-id={member.peerId}
              aria-label={member.isSelf ? `You, ${member.name}` : member.name}
              onPointerEnter={(e) =>
                schedule(
                  { kind: "peer", peerId: member.peerId, el: e.currentTarget },
                  OPEN_DELAY_MS,
                )
              }
              onPointerLeave={closeSoon}
              onFocus={(e) => {
                cancel();
                setOpen({ kind: "peer", peerId: member.peerId, el: e.currentTarget });
              }}
              onBlur={closeSoon}
              className={`relative grid size-[24px] shrink-0 place-items-center rounded-full
                focus-visible:outline-none ${i === 0 ? "" : "-ml-[6px]"}`}
              // Left-most on top. DOM order alone would lay the stack the other
              // way — each face over the one before it — which puts the last
              // avatar, and the overflow disc, in front of the first.
              style={{ zIndex: visible.length - i, boxShadow: "0 0 0 2px var(--color-bg)" }}
            >
              <Avatar
                peerId={member.peerId}
                name={member.name}
                initials={member.initials}
                size={24}
                dim={!online}
                title={member.isSelf ? "You" : member.name}
              />
            </motion.button>
          );
        })}
      </AnimatePresence>

      {/*
        The bottom of the stack: the row reads left-to-right with each disc
        tucked under the one before it, and this is the right-most thing in it.
      */}
      {hidden.length > 0 ? (
        <button
          type="button"
          aria-label={`${hidden.length} more members`}
          onPointerEnter={(e) => schedule({ kind: "overflow", el: e.currentTarget }, OPEN_DELAY_MS)}
          onPointerLeave={closeSoon}
          onFocus={(e) => {
            cancel();
            setOpen({ kind: "overflow", el: e.currentTarget });
          }}
          onBlur={closeSoon}
          className="relative -ml-[6px] grid size-[24px] shrink-0 place-items-center rounded-full
            border border-line-strong bg-white/[0.08] text-[10.5px] text-fg-2
            transition-colors duration-[160ms] ease-[cubic-bezier(0.2,0.8,0.2,1)]
            hover:text-fg focus-visible:outline-none"
          style={{ zIndex: 0, boxShadow: "0 0 0 2px var(--color-bg)" }}
        >
          +{hidden.length}
        </button>
      ) : null}

      <Popover
        open={open !== null}
        onClose={closeNow}
        anchor={open?.el ?? null}
        placement="bottom-end"
        kind="popover"
      >
        <div onPointerEnter={cancel} onPointerLeave={closeSoon}>
          {open?.kind === "peer" ? <PeerCard peerId={open.peerId} /> : null}
          {open?.kind === "overflow" ? (
            <div data-testid="presence-overflow" className="w-[216px] p-[6px]">
              {hidden.map((entry) => (
                <OverflowRow key={entry.member.peerId} entry={entry} />
              ))}
            </div>
          ) : null}
        </div>
      </Popover>
    </div>
  );
}

/**
 * One line of the overflow list. Deliberately not a card and not a button: the
 * overflow answers "who else", and making each line open another layer would
 * stack a hover on a hover for no gain.
 */
function OverflowRow({ entry }: { entry: Entry }) {
  const { member, online, idle, folderId } = entry;
  const vaultName = useWorkspace((s) => s.vaultName);
  const folder = useNode(folderId);
  const where = folder ? (folder.parentId === null ? vaultName : folder.name) : vaultName;

  return (
    <div
      data-peer-id={member.peerId}
      className="flex items-center gap-[8px] rounded-[8px] px-[7px] py-[5px]"
    >
      <Avatar
        peerId={member.peerId}
        name={member.name}
        initials={member.initials}
        size={20}
        dim={!online}
      />
      <div className="min-w-0 flex-1">
        <div className="truncate text-[12.5px] text-fg">
          {member.isSelf ? `${member.name} (you)` : member.name}
        </div>
        <div className="truncate text-[11px] text-fg-3">
          {online ? (idle ? `Idle · in ${where}` : `in ${where}`) : "Offline"}
        </div>
      </div>
      <span
        aria-hidden
        className="size-[6px] shrink-0 rounded-full"
        style={{ background: online ? "var(--color-success)" : "rgba(245,245,247,0.2)" }}
      />
    </div>
  );
}

export default PresenceAvatars;
