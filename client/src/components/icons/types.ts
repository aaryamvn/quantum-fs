/**
 * The vocabulary the whole icon system is built on.
 *
 * A file's extension resolves to exactly one *family* (the silhouette and the
 * ornaments drawn on it) plus a pair of hues and a short monogram. Keeping that
 * split means 200+ extensions share 23 drawings: `.py` and `.rb` are the same
 * Code family with different hues and labels, so adding a language is a data
 * row, never a new component.
 */

export type IconFamily =
  | "document"
  | "text"
  | "code"
  | "data"
  | "image"
  | "vector"
  | "video"
  | "audio"
  | "archive"
  | "spreadsheet"
  | "presentation"
  | "pdf"
  | "font"
  | "model3d"
  | "disk"
  | "executable"
  | "database"
  | "design"
  | "config"
  | "web"
  | "book"
  | "certificate"
  | "generic";

/** Every family, in drawing-registry order. `FAMILY_COMPONENTS` must cover all of these. */
export const ICON_FAMILIES: readonly IconFamily[] = [
  "document",
  "text",
  "code",
  "data",
  "image",
  "vector",
  "video",
  "audio",
  "archive",
  "spreadsheet",
  "presentation",
  "pdf",
  "font",
  "model3d",
  "disk",
  "executable",
  "database",
  "design",
  "config",
  "web",
  "book",
  "certificate",
  "generic",
] as const;

/**
 * The coarse bucket a file falls in — what the workspace filters, groups and
 * sorts by. Deliberately smaller than `IconFamily`: the user thinks "images",
 * not "raster vs. vector".
 */
export type IconCategory =
  | "folder"
  | "image"
  | "video"
  | "audio"
  | "document"
  | "code"
  | "archive"
  | "design"
  | "data"
  | "other";

/** One row of the extension registry: how a given file type is drawn. */
export interface IconSpec {
  family: IconFamily;
  /** ≤4 uppercase chars drawn on the body ("PY", "TSX", "ZIP"); "" = none */
  label: string;
  /** primary body hue #RRGGBB */
  hue: string;
  /** darker secondary hue #RRGGBB (bottom of body / extrusion) */
  hue2: string;
  category: IconCategory;
}

/** What every family component receives. Families never read the registry themselves. */
export interface IconFamilyProps {
  size: number;
  spec: IconSpec;
  /** unique prefix for gradient/clip ids: pass useId() output */
  uid: string;
}
