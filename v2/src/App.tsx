import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { commands } from "./bindings";
import Browse from "./screens/Browse";
import WorkBoard from "./screens/WorkBoard";

export default function App() {
  const qc = useQueryClient();
  const [mode, setMode] = useState<"tests" | "work">("tests");
  const [org, setOrg] = useState("");
  const [project, setProject] = useState("");

  const status = useQuery({
    queryKey: ["auth"],
    queryFn: () => commands.authStatus(),
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
              onClick={() => setMode((m) => (m === "tests" ? "work" : "tests"))}
            >
              {mode === "tests" ? "Work Manager (Beta)" : "Test Case Manager"}
            </button>
          )}
          {status.data?.account && (
            <span className="text-sm text-neutral-400">{status.data.account}</span>
          )}
        </div>
      </header>
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
