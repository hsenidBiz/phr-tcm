// Authoring the actions for one case, as JSON.
//
// JSON on purpose, for now: an assistant will generate these scripts
// later, and the format has to be proven by hand before anything
// generates it. The case's own steps sit alongside as the reference.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { commands, type StepScript } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Select } from "../../components/ui/select";
import { Textarea } from "../../components/ui/input";
import { Modal } from "../../components/ui/modal";
import { unwrapStr } from "../../lib/ipc";
import { IconCancel, IconConfirm } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { floorOf } from "./floor";

const PLACEHOLDER = `[
  {
    "step_number": 1,
    "actions": [
      { "kind": "navigate", "url": "https://app.example/leave" },
      { "kind": "click", "selector": { "role": "button", "name": "New request" } },
      { "kind": "fill", "selector": { "role": "textbox", "name": "Reason" }, "value": "Family event" },
      { "kind": "expect_visible", "selector": { "role": "heading", "name": "Leave request" } }
    ]
  }
]`;

export default function ScriptEditor({
  caseId,
  title,
  steps,
  onClose,
}: {
  caseId: number;
  title: string;
  steps: { action: string; expected: string }[];
  onClose: () => void;
}) {
  const queryClient = useQueryClient();

  const existing = useQuery({
    queryKey: ["autorun-script", caseId],
    queryFn: () => unwrapStr(commands.autoRunLoadScript(caseId)),
    retry: false,
  });

  const accounts = useQuery({
    queryKey: ["autorun-accounts"],
    queryFn: () => unwrapStr(commands.autoRunListAccounts()),
    retry: false,
  });
  // null = untouched, so the saved script's account shows until the person picks.
  const [picked, setPicked] = useState<string | null>(null);
  const account = picked ?? existing.data?.account ?? "";
  const known = (accounts.data ?? []).some((a) => a.key === account);

  const [text, setText] = useState<string | null>(null);
  const [problem, setProblem] = useState("");
  const value =
    text ?? (existing.data ? JSON.stringify(existing.data.steps, null, 2) : "");

  // The "Checks" line reads the box as it stands right now, not the last
  // saved script - so a person sees a step go from NOT CHECKED to checked
  // as they type, before they ever press Save. Invalid JSON reads as no
  // script at all rather than throwing mid-render.
  let scriptForChecks: StepScript[] = [];
  try {
    const parsed: unknown = JSON.parse(value || "[]");
    if (Array.isArray(parsed)) scriptForChecks = parsed as StepScript[];
  } catch {
    // Left as [] - every step shows NOT CHECKED until the JSON is valid again.
  }
  const checks = floorOf(steps, scriptForChecks);

  // A save while the existing script hasn't resolved yet (still loading, or
  // failed to load) would write an empty `steps: []` over whatever is
  // already there - silent data loss. Block it rather than let an empty
  // textarea pass for "this case genuinely has no script".
  const blockedReason = existing.isLoading
    ? "Still loading the existing script - wait for it before saving."
    : existing.isError
      ? `Could not load the existing script, so saving is blocked: ${existing.error.message}`
      : "";

  const save = async () => {
    if (blockedReason) {
      setProblem(blockedReason);
      return;
    }
    let parsed: StepScript[];
    try {
      parsed = JSON.parse(value || "[]") as StepScript[];
    } catch (e) {
      setProblem(`That is not valid JSON: ${(e as Error).message}`);
      return;
    }
    if (!Array.isArray(parsed)) {
      setProblem("That is not valid JSON: the script must be an array of steps.");
      return;
    }
    setProblem("");
    const r = await commands.autoRunSaveScript({
      case_id: caseId,
      title,
      steps: parsed,
      account: account === "" ? null : account,
    });
    if (r.status === "error") {
      toast.error(`Could not save the script: ${r.error}`);
      return;
    }
    toast.success("Script saved.");
    await queryClient.invalidateQueries({ queryKey: ["autorun-script", caseId] });
    onClose();
  };

  return (
    <Modal onClose={onClose} className="w-full max-w-3xl space-y-3 p-4">
      <h2 className="text-sm font-semibold text-text">
        <span className="id-mono text-faint">#{caseId}</span> {title}
      </h2>

      <div className="grid gap-3 lg:grid-cols-2">
        <div className="space-y-1">
          <span className="text-xs font-medium text-muted">The case's steps</span>
          <ol className="max-h-64 space-y-1 overflow-y-auto text-xs text-muted">
            {steps.map((s, i) => (
              <li key={i} className="rounded border border-border/60 px-2 py-1">
                <span className="text-text">
                  {i + 1}. {s.action}
                </span>
                {s.expected && <div className="text-faint">→ {s.expected}</div>}
              </li>
            ))}
          </ol>
        </div>

        <div className="space-y-2">
          <label className="block text-xs text-muted">
            Runs as
            <Select
              aria-label="Runs as"
              className="mt-1 w-full"
              value={account}
              onChange={(e) => setPicked(e.target.value)}
            >
              <option value="">No sign-in</option>
              {(accounts.data ?? []).map((a) => (
                <option key={a.key} value={a.key}>
                  {`${a.label.trim() || a.key} (${a.key})`}
                </option>
              ))}
              {account !== "" && !known && (
                <option value={account}>{`${account} (not on this machine)`}</option>
              )}
            </Select>
          </label>

          <label className="block text-xs text-muted">
            Action script JSON
            <Textarea
              aria-label="Action script JSON"
              className="mt-1 h-64 w-full font-mono text-xs"
              placeholder={PLACEHOLDER}
              value={value}
              onChange={(e) => setText(e.target.value)}
            />
          </label>

          {checks.length > 0 && (
            <div className="space-y-1">
              <span className="text-xs font-medium text-muted">Checks</span>
              <ul className="space-y-0.5 text-xs">
                {checks.map(({ step_number, state }) => (
                  <li
                    key={step_number}
                    className={cn(state.kind === "unchecked" ? "text-warning" : "text-muted")}
                  >
                    {state.kind === "checked" && `Step ${step_number}: checked`}
                    {state.kind === "explained" && `Step ${step_number}: not checked - ${state.reason}`}
                    {state.kind === "unchecked" && `Step ${step_number}: NOT CHECKED`}
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>
      </div>

      {existing.isError && (
        <p className="text-xs text-danger">
          Could not load the existing script, so saving is blocked: {existing.error.message}
        </p>
      )}
      {problem && <p className="text-xs text-danger">{problem}</p>}

      <div className="flex justify-end gap-2">
        <Button variant="ghost" size="sm" onClick={onClose}>
          <IconCancel aria-hidden />
          Cancel
        </Button>
        <Button size="sm" onClick={save} disabled={Boolean(blockedReason)}>
          <IconConfirm aria-hidden />
          Save script
        </Button>
      </div>
    </Modal>
  );
}
