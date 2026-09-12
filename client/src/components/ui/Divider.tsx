/**
 * The hairline that separates a toolbar's groups or a panel's sections. One
 * pixel of `line` and nothing else: at this contrast a divider should register
 * as a pause, not as a drawn edge.
 */
export function Divider({
  className,
  vertical = false,
}: {
  className?: string;
  vertical?: boolean;
}) {
  return (
    <div
      role="separator"
      aria-orientation={vertical ? "vertical" : "horizontal"}
      className={[
        "shrink-0 bg-line",
        vertical ? "h-full w-px" : "h-px w-full",
        className ?? "",
      ]
        .filter(Boolean)
        .join(" ")}
    />
  );
}
