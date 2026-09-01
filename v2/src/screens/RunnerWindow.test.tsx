import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
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
  // Mark step 1 passed.
  fireEvent.click(screen.getAllByTitle("Passed")[0]);
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
/// un-marks). A previously-run case opens unmarked with the last verdict
/// pulsing on its button; never-run cases stay entirely blank.
test("a previously-run case opens unmarked with its last verdict pulsing", async () => {
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

  // Case 201 failed last time: the Failed button arrives UNLIT, carrying
  // a pulsing dot and a tooltip naming it as the previous result instead.
  const failedBtn = await screen.findByTitle("Failed - the previous run's result");
  expect(failedBtn).not.toHaveClass("bg-danger");
  expect(failedBtn.querySelector(".animate-pulse")).toBeTruthy();
  // Nothing is marked yet - the indicator is information, not a mark -
  // and the off-list point 999 adds nothing either.
  expect(screen.getByTitle("0 of 2 marked")).toHaveTextContent("1/2");
  expect(screen.getByRole("button", { name: /Finish \(0\)/ })).toBeInTheDocument();

  // Case 202 has never run: it must arrive with NOTHING selected and
  // nothing pulsing - a blank slate is information too.
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
/// header names the suite instead of a meaningless "#0".
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
  expect(screen.getByText("Sprint 1 - Story suite")).toBeInTheDocument();
  expect(screen.queryByText("#0")).not.toBeInTheDocument();
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

  const file = new File([new Uint8Array([137, 80, 78, 71])], "clip.png", { type: "image/png" });
  fireEvent.paste(window, {
    clipboardData: { items: [{ type: "image/png", getAsFile: () => file }] },
  });

  // The attachment lands as this case's next pasted-*.png thumbnail
  // (the fullscreen viewer holds a second copy of the same image).
  expect((await screen.findAllByAltText("pasted-201-1.png")).length).toBeGreaterThan(0);
});

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
