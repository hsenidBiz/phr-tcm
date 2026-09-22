// Everything this machine has run. Read straight off disk - a run only
// exists in Azure DevOps too once a person has reviewed it and pressed
// Send; until then, nothing here has reached Azure DevOps at all.

import { useQuery } from "@tanstack/react-query";
import { commands } from "../../bindings";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { cn } from "../../lib/cn";
import { IconReview } from "../../lib/actionIcons";

/** A case's own verdict, text-only - a lighter touch than the pressed-button
 * tone in verdicts.ts, which this plain row was never meant to borrow. */
const rowTone: Record<string, string> = {
  Passed: "text-success",
  Failed: "text-danger",
  Blocked: "text-warning",
};

/** Epoch milliseconds as a string; the Rust side sends it that way because
 * specta will not carry a u64 across IPC. */
function when(startedAt: string): string {
  const n = Number(startedAt);
  if (!Number.isFinite(n) || n <= 0) return "unknown time";
  return new Date(n).toLocaleString();
}

export default function PastRuns({
  pbiId,
  onReview,
}: {
  /** The PBI currently selected on the Auto Run screen. A run reviewed
   * here is sent with THIS PBI's title and step ids (see `RunReview`), so
   * a run saved under a different PBI must never be offered for review
   * while some other PBI is selected - it would be sent under the wrong
   * name with every `step_ids` empty, silently. */
  pbiId: number | null;
  onReview: (runId: string) => void;
}) {
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
      <div className="space-y-2">
        {(runs.data ?? []).map((run) => {
          const unattended = run.mode === "unattended";
          const unconfirmed = run.cases.filter((c) => !c.verdict).length;
          const otherPbi = pbiId != null && run.pbi_id !== pbiId;
          return (
            <div key={run.id} className="space-y-1 rounded-md border border-border bg-surface p-2">
              <div className="flex items-center justify-between gap-2">
                <div className="flex items-center gap-2 text-xs text-muted">
                  <span>{when(run.started_at)}</span>
                  <Badge>{unattended ? "unattended" : "supervised"}</Badge>
                </div>
                {/* Reviewing only makes sense for an unattended run - a
                    supervised one was decided by the person watching it in
                    real time, so there is nothing here to propose again. */}
                {run.published ? (
                  <span className="text-xs font-medium text-faint">Sent</span>
                ) : unattended && otherPbi ? (
                  <span className="text-xs font-medium text-faint">for PBI #{run.pbi_id}</span>
                ) : unattended ? (
                  <div className="flex items-center gap-2">
                    {unconfirmed > 0 && (
                      <span className="text-xs text-muted">{unconfirmed} to review</span>
                    )}
                    <Button size="sm" variant="outline" onClick={() => onReview(run.id)}>
                      <IconReview aria-hidden />
                      {unconfirmed > 0 ? "Review" : "Open review"}
                    </Button>
                  </div>
                ) : null}
              </div>
              <ul className="space-y-1">
                {run.cases.map((c) => (
                  <li
                    key={`${run.id}-${c.case_id}`}
                    aria-label={`Run of ${c.title}`}
                    className="rounded-md border border-border/60 bg-surface px-3 py-2 text-sm"
                  >
                    <div className="flex items-center gap-2">
                      <span className="id-mono text-faint">#{c.case_id}</span>
                      <span className="min-w-0 flex-1 truncate text-text">{c.title}</span>
                      {c.verdict ? (
                        <span className={cn("text-xs font-medium", rowTone[c.verdict] ?? "text-faint")}>
                          {c.verdict}
                        </span>
                      ) : c.proposed ? (
                        <span className="text-xs font-medium text-faint">proposed {c.proposed}</span>
                      ) : (
                        <span className="text-xs font-medium text-faint">unset</span>
                      )}
                    </div>
                    {c.note && <p className="mt-0.5 text-xs text-muted">{c.note}</p>}
                  </li>
                ))}
              </ul>
            </div>
          );
        })}
      </div>
    </div>
  );
}
