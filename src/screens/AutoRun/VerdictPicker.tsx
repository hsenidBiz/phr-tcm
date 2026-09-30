// The row of verdict buttons a person presses to decide a case - one
// component for the supervised pane (RunPane) and the review dialog
// (RunReview), so the two can never drift apart in look or behaviour.
//
// Each verdict is a toggle button (`aria-pressed`) named by the verdict
// alone ("Passed", "Failed", "Blocked"); the row is a labelled group so a
// screen reader hears which case the three belong to. What a press MEANS is
// the caller's: the supervised pane sets the verdict, the review dialog
// also lets a second press on the same one clear it.

import { cn } from "../../lib/cn";
import { VERDICTS, verdictTone } from "./verdicts";

export default function VerdictPicker({
  value,
  onPick,
  label,
  disabled,
}: {
  /** The verdict currently picked, or "" for none. */
  value: string;
  onPick: (verdict: (typeof VERDICTS)[number]) => void;
  /** Names the group, e.g. "Your verdict" or "Verdict for #201". */
  label: string;
  disabled?: boolean;
}) {
  return (
    <div role="group" aria-label={label} className="flex gap-2">
      {VERDICTS.map((v) => {
        const pressed = value === v;
        return (
          <button
            key={v}
            type="button"
            aria-pressed={pressed}
            disabled={disabled}
            className={cn(
              "rounded-md border border-border px-3 py-1.5 text-xs font-medium transition-colors",
              pressed ? verdictTone[v] : "text-muted hover:border-border-strong",
            )}
            onClick={() => onPick(v)}
          >
            {v}
          </button>
        );
      })}
    </div>
  );
}
