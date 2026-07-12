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
  THEMES,
  getAccent,
  getThemeChoice,
  setAccent,
  setThemeChoice,
  type Accent,
  type ThemeChoice,
} from "../lib/theme";

const ACCENT_SWATCH: Record<Accent, string> = {
  default: "var(--color-accent)", // live preview of the theme's own accent
  green: "#22c55e",
  blue: "#3b82f6",
  violet: "#8b5cf6",
  amber: "#f59e0b",
  rose: "#f43f5e",
};

const ACCENT_TITLE: Record<Accent, string> = {
  default: "Theme default",
  green: "Green",
  blue: "Blue",
  violet: "Violet",
  amber: "Amber",
  rose: "Rose",
};

export default function Settings({ org, project }: { org: string; project: string }) {
  const [choice, setChoiceState] = useState<ThemeChoice>(getThemeChoice());
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

  const pick = (t: ThemeChoice) => {
    setChoiceState(t);
    setThemeChoice(t);
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
        <div>
          <p className="mb-2 text-xs text-muted">Theme - changes the entire UI palette</p>
          <div className="flex flex-wrap gap-2">
            {THEMES.map((t) => (
              <button
                key={t.id}
                aria-label={`Theme ${t.label}`}
                className={`w-20 rounded-md border-2 p-1 text-left transition-transform hover:scale-105 ${
                  choice === t.id ? "border-accent" : "border-border"
                }`}
                onClick={() => pick(t.id)}
              >
                <span
                  className="block h-10 w-full overflow-hidden rounded"
                  style={{ backgroundColor: t.preview.bg }}
                >
                  <span
                    className="mx-1.5 mt-1.5 block h-3 rounded-sm"
                    style={{ backgroundColor: t.preview.surface }}
                  />
                  <span
                    className="mx-1.5 mt-1 block h-1.5 w-6 rounded-sm"
                    style={{ backgroundColor: t.preview.accent }}
                  />
                </span>
                <span className="mt-1 block text-center text-xs text-muted">{t.label}</span>
              </button>
            ))}
            <button
              aria-label="Theme System"
              className={`w-20 rounded-md border-2 p-1 text-left transition-transform hover:scale-105 ${
                choice === "system" ? "border-accent" : "border-border"
              }`}
              onClick={() => pick("system")}
            >
              <span className="block h-10 w-full overflow-hidden rounded">
                <span className="flex h-full">
                  <span className="h-full w-1/2" style={{ backgroundColor: "#f8fafc" }} />
                  <span className="h-full w-1/2" style={{ backgroundColor: "#0f172a" }} />
                </span>
              </span>
              <span className="mt-1 block text-center text-xs text-muted">System</span>
            </button>
          </div>
        </div>
        <div>
          <p className="mb-2 text-xs text-muted">Accent - overrides the theme's accent color</p>
          <div className="flex gap-2">
            {ACCENTS.map((a) => (
              <button
                key={a}
                aria-label={`Accent ${ACCENT_TITLE[a]}`}
                title={ACCENT_TITLE[a]}
                className="flex h-8 w-8 items-center justify-center rounded-full border-2 transition-transform hover:scale-110"
                style={{
                  backgroundColor: ACCENT_SWATCH[a],
                  borderColor: accent === a ? "var(--color-text)" : "transparent",
                  // The default swatch previews the active theme's accent,
                  // marked with a dashed ring so it reads as "auto".
                  borderStyle: a === "default" && accent !== a ? "dashed" : "solid",
                  ...(a === "default" && accent !== a
                    ? { borderColor: "var(--color-border-strong)" }
                    : {}),
                }}
                onClick={() => {
                  setAccentState(a);
                  setAccent(a);
                }}
              >
                {accent === a && <span className="text-xs font-bold text-white">✓</span>}
                {a === "default" && accent !== a && (
                  <span className="text-[10px] font-semibold text-on-accent">A</span>
                )}
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
          Version {version.data ?? "-"} - updates install automatically from
          the releases feed.
        </p>
        <Button size="sm" variant="outline" disabled={check.isPending} onClick={() => check.mutate()}>
          {check.isPending ? "Checking" : "Check for updates"}
        </Button>
      </section>
    </div>
  );
}
