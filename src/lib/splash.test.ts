// The loading screen in index.html: shown in full from the page's first
// paint, and taken away only once the app has something real to show.

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { afterEach, expect, test, vi } from "vitest";
import { hideSplash, resetSplashForTests } from "./splash";

const indexHtml = () => readFileSync(resolve(__dirname, "../../index.html"), "utf8");

function mountSplash() {
  const el = document.createElement("div");
  el.id = "splash";
  document.body.appendChild(el);
  return el;
}

afterEach(() => {
  vi.useRealTimers();
  document.getElementById("splash")?.remove();
  resetSplashForTests();
});

test("hideSplash fades the loading screen out, then removes it", () => {
  vi.useFakeTimers();
  const el = mountSplash();
  hideSplash();
  vi.advanceTimersByTime(20);
  expect(el.style.opacity).toBe("0");
  expect(document.getElementById("splash")).not.toBeNull();
  vi.advanceTimersByTime(400);
  expect(document.getElementById("splash")).toBeNull();
});

test("hideSplash is safe to call again, and without a loading screen", () => {
  vi.useFakeTimers();
  expect(() => hideSplash()).not.toThrow();
  mountSplash();
  hideSplash();
  hideSplash();
  vi.advanceTimersByTime(400);
  expect(document.getElementById("splash")).toBeNull();
});

// The page is on screen for well under a second on a normal launch, so
// anything that fades or draws itself in never gets seen: the flask, the
// name and the bar are all there from the first paint.
test("index.html's loading screen shows the flask and the name from its first paint", () => {
  const style = indexHtml().match(/<style>([\s\S]*?)<\/style>/)?.[1] ?? "";
  const splashRules = style.split("}").filter((r) => r.includes("#splash"));
  expect(splashRules.length).toBeGreaterThan(0);
  for (const rule of splashRules) {
    expect(rule, rule).not.toMatch(/opacity:\s*0\b/);
    expect(rule, rule).not.toMatch(/stroke-dashoffset/);
  }
  expect(indexHtml()).toContain("TEST CASE MANAGER");
});
