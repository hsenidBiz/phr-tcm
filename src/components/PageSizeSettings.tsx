import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { commands, type AppSettings, type PageSize } from "../bindings";
import { toast } from "../lib/toast";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { SettingRow } from "./settings/SettingsCard";

/** What the app ships with. Rust holds the same numbers (`viewport.rs`); this
 *  is only for deciding whether Reset has anything to undo - the size that
 *  counts always comes back from Rust. */
const SHIPPED: PageSize = { width: 1280, height: 720 };

const same = (a: PageSize, b: PageSize) => a.width === b.width && a.height === b.height;

/** The "Browser page size" row of Settings' Auto Run card: the page every
 *  browser the app drives shows, kept by Rust with the other app settings
 *  (the `["app-settings"]` query). Rust refuses a size outside its range and
 *  says so in a sentence; that sentence is shown as it is. */
export default function PageSizeSettings() {
  const qc = useQueryClient();
  const settings = useQuery({ queryKey: ["app-settings"], queryFn: () => commands.getAppSettings() });
  const saved: PageSize = settings.data?.page_size ?? SHIPPED;
  const [width, setWidth] = useState(String(saved.width));
  const [height, setHeight] = useState(String(saved.height));
  const [refusal, setRefusal] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // The boxes follow what Rust has once it is known (and after a save or a reset).
  useEffect(() => {
    setWidth(String(saved.width));
    setHeight(String(saved.height));
  }, [saved.width, saved.height]);

  const typed: PageSize = { width: Number(width), height: Number(height) };
  const remember = (page_size: PageSize) => {
    const before = qc.getQueryData<AppSettings>(["app-settings"]);
    if (before) qc.setQueryData<AppSettings>(["app-settings"], { ...before, page_size });
    else void qc.invalidateQueries({ queryKey: ["app-settings"] });
  };

  const save = async () => {
    setBusy(true);
    setRefusal(null);
    try {
      const r = await commands.setPageSize(typed);
      if (r.status === "error") setRefusal(r.error);
      else {
        remember(r.data);
        setWidth(String(r.data.width));
        setHeight(String(r.data.height));
      }
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusy(false);
    }
  };

  const reset = async () => {
    setBusy(true);
    setRefusal(null);
    try {
      const size = await commands.resetPageSize();
      remember(size);
      setWidth(String(size.width));
      setHeight(String(size.height));
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusy(false);
    }
  };

  const edited = !same(typed, saved);
  return (
    <SettingRow
      name="Browser page size"
      description="Every browser the app drives shows its page at this size. It applies to the next browser the app opens. Default 1280 x 720."
      control={
        <>
          <label className="flex items-center gap-1.5 text-xs text-muted">
            Width
            <Input
              type="number"
              inputMode="numeric"
              min={800}
              max={3840}
              className="w-20 py-1"
              value={width}
              disabled={!settings.data || busy}
              onChange={(e) => setWidth(e.target.value)}
            />
          </label>
          <label className="flex items-center gap-1.5 text-xs text-muted">
            Height
            <Input
              type="number"
              inputMode="numeric"
              min={600}
              max={2160}
              className="w-20 py-1"
              value={height}
              disabled={!settings.data || busy}
              onChange={(e) => setHeight(e.target.value)}
            />
          </label>
          <Button size="sm" disabled={!settings.data || busy || !edited} onClick={() => void save()}>
            Save
          </Button>
          <Button
            size="sm"
            variant="outline"
            disabled={!settings.data || busy || (same(saved, SHIPPED) && !edited)}
            onClick={() => void reset()}
          >
            Reset to default
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
