import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronDown, ChevronRight, RefreshCw, X } from "lucide-react";
import { Fragment, useEffect, useMemo, useState, useSyncExternalStore } from "react";
import { createPortal } from "react-dom";
import { toast } from "sonner";
import { commands, events, type EnsuredSuite, type TestPoint } from "../../bindings";
import { onPointRecorded, patchPointRows } from "../../lib/runnerBus";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import { Input } from "../../components/ui/input";
import { Select } from "../../components/ui/select";
import HistoryDots from "../../components/HistoryDots";
import ScanProgress from "../../components/ScanProgress";
import { cn } from "../../lib/cn";
import { pagePalette } from "../../lib/reportTheme";
import { CACHE, persistentQuery } from "../../lib/persistentQuery";
import { usePersistedStringSet } from "../../lib/collapsedGroups";
import {
  sidebarCollapsedSnapshot,
  stickyLeftPx,
  subscribeSidebar,
} from "../../lib/sidebarState";
import { groupIndices } from "../../lib/grouping";
import { unwrap, unwrapStr } from "../../lib/ipc";
import { outcomeLabel } from "../../lib/outcomes";
import { openRunnerWindow } from "../../lib/openRunner";
import CasePreview from "./CasePreview";
import { IconCollapseAll, IconOpenWindow, IconReport, IconRun } from "../../lib/actionIcons";


export { outcomeLabel };


const outcomeColor: Record<string, string> = {
  passed: "text-success",
  failed: "text-danger",
  paused: "text-muted",
  blocked: "text-warning",
  notapplicable: "text-faint",
};

/** v1 outcome_style parity: soft background tint per last outcome. */
const outcomeRowTint: Record<string, string> = {
  passed: "bg-success/10",
  failed: "bg-danger/10",
  paused: "bg-muted/10",
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
  const [filterText, setFilterText] = useState("");
  const [filterOutcome, setFilterOutcome] = useState("");
  const [selected, setSelected] = useState<Set<number>>(new Set()); // test case ids
  const [scan, setScan] = useState<{ done: number; total: number } | null>(null);
  const [grouped, setGrouped] = useState(
    () => localStorage.getItem("tcm-v2-group-points") === "on",
  );
  const [anchor, setAnchor] = useState<number | null>(null); // shift-range start
  // Open previews, PLURAL (point ids) - same model as View Test Cases:
  // several can be open for comparison, an open one survives its group
  // being collapsed, and the sticky Close all clears the lot.
  const [expanded, setExpanded] = useState<Set<number>>(new Set());
  const toggleExpanded = (pointId: number) =>
    setExpanded((s) => {
      const next = new Set(s);
      if (next.has(pointId)) next.delete(pointId);
      else next.add(pointId);
      return next;
    });
  const [collapsedGroups, toggleCollapsed, collapseGroups] = usePersistedStringSet(
    "tcm-v2-run-collapsed-groups",
  );
  const sidebarCollapsed = useSyncExternalStore(subscribeSidebar, sidebarCollapsedSnapshot);

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

  // Live repaint: the runner announces every recorded (or reset) point as
  // the tester clicks Next; patch the cached rows in place so the tint and
  // outcome column follow the session without refetching the whole suite.
  useEffect(() => {
    const un = onPointRecorded((p) => {
      if (p.org !== org || p.project !== project) return;
      qc.setQueriesData<TestPoint[] | undefined>({ queryKey: ["points"] }, (rows) =>
        patchPointRows(rows, p),
      );
    });
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, [org, project, qc]);

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

  // Last-5 outcome history per case (spec: Execution Depth & Trust, A).
  const history = useQuery({
    queryKey: ["run-history", org, project, suite.data?.plan_id],
    ...persistentQuery({
      key: `run-history:${org}/${project}/${suite.data?.plan_id}`,
      fetcher: () => unwrap(commands.runHistory(org, project, suite.data!.plan_id)),
      ...CACHE.outcomes,
    }),
    enabled: Boolean(suite.data),
    gcTime: 30 * 60_000,
    retry: false,
  });
  const historyByCase = useMemo(
    () => new Map((history.data ?? []).map((h) => [h.test_case_id, h.outcomes])),
    [history.data],
  );

  // Seeded from disk so opening Run Tests paints immediately, then
  // revalidates - outcomes change with every run, so they are never
  // served stale for longer than the refetch takes.
  const points = useQuery({
    queryKey: ["points", org, project, suite.data?.plan_id, suite.data?.suite_id],
    ...persistentQuery({
      key: `points:${org}/${project}/${suite.data?.plan_id}/${suite.data?.suite_id}`,
      fetcher: () =>
        unwrap(commands.listTestPoints(org, project, suite.data!.plan_id, suite.data!.suite_id)),
      ...CACHE.outcomes,
    }),
    enabled: Boolean(suite.data),
    gcTime: 30 * 60_000, // keep the background prefetch alive while unobserved
    retry: false,
  });

  const report = useMutation({
    mutationFn: () =>
      unwrapStr(
        commands.viewExecutionReport(
          org,
          project,
          suite.data!.plan_id,
          [suite.data!.suite_id],
          `PBI #${pbiId} — ${pbiTitle}`,
          // Read at click time, so the report matches the theme in front
          // of the user rather than whatever was set at startup. Both
          // schemes go along, for the switch in the page's corner.
          pagePalette(),
        ),
      ),
    onError: (e) => toast.error(`Report failed: ${e.message ?? e}`),
  });

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

  // No dedicated re-run-failures button (it existed for one release): the
  // outcome filter + a group-header click selects the failed set in two
  // clicks, and one path into a selective run is easier to trust than two.

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

  /** Groups on screen not yet folded - Collapse all folds these too. */
  const openGroupNames = sections
    .map((g) => g.name)
    .filter((n) => n && !collapsedGroups.has(n));
  const collapsible = expanded.size + openGroupNames.length;

  // A collapsed group hides its LIST, not the case someone is reading:
  // rows with an open preview stay rendered until closed - their own
  // chevron, or the sticky Close all. Same rule as View Test Cases.
  const visibleRows = (name: string, pts: TestPoint[]) =>
    name && collapsedGroups.has(name) ? pts.filter((p) => expanded.has(p.point_id)) : pts;

  // With every group shut and nothing held open, the table is nothing but
  // group headings - and a "Test case / Last outcome / History" header
  // sitting above them labels columns that aren't there. It comes back
  // with the first row that shows.
  const anyRowsShown = sections.some(({ name, pts }) => visibleRows(name, pts).length > 0);

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
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              disabled={report.isPending}
              onClick={() => report.mutate()}
            >
              <IconReport aria-hidden />
              {report.isPending ? "Building report" : "Execution report"}
            </Button>
            <Button variant="outline" size="sm" onClick={() => openRunner()}>
              <IconOpenWindow aria-hidden />
              Open runner window
            </Button>
          </div>
        )}
      </div>
      <p className="text-xs text-muted">
        Outcomes are recorded through the runner window - a step-by-step
        player with screenshots and bug filing. Click rows to pick the cases
        for a selective runner session.
      </p>

      {suite.isFetching && !suite.data && (
        <ScanProgress
          label={scan ? "Scanning test plans" : "Resolving test suite"}
          done={scan?.done}
          total={scan?.total}
        />
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
            <RefreshCw size={12} className={suite.isFetching ? "animate-spin" : undefined} />
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
            <option value="paused">Paused</option>
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
          {anyRowsShown && (
            <thead>
              <tr className="border-b border-border text-left text-xs text-muted">
                <th className="px-2 py-1 font-medium">Test case</th>
                <th className="px-2 py-1 font-medium">Last outcome</th>
                <th className="px-2 py-1 font-medium">History</th>
              </tr>
            </thead>
          )}
          <tbody>
            {sections.map(({ name, pts }) => (
              <Fragment key={name || "__all"}>
                {name && (
                  <tr>
                    <td colSpan={3} className="px-2 pb-1 pt-2">
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
                          className="group flex items-center gap-2"
                          title="Select all test cases in this group"
                          onClick={() => toggleSection(pts)}
                        >
                          <span className="text-sm font-semibold tracking-wide text-muted transition-colors group-hover:text-accent">
                            {name} ({pts.length})
                          </span>
                          {/* Same marker as View / Update Test Cases:
                              folding a group hides its rows and the row
                              highlight with them, so the heading has to say
                              something is still selected in there. */}
                          {(() => {
                            const inGroup = pts.filter(
                              (p) => p.test_case_id != null && selected.has(p.test_case_id),
                            ).length;
                            return collapsedGroups.has(name) && inGroup > 0 ? (
                              <span
                                className="selection-dot"
                                role="status"
                                aria-label={`${inGroup} of ${pts.length} selected in ${name}`}
                                title={`${inGroup} selected in this group`}
                              />
                            ) : null;
                          })()}
                        </button>
                        <span aria-hidden className="h-px flex-1 bg-border" />
                      </div>
                    </td>
                  </tr>
                )}
                {visibleRows(name, pts).map((p) => (
              <Fragment key={p.point_id}>
              <tr
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
                  <button
                    aria-label={expanded.has(p.point_id) ? "Collapse test case" : "Expand test case"}
                    title={expanded.has(p.point_id) ? "Hide steps" : "Show steps & last result"}
                    className="mr-1 align-middle text-muted hover:text-accent"
                    onClick={(e) => {
                      e.stopPropagation();
                      toggleExpanded(p.point_id);
                    }}
                  >
                    {expanded.has(p.point_id) ? (
                      <ChevronDown size={14} />
                    ) : (
                      <ChevronRight size={14} />
                    )}
                  </button>
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
                <td className="px-2 py-1">
                  <HistoryDots
                    outcomes={
                      p.test_case_id != null ? (historyByCase.get(p.test_case_id) ?? []) : []
                    }
                  />
                </td>
              </tr>
              {expanded.has(p.point_id) && (
                <tr>
                  <td colSpan={3} className="p-0">
                    <CasePreview
                      org={org}
                      project={project}
                      point={p}
                      history={p.test_case_id != null ? (historyByCase.get(p.test_case_id) ?? []) : []}
                    />
                  </td>
                </tr>
              )}
              </Fragment>
                ))}
              </Fragment>
            ))}
          </tbody>
        </table>
      )}

      {/* Floating action bar, PORTALLED to <body> - and that is the whole
          reason it works. This screen renders inside AnimatedContent, whose
          GSAP transform becomes the containing block for any `fixed`
          descendant, so `bottom-6 right-6` pinned the bar to the bottom of
          the SCROLLABLE REGION rather than the viewport: pick some cases
          near the top of a long suite and the button to run them was
          somewhere below the fold, which is the opposite of a floating
          action bar. Same trap as ui/modal.tsx, CommentModal and
          WorkItemDrawer. Rendering at <body> makes `fixed` mean the
          viewport again, so it stays put while the table scrolls. */}
      {/* Sticky Close all for open previews, bottom LEFT - the selection
          bar owns the right corner, so the two can show together without
          covering each other. Portalled for the same AnimatedContent
          reason as the bar below. */}
      {collapsible > 0 &&
        createPortal(
          /* Left offset clears the sidebar at its CURRENT width - parked at
             left-6 this sat exactly on the sidebar's Close button. */
          <div
            className="fixed bottom-6 z-40 rounded-full border border-border bg-surface shadow-2xl transition-[left] duration-200"
            style={{ left: stickyLeftPx(sidebarCollapsed) }}
          >
            <Button
              size="sm"
              variant="outline"
              className="rounded-full"
              onClick={() => {
                setExpanded(new Set());
                collapseGroups(openGroupNames);
              }}
            >
              <IconCollapseAll aria-hidden />
              Collapse all ({collapsible})
            </Button>
          </div>,
          document.body,
        )}

      {suite.data &&
        selected.size > 0 &&
        createPortal(
          <div className="fixed bottom-6 right-6 z-40 flex items-center gap-2 rounded-full border border-border bg-surface py-1.5 pl-4 pr-2 shadow-2xl">
            <span className="text-xs font-medium text-muted">{selected.size} selected</span>
            <Button size="sm" onClick={() => openRunner([...selected])}>
              <IconRun aria-hidden />
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
          </div>,
          document.body,
        )}
    </section>
  );
}
