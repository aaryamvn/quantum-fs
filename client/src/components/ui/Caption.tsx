import type { ReactNode } from "react";

/**
 * The line that names a section — "Recents", "Servers", "Access", "Color".
 *
 * Sentence case and quiet, never all caps: uppercase micro-type is a shout that
 * a panel full of them turns into noise, and it costs legibility at the exact
 * size where Inter is doing the most work. The caption's job is to be findable
 * when you look for it and invisible when you don't, so it stays at the same
 * 12.5px as the smallest body line and leans on color, not weight or tracking.
 *
 * `action` is the optional control that belongs to the section rather than to
 * any one row in it ("See all", "Add"), parked on the same baseline.
 */
export function Caption({
  children,
  action,
  className,
}: {
  children: ReactNode;
  action?: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={[
        "flex items-center justify-between text-[12.5px] leading-[16px] text-fg-3",
        className ?? "",
      ]
        .filter(Boolean)
        .join(" ")}
    >
      <span className="min-w-0 truncate">{children}</span>
      {action ? <span className="ml-[8px] shrink-0">{action}</span> : null}
    </div>
  );
}

export default Caption;
