import { Lock } from "lucide-react";
import { useEffect, useState } from "react";

import { Avatar } from "@/components/ui/Avatar";
import { GhostButton } from "@/components/ui/GhostButton";
import { Tooltip } from "@/components/ui/Tooltip";
import type { FsNode, Member, NodeAccess } from "@/lib/backend";

import { useIsCreator, useWorkspace } from "../store";

/** Faces past this collapse into a "+n"; a longer row stops being readable at 18px. */
const MAX_FACES = 5;

/** Broadcast by the access modal after a successful write, so open summaries re-read. */
const ACCESS_CHANGED = "qfs:access-changed";

export interface AccessSummaryProps {
  node: FsNode;
}

function plural(n: number, one: string, many: string): string {
  return `${n} ${n === 1 ? one : many}`;
}

/**
 * Who can touch this, in one line.
 *
 * The full list is a modal; this is the glance that tells you whether opening it
 * is worth it. Faces before words, because recognizing two avatars is faster
 * than parsing "2 editors", and the counts are there for the case where the
 * faces overflow.
 *
 * `getAccess` is asynchronous and the answer can change from under us — another
 * member edits the list, or an ancestor's list changes and this node inherits
 * the difference — so the read is re-run on the node's own modification stamp
 * and on a window event the access modal fires. The Manage button is gated on
 * creator-ness rather than hidden: a viewer who cannot change access should
 * still learn that the control exists and why it is closed to them.
 */
export function AccessSummary({ node }: AccessSummaryProps) {
  const client = useWorkspace((s) => s.client);
  const members = useWorkspace((s) => s.members);
  const nodes = useWorkspace((s) => s.nodes);
  const vaultName = useWorkspace((s) => s.vaultName);
  const openModal = useWorkspace((s) => s.openModal);
  const isCreator = useIsCreator(node.id);

  const [access, setAccess] = useState<NodeAccess | null>(null);
  /** Null until the first reply lands: "no explicit list" and "no answer yet" are not the same fact. */
  const [pending, setPending] = useState(true);
  const [tick, setTick] = useState(0);

  useEffect(() => {
    const onChanged = () => setTick((n) => n + 1);
    window.addEventListener(ACCESS_CHANGED, onChanged);
    return () => window.removeEventListener(ACCESS_CHANGED, onChanged);
  }, []);

  useEffect(() => {
    if (!client) {
      // No client is no read, so there is no reply to wait for: leaving `pending`
      // set here would hold the line on "Checking access…" for as long as the
      // panel is open. Cleared with it, so nothing from a previous node stands.
      setAccess(null);
      setPending(false);
      return;
    }
    let live = true;
    setPending(true);
    void client.getAccess(node.vaultId, node.id).then(
      (value) => {
        if (!live) return;
        setAccess(value);
        setPending(false);
      },
      () => {
        if (!live) return;
        setAccess(null);
        setPending(false);
      },
    );
    return () => {
      live = false;
    };
  }, [client, node.vaultId, node.id, node.modifiedAt, tick]);

  const entries = access?.entries ?? [];
  const explicit = entries.length > 0;

  // No explicit list anywhere up the chain means the vault default: everyone
  // edits — but only once the daemon has actually said so. Claiming it while the
  // read is out would be the panel guessing the most permissive answer there is.
  const faces: Member[] = pending
    ? []
    : explicit
      ? entries
          .map((entry) => members.find((member) => member.peerId === entry.peerId))
          .filter((member): member is Member => member !== undefined)
      : members;

  const editors = entries.filter((entry) => entry.level === "editor").length;
  const viewers = entries.length - editors;
  const summary = pending
    ? "Checking access…"
    : explicit
      ? `${plural(editors, "editor", "editors")} · ${plural(viewers, "viewer", "viewers")}`
      : "Everyone can edit";

  const inheritedFrom =
    !pending && access?.inherit && explicit
      ? (node.parentId ? nodes[node.parentId]?.name : undefined) ?? vaultName
      : null;

  const manage = (
    <GhostButton
      variant="secondary"
      icon={<Lock size={14} strokeWidth={1.75} />}
      disabled={!isCreator}
      onClick={() => openModal({ kind: "access", nodeId: node.id })}
    >
      Manage
    </GhostButton>
  );

  return (
    <div data-testid="inspector-access" data-node-id={node.id}>
      <div className="flex items-center gap-[8px]">
        <div className="flex shrink-0 items-center">
          {faces.slice(0, MAX_FACES).map((member, index) => (
            <Avatar
              key={member.peerId}
              peerId={member.peerId}
              name={member.name}
              initials={member.initials}
              size={18}
              title={member.name}
              className={index === 0 ? "" : "-ml-[6px]"}
            />
          ))}
          {faces.length > MAX_FACES ? (
            // Positioned and lifted: the faces are `relative`, so an unpositioned
            // chip would slide under the last avatar instead of over it.
            <span className="relative z-[1] -ml-[6px] grid h-[18px] w-[18px] place-items-center rounded-full border border-line-strong bg-surface-2 text-[9px] text-fg-3">
              +{faces.length - MAX_FACES}
            </span>
          ) : null}
        </div>

        <div className="min-w-0 flex-1">
          {/* Wraps rather than truncates: "2 editors · 2 v…" is the count the line exists to give. */}
          <p className="text-[12.5px] leading-[16px] break-words text-fg-2">{summary}</p>
          {inheritedFrom ? (
            <p className="text-[11px] leading-[15px] break-words text-fg-3">
              Inherited from {inheritedFrom}
            </p>
          ) : null}
        </div>

        {isCreator ? (
          manage
        ) : (
          <Tooltip label="Only the creator can change access">
            <span className="inline-flex shrink-0">{manage}</span>
          </Tooltip>
        )}
      </div>
    </div>
  );
}
