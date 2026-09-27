import { useQuery, useQueryClient } from "@tanstack/react-query";
import { commands, type AppSettings } from "../bindings";
import { toast } from "../lib/toast";
import { Switch } from "./ui/switch";
import { SettingRow } from "./settings/SettingsCard";

/** The background rows of Settings' General card: whether closing the
 *  window keeps the app running in the tray (so the AI tools stay
 *  available), whether it starts at sign-in, and whether that start stays
 *  in the tray or opens the window. All are kept by Rust. Rendered as rows, not a section, so they sit inside the card's
 *  divided list beside the other General settings. */
export default function BackgroundSettings() {
  const qc = useQueryClient();
  const settings = useQuery({ queryKey: ["app-settings"], queryFn: () => commands.getAppSettings() });
  const autostart = useQuery({ queryKey: ["autostart"], queryFn: () => commands.getAutostart() });

  const setTray = async (on: boolean) => {
    const before = settings.data;
    if (before) qc.setQueryData<AppSettings>(["app-settings"], { ...before, close_to_tray: on });
    try {
      const r = await commands.setCloseToTray(on);
      if (r.status === "error") throw r.error;
      qc.setQueryData(["app-settings"], r.data);
    } catch (e) {
      if (before) qc.setQueryData(["app-settings"], before);
      toast.error(String(e));
    }
  };

  const setStartMinimized = async (on: boolean) => {
    const before = settings.data;
    if (before) qc.setQueryData<AppSettings>(["app-settings"], { ...before, start_minimized: on });
    try {
      const r = await commands.setStartMinimized(on);
      if (r.status === "error") throw r.error;
      qc.setQueryData(["app-settings"], r.data);
    } catch (e) {
      if (before) qc.setQueryData(["app-settings"], before);
      toast.error(String(e));
    }
  };

  const setAutostart = async (on: boolean) => {
    const before = autostart.data ?? false;
    qc.setQueryData(["autostart"], on);
    try {
      const r = await commands.setAutostart(on);
      if (r.status === "error") throw r.error;
      qc.setQueryData(["autostart"], r.data);
    } catch (e) {
      qc.setQueryData(["autostart"], before);
      toast.error(String(e));
    }
  };

  return (
    <>
      <SettingRow
        asLabel
        name="Keep running in the tray when closed"
        description="Keeps your AI tools available. Right-click the tray icon to quit."
        control={
          <Switch
            checked={settings.data?.close_to_tray ?? true}
            disabled={!settings.data}
            onCheckedChange={(on) => void setTray(on)}
            ariaLabel="Keep running in the tray when closed"
          />
        }
      />
      <SettingRow
        asLabel
        name="Start with Windows"
        description="Starts the app when you sign in to Windows."
        control={
          <Switch
            checked={autostart.data ?? false}
            disabled={autostart.data === undefined}
            onCheckedChange={(on) => void setAutostart(on)}
            ariaLabel="Start with Windows"
          />
        }
      />
      {/* Only means something for a start at sign-in, so it waits for
          Start with Windows to be on. */}
      <SettingRow
        asLabel
        name="Start minimized"
        description="At sign-in, stays in the Show hidden icons area instead of opening the window."
        control={
          <Switch
            checked={settings.data?.start_minimized ?? true}
            disabled={!settings.data || !autostart.data}
            onCheckedChange={(on) => void setStartMinimized(on)}
            ariaLabel="Start minimized"
          />
        }
      />
    </>
  );
}
