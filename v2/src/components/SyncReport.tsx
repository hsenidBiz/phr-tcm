// What the watched file just did to the queue, shown the way a diff is:
// counts at a glance, per-case detail on demand, colour by kind.
//
// Temporary by design - the caller drops it after a while, because the
// queue itself is the durable record. It stays put while it is open so a
// report cannot vanish mid-read.

import { FilePlus2, FileMinus2, FilePen, X } from "lucide-react";
import { useEffect, useState } from "react";
import { cn } from "../lib/cn";
import { countBy, type SyncChange } from "../lib/fileSync";

/** Long enough to notice and read the counts, short enough that the panel
 * doesn't become permanent furniture. */
const LINGER_MS = 45_000;

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

  // Expiry lives here, not in the caller, because only this component
  // knows whether the detail list is open - and pulling a report out from
  // under someone who is reading it would be the whole point missed.
  // `onDismiss` must be stable or the timer restarts every render.
  useEffect(() => {
    if (open || (changes.length === 0 && warnings === 0)) return;
    const t = setTimeout(onDismiss, LINGER_MS);
    return () => clearTimeout(t);
  }, [open, changes, warnings, onDismiss]);

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
        <ul className="mt-2 space-y-1 border-t border-accent/20 pt-2">
          {changes.map((c) => {
            const { icon: Icon, tone } = KIND[c.kind];
            return (
              <li key={`${c.kind}:${c.key}`} className="flex items-start gap-2 text-xs">
                <Icon size={13} className={cn("mt-0.5 shrink-0", tone)} />
                <span className="min-w-0 flex-1 break-words text-text">
                  {c.title}
                  {c.fields.length > 0 && (
                    <span className="text-muted"> — {c.fields.join(", ")}</span>
                  )}
                </span>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
