import type { ReactNode } from "react";

import { EXTENSION_SPECS, FileIcon, FolderIcon, ICON_FAMILIES } from "@/components/icons";
import type { IconFamily, IconSpec } from "@/components/icons";
import { FOLDER_COLORS } from "@/lib/backend/types";

/**
 * Dev-only contact sheet for the icon system, reached at `?ui=icons`.
 *
 * Every drawing is data-driven off one registry row, so a broken family, a hue
 * pair with no contrast, or a monogram that overflows its body is invisible in
 * the product until someone happens to open a folder holding that extension.
 * Laying all of them out at once turns that into a glance: a family that still
 * renders as a plain slab, or a label that has clipped, stands out against its
 * neighbours immediately.
 *
 * Each entry is shown twice — 64px, where the ornaments are designed, and 20px,
 * the list-row size where they either survive or turn to mud. Nothing here
 * animates: this is a measuring surface, and motion would only make two passes
 * harder to compare.
 */

/** Every registry row, grouped by family in drawing order so sections are stable. */
function byFamily(): { family: IconFamily; entries: [string, IconSpec][] }[] {
  const all = Object.entries(EXTENSION_SPECS).sort(([a], [b]) => a.localeCompare(b));

  return ICON_FAMILIES.map((family) => ({
    family,
    entries: all.filter(([, spec]) => spec.family === family),
  })).filter((section) => section.entries.length > 0);
}

function Caption({ children }: { children: string }) {
  return (
    <div className="px-[10px] py-[4px] text-[12.5px] leading-[16px] text-fg-3">
      {children}
    </div>
  );
}

/** One specimen: the big drawing, the list-size drawing beside it, the label under both. */
function Card({
  title,
  big,
  small,
}: {
  title: string;
  big: ReactNode;
  small: ReactNode;
}) {
  return (
    <div className="flex w-[112px] flex-col items-center gap-[8px] rounded-[10px] border border-line bg-surface p-[10px]">
      <div className="flex items-end gap-[8px]">
        {big}
        {small}
      </div>
      <div className="max-w-full truncate text-[11px] leading-none text-fg-3">{title}</div>
    </div>
  );
}

export function IconGallery() {
  const sections = byFamily();
  const count = sections.reduce((total, section) => total + section.entries.length, 0);

  return (
    <div
      data-testid="icon-gallery"
      className="scroll-thin h-full w-full overflow-y-auto bg-bg px-[24px] py-[24px]"
    >
      <h1 className="px-[10px] pb-[16px] text-[15px] leading-none font-medium text-fg">
        Icon gallery · {count} extensions
      </h1>

      {sections.map(({ family, entries }) => (
        <section key={family} className="pb-[20px]">
          <Caption>{family}</Caption>
          <div className="flex flex-wrap gap-[10px] pt-[8px]">
            {entries.map(([ext]) => (
              <Card
                key={ext}
                title={ext}
                big={<FileIcon name={`x.${ext}`} size={64} />}
                small={<FileIcon name={`x.${ext}`} size={20} />}
              />
            ))}
          </div>
        </section>
      ))}

      <section className="pb-[20px]">
        <Caption>Folders</Caption>
        <div className="flex flex-wrap gap-[10px] pt-[8px]">
          {FOLDER_COLORS.map((color) => (
            <Card
              key={color}
              title={color}
              big={<FolderIcon color={color} size={64} />}
              small={<FolderIcon color={color} size={20} />}
            />
          ))}
          <Card
            title="open"
            big={<FolderIcon color="violet" size={64} open />}
            small={<FolderIcon color="violet" size={20} open />}
          />
        </div>
      </section>
    </div>
  );
}
