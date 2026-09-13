import { mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect } from "vitest";
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

/** Pick plan 9 and the given suite; resolves to the case list element.
 *
 * The list itself can appear a render before React flushes the passive
 * effects that follow it - including the screen's own `order`-from-
 * `cases.data` effect and each `useMutation`'s internal option sync,
 * which only runs in a `useEffect`. A `fireEvent` right after this
 * `await` would then fire a mutation whose `mutationFn` still closes
 * over the suite's *previous* (often empty) `order`. Yielding one more
 * macrotask here lets that catch up before a caller acts on the result. */
const suiteOptionName = (s: (typeof PLANS)[number]["suites"][number]) =>
  s.suite_type === "requirementTestSuite" && s.requirement_id != null
    ? `PBI ${s.requirement_id}: ${s.name}`
    : s.name;

export async function pickSuite(suiteId: number) {
  const suiteById = new Map(PLANS.flatMap((p) => p.suites).map((s) => [s.id, s]));
  const plan = await screen.findByRole("combobox", { name: "Test plan" });
  // The Select's options only exist in the DOM while it is open, and only
  // once the plan list has loaded - unlike a native <select>, which always
  // renders its (possibly still-empty) <option>s. Wait for the trigger to
  // come off "disabled" (set while `plans.data` is undefined) before
  // opening it, or the listbox opens over the stale "Loading plans" row.
  await waitFor(() => expect(plan).toBeEnabled());
  fireEvent.click(plan);
  fireEvent.click(await screen.findByRole("option", { name: "Auth - Test Plan" }));

  const suite = screen.getByRole("combobox", { name: "Test suite" });
  await waitFor(() => expect(suite).toBeEnabled());
  fireEvent.click(suite);
  fireEvent.click(screen.getByRole("option", { name: suiteOptionName(suiteById.get(suiteId)!) }));
  const list = await screen.findByRole("list", { name: "Test cases in order" });
  await new Promise((resolve) => setTimeout(resolve, 0));
  return list;
}
