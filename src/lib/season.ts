import { isCaptureMode } from "../dev/capture";

/** The Halloween 2026 switch: every install is moved onto the Halloween
 * theme once, for a week, then put back on what it had. One record in
 * localStorage tracks it. The raw theme/accent keys are used directly (not
 * theme.ts) so the two files do not import each other, and so the switch
 * does not count as a pick: it never touches the sun/moon toggle's dark
 * theme either. */

export const SEASON_KEY = "tcm-v2-season-halloween-2026";
const THEME_KEY = "tcm-v2-theme-id";
const ACCENT_KEY = "tcm-v2-accent";
const WEEK = 7 * 24 * 60 * 60 * 1000;

export type SeasonRecord = {
  switchedAt: number;
  prevTheme: string | null;
  prevAccent: string | null;
  themeState: "active" | "done";
  accentState: "active" | "done";
};

const DONE: SeasonRecord = { switchedAt: 0, prevTheme: null, prevAccent: null, themeState: "done", accentState: "done" };

/** null = no record; a record that cannot be read counts as done, so a
 * broken one never switches anybody. The first shape had one `state` for
 * both halves: it maps onto both. */
function readRecord(): SeasonRecord | null {
  try {
    const raw = localStorage.getItem(SEASON_KEY);
    if (raw === null) return null;
    const r = JSON.parse(raw) as Partial<SeasonRecord> & { state?: string };
    if (r && typeof r === "object" && typeof r.switchedAt === "number") {
      const fallback = r.state === "active" ? "active" : "done";
      return {
        switchedAt: r.switchedAt,
        prevTheme: r.prevTheme ?? null,
        prevAccent: r.prevAccent ?? null,
        themeState: r.themeState ?? fallback,
        accentState: r.accentState ?? fallback,
      };
    }
  } catch {
    // fall through
  }
  return DONE;
}

function writeRecord(r: SeasonRecord): boolean {
  try {
    localStorage.setItem(SEASON_KEY, JSON.stringify(r));
    return true;
  } catch {
    return false;
  }
}

function restore(key: string, value: string | null) {
  if (value === null) localStorage.removeItem(key);
  else localStorage.setItem(key, value);
}

export function runSeason(now: number = Date.now()): void {
  try {
    // A docs screenshot run (dev/capture.ts) keeps its pinned themes.
    if (isCaptureMode()) return;
    const rec = readRecord();
    if (!rec) {
      // Launches on or after 1 November (local time) never switch.
      if (now >= new Date(2026, 10, 1).getTime()) return;
      // The record goes first: without it nothing could be restored.
      const saved = writeRecord({
        switchedAt: now,
        prevTheme: localStorage.getItem(THEME_KEY),
        prevAccent: localStorage.getItem(ACCENT_KEY),
        themeState: "active",
        accentState: "active",
      });
      if (!saved) return;
      localStorage.setItem(THEME_KEY, "halloween");
      localStorage.setItem(ACCENT_KEY, "default");
    } else if ((rec.themeState === "active" || rec.accentState === "active") && now - rec.switchedAt >= WEEK) {
      // Only what is still the week's own look goes back.
      if (rec.themeState === "active" && localStorage.getItem(THEME_KEY) === "halloween") {
        restore(THEME_KEY, rec.prevTheme);
      }
      if (rec.accentState === "active" && localStorage.getItem(ACCENT_KEY) === "default") {
        restore(ACCENT_KEY, rec.prevAccent);
      }
      writeRecord({ ...rec, themeState: "done", accentState: "done" });
    }
  } catch {
    // storage unavailable: no season
  }
}

/** A real accent pick during the week ends only the accent restore. */
export function seasonAccentPicked(): void {
  try {
    const rec = readRecord();
    if (rec && rec.accentState === "active") writeRecord({ ...rec, accentState: "done" });
  } catch {
    // storage unavailable
  }
}

/** A real theme pick during the week ends the theme restore. Picking
 * Halloween keeps the whole look; picking anything else also puts the
 * accent back now, since the orange one was only for Halloween. */
export function seasonThemePicked(choice: string): void {
  try {
    const rec = readRecord();
    if (!rec) return;
    const next = { ...rec, themeState: "done" as const };
    if (rec.accentState === "active") {
      if (choice !== "halloween") restore(ACCENT_KEY, rec.prevAccent);
      next.accentState = "done";
    }
    if (rec.themeState === "active" || rec.accentState === "active") writeRecord(next);
  } catch {
    // storage unavailable
  }
}
