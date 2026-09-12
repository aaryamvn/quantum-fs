import { Link, Trash2 } from "lucide-react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { useEffect, useMemo, useRef, useState } from "react";

import { Avatar } from "@/components/ui/Avatar";
import { Caption } from "@/components/ui/Caption";
import { GhostButton } from "@/components/ui/GhostButton";
import { IconButton } from "@/components/ui/IconButton";
import { Segmented } from "@/components/ui/Segmented";
import { EASE } from "@/features/workspace/layout";
import { useWorkspace } from "@/features/workspace/store";
import type { Member, MemberRole } from "@/lib/backend";
import { formatRelative } from "@/lib/time";

import { useSettingsVault } from "./SettingsVaultContext";

/** An armed remove disarms itself, so a dialog left open is never one click from a removal. */
const CONFIRM_MS = 4000;

/** The name the daemon gives the host's own member record. */
const SERVER_NAME = "Vault server";

const ROLE_OPTIONS: { value: MemberRole; label: string }[] = [
  { value: "admin", label: "Admin" },
  { value: "member", label: "Member" },
];

export interface MembersTabProps {
  /** Jumps the dialog to the Sharing pane, where the join code lives. */
  onInvite(): void;
}

/**
 * Who is in the vault, what they may do, and whether they are here right now.
 *
 * Ordering is admins, then whoever is online, then alphabetical — the two
 * questions actually asked of this list are "who can change things" and "who is
 * around", so both answers sit at the top instead of being hunted for.
 *
 * There is no "add member" button because membership is not granted from here:
 * a vault is joined with a code (docs/decisions/net-vault-join-directory.md),
 * so Invite hands you to Sharing rather than pretending to mint an account.
 */
export function MembersTab({ onInvite }: MembersTabProps) {
  const { members, loading, isOpenVault, isAdmin } = useSettingsVault();
  const presence = useWorkspace((s) => s.presence);
  const reduced = useReducedMotion() ?? false;

  const rows = useMemo(() => {
    // Presence only describes the vault that is open; for any other vault the
    // member record's own flag is the most recent thing anyone knows.
    const live = (member: Member) =>
      (isOpenVault ? presence.find((peer) => peer.peerId === member.peerId)?.online : undefined) ??
      member.online;
    return [...members]
      .map((member) => ({ member, online: live(member) }))
      .sort((a, b) => {
        const adminDelta = Number(b.member.role === "admin") - Number(a.member.role === "admin");
        if (adminDelta !== 0) return adminDelta;
        const onlineDelta = Number(b.online) - Number(a.online);
        if (onlineDelta !== 0) return onlineDelta;
        return a.member.name.localeCompare(b.member.name, undefined, { sensitivity: "base" });
      });
  }, [members, presence, isOpenVault]);

  const onlineCount = rows.filter((row) => row.online).length;

  return (
    <div data-testid="vault-settings-members" className="flex flex-col">
      <Caption
        className="h-[36px]"
        action={
          <GhostButton
            variant="secondary"
            icon={<Link size={14} strokeWidth={1.75} aria-hidden />}
            onClick={onInvite}
          >
            Invite
          </GhostButton>
        }
      >
        <span className="tabular-nums">
          {members.length} {members.length === 1 ? "member" : "members"} · {onlineCount} online
        </span>
      </Caption>

      <div className="mt-[8px] flex flex-col">
        {rows.length === 0 && loading
          ? /* Three bars at the real 52px row height: a member list that arrives
               into an empty pane makes the dialog jump, and an empty pane on a
               vault that certainly has at least you in it reads as a failure. */
            [0, 1, 2].map((i) => (
              <motion.div
                key={i}
                data-testid="members-skeleton"
                aria-hidden
                className="flex h-[52px] items-center gap-[12px] border-b border-line last:border-0"
                initial={{ opacity: 0.5 }}
                animate={reduced ? { opacity: 0.5 } : { opacity: [0.5, 1, 0.5] }}
                transition={
                  reduced
                    ? { duration: 0 }
                    : { duration: 1.2, repeat: Infinity, ease: "easeInOut", delay: i * 0.1 }
                }
              >
                <span className="h-[32px] w-[32px] shrink-0 rounded-full bg-white/[0.05]" />
                <span className="flex min-w-0 flex-1 flex-col gap-[6px]">
                  <span className="h-[10px] w-[124px] rounded-full bg-white/[0.05]" />
                  <span className="h-[9px] w-[78px] rounded-full bg-white/[0.035]" />
                </span>
              </motion.div>
            ))
          : rows.map(({ member, online }) => (
              <MemberRow key={member.peerId} member={member} online={online} canManage={isAdmin} />
            ))}
      </div>
    </div>
  );
}

function MemberRow({
  member,
  online,
  canManage,
}: {
  member: Member;
  online: boolean;
  canManage: boolean;
}) {
  const reduced = useReducedMotion() ?? false;
  const client = useWorkspace((s) => s.client);
  const toast = useWorkspace((s) => s.toast);
  const { vaultId, refresh } = useSettingsVault();

  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const timer = useRef<number | null>(null);

  useEffect(() => {
    if (!confirming) return;
    timer.current = window.setTimeout(() => {
      timer.current = null;
      setConfirming(false);
    }, CONFIRM_MS);
    return () => {
      if (timer.current !== null) window.clearTimeout(timer.current);
      timer.current = null;
    };
  }, [confirming]);

  const setRole = async (role: MemberRole) => {
    if (!client || !vaultId || role === member.role || busy) return;
    setBusy(true);
    try {
      await client.setMemberRole(vaultId, member.peerId, role);
      await refresh();
    } catch (e) {
      // The daemon owns the rule (a vault keeps at least one admin); the row
      // simply reports whatever it says rather than second-guessing it here.
      toast(e instanceof Error ? e.message : String(e), "error");
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    if (!client || !vaultId || busy) return;
    setBusy(true);
    try {
      await client.removeMember(vaultId, member.peerId);
      await refresh();
      toast(`Removed ${member.name}`, "success");
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e), "error");
    } finally {
      setBusy(false);
      setConfirming(false);
    }
  };

  const fade = reduced
    ? { initial: { opacity: 0 }, animate: { opacity: 1 }, exit: { opacity: 0 }, transition: { duration: 0 } }
    : {
        initial: { opacity: 0 },
        animate: { opacity: 1 },
        exit: { opacity: 0 },
        transition: { duration: 0.14, ease: EASE },
      };

  const presence = online ? "Online" : `Offline · last seen ${formatRelative(member.lastSeenAt)}`;
  // The host's own record is a machine, not a person, and the list is the one
  // place that reads as a roster of people — so it says which row it is.
  const second = member.name === SERVER_NAME ? `Server · ${presence}` : presence;

  /*
    Admins are the two rows nothing here can act on: the vault server holds the
    data and the creator is the vault's root of trust, and the daemon refuses
    both a demotion and a removal for them. A control that is offered and then
    answers with an error is worse than one that was never offered, so the row
    stays legible and inert rather than pretending.
  */
  const locked = member.role === "admin";
  const actionable = canManage && !locked;

  return (
    <div
      data-peer-id={member.peerId}
      className="flex h-[52px] items-center gap-[12px] border-b border-line last:border-0"
    >
      <AnimatePresence mode="wait" initial={false}>
        {confirming ? (
          <motion.div
            key="confirm"
            {...fade}
            className="flex w-full items-center justify-between gap-[12px]"
          >
            <span className="min-w-0 flex-1 text-[13px] leading-[17px] text-fg">
              Remove {member.name}?{" "}
              <span className="text-fg-3">Removing a member also rotates the join code.</span>
            </span>
            <span className="flex shrink-0 items-center gap-[4px]">
              <GhostButton variant="danger" disabled={busy} onClick={() => void remove()}>
                Remove
              </GhostButton>
              <GhostButton variant="secondary" onClick={() => setConfirming(false)}>
                Cancel
              </GhostButton>
            </span>
          </motion.div>
        ) : (
          <motion.div key="row" {...fade} className="flex w-full items-center gap-[12px]">
            <Avatar
              peerId={member.peerId}
              name={member.name}
              initials={member.initials}
              size={32}
              online={online}
              dim={!online}
            />

            <span className="flex min-w-0 flex-1 flex-col gap-[2px]">
              <span className="truncate text-[14px] leading-none text-fg">
                {member.name}
                {member.isSelf ? <span className="text-fg-3"> (you)</span> : null}
              </span>
              <span className="truncate text-[11.5px] leading-none text-fg-3">{second}</span>
            </span>

            {/*
              A disabled fieldset rather than a per-control flag: Segmented has no
              disabled prop, and this takes its radios out of the tab order too.
            */}
            <fieldset
              disabled={!actionable || busy}
              aria-label={`Role for ${member.name}`}
              className={`m-0 shrink-0 border-0 p-0 ${actionable ? "" : "opacity-50"}`}
            >
              <Segmented
                value={member.role}
                onChange={(role) => void setRole(role)}
                options={ROLE_OPTIONS}
              />
            </fieldset>

            {/* No tooltip: a hover label on a destructive control is noise in front
                of a button that already says what it does once it is pressed.
                An admin gets the slot but not the button — a greyed-out bin still
                reads as an offer, and the space has to stay so every row's
                role control lands on the same edge. */}
            {locked ? (
              <span aria-hidden className="block h-[28px] w-[28px] shrink-0" />
            ) : (
              <IconButton
                icon={<Trash2 size={16} strokeWidth={1.75} aria-hidden />}
                label={`Remove ${member.name}`}
                tooltip={false}
                tone="danger"
                size={28}
                disabled={!canManage || member.isSelf || busy}
                onClick={() => setConfirming(true)}
              />
            )}
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
