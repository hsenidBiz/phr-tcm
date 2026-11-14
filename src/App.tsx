import { QueryClientProvider, useQuery, useQueryClient } from "@tanstack/react-query";
import { getVersion } from "@tauri-apps/api/app";
import { Suspense, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { toast } from "./lib/toast";
import { useActiveEnvironmentName } from "./lib/environments";
import { Toaster } from "./components/ui/toaster";
import { commands, type PbiHit } from "./bindings";
import { onlineSnapshot, subscribeOnline } from "./lib/network";
import { clearSessionExpired } from "./lib/sessionExpired";
import {
  clearWorkAlerts,
  subscribeWorkAlerts,
  workAlertsSnapshot,
} from "./lib/workAlerts";
import type { NotificationTarget } from "./lib/notifications";
import { requestAutoRunFocus } from "./lib/autoRunFocus";
import ActivityStrip from "./components/ActivityStrip";
import { unwrap } from "./lib/ipc";
import { isBetaVersion } from "./lib/changelogSeen";
import type { RunFolderTarget } from "./screens/RunTests";
import SessionExpiredModal from "./components/SessionExpiredModal";
import RunPill from "./components/RunPill";
import { clearReviewRequest } from "./lib/backgroundRun";
import { useOpenOnExportRequest } from "./lib/playwrightExport";
import { useBackgroundRunHost } from "./hooks/useBackgroundRunHost";
import { useAutoRunDataSync } from "./hooks/useAutoRunDataSync";
import { waitingRows } from "./lib/waitingRows";
import { useEnvironmentChangeSync } from "./hooks/useEnvironmentChangeSync";
import { useAutoRunScriptsSync } from "./hooks/useAutoRunScriptsSync";
import ContextBar from "./components/ContextBar";
import { effectiveProject, useAllProjects } from "./lib/projectScope";
import Sidebar, { WORK_ITEMS, type Section, type WorkSection } from "./components/Sidebar";
import { useModules, useModulesHydrated } from "./lib/modules";
import TitleBar from "./components/TitleBar";
import UiTour from "./tour/UiTour";
import { tourControl, tourStepsFor, type TourWhere } from "./tour/tourScript";
import { logUi } from "./lib/uiLog";
import { loadPrefs } from "./lib/prefs";
import { isCaptureMode } from "./dev/capture";
import HalloweenDecor, { HalloweenWeb } from "./components/HalloweenDecor";
import ChristmasDecor, { ChristmasCorner } from "./components/ChristmasDecor";
import {
  ChangelogModal,
  CommandPalette,
  DEV_TOOLS,
  DevPanel,
  SignIn,
} from "./app/lazyScreens";
import CaseScreens from "./app/CaseScreens";
import WorkScreens from "./app/WorkScreens";
import UpdateBanner from "./app/UpdateBanner";
import { RunWindowHost } from "./app/RunWindowHost";
import type { AppContext, CaseSelection } from "./app/types";
import { useAppTour, useTourLifecycle } from "./app/useAppTour";
import { useAppNavigation } from "./app/useAppNavigation";
import { useAppUpdate } from "./app/useAppUpdate";
import {
  useAssignedWork,
  useGlobalBackendEvents,
  useIntakeWatch,
  useInterruptedUploadCheck,
  useWaitingNotices,
} from "./app/useBackendEvents";
import { useBridgeContextSync } from "./app/useBridgeContextSync";
import { useChangelog } from "./app/useChangelog";
import { useHomeProject } from "./app/useHomeProject";
import { useKeyboardShortcuts, useModuleGuards, usePersistPrefs, useStartupEffects } from "./app/useShellEffects";
import { useAppSession } from "./app/useAppSession";
import { useWarmCaches } from "./app/useWarmCaches";

export { RunWindowHost };

export default function App() {
  const qc = useQueryClient();
  const initial = loadPrefs();
  const [section, setSection] = useState<Section>(initial.section);
  const modules = useModules();
  const modulesHydrated = useModulesHydrated();
  // Settings is showing a page that fills the window (Modules) rather than
  // a page that scrolls inside the shell.
  const [settingsFill, setSettingsFill] = useState(false);
  // The tour skips the stop for a card whose module is off.
  const tourSteps = useMemo(() => tourStepsFor(modules), [modules]);
  const [org, setOrgRaw] = useState(initial.org);
  // The home project: where Auto Run, API templates and Playwright keep their
  // files, and what a screen with no PBI falls back to. It is the project the
  // person picked (switched off) or the one the first PBI / the only project /
  // the Auto Run files chose (projects shown together).
  const [homeProject, setHomeProjectRaw] = useState(initial.project);
  const [pbi, setPbiRaw] = useState<PbiHit | null>(initial.pbi);
  const allProjects = useAllProjects();
  // The project every PBI-scoped screen and write uses: the PBI's own while
  // the projects show together, otherwise the home project.
  const project = effectiveProject(allProjects, pbi, homeProject);
  const [workMode, setWorkMode] = useState(initial.workMode);
  // Work Manager's own rail section; the board stays the landing view.
  const [workSection, setWorkSection] = useState<WorkSection>("board");
  // Suite-browser handoff: edit an arbitrary set of cases (not persisted).
  // The cases belong to their plan's project, which may not be the effective
  // one: Update Test Cases deletes, edits and tags them there.
  const [caseSelection, setCaseSelection] = useState<CaseSelection | null>(null);
  // A Test plan folder opened in Run Tests (from Test plans), so a static
  // suite runs there like a PBI's does. Picking a PBI, or any direct
  // navigation, drops it.
  const [runFolder, setRunFolder] = useState<RunFolderTarget | null>(null);
  // A suite handed over from Search Suites' Manage option. Cleared once
  // Suite Management has it, so going back to the tab later shows the
  // ordinary PBI-driven view rather than reopening an old click. It carries
  // its plan's project: Suite Management opens and writes the plan there,
  // which with the projects together is not the effective project.
  const [manageFocus, setManageFocus] = useState<{ planId: number; suiteId: number; project: string } | null>(
    null,
  );
  // The same one-shot handoff for the bell: a notification names a work
  // item or a pull request, and the Work Manager screen that can show it
  // clears this the moment it has.
  const [workFocus, setWorkFocus] = useState<NotificationTarget | null>(null);

  // Read inside stable callbacks, so starting the tour does not depend on
  // a fresh closure over seven pieces of state.
  const ctx = useRef<AppContext>({ section, org, homeProject, pbi, workMode, workSection, caseSelection });
  ctx.current = { section, org, homeProject, pbi, workMode, workSection, caseSelection };

  // First-run walkthrough: opens once after the first sign-in, and again
  // whenever Settings asks for it. Declared ahead of every other effect in
  // this component, because several of them gate on `tourOpen`.
  const {
    tourOpen,
    tourQc,
    tourAwaited,
    setTourAwaited,
    tourActing,
    setTourActing,
    startTour,
    endTour,
    tourNavigate,
  } = useAppTour({
    qc,
    ctx,
    setOrgRaw,
    setHomeProjectRaw,
    setPbiRaw,
    setSection,
    setWorkMode,
    setWorkSection,
    setCaseSelection,
  });
  // Where the app is - the overlay measures "is this stop a move?" against
  // it - and, while the tour waits, the ONE control that gets the user
  // from here to there. Every other rail row, and the whole context bar,
  // locks itself on `tourOpen` alone.
  const tourAt: TourWhere = workMode ? { area: "work", workSection } : { area: "cases", section };
  const tourControlNow = tourOpen && tourAwaited ? tourControl(tourAwaited, tourAt) : null;

  // Turning the switch OFF while a PBI of another project is open: the
  // Project selector comes back pre-filled with that PBI's project, so the
  // PBI is kept and every screen stays where it was. Done while rendering,
  // not in an effect, so no screen is ever drawn for the old home project in
  // between. The tour refuses the switch (saveAllProjects), so this never
  // runs over the walkthrough's sample scope - and is skipped there anyway.
  // Turning it ON changes nothing else, except that the case selection (work
  // item ids of the old project) goes when the PBI's project is not the home
  // project, the same rule a PBI pick follows.
  const [seenAllProjects, setSeenAllProjects] = useState(allProjects);
  if (seenAllProjects !== allProjects) {
    setSeenAllProjects(allProjects);
    // (Turning it off sets the home project to the PBI's just below, so the
    // effective project does not move then.)
    if (allProjects && effectiveProject(true, pbi, homeProject) !== homeProject) setCaseSelection(null);
    if (!allProjects && !tourOpen && pbi?.project && pbi.project !== homeProject) {
      setHomeProjectRaw(pbi.project);
    }
  }

  useStartupEffects();
  useModuleGuards(section, modules, modulesHydrated, workMode, tourOpen, setSection);
  useKeyboardShortcuts(setSection, setWorkMode);
  useGlobalBackendEvents();
  usePersistPrefs({ org, homeProject, section, pbi, workMode, tourOpen });

  // Changing scope drops the case selection too. It is a list of work item
  // ids handed over from Search Suites, and ids mean nothing in a different
  // project - Update Test Cases would go on showing project A's cases while
  // every permission and every write was aimed at project B. goToSection
  // already clears it, but neither the scope pickers nor Ctrl+K go through
  // goToSection, so it survived the one change that invalidates it.
  const setOrg = (o: string) => {
    setOrgRaw(o);
    setHomeProjectRaw("");
    setPbiRaw(null);
    setCaseSelection(null);
    // A pending notification handoff belongs to the organisation it was
    // raised in; carrying it across would open someone else's item id.
    setWorkFocus(null);
  };
  const setProject = (p: string) => {
    setHomeProjectRaw(p);
    setPbiRaw(null);
    setCaseSelection(null);
  };
  // Every PBI pick (the context bar, the screens' own pickers, Search
  // Suites' hand-over) comes through here. A PBI of another project moves the
  // effective project exactly as a project change does, so the case
  // selection (work item ids of the old project) goes with it. On a fresh
  // install the first PBI's project becomes the home project. The tour and
  // the unattended-run host keep the raw setter.
  const setPbi = (p: PbiHit | null) => {
    if (effectiveProject(allProjects, p, homeProject) !== project) setCaseSelection(null);
    if (allProjects && !homeProject && p?.project) {
      setHomeProjectRaw(p.project);
      // Auto Run, API Templates and Playwright keep their files there, so
      // say so rather than let it happen unseen. An organization with one
      // project had it chosen already: nothing to announce.
      if (projects.data?.length !== 1) {
        toast.info(`Home project set to ${p.project}. Change it in Settings.`);
      }
    }
    if (p) setRunFolder(null);
    setPbiRaw(p);
  };
  // Settings' Home project picker. Moving the home project moves the
  // effective project only when no PBI pins it, and then the case selection
  // (work item ids of the old project) goes, the same rule setPbi follows.
  const changeHomeProject = (p: string) => {
    if (effectiveProject(allProjects, pbi, p) !== project) setCaseSelection(null);
    setHomeProjectRaw(p);
  };
  const { startOnModules, setStartOnModules, goToSection, openPbiById, toggleSettings } = useAppNavigation({
    section,
    workMode,
    tourOpen,
    org,
    project,
    allProjects,
    setSection,
    setWorkMode,
    setCaseSelection,
    setRunFolder,
    setProject,
    setPbi,
  });
  // Auto Run's Export to Playwright is a shortcut to the Playwright tab,
  // which holds the Export dialog (lib/playwrightExport).
  useOpenOnExportRequest(() => {
    if (section !== "playwright" || workMode) goToSection("playwright");
  });
  // A script saved, repaired or deleted anywhere shows on Auto Run at once.
  useAutoRunScriptsSync();
  // What the assistant adds beside the scripts (areas, components, the map,
  // quirks, test files, accounts, fixtures) shows on Auto Run at once too.
  useAutoRunDataSync();
  // A switch, copy or import of environments reads everything of the old
  // environment's again.
  useEnvironmentChangeSync();
  // The unattended run outlives the Auto Run screen (lib/backgroundRun):
  // its end is caught here, and Review on its pill or toast brings the
  // person to the run's PBI in Auto Run, whose screen opens the review.
  // Not over the walkthrough, whose sample scope is not the person's.
  useBackgroundRunHost((r) => {
    if (tourOpen) {
      clearReviewRequest();
      return;
    }
    if (allProjects) {
      // Together, the run's PBI carries its own project, so a run of this
      // organization leaves the home project alone. A run of another
      // organization brings that run's project home: the one held now is a
      // project of the organization being left.
      if (org !== r.org) {
        setOrgRaw(r.org);
        setHomeProjectRaw(r.project);
        setCaseSelection(null);
      }
      if (pbi?.id !== r.pbi.id) {
        if (effectiveProject(true, r.pbi, homeProject) !== project) setCaseSelection(null);
        setPbiRaw(r.pbi);
      }
    } else {
      if (org !== r.org || homeProject !== r.project) {
        setOrgRaw(r.org);
        setHomeProjectRaw(r.project);
        setCaseSelection(null);
      }
      if (pbi?.id !== r.pbi.id) setPbiRaw(r.pbi);
    }
    if (section !== "autorun" || workMode) goToSection("autorun");
  });

  const { status, signIn, sessionExpired, reSignIn, signedIn, setDevAuth } = useAppSession();

  // The release check, and the download behind the banner below.
  const { update, dl, applyUpdate } = useAppUpdate();

  // One shared signal for "the machine has no network". Reads pause via
  // React Query's own networkMode; the banner below is the part that
  // tells the HUMAN, and the write surfaces gate themselves on it.
  const online = useSyncExternalStore(subscribeOnline, onlineSnapshot);

  // Assignments announced while the user was elsewhere, counted on the
  // Board rail item. Looking at the board IS the acknowledgement.
  const workAlerts = useSyncExternalStore(subscribeWorkAlerts, workAlertsSnapshot);
  useEffect(() => {
    if (workMode && workSection === "board") clearWorkAlerts();
  }, [workMode, workSection, workAlerts]);
  // Uploads that died before they finished get one bell notice each.
  useInterruptedUploadCheck(signedIn, org, tourOpen, status.data?.account ?? "");

  // What the assistants left for a person to decide, for the Auto Run rail's
  // count and the bell. Auto Run's files belong to the home project, so that
  // is the project asked about while the projects show together.
  const autoRunProject = allProjects ? homeProject : project;
  const waiting = useWaitingNotices(
    org,
    autoRunProject,
    signedIn && modules.has("auto_run") && !tourOpen,
    tourOpen,
  );

  // Post-update "What's new".
  const { changelog, dismissChangelog } = useChangelog(signedIn);

  // The project the bridge learns is the home project while the projects
  // show together: it stays put as PBIs of other projects are picked (the
  // assistant names a project when it means another one).
  const bridgeProject = allProjects ? homeProject : project;
  useBridgeContextSync({ signedIn, org, project, bridgeProject, allProjects, pbi, tourOpen });

  // The organization's projects, and the home project nobody has to pick.
  const projects = useHomeProject({
    signedIn,
    org,
    tourOpen,
    allProjects,
    homeProject,
    hadProject: Boolean(initial.project),
    ctx,
    setHomeProjectRaw,
    setProject,
  });

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

  useAssignedWork(signedIn, org, bridgeProject, allProjects, tourOpen);

  // The title bar names the active environment once there are two or more.
  const environmentName = useActiveEnvironmentName();
  // The running version, for the title bar's Beta pill - the same cached
  // query Settings reads, so asking twice costs nothing.
  const appVersion = useQuery({
    queryKey: ["app-version"],
    queryFn: () => getVersion().catch(() => "dev"),
    staleTime: Infinity,
  });

  useTourLifecycle(signedIn, tourOpen, startTour, endTour);

  const pbiId = pbi?.id;
  useWarmCaches(signedIn, org, project, pbiId, tourOpen);
  useIntakeWatch(signedIn, org, pbiId, tourOpen);

  return (
    <div className="flex h-screen flex-col bg-bg text-text">
      {/* The window's one toast host. XiodUI's toasts are select-none
          already, so dragging one away highlights nothing. */}
      <Toaster />
      <Suspense fallback={null}>
        <CommandPalette
          onNavigate={goToSection}
          org={org}
          allProjects={allProjects}
          onSwitchProject={setProject}
          onToggleWork={() => setWorkMode((w) => !w)}
        />
      </Suspense>

      <TitleBar
        // Capture mode: the custom title bar renders on every screen, so
        // " — DEV" would land in every shot - see dev/capture.ts. The Beta
        // pill likewise: shots taken on a beta build would all carry it.
        title={
          (workMode ? "Work Manager" : "Test Case Manager") +
          (DEV_TOOLS && !isCaptureMode() ? " — DEV" : "")
        }
        beta={isBetaVersion(appVersion.data ?? "") && !isCaptureMode()}
        // Environments are an Auto Run setting: named only where it shows.
        environment={modules.has("auto_run") ? environmentName : null}
        // An unattended run going, paused, or ended: a click opens its
        // window from anywhere, or its review once it has finished.
        status={<RunPill />}
      />
      {tourOpen && signedIn && (
        <UiTour
          at={tourAt}
          onNavigate={tourNavigate}
          onAwait={setTourAwaited}
          onAct={setTourActing}
          onClose={endTour}
          steps={tourSteps}
        />
      )}

      <QueryClientProvider client={tourQc ?? qc}>
        {/* The rail and the bar sit OUTSIDE the inert region below: `inert`
            is inherited, so a hole cannot be punched through it for the one
            control the tour is waiting for. They lock themselves instead. */}
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
                // A manual section pick supersedes any pending notification
                // handoff - otherwise a focus the target screen never got
                // to claim (the board failed to load, say) could pop its
                // drawer later, on a screen the user navigated to on
                // their own and was not expecting it to react to.
                setWorkFocus(null);
              }}
              items={WORK_ITEMS}
              badges={{ board: workAlerts }}
              locked={tourOpen}
              liveItem={tourControlNow?.kind === "work" ? tourControlNow.workSection : null}            />
          ) : (
            <Sidebar
              section={section}
              onSelect={goToSection}
              badges={{ autorun: waitingRows(waiting?.items ?? []).length }}
              locked={tourOpen}
              liveItem={tourControlNow?.kind === "case" ? tourControlNow.section : null}            />
          ))}

        {/* isolate: the seasonal tree sits at z-index -1 in here, behind
            the page's own buttons but still above the window background. */}
        <div className="relative isolate flex min-w-0 flex-1 flex-col">
          {signedIn && (
            <ContextBar
              org={org}
              setOrg={setOrg}
              project={project}
              setProject={setProject}
              allProjects={allProjects}
              homeProject={homeProject}
              pbi={pbi}
              setPbi={setPbi}
              account={status.data?.account ?? null}
              workMode={workMode}
              onToggleWork={() => {
                setWorkMode((w) => !w);
                // Same reasoning as the rail's onSelect: leaving Work
                // Manager (or entering it by hand) drops a stale focus
                // rather than letting it surface on whatever is next.
                setWorkFocus(null);
              }}
              onOpenSettings={toggleSettings}
              settingsOpen={section === "settings" && !workMode}
              locked={tourOpen}
              workLive={tourControlNow?.kind === "switch"}
              // Settings has no rail row, so the gear is the one control
              // left live while a Settings stop waits.
              settingsLive={tourControlNow?.kind === "case" && tourControlNow.section === "settings"}
              onOpenNotification={(target) => {
                logUi(`nav: notification/${target.kind}`);
                // The target's own project, not necessarily the one on
                // screen: a notification can arrive for a project you have
                // since navigated away from. Picked one at a time, that means
                // switching there (setProject only clears the PBI and case
                // selection, not workFocus, so setting the focus after it
                // survives). Shown together there is nothing to switch and
                // the home project is not the notification's to overwrite:
                // the focus carries the project, and the board and the Pull
                // Requests panel, which read every project, open the item or
                // pull request in it.
                if (target.kind === "pbi") {
                  void openPbiById(target.id, target.project, "import");
                  return;
                }
                if (target.kind === "auto-run") {
                  // Auto Run switched off: there is no screen to open.
                  if (!modules.has("auto_run")) return;
                  const f = target.focus;
                  goToSection("autorun");
                  requestAutoRunFocus(f);
                  return;
                }
                if (!allProjects && target.project !== project) setProject(target.project);
                setWorkFocus(target);
                setWorkSection(target.kind === "pr" ? "prs" : "board");
                setWorkMode(true);
              }}
            />
          )}
          {signedIn && <HalloweenDecor />}
          {signedIn && <HalloweenWeb />}
          {signedIn && <ChristmasDecor />}
          {signedIn && <ChristmasCorner />}
          {/* Inert for the whole tour, EXCEPT at a stop the user is meant
              to act on: there the ringed control has to answer a click,
              and inert is inherited, so nothing inside it could. The
              overlay covers everything but the ring for those stops. */}
          <div className="flex min-h-0 flex-1 flex-col" inert={tourOpen && !tourActing}>

          {!online && (
            <div className="border-b border-warning/40 bg-warning/10 px-6 py-2 text-sm text-warning">
              No internet connection - network actions are paused until it returns. Drafts,
              comments and everything local keep working.
            </div>
          )}

          {/* Capture mode: the update banner must never cover a shot. */}
          {update.data?.available && !isCaptureMode() && (
            <UpdateBanner
              available={update.data.available}
              failedAttempt={Boolean(update.data.failed_attempt)}
              applying={applyUpdate.isPending}
              dl={dl}
              onApply={() => applyUpdate.mutate()}
            />
          )}

          {/* What the assistants are doing, on every screen: in the layout
              flow, so it pushes the page down rather than covering it. */}
          {signedIn && modules.has("auto_run") && <ActivityStrip onOpen={() => goToSection("autorun")} />}

          {/* Work Manager scrolls inside its own columns - the outer main
              must not add a second scrollbar around the board. */}
          <main
            className={
              signedIn &&
              (workMode ||
                (modules.has("playwright") && section === "playwright") ||
                (modules.has("crystal_reports") && section === "crystal-reports") ||
                // Settings' Modules page: its board scrolls inside its own frame.
                (section === "settings" && settingsFill))
                ? // No bottom padding in work mode: the board's columns own
                  // the full height, and a padded strip under them read as a
                  // gap at the bottom of the screen - boards run flush to the
                  // edge, the way every kanban surface does. The Playwright
                  // tab is laid out the same way: its runner's panels fill
                  // the window and scroll inside themselves. So is Crystal
                  // Reports: its pages fill what is left of the window and
                  // scroll inside their own view.
                  "flex min-h-0 flex-1 flex-col overflow-hidden px-6 pt-6"
                : // Room under the last thing on every screen, so what floats
                  // at the bottom - the sticky Collapse all at the left, the
                  // action dock at the right - never sits over the last row
                  // once the page is scrolled to its end.
                  "min-h-0 flex-1 overflow-y-auto px-6 pt-6 pb-24"
            }
          >
            {/* Screens load on demand (see app/lazyScreens). The boundary sits
                outside the fade wrapper, so the fade-in plays when a screen
                has arrived rather than on an empty box. */}
            <Suspense fallback={null}>
            {!signedIn ? (
              <SignIn signingIn={signIn.isPending} onSignIn={() => signIn.mutate()}>
                {/* Capture mode: sign-in is a documented screen too. */}
                {DEV_TOOLS && !isCaptureMode() && (
                  <button
                    className="text-xs text-muted underline underline-offset-2 hover:text-text"
                    onClick={() => setDevAuth("in")}
                  >
                    Skip sign-in — dev only (pair with demo data)
                  </button>
                )}
              </SignIn>
            ) : workMode ? (
              <WorkScreens
                workSection={workSection}
                org={org}
                project={project}
                homeProject={homeProject}
                allProjects={allProjects}
                workFocus={workFocus}
                onFocusHandled={() => setWorkFocus(null)}
              />
            ) : (
              <CaseScreens
                section={section}
                org={org}
                project={project}
                homeProject={homeProject}
                allProjects={allProjects}
                pbi={pbi}
                modules={modules}
                caseSelection={caseSelection}
                runFolder={runFolder}
                manageFocus={manageFocus}
                settingsFill={settingsFill}
                startOnModules={startOnModules}
                setPbi={setPbi}
                setCaseSelection={setCaseSelection}
                setRunFolder={setRunFolder}
                setManageFocus={setManageFocus}
                setSection={setSection}
                setWorkMode={setWorkMode}
                setSettingsFill={setSettingsFill}
                setStartOnModules={setStartOnModules}
                goToSection={goToSection}
                changeHomeProject={changeHomeProject}
              />
            )}
            </Suspense>
          </main>
          </div>
        </div>
        </div>
      </QueryClientProvider>

      {changelog && (
        <Suspense fallback={null}>
          <ChangelogModal entries={changelog} onClose={dismissChangelog} />
        </Suspense>
      )}

      {/* The unattended run's window: mounted here, not in Auto Run, so the
          run's progress and its pauses can be opened from any section (the
          title-bar pill), and Run in background closes it without touching
          the run. It shows only while the run's store says it is open. */}
      <RunWindowHost />

      {/* Only over a signed-in app: before sign-in the SignIn screen IS the
          prompt. "Not now" just closes it - cached data stays readable and
          the next failed fetch raises it again. */}
      {/* Capture mode: the session-expired modal must never cover a shot. */}
      {signedIn && sessionExpired && !isCaptureMode() && (
        <SessionExpiredModal
          signingIn={signIn.isPending}
          onSignIn={reSignIn}
          onDismiss={clearSessionExpired}
        />
      )}

      {/* Capture mode: the DEV BUILD panel must never appear in a shot. */}
      {DEV_TOOLS && signedIn && !isCaptureMode() && (
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
