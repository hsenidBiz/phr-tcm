import { useQuery, useQueryClient } from "@tanstack/react-query";
import { commands, type AppSettings } from "../bindings";
import { toast } from "../lib/toast";
import { Switch } from "./ui/switch";

/** Settings' Background section: whether closing the window keeps the app
 *  running in the tray (so the AI tools stay available), and whether it
 *  starts in the tray at sign-in. Both are kept by Rust. */
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
    <section className="space-y-3">
      <h2 className="text-sm font-semibold text-text">Background</h2>
      <p className="text-sm text-muted">
        Closing the window can leave the app running in the notification area, so the tools your
        AI assistant uses stay available. Right-click its icon there to quit.
      </p>
      <label className="flex items-center gap-2 text-sm text-text">
        <Switch
          checked={settings.data?.close_to_tray ?? true}
          disabled={!settings.data}
          onCheckedChange={(on) => void setTray(on)}
          ariaLabel="Keep running in the tray when closed"
        />
        Keep running in the tray when closed
      </label>
      <label className="flex items-center gap-2 text-sm text-text">
        <Switch
          checked={autostart.data ?? false}
          disabled={autostart.data === undefined}
          onCheckedChange={(on) => void setAutostart(on)}
          ariaLabel="Start with Windows"
        />
        Start with Windows
      </label>
    </section>
  );
}
