import { emit } from "@tauri-apps/api/event";
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { MY_ORDER_EVENT } from "../lib/runOrder";
import RunnerWindow from "./RunnerWindow";

// getCurrentWindow() must be a no-op in jsdom - and its methods must
// return PROMISES: the component chains .catch() on them, and a bare
// vi.fn() (undefined) threw an unhandled error that failed the release
// gate even with every assertion green. ONE shared object, so tests can
// assert calls like minimize/unminimize across getCurrentWindow() calls.
const windowMock = vi.hoisted(() => ({
  close: vi.fn(() => Promise.resolve()),
  setFocus: vi.fn(() => Promise.resolve()),
  setAlwaysOnTop: vi.fn(() => Promise.resolve()),
  minimize: vi.fn(() => Promise.resolve()),
  unminimize: vi.fn(() => Promise.resolve()),
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => windowMock,
}));

// The toasts are asserted on (a broken paste must never claim success), so
// they are spies. Every method the runner and its toaster call is here.
const toastMock = vi.hoisted(() => ({
  success: vi.fn(),
  error: vi.fn(),
  info: vi.fn(),
  warning: vi.fn(),
  dismiss: vi.fn(),
}));
vi.mock("../lib/toast", () => ({ toast: toastMock }));

// jsdom cannot decode an image, so the decoder validImageB64 uses is
// swapped for one that accepts whatever the structural checks let through.
// `decode.accepts` forces it fully open for the one test that reaches the
// last-resort guard in addImage.
const decode = vi.hoisted(() => ({
  accepts: false,
  // Every picture the runner asked to have checked, in order.
  calls: [] as Array<{ b64: string; mime?: string }>,
}));
vi.mock("../lib/blob", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/blob")>();
  return {
    ...actual,
    validImageB64: (b64: string, mime?: string) => {
      decode.calls.push({ b64, mime });
      return decode.accepts ? Promise.resolve(true) : actual.validImageB64(b64, mime, async () => true);
    },
  };
});

/** A real 1x1 PNG (69 bytes), base64. */
const PNG_B64 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
const pngBytes = () => Uint8Array.from(atob(PNG_B64), (c) => c.charCodeAt(0));
const bytesToB64 = (bytes: Uint8Array) => btoa(String.fromCharCode(...bytes));
/** A minimal whole JPEG: start of image, a JFIF header, end of image. */
const jpegBytes = () =>
  new Uint8Array([0xff, 0xd8, 0xff, 0xe0, 0, 16, 0x4a, 0x46, 0x49, 0x46, 0, 1, 1, 0, 0, 1, 0, 1, 0, 0, 0xff, 0xd9]);

beforeEach(() => {
  localStorage.setItem(
    "tcm-v2-runner-session",
    JSON.stringify({
      org: "acme",
      project: "Web",
      planId: 9,
      planName: "Plan",
      suiteId: 91,
      pbi: { id: 42, title: "Login flow", work_item_type: "Product Backlog Item" },
    }),
  );
});

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

const fullCase = {
  id: 201,
  title: "Valid login",
  tags: "",
  automation_status: "Planned",
  steps: [
    { action: "Open page", expected: "Shown" },
    { action: "Submit", expected: "" },
  ],
  step_ids: ["2", "3"],
  module_value: "",
  preconditions: "",
};

function renderRunner() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <RunnerWindow />
    </QueryClientProvider>,
  );
}

/** The incremental protocol, end to end: the run opens lazily, the case's
 * marks land via record_result, and Finish completes the run - the same
 * three commands Next-driven recording uses. */
test("plays a case, records it into a lazily-opened run, and Finish completes it", async () => {
  const calls: Array<{ cmd: string; args: Record<string, unknown> }> = [];
  mockIPC((cmd, args) => {
    if (cmd === "run_history") return [];
    if (cmd === "pbi_test_cases_full") return [fullCase];
    if (cmd === "list_test_points")
      return [
        {
          point_id: 7,
          test_case_id: 201,
          test_case_name: "Valid login",
          config_name: "W10",
          tester: "",
          last_outcome: "",
          last_run_id: null,
          last_result_id: null,
        },
      ];
    if (cmd === "start_test_run") {
      calls.push({ cmd, args: args as Record<string, unknown> });
      return {
        run_id: 300,
        web_url: "https://x/run/300",
        results: [{ point_id: 7, result_id: 70 }],
        unmatched: [],
      };
    }
    if (cmd === "record_result") {
      calls.push({ cmd, args: args as Record<string, unknown> });
      return [];
    }
    if (cmd === "finish_test_run") {
      calls.push({ cmd, args: args as Record<string, unknown> });
      return null;
    }
  });
  renderRunner();

  expect(await screen.findByText("Valid login")).toBeInTheDocument();
  // Mark step 1 passed: the mark is named for its step and says it is on.
  const stepPass = screen.getByRole("button", { name: "Mark step 1 passed" });
  expect(stepPass).toHaveAttribute("aria-pressed", "false");
  fireEvent.click(screen.getAllByTitle("Passed")[0]);
  expect(stepPass).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByRole("button", { name: "Mark step 1 failed" })).toHaveAttribute("aria-pressed", "false");
  // Overall Passed.
  fireEvent.click(screen.getByRole("button", { name: "Passed" }));
  fireEvent.click(screen.getByRole("button", { name: /Finish \(1\)/ }));

  await vi.waitFor(() => expect(calls.some((c) => c.cmd === "finish_test_run")).toBe(true));
  const start = calls.find((c) => c.cmd === "start_test_run")!;
  expect(start.args.runName).toBe("Login flow - manual run");
  expect(start.args.pointIds).toEqual([7]);
  const rec = calls.find((c) => c.cmd === "record_result")!;
  expect(rec.args.resultId).toBe(70);
  const o = rec.args.outcome as Record<string, unknown>;
  expect(o.point_id).toBe(7);
  expect(o.outcome).toBe("Passed");
  expect(o.step_ids).toEqual(["2", "3"]);
  expect(o.step_outcomes).toEqual(["Passed", null]);
  expect(calls.map((c) => c.cmd)).toEqual(["start_test_run", "record_result", "finish_test_run"]);
});

/** The point of the feature: clicking NEXT records the case being left,
 * before any Finish - a session abandoned half way has everything it
 * passed through already saved in Azure DevOps. */
test("Next records the outcome immediately, without waiting for Finish", async () => {
  const recorded: Array<Record<string, unknown>> = [];
  mockIPC((cmd, args) => {
    if (cmd === "run_history") return [];
    if (cmd === "pbi_test_cases_full")
      return [fullCase, { ...fullCase, id: 202, title: "Invalid login" }];
    if (cmd === "list_test_points")
      return [
        {
          point_id: 7,
          test_case_id: 201,
          test_case_name: "Valid login",
          config_name: "W10",
          tester: "",
          last_outcome: "",
          last_run_id: null,
          last_result_id: null,
        },
        {
          point_id: 8,
          test_case_id: 202,
          test_case_name: "Invalid login",
          config_name: "W10",
          tester: "",
          last_outcome: "",
          last_run_id: null,
          last_result_id: null,
        },
      ];
    if (cmd === "start_test_run")
      return {
        run_id: 300,
        web_url: "",
        results: [
          { point_id: 7, result_id: 70 },
          { point_id: 8, result_id: 80 },
        ],
        unmatched: [],
      };
    if (cmd === "record_result") {
      recorded.push(args as Record<string, unknown>);
      return [];
    }
  });
  renderRunner();
  await screen.findByText("Valid login");

  fireEvent.click(screen.getByRole("button", { name: "Failed" }));
  fireEvent.click(screen.getByRole("button", { name: "Next" }));

  // Recorded on navigation - no Finish anywhere in sight.
  await vi.waitFor(() => expect(recorded).toHaveLength(1));
  expect(recorded[0].resultId).toBe(70);
  expect((recorded[0].outcome as Record<string, unknown>).outcome).toBe("Failed");

  // Going BACK and changing the verdict re-records the same row.
  fireEvent.click(screen.getByRole("button", { name: "Prev" }));
  await screen.findByText("Valid login");
  fireEvent.click(screen.getByRole("button", { name: "Passed" }));
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  await vi.waitFor(() => expect(recorded).toHaveLength(2));
  expect(recorded[1].resultId).toBe(70);
  expect((recorded[1].outcome as Record<string, unknown>).outcome).toBe("Passed");

  // An UNCHANGED case is not re-sent on the way past.
  fireEvent.click(screen.getByRole("button", { name: "Prev" }));
  await screen.findByText("Valid login");
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  await new Promise((r) => setTimeout(r, 50));
  expect(recorded).toHaveLength(2);
});

/// The previous run's outcome is SHOWN, never pre-applied: pre-selecting
/// the button made "pass it again" impossible (the lit button's click
/// un-marks). A previously-run case opens unmarked with the last verdict's
/// button softly tinted; never-run cases stay entirely blank.
test("a previously-run case opens unmarked with its last verdict tinted", async () => {
  mockIPC((cmd) => {
    if (cmd === "run_history") return [];
    if (cmd === "pbi_test_cases_full")
      return [fullCase, { ...fullCase, id: 202, title: "Invalid login" }];
    if (cmd === "list_test_points")
      return [
        {
          point_id: 7,
          test_case_id: 201,
          test_case_name: "Valid login",
          config_name: "W10",
          tester: "",
          last_outcome: "failed",
          last_run_id: 3,
          last_result_id: 30,
        },
        {
          point_id: 8,
          test_case_id: 202,
          test_case_name: "Invalid login",
          config_name: "W10",
          tester: "",
          last_outcome: "",
          last_run_id: null,
          last_result_id: null,
        },
        // A point for a case NOT in this runner's list (another config or
        // a filtered run). Its pre-seeded state must not count: the header
        // once read "68/53" from exactly this, and Finish flushes only the
        // listed cases.
        {
          point_id: 9,
          test_case_id: 999,
          test_case_name: "Elsewhere",
          config_name: "W11",
          tester: "",
          last_outcome: "passed",
          last_run_id: 3,
          last_result_id: 31,
        },
      ];
    if (cmd === "get_result_detail") return { outcome: "failed", comment: "" };
    if (cmd === "result_screenshots") return [];
  });
  renderRunner();
  await screen.findByText("Valid login");

  // Case 201 failed last time: the Failed button arrives UNLIT - a soft
  // tint, not the solid fill a mark gets - with a tooltip naming it as the
  // previous result.
  const failedBtn = await screen.findByTitle("Failed - the previous run's result");
  expect(failedBtn).not.toHaveClass("bg-danger");
  expect(failedBtn).toHaveClass("bg-danger/15");
  // The others are outlined in their colour, untinted.
  expect(screen.getByRole("button", { name: "Passed" })).toHaveClass("border-success/70");
  expect(screen.getByRole("button", { name: "Passed" })).not.toHaveClass("bg-success/15");
  // Nothing is marked yet - the indicator is information, not a mark -
  // and the off-list point 999 adds nothing either.
  expect(screen.getByTitle("0 of 2 marked")).toHaveTextContent("1/2");
  expect(screen.getByRole("button", { name: /Finish \(0\)/ })).toBeInTheDocument();

  // Case 202 has never run: it must arrive with NOTHING selected and
  // nothing tinted - a blank slate is information too.
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  await screen.findByText("Invalid login");
  // ...and walking forward increments the position.
  expect(screen.getByTitle("0 of 2 marked")).toHaveTextContent("2/2");
  expect(screen.getByRole("button", { name: "Failed" })).not.toHaveClass("bg-danger");
  expect(screen.getByRole("button", { name: "Passed" })).not.toHaveClass("bg-success");
  expect(screen.queryByTitle(/previous run's result/)).not.toBeInTheDocument();
});

/// The point of showing instead of pre-selecting: a case that passed last
/// time can be PASSED AGAIN - the click is an ordinary mark, and Next
/// records it as a fresh result.
test("clicking the previous run's own verdict records a fresh result", async () => {
  const recorded: Array<Record<string, unknown>> = [];
  mockIPC((cmd, args) => {
    if (cmd === "run_history") return [];
    if (cmd === "pbi_test_cases_full")
      return [fullCase, { ...fullCase, id: 202, title: "Invalid login" }];
    if (cmd === "list_test_points")
      return [
        {
          point_id: 7,
          test_case_id: 201,
          test_case_name: "Valid login",
          config_name: "W10",
          tester: "",
          last_outcome: "passed",
          last_run_id: 3,
          last_result_id: 30,
        },
        {
          point_id: 8,
          test_case_id: 202,
          test_case_name: "Invalid login",
          config_name: "W10",
          tester: "",
          last_outcome: "",
          last_run_id: null,
          last_result_id: null,
        },
      ];
    if (cmd === "get_result_detail") return { outcome: "passed", comment: "" };
    if (cmd === "result_screenshots") return [];
    if (cmd === "start_test_run")
      return {
        run_id: 300,
        web_url: "",
        results: [
          { point_id: 7, result_id: 70 },
          { point_id: 8, result_id: 80 },
        ],
        unmatched: [],
      };
    if (cmd === "record_result") {
      recorded.push(args as Record<string, unknown>);
      return [];
    }
  });
  renderRunner();
  await screen.findByText("Valid login");
  await screen.findByTitle("Passed - the previous run's result");

  // Clicking Passed - the SAME verdict as last run - is a real mark...
  fireEvent.click(screen.getByRole("button", { name: "Passed" }));
  expect(screen.getByRole("button", { name: "Passed" })).toHaveClass("bg-success");

  // ...and Next records it like any other.
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  await vi.waitFor(() => expect(recorded).toHaveLength(1));
  expect(recorded[0].resultId).toBe(70);
  expect((recorded[0].outcome as Record<string, unknown>).outcome).toBe("Passed");
});

/// The Run Tests list and the runner used to disagree about order: the
/// list shows the suite's points, while the runner fetches the PBI's
/// Tested-By links - two ADO artifacts with no shared ordering contract.
/// The session now carries the list's order and the runner follows it.
test("the runner walks cases in the session's caseIds order", async () => {
  localStorage.setItem(
    "tcm-v2-runner-session",
    JSON.stringify({
      org: "acme",
      project: "Web",
      planId: 9,
      planName: "Plan",
      suiteId: 91,
      pbi: { id: 42, title: "Login flow", work_item_type: "Product Backlog Item" },
      // Reversed relative to the fetch below: the session's order must win.
      caseIds: [202, 201],
    }),
  );
  mockIPC((cmd) => {
    if (cmd === "run_history") return [];
    if (cmd === "pbi_test_cases_full")
      return [fullCase, { ...fullCase, id: 202, title: "Second case" }];
    if (cmd === "list_test_points") return [];
  });
  renderRunner();
  // First up is 202, not the fetch's first (201).
  expect(await screen.findByText("Second case")).toBeInTheDocument();
  expect(screen.queryByText("Valid login")).not.toBeInTheDocument();
});

test("a caseOrder hint orders the full run without restricting it", async () => {
  localStorage.setItem(
    "tcm-v2-runner-session",
    JSON.stringify({
      org: "acme",
      project: "Web",
      planId: 9,
      planName: "Plan",
      suiteId: 91,
      pbi: { id: 42, title: "Login flow", work_item_type: "Product Backlog Item" },
      // Only 202 is hinted - 201 must still be in the run, after it.
      caseOrder: [202],
    }),
  );
  mockIPC((cmd) => {
    if (cmd === "run_history") return [];
    if (cmd === "pbi_test_cases_full")
      return [fullCase, { ...fullCase, id: 202, title: "Second case" }];
    if (cmd === "list_test_points") return [];
  });
  renderRunner();
  expect(await screen.findByText("Second case")).toBeInTheDocument();
  // Both cases are in the session: "1/2", not a filtered "1/1". The
  // counter renders as sibling text nodes, so match on textContent.
  expect(
    screen.getByText((_, el) => el?.tagName === "SPAN" && el.textContent === "1/2"),
  ).toBeInTheDocument();
});

/// A case can sit in the suite (so the Run Tests list shows it) without a
/// Tested-By link to the PBI (so the runner's fetch misses it). It used to
/// vanish silently - "Run 2" walked 1. Missing session cases are now
/// fetched by id and merged in the session's order.
test("a session case missing from the PBI fetch is backfilled by id", async () => {
  localStorage.setItem(
    "tcm-v2-runner-session",
    JSON.stringify({
      org: "acme",
      project: "Web",
      planId: 9,
      planName: "Plan",
      suiteId: 91,
      pbi: { id: 42, title: "Login flow", work_item_type: "Product Backlog Item" },
      caseIds: [999, 201], // 999 has no Tested-By link; it still leads.
    }),
  );
  mockIPC((cmd, args) => {
    if (cmd === "run_history") return [];
    if (cmd === "pbi_test_cases_full") return [fullCase];
    if (cmd === "test_cases_by_ids") {
      expect((args as { ids: number[] }).ids).toEqual([999]);
      return [{ ...fullCase, id: 999, title: "Unlinked case" }];
    }
    if (cmd === "list_test_points") return [];
  });
  renderRunner();
  expect(await screen.findByText("Unlinked case")).toBeInTheDocument();
  expect(
    screen.getByText((_, el) => el?.tagName === "SPAN" && el.textContent === "1/2"),
  ).toBeInTheDocument();
});

/// A static suite has no PBI at all: its runner session says pbi id 0 and
/// carries every case id. Bodies come from the by-ids fetch alone, and the
/// header names the case on screen - never a meaningless "#0".
test("a suite-scoped session runs without any PBI", async () => {
  localStorage.setItem(
    "tcm-v2-runner-session",
    JSON.stringify({
      org: "acme",
      project: "Web",
      planId: 9,
      planName: "Plan",
      suiteId: 91,
      pbi: { id: 0, title: "Sprint 1 - Story suite", work_item_type: "" },
      caseIds: [301, 302],
    }),
  );
  mockIPC((cmd) => {
    if (cmd === "run_history") return [];
    if (cmd === "pbi_test_cases_full") throw new Error("must not ask a PBI that does not exist");
    if (cmd === "test_cases_by_ids")
      return [
        { ...fullCase, id: 301, title: "Suite case one" },
        { ...fullCase, id: 302, title: "Suite case two" },
      ];
    if (cmd === "list_test_points") return [];
  });
  renderRunner();
  expect(await screen.findByText("Suite case one")).toBeInTheDocument();
  expect(screen.getByText("#301")).toBeInTheDocument();
  expect(screen.queryByText("#0")).not.toBeInTheDocument();
});

/// The number beside the title is the CASE on screen, not the PBI. The
/// tester is looking at one work item - that is the number they quote in
/// a bug or look up in Azure DevOps; the PBI is the main window's context.
test("the header names the case on screen, not the PBI", async () => {
  mockIPC((cmd) => {
    if (cmd === "run_history") return [];
    if (cmd === "pbi_test_cases_full")
      return [fullCase, { ...fullCase, id: 202, title: "Invalid login" }];
    if (cmd === "list_test_points") return [];
  });
  renderRunner();
  expect(await screen.findByText("Valid login")).toBeInTheDocument();
  expect(screen.getByText("#201")).toBeInTheDocument();
  expect(screen.queryByText("#42")).not.toBeInTheDocument();
});

test("session caseIds restrict the runner's case list", async () => {
  localStorage.setItem(
    "tcm-v2-runner-session",
    JSON.stringify({
      org: "acme",
      project: "Web",
      planId: 9,
      planName: "Plan",
      suiteId: 91,
      pbi: { id: 42, title: "Login flow", work_item_type: "Product Backlog Item" },
      caseIds: [999],
    }),
  );
  mockIPC((cmd) => {
    if (cmd === "run_history") return [];
    if (cmd === "pbi_test_cases_full") return [fullCase];
    // The unknown id is backfilled by id and comes back empty - it
    // genuinely does not exist, so the run is genuinely empty.
    if (cmd === "test_cases_by_ids") return [];
    if (cmd === "list_test_points") return [];
  });
  renderRunner();
  expect(await screen.findByText("No linked test cases to run.")).toBeInTheDocument();
  expect(screen.queryByText("Valid login")).not.toBeInTheDocument();
});

/// The runner offers the same verdicts ADO's own runner does - including
/// Paused, for a case someone had to stop half way through. It records
/// like any other outcome.
test("Paused is offered and records as a real outcome", async () => {
  const recorded: Array<Record<string, unknown>> = [];
  mockIPC((cmd, args) => {
    if (cmd === "run_history") return [];
    if (cmd === "pbi_test_cases_full") return [fullCase];
    if (cmd === "list_test_points")
      return [
        {
          point_id: 7,
          test_case_id: 201,
          test_case_name: "Valid login",
          config_name: "W10",
          tester: "",
          last_outcome: "",
          last_run_id: null,
          last_result_id: null,
        },
      ];
    if (cmd === "start_test_run")
      return { run_id: 300, web_url: "", results: [{ point_id: 7, result_id: 70 }], unmatched: [] };
    if (cmd === "record_result") {
      recorded.push(args as Record<string, unknown>);
      return [];
    }
    if (cmd === "finish_test_run") return null;
  });
  renderRunner();
  await screen.findByText("Valid login");

  fireEvent.click(screen.getByRole("button", { name: "Paused" }));
  expect(screen.getByRole("button", { name: "Paused" })).toHaveClass("bg-muted");
  fireEvent.click(screen.getByRole("button", { name: /Finish \(1\)/ }));
  await vi.waitFor(() => expect(recorded).toHaveLength(1));
  expect((recorded[0].outcome as Record<string, unknown>).outcome).toBe("Paused");
});

/** Offline, the runner PAUSES instead of failing: Next defers the record
 * silently (the mark is kept), Finish is disabled with the reason, and
 * the connection returning flushes everything unsent. */
test("offline defers records and the reconnect flushes them", async () => {
  const recorded: Array<Record<string, unknown>> = [];
  mockIPC((cmd, args) => {
    if (cmd === "run_history") return [];
    if (cmd === "pbi_test_cases_full")
      return [fullCase, { ...fullCase, id: 202, title: "Invalid login" }];
    if (cmd === "list_test_points")
      return [
        {
          point_id: 7,
          test_case_id: 201,
          test_case_name: "Valid login",
          config_name: "W10",
          tester: "",
          last_outcome: "",
          last_run_id: null,
          last_result_id: null,
        },
        {
          point_id: 8,
          test_case_id: 202,
          test_case_name: "Invalid login",
          config_name: "W10",
          tester: "",
          last_outcome: "",
          last_run_id: null,
          last_result_id: null,
        },
      ];
    if (cmd === "start_test_run")
      return {
        run_id: 300,
        web_url: "",
        results: [
          { point_id: 7, result_id: 70 },
          { point_id: 8, result_id: 80 },
        ],
        unmatched: [],
      };
    if (cmd === "record_result") {
      recorded.push(args as Record<string, unknown>);
      return [];
    }
  });
  renderRunner();
  await screen.findByText("Valid login");

  // The network dies.
  Object.defineProperty(navigator, "onLine", { value: false, configurable: true });
  window.dispatchEvent(new Event("offline"));
  try {
    fireEvent.click(screen.getByRole("button", { name: "Failed" }));
    fireEvent.click(screen.getByRole("button", { name: "Next" }));

    // Deferred, not attempted - a record now is known-doomed.
    await new Promise((r) => setTimeout(r, 50));
    expect(recorded).toHaveLength(0);
    // And Finish says why it is off.
    expect(screen.getByRole("button", { name: /Finish/ })).toBeDisabled();

    // The connection returns: everything unsent flushes on its own.
    Object.defineProperty(navigator, "onLine", { value: true, configurable: true });
    window.dispatchEvent(new Event("online"));
    await vi.waitFor(() => expect(recorded).toHaveLength(1));
    expect((recorded[0].outcome as Record<string, unknown>).outcome).toBe("Failed");
  } finally {
    Object.defineProperty(navigator, "onLine", { value: true, configurable: true });
    window.dispatchEvent(new Event("online"));
  }
});

test("File bug appears only after a failure", async () => {
  mockIPC((cmd) => {
    if (cmd === "run_history") return [];
    if (cmd === "pbi_test_cases_full") return [fullCase];
    if (cmd === "list_test_points") return [];
  });
  renderRunner();
  await screen.findByText("Valid login");
  expect(screen.queryByRole("button", { name: "File bug" })).not.toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Failed" }));
  expect(screen.getByRole("button", { name: "File bug" })).toBeInTheDocument();
});

test("the case's preconditions show above the steps; absent ones render nothing", async () => {
  // Preconditions live in an org-specific CUSTOM field: the runner must
  // pass the detected reference name or ADO returns them empty (the bug
  // where the block never showed). Seed the shared field-prefs cache and
  // assert the ref reaches the fetch.
  localStorage.setItem(
    "tcm-v2-fields:acme/Web",
    JSON.stringify({ moduleRef: "Custom.Module", preconditionsRef: "Custom.Preconditions" }),
  );
  let fetchArgs: Record<string, unknown> | null = null;
  mockIPC((cmd, args) => {
    if (cmd === "pbi_test_cases_full") {
      fetchArgs = args as Record<string, unknown>;
      return [{ ...fullCase, preconditions: "A demo account exists and is unlocked" }];
    }
    if (cmd === "list_test_points") return [];
    if (cmd === "run_history") return [];
    if (cmd === "list_test_case_fields") return [];
  });
  renderRunner();
  expect(await screen.findByText("Preconditions")).toBeInTheDocument();
  expect(screen.getByText("A demo account exists and is unlocked")).toBeInTheDocument();
  expect(fetchArgs).toMatchObject({ preconditionsRef: "Custom.Preconditions" });
});

test("no preconditions - no block", async () => {
  mockIPC((cmd) => {
    if (cmd === "pbi_test_cases_full") return [fullCase]; // preconditions: ""
    if (cmd === "list_test_points") return [];
    if (cmd === "run_history") return [];
  });
  renderRunner();
  await screen.findByText("Valid login");
  expect(screen.queryByText("Preconditions")).not.toBeInTheDocument();
});

test("the pin toggle is remembered, and a stored 'off' starts unpinned", async () => {
  localStorage.setItem("tcm-v2-runner-pinned", "off");
  mockIPC((cmd) => {
    if (cmd === "pbi_test_cases_full") return [fullCase];
    if (cmd === "list_test_points") return [];
    if (cmd === "run_history") return [];
    if (cmd === "list_test_case_fields") return [];
  });
  renderRunner();
  // Stored preference wins over the old always-pinned default.
  const pin = await screen.findByLabelText("Pin on top");
  fireEvent.click(pin);
  // Toggling back on persists for the NEXT run.
  expect(localStorage.getItem("tcm-v2-runner-pinned")).toBe("on");
  expect(screen.getByLabelText("Unpin (allow other windows on top)")).toBeInTheDocument();
});

/// Ctrl+V is the paste that always works: the event carries the image
/// with no permission prompt, unlike the Paste button's async clipboard
/// read, which some WebView2 setups refuse.
test("pasting an image (Ctrl+V) attaches it to the current case", async () => {
  mockIPC((cmd) => {
    if (cmd === "run_history") return [];
    if (cmd === "pbi_test_cases_full") return [fullCase];
    if (cmd === "list_test_points") return [];
  });
  renderRunner();
  await screen.findByText("Valid login");
  // The window's paste listener is attached by an effect that runs AFTER
  // the case renders. `findByText` returns on the render, so under the full
  // suite's load the paste used to fire before that effect had run: it
  // reached no listener, and no wait was long enough for an image that was
  // never going to come. Flushing React's pending effects first is what
  // makes the paste land.
  await act(async () => {});

  // A whole image: the runner no longer attaches bytes it cannot decode.
  const file = new File([pngBytes() as BlobPart], "clip.png", { type: "image/png" });
  fireEvent.paste(window, {
    clipboardData: { items: [{ type: "image/png", getAsFile: () => file }] },
  });

  // The attachment lands as this case's next pasted-*.png thumbnail
  // (the fullscreen viewer holds a second copy of the same image), after a
  // file read and a render - several async hops, so the longer wait stays.
  expect((await screen.findAllByAltText("pasted-201-1.png", undefined, { timeout: 5000 })).length).toBeGreaterThan(0);
});

// --- A screenshot that has not reached the clipboard yet ---
//
// A virtual machine's clipboard sync offers the image/png format before the
// image data is ready, so the paste event's file is empty or cut short. The
// runner used to attach that as it came - a thumbnail showing only its alt
// text - and toast "Pasted screenshot". Now it reads the clipboard again a
// moment later, and attaches nothing it cannot decode.

const NOT_YET = "The screenshot had not reached the clipboard yet - paste again.";
/** The first bytes of a real PNG: what a paste cut off mid-transfer holds. */
const truncatedPng = () => pngBytes().slice(0, 40);

/** navigator.clipboard.read() answering one entry per call (the last one
 * repeats): bytes for an image, null for no image, "throw" for a refusal. */
function mockClipboardReads(reads: Array<Uint8Array | null | "throw">) {
  let n = 0;
  const read = vi.fn(async () => {
    const r = reads[Math.min(n++, reads.length - 1)];
    if (r === "throw") throw new Error("denied");
    if (r === null) return [];
    return [
      {
        types: ["image/png"],
        getType: async () => new Blob([r as BlobPart], { type: "image/png" }),
      },
    ];
  });
  Object.defineProperty(navigator, "clipboard", { value: { read }, configurable: true });
  return read;
}

function noClipboardRead() {
  Object.defineProperty(navigator, "clipboard", { value: undefined, configurable: true });
}

afterEach(() => {
  noClipboardRead();
  decode.accepts = false;
  decode.calls.length = 0;
  for (const m of Object.values(toastMock)) m.mockClear();
});

async function openRunnerForPaste() {
  mockIPC((cmd) => {
    if (cmd === "run_history") return [];
    if (cmd === "pbi_test_cases_full") return [fullCase];
    if (cmd === "list_test_points") return [];
  });
  renderRunner();
  await screen.findByText("Valid login");
  await act(async () => {}); // the paste listener's effect has run
}

function pasteEvent(bytes: Uint8Array, type = "image/png") {
  const file = new File([bytes as BlobPart], "clip", { type });
  fireEvent.paste(window, {
    clipboardData: { items: [{ type, getAsFile: () => file }] },
  });
}

const successToasts = () =>
  toastMock.success.mock.calls.filter((c) => /screenshot|snip/i.test(String(c[0])));

test("an empty image in a paste event attaches nothing and says it had not reached the clipboard yet", async () => {
  noClipboardRead();
  await openRunnerForPaste();

  pasteEvent(new Uint8Array(0));

  await vi.waitFor(() => expect(toastMock.info).toHaveBeenCalledWith(NOT_YET), { timeout: 5000 });
  expect(screen.queryAllByAltText("pasted-201-1.png")).toHaveLength(0);
  expect(successToasts()).toHaveLength(0);
});

test("a truncated PNG in a paste event attaches nothing either", async () => {
  noClipboardRead();
  await openRunnerForPaste();

  pasteEvent(truncatedPng());

  await vi.waitFor(() => expect(toastMock.info).toHaveBeenCalledWith(NOT_YET), { timeout: 5000 });
  expect(screen.queryAllByAltText("pasted-201-1.png")).toHaveLength(0);
  expect(successToasts()).toHaveLength(0);
});

test("a paste event that stays unreadable after three more reads attaches nothing", async () => {
  const read = mockClipboardReads([new Uint8Array(0)]);
  await openRunnerForPaste();

  pasteEvent(new Uint8Array(0));

  await vi.waitFor(() => expect(toastMock.info).toHaveBeenCalledWith(NOT_YET), { timeout: 5000 });
  expect(read).toHaveBeenCalledTimes(3);
  expect(screen.queryAllByAltText("pasted-201-1.png")).toHaveLength(0);
  expect(successToasts()).toHaveLength(0);
});

test("an invalid then valid clipboard read attaches exactly one image after a retry", async () => {
  const read = mockClipboardReads([new Uint8Array(0), pngBytes()]);
  await openRunnerForPaste();

  pasteEvent(new Uint8Array(0));

  const thumbs = await screen.findAllByAltText("pasted-201-1.png", undefined, { timeout: 5000 });
  expect(thumbs[0]).toHaveAttribute("src", `data:image/png;base64,${PNG_B64}`);
  expect(read).toHaveBeenCalledTimes(2); // the empty read, then the real one
  expect(screen.queryAllByAltText("pasted-201-2.png")).toHaveLength(0);
  expect(toastMock.success).toHaveBeenCalledTimes(1);
  expect(toastMock.info).not.toHaveBeenCalledWith(NOT_YET);
});

// The paste event hands over the image itself. Its retry goes back to that
// file first; the async clipboard (which a WebView2 setup may prompt for,
// and which holds whatever was copied LAST) is read only when the event
// had nothing.
test("a cut-off image in the paste event is retried from the event, never from the async clipboard", async () => {
  const read = mockClipboardReads([pngBytes()]);
  await openRunnerForPaste();

  pasteEvent(truncatedPng());

  await vi.waitFor(() => expect(toastMock.info).toHaveBeenCalledWith(NOT_YET), { timeout: 5000 });
  expect(read).not.toHaveBeenCalled();
  expect(screen.queryAllByAltText("pasted-201-1.png")).toHaveLength(0);
  expect(successToasts()).toHaveLength(0);
});

// A copied image file (Explorer, a browser) is a JPEG or a GIF, not a PNG.
// It used to be judged as a PNG and refused every time.
test("pasting a whole JPEG attaches it with its own extension", async () => {
  noClipboardRead();
  await openRunnerForPaste();

  pasteEvent(jpegBytes(), "image/jpeg");

  const thumbs = await screen.findAllByAltText("pasted-201-1.jpg", undefined, { timeout: 5000 });
  expect(thumbs[0]).toHaveAttribute("src", `data:image/jpeg;base64,${bytesToB64(jpegBytes())}`);
  expect(decode.calls[decode.calls.length - 1]?.mime).toBe("image/jpeg");
  expect(toastMock.success).toHaveBeenCalledWith("Pasted screenshot");
  expect(toastMock.info).not.toHaveBeenCalledWith(NOT_YET);
});

test("a JPEG that is cut off is refused like any other picture", async () => {
  noClipboardRead();
  await openRunnerForPaste();

  pasteEvent(jpegBytes().slice(0, 12), "image/jpeg");

  await vi.waitFor(() => expect(toastMock.info).toHaveBeenCalledWith(NOT_YET), { timeout: 5000 });
  expect(screen.queryAllByAltText("pasted-201-1.jpg")).toHaveLength(0);
  expect(successToasts()).toHaveLength(0);
});

test("the Paste button waits for a screenshot that has not arrived, then attaches it once", async () => {
  mockClipboardReads([truncatedPng(), pngBytes()]);
  await openRunnerForPaste();

  fireEvent.click(screen.getByRole("button", { name: "Paste" }));

  const thumbs = await screen.findAllByAltText("pasted-201-1.png", undefined, { timeout: 5000 });
  expect(thumbs[0]).toHaveAttribute("src", `data:image/png;base64,${PNG_B64}`);
  expect(screen.queryAllByAltText("pasted-201-2.png")).toHaveLength(0);
  expect(toastMock.success).toHaveBeenCalledTimes(1);
});

test("the Paste button attaches nothing when the image never becomes readable", async () => {
  mockClipboardReads([new Uint8Array(0)]);
  await openRunnerForPaste();

  fireEvent.click(screen.getByRole("button", { name: "Paste" }));

  await vi.waitFor(() => expect(toastMock.info).toHaveBeenCalledWith(NOT_YET), { timeout: 5000 });
  expect(screen.queryAllByAltText("pasted-201-1.png")).toHaveLength(0);
  expect(successToasts()).toHaveLength(0);
});

test("the snip poll ignores an invalid read and keeps polling", async () => {
  windowMock.minimize.mockClear();
  // before the snip: no image; first poll: a cut-off one; second poll: whole.
  const read = mockClipboardReads([null, truncatedPng(), pngBytes()]);
  mockIPC((cmd) => {
    if (cmd === "run_history") return [];
    if (cmd === "pbi_test_cases_full") return [fullCase];
    if (cmd === "list_test_points") return [];
    if (cmd === "open_snip") return null;
  });
  renderRunner();
  await screen.findByText("Valid login");
  await act(async () => {});

  fireEvent.click(screen.getByRole("button", { name: "Snip" }));

  const thumbs = await screen.findAllByAltText("snip-201-1.png", undefined, { timeout: 8000 });
  // The cut-off read was skipped, not attached: the image that landed is the whole one.
  expect(thumbs[0]).toHaveAttribute("src", `data:image/png;base64,${PNG_B64}`);
  expect(read).toHaveBeenCalledTimes(3);
  expect(toastMock.success).toHaveBeenCalledTimes(1);
}, 15000);

test("addImage ignores an empty image even if the decoder were to accept it", async () => {
  decode.accepts = true; // a future caller that skips the real check
  noClipboardRead();
  await openRunnerForPaste();

  pasteEvent(new Uint8Array(0));

  await vi.waitFor(() => expect(toastMock.info).toHaveBeenCalledWith(NOT_YET), { timeout: 5000 });
  expect(screen.queryAllByAltText("pasted-201-1.png")).toHaveLength(0);
  expect(successToasts()).toHaveLength(0);
});

/** Mocks for a two-case run that records into a run, answering the Attach
 * file dialog with `files` one after another. */
function mockRecordingWithFiles(
  calls: Array<{ cmd: string; args: Record<string, unknown> }>,
  files: Array<{ file_name: string; b64: string }>,
) {
  let next = 0;
  mockIPC((cmd, args) => {
    if (cmd === "run_history") return [];
    if (cmd === "pbi_test_cases_full") return [fullCase, { ...fullCase, id: 202, title: "Invalid login" }];
    if (cmd === "list_test_points")
      return [7, 8].map((id, i) => ({
        point_id: id,
        test_case_id: 201 + i,
        test_case_name: i ? "Invalid login" : "Valid login",
        config_name: "W10",
        tester: "",
        last_outcome: "",
        last_run_id: null,
        last_result_id: null,
      }));
    if (cmd === "start_test_run")
      return {
        run_id: 300,
        web_url: "https://x/run/300",
        results: [
          { point_id: 7, result_id: 70 },
          { point_id: 8, result_id: 80 },
        ],
        unmatched: [],
      };
    if (cmd === "plugin:dialog|open") return `C:/files/${files[next].file_name}`;
    if (cmd === "read_file_b64") return files[next++];
    if (cmd === "record_result") {
      calls.push({ cmd, args: args as Record<string, unknown> });
      return [];
    }
    if (cmd === "finish_test_run") return null;
  });
}

const attachedTo = (call: { args: Record<string, unknown> }) =>
  (call.args.outcome as { attachments: Array<{ file_name: string }> | null }).attachments;

test("an image from Attach file that is damaged when results are recorded is left out and said so", async () => {
  const calls: Array<{ cmd: string; args: Record<string, unknown> }> = [];
  mockRecordingWithFiles(calls, [{ file_name: "cut.png", b64: bytesToB64(truncatedPng()) }]);
  renderRunner();
  await screen.findByText("Valid login");
  await act(async () => {});

  // Attach file does not look at the picture: a cut-off one gets onto the case...
  fireEvent.click(screen.getByRole("button", { name: /Attach file/ }));
  await screen.findByLabelText("Remove cut.png");

  // ...and the check at record time keeps it off the result.
  fireEvent.click(screen.getByRole("button", { name: "Passed" }));
  fireEvent.click(screen.getByRole("button", { name: "Next" }));

  await vi.waitFor(() => expect(calls.length).toBeGreaterThan(0), { timeout: 5000 });
  expect(attachedTo(calls[0])).toBeNull();
  await vi.waitFor(() =>
    expect(toastMock.warning.mock.calls.some((c) => String(c[0]).includes("cut.png"))).toBe(true),
  );
});

/// An image is decoded once, not at every sync. A picture pasted or
/// snipped was checked whole when it was attached; one from Attach file is
/// checked the first time it is synced; a later sync of the same case only
/// reads the answer.
test("each image is decoded once, however many times its case is recorded", async () => {
  const calls: Array<{ cmd: string; args: Record<string, unknown> }> = [];
  mockRecordingWithFiles(calls, [{ file_name: "photo.jpg", b64: bytesToB64(jpegBytes()) }]);
  renderRunner();
  await screen.findByText("Valid login");
  await act(async () => {});

  // A pasted picture is checked once, when it is pasted.
  pasteEvent(pngBytes());
  await screen.findAllByAltText("pasted-201-1.png", undefined, { timeout: 5000 });
  const afterPaste = decode.calls.length;
  fireEvent.click(screen.getByRole("button", { name: /Attach file/ }));
  await screen.findByLabelText("Remove photo.jpg");

  fireEvent.click(screen.getByRole("button", { name: "Passed" }));
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  await vi.waitFor(() => expect(calls).toHaveLength(1), { timeout: 5000 });
  expect(attachedTo(calls[0])!.map((a) => a.file_name)).toEqual(["pasted-201-1.png", "photo.jpg"]);
  // Only the file from Attach file was decoded at the sync; the pasted
  // picture was answered from what the paste found.
  expect(decode.calls.length - afterPaste).toBe(1);

  // Change the case and record it again: nothing is decoded a second time.
  fireEvent.click(screen.getByRole("button", { name: "Prev" }));
  await screen.findByText("Valid login");
  fireEvent.click(screen.getByRole("button", { name: "Failed" }));
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  await vi.waitFor(() => expect(calls).toHaveLength(2), { timeout: 5000 });
  expect(attachedTo(calls[1])).toHaveLength(2);
  expect(decode.calls.length - afterPaste).toBe(1);
}, 20000);

/** Single-screen snipping: the runner covers the very thing the tester
 * wants to capture, so Snip minimizes the window BEFORE the overlay
 * freezes the screen - and Cancel snip brings it back with focus. (The
 * success and timeout paths restore through the same helper.) */
test("Snip minimizes the runner and Cancel snip restores it", async () => {
  windowMock.minimize.mockClear();
  windowMock.unminimize.mockClear();
  windowMock.setFocus.mockClear();
  mockIPC((cmd) => {
    if (cmd === "run_history") return [];
    if (cmd === "pbi_test_cases_full") return [fullCase];
    if (cmd === "list_test_points") return [];
    if (cmd === "open_snip") return null;
  });
  renderRunner();
  expect(await screen.findByText("Valid login")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Snip" }));
  // Out of the way first: minimize precedes the overlay opening.
  await vi.waitFor(() => expect(windowMock.minimize).toHaveBeenCalled());
  expect(windowMock.unminimize).not.toHaveBeenCalled();

  // The overlay is up; the same control now cancels - and brings the
  // window back, focused.
  fireEvent.click(await screen.findByRole("button", { name: "Cancel snip" }));
  await vi.waitFor(() => expect(windowMock.unminimize).toHaveBeenCalled());
  await vi.waitFor(() => expect(windowMock.setFocus).toHaveBeenCalled());
  expect(screen.getByRole("button", { name: "Snip" })).toBeInTheDocument();
});

// --- Staying in step with Run Tests (design doc §5.2) ---

const RUN3 = [
  { ...fullCase, id: 201, title: "Alpha check" },
  { ...fullCase, id: 202, title: "Bravo check" },
  { ...fullCase, id: 203, title: "Charlie check" },
];

const RUN4 = [
  { ...fullCase, id: 201, title: "Alpha check" },
  { ...fullCase, id: 202, title: "Bravo check" },
  { ...fullCase, id: 203, title: "Charlie check" },
  { ...fullCase, id: 204, title: "Delta check" },
];

// Events mocked for real (shouldMockEvents), so an emit reaches this
// window's listener the way another window's My-order save would.
function mockRunnerCases(cases: typeof RUN3) {
  mockIPC(
    (cmd) => {
      if (cmd === "run_history") return [];
      if (cmd === "pbi_test_cases_full") return cases;
      if (cmd === "list_test_points") return [];
    },
    { shouldMockEvents: true },
  );
}

test("a My order save from Run Tests re-sorts the upcoming cases, leaving the one on screen alone", async () => {
  mockRunnerCases(RUN3);
  renderRunner();
  await screen.findByText("Alpha check");

  localStorage.setItem("tcm-v2-run-order:acme/9/91", JSON.stringify([201, 203, 202]));
  // act() flushes the listener's state update, so the Next click below
  // walks the re-sorted order - without the listener, Next lands on Bravo.
  await act(async () => {
    await emit(MY_ORDER_EVENT, { org: "acme", planId: 9, suiteId: 91 });
  });

  // The re-sort only ever touches what comes AFTER the current case.
  expect(screen.getByText("Alpha check")).toBeInTheDocument();

  // Next now walks to Charlie, per the new order, not Bravo.
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  await screen.findByText("Charlie check");
  expect(screen.queryByText("Bravo check")).not.toBeInTheDocument();
});

test("marked cases after the current one keep their position under a re-sort", async () => {
  mockRunnerCases(RUN4);
  renderRunner();
  await screen.findByText("Alpha check");

  // Mark Bravo, then come back to Alpha - it stays marked from here on.
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  await screen.findByText("Bravo check");
  fireEvent.click(screen.getByRole("button", { name: "Passed" }));
  fireEvent.click(screen.getByRole("button", { name: "Prev" }));
  await screen.findByText("Alpha check");

  // Another window's My order puts Delta ahead of Charlie.
  localStorage.setItem("tcm-v2-run-order:acme/9/91", JSON.stringify([201, 204, 202, 203]));
  await emit(MY_ORDER_EVENT, { org: "acme", planId: 9, suiteId: 91 });

  // Bravo (marked) is still the very next case - untouched by the re-sort.
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  await screen.findByText("Bravo check");
  // Delta now comes before Charlie, per the new order.
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  await screen.findByText("Delta check");
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  await screen.findByText("Charlie check");
});

test("a late backfill never swaps the case on screen once the tester has touched it", async () => {
  localStorage.setItem(
    "tcm-v2-runner-session",
    JSON.stringify({
      org: "acme",
      project: "Web",
      planId: 9,
      planName: "Plan",
      suiteId: 91,
      pbi: { id: 42, title: "Login flow", work_item_type: "Product Backlog Item" },
      caseIds: [999, 201, 202], // 999 has no Tested-By link and ranks FIRST.
    }),
  );
  let resolveBackfill!: (v: unknown) => void;
  const backfillPromise = new Promise((resolve) => {
    resolveBackfill = resolve;
  });
  mockIPC((cmd, args) => {
    if (cmd === "run_history") return [];
    if (cmd === "pbi_test_cases_full")
      return [
        { ...fullCase, id: 201, title: "Alpha check" },
        { ...fullCase, id: 202, title: "Bravo check" },
      ];
    if (cmd === "test_cases_by_ids") {
      expect((args as { ids: number[] }).ids).toEqual([999]);
      return backfillPromise;
    }
    if (cmd === "list_test_points") return [];
  });
  renderRunner();
  await screen.findByText("Alpha check");

  // The tester marks the case on screen BEFORE 999 arrives.
  fireEvent.click(screen.getByRole("button", { name: "Passed" }));

  // 999 lands now, ranked FIRST by the session - but the tester already
  // touched Alpha, so it must not swap out from under them.
  resolveBackfill([{ ...fullCase, id: 999, title: "Unlinked case" }]);
  await vi.waitFor(() =>
    expect(
      screen.getByText((_, el) => el?.tagName === "SPAN" && el.textContent === "2/3"),
    ).toBeInTheDocument(),
  );
  expect(screen.getByText("Alpha check")).toBeInTheDocument();
});

/** Per-step marks on a shared row cannot be recorded (Task 9), so the
 * Runner shows the reference and offers no marks for it. */
test("a shared step shows its reference and has no Pass/Fail marks", async () => {
  mockIPC((cmd, args) => {
    if (cmd === "run_history") return [];
    if (cmd === "pbi_test_cases_full")
      return [
        {
          ...fullCase,
          steps: [fullCase.steps[0], { action: "", expected: "", shared: 812 }, fullCase.steps[1]],
          step_ids: ["2", "", "3"],
        },
      ];
    if (cmd === "list_test_points")
      return [
        {
          point_id: 7, test_case_id: 201, test_case_name: "Valid login", config_name: "W10",
          tester: "", last_outcome: "", last_run_id: null, last_result_id: null,
        },
      ];
    if (cmd === "test_cases_by_ids")
      return (args as { ids: number[] }).ids.map((id) => ({
        ...fullCase, id, title: "Sign in as an admin", steps: [], step_ids: [], steps_xml: "",
      }));
  });
  renderRunner();
  expect(await screen.findByText("Valid login")).toBeInTheDocument();
  expect(screen.getByText("Shared steps #812")).toBeInTheDocument();
  expect(await screen.findByText(/Sign in as an admin/)).toBeInTheDocument();
  expect(screen.getAllByTitle("Passed")).toHaveLength(2); // the two local steps only
});
