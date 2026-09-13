import { mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import type { PbiHit } from "../../bindings";
import ManageCases from "./index";

export const PLANS = [
  {
    plan: { id: 9, name: "Auth - Test Plan", area_path: "Proj\\Auth", root_suite_id: 90 },
    suites: [
      { id: 91, name: "Regression", suite_type: "staticTestSuite", requirement_id: null, parent_id: null },
      { id: 92, name: "Smoke", suite_type: "staticTestSuite", requirement_id: null, parent_id: 91 },
      { id: 93, name: "PBI 42 suite", suite_type: "requirementTestSuite", requirement_id: 42, parent_id: null },
      { id: 96, name: "PBI 55 suite", suite_type: "requirementTestSuite", requirement_id: 55, parent_id: null },
    ],
  },
  {
    plan: { id: 10, name: "Billing - Test Plan", area_path: "Proj\\Billing", root_suite_id: 100 },
    suites: [
      { id: 101, name: "Invoices", suite_type: "staticTestSuite", requirement_id: null, parent_id: null },
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

/** Mount the screen over two plans. Every suite answers with cases 201,
 * 202, 203 (the fixture does not vary per suite). `extra` answers any
 * other command. Returns every IPC call for assertions. */
export function mountScreen(
  extra: (cmd: string, args: unknown) => unknown = () => undefined,
  pbi: PbiHit | null = null,
) {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    const answered = extra(cmd, args);
    if (answered !== undefined) return answered;
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_plans_with_suites") return PLANS;
    if (cmd === "list_suite_entries") return ENTRIES;
    if (cmd === "list_test_points")
      return [point(201, "Valid login"), point(201, "Valid login", "Windows 11"), point(202, "Bad password"), point(203, "Locked out")];
    return undefined;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const { rerender: rtlRerender } = render(
    <QueryClientProvider client={qc}>
      <ManageCases org="acme" project="Web" pbi={pbi} />
    </QueryClientProvider>,
  );
  /** Re-render the same tree with a newly picked PBI (or none). */
  const rerender = (nextPbi: PbiHit | null) =>
    rtlRerender(
      <QueryClientProvider client={qc}>
        <ManageCases org="acme" project="Web" pbi={nextPbi} />
      </QueryClientProvider>,
    );
  return { calls, rerender };
}

/** Expand a suite block by name and resolve to its case list. */
export async function expandSuite(name: string) {
  fireEvent.click(await screen.findByRole("button", { name: `Expand ${name}` }));
  const l = await screen.findByRole("list", { name: `Test cases in ${name}` });
  await new Promise((r) => setTimeout(r, 0));
  return l;
}
