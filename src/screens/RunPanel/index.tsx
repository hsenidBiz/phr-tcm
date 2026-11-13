import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronDown, ChevronRight, RefreshCw, X } from "lucide-react";
import { Fragment, useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore, type MouseEvent } from "react";
import { ListSkeleton } from "../../components/ui/list-skeleton";
import { useRowKeys } from "../../lib/useRowKeys";
import { usePruneSelection, useScreenKeys, useScreenSet, useScreenState } from "../../lib/screenState";
import { createPortal } from "react-dom";
import { toast } from "../../lib/toast";
import { commands, events, type SuiteScanProgress, type TestPoint } from "../../bindings";
import { onPointRecorded, patchPointRows } from "../../lib/runnerBus";
import { logUi } from "../../lib/uiLog";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import { Collapse, useSettled } from "../../components/ui/collapse";
import { ListToolbar } from "../../components/ui/list-toolbar";
import { PageHeader } from "../../components/ui/page-header";
import { SearchInput } from "../../components/ui/search-input";
import { Select } from "../../components/ui/select";
import ScanProgress from "../../components/ScanProgress";
import ActionDock from "../../components/ActionDock";
import WidthReveal from "../../components/WidthReveal";
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
import { matchesQuery, searchPlaceholder } from "../../lib/search";
import { openRunnerWindow } from "../../lib/openRunner";
import { RunRows } from "./RunRows";
import { IconCollapseAll, IconReport, IconRun, IconSetOrder } from "../../lib/actionIcons";
import ExecutionOrderModal from "./ExecutionOrderModal";
import { runOrderQueryOptions, useRunOrder } from "./useRunOrder";
import { suiteCasesKey } from "../ManageCases/suiteCasesQuery";
import { loadGroupMode, saveGroupMode, type GroupMode } from "../../lib/runOrder";
import { SpinHolder } from "../../components/ui/spin-holder";


export { outcomeLabel };


const outcomeColor: Record<string, string> = {
  passed: "text-success",
  failed: "text-danger",
  paused: "text-muted",
  blocked: "text-warning",
  // The theme's own ink, as the runner's badge uses for it: Paused is the
  // grey above, and the two must not read as the same "no verdict".
  notapplicable: "text-text",
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
  if (
    filterText &&
    !matchesQuery(
      [p.test_case_id, p.test_case_name, p.tags, outcomeLabel(p.last_outcome) || "Never run", p.tester, p.config_name],
      filterText,
    )
  )
    return false;
  return true;
}

/** A Test plan folder (a static suite) run directly, with no PBI behind it:
 * its plan and suite are known, and its label stands where a PBI's title
 * would. */
export type RunFolder = { planId: number; suiteId: number; planName: string; title: string };

export default function RunPanel({
  org,
  project,
  pbiId = 0,
  pbiTitle = "",
  folder,
  onCloseFolder,
  closeLabel,
}: {
  org: string;
  project: string;
  pbiId?: number;
  pbiTitle?: string;
  /** Run this folder's own cases instead of a PBI's. The suite is handed
   * in (nothing is resolved or cached under a PBI) and pbiId stays 0. */
  folder?: RunFolder;
  /** The way back out of a folder (a button in the header). */
  onCloseFolder?: () => void;
  closeLabel?: string;
}) {
  const qc = useQueryClient();
  // Filters, selection and open rows survive a tab change (lib/screenState).
  const keyOf = useScreenKeys(
    `run:${org}/${project}/${folder ? `folder:${folder.planId}/${folder.suiteId}` : pbiId}`,
  );
  const [filterText, setFilterText] = useScreenState(keyOf("filter"), "");
  const [filterOutcome, setFilterOutcome] = useScreenState(keyOf("outcome"), "");
  const [selected, setSelected] = useScreenSet<number>(keyOf("selected")); // test case ids
  const [scan, setScan] = useState<SuiteScanProgress | null>(null);
  const [groupMode, setGroupModeState] = useState<GroupMode>(() => loadGroupMode());
  const setGroupMode = (mode: GroupMode) => {
    setGroupModeState(mode);
    saveGroupMode(mode);
  };
  const [anchor, setAnchor] = useScreenState<number | null>(keyOf("anchor"), null); // shift-range start
  // Open previews, PLURAL (point ids) - same model as View Test Cases:
  // several can be open for comparison, an open one survives its group
  // being collapsed, and the sticky Close all clears the lot.
  const [expanded, setExpanded] = useScreenSet<number>(keyOf("expanded"));
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
  const isFolder = Boolean(folder);
  const folderSuite = useMemo(
    () => (folder ? { plan_id: folder.planId, plan_name: folder.planName, suite_id: folder.suiteId } : undefined),
    [folder?.planId, folder?.planName, folder?.suiteId], // eslint-disable-line react-hooks/exhaustive-deps
  );
  const suite = useQuery({
    queryKey: folderSuite
      ? ["suite-folder", org, project, folderSuite.plan_id, folderSuite.suite_id]
      : ["suite", org, project, pbiId],
    queryFn: async () => {
      if (folderSuite) return folderSuite;
      const seed = readSuiteSeed(org, pbiId);
      if (seed) return seed;
      const s = await unwrap(commands.ensurePbiSuite(org, project, pbiId));
      writeSuiteSeed(org, pbiId, s);
      return s;
    },
    initialData: () => folderSuite ?? readSuiteSeed(org, pbiId),
    staleTime: Infinity,
    retry: false,
  });

  useEffect(() => {
    // Only the search for THIS PBI's suite: the plans tree reading every
    // plan (Search Suites, the background prefetch) is not this screen's
    // wait, and neither is another PBI's search still running after the
    // person moved on (the background finder cannot be stopped).
    // Starting on another PBI also drops the last PBI's bar: a suite found
    // from its test cases reports no progress, so nothing else would.
    setScan(null);
    // A folder has no PBI to search for.
    if (isFolder) return;
    const un = events.suiteScanProgress.listen((e) => {
      if (e.payload.kind === "suite" && e.payload.pbi_id === pbiId) setScan(e.payload);
    });
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, [org, project, pbiId, isFolder]);

  // Live repaint: the runner announces every recorded (or reset) point as
  // the tester clicks Next; patch the cached rows in place so the tint and
  // outcome column follow the session without refetching the whole suite.
  useEffect(() => {
    const un = onPointRecorded((p) => {
      if (p.org !== org || p.project !== project) {
        logUi(`run list: ignored an announcement for case ${p.testCaseId} (another project)`);
        return;
      }
      const patched = qc.setQueriesData<TestPoint[] | undefined>({ queryKey: ["points"] }, (rows) =>
        patchPointRows(rows, p),
      );
      logUi(`run list: case ${p.testCaseId} is now ${p.outcome || "not run"} (${patched.length} list(s) updated)`);
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
    if (!folder) qc.invalidateQueries({ queryKey: runOrderQueryOptions(org, project, pbiId).queryKey });
    if (suite.data) {
      qc.invalidateQueries({
        queryKey: suiteCasesKey(org, project, suite.data.plan_id, suite.data.suite_id),
      });
    }
  };

  // The full re-resolve (Shift-click, or automatic when the cached suite
  // turns out to be deleted): drop the seed and scan plans from scratch.
  const redetectSuite = () => {
    // A folder is the suite itself: there is nothing to re-detect.
    if (folder) return;
    clearSuiteSeed(org, pbiId);
    setScan(null);
    qc.removeQueries({ queryKey: ["suite", org, project, pbiId] });
    qc.invalidateQueries({ queryKey: ["points"] });
  };

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

  // Last-5 outcome history per case (spec: Execution Depth & Trust, A).
  // Asked for this suite's cases, so the sweep knows when it can stop.
  const historyCaseIds = useMemo(
    () => [...new Set((points.data ?? []).flatMap((p) => (p.test_case_id == null ? [] : [p.test_case_id])))].sort((a, b) => a - b),
    [points.data],
  );
  usePruneSelection(points.data ? historyCaseIds : undefined, setSelected);
  const history = useQuery({
    queryKey: ["run-history", org, project, suite.data?.plan_id, suite.data?.suite_id, historyCaseIds.join(",")],
    ...persistentQuery({
      key: cacheKeys.suiteRunHistory(org, project, suite.data?.plan_id, suite.data?.suite_id),
      fetcher: () => unwrap(commands.runHistory(org, project, suite.data!.plan_id, historyCaseIds)),
      ...CACHE.outcomes,
    }),
    enabled: Boolean(suite.data && points.data && historyCaseIds.length > 0),
    gcTime: 30 * 60_000,
    retry: false,
  });
  const historyByCase = useMemo(
    () => new Map((history.data ?? []).map((h) => [h.test_case_id, h.outcomes])),
    [history.data],
  );

  const report = useMutation({
    mutationFn: () =>
      unwrapStr(
        commands.viewExecutionReport(
          org,
          project,
          suite.data!.plan_id,
          [suite.data!.suite_id],
          folder ? folder.title : `PBI #${pbiId} — ${pbiTitle}`,
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
      pbi: { id: pbiId, title: folder ? folder.title : pbiTitle, work_item_type: "", project },
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
  const handleRowClick = (p: TestPoint, e: MouseEvent) => {
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

  // Stable handlers for the memoized rows (RunRows): each reads the latest
  // closure through a ref, so a row re-renders only when its own flags do.
  const rowClickRef = useRef(handleRowClick);
  rowClickRef.current = handleRowClick;
  const onRowClick = useCallback((p: TestPoint, e: MouseEvent) => rowClickRef.current(p, e), []);
  const toggleRef = useRef(toggleExpanded);
  toggleRef.current = toggleExpanded;
  const onToggleExpanded = useCallback((pointId: number) => toggleRef.current(pointId), []);

  // Arrow keys and Space (backlog A1.8), by point, in the order the rows
  // are on screen (a folded group's rows left out unless held open).
  const shownPoints = sections.flatMap(({ name, pts }) => visibleRows(name, pts));
  const navIds = shownPoints.map((p) => p.point_id);
  const rowKeys = useRowKeys({
    order: navIds,
    onSelect: (pointId, keys) => {
      const p = shownPoints.find((x) => x.point_id === pointId);
      if (p) handleRowClick(p, keys as unknown as MouseEvent);
    },
  });

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
      <PageHeader
        title={folder ? `Run tests for ${folder.title}` : `Run tests for #${pbiId}`}
        info="Outcomes are recorded through the runner window - a step-by-step player with screenshots and bug filing. Click rows to select cases, then run them in the runner."
        actions={
          <>
            {folder && onCloseFolder && (
              <Button variant="ghost" size="sm" onClick={onCloseFolder}>
                <X aria-hidden />
                {closeLabel ?? "Back"}
              </Button>
            )}
            {points.data && points.data.length > 0 && (
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
            )}
            {suite.data && (
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
            )}
          </>
        }
      >
        {suite.data && (
          <p className="flex items-center gap-1.5 text-xs text-faint">
            Plan "{suite.data.plan_name}"
            <button
              aria-label="Refresh outcomes"
              title={folder ? "Refresh outcomes" : "Refresh outcomes (Shift-click: re-detect the test suite)"}
              className="rounded p-0.5 text-muted hover:text-accent"
              onClick={(e) => (e.shiftKey ? redetectSuite() : refreshPoints())}
            >
              <SpinHolder spinning={suite.isFetching || points.isFetching || history.isFetching} className="-m-0.5">
                <RefreshCw size={12} />
              </SpinHolder>
            </button>
            {suite.isFetching && <span>(re-detecting)</span>}
          </p>
        )}
      </PageHeader>

      {suite.isFetching && !suite.data && (
        <ScanProgress
          label={
            scan?.pages
              ? `Reading a large plan (page ${scan.pages})`
              : scan
                ? "Scanning test plans"
                : "Resolving test suite"
          }
          done={scan?.pages ? undefined : scan?.done}
          total={scan?.pages ? undefined : scan?.total}
        />
      )}
      {suite.isError && <p className="text-sm text-danger">{suite.error.message}</p>}
      {points.isFetching && suite.data && !points.data && (
        <ListSkeleton label="Loading test points" rows={6} />
      )}
      {points.isError && <p className="text-sm text-danger">{points.error.message}</p>}
      {points.data && points.data.length === 0 && (
        <p className="text-sm text-muted">
          {folder ? "No test cases in this folder." : "No test points in this suite yet."}
        </p>
      )}

      {points.data && points.data.length > 0 && order.note && (
        <p className="text-xs text-warning">{order.note}</p>
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
        <ListToolbar
          surface
          search={
            <SearchInput
              aria-label="Filter points"
              className="w-full"
              placeholder={searchPlaceholder(["name", "id", "tag", "outcome", "tester", "configuration"])}
              value={filterText}
              onChange={setFilterText}
            />
          }
        >
          <Select
            aria-label="Filter by last outcome"
            triggerClassName="h-10 px-2"
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
              className="h-10"
              disabled={filtered.length === 0}
              title="Select every case shown (Ctrl+A)"
              onClick={selectAllVisible}
            >
              Select all
            </Button>
          )}
          {/* The selection's actions, at the right end of the row directly
              above the list they act on, floating bottom-right once this
              row scrolls away - the same dock Update Test Cases' selection
              uses. `suite.data` is what openRunner needs for the plan id. */}
          <WidthReveal show={!!suite.data && selected.size > 0}>
            {(settled) => suite.data && selected.size > 0 && (
            <ActionDock label="Run selection" surface active={settled} className="flex-nowrap">
              {(floating) => (
                <>
                  {/* No "N selected" text - the count is already in the
                      button's own label. */}
                  <Button
                    size="sm"
                    tabIndex={floating ? -1 : undefined}
                    onClick={() => openRunner(visibleCaseOrder().filter((id) => selected.has(id)))}
                  >
                    <IconRun aria-hidden />
                    Run {selected.size} in runner
                  </Button>
                  <button
                    aria-label="Clear selection"
                    title="Clear selection"
                    tabIndex={floating ? -1 : undefined}
                    className="rounded-full p-1.5 text-muted transition-colors hover:text-danger"
                    onClick={() => setSelected(new Set())}
                  >
                    <X size={14} />
                  </button>
                </>
              )}
            </ActionDock>
            )}
          </WidthReveal>
        </ListToolbar>
      )}

      {points.data && points.data.length > 0 && (
        <div {...rowKeys.listProps}>
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
                          <span className="text-sm font-semibold text-muted transition-colors group-hover:text-accent">
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
                <RunRows
                  rows={shown}
                  selected={selected}
                  expanded={expanded}
                  historyByCase={historyByCase}
                  org={org}
                  project={project}
                  rowTint={outcomeRowTint}
                  outcomeColor={outcomeColor}
                  onRowClick={onRowClick}
                  onToggleExpanded={onToggleExpanded}
                  tabKey={rowKeys.tabKey}
                  onRowFocus={rowKeys.onRowFocus}
                />
                </table>
                </Collapse>
              </Fragment>
            );
          })}
        </div>
      )}

      {/* Sticky Close all for open previews, bottom LEFT - the selection
          dock owns the right corner, so the two can show together without
          covering each other. PORTALLED to <body>: this screen renders
          inside the screen fade, whose transform becomes the
          containing block for any `fixed` descendant, so without the
          portal `bottom-6` would pin to the bottom of the SCROLLABLE REGION
          rather than the viewport. Same trap as ui/modal.tsx and
          ActionDock. */}
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
    </section>
  );
}
