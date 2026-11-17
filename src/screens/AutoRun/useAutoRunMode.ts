import { useQuery, useQueryClient } from "@tanstack/react-query";
import { commands, type AutorunMode } from "../../bindings";

/** The key the app's settings are cached under (Settings, Background, ...). */
const APP_SETTINGS_KEY = ["app-settings"] as const;

/** Whether Auto Run shows its Simple or its Advanced screen. A setting
 * that was never written (`null`) reads as Advanced: every install that
 * has Auto Run on is settled by Rust at start-up, so `null` here is an edge
 * case and Advanced is the safe default. `loading` is true until the settings
 * have answered: a screen that hides things by mode waits for it. */
export function useAutoRunMode(): { mode: AutorunMode; loading: boolean; setMode: (m: AutorunMode) => Promise<void> } {
  const qc = useQueryClient();
  const settings = useQuery({ queryKey: APP_SETTINGS_KEY, queryFn: () => commands.getAppSettings() });
  const mode: AutorunMode = settings.data?.autorun_mode ?? "advanced";

  const setMode = async (next: AutorunMode) => {
    const res = await commands.setAutorunMode(next);
    if (res.status === "error") throw new Error(res.error);
    qc.setQueryData(APP_SETTINGS_KEY, res.data);
  };
  return { mode, loading: settings.data === undefined, setMode };
}
