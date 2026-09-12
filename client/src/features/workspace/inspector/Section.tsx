import type { ReactNode } from "react";

import { Caption } from "@/components/ui/Caption";

export interface SectionProps {
  title: string;
  /** Optional control on the caption's right — an "Info" or "View history" ghost button. */
  action?: ReactNode;
  children: ReactNode;
}

/**
 * One labeled block of the inspector.
 *
 * The inspector is a stack of unrelated facts — details, access, color,
 * activity — and without a caption they read as one undifferentiated wall. The
 * caption is the app's standard section line: sentence case at 12.5px, never
 * all caps, because uppercase micro-type in a column this narrow reads as a row
 * of shouting and costs legibility at the exact size Inter is carrying. It has
 * to say where a group starts and then get out of the way, because the value
 * under it is what the eye is looking for. Spacing, not rules or boxes, does
 * the separating — and the space above a caption is deliberately larger than
 * the space below it, so each caption belongs to what follows it.
 */
export function Section({ title, action, children }: SectionProps) {
  return (
    <section data-testid={`inspector-section-${title.toLowerCase()}`} className="mt-[24px]">
      <Caption className="mb-[10px] min-h-[20px]" action={action}>
        {title}
      </Caption>
      {children}
    </section>
  );
}
