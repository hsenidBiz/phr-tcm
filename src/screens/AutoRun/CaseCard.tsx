// One case on the Auto Run Test cases list, as a card that opens.
//
// Collapsed it is a single line: the checkbox, the id, the title and the
// case's last result. Nothing on it is a button but the chevron and the
// title, which both open it. Open, it shows what the saved script does (its
// summary, its steps, the files it uploads and checks), the last run's
// downloads, and then the Script and Run buttons, or Add script for a case
// that has none.

import { useId } from "react";
import type { Action, CaseScript } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import SharedStepLabel from "../../components/SharedStepLabel";
import { IconAdd, IconEdit, IconHideDetails, IconRun, IconShowDetails } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import RunDownloads from "./RunDownloads";
import { ClearConfirm, SuspectedDefectBadge } from "./SuspectedDefectMark";
import { bucketTone, type ResultBucket } from "./verdicts";

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** Every action of a script, the ones nested in a `when_visible` too. */
function allActions(script: CaseScript): Action[] {
  const out: Action[] = [];
  const walk = (list: readonly Action[]) => {
    for (const a of list) {
      out.push(a);
      if (a.kind === "when_visible") walk(a.then);
    }
  };
  for (const step of script.steps) walk(step.actions);
  return out;
}

/** The test files a script uploads, and the downloads it checks, each
 * once, in the order the script first names them. */
export function scriptFiles(script: CaseScript): { uploads: string[]; downloads: string[] } {
  const uploads = new Set<string>();
  const downloads = new Set<string>();
  for (const a of allActions(script)) {
    if (a.kind === "upload") uploads.add(a.file);
    if (a.kind === "expect_download") downloads.add(a.name);
  }
  return { uploads: [...uploads], downloads: [...downloads] };
}

/** The script's facts worth a line each. A fact with no value is left out,
 * so the card never prints a label with nothing beside it. */
export function scriptFacts(script: CaseScript): { label: string; value: string }[] {
  const facts: { label: string; value: string | null | undefined }[] = [
    { label: "Step count", value: plural(script.steps.length, "step") },
    { label: "Runs as", value: script.account?.trim() },
    { label: "Area", value: script.area?.trim() },
    { label: "Must not save", value: script.no_save ? "Yes, saves are stopped while it runs" : null },
    {
      label: "Preconditions",
      value: (script.preconditions ?? []).map((p) => `${p.flow}: ${p.stage}`).join(", "),
    },
    { label: "Changes", value: (script.changes ?? []).join(", ") },
    { label: "Needs unchanged", value: (script.needs_unchanged ?? []).join(", ") },
    { label: "Last repair", value: script.last_repair?.trim() },
  ];
  return facts.flatMap(({ label, value }) => (value ? [{ label, value }] : []));
}

/** A small label over one section of an open card. */
function SectionLabel({ children }: { children: string }) {
  return <p className="text-xs font-medium text-muted">{children}</p>;
}

export default function CaseCard({
  c,
  org,
  script,
  result,
  selected,
  onSelect,
  open,
  onToggleOpen,
  onEdit,
  onRun,
  confirmingClear,
  onAskClear,
  onClearDone,
  lastRun,
}: {
  c: { id: number; title: string; steps: { action: string; expected: string; shared?: number | null }[] };
  org: string;
  /** The saved script, or null/undefined for a case with none. */
  script: CaseScript | null | undefined;
  result: ResultBucket;
  selected: boolean;
  onSelect: () => void;
  open: boolean;
  onToggleOpen: () => void;
  onEdit: () => void;
  onRun: () => void;
  /** The suspected-defect Clear is waiting on Keep or Clear. */
  confirmingClear: boolean;
  onAskClear: () => void;
  onClearDone: () => void;
  /** The run the last result came from, with this case's steps in it. */
  lastRun?: { runId: string; steps?: ReadonlyArray<{ downloads?: string[] }> };
}) {
  const bodyId = useId();
  const ready = Boolean(script);
  const defect = script?.suspected_defect;
  const files = script ? scriptFiles(script) : { uploads: [], downloads: [] };
  const hasRunDownloads = (lastRun?.steps ?? []).some((s) => (s.downloads ?? []).length > 0);
  const showFiles = files.uploads.length > 0 || files.downloads.length > 0 || hasRunDownloads;
  const facts = script ? scriptFacts(script) : [];

  return (
    <li className="rounded-md border border-border bg-surface text-sm">
      <div className="flex items-center gap-2 px-3 py-2">
        <Button
          variant="ghost"
          size="sm"
          className="shrink-0 px-1 py-1"
          aria-label={`${open ? "Hide" : "Show"} details for #${c.id}`}
          aria-expanded={open}
          aria-controls={open ? bodyId : undefined}
          onClick={onToggleOpen}
        >
          {open ? <IconHideDetails aria-hidden /> : <IconShowDetails aria-hidden />}
        </Button>
        {/* Only a scripted case can be run, so only a scripted case can be
            ticked - a checkbox that selects something unrunnable would
            just make the count lie. */}
        <Checkbox
          checked={selected}
          ariaLabel={`Select #${c.id}`}
          className={ready ? undefined : "invisible"}
          onCheckedChange={() => ready && onSelect()}
        />
        <span className="id-mono shrink-0 text-faint">#{c.id}</span>
        {/* The title gets the room and wraps: a truncated title is exactly
            the part that tells two similar cases apart. A click on it opens
            the card, as the chevron does. */}
        <button
          type="button"
          aria-expanded={open}
          aria-controls={open ? bodyId : undefined}
          className="min-w-0 flex-1 break-words text-left text-text transition-colors hover:text-accent"
          onClick={onToggleOpen}
        >
          {c.title}
        </button>
        {/* The result of the case's last run, in Past runs' words and
            colours. */}
        <span className={cn("shrink-0 text-xs font-medium", bucketTone[result])}>
          <span className="sr-only">Last result: </span>
          {result}
        </span>
      </div>

      {open && (
        <div id={bodyId} className="space-y-3 border-t border-border/60 px-3 pb-3 pt-2">
          {!script ? (
            <p className="text-xs text-muted">No script yet</p>
          ) : (
            <>
              <div className="space-y-1">
                <SectionLabel>Script</SectionLabel>
                <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-3 gap-y-0.5 text-xs">
                  {facts.map(({ label, value }) => (
                    <div key={label} className="contents">
                      <dt className="text-muted">{label}</dt>
                      <dd className="break-words text-text">{value}</dd>
                    </div>
                  ))}
                </dl>
                {defect && (
                  <div className="flex flex-wrap items-center gap-2 pt-1">
                    <SuspectedDefectBadge caseId={c.id} defect={defect} onClear={onAskClear} />
                    <span className="min-w-0 flex-1 break-words text-xs text-text">
                      Step {defect.step_number}: {defect.note}
                    </span>
                    {confirmingClear && <ClearConfirm caseId={c.id} onDone={onClearDone} />}
                  </div>
                )}
              </div>

              <div className="space-y-1">
                <SectionLabel>Steps</SectionLabel>
                {script.steps.length === 0 ? (
                  <p className="text-xs text-muted">The script has no steps yet.</p>
                ) : (
                  <ol aria-label={`Script steps of #${c.id}`} className="space-y-0.5 text-xs">
                    {script.steps.map((s) => {
                      // The case's own words for the step, as the script
                      // editor lists them, then its action count, as the
                      // run pane does.
                      const own = c.steps[s.step_number - 1];
                      return (
                        <li key={s.step_number} className="flex min-w-0 gap-2">
                          <span className="shrink-0 text-faint">{s.step_number}.</span>
                          <span className="min-w-0 flex-1 truncate text-text" title={own?.action}>
                            {own?.shared != null ? <SharedStepLabel id={own.shared} org={org} /> : own?.action}
                          </span>
                          <span className="shrink-0 text-faint">{plural(s.actions.length, "action")}</span>
                        </li>
                      );
                    })}
                  </ol>
                )}
              </div>

              {showFiles && (
                <div className="space-y-2">
                  <SectionLabel>Files</SectionLabel>
                  {files.uploads.length > 0 && (
                    <p className="text-xs text-text">
                      <span className="text-muted">Uploads </span>
                      {files.uploads.join(", ")}
                    </p>
                  )}
                  {files.downloads.length > 0 && (
                    <p className="text-xs text-text">
                      <span className="text-muted">Checks downloads </span>
                      {files.downloads.join(", ")}
                    </p>
                  )}
                  {lastRun && hasRunDownloads && <RunDownloads runId={lastRun.runId} steps={lastRun.steps} />}
                </div>
              )}
            </>
          )}

          {/* No "Script ready" badge: the card says it with its buttons. A
              scripted case has a Run button, outlined in the success
              colour; a case with no script has no Run button, and its
              script button says Add. Both stay outline buttons: the one
              primary action on the screen is running the selection, in the
              dock. */}
          <div className="flex flex-wrap items-center gap-2">
            <Button
              size="sm"
              variant="outline"
              aria-label={`${ready ? "Edit" : "Add"} script for #${c.id}`}
              onClick={onEdit}
            >
              {ready ? <IconEdit aria-hidden /> : <IconAdd aria-hidden />}
              {ready ? "Script" : "Add script"}
            </Button>
            {ready && (
              <Button
                size="sm"
                variant="outline"
                className="border-success text-success hover:border-success hover:bg-success/10 hover:text-success"
                aria-label={`Run #${c.id}`}
                onClick={onRun}
              >
                <IconRun aria-hidden />
                Run
              </Button>
            )}
          </div>
        </div>
      )}
    </li>
  );
}
