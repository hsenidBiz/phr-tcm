// Simple mode keeps two switches on for every environment a person has not
// chosen them for: the API write switch and the Network tools. A person's own
// choice is a marker (apiTemplates.ts, networkTools.ts), and a marker wins.
//
// Installs that predate Simple mode have their markers written once, so
// turning Simple mode on never changes what they already had.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef } from "react";
import { commands } from "../bindings";
import { markApiWritesSetFor, turnOnApiWritesWhereUnset } from "./apiTemplates";
import { useEnvironments } from "./environments";
import { markNetworkToolsSetFor, turnOnNetworkToolsWhereUnset } from "./networkTools";

/** The key the app's settings are cached under (Settings, Background, ...). */
const APP_SETTINGS_KEY = ["app-settings"] as const;

/** Mark both switches as chosen for each environment, leaving them as they are. */
export function markAllAsSet(envIds: string[]): void {
  markApiWritesSetFor(envIds);
  markNetworkToolsSetFor(envIds);
}

/** Switch both switches on for each environment with no marker for that
 * switch. Writes no marker, and returns how many each changed. */
export function turnOnUnsetSwitches(envIds: string[]): { apiWrites: number; networkTools: number } {
  return {
    apiWrites: turnOnApiWritesWhereUnset(envIds),
    networkTools: turnOnNetworkToolsWhereUnset(envIds),
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

  const loaded = settings.data !== undefined;
  const migrated = settings.data?.switch_markers_migrated;
  const simple = settings.data?.app_simple_mode === true;
  const envIds = envs.data?.environments.map((e) => e.id);
  const envKey = envIds?.join("\u0000");

  useEffect(() => {
    if (!loaded || !envIds || migrated !== false || migrating.current) return;
    migrating.current = true;
    markAllAsSet(envIds);
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
