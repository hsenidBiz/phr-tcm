import { afterEach, expect, test } from "vitest";
import {
  darkPref,
  getAccent,
  getTheme,
  getThemeChoice,
  setAccent,
  setTheme,
  setThemeChoice,
} from "./theme";
import { cn } from "./cn";

afterEach(() => {
  localStorage.clear();
  document.documentElement.classList.remove("dark");
  document.documentElement.removeAttribute("data-theme");
  document.documentElement.removeAttribute("data-accent");
});

test("full themes apply the dark class and data-theme palette", () => {
  setThemeChoice("midnight");
  expect(getThemeChoice()).toBe("midnight");
  expect(document.documentElement.classList.contains("dark")).toBe(true);
  expect(document.documentElement.getAttribute("data-theme")).toBe("midnight");
  expect(getTheme()).toBe("dark");

  setThemeChoice("light");
  expect(document.documentElement.classList.contains("dark")).toBe(false);
  expect(document.documentElement.getAttribute("data-theme")).toBeNull();
  expect(getTheme()).toBe("light");
});

test("base slate theme uses no data-theme attribute", () => {
  setThemeChoice("slate");
  expect(document.documentElement.classList.contains("dark")).toBe(true);
  expect(document.documentElement.getAttribute("data-theme")).toBeNull();
});

test("the light/dark toggle returns to the last dark theme", () => {
  setThemeChoice("ocean");
  setTheme("light");
  expect(getTheme()).toBe("light");
  setTheme("dark"); // sun/moon toggle back
  expect(getThemeChoice()).toBe("ocean");
  expect(document.documentElement.getAttribute("data-theme")).toBe("ocean");
});

test("system is stored explicitly, distinct from an unset choice", () => {
  setThemeChoice("graphite");
  setThemeChoice("system");
  expect(localStorage.getItem("tcm-v2-theme-id")).toBe("system");
  expect(getThemeChoice()).toBe("system");
});

test("legacy dark key migrates to slate", () => {
  localStorage.setItem("tcm-v2-theme", "dark"); // pre-theme-system value
  expect(getThemeChoice()).toBe("slate");
});

test("an unset choice defaults to Light, regardless of OS preference", () => {
  expect(localStorage.getItem("tcm-v2-theme-id")).toBeNull();
  expect(getThemeChoice()).toBe("light");
});

test("darkPref defaults to Graphite when nothing dark has been chosen yet", () => {
  expect(darkPref()).toBe("graphite");
});

test("the light/dark toggle lands on Graphite by default", () => {
  setTheme("dark");
  expect(getThemeChoice()).toBe("graphite");
  expect(document.documentElement.getAttribute("data-theme")).toBe("graphite");
});

test("an explicit theme choice is kept - darkPref only falls back when unset", () => {
  setThemeChoice("ocean");
  expect(darkPref()).toBe("ocean");
});

test("accent: unset resolves to violet, matching the icon", () => {
  expect(localStorage.getItem("tcm-v2-accent")).toBeNull();
  expect(getAccent()).toBe("violet");
});

test("accent: explicitly picking \"default\" (the theme's own colour) is stored and honoured", () => {
  setAccent("default");
  expect(localStorage.getItem("tcm-v2-accent")).toBe("default");
  expect(getAccent()).toBe("default");
  expect(document.documentElement.hasAttribute("data-accent")).toBe(false);
});

test("accent: an explicit non-default choice is kept and applied", () => {
  setAccent("blue");
  expect(getAccent()).toBe("blue");
  expect(document.documentElement.getAttribute("data-accent")).toBe("blue");
});

test("cn merges tailwind classes with later-wins semantics", () => {
  expect(cn("px-2", "px-4")).toBe("px-4");
  expect(cn("text-sm", false && "hidden", "font-bold")).toBe("text-sm font-bold");
});
