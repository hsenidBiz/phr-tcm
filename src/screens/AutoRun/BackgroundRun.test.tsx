// An unattended run that carries on in the background.
//
// The run lives in lib/backgroundRun, its window and the title-bar pill are
// mounted beside the screens (as App does), and the Auto Run screen comes
// and goes as the person moves between sections. What is under test: the
// run outlives the screen, the pill brings its window back from anywhere,
// a finish in the background waits for Review instead of opening over the
// person's work, and nothing else can start while it goes.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, test, vi } from "vitest";
import RunPill from "../../components/RunPill";
import { useBackgroundRunHost } from "../../hooks/useBackgroundRunHost";
import { backgroundRunSnapshot, openRunSetup, resetBackgroundRun, startRun } from "../../lib/backgroundRun";
import { setDiscoveryActive } from "../../lib/discoveryActive";
import { toast } from "../../lib/toast";
import AutoRun from "./index";
import ReplayPane from "./ReplayPane";

vi.mock("../../lib/toast", () => ({
  toast: { success: vi.fn(() => "t1"), error: vi.fn(() => "t2"), warning: vi.fn(), info: vi.fn(), dismiss: vi.fn() },
}));

afterEach(() => {
  resetBackgroundRun();
  setDiscoveryActive(false);
  clearMocks();
  localStorage.clear();
  vi.clearAllMocks();
});

const pbi = { id: 42, title: "Login work", work_item_type: "Product Backlog Item" };

function caseRow(id: number, title: string) {
  return {
    id,
    title,
    tags: "",
    automation_status: "Not Automated",
    steps: [],
    step_ids: [],
    steps_xml: "",
    module_value: "",
    preconditions: "",
  };
}

const RUN = {
  id: "run-9",
  pbi_id: 42,
  started_at: "1786000200000",
  mode: "unattended",
  published: null,
  cases: [
    {
      case_id: 1,
      title: "Alpha check",
      verdict: "",
      note: "",
      proposed: "Failed",
      reason: "step 2: button not found",
      steps: [
        { step_number: 1, outcomes: [{ ok: true, detail: "clicked Edit" }] },
        { step_number: 2, outcomes: [{ ok: false, detail: "button not found" }] },
      ],
    },
  ],
};

/** The screen's answers, with `auto_run_replay` left pending until the
 * test settles it through the returned handle. */
function mockScreen(extra?: (cmd: string, args: unknown) => unknown) {
  const run = {
    resolve: (_v: unknown) => {},
    reject: (_e: unknown) => {},
  };
  mockIPC(
    (cmd, args) => {
      if (cmd === "list_test_case_fields") return [];
      if (cmd === "pbi_test_cases_full") return [caseRow(1, "Alpha check"), caseRow(2, "Beta check")];
      if (cmd === "auto_run_load_script") {
        const id = (args as { caseId: number }).caseId;
        return { case_id: id, title: "s", steps: [{ step_number: 1, actions: [] }] };
      }
      if (cmd === "auto_run_list_runs") return [RUN];
      if (cmd === "auto_run_list_accounts") return [];
      if (cmd === "auto_run_plan") return { order: [1, 2], phases: [[1, 2]], resets: [], counts: null, saved: true };
      if (cmd === "auto_run_load_run") return RUN;
      if (cmd === "auto_run_replay") {
        return new Promise((resolve, reject) => {
          run.resolve = resolve;
          run.reject = reject;
        });
      }
      if (extra) return extra(cmd, args);
      return null;
    },
    { shouldMockEvents: true },
  );
  return run;
}

/** The app around the screen: a title-bar pill, a way to another section,
 * and the run window, the way App mounts them. */
function Shell() {
  const [section, setSection] = useState<"autorun" | "manual">("autorun");
  useBackgroundRunHost(() => setSection("autorun"));
  return (
    <>
      <header>
        <RunPill />
        <button type="button" onClick={() => setSection("manual")}>
          Go to Manual Entry
        </button>
      </header>
      {section === "autorun" ? (
        <AutoRun org="acme" project="proj" pbi={pbi as never} />
      ) : (
        <p>Manual Entry screen</p>
      )}
      <ReplayPane />
    </>
  );
}

function renderShell() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <Shell />
    </QueryClientProvider>,
  );
}

async function emitProgress(p: Partial<Record<string, unknown>>) {
  const { emit } = await import("@tauri-apps/api/event");
  await act(async () => {
    await emit("replay-progress", {
      run_id: "run-9",
      index: 0,
      total: 2,
      case_id: 1,
      title: "Alpha check",
      phase: "opening",
      step_number: 0,
      steps: 1,
      proposed: "",
      ...p,
    });
  });
}

/** Ticks both cases, opens the unattended run and starts it. */
async function startUnattended() {
  await screen.findByText("Alpha check");
  fireEvent.click(screen.getByRole("checkbox", { name: "Select #1" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Select #2" }));
  fireEvent.click(await screen.findByRole("button", { name: "Run 2 unattended" }));
  fireEvent.click(await screen.findByRole("button", { name: "Start" }));
  await waitFor(() => expect(backgroundRunSnapshot().run?.phase).toBe("running"));
  await emitProgress({ phase: "opening" });
}

const heading = () => screen.queryByRole("heading", { name: "Unattended run" });

test("Run in background closes the window, and the pill reopens it from another section", async () => {
  mockScreen();
  renderShell();
  await startUnattended();
  expect(await screen.findByText("case 1 of 2")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Run in background" }));
  await waitFor(() => expect(heading()).not.toBeInTheDocument());

  // The person goes elsewhere; the run carries on and the pill follows it.
  fireEvent.click(screen.getByRole("button", { name: "Go to Manual Entry" }));
  expect(screen.getByText("Manual Entry screen")).toBeInTheDocument();
  await emitProgress({ index: 1, case_id: 2, title: "Beta check", phase: "step", step_number: 1 });
  const pill = screen.getByRole("button", { name: "Auto Run 2 of 2. Open the run window" });
  expect(pill).toHaveAttribute("title", "Open the run window");

  // A click opens the run window right here, over Manual Entry.
  fireEvent.click(pill);
  expect(await screen.findByRole("heading", { name: "Unattended run" })).toBeInTheDocument();
  expect(screen.getByText("Manual Entry screen")).toBeInTheDocument();
  expect(screen.getByText("case 2 of 2")).toBeInTheDocument();
  expect(within(screen.getByText("Beta check").closest("li")!).getByText("Step 1 of 1")).toBeInTheDocument();
});

test("a run that finishes in the background waits for Review, which opens it on Past runs", async () => {
  const run = mockScreen();
  renderShell();
  await startUnattended();
  fireEvent.keyDown(window, { key: "Escape" });
  await waitFor(() => expect(heading()).not.toBeInTheDocument());
  fireEvent.click(screen.getByRole("button", { name: "Go to Manual Entry" }));

  await act(async () => {
    run.resolve(RUN);
    await Promise.resolve();
  });

  // Nothing opens over the person's work: a toast with Review, and the pill.
  expect(screen.getByText("Manual Entry screen")).toBeInTheDocument();
  expect(screen.queryByRole("listitem", { name: "Case #1 Alpha check" })).not.toBeInTheDocument();
  expect(toast.success).toHaveBeenCalledWith(
    "The unattended run finished",
    expect.objectContaining({ action: expect.objectContaining({ label: "Review" }) }),
  );
  const pill = await screen.findByRole("button", { name: "Run finished, Review. Open the finished run's review in Auto Run" });

  // Review takes the person to Auto Run, on Past runs, with the review open.
  fireEvent.click(pill);
  expect(await screen.findByRole("listitem", { name: "Case #1 Alpha check" })).toBeInTheDocument();
  expect(screen.getByRole("tab", { name: /^Past runs/ })).toHaveAttribute("aria-selected", "true");
  expect(screen.queryByRole("button", { name: /^Run finished/ })).not.toBeInTheDocument();
  expect(backgroundRunSnapshot()).toEqual({ run: null, review: null });
  // Its toast's Review would now do nothing, so the toast goes too.
  expect(toast.dismiss).toHaveBeenCalledWith("t1");
});

test("the toast's Review does what the pill's does", async () => {
  const run = mockScreen();
  renderShell();
  await startUnattended();
  fireEvent.click(screen.getByRole("button", { name: "Run in background" }));
  fireEvent.click(screen.getByRole("button", { name: "Go to Manual Entry" }));
  await act(async () => {
    run.resolve(RUN);
    await Promise.resolve();
  });
  const opts = vi.mocked(toast.success).mock.calls[0][1]!;
  act(() => opts.action!.onClick());
  expect(await screen.findByRole("listitem", { name: "Case #1 Alpha check" })).toBeInTheDocument();
  expect(screen.getByRole("tab", { name: /^Past runs/ })).toHaveAttribute("aria-selected", "true");
});

test("a run that finishes with its window open goes straight to the review, with no toast", async () => {
  const run = mockScreen();
  renderShell();
  await startUnattended();
  await act(async () => {
    run.resolve(RUN);
    await Promise.resolve();
  });
  expect(await screen.findByRole("listitem", { name: "Case #1 Alpha check" })).toBeInTheDocument();
  // As before: over Test cases, which the screen was on.
  expect(screen.getByRole("tab", { name: /^Test cases/ })).toHaveAttribute("aria-selected", "true");
  expect(toast.success).not.toHaveBeenCalled();
  expect(screen.queryByRole("button", { name: /^Run finished/ })).not.toBeInTheDocument();
  // The selection has been run.
  expect(screen.getByRole("checkbox", { name: "Select #1" })).toHaveAttribute("aria-checked", "false");
});

test("a pause at a reset point while in the background turns the pill amber and opens on Continue / Stop", async () => {
  const answers: unknown[] = [];
  mockScreen((cmd, args) => {
    if (cmd === "auto_run_answer_reset") {
      answers.push(args);
      return null;
    }
    if (cmd === "auto_run_waiting_reset") return null;
    return null;
  });
  renderShell();
  await startUnattended();
  fireEvent.click(screen.getByRole("button", { name: "Run in background" }));
  await waitFor(() => expect(heading()).not.toBeInTheDocument());

  const { emit } = await import("@tauri-apps/api/event");
  await act(async () => {
    await emit("autorun-reset-needed", {
      run_id: "run-9",
      before_case_id: 2,
      names: ["cycle published"],
      changed_by: [["cycle published", [1]]],
      remaining: [2],
    });
  });

  // One place shows the pause: not the screen's own fallback modal.
  expect(screen.queryByRole("region", { name: "Reset needed" })).not.toBeInTheDocument();
  const pill = screen.getByRole("button", {
    name: "Reset needed. Open the run window to continue or stop the run",
  });
  expect(pill.className).toMatch(/text-warning/);

  fireEvent.click(pill);
  const panel = await screen.findByRole("region", { name: "Reset needed" });
  expect(screen.getAllByRole("region", { name: "Reset needed" })).toHaveLength(1);
  fireEvent.click(within(panel).getByRole("button", { name: "Continue after reset" }));
  await waitFor(() => expect(answers).toEqual([{ runId: "run-9", continueRun: true }]));
});

test("a run that fails in the background says so, and the pill reopens the window on the error", async () => {
  const run = mockScreen();
  renderShell();
  await startUnattended();
  fireEvent.click(screen.getByRole("button", { name: "Run in background" }));
  await act(async () => {
    run.reject("the browser could not be opened");
    await Promise.resolve();
  });
  expect(toast.error).toHaveBeenCalledWith("The unattended run stopped with an error", expect.anything());
  const pill = await screen.findByRole("button", {
    name: "Run stopped with an error. Open the run window to see the error",
  });
  fireEvent.click(pill);
  expect(await screen.findByText("the browser could not be opened")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Start" })).toBeEnabled();
});

test("while a run goes, every Run button in Auto Run waits for it and says why", async () => {
  mockScreen();
  renderShell();
  await startUnattended();
  fireEvent.click(screen.getByRole("button", { name: "Run in background" }));
  await waitFor(() => expect(heading()).not.toBeInTheDocument());

  const reason = "An unattended run is already going. Wait for it, or stop it.";
  // The dock draws in place and, once scrolled away, floating: both say it.
  expect(screen.getAllByText("A run is already going").length).toBeGreaterThan(0);
  for (const name of ["Run 2 selected", "Run 2 unattended"]) {
    for (const b of screen.getAllByRole("button", { name })) {
      expect(b).toBeDisabled();
      expect(b).toHaveAttribute("title", reason);
    }
  }

  // A card's own Run.
  fireEvent.click(screen.getByRole("button", { name: "Show details for #1" }));
  const cardRun = await screen.findByRole("button", { name: "Run #1" });
  expect(cardRun).toBeDisabled();
  expect(cardRun).toHaveAttribute("title", reason);

  // Replay to step, in Past runs.
  fireEvent.click(screen.getByRole("tab", { name: /^Past runs/ }));
  const replay = await screen.findByRole("button", { name: "Replay to step 2 for case 1" });
  expect(replay).toBeDisabled();
  expect(replay).toHaveAttribute("title", reason);
});

test("while discovery holds the browser, no unattended run starts and the pill says Discovering", async () => {
  let active = true;
  mockScreen((cmd) => {
    if (cmd === "auto_run_discovery_active") return active;
    return null;
  });
  renderShell();
  await screen.findByText("Alpha check");
  fireEvent.click(screen.getByRole("checkbox", { name: "Select #1" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Select #2" }));

  const busy = "Discovery is using the Auto Run browser";
  await waitFor(() => {
    for (const b of screen.getAllByRole("button", { name: "Run 2 unattended" })) {
      expect(b).toBeDisabled();
      expect(b).toHaveAttribute("title", busy);
    }
  });
  expect(screen.getByRole("status", { name: /^Discovering\. Discovery is using the Auto Run browser/ })).toBeInTheDocument();

  // The store refuses too: no setup opens, so nothing can start.
  let opened = true;
  act(() => {
    opened = openRunSetup({ org: "acme", project: "proj", pbi, cases: [{ id: 1, title: "Alpha check" }], plan: null });
  });
  expect(opened).toBe(false);
  expect(backgroundRunSnapshot().run).toBeNull();

  // A setup opened before discovery began cannot start once it holds the
  // browser: Start waits, and the store does nothing.
  const { emit } = await import("@tauri-apps/api/event");
  active = false;
  await act(async () => {
    await emit("autorun-discovery-changed", { active: false });
  });
  await waitFor(() => expect(screen.queryByRole("status", { name: /^Discovering/ })).not.toBeInTheDocument());
  act(() => {
    opened = openRunSetup({ org: "acme", project: "proj", pbi, cases: [{ id: 1, title: "Alpha check" }], plan: null });
  });
  expect(opened).toBe(true);
  const start = await screen.findByRole("button", { name: "Start" });
  expect(start).toBeEnabled();
  active = true;
  await act(async () => {
    await emit("autorun-discovery-changed", { active: true });
  });
  await waitFor(() => expect(start).toBeDisabled());
  expect(start).toHaveAttribute("title", busy);
  await act(async () => {
    await startRun({ account: null, browserName: "edge", watch: false, retryTransient: true, dbReadAccess: true });
  });
  // Start's first act is to mark the run going: it never got that far.
  expect(backgroundRunSnapshot().run?.phase).toBe("setup");
});
