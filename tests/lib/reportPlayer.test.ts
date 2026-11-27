/**
 * The run report's own video player (src-tauri/web/autorun-report-player.js).
 * It draws the same player as the app, from the timeline JSON in the page's
 * data blob. jsdom plays no media, so the video element's clock is stubbed.
 */
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, beforeAll, beforeEach, expect, test, vi } from "vitest";
import { fireEvent, within } from "@testing-library/dom";

const here = dirname(fileURLToPath(import.meta.url));
const src = readFileSync(resolve(here, "../../src-tauri/web/autorun-report-player.js"), "utf8");

type Mark = { label: string; at_ms: number };
type Step = { name: string; at_ms: number; failed: boolean; actions: Mark[] };
type Timeline = { title: string; length_ms: number | null; steps: Step[]; marks: Mark[] };
type Player = {
  open: (t: Timeline, src: string, opener?: HTMLElement | null) => void;
  close: () => void;
  isOpen: () => boolean;
  at: (starts: number[], ms: number) => number;
  target: (t: Timeline, ms: number, dir: -1 | 1) => number | null;
  percent: (ms: number, len: number) => number;
  clock: (ms: number) => string;
};
const player = () => (window as unknown as { ReportPlayer: Player }).ReportPlayer;

const timeline: Timeline = {
  title: "Video of #7 Leave request",
  length_ms: 20000,
  steps: [
    { name: "Sign in", at_ms: 0, failed: false, actions: [] },
    { name: "Step 1", at_ms: 4000, failed: false, actions: [{ label: "Click", at_ms: 4200 }] },
    { name: "Step 2", at_ms: 9000, failed: true, actions: [{ label: "Check: shows 'x'", at_ms: 9100 }] },
  ],
  marks: [],
};

let state: { time: number; rate: number; duration: number };
let play: ReturnType<typeof vi.fn>;
let pause: ReturnType<typeof vi.fn>;

beforeAll(() => {
  new Function(src)();
});

beforeEach(() => {
  state = { time: 0, rate: 1, duration: Number.NaN };
  play = vi.fn(() => Promise.resolve());
  pause = vi.fn();
  const proto = HTMLMediaElement.prototype;
  Object.defineProperty(proto, "play", { configurable: true, value: play });
  Object.defineProperty(proto, "pause", { configurable: true, value: pause });
  Object.defineProperty(proto, "load", { configurable: true, value: vi.fn() });
  Object.defineProperty(proto, "currentTime", { configurable: true, get: () => state.time, set: (v: number) => { state.time = v; } });
  Object.defineProperty(proto, "playbackRate", { configurable: true, get: () => state.rate, set: (v: number) => { state.rate = v; } });
  Object.defineProperty(proto, "duration", { configurable: true, get: () => state.duration });
  document.body.innerHTML = `<button id="opener">Watch</button>`;
});

afterEach(() => {
  player().close();
  document.getElementById("vp")?.remove();
});

const dialog = () => document.getElementById("vp") as HTMLElement;
const video = () => dialog().querySelector("video") as HTMLVideoElement;
const buttonNamed = (name: string) => within(dialog()).queryByRole("button", { name });
const press = (name: string) => fireEvent.click(buttonNamed(name)!);
const open = (t: Timeline = timeline) => player().open(t, "../shots/video-1-1.mp4", document.getElementById("opener"));

test("the script has no network call, no eval and never writes a video tag", () => {
  for (const banned of ["fetch(", "XMLHttpRequest", "WebSocket", "eval(", "new Function", "<video"]) expect(src).not.toContain(banned);
});

test("open builds a dialog with the steps and the failed one marked", () => {
  open();
  expect(dialog().getAttribute("role")).toBe("dialog");
  expect(dialog().getAttribute("aria-modal")).toBe("true");
  expect(dialog().getAttribute("aria-label")).toBe("Video of #7 Leave request");
  expect(buttonNamed("Step 2 at 0:09, failed")).toBeTruthy();
  expect(buttonNamed("Check: shows 'x' at 0:09")).toBeTruthy(); // the failed step starts open
  expect(buttonNamed("Click at 0:04")).toBeNull(); // the others start closed
  expect((dialog().querySelector(".vp__fail") as HTMLElement).style.left).toBe("45%");
  expect(video().getAttribute("src")).toBe("../shots/video-1-1.mp4");
  expect(player().isOpen()).toBe(true);
  // It starts playing, and the focus is on Play/Pause (now Pause), never on Close.
  expect(play).toHaveBeenCalledTimes(1);
  expect(document.activeElement).toBe(buttonNamed("Pause"));
  expect(document.activeElement).not.toBe(buttonNamed("Close player"));
});

test("a refused play leaves it paused and does not throw", async () => {
  play.mockImplementation(() => Promise.reject(new Error("blocked")));
  open();
  await Promise.resolve();
  await Promise.resolve();
  expect(buttonNamed("Play")).toBeTruthy();
});

test("each step has a tick titled with its name, and Save video is a download link", () => {
  open();
  const slider = within(dialog()).getByRole("slider", { name: "Position" });
  for (const name of ["Sign in", "Step 1", "Step 2"]) expect(slider.querySelector(`[title="${name}"]`)).toBeTruthy();
  const save = within(dialog()).getByRole("link", { name: "Save video" });
  expect(save.hasAttribute("download")).toBe(true);
  expect(save.getAttribute("href")).toBe("../shots/video-1-1.mp4");
});

test("positions follow the video's duration once it is known, and clamp", () => {
  open();
  const fail = () => dialog().querySelector(".vp__fail") as HTMLElement;
  expect(fail().style.left).toBe("45%");
  state.duration = 30;
  fireEvent.loadedMetadata(video());
  expect(fail().style.left).toBe("30%");
  expect(dialog().innerHTML).not.toContain("NaN");
  player().close();
  state.duration = Number.NaN;
  open({ ...timeline, length_ms: 20000, steps: [{ name: "Late", at_ms: 25000, failed: false, actions: [] }] });
  expect((dialog().querySelector('[title="Late"]') as HTMLElement).style.left).toBe("100%");
});

test("clicking a step or an action seeks there", () => {
  open();
  press("Step 1 at 0:04");
  expect(state.time).toBe(4);
  press("Actions of Step 1");
  expect(buttonNamed("Actions of Step 1")!.getAttribute("aria-expanded")).toBe("true");
  press("Click at 0:04");
  expect(state.time).toBe(4.2);
});

test("keys: Space, arrows, [ ] act; Space on a button is left to the button; Esc closes and focus returns", () => {
  open();
  const key = (k: string, target: Element = dialog()) => fireEvent.keyDown(target, { key: k });
  expect(play).toHaveBeenCalledTimes(1); // autoplay
  key(" ");
  expect(pause).toHaveBeenCalledTimes(1);
  state.time = 4;
  key("ArrowRight");
  expect(state.time).toBe(9);
  key("ArrowLeft");
  expect(state.time).toBe(4);
  key("]");
  expect(state.time).toBe(9);
  state.time = 9.2;
  key("[");
  expect(state.time).toBe(4);
  state.time = 0.5;
  key("[");
  expect(state.time).toBe(0.5); // no earlier step: the key does nothing
  // Space on a focused button presses it and does not toggle play.
  play.mockClear();
  const step = buttonNamed("Step 1 at 0:04")!;
  step.focus();
  key(" ", step);
  key("Enter", step);
  expect(play).not.toHaveBeenCalled();
  key("Escape");
  expect(player().isOpen()).toBe(false);
  expect(dialog().hidden).toBe(true);
  expect(pause).toHaveBeenCalled();
  expect(document.activeElement).toBe(document.getElementById("opener"));
});

test("an arrow past the end clamps to the length", () => {
  open();
  state.time = 18;
  fireEvent.keyDown(dialog(), { key: "ArrowRight" });
  expect(state.time).toBe(20);
  for (let i = 0; i < 4; i++) fireEvent.keyDown(dialog(), { key: "ArrowLeft" });
  expect(state.time).toBe(0);
});

test("Tab stays inside the dialog", () => {
  open();
  const focusable = Array.from(dialog().querySelectorAll<HTMLElement>("button:not([disabled]), a[href], [tabindex='0']"));
  focusable[focusable.length - 1].focus();
  const e = new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true });
  document.dispatchEvent(e);
  expect(e.defaultPrevented).toBe(true);
  expect(document.activeElement).toBe(focusable[0]);
});

test("the playing step is marked as the video plays", () => {
  open();
  state.time = 9.5;
  fireEvent.timeUpdate(video());
  expect(buttonNamed("Step 2 at 0:09, failed")!.getAttribute("aria-current")).toBe("step");
  expect(buttonNamed("Step 1 at 0:04")!.hasAttribute("aria-current")).toBe(false);
  expect(within(dialog()).getByRole("slider", { name: "Position" }).getAttribute("aria-valuetext")).toBe("0:09 of 0:20, Step 2");
});

test("Play turns into Pause, and the speed buttons set the playback rate", () => {
  open();
  expect(buttonNamed("Pause")).toBeTruthy(); // autoplay
  press("Pause");
  expect(buttonNamed("Play")).toBeTruthy();
  press("2x");
  expect(state.rate).toBe(2);
  expect(buttonNamed("2x")!.getAttribute("aria-pressed")).toBe("true");
  expect(buttonNamed("1x")!.getAttribute("aria-pressed")).toBe("false");
});

test("a video with no timeline opens without markers", () => {
  player().open(undefined as unknown as Timeline, "../shots/video-1-1.mp4", null);
  expect(dialog().getAttribute("aria-label")).toBe("Video");
  expect(dialog().textContent).toContain("No step markers: this video was recorded before they were kept.");
  expect(dialog().querySelector("ol")).toBeNull();
  expect(dialog().querySelector(".vp__fail")).toBeNull();
});

test("marks without steps give a flat Actions list", () => {
  open({ title: "Recording: Bug 1", length_ms: 94000, steps: [], marks: [{ label: "Click", at_ms: 3000 }] });
  expect(within(dialog()).getByRole("list", { name: "Actions" })).toBeTruthy();
  press("Click at 0:03");
  expect(state.time).toBe(3);
});

test("a video that will not play says so and keeps the list", () => {
  open();
  fireEvent.error(video());
  expect(dialog().textContent).toContain("This video cannot be played here - save it instead.");
  expect(within(dialog()).getByRole("list", { name: "Steps" })).toBeTruthy();
});

test("Close player closes it", () => {
  open();
  press("Close player");
  expect(player().isOpen()).toBe(false);
});

test("the same maths as the app", () => {
  expect(player().at([0, 0, 4000], 0)).toBe(1);
  expect(player().at([1000, 4000], 500)).toBe(-1);
  expect(player().target(timeline, 9200, -1)).toBe(4000);
  expect(player().target(timeline, 9500, 1)).toBeNull();
  expect(player().percent(5000, Number.NaN)).toBe(0);
  expect(player().percent(25000, 20000)).toBe(100);
  expect(player().clock(83000)).toBe("1:23");
  expect(player().clock(-5)).toBe("0:00");
});

test("untidy marks are sorted, dropped or labelled instead of breaking the player", () => {
  open({
    title: "Messy",
    length_ms: 20000,
    steps: [
      { name: "Late", at_ms: 9000, failed: false, actions: [] },
      { name: "No start", at_ms: null as unknown as number, failed: false, actions: [] },
      { name: "", at_ms: 1000, failed: false, actions: [{ label: "", at_ms: 1500 }, { label: "Bad", at_ms: Number.NaN }] },
    ],
    marks: [],
  });
  const names = Array.from(dialog().querySelectorAll(".vp__steps > li .vp__jump .vp__name")).map((n) => n.textContent);
  expect(names).toEqual(["Step 3", "Late"]);
  expect(buttonNamed("No start at 0:00")).toBeNull();
  press("Actions of Step 3");
  expect(buttonNamed("Action 1 at 0:01")).toBeTruthy();
  expect(buttonNamed("Bad at 0:00")).toBeNull();
  player().close();
  open({ title: "Flat", length_ms: 20000, steps: [], marks: [{ label: "B", at_ms: 5000 }, { label: "", at_ms: 2000 }, { label: "X", at_ms: null as unknown as number }] });
  expect(Array.from(dialog().querySelectorAll(".vp__steps button")).map((b) => b.getAttribute("aria-label"))).toEqual(["Action 2 at 0:02", "B at 0:05"]);
});

test("closing releases the video file", () => {
  open();
  const v = video();
  const load = vi.fn();
  Object.defineProperty(v, "load", { configurable: true, value: load });
  player().close();
  expect(v.hasAttribute("src")).toBe(false);
  expect(load).toHaveBeenCalled();
});

test("the failed stretch and ticks wait for a length", () => {
  open({ ...timeline, length_ms: null });
  const fail = () => dialog().querySelector(".vp__fail") as HTMLElement;
  const tick = () => dialog().querySelector(".vp__tick") as HTMLElement;
  expect(fail().hidden).toBe(true);
  expect(tick().hidden).toBe(true);
  state.duration = 30;
  fireEvent.loadedMetadata(video());
  expect(fail().hidden).toBe(false);
  expect(tick().hidden).toBe(false);
  expect(fail().style.left).toBe("30%");
});

test("Space on the Save video link is the player's; Enter on it stays the link's", () => {
  open();
  const save = within(dialog()).getByRole("link", { name: "Save video" });
  const space = new KeyboardEvent("keydown", { key: " ", bubbles: true, cancelable: true });
  save.dispatchEvent(space);
  expect(space.defaultPrevented).toBe(true);
  expect(play).toHaveBeenCalledTimes(1);
  const enter = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
  save.dispatchEvent(enter);
  expect(enter.defaultPrevented).toBe(false);
});
