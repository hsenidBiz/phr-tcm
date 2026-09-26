// A screenshot's markers pulse one at a time, in number order, with a pause
// between: never two at once, and back to the first after the last.

import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { PULSE_STEP_MS, nextIndex, registerPulse, resetPulses } from "./render/pulse";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

function figureWith(count: number) {
  const figure = document.createElement("figure");
  const markers = Array.from({ length: count }, (_, i) => {
    const m = document.createElement("span");
    m.className = "marker pulse";
    m.textContent = String(i + 1);
    figure.appendChild(m);
    return m;
  });
  document.body.appendChild(figure);
  return { figure, markers };
}

const pulsing = (markers: HTMLElement[]) =>
  markers.filter((m) => m.classList.contains("pulse-now")).map((m) => m.textContent);

// Figures report as on screen the moment they are observed; `offscreen`
// and `onscreen` scroll one out and back.
let io: ((entries: { target: Element; isIntersecting: boolean }[]) => void) | null = null;
class OnScreen {
  constructor(cb: typeof io) {
    io = cb;
  }
  observe(target: Element) {
    io?.([{ target, isIntersecting: true }]);
  }
  unobserve() {}
  disconnect() {}
}
const offscreen = (target: Element) => io?.([{ target, isIntersecting: false }]);
const onscreen = (target: Element) => io?.([{ target, isIntersecting: true }]);

beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal("IntersectionObserver", OnScreen);
});
afterEach(() => {
  resetPulses();
  document.body.innerHTML = "";
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

test("markers take turns in number order and wrap to the first", () => {
  const { figure, markers } = figureWith(3);
  registerPulse(figure, markers);
  expect(pulsing(markers)).toEqual([]);
  const seen: (string | null)[][] = [];
  for (let i = 0; i < 4; i++) {
    vi.advanceTimersByTime(PULSE_STEP_MS);
    seen.push(pulsing(markers));
  }
  expect(seen).toEqual([["1"], ["2"], ["3"], ["1"]]);
});

test("nothing pulses while a marker on the figure is pointed at", () => {
  const { figure, markers } = figureWith(2);
  registerPulse(figure, markers);
  vi.advanceTimersByTime(PULSE_STEP_MS);
  figure.dataset.active = "a";
  vi.advanceTimersByTime(PULSE_STEP_MS);
  expect(pulsing(markers)).toEqual([]);
  delete figure.dataset.active;
  vi.advanceTimersByTime(PULSE_STEP_MS);
  expect(pulsing(markers)).toEqual(["2"]);
});

test("a figure scrolled out of view rests, and starts again from 1 when back", () => {
  const { figure, markers } = figureWith(3);
  registerPulse(figure, markers);
  vi.advanceTimersByTime(PULSE_STEP_MS * 2);
  expect(pulsing(markers)).toEqual(["2"]);
  offscreen(figure);
  expect(pulsing(markers)).toEqual([]);
  vi.advanceTimersByTime(PULSE_STEP_MS * 3);
  expect(pulsing(markers)).toEqual([]);
  onscreen(figure);
  vi.advanceTimersByTime(PULSE_STEP_MS);
  expect(pulsing(markers)).toEqual(["1"]);
});

test("a figure taken off the page stops being stepped", () => {
  const { figure, markers } = figureWith(2);
  registerPulse(figure, markers);
  figure.remove();
  vi.advanceTimersByTime(PULSE_STEP_MS * 3);
  expect(pulsing(markers)).toEqual([]);
});

test("nextIndex wraps and handles an empty list", () => {
  expect(nextIndex(-1, 3)).toBe(0);
  expect(nextIndex(2, 3)).toBe(0);
  expect(nextIndex(0, 0)).toBe(-1);
});

test("the pulse plays once per turn, not on a loop of its own", () => {
  const css = readFileSync(resolve(__dirname, "styles.css"), "utf8");
  const rule = css.match(/\.marker\.pulse\.pulse-now::after\s*\{([^}]*)\}/);
  expect(rule).not.toBeNull();
  expect(rule![1]).not.toMatch(/infinite/);
  expect(css).not.toMatch(/\.marker\.pulse::after\s*\{[^}]*animation:/);
});
