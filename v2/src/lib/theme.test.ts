import { afterEach, expect, test } from "vitest";
import { applyTheme, getTheme, setTheme } from "./theme";
import { cn } from "./cn";

afterEach(() => {
  localStorage.clear();
  document.documentElement.classList.remove("dark");
});

test("explicit theme persists and applies the dark class", () => {
  setTheme("dark");
  expect(getTheme()).toBe("dark");
  expect(document.documentElement.classList.contains("dark")).toBe(true);

  setTheme("light");
  expect(getTheme()).toBe("light");
  expect(document.documentElement.classList.contains("dark")).toBe(false);
});

test("system theme clears the stored choice", () => {
  setTheme("dark");
  setTheme("system");
  expect(localStorage.getItem("tcm-v2-theme")).toBeNull();
  expect(getTheme()).toBe("system");
  applyTheme("system"); // must not throw regardless of matchMedia support
});

test("cn merges tailwind classes with later-wins semantics", () => {
  expect(cn("px-2", "px-4")).toBe("px-4");
  expect(cn("text-sm", false && "hidden", "font-bold")).toBe("text-sm font-bold");
});
