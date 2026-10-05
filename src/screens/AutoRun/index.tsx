// Supervised and unattended auto-run: the app drives a real Edge window
// through a case's steps, either with a person watching and deciding the
// verdict, or unattended with the machine proposing one.
//
// Nothing a script or a run does reaches Azure DevOps by itself. A
// person reviewing a finished run and pressing Send (`RunReview`) is the
// one door out - see `autorun::publish` on the Rust side.

import { ChevronDown, ChevronRight, TriangleAlert } from "lucide-react";
import { useMutation, useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { commands, type PbiHit } from "../../bindings";
import { Checkbox } from "../../components/ui/checkbox";
import MoreActionsMenu from "../../components/MoreActionsMenu";
import { Collapse, useSettled } from "../../components/ui/collapse";
import { groupIndices } from "../../lib/grouping";
import { usePersistedStringSet } from "../../lib/collapsedGroups";
import { Button } from "../../components/ui/button";
import ActionDock from "../../components/ActionDock";
import { useFieldRefs } from "../../hooks/useFieldRefs";
import { cn } from "../../lib/cn";
import { activeEnvironment, effectiveSite, useEnvironments } from "../../lib/environments";
import { unwrap, unwrapStr } from "../../lib/ipc";
import {
  IconAccounts,
  IconAdd,
  IconCancel,
  IconClearScripts,
  IconEdit,
  IconModulePaths,
  IconRecipe,
  IconRecord,
  IconRun,
  IconSiteAddress,
  IconTestFiles,
  IconSaveWords,
  IconUnattended,
} from "../../lib/actionIcons";
import { open } from "@tauri-apps/plugin-dialog";
import { toast } from "../../lib/toast";
import { Modal } from "../../components/ui/modal";
import AccountsDialog from "./AccountsDialog";
import AreasDialog from "./AreasDialog";
import PastRuns from "./PastRuns";
import RecipeEditor from "./RecipeEditor";
import ReadinessStrip from "./ReadinessStrip";
import RecordSignInDialog from "./RecordSignInDialog";
import ReplayPane from "./ReplayPane";
import RunPane from "./RunPane";
import RunReview from "./RunReview";
import ScriptEditor from "./ScriptEditor";
import { ClearConfirm, SuspectedDefectBadge } from "./SuspectedDefectMark";
import { ResultToggleRow } from "./ResultFilterRow";
import {
  RESULT_BUCKETS,
  bucketTone,
  lastResultFor,
  lastResults,
  type ResultBucket,
  type ResultFilter,
} from "./verdicts";
import SiteAddressDialog, { siteHost } from "./SiteAddressDialog";
import SaveWordsDialog, { BUILT_IN_SAVE_WORDS } from "./SaveWordsDialog";
import TestFilesDialog, { useTestFiles } from "./TestFilesDialog";
import { useAutoRunReadiness, type AutoRunTab } from "./useAutoRunReadiness";

/** The screen's tabs, in order. The arrow keys walk this list. */
const TABS: { id: AutoRunTab; label: string }[] = [
  { id: "cases", label: "Test cases" },
  { id: "runs", label: "Past runs" },
  { id: "setup", label: "Setup" },
];

// How many imported case ids the success toast spells out before it falls
// back to a count - the same shape as the assigned-work notification
// summary. A 60-case import naming every one of them is unreadable.
const MAX_IDS_IN_TOAST = 10;

/** The filter that lets every case through. */
const NO_FILTER: ReadonlySet<ResultBucket> = new Set();

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** One line of the Setup card: what it is, where it stands, one button.
 * A labelled group, so a screen reader hears the row's name with its state
 * and its button. */
function SetupRow({
  label,
  state,
  children,
}: {
  label: string;
  state: ReactNode;
  children: ReactNode;
}) {
  return (
    <div
      role="group"
      aria-label={label}
      className="flex flex-wrap items-center gap-x-3 gap-y-1 border-t border-border/60 pt-3 first-of-type:border-t-0 first-of-type:pt-0"
    >
      <span className="w-28 shrink-0 text-xs font-medium text-muted">{label}</span>
      <span className="min-w-0 flex-1 text-sm text-text">{state}</span>
      {children}
    </div>
  );
}

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
  // same object while no script has changed - the readiness check below
  // reads it, and would otherwise redo its work on every render.
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
  const [accountsOpen, setAccountsOpen] = useState(false);
  const [recipeOpen, setRecipeOpen] = useState(false);
  const [recordOpen, setRecordOpen] = useState(false);
  const [navOpen, setNavOpen] = useState(false);
  const [siteOpen, setSiteOpen] = useState(false);
  const [testFilesOpen, setTestFilesOpen] = useState(false);
  const [saveWordsOpen, setSaveWordsOpen] = useState(false);
  const [clearScriptsOpen, setClearScriptsOpen] = useState(false);
  const queryClient = useQueryClient();

  /** What the Setup card and the header line report. All three read small
   * local files, and each shares its query key with the dialog that edits
   * it, so a save there updates the card here. `?? null` because "nothing
   * saved yet" is an answer, not a missing one. */
  const setupReady = Boolean(org && project);
  const recipe = useQuery({
    queryKey: ["autorun-recipe", org, project],
    queryFn: async () => (await unwrapStr(commands.autoRunLoadRecipe(org, project))) ?? null,
    enabled: setupReady,
    retry: false,
  });
  const envs = useEnvironments();
  const accounts = useQuery({
    queryKey: ["autorun-accounts"],
    queryFn: async () => (await unwrapStr(commands.autoRunListAccounts())) ?? null,
    retry: false,
  });
  const nav = useQuery({
    queryKey: ["autorun-nav", org, project],
    queryFn: async () => (await unwrapStr(commands.autoRunLoadNav(org, project))) ?? null,
    enabled: setupReady,
    retry: false,
  });
  /** A count once its query has answered - `null` only while it has not
   * (pending) or could not (error). An answer of "no data" counts as none:
   * keyed on the answer rather than on the data, so the row can never sit
   * on "Loading…" after the query has already settled. */
  const accountCount = accounts.isSuccess ? (accounts.data?.length ?? 0) : null;
  // The documents this project's scripts upload. Shares its key with the
  // dialog, so adding or removing one there updates the row.
  const testFiles = useTestFiles(org, project);
  const testFileCount = testFiles.isSuccess ? (testFiles.data?.length ?? 0) : null;
  const areaCount = nav.isSuccess ? (nav.data?.modules.length ?? 0) : null;

  const saved = recipe.data;
  // Where a run goes now: the active environment's address when it has one,
  // else the saved recipe's - the header and the Setup row both say this.
  // With no saved recipe the built-in one signs in at the environment's.
  const site = effectiveSite(envs.data, saved);
  const activeEnv = activeEnvironment(envs.data);
  /** The same address, as far as it is KNOWN: `undefined` while the
   * environments or the recipe it may fall back to have not answered, or
   * could not be read - an unreadable recipe may well hold an address, so
   * it never reads as "none". */
  const knownSiteUrl = ((): string | undefined => {
    if (envs.isPending) return undefined;
    const own = activeEnv?.start_url.trim();
    if (own) return own;
    if (setupReady && (recipe.isPending || recipe.isError)) return undefined;
    if (site.start_url) return site.start_url;
    return envs.isError ? undefined : "";
  })();
  const testFileNames = useMemo(
    () => (testFiles.isSuccess ? (testFiles.data ?? []).map((f) => f.name) : null),
    [testFiles.isSuccess, testFiles.data],
  );
  // No project, no sign-in: the recipe is a project's. A recipe that could
  // not be read is unknown, and its row says why.
  const signIn: "saved" | "builtin" | "none" | null = !setupReady
    ? "none"
    : recipe.isSuccess
      ? saved
        ? "saved"
        : "builtin"
      : null;
  const readiness = useAutoRunReadiness({
    siteUrl: knownSiteUrl,
    signIn,
    accountCount,
    areaCount,
    scripts,
    testFileNames,
  });

  /** Which tab shows. `null` until the screen has decided, once: Setup when
   * something a run cannot go without is missing, Test cases otherwise.
   * After that only the person's clicks - and a review closing - move it,
   * so setup that changes later (the last account removed, say) never
   * pulls anyone off the tab they are on. */
  const [tab, setTab] = useState<AutoRunTab | null>(null);
  /** Past runs' result filter. Here rather than in the panel, which is not
   * mounted while another tab shows - the choice outlives it. */
  const [runsFilter, setRunsFilter] = useState<ResultFilter>("All");
  /** Every essential read has answered, or failed. A failed read is shown
   * on its Setup row and is never "missing" (see `useAutoRunReadiness`), so
   * the screen still opens - on what it does know. */
  const essentialsSettled =
    !envs.isPending && !(setupReady && recipe.isPending) && !accounts.isPending;
  /** Reads that failed, each as the sentence its Setup row already says.
   * A failed read is never "missing" (it does not route the screen), but
   * it must be visible on the tab the screen opens on, not only on Setup.
   * The first three are what a run cannot go without; they also flag the
   * Setup tab. */
  const essentialUnreadable = [
    ...(envs.isError ? ["The environments could not be read"] : []),
    ...(setupReady && recipe.isError ? ["The saved recipe could not be read"] : []),
    ...(accounts.isError ? ["The accounts could not be read"] : []),
  ];
  const unreadable = [
    ...essentialUnreadable,
    ...(setupReady && testFiles.isError ? ["The test files could not be read"] : []),
  ];
  const opening: AutoRunTab | null = essentialsSettled
    ? readiness.essentialMissing
      ? "setup"
      : "cases"
    : null;
  useEffect(() => {
    if (tab === null && opening !== null) setTab(opening);
  }, [tab, opening]);
  // The render that first knows the answer shows it, rather than one frame
  // of nothing before the effect stores it.
  const shown = tab ?? opening;

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

  /** The active environment's database, named on the Setup tab. The same
   * list, under the same key, AI Bridge reads and edits. */
  const databases = useQuery({
    queryKey: ["db-databases"],
    queryFn: async () => (await commands.dbDatabases()) ?? [],
    retry: false,
  });
  const activeDb = databases.data?.find((d) => d.id === activeEnv?.db_id);
  const dbLine = databases.isPending
    ? "loading…"
    : databases.isError
      ? "could not be read"
      : !activeDb
        ? "not set up any more"
        : activeDb.server
          ? `${activeDb.label}: ${activeDb.database} on ${activeDb.server}`
          : `${activeDb.label}: not set up yet`;

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
  /** The case ids queued for an UNATTENDED run. Its own state, separate
   * from `running` - the two panes are never open at once (both come from
   * the same selection dock), but they are different flows with different
   * dialogs. */
  const [replaying, setReplaying] = useState<number[] | null>(null);
  /** The run id under review, or null while no review dialog is open. An
   * unattended run opens straight into this once it finishes - see
   * ReplayPane's `onFinished` below. */
  const [reviewing, setReviewing] = useState<string | null>(null);
  /** Ticked cases, by id. A bulk run is these, in list order. */
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [grouped, setGrouped] = useState(
    () => localStorage.getItem("tcm-v2-autorun-group") === "on",
  );
  const [collapsed, toggleCollapsed] = usePersistedStringSet("tcm-v2-autorun-collapsed");
  /** The last results the Test cases tab shows. Empty is every case. Lives
   * here, not in the tab, so it outlasts a trip to another tab and is gone
   * once the screen is left. */
  const [lastFilter, setLastFilter] = useState<ReadonlySet<ResultBucket>>(new Set());

  const rows = cases.data ?? [];
  const settled = useSettled(rows.length > 0);
  /** Each case's result in the newest run that holds it - the runs Past
   * runs lists, so the two tabs never disagree. */
  const last = useMemo(() => lastResults(runs.data ?? []), [runs.data]);
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
  /** The rows the filter lets through, by index in `rows`. */
  const shownIdx = rows.flatMap((_, i) => (activeFilter.size === 0 || activeFilter.has(lastOf(i)) ? [i] : []));
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

  /** A case the filter hides leaves the selection, so a run only ever holds
   * what the person can see. Checked after every render rather than in the
   * filter's click, because the runs refreshing (one just finished) can hide
   * a ticked case without anyone pressing anything. Not while the case list
   * itself is empty (loading): that is not the person's filter hiding them. */
  useEffect(() => {
    if (rows.length === 0) return;
    setSelected((prev) => {
      const kept = [...prev].filter((id) => shownIds.has(id));
      return kept.length === prev.size ? prev : new Set(kept);
    });
  });

  /** The scripted cases the filter shows - what Select all shown covers. */
  const shownRunnable = runnableIn(shownIdx);
  const shownTicked = shownRunnable.filter((id) => selected.has(id)).length;

  /** List order, not click order - the run reads top to bottom the way
   * the screen does. */
  const selectedInOrder = rows.filter((c) => selected.has(c.id)).map((c) => c.id);

  /** One case row, by its index in `rows` - grouped and flat both render
   * the same thing, and `scripts[i]` is indexed the same way. */
  const row = (i: number) => {
    const c = rows[i];
    const ready = hasScript(i);
    const defect = scripts[i]?.suspected_defect;
    const result = lastOf(i);
    return (
      <li
        key={c.id}
        className="flex flex-wrap items-center gap-2 rounded-md border border-border bg-surface px-3 py-2 text-sm"
      >
        {/* Only a scripted case can be run, so only a scripted case can be
            ticked - a checkbox that selects something unrunnable would
            just make the count lie. */}
        <Checkbox
          checked={selected.has(c.id)}
          ariaLabel={`Select #${c.id}`}
          className={ready ? undefined : "invisible"}
          onCheckedChange={() => ready && toggleOne(c.id)}
        />
        <span className="id-mono text-faint">#{c.id}</span>
        {/* The title gets the room and wraps, the way Update Test Cases'
            rows do - a truncated title is exactly the part that tells two
            similar cases apart. */}
        <span className="min-w-0 flex-1 break-words text-text">{c.title}</span>
        {/* The result of the case's last run, in Past runs' words and colours.
            A case never run carries nothing: a mark that says "none" on
            every fresh row is noise. */}
        {result !== "Not run" && (
          <span className={cn("shrink-0 text-xs font-medium", bucketTone[result])}>
            <span className="sr-only">Last result: </span>
            {result}
          </span>
        )}
        {defect && (
          <SuspectedDefectBadge
            caseId={c.id}
            defect={defect}
            onClear={() => setConfirmingClear(c.id)}
          />
        )}
        {/* No "Script ready" badge: the row says it with its buttons. A
            scripted case has a Run button, outlined in the success colour
            so a list reads at a glance as "these can run"; a case with no
            script has no Run button, and its script button says Add.
            Both stay outline buttons of one size, so a list of twenty is
            not twenty bright buttons - the one primary action on the
            screen is running the selection, in the dock. */}
        <Button
          size="sm"
          variant="outline"
          className="shrink-0"
          aria-label={`${ready ? "Edit" : "Add"} script for #${c.id}`}
          onClick={() => setEditing(c.id)}
        >
          {ready ? <IconEdit aria-hidden /> : <IconAdd aria-hidden />}
          {ready ? "Script" : "Add script"}
        </Button>
        {ready && (
          <Button
            size="sm"
            variant="outline"
            className="shrink-0 border-success text-success hover:border-success hover:bg-success/10 hover:text-success"
            aria-label={`Run #${c.id}`}
            onClick={() => setRunning([c.id])}
          >
            <IconRun aria-hidden />
            Run
          </Button>
        )}
        {defect && confirmingClear === c.id && (
          <ClearConfirm caseId={c.id} onDone={() => setConfirmingClear(null)} />
        )}
      </li>
    );
  };

  if (!org || !pbi) {
    return <p className="text-sm text-muted">Pick a PBI in the bar above to auto-run its cases.</p>;
  }

  const needsProject = setupReady ? undefined : "Pick an organization and project first";
  const extraSites = site.allowed_origins.length;
  const caseCount = cases.data ? rows.length : null;

  return (
    <>
      {/* Three tabs, one panel at a time: the cases to run, what came of
          past runs, and the setup a run needs. One reading width for all
          three - the case rows read no better wider, and nothing ever sits
          beside anything else. Normal page flow, so the page's own bottom
          padding keeps the floating dock clear of the last row. */}
      <div className="max-w-3xl space-y-4">
        <div className="min-w-0 space-y-4">
          {/* The Templates/Flows tab pattern from API Templates, with the
              keyboard a tab list owes: only the chosen tab is in the Tab
              order (the first, before the screen has chosen), and the arrow
              keys move along. */}
          <div role="tablist" aria-label="Auto Run sections" className="flex gap-1 border-b border-border">
            {TABS.map(({ id, label }, i) => {
              const selected = shown === id;
              const count = id === "cases" ? caseCount : id === "runs" ? runCount : null;
              const attention =
                id === "setup" && (readiness.essentialMissing || essentialUnreadable.length > 0);
              return (
                <button
                  key={id}
                  ref={(el) => {
                    tabRefs.current[id] = el;
                  }}
                  id={`${tabIds}-${id}-tab`}
                  role="tab"
                  aria-selected={selected}
                  aria-controls={selected ? `${tabIds}-${id}-panel` : undefined}
                  tabIndex={selected || (shown === null && i === 0) ? 0 : -1}
                  className={cn(
                    "-mb-px border-b-2 px-3 py-1.5 text-sm font-medium transition-colors",
                    selected ? "border-accent text-text" : "border-transparent text-muted hover:text-accent",
                  )}
                  onClick={() => setTab(id)}
                  onKeyDown={(e) => onTabKey(e, i)}
                >
                  {label}
                  {count != null && (
                    <>
                      {" "}
                      <span className="text-xs text-faint">{count}</span>
                    </>
                  )}
                  {/* There is no site address, no way to sign in, or no
                      account - said in words too, for a screen reader. */}
                  {attention && (
                    <>
                      <TriangleAlert aria-hidden className="ml-1.5 inline size-3.5 align-[-2px] text-warning" />
                      <span className="sr-only">, needs attention</span>
                    </>
                  )}
                </button>
              );
            })}
          </div>
        </div>

        {shown && (
          <div
            role="tabpanel"
            id={`${tabIds}-${shown}-panel`}
            aria-labelledby={`${tabIds}-${shown}-tab`}
            className="min-w-0"
          >
            {shown === "setup" && (
              <div className="space-y-3">
                {/* Read-only here: the environment and its database are chosen on
                    AI Bridge, which owns them. */}
                {activeEnv && (
                  <p className="text-xs text-muted">
                    Environment <span className="font-medium text-text">{activeEnv.name}</span>, database{" "}
                    <span className="font-medium text-text">{dbLine}</span>. Both change on the AI Bridge tab.
                  </p>
                )}
                <section className="space-y-3 rounded-md border border-border bg-surface p-4">
                  <h2 className="text-sm font-semibold text-text">Setup</h2>
                  <div className="space-y-3">
                    <SetupRow
                      label="Site address"
                      state={
                        !setupReady ? (
                          <span className="text-muted">{needsProject}</span>
                        ) : recipe.isLoading ? (
                          <span className="text-muted">Loading…</span>
                        ) : recipe.isError ? (
                          <span className="text-danger">The saved recipe could not be read</span>
                        ) : envs.isError && !site.start_url ? (
                          <span className="text-danger">The environments could not be read</span>
                        ) : site.start_url ? (
                          <>
                            <span className="id-mono break-all">{site.start_url}</span>
                            {extraSites > 0 && (
                              <span className="ml-2 text-xs text-faint">
                                +{plural(extraSites, "allowed site")}
                              </span>
                            )}
                          </>
                        ) : (
                          <span className="text-muted">Not set up yet</span>
                        )
                      }
                    >
                      {/* The address is the active environment's, not the recipe's,
                          so it is set here with or without a saved recipe: the
                          built-in sign-in needs nothing more than this. */}
                      <Button
                        size="sm"
                        variant="outline"
                        aria-label="Edit site address"
                        disabled={!setupReady}
                        title={needsProject}
                        onClick={() => setSiteOpen(true)}
                      >
                        <IconSiteAddress aria-hidden />
                        Edit
                      </Button>
                    </SetupRow>

                    <SetupRow
                      label="Sign-in"
                      state={
                        !setupReady ? (
                          <span className="text-muted">{needsProject}</span>
                        ) : recipe.isLoading ? (
                          <span className="text-muted">Loading…</span>
                        ) : recipe.isError ? (
                          <span className="text-danger">Could not be read - open it to see why</span>
                        ) : saved ? (
                          "Recipe saved"
                        ) : (
                          // No saved recipe: the app's own runs. Recording or
                          // editing one saves this project's, which replaces it.
                          "Built-in"
                        )
                      }
                    >
                      {/* Record: sign in by hand once and the recipe is written.
                          Edit: the recipe as JSON, for what a recording cannot say. */}
                      <Button
                        size="sm"
                        variant="outline"
                        aria-label="Record sign-in"
                        disabled={!setupReady}
                        title={needsProject}
                        onClick={() => setRecordOpen(true)}
                      >
                        <IconRecord aria-hidden />
                        Record
                      </Button>
                      <Button
                        size="sm"
                        variant="outline"
                        aria-label="Edit sign-in recipe"
                        disabled={!setupReady}
                        title={needsProject}
                        onClick={() => setRecipeOpen(true)}
                      >
                        <IconRecipe aria-hidden />
                        Edit
                      </Button>
                    </SetupRow>

                    <SetupRow
                      label="Accounts"
                      state={
                        accounts.isError ? (
                          <span className="text-danger">The accounts could not be read</span>
                        ) : accounts.isPending || accountCount == null ? (
                          <span className="text-muted">Loading…</span>
                        ) : accountCount === 0 ? (
                          <span className="text-muted">None yet</span>
                        ) : (
                          `${plural(accountCount, "account")} on this machine`
                        )
                      }
                    >
                      <Button
                        size="sm"
                        variant="outline"
                        aria-label="Edit accounts"
                        onClick={() => setAccountsOpen(true)}
                      >
                        <IconAccounts aria-hidden />
                        Edit
                      </Button>
                    </SetupRow>

                    <SetupRow
                      label="Areas"
                      state={
                        !setupReady ? (
                          <span className="text-muted">{needsProject}</span>
                        ) : nav.isError ? (
                          <span className="text-danger">The areas could not be read</span>
                        ) : nav.isPending || areaCount == null ? (
                          <span className="text-muted">Loading…</span>
                        ) : areaCount === 0 ? (
                          <span className="text-muted">None recorded yet</span>
                        ) : (
                          `${plural(areaCount, "area")} recorded`
                        )
                      }
                    >
                      <Button
                        size="sm"
                        variant="outline"
                        aria-label="Edit areas"
                        disabled={!setupReady}
                        title={needsProject}
                        onClick={() => setNavOpen(true)}
                      >
                        <IconModulePaths aria-hidden />
                        Edit
                      </Button>
                    </SetupRow>

                    <SetupRow
                      label="Test files"
                      state={
                        !setupReady ? (
                          <span className="text-muted">{needsProject}</span>
                        ) : testFiles.isError ? (
                          <span className="text-danger">The test files could not be read</span>
                        ) : testFiles.isPending || testFileCount == null ? (
                          <span className="text-muted">Loading…</span>
                        ) : testFileCount === 0 ? (
                          <span className="text-muted">None yet</span>
                        ) : (
                          plural(testFileCount, "file")
                        )
                      }
                    >
                      <Button
                        size="sm"
                        variant="outline"
                        aria-label="Manage test files"
                        disabled={!setupReady}
                        title={needsProject}
                        onClick={() => setTestFilesOpen(true)}
                      >
                        <IconTestFiles aria-hidden />
                        Manage
                      </Button>
                    </SetupRow>

                    {/* What a script marked Must not save has stopped: the
                        built-in words, fixed, then the project's own. */}
                    <SetupRow
                      label="Save words"
                      state={
                        !setupReady ? (
                          <span className="text-muted">{needsProject}</span>
                        ) : nav.isError ? (
                          <span className="text-danger">The save words could not be read</span>
                        ) : nav.isPending ? (
                          <span className="text-muted">Loading…</span>
                        ) : (
                          <>
                            <span className="text-muted">
                              {(nav.data?.built_in_save_words?.length
                                ? nav.data.built_in_save_words
                                : BUILT_IN_SAVE_WORDS
                              ).join(", ")}
                            </span>
                            {(nav.data?.save_words ?? []).length > 0 && (
                              <span>, {(nav.data?.save_words ?? []).join(", ")}</span>
                            )}
                          </>
                        )
                      }
                    >
                      <Button
                        size="sm"
                        variant="outline"
                        aria-label="Edit save words"
                        disabled={!setupReady || nav.isPending || nav.isError}
                        title={needsProject}
                        onClick={() => setSaveWordsOpen(true)}
                      >
                        <IconSaveWords aria-hidden />
                        Edit
                      </Button>
                    </SetupRow>
                  </div>
                </section>
                <p className="text-xs text-muted">
                  An assistant&apos;s <span className="id-mono">/tcm:setup</span> command can walk you through
                  this.
                </p>
              </div>
            )}

            {shown === "cases" && (
              <section className="space-y-2">
                <h2 className="text-sm font-semibold text-text">
                  Test cases
                  {cases.data && <span className="ml-1.5 font-normal text-faint">({rows.length})</span>}
                </h2>
                {/* Where runs go and whether the setup is in place, in one line.
                    Only once the three things a run cannot go without are
                    known, so a slow read never shows as a warning. */}
                {essentialsSettled && (
                  <ReadinessStrip
                    envName={activeEnv?.name ?? null}
                    // undefined: the address could not be told (a read failed).
                    siteHost={
                      knownSiteUrl === undefined ? undefined : site.start_url ? siteHost(site.start_url) : null
                    }
                    signIn={signIn}
                    accountCount={accountCount}
                    areaCount={areaCount}
                    testFileCount={testFileCount}
                    missingTestFiles={readiness.missingTestFiles}
                    unreadable={unreadable}
                    onOpenSetup={() => {
                      setTab("setup");
                      // The button is in the panel that is about to unmount.
                      tabRefs.current.setup?.focus();
                    }}
                  />
                )}
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
                  {/* Ticks what the filter below shows (scripted cases only), so
                      "run everything that failed" is two clicks. Off while
                      nothing shown could be run. */}
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
                  {rows.length > 0 &&
                    (runsFailed ? (
                      <span className="text-xs text-muted">Past results could not be read</span>
                    ) : (
                      <span className="flex flex-wrap items-center gap-2">
                        <span className="text-xs text-muted">Last result</span>
                        {/* Without numbers, and out of reach, until the runs are read. */}
                        <ResultToggleRow
                          label="Filter by last result"
                          pressed={activeFilter}
                          onToggle={toggleLast}
                          counts={runsReady ? lastCounts : undefined}
                          disabled={!runsReady}
                        />
                      </span>
                    ))}
                  {/* The rare actions. Clear scripts is housekeeping shown wherever
                      Auto Run is (dev, or unlocked) - the whole tab is gated in one
                      place (`autoRunVisible` in lib/extras.ts), so no further gating
                      belongs here. Disabled rather than hidden: an item that vanishes
                      the moment it would do nothing invites "where did it go". Danger
                      only on hover, and it still asks first. */}
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
                  <p className="text-xs text-muted">No cases match this filter.</p>
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
                          <ul className="space-y-1">{indices.map(row)}</ul>
                        </Collapse>
                      </div>
                    );
                  })
                ) : (
                  <ul className="space-y-1">{shownIdx.map(row)}</ul>
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
                        <Button
                          size="sm"
                          tabIndex={floating ? -1 : undefined}
                          onClick={() => setRunning(selectedInOrder)}
                        >
                          <IconRun aria-hidden />
                          Run {selectedInOrder.length} selected
                        </Button>
                        <Button
                          size="sm"
                          variant="outline"
                          tabIndex={floating ? -1 : undefined}
                          onClick={() => setReplaying(selectedInOrder)}
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
            )}

            {shown === "runs" && (
              <PastRuns
                pbiId={pbi.id}
                onReview={setReviewing}
                filter={runsFilter}
                onFilterChange={setRunsFilter}
              />
            )}
          </div>
        )}
      </div>

      {accountsOpen && <AccountsDialog onClose={() => setAccountsOpen(false)} />}
      {recipeOpen && (
        <RecipeEditor org={org} project={project} onClose={() => setRecipeOpen(false)} />
      )}
      {recordOpen && (
        <RecordSignInDialog org={org} project={project} onClose={() => setRecordOpen(false)} />
      )}
      {testFilesOpen && (
        <TestFilesDialog org={org} project={project} onClose={() => setTestFilesOpen(false)} />
      )}
      {siteOpen && (
        <SiteAddressDialog org={org} project={project} onClose={() => setSiteOpen(false)} />
      )}
      {saveWordsOpen && (
        <SaveWordsDialog
          org={org}
          project={project}
          view={nav.data ?? null}
          onClose={() => setSaveWordsOpen(false)}
        />
      )}
      {navOpen && (
        <AreasDialog
          org={org}
          project={project}
          caseModules={caseModules}
          onClose={() => setNavOpen(false)}
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
          const picked = running
            .map((id) => rows.find((x) => x.id === id))
            .filter((c): c is (typeof rows)[number] => Boolean(c))
            .map((c) => ({ id: c.id, title: c.title }));
          if (picked.length === 0) return null;
          return (
            <RunPane
              org={org}
              project={project}
              pbiId={pbi.id}
              cases={picked}
              onClose={() => {
                setRunning(null);
                // The selection has been run - leaving it ticked invites a
                // second run of cases that were just decided.
                setSelected(new Set());
              }}
            />
          );
        })()}

      {replaying != null &&
        (() => {
          const picked = replaying
            .map((id) => rows.find((x) => x.id === id))
            .filter((c): c is (typeof rows)[number] => Boolean(c))
            .map((c) => ({ id: c.id, title: c.title, module: c.module_value, steps: c.steps }));
          if (picked.length === 0) return null;
          return (
            <ReplayPane
              org={org}
              project={project}
              pbiId={pbi.id}
              cases={picked}
              onClose={() => setReplaying(null)}
              onFinished={(runId) => {
                setReplaying(null);
                // Same reasoning as the supervised pane's onClose - the
                // selection has been run, so leaving it ticked invites a
                // second run of cases that were just decided.
                setSelected(new Set());
                void queryClient.invalidateQueries({ queryKey: ["autorun-runs"] });
                // A run that passes a marked step clears the mark on disk;
                // the rows read each script through their own query.
                void queryClient.invalidateQueries({ queryKey: ["autorun-script"] });
                // Straight into its review rather than a toast pointing at
                // Past runs - every case is still unconfirmed at this point,
                // so there is nothing useful to do with this run BUT review it.
                setReviewing(runId);
              }}
            />
          );
        })()}

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
