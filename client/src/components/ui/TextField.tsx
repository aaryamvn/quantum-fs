import { useState } from "react";
import type { HTMLInputTypeAttribute } from "react";

export interface TextFieldProps {
  value: string;
  onChange(value: string): void;
  onEnter?(): void;
  placeholder?: string;
  autoFocus?: boolean;
  maxLength?: number;
  inputMode?: "text" | "url" | "numeric" | "decimal" | "email" | "tel" | "search";
  type?: HTMLInputTypeAttribute;
  "aria-label"?: string;
  "aria-invalid"?: boolean;
}

/**
 * Dialog text input. The border is the only state signal — a filled focus ring
 * would fight the white primary button directly beneath it.
 */
export function TextField({
  value,
  onChange,
  onEnter,
  placeholder,
  autoFocus = false,
  maxLength,
  inputMode = "text",
  type = "text",
  "aria-label": ariaLabel,
  "aria-invalid": ariaInvalid,
}: TextFieldProps) {
  const [focused, setFocused] = useState(false);

  return (
    <input
      type={type}
      value={value}
      autoFocus={autoFocus}
      inputMode={inputMode}
      placeholder={placeholder}
      maxLength={maxLength}
      aria-label={ariaLabel}
      aria-invalid={ariaInvalid}
      autoComplete="off"
      autoCorrect="off"
      autoCapitalize="off"
      spellCheck={false}
      onChange={(e) => onChange(e.target.value)}
      onFocus={() => setFocused(true)}
      onBlur={() => setFocused(false)}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          onEnter?.();
        }
      }}
      className="h-[44px] w-full rounded-[10px] border bg-field px-[14px] text-[15px] text-fg outline-none
        transition-[border-color] duration-[160ms] ease-[cubic-bezier(0.2,0.8,0.2,1)]
        placeholder:text-fg-3"
      style={{
        borderColor: focused ? "rgba(255,255,255,0.4)" : "var(--color-line-strong)",
      }}
    />
  );
}
