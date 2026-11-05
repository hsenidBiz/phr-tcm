import { act, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import Settings from "../screens/Settings";
import { extrasKnown, hydrateExtras, resetExtrasStore, setExtrasForTest, subscribeExtras } from "./extras";
import { SEASON_KEY } from "./season";
import { getAccent, getThemeChoice, initRunnerTheme, initTheme, setAccent, setThemeChoice, visibleThemes } from "./theme";

const OCT29 = new Date(2026, 9, 29, 12).getTime();
const DAY = 86_400_000;
const root = document.documentElement;

afterEach(() => {
  resetExtrasStore();
  clearMocks();
  vi.restoreAllMocks();
  vi.unstubAllEnvs();
  localStorage.clear();
  root.classList.remove("dark");
  root.removeAttribute("data-theme");
  root.removeAttribute("data-accent");
});

function ocean() {
  localStorage.setItem("tcm-v2-theme-id", "ocean");
  localStorage.setItem("tcm-v2-accent", "blue");
}

describe("Halloween, held behind the gate", () => {
  beforeEach(() => setExtrasForTest(true));

  test("T1.a: an install is switched onto Halloween with the theme's own accent", () => {
    ocean();
    initTheme(OCT29);
    expect(getThemeChoice()).toBe("halloween");
    expect(getAccent()).toBe("default");
    expect(root.getAttribute("data-theme")).toBe("halloween");
    expect(root.classList.contains("dark")).toBe(true);
  });

  test("T1.b: launches on or after 1 November never switch", () => {
    ocean();
    initTheme(new Date(2026, 10, 1).getTime());
    expect(getThemeChoice()).toBe("ocean");
    expect(localStorage.getItem(SEASON_KEY)).toBeNull();
  });

  test("T1.c: the week ends with the person's own look back", () => {
    ocean();
    initTheme(OCT29);
    initTheme(OCT29 + 6 * DAY);
    expect(getThemeChoice()).toBe("halloween");
    initTheme(OCT29 + 7 * DAY);
    expect(getThemeChoice()).toBe("ocean");
    expect(getAccent()).toBe("blue");
    initTheme(OCT29 + 8 * DAY);
    expect(getThemeChoice()).toBe("ocean");
  });

  test("T1.d: an accent pick ends only the accent restore: the theme still goes back", () => {
    ocean();
    initTheme(OCT29);
    setAccent("rose");
    initTheme(OCT29 + 7 * DAY);
    expect(getThemeChoice()).toBe("ocean");
    expect(getAccent()).toBe("rose");
  });

  test("T1.d2: leaving Halloween early puts the accent back at once", () => {
    ocean();
    initTheme(OCT29);
    setThemeChoice("ocean");
    expect(getAccent()).toBe("blue");
    expect(root.getAttribute("data-accent")).toBe("blue");
    initTheme(OCT29 + 8 * DAY);
    expect(getThemeChoice()).toBe("ocean");
    expect(getAccent()).toBe("blue");
  });

  test("T1.d3: someone who never chose an accent gets violet back on leaving", () => {
    initTheme(OCT29);
    setThemeChoice("slate");
    expect(localStorage.getItem("tcm-v2-accent")).toBeNull();
    expect(getAccent()).toBe("violet");
    expect(getThemeChoice()).toBe("slate");
  });

  test("T1.d4: choosing Halloween on purpose keeps the whole look for good", () => {
    ocean();
    initTheme(OCT29);
    setThemeChoice("halloween");
    initTheme(OCT29 + 8 * DAY);
    expect(getThemeChoice()).toBe("halloween");
    expect(getAccent()).toBe("default");
  });

  test("T1.d5: a record of the first shape still restores both halves", () => {
    localStorage.setItem("tcm-v2-theme-id", "halloween");
    localStorage.setItem("tcm-v2-accent", "default");
    localStorage.setItem(
      SEASON_KEY,
      JSON.stringify({ switchedAt: OCT29, prevTheme: "ocean", prevAccent: "blue", state: "active" }),
    );
    initTheme(OCT29 + 7 * DAY);
    expect(getThemeChoice()).toBe("ocean");
    expect(getAccent()).toBe("blue");
  });

  test("T1.e: someone who never picked anything lands back on Light and violet", () => {
    initTheme(OCT29);
    expect(getThemeChoice()).toBe("halloween");
    initTheme(OCT29 + 7 * DAY);
    expect(localStorage.getItem("tcm-v2-theme-id")).toBeNull();
    expect(localStorage.getItem("tcm-v2-accent")).toBeNull();
    expect(getThemeChoice()).toBe("light");
    expect(getAccent()).toBe("violet");
  });

  test("T1.f: a second launch changes nothing, a restore is final, a later pick sticks", () => {
    ocean();
    initTheme(OCT29);
    const record = localStorage.getItem(SEASON_KEY);
    initTheme(OCT29 + DAY);
    expect(localStorage.getItem(SEASON_KEY)).toBe(record);
    expect(getThemeChoice()).toBe("halloween");
    initTheme(OCT29 + 7 * DAY);
    initTheme(OCT29 + 7 * DAY + DAY);
    expect(getThemeChoice()).toBe("ocean");
    setThemeChoice("slate");
    initTheme(OCT29 + 9 * DAY);
    expect(getThemeChoice()).toBe("slate");
  });

  test("T1.g: the switch leaves the sun/moon toggle's dark theme alone", () => {
    localStorage.setItem("tcm-v2-theme-dark", "ocean");
    initTheme(OCT29);
    expect(localStorage.getItem("tcm-v2-theme-dark")).toBe("ocean");
  });

  test("T1.h: a broken record never switches anyone", () => {
    localStorage.setItem(SEASON_KEY, "{not json");
    initTheme(OCT29);
    expect(getThemeChoice()).toBe("light");
  });

  test("T1.i: a docs screenshot run keeps its pinned theme", () => {
    vi.stubEnv("VITE_DOCS_CAPTURE", "1");
    localStorage.setItem("tcm-v2-dev-capture", "on");
    localStorage.setItem("tcm-v2-theme-id", "light");
    initTheme(OCT29);
    expect(getThemeChoice()).toBe("light");
    expect(localStorage.getItem(SEASON_KEY)).toBeNull();
  });

  test("T1.j: nothing is switched unless the record is saved", () => {
    ocean();
    const real = Storage.prototype.setItem;
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(function (this: Storage, k: string, v: string) {
      if (k === SEASON_KEY) throw new Error("quota");
      real.call(this, k, v);
    });
    initTheme(OCT29);
    expect(getThemeChoice()).toBe("ocean");
    expect(getAccent()).toBe("blue");
  });

  test("T1.k: at day 7 only what is still the week's look goes back", () => {
    ocean();
    initTheme(OCT29);
    localStorage.setItem("tcm-v2-theme-id", "midnight");
    localStorage.setItem("tcm-v2-accent", "rose");
    initTheme(OCT29 + 7 * DAY);
    expect(getThemeChoice()).toBe("midnight");
    expect(getAccent()).toBe("rose");
    expect(JSON.parse(localStorage.getItem(SEASON_KEY) ?? "{}")).toMatchObject({
      themeState: "done",
      accentState: "done",
    });
  });

  test("T1.l: with the gate closed it never switches, and a stored pick reads as Light", () => {
    ocean();
    setExtrasForTest(false);
    initTheme(OCT29);
    expect(getThemeChoice()).toBe("ocean");
    expect(localStorage.getItem(SEASON_KEY)).toBeNull();
    localStorage.setItem("tcm-v2-theme-id", "halloween");
    expect(getThemeChoice()).toBe("light");
    expect(localStorage.getItem("tcm-v2-theme-id")).toBe("halloween");
    setExtrasForTest(true);
    expect(getThemeChoice()).toBe("halloween");
  });

  test("T1.m: the picker lists it only with the gate open, on any date", () => {
    for (const d of [OCT29, new Date(2026, 11, 20).getTime(), new Date(2027, 5, 1).getTime()]) {
      expect(visibleThemes(false, d).map((t) => t.id)).not.toContain("halloween");
    }
    expect(visibleThemes(true).map((t) => t.id)).toContain("halloween");
  });
});

describe("Christmas, offered to everyone from 7 to 31 December", () => {
  const xmasKey = (y: number) => `tcm-v2-season-christmas-${y}`;
  const at = (y: number, m: number, d: number, h = 0, min = 0) => new Date(y, m - 1, d, h, min, 0).getTime();
  const DEC7 = at(2026, 12, 7);
  const JAN1 = at(2027, 1, 1);

  // getThemeChoice reads the clock itself, so the clock moves with `now`.
  beforeEach(() => vi.useFakeTimers({ toFake: ["Date"] }));
  afterEach(() => vi.useRealTimers());
  function go(now: number) {
    vi.setSystemTime(now);
    initTheme(now);
  }

  test("T2.a: a fresh machine with the gate closed is switched on 7 December and on 31 December 23:00", () => {
    ocean();
    setExtrasForTest(false);
    go(DEC7);
    expect(getThemeChoice()).toBe("christmas");
    expect(getAccent()).toBe("default");
    expect(root.getAttribute("data-theme")).toBe("christmas");
    expect(root.classList.contains("dark")).toBe(true);
    expect(localStorage.getItem(xmasKey(2026))).not.toBeNull();

    localStorage.clear();
    ocean();
    go(at(2026, 12, 31, 23));
    expect(getThemeChoice()).toBe("christmas");
    expect(localStorage.getItem(xmasKey(2026))).not.toBeNull();
  });

  test("T2.b: it does not switch on 6 December 23:59 or on 1 January 00:00", () => {
    ocean();
    setExtrasForTest(false);
    go(at(2026, 12, 6, 23, 59));
    expect(getThemeChoice()).toBe("ocean");
    expect(localStorage.getItem(xmasKey(2026))).toBeNull();
    go(JAN1);
    expect(getThemeChoice()).toBe("ocean");
    expect(localStorage.getItem(xmasKey(2026))).toBeNull();
    expect(localStorage.getItem(xmasKey(2027))).toBeNull();
  });

  test("T2.c: a record switched in December is restored at 1 January 00:00 with the gate closed", () => {
    ocean();
    setExtrasForTest(false);
    go(DEC7);
    go(at(2026, 12, 31, 23, 59));
    expect(getThemeChoice()).toBe("christmas");
    go(JAN1);
    expect(getThemeChoice()).toBe("ocean");
    expect(getAccent()).toBe("blue");
    go(at(2027, 1, 2));
    expect(getThemeChoice()).toBe("ocean");

    // A late first launch after the end restores as well.
    localStorage.clear();
    ocean();
    go(DEC7);
    go(at(2027, 1, 10, 9));
    expect(getThemeChoice()).toBe("ocean");
    expect(getAccent()).toBe("blue");
  });

  test("T2.d: next year's 7 December switches again, under a new key", () => {
    ocean();
    setExtrasForTest(false);
    go(DEC7);
    go(JAN1);
    expect(getThemeChoice()).toBe("ocean");
    go(at(2027, 12, 7));
    expect(getThemeChoice()).toBe("christmas");
    expect(localStorage.getItem(xmasKey(2027))).not.toBeNull();
    expect(localStorage.getItem(xmasKey(2026))).not.toBeNull();
  });

  test("T2.e: the gate being open or closed changes nothing in the window", () => {
    ocean();
    setExtrasForTest(true);
    go(DEC7);
    setExtrasForTest(false);
    go(at(2026, 12, 20));
    expect(getThemeChoice()).toBe("christmas");
    setExtrasForTest(true);
    go(at(2026, 12, 21));
    expect(getThemeChoice()).toBe("christmas");
  });

  test("T2.f: picking another theme cancels the restore", () => {
    ocean();
    setExtrasForTest(false);
    go(DEC7);
    setThemeChoice("slate");
    expect(getAccent()).toBe("blue");
    go(at(2027, 1, 2));
    expect(getThemeChoice()).toBe("slate");
  });

  test("T2.g: choosing it on purpose keeps the look past the end, with the gate open", () => {
    ocean();
    setExtrasForTest(true);
    go(DEC7);
    setThemeChoice("christmas");
    go(at(2027, 1, 5));
    expect(getThemeChoice()).toBe("christmas");
    expect(getAccent()).toBe("default");
  });

  test("T2.h: a stored pick is worn through 31 December and reads as Light on 1 January, gate closed", () => {
    localStorage.setItem("tcm-v2-theme-id", "christmas");
    setExtrasForTest(false);
    vi.setSystemTime(at(2026, 12, 31, 23, 59));
    expect(getThemeChoice()).toBe("christmas");
    vi.setSystemTime(JAN1);
    expect(getThemeChoice()).toBe("light");
    go(at(2027, 3, 1));
    expect(root.getAttribute("data-theme")).toBeNull();
    expect(localStorage.getItem("tcm-v2-theme-id")).toBe("christmas");
    setExtrasForTest(true);
    expect(getThemeChoice()).toBe("christmas");
  });

  test("T2.i: an active Halloween record blocks a second season", () => {
    ocean();
    setExtrasForTest(true);
    go(OCT29);
    expect(getThemeChoice()).toBe("halloween");
    // Synthetic: a machine whose first week is still running on 7 December.
    const rec = JSON.parse(localStorage.getItem(SEASON_KEY) ?? "{}");
    localStorage.setItem(SEASON_KEY, JSON.stringify({ ...rec, switchedAt: DEC7 - DAY }));
    go(DEC7);
    expect(getThemeChoice()).toBe("halloween");
    expect(localStorage.getItem(xmasKey(2026))).toBeNull();
  });

  test("T2.j: a docs screenshot run is never switched", () => {
    vi.stubEnv("VITE_DOCS_CAPTURE", "1");
    localStorage.setItem("tcm-v2-dev-capture", "on");
    setExtrasForTest(false);
    go(DEC7);
    expect(localStorage.getItem(xmasKey(2026))).toBeNull();
  });

  test("T2.k: the picker offers it from 7 to 31 December for everyone, never in a capture run", () => {
    const ids = (open: boolean, now: number) => visibleThemes(open, now).map((t) => t.id);
    expect(ids(false, DEC7)).toContain("christmas");
    expect(ids(false, at(2026, 12, 31, 23, 59))).toContain("christmas");
    expect(ids(false, at(2026, 12, 6, 23, 59))).not.toContain("christmas");
    expect(ids(false, JAN1)).not.toContain("christmas");
    expect(ids(true, JAN1)).toContain("christmas");
    vi.stubEnv("VITE_DOCS_CAPTURE", "1");
    localStorage.setItem("tcm-v2-dev-capture", "on");
    expect(ids(false, DEC7)).not.toContain("christmas");
    expect(ids(true, DEC7)).not.toContain("christmas");
  });

  test("T2.l: Settings shows the card with a Limited time line in the window, and no Halloween card", () => {
    mockIPC(() => undefined);
    setExtrasForTest(false);
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const ui = (
      <QueryClientProvider client={qc}>
        <Settings org="acme" project="Web" homeProject="Web" onHomeProjectChange={() => {}} onAllProjectsChange={() => {}} />
      </QueryClientProvider>
    );
    vi.setSystemTime(at(2026, 11, 20));
    const { unmount } = render(ui);
    expect(screen.queryByLabelText("Theme Christmas")).toBeNull();
    expect(screen.queryByLabelText("Theme Halloween")).toBeNull();
    expect(screen.queryByText("Limited time")).toBeNull();
    unmount();

    vi.setSystemTime(DEC7);
    render(ui);
    expect(screen.getByLabelText("Theme Christmas")).toBeTruthy();
    expect(screen.getByText("Limited time")).toBeTruthy();
    expect(screen.queryByLabelText("Theme Halloween")).toBeNull();
    act(() => setExtrasForTest(true));
    expect(screen.getByLabelText("Theme Halloween")).toBeTruthy();
  });

  test("T2.m: a record whose window starts after now (the clock moved back) is restored and closed", () => {
    ocean();
    setExtrasForTest(false);
    go(at(2027, 12, 20));
    expect(getThemeChoice()).toBe("christmas");
    expect(localStorage.getItem(xmasKey(2027))).not.toBeNull();
    go(at(2027, 12, 21));
    expect(getThemeChoice()).toBe("christmas");
    // The clock is corrected to 2026: the 2027 record is not in its window.
    go(at(2026, 11, 20));
    expect(getThemeChoice()).toBe("ocean");
    expect(JSON.parse(localStorage.getItem(xmasKey(2027)) ?? "{}").themeState).toBe("done");
  });
});

describe("a gated season while the gate's answer has not come yet (Halloween)", () => {
  const OCT30 = OCT29 + DAY;

  // An unlocked machine switched on 29 October, then restarted: the extras
  // store is "not known" again until Rust answers.
  function restart() {
    resetExtrasStore();
    root.classList.remove("dark");
    root.removeAttribute("data-theme");
    root.removeAttribute("data-accent");
  }
  function switched() {
    ocean();
    setExtrasForTest(true);
    initTheme(OCT29);
    expect(getThemeChoice()).toBe("halloween");
    restart();
  }
  const record = () => JSON.parse(localStorage.getItem(SEASON_KEY) ?? "{}");

  test("T3.a: a restart with the flag unknown leaves the season on; an open answer keeps it", async () => {
    switched();
    expect(extrasKnown()).toBe(false);
    const stop = subscribeExtras(() => initTheme(OCT30));
    initTheme(OCT30);
    expect(getThemeChoice()).toBe("halloween");
    expect(root.getAttribute("data-theme")).toBe("halloween");
    expect(record().themeState).toBe("active");
    mockIPC((cmd) => (cmd === "get_extras_unlocked" ? true : undefined));
    await hydrateExtras();
    stop();
    expect(extrasKnown()).toBe(true);
    expect(getThemeChoice()).toBe("halloween");
    expect(root.getAttribute("data-theme")).toBe("halloween");
    expect(record().themeState).toBe("active");
    expect(record().accentState).toBe("active");
  });

  test("T3.b: the same restart where Rust answers closed restores the look", async () => {
    switched();
    const stop = subscribeExtras(() => initTheme(OCT30));
    initTheme(OCT30);
    expect(getThemeChoice()).toBe("halloween");
    mockIPC((cmd) => (cmd === "get_extras_unlocked" ? false : undefined));
    await hydrateExtras();
    stop();
    expect(getThemeChoice()).toBe("ocean");
    expect(getAccent()).toBe("blue");
    expect(root.getAttribute("data-theme")).toBe("ocean");
    expect(record().themeState).toBe("done");
  });

  test("T3.c: a stored manual pick shows while unknown, and reads as light once the answer is closed", async () => {
    localStorage.setItem("tcm-v2-theme-id", "halloween");
    initTheme(OCT30);
    expect(getThemeChoice()).toBe("halloween");
    expect(root.getAttribute("data-theme")).toBe("halloween");
    const stop = subscribeExtras(() => initTheme(OCT30));
    mockIPC((cmd) => (cmd === "get_extras_unlocked" ? false : undefined));
    await hydrateExtras();
    stop();
    expect(getThemeChoice()).toBe("light");
    expect(root.getAttribute("data-theme")).toBeNull();
    expect(localStorage.getItem("tcm-v2-theme-id")).toBe("halloween");
  });

  test("T3.d: while unknown a gated season does not switch, even on a fresh machine in its dates", () => {
    ocean();
    initTheme(OCT29);
    expect(getThemeChoice()).toBe("ocean");
    expect(localStorage.getItem(SEASON_KEY)).toBeNull();
  });

  test("T3.e: the runner window asks for the flag, and never restores while it cannot get one", async () => {
    switched();
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(OCT30);
    mockIPC((cmd) => {
      if (cmd === "get_extras_unlocked") throw new Error("no answer");
    });
    initRunnerTheme();
    await hydrateExtras();
    await Promise.resolve();
    expect(extrasKnown()).toBe(false);
    expect(getThemeChoice()).toBe("halloween");
    expect(record().themeState).toBe("active");
    expect(record().accentState).toBe("active");
    vi.useRealTimers();
  });

  test("T3.f: the runner window learns the flag from Rust and applies the normal rules", async () => {
    switched();
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(OCT30);
    let asked = 0;
    mockIPC((cmd) => {
      if (cmd === "get_extras_unlocked") {
        asked += 1;
        return false;
      }
    });
    initRunnerTheme();
    await vi.waitFor(() => expect(extrasKnown()).toBe(true));
    expect(asked).toBeGreaterThan(0);
    expect(getThemeChoice()).toBe("ocean");
    expect(record().themeState).toBe("done");
    vi.useRealTimers();
  });
});
