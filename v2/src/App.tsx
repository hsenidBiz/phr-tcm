import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { getVersion } from "@tauri-apps/api/app";
import { lazy, Suspense, useEffect, useRef, useState, type ComponentType, useSyncExternalStore } from "react";
import { Toaster, toast } from "sonner";
import { commands, events, type PbiHit, type PlanWithSuites } from "./bindings";
import { loadWatches, saveWatches, upsertWatch } from "./lib/fileSync";
import { applyRateLevel } from "./lib/adoRate";
import { formatByteProgress } from "./lib/bytes";
import { onlineSnapshot, subscribeOnline } from "./lib/network";
import { appIsInView, osNotify, summarize } from "./lib/assignedAlerts";
import { disabledToolsSnapshot, subscribeDisabledTools } from "./lib/mcpTools";
import { cacheEntry, claimCacheFor } from "./lib/localCache";
import { CACHE, persistentQuery } from "./lib/persistentQuery";
import { saveNote } from "./lib/caseNotes";
import { useFieldRefs } from "./hooks/useFieldRefs";
import {
  CHANGELOG,
  markChangelogSeen,
  pendingChangelog,
  SHOW_CHANGELOG_EVENT,
  type ChangelogEntry,
} from "./lib/changelog";
import AnimatedContent from "./components/AnimatedContent";
import ChangelogModal from "./components/ChangelogModal";
import CommandPalette from "./components/CommandPalette";
import ContextBar from "./components/ContextBar";
import Sidebar, { WORK_ITEMS, type Section, type WorkSection } from "./components/Sidebar";
import TitleBar from "./components/TitleBar";
import UiTour, { START_TOUR_EVENT, tourDone } from "./components/UiTour";
import { Button } from "./components/ui/button";
import { unwrap } from "./lib/ipc";
import { logUi } from "./lib/uiLog";
import { loadPrefs, savePrefs } from "./lib/prefs";
import { getTheme, initTheme } from "./lib/theme";
import EditCases from "./screens/EditCases";
import ImportFile from "./screens/ImportFile";
import ManualEntry from "./screens/ManualEntry";
import CreateWorkItem from "./screens/CreateWorkItem";
import PrPanel from "./screens/PrPanel";
import RunTests from "./screens/RunTests";
import SignIn from "./screens/SignIn";
import ViewCases from "./screens/ViewCases";
/** Compile-time dev gate: statically false in `tauri build`, so everything
 * behind it (and the dev/ module itself) is dead-code-eliminated from
 * released builds. Test mode opts out so vitest sees the plain app. */
const DEV_TOOLS = import.meta.env.DEV && import.meta.env.MODE !== "test";

/** DevPanel must be a DYNAMIC import behind the gate: a static
 * `import DevPanel from "./dev/DevPanel"` bundles dev/demo's module-level
 * dataset into the release even though the render below is gated (the JSX
 * gets shaken, module side effects don't). Lazy inside the statically-false
 * branch drops the whole dev/ chunk from `tauri build`. */
const DevPanel: ComponentType<{
  org: string;
  project: string;
  pbi: PbiHit | null;
  section: string;
  workMode: boolean;
  onShowSignIn: () => void;
}> = DEV_TOOLS ? lazy(() => import("./dev/DevPanel")) : () => null;
import AiBridge from "./screens/AiBridge";
import Settings from "./screens/Settings";
import Suites from "./screens/Suites";
import WorkBoard from "./screens/WorkBoard";
import { IconRefresh } from "./lib/actionIcons";

/** How often to look for a new release, in the background. */
const UPDATE_CHECK_MS = 60 * 60 * 1000;

/**
 * Stop listening, without letting the teardown throw.
 *
 * The event plugin's `unlisten` reaches into
 * `window.__TAURI_EVENT_PLUGIN_INTERNALS__`, which only the Tauri runtime
 * fills in. Anywhere else - jsdom, a browser preview - calling it rejects,
 * and an unhandled rejection out of a React cleanup is attributed to
 * whatever test happened to be unmounting, so one listener took several
 * unrelated tests down with it.
 *
 * There is nothing to recover from here either way: the listener is going
 * away with the component.
 */
function detach(unlisten: (() => void) | undefined): void {
  if (!unlisten) return;
  try {
    // May return a promise (it does in @tauri-apps/api) or nothing.
    void Promise.resolve(unlisten() as unknown).catch(() => {});
  } catch {
    // Threw synchronously instead - same conclusion.
  }
}

const TITLES: Record<Section, string> = {
  manual: "Manual Entry",
  import: "Import File",
  edit: "Update Test Cases",
  view: "View Test Cases",
  run: "Run Tests",
  suites: "Test Suites",
  ai: "AI Bridge",
  settings: "Settings",
};

export default function App() {
  const qc = useQueryClient();
  const initial = loadPrefs();
  const [section, setSection] = useState<Section>(initial.section);
  const [org, setOrgRaw] = useState(initial.org);
  const [project, setProjectRaw] = useState(initial.project);
  const [pbi, setPbiRaw] = useState<PbiHit | null>(initial.pbi);
  const [workMode, setWorkMode] = useState(initial.workMode);
  // Work Manager's own rail section; the board stays the landing view.
  const [workSection, setWorkSection] = useState<WorkSection>("board");
  // Suite-browser handoff: edit an arbitrary set of cases (not persisted).
  const [caseSelection, setCaseSelection] = useState<{ label: string; caseIds: number[] } | null>(
    null,
  );

  useEffect(() => initTheme(), []);
  // Push the saved ADO pacing into the Rust limiter before anything fetches.
  useEffect(() => applyRateLevel(), []);

  // Keyboard shortcuts: Ctrl+1..5 = tabs, Ctrl+Shift+M = Work Manager
  // (v1's binding). Ctrl+K (palette) is registered in CommandPalette.
  useEffect(() => {
    const order: Section[] = ["manual", "import", "edit", "view", "run", "suites", "ai"];
    const onKey = (e: KeyboardEvent) => {
      if (!e.ctrlKey && !e.metaKey) return;
      if (e.shiftKey && e.key.toLowerCase() === "m") {
        e.preventDefault();
        setWorkMode((w) => !w);
        return;
      }
      const n = Number(e.key);
      if (n >= 1 && n <= order.length) {
        e.preventDefault();
        setSection(order[n - 1]);
        setWorkMode(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Comments typed in the "View in browser" report autosave here over the
  // loopback note listener - persist them into the local notes store.
  useEffect(() => {
    const un = events.caseNoteSaved.listen((e) =>
      saveNote(e.payload.org, e.payload.case_id, e.payload.text),
    );
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, []);

  // One writer for all prefs so no path forgets to persist.
  useEffect(() => {
    savePrefs({ org, project, section, pbi, workMode });
  }, [org, project, section, pbi, workMode]);

  // Changing scope drops the case selection too. It is a list of work item
  // ids handed over from Test Suites, and ids mean nothing in a different
  // project - Update Test Cases would go on showing project A's cases while
  // every permission and every write was aimed at project B. goToSection
  // already clears it, but neither the scope pickers nor Ctrl+K go through
  // goToSection, so it survived the one change that invalidates it.
  const setOrg = (o: string) => {
    setOrgRaw(o);
    setProjectRaw("");
    setPbiRaw(null);
    setCaseSelection(null);
  };
  const setProject = (p: string) => {
    setProjectRaw(p);
    setPbiRaw(null);
    setCaseSelection(null);
  };
  const goToSection = (s: Section) => {
    logUi(`nav: ${s}`);
    setSection(s);
    setWorkMode(false); // any tab click exits Work Manager mode
    setCaseSelection(null); // direct navigation returns Edit to PBI mode
  };
  // Settings acts as a toggle: opening it remembers where you were (tab or
  // Work Manager); clicking the gear again returns you there.
  const beforeSettings = useRef<{ section: Section; workMode: boolean } | null>(null);
  const toggleSettings = () => {
    if (section === "settings" && !workMode) {
      const back = beforeSettings.current;
      beforeSettings.current = null;
      setSection(back?.section ?? "manual");
      setWorkMode(back?.workMode ?? false);
      setCaseSelection(null);
      return;
    }
    beforeSettings.current = { section, workMode };
    goToSection("settings");
  };


  const status = useQuery({
    queryKey: ["auth"],
    queryFn: () => commands.authStatus(),
  });

  // Checked on launch and then quietly once an hour, because this app is
  // left open for days at a time - a launch-only check means a release
  // lands and nobody sees it until they next restart, which for some
  // people is next week.
  //
  // Silent by design: no toast, no spinner, nothing moves. React Query
  // holds the previous answer while a background refetch is in flight, so
  // the only visible effect is the banner below appearing the first time
  // there is genuinely something to say. `refetchIntervalInBackground`
  // keeps it running while the window is minimised, which is exactly when
  // an app left open all week is sitting.
  const update = useQuery({
    queryKey: ["update"],
    queryFn: () => commands.checkUpdate(),
    staleTime: UPDATE_CHECK_MS,
    refetchInterval: UPDATE_CHECK_MS,
    refetchIntervalInBackground: true,
    // A check that fails is not news. It reports itself as `blocked`, the
    // banner stays away, and the next hour tries again.
    retry: false,
  });

  // How far the update package has got. Null until the first event, which
  // is also the first moment the size is known - the backend has to re-ask
  // the feed before it can say how big the download is.
  const [dl, setDl] = useState<{ percent: number; downloaded: number; total: number } | null>(null);

  // One shared signal for "the machine has no network". Reads pause via
  // React Query's own networkMode; the banner below is the part that
  // tells the HUMAN, and the write surfaces gate themselves on it.
  const online = useSyncExternalStore(subscribeOnline, onlineSnapshot);

  const applyUpdate = useMutation({
    mutationFn: async () => {
      setDl(null);
      // The byte fields cross as f64, which the bindings widen to
      // `number | null` because a double can be NaN. A byte count cast from
      // a u64 never is - the fallback is here so the display cannot be, not
      // because it is expected to fire.
      const un = await events.updateProgress.listen((e) =>
        setDl({
          percent: e.payload.percent,
          downloaded: e.payload.downloaded ?? 0,
          total: e.payload.total ?? 0,
        }),
      );
      try {
        const r = await commands.applyUpdate();
        if (r.status === "error") throw new Error(r.error);
      } finally {
        // On success the app restarts into the new build, so this only
        // really matters on the failure path - but leaking a listener per
        // failed attempt is how a retry ends up updating state twice.
        detach(un);
      }
    },
    onError: (e) => {
      setDl(null);
      toast.error(`Update failed: ${e.message}`);
    },
  });

  const signIn = useMutation({
    mutationFn: async () => {
      const r = await commands.signIn();
      if (r.status === "error") throw new Error(r.error);
      return r.data;
    },
    onSuccess: () => qc.invalidateQueries({ queryKey: ["auth"] }),
    onError: (e) => toast.error(`Sign-in failed: ${e.message}`),
  });

  // Dev-only auth override: "out" shows the sign-in screen from a signed-in
  // app (to iterate on it), "in" proceeds without any real session (demo
  // data needs none). DEV_TOOLS is compile-time false in releases, so this
  // state and every branch on it is dead-code-eliminated from client builds.
  const [devAuth, setDevAuth] = useState<"real" | "out" | "in">("real");
  const signedIn =
    DEV_TOOLS && devAuth !== "real" ? devAuth === "in" : Boolean(status.data?.signed_in);

  // The persistent cache is scoped by org and project, which is not the same
  // as being scoped by person. Claim it for whoever is signed in NOW.
  //
  // During RENDER, not in an effect. An effect runs after the commit, so on
  // the render where a new account first appears the screens below have
  // already mounted and seeded their queries from the previous account's
  // cache - the wipe then landed a beat too late to stop it being read.
  // `claimCacheFor` is idempotent and guarded by its own owner key, so
  // calling it every render costs a string compare.
  claimCacheFor(status.data?.account ?? null);

  // Post-update "What's new": once per version change, after sign-in (so it
  // never covers the sign-in screen). Fresh installs record the version
  // silently - see lib/changelog.ts for the rules.
  const [changelog, setChangelog] = useState<ChangelogEntry[] | null>(null);
  const shownChangelogRef = useRef(false);
  useEffect(() => {
    if (!signedIn || shownChangelogRef.current) return;
    shownChangelogRef.current = true;
    getVersion()
      .then((v) => {
        const pending = pendingChangelog(v);
        if (pending.length > 0) setChangelog(pending);
      })
      .catch(() => {
        // version unavailable (tests) - skip quietly
      });
  }, [signedIn]);

  // Keep the AI bridge's defaults in sync with what the user is looking at:
  // org/project + the detected custom-field refs. Fire-and-forget; the
  // bridge simply serves stale context until the next push.
  const { prefs: bridgePrefs } = useFieldRefs(org, project);
  // Re-pushed when the AI Bridge tab toggles a tool, so the change reaches
  // an assistant on its next tools/list rather than after a restart.
  const disabledTools = useSyncExternalStore(subscribeDisabledTools, disabledToolsSnapshot);
  useEffect(() => {
    if (!signedIn || !org || !project) return;
    commands
      .bridgeStatus()
      .then(() =>
        commands.setBridgeContext(
          org,
          project,
          bridgePrefs.moduleRef,
          bridgePrefs.preconditionsRef,
          disabledTools,
        ),
      )
      .catch(() => {});
  }, [
    signedIn,
    org,
    project,
    bridgePrefs.moduleRef,
    bridgePrefs.preconditionsRef,
    disabledTools,
  ]);

  // Delete permission, asked ONCE at sign-in per org/project rather than
  // when the Update Test Cases screen opens. The screen reads this same
  // cache key, so by the time anyone can look for a Delete button the
  // answer is already in - and staleTime: Infinity means the session
  // never re-asks: the permission a user signed in with is the one the
  // UI reflects until they switch org/project or restart. App never
  // unmounts, so the entry is never garbage-collected either.
  useQuery({
    queryKey: ["can-delete", org, project],
    queryFn: () => unwrap(commands.canDeleteTestCases(org, project, null)),
    enabled: signedIn && Boolean(org && project),
    staleTime: Infinity,
    retry: false,
  });

  // Background check for work items newly assigned to you. Rust polls and
  // emits; the choice of toast vs Windows notification is made here,
  // because "can the user see the app" is a frontend question.
  useEffect(() => {
    if (!signedIn || !org || !project) return;
    commands.watchAssignedWork(org, project).catch(() => {});
  }, [signedIn, org, project]);

  useEffect(() => {
    const un = events.workAssigned.listen((e) => {
      const items = e.payload.items;
      if (items.length === 0) return;
      const { title, body } = summarize(items);
      if (appIsInView()) {
        toast.info(title, { description: body, duration: 10_000 });
        return;
      }
      // Out of view - go to the OS, and fall back to a toast they will
      // find on return if notifications are refused.
      osNotify(title, body)
        .then((sent) => {
          if (!sent) toast.info(title, { description: body, duration: 10_000 });
        })
        .catch(() => {});
    });
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, []);

  const dismissChangelog = () => {
    getVersion()
      .then(markChangelogSeen)
      .catch(() => {});
    setChangelog(null);
  };
  // Dev-only preview trigger (DevPanel -> window event). Two entries so the
  // multi-version stacking is visible. Compile-time eliminated in releases.
  useEffect(() => {
    if (!DEV_TOOLS) return;
    const fire = () => setChangelog(CHANGELOG.slice(0, 2));
    window.addEventListener(SHOW_CHANGELOG_EVENT, fire);
    return () => window.removeEventListener(SHOW_CHANGELOG_EVENT, fire);
  }, []);

  // First-run walkthrough: opens once after the first sign-in, and again
  // whenever Settings fires the start-tour event.
  const [tourOpen, setTourOpen] = useState(false);
  useEffect(() => {
    if (signedIn && !tourDone()) {
      const t = setTimeout(() => setTourOpen(true), 800);
      return () => clearTimeout(t);
    }
  }, [signedIn]);
  useEffect(() => {
    const start = () => setTourOpen(true);
    window.addEventListener(START_TOUR_EVENT, start);
    return () => window.removeEventListener(START_TOUR_EVENT, start);
  }, []);

  // Warm the Test Suites data in the background so the screen is ready
  // when the user navigates there (same key/staleTime as the screen).
  useEffect(() => {
    if (!signedIn || !org || !project) return;
    const key = `plans-suites:${org}/${project}`;
    // A fresh disk seed means the screen already has its data - warming
    // over the network would spend an expensive scan for nothing.
    const seed = cacheEntry<PlanWithSuites[]>(key, CACHE.structure.ttlMs);
    if (seed && Date.now() - seed.at < CACHE.structure.staleMs) {
      qc.setQueryData(["plans-suites", org, project], seed.data, { updatedAt: seed.at });
      return;
    }
    qc.prefetchQuery({
      queryKey: ["plans-suites", org, project],
      ...persistentQuery({
        key,
        fetcher: () => unwrap(commands.listPlansWithSuites(org, project)),
        ...CACHE.structure,
      }),
      // Without a long gcTime the unobserved prefetch is garbage-collected
      // after 5 minutes and the screen loads from scratch again.
      gcTime: 60 * 60_000,
    });
  }, [signedIn, org, project, qc]);

  // Warm Run Tests: resolve the PBI's suite via the READ-ONLY finder
  // (never creates a plan/suite - creation stays on the Run screen),
  // seed the same cache RunPanel uses, then prefetch its test points.
  const pbiId = pbi?.id;
  // `begin_test_case_writing` ends with the developer having said where
  // the finished JSON goes. Register that path as a watched file NOW, so
  // the assistant's first write folds into the queue on its own - the
  // second and later writes already did, and needing to import the first
  // one by hand was the only manual step left in the loop.
  //
  // Registration lives here rather than in ImportFile because only the
  // active screen is mounted: intake usually finishes while the developer
  // is on the AI Bridge tab, and a listener inside Import would never see
  // the event. ImportFile arms the OS watcher and checks the file once on
  // mount, so a file that lands while you are elsewhere is picked up when
  // you open the tab.
  useEffect(() => {
    if (!signedIn || !org || !pbiId) return;
    let unlisten: (() => void) | undefined;
    let live = true;
    void events.intakeOutputPath
      .listen((e) => {
        const path = e.payload.path?.trim();
        if (!path) return;
        const list = loadWatches(org, pbiId);
        // Re-running begin with the same answers must not reset a file
        // already being followed - that would drop its snapshot and make
        // the next edit read as though the whole file were new.
        if (list.some((w) => w.path === path)) return;
        saveWatches(org, pbiId, upsertWatch(list, { path, stamp: "", snapshot: [] }));
        toast.info(`Watching ${path.split(/[\\/]/).pop()} - it will import itself when written.`);
      })
      .then((f) => {
        if (live) unlisten = f;
        else detach(f);
      });
    return () => {
      live = false;
      detach(unlisten);
    };
  }, [signedIn, org, pbiId]);

  useEffect(() => {
    if (!signedIn || !org || !project || pbiId == null) return;
    const suiteKey = `tcm-v2-suite:${org}/${pbiId}`;
    (async () => {
      let suite: { plan_id: number; plan_name: string; suite_id: number } | null = null;
      try {
        const raw = localStorage.getItem(suiteKey);
        suite = raw ? JSON.parse(raw) : null;
      } catch {
        suite = null;
      }
      if (!suite) {
        const r = await commands.findPbiSuite(org, project, pbiId).catch(() => null);
        if (r && r.status === "ok" && r.data) {
          suite = r.data;
          try {
            localStorage.setItem(suiteKey, JSON.stringify(suite));
          } catch {
            // cache is best-effort
          }
        }
      }
      if (!suite) return;
      const s = suite;
      qc.setQueryData(["suite", org, project, pbiId], s);
      qc.prefetchQuery({
        queryKey: ["points", org, project, s.plan_id, s.suite_id],
        queryFn: () => unwrap(commands.listTestPoints(org, project, s.plan_id, s.suite_id)),
        gcTime: 30 * 60_000,
      });
    })();
  }, [signedIn, org, project, pbiId, qc]);

  return (
    <div className="flex h-screen flex-col bg-bg text-text">
      {/* select-none: dragging a toast to dismiss must not highlight its text. */}
      <Toaster
        theme={getTheme() === "light" ? "light" : "dark"}
        richColors
        position="bottom-right"
        toastOptions={{ className: "select-none" }}
      />
      <CommandPalette
        onNavigate={goToSection}
        org={org}
        onSwitchProject={setProject}
        onToggleWork={() => setWorkMode((w) => !w)}
      />

      <TitleBar
        title={(workMode ? "Work Manager" : "Test Case Manager") + (DEV_TOOLS ? " — DEV" : "")}
      />
      {tourOpen && signedIn && <UiTour onClose={() => setTourOpen(false)} />}

      <div className="flex min-h-0 flex-1">
      {/* Work Manager swaps the rail's contents: its own sections (Pull
          Requests first, then the board) instead of the test-case tabs. */}
      {signedIn &&
        (workMode ? (
          <Sidebar
            section={workSection}
            onSelect={(w) => {
              logUi(`nav: work/${w}`);
              setWorkSection(w);
            }}
            items={WORK_ITEMS}
          />
        ) : (
          <Sidebar section={section} onSelect={goToSection} />
        ))}

      <div className="flex min-w-0 flex-1 flex-col">
        {signedIn && (
          <ContextBar
            org={org}
            setOrg={setOrg}
            project={project}
            setProject={setProject}
            pbi={pbi}
            setPbi={setPbiRaw}
            account={status.data?.account ?? null}
            workMode={workMode}
            onToggleWork={() => setWorkMode((w) => !w)}
            onOpenSettings={toggleSettings}
            settingsOpen={section === "settings" && !workMode}
          />
        )}

        {!online && (
          <div className="border-b border-warning/40 bg-warning/10 px-6 py-2 text-sm text-warning">
            No internet connection - network actions are paused until it returns. Drafts,
            comments and everything local keep working.
          </div>
        )}

        {update.data?.available && (
          <div className="border-b border-accent/40 bg-accent-soft px-6 py-2 text-sm">
            {/* One row either way: the version line makes the offer, and
                once the button is clicked the progress bar takes its slot -
                the banner never grows a second row mid-download. */}
            <div className="flex items-center justify-between gap-4">
              {applyUpdate.isPending ? (
                <div className="flex min-w-0 flex-1 items-center gap-3">
                  <div
                    role="progressbar"
                    aria-label={`Downloading version ${update.data.available}`}
                    aria-valuemin={0}
                    aria-valuemax={100}
                    // Omitted, not zero, until the first event: an indeterminate
                    // bar is what "we do not know yet" means to a screen reader,
                    // and 0% would be a claim.
                    aria-valuenow={dl ? dl.percent : undefined}
                    aria-valuetext={dl ? formatByteProgress(dl.downloaded, dl.total) : undefined}
                    className="h-1.5 flex-1 overflow-hidden rounded-full bg-accent/20"
                  >
                    <div
                      className="h-full rounded-full bg-accent transition-[width] duration-300 ease-out"
                      style={{ width: `${dl?.percent ?? 0}%` }}
                    />
                  </div>
                  {/* Tabular figures: the numerator changes every few seconds
                      and proportional digits make the whole line jitter. */}
                  <span className="shrink-0 tabular-nums text-xs text-muted">
                    {!dl
                      ? "Preparing…"
                      : dl.percent >= 100
                        ? "Installing…"
                        : dl.total > 0
                          ? formatByteProgress(dl.downloaded, dl.total)
                          : `${dl.percent}%`}
                  </span>
                </div>
              ) : (
                <span>Version {update.data.available} is available.</span>
              )}
              <Button size="sm" disabled={applyUpdate.isPending} onClick={() => applyUpdate.mutate()}>
                <IconRefresh aria-hidden className={applyUpdate.isPending ? "animate-spin" : undefined} />
                {applyUpdate.isPending ? "Updating" : "Restart to update"}
              </Button>
            </div>
          </div>
        )}

        {/* Work Manager scrolls inside its own columns - the outer main
            must not add a second scrollbar around the board. */}
        <main
          className={
            signedIn && workMode
              ? // No bottom padding in work mode: the board's columns own
                // the full height, and a padded strip under them read as a
                // gap at the bottom of the screen - boards run flush to the
                // edge, the way every kanban surface does.
                "flex min-h-0 flex-1 flex-col overflow-hidden px-6 pt-6"
              : "min-h-0 flex-1 overflow-y-auto p-6"
          }
        >
          {!signedIn ? (
            <SignIn signingIn={signIn.isPending} onSignIn={() => signIn.mutate()}>
              {DEV_TOOLS && (
                <button
                  className="text-xs text-muted underline underline-offset-2 hover:text-text"
                  onClick={() => setDevAuth("in")}
                >
                  Skip sign-in — dev only (pair with demo data)
                </button>
              )}
            </SignIn>
          ) : workMode ? (
            // Same fade-up as the Test Case Manager tabs below: key remounts
            // on section switch; the flex classes keep the board's height
            // chain intact (the wrapper sits inside a flex-col main).
            <AnimatedContent
              key={workSection}
              distance={14}
              duration={0.3}
              threshold={0}
              className="flex min-h-0 flex-1 flex-col"
            >
              {workSection === "board" ? (
                <>
                  <h1 className="mb-4 text-lg font-semibold">Board</h1>
                  <div className="min-h-0 flex-1">
                    <WorkBoard org={org} project={project} />
                  </div>
                </>
              ) : workSection === "create" ? (
                <>
                  <h1 className="mb-4 text-lg font-semibold">New Work Item</h1>
                  <div className="min-h-0 flex-1 overflow-y-auto">
                    <CreateWorkItem org={org} project={project} />
                  </div>
                </>
              ) : (
                <>
                  <h1 className="mb-4 text-lg font-semibold">Pull Requests</h1>
                  <div className="min-h-0 flex-1 overflow-y-auto">
                    <PrPanel org={org} project={project} />
                  </div>
                </>
              )}
            </AnimatedContent>
          ) : (
            // key={section} remounts the wrapper on tab switch, so every
            // screen fades up briefly instead of snapping in.
            <AnimatedContent key={section} distance={14} duration={0.3} threshold={0}>
              <h1 className="mb-4 text-lg font-semibold">{TITLES[section]}</h1>
              {section === "manual" && (
                <ManualEntry org={org} project={project} pbi={pbi} onPickPbi={setPbiRaw} />
              )}
              {section === "import" && (
                <ImportFile org={org} project={project} pbi={pbi} onPickPbi={setPbiRaw} />
              )}
              {section === "edit" && (
                <EditCases
                  org={org}
                  project={project}
                  pbi={pbi}
                  caseSelection={caseSelection}
                  onClearSelection={() => setCaseSelection(null)}
                  onPickPbi={setPbiRaw}
                />
              )}
              {section === "view" && (
                <ViewCases org={org} project={project} pbi={pbi} onPickPbi={setPbiRaw} />
              )}
              {section === "run" && (
                <RunTests org={org} project={project} pbi={pbi} onPickPbi={setPbiRaw} />
              )}
              {section === "suites" && (
                <Suites
                  org={org}
                  project={project}
                  onOpenPbi={(p, target) => {
                    setPbiRaw({ id: p.id, title: p.title, work_item_type: "Product Backlog Item" });
                    goToSection(target === "edit" ? "edit" : "run");
                  }}
                  onEditCases={(label, caseIds) => {
                    setCaseSelection({ label, caseIds });
                    setSection("edit");
                    setWorkMode(false);
                  }}
                />
              )}
              {section === "ai" && <AiBridge />}
              {section === "settings" && <Settings org={org} project={project} />}
            </AnimatedContent>
          )}
        </main>
      </div>
      </div>

      {changelog && <ChangelogModal entries={changelog} onClose={dismissChangelog} />}

      {DEV_TOOLS && signedIn && (
        <Suspense fallback={null}>
          <DevPanel
            org={org}
            project={project}
            pbi={pbi}
            section={section}
            workMode={workMode}
            onShowSignIn={() => setDevAuth("out")}
          />
        </Suspense>
      )}
    </div>
  );
}
