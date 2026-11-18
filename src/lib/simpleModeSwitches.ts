// Simple mode keeps two switches on for every environment a person has not
// chosen them for: the API write switch and the Network tools. A person's own
// choice is a marker (apiTemplates.ts, networkTools.ts), and a marker wins.
//
// Installs that predate Simple mode have their markers written once, so
// turning Simple mode on never changes what they already had.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef } from "react";
import { commands } from "../bindings";
import { turnOnApiWritesWhereUnset } from "./apiTemplates";
import { useEnvironments } from "./environments";
import { turnOnNetworkToolsWhereUnset } from "./networkTools";

/** The key the app's settings are cached under (Settings, Background, ...). */
const APP_SETTINGS_KEY = ["app-settings"] as const;

/** Simple mode's own record that an environment existed before it did. Read
 * only by Simple mode: the switches' person-set markers stay a person's, so
 * nothing else that reads them (the Modules page's first Auto Run switch-on)
 * changes for an upgrade. */
const BASELINE_BASE = "tcm-v2-simple-baseline";

/** Record the given environments as ones Simple mode never switches on,
 * leaving every switch and person-set marker alone. */
export function markBaseline(envIds: string[]): void {
  try {
    for (const id of envIds) localStorage.setItem(`${BASELINE_BASE}:${id}`, "1");
  } catch {
    // storage unavailable -> nothing to record
  }
}

function inBaseline(id: string): boolean {
  try {
    return localStorage.getItem(`${BASELINE_BASE}:${id}`) === "1";
  } catch {
    return false;
  }
}

/** Switch both switches on for each environment outside the baseline, except
 * a switch a person has chosen. Writes no marker, and returns how many each
 * changed. */
export function turnOnUnsetSwitches(envIds: string[]): { apiWrites: number; networkTools: number } {
  const ids = envIds.filter((id) => !inBaseline(id));
  return {
    apiWrites: turnOnApiWritesWhereUnset(ids),
    networkTools: turnOnNetworkToolsWhereUnset(ids),
  };
}

/** Runs the one-time marker migration, then keeps the switches on for new
 * environments while Simple mode is on. Does nothing before the settings and
 * environments have loaded. */
export function useSimpleModeSwitches(): void {
  const qc = useQueryClient();
  const settings = useQuery({ queryKey: APP_SETTINGS_KEY, queryFn: () => commands.getAppSettings() });
  const envs = useEnvironments();
  const migrating = useRef(false);
  // The local baseline is written once; a failed save retries only the save.
  const baselineWritten = useRef(false);

  const loaded = settings.data !== undefined;
  const migrated = settings.data?.switch_markers_migrated;
  const simple = settings.data?.app_simple_mode === true;
  const envIds = envs.data?.environments.map((e) => e.id);
  const envKey = envIds?.join("\u0000");

  useEffect(() => {
    if (!loaded || !envIds || migrated !== false || migrating.current) return;
    migrating.current = true;
    if (!baselineWritten.current) {
      markBaseline(envIds);
      baselineWritten.current = true;
    }
    void commands
      .setSwitchMarkersMigrated()
      .then((res) => {
        if (res.status === "ok") qc.setQueryData(APP_SETTINGS_KEY, res.data);
        else migrating.current = false;
      })
      .catch(() => {
        migrating.current = false;
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loaded, migrated, envKey, qc]);

  useEffect(() => {
    // Migration first: until the markers are written, an existing
    // environment would look unchosen.
    if (!loaded || !envIds || migrated !== true || !simple) return;
    turnOnUnsetSwitches(envIds);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loaded, migrated, simple, envKey]);
}
