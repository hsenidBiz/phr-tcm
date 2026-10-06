// Authoring the actions for one case.
//
// The window opens on the script in plain sentences, grouped by the case's
// steps, which sit alongside as the reference. Edit script swaps that for
// the JSON itself - the full detail, and what Save writes - and Back to
// readable view returns once the JSON parses.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { toast } from "../../lib/toast";
import { commands, type Action, type StepScript } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import { Select } from "../../components/ui/select";
import { Input, Textarea } from "../../components/ui/input";
import { Modal } from "../../components/ui/modal";
import { MODAL_LARGE } from "./modalWidths";
import SharedStepLabel from "../../components/SharedStepLabel";
import { unwrapStr } from "../../lib/ipc";
import { IconAdd, IconBack, IconCancel, IconConfirm, IconEdit, IconRemove } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { sameAreaName } from "../../lib/areaName";
import { floorOf } from "./floor";
import SetupSection from "./SetupSection";
import { describeAction, sentenceText, UNREADABLE, type Sentence } from "./describeAction";

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

/** The button beside the script's label takes no more height than the
 * label line it replaced, so nothing under the script moves down. */
const HEADER_BUTTON = "-my-0.5 py-0.5";

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
  // A script may name an area in another case, or with other spaces, than
  // it was recorded in: matched as runs match it.
  const match = recorded.find((a) => sameAreaName(a.area, named));
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
  // The window always opens on the readable script; Edit script shows the
  // JSON in its place.
  const [editing, setEditing] = useState(false);
  // Where the focus goes when the view swaps: into the JSON after Edit
  // script, back onto Edit script after Back to readable view. Not on
  // opening, which is the modal's own business.
  const swapped = useRef(false);
  const jsonView = useRef<HTMLDivElement>(null);
  const readableView = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!swapped.current) return;
    if (editing) jsonView.current?.querySelector("textarea")?.focus();
    else readableView.current?.querySelector("button")?.focus();
  }, [editing]);
  const swapTo = (toEditing: boolean) => {
    swapped.current = true;
    setEditing(toEditing);
  };
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

  /** The box's JSON as steps, or null after saying why it is not. */
  const parseValue = (): StepScript[] | null => {
    let parsed: StepScript[];
    try {
      parsed = JSON.parse(value || "[]") as StepScript[];
    } catch (e) {
      setProblem(`That is not valid JSON: ${(e as Error).message}`);
      return null;
    }
    if (!Array.isArray(parsed)) {
      setProblem("That is not valid JSON: the script must be an array of steps.");
      return null;
    }
    setProblem("");
    return parsed;
  };

  const backToReadable = () => {
    if (parseValue()) swapTo(false);
  };

  const save = async () => {
    if (blockedReason) {
      setProblem(blockedReason);
      return;
    }
    const parsed = parseValue();
    if (!parsed) return;
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
      // Settings this editor has no control for are carried through as the
      // script has them, so a save from here never drops them.
      ...(existing.data?.fail_on_unexpected_dialog ? { fail_on_unexpected_dialog: true } : {}),
      ...(existing.data?.page_errors ? { page_errors: existing.data.page_errors } : {}),
      ...(existing.data?.ignore_page_errors?.length ? { ignore_page_errors: existing.data.ignore_page_errors } : {}),
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
    <Modal onClose={onClose} className={`${MODAL_LARGE} flex flex-col gap-3 overflow-y-auto p-4`}>
      <h2 className="text-sm font-semibold text-text">
        <span className="id-mono text-faint">#{caseId}</span> {title}
      </h2>

      <div className="grid gap-3 lg:min-h-0 lg:flex-1 lg:grid-cols-2 lg:grid-rows-[minmax(0,1fr)] lg:overflow-hidden">
        <div className="space-y-1 lg:min-h-0 lg:overflow-y-auto">
          <span className="text-xs font-medium text-muted">The case's steps</span>
          <ol className="space-y-1 text-xs text-muted">
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

        <div className="space-y-2 lg:min-h-0 lg:overflow-y-auto">
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

          {editing ? (
            <div ref={jsonView} className="space-y-1">
              <div className="flex items-center justify-between gap-2">
                <span className="text-xs text-muted">Action script JSON</span>
                <Button size="sm" variant="ghost" className={HEADER_BUTTON} onClick={backToReadable}>
                  <IconBack aria-hidden />
                  Back to readable view
                </Button>
              </div>
              <Textarea
                aria-label="Action script JSON"
                className="h-80 w-full font-mono text-xs"
                placeholder={PLACEHOLDER}
                value={value}
                onChange={(e) => setText(e.target.value)}
              />
            </div>
          ) : (
            <div ref={readableView} className="space-y-1">
              <div className="flex items-center justify-between gap-2">
                <span className="text-xs text-muted">Action script</span>
                <Button
                  size="sm"
                  variant="ghost"
                  className={HEADER_BUTTON}
                  onClick={() => {
                    setProblem("");
                    swapTo(true);
                  }}
                >
                  <IconEdit aria-hidden />
                  Edit script
                </Button>
              </div>
              <ReadableScript script={scriptForChecks} loading={existing.isLoading} steps={steps} org={org} />
            </div>
          )}

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

/** A sentence from `describeAction`: words as they are, an element known
 * only by its CSS in a quieter monospace, and the full selector on hover
 * wherever a part carries one. */
function SentenceView({ sentence }: { sentence: Sentence }) {
  return (
    <>
      {sentence.map((p, i) =>
        p.kind === "css" ? (
          <span key={i} className="id-mono break-all text-faint" title={p.title}>
            {p.text}
          </span>
        ) : (
          <span key={i} title={p.title}>
            {p.text}
            {/* Hover is not the only way to the selector: a screen reader
                hears it too. */}
            {p.title && <span className="sr-only">{` (selector ${p.title})`}</span>}
          </span>
        ),
      )}
    </>
  );
}

/** The unreadable line, in the sentence shape. */
const UNREADABLE_LINE: Sentence = [{ kind: "words", text: UNREADABLE }];

/** One action's sentence and the actions nested under it. The describer is
 * total already; this catches anyway, so one odd action can only ever cost
 * its own line, never the window. */
function readAction(a: unknown): { sentence: Sentence; then: readonly unknown[] | null } {
  try {
    const sentence = describeAction(a as Action);
    const rec = typeof a === "object" && a !== null ? (a as { kind?: unknown; then?: unknown }) : null;
    const then = rec?.kind === "when_visible" && Array.isArray(rec.then) && rec.then.length > 0 ? rec.then : null;
    return { sentence, then: sentenceText(sentence) === UNREADABLE ? null : then };
  } catch {
    return { sentence: UNREADABLE_LINE, then: null };
  }
}

/** A step's actions as a list of sentences; a `when_visible`'s own actions
 * indented under its "If ... appears" line. */
function ActionList({ actions }: { actions: readonly unknown[] }) {
  return (
    <ul className="space-y-0.5">
      {actions.map((a, i) => {
        const { sentence, then } = readAction(a);
        return (
          <li key={i} className="break-words">
            <SentenceView sentence={sentence} />
            {then && (
              <div className="mt-0.5 border-l border-border/60 pl-3">
                <ActionList actions={then} />
              </div>
            )}
          </li>
        );
      })}
    </ul>
  );
}

/** The script in plain sentences, a group per scripted step headed by the
 * step's number and the case's own words for it. The same size as the JSON
 * box it stands in for. */
function ReadableScript({
  script,
  loading,
  steps,
  org,
}: {
  script: StepScript[];
  loading: boolean;
  steps: { action: string; expected: string; shared?: number | null }[];
  org: string;
}) {
  // Only entries shaped like steps: the box can hold any JSON array. The
  // rest are counted, so nothing in the script goes unmentioned.
  const groups = (script as unknown[]).filter(
    (s): s is StepScript =>
      typeof s === "object" && s !== null && Number.isInteger((s as { step_number?: unknown }).step_number),
  );
  const skipped = script.length - groups.length;
  return (
    <div
      role="region"
      aria-label="Action script"
      tabIndex={0}
      className="h-80 w-full space-y-3 overflow-y-auto rounded-md border border-border bg-surface px-3 py-2 text-xs text-text focus:border-accent focus:outline-none"
    >
      {loading ? (
        <p className="text-muted">Loading the script...</p>
      ) : groups.length === 0 && skipped === 0 ? (
        <p className="text-muted">No actions yet. Press Edit script to write them.</p>
      ) : (
        groups.map((s, i) => {
          const own = steps[s.step_number - 1];
          const actions: unknown[] | null = Array.isArray(s.actions) ? s.actions : null;
          return (
            <section key={`${i}-${s.step_number}`} aria-label={`Step ${s.step_number}`} className="space-y-1">
              <h3 className="text-xs font-medium text-text">
                <span className="text-faint">Step {s.step_number}</span>
                {own && (
                  <>
                    {" "}
                    {own.shared != null ? <SharedStepLabel id={own.shared} org={org} /> : own.action}
                  </>
                )}
              </h3>
              <div className="pl-3">
                {actions === null ? (
                  <p>{UNREADABLE}</p>
                ) : actions.length > 0 ? (
                  <ActionList actions={actions} />
                ) : (
                  <p className="text-muted">No actions</p>
                )}
                {typeof s.unchecked === "string" && s.unchecked && (
                  <p className="mt-0.5 text-muted">Not checked: {s.unchecked}</p>
                )}
              </div>
            </section>
          );
        })
      )}
      {!loading && skipped > 0 && (
        <p className="text-muted">
          {skipped === 1
            ? "1 more entry is shown only in Edit script."
            : `${skipped} more entries are shown only in Edit script.`}
        </p>
      )}
    </div>
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
