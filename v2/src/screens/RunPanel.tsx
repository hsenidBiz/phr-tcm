import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { commands } from "./../bindings";
import { unwrap } from "../lib/ipc";

const OUTCOMES = ["Passed", "Failed", "Blocked", "NotApplicable"] as const;
type Outcome = (typeof OUTCOMES)[number] | "";

const outcomeColor: Record<string, string> = {
  passed: "text-green-400",
  failed: "text-red-400",
  blocked: "text-amber-400",
  notapplicable: "text-neutral-500",
};

const inputCls =
  "rounded-md border border-neutral-700 bg-neutral-900 px-2 py-1 text-sm focus:border-blue-500 focus:outline-none";

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
        commands.submitTestRun(
          org,
          project,
          suite.data!.plan_id,
          `${pbiTitle} - manual run`,
          outcomes,
        ),
      );
    },
    onSuccess: (run) => {
      setRunUrl(run.web_url);
      setChosen({});
      setComments({});
      qc.invalidateQueries({ queryKey: ["points"] });
    },
  });

  const selectedCount = Object.values(chosen).filter(Boolean).length;

  return (
    <section className="space-y-3 rounded-md border border-neutral-800 p-4">
      <h2 className="text-sm font-semibold text-neutral-300">Run tests for #{pbiId}</h2>

      {suite.isLoading && <p className="text-sm text-neutral-400">Resolving test suite...</p>}
      {suite.isError && <p className="text-sm text-red-400">{suite.error.message}</p>}
      {suite.data && (
        <p className="text-xs text-neutral-500">
          Plan "{suite.data.plan_name}" / suite {suite.data.suite_id}
        </p>
      )}

      {points.isLoading && suite.data && (
        <p className="text-sm text-neutral-400">Loading test points...</p>
      )}
      {points.isError && <p className="text-sm text-red-400">{points.error.message}</p>}
      {points.data && points.data.length === 0 && (
        <p className="text-sm text-neutral-400">No test points in this suite yet.</p>
      )}

      {points.data && points.data.length > 0 && (
        <table className="w-full border-collapse text-sm">
          <thead>
            <tr className="border-b border-neutral-800 text-left text-xs text-neutral-400">
              <th className="px-2 py-1 font-medium">Test case</th>
              <th className="px-2 py-1 font-medium">Last outcome</th>
              <th className="px-2 py-1 font-medium">This run</th>
              <th className="px-2 py-1 font-medium">Comment</th>
            </tr>
          </thead>
          <tbody>
            {points.data.map((p) => (
              <tr key={p.point_id} className="border-b border-neutral-900">
                <td className="px-2 py-1">
                  <span className="text-neutral-500">#{p.test_case_id}</span> {p.test_case_name}
                </td>
                <td
                  className={
                    "px-2 py-1 " + (outcomeColor[p.last_outcome.toLowerCase()] ?? "text-neutral-500")
                  }
                >
                  {p.last_outcome || "—"}
                </td>
                <td className="px-2 py-1">
                  <select
                    aria-label={`Outcome for ${p.test_case_name}`}
                    className={inputCls}
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
                  </select>
                </td>
                <td className="px-2 py-1">
                  <input
                    className={inputCls + " w-full"}
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
        <button
          className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium hover:bg-blue-500 disabled:opacity-50"
          disabled={selectedCount === 0 || submit.isPending}
          onClick={() => submit.mutate()}
        >
          {submit.isPending ? "Recording..." : `Record ${selectedCount} outcome${selectedCount === 1 ? "" : "s"}`}
        </button>
        {submit.isError && <p className="text-sm text-red-400">{submit.error.message}</p>}
        {runUrl && (
          <a className="text-sm text-blue-400 underline" href={runUrl} target="_blank" rel="noreferrer">
            View run in Azure DevOps
          </a>
        )}
      </div>
    </section>
  );
}
