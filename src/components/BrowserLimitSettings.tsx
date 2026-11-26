import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { commands, type AppSettings } from "../bindings";
import { toast } from "../lib/toast";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { SettingRow } from "./settings/SettingsCard";

/** What Rust holds when nothing was chosen (`app_settings.rs`). */
const SHIPPED = 5;
const MIN = 1;
const MAX = 8;

const REFUSAL = "Browsers at once must be a whole number from 1 to 8.";

/** The "Browsers at once" row of Auto Test's settings dialog: how many Auto
 *  Test browsers can be open together, kept by Rust with the other app
 *  settings (the `["app-settings"]` query). Lowering it closes no browser; it
 *  only stops new ones until the count falls below it. */
export default function BrowserLimitSettings() {
  const qc = useQueryClient();
  const settings = useQuery({ queryKey: ["app-settings"], queryFn: () => commands.getAppSettings() });
  const saved = settings.data?.autorun_browsers ?? SHIPPED;
  const [text, setText] = useState(String(saved));
  const [refusal, setRefusal] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // The box follows what Rust has once it is known (and after a save).
  useEffect(() => setText(String(saved)), [saved]);

  const save = async () => {
    if (!/^\d{1,3}$/.test(text.trim()) || Number(text) < MIN || Number(text) > MAX) {
      setRefusal(REFUSAL);
      return;
    }
    setBusy(true);
    setRefusal(null);
    try {
      const r = await commands.setAutorunBrowsers(Number(text));
      if (r.status === "error") setRefusal(r.error);
      else {
        const before = qc.getQueryData<AppSettings>(["app-settings"]);
        if (before) qc.setQueryData<AppSettings>(["app-settings"], { ...before, autorun_browsers: r.data });
        else void qc.invalidateQueries({ queryKey: ["app-settings"] });
        setText(String(r.data));
      }
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <SettingRow
      name="Browsers at once"
      description="How many Auto Test browsers can be open at the same time. Each one uses memory."
      control={
        <>
          <Input
            type="number"
            inputMode="numeric"
            aria-label="Browsers at once"
            min={MIN}
            max={MAX}
            className="w-20 py-1"
            value={text}
            disabled={!settings.data || busy}
            onChange={(e) => setText(e.target.value)}
          />
          <Button
            size="sm"
            aria-label="Save: browsers at once"
            disabled={!settings.data || busy || text === String(saved)}
            onClick={() => void save()}
          >
            Save
          </Button>
        </>
      }
    >
      {refusal && (
        <p role="alert" className="text-xs text-danger">
          {refusal}
        </p>
      )}
    </SettingRow>
  );
}
