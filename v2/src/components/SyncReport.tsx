// What the watched file just did to the queue, shown the way a diff is:
// counts at a glance, per-case detail on demand, colour by kind - and for
// a changed case, the words that actually changed.
//
// It stays until it is dismissed. It used to expire on a timer, on the
// reasoning that the queue is the durable record; but the report is the
// only place the EDIT is visible at all - once it goes, the only way to
// see what an assistant changed is to diff the file yourself. Closing it
// is now a decision, not a timeout.

import { FilePlus2, FileMinus2, FilePen, X } from "lucide-react";
import { useState } from "react";
import { cn } from "../lib/cn";
import { countBy, type SyncChange } from "../lib/fileSync";
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
  onDismiss,
}: {
  changes: SyncChange[];
  fileName: string;
  /** Problems the importer found in the new contents. Surfaced here so a
   * bad save is caught the moment it happens - no separate check, and no
   * AI round-trip to ask whether the draft is valid. */
  warnings?: number;
  onDismiss: () => void;
}) {
  const [open, setOpen] = useState(false);

  if (changes.length === 0 && warnings === 0) return null;

  const counts = (["added", "changed", "removed"] as const)
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
        <span className="font-medium text-text">Updated from {fileName}</span>
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
            return (
              <li key={`${c.kind}:${c.key}`} className="flex items-start gap-2 text-xs">
                <Icon size={13} className={cn("mt-0.5 shrink-0", tone)} />
                <div className="min-w-0 flex-1 space-y-1">
                  <span className="break-words text-text">{c.title}</span>
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
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
