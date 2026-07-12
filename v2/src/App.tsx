import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { Toaster, toast } from "sonner";
import { commands, type PbiHit } from "./bindings";
import CommandPalette from "./components/CommandPalette";
import ContextBar from "./components/ContextBar";
import Sidebar, { type Section } from "./components/Sidebar";
import { Button } from "./components/ui/button";
import { getTheme, initTheme } from "./lib/theme";
import EditCases from "./screens/EditCases";
import ImportFile from "./screens/ImportFile";
import ManualEntry from "./screens/ManualEntry";
import RunTests from "./screens/RunTests";
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

  useEffect(() => initTheme(), []);

  // Keyboard shortcuts: Ctrl+1..5 = tabs, Ctrl+Shift+M = Work Manager
  // (v1's binding). Ctrl+K (palette) is registered in CommandPalette.
  useEffect(() => {
    const order: Section[] = ["manual", "import", "edit", "run", "suites"];
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

  return (
    <div className="flex h-screen bg-bg text-text">
      <Toaster theme={getTheme() === "light" ? "light" : "dark"} richColors position="bottom-right" />
      <CommandPalette
        onNavigate={goToSection}
        org={org}
        onSwitchProject={setProject}
        onToggleWork={() => setWorkMode((w) => !w)}
      />

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
            onOpenSettings={() => goToSection("settings")}
          />
        )}

        {update.data && (
          <div className="flex items-center justify-between border-b border-accent/40 bg-accent-soft px-6 py-2 text-sm">
            <span>Version {update.data} is available.</span>
            <Button size="sm" disabled={applyUpdate.isPending} onClick={() => applyUpdate.mutate()}>
              {applyUpdate.isPending ? "Updating..." : "Restart to update"}
            </Button>
          </div>
        )}

        <main className="min-h-0 flex-1 overflow-y-auto p-6">
          {!signedIn ? (
            <div className="flex h-full flex-col items-center justify-center gap-4">
              <h1 className="text-xl font-semibold">Test Case Manager V2</h1>
              <p className="max-w-sm text-center text-sm text-muted">
                Sign in with your Microsoft account to manage Azure DevOps test
                cases, runs, and work items.
              </p>
              <Button disabled={signIn.isPending} onClick={() => signIn.mutate()}>
                {signIn.isPending ? "Waiting for browser..." : "Sign in with Microsoft"}
              </Button>
            </div>
          ) : workMode ? (
            <>
              <h1 className="mb-4 text-lg font-semibold">Work Manager</h1>
              <WorkBoard org={org} project={project} />
            </>
          ) : (
            <>
              <h1 className="mb-4 text-lg font-semibold">{TITLES[section]}</h1>
              {section === "manual" && <ManualEntry org={org} project={project} pbi={pbi} />}
              {section === "import" && <ImportFile org={org} project={project} pbi={pbi} />}
              {section === "edit" && <EditCases org={org} project={project} pbi={pbi} />}
              {section === "run" && <RunTests org={org} project={project} pbi={pbi} />}
              {section === "suites" && (
                <Suites
                  org={org}
                  project={project}
                  onOpenPbi={(p, target) => {
                    setPbiRaw({ id: p.id, title: p.title, work_item_type: "Product Backlog Item" });
                    goToSection(target === "edit" ? "edit" : "run");
                  }}
                />
              )}
              {section === "settings" && <Settings org={org} project={project} />}
            </>
          )}
        </main>
      </div>
    </div>
  );
}
