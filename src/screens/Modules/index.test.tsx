import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { resetModulesStore, setModulesForTest, type Module } from "../../lib/modules";
import { isNewNav, resetNewNavForTest } from "../../lib/newNav";
import Modules from "./index";

// WebGL never runs in jsdom: a stand-in records what the board asks the
// electric layer to draw. (The real component's own guard is tested beside it.)
vi.mock("../../components/ElectricLogo", () => ({
  default: (p: { src: string; color: string; glowColor: string; theme: string; interactive: boolean; fitTo: string; raster: number }) => (
    <div
      data-testid="electric"
      data-src={p.src}
      data-color={p.color}
      data-glow={p.glowColor}
      data-theme-mode={p.theme}
      data-interactive={String(p.interactive)}
      data-fit={p.fitTo}
      data-raster={p.raster}
    />
  ),
}));

const SVG_URL = "data:image/svg+xml;charset=utf-8,";

/** The paths the electric layer is tracing, as path data. */
function electricPaths(): string[] | null {
  const el = screen.queryByTestId("electric");
  if (!el) return null;
  const src = el.getAttribute("data-src")!;
  expect(src.startsWith(SVG_URL)).toBe(true);
  const svg = decodeURIComponent(src.slice(SVG_URL.length));
  expect(svg).toContain('viewBox="0 0 800 750"');
  return [...svg.matchAll(/ d="([^"]+)"/g)].map((m) => m[1]);
}

/** The board's own path data for these modules. */
const edges = (mods: Module[]) =>
  mods.map((m) => document.querySelector(`[data-edge-line="${m}"]`)!.getAttribute("d"));

afterEach(() => {
  clearMocks();
  resetModulesStore();
  localStorage.clear();
  resetNewNavForTest();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

function show(on: Module[], onBack = vi.fn()) {
  act(() => setModulesForTest(on));
  return { onBack, ...render(<Modules onBack={onBack} />) };
}

const sw = (name: RegExp | string) => screen.getByRole("switch", { name });

test("lists the six modules with their statuses", () => {
  show(["db_access"]);
  expect(screen.getAllByRole("switch")).toHaveLength(6);
  // Crystal Reports hangs off Core, so it is never locked.
  expect(sw(/^Crystal Reports/)).toHaveAccessibleName("Crystal Reports Off · click to switch on");
  expect(sw(/^Crystal Reports/)).not.toHaveAttribute("aria-disabled", "true");
  expect(screen.getByText(/Lets your AI assistant open, format and preview Crystal Reports files/)).toBeInTheDocument();
  expect(sw(/^Database Access/)).toHaveAttribute("aria-checked", "true");
  expect(sw(/^Database Access/)).toHaveAccessibleName(/On/);
  expect(sw(/^Writing Style/)).toHaveAttribute("aria-checked", "false");
  expect(sw(/^Writing Style/)).toHaveAccessibleName("Writing Style Off · click to switch on");
  expect(sw(/^Auto Run/)).toHaveAccessibleName("Auto Run Off · click to switch on");
  expect(sw(/^API Templates/)).toHaveAccessibleName("API Templates Locked · needs Auto Run");
  expect(screen.getByText("Core")).toBeInTheDocument();
  expect(screen.getByText("Test cases, suites, runs and the board")).toBeInTheDocument();
  expect(screen.getByText(/Connect TCM to your test environments' databases/)).toBeInTheDocument();
});

test("a locked switch is aria-disabled, names its parent, and does nothing", () => {
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(cmd);
    return [];
  });
  show([]);
  const auto = sw(/^Auto Run/);
  expect(auto).toHaveAttribute("aria-disabled", "true");
  expect(auto).toHaveAccessibleName("Auto Run Locked · needs Database Access");
  expect(auto).toHaveAccessibleDescription(/Switch on Database Access first/);
  fireEvent.click(auto);
  expect(calls).not.toContain("modules_set");
  expect(auto).toHaveAttribute("aria-checked", "false");
});

test("switching on asks Rust, and follows its answer", async () => {
  let args: unknown;
  mockIPC((cmd, a) => {
    if (cmd === "modules_set") {
      args = a;
      return ["db_access", "auto_run"];
    }
  });
  show(["db_access"]);
  fireEvent.click(sw(/^Auto Run/));
  await waitFor(() => expect(sw(/^Auto Run/)).toHaveAttribute("aria-checked", "true"));
  expect(args).toEqual({ module: "auto_run", on: true });
  expect(sw(/^API Templates/)).not.toHaveAttribute("aria-disabled", "true");
});

test("Space and Enter toggle a switch from the keyboard", async () => {
  mockIPC((cmd) => (cmd === "modules_set" ? ["writing_style"] : undefined));
  show([]);
  const ws = sw(/^Writing Style/);
  expect(ws.tagName).toBe("BUTTON");
  ws.focus();
  expect(ws).toHaveFocus();
  fireEvent.click(ws); // a button turns Space/Enter into this click
  await waitFor(() => expect(ws).toHaveAttribute("aria-checked", "true"));
});

test("switching Auto Run off asks first and names what goes with it", async () => {
  const calls: unknown[] = [];
  mockIPC((cmd, a) => {
    if (cmd === "modules_set") {
      calls.push(a);
      return ["db_access"];
    }
  });
  show(["db_access", "auto_run", "api_templates", "playwright"]);
  fireEvent.click(sw(/^Auto Run/));
  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).getByText("Switching off Auto Run also switches off API Templates and Playwright.")).toBeInTheDocument();
  expect(calls).toHaveLength(0);
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(calls).toHaveLength(0);

  fireEvent.click(sw(/^Auto Run/));
  fireEvent.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Switch off" }));
  await waitFor(() => expect(sw(/^Auto Run/)).toHaveAttribute("aria-checked", "false"));
  expect(calls).toEqual([{ module: "auto_run", on: false }]);
  expect(sw(/^Playwright/)).toHaveAttribute("aria-checked", "false");
});

test("a module with nothing under it switches off without asking", async () => {
  mockIPC((cmd) => (cmd === "modules_set" ? ["db_access", "auto_run"] : undefined));
  show(["db_access", "auto_run", "playwright"]);
  fireEvent.click(sw(/^Playwright/));
  await waitFor(() => expect(sw(/^Playwright/)).toHaveAttribute("aria-checked", "false"));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});

test("a refusal from Rust is shown and nothing changes", async () => {
  const busy = "Auto Run can't be switched off while a run is going - stop the run first.";
  mockIPC((cmd) => {
    if (cmd === "modules_set") throw busy;
  });
  show(["db_access", "auto_run"]);
  fireEvent.click(sw(/^Auto Run/));
  expect(await screen.findByRole("alert")).toHaveTextContent(busy);
  expect(sw(/^Auto Run/)).toHaveAttribute("aria-checked", "true");
});

test("the light runs before the card lights, unless motion is reduced", async () => {
  vi.useFakeTimers();
  mockIPC((cmd) => (cmd === "modules_set" ? ["writing_style"] : undefined));
  show([]);
  fireEvent.click(sw(/^Writing Style/));
  await act(async () => {});
  const node = sw(/^Writing Style/).closest("[data-module]")!;
  expect(sw(/^Writing Style/)).toHaveAttribute("aria-checked", "true");
  expect(node).toHaveAttribute("data-lit", "false");
  expect(document.querySelector('[data-edge="writing_style"]')).toHaveAttribute("data-running", "true");
  await act(async () => {
    vi.advanceTimersByTime(900);
  });
  expect(node).toHaveAttribute("data-lit", "true");
  expect(document.querySelector('[data-edge="writing_style"]')).toHaveAttribute("data-running", "false");
});

test("with reduced motion the end state shows at once", async () => {
  vi.stubGlobal("matchMedia", (q: string) => ({ matches: q.includes("reduce"), media: q, addEventListener() {}, removeEventListener() {} }));
  mockIPC((cmd) => (cmd === "modules_set" ? ["writing_style"] : undefined));
  show([]);
  fireEvent.click(sw(/^Writing Style/));
  await waitFor(() => expect(sw(/^Writing Style/).closest("[data-module]")).toHaveAttribute("data-lit", "true"));
  expect(document.querySelector('[data-edge="writing_style"]')).toHaveAttribute("data-running", "false");
});

test("the page fills the area it is given, so only the board scrolls", () => {
  const { container } = show([]);
  const root = container.firstElementChild as HTMLElement;
  for (const c of ["flex", "min-h-0", "flex-1", "flex-col", "pb-6"]) expect(root, c).toHaveClass(c);
  // The header and notes keep their height; the board takes what is left.
  const frame = document.querySelector<HTMLElement>("[data-board-frame]")!;
  for (const c of ["min-h-0", "flex-1", "overflow-auto"]) expect(frame, c).toHaveClass(c);
  // A height worked out from the viewport is what made a second scrollbar.
  expect(frame.className).not.toMatch(/100vh|calc\(/);
  expect(frame.className).not.toMatch(/(^|\s)(h-|min-h-80)/);
  for (const el of [root.querySelector(":scope > div"), root.querySelector(":scope > p")]) {
    expect(el, "header / note").toHaveClass("shrink-0");
  }
});

test("the paths are decorative and Back returns", () => {
  const { onBack } = show([]);
  expect(document.querySelector("svg.mod-edges")).toHaveAttribute("aria-hidden", "true");
  fireEvent.click(screen.getByRole("button", { name: /Back to Settings/ }));
  expect(onBack).toHaveBeenCalled();
});

test("a path runs from the parent card's bottom edge to the child card's top edge", () => {
  // jsdom lays nothing out, so give every card a height the paths can read.
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(120);
  show(["db_access"]);
  const card = (m: Module) => document.querySelector<HTMLElement>(`[data-module="${m}"]`)!;
  const top = (m: Module) => parseFloat(card(m).style.top);
  const ends = (m: Module) => {
    const nums = document.querySelector(`[data-edge-line="${m}"]`)!.getAttribute("d")!.match(/-?[\d.]+/g)!.map(Number);
    return { fromY: nums[1], toY: nums[nums.length - 1] };
  };
  // Behind a see-through locked card, a centre-to-centre line showed through it.
  expect(ends("playwright")).toEqual({ fromY: top("auto_run") + 120, toY: top("playwright") });
  expect(ends("auto_run")).toEqual({ fromY: top("db_access") + 120, toY: top("auto_run") });
  vi.restoreAllMocks();
});

test("no two cards overlap", () => {
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(140);
  show(["crystal_reports"]);
  const rect = (m: Module) => {
    const el = document.querySelector<HTMLElement>(`[data-module="${m}"]`)!;
    const left = parseFloat(el.style.left) - 140;
    return { m, left, right: left + 280, top: parseFloat(el.style.top), bottom: parseFloat(el.style.top) + 140 };
  };
  const cards = (["db_access", "writing_style", "auto_run", "api_templates", "playwright", "crystal_reports"] as Module[]).map(rect);
  for (const a of cards) {
    expect(a.left, a.m).toBeGreaterThanOrEqual(0);
    expect(a.right, a.m).toBeLessThanOrEqual(800);
    expect(a.bottom, a.m).toBeLessThanOrEqual(750);
    for (const b of cards) {
      if (a.m >= b.m) continue;
      const apart = a.right <= b.left || b.right <= a.left || a.bottom <= b.top || b.bottom <= a.top;
      expect(apart, `${a.m} / ${b.m}`).toBe(true);
    }
  }
  // It still ends on the top centre of its own card.
  const d = document.querySelector(`[data-edge-line="crystal_reports"]`)!.getAttribute("d")!;
  expect(d.endsWith(`${rect("crystal_reports").left + 140},${rect("crystal_reports").top}`)).toBe(true);
  vi.restoreAllMocks();
});

// --- the turn from Core to Crystal Reports -------------------------------

type Pt = { x: number; y: number };
type Seg = { kind: "L" | "C"; pts: Pt[] };

/** The M / L / C commands of a path, as segments (each starts where the last ended). */
function parsePath(d: string): Seg[] {
  const toks = d.match(/[MLC]|-?[0-9.]+,-?[0-9.]+/g)!;
  const segs: Seg[] = [];
  let cur: Pt = { x: 0, y: 0 };
  let kind = "M";
  let args: Pt[] = [];
  const flush = () => {
    if (args.length === 0) return;
    if (kind === "M") cur = args[0];
    else {
      segs.push({ kind: kind as "L" | "C", pts: [cur, ...args] });
      cur = args[args.length - 1];
    }
    args = [];
  };
  for (const t of toks) {
    if (/^[MLC]$/.test(t)) {
      flush();
      kind = t;
    } else {
      const [x, y] = t.split(",").map(Number);
      args.push({ x, y });
    }
  }
  flush();
  return segs;
}

const at = (s: Seg, t: number): Pt => {
  const p = s.pts;
  const u = 1 - t;
  if (s.kind === "L") return { x: p[0].x + (p[1].x - p[0].x) * t, y: p[0].y + (p[1].y - p[0].y) * t };
  const w = [u * u * u, 3 * u * u * t, 3 * u * t * t, t * t * t];
  return { x: w[0] * p[0].x + w[1] * p[1].x + w[2] * p[2].x + w[3] * p[3].x, y: w[0] * p[0].y + w[1] * p[1].y + w[2] * p[2].y + w[3] * p[3].y };
};

/** The smallest radius of curvature along the path (a straight line has none). */
function minRadius(segs: Seg[]): number {
  let min = Infinity;
  for (const s of segs) {
    if (s.kind === "L") continue;
    const [p0, p1, p2, p3] = s.pts;
    for (let i = 0; i <= 1000; i++) {
      const t = i / 1000;
      const u = 1 - t;
      const dx = 3 * u * u * (p1.x - p0.x) + 6 * u * t * (p2.x - p1.x) + 3 * t * t * (p3.x - p2.x);
      const dy = 3 * u * u * (p1.y - p0.y) + 6 * u * t * (p2.y - p1.y) + 3 * t * t * (p3.y - p2.y);
      const ddx = 6 * u * (p2.x - 2 * p1.x + p0.x) + 6 * t * (p3.x - 2 * p2.x + p1.x);
      const ddy = 6 * u * (p2.y - 2 * p1.y + p0.y) + 6 * t * (p3.y - 2 * p2.y + p1.y);
      const cross = Math.abs(dx * ddy - dy * ddx);
      if (cross > 1e-9) min = Math.min(min, Math.pow(dx * dx + dy * dy, 1.5) / cross);
    }
  }
  return min;
}

for (const cardHeight of [120, 134, 140, 153]) {
  test(`the turn from Core to Crystal Reports is one smooth curve through the gap (cards ${cardHeight}px tall)`, () => {
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(cardHeight);
    show(["crystal_reports"]);
    const card = (m: Module) => {
      const el = document.querySelector<HTMLElement>(`[data-module="${m}"]`)!;
      return { x: parseFloat(el.style.left), y: parseFloat(el.style.top) };
    };
    const core = { x: 400, y: 166 };
    const to = card("crystal_reports");
    const left = card("db_access");
    const right = card("writing_style");
    // The first row's cards are 280 wide, centred on their x.
    const MARGIN = 2.5 + 8; // the widest stroke's half width, plus 8
    const rects = [left, right].map((c) => ({ l: c.x - 140, r: c.x + 140, t: c.y, b: c.y + cardHeight }));
    const distance = (p: Pt, k: { l: number; r: number; t: number; b: number }) =>
      Math.hypot(Math.max(k.l - p.x, 0, p.x - k.r), Math.max(k.t - p.y, 0, p.y - k.b));

    const d = document.querySelector(`[data-edge-line="crystal_reports"]`)!.getAttribute("d")!;
    const segs = parsePath(d);

    // (c) The same two ends as before, and the start leaves straight down.
    expect(segs[0].pts[0]).toEqual(core);
    expect(segs[0].pts[1].x).toBe(core.x);
    expect(segs[segs.length - 1].pts[3]).toEqual(to);
    expect(segs[segs.length - 1].pts[2].x).toBe(to.x);
    expect(segs[segs.length - 1].pts[2].y).toBeLessThan(to.y);

    // (a) Every point of it keeps the stroke's half width plus 8px from each
    // first-row card (the whole rectangle, corners included), and passes
    // between them, not round the outside.
    let nearest = Infinity;
    for (const seg of segs) {
      for (let i = 0; i <= 2000; i++) {
        const p = at(seg, i / 2000);
        for (const k of rects) {
          const dist = distance(p, k);
          nearest = Math.min(nearest, dist);
          expect(dist, `at ${p.x.toFixed(1)},${p.y.toFixed(1)}`).toBeGreaterThanOrEqual(MARGIN - 1e-6);
        }
        if (p.y <= rects[0].b) expect(p.x, `x at y=${p.y}`).toBeGreaterThan(rects[0].r);
        if (p.y <= rects[1].b) expect(p.x, `x at y=${p.y}`).toBeLessThan(rects[1].l);
      }
    }
    expect(nearest).toBeLessThan(MARGIN + 6); // it does use the gap rather than keep far away

    // (b) The old path: straight down to just under the row, then a bend with
    // both control points halfway down to the card. The new one bends far
    // more gently: its sharpest turn has a radius over 20px (the old one's is
    // under 10), and several times the old one's.
    const clear = Math.max(left.y, right.y) + cardHeight + 12;
    const mid = (clear + to.y) / 2;
    const old = parsePath(`M${core.x},${core.y} L${core.x},${clear} C${core.x},${mid} ${to.x},${mid} ${to.x},${to.y}`);
    expect(minRadius(segs)).toBeGreaterThanOrEqual(20);
    expect(minRadius(segs)).toBeGreaterThanOrEqual(2.5 * minRadius(old));

    // It never runs back up, and one curve joins the next without a kink.
    for (const s of segs) for (let i = 0; i < 1000; i++) expect(at(s, (i + 1) / 1000).y).toBeGreaterThanOrEqual(at(s, i / 1000).y - 1e-9);
    for (let i = 1; i < segs.length; i++) {
      const a = segs[i - 1].pts;
      const b = segs[i].pts;
      const before = { x: a[3].x - a[2].x, y: a[3].y - a[2].y };
      const after = { x: b[1].x - b[0].x, y: b[1].y - b[0].y };
      const cos = (before.x * after.x + before.y * after.y) / (Math.hypot(before.x, before.y) * Math.hypot(after.x, after.y));
      expect(cos).toBeGreaterThan(0.999);
    }
    vi.restoreAllMocks();
  });

  test(`every other module's path is unchanged (cards ${cardHeight}px tall)`, () => {
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(cardHeight);
    show([]);
    const card = (m: Module) => {
      const el = document.querySelector<HTMLElement>(`[data-module="${m}"]`)!;
      return { x: parseFloat(el.style.left), y: parseFloat(el.style.top) };
    };
    const core = { x: 400, y: 166 };
    const curve = (from: Pt, to: Pt) => {
      const mid = (from.y + to.y) / 2;
      return `M${from.x},${from.y} C${from.x},${mid} ${to.x},${mid} ${to.x},${to.y}`;
    };
    const below = (m: Module): Pt => ({ x: card(m).x, y: card(m).y + cardHeight });
    const expected: [Module, string][] = [
      ["db_access", curve(core, card("db_access"))],
      ["writing_style", curve(core, card("writing_style"))],
      ["auto_run", curve(below("db_access"), card("auto_run"))],
      ["api_templates", curve(below("auto_run"), card("api_templates"))],
      ["playwright", curve(below("auto_run"), card("playwright"))],
    ];
    for (const [m, d] of expected) expect(edges([m])[0], m).toBe(d);
    vi.restoreAllMocks();
  });
}

test("while one change saves, every switch waits", async () => {
  const sets: unknown[] = [];
  let release!: (v: Module[]) => void;
  mockIPC((cmd, args) => {
    if (cmd !== "modules_set") return [];
    sets.push(args);
    return new Promise<Module[]>((r) => (release = r));
  });
  show(["db_access"]);
  fireEvent.click(sw(/^Auto Run/));
  await waitFor(() => expect(sets).toHaveLength(1));
  expect(sw(/^Writing Style/)).toHaveAttribute("aria-disabled", "true");
  fireEvent.click(sw(/^Writing Style/));
  expect(sets).toHaveLength(1);
  await act(async () => release(["db_access", "auto_run"]));
  await waitFor(() => expect(sw(/^Writing Style/)).not.toHaveAttribute("aria-disabled"));
});

test("the core's name and line are read out", () => {
  show([]);
  expect(screen.getByText("Core").closest("[aria-hidden]")).toBeNull();
  expect(screen.getByText("Test cases, suites, runs and the board").closest("[aria-hidden]")).toBeNull();
});

// A person's own switch-on marks the sidebar row it reveals; switching it
// off again takes the mark away.
test("switching a module on here marks its sidebar row, and off clears it", async () => {
  let answer: Module[] = ["db_access", "auto_run"];
  mockIPC((cmd) => (cmd === "modules_set" ? answer : undefined));
  show(["db_access"]);
  fireEvent.click(sw(/^Auto Run/));
  await waitFor(() => expect(isNewNav("autorun")).toBe(true));
  expect(isNewNav("ai")).toBe(false);

  answer = ["db_access"];
  fireEvent.click(sw(/^Auto Run/));
  await waitFor(() => expect(isNewNav("autorun")).toBe(false));
});

test("Database Access and Writing Style mark AI Bridge, where their cards go", async () => {
  mockIPC((cmd) => (cmd === "modules_set" ? ["writing_style"] : undefined));
  show([]);
  fireEvent.click(sw(/^Writing Style/));
  await waitFor(() => expect(isNewNav("ai")).toBe(true));
});

test("a refused switch marks nothing", async () => {
  mockIPC((cmd) => {
    if (cmd === "modules_set") throw "no";
  });
  show(["db_access"]);
  fireEvent.click(sw(/^Auto Run/));
  expect(await screen.findByRole("alert")).toBeInTheDocument();
  expect(isNewNav("autorun")).toBe(false);
});

test("power flows only down the paths of modules that are on", () => {
  show(["db_access", "auto_run"]);
  // One electric layer for the whole board, tracing exactly the lit paths.
  expect(screen.getAllByTestId("electric")).toHaveLength(1);
  expect(electricPaths()).toEqual(edges(["db_access", "auto_run"]));
  const lit = [...document.querySelectorAll(".mod-edge-lit")].map((p) => p.getAttribute("data-edge-line"));
  expect(lit.sort()).toEqual(["auto_run", "db_access"]);
  // It is decoration only, never in the way of a click, and maps the whole
  // board picture onto the board.
  const layer = document.querySelector("[data-electric]")!;
  expect(layer).toHaveAttribute("aria-hidden", "true");
  expect(layer).toHaveClass("pointer-events-none", "absolute", "inset-0");
  expect(screen.getByTestId("electric")).toHaveAttribute("data-interactive", "false");
  expect(screen.getByTestId("electric")).toHaveAttribute("data-fit", "image");
  // Traced at the board's widest on-screen size, so curves stay smooth.
  expect(screen.getByTestId("electric")).toHaveAttribute("data-raster", "1440");
  expect(document.querySelector("svg.mod-edges")).toHaveAttribute("aria-hidden", "true");
});

test("no electric layer when nothing is lit", () => {
  show([]);
  expect(screen.queryByTestId("electric")).toBeNull();
  expect(document.querySelector("[data-electric]")).toBeNull();
});

test("the electric layer follows a module switching on", async () => {
  vi.useFakeTimers();
  show(["db_access"]);
  expect(electricPaths()).toEqual(edges(["db_access"]));
  act(() => setModulesForTest(["db_access", "auto_run"]));
  // The light runs the path first; the power arrives as the card lights.
  expect(electricPaths()).toEqual(edges(["db_access"]));
  await act(async () => {
    vi.advanceTimersByTime(500);
  });
  expect(electricPaths()).toEqual(edges(["db_access", "auto_run"]));
});

test("switching the last module off fades the electric layer out over its drain", async () => {
  vi.useFakeTimers();
  show(["db_access"]);
  const layer = () => document.querySelector<HTMLElement>("[data-electric]");
  expect(layer()).toHaveAttribute("data-fading", "false");
  expect(layer()!.style.opacity).toBe("1");
  act(() => setModulesForTest([]));
  // Still there for the drain, still tracing what was lit, fading as it empties.
  expect(layer()).toHaveAttribute("data-fading", "true");
  expect(layer()!.style.opacity).toBe("0");
  expect(layer()!.style.transition).toBe("opacity 600ms ease-in");
  expect(electricPaths()).toEqual(edges(["db_access"]));
  await act(async () => {
    vi.advanceTimersByTime(650);
  });
  expect(layer()).toBeNull();
  expect(screen.queryByTestId("electric")).toBeNull();
});

test("with reduced motion there is no electric layer", () => {
  vi.stubGlobal("matchMedia", (q: string) => ({ matches: q.includes("reduce"), media: q, addEventListener() {}, removeEventListener() {} }));
  show(["db_access", "auto_run"]);
  expect(screen.queryByTestId("electric")).toBeNull();
  // The lit path is still drawn, as its steady end state.
  expect(document.querySelector('[data-edge-line="auto_run"]')).toHaveClass("mod-edge-lit");
});

test("the electricity takes its colours from the theme tokens, and follows a theme change", async () => {
  const root = document.documentElement;
  root.style.setProperty("--color-accent", "#111111");
  root.style.setProperty("--color-accent-hover", "#222222");
  root.style.setProperty("--color-on-accent", "#333333");
  try {
    show(["db_access"]);
    const el = () => screen.getByTestId("electric");
    expect(el()).toHaveAttribute("data-glow", "#111111");
    expect(el()).toHaveAttribute("data-color", "#333333");
    expect(el()).toHaveAttribute("data-theme-mode", "light");
    act(() => root.classList.add("dark"));
    await waitFor(() => expect(el()).toHaveAttribute("data-theme-mode", "dark"));
    expect(el()).toHaveAttribute("data-color", "#222222");
  } finally {
    root.classList.remove("dark");
    root.style.removeProperty("--color-accent");
    root.style.removeProperty("--color-accent-hover");
    root.style.removeProperty("--color-on-accent");
  }
});

test("switching off drains the power back up the path, the deepest first", async () => {
  vi.useFakeTimers();
  show(["db_access", "auto_run", "playwright"]);
  act(() => setModulesForTest(["db_access"]));
  const drain = (m: Module) => document.querySelector<SVGPathElement>(`[data-draining="${m}"]`);
  // Playwright hangs below Auto Run, so its path empties first.
  expect(drain("playwright")?.style.animationDelay).toBe("0ms");
  expect(drain("auto_run")?.style.animationDelay).toBe("450ms");
  expect(drain("db_access")).toBeNull();
  // A draining path carries no electricity, and its card has lost power at once.
  expect(electricPaths()).toEqual(edges(["db_access"]));
  const card = (m: Module) => document.querySelector(`[data-module="${m}"]`)!;
  expect(card("auto_run")).toHaveAttribute("data-lit", "false");
  expect(card("playwright")).toHaveAttribute("data-lit", "false");
  await act(async () => {
    vi.advanceTimersByTime(1100);
  });
  expect(drain("auto_run")).toBeNull();
  expect(drain("playwright")).toBeNull();
  expect(card("auto_run")).toHaveAttribute("data-lit", "false");
  expect(card("db_access")).toHaveAttribute("data-lit", "true");
});

test("with reduced motion a switch-off goes dark at once", () => {
  vi.stubGlobal("matchMedia", (q: string) => ({ matches: q.includes("reduce"), media: q, addEventListener() {}, removeEventListener() {} }));
  show(["db_access", "auto_run"]);
  act(() => setModulesForTest(["db_access"]));
  expect(document.querySelector("[data-draining]")).toBeNull();
  expect(document.querySelector('[data-module="auto_run"]')).toHaveAttribute("data-lit", "false");
});

test("switching off while it lights up stops the light-up and only drains", async () => {
  vi.useFakeTimers();
  show(["db_access", "auto_run"]);
  act(() => setModulesForTest(["db_access", "auto_run", "playwright"]));
  expect(document.querySelector('[data-edge="playwright"]')).toHaveAttribute("data-running", "true");
  act(() => setModulesForTest(["db_access", "auto_run"]));
  expect(document.querySelector('[data-edge="playwright"]')).toHaveAttribute("data-running", "false");
  expect(document.querySelector('[data-draining="playwright"]')).not.toBeNull();
  await act(async () => {
    vi.advanceTimersByTime(1500);
  });
  // The light-up's own timers must not light it again after it went off.
  expect(document.querySelector('[data-module="playwright"]')).toHaveAttribute("data-lit", "false");
  expect(document.querySelector('[data-draining="playwright"]')).toBeNull();
});
