import { Lock } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { Avatar } from "@/components/ui/Avatar";
import { Chip } from "@/components/ui/Chip";
import { ConfirmState } from "@/components/ui/ConfirmState";
import { EmptyState } from "@/components/ui/EmptyState";
import { Modal } from "@/components/ui/Modal";
import { PrimaryButton } from "@/components/ui/PrimaryButton";
import { Segmented } from "@/components/ui/Segmented";
import { Toggle } from "@/components/ui/Toggle";

import type { AccessEntry, AccessLevel, Member, NodeId } from "@/lib/backend";
import { formatPath } from "@/lib/path";

import {
  useIsCreator,
  useMember,
  useNode,
  useOnlineMembers,
  usePath,
  useWorkspace,
} from "../store";

/** "none" is not a backend level: it is the absence of an entry, named so the switch can show it. */
type RowLevel = AccessLevel | "none";

const LEVEL_OPTIONS: { value: RowLevel; label: string }[] = [
  { value: "viewer", label: "Viewer" },
  { value: "editor", label: "Editor" },
  { value: "none", label: "None" },
];

/** How long the success state stands before the dialog dismisses itself. */
const CONFIRM_MS = 1000;

/** Fired after a successful write so any open surface can re-read the node's access. */
const ACCESS_CHANGED = "qfs:access-changed";

/**
 * `inherit: true` with no entries is the backend's spelling of the vault default —
 * every member is an editor — not "nobody has access". Seeding it as an empty map
 * would both lie in the grayed inherited list and, once inheritance is switched
 * off, make Save write an empty entry list: one click, whole vault revoked.
 */
function levelsOf(
  access: { inherit: boolean; entries: AccessEntry[] },
  members: Member[],
): Record<string, RowLevel> {
  const map: Record<string, RowLevel> = {};
  if (access.inherit && access.entries.length === 0) {
    for (const member of members) map[member.peerId] = "editor";
    return map;
  }
  for (const entry of access.entries) map[entry.peerId] = entry.level;
  return map;
}

/** Stable, order-independent comparison — the backend is free to return entries in any order. */
function fingerprint(members: Member[], levels: Record<string, RowLevel>): string {
  return members
    .map((member) => `${member.peerId}:${levels[member.peerId] ?? "none"}`)
    .sort()
    .join(",");
}

/**
 * Per-node permissions, and the one surface in the app that tells the truth
 * about them (`useCanEdit` only knows the vault default).
 *
 * Two ideas carry the whole dialog. First, inheritance is a switch, not a state
 * you fall out of by editing: turning it off freezes today's effective list as
 * the starting point, so granting one person access never silently revokes
 * everyone else's. Second, absence is spelled out — a member set to "None" has
 * no entry, and with inheritance off that means no access at all, which is a
 * sentence under the list rather than something you infer from an empty row.
 *
 * Only the creator may write. Everyone else sees the same list, read-only, with
 * the creator named: "ask this person" is more useful than a disabled control.
 */
export function AccessModal() {
  const modal = useWorkspace((s) => s.modal);
  const closeModal = useWorkspace((s) => s.closeModal);
  const client = useWorkspace((s) => s.client);
  const vaultId = useWorkspace((s) => s.vaultId);
  const members = useWorkspace((s) => s.members);
  const toast = useWorkspace((s) => s.toast);

  const open = modal?.kind === "access";

  /**
   * The node id outlives the store's modal slot: `closeModal` clears it while the
   * panel is still animating out, and a body that emptied itself mid-exit would
   * flash. The shadow copy keeps the last opened node until the next open.
   */
  const [nodeId, setNodeId] = useState<NodeId | null>(null);
  useEffect(() => {
    if (modal?.kind === "access" && modal.nodeId !== nodeId) setNodeId(modal.nodeId);
  }, [modal, nodeId]);

  const node = useNode(nodeId);
  const isCreator = useIsCreator(nodeId);
  const creator = useMember(node?.createdBy ?? null);
  const parents = usePath(node?.parentId ?? null);
  const onlineMembers = useOnlineMembers();

  const [inherit, setInherit] = useState(true);
  const [levels, setLevels] = useState<Record<string, RowLevel>>({});
  const [baseline, setBaseline] = useState<{ inherit: boolean; print: string }>({
    inherit: true,
    print: "",
  });
  const [loaded, setLoaded] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const timer = useRef<number | null>(null);

  // Every open re-reads: access is shared state, and a stale list here is the one
  // kind of staleness that silently hands out permissions.
  useEffect(() => {
    if (!open || !client || !vaultId || !nodeId) return;
    let live = true;
    setLoaded(false);
    setSaved(false);
    void client
      .getAccess(vaultId, nodeId)
      .then((access) => {
        if (!live) return;
        const map = levelsOf(access, members);
        setInherit(access.inherit);
        setLevels(map);
        setBaseline({ inherit: access.inherit, print: fingerprint(members, map) });
        setLoaded(true);
      })
      .catch(() => {
        if (!live) return;
        setLoaded(true);
        toast("Couldn't read access for this item", "error");
      });
    return () => {
      live = false;
    };
  }, [open, client, vaultId, nodeId, members, toast]);

  useEffect(
    () => () => {
      if (timer.current !== null) window.clearTimeout(timer.current);
    },
    [],
  );

  const online = new Set(onlineMembers.map((member) => member.peerId));
  // The creator is never a row: their access is not revocable, so a switch there
  // would be a control that cannot do anything.
  const rows = members.filter((member) => member.peerId !== node?.createdBy);
  const parentName = parents.length > 0 ? parents[parents.length - 1].name : "the vault";
  const dirty =
    loaded && (inherit !== baseline.inherit || fingerprint(members, levels) !== baseline.print);

  const save = () => {
    if (!client || !vaultId || !nodeId || !dirty || saving) return;
    const entries: AccessEntry[] = inherit
      ? []
      : rows
          .filter((member) => (levels[member.peerId] ?? "none") !== "none")
          .map((member) => ({
            peerId: member.peerId,
            level: levels[member.peerId] as AccessLevel,
          }));

    setSaving(true);
    void client
      .setAccess({ vaultId, nodeId, inherit, entries })
      .then(() => {
        setSaving(false);
        setSaved(true);
        window.dispatchEvent(new CustomEvent(ACCESS_CHANGED, { detail: { nodeId } }));
        timer.current = window.setTimeout(() => {
          timer.current = null;
          closeModal();
        }, CONFIRM_MS);
      })
      .catch((error: unknown) => {
        setSaving(false);
        toast(error instanceof Error ? error.message : "Couldn't update access", "error");
      });
  };

  const row = (member: Member, control: "read-only" | "editable") => (
    <div
      key={member.peerId}
      data-peer-id={member.peerId}
      className="flex h-[38px] items-center gap-[10px]"
    >
      <Avatar
        peerId={member.peerId}
        name={member.name}
        initials={member.initials}
        size={24}
        online={online.has(member.peerId)}
      />
      <span className="min-w-0 flex-1 truncate text-[13px] leading-[18px] text-fg">
        {member.name}
        {member.isSelf ? <span className="text-fg-3"> (you)</span> : null}
      </span>
      <Chip size="xs" tone={member.role === "admin" ? "violet" : "neutral"}>
        {member.role === "admin" ? "Admin" : "Member"}
      </Chip>
      {control === "editable" ? (
        <Segmented
          value={levels[member.peerId] ?? "none"}
          onChange={(next) => setLevels((cur) => ({ ...cur, [member.peerId]: next }))}
          options={LEVEL_OPTIONS}
        />
      ) : (
        <span className="text-[12.5px] leading-none text-fg-3 capitalize">
          {levels[member.peerId] ?? "none"}
        </span>
      )}
    </div>
  );

  const description = node
    ? `${node.name} · ${formatPath(parents.map((entry) => entry.name))}`
    : undefined;

  return (
    <Modal
      open={open}
      onClose={closeModal}
      title="Access"
      description={saved ? undefined : description}
      size="sm"
    >
      <div data-testid="access-modal" data-node-id={nodeId ?? undefined}>
        {saved ? (
          <ConfirmState title="Access updated" />
        ) : !isCreator ? (
          <>
            <EmptyState
              icon={<Lock size={16} strokeWidth={1.75} aria-hidden />}
              title="Only the creator can change access"
              detail="You can see who has access, but not who gets it."
            />
            {creator ? (
              <div className="mt-[16px] flex items-center justify-center gap-[8px]">
                <Avatar
                  peerId={creator.peerId}
                  name={creator.name}
                  initials={creator.initials}
                  size={20}
                  online={online.has(creator.peerId)}
                />
                <span className="text-[12.5px] leading-none text-fg-2">
                  Created by {creator.name}
                </span>
              </div>
            ) : null}
            <div className="mt-[18px] max-h-[220px] overflow-y-auto scroll-thin">
              {rows.map((member) => row(member, "read-only"))}
            </div>
            <p className="mt-[10px] text-[11px] leading-[16px] text-fg-3">
              {inherit
                ? `Inherited from ${parentName}`
                : "Members not listed cannot open this item"}
            </p>
          </>
        ) : (
          <>
            <div className="flex items-center gap-[10px]">
              <span className="min-w-0 flex-1">
                <span className="block text-[13px] leading-[18px] text-fg">
                  Inherit from parent
                </span>
                <span className="block text-[11px] leading-[16px] text-fg-3">
                  Use the permissions of {parentName}
                </span>
              </span>
              <Toggle
                checked={inherit}
                onChange={setInherit}
                disabled={!loaded}
                label="Inherit access from parent"
              />
            </div>

            <div
              className={`mt-[14px] max-h-[240px] overflow-y-auto scroll-thin transition-opacity duration-[160ms] ease-[cubic-bezier(0.2,0.8,0.2,1)] ${
                inherit ? "pointer-events-none opacity-45" : "opacity-100"
              }`}
              aria-disabled={inherit || undefined}
            >
              {rows.map((member) => row(member, inherit ? "read-only" : "editable"))}
            </div>

            <p className="mt-[10px] text-[11px] leading-[16px] text-fg-3">
              {inherit
                ? `Inherited from ${parentName}`
                : "Members not listed cannot open this item"}
            </p>

            <div className="mt-[20px]">
              <PrimaryButton onClick={save} disabled={!dirty || saving}>
                Save
              </PrimaryButton>
            </div>
          </>
        )}
      </div>
    </Modal>
  );
}
