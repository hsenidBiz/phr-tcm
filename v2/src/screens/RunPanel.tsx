import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronDown, ChevronRight, RefreshCw, X } from "lucide-react";
import { Fragment, useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import { commands, events, type EnsuredSuite, type TestPoint } from "./../bindings";
import { Button } from "../components/ui/button";
import { Checkbox } from "../components/ui/checkbox";
import { Input } from "../components/ui/input";
import { Select } from "../components/ui/select";
import { cn } from "../lib/cn";
import { groupIndices } from "../lib/grouping";
import { unwrap } from "../lib/ipc";
import { openRunnerWindow } from "../lib/openRunner";

const OUTCOMES = ["Passed", "Failed", "Blocked", "NotApplicable"] as const;
type Outcome = (typeof OUTCOMES)[number] | "";

/** Display label only - the ADO value is unchanged. Capitalizes the raw
 * lowercase outcomes ("passed" -> "Passed") and spells out Not Applicable. */
export function outcomeLabel(o: string): string {
  if (!o) return "";
  const k = o.toLowerCase();
  if (k === "notapplicable") return "Not Applicable";
  return k[0].toUpperCase() + k.slice(1);
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
  const [grouped, setGrouped] = useState(
    () => localStorage.getItem("tcm-v2-group-points") === "on",
  );
  const [anchor, setAnchor] = useState<number | null>(null); // shift-range start
  const [collapsedGroups, setCollapsedGroups] = useState<Set<string>>(new Set());
  const toggleCollapsed = (name: string) =>
    setCollapsedGroups((s) => {
      const next = new Set(s);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });

  const suiteKey = `tcm-v2-suite:${org}/${pbiId}`;
  const readSuiteSeed = (): EnsuredSuite | undefined => {
    try {
      const raw = localStorage.getItem(suiteKey);
      return raw ? (JSON.parse(raw) as EnsuredSuite) : undefined;
    } catch {
      return undefined;
    }
  };

  // Suite resolution is expensive (scans plans), so the queryFn itself
  // short-circuits to the persisted seed: no matter what triggers a
  // refetch (remount, focus, gc), the network is only hit when no seed
  // exists. Refresh deletes the seed to force a true re-resolve.
  const suite = useQuery({
    queryKey: ["suite", org, project, pbiId],
    queryFn: async () => {
      const seed = readSuiteSeed();
      if (seed) return seed;
      const s = await unwrap(commands.ensurePbiSuite(org, project, pbiId));
      try {
        localStorage.setItem(suiteKey, JSON.stringify(s));
      } catch {
        // cache is best-effort
      }
      return s;
    },
    initialData: readSuiteSeed,
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
    gcTime: 30 * 60_000, // keep the background prefetch alive while unobserved
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

  const filtered = useMemo(
    () =>
      (points.data ?? []).filter((p) => {
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
      }),
    [points.data, filterOutcome, filterText],
  );

  // v1 smart grouping over the visible rows (shared title-prefix folders).
  const sections = useMemo(() => {
    if (!grouped) return [{ name: "", pts: filtered }];
    return groupIndices(filtered.map((p) => p.test_case_name)).map(({ name, indices }) => ({
      name: name || "Ungrouped",
      pts: indices.map((i) => filtered[i]),
    }));
  }, [filtered, grouped]);

  // Click toggles a row; shift+click selects the whole range from the
  // last clicked row, in the visible (filtered/grouped) order.
  const handleRowClick = (p: TestPoint, e: React.MouseEvent) => {
    if (p.test_case_id == null) return;
    const flat = sections.flatMap((s) => s.pts);
    if (e.shiftKey && anchor != null) {
      const ids = flat.map((x) => x.test_case_id);
      const a = ids.indexOf(anchor);
      const b = ids.indexOf(p.test_case_id);
      if (a >= 0 && b >= 0) {
        const [lo, hi] = a < b ? [a, b] : [b, a];
        const range = ids.slice(lo, hi + 1).filter((x): x is number => x != null);
        setSelected((s) => new Set([...s, ...range]));
        return;
      }
    }
    const caseId = p.test_case_id;
    setSelected((s) => {
      const next = new Set(s);
      if (next.has(caseId)) next.delete(caseId);
      else next.add(caseId);
      return next;
    });
    setAnchor(caseId);
  };

  /** Group-header click: select every case under it (again to clear). */
  const toggleSection = (pts: TestPoint[]) => {
    const ids = pts.map((p) => p.test_case_id).filter((x): x is number => x != null);
    if (!ids.length) return;
    setSelected((s) => {
      const all = ids.every((id) => s.has(id));
      const next = new Set(s);
      if (all) ids.forEach((id) => next.delete(id));
      else ids.forEach((id) => next.add(id));
      return next;
    });
    setAnchor(ids[0]);
  };

  return (
    <section className="space-y-3 rounded-md border border-border bg-surface p-4">
      <div className="flex items-center justify-between gap-2">
        <h2 className="text-sm font-semibold text-text">Run tests for #{pbiId}</h2>
        {suite.data && (
          <Button variant="outline" size="sm" onClick={() => openRunner()}>
            Open runner window
          </Button>
        )}
      </div>
      <p className="text-xs text-muted">
        Quick outcomes below, or the runner window for a step-by-step player
        with screenshots and bug filing. Click a row to include it in a
        selective runner session.
      </p>

      {suite.isFetching && !suite.data && (
        <p className="text-sm text-muted">
          {scan ? `Scanning test plans ${scan.done} of ${scan.total}` : "Resolving test suite"}
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
          {suite.isFetching && <span>(re-detecting)</span>}
        </p>
      )}

      {points.isFetching && suite.data && !points.data && (
        <p className="text-sm text-muted">Loading test points</p>
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
            placeholder="Filter by name or id"
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
          <label className="flex items-center gap-1.5 text-xs text-muted">
            <Checkbox
              checked={grouped}
              onCheckedChange={(v) => {
                setGrouped(v);
                try {
                  localStorage.setItem("tcm-v2-group-points", v ? "on" : "off");
                } catch {
                  // session-only
                }
              }}
            />
            Group by title
          </label>
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
            {sections.map(({ name, pts }) => (
              <Fragment key={name || "__all"}>
                {name && (
                  <tr>
                    <td colSpan={4} className="px-2 pb-1 pt-2">
                      <div className="flex w-full items-center gap-3">
                        <span aria-hidden className="h-px flex-1 bg-border" />
                        <button
                          aria-label={`${collapsedGroups.has(name) ? "Expand" : "Collapse"} group ${name}`}
                          title={collapsedGroups.has(name) ? "Expand group" : "Collapse group"}
                          className="text-muted transition-colors hover:text-accent"
                          onClick={() => toggleCollapsed(name)}
                        >
                          {collapsedGroups.has(name) ? (
                            <ChevronRight size={15} />
                          ) : (
                            <ChevronDown size={15} />
                          )}
                        </button>
                        <button
                          className="group"
                          title="Select all test cases in this group"
                          onClick={() => toggleSection(pts)}
                        >
                          <span className="text-sm font-semibold tracking-wide text-muted transition-colors group-hover:text-accent">
                            {name} ({pts.length})
                          </span>
                        </button>
                        <span aria-hidden className="h-px flex-1 bg-border" />
                      </div>
                    </td>
                  </tr>
                )}
                {(name && collapsedGroups.has(name) ? [] : pts).map((p) => (
              <tr
                key={p.point_id}
                className={cn(
                  "cursor-pointer select-none border-b border-border/50 transition-colors",
                  p.test_case_id != null && selected.has(p.test_case_id)
                    ? "bg-accent-soft"
                    : (outcomeRowTint[p.last_outcome.toLowerCase()] ?? ""),
                  "hover:bg-surface-2",
                )}
                onClick={(e) => handleRowClick(p, e)}
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
              </Fragment>
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
            ? "Recording"
            : `Record ${selectedCount} outcome${selectedCount === 1 ? "" : "s"}`}
        </Button>
        {runUrl && (
          <a className="text-sm text-accent underline" href={runUrl} target="_blank" rel="noreferrer">
            View run in Azure DevOps
          </a>
        )}
      </div>

      {/* Floating action bar: stays on screen while scrolling the table. */}
      {suite.data && selected.size > 0 && (
        <div className="fixed bottom-6 right-6 z-40 flex items-center gap-2 rounded-full border border-border bg-surface py-1.5 pl-4 pr-2 shadow-2xl">
          <span className="text-xs font-medium text-muted">{selected.size} selected</span>
          <Button size="sm" onClick={() => openRunner([...selected])}>
            Run {selected.size} in runner
          </Button>
          <button
            aria-label="Clear selection"
            title="Clear selection"
            className="rounded-full p-1.5 text-muted transition-colors hover:text-danger"
            onClick={() => setSelected(new Set())}
          >
            <X size={14} />
          </button>
        </div>
      )}
    </section>
  );
}
