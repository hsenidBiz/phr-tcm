import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { getVersion } from "@tauri-apps/api/app";
import { CHANGELOG } from "../lib/changelog";
import { useState } from "react";
import { toast } from "sonner";
import { commands } from "../bindings";
import { Button } from "../components/ui/button";
import { START_TOUR_EVENT } from "../components/UiTour";
import { unwrapStr } from "../lib/ipc";
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

  const version = useQuery({
    queryKey: ["app-version"],
    queryFn: () => getVersion().catch(() => "dev"),
    staleTime: Infinity,
  });

  const bridge = useQuery({
    queryKey: ["bridge-status"],
    queryFn: () => unwrapStr(commands.bridgeStatus()),
    retry: false,
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

      <section className="space-y-3">
        <h2 className="text-sm font-semibold text-text">Changelog</h2>
        <p className="text-sm text-muted">
          What changed in each version - the same notes the post-update popup shows.
        </p>
        <div className="max-h-72 space-y-4 overflow-y-auto rounded-md border border-border p-3">
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
      </section>

      <section className="space-y-3">
        <h2 className="text-sm font-semibold text-text">AI Bridge</h2>
        <p className="text-sm text-muted">
          Lets AI tools (Claude Code, Cursor...) fetch the writing guide, real
          example test cases, and validation from this app while it runs.
          Read-only - AI can never create or change anything in Azure DevOps.
        </p>
        {bridge.data ? (
          <>
            <p className="text-xs text-success">
              Running on 127.0.0.1:{bridge.data.port}
            </p>
            <p className="text-xs text-muted">Register in Claude Code:</p>
            <div className="flex items-center gap-2">
              <code className="id-mono flex-1 truncate rounded bg-surface-2 px-2 py-1 text-xs text-text">
                claude mcp add tcm-testcases -- "{bridge.data.mcp_exe}"
              </code>
              <Button
                size="sm"
                variant="outline"
                onClick={() => {
                  navigator.clipboard
                    .writeText(`claude mcp add tcm-testcases -- "${bridge.data!.mcp_exe}"`)
                    .then(() => toast.success("Copied."));
                }}
              >
                Copy
              </Button>
            </div>
          </>
        ) : (
          <p className="text-xs text-faint">Bridge not running.</p>
        )}
      </section>
    </div>
  );
}
