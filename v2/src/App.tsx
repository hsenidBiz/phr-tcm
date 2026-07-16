import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { Toaster, toast } from "sonner";
import { commands, events, type PbiHit } from "./bindings";
import { saveNote } from "./lib/caseNotes";
import AnimatedContent from "./components/AnimatedContent";
import AnimatedFlask from "./components/AnimatedFlask";
import ShinyText from "./components/ShinyText";
import SplitText from "./components/SplitText";
import Threads from "./components/Threads";
import CommandPalette from "./components/CommandPalette";
import ContextBar from "./components/ContextBar";
import Sidebar, { type Section } from "./components/Sidebar";
import TitleBar from "./components/TitleBar";
import UiTour, { START_TOUR_EVENT, tourDone } from "./components/UiTour";
import { Button } from "./components/ui/button";
import { unwrap } from "./lib/ipc";
import { getTheme, initTheme } from "./lib/theme";
import { hasWebGL } from "./lib/webgl";
import EditCases from "./screens/EditCases";
import ImportFile from "./screens/ImportFile";
import ManualEntry from "./screens/ManualEntry";
import RunTests from "./screens/RunTests";
import ViewCases from "./screens/ViewCases";
import DevPanel from "./dev/DevPanel";

/** Compile-time dev gate: statically false in `tauri build`, so everything
 * behind it (and the dev/ module itself) is dead-code-eliminated from
 * released builds. Test mode opts out so vitest sees the plain app. */
const DEV_TOOLS = import.meta.env.DEV && import.meta.env.MODE !== "test";
import Settings from "./screens/Settings";
import Suites from "./screens/Suites";
import WorkBoard from "./screens/WorkBoard";

const PREFS_KEY = "tcm-v2-prefs";

type Prefs = {
  org: string;
  project: string;
  section: Section;
  pbi: PbiHit | null;
  workMode: boolean;
};

const SECTIONS: Section[] = ["manual", "import", "edit", "run", "suites", "settings"];

function loadPrefs(): Prefs {
  const defaults: Prefs = {
    org: "",
    project: "",
    section: "manual",
    pbi: null,
    workMode: false,
  };
  try {
    const raw = localStorage.getItem(PREFS_KEY);
    if (raw) {
      const p = JSON.parse(raw);
      return {
        org: p.org ?? "",
        project: p.project ?? "",
        section: SECTIONS.includes(p.section) ? p.section : "manual",
        pbi:
          p.pbi && typeof p.pbi.id === "number" && typeof p.pbi.title === "string"
            ? { id: p.pbi.id, title: p.pbi.title, work_item_type: p.pbi.work_item_type ?? "" }
            : null,
        workMode: Boolean(p.workMode),
      };
    }
  } catch {
    // corrupted prefs -> defaults
  }
  return defaults;
}

const TITLES: Record<Section, string> = {
  manual: "Manual Entry",
  import: "Import File",
  edit: "Edit Test Cases",
  view: "View Test Cases",
  run: "Run Tests",
  suites: "Test Suites",
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
  // Suite-browser handoff: edit an arbitrary set of cases (not persisted).
  const [caseSelection, setCaseSelection] = useState<{ label: string; caseIds: number[] } | null>(
    null,
  );

  useEffect(() => initTheme(), []);

  // Sign-in Threads backdrop, tinted to the accent the theme resolved above.
  // Stays null without WebGL, which is the signal not to render it at all.
  const [threadsColor, setThreadsColor] = useState<[number, number, number] | null>(null);
  useEffect(() => {
    if (!hasWebGL()) return;
    const accent = getComputedStyle(document.documentElement)
      .getPropertyValue("--color-accent")
      .trim();
    const m = /^#([0-9a-f]{6})$/i.exec(accent);
    if (!m) return;
    const int = parseInt(m[1], 16);
    setThreadsColor([((int >> 16) & 255) / 255, ((int >> 8) & 255) / 255, (int & 255) / 255]);
  }, []);

  // Keyboard shortcuts: Ctrl+1..5 = tabs, Ctrl+Shift+M = Work Manager
  // (v1's binding). Ctrl+K (palette) is registered in CommandPalette.
  useEffect(() => {
    const order: Section[] = ["manual", "import", "edit", "view", "run", "suites"];
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
    try {
      localStorage.setItem(
        PREFS_KEY,
        JSON.stringify({ org, project, section, pbi, workMode }),
      );
    } catch {
      // storage unavailable -> session-only
    }
  }, [org, project, section, pbi, workMode]);

  const setOrg = (o: string) => {
    setOrgRaw(o);
    setProjectRaw("");
    setPbiRaw(null);
  };
  const setProject = (p: string) => {
    setProjectRaw(p);
    setPbiRaw(null);
  };
  const goToSection = (s: Section) => {
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

  const update = useQuery({
    queryKey: ["update"],
    queryFn: () => commands.checkUpdate(),
    staleTime: Infinity,
    retry: false,
  });

  const applyUpdate = useMutation({
    mutationFn: async () => {
      const r = await commands.applyUpdate();
      if (r.status === "error") throw new Error(r.error);
    },
    onError: (e) => toast.error(`Update failed: ${e.message}`),
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

  const signedIn = Boolean(status.data?.signed_in);

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
    qc.prefetchQuery({
      queryKey: ["plans-suites", org, project],
      queryFn: () => unwrap(commands.listPlansWithSuites(org, project)),
      staleTime: Infinity,
      // Without a long gcTime the unobserved prefetch is garbage-collected
      // after 5 minutes and the screen loads from scratch again.
      gcTime: 60 * 60_000,
    });
  }, [signedIn, org, project, qc]);

  // Warm Run Tests: resolve the PBI's suite via the READ-ONLY finder
  // (never creates a plan/suite - creation stays on the Run screen),
  // seed the same cache RunPanel uses, then prefetch its test points.
  const pbiId = pbi?.id;
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
      <Toaster theme={getTheme() === "light" ? "light" : "dark"} richColors position="bottom-right" />
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
      {signedIn && <Sidebar section={section} onSelect={goToSection} />}

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

        {update.data && (
          <div className="flex items-center justify-between border-b border-accent/40 bg-accent-soft px-6 py-2 text-sm">
            <span>Version {update.data} is available.</span>
            <Button size="sm" disabled={applyUpdate.isPending} onClick={() => applyUpdate.mutate()}>
              {applyUpdate.isPending ? "Updating" : "Restart to update"}
            </Button>
          </div>
        )}

        {/* Work Manager scrolls inside its own columns - the outer main
            must not add a second scrollbar around the board. */}
        <main
          className={
            signedIn && workMode
              ? "flex min-h-0 flex-1 flex-col overflow-hidden p-6"
              : "min-h-0 flex-1 overflow-y-auto p-6"
          }
        >
          {!signedIn ? (
            <div className="relative flex h-full flex-col items-center justify-center">
              {/* React Bits Threads backdrop: slow accent-tinted lines, kept
                  subtle (low amplitude, no mouse tracking, faded). Gated on
                  WebGL so sign-in still renders where there is no GPU
                  context (RDP, software-rendered VDI). */}
              {/* -inset-6 cancels <main>'s p-6 so the lines run edge to edge
                  instead of stopping at the content padding. */}
              {threadsColor && (
                <div
                  aria-hidden
                  className="pointer-events-none absolute -inset-6 overflow-hidden opacity-40"
                >
                  <Threads color={threadsColor} amplitude={0.8} distance={0} />
                </div>
              )}
              <div className="relative flex flex-col items-center gap-4">
                <AnimatedFlask />
                <SplitText
                  text="Test Case Manager"
                  tag="h1"
                  className="text-xl font-semibold"
                  delay={40}
                  duration={0.8}
                />
                <p className="max-w-sm text-center text-sm text-muted">
                  Sign in with your Microsoft account to manage Azure DevOps test
                  cases, runs, and work items.
                </p>
                <Button disabled={signIn.isPending} onClick={() => signIn.mutate()}>
                  {signIn.isPending ? (
                    "Waiting for browser"
                  ) : (
                    <ShinyText
                      text="Sign in with Microsoft"
                      speed={3}
                      color="rgba(255, 255, 255, 0.85)"
                      shineColor="#ffffff"
                    />
                  )}
                </Button>
              </div>
            </div>
          ) : workMode ? (
            <>
              <h1 className="mb-4 text-lg font-semibold">Work Manager</h1>
              <div className="min-h-0 flex-1">
                <WorkBoard org={org} project={project} />
              </div>
            </>
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
              {section === "settings" && <Settings org={org} project={project} />}
            </AnimatedContent>
          )}
        </main>
      </div>
      </div>

      {DEV_TOOLS && signedIn && (
        <DevPanel org={org} project={project} pbi={pbi} section={section} workMode={workMode} />
      )}
    </div>
  );
}
