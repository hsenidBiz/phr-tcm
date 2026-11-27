// Authoring the actions for one case.
//
// The window opens on the script in plain sentences, grouped by the case's
// steps, which sit alongside as the reference. Edit script swaps that for
// the JSON itself - the full detail, and what Save writes - and Back to
// readable view returns once the JSON parses.

import { useCloseOnEnvironmentChange } from "../../hooks/useCloseOnEnvironmentChange";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { toast } from "../../lib/toast";
import { commands, type Action, type CaseScript, type NeededRecord, type Precondition, type StepScript } from "../../bindings";
import { useEnvironments } from "../../lib/environments";
import { loadScript } from "../../lib/scriptBatch";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import Combobox from "../../components/ui/combobox";
import { Select } from "../../components/ui/select";
import { Input, Textarea } from "../../components/ui/input";
import { Modal } from "../../components/ui/modal";
import { MODAL_LARGE } from "./modalWidths";
import SharedStepLabel from "../../components/SharedStepLabel";
import CaseText from "../../components/CaseText";
import { unwrapStr } from "../../lib/ipc";
import {
  IconAbout,
  IconAdd,
  IconBack,
  IconCancel,
  IconConfirm,
  IconCopyBetweenEnvironments,
  IconCopyFromEnvironment,
  IconCopyToEnvironment,
  IconEdit,
  IconHideSteps,
  IconRemove,
  IconShowSteps,
  IconUndo,
} from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { plural } from "../../lib/plural";
import { sameAreaName } from "../../lib/areaName";
import { floorOf, type CheckState } from "./floor";
import { uncheckedExpectations, uncheckedParts } from "./unchecked";
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

/** What Must not save does: the checkbox's tooltip and its accessible description. */
const NO_SAVE_HELP =
  "For a case that works on a shared draft: while it runs, any save the page tries to send is stopped before it reaches the server, and the case fails.";

/** The button beside the script's label takes no more height than the
 * label line it replaced, so nothing under the script moves down. */
const HEADER_BUTTON = "-my-0.5 py-0.5";

/** A pull replaces the script on disk, and every field of the editor that
 * was touched would keep showing what was typed before it: the body is
 * mounted afresh after one, so it shows what was pulled. */
export default function ScriptEditor(props: ScriptEditorProps) {
  const [pulls, setPulls] = useState(0);
  return <ScriptEditorBody key={pulls} {...props} onPulled={() => setPulls((n) => n + 1)} />;
}

type ScriptEditorProps = {
  caseId: number;
  title: string;
  steps: { action: string; expected: string; shared?: number | null }[];
  /** The project whose address rule a save follows - the backend refuses a
   * save that names none. */
  org: string;
  project: string;
  /** Basic mode: the script is shown as text, with nothing to change, save
   * or copy. */
  readOnly?: boolean;
  onClose: () => void;
};

function ScriptEditorBody({
  caseId,
  title,
  steps,
  org,
  project,
  readOnly = false,
  onClose,
  onPulled,
}: ScriptEditorProps & { onPulled: () => void }) {
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
  // The case's divergences, from the list the screen and the case card read
  // (same key, so it costs nothing extra): a step with one that is open or
  // waiting for a repair is never folded away.
  const divergences = useQuery({
    queryKey: ["autorun-divergences", org, project],
    queryFn: () => unwrapStr(commands.autoRunLoadDivergences(org, project)),
    enabled: Boolean(org && project),
    retry: false,
  });
  const attention = new Set(
    (divergences.data ?? [])
      .filter((d) => d.case_id === caseId && (d.status === "open" || d.status === "awaiting_repair"))
      .map((d) => d.step),
  );
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
  // The kept records the case needs, by what the person removed here. The
  // save merges whatever was added since the editor opened.
  const [removedNeeds, setRemovedNeeds] = useState<NeededRecord[]>([]);
  const sameNeed = (a: NeededRecord, b: NeededRecord) => a.setup === b.setup && a.why === b.why;
  const needsRecords = (existing.data?.needs_records ?? []).filter((n) => !removedNeeds.some((r) => sameNeed(r, n)));
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
  // The header holds Edit script, which takes the focus back after Back to
  // readable view.
  const headerView = useRef<HTMLDivElement>(null);
  const noSaveHelpId = useId();
  useEffect(() => {
    if (!swapped.current) return;
    if (editing) jsonView.current?.querySelector("textarea")?.focus();
    else headerView.current?.querySelector("button")?.focus();
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
  // Anything the person has changed and not saved: a pull would put another
  // script under it, and Save would then overwrite what was pulled.
  const unsaved =
    text !== null ||
    picked !== null ||
    pickedArea !== null ||
    pickedNoSave !== null ||
    pickedChanges !== null ||
    pickedNeeds !== null ||
    pickedState !== null ||
    removedPre.length > 0 ||
    removedNeeds.length > 0 ||
    putBack.length > 0;
  const UNSAVED_REASON = "Save or discard your changes first";

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
    // Records an assistant added since this editor opened are kept.
    const latest = (await existing.refetch()).data?.needs_records ?? existing.data?.needs_records ?? [];
    const keptNeeds = latest.filter((n) => !removedNeeds.some((r) => sameNeed(r, n)));
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
      // Kept records the case needs are the assistant's to declare; the editor carries them through.
      ...(keptNeeds.length ? { needs_records: keptNeeds } : {}),
    });
    if (r.status === "error") {
      toast.error(`Could not save the script: ${r.error}`);
      return;
    }
    toast.success("Script saved.");
    await queryClient.invalidateQueries({ queryKey: ["autorun-script", caseId] });
    onClose();
  };

  const showJson = editing && !readOnly;
  const allChecked = checks.length > 0 && checks.every((c) => c.state.kind === "checked");
  const checksBlock = checks.length > 0 && !allChecked && (
    <div className="space-y-1">
      <span className="text-xs font-medium text-muted">Checks</span>
      <ul className="space-y-0.5 text-xs">
        {checks.map(({ step_number, state }) => (
          <li key={step_number} className={cn(state.kind === "unchecked" ? "text-warning" : "text-muted")}>
            {state.kind === "checked" && `Step ${step_number}: checked`}
            {state.kind === "explained" && `Step ${step_number}: not checked - ${state.reason}`}
            {state.kind === "unchecked" && `Step ${step_number}: NOT CHECKED`}
          </li>
        ))}
      </ul>
    </div>
  );

  return (
    <Modal onClose={onClose} className={`${MODAL_LARGE} flex flex-col gap-3 overflow-y-auto p-4`}>
      <div ref={headerView} className="flex items-start justify-between gap-3">
        <h2 className="min-w-0 text-sm font-semibold text-text">
          <span className="id-mono text-faint">#{caseId}</span> {title}
        </h2>
        {!readOnly && !showJson && (
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
        )}
      </div>

      {/* Who it runs as, where, and whether it may save: one row, stacked
          when the window is narrow. */}
      <div
        className={cn(
          "grid gap-3",
          readOnly
            ? "sm:grid-cols-[repeat(auto-fit,minmax(10rem,auto))]"
            : "sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] sm:items-end",
        )}
      >
        {readOnly ? (
          <p className="text-xs text-muted">
            Runs as{" "}
            <span className="text-text">
              {account === ""
                ? "No sign-in"
                : known
                  ? `${(accounts.data ?? []).find((a) => a.key === account)?.label.trim() || account} (${account})`
                  : `${account} (not on this machine)`}
            </span>
          </p>
        ) : (
          <label className="block text-xs text-muted">
            Runs as
            <Combobox
              ariaLabel="Runs as"
              className="mt-1 w-full"
              value={account}
              onChange={setPicked}
              clearable={false}
              triggerClassName="px-3 py-2"
              items={[
                { value: "", label: "No sign-in" },
                ...(accounts.data ?? []).map((a) => ({ value: a.key, label: `${a.label.trim() || a.key} (${a.key})` })),
                ...(account !== "" && !known ? [{ value: account, label: `${account} (not on this machine)` }] : []),
              ]}
            />
          </label>
        )}

        {readOnly ? (
          <p className="text-xs text-muted">
            Area <span className="text-text">{area === "" ? "the case's Module" : area}</span>
          </p>
        ) : (
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
        )}

        {readOnly ? (
          noSave && <p className="text-xs text-muted">Must not save: a save the page tries to send is stopped.</p>
        ) : (
          <div className="flex items-center gap-2 pb-2 text-xs text-muted">
            <label className="flex cursor-pointer items-center gap-2">
              <Checkbox checked={noSave} ariaLabel="Must not save" onCheckedChange={setPickedNoSave} />
              Must not save
            </label>
            {/* A button, so the keyboard reaches it; the app's tooltip layer
                shows a title on focus as well as on hover. */}
            <button
              type="button"
              aria-label="About Must not save"
              aria-describedby={noSaveHelpId}
              title={NO_SAVE_HELP}
              className="inline-flex text-faint hover:text-text focus-visible:outline-2 focus-visible:outline-accent"
            >
              <IconAbout aria-hidden className="size-3.5" />
            </button>
            <span id={noSaveHelpId} className="sr-only">
              {NO_SAVE_HELP}
            </span>
          </div>
        )}
      </div>

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
                  {!readOnly && (
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
                  )}
                </li>
              );
            })}
          </ul>
        </div>
      )}

      {showJson ? (
        // Editing: the case's steps stay beside the JSON as the reference,
        // all of them - nothing is folded while the script is being written.
        <div className="grid gap-3 lg:grid-cols-2">
          <div className="space-y-1">
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
          <div className="space-y-2">
            <p className="text-xs text-muted">
              <span className="font-medium text-text">Steps</span>
              {summaryLine(scriptForChecks, checks) && ` · ${summaryLine(scriptForChecks, checks)}`}
            </p>
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
            {checksBlock}
          </div>
        </div>
      ) : (
        <>
          <StepsPanel
            script={scriptForChecks}
            loading={existing.isLoading}
            steps={steps}
            org={org}
            checks={checks}
            attention={attention}
          />
          {checksBlock}
        </>
      )}

      <BeforeItRuns
        changes={changes}
        needsUnchanged={needsUnchanged}
        settings={needsState}
        readOnly={readOnly}
        onChanges={setPickedChanges}
        onNeeds={setPickedNeeds}
        onSettings={setPickedState}
      />

      <SetupSection org={org} project={project} caseId={caseId} />

      {needsRecords.length > 0 && (
        <div className="space-y-1">
          <span className="text-xs font-medium text-muted">Needed records</span>
          <ul aria-label="Needed records" className="space-y-1 text-xs">
            {needsRecords.map((n) => (
              <li key={`${n.setup}|${n.why}`} className="flex items-center justify-between gap-2">
                <span className="min-w-0 break-words text-text">{`${n.setup}: ${n.why}`}</span>
                {!readOnly && (
                  <Button
                    size="sm"
                    variant="ghost"
                    aria-label={`Remove needed record ${n.setup}`}
                    onClick={() => setRemovedNeeds((r) => [...r, n])}
                  >
                    <IconRemove aria-hidden />
                    Remove
                  </Button>
                )}
              </li>
            ))}
          </ul>
        </div>
      )}

      {preconditions.length > 0 && (
        <div className="space-y-1">
          <span className="text-xs font-medium text-muted">Preconditions</span>
          <ul aria-label="Preconditions" className="space-y-1 text-xs">
            {preconditions.map(({ p, n }) => (
              <li key={n} className="flex items-center justify-between gap-2">
                <span className="id-mono min-w-0 break-words text-text">{preconditionText(p)}</span>
                {!readOnly && (
                  <Button
                    size="sm"
                    variant="ghost"
                    aria-label={`Remove precondition ${n}`}
                    onClick={() => setRemovedPre((r) => [...r, p])}
                  >
                    <IconRemove aria-hidden />
                    Remove
                  </Button>
                )}
              </li>
            ))}
          </ul>
        </div>
      )}

      {existing.isError && (
        <p className="text-xs text-danger">
          {readOnly
            ? `Could not read the script: ${existing.error.message}`
            : `Could not load the existing script, so saving is blocked: ${existing.error.message}`}
        </p>
      )}
      {problem && <p className="text-xs text-danger">{problem}</p>}

      {readOnly ? (
        <div className="flex flex-wrap items-center justify-end gap-3">
          <p className="text-xs text-muted">Switch to Advanced to edit.</p>
          <Button variant="ghost" size="sm" onClick={onClose}>
            <IconCancel aria-hidden />
            Close
          </Button>
        </div>
      ) : (
        <div className="flex flex-wrap items-center justify-end gap-2">
          {hasOthers && (
            <CopyMenu
              canCopyTo={Boolean(existing.data)}
              fromBlocked={unsaved ? UNSAVED_REASON : undefined}
              onPick={setCopying}
            />
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
      )}
      {copying && (
        <CopyCaseDialog
          caseId={caseId}
          org={org}
          project={project}
          direction={copying}
          blockedReason={unsaved && copying === "from" ? UNSAVED_REASON : undefined}
          onCopied={() => {
            if (copying === "from") onPulled();
          }}
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
    const nests = rec?.kind === "when_visible" || rec?.kind === "when_hidden" || rec?.kind === "repeat_while_visible";
    const list = rec?.then;
    const then = nests && Array.isArray(list) && list.length > 0 ? list : null;
    return { sentence, then: sentenceText(sentence) === UNREADABLE ? null : then };
  } catch {
    return { sentence: UNREADABLE_LINE, then: null };
  }
}

/** The SQL of a database check, shown read-only under its sentence. */
function databaseSql(a: unknown): string | null {
  const rec = typeof a === "object" && a !== null ? (a as { kind?: unknown; sql?: unknown }) : null;
  return rec?.kind === "expect_database" && typeof rec.sql === "string" ? rec.sql : null;
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
            {databaseSql(a) !== null && (
              <pre className="mt-1 whitespace-pre-wrap break-words rounded border border-border bg-surface-2 px-2 py-1 font-mono text-xs text-text">
                {databaseSql(a)}
              </pre>
            )}
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

type CaseStep = { action: string; expected: string; shared?: number | null };

/** Past this many steps the rest fold away behind one button. */
const STEPS_SHOWN = 3;

/** True when an action in the group cannot be read: the step is shown
 * whatever the fold says, since it is the one that needs a look. */
function groupUnreadable(g: StepScript): boolean {
  if (!Array.isArray(g.actions)) return true;
  return (g.actions as unknown[]).some((a) => sentenceText(readAction(a).sentence) === UNREADABLE);
}

/** One script step's actions, as sentences, and what it leaves unchecked
 * under them, each part behind the warning tag "Not checked". */
function ActionsCell({ g }: { g: StepScript }) {
  const actions: unknown[] | null = Array.isArray(g.actions) ? g.actions : null;
  const parts = uncheckedParts(g.unchecked);
  return (
    <>
      {actions === null ? (
        <p>{UNREADABLE}</p>
      ) : actions.length > 0 ? (
        <ActionList actions={actions} />
      ) : (
        <p className="text-muted">No actions</p>
      )}
      {parts.length > 0 && (
        <ul className="mt-1 space-y-0.5">
          {parts.map((p, i) => (
            <li key={i} className="flex flex-wrap items-baseline gap-x-1.5 text-muted">
              <span className="rounded bg-warning/15 px-1.5 py-0.5 text-[10px] font-semibold text-warning">
                <span className="label-trim">Not checked</span>
              </span>
              <span className="min-w-0 break-words">{p}</span>
            </li>
          ))}
        </ul>
      )}
    </>
  );
}

/** A script step that belongs to no step of the case (the clean-up, or a
 * number the case does not have): its own heading, then its actions. */
function ExtraGroup({ g }: { g: StepScript }) {
  const label = g.cleanup === true ? "Clean-up" : `Step ${g.step_number}`;
  return (
    <section aria-label={label} className="space-y-1">
      <h3 className="text-xs font-medium text-text">
        <span className="text-faint">{label}</span>
      </h3>
      <div className="pl-3">
        <ActionsCell g={g} />
      </div>
    </section>
  );
}

/** "all N checked · M parts not checked", or "K of N checked": how the
 * script stands against the case's expected results. The parts count is the
 * one shown elsewhere as unchecked expectations (no clean-up, no step Auto
 * Test does itself). Empty when there is nothing to say. */
function summaryLine(script: StepScript[], checks: { step_number: number; state: CheckState }[]): string {
  const shaped = (script as unknown[]).filter(
    (s): s is StepScript =>
      typeof s === "object" && s !== null && Number.isInteger((s as { step_number?: unknown }).step_number),
  );
  const checkedN = checks.filter((c) => c.state.kind === "checked").length;
  const parts = uncheckedExpectations({
    // The JSON may be mid-edit: a step without an actions list counts as having none.
    steps: shaped.map((s) => ({ ...s, actions: Array.isArray(s.actions) ? s.actions : [] })),
  } as unknown as CaseScript).length;
  return [
    checks.length === 0
      ? null
      : checkedN === checks.length
        ? `all ${checks.length} checked`
        : `${checkedN} of ${checks.length} checked`,
    parts > 0 ? `${plural(parts, "part")} not checked` : null,
  ]
    .filter(Boolean)
    .join(" · ");
}

/** The case's steps, each beside the script's actions for it: the step's
 * words and expected result on the left, what the script does (and what it
 * leaves unchecked) on the right. The header sums up the checks. Past
 * three steps the rest fold, except the ones that need a look. */
function StepsPanel({
  script,
  loading,
  steps,
  org,
  checks,
  attention,
}: {
  script: StepScript[];
  loading: boolean;
  steps: CaseStep[];
  org: string;
  checks: { step_number: number; state: CheckState }[];
  /** Steps with an open or awaiting-repair divergence. */
  attention: ReadonlySet<number>;
}) {
  const [open, setOpen] = useState(false);
  // Only entries shaped like steps: the box can hold any JSON array. The
  // rest are counted, so nothing in the script goes unmentioned.
  const groups = (script as unknown[]).filter(
    (s): s is StepScript =>
      typeof s === "object" && s !== null && Number.isInteger((s as { step_number?: unknown }).step_number),
  );
  const skipped = script.length - groups.length;
  const ofCase = (g: StepScript) => g.cleanup !== true && g.step_number >= 1 && g.step_number <= steps.length;
  // Before the first step, as a sign-in step has always been shown; after
  // the last, the clean-up and anything numbered beyond the case.
  const before = groups.filter((g) => g.cleanup !== true && g.step_number < 1);
  const after = groups.filter((g) => !ofCase(g) && !before.includes(g));

  const rows = steps.map((s, i) => {
    const n = i + 1;
    const own = groups.filter((g) => ofCase(g) && g.step_number === n);
    const kind = checks.find((c) => c.step_number === n)?.state.kind;
    // Not checked, explained, or unreadable: never folded away.
    const needsLook = kind === "unchecked" || kind === "explained" || attention.has(n) || own.some(groupUnreadable);
    return { n, s, own, needsLook };
  });
  const foldable = rows.filter((r, i) => i >= STEPS_SHOWN && !r.needsLook);
  const shown = rows.filter((r, i) => open || i < STEPS_SHOWN || r.needsLook);

  const summary = summaryLine(script, checks);

  // The fold never names a step that is showing: it says how many are
  // folded and lists their numbers in its accessible name.
  const foldLabel = foldable.length === 1 ? "1 more step" : `${foldable.length} more steps`;
  const foldName = `${foldLabel}: ${foldable.map((r) => r.n).join(", ")}`;

  return (
    <div role="region" aria-label="Action script" className="space-y-2 rounded-md border border-border bg-surface px-3 py-2 text-xs text-text">
      <div className="flex flex-wrap items-baseline gap-x-2">
        <h3 className="text-xs font-medium text-text">Steps</h3>
        {summary && <span className="text-muted">{` · ${summary}`}</span>}
      </div>
      {loading ? (
        <p className="text-muted">Loading the script...</p>
      ) : (
        <>
          {groups.length === 0 && skipped === 0 && (
            <p className="text-muted">No actions yet. Press Edit script to write them.</p>
          )}
          {before.map((g, i) => (
            <ExtraGroup key={`before-${i}-${g.step_number}`} g={g} />
          ))}
          {rows.length > 0 && (
            <ol className="divide-y divide-border/60">
              {shown.map(({ n, s, own }) => (
                <li key={n}>
                  <section
                    aria-label={`Step ${n}`}
                    className="grid gap-x-4 gap-y-1 py-2 first:pt-0 sm:grid-cols-[minmax(0,2fr)_minmax(0,3fr)]"
                  >
                    <div className="flex min-w-0 gap-2">
                      <span className="w-5 shrink-0 text-right tabular-nums text-faint" aria-hidden>
                        {n}.
                      </span>
                      <div className="min-w-0 flex-1 space-y-0.5">
                        <div className="text-text">
                          {s.shared != null ? (
                            <SharedStepLabel id={s.shared} org={org} />
                          ) : (
                            <CaseText kind="step" text={s.action} className="min-w-0" />
                          )}
                        </div>
                        {s.shared == null && s.expected && (
                          <div className="flex gap-1 text-faint">
                            <span className="shrink-0">→</span>
                            <CaseText kind="step" text={s.expected} className="min-w-0 flex-1" />
                          </div>
                        )}
                      </div>
                    </div>
                    <div className="min-w-0 space-y-1">
                      {own.length === 0 ? (
                        <p className="text-faint">Nothing scripted</p>
                      ) : (
                        own.map((g, i) => <ActionsCell key={i} g={g} />)
                      )}
                    </div>
                  </section>
                </li>
              ))}
            </ol>
          )}
          {foldable.length > 0 && (
            <Button size="sm" variant="ghost" aria-expanded={open} aria-label={foldName} onClick={() => setOpen((o) => !o)}>
              {open ? <IconHideSteps aria-hidden /> : <IconShowSteps aria-hidden />}
              {foldLabel}
            </Button>
          )}
          {after.map((g, i) => (
            <ExtraGroup key={`after-${i}-${g.step_number}`} g={g} />
          ))}
          {skipped > 0 && (
            <p className="text-muted">
              {skipped === 1
                ? "1 more entry is shown only in Edit script."
                : `${skipped} more entries are shown only in Edit script.`}
            </p>
          )}
        </>
      )}
    </div>
  );
}

/** The comparison key of a mark's name, as the backend's `marks::normalise`
 * takes it: trimmed, inner whitespace collapsed, lowercased. */
function markKey(name: string): string {
  return name.trim().split(/\s+/).join(" ").toLowerCase();
}

/** One entry in "Before it runs": its kind, what it says, and (unless the
 * window is read-only) a button that removes it. */
function Chip({
  kind,
  text,
  removeLabel,
  onRemove,
}: {
  kind: string;
  text: string;
  removeLabel?: string;
  onRemove: () => void;
}) {
  return (
    <li className="flex max-w-full items-center gap-1 rounded bg-surface-2 py-0.5 pl-2 text-text">
      <span className="min-w-0 break-words">
        <span className="text-faint">{kind}:</span> {text}
      </span>
      {removeLabel === undefined ? (
        <span className="pr-2" />
      ) : (
        <Button size="sm" variant="ghost" className="px-1.5 py-0.5" aria-label={removeLabel} onClick={onRemove}>
          <IconRemove aria-hidden />
        </Button>
      )}
    </li>
  );
}

/** A box to add one name with Enter or Add. A name already present, as
 * `markKey` compares them, is not added again. It takes the focus when it
 * opens. */
function MarkInput({
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
  const box = useRef<HTMLInputElement>(null);
  useEffect(() => box.current?.focus(), []);
  const add = () => {
    const name = draft.trim();
    setDraft("");
    if (!name || names.some((n) => markKey(n) === markKey(name))) return;
    onChange([...names, name]);
  };
  return (
    <div className="flex items-center gap-2">
      <Input
        ref={box}
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
  );
}

/** A setting box and a value box to add a setting with Enter or Add. A
 * setting already present, as `markKey` compares names, takes the new
 * value. The run pauses between batches to change them. */
function SettingInput({
  settings,
  onChange,
}: {
  settings: [string, string][];
  onChange: (settings: [string, string][]) => void;
}) {
  const [name, setName] = useState("");
  const [value, setValue] = useState("");
  const box = useRef<HTMLInputElement>(null);
  useEffect(() => box.current?.focus(), []);
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
    <div className="flex items-center gap-2">
      <Input
        ref={box}
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
  );
}

/** What the case needs in place before it runs: the shared state it
 * changes, the shared state it needs unchanged, and the settings it needs.
 * What is there shows as removable chips; a small button per kind opens
 * that kind's box to add one. Nothing empty shows until one is pressed. */
function BeforeItRuns({
  changes,
  needsUnchanged,
  settings,
  readOnly = false,
  onChanges,
  onNeeds,
  onSettings,
}: {
  changes: string[];
  needsUnchanged: string[];
  settings: [string, string][];
  readOnly?: boolean;
  onChanges: (names: string[]) => void;
  onNeeds: (names: string[]) => void;
  onSettings: (settings: [string, string][]) => void;
}) {
  const [shown, setShown] = useState({ change: false, needs: false, setting: false });
  const toggle = (k: keyof typeof shown) => setShown((s) => ({ ...s, [k]: !s[k] }));
  const any = changes.length + needsUnchanged.length + settings.length > 0;
  if (readOnly && !any) return null;
  const opener = (k: keyof typeof shown, text: string, name: string) => (
    <Button size="sm" variant="outline" aria-label={name} aria-expanded={shown[k]} onClick={() => toggle(k)}>
      <IconAdd aria-hidden />
      {text}
    </Button>
  );
  return (
    <div className="space-y-2">
      <span className="text-xs font-medium text-muted">Before it runs</span>
      {any && (
        <ul aria-label="Before it runs" className="flex flex-wrap gap-1 text-xs">
          {changes.map((name, i) => (
            <Chip
              key={`c-${i}-${name}`}
              kind="Changes"
              text={name}
              removeLabel={readOnly ? undefined : `Remove change ${name}`}
              onRemove={() => onChanges(changes.filter((_, j) => j !== i))}
            />
          ))}
          {needsUnchanged.map((name, i) => (
            <Chip
              key={`n-${i}-${name}`}
              kind="Needs unchanged"
              text={name}
              removeLabel={readOnly ? undefined : `Remove needs unchanged ${name}`}
              onRemove={() => onNeeds(needsUnchanged.filter((_, j) => j !== i))}
            />
          ))}
          {settings.map(([k, v], i) => (
            <Chip
              key={`s-${i}-${k}`}
              kind="Setting"
              text={`${k} = ${v}`}
              removeLabel={readOnly ? undefined : `Remove setting ${k}`}
              onRemove={() => onSettings(settings.filter((_, j) => j !== i))}
            />
          ))}
        </ul>
      )}
      {!readOnly && (
        <>
          <div className="flex flex-wrap gap-1.5">
            {opener("change", "Change", "Add a change")}
            {opener("needs", "Needs unchanged", "Add a needs-unchanged record")}
            {opener("setting", "Setting", "Add a setting")}
          </div>
          {shown.change && <MarkInput label="Changes" noun="change" names={changes} onChange={onChanges} />}
          {shown.needs && (
            <MarkInput label="Needs unchanged" noun="needs unchanged" names={needsUnchanged} onChange={onNeeds} />
          )}
          {shown.setting && <SettingInput settings={settings} onChange={onSettings} />}
        </>
      )}
    </div>
  );
}

/** The footer's one copy button: a disclosure with the two directions as
 * plain buttons, each opening the copy dialog. Copying out needs a saved
 * script; copying in is refused (and says why) while the editor holds
 * unsaved edits. It closes when focus leaves it, and on Escape (which
 * returns the focus to the button and leaves the window open). */
function CopyMenu({
  canCopyTo,
  fromBlocked,
  onPick,
}: {
  canCopyTo: boolean;
  fromBlocked?: string;
  onPick: (direction: "to" | "from") => void;
}) {
  const [open, setOpen] = useState(false);
  const wrap = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!open) return;
    const away = (e: MouseEvent) => {
      if (!wrap.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", away);
    return () => document.removeEventListener("mousedown", away);
  }, [open]);
  const pick = (direction: "to" | "from") => {
    setOpen(false);
    onPick(direction);
  };
  const item =
    "flex w-full items-center gap-2 whitespace-nowrap rounded px-3 py-1.5 text-left text-xs font-medium text-text transition-colors hover:bg-accent-soft hover:text-accent focus-visible:bg-accent-soft focus-visible:text-accent focus-visible:outline-none disabled:pointer-events-none disabled:opacity-50 [&_svg]:size-3.5 [&_svg]:shrink-0";
  return (
    <div
      ref={wrap}
      className="relative mr-auto"
      onKeyDown={(e) => {
        if (e.key === "Escape" && open) {
          // Closes this only, not the window behind it.
          e.stopPropagation();
          setOpen(false);
          trigger.current?.focus();
        }
      }}
      onBlur={(e) => {
        if (open && !wrap.current?.contains(e.relatedTarget as Node | null)) setOpen(false);
      }}
    >
      <Button ref={trigger} variant="outline" size="sm" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
        <IconCopyBetweenEnvironments aria-hidden />
        Copy between environments
      </Button>
      {open && (
        <div className="absolute bottom-full left-0 z-10 mb-1 min-w-52 rounded-md border border-border bg-surface p-1 shadow-2xl">
          {canCopyTo && (
            <button type="button" className={item} onClick={() => pick("to")}>
              <IconCopyToEnvironment aria-hidden />
              Copy to another environment
            </button>
          )}
          <button
            type="button"
            className={item}
            disabled={fromBlocked !== undefined}
            title={fromBlocked}
            onClick={() => pick("from")}
          >
            <IconCopyFromEnvironment aria-hidden />
            Copy from another environment
          </button>
        </div>
      )}
    </div>
  );
}
