import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { toast } from "sonner";
import { afterEach, expect, test, vi } from "vitest";
import { writeSuiteSeed } from "../lib/suiteSeed";
import RunPanel from "./RunPanel";

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.clearAllMocks();
});

function renderPanel() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <RunPanel org="acme" project="Web" pbiId={42} pbiTitle="Login flow" />
    </QueryClientProvider>,
  );
}

function mockAll() {
  mockIPC((cmd) => {
    switch (cmd) {
      case "plugin:event|listen":
        return 1;
      case "plugin:event|unlisten":
        return null;
      case "run_history":
        return [
          {
            test_case_id: 201,
            outcomes: [
              { outcome: "Failed", completed_date: "2026-07-12T10:00:00Z", run_id: 7, result_id: 70 },
              { outcome: "Passed", completed_date: "2026-07-11T10:00:00Z", run_id: 6, result_id: 60 },
            ],
          },
        ];
      case "ensure_pbi_suite":
        return { plan_id: 9, plan_name: "Auth - Test Plan", suite_id: 91 };
      case "list_test_points":
        return [
          {
            point_id: 7,
            test_case_id: 201,
            test_case_name: "Valid login",
            config_name: "Windows 10",
            tester: "",
            last_outcome: "failed",
            last_run_id: 3,
            last_result_id: 30,
          },
          {
            point_id: 8,
            test_case_id: 202,
            test_case_name: "Invalid login",
            config_name: "Windows 10",
            tester: "",
            last_outcome: "",
            last_run_id: null,
            last_result_id: null,
          },
        ];
    }
  });
}

test("loads suite + points as a read-only overview (outcomes live in the runner)", async () => {
  mockAll();
  renderPanel();

  expect(await screen.findByText(/Auth - Test Plan/)).toBeInTheDocument();
  expect(await screen.findByText("Valid login")).toBeInTheDocument();
  // Last-outcome cell shows the capitalized display value ("failed" from
  // ADO renders as "Failed"); the outcome filter options also say
  // "Failed", so assert specifically on a table cell.
  expect(screen.getAllByText("Failed").some((el) => el.tagName === "TD")).toBe(true);

  // History dots render for case 201 (newest first, tooltip carries date).
  expect(screen.getByTitle("Failed · 2026-07-12 (run #7)")).toBeInTheDocument();
  expect(screen.getByTitle("Passed · 2026-07-11 (run #6)")).toBeInTheDocument();

  // The quick-record flow is gone: no per-row outcome select, no comment
  // box, no Record button - the runner is the only way to set outcomes.
  expect(screen.queryByLabelText("Outcome for Valid login")).not.toBeInTheDocument();
  expect(screen.queryByPlaceholderText("Optional comment")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /Record .* outcome/ })).not.toBeInTheDocument();
});

test("expanding a row shows the case steps and the last failure detail", async () => {
  mockIPC((cmd, args) => {
    switch (cmd) {
      case "plugin:event|listen":
        return 1;
      case "plugin:event|unlisten":
        return null;
      case "run_history":
        return [];
      case "ensure_pbi_suite":
        return { plan_id: 9, plan_name: "Auth - Test Plan", suite_id: 91 };
      case "list_test_points":
        return [
          {
            point_id: 7,
            test_case_id: 201,
            test_case_name: "Valid login",
            config_name: "Windows 10",
            tester: "",
            last_outcome: "failed",
            last_run_id: 3,
            last_result_id: 30,
          },
        ];
      case "test_cases_by_ids": {
        const a = args as { ids: number[] };
        expect(a.ids).toEqual([201]);
        return [
          {
            id: 201,
            title: "Valid login",
            tags: "",
            automation_status: "Not Automated",
            steps: [{ action: "Open login page", expected: "Form shown" }],
            step_ids: ["2"],
            module_value: "",
            preconditions: "",
          },
        ];
      }
      case "result_failure_detail":
        return { comment: "Timed out waiting for redirect", bug_ids: [900] };
    }
  });
  renderPanel();

  await screen.findByText("Valid login");
  fireEvent.click(screen.getByLabelText("Expand test case"));

  // Steps and the last result comment + bug both appear.
  expect(await screen.findByText("Open login page")).toBeInTheDocument();
  expect(screen.getByText("Form shown")).toBeInTheDocument();
  expect(screen.getByText("Timed out waiting for redirect")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "#900" })).toBeInTheDocument();

  // Collapsing hides it again.
  fireEvent.click(screen.getByLabelText("Collapse test case"));
  expect(screen.queryByText("Open login page")).not.toBeInTheDocument();
});

/// "Similar to what we did for pipeline": the dots summarize, and a
/// button inside the expansion opens the runs behind them - each prior
/// result with its own comment, fetched only when the panel is opened.
test("an expanded row can open the execution history behind the dots", async () => {
  mockIPC((cmd, args) => {
    switch (cmd) {
      case "plugin:event|listen":
        return 1;
      case "plugin:event|unlisten":
        return null;
      case "run_history":
        return [
          {
            test_case_id: 201,
            outcomes: [
              { outcome: "Failed", completed_date: "2026-07-12T10:00:00Z", run_id: 7, result_id: 70, run_by: "Avin Alwis" },
              { outcome: "Passed", completed_date: "2026-07-11T10:00:00Z", run_id: 6, result_id: 60, run_by: "Buddhima Kaushalya" },
              { outcome: "Blocked", completed_date: "2026-07-10T10:00:00Z", run_id: 5, result_id: 50, run_by: "" },
            ],
          },
        ];
      case "result_screenshots": {
        const a = args as { runId: number; resultId: number };
        // Run 6's result carries one screenshot; the others carry none.
        return a.runId === 6 && a.resultId === 60 ? ["aGVsbG8="] : [];
      }
      case "ensure_pbi_suite":
        return { plan_id: 9, plan_name: "Auth - Test Plan", suite_id: 91 };
      case "list_test_points":
        return [
          {
            point_id: 7,
            test_case_id: 201,
            test_case_name: "Valid login",
            config_name: "Windows 10",
            tester: "",
            last_outcome: "failed",
            last_run_id: 7,
            last_result_id: 70,
          },
        ];
      case "test_cases_by_ids":
        return [
          {
            id: 201,
            title: "Valid login",
            tags: "",
            automation_status: "Not Automated",
            steps: [{ action: "Open login page", expected: "Form shown" }],
            step_ids: ["2"],
            module_value: "",
            preconditions: "",
          },
        ];
      case "result_failure_detail": {
        const a = args as { runId: number; resultId: number };
        // The latest result's detail is the danger box; the PRIOR ones are
        // what the history panel fetches - each by its own result id.
        if (a.runId === 6 && a.resultId === 60)
          return { comment: "Passed after the hotfix", bug_ids: [] };
        if (a.runId === 5 && a.resultId === 50) return { comment: "", bug_ids: [] };
        return { comment: "Timed out waiting for redirect", bug_ids: [] };
      }
    }
  });
  renderPanel();

  await screen.findByText("Valid login");
  fireEvent.click(screen.getByLabelText("Expand test case"));
  await screen.findByText("Open login page");

  // Two results predate the current one; the newest is NOT in the list.
  const toggle = screen.getByRole("button", { name: /Execution history \(2 earlier results\)/ });
  fireEvent.click(toggle);
  expect(await screen.findByText("Passed after the hotfix")).toBeInTheDocument();
  // Each entry carries the same details ADO's own execution history shows:
  // outcome, date, run, and who ran it - "by" omitted when unknown.
  expect(screen.getByText(/2026-07-11 · run #6 · by Buddhima Kaushalya/)).toBeInTheDocument();
  expect(screen.getByText(/2026-07-10 · run #5$/)).toBeInTheDocument();
  expect(await screen.findByText("No comment recorded.")).toBeInTheDocument();
  // ...plus the screenshots uploaded with that result, as zoomable thumbs
  // (the fullscreen viewer holds a second copy of the same image).
  expect((await screen.findAllByAltText("Run 6 screenshot 1")).length).toBeGreaterThan(0);

  // And it folds away again.
  fireEvent.click(screen.getByRole("button", { name: /Hide execution history/ }));
  expect(screen.queryByText("Passed after the hotfix")).not.toBeInTheDocument();
});

/// A case with exactly one result has no past to show - the button
/// would only ever open an empty list, so it never renders.
test("a single-result case offers no execution history button", async () => {
  mockAll();
  renderPanel();
  await screen.findByText("Valid login");
  fireEvent.click(screen.getAllByLabelText("Expand test case")[1]); // case 202: no history at all
  expect(screen.queryByRole("button", { name: /Execution history/ })).not.toBeInTheDocument();
});

test("row clicks select cases for a targeted runner session", async () => {
  mockAll();
  renderPanel();
  await screen.findByText("Valid login");
  expect(screen.queryByRole("button", { name: /Run 1 in runner/ })).not.toBeInTheDocument();

  fireEvent.click(screen.getByText("Valid login"));
  expect(screen.getByRole("button", { name: /Run 1 in runner/ })).toBeInTheDocument();

  // Clicking again deselects.
  fireEvent.click(screen.getByText("Valid login"));
  expect(screen.queryByRole("button", { name: /Run 1 in runner/ })).not.toBeInTheDocument();
});

/// Re-testing after a fix goes filter -> select -> run, on purpose: the
/// dedicated re-run button lasted one release before it came out again.
/// One path into a selective run is easier to trust than two, so this
/// pins that the path actually works for the failure case.
test("filtering by Failed and selecting the rows starts a failures-only run", async () => {
  mockAll();
  renderPanel();
  await screen.findByText("Valid login");
  expect(screen.queryByRole("button", { name: /Re-run/ })).not.toBeInTheDocument();

  // Filter to failures: the never-run case leaves the table.
  fireEvent.click(screen.getByLabelText("Filter by last outcome"));
  fireEvent.click(screen.getByRole("option", { name: "Failed" }));
  expect(screen.queryByText("Invalid login")).not.toBeInTheDocument();

  // Select what's left and run it.
  fireEvent.click(screen.getByText("Valid login"));
  fireEvent.click(screen.getByRole("button", { name: /Run 1 in runner/ }));
  const session = JSON.parse(localStorage.getItem("tcm-v2-runner-session") as string);
  expect(session.caseIds).toEqual([201]);
  expect(session.planId).toBe(9);
});

/// The runner walks cases in the order this list shows them, whatever
/// order they were clicked in - selection is a set, the run is a sequence.
test("Run selected hands the runner the list's order, not the click order", async () => {
  mockAll();
  renderPanel();
  await screen.findByText("Valid login");

  // Click bottom row first, top row second.
  fireEvent.click(screen.getByText("Invalid login"));
  fireEvent.click(screen.getByText("Valid login"));
  fireEvent.click(screen.getByRole("button", { name: /Run 2 in runner/ }));

  const session = JSON.parse(localStorage.getItem("tcm-v2-runner-session") as string);
  expect(session.caseIds).toEqual([201, 202]);
});

/// The run-everything button is gone: the runner opens only from a
/// selection, so an idle panel offers no way to start a 50-case session
/// by mis-click. Selecting rows surfaces the (only) way in.
test("the runner opens only from a selection", async () => {
  mockAll();
  renderPanel();
  await screen.findByText("Valid login");

  expect(screen.queryByRole("button", { name: /runner/i })).not.toBeInTheDocument();

  fireEvent.click(screen.getByText("Valid login"));
  expect(screen.getByRole("button", { name: /Run 1 in runner/ })).toBeInTheDocument();
});

/** The header checkbox is the one selection indicator - dash for partial,
 * tick for the whole group - and it keeps saying so while the group is
 * collapsed. The pulsing dot that used to ride beside collapsed headings
 * is gone: it repeated what the checkbox already shows. */
test("the header checkbox carries the selection state through a collapse", async () => {
  localStorage.setItem("tcm-v2-group-mode", "title");
  mockAll();
  renderPanel();
  await screen.findByText("Valid login");

  // Select one case: partial reads as the checkbox's mixed state.
  fireEvent.click(screen.getByText("Valid login"));
  const box = () => screen.getByRole("checkbox", { name: /Select all in/ });
  expect(box()).toHaveAttribute("aria-checked", "mixed");

  // Collapse the group: rows and highlight vanish, the checkbox stays
  // mixed - and no dot marker appears.
  fireEvent.click(screen.getByLabelText(/Collapse group/));
  expect(box()).toHaveAttribute("aria-checked", "mixed");
  expect(screen.queryByRole("status", { name: /selected in/ })).not.toBeInTheDocument();

  // Complete the selection: the dash becomes a tick, still collapsed.
  fireEvent.click(box());
  expect(box()).toHaveAttribute("aria-checked", "true");

  // Clear it: empty box.
  fireEvent.click(box());
  expect(box()).toHaveAttribute("aria-checked", "false");
});

/** Same model as View Test Cases: previews are plural, an open one
 * survives its group being collapsed, and the sticky Collapse all clears
 * the lot. */
test("open previews survive a group collapse until Collapse all", async () => {
  localStorage.setItem("tcm-v2-group-mode", "title");
  mockAll();
  renderPanel();
  await screen.findByText("Valid login");

  // Grouping alone gives the sticky its first fold target: the one open
  // group, before any preview is expanded.
  expect(screen.getByRole("button", { name: /Collapse all \(1\)/ })).toBeInTheDocument();

  // Open both previews at once.
  fireEvent.click(screen.getAllByLabelText("Expand test case")[0]);
  fireEvent.click(screen.getAllByLabelText("Expand test case")[0]);
  expect(screen.getByRole("button", { name: /Collapse all \(3\)/ })).toBeInTheDocument();

  // Collapse the group: both held-open rows stay on screen.
  fireEvent.click(screen.getByLabelText(/Collapse group/));
  expect(screen.getByText("Valid login")).toBeInTheDocument();
  expect(screen.getByText("Invalid login")).toBeInTheDocument();

  // Collapse all folds the previews AND the group, then retires.
  fireEvent.click(screen.getByRole("button", { name: /Collapse all \(2\)/ }));
  expect(screen.queryByText("Valid login")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /Collapse all/ })).not.toBeInTheDocument();
});

test("shift+click selects the whole range between two rows", async () => {
  mockAll();
  renderPanel();
  await screen.findByText("Valid login");

  fireEvent.click(screen.getByText("Valid login"));
  fireEvent.click(screen.getByText("Invalid login"), { shiftKey: true });
  expect(screen.getByRole("button", { name: /Run 2 in runner/ })).toBeInTheDocument();
});

test("suite is resolved once, then every later mount reuses the seed", async () => {
  let ensured = 0;
  mockIPC((cmd) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "ensure_pbi_suite") {
      ensured++;
      return { plan_id: 9, plan_name: "Fresh Plan", suite_id: 91 };
    }
    if (cmd === "list_test_points") return [];
  });

  // First visit: no seed -> one real resolve that writes the seed.
  const first = renderPanel();
  expect(await screen.findByText(/Fresh Plan/)).toBeInTheDocument();
  expect(ensured).toBe(1);
  first.unmount();

  // Tab switch back - even with a brand-new QueryClient (no in-memory
  // cache), the queryFn short-circuits to the localStorage seed.
  renderPanel();
  expect(await screen.findByText(/Fresh Plan/)).toBeInTheDocument();
  expect(ensured).toBe(1);
});

test("suite resolution is cached in localStorage and reused", async () => {
  writeSuiteSeed("acme", 42, { plan_id: 9, plan_name: "Cached Plan", suite_id: 91 });
  let ensured = 0;
  mockIPC((cmd) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "ensure_pbi_suite") {
      ensured++;
      return { plan_id: 9, plan_name: "Fresh Plan", suite_id: 91 };
    }
    if (cmd === "list_test_points") return [];
  });
  renderPanel();
  expect(await screen.findByText(/Cached Plan/)).toBeInTheDocument();
  expect(ensured).toBe(0);
});



/** The execution report is about the cases the tester picked: dead until
 * something is highlighted, and the IPC call carries exactly those ids.
 * Self-contained handler - mockIPC keeps ONE handler, so this cannot
 * layer a capture on top of mockAll(). */
test("Execution report arms on selection and reports only the highlighted cases", async () => {
  const reported: number[][] = [];
  mockIPC((cmd, args) => {
    switch (cmd) {
      case "plugin:event|listen":
        return 1;
      case "plugin:event|unlisten":
        return null;
      case "run_history":
        return [];
      case "ensure_pbi_suite":
        return { plan_id: 9, plan_name: "Auth - Test Plan", suite_id: 91 };
      case "list_test_points":
        return [
          {
            point_id: 7,
            test_case_id: 201,
            test_case_name: "Valid login",
            config_name: "Windows 10",
            tester: "",
            last_outcome: "",
            last_run_id: null,
            last_result_id: null,
          },
        ];
      case "view_execution_report":
        reported.push((args as { caseIds: number[] }).caseIds);
        return null;
    }
  });
  renderPanel();
  await screen.findByText("Valid login");

  const btn = screen.getByRole("button", { name: /Execution report/ });
  expect(btn).toBeDisabled();

  fireEvent.click(screen.getByText("Valid login"));
  expect(btn).toBeEnabled();
  fireEvent.click(btn);
  await vi.waitFor(() => expect(reported).toHaveLength(1));
  expect(reported[0]).toEqual([201]);
});

/// Refresh means "the outcomes moved", not "find the suite again": suite
/// resolution scans every test plan in the project, so the button must
/// refetch points without a second ensure_pbi_suite. Shift-click is the
/// deliberate full re-detect.
test("Refresh refetches points without re-resolving the suite; Shift-click re-detects", async () => {
  const counts = { ensure: 0, points: 0 };
  mockIPC((cmd) => {
    switch (cmd) {
      case "plugin:event|listen":
        return 1;
      case "plugin:event|unlisten":
        return null;
      case "run_history":
        return [];
      case "ensure_pbi_suite":
        counts.ensure += 1;
        return { plan_id: 9, plan_name: "Auth - Test Plan", suite_id: 91 };
      case "list_test_points":
        counts.points += 1;
        return [
          {
            point_id: 7,
            test_case_id: 201,
            test_case_name: "Valid login",
            config_name: "Windows 10",
            tester: "",
            last_outcome: "",
            last_run_id: null,
            last_result_id: null,
          },
        ];
    }
  });
  renderPanel();
  await screen.findByText("Valid login");
  expect(counts.ensure).toBe(1);
  const pointsBefore = counts.points;

  fireEvent.click(screen.getByRole("button", { name: "Refresh outcomes" }));
  await vi.waitFor(() => expect(counts.points).toBeGreaterThan(pointsBefore));
  expect(counts.ensure).toBe(1); // no re-scan on a plain refresh

  fireEvent.click(screen.getByRole("button", { name: "Refresh outcomes" }), { shiftKey: true });
  await vi.waitFor(() => expect(counts.ensure).toBe(2));
});

/// Bulk selection without a mouse marathon: ungrouped mode gets a Select
/// all button (grouped mode already has the per-group checkboxes), and
/// Ctrl+A selects every visible case - except while typing in a field,
/// where select-all must keep meaning the text.
test("Select all button (ungrouped) and Ctrl+A both select every case", async () => {
  mockAll();
  renderPanel();
  await screen.findByText("Valid login");

  fireEvent.click(screen.getByRole("button", { name: "Select all" }));
  expect(screen.getByRole("button", { name: /Run 2 in runner/ })).toBeInTheDocument();

  fireEvent.click(screen.getByLabelText("Clear selection"));
  expect(screen.queryByRole("button", { name: /Run \d+ in runner/ })).not.toBeInTheDocument();

  fireEvent.keyDown(window, { key: "a", ctrlKey: true });
  expect(screen.getByRole("button", { name: /Run 2 in runner/ })).toBeInTheDocument();
});

test("grouped mode hides the Select all button and Ctrl+A skips text fields", async () => {
  localStorage.setItem("tcm-v2-group-mode", "title");
  mockAll();
  renderPanel();
  await screen.findByText("Valid login");

  expect(screen.queryByRole("button", { name: "Select all" })).not.toBeInTheDocument();

  const filter = screen.getByPlaceholderText(/Filter/i);
  fireEvent.keyDown(filter, { key: "a", ctrlKey: true });
  expect(screen.queryByRole("button", { name: /Run \d+ in runner/ })).not.toBeInTheDocument();
});

// ---- Run order (design doc §5.1, as amended by the execution-order-modal design) ----

type OrderMock = {
  points: Array<{ point_id: number; test_case_id: number; name: string; config?: string }>;
  entries?: number[];
  /** What get_run_order answers. A function is asked on every read, so a
   * save can change what the next read sees, as the real backend does. */
  runOrder?: unknown;
  /** get_run_order fails with this AdoError instead of answering. */
  runOrderError?: unknown;
  /** Answers save_run_order, given its arguments. */
  onSave?: (args: unknown) => unknown;
  calls?: string[];
};

const RUN_ORDER_FILE = (cases: Array<{ id: number; group?: string }>) => ({
  state: "found",
  file: {
    format: "tcm-run-order",
    version: 1,
    saved_by: "lead@example.com",
    saved_at: "2026-09-23T10:15:00Z",
    cases,
  },
});

// Events are mocked for real here (shouldMockEvents), so an emit reaches
// the screen's listeners the way another window's save would.
function mockOrder({ points, entries, runOrder, runOrderError, onSave, calls }: OrderMock) {
  mockIPC((cmd, args) => {
    calls?.push(cmd);
    switch (cmd) {
      case "run_history":
        return [];
      case "ensure_pbi_suite":
        return { plan_id: 9, plan_name: "Auth - Test Plan", suite_id: 91 };
      case "list_test_points":
        return points.map((p) => ({
          point_id: p.point_id,
          test_case_id: p.test_case_id,
          test_case_name: p.name,
          config_name: p.config ?? "Windows 10",
          tester: "",
          last_outcome: "",
          last_run_id: null,
          last_result_id: null,
        }));
      case "list_suite_entries":
        return (entries ?? points.map((p) => p.test_case_id)).map((id, i) => ({
          id,
          sequence_number: i + 1,
          entry_type: "testCase",
        }));
      case "get_run_order":
        if (runOrderError) return Promise.reject(runOrderError);
        return (typeof runOrder === "function" ? runOrder() : runOrder) ?? { state: "none" };
      case "save_run_order":
        return onSave?.(args);
    }
  }, { shouldMockEvents: true });
}

const ABC = [
  { point_id: 1, test_case_id: 301, name: "Alpha check" },
  { point_id: 2, test_case_id: 302, name: "Bravo check" },
  { point_id: 3, test_case_id: 303, name: "Charlie check" },
];

/** The rows' titles top to bottom, as the table shows them. A fold's
 * closing copy (lib/exitGhost, e.g. after the modal's grouping regroups
 * the list) is a picture, not rows, so it is skipped. */
const rowNames = () =>
  Array.from(document.querySelectorAll("tbody tr .id-mono"))
    .filter((el) => !el.closest("[data-exit-ghost]"))
    .map((el) => (el.parentElement?.textContent ?? "").replace(/^#\d+\s*/, ""));

const openOrderModal = () => fireEvent.click(screen.getByRole("button", { name: "Set execution order" }));
const startFrom = () => screen.getByRole("combobox", { name: "Start from" });
const groupCases = () => screen.getByRole("combobox", { name: "Group cases" });
const modalList = () => screen.getByRole("list", { name: "Execution order" });
const rowOf = (title: string) => screen.getByText(title).closest("tr")!;
const MY_KEY = "tcm-v2-run-order:acme/9/91";
const VIEW_KEY = "tcm-v2-run-order-view:acme/9/91";
const CONFIRM = "Every tester will see this as the suggested run order for this PBI.";

/** Remembers a watched draft for PBI 42 the way the Import tab does. */
function watchDraft(path: string, cases: Array<{ id: number | null; order: number | null; area?: string }>) {
  const snapshot = cases.map((c, i) => ({
    title: `Case ${i}`,
    steps: [],
    tags: "",
    automation_status: "Not Automated",
    module_value: "",
    preconditions: "",
    update_id: c.id,
    tester_order: c.order,
    area: c.area ?? "",
  }));
  localStorage.setItem("tcm-v2-watch:acme/42", JSON.stringify([{ path, stamp: "s1", snapshot }]));
}

test("opens in the suggested run order when the PBI has one, not the points' order", async () => {
  mockOrder({ points: ABC, runOrder: RUN_ORDER_FILE([{ id: 302 }, { id: 303 }, { id: 301 }]) });
  renderPanel();
  await screen.findByText("Alpha check");
  await vi.waitFor(() => expect(rowNames()).toEqual(["Bravo check", "Charlie check", "Alpha check"]));
  openOrderModal();
  expect(startFrom()).toHaveTextContent("Suggested run order");
});

test("the Set execution order button explains itself while the run order is still loading", async () => {
  // get_run_order never resolves - the button stays disabled by
  // order.loading for as long as that read is outstanding.
  mockOrder({ points: ABC, runOrder: () => new Promise(() => {}) });
  renderPanel();
  await screen.findByText("Alpha check");

  const button = screen.getByRole("button", { name: "Set execution order" });
  expect(button).toBeDisabled();
  expect(button).toHaveAttribute("title", "Loading the run order…");
});

test("the list has no order controls of its own; one button opens the modal", async () => {
  mockOrder({ points: ABC, runOrder: RUN_ORDER_FILE([{ id: 302 }, { id: 303 }, { id: 301 }]) });
  renderPanel();
  await vi.waitFor(() => expect(rowNames()).toEqual(["Bravo check", "Charlie check", "Alpha check"]));

  expect(screen.queryByRole("combobox", { name: "Order" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /^Move / })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /^Reset to/ })).not.toBeInTheDocument();
  expect(rowOf("Alpha check")).not.toHaveAttribute("draggable");

  openOrderModal();
  expect(screen.getByRole("heading", { name: "Execution order" })).toBeInTheDocument();
  expect(startFrom()).toHaveTextContent("Suggested run order");
});

test("with no suggested order the list follows spec order and the modal offers only Spec order", async () => {
  mockOrder({ points: ABC, entries: [303, 301, 302] });
  renderPanel();
  await screen.findByText("Alpha check");
  await vi.waitFor(() => expect(rowNames()).toEqual(["Charlie check", "Alpha check", "Bravo check"]));

  openOrderModal();
  expect(startFrom()).toHaveTextContent("Spec order");
  expect(await within(screen.getByRole("dialog")).findByText("No suggested run order yet.")).toBeInTheDocument();
  fireEvent.click(startFrom());
  expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual(["Spec order"]);
});

/// The disk seed (design doc §4.4's reconciled order) is fresh the moment
/// the screen mounts, so a plain Refresh must still force a real re-read
/// of both the run-order file and the suite's cases - otherwise Refresh
/// looks like it worked but keeps serving what was cached before an
/// upload or a save for everyone landed.
test("Refresh outcomes also re-reads the suggested run order and the suite's cases", async () => {
  const calls: string[] = [];
  mockOrder({ points: ABC, runOrder: RUN_ORDER_FILE([{ id: 302 }, { id: 303 }, { id: 301 }]), calls });
  renderPanel();
  await vi.waitFor(() => expect(rowNames()).toEqual(["Bravo check", "Charlie check", "Alpha check"]));

  const before = {
    order: calls.filter((c) => c === "get_run_order").length,
    entries: calls.filter((c) => c === "list_suite_entries").length,
  };

  fireEvent.click(screen.getByRole("button", { name: "Refresh outcomes" }));

  await vi.waitFor(() => {
    expect(calls.filter((c) => c === "get_run_order").length).toBeGreaterThan(before.order);
    expect(calls.filter((c) => c === "list_suite_entries").length).toBeGreaterThan(before.entries);
  });
});

test("an unreadable run-order file says why, and the modal greys out Suggested", async () => {
  mockOrder({ points: ABC, runOrder: { state: "unreadable", reason: "the run-order file is damaged" } });
  renderPanel();
  const note = "The suggested run order could not be read: the run-order file is damaged. See Settings → Logs.";
  expect(await screen.findByText(note)).toBeInTheDocument();

  openOrderModal();
  expect(startFrom()).toHaveTextContent("Spec order");
  expect(within(screen.getByRole("dialog")).getByText(note)).toBeInTheDocument();
  fireEvent.click(startFrom());
  const suggested = screen.getByRole("option", { name: "Suggested run order" });
  expect(suggested).toBeDisabled();
  fireEvent.click(suggested);
  expect(startFrom()).toHaveTextContent("Spec order");
});

test("a reason that already points at the logs is not told to go there twice", async () => {
  mockOrder({
    points: ABC,
    runOrder: { state: "unreadable", reason: "the file is damaged; Settings → Logs has the details." },
  });
  renderPanel();
  expect(
    await screen.findByText(
      "The suggested run order could not be read: the file is damaged; Settings → Logs has the details.",
    ),
  ).toBeInTheDocument();
  expect(screen.queryByText(/See Settings/)).not.toBeInTheDocument();
});

test("a failed read of the run order shows the note, and the modal greys out Suggested", async () => {
  mockOrder({ points: ABC, runOrderError: { kind: "Forbidden" } });
  renderPanel();
  expect(
    await screen.findByText(
      "The suggested run order could not be read: You don't have permission for this resource. See Settings → Logs.",
    ),
  ).toBeInTheDocument();
  openOrderModal();
  expect(startFrom()).toHaveTextContent("Spec order");
  fireEvent.click(startFrom());
  expect(screen.getByRole("option", { name: "Suggested run order" })).toBeDisabled();
});

test("Start from lists My order and an uploaded draft's tester order", async () => {
  localStorage.setItem(MY_KEY, JSON.stringify([303, 301, 302]));
  watchDraft("C:/work/login.json", [
    { id: 301, order: 2 },
    { id: 303, order: 1 },
  ]);
  mockOrder({ points: ABC });
  renderPanel();
  await vi.waitFor(() => expect(rowNames()).toEqual(["Alpha check", "Bravo check", "Charlie check"]));
  openOrderModal();
  fireEvent.click(startFrom());
  expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual([
    "Spec order",
    "My order",
    "Tester order from login.json",
  ]);
});

test("Use this order on an unchanged Suggested run order sets the view only; My order is kept", async () => {
  localStorage.setItem(MY_KEY, JSON.stringify([303, 301, 302]));
  localStorage.setItem(VIEW_KEY, "spec");
  mockOrder({ points: ABC, runOrder: RUN_ORDER_FILE([{ id: 302 }, { id: 303 }, { id: 301 }]) });
  renderPanel();
  await vi.waitFor(() => expect(rowNames()).toEqual(["Alpha check", "Bravo check", "Charlie check"]));

  openOrderModal();
  expect(startFrom()).toHaveTextContent("Spec order");
  fireEvent.click(startFrom());
  fireEvent.click(screen.getByRole("option", { name: "Suggested run order" }));
  fireEvent.click(screen.getByRole("button", { name: "Use this order" }));

  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(rowNames()).toEqual(["Bravo check", "Charlie check", "Alpha check"]);
  expect(localStorage.getItem(VIEW_KEY)).toBe("suggested");
  expect(JSON.parse(localStorage.getItem(MY_KEY) as string)).toEqual([303, 301, 302]);
});

test("a reorder in the modal becomes My order on this machine and never writes a shared order", async () => {
  const calls: string[] = [];
  mockOrder({ points: ABC, runOrder: RUN_ORDER_FILE([{ id: 302 }, { id: 303 }, { id: 301 }]), calls });
  renderPanel();
  await vi.waitFor(() => expect(rowNames()).toEqual(["Bravo check", "Charlie check", "Alpha check"]));

  openOrderModal();
  fireEvent.click(within(modalList()).getByRole("button", { name: "Move #302 down" }));
  fireEvent.click(screen.getByRole("button", { name: "Use this order" }));

  expect(rowNames()).toEqual(["Charlie check", "Bravo check", "Alpha check"]);
  expect(JSON.parse(localStorage.getItem(MY_KEY) as string)).toEqual([303, 302, 301]);
  expect(localStorage.getItem(VIEW_KEY)).toBe("mine");
  expect(calls).not.toContain("reorder_suite_cases");
  expect(calls).not.toContain("save_run_order");
});

test("Use this order on a tester-order start saves it as My order", async () => {
  watchDraft("C:/work/login.json", [
    { id: 301, order: 2 },
    { id: 303, order: 1 },
  ]);
  mockOrder({ points: ABC });
  renderPanel();
  await vi.waitFor(() => expect(rowNames()).toEqual(["Alpha check", "Bravo check", "Charlie check"]));

  openOrderModal();
  fireEvent.click(startFrom());
  fireEvent.click(screen.getByRole("option", { name: "Tester order from login.json" }));
  fireEvent.click(screen.getByRole("button", { name: "Use this order" }));

  expect(JSON.parse(localStorage.getItem(MY_KEY) as string)).toEqual([303, 301, 302]);
  expect(localStorage.getItem(VIEW_KEY)).toBe("mine");
  expect(rowNames()).toEqual(["Charlie check", "Alpha check", "Bravo check"]);
});

test("Save for everyone asks first; Cancel sends nothing", async () => {
  const calls: string[] = [];
  mockOrder({ points: ABC, calls });
  renderPanel();
  await vi.waitFor(() => expect(rowNames()).toEqual(["Alpha check", "Bravo check", "Charlie check"]));

  openOrderModal();
  fireEvent.click(screen.getByRole("button", { name: "Save for everyone" }));
  expect(screen.getByText(CONFIRM)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

  expect(screen.queryByText(CONFIRM)).not.toBeInTheDocument();
  expect(screen.getByRole("dialog")).toBeInTheDocument();
  expect(calls).not.toContain("save_run_order");
});

test("Save for everyone sends the list with the saved groups, toasts, and switches the list to Suggested", async () => {
  const NEW = RUN_ORDER_FILE([{ id: 301, group: "Auth" }, { id: 303 }, { id: 302 }]);
  let current: unknown = RUN_ORDER_FILE([{ id: 301, group: "Auth" }, { id: 302 }, { id: 303 }]);
  const saved: unknown[] = [];
  localStorage.setItem(VIEW_KEY, "spec");
  mockOrder({
    points: ABC,
    runOrder: () => current,
    onSave: (args) => {
      saved.push(args);
      current = NEW;
      return NEW.file;
    },
  });
  renderPanel();
  await vi.waitFor(() => expect(rowNames()).toEqual(["Alpha check", "Bravo check", "Charlie check"]));

  openOrderModal();
  expect(startFrom()).toHaveTextContent("Spec order");
  fireEvent.click(within(modalList()).getByRole("button", { name: "Move #303 up" }));
  fireEvent.click(screen.getByRole("button", { name: "Save for everyone" }));
  fireEvent.click(screen.getByRole("button", { name: "Save" }));

  await vi.waitFor(() => expect(toast.success).toHaveBeenCalledWith("Suggested run order saved."));
  expect(saved).toEqual([
    {
      organization: "acme",
      project: "Web",
      pbiId: 42,
      cases: [{ id: 301, group: "Auth" }, { id: 303 }, { id: 302 }],
    },
  ]);
  await vi.waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  expect(localStorage.getItem(VIEW_KEY)).toBe("suggested");
  expect(rowNames()).toEqual(["Alpha check", "Charlie check", "Bravo check"]);
});

test("a failed Save for everyone keeps the modal open and says why", async () => {
  mockOrder({
    points: ABC,
    onSave: () => Promise.reject({ kind: "Network", detail: "Could not reach Azure DevOps." }),
  });
  renderPanel();
  await vi.waitFor(() => expect(rowNames()).toEqual(["Alpha check", "Bravo check", "Charlie check"]));

  openOrderModal();
  fireEvent.click(screen.getByRole("button", { name: "Save for everyone" }));
  fireEvent.click(screen.getByRole("button", { name: "Save" }));

  await vi.waitFor(() =>
    expect(toast.error).toHaveBeenCalledWith("Could not save the suggested run order: Could not reach Azure DevOps."),
  );
  expect(screen.getByRole("dialog")).toBeInTheDocument();
  expect(startFrom()).toHaveTextContent("Spec order");
});

test("choosing By area in the modal groups the list by the suggested file's areas, and groups have no move buttons", async () => {
  mockOrder({
    points: ABC,
    runOrder: RUN_ORDER_FILE([
      { id: 301, group: "Web\\Sign in" },
      { id: 302, group: "Web\\Checkout" },
      { id: 303, group: "Web\\Sign in" },
    ]),
  });
  renderPanel();
  await screen.findByText("Alpha check");

  openOrderModal();
  fireEvent.click(groupCases());
  fireEvent.click(screen.getByRole("option", { name: "By area" }));
  fireEvent.click(screen.getByRole("button", { name: "Use this order" }));

  expect(screen.getByText("Web\\Sign in (2)")).toBeInTheDocument();
  expect(screen.getByText("Web\\Checkout (1)")).toBeInTheDocument();
  expect(rowNames()).toEqual(["Alpha check", "Charlie check", "Bravo check"]);
  expect(screen.queryByRole("button", { name: /^Move group / })).not.toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-group-mode")).toBe("area");
});

test("choosing By title in the modal groups the list by title and remembers the choice", async () => {
  mockOrder({
    points: [
      { point_id: 1, test_case_id: 301, name: "Auth - Login" },
      { point_id: 2, test_case_id: 302, name: "Billing - Invoice" },
      { point_id: 3, test_case_id: 303, name: "Auth - Logout" },
    ],
  });
  renderPanel();
  await screen.findByText("Auth - Login");

  openOrderModal();
  fireEvent.click(groupCases());
  fireEvent.click(screen.getByRole("option", { name: "By title" }));
  fireEvent.click(screen.getByRole("button", { name: "Use this order" }));

  expect(screen.getByText("Auth (2)")).toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-group-mode")).toBe("title");
});

test("no Group by title checkbox and no active-order status text beside Set execution order", async () => {
  mockAll();
  renderPanel();
  await screen.findByText("Valid login");

  expect(screen.queryByText("Group by title")).not.toBeInTheDocument();
  expect(
    within(screen.getByRole("button", { name: "Set execution order" }).parentElement as HTMLElement).queryByRole(
      "status",
    ),
  ).not.toBeInTheDocument();
});

test("a case run on two configurations keeps both rows together", async () => {
  mockOrder({
    points: [
      { point_id: 1, test_case_id: 301, name: "Alpha check", config: "Windows" },
      { point_id: 2, test_case_id: 302, name: "Bravo check" },
      { point_id: 3, test_case_id: 301, name: "Alpha check", config: "Mac" },
    ],
    entries: [302, 301],
  });
  renderPanel();
  await vi.waitFor(() => expect(rowNames()).toEqual(["Bravo check", "Alpha check", "Alpha check"]));
});

test("the runner opens with the selection in the order on screen", async () => {
  mockOrder({ points: ABC, runOrder: RUN_ORDER_FILE([{ id: 303 }, { id: 301 }, { id: 302 }]) });
  renderPanel();
  await vi.waitFor(() => expect(rowNames()).toEqual(["Charlie check", "Alpha check", "Bravo check"]));

  fireEvent.click(screen.getByText("Alpha check"));
  fireEvent.click(screen.getByText("Charlie check"));
  fireEvent.click(screen.getByRole("button", { name: /Run 2 in runner/ }));

  const session = JSON.parse(localStorage.getItem("tcm-v2-runner-session") as string);
  expect(session.caseIds).toEqual([303, 301]);
});

test("the chosen order is remembered for the suite across a remount, and My order is kept", async () => {
  localStorage.setItem(MY_KEY, JSON.stringify([303, 302, 301]));
  mockOrder({ points: ABC, runOrder: RUN_ORDER_FILE([{ id: 302 }, { id: 303 }, { id: 301 }]) });
  const first = renderPanel();
  await vi.waitFor(() => expect(rowNames()).toEqual(["Bravo check", "Charlie check", "Alpha check"]));

  openOrderModal();
  fireEvent.click(startFrom());
  fireEvent.click(screen.getByRole("option", { name: "Spec order" }));
  fireEvent.click(screen.getByRole("button", { name: "Use this order" }));
  await vi.waitFor(() => expect(rowNames()).toEqual(["Alpha check", "Bravo check", "Charlie check"]));
  first.unmount();

  // No findByText on a title here: the closed modal's fading copy still
  // shows the same titles for a moment. rowNames reads the table only.
  renderPanel();
  await vi.waitFor(() => expect(rowNames()).toEqual(["Alpha check", "Bravo check", "Charlie check"]));
  expect(localStorage.getItem(VIEW_KEY)).toBe("spec");
  expect(JSON.parse(localStorage.getItem(MY_KEY) as string)).toEqual([303, 302, 301]);
});
