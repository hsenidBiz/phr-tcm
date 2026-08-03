import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { open } from "@tauri-apps/plugin-dialog";
import { Database } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { commands, type DbServerConfig } from "../bindings";
import { copyText } from "../lib/clipboard";
import { buildConnString, EMPTY_FIELDS, isRepresentable, parseConnString, type ConnFields } from "../lib/connString";
import { Button } from "../components/ui/button";
import { Checkbox } from "../components/ui/checkbox";
import { Switch } from "../components/ui/switch";
import { Input } from "../components/ui/input";
import { Select } from "../components/ui/select";
import { cn } from "../lib/cn";
import {
  forgetDbConfig,
  isDbConfigComplete,
  loadDbConfig,
  saveDbConfig,
} from "../lib/dbServer";
import { loadDisabledTools, MCP_TOOLS, saveDisabledTools, toggleTool } from "../lib/mcpTools";
import { unwrapStr } from "../lib/ipc";
import {
  IconBrowse,
  IconConfirm,
  IconCopy,
  IconRefresh,
  IconRegister,
  IconUnregister,
} from "../lib/actionIcons";

/** Config keys, mirroring `ai_tools.rs` - a tool row shows a separate
 * state for each server this app can register. */
const TCM_SERVER = "tcm-testcases";
const DB_SERVER = "phr-db-mcp";

/** Clipboard copies are fire-and-forget from the UI's perspective, but the
 * promise must always be handled - a bare `.then()` leaves rejected copies
 * (permission denied, no clipboard API) as unhandled rejections that fail
 * the test/release gate even though the app looks fine. */
function copy(text: string, label: string) {
  copyText(text)
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

  // Tools the user has switched off; App re-pushes these to the bridge.
  const [disabled, setDisabled] = useState<string[]>(loadDisabledTools);
  // The company's database MCP server. Settings persist locally so a
  // second editor can be registered without retyping the connection
  // string - see lib/dbServer.ts for why that is acceptable here.
  const [db, setDb] = useState<DbServerConfig>(loadDbConfig);
  const editDb = (patch: Partial<DbServerConfig>) => {
    const next = { ...db, ...patch };
    setDb(next);
    saveDbConfig(next);
  };

  // The connection-string FIELDS are a view over the stored string: parsed
  // out on every render, rebuilt on every keystroke. No second copy of the
  // secret, and a string saved before this form existed appears already
  // filled in. The raw editor opens automatically for a string the fields
  // cannot faithfully represent, so it is never silently rewritten.
  // An EMPTY config starts from the defaults (trust the certificate - the
  // company DB's is self-signed); an existing string is read as written,
  // where an absent flag genuinely means off.
  const conn = db.connection_string.trim()
    ? parseConnString(db.connection_string)
    : { ...EMPTY_FIELDS };
  const editConn = (patch: Partial<ConnFields>) =>
    editDb({ connection_string: buildConnString({ ...conn, ...patch }) });
  const [rawConn, setRawConn] = useState(() => !isRepresentable(db.connection_string));

  const registerDb = useMutation({
    mutationFn: (id: string) => unwrapStr(commands.registerDbServer(id, db)),
    onSuccess: () => {
      toast.success("Database server registered.");
      qc.invalidateQueries({ queryKey: ["ai-tools"] });
    },
    onError: (e) => toast.error(`Could not register: ${e.message}`),
  });

  const unregisterDb = useMutation({
    mutationFn: (id: string) => unwrapStr(commands.unregisterDbServer(id)),
    onSuccess: () => {
      toast.success("Database server unregistered.");
      qc.invalidateQueries({ queryKey: ["ai-tools"] });
    },
    onError: (e) => toast.error(`Could not unregister: ${e.message}`),
  });

  const pickExe = () => {
    open({
      multiple: false,
      filters: [{ name: "Server executable", extensions: ["exe"] }],
    })
      .then((path) => {
        if (typeof path === "string") editDb({ exe_path: path });
      })
      .catch(() => toast.error("Could not open the file picker."));
  };

  const exe = bridge.data?.mcp_exe ?? "";
  const installed = (tools.data ?? []).filter((t) => t.installed);
  const dbReady = isDbConfigComplete(db);

  return (
    // Two columns once there is room (the window floor is 900px, so
    // this only kicks in above it); a single column below, which is
    // also what the narrow runner-sized windows get.
    <div className="grid max-w-lg gap-6 lg:max-w-6xl lg:grid-cols-2 lg:items-start">
      <div className="space-y-6">
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
        <div className="flex items-center justify-between gap-2">
          <h2 className="text-sm font-semibold text-text">Connect your AI tools</h2>
          {/* Detection runs once on mount - rescan after installing a tool
              (or registering one outside the app) without a restart. */}
          <Button
            size="sm"
            variant="outline"
            disabled={tools.isFetching}
            onClick={() => {
              tools
                .refetch()
                .then((r) => {
                  const n = (r.data ?? []).filter((t) => t.installed).length;
                  toast.success(`Found ${n} installed AI tool${n === 1 ? "" : "s"}.`);
                })
                .catch(() => toast.error("Could not scan for AI tools."));
            }}
          >
            <IconRefresh aria-hidden className={cn(tools.isFetching && "animate-spin")} />
            {tools.isFetching ? "Scanning" : "Rescan"}
          </Button>
        </div>
        {tools.isPending ? (
          // "None detected" while the scan is still running reads as a
          // verdict - say what's actually happening instead.
          <p className="text-xs text-faint">Scanning for installed AI tools...</p>
        ) : installed.length === 0 ? (
          <p className="text-xs text-muted">No supported AI tools detected on this machine.</p>
        ) : (
          <ul className="space-y-2">
            {installed.map((t) => (
              <li key={t.id} className="flex items-center justify-between gap-2 text-sm">
                <span className="text-text">{t.name}</span>
                {(t.registered_servers ?? []).includes(TCM_SERVER) ? (
                  <span className="flex items-center gap-2">
                    <span className="text-xs text-success">Registered ✓</span>
                    <Button
                      size="sm"
                      variant="ghost"
                      disabled={unregister.isPending && unregister.variables === t.id}
                      onClick={() => unregister.mutate(t.id)}
                    >
                      <IconUnregister aria-hidden />
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
                    <IconRegister aria-hidden />
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
                  <IconCopy aria-hidden />
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
                  <IconCopy aria-hidden />
                  Copy
                </Button>
              </div>
            </div>
          </div>
        </details>
      </section>

      <section className="space-y-3 rounded-md border border-border bg-surface p-4">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <h2 className="text-sm font-semibold text-text">Tools an assistant may use</h2>
          <span className="text-xs text-faint">
            {MCP_TOOLS.length - disabled.length} of {MCP_TOOLS.length} on
          </span>
        </div>
        <p className="text-xs text-muted">
          Switch a tool off to keep it out of an assistant's reach. It disappears from
          the tool list on their next request, and a call to it is refused even if they
          cached the old list. Applies to this app's tools only.
        </p>
        <ul className="space-y-1.5">
          {MCP_TOOLS.map((t) => {
            const on = !disabled.includes(t.name);
            return (
              <li key={t.name} className="flex items-start gap-2">
                <Switch
                  checked={on}
                  ariaLabel={t.name}
                  onCheckedChange={() => {
                    const next = toggleTool(disabled, t.name);
                    setDisabled(next);
                    saveDisabledTools(next);
                  }}
                  className="mt-0.5"
                />
                <span className="min-w-0 flex-1">
                  <span className={cn("id-mono text-xs", on ? "text-text" : "text-faint")}>
                    {t.name}
                  </span>
                  <span className="block text-[11px] text-muted">{t.summary}</span>
                </span>
              </li>
            );
          })}
        </ul>
        {disabled.length > 0 && (
          <Button
            size="sm"
            variant="outline"
            onClick={() => {
              setDisabled([]);
              saveDisabledTools([]);
            }}
          >
            <IconConfirm aria-hidden />
            Turn all back on
          </Button>
        )}
      </section>

      </div>

      {/* Right column: the two tallest cards, so neither column runs
          far past the other. */}
      <div className="space-y-6">
      <section className="space-y-3 rounded-md border border-border bg-surface p-4">
        <div className="flex items-center gap-2">
          <Database size={14} className="shrink-0 text-muted" />
          <h2 className="text-sm font-semibold text-text">Company database (PHR-X)</h2>
        </div>
        <p className="text-xs text-muted">
          Register your company's database MCP server beside this one, so an assistant
          can read the schema and your test cases in the same session. Point it at the
          built <span className="id-mono">PeoplesHR.DBMCPServer.exe</span> and give it
          the connection settings from its README.
        </p>

        <div className="space-y-2">
          <div className="flex items-end gap-2">
            <label className="min-w-0 flex-1 text-xs text-muted">
              Server executable
              <Input
                aria-label="Database server executable"
                className="mt-1 w-full py-1.5 text-xs"
                placeholder="…\PeoplesHR.DBMCPServer.exe"
                value={db.exe_path}
                onChange={(e) => editDb({ exe_path: e.target.value })}
              />
            </label>
            <Button size="sm" variant="outline" onClick={pickExe}>
              <IconBrowse aria-hidden />
              Browse
            </Button>
          </div>

          <label className="block text-xs text-muted">
            DB_TYPE
            <Select
              aria-label="Database type"
              className="mt-1 w-full py-1.5 text-xs"
              value={db.db_type}
              onChange={(e) => editDb({ db_type: e.target.value })}
            >
              <option value="mssql">mssql</option>
              <option value="sqlserver">sqlserver</option>
            </Select>
          </label>

          {/* CONNECTION_STRING, built from fields rather than typed whole.
              The stored value is still the single string the MCP server
              receives - these inputs are a view over it, parsed out on
              every render and rebuilt on every keystroke, so a string
              saved before this form existed appears already filled in.
              The raw editor stays available for a string the fields
              cannot faithfully represent - which is also the mode the
              form OPENS in for such a string, so it is never silently
              rewritten into something simpler. */}
          <div className="space-y-2 rounded-md border border-border/60 p-2">
            <div className="flex items-center justify-between">
              <span className="text-xs font-medium text-muted">CONNECTION_STRING</span>
              <label className="flex items-center gap-1.5 text-[11px] text-faint">
                <Checkbox
                  ariaLabel="Edit connection string as text"
                  checked={rawConn}
                  onCheckedChange={setRawConn}
                />
                Edit as one string
              </label>
            </div>
            {rawConn ? (
              <Input
                aria-label="Connection string"
                // Masked on screen; it still travels into each tool's MCP
                // config, which is how MCP passes environment to a server.
                type="password"
                className="id-mono w-full py-1.5 text-xs"
                placeholder="Server=host,1433;Database=…;User Id=…;Password=…;TrustServerCertificate=True;"
                value={db.connection_string}
                onChange={(e) => editDb({ connection_string: e.target.value })}
              />
            ) : (
              <>
                <div className="flex gap-2">
                  <label className="min-w-0 flex-1 text-xs text-muted">
                    Server host
                    <Input
                      aria-label="Database host"
                      className="mt-1 w-full py-1.5 text-xs"
                      placeholder="phrx-db.internal"
                      value={conn.host}
                      onChange={(e) => editConn({ host: e.target.value })}
                    />
                  </label>
                  <label className="w-20 text-xs text-muted">
                    Port
                    <Input
                      aria-label="Database port"
                      className="mt-1 w-full py-1.5 text-xs"
                      placeholder="1433"
                      value={conn.port}
                      onChange={(e) => editConn({ port: e.target.value })}
                    />
                  </label>
                </div>
                <label className="block text-xs text-muted">
                  Database
                  <Input
                    aria-label="Database name"
                    className="mt-1 w-full py-1.5 text-xs"
                    value={conn.database}
                    onChange={(e) => editConn({ database: e.target.value })}
                  />
                </label>
                <div className="flex gap-2">
                  <label className="min-w-0 flex-1 text-xs text-muted">
                    User
                    <Input
                      aria-label="Database user"
                      className="mt-1 w-full py-1.5 text-xs"
                      value={conn.user}
                      onChange={(e) => editConn({ user: e.target.value })}
                    />
                  </label>
                  <label className="min-w-0 flex-1 text-xs text-muted">
                    Password
                    <Input
                      aria-label="Database password"
                      type="password"
                      className="mt-1 w-full py-1.5 text-xs"
                      value={conn.password}
                      onChange={(e) => editConn({ password: e.target.value })}
                    />
                  </label>
                </div>
                <label className="flex items-center gap-2 text-xs text-muted">
                  <Checkbox
                    ariaLabel="Trust the server certificate"
                    checked={conn.trustCert}
                    onCheckedChange={(v) => editConn({ trustCert: v })}
                  />
                  Trust the server certificate
                  <span className="text-faint">(company DB uses a self-signed one)</span>
                </label>
              </>
            )}
          </div>

          <label className="block text-xs text-muted">
            SCHEMA_FILTER <span className="text-faint">(optional)</span>
            <Input
              aria-label="Schema filter"
              className="mt-1 w-full py-1.5 text-xs"
              placeholder="dbo,hr — blank uses the server's default"
              value={db.schema_filter}
              onChange={(e) => editDb({ schema_filter: e.target.value })}
            />
          </label>
        </div>

        {!dbReady ? (
          <p className="text-xs text-faint">
            Fill in the executable and connection string to enable registration.
          </p>
        ) : installed.length === 0 ? (
          <p className="text-xs text-muted">No supported AI tools detected on this machine.</p>
        ) : (
          <ul className="space-y-2 border-t border-border/60 pt-2">
            {installed.map((t) => (
              <li key={t.id} className="flex items-center justify-between gap-2 text-sm">
                <span className="text-text">{t.name}</span>
                {(t.registered_servers ?? []).includes(DB_SERVER) ? (
                  <span className="flex items-center gap-2">
                    <span className="text-xs text-success">Registered ✓</span>
                    <Button
                      size="sm"
                      variant="ghost"
                      disabled={unregisterDb.isPending && unregisterDb.variables === t.id}
                      onClick={() => unregisterDb.mutate(t.id)}
                    >
                      <IconUnregister aria-hidden />
                      {unregisterDb.isPending && unregisterDb.variables === t.id
                        ? "Removing"
                        : "Unregister"}
                    </Button>
                  </span>
                ) : (
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={registerDb.isPending && registerDb.variables === t.id}
                    onClick={() => registerDb.mutate(t.id)}
                  >
                    <IconRegister aria-hidden />
                    {registerDb.isPending && registerDb.variables === t.id
                      ? "Registering"
                      : "Register"}
                  </Button>
                )}
              </li>
            ))}
          </ul>
        )}

        <p className="text-[11px] text-faint">
          These settings are stored on this machine so you can register another editor
          without retyping them, and are written into each tool's MCP config.{" "}
          <button
            className="underline underline-offset-2 hover:text-danger"
            onClick={() => {
              forgetDbConfig();
              setDb(loadDbConfig());
              toast.success("Database settings forgotten.");
            }}
          >
            Forget them
          </button>{" "}
          — this clears the form only; unregister above to remove them from a tool.
        </p>
      </section>

      <section className="space-y-3 rounded-md border border-border bg-surface p-4">
        <h2 className="text-sm font-semibold text-text">How it works</h2>
        <p className="text-sm text-muted">
          Connected AI tools can call ten tools this app exposes. All of them
          either read or reshape the AI's own draft — none can write to Azure
          DevOps:
        </p>
        <ul className="space-y-1.5 text-xs text-muted">
          <li>
            <code className="id-mono text-text">begin_test_case_writing</code> — the
            starting point. It hands the assistant a checklist to put to you in chat:
            where the JSON goes, which spec documents are authoritative, whether to
            check a PBI for duplicates, tags and module, what is out of scope. Your
            answers are checked against the real paths and values, and written to a
            plan file for you to approve before a single case exists.
          </li>
          <li>
            <code className="id-mono text-text">get_writing_guide</code> — the live
            guide for writing import JSON: format rules, your org's allowed Module
            values, and the recommended workflow.
          </li>
          <li>
            <code className="id-mono text-text">get_test_cases</code> — the test cases
            already linked to a PBI, in the exact import shape: to copy the house
            style, to check what is already covered, or just to read them.
          </li>
          <li>
            <code className="id-mono text-text">validate_cases</code> — runs a draft
            through this app's real importer and returns the case count, warnings, and
            errors. Large drafts can be validated from a file path.
          </li>
          <li>
            <code className="id-mono text-text">get_tags</code> — the tag names this
            project already uses, so the AI reuses yours instead of inventing
            near-duplicates. Served from this app's cache, at no request cost.
          </li>
          <li>
            <code className="id-mono text-text">optimize_cases</code> — reorganises a
            finished draft into a run sheet: navigation spelled out as steps rather
            than buried in preconditions, expected results cut down to the outcome,
            and the cases reordered so the tester changes environment as few times as
            possible.
          </li>
          <li>
            <code className="id-mono text-text">transform_cases</code> — bulk edits
            (retag, retitle, set module, find/replace in steps, sort, dedupe) so the
            AI reshapes a draft through tested operations instead of writing its own
            throwaway script.
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
          example cases, have it draft cases for your PBI, run them through
          optimize_cases, then import the result yourself via the Import File
          tab. Keep the file watched and any problems appear here as it saves —
          the AI never has to ask whether the draft is valid.
        </p>
        <p className="text-xs text-faint">
          AI can never create, update, or delete anything in Azure DevOps
          through this bridge - it only reads.
        </p>
      </section>
      </div>
    </div>
  );
}
