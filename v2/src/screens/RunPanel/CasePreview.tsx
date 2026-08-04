// Read-only expansion of a Run Tests row: the case's steps plus the last
// result's comment and linked bugs, so a failure can be checked (and a fix
// verified) without opening the runner.

import { useQuery } from "@tanstack/react-query";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useState } from "react";
import { toast } from "sonner";
import { commands, type RunOutcome, type TestPoint } from "../../bindings";
import { outcomeLabel } from "../../lib/outcomes";
import { unwrap } from "../../lib/ipc";
import { History } from "lucide-react";


/** Read-only expansion of a Run Tests row: the case's steps plus the last
 * result's comment and linked bugs, so a failure can be checked (and a fix
 * verified) without opening the runner. */
export default function CasePreview({
  org,
  project,
  point,
  history = [],
}: {
  org: string;
  project: string;
  point: TestPoint;
  /** Last-N outcomes for this case, newest first - the dots' data, handed
   * down so the expansion can open the runs BEHIND the dots. */
  history?: RunOutcome[];
}) {
  // Everything before the newest result. The panel exists for exactly the
  // cases that have a past - one lone result has no history to show.
  const prior = history.slice(1);
  const [showHistory, setShowHistory] = useState(false);
  const caseId = point.test_case_id;
  const steps = useQuery({
    queryKey: ["run-case-steps", org, caseId],
    queryFn: () => unwrap(commands.testCasesByIds(org, [caseId!], null, null)),
    enabled: caseId != null,
    staleTime: 5 * 60_000,
    retry: false,
  });
  const failure = useQuery({
    queryKey: ["run-fail", org, project, point.last_run_id, point.last_result_id],
    queryFn: () =>
      unwrap(
        commands.resultFailureDetail(org, project, point.last_run_id!, point.last_result_id!),
      ),
    enabled: point.last_run_id != null && point.last_result_id != null,
    staleTime: 60_000,
    retry: false,
  });

  const tc = steps.data?.[0];
  const comment = failure.data?.comment?.trim();
  const bugIds = failure.data?.bug_ids ?? [];

  return (
    <div className="space-y-3 border-l-2 border-accent/50 bg-surface-2/40 px-4 py-3">
      {(comment || bugIds.length > 0) && (
        <div className="rounded-md border border-danger/30 bg-danger/5 p-2">
          <p className="mb-1 text-[11px] font-semibold uppercase tracking-wide text-danger">
            Last result
          </p>
          {comment ? (
            <p className="whitespace-pre-wrap text-xs text-text">{comment}</p>
          ) : (
            <p className="text-xs text-muted">No comment recorded.</p>
          )}
          {bugIds.length > 0 && (
            <p className="mt-1.5 text-xs text-muted">
              Bugs:{" "}
              {bugIds.map((id) => (
                <button
                  key={id}
                  className="mr-2 text-accent underline"
                  onClick={() =>
                    openUrl(
                      `https://dev.azure.com/${org}/${encodeURIComponent(project)}/_workitems/edit/${id}`,
                    ).catch(() => toast.error("Could not open the browser."))
                  }
                >
                  #{id}
                </button>
              ))}
            </p>
          )}
        </div>
      )}

      {prior.length > 0 && (
        <div>
          {/* Same pattern as the PR panel's pipeline view: a button that
              opens the record behind the summary, fetched only when asked. */}
          <button
            className="flex items-center gap-1.5 text-xs font-medium text-accent hover:underline"
            aria-expanded={showHistory}
            onClick={() => setShowHistory((v) => !v)}
          >
            <History size={12} aria-hidden />
            {showHistory
              ? "Hide execution history"
              : `Execution history (${prior.length} earlier result${prior.length === 1 ? "" : "s"})`}
          </button>
          {showHistory && (
            <ul className="mt-2 space-y-1.5">
              {prior.map((o, i) => (
                <HistoryEntry key={`${o.run_id}-${o.result_id}-${i}`} org={org} project={project} entry={o} />
              ))}
            </ul>
          )}
        </div>
      )}

      {steps.isLoading && <p className="text-xs text-muted">Loading steps…</p>}
      {steps.isError && <p className="text-xs text-danger">{steps.error.message}</p>}
      {tc &&
        (tc.steps.length > 0 ? (
          <table className="w-full border-collapse text-xs">
            <thead>
              <tr className="text-left text-faint">
                <th className="w-8 px-2 py-1 font-medium">#</th>
                <th className="px-2 py-1 font-medium">Action</th>
                <th className="px-2 py-1 font-medium">Expected</th>
              </tr>
            </thead>
            <tbody>
              {tc.steps.map((s, i) => (
                <tr key={i} className="border-t border-border/40 align-top">
                  <td className="px-2 py-1 text-faint">{i + 1}</td>
                  <td className="whitespace-pre-wrap px-2 py-1 text-text">{s.action}</td>
                  <td className="whitespace-pre-wrap px-2 py-1 text-muted">{s.expected}</td>
                </tr>
              ))}
            </tbody>
          </table>
        ) : (
          <p className="text-xs text-muted">This test case has no steps.</p>
        ))}
    </div>
  );
}


/** One earlier result: verdict, when, which run - and the comment behind
 * it, fetched only once this row is on screen. */
function HistoryEntry({
  org,
  project,
  entry,
}: {
  org: string;
  project: string;
  entry: RunOutcome;
}) {
  const detail = useQuery({
    queryKey: ["run-fail", org, project, entry.run_id, entry.result_id],
    queryFn: () =>
      unwrap(commands.resultFailureDetail(org, project, entry.run_id, entry.result_id)),
    staleTime: Infinity, // a finished result never changes
    retry: false,
  });
  const tone: Record<string, string> = {
    passed: "text-success",
    failed: "text-danger",
    paused: "text-muted",
    blocked: "text-warning",
  };
  const comment = detail.data?.comment?.trim();
  return (
    <li className="rounded-md border border-border/60 bg-surface px-2.5 py-1.5">
      <p className="text-xs">
        <span className={tone[entry.outcome.toLowerCase()] ?? "text-faint"}>
          {outcomeLabel(entry.outcome)}
        </span>
        <span className="text-faint">
          {" "}
          · {entry.completed_date ? entry.completed_date.slice(0, 10) : "no date"} · run #
          {entry.run_id}
        </span>
      </p>
      {detail.isLoading && <p className="text-[11px] text-faint">Loading comment…</p>}
      {comment && <p className="mt-0.5 whitespace-pre-wrap text-[11px] text-muted">{comment}</p>}
      {detail.data && !comment && (
        <p className="text-[11px] text-faint">No comment recorded.</p>
      )}
    </li>
  );
}
