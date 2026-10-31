// Copy a whole environment's Auto Run data into another ("Copy Auto Run
// data…" on the environment's row). The sources are the other environments
// and the removed ones still kept for 30 days. Copying into an environment
// that already holds data replaces it - after asking - and what it held is
// kept for 30 days and offered here as a source.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useId, useState } from "react";
import { commands, type EnvView } from "../bindings";
import { useCloseOnEnvironmentChange } from "../hooks/useCloseOnEnvironmentChange";
import { IconCancel, IconCopy } from "../lib/actionIcons";
import { copyRefusalText, isNeedsConfirm } from "../lib/copyRefusal";
import { forgetEnvironmentData } from "../lib/environments";
import { unwrapStr } from "../lib/ipc";
import { toast } from "../lib/toast";
import { Button } from "./ui/button";
import { Modal } from "./ui/modal";
import { Select } from "./ui/select";

function sourceLabel(s: { name: string; removed: boolean; days_left: number | null }): string {
  if (!s.removed) return s.name;
  const days = s.days_left ?? 0;
  return `${s.name} (removed, ${days} ${days === 1 ? "day" : "days"} left)`;
}

export default function CopyAutoRunDialog({ target, onClose }: { target: EnvView; onClose: () => void }) {
  // The question below names the environments as they were opened: it never
  // outlives a switch.
  useCloseOnEnvironmentChange(onClose);
  const choices = useQuery({
    queryKey: ["autorun-copy-choices", target.id],
    queryFn: () => unwrapStr(commands.envCopyChoices(target.id)),
    retry: false,
    gcTime: 0,
  });
  const qc = useQueryClient();
  const titleId = useId();
  const [picked, setPicked] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const sources = choices.data?.sources ?? [];
  const source = sources.find((s) => s.key === (picked ?? sources[0]?.key));
  const replaces = choices.data ? !choices.data.target_empty : false;

  const copy = async () => {
    if (!source) return;
    setBusy(true);
    try {
      const r = await commands.envCopyAutorun(target.id, source.key, replaces);
      if (r.status === "error") {
        // The target gained data since this opened: ask, don't repeat.
        if (isNeedsConfirm(r.error)) await choices.refetch();
        else toast.error(copyRefusalText(r.error));
        return;
      }
      // Into the environment on screen, every screen reads it again.
      await forgetEnvironmentData(qc);
      toast.success(`Copied Auto Run data into ${target.name}`);
      onClose();
    } catch (e) {
      toast.error(copyRefusalText(e instanceof Error ? e.message : String(e)));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      onClose={onClose}
      labelledBy={titleId}
      className="flex w-full max-w-md flex-col gap-3 p-5"
    >
      <h2 id={titleId} className="text-sm font-semibold text-text">
        Copy Auto Run data into {target.name}
      </h2>
      {choices.isPending && <p className="text-xs text-muted">Looking for somewhere to copy from…</p>}
      {choices.isError && (
        <p role="alert" className="text-xs text-danger">
          {choices.error.message}
        </p>
      )}
      {choices.data && sources.length === 0 && (
        <p className="text-xs text-muted">There is no other environment to copy from yet.</p>
      )}
      {choices.data && source && (
        <>
          <label className="block space-y-1">
            <span className="text-xs font-medium text-muted">Copy from</span>
            <Select
              aria-label="Copy from"
              className="w-full"
              value={source.key}
              onChange={(e) => setPicked(e.target.value)}
            >
              {sources.map((s) => (
                <option key={s.key} value={s.key}>
                  {sourceLabel(s)}
                </option>
              ))}
            </Select>
          </label>
          <p className="text-xs text-muted">
            Scripts, areas, components, quirks, API templates and the sign-in recording come across.
          </p>
          <div aria-live="polite">
            {replaces && (
              <p className="text-xs text-text">
                {`This replaces ${target.name}'s scripts, areas, components, quirks and API templates with ${source.name}'s. ${target.name}'s current ones are kept for 30 days and can be copied back from this list.`}
              </p>
            )}
          </div>
        </>
      )}
      <div className="flex justify-end gap-2">
        <Button size="sm" variant="ghost" onClick={onClose}>
          <IconCancel aria-hidden />
          Cancel
        </Button>
        {choices.data && source && (
          <Button size="sm" disabled={busy} onClick={copy}>
            <IconCopy aria-hidden />
            {replaces ? "Replace" : "Copy"}
          </Button>
        )}
      </div>
    </Modal>
  );
}
