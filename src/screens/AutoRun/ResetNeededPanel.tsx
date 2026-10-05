// The pause at a reset point (design §3): what to put back, which cases
// changed it, what is still to run, and Continue or Stop. The unattended
// run and the supervised pane show the same panel.

import type { Reset } from "../../bindings";
import { Button } from "../../components/ui/button";
import { IconContinue, IconStop } from "../../lib/actionIcons";
import { caseLabel, resetLines } from "./plan";

export default function ResetNeededPanel({
  reset,
  remaining,
  titleOf,
  busy = false,
  onContinue,
  onStop,
}: {
  /** The names to revert, and the cases that changed each. */
  reset: Pick<Reset, "names" | "changed_by">;
  /** Every case still to run, the next one first. */
  remaining: number[];
  titleOf: (id: number) => string | undefined;
  /** An answer is on its way: both buttons wait for it. */
  busy?: boolean;
  onContinue: () => void;
  onStop: () => void;
}) {
  return (
    <section
      aria-label="Reset needed"
      className="space-y-2 rounded-md border border-warning/40 bg-surface-2 p-3 text-xs"
    >
      <h3 className="text-sm font-semibold text-warning">Reset needed</h3>
      <p className="text-muted">Put these back before the next case runs, then press Continue.</p>
      <ul className="space-y-1">
        {resetLines({ before_case_id: remaining[0] ?? 0, ...reset }, titleOf).map((line) => (
          <li key={line} className="border-l-2 border-l-warning pl-2 text-text">
            {line}
          </li>
        ))}
      </ul>
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
        <Button size="sm" variant="outline" aria-label="Stop the run at this reset" disabled={busy} onClick={onStop}>
          <IconStop aria-hidden />
          Stop
        </Button>
        <Button size="sm" aria-label="Continue after reset" disabled={busy} onClick={onContinue}>
          <IconContinue aria-hidden />
          Continue
        </Button>
      </div>
    </section>
  );
}
