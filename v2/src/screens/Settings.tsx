import { useMutation, useQuery } from "@tanstack/react-query";
import { getVersion } from "@tauri-apps/api/app";
import { useState } from "react";
import { toast } from "sonner";
import { commands } from "../bindings";
import { Button } from "../components/ui/button";
import { Select } from "../components/ui/select";
import { useFieldRefs } from "../hooks/useFieldRefs";
import { saveFieldPrefs } from "../lib/fieldPrefs";
import {
  ACCENTS,
  getAccent,
  getTheme,
  setAccent,
  setTheme,
  type Accent,
  type Theme,
} from "../lib/theme";

const ACCENT_SWATCH: Record<Accent, string> = {
  green: "#22c55e",
  blue: "#3b82f6",
  violet: "#8b5cf6",
  amber: "#f59e0b",
  rose: "#f43f5e",
};

export default function Settings({ org, project }: { org: string; project: string }) {
  const [theme, setThemeState] = useState<Theme>(getTheme());
  const [accent, setAccentState] = useState<Accent>(getAccent());
  const { fields, prefs } = useFieldRefs(org, project);
  const [, bump] = useState(0); // re-render after saving field prefs

  const version = useQuery({
    queryKey: ["app-version"],
    queryFn: () => getVersion().catch(() => "dev"),
    staleTime: Infinity,
  });

  const check = useMutation({
    mutationFn: () => commands.checkUpdate(),
    onSuccess: (v) =>
      v
        ? toast.info(`Version ${v} is available - use the banner to update.`)
        : toast.success("You are on the latest version."),
  });

  const pick = (t: Theme) => {
    setThemeState(t);
    setTheme(t);
  };

  const setRef = (which: "moduleRef" | "preconditionsRef", value: string) => {
    saveFieldPrefs(org, project, { ...prefs, [which]: value || null });
    bump((n) => n + 1);
    toast.success("Field mapping saved.");
  };

  return (
    <div className="max-w-lg space-y-8">
      <section className="space-y-3">
        <h2 className="text-sm font-semibold text-text">Appearance</h2>
        <div className="flex gap-2">
          {(["light", "dark", "system"] as Theme[]).map((t) => (
            <Button
              key={t}
              size="sm"
              variant={theme === t ? "default" : "outline"}
              onClick={() => pick(t)}
            >
              {t[0].toUpperCase() + t.slice(1)}
            </Button>
          ))}
        </div>
        <div>
          <p className="mb-2 text-xs text-muted">Accent theme</p>
          <div className="flex gap-2">
            {ACCENTS.map((a) => (
              <button
                key={a}
                aria-label={`Accent ${a}`}
                title={a[0].toUpperCase() + a.slice(1)}
                className="flex h-8 w-8 items-center justify-center rounded-full border-2 transition-transform hover:scale-110"
                style={{
                  backgroundColor: ACCENT_SWATCH[a],
                  borderColor: accent === a ? "var(--color-text)" : "transparent",
                }}
                onClick={() => {
                  setAccentState(a);
                  setAccent(a);
                }}
              >
                {accent === a && <span className="text-xs font-bold text-white">✓</span>}
              </button>
            ))}
          </div>
        </div>
      </section>

      <section className="space-y-3">
        <h2 className="text-sm font-semibold text-text">Test case fields</h2>
        {!org || !project ? (
          <p className="text-sm text-muted">
            Pick an organization and project to map the Module and
            Preconditions fields for this project's process.
          </p>
        ) : (
          <>
            <p className="text-sm text-muted">
              Where imported Module / Preconditions values are written for{" "}
              <span className="text-text">{project}</span>. Auto-detected; "Skip"
              leaves the field untouched.
            </p>
            {fields.isError && <p className="text-sm text-danger">{fields.error.message}</p>}
            <label className="flex flex-col gap-1 text-xs text-muted">
              Module field
              <Select
                value={prefs.moduleRef ?? ""}
                disabled={!fields.data}
                onChange={(e) => setRef("moduleRef", e.target.value)}
              >
                <option value="">None - skip this field</option>
                {(fields.data ?? []).map((f) => (
                  <option key={f.reference_name} value={f.reference_name}>
                    {f.name} ({f.reference_name})
                  </option>
                ))}
              </Select>
            </label>
            <label className="flex flex-col gap-1 text-xs text-muted">
              Preconditions field
              <Select
                value={prefs.preconditionsRef ?? ""}
                disabled={!fields.data}
                onChange={(e) => setRef("preconditionsRef", e.target.value)}
              >
                <option value="">None - skip this field</option>
                {(fields.data ?? []).map((f) => (
                  <option key={f.reference_name} value={f.reference_name}>
                    {f.name} ({f.reference_name})
                  </option>
                ))}
              </Select>
            </label>
          </>
        )}
      </section>

      <section className="space-y-3">
        <h2 className="text-sm font-semibold text-text">Updates</h2>
        <p className="text-sm text-muted">
          Version {version.data ?? "..."} - updates install automatically from
          the releases feed.
        </p>
        <Button size="sm" variant="outline" disabled={check.isPending} onClick={() => check.mutate()}>
          {check.isPending ? "Checking..." : "Check for updates"}
        </Button>
      </section>
    </div>
  );
}
