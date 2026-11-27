// One case on the Auto Test Test cases list, as a card that opens.
//
// Collapsed it is a single line: the checkbox, the id, the title and the
// case's last result. Nothing on it is a button but the chevron and the
// title, which both open it. Open, it shows what the saved script does (its
// summary, its steps, the files it uploads and checks), the last run's
// downloads, and then the View script and Run buttons. A case with no script
// has neither: only an assistant writes one, so the card just says so.

import { useQuery } from "@tanstack/react-query";
import { memo, useId, useState, type CSSProperties, type ReactNode } from "react";
import type { Action, CaseScript, Divergence } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import SharedStepLabel from "../../components/SharedStepLabel";
import {
  IconBug,
  IconCollapseAll,
  IconExpandAll,
  IconHideDetails,
  IconMustNotSave,
  IconRuling,
  IconRun,
  IconRunsAs,
  IconScriptArea,
  IconScriptSteps,
  IconShowDetails,
  IconViewScript,
  IconWarning,
} from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { caseTextLine } from "../../lib/markdown";
import { plural } from "../../lib/plural";
import { doneByAutoRun } from "./byAutoRun";
import { badgeText, DivergenceRow, needsAttention } from "./DivergenceMark";
import RunDownloads from "./RunDownloads";
import { loadSetupView, setupViewKey } from "./setupView";
import { ClearConfirm, ClearedDefectNote, SuspectedDefectBadge } from "./SuspectedDefectMark";
import { uncheckedDetail, uncheckedExpectations, uncheckedLine, uncheckedWords } from "./unchecked";
import { bucketTone, type LastResult } from "./verdicts";

/** Every action of a script, the ones nested in a `when_visible` or `when_hidden` too. */
function allActions(script: CaseScript): Action[] {
  const out: Action[] = [];
  const walk = (list: readonly Action[]) => {
    for (const a of list) {
      out.push(a);
      if (a.kind === "when_visible" || a.kind === "when_hidden" || a.kind === "repeat_while_visible") walk(a.then);
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

type Fact = { label: string; value: string };

function present(facts: { label: string; value: string | null | undefined }[]): Fact[] {
  return facts.flatMap(({ label, value }) => (value ? [{ label, value }] : []));
}

/** What the chips over an open card say: "8 steps", who it runs as, the area
 * path and whether it must not save. An account or area with no value is left
 * out (null). */
export function scriptSummary(script: CaseScript): {
  steps: string;
  account: string | null;
  area: string | null;
  noSave: boolean;
} {
  return {
    steps: plural(script.steps.length, "step"),
    account: script.account?.trim() || null,
    area: script.area?.trim() || null,
    noSave: Boolean(script.no_save),
  };
}

/** The facts about what the script does that a person acts on, a line each.
 * A fact with no value is left out. */
export function actionFacts(script: CaseScript): Fact[] {
  return present([
    {
      label: "Preconditions",
      value: (script.preconditions ?? []).map((p) => `${p.flow}: ${p.stage}`).join(", "),
    },
    { label: "Needs records", value: (script.needs_records ?? []).map((n) => `${n.setup} (${n.why})`).join(", ") },
    { label: "Invented values", value: becauseFacts(script, true) },
  ]);
}

/** What the assistant noted about the script: folded away on the card. */
export function assistantNotes(script: CaseScript): Fact[] {
  return present([
    { label: "Changes", value: (script.changes ?? []).join(", ") },
    { label: "Needs unchanged", value: (script.needs_unchanged ?? []).join(", ") },
    { label: "Last repair", value: script.last_repair?.trim() },
    {
      label: "Checks given up",
      value: (script.given_up ?? [])
        .map((g) => `step ${g.step} ${g.kind}: ${g.why} (${g.at.slice(0, 10)})`)
        .join("; "),
    },
    { label: "Page errors", value: script.page_errors },
    { label: "Ignored page errors", value: (script.ignore_page_errors ?? []).join(", ") },
    { label: "Unexpected dialogs", value: script.fail_on_unexpected_dialog ? "fail" : null },
  ]);
}

/** One step's "because" entries of one source (an entry with no source reads
 * as assumed), each as "value (why)". The card shows the assumed ones under
 * their step; the invented ones are a fact of the whole script. */
export function stepBecause(step: CaseScript["steps"][number], invented: boolean): string[] {
  return stepBecauseParts(step, invented).map((p) => (p.why ? `${p.value} (${p.why})` : p.value));
}

/** The same entries with the value and its reason apart, for the opened step's
 * grid. The reason is the entry's own `why`, with a leading "assumed:" or
 * "invented:" taken off if the assistant wrote one; nothing else is cut. */
export function stepBecauseParts(
  step: CaseScript["steps"][number],
  invented: boolean,
): { value: string; why: string }[] {
  return (step.because ?? [])
    .filter((b) => (b.provenance === "invented") === invented)
    .map((b) => ({ value: b.value, why: (b.why ?? "").replace(/^\s*(assumed|invented)\s*:\s*/i, "").trim() }));
}

/** Every step's "because" entries of one source, as "step 2: 6 (why); ...". */
function becauseFacts(script: CaseScript, invented: boolean): string {
  return script.steps.flatMap((s) => stepBecause(s, invented).map((t) => `step ${s.step_number}: ${t}`)).join("; ");
}

/** A small tag over a line of a step: a word, never colour alone. */
function NoteTag({ tone, children }: { tone: "neutral" | "warning" | "accent"; children: string }) {
  return (
    <span
      className={cn(
        "shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium",
        tone === "neutral" && "bg-surface-2 text-muted",
        tone === "warning" && "bg-warning/15 text-warning",
        tone === "accent" && "bg-accent/15 text-accent-fill",
      )}
    >
      <span className="label-trim">{children}</span>
    </span>
  );
}

/** One chip of the summary line over an open card. */
function SummaryChip({ icon, tone = "neutral", children }: { icon: ReactNode; tone?: "neutral" | "warning" | "accent"; children: string }) {
  return (
    <span
      className={cn(
        "inline-flex min-w-0 items-center gap-1 rounded-md px-2 py-0.5 text-xs [&_svg]:size-3 [&_svg]:shrink-0",
        tone === "warning" && "bg-warning/15 text-warning",
        tone === "accent" && "bg-accent/15 text-accent-fill",
        tone === "neutral" && "bg-surface-2 text-muted",
      )}
    >
      {icon}
      <span className="min-w-0 break-words">{children}</span>
    </span>
  );
}

/** A note line under a step: its tag, then the text. */
function NoteLine({ tone, tag, children }: { tone: "neutral" | "warning"; tag: string; children: string }) {
  return (
    <p className="flex min-w-0 items-start gap-2 text-xs">
      <NoteTag tone={tone}>{tag}</NoteTag>
      <span className="min-w-0 flex-1 break-words text-text">{children}</span>
    </p>
  );
}

/** An opened step's assumed or invented values: the value, then why in muted
 * text beside it. The reason is the entry's own; with none, the value stands. */
function BecauseGrid({ label, tag, entries }: { label: string; tag: string; entries: { value: string; why: string }[] }) {
  return (
    <div role="group" aria-label={label} className="flex min-w-0 items-start gap-2 text-xs">
      <NoteTag tone="neutral">{tag}</NoteTag>
      <dl className="grid min-w-0 flex-1 grid-cols-[max-content_minmax(0,1fr)] gap-x-3 gap-y-0.5">
        {entries.map((e, i) => (
          <div key={i} className="contents">
            <dt className="break-words text-text">{e.value}</dt>
            <dd className="break-words text-muted">{e.why}</dd>
          </div>
        ))}
      </dl>
    </div>
  );
}

/** A small label over one section of an open card. */
function SectionLabel({ children }: { children: string }) {
  return <p className="text-xs font-medium text-muted">{children}</p>;
}

/** One case of the list. Memo: its handlers take the case's id and are the
 * same functions on every render of the list, and its other props are the
 * list's own values, so ticking or opening one card draws only that card. */
export default memo(CaseCard);

function CaseCard({
  c,
  org,
  project,
  script,
  result,
  selected,
  onSelect,
  open,
  onToggleOpen,
  onEdit,
  onRun,
  runBlocked,
  hideRun,
  confirmingClear,
  divergences,
  onAskClear,
  onClearDone,
  lastRun,
  highlighted = false,
}: {
  c: { id: number; title: string; steps: { action: string; expected: string; shared?: number | null }[] };
  org: string;
  /** The project the setup is read for. */
  project: string;
  /** The saved script, or null/undefined for a case with none. */
  script: CaseScript | null | undefined;
  result: LastResult;
  selected: boolean;
  onSelect: (id: number) => void;
  open: boolean;
  onToggleOpen: (id: number) => void;
  onEdit: (id: number) => void;
  onRun: (id: number) => void;
  /** Why Run waits, when it does: an unattended run is going, or a
   * discovery holds the Auto Test browser. */
  runBlocked?: string;
  /** Basic mode: no step-by-step run of one case, so no Run button. */
  hideRun?: boolean;
  /** The suspected-defect Clear is waiting on Keep or Clear. */
  confirmingClear: boolean;
  /** The case's reported divergences, open and recently ruled. */
  divergences: readonly Divergence[];
  onAskClear: (id: number) => void;
  onClearDone: () => void;
  /** The run the last result came from, with this case's steps in it. */
  lastRun?: { runId: string; steps?: ReadonlyArray<{ downloads?: string[] }> };
  /** Ringed for a moment: a Waiting for you row or a bell notice opened it. */
  highlighted?: boolean;
}) {
  const bodyId = useId();
  const stepsId = useId();
  const ready = Boolean(script);
  const defect = script?.suspected_defect;
  const attention = divergences.find(needsAttention);
  const files = script ? scriptFiles(script) : { uploads: [], downloads: [] };
  const hasRunDownloads = (lastRun?.steps ?? []).some((s) => (s.downloads ?? []).length > 0);
  const showFiles = files.uploads.length > 0 || files.downloads.length > 0 || hasRunDownloads;
  const notes = script ? assistantNotes(script) : [];
  const autoSteps = script ? doneByAutoRun(script) : new Set<number>();
  const summary = script ? scriptSummary(script) : null;
  // A divergence with a ruling is a ruling row; one still open keeps its buttons.
  const rulings = divergences.filter((d) => d.status !== "open");
  const pending = divergences.filter((d) => d.status === "open");
  // What the script's checks leave out. A passed case says how many beside
  // its result; the open card lists them whatever the result. Read only when
  // one of the two will show.
  const unchecked = script && (open || result === "Passed") ? uncheckedExpectations(script) : [];
  // The open card puts each note under its own step; one with no step of the
  // list (step 0, or a number the script has no step for) stays a short list
  // after the steps.
  const listed = new Set((script?.steps ?? []).map((s) => s.step_number).filter((n) => n >= 1));
  // A ruling shows under its own step; one with no step of the list (or a case
  // with no script) stays a short list at the top, so Undo is always reachable.
  const stepRulings = rulings.filter((d) => listed.has(d.step));
  const orphanRulings = rulings.filter((d) => !listed.has(d.step));
  // What an opened step has to show. A step with none has no chevron.
  const detailsOf = (n: number) => {
    const step = (script?.steps ?? []).find((x) => x.step_number === n);
    if (!step || !listed.has(n)) return null;
    return {
      rulings: stepRulings.filter((d) => d.step === n),
      assumed: stepBecauseParts(step, false),
      invented: stepBecauseParts(step, true),
      notChecked: unchecked.filter((e) => e.step === n),
    };
  };
  const expandable = (script?.steps ?? [])
    .map((s) => s.step_number)
    .filter((n) => {
      const d = detailsOf(n);
      return d && d.rulings.length + d.assumed.length + d.invented.length + d.notChecked.length > 0;
    });
  const [openSteps, setOpenSteps] = useState<ReadonlySet<number>>(
    // Steps start closed; a card with one step opens it.
    () => new Set(script?.steps.length === 1 ? [script.steps[0].step_number] : []),
  );
  // The script may arrive after the card mounts: a one-step script opens its
  // step the first time it shows up, and never again once the person closes it.
  const [seeded, setSeeded] = useState(Boolean(script));
  if (!seeded && script) {
    setSeeded(true);
    if (script.steps.length === 1) setOpenSteps(new Set([script.steps[0].step_number]));
  }
  const toggleStep = (n: number) =>
    setOpenSteps((prev) => {
      const next = new Set(prev);
      if (!next.delete(n)) next.add(n);
      return next;
    });
  const allOpen = expandable.length > 0 && expandable.every((n) => openSteps.has(n));
  const orphanInvented = (script?.steps ?? [])
    .filter((s) => !listed.has(s.step_number))
    .flatMap((s) => stepBecause(s, true).map((t) => `step ${s.step_number}: ${t}`))
    .join("; ");
  // Each step's invented values show in its own opened step; only those of a
  // step the list lacks stay a fact of the whole script.
  const facts = script
    ? [
        ...actionFacts(script).filter((f) => f.label !== "Invented values"),
        ...(orphanInvented ? [{ label: "Invented values", value: orphanInvented }] : []),
      ]
    : [];
  const orphanUnchecked = unchecked.filter((e) => !listed.has(e.step));
  const orphanAssumed = (script?.steps ?? [])
    .filter((s) => !listed.has(s.step_number))
    .flatMap((s) => stepBecause(s, false).map((t) => `step ${s.step_number}: ${t}`));
  // The setup's fixture is read only when a card with a setup opens: a list
  // of many cases reads none until one is looked at.
  const setup = useQuery({
    queryKey: setupViewKey(org, project, c.id),
    queryFn: () => loadSetupView(org, project, c.id),
    enabled: open && Boolean(script?.setup) && Boolean(org && project),
    retry: false,
  });

  return (
    <li
      data-case-id={c.id}
      className={cn(
        // The case a Waiting for you row or a bell notice just opened.
        highlighted && "ring-2 ring-accent",
        // A long list skips painting the cards off screen. An open card
        // drops it: its confirm and its files must paint past its box.
        // The ring is painted outside the box, which skipping would clip.
        !open && !highlighted && "cv-row",
        "rounded-md border border-border bg-surface text-sm",
      )}
      style={{ "--cv-size": "42px" } as CSSProperties}
    >
      <div className="flex items-center gap-2 px-3 py-2">
        <Button
          variant="ghost"
          size="sm"
          className="shrink-0 px-1 py-1"
          aria-label={`${open ? "Hide" : "Show"} details for #${c.id}`}
          aria-expanded={open}
          aria-controls={open ? bodyId : undefined}
          onClick={() => onToggleOpen(c.id)}
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
          onCheckedChange={() => ready && onSelect(c.id)}
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
          onClick={() => onToggleOpen(c.id)}
        >
          {c.title}
        </button>
        {/* The result of the case's last run, in Past runs' words and
            colours. */}
        {/* A collapsed card says it has a suspected defect with an icon
            only, not a button; open, the badge below says it. */}
        {!open && defect && (
          <span
            role="img"
            aria-label={`Suspected defect: ${defect.note}`}
            title={`Suspected defect: ${defect.note}`}
            className="shrink-0 text-warning"
          >
            <IconBug aria-hidden className="size-3.5" />
          </span>
        )}
        {!open && attention && (
          <span
            role="img"
            aria-label={`${badgeText(attention)}: step ${attention.step}`}
            title={`${badgeText(attention)}: step ${attention.step}`}
            className="shrink-0 text-warning"
          >
            <IconWarning aria-hidden className="size-3.5" />
          </span>
        )}
        <span className={cn("shrink-0 text-xs font-medium", bucketTone[result])}>
          <span className="sr-only">Last result: </span>
          {result}
        </span>
        {result === "Passed" && unchecked.length > 0 && (
          <span className="shrink-0 text-xs text-warning">{uncheckedWords(unchecked.length)}</span>
        )}
      </div>

      {open && (
        <div id={bodyId} className="space-y-3 border-t border-border/60 px-3 pb-3 pt-2">
          {pending.length > 0 && (
            <div className="space-y-1">
              {pending.map((d) => (
                <DivergenceRow key={d.id} org={org} project={project} caseId={c.id} divergence={d} />
              ))}
            </div>
          )}
          {orphanRulings.length > 0 && (
            <div className="space-y-1">
              <SectionLabel>{`Rulings (${orphanRulings.length})`}</SectionLabel>
              {orphanRulings.map((d) => (
                <DivergenceRow key={d.id} org={org} project={project} caseId={c.id} divergence={d} compact />
              ))}
            </div>
          )}
          {!script ? (
            <p className="text-xs text-muted">No script yet. An assistant writes it.</p>
          ) : (
            <>
              <div className="space-y-1">
                {summary && (
                  <div role="group" aria-label={`Summary of #${c.id}`} className="flex flex-wrap items-center gap-1.5">
                    <SummaryChip icon={<IconScriptSteps aria-hidden />}>{summary.steps}</SummaryChip>
                    {summary.account && <SummaryChip icon={<IconRunsAs aria-hidden />}>{`Runs as ${summary.account}`}</SummaryChip>}
                    {summary.area && <SummaryChip icon={<IconScriptArea aria-hidden />}>{summary.area}</SummaryChip>}
                    {rulings.length > 0 && (
                      <SummaryChip tone="accent" icon={<IconRuling aria-hidden />}>
                        {plural(rulings.length, "ruling")}
                      </SummaryChip>
                    )}
                    {summary.noSave && (
                      <SummaryChip tone="warning" icon={<IconMustNotSave aria-hidden />}>
                        Must not save
                      </SummaryChip>
                    )}
                  </div>
                )}
                <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-3 gap-y-0.5 text-xs">
                  {facts.map(({ label, value }) => (
                    <div key={label} className="contents">
                      <dt className="text-muted">{label}</dt>
                      <dd className="break-words text-text">{value}</dd>
                    </div>
                  ))}
                </dl>
                {script.setup && setup.data && (
                  <p className="text-xs text-text">
                    <span className="text-muted">Setup: </span>
                    {setup.data.fixture_name}
                  </p>
                )}
                {defect && (
                  <div className="flex flex-wrap items-center gap-2 pt-1">
                    <SuspectedDefectBadge caseId={c.id} defect={defect} onClear={() => onAskClear(c.id)} />
                    <span className="min-w-0 flex-1 break-words text-xs text-text">
                      Step {defect.step_number}: {defect.note}
                    </span>
                    {confirmingClear && <ClearConfirm caseId={c.id} onDone={onClearDone} />}
                  </div>
                )}
                {!defect && script.cleared_defect && <ClearedDefectNote cleared={script.cleared_defect} />}
              </div>

              <div className="space-y-1">
                <SectionLabel>Steps</SectionLabel>
                {script.steps.length === 0 ? (
                  <p className="text-xs text-muted">The script has no steps yet.</p>
                ) : (
                  <ol id={stepsId} aria-label={`Script steps of #${c.id}`} className="space-y-0.5 text-xs">
                    {script.steps.map((s) => {
                      // The case's own words for the step, as the script
                      // editor lists them.
                      const own = c.steps[s.step_number - 1];
                      const d = detailsOf(s.step_number);
                      const folds = expandable.includes(s.step_number);
                      const isOpen = folds && openSteps.has(s.step_number);
                      const panelId = `${stepsId}-step-${s.step_number}`;
                      const actionsText = autoSteps.has(s.step_number) ? "Done by Auto Test" : plural(s.actions.length, "action");
                      return (
                        <li key={s.step_number} className="min-w-0">
                          <div className="flex min-w-0 items-center gap-2">
                            {/* A step with nothing to show has no chevron; its
                                action count stays on the row, since it has no
                                opened step to sit in. */}
                            {folds ? (
                              <Button
                                variant="ghost"
                                size="sm"
                                className="size-5 shrink-0 p-0"
                                aria-label={`${isOpen ? "Hide" : "Show"} step ${s.step_number} of #${c.id}`}
                                aria-expanded={isOpen}
                                aria-controls={isOpen ? panelId : undefined}
                                onClick={() => toggleStep(s.step_number)}
                              >
                                {isOpen ? <IconHideDetails aria-hidden /> : <IconShowDetails aria-hidden />}
                              </Button>
                            ) : (
                              <span aria-hidden className="size-5 shrink-0" />
                            )}
                            <span className="shrink-0 text-faint">{s.step_number}.</span>
                            <span className="min-w-0 flex-1 truncate text-text" title={own ? caseTextLine(own.action) : undefined}>
                              {own?.shared != null ? <SharedStepLabel id={own.shared} org={org} /> : own && caseTextLine(own.action)}
                            </span>
                            {d && d.rulings.some((r) => r.status === "awaiting_repair") ? (
                              <NoteTag tone="warning">Waits for a repair</NoteTag>
                            ) : (
                              d && d.rulings.length > 0 && <NoteTag tone="accent">Ruled</NoteTag>
                            )}
                            {d && d.assumed.length > 0 && <NoteTag tone="neutral">{`${d.assumed.length} assumed`}</NoteTag>}
                            {d && d.invented.length > 0 && <NoteTag tone="neutral">{`${d.invented.length} invented`}</NoteTag>}
                            {d && d.notChecked.length > 0 && <NoteTag tone="warning">{`${d.notChecked.length} unchecked`}</NoteTag>}
                            {!folds && <span className="shrink-0 text-faint">{actionsText}</span>}
                          </div>
                          {isOpen && d && (
                            <div id={panelId} className="ml-12 space-y-1.5 py-1">
                              {d.rulings.length > 0 && (
                                <div role="group" aria-label={`Ruling in step ${s.step_number}`}>
                                  {d.rulings.map((r) => (
                                    <DivergenceRow key={r.id} org={org} project={project} caseId={c.id} divergence={r} compact tag="Ruling" />
                                  ))}
                                </div>
                              )}
                              {d.assumed.length > 0 && (
                                <BecauseGrid label={`Assumed in step ${s.step_number}`} tag="Assumed" entries={d.assumed} />
                              )}
                              {d.invented.length > 0 && (
                                <BecauseGrid label={`Invented in step ${s.step_number}`} tag="Invented" entries={d.invented} />
                              )}
                              {d.notChecked.length > 0 && (
                                <div role="group" aria-label={`Not checked in step ${s.step_number}`} className="space-y-0.5">
                                  {d.notChecked.map((e, i) => (
                                    <NoteLine key={i} tone="warning" tag="Not checked">
                                      {uncheckedDetail(e)}
                                    </NoteLine>
                                  ))}
                                </div>
                              )}
                              <p className="text-xs text-faint">{actionsText}</p>
                            </div>
                          )}
                        </li>
                      );
                    })}
                  </ol>
                )}
              </div>

              {orphanAssumed.length > 0 && (
                <div className="space-y-1">
                  <SectionLabel>Assumed values</SectionLabel>
                  <ul aria-label={`Assumed values of #${c.id}`} className="space-y-0.5 text-xs text-text">
                    {orphanAssumed.map((t, i) => (
                      <li key={i} className="break-words">
                        {t}
                      </li>
                    ))}
                  </ul>
                </div>
              )}

              {orphanUnchecked.length > 0 && (
                <div className="space-y-1">
                  <SectionLabel>Not checked</SectionLabel>
                  <ul aria-label={`Not checked expectations of #${c.id}`} className="space-y-0.5 text-xs text-text">
                    {orphanUnchecked.map((e, i) => (
                      <li key={i} className="break-words">
                        {uncheckedLine(e)}
                      </li>
                    ))}
                  </ul>
                </div>
              )}

              {(script.unfired_guards ?? []).length > 0 && (
                <div className="space-y-1">
                  <SectionLabel>Clean-up guard never fired</SectionLabel>
                  <ul aria-label={`Clean-up guards that never fired of #${c.id}`} className="space-y-0.5 text-xs text-text">
                    {(script.unfired_guards ?? []).map((g) => (
                      <li key={g} className="break-words">
                        {g}
                      </li>
                    ))}
                  </ul>
                </div>
              )}

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

              {(notes.length > 0 || expandable.length > 0) && (
                <div className="flex flex-wrap items-start justify-between gap-2">
                  {notes.length > 0 ? (
                    <details className="min-w-0 flex-1 text-xs">
                      <summary className="cursor-pointer text-muted">Assistant notes ({notes.length})</summary>
                      <dl className="mt-1 grid grid-cols-[max-content_minmax(0,1fr)] gap-x-3 gap-y-0.5">
                        {notes.map(({ label, value }) => (
                          <div key={label} className="contents">
                            <dt className="text-muted">{label}</dt>
                            <dd className="break-words text-text">{value}</dd>
                          </div>
                        ))}
                      </dl>
                    </details>
                  ) : (
                    <span />
                  )}
                  {expandable.length > 0 && (
                    <Button
                      size="sm"
                      variant="ghost"
                      className="shrink-0"
                      aria-expanded={allOpen}
                      aria-controls={stepsId}
                      onClick={() => setOpenSteps(allOpen ? new Set() : new Set(expandable))}
                    >
                      {allOpen ? <IconCollapseAll aria-hidden /> : <IconExpandAll aria-hidden />}
                      {allOpen ? "Collapse all steps" : "Expand all steps"}
                    </Button>
                  )}
                </div>
              )}
            </>
          )}

          {/* No "Script ready" badge: the card says it with its buttons. A
              scripted case has View script and a Run button, outlined in the
              success colour; a case with no script has neither, because a
              person never writes one by hand. Both stay outline buttons: the
              one primary action on the screen is running the selection, in
              the dock. */}
          {ready && (
            <div className="flex flex-wrap items-center gap-2">
              <Button
                size="sm"
                variant="outline"
                aria-label={`View script for #${c.id}`}
                onClick={() => onEdit(c.id)}
              >
                <IconViewScript aria-hidden />
                View script
              </Button>
              {!hideRun && (
                <Button
                  size="sm"
                  variant="outline"
                  className="border-success text-success hover:border-success hover:bg-success/10 hover:text-success disabled:pointer-events-auto"
                  aria-label={`Run #${c.id}`}
                  // One run at a time: while an unattended run is going, or a
                  // discovery holds the browser, this waits, and its title says why.
                  disabled={Boolean(runBlocked)}
                  title={runBlocked}
                  onClick={() => onRun(c.id)}
                >
                  <IconRun aria-hidden />
                  Run
                </Button>
              )}
            </div>
          )}
        </div>
      )}
    </li>
  );
}
