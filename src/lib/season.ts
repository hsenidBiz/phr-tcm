/** The Halloween 2026 switch: every install is moved onto the Halloween
 * theme once, for a week, then put back on what it had. One record in
 * localStorage tracks it. The raw theme/accent keys are used directly (not
 * theme.ts) so the two files do not import each other, and so the switch
 * does not count as a pick: it never touches the sun/moon toggle's dark
 * theme either. */

export const SEASON_KEY = "tcm-v2-season-halloween-2026";
const THEME_KEY = "tcm-v2-theme-id";
const ACCENT_KEY = "tcm-v2-accent";
const CAPTURE_KEY = "tcm-v2-dev-capture";
const WEEK = 7 * 24 * 60 * 60 * 1000;

export type SeasonRecord = {
  switchedAt: number;
  prevTheme: string | null;
  prevAccent: string | null;
  state: "active" | "done";
};

/** null = no record; a record that cannot be read counts as done, so a
 * broken one never switches anybody. */
function readRecord(): SeasonRecord | null {
  try {
    const raw = localStorage.getItem(SEASON_KEY);
    if (raw === null) return null;
    const r = JSON.parse(raw) as SeasonRecord;
    if (r && typeof r === "object" && r.state === "active" && typeof r.switchedAt === "number") return r;
  } catch {
    // fall through
  }
  return { switchedAt: 0, prevTheme: null, prevAccent: null, state: "done" };
}

function writeRecord(r: SeasonRecord) {
  try {
    localStorage.setItem(SEASON_KEY, JSON.stringify(r));
  } catch {
    // storage unavailable: nothing to remember
  }
}

function restore(key: string, value: string | null) {
  if (value === null) localStorage.removeItem(key);
  else localStorage.setItem(key, value);
}

export function runSeason(now: number = Date.now()): void {
  try {
    // A docs screenshot run (dev/capture.ts) keeps its pinned themes.
    if (localStorage.getItem(CAPTURE_KEY) !== null) return;
    const rec = readRecord();
    if (!rec) {
      // Launches on or after 1 November (local time) never switch.
      if (now >= new Date(2026, 10, 1).getTime()) return;
      writeRecord({
        switchedAt: now,
        prevTheme: localStorage.getItem(THEME_KEY),
        prevAccent: localStorage.getItem(ACCENT_KEY),
        state: "active",
      });
      localStorage.setItem(THEME_KEY, "halloween");
      localStorage.setItem(ACCENT_KEY, "default");
    } else if (rec.state === "active" && now - rec.switchedAt >= WEEK) {
      restore(THEME_KEY, rec.prevTheme);
      restore(ACCENT_KEY, rec.prevAccent);
      writeRecord({ ...rec, state: "done" });
    }
  } catch {
    // storage unavailable: no season
  }
}

/** A real pick during the week ends the restore: the person's choice
 * stands. No record, or one already done, is left alone. */
export function cancelSeasonRestore(): void {
  try {
    const rec = readRecord();
    if (rec && rec.state === "active") writeRecord({ ...rec, state: "done" });
  } catch {
    // storage unavailable
  }
}
