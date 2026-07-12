import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import Suites from "./Suites";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

function renderSuites(onEditCases?: (label: string, ids: number[]) => void) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <Suites org="acme" project="Web" onEditCases={onEditCases} />
    </QueryClientProvider>,
  );
}

const PLAN = { id: 9, name: "Auth - Test Plan", area_path: "Proj\\Auth", root_suite_id: 90 };

function baseMock(handler: (cmd: string, args: unknown) => unknown) {
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    return handler(cmd, args);
  });
}

test("plans render with suites; a suite click shows its points", async () => {
  baseMock((cmd, args) => {
    if (cmd === "list_plans_with_suites")
      return [
        {
          plan: PLAN,
          suites: [
            {
              id: 91,
              name: "PBI 42 suite",
              suite_type: "requirementTestSuite",
              requirement_id: 42,
              parent_id: null,
            },
          ],
        },
      ];
    if (cmd === "list_test_points" && (args as { suiteId: number }).suiteId === 91)
      return [
        {
          point_id: 7,
          test_case_id: 201,
          test_case_name: "Valid login",
          config_name: "Windows 10",
          tester: "",
          last_outcome: "passed",
          last_run_id: null,
          last_result_id: null,
        },
      ];
  });
  renderSuites();

  expect(await screen.findByText("Auth - Test Plan")).toBeInTheDocument();
  fireEvent.click(screen.getByText("PBI 42 suite"));
  expect(await screen.findByText("Valid login")).toBeInTheDocument();
  expect(screen.getByText("passed")).toBeInTheDocument();
});

test("folders build a collapsible tree from parent links", async () => {
  baseMock((cmd) => {
    if (cmd === "list_plans_with_suites")
      return [
        {
          plan: PLAN,
          suites: [
            { id: 95, name: "Regression", suite_type: "staticTestSuite", requirement_id: null, parent_id: null },
            {
              id: 96,
              name: "PBI 50 suite",
              suite_type: "requirementTestSuite",
              requirement_id: 50,
              parent_id: 95,
            },
          ],
        },
      ];
  });
  renderSuites();

  expect(await screen.findByText("Regression")).toBeInTheDocument();
  // Folders start collapsed; clicking expands, clicking again re-collapses.
  expect(screen.queryByText("PBI 50 suite")).not.toBeInTheDocument();
  fireEvent.click(screen.getByText("Regression"));
  expect(screen.getByText("PBI 50 suite")).toBeInTheDocument();
  fireEvent.click(screen.getByText("Regression"));
  expect(screen.queryByText("PBI 50 suite")).not.toBeInTheDocument();
});

test("folder Edit cases collects descendant case ids and hands off", async () => {
  const onEdit = vi.fn();
  baseMock((cmd, args) => {
    if (cmd === "list_plans_with_suites")
      return [
        {
          plan: PLAN,
          suites: [
            { id: 95, name: "Regression", suite_type: "staticTestSuite", requirement_id: null, parent_id: null },
            {
              id: 96,
              name: "PBI 50 suite",
              suite_type: "requirementTestSuite",
              requirement_id: 50,
              parent_id: 95,
            },
          ],
        },
      ];
    if (cmd === "list_test_points") {
      const sid = (args as { suiteId: number }).suiteId;
      if (sid === 95) return [];
      if (sid === 96)
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
      return [];
    }
  });
  renderSuites(onEdit);

  await screen.findByText("Regression");
  const editChips = screen.getAllByText("Edit cases");
  fireEvent.click(editChips[0]); // the folder's chip (folder row renders first)
  await vi.waitFor(() => expect(onEdit).toHaveBeenCalledWith("Regression", [201]));
});

test("search filters the tree and auto-expands matching branches", async () => {
  baseMock((cmd) => {
    if (cmd === "list_plans_with_suites")
      return [
        {
          plan: PLAN,
          suites: [
            { id: 95, name: "Regression", suite_type: "staticTestSuite", requirement_id: null, parent_id: null },
            {
              id: 96,
              name: "PBI 50 suite",
              suite_type: "requirementTestSuite",
              requirement_id: 50,
              parent_id: 95,
            },
            {
              id: 97,
              name: "Smoke pack",
              suite_type: "staticTestSuite",
              requirement_id: null,
              parent_id: null,
            },
          ],
        },
      ];
  });
  renderSuites();
  await screen.findByText("Regression");

  // A nested match keeps its ancestor folder, expanded, and hides the rest.
  fireEvent.change(screen.getByLabelText("Search suites"), { target: { value: "PBI 50" } });
  expect(screen.getByText("PBI 50 suite")).toBeInTheDocument();
  expect(screen.getByText("Regression")).toBeInTheDocument();
  expect(screen.queryByText("Smoke pack")).not.toBeInTheDocument();

  fireEvent.change(screen.getByLabelText("Search suites"), { target: { value: "zzz" } });
  expect(await screen.findByText(/Nothing matches "zzz"/)).toBeInTheDocument();
});

test("empty project shows the friendly message", async () => {
  baseMock((cmd) => {
    if (cmd === "list_plans_with_suites") return [];
  });
  renderSuites();
  expect(
    await screen.findByText(/No test plans with test suites in this project yet/),
  ).toBeInTheDocument();
});
