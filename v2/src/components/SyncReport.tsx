// What the watched file just did to the queue, shown the way a diff is:
// counts at a glance, per-case detail on demand, colour by kind - and for
// a changed case, the words that actually changed.
//
// It stays until it is dismissed. It used to expire on a timer, on the
// reasoning that the queue is the durable record; but the report is the
// only place the EDIT is visible at all - once it goes, the only way to
// see what an assistant changed is to diff the file yourself. Closing it
// is now a decision, not a timeout.

import { ChevronDown, ChevronRight, FilePlus2, FileMinus2, FilePen, X } from "lucide-react";
import { useState } from "react";
import { cn } from "../lib/cn";
import { countBy, type SyncChange } from "../lib/fileSync";
import CaseStepsTable from "./CaseStepsTable";
import InlineDiff from "./InlineDiff";
import StepDiffLines from "./StepDiffLines";

const KIND = {
  added: { icon: FilePlus2, tone: "text-success", sign: "+", word: "added" },
  changed: { icon: FilePen, tone: "text-warning", sign: "~", word: "changed" },
  removed: { icon: FileMinus2, tone: "text-danger", sign: "−", word: "removed" },
} as const;

export default function SyncReport({
  changes,
  fileName,
  warnings = 0,
  intoEmptyQueue = false,
  onDismiss,
}: {
  changes: SyncChange[];
  fileName: string;
  /** Problems the importer found in the new contents. Surfaced here so a
   * bad save is caught the moment it happens - no separate check, and no
   * AI round-trip to ask whether the draft is valid. */
  warnings?: number;
  /** The sync landed in an EMPTY queue - a load/refill, not an edit. Every
   * case counts as "added" then, and "+157 added" reads like 157 test
   * cases were just created when nothing of the sort happened - so the
   * banner says "loaded" instead (field report, 2026-08-25). */
  intoEmptyQueue?: boolean;
  onDismiss: () => void;
}) {
  const [open, setOpen] = useState(false);
  // Which cases have their full step list showing. Per case, like the
  // review list: a report covering twenty cases should not become twenty
  // step tables because you wanted to read one of them.
  const [openCases, setOpenCases] = useState<Set<string>>(new Set());
  const toggleCase = (id: string) =>
    setOpenCases((prev) => {
      const next = new Set(prev);
      if (!next.delete(id)) next.add(id);
      return next;
    });

  if (changes.length === 0 && warnings === 0) return null;

  // A pure load (empty queue, nothing but "added") drops the +/-/~ chips:
  // the headline already says everything, and "+157 added" is the part
  // that misreads as "157 created".
  const pureLoad = intoEmptyQueue && changes.length > 0 && changes.every((c) => c.kind === "added");
  const counts = pureLoad
    ? []
    : (["added", "changed", "removed"] as const)
        .map((k) => ({ k, n: countBy(changes, k) }))
        .filter((c) => c.n > 0);

  return (
    <div
      // Announced politely: the sync is unprompted, so a screen reader
      // should hear it without losing the user's place.
      role="status"
      aria-live="polite"
      className="rounded-md border border-accent/40 bg-accent-soft/40 px-3 py-2 text-sm"
    >
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        <span className="font-medium text-text">
          {pureLoad
            ? `Loaded ${changes.length} case${changes.length === 1 ? "" : "s"} from ${fileName} into the queue`
            : `Updated from ${fileName}`}
        </span>
        {counts.map(({ k, n }) => (
          <span key={k} className={cn("id-mono text-xs", KIND[k].tone)}>
            {KIND[k].sign}
            {n} {KIND[k].word}
          </span>
        ))}
        {warnings > 0 && (
          <span className="id-mono text-xs text-warning">
            ⚠ {warnings} warning{warnings === 1 ? "" : "s"}
          </span>
        )}
        <button
          className="ml-auto text-xs text-muted underline-offset-2 hover:text-accent hover:underline"
          aria-expanded={open}
          onClick={() => setOpen((o) => !o)}
        >
          {open ? "Hide details" : "Show details"}
        </button>
        <button
          aria-label="Dismiss the change report"
          className="rounded p-0.5 text-muted hover:text-text"
          onClick={onDismiss}
        >
          <X size={14} />
        </button>
      </div>

      {open && (
        <ul className="mt-2 space-y-2 border-t border-accent/20 pt-2">
          {changes.map((c) => {
            const { icon: Icon, tone } = KIND[c.kind];
            const id = `${c.kind}:${c.key}`;
            const showing = openCases.has(id);
            return (
              <li key={id} className="flex items-start gap-2 text-xs">
                <Icon size={13} className={cn("mt-0.5 shrink-0", tone)} />
                <div className="min-w-0 flex-1 space-y-1">
                  {/* The case's own toggle, next to its title - the same
                      affordance the review list has, for the same reason.
                      The diff below says what moved; this says what the
                      case now IS, which is the only way to judge whether a
                      step still follows from the one before it. */}
                  <button
                    className="flex w-full items-start gap-1 text-left hover:text-accent"
                    aria-expanded={showing}
                    onClick={() => toggleCase(id)}
                  >
                    {showing ? (
                      <ChevronDown size={12} className="mt-0.5 shrink-0 text-muted" />
                    ) : (
                      <ChevronRight size={12} className="mt-0.5 shrink-0 text-muted" />
                    )}
                    <span className="min-w-0 break-words text-text">{c.title}</span>
                  </button>
                  {/* The point of the panel: not "Title changed" but the
                      words that changed. Same InlineDiff and StepDiffLines
                      the submit review uses, so a file edit and a pending
                      update read identically. */}
                  {c.fields.map((f) => (
                    <div key={f.name} className="flex flex-wrap gap-x-2 break-words">
                      <span className="shrink-0 text-faint">{f.name}</span>
                      <span className="min-w-0 whitespace-pre-wrap text-text">
                        <InlineDiff old={f.old} next={f.new} emptyLabel="(empty)" />
                      </span>
                    </div>
                  ))}
                  {c.steps.length > 0 && (
                    <div className="space-y-0.5">
                      {c.steps.map((d) => (
                        <StepDiffLines key={`${d.kind}:${d.index}`} d={d} />
                      ))}
                    </div>
                  )}
                  {showing && (
                    <div className="overflow-hidden rounded border border-border/60">
                      <CaseStepsTable
                        steps={c.full.steps}
                        preconditions={c.full.preconditions}
                        reviewerNotes={c.full.reviewer_notes}
                      />
                    </div>
                  )}
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
