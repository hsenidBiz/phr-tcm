import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { StrictMode } from "react";
import { act, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import ChristmasDecor, { ChristmasCorner } from "./ChristmasDecor";

const capture = vi.hoisted(() => ({ on: false }));
vi.mock("../dev/capture", () => ({ isCaptureMode: () => capture.on }));

const root = document.documentElement;
const css = readFileSync(resolve(__dirname, "../index.css"), "utf8").split(String.fromCharCode(13)).join("");
const rule = (sel: string) => {
  const at = css.lastIndexOf(`\n${sel} {`);
  return at < 0 ? "" : css.slice(at, css.indexOf("}", at));
};
// The Christmas block: from its banner comment up to the Motion section.
const blockStart = css.indexOf("/* Christmas decorations");
const block = css.slice(blockStart, css.indexOf("/* ── Motion", blockStart));
const frames = (name: string) => {
  const at = css.indexOf(`@keyframes ${name} {`);
  return css.slice(at, css.indexOf("\n}\n", at));
};

function sequence() {
  let n = 0;
  return () => (n++ * 0.37 + 0.11) % 1;
}

const both = (random?: () => number) => (
  <>
    <ChristmasDecor random={random} />
    <ChristmasCorner />
  </>
);

beforeEach(() => {
  capture.on = false;
  root.setAttribute("data-theme", "christmas");
});
afterEach(() => {
  root.removeAttribute("data-theme");
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

test("C2.a: it appears only on the Christmas theme, follows the theme live, and has every piece", async () => {
  root.removeAttribute("data-theme");
  const { container, findByText } = render(both());
  expect(container.innerHTML).toBe("");
  await act(async () => root.setAttribute("data-theme", "halloween"));
  expect(container.innerHTML).toBe("");
  await act(async () => root.setAttribute("data-theme", "christmas"));
  await findByText((_, el) => el?.classList.contains("christmas-decor") ?? false);
  expect(container.querySelectorAll(".christmas-bulb")).toHaveLength(14);
  expect(container.querySelectorAll(".christmas-bauble")).toHaveLength(4);
  expect(container.querySelectorAll(".christmas-flake")).toHaveLength(20);
  expect(container.querySelectorAll(".christmas-sleigh")).toHaveLength(1);
  expect(container.querySelectorAll(".christmas-snowman")).toHaveLength(1);
  await act(async () => root.setAttribute("data-theme", "ocean"));
  await waitFor(() => expect(container.innerHTML).toBe(""));
});

test("C2.b: nothing renders in capture mode", () => {
  capture.on = true;
  const { container } = render(both());
  expect(container.innerHTML).toBe("");
});

test("C2.c: it is hidden from assistive tech", () => {
  const { container } = render(both());
  expect(container.querySelector(".christmas-decor")?.getAttribute("aria-hidden")).toBe("true");
  expect(container.querySelector(".christmas-snow")?.getAttribute("aria-hidden")).toBe("true");
  expect(container.querySelector(".christmas-snowman")?.getAttribute("aria-hidden")).toBe("true");
});

test("C2.d: the component names no colour of its own", () => {
  for (const f of ["ChristmasDecor.tsx", "decorShared.tsx"]) {
    const src = readFileSync(resolve(__dirname, f), "utf8");
    expect(src).not.toMatch(/#[0-9a-fA-F]{3,8}\b/);
    expect(src).not.toContain("rgb(");
  }
});

test("C2.e: the lights cycle four colours along a drooping wire; the baubles hang clear of the right-hand side", () => {
  const { container } = render(both());
  const bulbs = [...container.querySelectorAll<HTMLElement>(".christmas-bulb")].sort(
    (a, b) => parseFloat(a.style.left) - parseFloat(b.style.left),
  );
  const tones = bulbs.map((b) => b.className.match(/christmas-tone-(\d)/)?.[1]);
  expect(new Set(tones).size).toBe(4);
  const tops = bulbs.map((b) => parseFloat(b.style.top));
  // The wire droops: the middle hangs lower than the ends.
  expect(tops[7]).toBeGreaterThan(tops[0]);
  expect(tops[6]).toBeGreaterThan(tops[13]);
  // Twinkling is three shared groups, not an animation per bulb.
  const groups = [...container.querySelectorAll<HTMLElement>(".christmas-twinkle")];
  expect(groups).toHaveLength(3);
  expect(groups.every((g) => g.querySelectorAll(".christmas-bulb").length >= 4)).toBe(true);
  expect(bulbs.every((b) => b.style.animationDelay === "")).toBe(true);
  expect(block).not.toMatch(/\.christmas-bulb \{[^}]*animation/);
  expect(rule(".christmas-twinkle-1")).toContain("animation-delay");
  expect(rule(".christmas-twinkle-2")).toContain("animation-delay");
  const strings = [...container.querySelectorAll<HTMLElement>(".christmas-string")].map((e) => e.style.height);
  expect(strings).toEqual(["6px", "16px", "10px", "20px"]);
  const rights = [...container.querySelectorAll<HTMLElement>(".christmas-bauble")].map((e) => parseFloat(e.style.right));
  expect(Math.min(...rights)).toBeGreaterThanOrEqual(22);
  for (const sel of [".christmas-tone-0", ".christmas-tone-1", ".christmas-tone-2", ".christmas-tone-3"]) {
    expect(block).toContain(sel);
  }
});

test("C2.f: the snow is light, sits in its own clipped layer, and varies", () => {
  const { container } = render(<ChristmasCorner />);
  const flakes = [...container.querySelectorAll<HTMLElement>(".christmas-flake")];
  expect(flakes).toHaveLength(20);
  expect(new Set(flakes.map((f) => f.style.left)).size).toBeGreaterThan(15);
  expect(new Set(flakes.map((f) => f.style.animationDuration)).size).toBeGreaterThan(5);
  expect(new Set(flakes.map((f) => f.style.getPropertyValue("--drift"))).size).toBeGreaterThan(2);
  const snow = rule(".christmas-snow");
  expect(snow).toContain("position: absolute");
  expect(snow).toContain("overflow: hidden");
  expect(rule(".christmas-flake")).toContain("animation: christmas-fall");
});

test("C2.g: the snowman sits bottom left of the content, like the web", () => {
  const { container } = render(<ChristmasCorner />);
  expect(container.querySelector(".christmas-snowman")?.getAttribute("width")).toBe("90");
  const r = rule(".christmas-snowman");
  expect(r).toContain("position: absolute");
  expect(r).toContain("left: 0");
  expect(r).not.toContain("right: 0");
  expect(r).toContain("pointer-events: none");
});

test("C2.h: no decoration takes a click: every pointer-events in the Christmas block is none", () => {
  const all = [...block.matchAll(/pointer-events:\s*([a-z]+)/g)].map((m) => m[1]);
  expect(all.length).toBeGreaterThan(3);
  expect(all.every((v) => v === "none")).toBe(true);
  expect(block).not.toContain("cursor: pointer");
  expect(blockStart).toBeGreaterThan(0);
  expect(blockStart).toBeLessThan(css.indexOf("/* ── Motion"));
  expect(rule(".christmas-decor")).toContain("pointer-events: none");
  expect(rule(".christmas-decor *")).toContain("pointer-events: none");
  expect(rule(".christmas-snow")).toContain("pointer-events: none");
  expect(rule(".christmas-sleigh")).toContain("pointer-events: none");
});

test("C2.h2: the sleigh starts off the left edge in CSS, so it never shows before its first pass", () => {
  expect(rule(".christmas-sleigh")).toMatch(/transform: translateX\(-\d+px\)/);
});

test("C2.i: every animated class has a reduced-motion entry, and the sleigh and snow are hidden", () => {
  const animated = [...block.matchAll(/\n(\.[a-z0-9 .:-]+) \{[^}]*\banimation:/g)].map((m) => m[1]);
  expect(animated.length).toBeGreaterThan(3);
  const reduce = block.match(/@media \(prefers-reduced-motion: reduce\) \{(?:[^{}]|\{[^{}]*\})*\}/)?.[0] ?? "";
  expect(reduce).not.toBe("");
  for (const sel of animated) expect(reduce).toContain(sel);
  expect(reduce).toMatch(/\.christmas-sleigh[^{}]*\{[^}]*display: none/);
  expect(reduce).toMatch(/\.christmas-snow[^{}]*\{[^}]*display: none/);
});

test("C2.j: every Christmas keyframe moves only transform or opacity", () => {
  const names = [...block.matchAll(/@keyframes (christmas-[a-z-]+)/g)].map((m) => m[1]);
  expect(names.length).toBeGreaterThanOrEqual(4);
  for (const n of names) {
    const body = frames(n);
    const props = [...body.matchAll(/^\s+([a-z-]+):/gm)].map((m) => m[1]);
    expect(props.length).toBeGreaterThan(0);
    for (const p of props) expect(["transform", "opacity"]).toContain(p);
  }
});

// A press at (x, y) on `target`, as the browser would send it.
function press(target: Element, x: number, y: number, button = 0) {
  const e = new MouseEvent("pointerdown", { bubbles: true, cancelable: true, clientX: x, clientY: y, button });
  act(() => void target.dispatchEvent(e));
  return e;
}
const rectAt = (el: Element, left: number, top: number, w: number, h: number) =>
  vi.spyOn(el, "getBoundingClientRect").mockReturnValue({
    left, top, right: left + w, bottom: top + h, width: w, height: h, x: left, y: top, toJSON: () => ({}),
  } as DOMRect);
const plainDiv = () => document.body.appendChild(document.createElement("div"));

test("C2.k: pressing an empty spot over a bauble spins it for a moment, and again", () => {
  vi.useFakeTimers();
  const { container } = render(<ChristmasDecor />);
  const bauble = container.querySelector<HTMLElement>(".christmas-bauble")!;
  rectAt(bauble.querySelector("svg")!, 100, 20, 20, 22);
  const plain = plainDiv();
  expect(bauble.classList.contains("is-spin")).toBe(false);
  press(plain, 108, 30);
  expect(bauble.classList.contains("is-spin")).toBe(true);
  act(() => void vi.advanceTimersByTime(1300));
  expect(bauble.classList.contains("is-spin")).toBe(false);
  press(plain, 400, 400);
  press(plain, 108, 30, 2);
  expect(bauble.classList.contains("is-spin")).toBe(false);
  press(plain, 96, 30);
  expect(bauble.classList.contains("is-spin")).toBe(true);
  plain.remove();
  expect(rule(".christmas-bauble.is-spin .christmas-ball")).toContain("christmas-spin");
});

test("C2.l: a press on a control over a bauble leaves it alone and is never prevented", () => {
  vi.useFakeTimers();
  const { container } = render(<ChristmasDecor />);
  const bauble = container.querySelector<HTMLElement>(".christmas-bauble")!;
  rectAt(bauble.querySelector("svg")!, 100, 20, 20, 22);
  const wrap = document.body.appendChild(document.createElement("div"));
  wrap.innerHTML = '<button id="b"><span id="in">Go</span></button><a href="#x" id="a">x</a>';
  for (const sel of ["#b", "#in", "#a"]) {
    expect(press(wrap.querySelector(sel)!, 108, 30).defaultPrevented).toBe(false);
  }
  expect(bauble.classList.contains("is-spin")).toBe(false);
  const plain = wrap.appendChild(document.createElement("div"));
  expect(press(plain, 108, 30).defaultPrevented).toBe(false);
  expect(bauble.classList.contains("is-spin")).toBe(true);
  wrap.remove();
});

test("C2.m: the sleigh crosses left to right now and then, and is rescheduled", () => {
  vi.useFakeTimers();
  const { container } = render(<ChristmasDecor random={sequence()} />);
  const sleigh = container.querySelector<HTMLElement>(".christmas-sleigh")!;
  expect(sleigh.style.transform).toMatch(/^translateX\(-/);
  act(() => void vi.advanceTimersByTime(15_000));
  const m = sleigh.style.transform.match(/translateX\((-?[\d.]+)px\)/)!;
  expect(Number(m[1])).toBeGreaterThan(window.innerWidth - 1);
  expect(parseFloat(sleigh.style.transitionDuration)).toBeGreaterThan(5);
  expect(vi.getTimerCount()).toBe(1);
});

test("C2.n: passes keep coming, 45 to 70 seconds apart", () => {
  vi.useFakeTimers();
  const { container, unmount } = render(<ChristmasDecor random={() => 0} />);
  const sleigh = container.querySelector<HTMLElement>(".christmas-sleigh")!;
  // A pass starts by parking the sleigh off the left edge with no transition.
  const starts: number[] = [];
  let last = sleigh.style.transitionDuration + sleigh.style.transform;
  for (let t = 1; t <= 400; t++) {
    act(() => void vi.advanceTimersByTime(1000));
    const now = sleigh.style.transitionDuration + sleigh.style.transform;
    if (now !== last && sleigh.style.transform.startsWith("translateX(-")) starts.push(t);
    last = now;
  }
  expect(starts.length).toBeGreaterThanOrEqual(3);
  // Parked-to-parked is one crossing plus the wait; with random() = 0 the wait is the shortest.
  const gap = starts[2] - starts[1];
  expect(gap).toBeGreaterThanOrEqual(45);
  expect(gap).toBeLessThan(70);
  unmount();
});

test("C2.o: unmounting, or leaving the theme, clears the sleigh timer", async () => {
  vi.useFakeTimers();
  const a = render(<ChristmasDecor random={sequence()} />);
  expect(vi.getTimerCount()).toBeGreaterThan(0);
  a.unmount();
  expect(vi.getTimerCount()).toBe(0);
  const b = render(<ChristmasDecor random={sequence()} />);
  await act(async () => root.setAttribute("data-theme", "ocean"));
  expect(b.container.innerHTML).toBe("");
  expect(vi.getTimerCount()).toBe(0);
});

test("C2.p: under reduced motion the sleigh runs no timers, and it follows the setting live", () => {
  vi.useFakeTimers();
  const listeners = new Set<() => void>();
  const mq = {
    matches: true,
    media: "",
    addEventListener: (_: string, l: () => void) => void listeners.add(l),
    removeEventListener: (_: string, l: () => void) => void listeners.delete(l),
  };
  vi.stubGlobal("matchMedia", () => mq);
  const { unmount } = render(<ChristmasDecor random={sequence()} />);
  expect(vi.getTimerCount()).toBe(0);
  mq.matches = false;
  act(() => listeners.forEach((l) => l()));
  expect(vi.getTimerCount()).toBeGreaterThan(0);
  mq.matches = true;
  act(() => listeners.forEach((l) => l()));
  expect(vi.getTimerCount()).toBe(0);
  unmount();
  expect(listeners.size).toBe(0);
});

test("C2.q: the document listener goes on unmount and on leaving the theme, and StrictMode leaves one", async () => {
  const add = vi.spyOn(document, "addEventListener");
  const remove = vi.spyOn(document, "removeEventListener");
  const net = () =>
    add.mock.calls.filter((c) => c[0] === "pointerdown").length -
    remove.mock.calls.filter((c) => c[0] === "pointerdown").length;
  const a = render(<ChristmasDecor />);
  expect(net()).toBe(1);
  a.unmount();
  expect(net()).toBe(0);
  const b = render(<ChristmasDecor />);
  await act(async () => root.setAttribute("data-theme", "ocean"));
  expect(net()).toBe(0);
  b.unmount();
  await act(async () => root.setAttribute("data-theme", "christmas"));
  const c = render(
    <StrictMode>
      <ChristmasDecor />
    </StrictMode>,
  );
  expect(net()).toBe(1);
  c.unmount();
  expect(net()).toBe(0);
  add.mockRestore();
  remove.mockRestore();
});
