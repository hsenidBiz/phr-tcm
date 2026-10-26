import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { open } from "@tauri-apps/plugin-dialog";
import { Database, FolderOpen, Globe } from "lucide-react";
import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import Combobox from "../components/ui/combobox";
import { toast } from "../lib/toast";
import { commands, type AutoApproveOutcome, type DbDatabase, type EnvListView } from "../bindings";
import { copyText } from "../lib/clipboard";
import { DbCredentialsModal } from "../components/DbCredentialsModal";
import EnvironmentsDialog, { RECIPE_ADDRESS } from "../components/EnvironmentsDialog";
import { Button } from "../components/ui/button";
import { Switch } from "../components/ui/switch";
import { cn } from "../lib/cn";
import { siteName } from "../lib/siteName";
import {
  dbWritesSnapshot,
  forgetDbConfig,
  forgetDbWritesEverywhere,
  forgetRemovedDb,
  isDevLoginUser,
  saveDbWrites,
  saveSelectedDb,
  selectedDbSnapshot,
  subscribeDbSettings,
} from "../lib/dbServer";
import { apiWritesSnapshot, saveApiWrites, subscribeApiWrites } from "../lib/apiTemplates";
import { networkToolsSnapshot, saveNetworkTools, subscribeNetworkTools } from "../lib/networkTools";
import { isCaptureMode } from "../dev/capture";
import {
  activeDbMissing,
  envKeys,
  forgetEnvironmentData,
  switchEnvironment,
  toInput,
  useEnvironments,
} from "../lib/environments";
import { WritingStyleCard } from "../components/WritingStyleCard";
import { useModule } from "../lib/modules";
import { disabledToolsSnapshot, loadDisabledTools, subscribeDisabledTools, toolShown, saveDisabledTools, toggleRow, visibleRows } from "../lib/mcpTools";
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
  IconAbout,
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

/** Config keys, mirroring `ai_tools.rs`. The second is our own server's
 * name before it was renamed: a config still holding only that needs
 * updating, which registering does. The third is a server earlier versions
 * could register beside ours; it is only ever removed now. */
const TCM_SERVER = "tcm";
const LEGACY_TCM_SERVER = "tcm-testcases";
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
  // Each card follows its module (lib/modules). Environments exist for Auto
  // Run - its site, accounts and sign-ins - so that card needs Auto Run; the
  // API templates switch needs API Templates. Subscribing here also makes
  // the cards and the breakdown below follow a module switched while this
  // screen is open. A module's tools have no row or switch of their own in
  // the tool list: the module being on offers them.
  const dbOn = useModule("db_access");
  const writingOn = useModule("writing_style");
  const autoRunOn = useModule("auto_run");
  const apiTemplatesOn = useModule("api_templates");

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
  // Read off the store, not a one-shot copy: a mount before the modules
  // have loaded corrects itself, and a save re-renders every reader.
  const disabled = useSyncExternalStore(subscribeDisabledTools, disabledToolsSnapshot);
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
    enabled: dbOn,
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
  // it uses - the card above Database Read Access mirrors the active one.
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
      const { view, allow } = await switchEnvironment(id);
      qc.setQueryData(envKeys.list, view);
      // Each AI tool's own "always allow" now follows the new environment.
      // One that could not be updated is named, as the switch below names
      // them: it may still let an assistant run changes without asking.
      const now = view.environments.find((e) => e.id === view.active);
      setNoAskNotes(allow.filter((o) => o.note && (!o.applied || now?.db_auto_approve)));
      const failed = allow.filter((o) => !o.applied && o.note).map((o) => o.tool);
      if (failed.length > 0) {
        toast.error(
          `Could not update ${failed.join(", ")} to match ${now?.name ?? "this environment"} - see Run changes without asking.`,
        );
      }
      // Nothing read for the environment before may be shown - or saved
      // back by the Accounts dialog - in this one.
      void forgetEnvironmentData(qc);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    } finally {
      setSwitching(false);
    }
  };
  // Every switch below that changes what an assistant may do - the tool
  // list, database writes, running them without asking, API template
  // writes - is the ACTIVE environment's own: each environment keeps its
  // own, and a switch of environment shows (and enforces) the other's.
  // Read through the stores, so a switch re-renders them at once.
  //
  // Whether the assistant may create, update and delete. Half the
  // permission: the Rust side also requires the database's own user to be
  // the dev login, and refuses the write when either is missing.
  const dbWrites = useSyncExternalStore(subscribeDbSettings, dbWritesSnapshot);
  const setDbWrites = (on: boolean) => saveDbWrites(on);
  const devLogin = selectedDb ? isDevLoginUser(selectedDb.user) : false;
  // Whether the assistant may prove and run API templates - a separate
  // decision from whether the four tools are reachable at all (the "API
  // templates" row above), the same shape as the database's own
  // create/update/delete switch. Offered only where Auto Run is.
  const apiWrites = useSyncExternalStore(subscribeApiWrites, apiWritesSnapshot);
  const setApiWrites = (on: boolean) => saveApiWrites(on);
  // Whether an assistant gets the network tools at all - the Auto Run
  // browser's captured requests, HAR files, test headers and cookies. Off
  // until switched on, per environment, like the API templates switch.
  const networkTools = useSyncExternalStore(subscribeNetworkTools, networkToolsSnapshot);
  // "Run database changes without asking": kept by Rust on the environment
  // (it writes each registered tool's own "always allow" for db_query while
  // that environment is active, and re-applies it on a switch and to a tool
  // registered later). It means something only while writes can happen at
  // all, and only once there is an environment to keep it on.
  const noAsk = Boolean(activeEnv?.db_auto_approve);
  const writesLive = dbWrites && devLogin;
  const [noAskNotes, setNoAskNotes] = useState<AutoApproveOutcome[]>([]);
  const setNoAsk = async (on: boolean) => {
    if (!activeEnv) return;
    const before = qc.getQueryData<EnvListView>(envKeys.list);
    if (before) {
      qc.setQueryData<EnvListView>(envKeys.list, {
        ...before,
        environments: before.environments.map((e) => (e.id === activeEnv.id ? { ...e, db_auto_approve: on } : e)),
      });
    }
    try {
      const r = await commands.setDbAutoApprove(activeEnv.id, on, global ? null : workingDir || null);
      if (r.status === "error") throw r.error;
      setNoAskNotes(on ? r.data.filter((o) => o.note) : []);
      void qc.invalidateQueries({ queryKey: envKeys.list });
      logUi(`db auto-approve ${on ? "on" : "off"}`);
    } catch (e) {
      if (before) qc.setQueryData(envKeys.list, before);
      toast.error(String(e));
    }
  };
  // Says whose switches these are, beside the switches themselves.
  const envScope = activeEnv && (
    <p className="text-[11px] text-faint">
      For <span className="font-medium text-text">{activeEnv.name}</span> - each environment keeps its own.
    </p>
  );
  const exe = bridge.data?.mcp_exe ?? "";
  const installed = (tools.data ?? []).filter((t) => t.installed);
  const registerButton = (id: string) => (
    <Button
      size="sm"
      variant="outline"
      disabled={register.isPending && register.variables === id}
      onClick={() => register.mutate(id)}
    >
      <IconRegister aria-hidden />
      {register.isPending && register.variables === id ? "Registering" : "Register"}
    </Button>
  );

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
    forgetDbWritesEverywhere();
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
                <span className="label-trim">{s === "project" ? "This repository" : "Machine-wide"}</span>
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
      <div className="flex min-w-0 flex-col gap-6">
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
                      {/* The old name beside the current one - say a teammate
                          on an older version added it back - offers every
                          tool twice. Nothing is removed on a scan: Register
                          replaces it, so the label comes with that button. */}
                      {(t.registered_servers ?? []).includes(LEGACY_TCM_SERVER) && (
                        <>
                          <span className="text-xs text-warning">Needs updating</span>
                          {registerButton(t.id)}
                        </>
                      )}
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
                    <span className="flex items-center gap-2">
                      {/* Registered by an earlier version under the server's
                          old name. Registering replaces it in the same
                          config, so the same button is the fix. */}
                      {(t.registered_servers ?? []).includes(LEGACY_TCM_SERVER) && (
                        <span className="text-xs text-warning">Needs updating</span>
                      )}
                      {registerButton(t.id)}
                    </span>
                  )}
                </div>
                {/* A machine-wide copy of our server, left from before this
                    repository was registered (or from another one). Most
                    clients let a user-scope server shadow the project one,
                    so it is worth saying - and worth being able to remove
                    from here, since nothing else in the app reaches it.
                    A copy under the server's old name counts the same:
                    retiring takes out both. The old database server's copy
                    is not ours to show: it is removed quietly above. */}
                {(t.global_registered_servers ?? []).some(
                  (s) => s === TCM_SERVER || s === LEGACY_TCM_SERVER,
                ) && (
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
                  claude mcp add --scope project {TCM_SERVER} -- "{exe}" --mcp
                </code>
                <Button
                  size="sm"
                  variant="outline"
                  aria-label="Copy command"
                  onClick={() =>
                    copy(`claude mcp add --scope project ${TCM_SERVER} -- "${exe}" --mcp`, "Command")
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
                    { [TCM_SERVER]: { command: exe, args: ["--mcp"] } },
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
                        { [TCM_SERVER]: { command: exe, args: ["--mcp"] } },
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
          <div className="flex min-w-0 flex-wrap items-baseline gap-x-1.5">
            <h2 className="text-sm font-semibold text-text">Tools an assistant may use</h2>
            {activeEnv && (
              <span className="text-xs text-muted">
                on <span className="font-medium text-text">{activeEnv.name}</span>
              </span>
            )}
          </div>
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
                  ariaDescribedBy={`ai-tool-about-${row.key}`}
                  onCheckedChange={() => {
                    saveDisabledTools(toggleRow(disabled, row.names));
                  }}
                  className="mt-0.5"
                />
                <span className="min-w-0 flex-1">
                  <span className={cn("text-sm font-medium", on ? "text-text" : "text-faint")}>
                    {row.label}
                  </span>
                  {/* The explanation sits behind the info button's tooltip;
                      this hidden copy is what the switch is described by. */}
                  <span id={`ai-tool-about-${row.key}`} className="sr-only">
                    {row.summary}
                  </span>
                  <button
                    type="button"
                    aria-label={`About ${row.label}`}
                    title={row.summary}
                    className="ml-1 inline-flex align-middle text-muted hover:text-accent focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent"
                  >
                    <IconAbout aria-hidden className="size-3.5" />
                  </button>
                </span>
              </li>
            );
          })}
        </ul>
        {visible.some((r) => r.names.some((n) => disabled.includes(n))) && (
          <Button
            size="sm"
            variant="outline"
            onClick={() => {
              saveDisabledTools([]);
            }}
          >
            <IconConfirm aria-hidden />
            Turn all on
          </Button>
        )}
      </section>

      {/* Which rules the writing guide carries: the standard ones, or the
          person's own writing style. Needs the Writing Style module. */}
      {writingOn && <WritingStyleCard />}

      </div>

      {/* Right side. Each column stacks its own cards, so no card waits
          for a taller card in another column (a grid row would leave a
          gap under the shorter one). At lg this wrapper is the second
          column, B over C; at 2xl it dissolves (display:contents) and B
          and C become columns 2 and 3. `gap`, not space-y: space-y's
          child margins would leak through contents into the outer grid. */}
      <div className="flex flex-col gap-6 2xl:contents">
      {/* B holds only module cards. With none of them on it is left out, so
          the breakdown (C) moves up to the second column instead of leaving
          it empty. */}
      {(autoRunOn || dbOn || apiTemplatesOn) && (
      <div className="flex min-w-0 flex-col gap-6">
      {autoRunOn && (
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
              {activeEnv && siteName(activeEnv.start_url || RECIPE_ADDRESS)}
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
      )}

      {dbOn && (
      <section data-tour="ai-db" className="space-y-3 rounded-md border border-border bg-surface p-4">
        <div className="flex items-center gap-2">
          <Database size={14} className="shrink-0 text-muted" />
          <h2 className="text-sm font-semibold text-text">Database Read Access</h2>
        </div>
        <p className="text-xs text-muted">
          The database you choose here is the one this app&apos;s own database tools
          use. They are on while the Database Access module is on, and an assistant
          can then find the table behind a screen and read it while it writes cases.
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
                          Remove
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

          {/* The only switch. Reading is the Database Access module being
              on; writing is its own decision and lives
              here, beside the database it applies to, because that is
              what decides whether it may be made at all.

              Shown OFF on a database that cannot write, whatever is
              stored: the app refuses such a write anyway (both doors -
              this switch AND the database's user), and a switch reading
              "on" while every write comes back refused is a lie. The
              stored choice is kept, so going back to the dev login
              restores it. */}
          <div className="space-y-1 rounded-md border border-border/60 p-2">
            {envScope}
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
                disabled={!writesLive || !activeEnv}
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
          {/* Not tied to the switch above: the Auto Run and API template
              tools' "always allow" follows the modules, and the tools
              switched off in the list below. Hidden from the help site's
              shots, like the other Auto Run content (isCaptureMode). */}
          {autoRunOn && !isCaptureMode() && (
            <p className="text-[11px] text-faint">
              With Auto Run on, Claude Code and Cursor are told to allow the Auto Run tools (and, with
              API Templates on, the template tools) without asking.
            </p>
          )}
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
      )}

      {/* Proving and running a template writes test data through the
          application's own endpoints - a decision separate from whether
          the four tools are reachable at all (the "API templates" row
          above), the same shape as the database card's write switch.
          Shown while the API Templates module is on (capture mode reports
          every module on, since the help site documents it). It names no
          tool, so unlike the tool rows it needs no capture gate. */}
      {apiTemplatesOn && (
        <section data-tour="ai-api-templates" className="space-y-3 rounded-md border border-border bg-surface p-4">
          <h2 className="text-sm font-semibold text-text">API templates</h2>
          <div className="space-y-1 rounded-md border border-border/60 p-2">
            {envScope}
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
              environment&apos;s site address.
            </p>
          </div>
        </section>
      )}

      {/* Whether an assistant gets the network tools at all: until this is
          on, Rust neither lists them nor answers their routes. They are
          Auto Run's tools, so Auto Run off takes them too. Shown while Auto Run is on, but not in capture mode, which hides
          the Auto Run tools from the help site's shots (toolShown). */}
      {autoRunOn && !isCaptureMode() && (
        <section className="space-y-3 rounded-md border border-border bg-surface p-4">
          <h2 className="text-sm font-semibold text-text">Network tools</h2>
          <div className="space-y-1 rounded-md border border-border/60 p-2">
            {envScope}
            <div className="flex items-center justify-between gap-2">
              <span className="text-xs font-medium text-muted">Network tools</span>
              <Switch ariaLabel="Network tools" checked={networkTools} onCheckedChange={saveNetworkTools} />
            </div>
            <p className="text-[11px] text-faint">
              Off by default. Lets an assistant read the requests the Auto Run browser&apos;s
              page made, with secrets hidden, save and load them as HAR files, and set test
              headers and cookies in a browser it opened.
            </p>
          </div>
        </section>
      )}

      </div>
      )}

      <div className="flex min-w-0 flex-col gap-6">
      <section className="space-y-3 rounded-md border border-border bg-surface p-4">
        <h2 className="text-sm font-semibold text-text">AI Tools Breakdown</h2>
        <p className="text-sm text-muted">
          The best way to write test cases: ask the assistant to read the
          writing guide and a few example cases, then draft cases for your PBI
          and build the run sheet. Import the result yourself on the Import
          Test Cases tab. Keep the file watched, and any problems show up here
          as it saves.
        </p>
        <p className="text-sm text-muted">
          {/* No count in the sentence: it went stale twice - the list
              below is the inventory. */}
          Connected assistants can read, tidy up their own draft and save files
          on this computer. None of them can change anything in Azure DevOps.
          Each tool below has a switch above:
        </p>
        <ul className="space-y-1.5 text-xs text-muted">
          <li>
            <span className="font-medium text-text">Test Suites</span> — find a test suite by plan name, suite name or PBI id, and read its cases in order. Read only. Off: it can reach cases only through a PBI or case ids.
          </li>
          <li>
            <span className="font-medium text-text">Run results</span> — see how a PBI&apos;s cases did on their last run, with the testers&apos; comments. Off: the assistant sees no results.
          </li>
          <li>
            <span className="font-medium text-text">Project tags</span> — use the tags your project already has, so it does not invent new ones. Off: it has to guess.
          </li>
          <li>
            <span className="font-medium text-text">Find a PBI</span> — look up a PBI by its title or number. PBIs only, never bugs or tasks. Off: you give it the id.
          </li>
          <li>
            <span className="font-medium text-text">Project wiki</span> — search your wiki and read a page, for the background to a requirement. Searching and reading share one switch.
          </li>
          {dbOn && (
          <li>
            <span className="font-medium text-text">Database Read Access</span> — find the tables behind a topic and check a value in the database you chose. It can only look, unless you allow changes beside that connection. Off: no database tools.
          </li>
          )}
          {toolShown("save_autorun_script") && (
            <li>
              <span className="font-medium text-text">Auto Run scripts</span> — write and fix browser scripts for your test cases: read the script guide, look at the page you opened, try a step, then save the scripts. It never signs in. Reading the page&apos;s requests and setting test headers and cookies also needs the Network tools switch, which is off by default. Off: you run Auto Run by hand.
            </li>
          )}
          {toolShown("list_api_templates") && (
            <li>
              <span className="font-medium text-text">API templates</span> — build, test and run templates that create test data through the application itself. It can also map a module&apos;s steps as a flow of stages and check which stages are done before each run, so templates go in the order the application allows. Those checks read the company database, so they need Database Read Access switched on. Testing and running also need the API templates switch above, which is off by default. Off: you do template work by hand.
            </li>
          )}
          {toolShown("open_report") && (
            <li>
              <span className="font-medium text-text">Crystal Reports</span> — open a report file, read it, change it, compare two reports and draw a preview. Every change is saved over the file at once, so keep the report in git to undo it. Off: you do report work by hand.
            </li>
          )}
        </ul>
      </section>
      </div>
      </div>
    </div>
  );
}
