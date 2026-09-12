import { initialsOf } from "@/lib/color";

export interface AvatarProps {
  /** Kept for identity/keying by callers; the face itself never varies by person. */
  peerId: string;
  name: string;
  initials?: string;
  size?: 18 | 20 | 24 | 28 | 32 | 40 | 56;
  dim?: boolean;
  /** small online dot bottom-right */
  online?: boolean | null;
  className?: string;
  title?: string;
}

/**
 * A member's face, generated rather than fetched. There are no photos in this
 * app by design — an avatar service would mean an account, a request and a leak
 * of who is in the vault — so a face is initials on the house violet.
 *
 * One gradient for everybody, deliberately. A per-person color made the same
 * member read as two different people across the sidebar, the inspector and a
 * dialog, and it turned every avatar row into a paint chart; the initials carry
 * identity and the disc is just the app talking. The ring is a constant
 * violet-tinted hairline so the circle stays crisp on surface, glass and page
 * alike, and the dot only marks presence — it never carries meaning on its own.
 *
 * `dim` swaps the fill rather than lowering the alpha. A translucent disc let
 * whatever sat behind it through — in a stack, the avatar underneath — so an
 * offline face is a second, fully opaque palette: dark, desaturated, quiet.
 */
export function Avatar({
  name,
  initials,
  size = 24,
  dim = false,
  online = null,
  className = "",
  title,
}: AvatarProps) {
  const label = initials ?? initialsOf(name);
  const dot = size >= 28 ? 8 : 6;

  return (
    <span
      role="img"
      aria-label={name}
      title={title}
      className={`${dim ? "avatar-face-muted" : "avatar-face"} relative inline-flex shrink-0 items-center justify-center rounded-full ${className}`}
      style={{
        width: size,
        height: size,
        // Inset, so the hairline lands inside the disc and never fattens it.
        boxShadow: `0 0 0 1px var(${
          dim ? "--color-avatar-muted-ring" : "--color-avatar-ring"
        }) inset`,
      }}
    >
      {/*
        Caps have no descenders, so a line box centered on its em square hangs
        the initials a hair high at every size. The half-pixel nudge puts them
        back on the optical center — checked at 20/24/28/32/40.
      */}
      <span
        aria-hidden
        className="flex items-center justify-center font-medium"
        style={{
          width: size,
          height: size,
          fontSize: Math.round(size * 0.4),
          lineHeight: 1,
          letterSpacing: "0.01em",
          color: dim ? "var(--color-avatar-muted-fg)" : "#F5F5F7",
          transform: "translateY(0.5px)",
        }}
      >
        {label}
      </span>
      {online ? (
        <span
          aria-hidden
          className="absolute right-0 bottom-0 rounded-full"
          style={{
            width: dot,
            height: dot,
            background: "#3DDC84",
            border: "1.5px solid var(--color-bg, #010513)",
          }}
        />
      ) : null}
    </span>
  );
}

export default Avatar;
