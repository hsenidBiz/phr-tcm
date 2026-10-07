import { reportUpdateCheck } from "../lib/updateToast";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { getVersion } from "@tauri-apps/api/app";
import { changelogFor, isBetaVersion, type ChangelogEntry } from "../lib/changelog";
import { memo, useEffect, useRef, useState } from "react";
import { toast } from "../lib/toast";
import {
  hydrateExtras,
  setAdvancedFeatures,
  setExtrasUnlocked,
  useAdvancedFeatures,
  useExtrasUnlocked,
} from "../lib/extras";
import { isCaptureMode } from "../dev/capture";
import { saveFailedMessage, useExtrasSequence } from "./settingsExtras";
import { commands, events, type AppSettings, type GuideStatus } from "../bindings";
import { copyText } from "../lib/clipboard";
import AccountSettings from "../components/AccountSettings";
import BackgroundSettings from "../components/BackgroundSettings";
import { SettingRow, SettingsCard } from "../components/settings/SettingsCard";
import { useTileLayout } from "../components/settings/useTileLayout";
import ChangelogVersionTitle from "../components/ChangelogVersionTitle";
import { Button } from "../components/ui/button";
import { Switch } from "../components/ui/switch";
import { Collapse } from "../components/ui/collapse";
import { Modal } from "../components/ui/modal";
import { Input, Textarea } from "../components/ui/input";
import { openUrl } from "@tauri-apps/plugin-opener";
import { START_TOUR_EVENT } from "../tour/tourState";
import { RATE_LEVELS, getRateLevel, setRateLevel, type RateLevel } from "../lib/adoRate";
import { loadGlobalAllowed, saveGlobalAllowed } from "../lib/aiScope";
import { applyLocalStorage, collectLocalStorage } from "../lib/backup";
import { open, save } from "@tauri-apps/plugin-dialog";
import { cn } from "../lib/cn";
import { LOG_KIND_CLASS, levelOf, tokenizeLog } from "../lib/logSyntax";
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
import {
  IconBrowse,
  IconBug,
  IconCancel,
  IconCopy,
  IconHelp,
  IconDownload,
  IconPlayGame,
  IconRefresh,
  IconTour,
  IconUndo,
} from "../lib/actionIcons";
import RunnerGameModal from "../components/RunnerGameModal";

const ACCENT_SWATCH: Record<Accent, string> = {
  default: "var(--color-accent)", // live preview of the theme's own accent
  green: "#22c55e",
  blue: "#3b82f6",
  violet: "#8655f6",
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

/** The cards that change column when the changelog's history opens, in
 * the order they set off (their `data-settings-card` ids). */
const MOVING_CARDS = ["updates", "backup", "help"] as const;

/** The cards that stack beside those under the changelog when its column
 * has room for two cards across, and otherwise sit in the left column. */
const SPARE_CARDS = ["ai-tools", "extras"] as const;

/** The left track's width - min(32rem, the grid less the 2rem gap and the
 * right track's 24rem floor) - for the cards under the changelog, so a
 * card is the same size in either column and does not change width as it
 * slides. 100cqw is the grid's width (it is the @container). */
const LEFT_TRACK_WIDTH = "lg:w-[min(32rem,calc(100cqw_-_26rem))]";

const BYTES_PER_MB = 1024 * 1024;

/** Whole MB for a size on a button: rounded up, so a guide never reads
 * smaller than it is. */
function mbUp(bytes: number): number {
  return Math.ceil(bytes / BYTES_PER_MB);
}

/** A byte count part-way through a download: rounded down, so it never
 * reads ahead of what has actually arrived. */
function mbDown(bytes: number): number {
  return Math.floor(bytes / BYTES_PER_MB);
}

/** What the guide download shows beside the button, or nothing while the
 * total is not yet known. */
function guideProgressText(p: { received: number; total: number } | null): string | null {
  if (!p || !(p.total > 0)) return null;
  return `${mbDown(p.received)} of ${mbUp(p.total)} MB`;
}

/** How far the guide download is, as a whole percent rounded down, or null
 * while the total is not yet known. */
function guideProgressPercent(p: { received: number; total: number } | null): number | null {
  if (!p || !(p.total > 0)) return null;
  return Math.min(100, Math.floor((p.received / p.total) * 100));
}

export default function Settings({ org, project }: { org: string; project: string }) {
  const qc = useQueryClient();
  // The optional extras (settingsExtras.ts): the listener lives only
  // while this screen is mounted, and shakes this panel.
  const panelRef = useRef<HTMLDivElement>(null);
  useExtrasSequence(panelRef);
  const extrasUnlocked = useExtrasUnlocked();
  const [confirmReset, setConfirmReset] = useState(false);
  const [gameOpen, setGameOpen] = useState(false);
  useEffect(() => {
    void hydrateExtras();
  }, []);
  const resetExtras = () => {
    setExtrasUnlocked(false)
      .then(() => setConfirmReset(false))
      .catch((e) => toast.error(saveFailedMessage(e)));
  };
  // Enable Advanced Features: shows Auto Run, API Templates and their AI
  // tools. The store publishes only after Rust saved it, so a failed save
  // leaves the switch where it was.
  const advancedOn = useAdvancedFeatures();
  const toggleAdvanced = (on: boolean) => {
    setAdvancedFeatures(on).catch((e) => toast.error(saveFailedMessage(e)));
  };
  const [choice, setChoiceState] = useState<ThemeChoice>(getThemeChoice());
  const [accent, setAccentState] = useState<Accent>(getAccent());
  const [rate, setRate] = useState<RateLevel>(getRateLevel());
  // The right column shows one panel at a time - the changelog, or the
  // app's own log for when something needs reporting.
  const [rightPanel, setRightPanel] = useState<"changelog" | "logs">("changelog");
  // The changelog opens on the latest version only; the rest of the
  // history is one click away rather than filling the column. Opening it
  // also moves the cards under it aside, in the same motion (useTileLayout),
  // so whether it is open lives with the layout, not here.
  const changelogRef = useRef<HTMLElement>(null);
  const tiles = useTileLayout({
    rootRef: panelRef,
    changelogRef,
    moving: MOVING_CARDS,
    spare: SPARE_CARDS,
    fold: () => document.getElementById("changelog-history")?.closest<HTMLElement>(".t-collapse") ?? null,
    changelogShown: rightPanel === "changelog",
  });

  // Machine-wide AI tool registration is opt-in; the AI Bridge tab reads
  // the same store and offers the choice only while this is on.
  const [globalAllowed, setGlobalAllowed] = useState(loadGlobalAllowed);
  // Reporting a bug in the APP itself (bugs in the test cases go to
  // Azure DevOps from the runner). Nothing is posted from here - the
  // reporter reviews the prefilled issue and presses the button, which
  // is also why this feature needs no GitHub credential.
  const [reporting, setReporting] = useState(false);
  const [bugTitle, setBugTitle] = useState("");
  const [bugText, setBugText] = useState("");
  // Opening finds the downloaded guide on disk and hands it to the browser -
  // a pending state stops repeated clicks from opening several tabs while
  // that call is in flight.
  const [openingHelp, setOpeningHelp] = useState(false);
  // How To Use is fetched on demand. Anything but a clear answer from Rust
  // (offline from the start, a build that does not know the command) counts
  // as Ready: How To Use shows and nothing complains.
  const [guide, setGuide] = useState<GuideStatus>({ state: "Ready", size: null });
  const [guideProgress, setGuideProgress] = useState<{ received: number; total: number } | null>(null);
  const [downloadingGuide, setDownloadingGuide] = useState(false);
  // A ref, not the state, guards the second click: two clicks in one tick
  // both see `downloadingGuide === false`.
  const guideBusy = useRef(false);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  const askGuideStatus = async () => {
    try {
      const s = await commands.guideStatus();
      if (alive.current && s && typeof s.state === "string") setGuide(s);
    } catch {
      // Keep what is shown: a failed question is not a reason to toast.
    }
  };
  useEffect(() => {
    void askGuideStatus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const downloadGuide = async () => {
    if (guideBusy.current) return;
    guideBusy.current = true;
    setDownloadingGuide(true);
    setGuideProgress(null);
    let unlisten: (() => void) | undefined;
    try {
      unlisten = await events.guideProgress.listen((e) => {
        if (alive.current) setGuideProgress(e.payload);
      });
      const r = await commands.guideDownload();
      if (r.status === "error") toast.error(r.error);
    } catch {
      toast.error("Could not download How to Use. Check your connection and try again - Settings, Logs has the details.");
    } finally {
      try {
        // May return a promise (it does in @tauri-apps/api) or nothing.
        void Promise.resolve(unlisten?.() as unknown).catch(() => {});
      } catch {
        // Going away either way.
      }
      // Opening can fail after a good install, so the state is asked again
      // whether this succeeded or not.
      await askGuideStatus();
      guideBusy.current = false;
      if (alive.current) {
        setDownloadingGuide(false);
        setGuideProgress(null);
      }
    }
  };
  const logs = useQuery({
    queryKey: ["app-logs"],
    queryFn: () => commands.appLogs(2000),
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
  // Everything, requests included: the viewer used to hide the request
  // trail behind a switch, and the log people read was not the log they
  // sent with a bug report.
  const shownLogs = logs.data ?? [];


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
      reportUpdateCheck(v);
    },
  });

  const appSettings = useQuery({ queryKey: ["app-settings"], queryFn: () => commands.getAppSettings() });
  const onBeta = isBetaVersion(version.data ?? "");
  const changelog = changelogFor(version.data ?? "");
  const setBeta = async (on: boolean) => {
    const before = appSettings.data;
    if (before) qc.setQueryData<AppSettings>(["app-settings"], { ...before, beta_updates: on });
    try {
      const r = await commands.setBetaUpdates(on);
      if (r.status === "error") throw r.error;
      qc.setQueryData(["app-settings"], r.data);
      check.mutate();
    } catch (e) {
      if (before) qc.setQueryData(["app-settings"], before);
      toast.error(String(e));
    }
  };

  const pick = (t: ThemeChoice) => {
    setChoiceState(t);
    setThemeChoice(t);
  };

  // Backup & transfer. Import is two steps: pick the file, then confirm in
  // a modal - it overwrites this machine's settings and cache and reloads
  // the app, which is not something a stray double-click should do.
  const [importPath, setImportPath] = useState<string | null>(null);

  const exportBackup = useMutation({
    mutationFn: async () => {
      const stamp = new Date().toISOString().slice(0, 10);
      const path = await save({
        defaultPath: `tcm-backup-${stamp}.json`,
        filters: [{ name: "Test Case Manager backup", extensions: ["json"] }],
      });
      if (!path) return null;
      const r = await commands.exportAppBackup(collectLocalStorage(), path);
      if (r.status === "error") throw new Error(r.error);
      return r.data;
    },
    onSuccess: (s) => {
      if (!s) return; // dialog cancelled
      const skipped = s.skipped.length ? ` ${s.skipped.length} large file(s) were left out.` : "";
      toast.success(`Backup saved: ${s.keys} setting(s) and ${s.files} file(s).${skipped}`);
    },
    onError: (e) => toast.error(`Export failed: ${e.message}`),
  });

  const pickImport = async () => {
    const path = await open({
      multiple: false,
      filters: [{ name: "Test Case Manager backup", extensions: ["json"] }],
    });
    if (typeof path === "string") setImportPath(path);
  };

  const importBackup = useMutation({
    mutationFn: async (path: string) => {
      const r = await commands.importAppBackup(path);
      if (r.status === "error") throw new Error(r.error);
      applyLocalStorage(r.data.local_storage);
      return r.data;
    },
    onSuccess: () => {
      setImportPath(null);
      // Every screen reads its settings at mount - a full reload is the
      // only honest way to make the imported state the live state.
      window.location.reload();
    },
    onError: (e) => {
      setImportPath(null);
      toast.error(`Import failed: ${e.message}`);
    },
  });


  const selectedRate = RATE_LEVELS.find((l) => l.id === rate) ?? RATE_LEVELS[0];

  // Updates, Backup & transfer and Help & support: on a wide window they sit
  // under the changelog, and slide over to the left column while its history
  // is open (useTileLayout). Below the breakpoint they follow AI tools.
  const movingCards = (
    <>
      <SettingsCard
        title="Updates"
        data-tour="settings-updates"
        data-settings-card="updates"
        className={cn(tiles.placement === "right" && LEFT_TRACK_WIDTH)}
      >
        <SettingRow
          name={`Version ${version.data ?? "-"}${onBeta ? " (beta)" : ""}`}
          description="Updates install automatically from the releases feed."
          control={
            <Button size="sm" variant="outline" disabled={check.isPending} onClick={() => check.mutate()}>
              <IconRefresh aria-hidden className={check.isPending ? "animate-spin" : undefined} />
              {check.isPending ? "Checking" : "Check for updates"}
            </Button>
          }
        />
        <SettingRow
          asLabel
          name="Download beta builds"
          description="New features sooner, before the stable release."
          control={
            <Switch
              checked={appSettings.data?.beta_updates ?? false}
              disabled={!appSettings.data}
              onCheckedChange={(on) => void setBeta(on)}
              ariaLabel="Download beta builds"
            />
          }
        >
          {onBeta && appSettings.data && !appSettings.data.beta_updates && (
            <p className="text-xs text-muted">You&apos;re on a beta build. It stays until the next stable release.</p>
          )}
        </SettingRow>
      </SettingsCard>

      <SettingsCard
        title="Backup & transfer"
        data-tour="settings-backup"
        data-settings-card="backup"
        className={cn(tiles.placement === "right" && LEFT_TRACK_WIDTH)}
      >
        <SettingRow
          name="Move to another computer"
          description="Settings and local data in one file. Your sign-in and database logins stay on this computer."
          control={
            <>
              <Button
                size="sm"
                variant="outline"
                disabled={exportBackup.isPending}
                onClick={() => exportBackup.mutate()}
              >
                {exportBackup.isPending ? "Exporting" : "Export to file"}
              </Button>
              <Button size="sm" variant="outline" onClick={pickImport}>
                Import from file
              </Button>
            </>
          }
        />
        {importPath && (
          <Modal onClose={() => setImportPath(null)} className="w-full max-w-md space-y-4 p-5">
            <h3 className="text-sm font-semibold text-text">Import this backup?</h3>
            <p className="text-sm text-muted">
              This replaces the settings and local data on this machine with
              the backup&apos;s copy, then reloads the app. Anything you
              changed here since the backup was made will be overwritten.
            </p>
            <p className="break-all text-xs text-faint">{importPath}</p>
            <div className="flex justify-end gap-2">
              <Button size="sm" variant="outline" onClick={() => setImportPath(null)}>
                Cancel
              </Button>
              <Button
                size="sm"
                disabled={importBackup.isPending}
                onClick={() => importBackup.mutate(importPath)}
              >
                {importBackup.isPending ? "Importing" : "Import and reload"}
              </Button>
            </div>
          </Modal>
        )}
      </SettingsCard>

      <SettingsCard
        title="Help & support"
        data-settings-card="help"
        className={cn(tiles.placement === "right" && LEFT_TRACK_WIDTH)}
      >
        {/* Report a bug used to sit in the changelog panel's header; it is
            still one click from the gear, beside the other two ways of
            getting help. The report reads the log itself. */}
        <SettingRow
          control={
            <>
              {guide.state === "NotDownloaded" ? (
                <Button size="sm" variant="outline" disabled={downloadingGuide} onClick={() => void downloadGuide()}>
                  <IconDownload aria-hidden />
                  {downloadingGuide
                    ? "Downloading..."
                    : guide.size !== null
                      ? `Download How to Use (${mbUp(guide.size)} MB)`
                      : "Download How to Use"}
                </Button>
              ) : (
                <>
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={openingHelp || downloadingGuide}
                    onClick={() => {
                      setOpeningHelp(true);
                      // A failure re-asks what is on disk too: a click that
                      // came before the first answer (How To Use shows until
                      // then) gets the Download button it needed.
                      commands
                        .openHelp()
                        .then((r) => {
                          if (r.status === "error") {
                            toast.error(r.error);
                            void askGuideStatus();
                          }
                        })
                        .catch(() => {
                          toast.error("Could not open the help pages. Settings, Logs has the details.");
                          void askGuideStatus();
                        })
                        .finally(() => setOpeningHelp(false));
                    }}
                  >
                    <IconHelp aria-hidden />
                    {openingHelp ? "Opening" : "How To Use"}
                  </Button>
                  {guide.state === "UpdateAvailable" && (
                    <Button size="sm" variant="outline" disabled={downloadingGuide} onClick={() => void downloadGuide()}>
                      <IconRefresh aria-hidden />
                      {downloadingGuide ? "Downloading..." : "Update Guide"}
                    </Button>
                  )}
                </>
              )}
              {downloadingGuide && (
                <div
                  role="progressbar"
                  aria-label="Downloading How to Use"
                  aria-valuemin={0}
                  aria-valuemax={100}
                  // Omitted, not zero, until the first bytes arrive: an
                  // indeterminate bar is what "not known yet" means.
                  aria-valuenow={guideProgressPercent(guideProgress) ?? undefined}
                  aria-valuetext={guideProgressText(guideProgress) ?? undefined}
                  className="h-1.5 w-24 overflow-hidden rounded-full bg-accent/20"
                >
                  <div
                    className="h-full rounded-full bg-accent transition-[width] duration-300 ease-out"
                    style={{ width: `${guideProgressPercent(guideProgress) ?? 0}%` }}
                  />
                </div>
              )}
              {/* Always mounted, empty when idle, so a screen reader is
                  already listening when the first figure arrives; visually
                  hidden while empty so it takes no room in the row. */}
              <span
                role="status"
                className={cn("text-xs tabular-nums text-muted", !guideProgressText(guideProgress) && "sr-only")}
              >
                {guideProgressText(guideProgress) ?? ""}
              </span>
              <Button
                size="sm"
                variant="outline"
                onClick={() => window.dispatchEvent(new Event(START_TOUR_EVENT))}
              >
                <IconTour aria-hidden />
                Show UI tour
              </Button>
              <Button size="sm" variant="outline" onClick={() => setReporting(true)}>
                <IconBug aria-hidden />
                Report a bug
              </Button>
            </>
          }
        />
      </SettingsCard>
    </>
  );

  // AI tools and Extras sit in the left column, unless the right column has
  // room for two cards across: then they stack beside the cards under the
  // changelog, and move left with them while its history is open.
  const aiToolsCard = (
    <SettingsCard
      title="AI tools"
      data-settings-card="ai-tools"
      className={cn(tiles.spareStack && LEFT_TRACK_WIDTH)}
    >
      <SettingRow
        asLabel
        name="Allow registering AI tools machine-wide"
        description="For a machine without a repository. Writing test cases still needs one."
        control={
          <Switch
            checked={globalAllowed}
            onCheckedChange={(on) => {
              saveGlobalAllowed(on);
              setGlobalAllowed(on);
            }}
            ariaLabel="Allow registering AI tools machine-wide"
          />
        }
      />
    </SettingsCard>
  );

  // Only on a machine where the optional extras are unlocked (a key sequence
  // typed on this screen - see settingsExtras.ts). The heading stays neutral
  // on purpose. Capture mode: the owner's machine can be unlocked, but a shot
  // must never show it - see dev/capture.ts.
  const extrasCard = extrasUnlocked && !isCaptureMode() && (
    <SettingsCard
      title="Extras"
      data-settings-card="extras"
      className={cn(tiles.spareStack && LEFT_TRACK_WIDTH)}
    >
      <SettingRow
        description="Optional extras on this machine. While they are on, Auto Run shows in the sidebar and its tools are offered on the AI Bridge tab."
        control={
          <>
            <Button size="sm" variant="outline" onClick={() => setGameOpen(true)}>
              <IconPlayGame aria-hidden />
              Play the dino game
            </Button>
            <Button size="sm" variant="outline" onClick={() => setConfirmReset(true)}>
              <IconUndo aria-hidden />
              Reset to default
            </Button>
          </>
        }
      />
      {confirmReset && (
        <Modal onClose={() => setConfirmReset(false)} className="w-full max-w-sm space-y-4 p-5">
          <h3 className="text-sm font-semibold text-text">Hide these extras again?</h3>
          <div className="flex justify-end gap-2">
            <Button size="sm" variant="outline" onClick={() => setConfirmReset(false)}>
              Cancel
            </Button>
            <Button size="sm" onClick={resetExtras}>
              Reset
            </Button>
          </div>
        </Modal>
      )}
      {gameOpen && <RunnerGameModal onClose={() => setGameOpen(false)} />}
    </SettingsCard>
  );

  return (
    // Two columns on wide windows; below lg everything stacks into one
    // column: the cards, then the changelog/log panel.
    //
    // On a wide window the right column holds the changelog/log panel with
    // Updates, Backup & transfer and Help & support under it - a collapsed
    // changelog is short, and the space under it used to sit empty. While
    // the changelog's full history is open those three move to the foot of
    // the left column instead, sliding across as it unfolds and back as it
    // folds, timed so its edge never crosses them (useTileLayout). When the
    // right column has room for two cards across, AI tools and Extras stack
    // beside those three instead of sitting below them on the left, and
    // move left with them.
    //
    // Left: the settings, grouped into cards of one row per setting. It stops
    // growing at 32rem - none of its rows get better with more room - and the
    // right panel takes whatever is left, so a wide window turns dead space
    // into visible changelog/log lines rather than margin. The 24rem floor on
    // the right track matters at the lg boundary: without it the fixed left
    // track would claim its full width first and squeeze the panel narrower
    // than an even split.
    <div
      ref={panelRef}
      className="@container grid max-w-lg gap-8 lg:max-w-none lg:grid-cols-[minmax(0,32rem)_minmax(24rem,1fr)] lg:items-start"
    >
      <div className="space-y-4">
      {/* The tour walks the user here and rings this card so the theme is
          picked on the real screen, not on a copy in a card. */}
      <SettingsCard title="Appearance" data-tour="theme" data-settings-card="appearance">
        <SettingRow name="Theme" description="Changes the entire UI palette.">
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
        </SettingRow>
        <SettingRow name="Accent" description="Overrides the theme's accent colour.">
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
        </SettingRow>
      </SettingsCard>

      <SettingsCard title="General" data-settings-card="general">
        <AccountSettings />
        <BackgroundSettings />
        {/* Three levels, one pressed. Only the chosen level's explanation
            shows, under the row - the three used to be stacked as large
            buttons with all three hints at once. */}
        <SettingRow
          name="Azure DevOps request rate"
          description="Azure DevOps limits requests per user, and your browser shares that budget."
          control={
            <div className="flex rounded-md border border-border p-0.5">
              {RATE_LEVELS.map((l) => (
                <button
                  key={l.id}
                  aria-pressed={rate === l.id}
                  className={cn(
                    "rounded px-2 py-1 text-xs transition-colors",
                    rate === l.id ? "bg-accent-soft text-accent" : "text-muted hover:text-text",
                  )}
                  onClick={() => {
                    setRate(l.id);
                    setRateLevel(l.id);
                  }}
                >
                  <span className="label-trim">{l.label}</span>
                </button>
              ))}
            </div>
          }
        >
          <p aria-live="polite" className="text-xs text-muted">
            {selectedRate.hint}
          </p>
        </SettingRow>
        {/* Shown in capture mode too: the help site documents it. */}
        <SettingRow
          asLabel
          name="Enable Advanced Features"
          description="Shows Auto Run and API Templates, and the AI tools that go with them."
          control={
            <Switch
              checked={advancedOn}
              onCheckedChange={toggleAdvanced}
              ariaLabel="Enable Advanced Features"
            />
          }
        />
      </SettingsCard>

      {!tiles.spareStack && aiToolsCard}

      {tiles.placement !== "right" && movingCards}

      {!tiles.spareStack && extrasCard}

      {/* The Module / Preconditions field mapping is auto-detected
          (useFieldRefs ranked match) and deliberately NOT user-editable -
          re-add a "Test case fields" row here if that ever needs a
          manual override. */}

      {/* Default tags used to live here. They moved to Manual Entry, which
          is the only screen that uses them - a setting two screens from
          its effect is one you have to already know exists. */}

      </div>

      {/* The right column: the changelog (or the app log), and on a wide
          window the cards under it while its history is folded. It scrolls
          with the page - it used to be sticky when it held the panel alone,
          but a sticky panel would ride over the cards beneath it. */}
      <div className="space-y-4">
      {/* Masked in the visual regression suite: this panel's content
          changes with every release (and every log line), which would
          otherwise invalidate the Settings golden on each ship. */}
      <section ref={changelogRef} className="space-y-3" data-visual-mask="release-notes">
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
                // Through the layout: with the history open, leaving the
                // changelog brings the cards back under the panel. The
                // pressed one does nothing - a switch that never renders
                // would leave the layout waiting on it.
                onClick={() => rightPanel !== p && tiles.flip(() => setRightPanel(p))}
              >
                <span className="label-trim">{p === "changelog" ? "Changelog" : "Logs"}</span>
              </button>
            ))}
          </div>
        </div>

        {rightPanel === "logs" ? (
          <div key={rightPanel} className="t-panel-in space-y-3">
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
                  copyText(text)
                    .then(() => toast.success("Log copied."))
                    .catch(() => toast.error("Could not copy to clipboard."));
                }}
              >
                <IconCopy aria-hidden />
                Copy log
              </Button>
              <Button
                size="sm"
                variant="outline"
                disabled={!logDir.data}
                onClick={() => {
                  // Rust opens it: the webview's opener permission stops
                  // at URLs, so a direct plugin call here was refused.
                  commands
                    .openAppLogDir()
                    .then((r) => {
                      if (r.status === "error") toast.error(r.error);
                    })
                    .catch(() => toast.error("Could not open the log folder."));
                }}
              >
                <IconBrowse aria-hidden />
                Open log folder
              </Button>
              <Button
                size="sm"
                variant="outline"
                onClick={() => {
                  // Rust opens it, same reason as "Open log folder" above.
                  commands
                    .openActivityLogDir()
                    .then((r) => {
                      if (r.status === "error") toast.error(r.error);
                    })
                    .catch(() => toast.error("Could not open the activity folder."));
                }}
              >
                <IconBrowse aria-hidden />
                Open activity folder
              </Button>
            </div>
            <div className="max-h-72 space-y-0.5 overflow-y-auto rounded-md border border-border p-3 lg:max-h-[50vh]">
              {shownLogs.length === 0 ? (
                <p className="text-xs text-faint">Nothing logged yet this session.</p>
              ) : (
                // Coloured the way VS Code's Log mode colours a log: the
                // level tag by severity, and the numbers, hosts and ids the
                // eye scans for picked out from the prose (lib/logSyntax).
                shownLogs.map((l, i) => <LogLine key={i} at={l.at} level={l.level} message={l.message} />)
              )}
            </div>
          </div>
        ) : (
          <div key={rightPanel} className="t-panel-in space-y-4 rounded-md border border-border p-3">
          {changelog.slice(0, 1).map((e) => (
            <ChangelogVersion key={e.version} entry={e} />
          ))}
          {/* Earlier versions unfold in place, in a box of their own so a
              long history scrolls without pushing the panel's own edge
              off the screen. */}
          <Collapse open={tiles.expanded} onGrow={tiles.onGrow} onShrink={tiles.onShrink}>
            <div id="changelog-history" className="max-h-[50vh] space-y-4 overflow-y-auto pr-1">
              {changelog.slice(1).map((e) => (
                <ChangelogVersion key={e.version} entry={e} />
              ))}
            </div>
          </Collapse>
          {changelog.length > 1 && (
            <Button
              size="sm"
              variant="ghost"
              aria-expanded={tiles.expanded}
              aria-controls="changelog-history"
              onClick={tiles.toggle}
            >
              {tiles.expanded ? "Show less" : `Show more (${changelog.length - 1} earlier versions)`}
            </Button>
          )}
          </div>
        )}
      </section>

      {tiles.placement === "right" &&
        (tiles.spareStack ? (
          // Room for two across: a second stack beside the first, both
          // top-aligned, 1rem apart.
          <div className="flex items-start gap-4">
            <div className="space-y-4">{movingCards}</div>
            <div className="space-y-4">
              {aiToolsCard}
              {extrasCard}
            </div>
          </div>
        ) : (
          movingCards
        ))}
      </div>

      {/* Sizing and padding belong on the Modal, not inside it: the panel
          itself is only a bordered surface, so a child with no className
          sat flush against the border and shrink-wrapped to its text.
          Same shape as every other dialog in the app. */}
      {reporting && (
        <Modal
          onClose={() => setReporting(false)}
          className="flex max-h-[85vh] w-full max-w-lg flex-col gap-4 p-5"
        >
          <div className="shrink-0 space-y-1.5">
            <h2 className="text-sm font-semibold text-text">Report a bug in this app</h2>
            <p className="text-sm leading-relaxed text-muted">
              This opens a prefilled issue on GitHub for you to check and submit -
              nothing is sent from the app. Your organization, project and work
              item names are removed from the log first.
            </p>
          </div>
          {/* Header and description are SEPARATE boxes: with one box, the
              first line of whatever was typed silently became the issue
              title. Leaving the header blank still derives one from the
              description, so the quick path keeps working. */}
          <Input
            aria-label="Bug title"
            className="w-full shrink-0"
            autoFocus
            placeholder="One line for the issue list (optional - taken from the description if blank)"
            value={bugTitle}
            onChange={(e) => setBugTitle(e.target.value)}
          />
          <Textarea
            aria-label="What happened"
            className="h-32 w-full shrink-0"
            placeholder="What were you doing, and what happened instead?"
            value={bugText}
            onChange={(e) => setBugText(e.target.value)}
          />
          <div className="flex shrink-0 justify-end gap-2">
              <Button variant="ghost" size="sm" onClick={() => setReporting(false)}>
                <IconCancel aria-hidden />
                Cancel
              </Button>
              <Button
                size="sm"
                onClick={() => {
                  void commands
                    .prepareBugReport(bugTitle, bugText, org, project)
                    .then((r) => {
                      if (r.status === "error") {
                        toast.error(`Could not prepare the report: ${r.error}`);
                        return;
                      }
                      setReporting(false);
                      setBugTitle("");
                      setBugText("");
                      void openUrl(r.data.url);
                      // The log is a separate file because GitHub cannot take
                      // an attachment from a URL - opening its folder makes the
                      // drag the reporter has to do a short one.
                      void commands.openAppLogDir();
                      toast.info("Drag the tcm-bug-report log onto the issue before submitting.");
                    })
                    .catch(() => toast.error("Could not prepare the report."));
                }}
              >
                <IconBug aria-hidden />
                Open the issue
              </Button>
          </div>
        </Modal>
      )}
    </div>
  );
}

/** One version's notes, as the changelog lists them. */
function ChangelogVersion({ entry }: { entry: ChangelogEntry }) {
  return (
    <div className="space-y-1.5">
      <ChangelogVersionTitle entry={entry} />
      <ul className="list-disc space-y-1 pl-4 text-xs text-muted">
        {entry.items.map((item, i) => (
          <li key={i}>{item}</li>
        ))}
      </ul>
    </div>
  );
}

/** One app log line, coloured. Memoised: the view refetches every two
 * seconds, and re-colouring up to two thousand unchanged lines each time
 * is work nobody sees. */
const LogLine = memo(function LogLine({ at, level, message }: { at: string; level: string; message: string }) {
  return (
    <p className="id-mono flex gap-2 text-[11px] leading-relaxed">
      <span className="shrink-0 text-muted">{at}</span>
      <span className={cn("shrink-0 uppercase", LOG_KIND_CLASS[levelOf(level) ?? "info"])}>[{level}]</span>
      <span className="min-w-0 break-words">
        {tokenizeLog(message).map((t, j) => (
          <span key={j} className={LOG_KIND_CLASS[t.kind]}>
            {t.text}
          </span>
        ))}
      </span>
    </p>
  );
});
