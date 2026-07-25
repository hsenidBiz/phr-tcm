import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { commands } from "../bindings";
import { Button } from "../components/ui/button";
import { unwrapStr } from "../lib/ipc";

/** Clipboard copies are fire-and-forget from the UI's perspective, but the
 * promise must always be handled - a bare `.then()` leaves rejected copies
 * (permission denied, no clipboard API) as unhandled rejections that fail
 * the test/release gate even though the app looks fine. */
function copy(text: string, label: string) {
  navigator.clipboard
    .writeText(text)
    .then(() => toast.success(`${label} copied.`))
    .catch(() => toast.error("Could not copy to clipboard."));
}

export default function AiBridge() {
  const qc = useQueryClient();

  const bridge = useQuery({
    queryKey: ["bridge-status"],
    queryFn: () => unwrapStr(commands.bridgeStatus()),
    retry: false,
  });

  const tools = useQuery({
    queryKey: ["ai-tools"],
    queryFn: () => commands.detectAiTools(),
  });

  const register = useMutation({
    mutationFn: (id: string) => unwrapStr(commands.registerAiTool(id)),
    onSuccess: () => {
      toast.success("Registered.");
      qc.invalidateQueries({ queryKey: ["ai-tools"] });
    },
    onError: (e) => toast.error(`Could not register: ${e.message}`),
  });

  const unregister = useMutation({
    mutationFn: (id: string) => unwrapStr(commands.unregisterAiTool(id)),
    onSuccess: () => {
      toast.success("Unregistered.");
      qc.invalidateQueries({ queryKey: ["ai-tools"] });
    },
    onError: (e) => toast.error(`Could not unregister: ${e.message}`),
  });

  const exe = bridge.data?.mcp_exe ?? "";
  const installed = (tools.data ?? []).filter((t) => t.installed);

  return (
    <div className="max-w-lg space-y-6">
      <section className="space-y-3 rounded-md border border-border bg-surface p-4">
        <h2 className="text-sm font-semibold text-text">Status</h2>
        {bridge.data ? (
          <p className="text-xs text-success">
            Bridge running on port {bridge.data.port}
          </p>
        ) : (
          <p className="text-xs text-faint">Bridge not running.</p>
        )}
        <p className="text-xs text-muted">
          The bridge only runs while this app is open and signed in - AI
          tools can't reach it otherwise.
        </p>
      </section>

      <section className="space-y-3 rounded-md border border-border bg-surface p-4">
        <h2 className="text-sm font-semibold text-text">Connect your AI tools</h2>
        {installed.length === 0 ? (
          <p className="text-xs text-muted">No supported AI tools detected on this machine.</p>
        ) : (
          <ul className="space-y-2">
            {installed.map((t) => (
              <li key={t.id} className="flex items-center justify-between gap-2 text-sm">
                <span className="text-text">{t.name}</span>
                {t.registered ? (
                  <span className="flex items-center gap-2">
                    <span className="text-xs text-success">Registered ✓</span>
                    <Button
                      size="sm"
                      variant="ghost"
                      disabled={unregister.isPending && unregister.variables === t.id}
                      onClick={() => unregister.mutate(t.id)}
                    >
                      {unregister.isPending && unregister.variables === t.id
                        ? "Removing"
                        : "Unregister"}
                    </Button>
                  </span>
                ) : (
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={register.isPending && register.variables === t.id}
                    onClick={() => register.mutate(t.id)}
                  >
                    {register.isPending && register.variables === t.id
                      ? "Registering"
                      : "Register"}
                  </Button>
                )}
              </li>
            ))}
          </ul>
        )}

        <details className="pt-1 text-xs text-muted">
          <summary className="cursor-pointer select-none text-muted hover:text-text">
            Other tools
          </summary>
          <div className="mt-2 space-y-3">
            <div>
              <p className="mb-1 text-faint">Command-line registration:</p>
              <div className="flex items-center gap-2">
                <code className="id-mono flex-1 truncate rounded bg-surface-2 px-2 py-1 text-xs text-text">
                  claude mcp add --scope user tcm-testcases -- "{exe}" --mcp
                </code>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() =>
                    copy(`claude mcp add --scope user tcm-testcases -- "${exe}" --mcp`, "Command")
                  }
                >
                  Copy
                </Button>
              </div>
            </div>
            <div>
              <p className="mb-1 text-faint">Generic MCP config JSON:</p>
              <div className="flex items-start gap-2">
                <pre className="id-mono flex-1 overflow-x-auto rounded bg-surface-2 px-2 py-1 text-xs text-text">
                  {JSON.stringify(
                    { "tcm-testcases": { command: exe, args: ["--mcp"] } },
                    null,
                    2,
                  )}
                </pre>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() =>
                    copy(
                      JSON.stringify(
                        { "tcm-testcases": { command: exe, args: ["--mcp"] } },
                        null,
                        2,
                      ),
                      "Config",
                    )
                  }
                >
                  Copy
                </Button>
              </div>
            </div>
          </div>
        </details>
      </section>

      <section className="space-y-3 rounded-md border border-border bg-surface p-4">
        <h2 className="text-sm font-semibold text-text">How it works</h2>
        <p className="text-sm text-muted">
          Connected AI tools can call six read-only tools this app exposes:
        </p>
        <ul className="space-y-1.5 text-xs text-muted">
          <li>
            <code className="id-mono text-text">get_writing_guide</code> — the live
            guide for writing import JSON: format rules, your org's allowed Module
            values, and the recommended workflow.
          </li>
          <li>
            <code className="id-mono text-text">get_example_cases</code> — real test
            cases already linked to a PBI, in the exact import shape, so the AI can
            mimic their style and granularity.
          </li>
          <li>
            <code className="id-mono text-text">validate_cases</code> — runs a draft
            through this app's real importer and returns the case count, warnings,
            and errors.
          </li>
          <li>
            <code className="id-mono text-text">search_pbis</code> — finds the right
            work item id by searching PBI titles in the current project.
          </li>
          <li>
            <code className="id-mono text-text">search_wiki</code> — searches the
            project's Azure DevOps wiki and returns page paths with snippet
            highlights, for finding documentation about the implementation.
          </li>
          <li>
            <code className="id-mono text-text">get_wiki_page</code> — fetches a wiki
            page's full markdown content, for reading what search_wiki found.
          </li>
        </ul>
        <p className="text-sm text-muted">
          Recommended flow: ask the AI to read the writing guide and some
          example cases, have it draft cases for your PBI, validate them, then
          import the result yourself via the Import File tab.
        </p>
        <p className="text-xs text-faint">
          AI can never create, update, or delete anything in Azure DevOps
          through this bridge - it only reads.
        </p>
      </section>
    </div>
  );
}
