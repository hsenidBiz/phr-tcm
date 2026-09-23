import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronDown, ChevronRight, RefreshCw, X } from "lucide-react";
import { Fragment, useEffect, useMemo, useState, useSyncExternalStore } from "react";
import { createPortal } from "react-dom";
import { toast } from "../../lib/toast";
import { commands, events, type TestPoint } from "../../bindings";
import { onPointRecorded, patchPointRows } from "../../lib/runnerBus";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import { Collapse, useSettled } from "../../components/ui/collapse";
import { Input } from "../../components/ui/input";
import { Select } from "../../components/ui/select";
import HistoryDots from "../../components/HistoryDots";
import ScanProgress from "../../components/ScanProgress";
import { cn } from "../../lib/cn";
import { pagePalette } from "../../lib/reportTheme";
import { CACHE, cacheKeys, persistentQuery } from "../../lib/cache";
import { clearSuiteSeed, readSuiteSeed, writeSuiteSeed } from "../../lib/suiteSeed";
import { usePersistedStringSet } from "../../lib/collapsedGroups";
import {
  sidebarCollapsedSnapshot,
  stickyLeftPx,
  subscribeSidebar,
} from "../../lib/sidebarState";
import { describeAdoError, unwrap, unwrapStr } from "../../lib/ipc";
import { outcomeLabel } from "../../lib/outcomes";
import { openRunnerWindow } from "../../lib/openRunner";
import CasePreview from "./CasePreview";
import { IconCollapseAll, IconReport, IconRun, IconSetOrder } from "../../lib/actionIcons";
import ExecutionOrderModal from "./ExecutionOrderModal";
import { runOrderQueryOptions, useRunOrder } from "./useRunOrder";
import { suiteCasesKey } from "../ManageCases/suiteCasesQuery";
import { loadGroupMode, saveGroupMode, type GroupMode } from "../../lib/runOrder";


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

/** The three columns' widths, shared by every table on the screen so they
 * line up: the first takes what is left. */
function RunCols() {
  return (
    <colgroup>
      <col />
      <col className="w-32" />
      <col className="w-40" />
    </colgroup>
  );
}

const NO_POINTS: TestPoint[] = [];

/** Whether a point passes the outcome and text filters. */
function pointMatches(p: TestPoint, filterOutcome: string, filterText: string): boolean {
  if (filterOutcome === "none" && p.last_outcome) return false;
  if (filterOutcome && filterOutcome !== "none" && p.last_outcome.toLowerCase() !== filterOutcome) return false;
  if (filterText) {
    const t = filterText.toLowerCase();
    if (!p.test_case_name.toLowerCase().includes(t) && !String(p.test_case_id ?? "").includes(t)) return false;
  }
  return true;
}

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
  const [groupMode, setGroupModeState] = useState<GroupMode>(() => loadGroupMode());
  const setGroupMode = (mode: GroupMode) => {
    setGroupModeState(mode);
    saveGroupMode(mode);
  };
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

  // Suite resolution is expensive (scans plans), so the queryFn itself
  // short-circuits to the persisted seed: no matter what triggers a
  // refetch (remount, focus, gc), the network is only hit when no seed
  // exists. Refresh deletes the seed to force a true re-resolve.
  const suite = useQuery({
    queryKey: ["suite", org, project, pbiId],
    queryFn: async () => {
      const seed = readSuiteSeed(org, pbiId);
      if (seed) return seed;
      const s = await unwrap(commands.ensurePbiSuite(org, project, pbiId));
      writeSuiteSeed(org, pbiId, s);
      return s;
    },
    initialData: () => readSuiteSeed(org, pbiId),
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

  // The common refresh: outcomes moved (someone ran tests), the suite did
  // not - plan and suite ids are stable once resolved, so refetching the
  // points and history is two cheap requests instead of re-scanning every
  // test plan in the project.
  const refreshPoints = () => {
    qc.invalidateQueries({ queryKey: ["points"] });
    qc.invalidateQueries({ queryKey: ["run-history"] });
    // The disk seed makes both of these look fresh even right after an
    // upload or a Suite Management save changed them (design doc §4.4);
    // Refresh must force a real re-read, not just repaint from the seed.
    qc.invalidateQueries({ queryKey: runOrderQueryOptions(org, project, pbiId).queryKey });
    if (suite.data) {
      qc.invalidateQueries({
        queryKey: suiteCasesKey(org, project, suite.data.plan_id, suite.data.suite_id),
      });
    }
  };

  // The full re-resolve (Shift-click, or automatic when the cached suite
  // turns out to be deleted): drop the seed and scan plans from scratch.
  const redetectSuite = () => {
    clearSuiteSeed(org, pbiId);
    setScan(null);
    qc.removeQueries({ queryKey: ["suite", org, project, pbiId] });
    qc.invalidateQueries({ queryKey: ["points"] });
  };

  // Last-5 outcome history per case (spec: Execution Depth & Trust, A).
  const history = useQuery({
    queryKey: ["run-history", org, project, suite.data?.plan_id],
    ...persistentQuery({
      key: cacheKeys.runHistory(org, project, suite.data?.plan_id),
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
      key: cacheKeys.points(org, project, suite.data?.plan_id, suite.data?.suite_id),
      fetcher: async () => {
        const r = await commands.listTestPoints(
          org,
          project,
          suite.data!.plan_id,
          suite.data!.suite_id,
        );
        if (r.status === "error") {
          // The cached suite no longer exists in Azure DevOps (plan or
          // suite deleted): self-heal by re-resolving instead of leaving
          // the tab stuck on "Not found." until someone finds Shift-click.
          if (r.error.kind === "NotFound") redetectSuite();
          throw new Error(describeAdoError(r.error));
        }
        return r.data;
      },
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
          // The report covers exactly the highlighted cases - the button
          // does not arm without a selection.
          [...selected],
        ),
      ),
    onError: (e) => toast.error(`Report failed: ${e.message ?? e}`),
  });

  // No dedicated re-run-failures button (it existed for one release): the
  // outcome filter + a group-header click selects the failed set in two
  // clicks, and one path into a selective run is easier to trust than two.

  // The chosen order (suggested / spec / mine) and its sections. Ordering
  // and grouping happen over every point; filters only hide rows after
  // that, so a filter never reorders anything (design doc §5.1).
  const order = useRunOrder({
    org,
    project,
    pbiId,
    suite: suite.data ?? undefined,
    points: points.data ?? NO_POINTS,
    groupMode,
  });

  const filtered = useMemo(
    () => order.ordered.filter((p) => pointMatches(p, filterOutcome, filterText)),
    [order.ordered, filterOutcome, filterText],
  );

  const sections = useMemo(() => {
    if (groupMode === "none") return [{ name: "", pts: filtered }];
    return order.sections
      .map(({ name, pts }) => ({ name, pts: pts.filter((p) => pointMatches(p, filterOutcome, filterText)) }))
      .filter((s) => s.pts.length > 0);
  }, [order.sections, filtered, groupMode, filterOutcome, filterText]);

  // The order is chosen only in the Execution order modal: the list shows
  // it and never edits it (execution-order-modal design §2).
  const [orderOpen, setOrderOpen] = useState(false);

  /** The list's order as the eye reads it - filtered, grouped, flattened.
   * The runner's own fetch (the PBI's Tested-By links) orders differently,
   * so the handoff carries the selection in this order. */
  const visibleCaseOrder = () =>
    sections
      .flatMap((s) => s.pts)
      .map((p) => p.test_case_id)
      .filter((x): x is number => x != null);

  // The runner opens only for an explicit selection - a run-everything
  // button invited accidental 50-case sessions, so picking rows is the
  // one way in (select all via the group checkboxes if that IS the run).
  const openRunner = (caseIds: number[]) =>
    openRunnerWindow({
      org,
      project,
      planId: suite.data!.plan_id,
      planName: suite.data!.plan_name,
      suiteId: suite.data!.suite_id,
      pbi: { id: pbiId, title: pbiTitle, work_item_type: "" },
      caseIds,
    }).catch((e) => toast.error(`Could not open runner: ${e.message ?? e}`));

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
  const settled = useSettled((points.data?.length ?? 0) > 0);

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

  /** Every case the list is currently showing (the filter applies; a
   * collapsed group's cases still count - collapsing hides rows, it does
   * not unpick them). */
  const selectAllVisible = () => {
    const ids = filtered.map((p) => p.test_case_id).filter((x): x is number => x != null);
    if (!ids.length) return;
    setSelected(new Set(ids));
    setAnchor(ids[0]);
  };

  // Ctrl/Cmd+A selects every case instead of the page's text - unless the
  // user is in a text field, where select-all must keep meaning the text.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.key.toLowerCase() !== "a") return;
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable)) return;
      e.preventDefault();
      selectAllVisible();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

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
    <section data-tour="run-list" className="space-y-3 rounded-md border border-border bg-surface p-4">
      <div className="flex items-center justify-between gap-2">
        <h2 className="text-sm font-semibold text-text">Run tests for #{pbiId}</h2>
        {suite.data && (
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              disabled={report.isPending || selected.size === 0}
              title={
                selected.size === 0
                  ? "Select the cases to report on first - click rows to select"
                  : `Report on the ${selected.size} highlighted case${selected.size === 1 ? "" : "s"}`
              }
              onClick={() => report.mutate()}
            >
              <IconReport aria-hidden />
              {report.isPending ? "Building report" : "Execution report"}
            </Button>
          </div>
        )}
      </div>
      <p className="text-xs text-muted">
        Outcomes are recorded through the runner window - a step-by-step
        player with screenshots and bug filing. Click rows to select cases,
        then run them in the runner.
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
            aria-label="Refresh outcomes"
            title="Refresh outcomes (Shift-click: re-detect the test suite)"
            className="rounded p-0.5 text-muted hover:text-accent"
            onClick={(e) => (e.shiftKey ? redetectSuite() : refreshPoints())}
          >
            <RefreshCw
              size={12}
              className={
                suite.isFetching || points.isFetching || history.isFetching
                  ? "animate-spin"
                  : undefined
              }
            />
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
        <div className="space-y-1">
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              disabled={order.loading}
              title={order.loading ? "Loading the run order…" : undefined}
              onClick={() => setOrderOpen(true)}
            >
              <IconSetOrder aria-hidden />
              Set execution order
            </Button>
          </div>
          {order.note && <p className="text-xs text-warning">{order.note}</p>}
        </div>
      )}
      {/* Mounted fresh on every open, keyed by the suggested file's own
          timestamp: a refetch that lands while the modal is open must not
          leave it comparing a reorder against a file that has since moved
          on (execution-order-modal design, Task 1 review). */}
      {orderOpen && suite.data && (
        <ExecutionOrderModal
          key={order.file?.saved_at ?? "none"}
          org={org}
          project={project}
          pbiId={pbiId}
          cases={order.specCases}
          view={order.view}
          file={order.file}
          unreadableNote={order.note}
          loading={order.loading}
          myOrder={order.myOrder}
          groupMode={groupMode}
          hasAreas={order.hasAreas}
          onUseView={order.changeView}
          onUseMine={order.saveMine}
          onGroupMode={setGroupMode}
          onClose={() => setOrderOpen(false)}
        />
      )}

      {points.data && points.data.length > 0 && (
        <div className="flex gap-2">
          <Input
            aria-label="Filter points"
            className="w-56 px-2 py-1.5"
            placeholder="Filter by name or id"
            value={filterText}
            onChange={(e) => setFilterText(e.target.value)}
          />
          <Select
            aria-label="Filter by last outcome"
            triggerClassName="px-2 py-1.5"
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
          {groupMode === "none" && (
            <Button
              variant="outline"
              size="sm"
              disabled={filtered.length === 0}
              title="Select every case shown (Ctrl+A)"
              onClick={selectAllVisible}
            >
              Select all
            </Button>
          )}
        </div>
      )}

      {points.data && points.data.length > 0 && (
        <div>
          {/* One table per group, so a group can fold as one box - a run of
              table rows cannot be wrapped. Every table is fixed-layout with
              the same column widths (RunCols), so the columns line up down
              the screen exactly as they did in one table; the heading is a
              table of its own for the same reason. */}
          {anyRowsShown && (
            <table className="w-full table-fixed border-collapse text-sm">
              <RunCols />
              <thead>
                <tr className="border-b border-border text-left text-xs text-muted">
                  <th className="px-2 py-1 font-medium">Test case</th>
                  <th className="px-2 py-1 font-medium">Last outcome</th>
                  <th className="px-2 py-1 font-medium">History</th>
                </tr>
              </thead>
            </table>
          )}
          {sections.map(({ name, pts }) => {
            const shown = visibleRows(name, pts);
            return (
              <Fragment key={name || "__all"}>
                {name && (
                      <div className="flex w-full items-center gap-3 px-2 pb-1 pt-2">
                        {/* Left-anchored with a trailing rule - see ViewCases for why. */}
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
                        {/* Selection is the checkbox's job; the TITLE
                            toggles the fold, same as the chevron. */}
                        {(() => {
                          const selectable = pts.filter((p) => p.test_case_id != null);
                          const inGroup = selectable.filter((p) =>
                            selected.has(p.test_case_id!),
                          ).length;
                          return (
                            <Checkbox
                              ariaLabel={`Select all in ${name}`}
                              checked={selectable.length > 0 && inGroup === selectable.length}
                              indeterminate={inGroup > 0 && inGroup < selectable.length}
                              onCheckedChange={() => toggleSection(pts)}
                            />
                          );
                        })()}
                        <button
                          className="group flex items-center gap-2"
                          title={collapsedGroups.has(name) ? "Expand group" : "Collapse group"}
                          onClick={() => toggleCollapsed(name)}
                        >
                          <span className="text-sm font-semibold tracking-wide text-muted transition-colors group-hover:text-accent">
                            {name} ({pts.length})
                          </span>
                        </button>
                        <span aria-hidden className="h-px flex-1 bg-linear-to-r from-border to-transparent" />
                      </div>
                )}
                {/* The fold animates the list away only when nothing in it
                    is held open: an open preview stays, so its list stays. */}
                <Collapse open={shown.length > 0} animateIn={settled}>
                <table className="w-full table-fixed border-collapse text-sm">
                <RunCols />
                <tbody>
                {shown.map((p) => (
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
                  <div className="flex items-center gap-1">
                    <button
                      aria-label={expanded.has(p.point_id) ? "Collapse test case" : "Expand test case"}
                      title={expanded.has(p.point_id) ? "Hide steps" : "Show steps & last result"}
                      className="shrink-0 text-muted hover:text-accent"
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
                    <span className="min-w-0 flex-1">
                      <span className="id-mono text-faint">#{p.test_case_id}</span> {p.test_case_name}
                    </span>
                  </div>
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
              <Collapse row={3} open={expanded.has(p.point_id)}>
                <CasePreview
                  org={org}
                  project={project}
                  point={p}
                  history={p.test_case_id != null ? (historyByCase.get(p.test_case_id) ?? []) : []}
                />
              </Collapse>
              </Fragment>
                ))}
                </tbody>
                </table>
                </Collapse>
              </Fragment>
            );
          })}
        </div>
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
            className="fixed bottom-6 z-40 rounded-full border border-accent bg-bg shadow-2xl transition-[left] duration-200"
            style={{ left: stickyLeftPx(sidebarCollapsed) }}
          >
            <Button
              size="sm"
              variant="ghost"
              className="rounded-full text-text hover:bg-surface-2 hover:text-text"
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
          // p-2.5: at p-1.5 the button sat nearly flush with the pill's
          // edge and the pill read as a tight outline, not a surface.
          <div className="fixed bottom-6 right-6 z-40 flex items-center gap-2 rounded-full border border-border bg-surface p-2.5 shadow-2xl">
            {/* No "N selected" text - the count is already in the button's
                own label. */}
            <Button size="sm" onClick={() => openRunner(visibleCaseOrder().filter((id) => selected.has(id)))}>
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
