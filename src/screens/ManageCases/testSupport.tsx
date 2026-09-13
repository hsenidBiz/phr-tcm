import { mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import ManageCases from "./index";

export const PLANS = [
  {
    plan: { id: 9, name: "Auth - Test Plan", area_path: "Proj\\Auth", root_suite_id: 90 },
    suites: [
      { id: 91, name: "Regression", suite_type: "staticTestSuite", requirement_id: null, parent_id: null },
      { id: 93, name: "PBI 42 suite", suite_type: "requirementTestSuite", requirement_id: 42, parent_id: null },
    ],
  },
];

export const ENTRIES = [
  { id: 95, sequence_number: 0, entry_type: "suite" },
  { id: 201, sequence_number: 1, entry_type: "testCase" },
  { id: 202, sequence_number: 2, entry_type: "testCase" },
  { id: 203, sequence_number: 3, entry_type: "testCase" },
];

export const point = (id: number, name: string, config = "Windows 10") => ({
  point_id: id * 10,
  test_case_id: id,
  test_case_name: name,
  config_name: config,
  tester: "",
  last_outcome: "none",
  last_run_id: null,
  last_result_id: null,
});

/** Mount the screen over a plan with one static suite (91) and one PBI
 * suite (93), both holding cases 201, 202, 203. `extra` answers any other
 * command. Returns every IPC call for assertions. */
export function mountWithSuite(extra: (cmd: string, args: unknown) => unknown = () => undefined) {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_plans_with_suites") return PLANS;
    if (cmd === "list_suite_entries") return ENTRIES;
    if (cmd === "list_test_points")
      return [
        point(201, "Valid login"),
        point(201, "Valid login", "Windows 11"),
        point(202, "Bad password"),
        point(203, "Locked out"),
      ];
    return extra(cmd, args);
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <ManageCases org="acme" project="Web" />
    </QueryClientProvider>,
  );
  return { calls };
}

/** Pick plan 9 and the given suite; resolves to the case list element. */
export async function pickSuite(suiteId: number) {
  const plan = await screen.findByLabelText("Test plan");
  await screen.findByText("Auth - Test Plan");
  fireEvent.change(plan, { target: { value: "9" } });
  fireEvent.change(screen.getByLabelText("Test suite"), { target: { value: String(suiteId) } });
  return screen.findByRole("list", { name: "Test cases in order" });
}
