// Authoring the actions for one case, as JSON.
//
// JSON on purpose, for now: an assistant will generate these scripts
// later, and the format has to be proven by hand before anything
// generates it. The case's own steps sit alongside as the reference.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "../../lib/toast";
import { commands, type StepScript } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import { Select } from "../../components/ui/select";
import { Input, Textarea } from "../../components/ui/input";
import { Modal } from "../../components/ui/modal";
import SharedStepLabel from "../../components/SharedStepLabel";
import { unwrapStr } from "../../lib/ipc";
import { IconAdd, IconCancel, IconConfirm, IconRemove } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { floorOf } from "./floor";
import SetupSection from "./SetupSection";

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
  org,
  project,
  onClose,
}: {
  caseId: number;
  title: string;
  steps: { action: string; expected: string; shared?: number | null }[];
  /** The project whose address rule a save follows - the backend refuses a
   * save that names none. */
  org: string;
  project: string;
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
  // The project's recorded areas, for the Area select. Shares its key with
  // the Areas dialog, so a recording there updates this list.
  const areas = useQuery({
    queryKey: ["autorun-nav", org, project],
    queryFn: async () => (await unwrapStr(commands.autoRunLoadNav(org, project))) ?? null,
    enabled: Boolean(org && project),
    retry: false,
  });
  const recorded = areas.data?.modules ?? [];
  // null = untouched, so the saved script's area shows until the person picks.
  const [pickedArea, setPickedArea] = useState<string | null>(null);
  const named = pickedArea ?? existing.data?.area?.trim() ?? "";
  // A script may name an area in another case than it was recorded in.
  const match = recorded.find((a) => a.area.trim().toLowerCase() === named.toLowerCase());
  const area = match?.area ?? named;
  const areaKnown = match !== undefined;
  // null = untouched, so the saved script's account shows until the person picks.
  const [picked, setPicked] = useState<string | null>(null);
  const account = picked ?? existing.data?.account ?? "";
  const known = (accounts.data ?? []).some((a) => a.key === account);
  // null = untouched, so the saved script's flag shows until the person
  // changes it. Only a save from here can turn it off.
  const [pickedNoSave, setPickedNoSave] = useState<boolean | null>(null);
  const noSave = pickedNoSave ?? existing.data?.no_save ?? false;

  // Preconditions are not edited here, only removed: by their place in the
  // saved script, taking effect on Save. Removing is what gets a script
  // whose flow or stage has since been deleted saved again.
  const [removedPre, setRemovedPre] = useState<number[]>([]);
  const preconditions = (existing.data?.preconditions ?? [])
    .map((p, i) => ({ p, n: i + 1 }))
    .filter(({ n }) => !removedPre.includes(n));

  // The shared state the case changes or needs unchanged, by name. null =
  // untouched, so the saved script's names show until the person edits a
  // row. The save checks them (length, count, no name twice).
  const [pickedChanges, setPickedChanges] = useState<string[] | null>(null);
  const [pickedNeeds, setPickedNeeds] = useState<string[] | null>(null);
  const changes = pickedChanges ?? existing.data?.changes ?? [];
  const needsUnchanged = pickedNeeds ?? existing.data?.needs_unchanged ?? [];

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
    const r = await commands.autoRunSaveScript(org, project, {
      case_id: caseId,
      title,
      steps: parsed,
      account: account === "" ? null : account,
      // Left out when blank: the module's default area.
      ...(area === "" ? {} : { area }),
      ...(noSave ? { no_save: true } : {}),
      // Carried through, less any removed here (the save checks them
      // again).
      ...(preconditions.length ? { preconditions: preconditions.map(({ p }) => p) } : {}),
      // No `setup`: only the assistant writes one, and Rust keeps the one
      // stored for the case whatever a save sends.
      ...(changes.length ? { changes } : {}),
      ...(needsUnchanged.length ? { needs_unchanged: needsUnchanged } : {}),
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
                  {i + 1}.{" "}
                  {s.shared != null ? <SharedStepLabel id={s.shared} org={org} /> : s.action}
                </span>
                {s.shared == null && s.expected && (
                  <div className="text-faint">→ {s.expected}</div>
                )}
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
            Area
            <Select
              aria-label="Area"
              className="mt-1 w-full"
              value={area}
              onChange={(e) => setPickedArea(e.target.value)}
            >
              <option value="">the case's Module</option>
              {recorded.map((a) => (
                <option key={a.area} value={a.area}>
                  {a.area}
                </option>
              ))}
              {area !== "" && !areaKnown && <option value={area}>{`${area} (not recorded)`}</option>}
            </Select>
          </label>

          <label className="flex cursor-pointer items-center gap-2 text-xs text-muted">
            <Checkbox checked={noSave} ariaLabel="Must not save" onCheckedChange={setPickedNoSave} />
            Must not save
          </label>
          <p className="text-xs text-faint">
            For a case that works on a shared draft: while it runs, any save the page tries to send
            is stopped before it reaches the server, and the case fails.
          </p>

          {existing.data && (existing.data.repairs ?? 0) > 0 && (
            <p className="text-xs text-warning">
              Repaired {existing.data.repairs} of 3 times by an assistant since you last saved.
              {existing.data.last_repair && ` Last reason: ${existing.data.last_repair}`}
            </p>
          )}

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

          <MarkRow
            label="Changes"
            noun="change"
            names={changes}
            onChange={setPickedChanges}
          />
          <MarkRow
            label="Needs unchanged"
            noun="needs unchanged"
            names={needsUnchanged}
            onChange={setPickedNeeds}
          />

          <SetupSection org={org} project={project} caseId={caseId} />

          {preconditions.length > 0 && (
            <div className="space-y-1">
              <span className="text-xs font-medium text-muted">Preconditions</span>
              <ul aria-label="Preconditions" className="space-y-1 text-xs">
                {preconditions.map(({ p, n }) => (
                  <li key={n} className="flex items-center justify-between gap-2">
                    <span className="id-mono min-w-0 break-words text-text">
                      {`${p.flow} / ${p.stage}: ${typeof p.value === "string" ? p.value : JSON.stringify(p.value)}`}
                    </span>
                    <Button
                      size="sm"
                      variant="ghost"
                      aria-label={`Remove precondition ${n}`}
                      onClick={() => setRemovedPre((r) => [...r, n])}
                    >
                      <IconRemove aria-hidden />
                      Remove
                    </Button>
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

/** The comparison key of a mark's name, as the backend's `marks::normalise`
 * takes it: trimmed, inner whitespace collapsed, lowercased. */
function markKey(name: string): string {
  return name.trim().split(/\s+/).join(" ").toLowerCase();
}

/** One row of marks: each name a chip with a remove button, and a box to
 * add one with Enter or Add. A name already in the row, as `markKey`
 * compares them, is not added again. */
function MarkRow({
  label,
  noun,
  names,
  onChange,
}: {
  label: string;
  noun: string;
  names: string[];
  onChange: (names: string[]) => void;
}) {
  const [draft, setDraft] = useState("");
  const add = () => {
    const name = draft.trim();
    setDraft("");
    if (!name || names.some((n) => markKey(n) === markKey(name))) return;
    onChange([...names, name]);
  };
  return (
    <div className="space-y-1">
      <span className="text-xs font-medium text-muted">{label}</span>
      {names.length > 0 && (
        <ul aria-label={label} className="flex flex-wrap gap-1 text-xs">
          {names.map((name, i) => (
            <li
              key={`${i}-${name}`}
              className="flex items-center gap-1 rounded bg-surface-2 py-0.5 pl-2 text-text"
            >
              {name}
              <Button
                size="sm"
                variant="ghost"
                className="px-1.5 py-0.5"
                aria-label={`Remove ${noun} ${name}`}
                onClick={() => onChange(names.filter((_, j) => j !== i))}
              >
                <IconRemove aria-hidden />
              </Button>
            </li>
          ))}
        </ul>
      )}
      <div className="flex items-center gap-2">
        <Input
          aria-label={`Add to ${label}`}
          className="min-w-0 flex-1 px-2 py-1 text-xs"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              add();
            }
          }}
        />
        <Button size="sm" variant="outline" aria-label={`Add ${noun}`} onClick={add}>
          <IconAdd aria-hidden />
          Add
        </Button>
      </div>
    </div>
  );
}
