import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { StrictMode } from "react";
import { act, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import HalloweenDecor, { HalloweenWeb } from "./HalloweenDecor";

const root = document.documentElement;
const css = readFileSync(resolve(__dirname, "../index.css"), "utf8").split(String.fromCharCode(13)).join("");
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
  expect(container.querySelector(".halloween-web")?.getAttribute("width")).toBe("90");
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

test("T2.e: no decoration takes a click: nothing in the Halloween block sets pointer-events auto", () => {
  const start = css.indexOf("/* Halloween decorations");
  const block = css.slice(start, css.indexOf("@media (prefers-reduced-motion: reduce)", start));
  const all = [...block.matchAll(/pointer-events:\s*([a-z]+)/g)].map((m) => m[1]);
  expect(all.length).toBeGreaterThan(1);
  expect(all.every((v) => v === "none")).toBe(true);
  expect(block).not.toContain("cursor: pointer");
  expect(start).toBeLessThan(css.indexOf("/* ── Motion"));
  expect(rule(".halloween-decor")).toContain("pointer-events: none");
  expect(rule(".halloween-decor *")).toContain("pointer-events: none");
  expect(rule(".halloween-bat")).toContain("pointer-events: none");
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
  for (const w of [760, 1000, 1400]) {
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

test("T2.g: pressing a pumpkin whacks it for a moment, and it can be whacked again", () => {
  vi.useFakeTimers();
  const { container } = render(<HalloweenDecor />);
  const pumpkin = container.querySelector<HTMLElement>(".halloween-pumpkin")!;
  rectAt(pumpkin.querySelector("svg")!, 100, 20, 32, 27);
  const plain = plainDiv();
  expect(pumpkin.classList.contains("is-whack")).toBe(false);
  press(plain, 110, 30);
  expect(pumpkin.classList.contains("is-whack")).toBe(true);
  act(() => void vi.advanceTimersByTime(1300));
  expect(pumpkin.classList.contains("is-whack")).toBe(false);
  press(plain, 400, 400);
  press(plain, 110, 30, 2);
  expect(pumpkin.classList.contains("is-whack")).toBe(false);
  // Within the 4px pad counts.
  press(plain, 97, 30);
  expect(pumpkin.classList.contains("is-whack")).toBe(true);
  plain.remove();
  expect(rule(".halloween-pumpkin.is-whack")).toContain("halloween-whack");
  expect(css).toContain("@keyframes halloween-whack");
});

test("T2.g2: a press on a control over a pumpkin or spider leaves them alone and the control still gets its click", () => {
  vi.useFakeTimers();
  const { container } = render(<HalloweenDecor />);
  const pumpkin = container.querySelector<HTMLElement>(".halloween-pumpkin")!;
  const spider = container.querySelector<HTMLElement>(".halloween-spider")!;
  rectAt(pumpkin.querySelector("svg")!, 100, 20, 32, 27);
  rectAt(spider.querySelector(".halloween-spider-body")!, 300, 20, 16, 12);
  const onClick = vi.fn();
  const wrap = document.body.appendChild(document.createElement("div"));
  wrap.innerHTML =
    '<button id="b"><span id="in">Go</span></button><div role="menuitem" id="m"></div><a href="#x" id="a">x</a>';
  wrap.querySelector("button")!.addEventListener("click", onClick);
  for (const [sel, x, y] of [["#b", 110, 30], ["#in", 110, 30], ["#m", 110, 30], ["#a", 305, 25]] as const) {
    expect(press(wrap.querySelector(sel)!, x, y).defaultPrevented).toBe(false);
  }
  expect(pumpkin.classList.contains("is-whack")).toBe(false);
  expect(spider.classList.contains("is-scurry")).toBe(false);
  fireEvent.click(wrap.querySelector("button")!);
  expect(onClick).toHaveBeenCalledTimes(1);
  // The same points on a plain element do set them off, and are never prevented.
  const plain = wrap.appendChild(document.createElement("div"));
  expect(press(plain, 110, 30).defaultPrevented).toBe(false);
  expect(pumpkin.classList.contains("is-whack")).toBe(true);
  expect(press(plain, 305, 25).defaultPrevented).toBe(false);
  expect(spider.classList.contains("is-scurry")).toBe(true);
  wrap.remove();
});

test("T2.g3: the whack restarts on every press", () => {
  vi.useFakeTimers();
  const { container } = render(<HalloweenDecor />);
  const pumpkin = container.querySelector<HTMLElement>(".halloween-pumpkin")!;
  rectAt(pumpkin.querySelector("svg")!, 100, 20, 32, 27);
  const plain = plainDiv();
  press(plain, 110, 30);
  const observer = new MutationObserver(() => {});
  observer.observe(pumpkin, { attributes: true, attributeFilter: ["class"] });
  press(plain, 110, 30);
  // The class went off and back on within the press.
  expect(observer.takeRecords().length).toBeGreaterThan(1);
  expect(pumpkin.classList.contains("is-whack")).toBe(true);
  observer.disconnect();
  plain.remove();
});

test("T2.g4: the whack plays from its start and rests at 0deg on both ends, as the sway does", () => {
  expect(rule(".halloween-pumpkin.is-whack")).toContain("animation-delay: 0s");
  const frames = (name: string) => {
    const at = css.indexOf(`@keyframes ${name} {`);
    return css.slice(at, css.indexOf("\n}\n", at));
  };
  expect(frames("halloween-whack")).toMatch(/0% \{\s*transform: rotate\(0\);/);
  expect(frames("halloween-whack")).toMatch(/100% \{\s*transform: rotate\(0\);/);
  expect(frames("halloween-sway")).toMatch(/0%,\s*100% \{\s*transform: rotate\(0\);/);
  // The sway's own delay is a variable, so it can never override the whack's.
  const { container } = render(<HalloweenDecor />);
  const pumpkin = container.querySelector<HTMLElement>(".halloween-pumpkin")!;
  expect(pumpkin.style.animationDelay).toBe("");
  expect(pumpkin.style.getPropertyValue("--sway-delay")).toBe("-0.9s");
  expect(rule(".halloween-pumpkin")).toContain("animation-delay: var(--sway-delay");
});

test("T2.h: pressing a spider sends it up its thread, and it comes back down later", () => {
  vi.useFakeTimers();
  const { container } = render(<HalloweenDecor />);
  const spider = container.querySelector<HTMLElement>(".halloween-spider")!;
  rectAt(spider.querySelector(".halloween-spider-body")!, 300, 20, 16, 12);
  rectAt(spider.querySelector(".halloween-spider-thread")!, 307, 0, 1, 33);
  const plain = plainDiv();
  press(plain, 305, 25);
  expect(spider.classList.contains("is-scurry")).toBe(true);
  expect(spider.style.getPropertyValue("--from")).toBe("33px");
  expect(spider.style.getPropertyValue("--from-n")).toBe("33");
  act(() => void vi.advanceTimersByTime(7100));
  expect(spider.classList.contains("is-scurry")).toBe(false);
  expect(css).toContain("@keyframes halloween-up");
  // After the scurry it answers again.
  press(plain, 305, 25);
  expect(spider.classList.contains("is-scurry")).toBe(true);
  plain.remove();
});

test("T2.h2: spam-pressing a spider scurries it once, and it resumes its cycle", () => {
  vi.useFakeTimers();
  const { container } = render(<HalloweenDecor />);
  const spider = container.querySelector<HTMLElement>(".halloween-spider")!;
  rectAt(spider.querySelector(".halloween-spider-body")!, 300, 20, 16, 12);
  const thread = rectAt(spider.querySelector(".halloween-spider-thread")!, 307, 0, 1, 33);
  const plain = plainDiv();
  act(() => {
    for (let i = 0; i < 6; i++) plain.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, clientX: 305, clientY: 25 }));
  });
  expect(spider.classList.contains("is-scurry")).toBe(true);
  // Only the first press measured the thread.
  expect(thread).toHaveBeenCalledTimes(1);
  expect(spider.style.getPropertyValue("--from")).toBe("33px");
  act(() => void vi.advanceTimersByTime(3000));
  press(plain, 305, 25);
  expect(thread).toHaveBeenCalledTimes(1);
  act(() => void vi.advanceTimersByTime(4100));
  expect(spider.classList.contains("is-scurry")).toBe(false);
  plain.remove();
});

test("T2.h3: the thread and body share one timeline: same timing, delay and keyframe stops", () => {
  const sharedAt = css.indexOf("\n.halloween-spider-thread,\n.halloween-spider-body {");
  expect(sharedAt).toBeGreaterThan(0);
  const shared = css.slice(sharedAt, css.indexOf("}", sharedAt));
  expect(shared).toMatch(/animation: [^;]*ease-in-out infinite;\s*animation-duration: inherit;\s*animation-delay: inherit;/);
  const scurryAt = css.indexOf("\n.halloween-spider.is-scurry .halloween-spider-thread,\n.halloween-spider.is-scurry .halloween-spider-body {");
  expect(scurryAt).toBeGreaterThan(0);
  const scurry = css.slice(scurryAt, css.indexOf("}", scurryAt));
  expect(scurry).toContain("animation-duration: 7s");
  expect(scurry).toContain("animation-timing-function: linear");
  expect(scurry).toContain("animation-delay: 0s");
  expect(rule(".halloween-spider-thread")).toContain("animation-name: halloween-drop;");
  expect(rule(".halloween-spider-body")).toContain("animation-name: halloween-drop-body;");
  expect(rule(".halloween-spider.is-scurry .halloween-spider-thread")).toContain("animation-name: halloween-up;");
  expect(rule(".halloween-spider.is-scurry .halloween-spider-body")).toContain("animation-name: halloween-up-body;");
  const frames = (name: string) => {
    const at = css.indexOf(`@keyframes ${name} {`);
    return css.slice(at, css.indexOf("\n}\n", at));
  };
  const stops = (name: string) => [...frames(name).matchAll(/\n {2}([\d%,\s]+) \{/g)].map((m) => m[1].replace(/\s+/g, ""));
  expect(stops("halloween-drop").length).toBeGreaterThan(1);
  expect(stops("halloween-drop-body")).toEqual(stops("halloween-drop"));
  expect(stops("halloween-up-body")).toEqual(stops("halloween-up"));
  // Nothing animates layout or SVG geometry: transforms, and the twitch is opacity.
  for (const n of ["halloween-drop", "halloween-drop-body", "halloween-up", "halloween-up-body", "halloween-twitch"]) {
    expect(frames(n)).not.toMatch(/\b(height|width|top|left|rotate)\b[:(]/);
  }
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

test("T2.m: the document listener goes on unmount and on leaving the theme, and StrictMode leaves one", async () => {
  const add = vi.spyOn(document, "addEventListener");
  const remove = vi.spyOn(document, "removeEventListener");
  const net = () =>
    add.mock.calls.filter((c) => c[0] === "pointerdown").length -
    remove.mock.calls.filter((c) => c[0] === "pointerdown").length;
  const a = render(<HalloweenDecor />);
  expect(net()).toBe(1);
  a.unmount();
  expect(net()).toBe(0);
  const b = render(<HalloweenDecor />);
  expect(net()).toBe(1);
  await act(async () => root.setAttribute("data-theme", "ocean"));
  expect(net()).toBe(0);
  b.unmount();
  await act(async () => root.setAttribute("data-theme", "halloween"));
  const c = render(
    <StrictMode>
      <HalloweenDecor />
    </StrictMode>,
  );
  expect(net()).toBe(1);
  c.unmount();
  expect(net()).toBe(0);
  add.mockRestore();
  remove.mockRestore();
});

function stubMedia(initial: boolean) {
  const listeners = new Set<() => void>();
  const mq = {
    matches: initial,
    media: "",
    addEventListener: (_: string, l: () => void) => void listeners.add(l),
    removeEventListener: (_: string, l: () => void) => void listeners.delete(l),
  };
  vi.stubGlobal("matchMedia", () => mq);
  return {
    set: (v: boolean) => {
      mq.matches = v;
      listeners.forEach((l) => l());
    },
    listening: () => listeners.size > 0,
  };
}

test("T2.n: bats start and stop live when reduced motion changes", () => {
  vi.useFakeTimers();
  const media = stubMedia(true);
  const { container, unmount } = render(<HalloweenDecor random={sequence()} />);
  const bat = container.querySelector<HTMLElement>(".halloween-bat")!;
  expect(vi.getTimerCount()).toBe(0);
  expect(bat.style.transform).toBe("");
  act(() => media.set(false));
  expect(bat.style.transform).toMatch(/^translate\(/);
  expect(vi.getTimerCount()).toBeGreaterThan(0);
  act(() => media.set(true));
  expect(vi.getTimerCount()).toBe(0);
  unmount();
  expect(media.listening()).toBe(false);
});

test("T2.o: a resize sends a bat that is now outside the window to a point in view, and is cleaned up", () => {
  vi.useFakeTimers();
  const remove = vi.spyOn(window, "removeEventListener");
  const { container, unmount } = render(<HalloweenDecor random={sequence()} />);
  const bat = container.querySelector<HTMLElement>(".halloween-bat")!;
  const before = bat.style.transform;
  const w = window.innerWidth;
  const h = window.innerHeight;
  Object.assign(window, { innerWidth: 120, innerHeight: 100 });
  act(() => void window.dispatchEvent(new Event("resize")));
  const m = bat.style.transform.match(/translate\((-?[\d.]+)px, (-?[\d.]+)px\)/)!;
  expect(bat.style.transform).not.toBe(before);
  expect(Number(m[1])).toBeLessThanOrEqual(120);
  expect(Number(m[2])).toBeLessThanOrEqual(100);
  Object.assign(window, { innerWidth: w, innerHeight: h });
  unmount();
  expect(remove.mock.calls.some((c) => c[0] === "resize")).toBe(true);
  remove.mockRestore();
});
