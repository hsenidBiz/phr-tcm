import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { Toaster, toast } from "sonner";
import { commands } from "./bindings";
import CommandPalette from "./components/CommandPalette";
import ContextBar from "./components/ContextBar";
import Sidebar, { type Section } from "./components/Sidebar";
import { Button } from "./components/ui/button";
import { getTheme, initTheme } from "./lib/theme";
import Browse from "./screens/Browse";
import Settings from "./screens/Settings";
import WorkBoard from "./screens/WorkBoard";

const PREFS_KEY = "tcm-v2-prefs";

type Prefs = { org: string; project: string; section: Section };

function loadPrefs(): Prefs {
  try {
    const raw = localStorage.getItem(PREFS_KEY);
    if (raw) {
      const p = JSON.parse(raw);
      return {
        org: p.org ?? "",
        project: p.project ?? "",
        // "mode" was the pre-iteration-1 key; map it forward.
        section: p.section ?? (p.mode === "work" ? "work" : "tests"),
      };
    }
  } catch {
    // corrupted prefs -> defaults
  }
  return { org: "", project: "", section: "tests" };
}

function savePrefs(p: Prefs) {
  try {
    localStorage.setItem(PREFS_KEY, JSON.stringify(p));
  } catch {
    // storage unavailable -> session-only
  }
}

const TITLES: Record<Section, string> = {
  tests: "Test Cases",
  work: "Work",
  settings: "Settings",
};

export default function App() {
  const qc = useQueryClient();
  const prefs = loadPrefs();
  const [section, setSectionRaw] = useState<Section>(prefs.section);
  const [org, setOrgRaw] = useState(prefs.org);
  const [project, setProjectRaw] = useState(prefs.project);

  useEffect(() => initTheme(), []);

  const setSection = (s: Section) => {
    setSectionRaw(s);
    savePrefs({ org, project, section: s });
  };
  const setOrg = (o: string) => {
    setOrgRaw(o);
    savePrefs({ org: o, project: "", section });
  };
  const setProject = (p: string) => {
    setProjectRaw(p);
    savePrefs({ org, project: p, section });
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
        onNavigate={setSection}
        org={org}
        onSwitchProject={setProject}
      />

      {signedIn && <Sidebar section={section} onSelect={setSection} />}

      <div className="flex min-w-0 flex-1 flex-col">
        {signedIn && (
          <ContextBar
            org={org}
            setOrg={setOrg}
            project={project}
            setProject={setProject}
            account={status.data?.account ?? null}
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
          ) : (
            <>
              <h1 className="mb-4 text-lg font-semibold">{TITLES[section]}</h1>
              {section === "tests" && <Browse org={org} project={project} />}
              {section === "work" && <WorkBoard org={org} project={project} />}
              {section === "settings" && <Settings org={org} project={project} />}
            </>
          )}
        </main>
      </div>
    </div>
  );
}
