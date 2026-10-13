// The pause before a batch (design §3, run batches): the settings to set,
// what to put back and which cases changed it, what is still to run, and
// Continue or Stop. The unattended run and the supervised pane show the
// same panel.

import type { Pause } from "../../bindings";
import { Button } from "../../components/ui/button";
import { IconContinue, IconStop } from "../../lib/actionIcons";
import { caseLabel, pauseHeading, resetLines, setLines } from "./plan";

export default function ResetNeededPanel({
  reset,
  remaining,
  titleOf,
  busy = false,
  onContinue,
  onStop,
}: {
  /** The batch it opens, the settings to set, the names to revert, and the
   * cases that changed each. */
  reset: Pick<Pause, "names" | "changed_by"> & Partial<Pick<Pause, "batch" | "set">>;
  /** Every case still to run, the next one first. */
  remaining: number[];
  titleOf: (id: number) => string | undefined;
  /** An answer is on its way: both buttons wait for it. */
  busy?: boolean;
  onContinue: () => void;
  onStop: () => void;
}) {
  const heading = pauseHeading(reset.batch);
  const set = setLines({ batch: reset.batch ?? 0, set: reset.set ?? [] });
  const back = resetLines(reset, titleOf);
  return (
    <section aria-label={heading} className="space-y-2 rounded-md border border-warning/40 bg-surface-2 p-3 text-xs">
      <h3 className="text-sm font-semibold text-warning">{heading}</h3>
      <p className="text-muted">Do these before the next case runs, then press Continue.</p>
      {set.length > 0 && (
        <div>
          <p className="font-medium text-muted">Set</p>
          <ul aria-label="Set" className="mt-1 space-y-1">
            {set.map((line) => (
              <li key={line} className="border-l-2 border-l-warning pl-2 text-text">
                {line}
              </li>
            ))}
          </ul>
        </div>
      )}
      {back.length > 0 && (
        <div>
          <p className="font-medium text-muted">Put back</p>
          <ul aria-label="Put back" className="mt-1 space-y-1">
            {back.map((line) => (
              <li key={line} className="border-l-2 border-l-warning pl-2 text-text">
                {line}
              </li>
            ))}
          </ul>
        </div>
      )}
      <div>
        <p className="font-medium text-muted">Still to run</p>
        <ul aria-label="Cases still to run" className="mt-1 space-y-0.5">
          {remaining.map((id) => (
            <li key={id} className="text-text">
              {caseLabel(id, titleOf)}
            </li>
          ))}
        </ul>
      </div>
      <div className="flex justify-end gap-2">
        <Button size="sm" variant="outline" aria-label="Stop the run at this pause" disabled={busy} onClick={onStop}>
          <IconStop aria-hidden />
          Stop
        </Button>
        <Button size="sm" aria-label="Continue the run" disabled={busy} onClick={onContinue}>
          <IconContinue aria-hidden />
          Continue
        </Button>
      </div>
    </section>
  );
}
