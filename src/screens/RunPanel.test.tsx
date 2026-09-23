import { emit } from "@tauri-apps/api/event";
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { toast } from "sonner";
import { afterEach, expect, test, vi } from "vitest";
import { MY_ORDER_EVENT } from "../lib/runOrder";
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
  mockAll();
  renderPanel();
  await screen.findByText("Valid login");
  fireEvent.click(screen.getByLabelText(/Group by title/i) ?? screen.getByText("Group by title"));

  // Select one case: partial reads as the checkbox's mixed state.
  fireEvent.click(screen.getByText("Valid login"));
  const box = () => screen.getByRole("checkbox", { name: /Select all in/ });
  expect(box()).toHaveAttribute("aria-checked", "mixed");

  // Collapse the group: rows and highlight vanish, the checkbox stays
  // mixed - and no dot marker appears.
  fireEvent.click(screen.getByLabelText(/Collapse group/));
  expect(box()).toHaveAttribute("aria-checked", "mixed");
  expect(screen.queryByRole("status")).not.toBeInTheDocument();

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
  mockAll();
  renderPanel();
  await screen.findByText("Valid login");
  fireEvent.click(screen.getByText("Group by title"));

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
  mockAll();
  renderPanel();
  await screen.findByText("Valid login");

  fireEvent.click(screen.getByText("Group by title"));
  expect(screen.queryByRole("button", { name: "Select all" })).not.toBeInTheDocument();

  const filter = screen.getByPlaceholderText(/Filter/i);
  fireEvent.keyDown(filter, { key: "a", ctrlKey: true });
  expect(screen.queryByRole("button", { name: /Run \d+ in runner/ })).not.toBeInTheDocument();
});

// ---- Run order (design doc §5.1) ----

type OrderMock = {
  points: Array<{ point_id: number; test_case_id: number; name: string; config?: string }>;
  entries?: number[];
  runOrder?: unknown;
  /** get_run_order fails with this AdoError instead of answering. */
  runOrderError?: unknown;
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
function mockOrder({ points, entries, runOrder, runOrderError, calls }: OrderMock) {
  mockIPC((cmd) => {
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
        return runOrder ?? { state: "none" };
    }
  }, { shouldMockEvents: true });
}

const ABC = [
  { point_id: 1, test_case_id: 301, name: "Alpha check" },
  { point_id: 2, test_case_id: 302, name: "Bravo check" },
  { point_id: 3, test_case_id: 303, name: "Charlie check" },
];

/** The rows' titles top to bottom, read off each row's Move up button. */
const rowNames = () =>
  screen
    .getAllByRole("button", { name: /^Move .* up$/ })
    .map((b) => b.getAttribute("aria-label")!)
    .filter((l) => !l.startsWith("Move group "))
    .map((l) => l.replace(/^Move /, "").replace(/ up$/, ""));

const orderPicker = () => screen.getByRole("combobox", { name: "Order" });

test("opens in the suggested run order when the PBI has one, not the points' order", async () => {
  mockOrder({ points: ABC, runOrder: RUN_ORDER_FILE([{ id: 302 }, { id: 303 }, { id: 301 }]) });
  renderPanel();
  await screen.findByText("Alpha check");
  await vi.waitFor(() => expect(orderPicker()).toHaveTextContent("Suggested run order"));
  await vi.waitFor(() => expect(rowNames()).toEqual(["Bravo check", "Charlie check", "Alpha check"]));
});

test("with no suggested order the list follows the suite's spec order", async () => {
  mockOrder({ points: ABC, entries: [303, 301, 302] });
  renderPanel();
  await screen.findByText("Alpha check");
  await vi.waitFor(() => expect(rowNames()).toEqual(["Charlie check", "Alpha check", "Bravo check"]));
  expect(orderPicker()).toHaveTextContent("Spec order");
  fireEvent.click(orderPicker());
  expect(screen.queryByRole("option", { name: "Suggested run order" })).not.toBeInTheDocument();
  expect(screen.queryByRole("option", { name: "My order" })).not.toBeInTheDocument();
});

test("an unreadable run-order file says why and greys out Suggested", async () => {
  mockOrder({ points: ABC, runOrder: { state: "unreadable", reason: "the run-order file is damaged" } });
  renderPanel();
  expect(
    await screen.findByText(
      "The suggested run order could not be read: the run-order file is damaged. See Settings → Logs.",
    ),
  ).toBeInTheDocument();
  expect(orderPicker()).toHaveTextContent("Spec order");
  fireEvent.click(orderPicker());
  const suggested = screen.getByRole("option", { name: "Suggested run order" });
  expect(suggested).toBeDisabled();
  fireEvent.click(suggested);
  expect(orderPicker()).toHaveTextContent("Spec order");
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

test("a failed read of the run order shows the note and greys out Suggested", async () => {
  mockOrder({ points: ABC, runOrderError: { kind: "Forbidden" } });
  renderPanel();
  expect(
    await screen.findByText(
      "The suggested run order could not be read: You don't have permission for this resource. See Settings → Logs.",
    ),
  ).toBeInTheDocument();
  expect(orderPicker()).toHaveTextContent("Spec order");
  fireEvent.click(orderPicker());
  expect(screen.getByRole("option", { name: "Suggested run order" })).toBeDisabled();
});

test("moving a row copies the order into My order on this machine and never writes a shared order", async () => {
  const calls: string[] = [];
  mockOrder({ points: ABC, runOrder: RUN_ORDER_FILE([{ id: 302 }, { id: 303 }, { id: 301 }]), calls });
  renderPanel();
  await vi.waitFor(() => expect(rowNames()).toEqual(["Bravo check", "Charlie check", "Alpha check"]));

  fireEvent.click(screen.getByRole("button", { name: "Move Bravo check down" }));

  expect(rowNames()).toEqual(["Charlie check", "Bravo check", "Alpha check"]);
  expect(JSON.parse(localStorage.getItem("tcm-v2-run-order:acme/9/91") as string)).toEqual([303, 302, 301]);
  expect(orderPicker()).toHaveTextContent("My order");
  expect(toast.info).toHaveBeenCalledWith("Now using your own order, on this machine.");
  expect(toast.info).toHaveBeenCalledTimes(1);

  // A second move while already in My order is not another switch.
  fireEvent.click(screen.getByRole("button", { name: "Move Alpha check up" }));
  expect(rowNames()).toEqual(["Charlie check", "Alpha check", "Bravo check"]);
  expect(toast.info).toHaveBeenCalledTimes(1);

  expect(calls).not.toContain("reorder_suite_cases");
  expect(calls).not.toContain("save_run_order");
});

test("Reset returns to the suggested order and forgets My order", async () => {
  mockOrder({ points: ABC, runOrder: RUN_ORDER_FILE([{ id: 302 }, { id: 303 }, { id: 301 }]) });
  renderPanel();
  await vi.waitFor(() => expect(rowNames()).toEqual(["Bravo check", "Charlie check", "Alpha check"]));
  fireEvent.click(screen.getByRole("button", { name: "Move Bravo check down" }));
  expect(orderPicker()).toHaveTextContent("My order");

  fireEvent.click(screen.getByRole("button", { name: /Reset to suggested order/ }));

  expect(orderPicker()).toHaveTextContent("Suggested run order");
  expect(rowNames()).toEqual(["Bravo check", "Charlie check", "Alpha check"]);
  expect(localStorage.getItem("tcm-v2-run-order:acme/9/91")).toBeNull();
  expect(screen.queryByRole("button", { name: /Reset to/ })).not.toBeInTheDocument();
});

test("with no suggested order, Reset is to spec order", async () => {
  mockOrder({ points: ABC });
  renderPanel();
  await vi.waitFor(() => expect(rowNames()).toEqual(["Alpha check", "Bravo check", "Charlie check"]));
  fireEvent.click(screen.getByRole("button", { name: "Move Alpha check down" }));
  fireEvent.click(screen.getByRole("button", { name: /Reset to spec order/ }));
  expect(orderPicker()).toHaveTextContent("Spec order");
  expect(rowNames()).toEqual(["Alpha check", "Bravo check", "Charlie check"]);
});

test("grouping follows the suggested file's groups, and a group moves as one block", async () => {
  mockOrder({
    points: ABC,
    runOrder: RUN_ORDER_FILE([
      { id: 301, group: "Web\\Sign in" },
      { id: 302, group: "Web\\Checkout" },
      { id: 303, group: "Web\\Sign in" },
    ]),
  });
  renderPanel();
  await vi.waitFor(() => expect(orderPicker()).toHaveTextContent("Suggested run order"));
  fireEvent.click(screen.getByText("Group by title"));

  expect(screen.getByText("Web\\Sign in (2)")).toBeInTheDocument();
  expect(screen.getByText("Web\\Checkout (1)")).toBeInTheDocument();
  expect(rowNames()).toEqual(["Alpha check", "Charlie check", "Bravo check"]);

  fireEvent.click(screen.getByRole("button", { name: "Move group Web\\Sign in down" }));

  expect(rowNames()).toEqual(["Bravo check", "Alpha check", "Charlie check"]);
  expect(JSON.parse(localStorage.getItem("tcm-v2-run-order:acme/9/91") as string)).toEqual([302, 301, 303]);
  expect(orderPicker()).toHaveTextContent("My order");
  expect(toast.info).toHaveBeenCalledWith("Now using your own order, on this machine.");
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

const rowOf = (title: string) => screen.getByText(title).closest("tr")!;
const MY_KEY = "tcm-v2-run-order:acme/9/91";

test("grouped, a drag into another group is not applied; within a group it is", async () => {
  mockOrder({
    points: ABC,
    runOrder: RUN_ORDER_FILE([
      { id: 301, group: "Web\\Sign in" },
      { id: 302, group: "Web\\Checkout" },
      { id: 303, group: "Web\\Sign in" },
    ]),
  });
  renderPanel();
  await vi.waitFor(() => expect(orderPicker()).toHaveTextContent("Suggested run order"));
  fireEvent.click(screen.getByText("Group by title"));
  expect(rowNames()).toEqual(["Alpha check", "Charlie check", "Bravo check"]);

  // Alpha (Sign in) dropped on Bravo (Checkout): nothing happens at all.
  fireEvent.dragStart(rowOf("Alpha check"));
  fireEvent.dragOver(rowOf("Bravo check"));
  fireEvent.drop(rowOf("Bravo check"));
  expect(rowNames()).toEqual(["Alpha check", "Charlie check", "Bravo check"]);
  expect(localStorage.getItem(MY_KEY)).toBeNull();
  expect(orderPicker()).toHaveTextContent("Suggested run order");
  expect(toast.info).not.toHaveBeenCalled();

  // Charlie dropped on Alpha, both in Sign in: applied, into My order.
  fireEvent.dragStart(rowOf("Charlie check"));
  fireEvent.dragOver(rowOf("Alpha check"));
  fireEvent.drop(rowOf("Alpha check"));
  expect(rowNames()).toEqual(["Charlie check", "Alpha check", "Bravo check"]);
  expect(JSON.parse(localStorage.getItem(MY_KEY) as string)).toEqual([303, 301, 302]);
  expect(orderPicker()).toHaveTextContent("My order");
});

test("with storage unavailable a move says so and stays on the order it was on", async () => {
  mockOrder({ points: ABC, runOrder: RUN_ORDER_FILE([{ id: 302 }, { id: 303 }, { id: 301 }]) });
  renderPanel();
  await vi.waitFor(() => expect(rowNames()).toEqual(["Bravo check", "Charlie check", "Alpha check"]));

  const setItem = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("QuotaExceededError");
  });
  try {
    fireEvent.click(screen.getByRole("button", { name: "Move Bravo check down" }));
  } finally {
    setItem.mockRestore();
  }

  expect(toast.error).toHaveBeenCalledWith("Your own order could not be saved on this machine.");
  expect(toast.info).not.toHaveBeenCalled();
  expect(orderPicker()).toHaveTextContent("Suggested run order");
  expect(rowNames()).toEqual(["Bravo check", "Charlie check", "Alpha check"]);
});

test("the list follows My order saved by the runner for this suite, and ignores another suite's", async () => {
  localStorage.setItem(MY_KEY, JSON.stringify([301, 302, 303]));
  localStorage.setItem("tcm-v2-run-order-view:acme/9/91", "mine");
  mockOrder({ points: ABC });
  renderPanel();
  await vi.waitFor(() => expect(rowNames()).toEqual(["Alpha check", "Bravo check", "Charlie check"]));
  expect(orderPicker()).toHaveTextContent("My order");

  // The runner writes a new My order; the event names another suite first.
  localStorage.setItem(MY_KEY, JSON.stringify([303, 302, 301]));
  await emit(MY_ORDER_EVENT, { org: "acme", planId: 9, suiteId: 92 });
  await new Promise((r) => setTimeout(r, 30));
  expect(rowNames()).toEqual(["Alpha check", "Bravo check", "Charlie check"]);

  await emit(MY_ORDER_EVENT, { org: "acme", planId: 9, suiteId: 91 });
  await vi.waitFor(() => expect(rowNames()).toEqual(["Charlie check", "Bravo check", "Alpha check"]));
});

test("the chosen order is remembered for the suite across a remount", async () => {
  mockOrder({ points: ABC, runOrder: RUN_ORDER_FILE([{ id: 302 }, { id: 303 }, { id: 301 }]) });
  const first = renderPanel();
  await vi.waitFor(() => expect(orderPicker()).toHaveTextContent("Suggested run order"));
  fireEvent.click(orderPicker());
  fireEvent.click(screen.getByRole("option", { name: "Spec order" }));
  await vi.waitFor(() => expect(rowNames()).toEqual(["Alpha check", "Bravo check", "Charlie check"]));
  first.unmount();

  renderPanel();
  await screen.findByText("Alpha check");
  expect(orderPicker()).toHaveTextContent("Spec order");
  await vi.waitFor(() => expect(rowNames()).toEqual(["Alpha check", "Bravo check", "Charlie check"]));
});

test("a move under a text filter moves within the full order, hidden rows included", async () => {
  mockOrder({ points: ABC });
  renderPanel();
  await vi.waitFor(() => expect(rowNames()).toEqual(["Alpha check", "Bravo check", "Charlie check"]));

  // "ha" matches Alpha and Charlie; Bravo is hidden between them.
  fireEvent.change(screen.getByLabelText("Filter points"), { target: { value: "ha" } });
  expect(rowNames()).toEqual(["Alpha check", "Charlie check"]);

  // One step up in the FULL order puts Charlie above the hidden Bravo,
  // still below Alpha - so the filtered view looks the same.
  fireEvent.click(screen.getByRole("button", { name: "Move Charlie check up" }));
  expect(rowNames()).toEqual(["Alpha check", "Charlie check"]);
  expect(JSON.parse(localStorage.getItem(MY_KEY) as string)).toEqual([301, 303, 302]);

  fireEvent.change(screen.getByLabelText("Filter points"), { target: { value: "" } });
  expect(rowNames()).toEqual(["Alpha check", "Charlie check", "Bravo check"]);
});
