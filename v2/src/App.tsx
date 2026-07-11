import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { commands, type AdoError } from "./bindings";

function describeAdoError(e: AdoError): string {
  switch (e.kind) {
    case "Unauthorized":
      return "Not authorized - sign in again.";
    case "RateLimited":
      return `Rate limited - retry in ${e.detail.retry_after_secs}s.`;
    case "Forbidden":
      return "You don't have permission for this organization.";
    case "NotFound":
      return "Organization not found.";
    case "Http":
      return `Azure DevOps returned HTTP ${e.detail.status}.`;
    case "Network":
      return `Network error: ${e.detail}`;
  }
}

export default function App() {
  const qc = useQueryClient();
  const [org, setOrg] = useState("");

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

  const projects = useQuery({
    queryKey: ["projects", org],
    queryFn: async () => {
      const r = await commands.listProjects(org);
      if (r.status === "error") throw new Error(describeAdoError(r.error));
      return r.data;
    },
    enabled: Boolean(status.data?.signed_in && org),
    retry: false,
  });

  return (
    <div className="min-h-screen bg-neutral-950 text-neutral-100">
      <header className="flex items-center justify-between border-b border-neutral-800 px-6 py-4">
        <h1 className="text-lg font-semibold">Test Case Manager V2</h1>
        {status.data?.account && (
          <span className="text-sm text-neutral-400">{status.data.account}</span>
        )}
      </header>
      <main className="p-6">
        {!status.data?.signed_in ? (
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
        ) : (
          <div className="space-y-4">
            <input
              className="w-72 rounded-md border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm"
              placeholder="Organization name"
              value={org}
              onChange={(e) => setOrg(e.target.value)}
            />
            {projects.isLoading && org && (
              <p className="text-sm text-neutral-400">Loading projects...</p>
            )}
            {projects.isError && (
              <p className="text-sm text-red-400">{projects.error.message}</p>
            )}
            <ul className="space-y-1">
              {(projects.data ?? []).map((p) => (
                <li
                  key={p.id}
                  className="rounded-md border border-neutral-800 px-3 py-2 text-sm"
                >
                  {p.name}
                </li>
              ))}
            </ul>
          </div>
        )}
      </main>
    </div>
  );
}
