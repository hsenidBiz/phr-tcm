import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { commands, type PbiHit } from "../bindings";
import { useEnvironments } from "../lib/environments";
import { activeEnvIdSnapshot, subscribeActiveEnv } from "../lib/activeEnv";
import { disabledToolsSnapshot, subscribeDisabledTools } from "../lib/mcpTools";
import { dropRetiredAiSwitches } from "../lib/aiScope";
import {
  dbWritesSnapshot,
  isDevLoginUser,
  migrateLegacyDbConnection,
  selectedDbSnapshot,
  subscribeDbSettings,
} from "../lib/dbServer";
import { apiWritesSnapshot, subscribeApiWrites } from "../lib/apiTemplates";
import { networkToolsSnapshot, subscribeNetworkTools } from "../lib/networkTools";
import { migrateOldTrialSwitch, WRITING_STYLE_QUERY } from "../lib/writingStyle";
import { subscribeWorkingDir, workingDirSnapshot } from "../lib/workingDir";
import { useFieldRefs } from "../hooks/useFieldRefs";

type Args = {
  signedIn: boolean;
  org: string;
  /** The effective project (the PBI's own while projects show together). */
  project: string;
  /** The project the bridge learns: the home project while projects show together. */
  bridgeProject: string;
  allProjects: boolean;
  pbi: PbiHit | null;
  tourOpen: boolean;
};

/**
 * Keeps the AI bridge's defaults in sync with what the user is looking at:
 * org/project + the detected custom-field refs. Fire-and-forget; the
 * bridge simply serves stale context until the next push.
 */
export function useBridgeContextSync({
  signedIn,
  org,
  project,
  bridgeProject,
  allProjects,
  pbi,
  tourOpen,
}: Args) {
  const qc = useQueryClient();
  // No tour special-case needed for the field refs: `saveFieldPrefs` (which
  // this hook calls during render, not from an effect) refuses to write
  // while the tour is running - see `lib/fieldPrefs.ts`. That covers every
  // caller, this one included, so passing it the sample org/project during
  // a tour is safe.
  const { prefs: bridgePrefs } = useFieldRefs(org, bridgeProject);
  // Re-pushed when the AI Bridge tab toggles a tool, so the change reaches
  // an assistant on its next tools/list rather than after a restart.
  const disabledTools = useSyncExternalStore(subscribeDisabledTools, disabledToolsSnapshot);
  // The working repository decides where a writing job's file goes, so the
  // bridge learns of a change the moment the AI Bridge tab makes it.
  const workingDir = useSyncExternalStore(subscribeWorkingDir, workingDirSnapshot);
  // The database the two database tools run on, by id, and whether they
  // may write. Both live on the AI Bridge tab and both are pushed here, so
  // choosing a different database takes effect on the assistant's next
  // call rather than after a restart. Rust resolves the id to its login at
  // that call - no login ever crosses from here.
  const dbId = useSyncExternalStore(subscribeDbSettings, selectedDbSnapshot);
  const dbWrites = useSyncExternalStore(subscribeDbSettings, dbWritesSnapshot);
  // Whether the assistant may prove and run API templates - the AI Bridge
  // tab's own switch, pushed the moment it changes for the same reason the
  // database write switch is.
  const apiWrites = useSyncExternalStore(subscribeApiWrites, apiWritesSnapshot);
  // Whether an assistant gets the network tools - pushed the same way.
  const networkTools = useSyncExternalStore(subscribeNetworkTools, networkToolsSnapshot);
  // The switches above are the ACTIVE environment's, and every push
  // says which environment that is: Rust allows no write from a push for
  // any other one, so a push still on its way when the environment changes
  // can never carry one environment's writes into the next. The list is
  // read here, not only on the AI Bridge tab, so the switches are the
  // active environment's from the start.
  useEnvironments(signedIn);
  const envId = useSyncExternalStore(subscribeActiveEnv, activeEnvIdSnapshot);
  const lastPushedEnv = useRef<string | null>(null);
  // Who each database signs in as - the same list, under the same key, the
  // AI Bridge card reads and refreshes when a login is saved.
  const databases = useQuery({
    queryKey: ["db-databases"],
    queryFn: async () => (await commands.dbDatabases()) ?? [],
    enabled: signedIn,
  });
  // The write switch alone is only half the permission - the Rust side
  // also requires the database's own user to be the dev login, and
  // refuses a write when either is missing. Pushing the raw switch here
  // would tell the assistant it may write on a read-only database, which
  // is not true. `isDevLoginUser` is the same rule AiBridge uses to decide
  // whether the switch may even be moved.
  const devLogin = isDevLoginUser(databases.data?.find((d) => d.id === dbId)?.user ?? "");
  // A connection string an older version kept in the webview moves into
  // Rust first, so the very first push already names the database it
  // became - and nothing is pushed until that has been tried.
  const [dbMigrated, setDbMigrated] = useState(false);
  useEffect(() => {
    let live = true;
    // Switches an older version kept for the AI Bridge tab, gone with the
    // controls they drove.
    dropRetiredAiSwitches();
    // The old risk-tiered trial switch becomes the starting writing style
    // switched on - here, not on the AI Bridge tab, so it moves over even
    // if that tab is never opened.
    void migrateOldTrialSwitch().then((saved) => {
      if (saved) void qc.invalidateQueries({ queryKey: WRITING_STYLE_QUERY });
    });
    migrateLegacyDbConnection().finally(() => {
      if (live) setDbMigrated(true);
    });
    return () => {
      live = false;
    };
  }, []);
  // A burst of changes - the AI Bridge tab's fields write through the same
  // store on every keystroke - used to push one `set_bridge_context` +
  // `bridge_status` IPC pair per character typed. Debounced so a burst of
  // edits becomes one push once typing pauses; the pending push is flushed
  // on unmount rather than dropped, so App never silently skips the last
  // edit.
  const bridgePbiId = pbi?.id ?? null;
  const bridgePbiTitle = pbi?.title ?? "";
  const bridgePbiProject = pbi?.project || project;
  // Nothing is pushed without an org and a project, so the PBI of the org
  // just left (or of a signed-out session) would stay on the bridge: take it
  // off at once.
  useEffect(() => {
    if (!signedIn || !org || !bridgeProject) void commands.clearBridgePbi().catch(() => {});
  }, [signedIn, org, bridgeProject]);
  const pushBridgeContextRef = useRef<() => void>(() => {});
  const bridgeContextTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => {
    if (tourOpen || !dbMigrated) return;
    if (!signedIn || !org || !bridgeProject) return;
    const push = () => {
      commands
        .bridgeStatus()
        .then(() =>
          commands.setBridgeContext(
            org,
            bridgeProject,
            bridgePrefs.moduleRef,
            bridgePrefs.preconditionsRef,
            disabledTools,
            workingDir || null,
            dbId || null,
            {
              dbWrites: dbWrites && devLogin,
              apiWrites,
              networkTools,
              envId,
              allProjects,
              // The PBI in the top bar, for the assistants' get_current_pbi.
              pbi: bridgePbiId == null ? null : { id: bridgePbiId, title: bridgePbiTitle, project: bridgePbiProject },
            },
          ),
        )
        .catch(() => {});
    };
    pushBridgeContextRef.current = push;
    if (bridgeContextTimerRef.current) clearTimeout(bridgeContextTimerRef.current);
    // A new environment is pushed at once rather than after the pause:
    // Rust holds every write off from the switch until its switches arrive.
    const envMoved = lastPushedEnv.current !== null && lastPushedEnv.current !== envId;
    lastPushedEnv.current = envId;
    bridgeContextTimerRef.current = setTimeout(push, envMoved ? 0 : 400);
    return () => {
      if (bridgeContextTimerRef.current) clearTimeout(bridgeContextTimerRef.current);
    };
  }, [
    signedIn,
    org,
    bridgeProject,
    allProjects,
    bridgePrefs.moduleRef,
    bridgePrefs.preconditionsRef,
    disabledTools,
    workingDir,
    dbId,
    dbWrites,
    devLogin,
    apiWrites,
    networkTools,
    envId,
    bridgePbiId,
    bridgePbiTitle,
    bridgePbiProject,
    tourOpen,
    dbMigrated,
  ]);
  // App itself effectively never unmounts, but a debounced push that IS
  // still pending when it does must fire rather than vanish silently.
  useEffect(() => {
    return () => {
      if (bridgeContextTimerRef.current) {
        clearTimeout(bridgeContextTimerRef.current);
        pushBridgeContextRef.current();
      }
    };
  }, []);
}
