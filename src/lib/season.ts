import { isCaptureMode } from "../dev/capture";

/** Seasonal themes: on a season's dates an install is moved onto its theme
 * once, then put back on what it had. One record in localStorage per season
 * (per year, for a yearly one) tracks it. The raw theme/accent keys are used
 * directly (not theme.ts) so the two files do not import each other, and so
 * the switch does not count as a pick: it never touches the sun/moon
 * toggle's dark theme either. A gated season only runs while the gate is
 * open (runSeason's gateOpen); while the gate's answer is not known yet
 * (gateKnown false) a gated season is left exactly as it is. */

export const SEASON_KEY = "tcm-v2-season-halloween-2026";
const THEME_KEY = "tcm-v2-theme-id";
const ACCENT_KEY = "tcm-v2-accent";
const WEEK = 7 * 24 * 60 * 60 * 1000;
const PREFIX = "tcm-v2-season-";

type Season = {
  id: string;
  theme: string;
  gated?: boolean;
  /** Every record key of this season starts with this. */
  prefix: string;
  /** The record key while `now` is inside the switch window, else null. */
  keyAt: (now: number) => string | null;
  /** When a record's switch ends. */
  endOf: (key: string, rec: SeasonRecord) => number;
  /** When a record's switch window opened (the clock may since have moved
   * back before it). */
  startOf?: (key: string) => number;
};

const SEASONS: Season[] = [
  {
    // 2026 only, switched before 1 November (local time); a week per machine.
    id: "halloween",
    theme: "halloween",
    prefix: SEASON_KEY,
    keyAt: (now) => (now < new Date(2026, 10, 1).getTime() ? SEASON_KEY : null),
    endOf: (_key, rec) => rec.switchedAt + WEEK,
  },
  {
    // Every year, 18 December to 1 January (local time); the end is fixed.
    id: "christmas",
    theme: "christmas",
    gated: true,
    prefix: `${PREFIX}christmas-`,
    keyAt: (now) => {
      const d = new Date(now);
      return d.getMonth() === 11 && d.getDate() >= 18 ? `${PREFIX}christmas-${d.getFullYear()}` : null;
    },
    endOf: (key) => new Date(Number(key.slice(key.lastIndexOf("-") + 1)) + 1, 0, 1).getTime(),
    startOf: (key) => new Date(Number(key.slice(key.lastIndexOf("-") + 1)), 11, 18).getTime(),
  },
];

/** True for any season record key: a machine's own, never imported. */
export function isSeasonKey(k: string): boolean {
  return SEASONS.some((s) => k.startsWith(s.prefix));
}

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
function readRecord(key: string): SeasonRecord | null {
  try {
    const raw = localStorage.getItem(key);
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

function writeRecord(key: string, r: SeasonRecord): boolean {
  try {
    localStorage.setItem(key, JSON.stringify(r));
    return true;
  } catch {
    return false;
  }
}

function restore(key: string, value: string | null) {
  if (value === null) localStorage.removeItem(key);
  else localStorage.setItem(key, value);
}

/** Every season record on this machine, as [season, key, record]. */
function allRecords(): [Season, string, SeasonRecord][] {
  const out: [Season, string, SeasonRecord][] = [];
  for (let i = 0; i < localStorage.length; i++) {
    const key = localStorage.key(i);
    const season = key && SEASONS.find((s) => key.startsWith(s.prefix));
    const rec = season && key ? readRecord(key) : null;
    if (season && key && rec) out.push([season, key, rec]);
  }
  return out;
}

const isActive = (r: SeasonRecord) => r.themeState === "active" || r.accentState === "active";

export function runSeason(now: number = Date.now(), opts: { gateOpen?: boolean; gateKnown?: boolean } = {}): void {
  try {
    // A docs screenshot run (dev/capture.ts) keeps its pinned themes.
    if (isCaptureMode()) return;
    const gateKnown = opts.gateKnown ?? true;
    // Restores first: a record that ended (or whose season the gate now
    // holds back) puts the person's look back before anything switches.
    for (const [season, key, rec] of allRecords()) {
      if (!isActive(rec)) continue;
      // Not asked yet: a gated record is neither ended nor held back.
      if (season.gated && !gateKnown) continue;
      const held = season.gated && !opts.gateOpen;
      // A clock moved back before the window (a machine set to a future
      // year) ends the record as well.
      const early = season.startOf !== undefined && now < season.startOf(key);
      if (!held && !early && now < season.endOf(key, rec)) continue;
      // Only what is still the season's own look goes back.
      if (rec.themeState === "active" && localStorage.getItem(THEME_KEY) === season.theme) {
        restore(THEME_KEY, rec.prevTheme);
      }
      if (rec.accentState === "active" && localStorage.getItem(ACCENT_KEY) === "default") {
        restore(ACCENT_KEY, rec.prevAccent);
      }
      writeRecord(key, { ...rec, themeState: "done", accentState: "done" });
    }
    // One season at a time: none switches over a record that is still active.
    for (const season of SEASONS) {
      if (season.gated && (!gateKnown || !opts.gateOpen)) continue;
      const key = season.keyAt(now);
      if (!key || readRecord(key)) continue;
      if (allRecords().some(([, , r]) => isActive(r))) return;
      // The record goes first: without it nothing could be restored.
      const saved = writeRecord(key, {
        switchedAt: now,
        prevTheme: localStorage.getItem(THEME_KEY),
        prevAccent: localStorage.getItem(ACCENT_KEY),
        themeState: "active",
        accentState: "active",
      });
      if (!saved) return;
      localStorage.setItem(THEME_KEY, season.theme);
      localStorage.setItem(ACCENT_KEY, "default");
      return;
    }
  } catch {
    // storage unavailable: no season
  }
}

/** A real accent pick during a season ends only the accent restore. */
export function seasonAccentPicked(): void {
  try {
    for (const [, key, rec] of allRecords()) {
      if (rec.accentState === "active") writeRecord(key, { ...rec, accentState: "done" });
    }
  } catch {
    // storage unavailable
  }
}

/** A real theme pick during a season ends the theme restore. Picking the
 * season's own theme keeps the whole look; picking anything else also puts
 * the accent back now, since the season's accent was only for it. */
export function seasonThemePicked(choice: string): void {
  try {
    for (const [season, key, rec] of allRecords()) {
      const next = { ...rec, themeState: "done" as const };
      if (rec.accentState === "active") {
        if (choice !== season.theme) restore(ACCENT_KEY, rec.prevAccent);
        next.accentState = "done";
      }
      if (isActive(rec)) writeRecord(key, next);
    }
  } catch {
    // storage unavailable
  }
}
