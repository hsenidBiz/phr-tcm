import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { resetModulesStore, setModulesForTest, type Module } from "../../lib/modules";
import { isNewNav, resetNewNavForTest } from "../../lib/newNav";
import Modules from "./index";

// WebGL never runs in jsdom: a stand-in records what the board asks the
// electric layer to draw. (The real component's own guard is tested beside it.)
vi.mock("../../components/ElectricLogo", () => ({
  default: (p: { src: string; color: string; glowColor: string; theme: string; interactive: boolean; fitTo: string }) => (
    <div
      data-testid="electric"
      data-src={p.src}
      data-color={p.color}
      data-glow={p.glowColor}
      data-theme-mode={p.theme}
      data-interactive={String(p.interactive)}
      data-fit={p.fitTo}
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

test("lists the five modules with their statuses", () => {
  show(["db_access"]);
  expect(screen.getAllByRole("switch")).toHaveLength(5);
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
