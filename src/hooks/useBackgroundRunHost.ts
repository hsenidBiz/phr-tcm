// What the app does around an unattended run that outlives the Auto Run
// screen (lib/backgroundRun). Used once, by App, where something is always
// mounted; a hook of its own so a test can drive it without the whole app.

import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef } from "react";
import {
  onRunEnded,
  openRunWindow,
  reviewFinishedRun,
  useBackgroundRun,
  type ReviewRequest,
} from "../lib/backgroundRun";
import { toast } from "../lib/toast";

/**
 * - At a run's end, the runs and scripts it changed are read again. A run
 *   that ended while its window was closed says so in a toast rather than
 *   opening anything over the person's work; the title-bar pill says the
 *   same until it is seen.
 * - A finished run's review is opened by the Auto Run screen, for the run's
 *   own PBI: `goToReview` takes the person there (from any section, or
 *   another PBI), and the screen opens it once it has the PBI. It is
 *   called once per request, so the person's later moves never bounce
 *   back.
 */
export function useBackgroundRunHost(goToReview: (r: ReviewRequest) => void): void {
  const qc = useQueryClient();
  /** The toast a background end raised, while it still means something. */
  const endToast = useRef<string | null>(null);
  useEffect(
    () =>
      onRunEnded((e) => {
        void qc.invalidateQueries({ queryKey: ["autorun-runs"] });
        // A run that passes a marked step clears the mark on disk; the rows
        // read each script through their own query.
        void qc.invalidateQueries({ queryKey: ["autorun-script"] });
        if (!e.inBackground) return;
        if (e.ok) {
          endToast.current = toast.success("The unattended run finished", {
            description: "Its cases are waiting for your review.",
            duration: 10_000,
            action: { label: "Review", onClick: reviewFinishedRun },
          });
        } else {
          endToast.current = toast.error("The unattended run stopped with an error", {
            duration: 10_000,
            action: { label: "Show", onClick: openRunWindow },
          });
        }
      }),
    [qc],
  );

  const { run, review } = useBackgroundRun();
  // Seen through the pill instead (its window open, or the run handed to
  // its review): the toast's own button would now do nothing, so it goes.
  const seen = !run || run.open;
  useEffect(() => {
    if (seen && endToast.current) {
      toast.dismiss(endToast.current);
      endToast.current = null;
    }
  }, [seen]);
  // The latest callback, read when a request arrives: it closes over App's
  // section and scope, which must not re-run the move when they change.
  const goTo = useRef(goToReview);
  goTo.current = goToReview;
  useEffect(() => {
    if (review) goTo.current(review);
  }, [review]);
}
