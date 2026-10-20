// CountTo: counts up from 0 as it starts to play, eases onto the value,
// never moves under prefers-reduced-motion, and a screen reader only ever
// gets the final value.

import { act, render } from "@testing-library/react";
import { StrictMode } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import CountTo, { COUNT_MS } from "./CountTo";

/** The frames asked for and not yet run. */
let frames: Map<number, FrameRequestCallback>;
let nextId = 0;

beforeEach(() => {
  frames = new Map();
  vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
    nextId += 1;
    frames.set(nextId, cb);
    return nextId;
  });
  vi.stubGlobal("cancelAnimationFrame", (id: number) => frames.delete(id));
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

/** Runs the frames waiting now, at time `at` (ms). */
const frame = (at: number) =>
  act(() => {
    const now = [...frames.values()];
    frames.clear();
    for (const cb of now) cb(at);
  });

function reduceMotion() {
  vi.spyOn(window, "matchMedia").mockImplementation(
    (query: string) =>
      ({
        matches: query.includes("reduce"),
        media: query,
        onchange: null,
        addListener: () => {},
        removeListener: () => {},
        addEventListener: () => {},
        removeEventListener: () => {},
        dispatchEvent: () => false,
      }) as unknown as MediaQueryList,
  );
}

/** What is painted (the moving digits, when counting). */
const painted = (el: HTMLElement) => el.querySelector("[aria-hidden]")?.textContent ?? el.textContent;
/** What assistive technology gets: the text with aria-hidden parts left out. */
function heard(el: HTMLElement) {
  const copy = el.cloneNode(true) as HTMLElement;
  for (const h of copy.querySelectorAll("[aria-hidden]")) h.remove();
  return copy.textContent;
}

function mount(value: number, play = true) {
  const view = render(
    <StrictMode>
      <div data-testid="n">
        <CountTo value={value} play={play} />
      </div>
    </StrictMode>,
  );
  const el = () => view.getByTestId("n");
  const rerender = (v: number, p = play) =>
    view.rerender(
      <StrictMode>
        <div data-testid="n">
          <CountTo value={v} play={p} />
        </div>
      </StrictMode>,
    );
  return { el, rerender };
}

test("counts up from 0 to the value, easing out, and ends exactly on it", () => {
  const { el } = mount(100);
  expect(painted(el())).toBe("0");
  frame(1000);
  expect(painted(el())).toBe("0");
  frame(1000 + COUNT_MS / 2);
  // Ease-out: past the middle by half time.
  const mid = Number(painted(el()));
  expect(mid).toBeGreaterThan(50);
  expect(mid).toBeLessThan(100);
  frame(1000 + COUNT_MS);
  expect(el().textContent).toBe("100");
  expect(el().querySelector("[aria-hidden]")).toBeNull();
  expect(frames.size).toBe(0);
});

test("a screen reader hears only the final value while it counts", () => {
  const { el } = mount(42);
  frame(0);
  frame(COUNT_MS / 4);
  expect(painted(el())).not.toBe("42");
  expect(heard(el())).toBe("42");
});

test("a re-render with the same value does not count again", () => {
  const { el, rerender } = mount(7);
  frame(0);
  frame(COUNT_MS);
  expect(el().textContent).toBe("7");
  rerender(7);
  expect(el().textContent).toBe("7");
  expect(frames.size).toBe(0);
});

test("a new value counts on from the number on screen", () => {
  const { el, rerender } = mount(10);
  frame(0);
  frame(COUNT_MS);
  rerender(20);
  expect(painted(el())).toBe("10");
  frame(5000);
  frame(5000 + COUNT_MS / 2);
  const mid = Number(painted(el()));
  expect(mid).toBeGreaterThan(10);
  expect(mid).toBeLessThan(20);
  frame(5000 + COUNT_MS);
  expect(el().textContent).toBe("20");
});

test("not playing shows the value; playing again counts from 0", () => {
  const { el, rerender } = mount(5, false);
  expect(el().textContent).toBe("5");
  expect(frames.size).toBe(0);
  rerender(5, true);
  expect(painted(el())).toBe("0");
  frame(0);
  frame(COUNT_MS);
  expect(el().textContent).toBe("5");
  rerender(5, false);
  rerender(5, true);
  expect(painted(el())).toBe("0");
});

test("under prefers-reduced-motion the value is there at once", () => {
  reduceMotion();
  const { el, rerender } = mount(12);
  expect(el().textContent).toBe("12");
  expect(frames.size).toBe(0);
  rerender(30);
  expect(el().textContent).toBe("30");
  expect(frames.size).toBe(0);
});

test("0 needs no count", () => {
  const { el } = mount(0);
  expect(el().textContent).toBe("0");
  expect(frames.size).toBe(0);
});

test("unmounting mid-count cancels the frame it asked for", () => {
  const view = render(<CountTo value={50} play />);
  frame(0);
  frame(COUNT_MS / 3);
  expect(frames.size).toBe(1);
  view.unmount();
  expect(frames.size).toBe(0);
});

test("a value changed mid-count counts on from the number on screen, once", () => {
  const { el, rerender } = mount(100);
  frame(0);
  frame(COUNT_MS / 2);
  const mid = Number(painted(el()));
  rerender(10);
  expect(frames.size).toBe(1);
  expect(painted(el())).toBe(String(mid));
  frame(2000);
  frame(2000 + COUNT_MS);
  expect(el().textContent).toBe("10");
  expect(frames.size).toBe(0);
});

test("reduced motion switched on mid-count shows the value at once", () => {
  let reduce = false;
  const listeners = new Set<(e: MediaQueryListEvent) => void>();
  vi.spyOn(window, "matchMedia").mockImplementation(
    (query: string) =>
      ({
        get matches() {
          return query.includes("reduce") && reduce;
        },
        media: query,
        onchange: null,
        addListener: () => {},
        removeListener: () => {},
        addEventListener: (_: string, cb: (e: MediaQueryListEvent) => void) => listeners.add(cb),
        removeEventListener: (_: string, cb: (e: MediaQueryListEvent) => void) => listeners.delete(cb),
        dispatchEvent: () => false,
      }) as unknown as MediaQueryList,
  );
  const { el } = mount(80);
  frame(0);
  frame(COUNT_MS / 4);
  expect(painted(el())).not.toBe("80");
  reduce = true;
  act(() => {
    for (const cb of [...listeners]) cb({ matches: true } as MediaQueryListEvent);
  });
  expect(el().textContent).toBe("80");
  expect(el().querySelector("[aria-hidden]")).toBeNull();
  expect(frames.size).toBe(0);
  // The count is over: nothing listens any more.
  expect(listeners.size).toBe(0);
});
