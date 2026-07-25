import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { getVersion } from "@tauri-apps/api/app";
import { CHANGELOG } from "../lib/changelog";
import { useState } from "react";
import { toast } from "sonner";
import { openPath } from "@tauri-apps/plugin-opener";
import { commands } from "../bindings";
import { Button } from "../components/ui/button";
import { START_TOUR_EVENT } from "../components/UiTour";
import { RATE_LEVELS, getRateLevel, setRateLevel, type RateLevel } from "../lib/adoRate";
import { cn } from "../lib/cn";
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

// org/project stay in the signature (App passes them) for when a
// project-scoped setting returns here.
export default function Settings(_props: { org: string; project: string }) {
  const qc = useQueryClient();
  const [choice, setChoiceState] = useState<ThemeChoice>(getThemeChoice());
  const [accent, setAccentState] = useState<Accent>(getAccent());
  const [rate, setRate] = useState<RateLevel>(getRateLevel());
  // The right column shows one panel at a time - the changelog, or the
  // app's own log for when something needs reporting.
  const [rightPanel, setRightPanel] = useState<"changelog" | "logs">("changelog");

  const logs = useQuery({
    queryKey: ["app-logs"],
    queryFn: () => commands.appLogs(500),
    enabled: rightPanel === "logs",
    // Ongoing: refresh while the panel is open so it reads live.
    refetchInterval: rightPanel === "logs" ? 2000 : false,
  });
  const logDir = useQuery({
    queryKey: ["app-log-dir"],
    queryFn: () => commands.appLogDir(),
    enabled: rightPanel === "logs",
    staleTime: Infinity,
  });


  const version = useQuery({
    queryKey: ["app-version"],
    queryFn: () => getVersion().catch(() => "dev"),
    staleTime: Infinity,
  });

  const check = useMutation({
    mutationFn: () => commands.checkUpdate(),
    onSuccess: (v) => {
      // App's update banner renders from the ["update"] query (fetched once
      // at startup) - seed it so the banner appears for a manual check too.
      qc.setQueryData(["update"], v);
      if (v) toast.info(`Version ${v} is available - use the banner to update.`);
      else toast.success("You are on the latest version.");
    },
  });

  const pick = (t: ThemeChoice) => {
    setChoiceState(t);
    setThemeChoice(t);
  };


  return (
    // Two columns on wide windows (Changelog fills the otherwise-empty right
    // half); below lg everything stacks into the original single column.
    <div className="grid max-w-lg gap-8 lg:max-w-4xl lg:grid-cols-2 lg:items-start">
      <div className="space-y-8">
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
        <h2 className="text-sm font-semibold text-text">Azure DevOps request rate</h2>
        <p className="text-sm text-muted">
          Azure DevOps limits requests per user, not per app - so this app
          shares your budget with your browser. Slow it down if Azure DevOps
          starts warning you about usage.
        </p>
        <div className="space-y-1.5">
          {RATE_LEVELS.map((l) => (
            <button
              key={l.id}
              aria-pressed={rate === l.id}
              className={cn(
                "flex w-full flex-col items-start rounded-md border px-3 py-2 text-left transition-colors",
                rate === l.id
                  ? "border-accent bg-accent-soft"
                  : "border-border hover:border-border-strong",
              )}
              onClick={() => {
                setRate(l.id);
                setRateLevel(l.id);
              }}
            >
              <span className={cn("text-sm", rate === l.id ? "text-accent" : "text-text")}>
                {l.label}
              </span>
              <span className="text-xs text-muted">{l.hint}</span>
            </button>
          ))}
        </div>
      </section>

      <section className="space-y-3">
        <h2 className="text-sm font-semibold text-text">Interface tour</h2>
        <p className="text-sm text-muted">
          Replay the walkthrough that highlights each area of the app.
        </p>
        <Button
          size="sm"
          variant="outline"
          onClick={() => window.dispatchEvent(new Event(START_TOUR_EVENT))}
        >
          Show UI tour
        </Button>
      </section>

      {/* The Module / Preconditions field mapping is auto-detected
          (useFieldRefs ranked match) and deliberately NOT user-editable -
          re-add a "Test case fields" section here if that ever needs a
          manual override. */}

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

      <section className="space-y-3">
        <div className="flex items-center gap-2">
          <h2 className="text-sm font-semibold text-text">
            {rightPanel === "changelog" ? "Changelog" : "App log"}
          </h2>
          <div className="ml-auto flex rounded-md border border-border p-0.5">
            {(["changelog", "logs"] as const).map((p) => (
              <button
                key={p}
                aria-pressed={rightPanel === p}
                className={cn(
                  "rounded px-2 py-1 text-xs transition-colors",
                  rightPanel === p ? "bg-accent-soft text-accent" : "text-muted hover:text-text",
                )}
                onClick={() => setRightPanel(p)}
              >
                {p === "changelog" ? "Changelog" : "Logs"}
              </button>
            ))}
          </div>
        </div>

        {rightPanel === "logs" ? (
          <>
            <p className="text-sm text-muted">
              What the app has been doing - include this when reporting a bug.
              Daily files are kept for a week.
            </p>
            <div className="flex items-center gap-2">
              <Button
                size="sm"
                variant="outline"
                onClick={() => {
                  const text = (logs.data ?? [])
                    .map((l) => `${l.at} [${l.level.toUpperCase()}] ${l.message}`)
                    .join("\n");
                  navigator.clipboard
                    .writeText(text)
                    .then(() => toast.success("Log copied."))
                    .catch(() => toast.error("Could not copy to clipboard."));
                }}
              >
                Copy log
              </Button>
              <Button
                size="sm"
                variant="outline"
                disabled={!logDir.data}
                onClick={() => {
                  const dir = logDir.data;
                  if (!dir) return;
                  openPath(dir).catch(() => toast.error("Could not open the log folder."));
                }}
              >
                Open log folder
              </Button>
            </div>
            <div className="max-h-72 space-y-0.5 overflow-y-auto rounded-md border border-border p-3 lg:max-h-[70vh]">
              {(logs.data?.length ?? 0) === 0 ? (
                <p className="text-xs text-faint">Nothing logged yet this session.</p>
              ) : (
                logs.data!.map((l, i) => (
                  <p key={i} className="id-mono flex gap-2 text-[11px] leading-relaxed">
                    <span className="shrink-0 text-faint">{l.at}</span>
                    <span
                      className={cn(
                        "shrink-0 uppercase",
                        l.level === "error"
                          ? "text-danger"
                          : l.level === "warn"
                            ? "text-warning"
                            : "text-muted",
                      )}
                    >
                      {l.level}
                    </span>
                    <span className="break-words text-text">{l.message}</span>
                  </p>
                ))
              )}
            </div>
          </>
        ) : (
          <>
        <p className="text-sm text-muted">
          What changed in each version - the same notes the post-update popup shows.
        </p>
        <div className="max-h-72 space-y-4 overflow-y-auto rounded-md border border-border p-3 lg:max-h-[70vh]">
          {CHANGELOG.map((e) => (
            <div key={e.version} className="space-y-1.5">
              <h3 className="text-xs font-semibold text-text">
                Version {e.version}
                <span className="ml-2 font-normal text-faint">{e.date}</span>
              </h3>
              <ul className="list-disc space-y-1 pl-4 text-xs text-muted">
                {e.items.map((item, i) => (
                  <li key={i}>{item}</li>
                ))}
              </ul>
            </div>
          ))}
        </div>
          </>
        )}
      </section>
    </div>
  );
}
