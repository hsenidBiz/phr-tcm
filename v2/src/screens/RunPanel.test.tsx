import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import RunPanel from "./RunPanel";

afterEach(() => {
  clearMocks();
  localStorage.clear();
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
              { outcome: "Failed", completed_date: "2026-07-12T10:00:00Z", run_id: 7 },
              { outcome: "Passed", completed_date: "2026-07-11T10:00:00Z", run_id: 6 },
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

/** Folding a group hides the highlight along with the rows, so the heading
 * has to say that something is still selected in there - the same marker
 * View Test Cases and Update Test Cases carry. */
test("a collapsed group marks that it still holds selected cases", async () => {
  mockAll();
  renderPanel();
  await screen.findByText("Valid login");
  fireEvent.click(screen.getByLabelText(/Group by title/i) ?? screen.getByText("Group by title"));

  // Expanded and unselected: nothing to announce.
  expect(screen.queryByRole("status")).not.toBeInTheDocument();

  // Select one case, still expanded: the highlighted row itself is the
  // indicator, so the heading stays quiet.
  fireEvent.click(screen.getByText("Valid login"));
  expect(screen.queryByRole("status")).not.toBeInTheDocument();

  // Collapse the group: the rows and their highlight vanish, the dot
  // takes over.
  fireEvent.click(screen.getByLabelText(/Collapse group/));
  const dot = screen.getByRole("status");
  expect(dot.getAttribute("aria-label")).toMatch(/1 of 2 selected/);

  // Clearing the selection retires the marker while still collapsed.
  fireEvent.click(screen.getByRole("button", { name: /\(2\)/ }));
  fireEvent.click(screen.getByRole("button", { name: /\(2\)/ }));
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
});

/** Same model as View Test Cases: previews are plural, an open one
 * survives its group being collapsed, and the sticky Close all clears
 * the lot. */
test("open previews survive a group collapse until Close all", async () => {
  mockAll();
  renderPanel();
  await screen.findByText("Valid login");
  fireEvent.click(screen.getByText("Group by title"));

  expect(screen.queryByRole("button", { name: /Close all/ })).not.toBeInTheDocument();

  // Open both previews at once.
  fireEvent.click(screen.getAllByLabelText("Expand test case")[0]);
  fireEvent.click(screen.getAllByLabelText("Expand test case")[0]);
  expect(screen.getByRole("button", { name: /Close all \(2\)/ })).toBeInTheDocument();

  // Collapse the group: both held-open rows stay on screen.
  fireEvent.click(screen.getByLabelText(/Collapse group/));
  expect(screen.getByText("Valid login")).toBeInTheDocument();
  expect(screen.getByText("Invalid login")).toBeInTheDocument();

  // Close all folds the group completely and the button retires.
  fireEvent.click(screen.getByRole("button", { name: /Close all \(2\)/ }));
  expect(screen.queryByText("Valid login")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /Close all/ })).not.toBeInTheDocument();
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
  localStorage.setItem(
    "tcm-v2-suite:acme/42",
    JSON.stringify({ plan_id: 9, plan_name: "Cached Plan", suite_id: 91 }),
  );
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
