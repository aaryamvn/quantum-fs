/** The product name, spelled the one way it is ever spelled. */
const APP_NAME = "QuantumFS";

/**
 * The wordmark, set the same everywhere it appears — splash, home, sidebar —
 * so the app never looks like two products. It is one of the few places left
 * that still uses GT Walsheim: the name is the only word in the UI large enough
 * for a display face to do anything but blur (docs/decisions/client-typography-inter.md).
 *
 * Size is the only knob. Weight, tracking and color are fixed, because a
 * heavier or looser wordmark is a different logo.
 */
export function Wordmark({ size = 22, className }: { size?: number; className?: string }) {
  return (
    <span
      className={[
        "font-heading leading-none font-medium tracking-[-0.02em] text-fg",
        className ?? "",
      ]
        .filter(Boolean)
        .join(" ")}
      style={{ fontSize: size }}
    >
      {APP_NAME}
    </span>
  );
}

export default Wordmark;
