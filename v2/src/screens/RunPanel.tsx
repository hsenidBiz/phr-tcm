import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { commands } from "./../bindings";
import { Button } from "../components/ui/button";
import { Input } from "../components/ui/input";
import { Select } from "../components/ui/select";
import { Skeleton } from "../components/ui/skeleton";
import { cn } from "../lib/cn";
import { unwrap } from "../lib/ipc";

const OUTCOMES = ["Passed", "Failed", "Blocked", "NotApplicable"] as const;
type Outcome = (typeof OUTCOMES)[number] | "";

const outcomeColor: Record<string, string> = {
  passed: "text-success",
  failed: "text-danger",
  blocked: "text-warning",
  notapplicable: "text-faint",
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

  const suite = useQuery({
    queryKey: ["suite", org, project, pbiId],
    queryFn: () => unwrap(commands.ensurePbiSuite(org, project, pbiId)),
    staleTime: 5 * 60_000,
    retry: false,
  });

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
        }));
      return unwrap(
        commands.submitTestRun(org, project, suite.data!.plan_id, `${pbiTitle} - manual run`, outcomes),
      );
    },
    onSuccess: (run, _vars) => {
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

  return (
    <section className="space-y-3 rounded-md border border-border bg-surface p-4">
      <h2 className="text-sm font-semibold text-text">Run tests for #{pbiId}</h2>

      {suite.isLoading && <Skeleton className="h-16" />}
      {suite.isError && <p className="text-sm text-danger">{suite.error.message}</p>}
      {suite.data && (
        <p className="text-xs text-faint">
          Plan "{suite.data.plan_name}" / suite {suite.data.suite_id}
        </p>
      )}

      {points.isLoading && suite.data && <Skeleton className="h-24" />}
      {points.isError && <p className="text-sm text-danger">{points.error.message}</p>}
      {points.data && points.data.length === 0 && (
        <p className="text-sm text-muted">No test points in this suite yet.</p>
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
            {points.data.map((p) => (
              <tr key={p.point_id} className="border-b border-border/50">
                <td className="px-2 py-1 text-text">
                  <span className="text-faint">#{p.test_case_id}</span> {p.test_case_name}
                </td>
                <td
                  className={cn(
                    "px-2 py-1",
                    outcomeColor[p.last_outcome.toLowerCase()] ?? "text-faint",
                  )}
                >
                  {p.last_outcome || "—"}
                </td>
                <td className="px-2 py-1">
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
                        {o}
                      </option>
                    ))}
                  </Select>
                </td>
                <td className="px-2 py-1">
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
        {runUrl && (
          <a className="text-sm text-accent underline" href={runUrl} target="_blank" rel="noreferrer">
            View run in Azure DevOps
          </a>
        )}
      </div>
    </section>
  );
}
