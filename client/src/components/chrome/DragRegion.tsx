import { isTauri } from "@tauri-apps/api/core";

/**
 * Invisible strip along the top of the window that drags the native frame.
 * The app uses an overlay title bar, so without this the window cannot be moved.
 * Renders nothing in a plain browser, where the attribute is meaningless.
 */
export function DragRegion() {
  if (!isTauri()) return null;
  return <div data-tauri-drag-region className="fixed inset-x-0 top-0 z-50 h-7" aria-hidden />;
}
