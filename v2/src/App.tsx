import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { commands } from "./bindings";
import Browse from "./screens/Browse";
import WorkBoard from "./screens/WorkBoard";

// Last-used selection, restored on launch (user-facing prefs live in the
// webview; secrets never do).
const PREFS_KEY = "tcm-v2-prefs";

function loadPrefs(): { org: string; project: string; mode: "tests" | "work" } {
  try {
    const raw = localStorage.getItem(PREFS_KEY);
    if (raw) return { org: "", project: "", mode: "tests", ...JSON.parse(raw) };
  } catch {
    // corrupted prefs -> defaults
  }
  return { org: "", project: "", mode: "tests" };
}

function savePrefs(p: { org: string; project: string; mode: "tests" | "work" }) {
  try {
    localStorage.setItem(PREFS_KEY, JSON.stringify(p));
  } catch {
    // storage unavailable -> session-only
  }
}

export default function App() {
  const qc = useQueryClient();
  const prefs = loadPrefs();
  const [mode, setModeRaw] = useState<"tests" | "work">(prefs.mode);
  const [org, setOrgRaw] = useState(prefs.org);
  const [project, setProjectRaw] = useState(prefs.project);

  const setMode = (m: "tests" | "work") => {
    setModeRaw(m);
    savePrefs({ org, project, mode: m });
  };
  const setOrg = (o: string) => {
    setOrgRaw(o);
    savePrefs({ org: o, project: "", mode });
  };
  const setProject = (p: string) => {
    setProjectRaw(p);
    savePrefs({ org, project: p, mode });
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
  });

  const signIn = useMutation({
    mutationFn: async () => {
      const r = await commands.signIn();
      if (r.status === "error") throw new Error(r.error);
      return r.data;
    },
    onSuccess: () => qc.invalidateQueries({ queryKey: ["auth"] }),
  });

  const signedIn = Boolean(status.data?.signed_in);

  return (
    <div className="min-h-screen bg-neutral-950 text-neutral-100">
      <header className="flex items-center justify-between border-b border-neutral-800 px-6 py-4">
        <h1 className="text-lg font-semibold">
          {mode === "tests" ? "Test Case Manager V2" : "Work Manager V2"}
        </h1>
        <div className="flex items-center gap-4">
          {signedIn && (
            <button
              className="rounded-full border border-blue-500/60 px-4 py-1.5 text-sm text-blue-400 hover:bg-blue-500/10"
              onClick={() => setMode(mode === "tests" ? "work" : "tests")}
            >
              {mode === "tests" ? "Work Manager (Beta)" : "Test Case Manager"}
            </button>
          )}
          {status.data?.account && (
            <span className="text-sm text-neutral-400">{status.data.account}</span>
          )}
        </div>
      </header>

      {update.data && (
        <div className="flex items-center justify-between border-b border-blue-900 bg-blue-950/60 px-6 py-2 text-sm">
          <span>Version {update.data} is available.</span>
          <button
            className="rounded-md bg-blue-600 px-3 py-1 text-xs font-medium hover:bg-blue-500 disabled:opacity-50"
            disabled={applyUpdate.isPending}
            onClick={() => applyUpdate.mutate()}
          >
            {applyUpdate.isPending ? "Updating..." : "Restart to update"}
          </button>
        </div>
      )}
      {applyUpdate.isError && (
        <p className="px-6 py-1 text-sm text-red-400">Update failed: {applyUpdate.error.message}</p>
      )}

      <main className="p-6">
        {!signedIn ? (
          <div className="space-y-3">
            <button
              className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium hover:bg-blue-500 disabled:opacity-50"
              disabled={signIn.isPending}
              onClick={() => signIn.mutate()}
            >
              {signIn.isPending ? "Waiting for browser..." : "Sign in with Microsoft"}
            </button>
            {signIn.isError && (
              <p className="text-sm text-red-400">Sign-in failed: {signIn.error.message}</p>
            )}
          </div>
        ) : mode === "tests" ? (
          <Browse org={org} setOrg={setOrg} project={project} setProject={setProject} />
        ) : (
          <WorkBoard org={org} project={project} />
        )}
      </main>
    </div>
  );
}
