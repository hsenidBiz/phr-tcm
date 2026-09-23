import { afterEach, expect, test, vi } from "vitest";
import { CONFETTI_MS, burstConfetti } from "./confetti";

const realMatchMedia = window.matchMedia;

function fakeContext() {
  return {
    scale: vi.fn(),
    clearRect: vi.fn(),
    save: vi.fn(),
    restore: vi.fn(),
    translate: vi.fn(),
    rotate: vi.fn(),
    fillRect: vi.fn(),
    fillStyle: "",
    globalAlpha: 1,
  };
}

function reduceMotion() {
  window.matchMedia = ((q: string) => ({
    matches: q.includes("prefers-reduced-motion"),
    media: q,
    onchange: null,
    addListener: () => {},
    removeListener: () => {},
    addEventListener: () => {},
    removeEventListener: () => {},
    dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia;
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
  window.matchMedia = realMatchMedia;
  for (const c of document.querySelectorAll("canvas")) c.remove();
});

test("a burst is one canvas over the window that nothing can click or read, gone when it ends", () => {
  vi.useFakeTimers();
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(fakeContext() as never);
  burstConfetti();
  const canvas = document.querySelector<HTMLCanvasElement>("canvas[data-confetti]");
  expect(canvas).not.toBeNull();
  expect(canvas).toHaveAttribute("aria-hidden", "true");
  expect(canvas!.style.pointerEvents).toBe("none");
  expect(canvas!.style.position).toBe("fixed");
  vi.advanceTimersByTime(CONFETTI_MS);
  expect(document.querySelector("canvas[data-confetti]")).toBeNull();
});

test("stopping it early removes it at once", () => {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(fakeContext() as never);
  const stop = burstConfetti();
  expect(document.querySelector("canvas[data-confetti]")).not.toBeNull();
  stop();
  expect(document.querySelector("canvas[data-confetti]")).toBeNull();
  expect(() => stop()).not.toThrow();
});

test("under prefers-reduced-motion nothing is drawn at all", () => {
  reduceMotion();
  const ctx = vi.spyOn(HTMLCanvasElement.prototype, "getContext");
  burstConfetti();
  expect(ctx).not.toHaveBeenCalled();
  expect(document.querySelector("canvas")).toBeNull();
});

test("without a 2D context it does nothing and throws nothing", () => {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
  expect(() => burstConfetti()()).not.toThrow();
  expect(document.querySelector("canvas")).toBeNull();
});
