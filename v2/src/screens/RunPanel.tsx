import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { commands, events, type EnsuredSuite } from "./../bindings";
import { Button } from "../components/ui/button";
import { Input } from "../components/ui/input";
import { Select } from "../components/ui/select";
import { cn } from "../lib/cn";
import { unwrap } from "../lib/ipc";
import { openRunnerWindow } from "../lib/openRunner";

const OUTCOMES = ["Passed", "Failed", "Blocked", "NotApplicable"] as const;
type Outcome = (typeof OUTCOMES)[number] | "";

/** Display label only - the ADO value stays "NotApplicable". */
export function outcomeLabel(o: string): string {
  return o.toLowerCase() === "notapplicable" ? "Not Applicable" : o;
}

const outcomeColor: Record<string, string> = {
  passed: "text-success",
  failed: "text-danger",
  blocked: "text-warning",
  notapplicable: "text-faint",
};

/** v1 outcome_style parity: soft background tint per last outcome. */
const outcomeRowTint: Record<string, string> = {
  passed: "bg-success/10",
  failed: "bg-danger/10",
  blocked: "bg-warning/10",
  notapplicable: "bg-surface-2/60",
};

export default function RunPanel({
  org,
  project,
  pbiId,
  pbiTitle,
}: {
  org: string;
  project: string;
  pbiId: number;
  pbiTitle: string;
}) {
  const qc = useQueryClient();
  const [chosen, setChosen] = useState<Record<number, Outcome>>({});
  const [comments, setComments] = useState<Record<number, string>>({});
  const [runUrl, setRunUrl] = useState("");
  const [filterText, setFilterText] = useState("");
  const [filterOutcome, setFilterOutcome] = useState("");
  const [selected, setSelected] = useState<Set<number>>(new Set()); // test case ids
  const [scan, setScan] = useState<{ done: number; total: number } | null>(null);

  const suiteKey = `tcm-v2-suite:${org}/${pbiId}`;

  // Suite resolution is expensive (scans plans) - cache it forever and seed
  // from the last run's localStorage entry; Refresh forces a re-resolve.
  const suite = useQuery({
    queryKey: ["suite", org, project, pbiId],
    queryFn: async () => {
      const s = await unwrap(commands.ensurePbiSuite(org, project, pbiId));
      try {
        localStorage.setItem(suiteKey, JSON.stringify(s));
      } catch {
        // cache is best-effort
      }
      return s;
    },
    initialData: () => {
      try {
        const raw = localStorage.getItem(suiteKey);
        return raw ? (JSON.parse(raw) as EnsuredSuite) : undefined;
      } catch {
        return undefined;
      }
    },
    staleTime: Infinity,
    retry: false,
  });

  useEffect(() => {
    const un = events.suiteScanProgress.listen((e) => setScan(e.payload));
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, []);

  const refreshSuite = () => {
    try {
      localStorage.removeItem(suiteKey);
    } catch {
      // cache is best-effort
    }
    setScan(null);
    qc.removeQueries({ queryKey: ["suite", org, project, pbiId] });
    qc.invalidateQueries({ queryKey: ["points"] });
  };

  const points = useQuery({
    queryKey: ["points", org, project, suite.data?.plan_id, suite.data?.suite_id],
    queryFn: () =>
      unwrap(commands.listTestPoints(org, project, suite.data!.plan_id, suite.data!.suite_id)),
    enabled: Boolean(suite.data),
    retry: false,
  });

  const submit = useMutation({
    mutationFn: async () => {
      const outcomes = Object.entries(chosen)
        .filter(([, o]) => o)
        .map(([pointId, outcome]) => ({
          point_id: Number(pointId),
          outcome: outcome as string,
          comment: comments[Number(pointId)] || null,
          duration_ms: null,
          step_ids: null,
          step_outcomes: null,
          attachments: null,
          bug_ids: null,
        }));
      return unwrap(
        commands.submitTestRun(org, project, suite.data!.plan_id, `${pbiTitle} - manual run`, outcomes),
      );
    },
    onSuccess: (run) => {
      setRunUrl(run.web_url);
      const n = Object.values(chosen).filter(Boolean).length;
      setChosen({});
      setComments({});
      qc.invalidateQueries({ queryKey: ["points"] });
      toast.success(`Recorded ${n} outcome${n === 1 ? "" : "s"} (run #${run.run_id})`);
    },
    onError: (e) => toast.error(`Run failed: ${e.message}`),
  });

  const selectedCount = Object.values(chosen).filter(Boolean).length;

  const openRunner = (caseIds?: number[]) =>
    openRunnerWindow({
      org,
      project,
      planId: suite.data!.plan_id,
      planName: suite.data!.plan_name,
      suiteId: suite.data!.suite_id,
      pbi: { id: pbiId, title: pbiTitle, work_item_type: "" },
      caseIds,
    }).catch((e) => toast.error(`Could not open runner: ${e.message ?? e}`));

  const toggleRow = (caseId: number | null) => {
    if (caseId == null) return;
    setSelected((s) => {
      const next = new Set(s);
      if (next.has(caseId)) next.delete(caseId);
      else next.add(caseId);
      return next;
    });
  };

  return (
    <section className="space-y-3 rounded-md border border-border bg-surface p-4">
      <div className="flex items-center justify-between gap-2">
        <h2 className="text-sm font-semibold text-text">Run tests for #{pbiId}</h2>
        {suite.data && (
          <div className="flex gap-2">
            {selected.size > 0 && (
              <Button size="sm" onClick={() => openRunner([...selected])}>
                Run {selected.size} in runner
              </Button>
            )}
            <Button variant="outline" size="sm" onClick={() => openRunner()}>
              Open runner window
            </Button>
          </div>
        )}
      </div>
      <p className="text-xs text-muted">
        Quick outcomes below, or the runner window for a step-by-step player
        with screenshots and bug filing. Click a row to include it in a
        selective runner session.
      </p>

      {suite.isFetching && !suite.data && (
        <p className="text-sm text-muted">
          {scan ? `Scanning test plans ${scan.done} of ${scan.total}...` : "Resolving test suite..."}
        </p>
      )}
      {suite.isError && <p className="text-sm text-danger">{suite.error.message}</p>}
      {suite.data && (
        <p className="flex items-center gap-1.5 text-xs text-faint">
          Plan "{suite.data.plan_name}" / suite {suite.data.suite_id}
          <button
            aria-label="Re-detect test suite"
            title="Re-detect test suite"
            className="rounded p-0.5 text-muted hover:text-accent"
            onClick={refreshSuite}
          >
            <RefreshCw size={12} />
          </button>
          {suite.isFetching && <span>(re-detecting...)</span>}
        </p>
      )}

      {points.isFetching && suite.data && !points.data && (
        <p className="text-sm text-muted">Loading test points...</p>
      )}
      {points.isError && <p className="text-sm text-danger">{points.error.message}</p>}
      {points.data && points.data.length === 0 && (
        <p className="text-sm text-muted">No test points in this suite yet.</p>
      )}

      {points.data && points.data.length > 0 && (
        <div className="flex gap-2">
          <Input
            aria-label="Filter points"
            className="w-56 px-2 py-1"
            placeholder="Filter by name or id..."
            value={filterText}
            onChange={(e) => setFilterText(e.target.value)}
          />
          <Select
            aria-label="Filter by last outcome"
            className="px-2 py-1"
            value={filterOutcome}
            onChange={(e) => setFilterOutcome(e.target.value)}
          >
            <option value="">All outcomes</option>
            <option value="passed">Passed</option>
            <option value="failed">Failed</option>
            <option value="blocked">Blocked</option>
            <option value="notapplicable">Not Applicable</option>
            <option value="none">Never run</option>
          </Select>
        </div>
      )}

      {points.data && points.data.length > 0 && (
        <table className="w-full border-collapse text-sm">
          <thead>
            <tr className="border-b border-border text-left text-xs text-muted">
              <th className="px-2 py-1 font-medium">Test case</th>
              <th className="px-2 py-1 font-medium">Last outcome</th>
              <th className="px-2 py-1 font-medium">This run</th>
              <th className="px-2 py-1 font-medium">Comment</th>
            </tr>
          </thead>
          <tbody>
            {points.data
              .filter((p) => {
                if (filterOutcome === "none" && p.last_outcome) return false;
                if (
                  filterOutcome &&
                  filterOutcome !== "none" &&
                  p.last_outcome.toLowerCase() !== filterOutcome
                )
                  return false;
                if (filterText) {
                  const t = filterText.toLowerCase();
                  if (
                    !p.test_case_name.toLowerCase().includes(t) &&
                    !String(p.test_case_id ?? "").includes(t)
                  )
                    return false;
                }
                return true;
              })
              .map((p) => (
              <tr
                key={p.point_id}
                className={cn(
                  "cursor-pointer border-b border-border/50 transition-colors",
                  p.test_case_id != null && selected.has(p.test_case_id)
                    ? "bg-accent-soft"
                    : (outcomeRowTint[p.last_outcome.toLowerCase()] ?? ""),
                  "hover:bg-surface-2",
                )}
                onClick={() => toggleRow(p.test_case_id)}
              >
                <td className="px-2 py-1 text-text">
                  <span className="id-mono text-faint">#{p.test_case_id}</span> {p.test_case_name}
                </td>
                <td
                  className={cn(
                    "px-2 py-1",
                    outcomeColor[p.last_outcome.toLowerCase()] ?? "text-faint",
                  )}
                >
                  {outcomeLabel(p.last_outcome) || "—"}
                </td>
                <td className="px-2 py-1" onClick={(e) => e.stopPropagation()}>
                  <Select
                    aria-label={`Outcome for ${p.test_case_name}`}
                    className="px-2 py-1"
                    value={chosen[p.point_id] ?? ""}
                    onChange={(e) =>
                      setChosen((c) => ({ ...c, [p.point_id]: e.target.value as Outcome }))
                    }
                  >
                    <option value="">(skip)</option>
                    {OUTCOMES.map((o) => (
                      <option key={o} value={o}>
                        {outcomeLabel(o)}
                      </option>
                    ))}
                  </Select>
                </td>
                <td className="px-2 py-1" onClick={(e) => e.stopPropagation()}>
                  <Input
                    className="w-full px-2 py-1"
                    placeholder="Optional comment"
                    value={comments[p.point_id] ?? ""}
                    onChange={(e) =>
                      setComments((c) => ({ ...c, [p.point_id]: e.target.value }))
                    }
                  />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      <div className="flex items-center gap-3">
        <Button
          disabled={selectedCount === 0 || submit.isPending}
          onClick={() => submit.mutate()}
        >
          {submit.isPending
            ? "Recording..."
            : `Record ${selectedCount} outcome${selectedCount === 1 ? "" : "s"}`}
        </Button>
        {selected.size > 0 && (
          <button className="text-xs text-muted hover:text-text" onClick={() => setSelected(new Set())}>
            Clear selection ({selected.size})
          </button>
        )}
        {runUrl && (
          <a className="text-sm text-accent underline" href={runUrl} target="_blank" rel="noreferrer">
            View run in Azure DevOps
          </a>
        )}
      </div>
    </section>
  );
}
