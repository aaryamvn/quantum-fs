import { Copy } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";

import { Avatar } from "@/components/ui/Avatar";
import { Chip } from "@/components/ui/Chip";
import { ConfirmState } from "@/components/ui/ConfirmState";
import { IconButton } from "@/components/ui/IconButton";
import { PrimaryButton } from "@/components/ui/PrimaryButton";
import { Select } from "@/components/ui/Select";
import { TextField } from "@/components/ui/TextField";
import { useWorkspace } from "@/features/workspace/store";
import { useBackend } from "@/lib/backend";
import { formatDateTime, formatRelative } from "@/lib/time";

import { useSettingsVault } from "./SettingsVaultContext";

/** How long the success state stands before the form comes back. */
const CONFIRM_MS = 900;

const NAME_MAX = 40;
const DESCRIPTION_MAX = 200;

/**
 * What the vault *is*: its name, a line about it, and which server hosts it.
 *
 * Editing is explicit — nothing here saves as you type, because a vault rename
 * fans out to every member's sidebar and a stray keystroke should not travel.
 * The read-only block underneath is the identity the daemon assigned (id,
 * creation, encryption), which no one can edit and everyone occasionally needs
 * to quote, so it is selectable text rather than a field.
 */
export function GeneralTab() {
  const client = useWorkspace((s) => s.client);
  const toast = useWorkspace((s) => s.toast);
  const { vaultId, meta, members, isAdmin, refresh } = useSettingsVault();

  const { servers } = useBackend();
  // This vault's member list, not the store's: the dialog may be configuring a
  // vault the workspace has never opened.
  const creator = members.find((member) => member.peerId === meta?.createdBy);

  const [name, setName] = useState(meta?.name ?? "");
  const [description, setDescription] = useState(meta?.description ?? "");
  const [serverId, setServerId] = useState(meta?.serverId ?? "");
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const timer = useRef<number | null>(null);

  // The store's copy is the truth: whenever it lands (open, save, a peer's
  // rename) the fields re-seed from it. Typing never touches it, so an edit in
  // progress is only ever discarded by a change that really happened.
  useEffect(() => {
    if (!meta) return;
    setName(meta.name);
    setDescription(meta.description);
    setServerId(meta.serverId);
  }, [meta]);

  useEffect(
    () => () => {
      if (timer.current !== null) window.clearTimeout(timer.current);
    },
    [],
  );

  if (!meta) return <p className="text-[13px] text-fg-3">Loading vault settings…</p>;

  const trimmed = name.trim();
  const dirty =
    trimmed !== meta.name || description !== meta.description || serverId !== meta.serverId;
  const valid = trimmed.length > 0;

  const save = async () => {
    if (!client || !vaultId || !dirty || !valid || saving) return;
    setSaving(true);
    try {
      await client.updateVaultMeta(vaultId, { name: trimmed, description, serverId });
      await refresh();
      setSaved(true);
      timer.current = window.setTimeout(() => {
        timer.current = null;
        setSaved(false);
      }, CONFIRM_MS);
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e), "error");
    } finally {
      setSaving(false);
    }
  };

  const copyId = async () => {
    try {
      await navigator.clipboard.writeText(meta.id);
      toast("Vault ID copied", "success");
    } catch {
      toast("Couldn't copy the vault ID", "error");
    }
  };

  if (saved) {
    return (
      <div data-testid="vault-settings-general" className="pt-[40px]">
        <ConfirmState title="Settings saved" detail={trimmed} />
      </div>
    );
  }

  return (
    <div data-testid="vault-settings-general" className="flex flex-col gap-[20px]">
      {/*
        A disabled fieldset, not per-control props: it takes every input, button
        and menu trigger inside it out of the tab order in one place, so a member
        cannot reach a control that would only fail at the daemon.
      */}
      <fieldset
        disabled={!isAdmin}
        className={`m-0 flex flex-col gap-[20px] border-0 p-0 ${isAdmin ? "" : "opacity-60"}`}
      >
        <Field label="Name">
          <TextField
            value={name}
            onChange={setName}
            maxLength={NAME_MAX}
            aria-label="Vault name"
            aria-invalid={!valid}
            onEnter={() => void save()}
          />
          {valid ? null : (
            <p className="mt-[6px] text-[11px] text-coral">A vault needs a name.</p>
          )}
        </Field>

        <Field label="Description">
          <textarea
            rows={3}
            value={description}
            maxLength={DESCRIPTION_MAX}
            aria-label="Vault description"
            placeholder="What lives in this vault"
            onChange={(e) => setDescription(e.target.value)}
            className="w-full resize-none rounded-[10px] border border-line-strong bg-field px-[14px] py-[11px]
              text-[15px] leading-[21px] text-fg outline-none transition-[border-color] duration-[160ms]
              ease-[cubic-bezier(0.2,0.8,0.2,1)] placeholder:text-fg-3 focus:border-white/40"
          />
        </Field>

        <Field label="Server">
          <Select
            value={serverId}
            onChange={setServerId}
            size="md"
            aria-label="Host server"
            options={servers.map((server) => ({
              value: server.id,
              label: server.address,
            }))}
          />
        </Field>
      </fieldset>

      {isAdmin ? null : (
        <p className="text-[11px] leading-[15px] text-fg-3">Only admins can edit vault settings.</p>
      )}

      <div className="flex flex-col">
        <ReadOnlyRow label="Vault ID">
          <span
            data-selectable
            className="truncate font-mono text-[12.5px] text-fg-2"
            title={meta.id}
          >
            {meta.id}
          </span>
          <IconButton
            icon={<Copy size={16} strokeWidth={1.75} aria-hidden />}
            label="Copy vault ID"
            size={24}
            onClick={() => void copyId()}
          />
        </ReadOnlyRow>

        <ReadOnlyRow label="Created">
          <Avatar
            peerId={meta.createdBy}
            name={creator?.name ?? "Unknown member"}
            initials={creator?.initials}
            size={20}
          />
          <span className="truncate text-[12.5px] text-fg-2">
            {creator?.name ?? "Unknown member"}
          </span>
          <span className="shrink-0 text-[12.5px] text-fg-3 tabular-nums">
            {formatDateTime(meta.createdAt)}
          </span>
        </ReadOnlyRow>

        <ReadOnlyRow label="Encryption">
          <Chip tone="violet">Post-quantum hybrid</Chip>
          <span className="truncate text-[11px] text-fg-3">
            Keys rotate weekly · last rotated {formatRelative(meta.keyRotatedAt)}
          </span>
        </ReadOnlyRow>
      </div>

      <div className="w-[184px]">
        <PrimaryButton disabled={!isAdmin || !dirty || !valid || saving} onClick={() => void save()}>
          Save changes
        </PrimaryButton>
      </div>
    </div>
  );
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  // A div, not a <label>: one of these wraps a Select, whose trigger is a
  // button — a real label would fire it on every click of its own text.
  return (
    <div>
      <span className="mb-[8px] block text-[12.5px] leading-[16px] text-fg-3">{label}</span>
      {children}
    </div>
  );
}

function ReadOnlyRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex h-[44px] items-center gap-[10px] border-b border-line last:border-0">
      <span className="w-[104px] shrink-0 text-[12.5px] text-fg-3">{label}</span>
      <div className="flex min-w-0 flex-1 items-center gap-[8px]">{children}</div>
    </div>
  );
}
