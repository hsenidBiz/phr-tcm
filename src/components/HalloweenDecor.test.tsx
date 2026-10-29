import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { act, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import HalloweenDecor, { HalloweenWeb } from "./HalloweenDecor";

const root = document.documentElement;
const css = readFileSync(resolve(__dirname, "../index.css"), "utf8");
const rule = (sel: string) => {
  const at = css.lastIndexOf(`\n${sel} {`);
  return at < 0 ? "" : css.slice(at, css.indexOf("}", at));
};

// A repeatable "random" that does not repeat a value soon.
function sequence() {
  let n = 0;
  return () => (n++ * 0.37 + 0.11) % 1;
}

const both = (random?: () => number) => (
  <>
    <HalloweenDecor random={random} />
    <HalloweenWeb />
  </>
);

beforeEach(() => root.setAttribute("data-theme", "halloween"));
afterEach(() => {
  root.removeAttribute("data-theme");
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

test("T2.a: it appears only on the Halloween theme and follows the theme live", async () => {
  root.removeAttribute("data-theme");
  const { container, findByText } = render(both());
  expect(container.innerHTML).toBe("");
  await act(async () => root.setAttribute("data-theme", "graphite"));
  expect(container.innerHTML).toBe("");
  await act(async () => root.setAttribute("data-theme", "halloween"));
  await findByText((_, el) => el?.classList.contains("halloween-decor") ?? false);
  expect(container.querySelectorAll(".halloween-pumpkin")).toHaveLength(4);
  expect(container.querySelectorAll(".halloween-bat")).toHaveLength(3);
  expect(container.querySelectorAll(".halloween-spider")).toHaveLength(3);
  expect(container.querySelectorAll(".halloween-web")).toHaveLength(1);
  expect(container.querySelectorAll(".halloween-fly")).toHaveLength(1);
  expect(container.querySelectorAll(".halloween-beetle")).toHaveLength(1);
  expect(container.querySelectorAll(".halloween-cocoon")).toHaveLength(1);
  await act(async () => root.setAttribute("data-theme", "ocean"));
  await waitFor(() => expect(container.innerHTML).toBe(""));
});

test("T2.b: it is hidden from assistive tech and stops under reduced motion", () => {
  const { container } = render(both());
  expect(container.querySelector(".halloween-decor")?.getAttribute("aria-hidden")).toBe("true");
  expect(container.querySelector(".halloween-web")?.getAttribute("aria-hidden")).toBe("true");
  const reduce = css.match(/@media \(prefers-reduced-motion: reduce\) \{(?:[^{}]|\{[^{}]*\})*\.halloween-pumpkin(?:[^{}]|\{[^{}]*\})*\}/)?.[0] ?? "";
  for (const sel of [".halloween-bat", ".halloween-spider", ".halloween-fly-wing", ".halloween-pumpkin"]) {
    expect(reduce).toContain(sel);
  }
});

test("T2.c: the component names no colour of its own", () => {
  const src = readFileSync(resolve(__dirname, "HalloweenDecor.tsx"), "utf8");
  expect(src).not.toMatch(/#[0-9a-fA-F]{3,8}\b/);
  expect(src).not.toContain("rgb(");
});

test("T2.d: the scenery stays small and visible: short strings, muted bats, the web bottom left of the content", () => {
  const { container } = render(both());
  const strings = [...container.querySelectorAll<HTMLElement>(".halloween-string")].map((e) => e.style.height);
  expect(strings).toEqual(["6px", "16px", "10px", "20px"]);
  expect(container.querySelector(".halloween-pumpkin svg")?.getAttribute("width")).toBe("32");
  expect(container.querySelector(".halloween-bat svg")?.getAttribute("width")).toBe("26");
  expect(container.querySelector(".halloween-web")?.getAttribute("width")).toBe("110");
  expect(rule(".halloween-bat svg")).toContain("fill: var(--color-muted)");
  const web = rule(".halloween-web");
  expect(web).toContain("position: absolute");
  expect(web).toContain("left: 0");
  expect(web).not.toContain("right: 0");
  expect(web).toContain("stroke: var(--color-muted)");
  expect(web).toContain("opacity: 0.45");
  expect(web).toContain("pointer-events: none");
  expect(rule(".halloween-spider svg")).toContain("fill: var(--color-text)");
});

test("T2.e: only pumpkin and spider bodies take clicks; strings, threads and the root do not", () => {
  expect(rule(".halloween-decor")).toContain("pointer-events: none");
  expect(rule(".halloween-decor *")).toContain("pointer-events: none");
  expect(rule(".halloween-pumpkin svg")).toContain("pointer-events: auto");
  expect(rule(".halloween-pumpkin svg")).toContain("cursor: pointer");
  expect(rule(".halloween-spider-body")).toContain("pointer-events: auto");
  expect(rule(".halloween-spider-body")).toContain("cursor: pointer");
  expect(rule(".halloween-bat")).toContain("pointer-events: none");
  expect(rule(".halloween-string")).not.toContain("pointer-events: auto");
  expect(rule(".halloween-spider-thread")).not.toContain("pointer-events: auto");
});

test("T2.f: the spiders hang apart from each other and from the pumpkins, each on its own cycle", () => {
  const { container } = render(<HalloweenDecor />);
  const spiders = [...container.querySelectorAll<HTMLElement>(".halloween-spider")];
  const lefts = spiders.map((s) => s.style.left);
  expect(new Set(lefts).size).toBe(3);
  for (const s of spiders) {
    const drop = parseInt(s.style.getPropertyValue("--drop"), 10);
    const secs = parseFloat(s.style.animationDuration);
    expect(drop).toBeGreaterThanOrEqual(40);
    expect(drop).toBeLessThanOrEqual(90);
    expect(secs).toBeGreaterThanOrEqual(15);
    expect(secs).toBeLessThanOrEqual(28);
    // The body follows the thread in flow, so its top is the thread's bottom.
    expect(s.firstElementChild?.className).toBe("halloween-spider-thread");
    expect(s.lastElementChild?.className).toContain("halloween-spider-body");
  }
  // No spider's 16px body overlaps a 32px pumpkin, at a few column widths.
  const pumpkins = [...container.querySelectorAll<HTMLElement>(".halloween-pumpkin")].map((p) => parseFloat(p.style.right));
  for (const w of [800, 1000, 1400]) {
    for (const l of lefts) {
      const x0 = (parseFloat(l) / 100) * w;
      for (const r of pumpkins) {
        const right = w - (r / 100) * w;
        expect(x0 + 16 <= right - 32 || x0 >= right).toBe(true);
      }
    }
  }
  expect(rule(".halloween-spider")).not.toContain("transform");
});

test("T2.g: clicking a pumpkin whacks it for a moment, and it can be whacked again", () => {
  vi.useFakeTimers();
  const { container } = render(<HalloweenDecor />);
  const pumpkin = container.querySelector<HTMLElement>(".halloween-pumpkin")!;
  expect(pumpkin.classList.contains("is-whack")).toBe(false);
  fireEvent.click(pumpkin.querySelector("svg")!);
  expect(pumpkin.classList.contains("is-whack")).toBe(true);
  act(() => void vi.advanceTimersByTime(1300));
  expect(pumpkin.classList.contains("is-whack")).toBe(false);
  fireEvent.click(pumpkin.querySelector("svg")!);
  expect(pumpkin.classList.contains("is-whack")).toBe(true);
  expect(rule(".halloween-pumpkin.is-whack")).toContain("halloween-whack");
  expect(css).toContain("@keyframes halloween-whack");
});

test("T2.h: clicking a spider sends it up its thread, and it comes back down later", () => {
  vi.useFakeTimers();
  const { container } = render(<HalloweenDecor />);
  const spider = container.querySelector<HTMLElement>(".halloween-spider")!;
  fireEvent.click(spider.querySelector(".halloween-spider-body")!);
  expect(spider.classList.contains("is-scurry")).toBe(true);
  expect(spider.style.getPropertyValue("--from")).not.toBe("");
  act(() => void vi.advanceTimersByTime(7100));
  expect(spider.classList.contains("is-scurry")).toBe(false);
  expect(css).toContain("@keyframes halloween-up");
});

test("T2.i: a bat flies to a point, then on to another once it lands", () => {
  vi.useFakeTimers();
  const { container } = render(both(sequence()));
  const bat = container.querySelector<HTMLElement>(".halloween-bat")!;
  const first = bat.style.transform;
  expect(first).toMatch(/^translate\(/);
  expect(parseFloat(bat.style.transitionDuration)).toBeGreaterThan(0);
  fireEvent.transitionEnd(bat);
  act(() => void vi.advanceTimersByTime(2000));
  expect(bat.style.transform).not.toBe(first);
  // Lands in the window's top part, at least 40px from an edge.
  const m = bat.style.transform.match(/translate\((-?[\d.]+)px, (-?[\d.]+)px\)/)!;
  expect(Number(m[1])).toBeGreaterThanOrEqual(40);
  expect(Number(m[1])).toBeLessThanOrEqual(window.innerWidth - 40);
  expect(Number(m[2])).toBeGreaterThanOrEqual(40);
  expect(Number(m[2])).toBeLessThanOrEqual(window.innerHeight * 0.6);
});

test("T2.j: a bat that never gets a transitionend still moves on", () => {
  vi.useFakeTimers();
  const { container } = render(<HalloweenDecor random={sequence()} />);
  const bat = container.querySelector<HTMLElement>(".halloween-bat")!;
  const first = bat.style.transform;
  act(() => void vi.advanceTimersByTime(60_000));
  expect(bat.style.transform).not.toBe(first);
});

test("T2.k: unmounting, or leaving the theme, clears every bat timer", async () => {
  vi.useFakeTimers();
  const a = render(<HalloweenDecor random={sequence()} />);
  expect(vi.getTimerCount()).toBeGreaterThan(0);
  a.unmount();
  expect(vi.getTimerCount()).toBe(0);
  const b = render(<HalloweenDecor random={sequence()} />);
  await act(async () => root.setAttribute("data-theme", "ocean"));
  expect(b.container.innerHTML).toBe("");
  expect(vi.getTimerCount()).toBe(0);
});

test("T2.l: under reduced motion the bats run no timers", () => {
  vi.useFakeTimers();
  vi.stubGlobal("matchMedia", (q: string) => ({
    matches: q.includes("reduce"),
    media: q,
    addEventListener: () => {},
    removeEventListener: () => {},
  }));
  render(<HalloweenDecor random={sequence()} />);
  expect(vi.getTimerCount()).toBe(0);
});
