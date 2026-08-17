// Everything this machine has run. Read straight off disk - these
// results exist nowhere else, which is the whole arrangement while the
// feature earns trust.

import { useQuery } from "@tanstack/react-query";
import { commands } from "../../bindings";
import { cn } from "../../lib/cn";

const verdictTone: Record<string, string> = {
  Passed: "text-success",
  Failed: "text-danger",
  Blocked: "text-warning",
};

/** Epoch milliseconds as a string; the Rust side sends it that way
 * because specta will not carry a u64 across IPC. */
function when(startedAt: string): string {
  const n = Number(startedAt);
  if (!Number.isFinite(n) || n <= 0) return "unknown time";
  return new Date(n).toLocaleString();
}

export default function PastRuns() {
  const runs = useQuery({
    queryKey: ["autorun-runs"],
    queryFn: () => commands.autoRunListRuns(),
    retry: false,
  });

  return (
    <div className="space-y-2">
      <h2 className="text-sm font-semibold text-muted">Past runs (this machine)</h2>
      {(runs.data?.length ?? 0) === 0 && (
        <p className="text-xs text-muted">No runs on this machine yet.</p>
      )}
      <ul className="space-y-1">
        {(runs.data ?? []).flatMap((run) =>
          run.cases.map((c) => (
            <li
              key={`${run.id}-${c.case_id}`}
              aria-label={`Run of ${c.title}`}
              className="rounded-md border border-border bg-surface px-3 py-2 text-sm"
            >
              <div className="flex items-center gap-2">
                <span className="id-mono text-faint">#{c.case_id}</span>
                <span className="min-w-0 flex-1 truncate text-text">{c.title}</span>
                <span className={cn("text-xs font-medium", verdictTone[c.verdict] ?? "text-faint")}>
                  {c.verdict || "—"}
                </span>
                <span className="shrink-0 text-[11px] text-faint">{when(run.started_at)}</span>
              </div>
              {c.note && <p className="mt-0.5 text-xs text-muted">{c.note}</p>}
            </li>
          )),
        )}
      </ul>
    </div>
  );
}
