import type { ReactNode } from "react";

export interface EmptyStateProps {
  icon?: ReactNode;
  title: string;
  detail?: string;
  action?: ReactNode;
  className?: string;
}

/**
 * What a pane says when it holds nothing yet — an empty vault, a search with no
 * hits, a peer list before anyone connects. A blank area reads as a bug; a
 * sentence reads as a state.
 *
 * The title is fg-2 rather than fg: an empty pane is not the thing you came to
 * read, so it sits a step back from real content and never competes with the
 * surrounding chrome. The action slot is the only thing here allowed to be loud.
 */
export function EmptyState({ icon, title, detail, action, className = "" }: EmptyStateProps) {
  return (
    <div className={`flex flex-col items-center px-[24px] text-center ${className}`}>
      {icon ? (
        <span
          aria-hidden
          className="grid h-[28px] w-[28px] place-items-center text-fg-3"
        >
          {icon}
        </span>
      ) : null}
      <p className="mt-[12px] text-[14px] leading-[20px] font-medium text-fg-2">{title}</p>
      {detail ? (
        <p className="mt-[4px] text-[12.5px] leading-[18px] text-fg-3">{detail}</p>
      ) : null}
      {action ? <div className="mt-[14px]">{action}</div> : null}
    </div>
  );
}
