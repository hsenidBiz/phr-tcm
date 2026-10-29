import { act, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, test, vi } from "vitest";
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

describe("a gated yearly season", () => {
  const xmasKey = (y: number) => `tcm-v2-season-christmas-${y}`;
  const at = (y: number, m: number, d: number, h = 0, min = 0) => new Date(y, m - 1, d, h, min).getTime();
  const DEC18 = at(2026, 12, 18, 0, 1);

  test("T2.a: with the gate closed nothing switches on 20 December", () => {
    ocean();
    setExtrasForTest(false);
    initTheme(at(2026, 12, 20, 12));
    expect(getThemeChoice()).toBe("ocean");
    expect(localStorage.getItem(xmasKey(2026))).toBeNull();
  });

  test("T2.b: with the gate open it switches from 18 December 00:00, not before", () => {
    ocean();
    setExtrasForTest(true);
    initTheme(at(2026, 12, 17, 23, 59));
    expect(getThemeChoice()).toBe("ocean");
    initTheme(DEC18);
    expect(getThemeChoice()).toBe("christmas");
    expect(getAccent()).toBe("default");
    expect(root.getAttribute("data-theme")).toBe("christmas");
    expect(root.classList.contains("dark")).toBe(true);
    expect(localStorage.getItem(xmasKey(2026))).not.toBeNull();
  });

  test("T2.c: it ends on 1 January with the person's look back, even on a late first launch", () => {
    ocean();
    setExtrasForTest(true);
    initTheme(DEC18);
    initTheme(at(2026, 12, 31, 23, 59));
    expect(getThemeChoice()).toBe("christmas");
    initTheme(at(2027, 1, 1));
    expect(getThemeChoice()).toBe("ocean");
    expect(getAccent()).toBe("blue");
    initTheme(at(2027, 1, 2));
    expect(getThemeChoice()).toBe("ocean");

    localStorage.clear();
    ocean();
    initTheme(DEC18);
    initTheme(at(2027, 1, 10, 9));
    expect(getThemeChoice()).toBe("ocean");
    expect(getAccent()).toBe("blue");
  });

  test("T2.d: next year's 18 December switches again, under a new key", () => {
    ocean();
    setExtrasForTest(true);
    initTheme(DEC18);
    initTheme(at(2027, 1, 1));
    expect(getThemeChoice()).toBe("ocean");
    initTheme(at(2027, 12, 18, 0, 1));
    expect(getThemeChoice()).toBe("christmas");
    expect(localStorage.getItem(xmasKey(2027))).not.toBeNull();
    expect(localStorage.getItem(xmasKey(2026))).not.toBeNull();
  });

  test("T2.e: closing the gate mid-season restores at once, and it stays restored", () => {
    ocean();
    setExtrasForTest(true);
    initTheme(DEC18);
    setExtrasForTest(false);
    initTheme(at(2026, 12, 20));
    expect(getThemeChoice()).toBe("ocean");
    expect(getAccent()).toBe("blue");
    setExtrasForTest(true);
    initTheme(at(2026, 12, 21));
    expect(getThemeChoice()).toBe("ocean");
  });

  test("T2.f: picking another theme cancels the restore, as it does for Halloween", () => {
    ocean();
    setExtrasForTest(true);
    initTheme(DEC18);
    setThemeChoice("slate");
    expect(getAccent()).toBe("blue");
    initTheme(at(2027, 1, 2));
    expect(getThemeChoice()).toBe("slate");
  });

  test("T2.g: choosing it on purpose keeps the look past the end", () => {
    ocean();
    setExtrasForTest(true);
    initTheme(DEC18);
    setThemeChoice("christmas");
    initTheme(at(2027, 1, 5));
    expect(getThemeChoice()).toBe("christmas");
    expect(getAccent()).toBe("default");
  });

  test("T2.h: a stored pick reads as light while the gate is closed, and is kept", () => {
    localStorage.setItem("tcm-v2-theme-id", "christmas");
    setExtrasForTest(false);
    expect(getThemeChoice()).toBe("light");
    initTheme(OCT29 + 90 * DAY);
    expect(root.getAttribute("data-theme")).toBeNull();
    expect(localStorage.getItem("tcm-v2-theme-id")).toBe("christmas");
    setExtrasForTest(true);
    expect(getThemeChoice()).toBe("christmas");
  });

  test("T2.i: an active Halloween record blocks a second season", () => {
    ocean();
    setExtrasForTest(true);
    initTheme(OCT29);
    expect(getThemeChoice()).toBe("halloween");
    // Synthetic: a machine whose first week is still running on 18 December.
    const rec = JSON.parse(localStorage.getItem(SEASON_KEY) ?? "{}");
    localStorage.setItem(SEASON_KEY, JSON.stringify({ ...rec, switchedAt: DEC18 - DAY }));
    initTheme(DEC18);
    expect(getThemeChoice()).toBe("halloween");
    expect(localStorage.getItem(xmasKey(2026))).toBeNull();
  });

  test("T2.j: a docs screenshot run is never switched", () => {
    vi.stubEnv("VITE_DOCS_CAPTURE", "1");
    localStorage.setItem("tcm-v2-dev-capture", "on");
    setExtrasForTest(true);
    initTheme(DEC18);
    expect(localStorage.getItem(xmasKey(2026))).toBeNull();
  });

  test("T2.k: the picker list hides gated themes unless the gate is open and it is not a capture run", () => {
    expect(visibleThemes(false).map((t) => t.id)).not.toContain("christmas");
    expect(visibleThemes(true).map((t) => t.id)).toContain("christmas");
    expect(visibleThemes(true).length).toBe(visibleThemes(false).length + 1);
    vi.stubEnv("VITE_DOCS_CAPTURE", "1");
    localStorage.setItem("tcm-v2-dev-capture", "on");
    expect(visibleThemes(true).map((t) => t.id)).not.toContain("christmas");
  });

  test("T2.l: Settings shows the card only while the flag is on", () => {
    mockIPC(() => undefined);
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={qc}>
        <Settings org="acme" project="Web" homeProject="Web" onHomeProjectChange={() => {}} onAllProjectsChange={() => {}} />
      </QueryClientProvider>,
    );
    expect(screen.queryByLabelText("Theme Christmas")).toBeNull();
    expect(screen.getByLabelText("Theme Halloween")).toBeTruthy();
    act(() => setExtrasForTest(true));
    expect(screen.getByLabelText("Theme Christmas")).toBeTruthy();
    act(() => setExtrasForTest(false));
    expect(screen.queryByLabelText("Theme Christmas")).toBeNull();
  });
});

describe("the gate's answer has not come yet", () => {
  const xmasKey = (y: number) => `tcm-v2-season-christmas-${y}`;
  const at = (y: number, m: number, d: number, h = 0, min = 0) => new Date(y, m - 1, d, h, min).getTime();
  const DEC18 = at(2026, 12, 18, 0, 1);
  const DEC19 = at(2026, 12, 19, 9);

  // An unlocked machine switched on 18 December, then restarted: the extras
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
    initTheme(DEC18);
    expect(getThemeChoice()).toBe("christmas");
    restart();
  }
  const record = () => JSON.parse(localStorage.getItem(xmasKey(2026)) ?? "{}");

  test("T3.a: a restart with the flag unknown leaves the season on; an open answer keeps it", async () => {
    switched();
    expect(extrasKnown()).toBe(false);
    const stop = subscribeExtras(() => initTheme(DEC19));
    initTheme(DEC19);
    expect(getThemeChoice()).toBe("christmas");
    expect(root.getAttribute("data-theme")).toBe("christmas");
    expect(record().themeState).toBe("active");
    mockIPC((cmd) => (cmd === "get_extras_unlocked" ? true : undefined));
    await hydrateExtras();
    stop();
    expect(extrasKnown()).toBe(true);
    expect(getThemeChoice()).toBe("christmas");
    expect(root.getAttribute("data-theme")).toBe("christmas");
    expect(record().themeState).toBe("active");
    expect(record().accentState).toBe("active");
  });

  test("T3.b: the same restart where Rust answers closed restores the look", async () => {
    switched();
    const stop = subscribeExtras(() => initTheme(DEC19));
    initTheme(DEC19);
    expect(getThemeChoice()).toBe("christmas");
    mockIPC((cmd) => (cmd === "get_extras_unlocked" ? false : undefined));
    await hydrateExtras();
    stop();
    expect(getThemeChoice()).toBe("ocean");
    expect(getAccent()).toBe("blue");
    expect(root.getAttribute("data-theme")).toBe("ocean");
    expect(record().themeState).toBe("done");
  });

  test("T3.c: a stored manual pick shows while unknown, and reads as light once the answer is closed", async () => {
    localStorage.setItem("tcm-v2-theme-id", "christmas");
    initTheme(OCT29 + 90 * DAY);
    expect(getThemeChoice()).toBe("christmas");
    expect(root.getAttribute("data-theme")).toBe("christmas");
    const stop = subscribeExtras(() => initTheme(OCT29 + 90 * DAY));
    mockIPC((cmd) => (cmd === "get_extras_unlocked" ? false : undefined));
    await hydrateExtras();
    stop();
    expect(getThemeChoice()).toBe("light");
    expect(root.getAttribute("data-theme")).toBeNull();
    expect(localStorage.getItem("tcm-v2-theme-id")).toBe("christmas");
  });

  test("T3.d: while unknown a gated season does not switch, even on a fresh machine in the window", () => {
    ocean();
    initTheme(DEC18);
    expect(getThemeChoice()).toBe("ocean");
    expect(localStorage.getItem(xmasKey(2026))).toBeNull();
  });

  test("T3.e: the runner window asks for the flag, and never restores while it cannot get one", async () => {
    switched();
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(DEC19);
    mockIPC((cmd) => {
      if (cmd === "get_extras_unlocked") throw new Error("no answer");
    });
    initRunnerTheme();
    await hydrateExtras();
    await Promise.resolve();
    expect(extrasKnown()).toBe(false);
    expect(getThemeChoice()).toBe("christmas");
    expect(record().themeState).toBe("active");
    expect(record().accentState).toBe("active");
    vi.useRealTimers();
  });

  test("T3.f: the runner window learns the flag from Rust and applies the normal rules", async () => {
    switched();
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(DEC19);
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

  test("T3.g: a record whose window starts after now (the clock moved back) is restored and closed", () => {
    ocean();
    setExtrasForTest(true);
    initTheme(at(2027, 12, 20));
    expect(getThemeChoice()).toBe("christmas");
    expect(localStorage.getItem(xmasKey(2027))).not.toBeNull();
    initTheme(at(2027, 12, 21));
    expect(getThemeChoice()).toBe("christmas");
    // The clock is corrected to 2026: the 2027 record is not in its window.
    initTheme(at(2026, 12, 20));
    expect(getThemeChoice()).toBe("christmas");
    expect(JSON.parse(localStorage.getItem(xmasKey(2027)) ?? "{}").themeState).toBe("done");
  });
});
