import { Search, X } from "lucide-react";
import type { KeyboardEvent, RefObject } from "react";

import { IconButton } from "@/components/ui/IconButton";

export interface SearchInputProps {
  value: string;
  onChange(next: string): void;
  /** Navigation and activation live in the modal, so the field forwards its keys. */
  onKeyDown(e: KeyboardEvent<HTMLInputElement>): void;
  /** Lets the modal hand focus back after a click on a chip or a recent search. */
  fieldRef?: RefObject<HTMLInputElement | null>;
}

/**
 * The field the whole modal is built around: one line, no border, no box.
 *
 * It is drawn as a page heading rather than as a form control — 17px on a 52px
 * line with the glyph as its only chrome — because the panel *is* the field;
 * wrapping it in a second bordered rectangle inside an already bordered dialog
 * would put two frames around one caret.
 *
 * The placeholder teaches the grammar instead of describing the control. Nobody
 * discovers `ext:svg` from a help page, but everybody reads the empty field once.
 */
export function SearchInput({ value, onChange, onKeyDown, fieldRef }: SearchInputProps) {
  return (
    <div data-testid="search-input" className="flex h-[52px] min-w-0 flex-1 items-center gap-[10px]">
      <Search size={18} strokeWidth={1.75} className="shrink-0 text-fg-3" aria-hidden />
      <input
        ref={fieldRef}
        type="text"
        aria-controls="search-results"
        aria-label="Search files and folders"
        autoFocus
        autoComplete="off"
        spellCheck={false}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={onKeyDown}
        placeholder="Search files, folders, ext:svg, by:maya…"
        className="min-w-0 flex-1 bg-transparent text-[17px] leading-[22px] text-fg
          placeholder:text-fg-3 focus:outline-none"
      />
      {value.length > 0 ? (
        <IconButton
          icon={<X size={14} strokeWidth={1.75} aria-hidden />}
          label="Clear search"
          size={24}
          tooltip={false}
          onClick={() => onChange("")}
        />
      ) : null}
    </div>
  );
}
