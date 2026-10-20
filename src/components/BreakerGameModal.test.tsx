import { fireEvent, render, screen } from "@testing-library/react";
import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, expect, test, vi } from "vitest";
import BreakerGameModal, { BREAKER_SRC } from "./BreakerGameModal";
import { BREAKER_BEST_CAP, BREAKER_BEST_KEY, loadBreakerBest } from "../lib/breakerBest";

afterEach(() => {
  localStorage.clear();
  vi.restoreAllMocks();
});

const frameOf = () => screen.getByTitle("Brick game") as HTMLIFrameElement;
const messageFrom = (source: Window, data: unknown) => new MessageEvent("message", { data, source });

test("the game opens in a sandboxed frame of the bundled page, with focus in it", () => {
  render(<BreakerGameModal onClose={() => {}} />);
  const frame = frameOf();
  expect(frame.getAttribute("src")).toBe("/vendor/breaker/index.html");
  expect(frame.getAttribute("sandbox")).toBe("allow-scripts");
  expect(frame.getAttribute("sandbox")).not.toContain("allow-same-origin");
  expect(document.activeElement).toBe(frame);
  const publicDir = resolve(dirname(fileURLToPath(import.meta.url)), "../../public");
  for (const f of ["index.html", "logic.js", "game.js", "escape.js", "game.css"]) {
    expect(existsSync(join(publicDir, dirname(BREAKER_SRC), f))).toBe(true);
  }
});

test("a breaker-escape message from the frame's own window closes it", () => {
  const onClose = vi.fn();
  render(<BreakerGameModal onClose={onClose} />);
  window.dispatchEvent(messageFrom(frameOf().contentWindow!, { type: "breaker-escape" }));
  expect(onClose).toHaveBeenCalledTimes(1);
});

test("the same message from any other source, or the runner's escape, does not", () => {
  const onClose = vi.fn();
  render(<BreakerGameModal onClose={onClose} />);
  window.dispatchEvent(messageFrom(window, { type: "breaker-escape" }));
  window.dispatchEvent(messageFrom(frameOf().contentWindow!, { type: "runner-escape" }));
  expect(onClose).not.toHaveBeenCalled();
});

test("a score from the frame is kept when it is higher", () => {
  render(<BreakerGameModal onClose={() => {}} />);
  const win = frameOf().contentWindow!;
  window.dispatchEvent(messageFrom(win, { type: "breaker-score", best: 120 }));
  expect(loadBreakerBest()).toBe(120);
  window.dispatchEvent(messageFrom(win, { type: "breaker-score", best: 80 }));
  expect(loadBreakerBest()).toBe(120);
  window.dispatchEvent(messageFrom(win, { type: "breaker-score", best: 450 }));
  expect(loadBreakerBest()).toBe(450);
  expect(localStorage.getItem(BREAKER_BEST_KEY)).toBe("450");
});

test("a score is rejected from another source, or when it is not a sane whole number", () => {
  localStorage.setItem(BREAKER_BEST_KEY, "10");
  render(<BreakerGameModal onClose={() => {}} />);
  const win = frameOf().contentWindow!;
  window.dispatchEvent(messageFrom(window, { type: "breaker-score", best: 999 }));
  for (const best of [-5, Number.NaN, Number.POSITIVE_INFINITY, BREAKER_BEST_CAP, 1e12, 12.5, "999", null]) {
    window.dispatchEvent(messageFrom(win, { type: "breaker-score", best }));
  }
  window.dispatchEvent(messageFrom(win, { type: "something-else", best: 999 }));
  window.dispatchEvent(messageFrom(win, "breaker-score"));
  expect(loadBreakerBest()).toBe(10);
});

test("an unreadable stored best reads as none", () => {
  localStorage.setItem(BREAKER_BEST_KEY, "{not json");
  expect(loadBreakerBest()).toBe(0);
  localStorage.setItem(BREAKER_BEST_KEY, "-3");
  expect(loadBreakerBest()).toBe(0);
});

test("the best kept so far is sent into the frame once it loads", () => {
  localStorage.setItem(BREAKER_BEST_KEY, "321");
  render(<BreakerGameModal onClose={() => {}} />);
  const frame = frameOf();
  const post = vi.spyOn(frame.contentWindow!, "postMessage").mockImplementation(() => {});
  fireEvent.load(frame);
  expect(post).toHaveBeenCalledWith({ type: "breaker-best", best: 321 }, "*");
});

test("the hint says how to play, catching what falls included", () => {
  render(<BreakerGameModal onClose={() => {}} />);
  expect(screen.getByText(/Catch what falls from a brick/)).toBeTruthy();
});

test("the Close button closes it", () => {
  const onClose = vi.fn();
  render(<BreakerGameModal onClose={onClose} />);
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(onClose).toHaveBeenCalledTimes(1);
});
