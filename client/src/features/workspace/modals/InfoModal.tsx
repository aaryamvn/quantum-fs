import { useEffect, useState } from "react";
import type { ReactNode } from "react";

import { FileIcon, FolderIcon, iconSpecForName } from "@/components/icons";
import { Avatar } from "@/components/ui/Avatar";
import { Caption } from "@/components/ui/Caption";
import { Chip } from "@/components/ui/Chip";
import { Divider } from "@/components/ui/Divider";
import { GhostButton } from "@/components/ui/GhostButton";
import { Modal } from "@/components/ui/Modal";
import type { FsNode, Member, NodeAccess, NodeId } from "@/lib/backend";
import { formatBytes } from "@/lib/format";
import { formatPath } from "@/lib/path";
import { formatDateTime, formatRelative } from "@/lib/time";

import { useIsCreator, useMember, useNode, usePath, useWorkspace } from "../store";

/** Beyond this the access list stops naming people and starts counting them. */
const MAX_FACES = 6;

/**
 * "PY file", "Folder" — the noun a person would use, not the mime type.
 *
 * The registry already knows the monogram it draws on the icon ("PY", "TSX"),
 * so the label and the icon can never disagree; only when it draws none does
 * this fall back to the raw extension.
 */
function kindLabel(node: FsNode): string {
  if (node.kind === "folder") return "Folder";
  const label = iconSpecForName(node.name).label;
  return label ? `${label} file` : "File";
}

/** The line under the title: kind, then the one or two facts that size it up. */
function subtitle(node: FsNode): string {
  const parts = [kindLabel(node)];
  if (node.kind === "folder") {
    parts.push(`${node.childCount} ${node.childCount === 1 ? "item" : "items"}`);
  }
  parts.push(formatBytes(node.sizeBytes));
  return parts.join(" · ");
}

/** One row of the definition list. The label column is fixed so values align down the panel. */
function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <>
      <dt className="text-[12.5px] leading-[18px] text-fg-3">{label}</dt>
      <dd className="min-w-0 text-[12.5px] leading-[18px] break-words text-fg-2" data-selectable>
        {children}
      </dd>
    </>
  );
}

/** Avatar + name + when, the shape both "Created by" and "Last edited by" take. */
function Person({ member, at }: { member: Member | undefined; at: number }) {
  return (
    <span className="flex min-w-0 flex-1 items-center gap-[8px]">
      <Avatar
        peerId={member?.peerId ?? "unknown"}
        name={member?.name ?? "Unknown"}
        initials={member?.initials}
        size={24}
      />
      <span className="min-w-0 truncate text-[13px] leading-[18px] text-fg">
        {member?.name ?? "Unknown"}
      </span>
      <span className="ml-auto shrink-0 text-[11px] leading-[18px] text-fg-3">
        {formatRelative(at)}
      </span>
    </span>
  );
}

/**
 * Everything the vault knows about one node, on one card.
 *
 * Finder's Get Info, with the two facts a replicated file system adds: who made
 * it and who touched it last (identity is the point of a shared vault), and
 * whether the bytes are actually on this machine — a remote file looks
 * identical in the grid, and this is the surface that admits it and says how it
 * is fetched: by double-clicking it, the only way. Access is read, never edited,
 * here: the list answers "can they see
 * this" at a glance, and changing it is a different, heavier decision that the
 * Access modal owns.
 *
 * The node id is shadowed in state so the panel keeps drawing the node it was
 * opened on for the 180ms it spends animating out — the store's `modal` goes
 * null on the first frame of the exit.
 */
export function InfoModal() {
  const modal = useWorkspace((s) => s.modal);
  const closeModal = useWorkspace((s) => s.closeModal);
  const openModal = useWorkspace((s) => s.openModal);
  const client = useWorkspace((s) => s.client);
  const vaultId = useWorkspace((s) => s.vaultId);
  const vaultName = useWorkspace((s) => s.vaultName);
  const members = useWorkspace((s) => s.members);

  const open = modal !== null && modal.kind === "info";

  // Reset during render rather than in an effect, so the panel never paints one
  // frame of the previously inspected node when Get Info is used twice in a row.
  const [shownId, setShownId] = useState<NodeId | null>(null);
  if (modal !== null && modal.kind === "info" && modal.nodeId !== shownId) {
    setShownId(modal.nodeId);
  }

  const node = useNode(shownId);
  const creator = useMember(node?.createdBy);
  const editor = useMember(node?.modifiedBy);
  const isCreator = useIsCreator(shownId);
  const parents = usePath(node?.parentId ?? null);

  const [access, setAccess] = useState<NodeAccess | null>(null);

  useEffect(() => {
    if (!open || !client || vaultId === null || shownId === null) return;
    let alive = true;
    setAccess(null);
    void client
      .getAccess(vaultId, shownId)
      .then((next) => {
        if (alive) setAccess(next);
      })
      .catch(() => {
        // A failed read shows as "not loaded yet"; the dialog is a reader and
        // must never turn a permissions hiccup into an error dialog.
      });
    return () => {
      alive = false;
    };
  }, [open, client, vaultId, shownId]);

  const icon =
    node === undefined ? null : node.kind === "folder" ? (
      <FolderIcon color={node.color ?? "graphite"} size={20} />
    ) : (
      <FileIcon name={node.name} size={20} />
    );

  const title =
    node === undefined ? undefined : (
      <span className="inline-flex max-w-full items-center gap-[8px] align-middle">
        <span className="grid shrink-0 place-items-center" style={{ width: 20, height: 20 }}>
          {icon}
        </span>
        <span className="min-w-0 truncate">{node.name}</span>
      </span>
    );

  const where = parents.length > 0 ? formatPath(parents.map((n) => n.name)) : "—";

  // "Everyone can edit" is the vault default (NodeAccess.inherit with no
  // entries anywhere up the chain), which is the common case and deserves a
  // sentence rather than a list of every member.
  const everyone = access !== null && access.inherit && access.entries.length === 0;
  const entries = access?.entries ?? [];
  const shownEntries = entries.slice(0, MAX_FACES);
  const overflow = entries.length - shownEntries.length;

  const availability =
    node === undefined
      ? ""
      : node.availability === "local"
        ? "On this device"
        : node.availability === "downloading"
          ? `Downloading ${Math.round((node.progress ?? 0) * 100)}%`
          : "Not on this device";

  return (
    <Modal
      open={open}
      onClose={closeModal}
      title={title}
      description={node === undefined ? undefined : subtitle(node)}
      size="sm"
    >
      <div data-testid="info-modal" data-node-id={shownId ?? undefined}>
        {node === undefined ? null : (
          <>
            <div className="scroll-thin max-h-[46vh] overflow-y-auto">
              <dl className="grid grid-cols-[84px_1fr] items-baseline gap-x-[12px] gap-y-[9px]">
                <Row label="Kind">{kindLabel(node)}</Row>
                <Row label="Size">{formatBytes(node.sizeBytes)}</Row>
                {node.kind === "folder" ? (
                  <Row label="Items">
                    {node.childCount} {node.childCount === 1 ? "item" : "items"}
                  </Row>
                ) : null}
                <Row label="Where">{where}</Row>
                <Row label="Vault">{vaultName || "—"}</Row>
                <Row label="Created">{formatDateTime(node.createdAt)}</Row>
                <Row label="Modified">{formatDateTime(node.modifiedAt)}</Row>
                <Row label="Availability">
                  <span className="flex flex-wrap items-center gap-x-[8px] gap-y-[4px]">
                    <span>{availability}</span>
                    {node.kind === "file" ? (
                      <span className="text-fg-3">
                        {node.holders.length}{" "}
                        {node.holders.length === 1 ? "holder" : "holders"}
                      </span>
                    ) : null}
                    {node.availability === "remote" && node.kind === "file" ? (
                      <span className="text-fg-3">Double-click to download</span>
                    ) : null}
                  </span>
                </Row>
              </dl>

              <Divider className="my-[16px]" />

              <Caption className="mb-[10px]">People</Caption>
              <div className="grid gap-[10px]">
                <div className="flex items-center gap-[10px]">
                  <span className="w-[74px] shrink-0 text-[12.5px] leading-[18px] text-fg-3">
                    Created by
                  </span>
                  <Person member={creator} at={node.createdAt} />
                </div>
                <div className="flex items-center gap-[10px]">
                  <span className="w-[74px] shrink-0 text-[12.5px] leading-[18px] text-fg-3">
                    Last edited
                  </span>
                  <Person member={editor} at={node.modifiedAt} />
                </div>
              </div>

              <Divider className="my-[16px]" />

              <Caption className="mb-[10px]">Access</Caption>
              {access === null ? (
                <p className="text-[12.5px] leading-[18px] text-fg-3">Checking…</p>
              ) : everyone ? (
                <p className="text-[12.5px] leading-[18px] text-fg-2">Everyone can edit</p>
              ) : (
                <div className="grid gap-[8px]">
                  {shownEntries.map((entry) => {
                    const member = members.find((m) => m.peerId === entry.peerId);
                    return (
                      <div key={entry.peerId} className="flex items-center gap-[8px]">
                        <Avatar
                          peerId={entry.peerId}
                          name={member?.name ?? "Unknown"}
                          initials={member?.initials}
                          size={18}
                        />
                        <span className="min-w-0 truncate text-[12.5px] leading-[18px] text-fg-2">
                          {member?.name ?? "Unknown"}
                        </span>
                        <Chip
                          className="ml-auto"
                          tone={entry.level === "editor" ? "violet" : "neutral"}
                        >
                          {entry.level === "editor" ? "Editor" : "Viewer"}
                        </Chip>
                      </div>
                    );
                  })}
                  {overflow > 0 ? (
                    <p className="text-[11px] leading-none text-fg-3">+{overflow} more</p>
                  ) : null}
                  {entries.length === 0 ? (
                    <p className="text-[12.5px] leading-[18px] text-fg-3">No one yet</p>
                  ) : null}
                </div>
              )}
            </div>

            <div className="mt-[18px] flex items-center gap-[6px]">
              <GhostButton
                variant="secondary"
                onClick={() => {
                  if (shownId !== null) openModal({ kind: "history", nodeId: shownId });
                }}
              >
                History
              </GhostButton>
              {isCreator ? (
                <GhostButton
                  variant="secondary"
                  onClick={() => {
                    if (shownId !== null) openModal({ kind: "access", nodeId: shownId });
                  }}
                >
                  Manage access
                </GhostButton>
              ) : null}
            </div>
          </>
        )}
      </div>
    </Modal>
  );
}

export default InfoModal;
