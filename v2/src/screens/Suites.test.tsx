import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import Suites from "./Suites";

afterEach(() => clearMocks());

function renderSuites() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <Suites org="acme" project="Web" />
    </QueryClientProvider>,
  );
}

test("plans render with suites; a suite click shows its points", async () => {
  mockIPC((cmd, args) => {
    if (cmd === "list_plans_with_suites")
      return [
        {
          plan: { id: 9, name: "Auth - Test Plan", area_path: "Proj\\Auth", root_suite_id: 90 },
          suites: [
            { id: 91, name: "PBI 42 suite", suite_type: "requirementTestSuite", requirement_id: 42 },
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

test("empty project shows the friendly message", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_plans_with_suites") return [];
  });
  renderSuites();
  expect(
    await screen.findByText(/No test plans with test suites in this project yet/),
  ).toBeInTheDocument();
});
