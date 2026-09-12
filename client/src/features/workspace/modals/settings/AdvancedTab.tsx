import { LogOut, Trash2 } from "lucide-react";
import { useState } from "react";
import type { ReactNode } from "react";

import { GhostButton } from "@/components/ui/GhostButton";
import { TextField } from "@/components/ui/TextField";
import { useWorkspace } from "@/features/workspace/store";

import { useSettingsVault } from "./SettingsVaultContext";

/**
 * The two ways a vault ends for you, kept apart from every other setting.
 *
 * Both are irreversible and neither has an undo toast, so they are gated in
 * proportion to their blast radius: leaving costs one confirmation, deleting
 * costs typing the vault's name, which is the only thing in this dialog that
 * cannot be done by muscle memory.
 *
 * Afterwards the workspace has nothing to show, so both dismiss the dialog and
 * fire `qfs:home` — the shell owns routing, and a modal should not know how.
 * That only applies to the vault actually on screen: leaving one the sidebar
 * merely lists is not a reason to throw the person out of the vault they are in.
 */
export function AdvancedTab() {
  const client = useWorkspace((s) => s.client);
  const closeModal = useWorkspace((s) => s.closeModal);
  const toast = useWorkspace((s) => s.toast);
  const { vaultId, meta, isOpenVault, isAdmin } = useSettingsVault();
  const vaultName = meta?.name ?? "";

  const [leaving, setLeaving] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);

  const done = () => {
    closeModal();
    if (isOpenVault) window.dispatchEvent(new CustomEvent("qfs:home"));
  };

  const leave = async () => {
    if (!client || !vaultId || busy) return;
    setBusy(true);
    try {
      await client.leaveVault(vaultId);
      done();
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e), "error");
      setLeaving(false);
    } finally {
      setBusy(false);
    }
  };

  const destroy = async () => {
    if (!client || !vaultId || busy) return;
    setBusy(true);
    try {
      await client.deleteVault(vaultId);
      done();
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e), "error");
      setDeleting(false);
      setTyped("");
    } finally {
      setBusy(false);
    }
  };

  return (
    <div data-testid="vault-settings-advanced" className="flex flex-col gap-[16px]">
      <DangerCard
        icon={<LogOut size={16} strokeWidth={1.75} aria-hidden />}
        title="Leave vault"
        detail="You stop syncing this vault and lose your local copy. The vault keeps running for everyone else."
      >
        {leaving ? (
          <div className="flex items-center gap-[6px]">
            <span className="truncate text-[12.5px] text-fg-2">Leave {vaultName}?</span>
            <GhostButton variant="danger" disabled={busy} onClick={() => void leave()}>
              Leave
            </GhostButton>
            <GhostButton variant="secondary" onClick={() => setLeaving(false)}>
              Cancel
            </GhostButton>
          </div>
        ) : (
          <GhostButton variant="secondary" onClick={() => setLeaving(true)}>
            Leave&hellip;
          </GhostButton>
        )}
      </DangerCard>

      <DangerCard
        icon={<Trash2 size={16} strokeWidth={1.75} aria-hidden />}
        title="Delete vault"
        detail="Every file and the member list go with it, on every machine. This cannot be undone."
        dimmed={!isAdmin}
      >
        {!isAdmin ? (
          <p className="text-[11px] leading-[16px] text-fg-3">Only admins can delete this vault.</p>
        ) : deleting ? (
          <div className="flex flex-col gap-[10px]">
            <p className="text-[12.5px] leading-[17px] text-fg-2">
              Type <span className="text-fg">{vaultName}</span> to confirm.
            </p>
            <TextField
              value={typed}
              onChange={setTyped}
              autoFocus
              maxLength={40}
              placeholder={vaultName}
              aria-label="Vault name to confirm deletion"
              onEnter={() => {
                if (typed.trim() === vaultName) void destroy();
              }}
            />
            <div className="flex items-center gap-[6px]">
              <GhostButton
                variant="danger"
                disabled={busy || typed.trim() !== vaultName}
                onClick={() => void destroy()}
              >
                Delete vault
              </GhostButton>
              <GhostButton
                variant="secondary"
                onClick={() => {
                  setDeleting(false);
                  setTyped("");
                }}
              >
                Cancel
              </GhostButton>
            </div>
          </div>
        ) : (
          <GhostButton variant="danger" onClick={() => setDeleting(true)}>
            Delete&hellip;
          </GhostButton>
        )}
      </DangerCard>
    </div>
  );
}

function DangerCard({
  icon,
  title,
  detail,
  dimmed = false,
  children,
}: {
  icon: ReactNode;
  title: string;
  detail: string;
  dimmed?: boolean;
  children: ReactNode;
}) {
  return (
    <section
      className={`rounded-[12px] border border-coral/30 bg-coral/[0.06] p-[16px] ${
        dimmed ? "opacity-60" : ""
      }`}
    >
      <div className="flex items-center gap-[8px]">
        <span className="grid shrink-0 place-items-center text-coral">{icon}</span>
        <h3 className="text-[13px] leading-none font-medium text-fg">{title}</h3>
      </div>
      <p className="mt-[8px] text-[11.5px] leading-[16px] text-fg-3">{detail}</p>
      <div className="mt-[12px]">{children}</div>
    </section>
  );
}
