import { useQuery } from "@tanstack/react-query";
import { ChevronDown, ChevronRight, FolderTree } from "lucide-react";
import { useState } from "react";
import { commands, type SuiteRef } from "../bindings";
import { Badge } from "../components/ui/badge";
import { Skeleton } from "../components/ui/skeleton";
import { cn } from "../lib/cn";
import { unwrap } from "../lib/ipc";

const outcomeColor: Record<string, string> = {
  passed: "text-success",
  failed: "text-danger",
  blocked: "text-warning",
  notapplicable: "text-faint",
};

function SuitePoints({
  org,
  project,
  planId,
  suite,
}: {
  org: string;
  project: string;
  planId: number;
  suite: SuiteRef;
}) {
  const points = useQuery({
    queryKey: ["points", org, project, planId, suite.id],
    queryFn: () => unwrap(commands.listTestPoints(org, project, planId, suite.id)),
    retry: false,
  });

  if (points.isLoading) return <Skeleton className="ml-8 h-12" />;
  if (points.isError)
    return <p className="ml-8 text-sm text-danger">{points.error.message}</p>;
  if (!points.data || points.data.length === 0)
    return <p className="ml-8 text-sm text-muted">No test points in this suite.</p>;

  return (
    <table className="ml-8 w-[calc(100%-2rem)] border-collapse text-sm">
      <thead>
        <tr className="border-b border-border text-left text-xs text-muted">
          <th className="px-2 py-1 font-medium">Test case</th>
          <th className="px-2 py-1 font-medium">Configuration</th>
          <th className="px-2 py-1 font-medium">Last outcome</th>
        </tr>
      </thead>
      <tbody>
        {points.data.map((p) => (
          <tr key={p.point_id} className="border-b border-border/50">
            <td className="px-2 py-1 text-text">
              <span className="id-mono text-faint">#{p.test_case_id}</span> {p.test_case_name}
            </td>
            <td className="px-2 py-1 text-muted">{p.config_name}</td>
            <td className={cn("px-2 py-1", outcomeColor[p.last_outcome.toLowerCase()] ?? "text-faint")}>
              {p.last_outcome || "—"}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** The v1 Test Suites browser: plan -> suite tree (plans with no suites are
 * hidden - the v1 rule), a suite click shows its points read-only, and a
 * requirement suite can jump straight into Edit/Run for its PBI. */
export default function Suites({
  org,
  project,
  onOpenPbi,
}: {
  org: string;
  project: string;
  onOpenPbi?: (pbi: { id: number; title: string }, target: "edit" | "run") => void;
}) {
  const [openSuite, setOpenSuite] = useState<number | null>(null);

  const plans = useQuery({
    queryKey: ["plans-suites", org, project],
    queryFn: () => unwrap(commands.listPlansWithSuites(org, project)),
    enabled: Boolean(org && project),
    retry: false,
  });

  if (!org || !project) {
    return (
      <p className="text-sm text-muted">
        Pick an organization and project in the bar above to browse test suites.
      </p>
    );
  }

  return (
    <div className="space-y-3">
      {plans.isLoading && (
        <div className="space-y-2">
          <Skeleton className="h-10" />
          <Skeleton className="h-10" />
        </div>
      )}
      {plans.isError && <p className="text-sm text-danger">{plans.error.message}</p>}
      {plans.data && plans.data.length === 0 && (
        <p className="text-sm text-muted">No test plans with test suites in this project yet.</p>
      )}

      {(plans.data ?? []).map(({ plan, suites }) => (
        <section key={plan.id} className="rounded-md border border-border bg-surface">
          <header className="flex items-center gap-2 border-b border-border px-3 py-2 text-sm font-medium text-text">
            <FolderTree size={14} className="text-accent" />
            {plan.name}
            <span className="text-xs text-faint">{plan.area_path}</span>
          </header>
          <ul className="p-1">
            {suites.map((s) => (
              <li key={s.id}>
                <button
                  className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-sm text-text hover:bg-accent-soft"
                  onClick={() => setOpenSuite((o) => (o === s.id ? null : s.id))}
                >
                  {openSuite === s.id ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                  {s.name}
                  {s.suite_type === "requirementTestSuite" && (
                    <Badge className="bg-accent-soft text-accent">
                      PBI {s.requirement_id}
                    </Badge>
                  )}
                  {onOpenPbi && s.suite_type === "requirementTestSuite" && s.requirement_id && (
                    <span className="ml-auto flex gap-1">
                      {(["edit", "run"] as const).map((target) => (
                        <span
                          key={target}
                          role="button"
                          className="rounded border border-border px-1.5 py-0.5 text-[10px] text-muted hover:border-accent hover:text-accent"
                          onClick={(e) => {
                            e.stopPropagation();
                            onOpenPbi({ id: s.requirement_id!, title: s.name }, target);
                          }}
                        >
                          {target === "edit" ? "Edit cases" : "Run"}
                        </span>
                      ))}
                    </span>
                  )}
                </button>
                {openSuite === s.id && (
                  <SuitePoints org={org} project={project} planId={plan.id} suite={s} />
                )}
              </li>
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}
