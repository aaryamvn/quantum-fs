/**
 * The workspace contract, in one import.
 *
 * Every UI unit of the workspace — sidebar, top bar, canvas, inspector, menus,
 * drag, modals — reaches the store through this barrel, so none of them depends
 * on how the store is split up and the split can change without touching a
 * component.
 */

export * from "./types";
export * from "./workspaceStore";
export * from "./selectors";
export * from "./events";
export * from "./geometry";
