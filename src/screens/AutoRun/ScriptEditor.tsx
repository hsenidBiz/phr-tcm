// Authoring the actions for one case.
//
// The window opens on the script in plain sentences, grouped by the case's
// steps, which sit alongside as the reference. Edit script swaps that for
// the JSON itself - the full detail, and what Save writes - and Back to
// readable view returns once the JSON parses.

import { useCloseOnEnvironmentChange } from "../../hooks/useCloseOnEnvironmentChange";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { toast } from "../../lib/toast";
import { commands, type Action, type Precondition, type StepScript } from "../../bindings";
import { useEnvironments } from "../../lib/environments";
import { loadScript } from "../../lib/scriptBatch";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import { Select } from "../../components/ui/select";
import { Input, Textarea } from "../../components/ui/input";
import { Modal } from "../../components/ui/modal";
import { MODAL_LARGE } from "./modalWidths";
import SharedStepLabel from "../../components/SharedStepLabel";
import CaseText from "../../components/CaseText";
import { unwrapStr } from "../../lib/ipc";
import {
  IconAdd,
  IconBack,
  IconCancel,
  IconConfirm,
  IconCopyFromEnvironment,
  IconCopyToEnvironment,
  IconEdit,
  IconRemove,
  IconUndo,
} from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { caseTextLine } from "../../lib/markdown";
import { sameAreaName } from "../../lib/areaName";
import { floorOf } from "./floor";
import { uncheckedText } from "./unchecked";
import CopyCaseDialog from "./CopyCaseDialog";
import SetupSection from "./SetupSection";
import { canPutBack, preconditionText, putBackPreconditions, repairText } from "./preconditionRepairs";
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
  // What it holds is this environment's: it never outlives a switch.
  useCloseOnEnvironmentChange(onClose);
  const queryClient = useQueryClient();
  // Copying to or from another environment needs one to copy with.
  const environments = useEnvironments();
  const hasOthers = (environments.data?.environments.length ?? 0) >= 2;
  const [copying, setCopying] = useState<"to" | "from" | null>(null);

  const existing = useQuery({
    queryKey: ["autorun-script", caseId],
    queryFn: () => loadScript(caseId),
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

  // Preconditions are not edited here, only removed, taking effect on
  // Save. Removing is what gets a script whose flow or stage has since been
  // deleted saved again. What an assistant's repairs did to them can be
  // put back (by its place in the list of those repairs), also on Save;
  // the numbers are places in the list as put back.
  const [removedPre, setRemovedPre] = useState<Precondition[]>([]);
  const [putBack, setPutBack] = useState<number[]>([]);
  const preconditionRepairs = existing.data?.precondition_repairs ?? [];
  const putBackList = putBackPreconditions(existing.data?.preconditions ?? [], preconditionRepairs, putBack);
  const preconditions = putBackList
    .map((p, i) => ({ p, n: i + 1 }))
    .filter(({ p }) => !removedPre.includes(p));

  // The shared state the case changes or needs unchanged, by name. null =
  // untouched, so the saved script's names show until the person edits a
  // row. The save checks them (length, count, no name twice).
  const [pickedChanges, setPickedChanges] = useState<string[] | null>(null);
  const [pickedNeeds, setPickedNeeds] = useState<string[] | null>(null);
  const changes = pickedChanges ?? existing.data?.changes ?? [];
  const needsUnchanged = pickedNeeds ?? existing.data?.needs_unchanged ?? [];
  // The settings the case needs, each with its value. Same rule: null =
  // untouched. The save refuses one setting needed at two values.
  const [pickedState, setPickedState] = useState<[string, string][] | null>(null);
  const needsState: [string, string][] =
    pickedState ??
    Object.entries(existing.data?.needs_state ?? {}).flatMap(([k, v]) => (typeof v === "string" ? [[k, v] as [string, string]] : []));

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
      ...(needsState.length ? { needs_state: Object.fromEntries(needsState) } : {}),
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
                <div className="flex gap-1 text-text">
                  <span className="shrink-0">{i + 1}.</span>
                  {s.shared != null ? (
                    <SharedStepLabel id={s.shared} org={org} />
                  ) : (
                    <CaseText kind="step" text={s.action} className="min-w-0 flex-1" />
                  )}
                </div>
                {s.shared == null && s.expected && (
                  <div className="flex gap-1 text-faint">
                    <span className="shrink-0">→</span>
                    <CaseText kind="step" text={s.expected} className="min-w-0 flex-1" />
                  </div>
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
              Repaired {existing.data.repairs} {existing.data.repairs === 1 ? "time" : "times"} by an assistant since you
              last saved.
              {existing.data.last_repair && ` Last reason: ${existing.data.last_repair}`}
            </p>
          )}

          {preconditionRepairs.length > 0 && (
            <div className="space-y-1">
              <span className="text-xs font-medium text-muted">Precondition changes by an assistant</span>
              <ul aria-label="Precondition changes by an assistant" className="space-y-1 text-xs">
                {preconditionRepairs.map((r, i) => {
                  const done = putBack.includes(i);
                  // A later change changed its result again: putting this
                  // one back first would do nothing.
                  const changedLater = !done && !canPutBack(putBackList, r);
                  const label = done ? "Put back on Save" : "Put back";
                  return (
                    <li key={i} className="flex items-center justify-between gap-2">
                      <span className="min-w-0 break-words text-text">
                        {`${repairText(r)} - ${r.reason}`}
                        {changedLater && (
                          <span className="block text-faint">Changed again later - put back the newer change first.</span>
                        )}
                      </span>
                      <Button
                        size="sm"
                        variant="ghost"
                        aria-label={`${label}: precondition change ${i + 1}`}
                        disabled={done || changedLater}
                        onClick={() => setPutBack((b) => [...b, i])}
                      >
                        <IconUndo aria-hidden />
                        {label}
                      </Button>
                    </li>
                  );
                })}
              </ul>
            </div>
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

          {/* Side by side where there is room, so the settings row below
              them costs the window no height. */}
          <div className="grid gap-3 sm:grid-cols-2">
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
          </div>
          <SettingRow settings={needsState} onChange={setPickedState} />

          <SetupSection org={org} project={project} caseId={caseId} />

          {preconditions.length > 0 && (
            <div className="space-y-1">
              <span className="text-xs font-medium text-muted">Preconditions</span>
              <ul aria-label="Preconditions" className="space-y-1 text-xs">
                {preconditions.map(({ p, n }) => (
                  <li key={n} className="flex items-center justify-between gap-2">
                    <span className="id-mono min-w-0 break-words text-text">{preconditionText(p)}</span>
                    <Button
                      size="sm"
                      variant="ghost"
                      aria-label={`Remove precondition ${n}`}
                      onClick={() => setRemovedPre((r) => [...r, p])}
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

      <div className="flex flex-wrap justify-end gap-2">
        {hasOthers && (
          <div className="mr-auto flex flex-wrap gap-2">
            {existing.data && (
              <Button variant="outline" size="sm" onClick={() => setCopying("to")}>
                <IconCopyToEnvironment aria-hidden />
                Copy to environment…
              </Button>
            )}
            <Button variant="outline" size="sm" onClick={() => setCopying("from")}>
              <IconCopyFromEnvironment aria-hidden />
              Copy from environment…
            </Button>
          </div>
        )}
        <Button variant="ghost" size="sm" onClick={onClose}>
          <IconCancel aria-hidden />
          Cancel
        </Button>
        <Button size="sm" onClick={save} disabled={Boolean(blockedReason)}>
          <IconConfirm aria-hidden />
          Save script
        </Button>
      </div>
      {copying && (
        <CopyCaseDialog
          caseId={caseId}
          org={org}
          project={project}
          direction={copying}
          onClose={() => setCopying(null)}
        />
      )}
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
    const nests = rec?.kind === "when_visible" || rec?.kind === "repeat_while_visible";
    const list = rec?.then;
    const then = nests && Array.isArray(list) && list.length > 0 ? list : null;
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
          // The clean-up is no step of the case: named for what it is, with
          // no case text beside it.
          const tidy = s.cleanup === true;
          const own = tidy ? undefined : steps[s.step_number - 1];
          const label = tidy ? "Clean-up" : `Step ${s.step_number}`;
          const actions: unknown[] | null = Array.isArray(s.actions) ? s.actions : null;
          return (
            <section key={`${i}-${s.step_number}`} aria-label={label} className="space-y-1">
              <h3 className="text-xs font-medium text-text">
                <span className="text-faint">{label}</span>
                {own && (
                  <>
                    {" "}
                    {own.shared != null ? <SharedStepLabel id={own.shared} org={org} /> : caseTextLine(own.action)}
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
                {uncheckedText(s.unchecked) && (
                  <p className="mt-0.5 text-muted">Not checked: {uncheckedText(s.unchecked)}</p>
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

/** The settings a case needs: each `name: value` a chip with a remove
 * button, and a setting box and a value box to add one with Enter or Add.
 * A setting already in the row, as `markKey` compares names, takes the new
 * value. The run pauses between batches to change them. */
function SettingRow({
  settings,
  onChange,
}: {
  settings: [string, string][];
  onChange: (settings: [string, string][]) => void;
}) {
  const label = "Needs these settings";
  const [name, setName] = useState("");
  const [value, setValue] = useState("");
  const add = () => {
    const n = name.trim();
    const v = value.trim();
    if (!n || !v) return;
    setName("");
    setValue("");
    onChange([...settings.filter(([k]) => markKey(k) !== markKey(n)), [n, v]]);
  };
  const onEnter = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") {
      e.preventDefault();
      add();
    }
  };
  return (
    <div className="space-y-1">
      <span className="text-xs font-medium text-muted">{label}</span>
      {settings.length > 0 && (
        <ul aria-label={label} className="flex flex-wrap gap-1 text-xs">
          {settings.map(([k, v], i) => (
            <li key={`${i}-${k}`} className="flex items-center gap-1 rounded bg-surface-2 py-0.5 pl-2 text-text">
              {k}: {v}
              <Button
                size="sm"
                variant="ghost"
                className="px-1.5 py-0.5"
                aria-label={`Remove setting ${k}`}
                onClick={() => onChange(settings.filter((_, j) => j !== i))}
              >
                <IconRemove aria-hidden />
              </Button>
            </li>
          ))}
        </ul>
      )}
      <div className="flex items-center gap-2">
        <Input
          aria-label="Setting"
          placeholder="Setting"
          className="min-w-0 flex-1 px-2 py-1 text-xs"
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={onEnter}
        />
        <Input
          aria-label="Value"
          placeholder="Value"
          className="w-32 px-2 py-1 text-xs"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={onEnter}
        />
        <Button size="sm" variant="outline" aria-label="Add setting" onClick={add}>
          <IconAdd aria-hidden />
          Add
        </Button>
      </div>
    </div>
  );
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
