import { Check } from "lucide-react";

export interface ConfirmStateProps {
  title: string;
  detail?: string;
}

/**
 * The other half of every dialog: what the form turns into once it succeeds.
 * Centred, three elements, no controls — the dialog closes itself.
 */
export function ConfirmState({ title, detail }: ConfirmStateProps) {
  return (
    <div className="flex flex-col items-center py-[8px] text-center">
      <span
        className="grid h-[44px] w-[44px] place-items-center rounded-full border border-line-strong"
        style={{ background: "rgba(255,255,255,0.06)" }}
      >
        <Check size={20} strokeWidth={1.75} className="text-fg" aria-hidden />
      </span>
      <p className="mt-[12px] text-[16px] leading-[21px] font-medium text-fg">{title}</p>
      {detail ? (
        <p className="mt-[4px] text-[13px] leading-[18px] text-fg-3 tabular-nums">{detail}</p>
      ) : null}
    </div>
  );
}
