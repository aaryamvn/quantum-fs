import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { useEffect, useState } from "react";

import { Modal } from "@/components/ui/Modal";
import { EASE } from "@/features/workspace/layout";
import { useWorkspace } from "@/features/workspace/store";
import type { VaultSettingsTab } from "@/features/workspace/store";
import type { VaultId } from "@/lib/backend";

import { AdvancedTab } from "./AdvancedTab";
import { GeneralTab } from "./GeneralTab";
import { MembersTab } from "./MembersTab";
import { SettingsNav } from "./SettingsNav";
import { SettingsVaultProvider, useSettingsVault } from "./SettingsVaultContext";
import { SharingTab } from "./SharingTab";
import { StorageTab } from "./StorageTab";

/** The crossfade between panes: long enough to read as a change, too short to wait on. */
const SWAP_MS = 0.14;

const TITLES: Record<VaultSettingsTab, string> = {
  general: "General",
  members: "Members",
  sharing: "Sharing",
  storage: "Storage",
  advanced: "Advanced",
};

/**
 * Everything about a vault that is not a file: its name, its people, its code,
 * its footprint, and the two ways out.
 *
 * The pane lives in the store's modal state (`{ kind: "vault-settings", tab }`)
 * rather than in local state, so the cog in the sidebar can open the dialog
 * straight onto Members and a deep link can do the same. Switching panes is an
 * `openModal` with a new tab — the dialog never closes and reopens, so nothing
 * remounts and the header keeps its place.
 *
 * The vault it configures is `modal.vaultId`, not the one the canvas is showing:
 * the cog lives on every row of the sidebar, so the two are routinely different.
 * {@link SettingsVaultProvider} owns that distinction and every pane reads the
 * vault through it, including the fresh read each open performs.
 */
export function VaultSettingsModal() {
  const reduced = useReducedMotion() ?? false;
  const modal = useWorkspace((s) => s.modal);
  const openModal = useWorkspace((s) => s.openModal);
  const closeModal = useWorkspace((s) => s.closeModal);

  const settings = modal?.kind === "vault-settings" ? modal : null;
  const open = settings !== null;

  // The dialog fades out after `modal` is already null, so the pane and the vault
  // it was on are held here — without them the body would snap to General, and to
  // the open vault, on the way out.
  const [lastTab, setLastTab] = useState<VaultSettingsTab>("general");
  const [lastVaultId, setLastVaultId] = useState<VaultId | null>(null);
  useEffect(() => {
    if (!settings) return;
    setLastTab(settings.tab);
    setLastVaultId(settings.vaultId);
  }, [settings]);
  const tab = settings?.tab ?? lastTab;
  const vaultId = settings?.vaultId ?? lastVaultId;

  const setTab = (next: VaultSettingsTab) => {
    if (!settings) return;
    openModal({ ...settings, tab: next });
  };

  return (
    <Modal open={open} onClose={closeModal} size="lg" padded={false}>
      <SettingsVaultProvider vaultId={vaultId} open={open}>
        <div data-testid="vault-settings" className="flex min-h-0 flex-1">
          <SettingsNav active={tab} onChange={setTab} />

          <div className="flex min-w-0 flex-1 flex-col">
            <SettingsHeader title={TITLES[tab]} />

            <div className="scroll-thin min-h-0 flex-1 overflow-y-auto px-[28px] pb-[28px]">
              <AnimatePresence mode="wait" initial={false}>
                <motion.div
                  key={tab}
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0 }}
                  transition={{ duration: reduced ? 0 : SWAP_MS, ease: EASE }}
                >
                  {tab === "general" ? <GeneralTab /> : null}
                  {tab === "members" ? <MembersTab onInvite={() => setTab("sharing")} /> : null}
                  {tab === "sharing" ? <SharingTab /> : null}
                  {tab === "storage" ? <StorageTab /> : null}
                  {tab === "advanced" ? <AdvancedTab /> : null}
                </motion.div>
              </AnimatePresence>
            </div>
          </div>
        </div>
      </SettingsVaultProvider>
    </Modal>
  );
}

/**
 * Pane title and the vault it belongs to. Inside the provider, so the subtitle
 * names the vault being configured rather than the one behind the dialog — the
 * whole point of the header is that you can never edit the wrong vault.
 */
function SettingsHeader({ title }: { title: string }) {
  const { meta } = useSettingsVault();
  return (
    <header className="px-[28px] pt-[24px] pr-[44px] pb-[14px]">
      <h2 className="font-heading text-[20px] leading-[26px] font-medium tracking-[-0.015em] text-fg">
        {title}
      </h2>
      <p className="mt-[2px] truncate text-[12.5px] leading-[17px] text-fg-3">{meta?.name ?? ""}</p>
    </header>
  );
}
