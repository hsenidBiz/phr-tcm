// Which cases the person marked Flaky, read off the scripts on this machine. Run summaries (Past runs, the
// review) count a marked case's failure apart from the failures that make a run read as failed. A local
// read like the run list, so a plain query: it is changed only by `useSetFlaky`, which refreshes it.

import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { commands } from "../../bindings";
import { unwrapStr } from "../../lib/ipc";
import { toast } from "../../lib/toast";

export const FLAKY_MARKS_KEY = ["autorun-flaky-marks"] as const;
export const FLAKY_CASES_KEY = ["autorun-flaky-cases"] as const;

/** Refreshes the saved-run list and the two flaky reads that come from the same files: wherever the run
 * files change, the Flaky cases list is stale too. */
export function invalidateRuns(queryClient: QueryClient): Promise<unknown> {
  return Promise.all([
    queryClient.invalidateQueries({ queryKey: ["autorun-runs"] }),
    queryClient.invalidateQueries({ queryKey: FLAKY_CASES_KEY }),
    queryClient.invalidateQueries({ queryKey: FLAKY_MARKS_KEY }),
  ]);
}

const NONE: ReadonlySet<number> = new Set();

/** The ids of the cases marked Flaky; empty until read. */
export function useFlakyMarks(): ReadonlySet<number> {
  const marks = useQuery({
    queryKey: FLAKY_MARKS_KEY,
    queryFn: () => commands.autoRunFlakyMarks(),
    retry: false,
    select: (ids) => new Set(ids),
    structuralSharing: false,
  });
  return marks.data ?? NONE;
}

/** Marks or clears one case's Flaky mark, then refreshes what reads it. */
export function useSetFlaky() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ caseId, flaky }: { caseId: number; flaky: boolean }) =>
      unwrapStr(commands.autoRunSetFlaky(caseId, flaky)),
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: FLAKY_MARKS_KEY }),
        queryClient.invalidateQueries({ queryKey: FLAKY_CASES_KEY }),
      ]);
    },
    onError: (e) => toast.error(`Could not change the Flaky mark: ${e.message}`),
  });
}
