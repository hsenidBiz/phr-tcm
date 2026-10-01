// Everything this machine has run. Read straight off disk - a run only
// exists in Azure DevOps too once a person has reviewed it and pressed
// Send; until then, nothing here has reached Azure DevOps at all.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { commands } from "../../bindings";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Modal } from "../../components/ui/modal";
import { cn } from "../../lib/cn";
import { unwrapStr } from "../../lib/ipc";
import { toast } from "../../lib/toast";
import { IconCancel, IconClearResults, IconReview } from "../../lib/actionIcons";

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
  const queryClient = useQueryClient();
  // The one subscriber to "autorun-runs" on the screen. "Clear results"
  // lives here, beside the runs it clears, so it reads the count straight
  // off this query - a second `useQuery(["autorun-runs"])` in the parent
  // once shifted render timing enough to paint a run's case title here
  // and the matching case row above at the same instant.
  const runs = useQuery({
    queryKey: ["autorun-runs"],
    queryFn: () => commands.autoRunListRuns(),
    retry: false,
  });
  const runCount = runs.data?.length ?? 0;
  const [clearOpen, setClearOpen] = useState(false);

  /** Housekeeping: wipe every saved run and screenshot on this machine,
   * including runs already sent to Azure DevOps - the confirm dialog says
   * so before this ever runs. Shown wherever Auto Run is (dev, or
   * unlocked); the tab is gated in one place, so no gating here. */
  const clearRuns = useMutation({
    mutationFn: () => unwrapStr(commands.autoRunClearRuns()),
    onSuccess: async (removed) => {
      setClearOpen(false);
      await queryClient.invalidateQueries({ queryKey: ["autorun-runs"] });
      toast.success(`${removed} run${removed === 1 ? "" : "s"} removed.`);
    },
    onError: (e) => toast.error(`Could not clear runs: ${e.message}`),
  });

  return (
    <section className="space-y-2">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h2 className="text-sm font-semibold text-text">Past runs</h2>
        {/* Disabled rather than hidden: a button that vanishes the moment
            it would do nothing invites "where did it go". */}
        <Button
          size="sm"
          variant="outline"
          className="hover:border-danger hover:bg-danger/10 hover:text-danger"
          disabled={runCount === 0}
          onClick={() => setClearOpen(true)}
        >
          <IconClearResults aria-hidden />
          Clear results
        </Button>
      </div>
      <p className="text-xs text-faint">
        Results are saved on this machine. Nothing goes to Azure DevOps unless you press Send to
        Azure DevOps on a run you have reviewed.
      </p>
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
                  {/* Absent on a run saved before environments existed. */}
                  {run.environment && (
                    <Badge title="The environment this run was made in">{run.environment}</Badge>
                  )}
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

      {clearOpen && (
        <Modal onClose={() => setClearOpen(false)} className="flex w-full max-w-md flex-col gap-4 p-5">
          <h2 className="text-sm font-semibold text-text">Clear results?</h2>
          <p className="text-xs text-muted">
            This removes every Auto Run result and picture on this machine, including runs already
            sent to Azure DevOps (those stay there). Nothing in Azure DevOps changes.
          </p>
          <div className="flex justify-end gap-2">
            <Button
              variant="ghost"
              size="sm"
              disabled={clearRuns.isPending}
              onClick={() => setClearOpen(false)}
            >
              <IconCancel aria-hidden />
              Cancel
            </Button>
            <Button
              size="sm"
              variant="danger"
              disabled={clearRuns.isPending}
              onClick={() => clearRuns.mutate()}
            >
              <IconClearResults aria-hidden />
              {clearRuns.isPending ? "Clearing" : `Clear ${runCount} run${runCount === 1 ? "" : "s"}`}
            </Button>
          </div>
        </Modal>
      )}
    </section>
  );
}
