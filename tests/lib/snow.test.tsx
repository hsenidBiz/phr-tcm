import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import SnowLayer from "../../src/components/SnowLayer";
import Settings from "../../src/screens/Settings";
import { resetExtrasStore, setExtrasForTest } from "../../src/lib/extras";
import { snowSeason } from "../../src/lib/season";
import { SNOW_OFF_KEY, getSnowOff, setSnowOff, snowShows } from "../../src/lib/snow";

const capture = vi.hoisted(() => ({ on: false }));
vi.mock("../../src/dev/capture", () => ({ isCaptureMode: () => capture.on }));

const root = document.documentElement;
const DEC1 = new Date(2026, 11, 1, 9).getTime();
const NOV15 = new Date(2026, 10, 15, 9).getTime();
const base = { now: DEC1, theme: "light", snowOff: false, unlocked: false, capture: false, reducedMotion: false };

beforeEach(() => {
  capture.on = false;
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  clearMocks();
  resetExtrasStore();
  localStorage.clear();
  root.removeAttribute("data-theme");
});

describe("the window", () => {
  test("snowSeason: 1 December 00:00 through 31 December 23:59:59 local", () => {
    expect(snowSeason(new Date(2026, 10, 30, 23, 59).getTime())).toBe(false);
    expect(snowSeason(new Date(2026, 11, 1, 0, 0).getTime())).toBe(true);
    expect(snowSeason(new Date(2026, 11, 31, 23, 59, 59).getTime())).toBe(true);
    expect(snowSeason(new Date(2027, 0, 1, 0, 0).getTime())).toBe(false);
  });
});

describe("the decision", () => {
  test("1 December in a light theme: snow", () => expect(snowShows(base)).toBe(true));
  test("Halloween: none", () => expect(snowShows({ ...base, theme: "halloween" })).toBe(false));
  test("the Christmas theme: none here, it has its own", () => expect(snowShows({ ...base, theme: "christmas" })).toBe(false));
  test("switched off: none", () => expect(snowShows({ ...base, snowOff: true })).toBe(false));
  test("a docs capture: none", () => expect(snowShows({ ...base, capture: true })).toBe(false));
  test("reduced motion: none", () => expect(snowShows({ ...base, reducedMotion: true })).toBe(false));
  test("15 November with the unlock: snow", () => expect(snowShows({ ...base, now: NOV15, unlocked: true })).toBe(true));
  test("15 November without it: none", () => expect(snowShows({ ...base, now: NOV15 })).toBe(false));
});

describe("the pref", () => {
  test("defaults to on, saves and clears", () => {
    expect(getSnowOff()).toBe(false);
    setSnowOff(true);
    expect(localStorage.getItem(SNOW_OFF_KEY)).toBe("1");
    expect(getSnowOff()).toBe(true);
    setSnowOff(false);
    expect(getSnowOff()).toBe(false);
  });
});

describe("SnowLayer", () => {
  test("renders flakes on 1 December in a light theme, and not under Halloween", async () => {
    vi.useFakeTimers({ toFake: ["Date", "setInterval", "clearInterval"] });
    vi.setSystemTime(DEC1);
    const { container } = render(<SnowLayer />);
    const layer = container.querySelector(".snow-layer");
    expect(layer?.getAttribute("aria-hidden")).toBe("true");
    expect(container.querySelectorAll(".christmas-flake")).toHaveLength(20);
    expect(container.querySelector(".christmas-snowman")).toBeNull();
    expect(container.querySelector(".christmas-tree")).toBeNull();
    await act(async () => root.setAttribute("data-theme", "halloween"));
    expect(container.innerHTML).toBe("");
    await act(async () => root.setAttribute("data-theme", "christmas"));
    expect(container.innerHTML).toBe("");
  });

  test("starts at midnight on 1 December and stops on 1 January without a restart", () => {
    vi.useFakeTimers({ toFake: ["Date", "setInterval", "clearInterval"] });
    vi.setSystemTime(new Date(2026, 10, 30, 23, 59, 30));
    const { container } = render(<SnowLayer />);
    expect(container.innerHTML).toBe("");
    act(() => vi.advanceTimersByTime(60_000));
    expect(container.querySelector(".snow-layer")).not.toBeNull();
    vi.setSystemTime(new Date(2026, 11, 31, 23, 59, 30));
    act(() => vi.advanceTimersByTime(60_000));
    expect(container.innerHTML).toBe("");
  });

  test("shows in November with the unlock; hides when switched off or under reduced motion", async () => {
    vi.useFakeTimers({ toFake: ["Date", "setInterval", "clearInterval"] });
    vi.setSystemTime(NOV15);
    const { container } = render(<SnowLayer />);
    expect(container.innerHTML).toBe("");
    await act(async () => setExtrasForTest(true));
    expect(container.querySelector(".snow-layer")).not.toBeNull();
    await act(async () => setSnowOff(true));
    expect(container.innerHTML).toBe("");
    await act(async () => setSnowOff(false));
    expect(container.querySelector(".snow-layer")).not.toBeNull();
  });

  test("reduced motion and a docs capture draw nothing", () => {
    vi.useFakeTimers({ toFake: ["Date", "setInterval", "clearInterval"] });
    vi.setSystemTime(DEC1);
    vi.stubGlobal("matchMedia", () => ({ matches: true, addEventListener() {}, removeEventListener() {} }));
    expect(render(<SnowLayer />).container.innerHTML).toBe("");
    vi.unstubAllGlobals();
    capture.on = true;
    expect(render(<SnowLayer />).container.innerHTML).toBe("");
  });
});

describe("the snow CSS", () => {
  const css = readFileSync(resolve(__dirname, "../../src/index.css"), "utf8").split(String.fromCharCode(13)).join("");

  test("flake colours are theme tokens, set for light and for the dark themes; reduced motion hides the layer", () => {
    expect(css).toMatch(/:root \{[^}]*--color-snow: #[0-9a-f]{6};[^}]*--color-snow-edge:/);
    expect(css).toMatch(/\n\.dark \{[^}]*--color-snow: #f4f8ff;/);
    expect(css).toContain("background: var(--color-snow)");
    const reduce = css.slice(css.indexOf("@media (prefers-reduced-motion: reduce) {\n  .christmas-twinkle"));
    expect(reduce.slice(0, reduce.indexOf("/* ── Motion"))).toContain(".snow-layer");
  });
});

describe("the Settings switch", () => {
  function renderSettings() {
    mockIPC((cmd) => {
      if (cmd === "get_extras_unlocked") return false;
      if (cmd === "app_logs") return [];
      if (cmd === "app_log_dir") return "C:\\logs";
      return undefined;
    });
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    return render(
      <QueryClientProvider client={qc}>
        <Settings org="acme" project="Web" homeProject="Web" onHomeProjectChange={() => {}} onAllProjectsChange={() => {}} />
      </QueryClientProvider>,
    );
  }

  test("shows in December, and toggling saves snowOff", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(DEC1);
    renderSettings();
    const sw = await screen.findByRole("switch", { name: "Snow" });
    expect(screen.getByText("Falling snow in December. Turn it off any time.")).toBeInTheDocument();
    expect(sw).toBeChecked();
    fireEvent.click(sw);
    expect(getSnowOff()).toBe(true);
    expect(sw).not.toBeChecked();
    fireEvent.click(sw);
    expect(getSnowOff()).toBe(false);
  });

  test("shows with the unlock in November, and is absent without it", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(NOV15);
    renderSettings();
    await screen.findByRole("heading", { name: "Appearance" });
    expect(screen.queryByRole("switch", { name: "Snow" })).toBeNull();
    await act(async () => setExtrasForTest(true));
    expect(await screen.findByRole("switch", { name: "Snow" })).toBeInTheDocument();
  });

  test("a docs capture never shows it", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(DEC1);
    capture.on = true;
    renderSettings();
    await screen.findByRole("heading", { name: "Appearance" });
    expect(screen.queryByRole("switch", { name: "Snow" })).toBeNull();
  });
});
