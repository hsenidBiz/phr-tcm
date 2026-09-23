import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, expect, test, vi, type MockInstance } from "vitest";
import Settings from "./Settings";
import { burstConfetti } from "../lib/confetti";
import { resetExtrasStore } from "../lib/extras";
import { SEQUENCE } from "../lib/extrasSequence";
import { toast } from "../lib/toast";

vi.mock("../lib/confetti", () => ({ CONFETTI_MS: 2800, burstConfetti: vi.fn(() => () => {}) }));

const realMatchMedia = window.matchMedia;
let success: MockInstance;
let failure: MockInstance;

beforeEach(() => {
  success = vi.spyOn(toast, "success").mockImplementation((() => undefined) as never);
  failure = vi.spyOn(toast, "error").mockImplementation((() => undefined) as never);
});

afterEach(() => {
  clearMocks();
  resetExtrasStore();
  vi.mocked(burstConfetti).mockClear();
  vi.restoreAllMocks();
  window.matchMedia = realMatchMedia;
  localStorage.clear();
});

/** Rust's side of the switch, in memory. Returns every value saved. */
function ipc(start: boolean, failSaves = false) {
  const saves: boolean[] = [];
  let unlocked = start;
  mockIPC((cmd, args) => {
    if (cmd === "get_extras_unlocked") return unlocked;
    if (cmd === "set_extras_unlocked") {
      if (failSaves) throw "Could not save this setting. The app log in Settings has the details.";
      unlocked = (args as { unlocked: boolean }).unlocked;
      saves.push(unlocked);
      return null;
    }
    return undefined;
  });
  return saves;
}

function renderSettings(extra?: ReactNode) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      {extra}
      <Settings org="acme" project="Web" />
    </QueryClientProvider>,
  );
}

/** One keydown per key; returns fireEvent's answers (false = default prevented). */
const press = (keys: readonly string[], target: Element = document.body) =>
  keys.map((key) => fireEvent.keyDown(target, { key }));

const settle = () => new Promise((r) => setTimeout(r, 0));

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

test("nothing listens for it outside the Settings screen", async () => {
  const saves = ipc(false);
  press(SEQUENCE);
  const { unmount } = renderSettings();
  unmount();
  press(SEQUENCE);
  await settle();
  expect(saves).toEqual([]);
  expect(burstConfetti).not.toHaveBeenCalled();
});

test("the full sequence unlocks: saved, the section appears, confetti and a toast", async () => {
  const saves = ipc(false);
  renderSettings();
  await settle();
  expect(screen.queryByRole("heading", { name: "Extras" })).not.toBeInTheDocument();
  press(SEQUENCE);
  expect(await screen.findByRole("heading", { name: "Extras" })).toBeInTheDocument();
  expect(saves).toEqual([true]);
  expect(burstConfetti).toHaveBeenCalledTimes(1);
  expect(success).toHaveBeenCalledWith("Unlocked.");
});

test("typing in a field neither counts nor resets", async () => {
  const saves = ipc(false);
  renderSettings(<input aria-label="Scratch" />);
  const field = screen.getByLabelText("Scratch");
  press(SEQUENCE.slice(0, 6));
  press(["x", "ArrowUp", "Enter", "b"], field);
  press(SEQUENCE.slice(6));
  await waitFor(() => expect(saves).toEqual([true]));
});

test("Shift for a capital B or A does not reset the attempt", async () => {
  const saves = ipc(false);
  renderSettings();
  press([...SEQUENCE.slice(0, 8), "Shift", "B", "Shift", "A", "Enter"]);
  await waitFor(() => expect(saves).toEqual([true]));
});

test("a wrong key resets the attempt", async () => {
  const saves = ipc(false);
  renderSettings();
  press([...SEQUENCE.slice(0, 7), "x", ...SEQUENCE.slice(7)]);
  await settle();
  expect(saves).toEqual([]);
});

test("the panel shakes from the fifth correct input on, and not for a wrong one", () => {
  ipc(false);
  const { container } = renderSettings();
  const panel = container.firstElementChild as HTMLElement;
  press(SEQUENCE.slice(0, 4));
  expect(panel).not.toHaveClass("t-shake");
  press(SEQUENCE.slice(4, 5));
  expect(panel).toHaveClass("t-shake");
  panel.classList.remove("t-shake");
  press(["x"]);
  expect(panel).not.toHaveClass("t-shake");
});

test("the Enter that completes it is not passed on to the focused control", () => {
  ipc(false);
  renderSettings();
  expect(press(["Enter"])[0]).toBe(true);
  press(SEQUENCE.slice(0, 10));
  expect(press(["Enter"])[0]).toBe(false);
});

test("under reduced motion: no shake, no confetti, but still the unlock and the toast", async () => {
  reduceMotion();
  const saves = ipc(false);
  const { container } = renderSettings();
  const panel = container.firstElementChild as HTMLElement;
  press(SEQUENCE);
  await waitFor(() => expect(saves).toEqual([true]));
  expect(panel).not.toHaveClass("t-shake");
  expect(burstConfetti).not.toHaveBeenCalled();
  await waitFor(() => expect(success).toHaveBeenCalledWith("Unlocked."));
});

test("entering it again while unlocked replays the confetti and does nothing else", async () => {
  const saves = ipc(true);
  renderSettings();
  await screen.findByRole("heading", { name: "Extras" });
  press(SEQUENCE);
  await waitFor(() => expect(burstConfetti).toHaveBeenCalledTimes(1));
  expect(saves).toEqual([]);
  expect(success).not.toHaveBeenCalled();
});

test("a save that fails says so and stays locked", async () => {
  ipc(false, true);
  renderSettings();
  press(SEQUENCE);
  await waitFor(() =>
    expect(failure).toHaveBeenCalledWith("Could not save this setting. The app log in Settings has the details."),
  );
  expect(screen.queryByRole("heading", { name: "Extras" })).not.toBeInTheDocument();
  expect(burstConfetti).not.toHaveBeenCalled();
  expect(success).not.toHaveBeenCalled();
});

test("Reset to default asks first, then hides the section and touches nothing else", async () => {
  const saves = ipc(true);
  localStorage.setItem("tcm-v2-theme", "dark");
  renderSettings();
  fireEvent.click(await screen.findByRole("button", { name: "Reset to default" }));
  expect(screen.getByText("Hide these extras again?")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(saves).toEqual([]);
  expect(screen.getByRole("heading", { name: "Extras" })).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Reset to default" }));
  fireEvent.click(screen.getByRole("button", { name: "Reset" }));
  await waitFor(() => expect(screen.queryByRole("heading", { name: "Extras" })).not.toBeInTheDocument());
  expect(saves).toEqual([false]);
  expect(localStorage.getItem("tcm-v2-theme")).toBe("dark");
});
