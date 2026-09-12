import { AlertTriangle, HardDrive, Link, Settings, Shield, Users } from "lucide-react";
import { useRef } from "react";
import type { KeyboardEvent, ReactNode } from "react";

import type { VaultSettingsTab } from "@/features/workspace/store";

import { useSettingsVault } from "./SettingsVaultContext";

/**
 * The rail of the vault settings dialog: which pane you are on, and what else
 * there is. A rail rather than a row of tabs because the list is five long and
 * still growing — horizontal tabs would either truncate or shrink the type, and
 * the vault's name has to sit somewhere anyway so you never edit the wrong vault.
 *
 * Up/Down move between panes the way they do in a listbox, so the whole rail is
 * one tab stop and the dialog's Tab cycle stays short.
 */

interface NavEntry {
  tab: VaultSettingsTab;
  label: string;
  icon: ReactNode;
}

const ENTRIES: NavEntry[] = [
  { tab: "general", label: "General", icon: <Settings size={16} strokeWidth={1.75} aria-hidden /> },
  { tab: "members", label: "Members", icon: <Users size={16} strokeWidth={1.75} aria-hidden /> },
  { tab: "sharing", label: "Sharing", icon: <Link size={16} strokeWidth={1.75} aria-hidden /> },
  { tab: "storage", label: "Storage", icon: <HardDrive size={16} strokeWidth={1.75} aria-hidden /> },
  {
    tab: "advanced",
    label: "Advanced",
    icon: <AlertTriangle size={16} strokeWidth={1.75} aria-hidden />,
  },
];

export interface SettingsNavProps {
  active: VaultSettingsTab;
  onChange(tab: VaultSettingsTab): void;
}

export function SettingsNav({ active, onChange }: SettingsNavProps) {
  // The vault the dialog was opened on, which is not always the open one.
  const vaultName = useSettingsVault().meta?.name ?? "";
  const root = useRef<HTMLDivElement | null>(null);

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    const index = ENTRIES.findIndex((entry) => entry.tab === active);
    if (index < 0) return;
    e.preventDefault();
    const step = e.key === "ArrowDown" ? 1 : -1;
    const next = ENTRIES[(index + step + ENTRIES.length) % ENTRIES.length];
    onChange(next.tab);
    // Focus follows selection, so the roving tab stop stays on the chosen pane.
    root.current?.querySelectorAll<HTMLButtonElement>("button")[ENTRIES.indexOf(next)]?.focus();
  };

  return (
    <div
      ref={root}
      data-testid="vault-settings-nav"
      role="tablist"
      aria-orientation="vertical"
      aria-label="Vault settings"
      onKeyDown={onKeyDown}
      className="flex w-[200px] shrink-0 flex-col gap-[2px] border-r border-line bg-surface p-[12px]"
    >
      <div className="mb-[10px] flex items-center gap-[8px] px-[10px] pt-[6px]">
        <Shield size={16} strokeWidth={1.75} className="shrink-0 text-fg-3" aria-hidden />
        <span className="truncate text-[13px] leading-none text-fg" title={vaultName}>
          {vaultName}
        </span>
      </div>

      {ENTRIES.map((entry) => {
        const selected = entry.tab === active;
        return (
          <button
            key={entry.tab}
            type="button"
            role="tab"
            aria-selected={selected}
            tabIndex={selected ? 0 : -1}
            onClick={() => onChange(entry.tab)}
            className={`flex h-[32px] items-center gap-[8px] rounded-[7px] px-[10px] text-[13px] leading-none
              transition-colors duration-[160ms] ease-[cubic-bezier(0.2,0.8,0.2,1)]
              ${selected ? "bg-white/[0.08] text-fg" : "text-fg-2 hover:bg-white/[0.05]"}`}
          >
            <span className="grid shrink-0 place-items-center text-fg-3">{entry.icon}</span>
            <span className="truncate">{entry.label}</span>
          </button>
        );
      })}
    </div>
  );
}
