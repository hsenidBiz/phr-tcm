import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { open } from "@tauri-apps/plugin-dialog";
import { Database, FolderOpen } from "lucide-react";
import { useEffect, useState, useSyncExternalStore } from "react";
import Combobox from "../components/ui/combobox";
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
  hasStoredDbConfig,
  isDbConfigComplete,
  loadDbConfig,
  saveDbConfig,
} from "../lib/dbServer";
import { loadDisabledTools, MCP_TOOLS, saveDisabledTools, toggleTool } from "../lib/mcpTools";
import { unwrapStr } from "../lib/ipc";
import { saveWorkingDir, subscribeWorkingDir, workingDirSnapshot } from "../lib/workingDir";
import { globalAllowedSnapshot, saveScope, scopeSnapshot, subscribeAiScope } from "../lib/aiScope";
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

  // The repository everything on this tab is scoped to. Read through the
  // store so App's bridge push and this tab agree the moment it changes.
  const workingDir = useSyncExternalStore(subscribeWorkingDir, workingDirSnapshot);
  const pickWorkingDir = () => {
    open({ multiple: false, directory: true })
      .then((path) => {
        if (typeof path === "string") {
          saveWorkingDir(path);
          toast.success(`Working repository set to ${path}`);
        }
      })
      .catch(() => toast.error("Could not open the folder picker."));
  };

  const bridge = useQuery({
    queryKey: ["bridge-status"],
    queryFn: () => unwrapStr(commands.bridgeStatus()),
    retry: false,
  });

  // Machine-wide registration is opt-in from Settings; with it on, the
  // repository card offers the choice, and "global" lifts the gate below.
  const globalAllowed = useSyncExternalStore(subscribeAiScope, globalAllowedSnapshot);
  const scopeChoice = useSyncExternalStore(subscribeAiScope, scopeSnapshot);
  const global = globalAllowed && scopeChoice === "global";
  // What every call below is told: the repository, or null for the whole
  // machine (detection reads the global configs on null; registration is
  // ALSO told `global` explicitly, so null alone can never mean "global").
  const target = global ? null : workingDir || null;

  const tools = useQuery({
    queryKey: ["ai-tools", global ? "global" : workingDir],
    queryFn: () => commands.detectAiTools(target),
    enabled: global || Boolean(workingDir),
  });

  // The disabled set travels with the registration: it writes this
  // repository's command files, and an empty set would hand back the
  // commands for tools switched off on this very tab.
  const register = useMutation({
    mutationFn: (id: string) =>
      unwrapStr(commands.registerAiTool(id, target, loadDisabledTools(), global)),
    onSuccess: () => {
      toast.success("Registered.");
      qc.invalidateQueries({ queryKey: ["ai-tools"] });
    },
    onError: (e) => toast.error(`Could not register: ${e.message}`),
  });

  // Our own leftovers in a tool's machine-wide config, once the repository
  // carries its own. Surfaced rather than removed silently: it is a
  // registration the user (or an older version of this app) made.
  const retireGlobal = useMutation({
    mutationFn: (id: string) => unwrapStr(commands.retireGlobalRegistrations(id)),
    onSuccess: () => {
      toast.success("Global copies retired.");
      qc.invalidateQueries({ queryKey: ["ai-tools"] });
    },
    onError: (e) => toast.error(`Could not retire the global copies: ${e.message}`),
  });

  const unregister = useMutation({
    mutationFn: (id: string) => unwrapStr(commands.unregisterAiTool(id, target, global)),
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

  // Shipped defaults fill a form NOTHING was ever saved into - a machine
  // that configured (or deliberately cleared) its own values never has
  // them overwritten. Prefill only: nothing persists or registers until
  // the person edits or clicks Register themselves.
  const dbDefaults = useQuery({
    queryKey: ["db-defaults"],
    queryFn: () => commands.dbServerDefaults(),
    staleTime: Infinity,
  });
  const dbPresets = useQuery({
    queryKey: ["db-presets"],
    queryFn: () => commands.dbServerPresets(),
    staleTime: Infinity,
  });
  useEffect(() => {
    const d = dbDefaults.data;
    if (!d || !d.connection_string.trim()) return; // no defaults shipped
    if (hasStoredDbConfig()) return;
    // Only replace a still-pristine form, in case typing raced the IPC.
    setDb((cur) => (JSON.stringify(cur) === JSON.stringify(loadDbConfig()) ? { ...d } : cur));
  }, [dbDefaults.data]);

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

  // A warning back means the registration worked but the connection string
  // is somewhere git can carry it away (the file is already tracked, or the
  // folder is not a checkout). That is not a success sentence - it is the
  // one thing on this tab worth reading, so it replaces the toast and stays
  // up long enough to act on.
  const registerDb = useMutation({
    mutationFn: (id: string) => unwrapStr(commands.registerDbServer(id, db, target, global)),
    onSuccess: (warning) => {
      if (warning) toast.warning(warning, { duration: 12000 });
      else toast.success("Database server registered.");
      qc.invalidateQueries({ queryKey: ["ai-tools"] });
    },
    onError: (e) => toast.error(`Could not register: ${e.message}`),
  });

  const unregisterDb = useMutation({
    mutationFn: (id: string) => unwrapStr(commands.unregisterDbServer(id, target, global)),
    onSuccess: () => {
      toast.success("Database server unregistered.");
      qc.invalidateQueries({ queryKey: ["ai-tools"] });
    },
    onError: (e) => toast.error(`Could not unregister: ${e.message}`),
  });

  const pickExe = () => {
    open({
      multiple: false,
      // .exe first as the common case, but any file is selectable - the
      // server may be an extension-less binary, a script, or a shim, and
      // the Rust side only requires that the picked path exists.
      filters: [
        { name: "Server executable", extensions: ["exe"] },
        { name: "All files", extensions: ["*"] },
      ],
    })
      .then((path) => {
        if (typeof path === "string") editDb({ exe_path: path });
      })
      .catch(() => toast.error("Could not open the file picker."));
  };

  // A native dialog picks EITHER files or folders, never both - so the
  // folder case gets its own button. Some server layouts are addressed by
  // their directory rather than a specific file.
  const pickFolder = () => {
    open({ multiple: false, directory: true })
      .then((path) => {
        if (typeof path === "string") editDb({ exe_path: path });
      })
      .catch(() => toast.error("Could not open the folder picker."));
  };

  const exe = bridge.data?.mcp_exe ?? "";
  const installed = (tools.data ?? []).filter((t) => t.installed);
  const dbReady = isDbConfigComplete(db);

  const repoCard = (
    <section className="space-y-3 rounded-md border border-border bg-surface p-4">
      <div className="flex items-center gap-2">
        <FolderOpen size={14} className="shrink-0 text-muted" />
        <h2 className="text-sm font-semibold text-text">Working repository</h2>
      </div>
      {workingDir ? (
        <p className="id-mono break-all text-xs text-text">{workingDir}</p>
      ) : (
        <p className="text-xs text-muted">
          Not set. Pick the repository these test cases belong to: its{" "}
          <span className="id-mono">.test-cases</span> folder is where written and imported
          files go, and the AI tools below register into it rather than machine-wide.
        </p>
      )}
      <Button size="sm" variant="outline" onClick={pickWorkingDir}>
        <FolderOpen aria-hidden />
        {workingDir ? "Change" : "Pick repository"}
      </Button>
      {/* Only with the Settings switch on: machine-wide is the
          pre-per-repo behaviour, kept as an explicit choice for a machine
          that does not work from a repository. Writing test cases still
          needs a repository - this decides where the TOOLS register. */}
      {globalAllowed && (
        <div className="flex flex-wrap items-center gap-2 text-xs text-muted">
          <span>Register in:</span>
          <div className="flex rounded-md border border-border p-0.5">
            {(["project", "global"] as const).map((s) => (
              <button
                key={s}
                type="button"
                aria-pressed={scopeChoice === s}
                className={cn(
                  "rounded px-2 py-1 text-xs transition-colors",
                  scopeChoice === s ? "bg-accent-soft text-accent" : "text-muted hover:text-text",
                )}
                onClick={() => saveScope(s)}
              >
                {s === "project" ? "This repository" : "Machine-wide"}
              </button>
            ))}
          </div>
        </div>
      )}
    </section>
  );

  // Nothing else on this tab means anything until there is a repository -
  // registration would land in a global config, and a writing job would
  // have nowhere agreed to put its file. The rest of the app is unaffected.
  // The one way past it is the explicit machine-wide choice above.
  if (!workingDir && !global) {
    return <div className="max-w-lg">{repoCard}</div>;
  }

  return (
    // Two columns once there is room (the window floor is 900px, so
    // this only kicks in above it); a single column below, which is
    // also what the narrow runner-sized windows get. At 2xl the right
    // stack dissolves (display:contents) and How-it-works becomes its
    // own third column instead of leaving the window's right third empty.
    <div className="grid max-w-lg gap-6 lg:max-w-6xl lg:grid-cols-2 lg:items-start 2xl:max-w-none 2xl:grid-cols-3">
      <div className="space-y-6">
      {repoCard}
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
              <li key={t.id} className="text-sm">
                <div className="flex items-center justify-between gap-2">
                  <span className="text-text">{t.name}</span>
                  <span className="flex-1 text-xs text-faint">
                    {t.scope === "project" ? "in this repo" : "global"}
                  </span>
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
                </div>
                {/* A machine-wide copy of our servers, left from before this
                    repository was registered (or from another one). Most
                    clients let a user-scope server shadow the project one,
                    so it is worth saying - and worth being able to remove
                    from here, since nothing else in the app reaches it. */}
                {(t.global_registered_servers ?? []).length > 0 && (
                  <div className="mt-1 flex items-center gap-2">
                    <span className="flex-1 text-xs text-faint">also registered globally</span>
                    <Button
                      size="sm"
                      variant="ghost"
                      disabled={retireGlobal.isPending && retireGlobal.variables === t.id}
                      onClick={() => retireGlobal.mutate(t.id)}
                    >
                      <IconUnregister aria-hidden />
                      {retireGlobal.isPending && retireGlobal.variables === t.id
                        ? "Retiring"
                        : "Retire global copies"}
                    </Button>
                  </div>
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
              <p className="mb-1 text-faint">Command-line registration (run inside the repository):</p>
              <div className="flex items-center gap-2">
                <code className="id-mono flex-1 truncate rounded bg-surface-2 px-2 py-1 text-xs text-text">
                  claude mcp add --scope project tcm-testcases -- "{exe}" --mcp
                </code>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() =>
                    copy(`claude mcp add --scope project tcm-testcases -- "${exe}" --mcp`, "Command")
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
          far past the other. `grid gap-6`, not space-y: at 2xl this
          wrapper turns into display:contents so the two sections place
          as grid columns 2 and 3 - and space-y's child margins would
          leak through contents into the outer grid, where gap does not. */}
      <div className="grid gap-6 2xl:contents">
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
              Server path (file or folder)
              <Input
                aria-label="Database server path"
                className="mt-1 w-full py-1.5 text-xs"
                placeholder="…\PeoplesHR.DBMCPServer.exe or its folder"
                value={db.exe_path}
                onChange={(e) => editDb({ exe_path: e.target.value })}
              />
            </label>
            <Button size="sm" variant="outline" onClick={pickExe}>
              <IconBrowse aria-hidden />
              File
            </Button>
            <Button size="sm" variant="outline" onClick={pickFolder}>
              <IconBrowse aria-hidden />
              Folder
            </Button>
          </div>

          <label className="block text-xs text-muted">
            DB_TYPE
            <Select
              aria-label="Database type"
              className="mt-1 w-full"
              triggerClassName="py-1.5 text-xs"
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
          {/* Shipped environments: picking one fills the connection (and
              the schema/type defaults) - an explicit act, so it persists
              like any edit. The trigger shows which preset the current
              string IS, or stays blank for a hand-rolled one. */}
          {(dbPresets.data?.length ?? 0) > 0 && (
            <label className="block text-xs text-muted">
              Default connections
              <Combobox
                ariaLabel="Default connections"
                className="mt-1 w-full"
                placeholder="Pick an environment…"
                value={
                  dbPresets.data!.find((p) => p.connection_string === db.connection_string)
                    ?.label ?? ""
                }
                options={dbPresets.data!.map((p) => p.label)}
                onChange={(label) => {
                  const preset = dbPresets.data!.find((p) => p.label === label);
                  if (preset) {
                    editDb({
                      connection_string: preset.connection_string,
                      db_type: "mssql",
                      schema_filter: "PeoplesHR",
                    });
                    setRawConn(!isRepresentable(preset.connection_string));
                  }
                }}
              />
            </label>
          )}

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
                {/* The same label as the list above: which config this row
                    is about is exactly what a person needs to know before
                    putting a connection string into it. */}
                <span className="flex-1 text-xs text-faint">
                  {t.scope === "project" ? "in this repo" : "global"}
                </span>
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
          {/* No count in the sentence: it went stale twice - the list
              below is the inventory. */}
          Connected AI tools can call the tools this app exposes. All of them
          read, reshape the AI's own draft, or save local files — none can
          write to Azure DevOps:
        </p>
        <ul className="space-y-1.5 text-xs text-muted">
          <li>
            <code className="id-mono text-text">begin_test_case_writing</code> — the
            starting point. It hands the assistant a checklist to put to you in chat:
            what the file is called (it goes in this repository's{" "}
            <span className="id-mono">.test-cases</span> folder), which spec documents
            are authoritative, whether to check a PBI for duplicates, tags and module,
            what is out of scope. Your answers are checked against the real paths and
            values, and written to a plan file for you to approve before a single case
            exists.
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
            <code className="id-mono text-text">check_spec_coverage</code> — reports
            which parts of a specification have no test case yet, by joining the
            draft's own citations against the spec documents. Gaps come back as
            findings to account for — a partial draft is a normal state, not an
            error.
          </li>
          <li>
            <code className="id-mono text-text">merge_case_files</code> — merges the
            slice files of a fanned-out draft into one file through the real
            importer, with each slice's warnings labelled by the file they came
            from.
          </li>
          <li>
            <code className="id-mono text-text">get_autorun_guide</code> — how to
            write an Auto Run browser script: the actions the runner understands
            and where assertions are allowed to come from.
          </li>
          <li>
            <code className="id-mono text-text">save_autorun_script</code> — saves
            browser scripts for a PBI's cases so Auto Run can drive them; one call
            covers the whole set, saved locally, all-or-nothing.
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
          Tool is unable to create, update or delete in Azure DevOps, only
          read data.
        </p>
      </section>
      </div>
    </div>
  );
}
