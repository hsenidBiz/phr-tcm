import { afterEach, expect, test } from "vitest";
import { SEASON_KEY } from "./season";
import { getAccent, getThemeChoice, initTheme, setAccent, setThemeChoice } from "./theme";

const OCT29 = new Date(2026, 9, 29, 12).getTime();
const DAY = 86_400_000;
const root = document.documentElement;

afterEach(() => {
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

test.each([
  ["a theme pick", () => setThemeChoice("midnight"), "midnight", "default"],
  ["re-picking Halloween", () => setThemeChoice("halloween"), "halloween", "default"],
  ["an accent pick", () => setAccent("rose"), "halloween", "rose"],
])("T1.d: %s during the week is never undone", (_n, pick, theme, accent) => {
  ocean();
  initTheme(OCT29);
  pick();
  initTheme(OCT29 + 8 * DAY);
  expect(getThemeChoice()).toBe(theme);
  expect(getAccent()).toBe(accent);
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
