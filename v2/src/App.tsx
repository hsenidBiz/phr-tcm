import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { commands } from "./bindings";
import Browse from "./screens/Browse";

export default function App() {
  const qc = useQueryClient();

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
          <Browse />
        )}
      </main>
    </div>
  );
}
