import { afterEach, expect, test, vi } from "vitest";
import { SEASON_KEY } from "./season";
import { getAccent, getThemeChoice, initTheme, setAccent, setThemeChoice } from "./theme";

const OCT29 = new Date(2026, 9, 29, 12).getTime();
const DAY = 86_400_000;
const root = document.documentElement;

afterEach(() => {
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
