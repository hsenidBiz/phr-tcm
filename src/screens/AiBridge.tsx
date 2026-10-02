import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { open } from "@tauri-apps/plugin-dialog";
import { Database, FolderOpen, Globe } from "lucide-react";
import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import Combobox from "../components/ui/combobox";
import { toast } from "../lib/toast";
import { commands, type AppSettings, type AutoApproveOutcome, type DbDatabase } from "../bindings";
import { copyText } from "../lib/clipboard";
import { DbCredentialsModal } from "../components/DbCredentialsModal";
import EnvironmentsDialog, { RECIPE_ADDRESS } from "../components/EnvironmentsDialog";
import { Button } from "../components/ui/button";
import { Switch } from "../components/ui/switch";
import { cn } from "../lib/cn";
import {
  forgetDbConfig,
  forgetRemovedDb,
  isDevLoginUser,
  loadDbWrites,
  saveDbWrites,
  saveSelectedDb,
  selectedDbSnapshot,
  subscribeDbSettings,
} from "../lib/dbServer";
import { loadApiWrites, saveApiWrites } from "../lib/apiTemplates";
import {
  activeDbMissing,
  envKeys,
  forgetEnvironmentData,
  switchEnvironment,
  toInput,
  useEnvironments,
} from "../lib/environments";
import { loadRiskTiered, saveRiskTiered } from "../lib/riskTieredGuide";
import { autoRunToolsShown, loadDisabledTools, saveDisabledTools, toggleRow, visibleRows } from "../lib/mcpTools";
import { unwrapStr } from "../lib/ipc";
import { logUi } from "../lib/uiLog";
import {
  addRepository,
  currentPathSnapshot,
  removeRepository,
  repositoriesSnapshot,
  samePath,
  setCurrentRepository,
  setRepositoryEnabled,
  subscribeWorkingDir,
  workingDirSnapshot,
} from "../lib/workingDir";
import {
  globalAllowedSnapshot,
  legacyCleanupKey,
  legacyDbCleaned,
  markLegacyDbCleaned,
  saveScope,
  scopeSnapshot,
  subscribeAiScope,
} from "../lib/aiScope";
import {
  IconAdd,
  IconCancel,
  IconConfirm,
  IconCopy,
  IconEdit,
  IconRefresh,
  IconRegister,
  IconRemove,
  IconUnregister,
} from "../lib/actionIcons";

/** Config keys, mirroring `ai_tools.rs`. The second is a server earlier
 * versions could register beside ours; it is only ever removed now. */
const TCM_SERVER = "tcm-testcases";
const LEGACY_DB_SERVER = "phr-db-mcp";

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

  // The repository everything on this tab is scoped to: the CURRENT one of
  // the saved list, while its AI-tools switch is on. Read through the
  // store so App's bridge push and this tab agree the moment it changes.
  const workingDir = useSyncExternalStore(subscribeWorkingDir, workingDirSnapshot);
  const repos = useSyncExternalStore(subscribeWorkingDir, repositoriesSnapshot);
  const currentPath = useSyncExternalStore(subscribeWorkingDir, currentPathSnapshot);
  const addRepo = () => {
    open({ multiple: false, directory: true })
      .then((path) => {
        if (typeof path === "string") {
          addRepository(path);
          toast.success(`Working repository: ${path}`);
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
  const visible = visibleRows();
  // Which database the app's own tools use. Read through the store so App's
  // bridge push and this card agree the moment it changes.
  const dbId = useSyncExternalStore(subscribeDbSettings, selectedDbSnapshot);
  // Who each database signs in as and whether a password is saved - never
  // the password. Not in the disk cache: a list of logins has no business
  // outliving the session in storage.
  const databases = useQuery({
    queryKey: ["db-databases"],
    queryFn: async () => (await commands.dbDatabases()) ?? [],
  });
  const selectedDb = databases.data?.find((d) => d.id === dbId) ?? null;
  // Every database is listed under the picker - the one place a login is
  // edited and one of your own is removed - so neither needs the database
  // chosen first: choosing one also makes it the active environment's, and
  // an environment that uses a database is exactly what refuses its removal.
  const allDbs = databases.data ?? [];
  const [addingDb, setAddingDb] = useState(false);
  const [editingDbId, setEditingDbId] = useState<string | null>(null);
  const editingDb = allDbs.find((d) => d.id === editingDbId) ?? null;
  const [removingDb, setRemovingDb] = useState<string | null>(null);
  const [removeBusy, setRemoveBusy] = useState(false);
  const [removeProblem, setRemoveProblem] = useState<{ id: string; text: string } | null>(null);
  const removeDb = async (d: DbDatabase) => {
    setRemoveBusy(true);
    setRemoveProblem(null);
    try {
      const res = await commands.dbRemoveCustom(d.id);
      setRemovingDb(null);
      if (res.status === "error") {
        // In use by an environment, most likely: the sentence names it.
        setRemoveProblem({ id: d.id, text: res.error });
        return;
      }
      forgetRemovedDb(d.id);
      toast.success(`Removed ${d.label}.`);
      await qc.invalidateQueries({ queryKey: ["db-databases"] });
    } catch (e) {
      setRemoveProblem({ id: d.id, text: e instanceof Error ? e.message : String(e) });
    } finally {
      setRemoveBusy(false);
    }
  };
  // The environments: which one is active, its address, and the database
  // it uses - the card above Company database mirrors the active one.
  const envs = useEnvironments();
  const activeEnv = envs.data?.environments.find((e) => e.id === envs.data?.active) ?? null;
  const dbGone = activeDbMissing(
    envs.data,
    databases.data?.map((d) => d.id),
  );
  const [managingEnvs, setManagingEnvs] = useState(false);
  const [switching, setSwitching] = useState(false);
  const chooseEnv = async (id: string) => {
    if (id === envs.data?.active) return;
    setSwitching(true);
    try {
      const { view } = await switchEnvironment(id);
      qc.setQueryData(envKeys.list, view);
      // Nothing read for the environment before may be shown - or saved
      // back by the Accounts dialog - in this one.
      void forgetEnvironmentData(qc);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    } finally {
      setSwitching(false);
    }
  };
  // Whether the assistant may create, update and delete. Half the
  // permission: the Rust side also requires the database's own user to be
  // the dev login, and refuses the write when either is missing.
  const [dbWrites, setWrites] = useState<boolean>(loadDbWrites);
  const setDbWrites = (on: boolean) => {
    setWrites(on);
    saveDbWrites(on);
  };
  const devLogin = selectedDb ? isDevLoginUser(selectedDb.user) : false;
  // Whether the assistant may prove and run API templates - a separate
  // decision from whether the four tools are reachable at all (the "API
  // templates" row above), the same shape as the database's own
  // create/update/delete switch. Offered only where Auto Run is.
  const [apiWrites, setApiWritesState] = useState<boolean>(loadApiWrites);
  // Which rules the writing guide carries: the plain ones, or the
  // risk-tiered trial. Every assistant reads it the next time it asks.
  const [riskTiered, setRiskTieredState] = useState<boolean>(loadRiskTiered);
  const setRiskTiered = (on: boolean) => {
    setRiskTieredState(on);
    saveRiskTiered(on);
  };
  const setApiWrites = (on: boolean) => {
    setApiWritesState(on);
    saveApiWrites(on);
  };
  // "Run database changes without asking": kept by Rust (it writes each
  // registered tool's own "always allow" for db_query, and re-applies it
  // to a tool registered later), so it is read from the app settings. It
  // means something only while writes can happen at all.
  const appSettings = useQuery({ queryKey: ["app-settings"], queryFn: () => commands.getAppSettings() });
  const noAsk = Boolean(appSettings.data?.db_auto_approve);
  const writesLive = dbWrites && devLogin;
  const [noAskNotes, setNoAskNotes] = useState<AutoApproveOutcome[]>([]);
  const setNoAsk = async (on: boolean) => {
    const before = appSettings.data;
    if (before) qc.setQueryData<AppSettings>(["app-settings"], { ...before, db_auto_approve: on });
    try {
      const r = await commands.setDbAutoApprove(on, global ? null : workingDir || null);
      if (r.status === "error") throw r.error;
      setNoAskNotes(on ? r.data.filter((o) => o.note) : []);
      qc.invalidateQueries({ queryKey: ["app-settings"] });
      logUi(`db auto-approve ${on ? "on" : "off"}`);
    } catch (e) {
      if (before) qc.setQueryData(["app-settings"], before);
      toast.error(String(e));
    }
  };
  const exe = bridge.data?.mcp_exe ?? "";
  const installed = (tools.data ?? []).filter((t) => t.installed);

  // Earlier versions could register a separate database server beside ours;
  // the app's own database tools replaced it. A tool that still carries that
  // entry has it removed here, quietly: it is a registration this app made,
  // and nothing on the tab mentions it. Once per tool and config, ever: a
  // removal that worked is remembered (legacyDbCleaned), so an entry
  // someone adds back by hand afterwards is theirs and is left alone. A
  // failure is logged and waits for the next scan, a call already on its
  // way is not made twice, and a removal never triggers a rescan of its
  // own, so nothing here can loop.
  const legacyScanned = useRef(0);
  const legacyInFlight = useRef(new Set<string>());
  useEffect(() => {
    if (!tools.data || legacyScanned.current === tools.dataUpdatedAt) return;
    legacyScanned.current = tools.dataUpdatedAt;
    const removeLegacy = (id: string, where: string, workingDir: string | null, machineWide: boolean) => {
      const key = legacyCleanupKey(id, machineWide ? null : workingDir);
      if (legacyDbCleaned(key) || legacyInFlight.current.has(key)) return;
      legacyInFlight.current.add(key);
      const failed = (why: string) =>
        logUi(`AI tools: could not remove the old database server from ${id} (${where} config): ${why}`);
      commands
        .removeLegacyDbServer(id, workingDir, machineWide)
        .then((res) => {
          if (res.status === "error") return failed(res.error);
          markLegacyDbCleaned(key);
          logUi(`AI tools: removed the old database server from ${id} (${where} config)`);
        })
        .catch((e: unknown) => failed(String(e)))
        .finally(() => legacyInFlight.current.delete(key));
    };
    for (const t of tools.data) {
      if (!t.installed) continue;
      if ((t.registered_servers ?? []).includes(LEGACY_DB_SERVER)) {
        removeLegacy(t.id, t.scope, target, global);
      }
      // A copy left in the machine-wide config while this row reads the
      // repository's - the same leftover, one config over.
      if ((t.global_registered_servers ?? []).includes(LEGACY_DB_SERVER)) {
        removeLegacy(t.id, "global", null, true);
      }
    }
  }, [tools.data, tools.dataUpdatedAt, target, global]);

  // One setting, not two: the database is the active environment's, so a
  // new choice is saved into it too.
  // If the environment refuses the save, the card goes back to what it
  // showed: it must not name a database the active environment does not.
  const chooseDb = (id: string) => {
    const previous = dbId;
    saveSelectedDb(id);
    if (!activeEnv || !id || activeEnv.db_id === id) return;
    const rollBack = (why: string) => {
      saveSelectedDb(previous);
      toast.error(why);
    };
    commands
      .envSave({ ...toInput(activeEnv), db_id: id })
      .then((res) => {
        if (res.status === "error") rollBack(res.error);
        else qc.setQueryData(envKeys.list, res.data);
      })
      .catch((e: unknown) => rollBack(e instanceof Error ? e.message : String(e)));
  };
  // Every saved login goes with the local settings, and permission to
  // write with them: leaving it standing would hand the next database a
  // decision nobody made about it.
  const forgetDb = async () => {
    forgetDbConfig();
    setDbWrites(false);
    let failed: string | null = null;
    try {
      const res = await commands.forgetDbCredentials();
      if (res.status === "error") failed = res.error;
    } catch (e) {
      failed = e instanceof Error ? e.message : String(e);
    }
    qc.invalidateQueries({ queryKey: ["db-databases"] });
    if (failed) toast.error(failed);
    else toast.success("Database settings forgotten.");
  };
  const repoCard = (
    <section data-tour="ai-repos" className="space-y-3 rounded-md border border-border bg-surface p-4">
      <div className="flex items-center gap-2">
        <FolderOpen size={14} className="shrink-0 text-muted" />
        <h2 className="text-sm font-semibold text-text">Working repositories</h2>
      </div>
      {repos.length === 0 ? (
        <p className="text-xs text-muted">
          None yet. Add the repository these test cases belong to: its{" "}
          <span className="id-mono">.test-cases</span> folder is where written and imported
          files go, and the AI tools below register into it rather than machine-wide.
        </p>
      ) : (
        <ul className="space-y-1">
          {/* One row per saved repository: the dot picks which one is
              CURRENT (files and registration go there), the switch is that
              repository's AI tooling. The current one switched off is how
              the tooling is turned off without forgetting the folder. */}
          {repos.map((r) => {
            const isCurrent = samePath(r.path, currentPath);
            return (
              <li key={r.path} className="flex items-center gap-2 text-xs">
                <button
                  type="button"
                  role="radio"
                  aria-checked={isCurrent}
                  aria-label={`Use ${r.path}`}
                  title="Make this the current repository"
                  className={cn(
                    "h-3 w-3 shrink-0 rounded-full border transition-colors",
                    isCurrent ? "border-accent bg-accent" : "border-border-strong hover:border-accent",
                  )}
                  onClick={() => setCurrentRepository(r.path)}
                />
                <span
                  className={cn(
                    "id-mono min-w-0 flex-1 break-all",
                    r.enabled ? "text-text" : "text-faint line-through",
                  )}
                >
                  {r.path}
                </span>
                <Switch
                  checked={r.enabled}
                  onCheckedChange={(on) => setRepositoryEnabled(r.path, on)}
                  ariaLabel={`AI tools for ${r.path}`}
                />
                <button
                  type="button"
                  aria-label={`Remove ${r.path}`}
                  title="Remove from the list"
                  className="rounded p-1 text-muted transition-colors hover:text-danger"
                  onClick={() => removeRepository(r.path)}
                >
                  <IconUnregister aria-hidden />
                </button>
              </li>
            );
          })}
        </ul>
      )}
      <div className="flex flex-wrap items-center gap-2">
        <Button size="sm" variant="outline" onClick={addRepo}>
          <FolderOpen aria-hidden />
          Add repository
        </Button>
      </div>
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
      {/* Whether the bridge is up is the glowing badge beside the tab title
          (BridgeStatusBadge, in App) - the card that said it in a sentence
          has gone. */}

      <section data-tour="ai-tools" className="space-y-3 rounded-md border border-border bg-surface p-4">
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
                {/* A machine-wide copy of our server, left from before this
                    repository was registered (or from another one). Most
                    clients let a user-scope server shadow the project one,
                    so it is worth saying - and worth being able to remove
                    from here, since nothing else in the app reaches it.
                    The old database server's copy is not ours to show: it
                    is removed quietly above. */}
                {(t.global_registered_servers ?? []).includes(TCM_SERVER) && (
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
                  aria-label="Copy command"
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
                  aria-label="Copy config"
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

      <section data-tour="ai-toolset" className="space-y-3 rounded-md border border-border bg-surface p-4">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <h2 className="text-sm font-semibold text-text">Tools an assistant may use</h2>
          <span className="text-xs text-faint">
            {/* Counted over ROWS, not names: the wiki row carries two
                tools, so subtracting the disabled names would report one
                switch off as two. */}
            {visible.filter((r) => !r.names.some((n) => disabled.includes(n))).length} of{" "}
            {visible.length} on
          </span>
        </div>
        <p className="text-xs text-muted">
          Switch a tool off to keep it out of an assistant's reach.
        </p>
        <ul className="space-y-1.5">
          {visible.map((row) => {
            const on = !row.names.some((n) => disabled.includes(n));
            return (
              <li key={row.key} className="flex items-start gap-2">
                <Switch
                  checked={on}
                  ariaLabel={row.label}
                  onCheckedChange={() => {
                    const next = toggleRow(disabled, row.names);
                    setDisabled(next);
                    saveDisabledTools(next);
                  }}
                  className="mt-0.5"
                />
                <span className="min-w-0 flex-1">
                  <span className={cn("text-sm font-medium", on ? "text-text" : "text-faint")}>
                    {row.label}
                  </span>
                  <span className="block text-[11px] text-muted">{row.summary}</span>
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
          <Globe size={14} className="shrink-0 text-muted" />
          <h2 className="text-sm font-semibold text-text">Environment</h2>
        </div>
        <p className="text-xs text-muted">
          Which website and database the app&apos;s tools work against. Each environment keeps its
          own accounts and saved sign-ins; switching moves the database below with it.
        </p>
        <div className="space-y-2">
          <label className="block text-xs text-muted">
            Environment
            <Combobox
              ariaLabel="Environment"
              className="mt-1 w-full"
              placeholder="Pick an environment…"
              value={envs.data?.active ?? ""}
              items={(envs.data?.environments ?? []).map((e) => ({ value: e.id, label: e.name }))}
              loading={envs.isPending || switching}
              onChange={chooseEnv}
            />
          </label>
          <div className="flex items-center justify-between gap-2">
            <span className="min-w-0 truncate text-xs text-muted">
              {activeEnv && (activeEnv.start_url || RECIPE_ADDRESS)}
            </span>
            <Button size="sm" variant="outline" onClick={() => setManagingEnvs(true)}>
              <IconEdit aria-hidden />
              Edit environments
            </Button>
          </div>
          {dbGone && (
            <p role="status" className="text-xs text-warning">
              the database this environment uses is not set up any more - pick one
            </p>
          )}
          {managingEnvs && <EnvironmentsDialog onClose={() => setManagingEnvs(false)} />}
        </div>
      </section>

      <section data-tour="ai-db" className="space-y-3 rounded-md border border-border bg-surface p-4">
        <div className="flex items-center gap-2">
          <Database size={14} className="shrink-0 text-muted" />
          <h2 className="text-sm font-semibold text-text">Company database</h2>
        </div>
        <p className="text-xs text-muted">
          The database you choose here is the one this app&apos;s own database tools
          use. Switch them on with{" "}
          <span className="font-medium text-text">Company database (read)</span> in the
          tool list, and an assistant can find the table behind a screen and read it
          while it writes cases.
        </p>

        <div className="space-y-2">
          {/* Which database, by id. Its login is Rust's, kept in Windows
              Credential Manager: the card says who signs in, and never
              holds the password or a connection string. */}
          <label className="block text-xs text-muted">
            Database
            <Combobox
              ariaLabel="Database"
              className="mt-1 w-full"
              placeholder="Pick a database…"
              value={dbId}
              items={(databases.data ?? []).map((d) => ({ value: d.id, label: d.label }))}
              loading={databases.isPending}
              onChange={chooseDb}
            />
          </label>
          <p className="min-h-4 truncate text-xs text-muted">
            {selectedDb && (selectedDb.user ? `Signs in as ${selectedDb.user}` : "No login saved")}
          </p>

          <div className="space-y-1">
            <div className="flex items-center justify-between gap-2">
              <span className="text-xs font-medium text-muted">Databases</span>
              <Button size="sm" variant="outline" onClick={() => setAddingDb(true)}>
                <IconAdd aria-hidden />
                Add database
              </Button>
            </div>
            {allDbs.length > 0 && (
              <ul className="space-y-1">
                {allDbs.map((d) => (
                  <li key={d.id} className="space-y-1 rounded-md border border-border/60 p-2">
                    <div className="flex items-center gap-2">
                      <div className="min-w-0 flex-1">
                        <p className="truncate text-xs font-medium text-text">{d.label}</p>
                        <p className="truncate text-[11px] text-faint">
                          {d.server ? `${d.database} on ${d.server}` : "Not set up yet"}
                        </p>
                      </div>
                      <Button
                        size="sm"
                        variant="outline"
                        aria-label={`Edit ${d.label}`}
                        onClick={() => setEditingDbId(d.id)}
                      >
                        <IconEdit aria-hidden />
                        Edit
                      </Button>
                      {/* A shipped database is the app's, so only its login
                          is the person's to change - it cannot be removed. */}
                      {!d.shipped && (
                        <Button
                          size="sm"
                          variant="ghost"
                          aria-label={`Remove ${d.label}`}
                          onClick={() => {
                            setRemoveProblem(null);
                            setRemovingDb(d.id);
                          }}
                        >
                          <IconRemove aria-hidden />
                        </Button>
                      )}
                    </div>
                    {removingDb === d.id && (
                      <div className="flex flex-wrap items-center gap-2 border-t border-border/60 pt-2">
                        <span className="min-w-0 flex-1 text-xs text-text">
                          Remove {d.label}? Its saved login is deleted from this machine.
                        </span>
                        <Button size="sm" variant="ghost" onClick={() => setRemovingDb(null)}>
                          <IconCancel aria-hidden />
                          Keep
                        </Button>
                        <Button size="sm" variant="outline" disabled={removeBusy} onClick={() => void removeDb(d)}>
                          <IconRemove aria-hidden />
                          Confirm remove
                        </Button>
                      </div>
                    )}
                    {removeProblem?.id === d.id && (
                      <p role="status" className="break-words text-xs text-danger">
                        {removeProblem.text}
                      </p>
                    )}
                  </li>
                ))}
              </ul>
            )}
          </div>
          {addingDb && (
            <DbCredentialsModal
              database={null}
              onClose={() => setAddingDb(false)}
              onSaved={() => qc.invalidateQueries({ queryKey: ["db-databases"] })}
            />
          )}
          {editingDb && (
            <DbCredentialsModal
              database={editingDb}
              onClose={() => setEditingDbId(null)}
              onSaved={() => qc.invalidateQueries({ queryKey: ["db-databases"] })}
            />
          )}

          {/* The second switch. Reading is the "Company database (read)"
              row in the tool list; writing is its own decision and lives
              here, beside the database it applies to, because that is
              what decides whether it may be made at all.

              Shown OFF on a database that cannot write, whatever is
              stored: the app refuses such a write anyway (both doors -
              this switch AND the database's user), and a switch reading
              "on" while every write comes back refused is a lie. The
              stored choice is kept, so going back to the dev login
              restores it. */}
          <div className="space-y-1 rounded-md border border-border/60 p-2">
            <div className="flex items-center justify-between gap-2">
              <span className="text-xs font-medium text-muted">
                Create, update and delete
              </span>
              <Switch
                ariaLabel="Create, update and delete"
                checked={dbWrites && devLogin}
                disabled={!devLogin}
                onCheckedChange={setDbWrites}
              />
            </div>
            <p className="text-[11px] text-faint">
              Only on a dev login database (a user ending in _devlogin), and every
              statement is written to the log.
            </p>
            {/* The third: whether the assistant asks first. The app sets
                each registered tool's own "always allow" for db_query and
                tells the assistant it need not ask; a tool that keeps that
                setting only in its own window is named below. */}
            <div className="flex items-center justify-between gap-2 pt-1">
              <span className="text-xs font-medium text-muted">Run changes without asking</span>
              <Switch
                ariaLabel="Run database changes without asking"
                checked={noAsk && writesLive}
                disabled={!writesLive || appSettings.isLoading}
                onCheckedChange={(on) => void setNoAsk(on)}
              />
            </div>
            <p className="text-[11px] text-faint">
              Your AI tools run database changes without stopping to ask you first. They
              still try each batch as a dry run before saving it.
            </p>
            {noAskNotes.length > 0 && (
              <ul className="space-y-0.5 text-[11px] text-warning">
                {noAskNotes.map((o) => (
                  <li key={o.tool}>{o.note}</li>
                ))}
              </ul>
            )}
          </div>
        </div>

        <p className="text-[11px] text-faint">
          Logins are kept in Windows Credential Manager and the other settings on this
          machine.{" "}
          <button
            className="underline underline-offset-2 hover:text-danger"
            onClick={() => void forgetDb()}
          >
            Forget them
          </button>
          .
        </p>
      </section>

      {/* Which rules the writing guide carries. A trial of the team's
          risk-tiering policy: off, the assistant writes cases exactly as
          before. Shown only where Auto Run is, like the API templates card
          below: capture mode and a locked release build hide it. */}
      {autoRunToolsShown() && (
      <section className="space-y-3 rounded-md border border-border bg-surface p-4">
        <h2 className="text-sm font-semibold text-text">Test design rules</h2>
        <div className="space-y-1 rounded-md border border-border/60 p-2">
          <div className="flex items-center justify-between gap-2">
            <span className="text-xs font-medium text-muted">Risk-tiered test design (trial)</span>
            <Switch
              ariaLabel="Risk-tiered test design (trial)"
              checked={riskTiered}
              onCheckedChange={setRiskTiered}
            />
          </div>
          <p className="text-[11px] text-faint">
            Off: the assistant writes cases with the standard guide. On: it tiers each
            scenario by risk (T1 critical, T2 core, T3 low), lists the scenarios for your
            approval before writing, keeps to a budget per story, and tags every case with
            its trace, tier and run category (Smoke, Regression or Extended). Takes effect
            the next time the assistant reads the writing guide.
          </p>
        </div>
      </section>
      )}

      {/* Proving and running a template writes test data through the
          application's own endpoints - a decision separate from whether
          the four tools are reachable at all (the "API templates" row
          above), the same shape as the database card's write switch.
          Shown only where Auto Run is: capture mode and a locked release
          build hide it, like every other Auto Run/API template control. */}
      {autoRunToolsShown() && (
        <section data-tour="ai-api-templates" className="space-y-3 rounded-md border border-border bg-surface p-4">
          <h2 className="text-sm font-semibold text-text">API templates</h2>
          <div className="space-y-1 rounded-md border border-border/60 p-2">
            <div className="flex items-center justify-between gap-2">
              <span className="text-xs font-medium text-muted">
                API templates (create, edit and delete)
              </span>
              <Switch
                ariaLabel="API templates (create, edit and delete)"
                checked={apiWrites}
                onCheckedChange={setApiWrites}
              />
            </div>
            <p className="text-[11px] text-faint">
              Off by default. Templates run as your Auto Run accounts, against the
              sign-in recipe&apos;s site.
            </p>
          </div>
        </section>
      )}

      <section className="space-y-3 rounded-md border border-border bg-surface p-4">
        <h2 className="text-sm font-semibold text-text">AI Tools Breakdown</h2>
        <p className="text-sm text-muted">
          {/* No count in the sentence: it went stale twice - the list
              below is the inventory. */}
          Connected AI tools can call the tools this app exposes. All of them
          read, reshape the AI's own draft, or save local files — none can
          write to Azure DevOps. These are the ones you can switch above:
        </p>
        <ul className="space-y-1.5 text-xs text-muted">
          <li>
            <span className="font-medium text-text">Test Suites</span> — two tools under one switch: one finds a test suite by plan name, suite name or
            PBI id, the other reads the cases in it, in the suite&apos;s own order, with an option to
            include every suite beneath a folder. This is how an assistant reads a static or
            query-based suite as a whole, without knowing its PBI or every case id. Reading
            only — the same view as the Search Suites tab. Off, an assistant reaches cases through
            a PBI or their own ids alone.
          </li>
          <li>
            <span className="font-medium text-text">Run results</span> — how this PBI&apos;s cases did the last time they were run: failed, blocked,
            passed or not run yet, with a count of each and the tester&apos;s own comments.
            The fastest way to turn a failed run into the cases that should have caught
            it. Switch it off to keep run results away from an assistant.
          </li>
          <li>
            <span className="font-medium text-text">Project tags</span> — the tag names this project already uses, so an assistant reuses yours instead of
            inventing near-duplicates. Served from this app&apos;s cache, at no request
            cost. Off, it will still tag cases — it just has to guess.
          </li>
          <li>
            <span className="font-medium text-text">Find a PBI</span> — searches this project&apos;s Product Backlog Items by title, so a job can start
            from a name rather than an id you looked up yourself. Typing a number finds
            that item directly. It matches Product Backlog Items only, never bugs or
            tasks. Off, you supply the id.
          </li>
          <li>
            <span className="font-medium text-text">Project wiki</span> — searches your Azure DevOps wiki and reads the pages it finds, for the documentation
            behind a requirement. A page opens from a search result, from its path, or
            from the address in your browser. Searching and reading are one switch
            because reading only works on a page the search found.
          </li>
          <li>
            <span className="font-medium text-text">Company database (read)</span>: two tools under one switch: one finds the tables and columns behind a
            topic, the other runs a single statement on the connection you chose
            under Company database. It is how an assistant checks what a screen
            actually reads, or what a value is today, instead of guessing. SELECT
            only, unless you switch creating, updating and deleting on separately
            beside that connection. Off, the assistant's database tools are
            switched off.
          </li>
          {autoRunToolsShown() && (
            <li>
              <span className="font-medium text-text">Auto Run scripts</span>: everything an assistant needs to write one
              browser script and keep it working. It reads the script guide, looks
              at the page in the browser you opened, tries a locator or a single
              action there, reads what failed in a run, saves the scripts back one
              call for a whole PBI&apos;s cases, and notes what it learned about your
              application. It never signs in, never removes a check you had, and
              a script it has repaired three times comes back to you. Off, Auto Run stays
              something you drive by hand.
            </li>
          )}
          {autoRunToolsShown() && (
            <li>
              <span className="font-medium text-text">API templates</span>: everything an
              assistant needs to build, prove and run a template that writes test data
              through this application&apos;s own endpoints. It reads the template format
              and this project&apos;s account keys and address, lists every saved template
              with its params, outputs and last run, proves a draft by running it end to
              end and saves it only if every step passed, and runs a saved template for
              its outputs. It can also map a module&apos;s wizard as a flow of stages, each
              with a check on the chosen company database, save that flow with a sample
              record, and ask which stages are done for a record before every run, so
              templates go in the order the application allows. Those stage checks read
              the company database, so they need Company database (read) switched on;
              while it is off, flows and the templates that depend on them are refused.
              Proving and running also need the separate API templates switch above, off
              by default. Off, template work stays something you drive by hand.
            </li>
          )}
        </ul>
        <p className="text-sm text-muted">
          Recommended flow: ask the AI to read the writing guide and some
          example cases, have it draft cases for your PBI, have it build the run
          sheet, then import the result yourself via the Import Test Cases tab. Keep
          the file watched and any problems appear here as it saves — the AI
          never has to ask whether the draft is valid.
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
