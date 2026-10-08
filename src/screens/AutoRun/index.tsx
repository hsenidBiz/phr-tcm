// Supervised and unattended auto-run: the app drives a real Edge window
// through a case's steps, either with a person watching and deciding the
// verdict, or unattended with the machine proposing one.
//
// Nothing a script or a run does reaches Azure DevOps by itself. A
// person reviewing a finished run and pressing Send (`RunReview`) is the
// one door out - see `autorun::publish` on the Rust side.

import { ChevronDown, ChevronRight } from "lucide-react";
import { useMutation, useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useId, useMemo, useRef, useState, useSyncExternalStore, type KeyboardEvent } from "react";
import { createPortal } from "react-dom";
import { sidebarCollapsedSnapshot, stickyLeftPx, subscribeSidebar } from "../../lib/sidebarState";
import { commands, events, type AutorunResetNeeded, type PbiHit, type PlanView } from "../../bindings";
import { Checkbox } from "../../components/ui/checkbox";
import MoreActionsMenu from "../../components/MoreActionsMenu";
import { Collapse, useSettled } from "../../components/ui/collapse";
import { groupIndices } from "../../lib/grouping";
import { usePersistedStringSet } from "../../lib/collapsedGroups";
import { Button } from "../../components/ui/button";
import ActionDock from "../../components/ActionDock";
import { useFieldRefs } from "../../hooks/useFieldRefs";
import { cn } from "../../lib/cn";
import { useDiscoveryActive } from "../../lib/discoveryActive";
import { unwrap, unwrapStr } from "../../lib/ipc";
import {
  IconCancel,
  IconClearScripts,
  IconCollapseAll,
  IconExpandAll,
  IconRun,
  IconUnattended,
} from "../../lib/actionIcons";
import { open } from "@tauri-apps/plugin-dialog";
import { toast } from "../../lib/toast";
import { logUi } from "../../lib/uiLog";
import {
  cancelRunSetup,
  clearReviewRequest,
  onRunEnded,
  openRunSetup,
  runBlockedReason,
  runIsGoing,
  useBackgroundRun,
} from "../../lib/backgroundRun";
import { Modal } from "../../components/ui/modal";
import CaseCard from "./CaseCard";
import CaseSearch, { matchesSearch } from "./CaseSearch";
import ExecutionOrderDialog from "./ExecutionOrderDialog";
import ResetNeededPanel from "./ResetNeededPanel";
import { fetchPlan } from "./plan";
import PastRuns from "./PastRuns";
import ReadinessStrip from "./ReadinessStrip";
import RunPane from "./RunPane";
import RunReview from "./RunReview";
import ScriptEditor from "./ScriptEditor";
import SetupPanel, { useAutoRunSetup } from "./SetupPanel";
import { ResultToggleRow } from "./ResultFilterRow";
import {
  RESULT_BUCKETS,
  lastRecords,
  lastResultFor,
  lastResults,
  type ResultBucket,
  type ResultFilter,
} from "./verdicts";
import { siteHost } from "./SiteAddressDialog";
import type { AutoRunTab } from "./useAutoRunReadiness";

/** The screen's tabs, in order. The arrow keys walk this list. Setup is
 * not one: it is a panel beside the Test cases list. */
/** The content on the left, the Setup panel on the right; stacked (panel
 * first) until the area's own width has room for both: 69rem is the list's
 * 44rem minimum, a 1rem gap and the panel's 24rem minimum. */
const TWO_COLUMNS =
  "grid gap-4 @min-[69rem]:grid-cols-[minmax(0,1fr)_clamp(24rem,28%,30rem)] @min-[69rem]:items-start";

const TABS: { id: AutoRunTab; label: string }[] = [
  { id: "cases", label: "Test cases" },
  { id: "runs", label: "Past runs" },
];

// How many imported case ids the success toast spells out before it falls
// back to a count - the same shape as the assigned-work notification
// summary. A 60-case import naming every one of them is unreadable.
const MAX_IDS_IN_TOAST = 10;

/** The filter that lets every case through. */
const NO_FILTER: ReadonlySet<ResultBucket> = new Set();
/** No card open. */
const NONE_OPEN: ReadonlySet<number> = new Set();

export default function AutoRun({
  org,
  project,
  pbi,
}: {
  org: string;
  project: string;
  pbi: PbiHit | null;
}) {
  const { prefs } = useFieldRefs(org, project);

  const cases = useQuery({
    queryKey: ["autorun-cases", org, pbi?.id, prefs.moduleRef, prefs.preconditionsRef],
    queryFn: () =>
      unwrap(
        commands.pbiTestCasesFull(org, pbi!.id, prefs.moduleRef, prefs.preconditionsRef),
      ),
    enabled: Boolean(org && pbi),
    retry: false,
  });

  // One script lookup per case, so the list can say which are drivable.
  // `combine` hands back just each script, and TanStack keeps the array the
  // same object while no script has changed - the readiness check reads
  // it, and would otherwise redo its work on every render.
  const scripts = useQueries({
    queries: (cases.data ?? []).map((c) => ({
      queryKey: ["autorun-script", c.id],
      queryFn: () => unwrapStr(commands.autoRunLoadScript(c.id)),
      retry: false,
    })),
    combine: (results) => results.map((q) => q.data),
  });

  const [editing, setEditing] = useState<number | null>(null);
  // The case whose suspected-defect Clear is waiting on Keep / Clear.
  const [confirmingClear, setConfirmingClear] = useState<number | null>(null);
  const [clearScriptsOpen, setClearScriptsOpen] = useState(false);
  const queryClient = useQueryClient();
  /** Whether the assistant's discovery holds the Auto Run browser. */
  const discovering = useDiscoveryActive();

  /** What the Setup panel and the readiness strip report, and whether a
   * run has what it needs. */
  const setup = useAutoRunSetup({ org, project, scripts });
  const { readiness, essentialsSettled, activeEnv, site, knownSiteUrl } = setup;

  /** Which tab shows. The screen always opens on Test cases; after that
   * only the person's clicks, and a review closing, move it. */
  const [tab, setTab] = useState<AutoRunTab>("cases");
  const sidebarCollapsed = useSyncExternalStore(subscribeSidebar, sidebarCollapsedSnapshot);
  const shown = tab;
  /** Past runs' result filter. Here rather than in the panel, which is not
   * mounted while another tab shows - the choice outlives it. */
  const [runsFilter, setRunsFilter] = useState<ResultFilter>("All");

  /** Whether the Setup panel shows its full rows. `null` until the screen
   * has decided, once: open when something a run cannot go without is
   * missing, shut otherwise. After that only the person moves it (the
   * toggle, or anything that asks to open the setup), so setup that
   * changes later never springs it open under anyone. */
  const [panelChoice, setPanelChoice] = useState<boolean | null>(null);
  useEffect(() => {
    if (panelChoice === null && essentialsSettled) setPanelChoice(readiness.essentialMissing);
  }, [panelChoice, essentialsSettled, readiness.essentialMissing]);
  // The render that first knows the answer shows it, rather than one frame
  // of the other state before the effect stores it.
  const panelOpen = panelChoice ?? (essentialsSettled ? readiness.essentialMissing : false);
  const panelRef = useRef<HTMLElement | null>(null);
  const panelToggleRef = useRef<HTMLButtonElement | null>(null);
  /** Set by anything that asks for the setup (the strip's Open setup):
   * once the panel is open and drawn, it is scrolled to and its toggle
   * takes the focus, so the keyboard lands where the rows are. */
  const [focusPanel, setFocusPanel] = useState(false);
  useEffect(() => {
    if (!focusPanel) return;
    panelRef.current?.scrollIntoView?.({ block: "nearest", behavior: "smooth" });
    panelToggleRef.current?.focus();
    setFocusPanel(false);
  }, [focusPanel]);
  const openSetup = () => {
    setPanelChoice(true);
    setFocusPanel(true);
  };

  const tabIds = useId();
  const tabRefs = useRef<Partial<Record<AutoRunTab, HTMLButtonElement | null>>>({});
  /** Set when a closing review sends the person to Past runs: the button
   * they pressed to open it is gone by then (the run that opened it was
   * on Test cases), so focus would fall to the page. */
  const [focusRunsTab, setFocusRunsTab] = useState(false);
  useEffect(() => {
    if (!focusRunsTab) return;
    tabRefs.current.runs?.focus();
    setFocusRunsTab(false);
  }, [focusRunsTab]);
  /** Arrow keys move between the tabs (wrapping), Home and End go to the
   * ends - and the chosen tab takes the focus, as a tab list's should. */
  const onTabKey = (e: KeyboardEvent<HTMLButtonElement>, i: number) => {
    // Alt+Left is the browser's "back" (and Ctrl/Meta+Arrow are other
    // shortcuts' too): a tab list only answers the bare keys.
    if (e.altKey || e.ctrlKey || e.metaKey) return;
    const last = TABS.length - 1;
    const next =
      e.key === "ArrowRight"
        ? i === last
          ? 0
          : i + 1
        : e.key === "ArrowLeft"
          ? i === 0
            ? last
            : i - 1
          : e.key === "Home"
            ? 0
            : e.key === "End"
              ? last
              : null;
    if (next === null) return;
    e.preventDefault();
    const id = TABS[next].id;
    setTab(id);
    tabRefs.current[id]?.focus();
  };

  /** The run count on the Past runs tab. The same query PastRuns reads, so
   * a saved or cleared run moves the count without that tab being open. */
  const runs = useQuery({
    queryKey: ["autorun-runs"],
    queryFn: () => commands.autoRunListRuns(),
    retry: false,
  });
  const runCount = runs.data ? runs.data.length : null;

  /** One file, many cases - the shape `save_autorun_script` writes, so an
   * assistant's whole-PBI output imports in one go. Every script
   * query is invalidated afterwards, or the rows would keep offering "Add
   * script" and no Run for the cases that just gained one.
   *
   * The path goes to Rust rather than reading the file here and sending
   * its contents: `readFileB64` + `atob` decodes to a latin-1 binary
   * string, so any non-ASCII byte (an accent, a curly quote) came out
   * mojibake, and a UTF-8 BOM made the JSON look corrupt before it ever
   * reached the parser. Rust reads the bytes itself now, the same way
   * the test-case JSON importer does. */
  const importScripts = useMutation({
    mutationFn: async () => {
      const path = await open({
        multiple: false,
        filters: [{ name: "Action scripts", extensions: ["json"] }],
      });
      if (typeof path !== "string") return null;
      const r = await commands.autoRunImportScripts(org, project, path);
      if (r.status === "error") throw new Error(r.error);
      return r.data;
    },
    onSuccess: async (ids) => {
      if (!ids) return;
      await queryClient.invalidateQueries({ queryKey: ["autorun-script"] });
      const shown = ids.slice(0, MAX_IDS_IN_TOAST);
      const rest = ids.length - shown.length;
      const caseList = `${shown.join(", ")}${rest > 0 ? `, and ${rest} more` : ""}`;
      toast.success(
        `Imported ${ids.length} script${ids.length === 1 ? "" : "s"} (case${
          ids.length === 1 ? "" : "s"
        } ${caseList}).`,
      );
    },
    // The generated `typedError` wrapper rethrows when the IPC call itself
    // rejects with an Error (rather than resolving to {status: "error"}) -
    // react-query's mutation still routes that here, same as an explicit
    // throw above, so this is the one place that needs to handle it.
    onError: (e) => toast.error(`Could not import that file: ${e.message}`),
  });
  /** The case ids queued for a run. `null` means no run is open. */
  const [running, setRunning] = useState<number[] | null>(null);
  /** The unattended run lives in lib/backgroundRun, not here: it outlives
   * this screen. Its window is mounted at App level. */
  const background = useBackgroundRun();
  /** One run at a time: while the store holds a run going (or paused at a
   * reset point), every Run button here waits for it. */
  const runGoing = runIsGoing(background);
  /** Why every way of starting a run here waits, when it does: a run going,
   * or the assistant's discovery holding the browser. Said in the title. */
  const runBlocked = runBlockedReason(background, discovering);
  /** The plan the supervised pane pauses by at each reset point. */
  const [runPlan, setRunPlan] = useState<PlanView | null>(null);
  /** An unattended run paused at a reset point that the store does not
   * know: one this app did not start in this session (the window was
   * reloaded while it ran). A run the store holds has its pause shown by
   * its own window, reopened from the title-bar pill, so this stands
   * aside for it and the two never show at once. The run waits with no
   * time limit, so it is found again here. */
  const [waitingReset, setWaitingReset] = useState<AutorunResetNeeded | null>(null);
  const [answeringReset, setAnsweringReset] = useState(false);
  useEffect(() => {
    if (runGoing) {
      setWaitingReset(null);
      return;
    }
    let live = true;
    commands
      .autoRunWaitingReset()
      .then((r) => {
        if (live && r) setWaitingReset(r);
      })
      .catch(() => {});
    const un = events.autorunResetNeeded.listen((e) => {
      if (live) setWaitingReset(e.payload);
    });
    return () => {
      live = false;
      un.then((f) => f()).catch(() => {});
    };
  }, [runGoing]);
  const answerWaitingReset = async (continueRun: boolean) => {
    if (!waitingReset) return;
    setAnsweringReset(true);
    try {
      const r = await commands.autoRunAnswerReset(waitingReset.run_id, continueRun);
      if (r.status === "error") logUi(`auto-run: the reset answer was refused: ${r.error}`);
    } catch (e) {
      logUi(`auto-run: the reset answer failed: ${e instanceof Error ? e.message : String(e)}`);
    }
    setWaitingReset(null);
    setAnsweringReset(false);
  };
  /** The Execution order dialog, with the cases it orders. */
  const [orderingOpen, setOrderingOpen] = useState(false);
  /** The run id under review, or null while no review dialog is open. An
   * unattended run opens straight into this once it finishes - see the
   * review request below. */
  const [reviewing, setReviewing] = useState<string | null>(null);
  /** Replay to step N, pressed in Past runs or the review: the supervised
   * pane opens on the case (`running`) and replays it up to `step`. The
   * title is the run's, for a case no longer listed under this PBI. */
  const [replayTo, setReplayTo] = useState<{ caseId: number; title: string; step: number } | null>(null);
  const replayCase = (caseId: number, title: string, step: number) => {
    // One run at a time: the buttons say so, and this is the backstop.
    if (runIsGoing()) return;
    // The review is a dialog of its own: closing it leaves the pane in front.
    setReviewing(null);
    setReplayTo({ caseId, title, step });
    setRunning([caseId]);
  };
  /** Ticked cases, by id. A bulk run is these, in list order. */
  const [selected, setSelected] = useState<Set<number>>(new Set());
  // A finished unattended run has run the selection, whether its window was
  // open or not: leaving it ticked invites a second run of cases that were
  // just decided.
  useEffect(
    () =>
      onRunEnded((e) => {
        if (e.ok) setSelected(new Set());
      }),
    [],
  );
  /** A finished run's review, handed over by the store: straight into it
   * when its window was open at the finish, and on Past runs when the
   * person pressed Review on the pill or the toast. App has already brought
   * the screen to the run's PBI. */
  const reviewRequest = background.review;
  useEffect(() => {
    if (!reviewRequest || !pbi || reviewRequest.pbi.id !== pbi.id || reviewRequest.org !== org) return;
    clearReviewRequest();
    if (reviewRequest.toPastRuns) setTab("runs");
    setReviewing(reviewRequest.runId);
  }, [reviewRequest, pbi, org]);
  // A run window still on its setup belongs to the PBI it was opened for:
  // another PBI, or leaving the screen, cancels it. A run that started
  // carries on.
  useEffect(() => () => cancelRunSetup(), [pbi?.id]);
  const [grouped, setGrouped] = useState(
    () => localStorage.getItem("tcm-v2-autorun-group") === "on",
  );
  const [collapsed, toggleCollapsed] = usePersistedStringSet("tcm-v2-autorun-collapsed");
  /** The last results the Test cases tab shows. Empty is every case. Lives
   * here, not in the tab, so it outlasts a trip to another tab and is gone
   * once the screen is left. */
  const [lastFilter, setLastFilter] = useState<ReadonlySet<ResultBucket>>(new Set());
  /** What the search box holds. Lives here for the same reason as the
   * filter, and is not saved. */
  const [search, setSearch] = useState("");
  /** The open cards, per PBI, while the screen is mounted. Not saved: a
   * list that comes back half open on the next visit reads as clutter. */
  const [openCards, setOpenCards] = useState<Record<number, ReadonlySet<number>>>({});
  const openHere = (pbi && openCards[pbi.id]) || NONE_OPEN;
  const setOpenHere = (change: (prev: ReadonlySet<number>) => ReadonlySet<number>) => {
    if (!pbi) return;
    const key = pbi.id;
    setOpenCards((all) => ({ ...all, [key]: change(all[key] ?? NONE_OPEN) }));
  };
  const toggleCard = (id: number) =>
    setOpenHere((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  const expandAll = (ids: number[]) => setOpenHere((prev) => new Set([...prev, ...ids]));
  const collapseAll = () => setOpenHere(() => NONE_OPEN);

  const rows = cases.data ?? [];
  const settled = useSettled(rows.length > 0);
  /** Each case's result in the newest run that holds it - the runs Past
   * runs lists, so the two tabs never disagree. */
  const last = useMemo(() => lastResults(runs.data ?? []), [runs.data]);
  /** The same newest run for each case, with its record: an open card's
   * last-run downloads come from the run its result came from. */
  const lastRec = useMemo(() => lastRecords(runs.data ?? []), [runs.data]);
  /** Until the runs are read there is nothing to filter by, and if they
   * cannot be read there never will be: either way every case shows. */
  const runsReady = runs.data !== undefined;
  const runsFailed = runs.isError && !runsReady;
  const activeFilter: ReadonlySet<ResultBucket> = runsReady ? lastFilter : NO_FILTER;
  const lastOf = (i: number) => lastResultFor(last, rows[i].id);
  /** How many of ALL the rows each result holds, whatever is pressed. */
  const lastCounts = RESULT_BUCKETS.reduce(
    (acc, b) => ({ ...acc, [b]: rows.filter((_, i) => lastOf(i) === b).length }),
    {} as Record<ResultBucket, number>,
  );
  /** The rows the filters and the search let through, by index in `rows`.
   * Everything that acts on "what is shown" (Select all shown, a group's
   * Select all, the selection itself) reads this one list. */
  const shownIdx = rows.flatMap((c, i) =>
    (activeFilter.size === 0 || activeFilter.has(lastOf(i))) && matchesSearch(c, search) ? [i] : [],
  );
  const shownIds = new Set(shownIdx.map((i) => rows[i].id));
  /** Same title-prefix grouping View Test Cases uses, so a person reading
   * both screens is reading one idea. */
  const allGroups = useMemo(
    () => (grouped ? groupIndices(rows.map((c) => c.title)) : []),
    [grouped, rows],
  );
  /** A group keeps only its shown rows, and one with none is gone. */
  const groups = allGroups
    .map((g) => ({ ...g, indices: g.indices.filter((i) => shownIds.has(rows[i].id)) }))
    .filter((g) => g.indices.length > 0);
  /** The Module values of the loaded cases, for the Areas dialog's
   * picker. A person can still type one that is not here. */
  const caseModules = useMemo(
    () =>
      Array.from(new Set(rows.map((c) => c.module_value.trim()).filter(Boolean))).sort((a, b) =>
        a.localeCompare(b),
      ),
    [rows],
  );

  /** The one Setup panel, drawn on whichever tab shows; its open state is
   * the one above, so it carries across a tab switch. */
  const setupPanel = (
    <SetupPanel
      setup={setup}
      org={org}
      project={project}
      caseModules={caseModules}
      open={panelOpen}
      onToggle={() => setPanelChoice(!panelOpen)}
      toggleRef={panelToggleRef}
      panelRef={panelRef}
    />
  );
  const hasScript = (i: number) => Boolean(scripts[i]);
  /** Only scripted cases can be run, so only they can be ticked. */
  const runnableIn = (indices: number[]) =>
    indices.filter(hasScript).map((i) => rows[i].id);

  /** Development-only housekeeping: wipe the saved scripts for every case
   * currently listed for this PBI. A missing script for one of them is not
   * an error - `store::clear_scripts` skips it - so this always hands over
   * the full list rather than just the ones the rows show as scripted. */
  const clearScripts = useMutation({
    mutationFn: () => unwrapStr(commands.autoRunClearScripts(rows.map((c) => c.id))),
    onSuccess: async (removed) => {
      setClearScriptsOpen(false);
      await queryClient.invalidateQueries({ queryKey: ["autorun-script"] });
      toast.success(`${removed} script${removed === 1 ? "" : "s"} removed.`);
    },
    onError: (e) => toast.error(`Could not clear scripts: ${e.message}`),
  });

  const toggleOne = (id: number) =>
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  /** A group heading ticks or clears every scripted case under it. The
   * "are they all on already" question is asked of `prev` inside the
   * updater, not of the `selected` this render closed over. */
  const toggleGroup = (indices: number[]) => {
    const ids = runnableIn(indices);
    setSelected((prev) => {
      const allOn = ids.length > 0 && ids.every((id) => prev.has(id));
      const next = new Set(prev);
      for (const id of ids) {
        if (allOn) next.delete(id);
        else next.add(id);
      }
      return next;
    });
  };

  const toggleLast = (b: ResultBucket) =>
    setLastFilter((prev) => {
      const next = new Set(prev);
      if (next.has(b)) next.delete(b);
      else next.add(b);
      return next;
    });

  /** A case the filters or the search hide leaves the selection, so a run
   * only ever holds what the person can see. Checked after every render
   * rather than in the filter's click, because the runs refreshing (one just finished) can hide
   * a ticked case without anyone pressing anything. Not while the case list
   * itself is empty (loading): that is not the person's filter hiding them.
   *
   * Only an actual removal sets the selection. Setting it on every render,
   * even to the same set, queued one more update per draw; with hundreds of
   * scripts answering one after another, React took that chain for an
   * endless loop (error #185) and the screen crashed. */
  useEffect(() => {
    if (rows.length === 0) return;
    if ([...selected].every((id) => shownIds.has(id))) return;
    setSelected((prev) => {
      const kept = [...prev].filter((id) => shownIds.has(id));
      return kept.length === prev.size ? prev : new Set(kept);
    });
  });

  /** The scripted cases shown now - what Select all shown covers. */
  const shownRunnable = runnableIn(shownIdx);
  const shownTicked = shownRunnable.filter((id) => selected.has(id)).length;

  /** List order, not click order - the run reads top to bottom the way
   * the screen does. */
  const selectedInOrder = rows.filter((c) => selected.has(c.id)).map((c) => c.id);

  /** Both run buttons start from the plan, not from list order: the saved
   * order for this PBI, else the suggested one. A plan that cannot be had
   * leaves list order, as before. */
  const [planning, setPlanning] = useState(false);
  const planningRef = useRef(false);
  const pbiIdNow = useRef<number | undefined>(undefined);
  pbiIdNow.current = pbi?.id;
  const startPlanned = async (kind: "supervised" | "unattended") => {
    // One at a time: a second click while the plan is on its way would
    // start a second run.
    if (!pbi || planningRef.current) return;
    planningRef.current = true;
    setPlanning(true);
    const asked = pbi.id;
    try {
      const plan = await fetchPlan(org, project, asked, selectedInOrder);
      // The person moved to another PBI while the plan was on its way.
      if (pbiIdNow.current !== asked) return;
      const order = plan?.order ?? selectedInOrder;
      if (kind === "supervised") {
        setRunPlan(plan);
        setRunning(order);
      } else {
        const picked = order
          .map((id) => rows.find((x) => x.id === id))
          .filter((c): c is (typeof rows)[number] => Boolean(c))
          .map((c) => ({ id: c.id, title: c.title, module: c.module_value, steps: c.steps }));
        if (picked.length > 0) openRunSetup({ org, project, pbi, cases: picked, plan });
      }
    } catch (e) {
      // The raw error goes to the app log; the person gets a sentence.
      logUi(`auto-run: the plan for PBI ${asked} could not be worked out: ${e instanceof Error ? e.message : String(e)}`);
      if (pbiIdNow.current === asked) toast.error("Could not work out the order. Try again, or see Settings → Logs.");
    } finally {
      planningRef.current = false;
      setPlanning(false);
    }
  };


  /** One case card, by its index in `rows` - grouped and flat both render
   * the same thing, and `scripts[i]` is indexed the same way. */
  const card = (i: number) => {
    const c = rows[i];
    const rec = lastRec.get(c.id);
    return (
      <CaseCard
        key={c.id}
        c={c}
        org={org}
        project={project}
        script={scripts[i]}
        result={lastOf(i)}
        selected={selected.has(c.id)}
        onSelect={() => toggleOne(c.id)}
        open={openHere.has(c.id)}
        onToggleOpen={() => toggleCard(c.id)}
        onEdit={() => setEditing(c.id)}
        onRun={() => {
          if (!runIsGoing()) setRunning([c.id]);
        }}
        runBlocked={runBlocked}
        confirmingClear={confirmingClear === c.id}
        onAskClear={() => setConfirmingClear(c.id)}
        onClearDone={() => setConfirmingClear(null)}
        lastRun={rec ? { runId: rec.run.id, steps: rec.record.steps } : undefined}
      />
    );
  };

  if (!org || !pbi) {
    return <p className="max-w-prose text-sm text-muted">Pick a PBI in the bar above to auto-run its cases.</p>;
  }

  const caseCount = cases.data ? rows.length : null;
  const searched = search.trim();
  // Every open card counts, shown or not: a card a search or a filter hides
  // is still open, and Collapse all is how it gets shut.
  const anyOpen = rows.some((c) => openHere.has(c.id));

  return (
    <>
      {/* Two tabs, one panel at a time: the cases to run, with the setup a
          run needs beside them, and what came of past runs. Normal page
          flow, so the page's own bottom padding keeps the floating dock
          clear of the last card. Test cases may grow wider, for the Setup
          panel's column, when its own width has room for one. */}
      <div className="space-y-4">
        {/* The Templates/Flows tab pattern from API Templates, with the
            keyboard a tab list owes: only the chosen tab is in the Tab
            order, and the arrow keys move along. */}
        <div role="tablist" aria-label="Auto Run sections" className="flex gap-1 border-b border-border">
          {TABS.map(({ id, label }) => {
            const isOn = shown === id;
            const count = id === "cases" ? caseCount : runCount;
            return (
              <button
                key={id}
                ref={(el) => {
                  tabRefs.current[id] = el;
                }}
                id={`${tabIds}-${id}-tab`}
                role="tab"
                aria-selected={isOn}
                aria-controls={isOn ? `${tabIds}-${id}-panel` : undefined}
                tabIndex={isOn ? 0 : -1}
                className={cn(
                  "-mb-px border-b-2 px-3 py-1.5 text-sm font-medium transition-colors",
                  isOn ? "border-accent text-text" : "border-transparent text-muted hover:text-accent",
                )}
                onClick={() => setTab(id)}
                onKeyDown={(e) => onTabKey(e, TABS.findIndex((t) => t.id === id))}
              >
                <span className="label-trim">
                  {label}
                  {count != null && (
                    <>
                      {" "}
                      <span className="text-xs text-faint">{count}</span>
                    </>
                  )}
                </span>
              </button>
            );
          })}
        </div>

        <div
          role="tabpanel"
          id={`${tabIds}-${shown}-panel`}
          aria-labelledby={`${tabIds}-${shown}-tab`}
          className="@container min-w-0"
        >
          {shown === "cases" && (
            // The list on the left and the Setup panel on the right, at a
            // fixed readable width. Measured on this panel's own width, not
            // the window's, so an open sidebar never squeezes the list: the
            // panel goes beside the list only where the list keeps its full
            // reading width (48rem, a 1rem gap, then 20rem). Narrower, the
            // panel goes above the list at the list's width: it comes first
            // in the page, and moves last only where there is room.
            <div className={TWO_COLUMNS}>
              <div className="min-w-0 @min-[69rem]:order-last">
                {setupPanel}
              </div>

              <section className="min-w-0 space-y-2">
                <h2 className="text-sm font-semibold text-text">
                  Test cases
                  {cases.data && <span className="ml-1.5 font-normal text-faint">({rows.length})</span>}
                </h2>
                {/* Where runs go and whether the setup is in place, in one
                    line. Only once the three things a run cannot go without
                    are known, so a slow read never shows as a warning. */}
                {essentialsSettled && (
                  <ReadinessStrip
                    envName={activeEnv?.name ?? null}
                    // undefined: the address could not be told (a read failed).
                    siteHost={
                      knownSiteUrl === undefined ? undefined : site.start_url ? siteHost(site.start_url) : null
                    }
                    signIn={setup.signIn}
                    accountCount={setup.accountCount}
                    missingTestFiles={readiness.missingTestFiles}
                    unreadable={setup.unreadable}
                    onOpenSetup={openSetup}
                  />
                )}

                {/* Finding cases: the search, the last-result filters, and
                    opening or closing every card shown. */}
                <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
                  <CaseSearch value={search} onChange={setSearch} />
                  {rows.length > 0 &&
                    (runsFailed ? (
                      <span className="text-xs text-muted">Past results could not be read</span>
                    ) : (
                      // Without numbers, and out of reach, until the runs are read.
                      <ResultToggleRow
                        label="Filter by last result"
                        pressed={activeFilter}
                        onToggle={toggleLast}
                        counts={runsReady ? lastCounts : undefined}
                        disabled={!runsReady}
                      />
                    ))}
                </div>

                <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
                  <label className="flex cursor-pointer items-center gap-2 text-xs text-muted">
                    <Checkbox
                      checked={grouped}
                      ariaLabel="Group by title"
                      onCheckedChange={(on) => {
                        setGrouped(on);
                        try {
                          localStorage.setItem("tcm-v2-autorun-group", on ? "on" : "off");
                        } catch {
                          // storage unavailable -> the choice lasts this session
                        }
                      }}
                    />
                    Group by title
                  </label>
                  {/* Ticks what the search and the filters show (scripted
                      cases only), so "run everything that failed" is two
                      clicks. Off while nothing shown could be run. */}
                  <label
                    className={cn(
                      "flex items-center gap-2 text-xs",
                      shownRunnable.length === 0 ? "text-faint" : "cursor-pointer text-muted",
                    )}
                  >
                    <Checkbox
                      checked={shownRunnable.length > 0 && shownTicked === shownRunnable.length}
                      indeterminate={shownTicked > 0 && shownTicked < shownRunnable.length}
                      disabled={shownRunnable.length === 0}
                      ariaLabel="Select all shown"
                      onCheckedChange={() => toggleGroup(shownIdx)}
                    />
                    Select all shown
                  </label>
                  {/* The rare actions. Clear scripts is housekeeping shown
                      wherever Auto Run is (dev, or unlocked) - the whole tab
                      is gated in one place (`autoRunVisible` in
                      lib/extras.ts), so no further gating belongs here.
                      Disabled rather than hidden: an item that vanishes the
                      moment it would do nothing invites "where did it go".
                      Danger only on hover, and it still asks first. */}
                  <span className="ml-auto">
                    <MoreActionsMenu
                      label="More"
                      actions={[
                        {
                          label: "Import scripts",
                          description: "One JSON file can carry every case in this PBI.",
                          disabled: importScripts.isPending,
                          onSelect: () => importScripts.mutate(),
                        },
                        {
                          label: "Execution order",
                          description: "The order Auto Run uses for this PBI, and where the shared state is put back.",
                          disabled: !rows.some((_, i) => hasScript(i)),
                          onSelect: () => setOrderingOpen(true),
                        },
                        {
                          label: "Clear scripts",
                          danger: true,
                          disabled: !rows.some((_, i) => hasScript(i)),
                          onSelect: () => setClearScriptsOpen(true),
                        },
                      ]}
                    />
                  </span>
                </div>

                {cases.isLoading && <p className="text-sm text-muted">Loading test cases…</p>}
                {cases.isError && <p className="text-sm text-danger">{cases.error.message}</p>}

                {rows.length > 0 && shownIdx.length === 0 && (
                  <p className="text-xs text-muted">
                    {searched ? `No test cases match "${searched}".` : "No cases match this filter."}
                  </p>
                )}
                {grouped ? (
                  groups.map(({ name, indices }) => {
                    const label = name || "Ungrouped";
                    const shut = collapsed.has(label);
                    return (
                      <div key={label} className="space-y-1">
                        <div className="flex w-full items-center gap-3 pb-1 pt-2">
                          {/* Left-anchored with a trailing rule - see ViewCases for why. */}
                          <button
                            aria-label={`${shut ? "Expand" : "Collapse"} group ${label}`}
                            title={shut ? "Expand group" : "Collapse group"}
                            className="text-muted transition-colors hover:text-accent"
                            onClick={() => toggleCollapsed(label)}
                          >
                            {shut ? <ChevronRight size={15} /> : <ChevronDown size={15} />}
                          </button>
                          {/* Same contract as the other grouped screens: selection
                              is the checkbox's job (scripted cases only), the TITLE
                              toggles the fold like the chevron. */}
                          {(() => {
                            const runnable = runnableIn(indices);
                            const on = runnable.filter((id) => selected.has(id)).length;
                            return (
                              <Checkbox
                                ariaLabel={`Select all in ${label}`}
                                checked={runnable.length > 0 && on === runnable.length}
                                indeterminate={on > 0 && on < runnable.length}
                                onCheckedChange={() => toggleGroup(indices)}
                              />
                            );
                          })()}
                          <button
                            className="group flex items-center gap-2"
                            title={shut ? "Expand group" : "Collapse group"}
                            onClick={() => toggleCollapsed(label)}
                          >
                            <span className="text-sm font-semibold tracking-wide text-muted transition-colors group-hover:text-accent">
                              {label} ({indices.length})
                            </span>
                          </button>
                          <span aria-hidden className="h-px flex-1 bg-linear-to-r from-border to-transparent" />
                        </div>
                        <Collapse open={!shut} animateIn={settled}>
                          <ul className="space-y-1">{indices.map(card)}</ul>
                        </Collapse>
                      </div>
                    );
                  })
                ) : (
                  <ul className="space-y-1">{shownIdx.map(card)}</ul>
                )}

                {/* Actions on the selection live bottom-right, in the one shared
                    dock (see ActionDock): in place under the list, and floating
                    bottom-right once that row has scrolled away. It only exists
                    while something is ticked, so the screen never carries a
                    permanently disabled button nobody can use. */}
                {selectedInOrder.length > 0 && (
                  <ActionDock label="Run selection" surface className="pt-1">
                    {(floating) => (
                      <>
                        {/* No "N cases selected" text - the count is already in the
                            button's own label, same as Run Tests' floating pill. */}
                        {/* One run at a time, said in words beside the
                            buttons it holds back, not only in a title. */}
                        {runGoing && <span className="text-xs text-muted">A run is already going</span>}
                        <Button
                          size="sm"
                          tabIndex={floating ? -1 : undefined}
                          className="disabled:pointer-events-auto"
                          disabled={planning || Boolean(runBlocked)}
                          title={runBlocked}
                          onClick={() => void startPlanned("supervised")}
                        >
                          <IconRun aria-hidden />
                          Run {selectedInOrder.length} selected
                        </Button>
                        <Button
                          size="sm"
                          variant="outline"
                          tabIndex={floating ? -1 : undefined}
                          className="disabled:pointer-events-auto"
                          disabled={planning || Boolean(runBlocked)}
                          title={runBlocked}
                          onClick={() => void startPlanned("unattended")}
                        >
                          <IconUnattended aria-hidden />
                          Run {selectedInOrder.length} unattended
                        </Button>
                        <Button
                          size="sm"
                          variant="ghost"
                          aria-label="Clear selection"
                          title="Clear selection"
                          tabIndex={floating ? -1 : undefined}
                          className="rounded-full px-2 hover:text-danger"
                          onClick={() => setSelected(new Set())}
                        >
                          <IconCancel aria-hidden />
                        </Button>
                      </>
                    )}
                  </ActionDock>
                )}
              </section>
            </div>
          )}

          {shown === "runs" && (
            // The same two columns as Test cases, around the same panel.
            <div className={TWO_COLUMNS}>
              <div className="min-w-0 @min-[69rem]:order-last">{setupPanel}</div>
              <div className="min-w-0">
                <PastRuns
                  pbiId={pbi.id}
                  onReview={setReviewing}
                  onReplay={replayCase}
                  replayBlocked={runBlocked}
                  filter={runsFilter}
                  onFilterChange={setRunsFilter}
                />
              </div>
            </div>
          )}
        </div>
      </div>

      {/* Expand all / Collapse all, stuck bottom left like the other screens'
          own (view controls live bottom-left, actions bottom-right). One
          button: Collapse all while any card is open, Expand all when none
          is. Portalled so it pins to the window, not the screen fade. */}
      {shown === "cases" &&
        rows.length > 0 &&
        createPortal(
          <div
            className="fixed bottom-6 z-40 rounded-full border border-accent bg-bg shadow-2xl transition-[left] duration-200"
            style={{ left: stickyLeftPx(sidebarCollapsed) }}
          >
            <Button
              size="sm"
              variant="ghost"
              className="rounded-full text-text hover:bg-surface-2 hover:text-text"
              onClick={anyOpen ? collapseAll : () => expandAll(shownIdx.map((i) => rows[i].id))}
            >
              {anyOpen ? <IconCollapseAll aria-hidden /> : <IconExpandAll aria-hidden />}
              {anyOpen ? "Collapse all" : "Expand all"}
            </Button>
          </div>,
          document.body,
        )}

      {orderingOpen && (
        <ExecutionOrderDialog
          org={org}
          project={project}
          pbiId={pbi.id}
          cases={(selectedInOrder.length > 0
            ? rows.filter((c) => selected.has(c.id))
            : rows.filter((_, i) => hasScript(i))
          ).map((c) => ({ id: c.id, title: c.title }))}
          onClose={() => setOrderingOpen(false)}
        />
      )}

      {clearScriptsOpen && (
        <Modal
          onClose={() => setClearScriptsOpen(false)}
          className="flex w-full max-w-md flex-col gap-4 p-5"
        >
          <h2 className="text-sm font-semibold text-text">Clear scripts?</h2>
          <p className="text-xs text-muted">
            This removes the scripts of the {rows.length} case{rows.length === 1 ? "" : "s"} listed
            for this PBI from this machine. Nothing in Azure DevOps changes.
          </p>
          <div className="flex justify-end gap-2">
            <Button
              variant="ghost"
              size="sm"
              disabled={clearScripts.isPending}
              onClick={() => setClearScriptsOpen(false)}
            >
              <IconCancel aria-hidden />
              Cancel
            </Button>
            <Button
              size="sm"
              variant="danger"
              disabled={clearScripts.isPending}
              onClick={() => clearScripts.mutate()}
            >
              <IconClearScripts aria-hidden />
              {clearScripts.isPending
                ? "Clearing"
                : `Clear ${rows.length} script${rows.length === 1 ? "" : "s"}`}
            </Button>
          </div>
        </Modal>
      )}

      {editing != null &&
        (() => {
          const c = (cases.data ?? []).find((x) => x.id === editing);
          if (!c) return null;
          return (
            <ScriptEditor
              caseId={c.id}
              title={c.title}
              steps={c.steps}
              org={org}
              project={project}
              onClose={() => setEditing(null)}
            />
          );
        })()}

      {running != null &&
        (() => {
          const picked = running.flatMap((id) => {
            const c = rows.find((x) => x.id === id);
            if (c) return [{ id: c.id, title: c.title }];
            return replayTo?.caseId === id ? [{ id, title: replayTo.title }] : [];
          });
          if (picked.length === 0) return null;
          return (
            <RunPane
              org={org}
              project={project}
              pbiId={pbi.id}
              cases={picked}
              replayTo={replayTo?.step}
              // A replay to a step runs one case and never pauses.
              plan={replayTo ? null : runPlan}
              browserBlocked={runBlocked}
              onClose={() => {
                setRunning(null);
                setRunPlan(null);
                setReplayTo(null);
                // The selection has been run - leaving it ticked invites a
                // second run of cases that were just decided. A replay ran
                // no selection.
                if (!replayTo) setSelected(new Set());
              }}
            />
          );
        })()}

      {waitingReset && !runGoing && (
        // Only Continue or Stop ends the pause: the run waits for one.
        <Modal onClose={() => {}} label="Reset needed" className="w-full max-w-2xl p-4">
          <ResetNeededPanel
            reset={waitingReset}
            remaining={waitingReset.remaining}
            titleOf={(id) => rows.find((c) => c.id === id)?.title}
            busy={answeringReset}
            onContinue={() => void answerWaitingReset(true)}
            onStop={() => void answerWaitingReset(false)}
          />
        </Modal>
      )}

      {reviewing != null && (
        <RunReview
          // A different run id must mount a fresh instance - without this,
          // switching from reviewing one run straight to another (Past
          // Runs lets you) would carry the previous run's local edit state
          // (and its `baseline`) into a screen that has not loaded the
          // new run yet.
          key={reviewing}
          org={org}
          project={project}
          pbiTitle={pbi.title}
          pbiId={pbi.id}
          runId={reviewing}
          stepIds={Object.fromEntries(rows.map((c) => [c.id, c.step_ids]))}
          sharedSteps={Object.fromEntries(
            rows.map((c) => [c.id, c.steps.flatMap((s, i) => (s.shared != null ? [i + 1] : []))]),
          )}
          onReplay={replayCase}
          replayBlocked={runBlocked}
          onClose={() => {
            setReviewing(null);
            // Opened from Past runs, the card's Review button is still there
            // and the dialog hands focus back to it. Only a review opened
            // from elsewhere (a finished unattended run over Test cases) has
            // lost its opener.
            if (shown !== "runs") setFocusRunsTab(true);
            // Wherever the review opened from - Past runs, or a finished
            // unattended run over Test cases - the run now lives in Past runs.
            setTab("runs");
          }}
        />
      )}
    </>
  );
}
