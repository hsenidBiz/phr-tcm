import { afterEach, expect, test } from "vitest";
import { getTheme, getThemeChoice, setTheme, setThemeChoice } from "./theme";
import { cn } from "./cn";

afterEach(() => {
  localStorage.clear();
  document.documentElement.classList.remove("dark");
  document.documentElement.removeAttribute("data-theme");
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

test("system clears the stored choice; legacy dark key migrates to slate", () => {
  setThemeChoice("graphite");
  setThemeChoice("system");
  expect(localStorage.getItem("tcm-v2-theme-id")).toBeNull();
  expect(getThemeChoice()).toBe("system");

  localStorage.setItem("tcm-v2-theme", "dark"); // pre-theme-system value
  expect(getThemeChoice()).toBe("slate");
});

test("cn merges tailwind classes with later-wins semantics", () => {
  expect(cn("px-2", "px-4")).toBe("px-4");
  expect(cn("text-sm", false && "hidden", "font-bold")).toBe("text-sm font-bold");
});
