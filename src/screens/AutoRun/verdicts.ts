// The words a human can pick as a verdict, and how each one is coloured
// once picked - shared by the supervised pane (RunPane) and the review
// screen (RunReview) so a "Failed" button looks the same wherever it is
// pressed. One copy: a second one is how the two panes quietly drift.

export const VERDICTS = ["Passed", "Failed", "Blocked"] as const;

export const verdictTone: Record<string, string> = {
  Passed: "bg-success/20 text-success",
  Failed: "bg-danger/20 text-danger",
  Blocked: "bg-warning/20 text-warning",
};

/** The four results a case can be filtered by and counted in. */
export const RESULT_BUCKETS = ["Passed", "Failed", "Blocked", "Not run"] as const;
export type ResultBucket = (typeof RESULT_BUCKETS)[number];
/** A filter: every case, or the cases in one bucket. */
export type ResultFilter = "All" | ResultBucket;

/** How each bucket's count is coloured - text only, like a Past runs row. */
export const bucketTone: Record<ResultBucket, string> = {
  Passed: "text-success",
  Failed: "text-danger",
  Blocked: "text-warning",
  "Not run": "text-faint",
};

/**
 * Which bucket a case's result falls in. The person's confirmed `verdict`
 * when it is set, else the machine's `proposed`; that word, exactly
 * "Passed", "Failed" or "Blocked", is its own bucket. Everything else is
 * Not run: nothing proposed (a script that checks nothing, a case a stopped
 * run never reached), or a word that is none of the three.
 *
 * The run report applies the same rule in Rust (`autorun::report::bucket`).
 * Both are held to one table, src-tauri/tests/fixtures/verdict_buckets.json,
 * so the screen and the report can never count a case differently.
 */
export function resultBucket(c: { verdict: string; proposed?: string | null }): ResultBucket {
  const word = c.verdict !== "" ? c.verdict : (c.proposed ?? "");
  return word === "Passed" || word === "Failed" || word === "Blocked" ? word : "Not run";
}

/** How many of `cases` fall in each bucket. */
export function countBuckets(
  cases: readonly { verdict: string; proposed?: string | null }[],
): Record<ResultBucket, number> {
  const out: Record<ResultBucket, number> = { Passed: 0, Failed: 0, Blocked: 0, "Not run": 0 };
  for (const c of cases) out[resultBucket(c)] += 1;
  return out;
}

/** Whether a case shows under `filter`. */
export function matchesFilter(c: { verdict: string; proposed?: string | null }, filter: ResultFilter): boolean {
  return filter === "All" || resultBucket(c) === filter;
}

/**
 * Each case's last result: the bucket of its record in the NEWEST run that
 * holds it. A run that never reached a case says nothing about it, so a case
 * missing from the newest run keeps the result of the newest one that has it.
 * `started_at` is epoch milliseconds as text, so it is compared as a number;
 * of two runs that started in the same millisecond, the later in the list
 * wins. A case in no run is not in the map - read it with `lastResultFor`.
 */
export function lastResults(
  runs: readonly {
    started_at: string;
    cases: readonly { case_id: number; verdict: string; proposed?: string | null }[];
  }[],
): Map<number, ResultBucket> {
  const out = new Map<number, ResultBucket>();
  const at = new Map<number, number>();
  for (const run of runs) {
    const started = Number(run.started_at);
    const when = Number.isFinite(started) ? started : 0;
    for (const c of run.cases) {
      const seen = at.get(c.case_id);
      if (seen !== undefined && seen > when) continue;
      at.set(c.case_id, when);
      out.set(c.case_id, resultBucket(c));
    }
  }
  return out;
}

/** A case's last result, Not run when no run holds it. */
export function lastResultFor(last: ReadonlyMap<number, ResultBucket>, caseId: number): ResultBucket {
  return last.get(caseId) ?? "Not run";
}
