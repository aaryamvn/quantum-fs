import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";

import { useWorkspace } from "@/features/workspace/store";
import type { Member, VaultId, VaultMeta } from "@/lib/backend";

/** What every settings pane reads: the vault the dialog was opened on, not the one on screen. */
export interface SettingsVault {
  vaultId: VaultId | null;
  meta: VaultMeta | null;
  members: Member[];
  /** True while a read for this vault is in flight, so a pane can draw a placeholder. */
  loading: boolean;
  /** True when this vault is also the one the workspace has open behind the dialog. */
  isOpenVault: boolean;
  /** Admin of *this* vault — a role is per vault, so the store's `me` is not the answer. */
  isAdmin: boolean;
  /** Re-reads meta and members for this vault after a write. */
  refresh(): Promise<void>;
}

const EMPTY: SettingsVault = {
  vaultId: null,
  meta: null,
  members: [],
  loading: false,
  isOpenVault: false,
  isAdmin: false,
  refresh: async () => {},
};

const SettingsVaultContext = createContext<SettingsVault>(EMPTY);

/** The pane's handle on the vault being configured. */
export function useSettingsVault(): SettingsVault {
  return useContext(SettingsVaultContext);
}

export interface SettingsVaultProviderProps {
  /** `modal.vaultId` — the cog in the sidebar can be any vault's, not only the open one. */
  vaultId: VaultId | null;
  open: boolean;
  children: ReactNode;
}

/**
 * The vault the settings dialog is about.
 *
 * The sidebar's cog opens settings for whichever vault it sits on, which is very
 * often not the vault the canvas is showing — so the panes cannot read
 * `s.vaultMeta` / `s.members`, which only ever describe the open one. Editing the
 * wrong vault's name is the kind of mistake a settings dialog must make
 * impossible, so the id travels in the modal state and everything below reads it
 * from here.
 *
 * When the two coincide this is a thin pass-through over the store, so a rename
 * still lands in the sidebar through the usual refresh; when they differ the
 * dialog fetches that vault itself and keeps the copy local, leaving the store's
 * idea of the open vault untouched.
 */
export function SettingsVaultProvider({ vaultId, open, children }: SettingsVaultProviderProps) {
  const client = useWorkspace((s) => s.client);
  const openVaultId = useWorkspace((s) => s.vaultId);
  const storeMeta = useWorkspace((s) => s.vaultMeta);
  const storeMembers = useWorkspace((s) => s.members);
  const storeMe = useWorkspace((s) => s.me);
  const refreshVaultMeta = useWorkspace((s) => s.refreshVaultMeta);
  const refreshMembers = useWorkspace((s) => s.refreshMembers);
  const toast = useWorkspace((s) => s.toast);

  const isOpenVault = vaultId !== null && vaultId === openVaultId;

  const [meta, setMeta] = useState<VaultMeta | null>(null);
  const [members, setMembers] = useState<Member[]>([]);
  const [loading, setLoading] = useState(false);
  // The last vault asked for, so a slow answer for a vault the dialog has since
  // left cannot overwrite the one it is now showing.
  const wanted = useRef<VaultId | null>(null);

  const load = useCallback(async () => {
    if (isOpenVault) {
      setLoading(true);
      try {
        await Promise.all([refreshVaultMeta(), refreshMembers()]);
      } finally {
        setLoading(false);
      }
      return;
    }
    if (!client || vaultId === null) return;
    wanted.current = vaultId;
    setLoading(true);
    try {
      const [nextMeta, nextMembers] = await Promise.all([
        client.getVaultMeta(vaultId),
        client.listMembers(vaultId),
      ]);
      if (wanted.current !== vaultId) return;
      setMeta(nextMeta);
      setMembers(nextMembers);
    } catch (e) {
      if (wanted.current !== vaultId) return;
      setMeta(null);
      setMembers([]);
      toast(e instanceof Error ? e.message : "Couldn't read that vault", "error");
    } finally {
      if (wanted.current === vaultId) setLoading(false);
    }
  }, [client, vaultId, isOpenVault, refreshVaultMeta, refreshMembers, toast]);

  // Fresh on every open: a settings dialog is the one surface where a stale
  // member list or join code would actually mislead.
  useEffect(() => {
    if (!open) return;
    void load();
  }, [open, load]);

  const value = useMemo<SettingsVault>(() => {
    const list = isOpenVault ? storeMembers : members;
    const self = list.find((member) => member.isSelf) ?? (isOpenVault ? storeMe : null);
    return {
      vaultId,
      meta: isOpenVault ? storeMeta : meta,
      members: list,
      loading,
      isOpenVault,
      isAdmin: self?.role === "admin",
      refresh: load,
    };
  }, [vaultId, isOpenVault, storeMeta, storeMembers, storeMe, meta, members, loading, load]);

  return <SettingsVaultContext.Provider value={value}>{children}</SettingsVaultContext.Provider>;
}
