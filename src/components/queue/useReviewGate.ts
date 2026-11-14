import { useQuery } from "@tanstack/react-query";
import { useCallback, useEffect, useMemo, useState } from "react";
import { commands, type TestCase } from "../../bindings";
import { setPbiGlow } from "../../lib/pbiGlow";
import { unwrap } from "../../lib/ipc";

/**
 * The review stage before an upload: the first Confirm click arms the
 * submit and spotlights the PBI chip, and a fresh check against Azure
 * DevOps stops duplicate titles before anything is written.
 */
export function useReviewGate({
  org,
  pbiId,
  queue,
}: {
  org: string;
  pbiId: number;
  queue: TestCase[];
}) {
  const [reviewing, setReviewing] = useState(false);

  // Final confirmation stage: the first Confirm click arms the submit and
  // spotlights the PBI chip; only the explicit second click writes.
  //
  // Skipped for a queue that is NOTHING but updates: an update PATCHes its
  // own work item where it already lives and never reads the selected PBI
  // (only creates link to it, take its area/iteration, and populate its
  // suite). The check-the-PBI stage exists to stop creates landing under
  // the wrong PBI - for pure updates it is friction with nothing to catch.
  const pureUpdates = queue.length > 0 && queue.every((tc) => tc.update_id != null);
  const [armed, setArmed] = useState(false);
  // The last safeguard before anything is written: titles of the CREATE
  // rows that already exist on the PBI, checked FRESH against ADO at the
  // final click. Set = the submit is stopped until the user explicitly
  // chooses. The per-row hint was scrollable-past; 43 duplicates once
  // sailed through it.
  //
  // Checked when REVIEW OPENS rather than at the final click. The review
  // already showed a per-row duplicate hint, but computed off a cache the
  // comment above admits can be minutes old - so it warned using data it
  // did not trust and then re-asked properly at the worst moment, after a
  // button that said it would create. Fetching once up front makes the
  // hints true and puts the wait where someone is reading.
  //
  // Titles as the fetch last saw them; null until it has answered.
  const [freshTitles, setFreshTitles] = useState<string[] | null>(null);
  const [checkingDups, setCheckingDups] = useState(false);
  // Titles the user has looked at and chosen to duplicate anyway.
  const [acceptedDups, setAcceptedDups] = useState<string[]>([]);
  const arm = (on: boolean) => {
    setArmed(on);
    setPbiGlow(on);
  };
  // Never leave the chip glowing if this screen unmounts mid-confirmation.
  useEffect(() => () => setPbiGlow(false), []);

  // An emptied queue (Clear queue, removing the last item) has nothing to
  // review - leave review mode so the confirm controls disappear too.
  //
  // The duplicate answer needs no clearing here: `dupsPending` is derived
  // from the current queue, so removing the offending row clears the
  // warning by itself, and an acceptance stays attached to the title it
  // was given for.
  useEffect(() => {
    // Clear queue stays enabled during review, so this is a real exit from
    // the armed confirmation too - the chip must stop glowing with it.
    if (queue.length === 0) {
      arm(false);
      setReviewing(false);
    }
  }, [queue.length]);

  const existing = useQuery({
    queryKey: ["pbi-tc-titles", org, pbiId],
    queryFn: () => unwrap(commands.pbiTestCases(org, pbiId)),
    retry: false,
    // Cached-first: a tab flip inside a minute renders instantly from
    // cache; after that the cached list still paints while a background
    // refetch updates it. Refresh buttons and mutation invalidations
    // bypass this and always hit the network.
    staleTime: 60_000,
  });
  const existingCases = useMemo(
    () => (existing.data ?? []).map((t) => ({ id: t.id, title: t.title })),
    [existing.data],
  );

  /** Ask ADO what titles it already has, and remember them.
   *
   * An unreachable ADO falls back to the cache rather than blocking: the
   * submit itself would surface the outage anyway, and refusing to open a
   * review because a check could not run helps nobody. */
  const refreshTitles = useCallback(async (): Promise<{ id: number; title: string }[]> => {
    setCheckingDups(true);
    let cases = existingCases;
    try {
      const fresh = await existing.refetch();
      if (fresh.data) cases = fresh.data.map((t) => ({ id: t.id, title: t.title }));
    } catch {
      // keep the cached list
    }
    setFreshTitles(cases.map((t) => t.title));
    setCheckingDups(false);
    return cases;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [existingCases]);

  /** Which creates clash with a title already on the PBI, minus the ones
   * the user has looked at and accepted.
   *
   * Derived, not stored, so editing or removing a row during the review
   * updates it without another round trip - remove the offending case and
   * the warning clears itself. */
  const dupsPending = useMemo(() => {
    if (!freshTitles) return [];
    const have = new Set(freshTitles.map((t) => t.trim().toLowerCase()));
    const ok = new Set(acceptedDups.map((t) => t.trim().toLowerCase()));
    return queue
      .filter((tc) => {
        const key = tc.title.trim().toLowerCase();
        return tc.update_id == null && have.has(key) && !ok.has(key);
      })
      .map((tc) => tc.title);
  }, [queue, freshTitles, acceptedDups]);

  /** Open the review: arm the PBI check for a queue that creates anything,
   * and find out about duplicates now rather than after the last click. */
  const openReview = () => {
    setReviewing(true);
    if (!pureUpdates) arm(true);
    void refreshTitles();
  };

  return {
    reviewing,
    setReviewing,
    armed,
    arm,
    pureUpdates,
    existingCases,
    checkingDups,
    acceptedDups,
    setAcceptedDups,
    dupsPending,
    refreshTitles,
    openReview,
  };
}
