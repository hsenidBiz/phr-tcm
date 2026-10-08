// The unattended run's pill in the title bar: how far the run has got, that
// it is paused at a reset point, or that it has ended - and a click that
// says what it does.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import {
  backgroundRunSnapshot,
  openRunSetup,
  resetBackgroundRun,
  sendRunToBackground,
  startRun,
  type BackgroundRun,
} from "../lib/backgroundRun";
import { setDiscoveryActive } from "../lib/discoveryActive";
import RunPill, { runPillView } from "./RunPill";
import TitleBar from "./TitleBar";

afterEach(() => {
  resetBackgroundRun();
  setDiscoveryActive(false);
});

const CASES = Array.from({ length: 8 }, (_, i) => ({ id: i + 1, title: `Case ${i + 1}` }));

/** A run as the store holds it, in the state a test names. */
function runIn(over: Partial<BackgroundRun>): BackgroundRun {
  return {
    org: "acme",
    project: "Web",
    pbi: { id: 42, title: "Login work", work_item_type: "Product Backlog Item" },
    cases: CASES,
    plan: null,
    phase: "running",
    open: false,
    runId: "run-9",
    rows: {},
    phases: {},
    latest: null,
    position: null,
    records: {},
    resetNeeded: null,
    answering: false,
    stopping: false,
    resultId: null,
    error: "",
    ...over,
  };
}

const RESET = { run_id: "run-9", before_case_id: 4, names: ["x"], changed_by: [], remaining: [4] };

test("running: how far it has got; paused: amber Reset needed; finished: Review", () => {
  expect(runPillView(runIn({ position: { index: 2, total: 8 } }))).toEqual({
    text: "Auto Run 3 of 8",
    action: "Open the run window",
    tone: "accent",
  });
  // Before the first case reports, it is on the first.
  expect(runPillView(runIn({}))?.text).toBe("Auto Run 1 of 8");
  expect(runPillView(runIn({ position: { index: 2, total: 8 }, resetNeeded: RESET }))).toEqual({
    text: "Reset needed",
    action: "Open the run window to continue or stop the run",
    tone: "warning",
  });
  expect(runPillView(runIn({ phase: "finished", resultId: "run-9" }))).toEqual({
    text: "Run finished, Review",
    action: "Open the finished run's review in Auto Run",
    tone: "success",
  });
  expect(runPillView(runIn({ phase: "failed", error: "no browser" }))?.text).toBe("Run stopped with an error");
});

test("no run, or one still on its setup, shows no pill", () => {
  expect(runPillView(null)).toBeNull();
  expect(runPillView(runIn({ phase: "setup", open: true }))).toBeNull();
  render(<RunPill />);
  expect(screen.queryByRole("button")).not.toBeInTheDocument();
  act(() => {
    openRunSetup({ org: "acme", project: "Web", pbi: runIn({}).pbi, cases: CASES, plan: null });
  });
  expect(screen.queryByRole("button")).not.toBeInTheDocument();
});

test("the pill sits in the title bar, outside its drag group, centred with label-trim", () => {
  render(<TitleBar title="Test Case Manager" status={<span>Pill here</span>} />);
  const status = screen.getByText("Pill here");
  // Not inside the pointer-events-none group that holds the title.
  expect(status.closest(".pointer-events-none")).toBeNull();
  expect(status.closest("header")).not.toBeNull();
});

test("a running run's pill names its click, and the click opens the run window", async () => {
  mockIPC((cmd) => (cmd === "auto_run_replay" ? new Promise(() => {}) : null), { shouldMockEvents: true });
  act(() => {
    openRunSetup({ org: "acme", project: "Web", pbi: runIn({}).pbi, cases: CASES, plan: null });
  });
  await act(async () => {
    void startRun({ account: null, browserName: "edge", watch: false, retryTransient: true, dbReadAccess: true });
    await Promise.resolve();
  });
  act(() => sendRunToBackground());
  render(<RunPill />);
  const pill = screen.getByRole("button", { name: "Auto Run 1 of 8. Open the run window" });
  expect(pill).toHaveAttribute("title", "Open the run window");
  expect(screen.getByText("Auto Run 1 of 8")).toHaveClass("label-trim");
  expect(backgroundRunSnapshot().run?.open).toBe(false);
  fireEvent.click(pill);
  expect(backgroundRunSnapshot().run).toEqual(expect.objectContaining({ phase: "running", open: true }));
  clearMocks();
});

test("discovery with no run reads Discovering, in accent, as text that says what ends it", () => {
  const note = "Discovery is using the Auto Run browser. It ends from End discovery on Auto Run's Discovery card.";
  expect(runPillView(null, true)).toEqual({ text: "Discovering", action: note, tone: "accent" });
  expect(runPillView(runIn({ phase: "setup", open: true }), true)?.text).toBe("Discovering");
  // A run the store holds still shows as itself.
  expect(runPillView(runIn({ phase: "finished", resultId: "run-9" }), true)?.text).toBe("Run finished, Review");

  render(<RunPill />);
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
  act(() => setDiscoveryActive(true));
  const pill = screen.getByRole("status", { name: `Discovering. ${note}` });
  expect(pill).toHaveAttribute("title", note);
  expect(pill.className).toMatch(/\bbg-accent\/15\b/);
  expect(pill.className).toMatch(/\btext-accent-fill\b/);
  expect(screen.getByText("Discovering")).toHaveClass("label-trim");
  // Nothing to click: End discovery on Auto Run is what ends it.
  expect(screen.queryByRole("button")).not.toBeInTheDocument();

  act(() => setDiscoveryActive(false));
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
});
