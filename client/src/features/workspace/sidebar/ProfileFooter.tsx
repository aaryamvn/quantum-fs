import { MoreHorizontal } from "lucide-react";
import { useRef, useState } from "react";

import { Avatar } from "@/components/ui/Avatar";
import { Chip } from "@/components/ui/Chip";
import { IconButton } from "@/components/ui/IconButton";
import { MenuItem, MenuList, MenuSeparator } from "@/components/ui/Menu";
import { Popover } from "@/components/ui/Popover";

import { useWorkspace } from "../store";

/**
 * Who you are on this machine, pinned to the bottom of the rail.
 *
 * There is no account behind this row: identity in this app is a keypair, so a
 * peer id *is* the credential you hand someone when you join a vault. The row
 * itself stays down to a face and a name — an id is a string you copy, not a
 * thing you read — which is why "Copy peer ID" sits one click away in the menu
 * and why sign-out is inert: there is nothing to sign out of yet.
 *
 * The row keeps its height even before the store has a member for us, so the
 * rail does not grow by 56px the moment a vault attaches.
 */
export function ProfileFooter() {
  const me = useWorkspace((s) => s.me);
  const vaultId = useWorkspace((s) => s.vaultId);
  const anchor = useRef<HTMLSpanElement>(null);
  const [open, setOpen] = useState(false);

  function copyPeerId() {
    if (!me) return;
    const toast = useWorkspace.getState().toast;
    void navigator.clipboard.writeText(me.peerId).then(
      () => toast("Peer ID copied", "success"),
      () => toast("Could not copy peer ID", "error"),
    );
  }

  function openVaultSettings() {
    if (vaultId === null) return;
    useWorkspace.getState().openModal({ kind: "vault-settings", vaultId, tab: "general" });
  }

  return (
    <div
      data-testid="profile-footer"
      className="flex h-[56px] shrink-0 items-center gap-[10px] border-t border-line px-[12px]"
    >
      {me ? (
        <>
          <Avatar peerId={me.peerId} name={me.name} initials={me.initials} size={32} online />

          <span className="min-w-0 flex-1 truncate text-[13px] text-fg">{me.name}</span>

          <span ref={anchor} className="shrink-0">
            <IconButton
              icon={<MoreHorizontal size={16} strokeWidth={1.75} />}
              label="Account"
              size={24}
              active={open}
              onClick={() => setOpen((was) => !was)}
            />
          </span>

          <Popover
            open={open}
            onClose={() => setOpen(false)}
            anchor={anchor.current}
            placement="top-end"
            kind="menu"
          >
            <MenuList onClose={() => setOpen(false)}>
              <MenuItem onSelect={openVaultSettings} disabled={vaultId === null}>
                Vault settings
              </MenuItem>
              <MenuItem onSelect={copyPeerId}>Copy peer ID</MenuItem>
              <MenuSeparator />
              <MenuItem disabled trailing={<Chip size="xs">soon</Chip>}>
                Sign out
              </MenuItem>
            </MenuList>
          </Popover>
        </>
      ) : (
        <span className="truncate text-[12.5px] text-fg-3">Connecting…</span>
      )}
    </div>
  );
}

export default ProfileFooter;
