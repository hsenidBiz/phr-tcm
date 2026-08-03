import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import RunnerWindow from "./RunnerWindow";

// getCurrentWindow() must be a no-op in jsdom - and its methods must
// return PROMISES: the component chains .catch() on them, and a bare
// vi.fn() (undefined) threw an unhandled error that failed the release
// gate even with every assertion green.
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    close: vi.fn(() => Promise.resolve()),
    setFocus: vi.fn(() => Promise.resolve()),
    setAlwaysOnTop: vi.fn(() => Promise.resolve()),
  }),
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

test("plays a case, records an outcome, submits per-point with step results", async () => {
  let submitted: { runName?: string; outcomes?: Array<Record<string, unknown>> } = {};
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
    if (cmd === "submit_test_run") {
      const a = args as { runName: string; outcomes: Array<Record<string, unknown>> };
      submitted = { runName: a.runName, outcomes: a.outcomes };
      return { run_id: 300, web_url: "https://x/run/300" };
    }
  });
  renderRunner();

  expect(await screen.findByText("Valid login")).toBeInTheDocument();
  // Mark step 1 passed.
  fireEvent.click(screen.getAllByTitle("Passed")[0]);
  // Overall Passed.
  fireEvent.click(screen.getByRole("button", { name: "Passed" }));
  fireEvent.click(screen.getByRole("button", { name: /Finish \(1\)/ }));

  await vi.waitFor(() => expect(submitted.outcomes).toBeTruthy());
  expect(submitted.runName).toBe("Login flow - manual run");
  const o = submitted.outcomes![0];
  expect(o.point_id).toBe(7);
  expect(o.outcome).toBe("Passed");
  expect(o.step_ids).toEqual(["2", "3"]);
  expect(o.step_outcomes).toEqual(["Passed", null]);
});

/// A tester re-running a suite only touches what changed: every case opens
/// with its last outcome already selected, and pre-selected marks count
/// toward Finish exactly like clicked ones.
test("each case opens with its last outcome pre-selected, and it counts", async () => {
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
      ];
    if (cmd === "get_result_detail") return { outcome: "failed", comment: "" };
    if (cmd === "result_screenshots") return [];
  });
  renderRunner();
  await screen.findByText("Valid login");

  // Case 201 failed last time: the Failed button arrives lit (the outcome
  // classes, not the unselected border) and the mark is already counted.
  await vi.waitFor(() => {
    expect(screen.getByRole("button", { name: "Failed" })).toHaveClass("bg-danger");
  });
  expect(screen.getByText("1/2 marked")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /Finish \(1\)/ })).toBeInTheDocument();

  // Case 202 has never run: it must arrive with NOTHING selected - a blank
  // slate is information too.
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  await screen.findByText("Invalid login");
  expect(screen.getByRole("button", { name: "Failed" })).not.toHaveClass("bg-danger");
  expect(screen.getByRole("button", { name: "Passed" })).not.toHaveClass("bg-success");
});

/// The pre-selection is a starting point, never an override: once the
/// tester marks a case, a later refetch of the points must not undo it.
test("a mark the tester makes wins over the pre-selection", async () => {
  mockIPC((cmd) => {
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
          last_outcome: "failed",
          last_run_id: 3,
          last_result_id: 30,
        },
      ];
    if (cmd === "get_result_detail") return { outcome: "failed", comment: "" };
    if (cmd === "result_screenshots") return [];
  });
  renderRunner();
  await screen.findByText("Valid login");
  await vi.waitFor(() => {
    expect(screen.getByRole("button", { name: "Failed" })).toHaveClass("bg-danger");
  });

  // The fix landed and this time it passes.
  fireEvent.click(screen.getByRole("button", { name: "Passed" }));
  expect(screen.getByRole("button", { name: "Passed" })).toHaveClass("bg-success");
  expect(screen.getByRole("button", { name: "Failed" })).not.toHaveClass("bg-danger");
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
    if (cmd === "list_test_points") return [];
  });
  renderRunner();
  expect(await screen.findByText("No linked test cases to run.")).toBeInTheDocument();
  expect(screen.queryByText("Valid login")).not.toBeInTheDocument();
});

/// The runner offers the same verdicts ADO's own runner does - including
/// Paused, for a case someone had to stop half way through. It records
/// like any other outcome.
test("Paused is offered and submits as a real outcome", async () => {
  let submitted: { outcomes?: Array<Record<string, unknown>> } = {};
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
    if (cmd === "submit_test_run") {
      submitted = args as typeof submitted;
      return { run_id: 300, web_url: "", outcomes_unrecorded: [], extras_failed: [] };
    }
  });
  renderRunner();
  await screen.findByText("Valid login");

  fireEvent.click(screen.getByRole("button", { name: "Paused" }));
  expect(screen.getByRole("button", { name: "Paused" })).toHaveClass("bg-muted");
  fireEvent.click(screen.getByRole("button", { name: /Finish \(1\)/ }));
  await vi.waitFor(() => expect(submitted.outcomes).toBeTruthy());
  expect(submitted.outcomes![0].outcome).toBe("Paused");
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
