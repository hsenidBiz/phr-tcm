import type { MutableRefObject } from "react";
import { OFFLINE_HINT } from "../../lib/network";
import { submitLabel, type SubmitPhase } from "../../lib/submitRun";
import { IconBack, IconConfirm, IconReview } from "../../lib/actionIcons";
import ActionDock from "../ActionDock";
import { Button } from "../ui/button";

/** The Review / Confirm row under the queue, and its floating copy. */
export default function QueueActionBar({
  pbiId,
  queueLength,
  progress,
  reviewing,
  armed,
  arm,
  setReviewing,
  holdActive,
  hasBlockers,
  submitPending,
  online,
  pureUpdates,
  checkingDups,
  dupsPending,
  setAcceptedDups,
  actionLabel,
  actionRowEl,
  onOpenReview,
  onSubmit,
}: {
  pbiId: number;
  queueLength: number;
  progress: SubmitPhase;
  reviewing: boolean;
  armed: boolean;
  arm: (on: boolean) => void;
  setReviewing: (on: boolean) => void;
  holdActive: boolean;
  hasBlockers: boolean;
  submitPending: boolean;
  online: boolean;
  pureUpdates: boolean;
  checkingDups: boolean;
  dupsPending: string[];
  setAcceptedDups: (update: (prev: string[]) => string[]) => void;
  actionLabel: string;
  /** The row, held as a plain node so it can be scrolled to. */
  actionRowEl: MutableRefObject<HTMLDivElement | null>;
  onOpenReview: () => void;
  onSubmit: () => void;
}) {
  return (
  <ActionDock
    label="Queue actions"
    // The tour rings this row, not the Review button inside it: the
    // row is what holds Review before review and the confirm/upload
    // button during it, so the stop makes sense either way.
    rowProps={{ "data-tour": "queue-review" }}
    rowRef={(el) => {
      actionRowEl.current = el;
    }}
    // The dock's default row is right-aligned for the standard case;
    // this row has always read left-to-right instead, and Import Test Cases
    // has to look unchanged.
    className="justify-start gap-3"
    // The floating copy stands down for the armed warning and the
    // duplicate gate on purpose, since those have to be read - the row
    // comes to the reader for those instead (see the scroll effect in
    // QueueSection.tsx).
    active={queueLength > 0 && !armed && dupsPending.length === 0}
  >
    {(floating) =>
      progress ? (
        <Button tabIndex={floating ? -1 : undefined} disabled>
          <IconConfirm aria-hidden />
          {submitLabel(progress)}
        </Button>
      ) : !reviewing ? (
        <Button
          tabIndex={floating ? -1 : undefined}
          disabled={!floating && queueLength === 0}
          onClick={onOpenReview}
        >
          <IconReview aria-hidden />
          Review {queueLength} test case{queueLength === 1 ? "" : "s"}
        </Button>
      ) : floating ? (
        // Simplified on purpose: the floating copy skips Back, the
        // blockers hint and the duplicate/armed panels - those need
        // the full row, which the scroll effect in QueueSection.tsx brings back.
        // Arms rather than submitting outright unless every row is a
        // pure update - a floating corner button is an easy accident,
        // and creating something needs the fuller row read first.
        <Button
          tabIndex={-1}
          disabled={holdActive || hasBlockers || submitPending || !online}
          title={online ? undefined : OFFLINE_HINT}
          onClick={() => (pureUpdates ? onSubmit() : arm(true))}
        >
          <IconConfirm aria-hidden />
          {submitPending
            ? progress
              ? submitLabel(progress)
              : "Checking"
            : `Confirm & ${actionLabel || "create 0"}`}
        </Button>
      ) : (
        <div className="w-full space-y-2">
          {/* The duplicate warning sits ABOVE the way on rather than
              replacing it, and the button below is disabled while it
              stands - the per-row hint it replaced was scrollable-past,
              and 43 duplicates once sailed through that. */}
          {dupsPending.length > 0 && (
            <div className="space-y-2 rounded-md border border-danger/50 bg-danger/10 p-3">
              <p className="text-sm font-semibold text-text">
                Stopped: {dupsPending.length} case{dupsPending.length === 1 ? "" : "s"} with the
                same title already exist{dupsPending.length === 1 ? "s" : ""} on PBI #{pbiId}.
              </p>
              <ul className="max-h-32 space-y-0.5 overflow-y-auto text-xs text-muted">
                {dupsPending.map((t) => (
                  <li key={t}>• {t}</li>
                ))}
              </ul>
              <p className="text-xs text-muted">
                If you meant to update the existing cases, import a file that includes their
                ids (View Test Cases → Export JSON has them). Creating anyway makes
                duplicates - and cleaning those up needs delete permission.
              </p>
              <div className="flex items-center gap-2">
                <Button
                  size="sm"
                  onClick={() => {
                    // Backing out here abandons the armed confirmation
                    // too - the chip must stop glowing, same as Back.
                    arm(false);
                    setReviewing(false);
                  }}
                >
                  <IconBack aria-hidden />
                  Go back
                </Button>
                <Button
                  variant="danger"
                  size="sm"
                  disabled={submitPending || !online}
                  // Accepting does NOT write. It records the choice and
                  // frees the button, so the last click is still the one
                  // that creates and still says so.
                  onClick={() => setAcceptedDups((prev) => [...prev, ...dupsPending])}
                >
                  Create duplicates anyway
                </Button>
              </div>
            </div>
          )}
          {armed && (
            <div className="rounded-md border border-warning/50 bg-warning/10 p-3">
              <p className="text-sm text-text">
                Check the highlighted PBI above — everything here will be written to{" "}
                <span className="font-semibold">PBI #{pbiId}</span>. Removing them afterwards{" "}
                <span className="font-semibold">needs delete permission</span> in Azure DevOps,
                and deleting a test case in Azure DevOps is permanent.
              </p>
            </div>
          )}
          <div className="flex items-center gap-2">
            <Button
              disabled={
                queueLength === 0 ||
                holdActive ||
                hasBlockers ||
                submitPending ||
                !online ||
                // While the check is in flight, and while a duplicate is
                // waiting to be looked at.
                checkingDups ||
                dupsPending.length > 0
              }
              title={online ? undefined : OFFLINE_HINT}
              onClick={() => onSubmit()}
            >
              <IconConfirm aria-hidden />
              {submitPending
                ? progress
                  ? submitLabel(progress)
                  : "Checking"
                : armed
                  ? `Yes — ${actionLabel}`
                  : `Confirm & ${actionLabel || "create 0"}`}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                arm(false);
                setReviewing(false);
              }}
            >
              <IconBack aria-hidden />
              Back
            </Button>
            {hasBlockers && (
              <span className="text-xs text-danger">Fix the flagged items first.</span>
            )}
            {holdActive && (
              <span className="text-xs text-warning">Check the cases marked "Outcome unknown" first.</span>
            )}
          </div>
        </div>
      )
    }
  </ActionDock>
  );
}
