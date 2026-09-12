import {
  ArrowRightLeft,
  Copy,
  FileEdit,
  Lock,
  Palette,
  Pencil,
  Plus,
  Trash2,
  CloudDownload,
} from "lucide-react";
import { motion, useReducedMotion } from "motion/react";
import { useEffect, useMemo, useRef } from "react";
import type { ComponentType } from "react";

import { Avatar } from "@/components/ui/Avatar";
import { Chip } from "@/components/ui/Chip";
import { Tooltip } from "@/components/ui/Tooltip";
import type { HistoryEvent, HistoryKind, Member, PeerId } from "@/lib/backend";
import { formatDateTime, formatDayHeading } from "@/lib/time";

import { EASE } from "../layout";
import { actorLabel } from "../store";

/** Per-row entrance delay. Fast enough to read as one wave, slow enough to see direction. */
const STAGGER = 0.025;

/** The kinds that carry a "from → to" pair worth drawing as two chips. */
const PAIRED: ReadonlySet<HistoryKind> = new Set<HistoryKind>(["renamed", "moved"]);

type Glyph = ComponentType<{ size?: number | string; strokeWidth?: number | string }>;

/**
 * One glyph per kind, so a column of events can be scanned by shape before it
 * is read: a wall of pencils is an editing session, a lock among them is the
 * moment the permissions changed.
 */
const GLYPHS: Record<HistoryKind, Glyph> = {
  created: Plus,
  renamed: Pencil,
  moved: ArrowRightLeft,
  modified: FileEdit,
  duplicated: Copy,
  colored: Palette,
  access: Lock,
  deleted: Trash2,
  downloaded: CloudDownload,
};

/** "12 Sep 2026, 14:02" → "14:02". The day is already the group's caption. */
function timeOf(at: number): string {
  return formatDateTime(at).split(", ")[1] ?? "";
}

interface DayGroup {
  heading: string;
  events: HistoryEvent[];
}

/**
 * Groups consecutive events by calendar day, keeping the order they arrived in
 * (the backend hands them over newest first). Consecutive rather than by key,
 * so a day never splits into two captions and the list stays a timeline.
 */
function groupByDay(events: HistoryEvent[], now: number): DayGroup[] {
  const groups: DayGroup[] = [];
  for (const event of events) {
    const heading = formatDayHeading(event.at, now);
    const last = groups[groups.length - 1];
    if (last && last.heading === heading) last.events.push(event);
    else groups.push({ heading, events: [event] });
  }
  return groups;
}

export interface HistoryTimelineProps {
  events: HistoryEvent[];
  members: Member[];
}

/**
 * The life of one node, drawn as a thread.
 *
 * A file in a shared vault is never just its current state — it was renamed by
 * someone, moved by someone else, and its permissions changed at a moment that
 * explains everything after it. So the events are attributed (avatar first,
 * name second) and hung off a single continuous line, which is what makes a
 * list of sentences read as a sequence rather than a log.
 *
 * The staggered entrance plays once per mount: switching the filter re-renders
 * the same instance, and a wave replaying on every toggle turns a filter into
 * an animation.
 */
export function HistoryTimeline({ events, members }: HistoryTimelineProps) {
  const reduced = useReducedMotion() ?? false;

  // Read during render for this pass, flipped after it — so only the very first
  // render of this instance carries the stagger.
  const played = useRef(false);
  const stagger = !played.current && !reduced;
  useEffect(() => {
    played.current = true;
  }, []);

  const byPeer = useMemo(() => {
    const map = new Map<PeerId, Member>();
    for (const member of members) map.set(member.peerId, member);
    return map;
  }, [members]);

  // Frozen for the render pass: two rows an hour either side of midnight must
  // not land in different groups because the clock ticked between them.
  const groups = useMemo(() => groupByDay(events, Date.now()), [events]);

  let index = -1;

  return (
    <div data-testid="history-timeline" className="flex flex-col">
      {groups.map((group) => (
        <section key={group.heading}>
          <h3 className="sticky top-0 z-[1] bg-surface-2 pt-[10px] pb-[6px] text-[12.5px] leading-[16px] text-fg-3">
            {group.heading}
          </h3>
          <div className="ml-[14px] border-l border-line">
            {group.events.map((event) => {
              index += 1;
              const Glyph = GLYPHS[event.kind];
              const member = event.by ? byPeer.get(event.by) : undefined;
              const actor = actorLabel(member);
              const paired = PAIRED.has(event.kind) && event.from !== null && event.to !== null;
              const entrance = reduced
                ? { initial: { opacity: 0 }, animate: { opacity: 1 }, transition: { duration: 0 } }
                : {
                    initial: { opacity: 0, y: 4 },
                    animate: { opacity: 1, y: 0 },
                    transition: {
                      duration: 0.3,
                      ease: EASE,
                      delay: stagger ? index * STAGGER : 0,
                    },
                  };

              return (
                <motion.div
                  key={event.id}
                  {...entrance}
                  className="relative flex items-start gap-[9px] py-[9px] pl-[26px]"
                >
                  <span
                    aria-hidden
                    className="absolute top-[8px] left-[-14px] grid h-[28px] w-[28px] place-items-center rounded-full bg-surface-2 text-fg-2 ring-1 ring-line"
                  >
                    <Glyph size={14} strokeWidth={1.75} />
                  </span>

                  <Avatar
                    peerId={event.by}
                    name={actor.name}
                    initials={actor.initials}
                    size={18}
                    className="mt-[1px]"
                  />

                  <div className="min-w-0 flex-1">
                    <p className="text-[13px] leading-[18px] text-fg-2" data-selectable>
                      <span className="text-fg">{actor.name}</span>{" "}
                      {event.summary}
                    </p>
                    {paired ? (
                      <span className="mt-[6px] flex flex-wrap items-center gap-[6px]">
                        <Chip size="xs">{event.from}</Chip>
                        <span aria-hidden className="text-[11px] leading-none text-fg-3">
                          →
                        </span>
                        <Chip size="xs" tone="violet">
                          {event.to}
                        </Chip>
                      </span>
                    ) : null}
                  </div>

                  <Tooltip label={formatDateTime(event.at)} placement="left">
                    <span className="mt-[1px] shrink-0 text-[11px] leading-[16px] text-fg-3 tabular-nums">
                      {timeOf(event.at)}
                    </span>
                  </Tooltip>
                </motion.div>
              );
            })}
          </div>
        </section>
      ))}
    </div>
  );
}

export default HistoryTimeline;
