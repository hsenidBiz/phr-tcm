import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, test, vi } from "vitest";
import type { RuleCheck, SubmitItemResult, TestCase } from "../../src/bindings";
import { submitFinished, submitPhaseSnapshot } from "../../src/lib/submitRun";
import { toast } from "../../src/lib/toast";
import QueueSection from "../../src/components/QueueSection";

vi.mock("../../src/hooks/useOnScreen", () => ({ useOnScreen: () => [() => {}, true] }));
vi.mock("../../src/lib/toast", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn(), dismiss: vi.fn() },
}));

afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.clearAllMocks();
  const p = submitPhaseSnapshot();
  if (p) submitFinished(p.run);
});

function makeCase(title: string, overrides: Partial<TestCase> = {}): TestCase {
  return {
    title,
    steps: [{ action: "Open page", expected: "Page shown" }],
    tags: "smoke",
    automation_status: "Not Automated",
    module_value: "",
    preconditions: "",
    update_id: null,
    spec_order: null,
    tester_order: null,
    ...overrides,
  };
}

function Harness({ initial }: { initial: TestCase[] }) {
  const [queue, setQueue] = useState<TestCase[]>(initial);
  return <QueueSection org="acme" project="Web" pbiId={42} queue={queue} setQueue={setQueue} />;
}

function renderQueue(initial: TestCase[]) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <Harness initial={initial} />
    </QueryClientProvider>,
  );
}

const moduleRequired = (index: number) => ({
  index,
  reference_name: "Custom.Module",
  field: "Module",
  kind: "required",
  message: "Module is required for a Test Case in this project - pick one.",
});

function mocks(check: RuleCheck, submit?: (queue: TestCase[]) => SubmitItemResult[]) {
  const state = { submitted: 0 };
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "check_field_rules") return check;
    if (cmd === "submit_queue") {
      state.submitted += 1;
      return submit?.((args as { queue: TestCase[] }).queue) ?? [];
    }
    return undefined;
  });
  return state;
}

test("cases missing a required field are warned on their rows, summarised, and not sent", async () => {
  const state = mocks({ checked: true, problems: [moduleRequired(0), moduleRequired(2)] });
  renderQueue([makeCase("First"), makeCase("Second", { module_value: "Payroll" }), makeCase("Third")]);

  // The summary above the queue, in the owner's words.
  expect(await screen.findByText("2 cases need a Module before they can be sent")).toBeInTheDocument();

  // Each of the two rows says so itself; the one with a Module does not.
  const warning = "Module is required for a Test Case in this project - pick one.";
  expect(await screen.findAllByText(warning)).toHaveLength(2);
  const rows = screen.getAllByRole("row");
  expect(within(rows[0]).getByText(warning)).toBeInTheDocument();
  expect(within(rows[1]).queryByText(warning)).not.toBeInTheDocument();
  expect(within(rows[2]).getByText(warning)).toBeInTheDocument();

  // The review still opens (that is where they are read), but the way on
  // is shut until they are put right, and nothing was sent.
  fireEvent.click(screen.getByRole("button", { name: /Review 3 test cases/ }));
  const go = await screen.findByRole("button", { name: /create 3/ });
  expect(go).toBeDisabled();
  expect(screen.getByText("Fix the flagged items first.")).toBeInTheDocument();
  fireEvent.click(go);
  expect(state.submitted).toBe(0);
});

test("a value outside the allowed values reads in its own words", async () => {
  mocks({
    checked: true,
    problems: [
      {
        index: 0,
        reference_name: "Custom.Module",
        field: "Module",
        kind: "not_allowed",
        message: 'Module "Billing" is not one of the allowed values for a Test Case in this project - pick one of: Payroll.',
      },
    ],
  });
  renderQueue([makeCase("First", { module_value: "Billing" })]);
  expect(await screen.findByText("1 case has a Module that is not one of the allowed values")).toBeInTheDocument();
  expect(screen.getByText(/is not one of the allowed values for a Test Case/)).toBeInTheDocument();
});

test("rules that could not be read block nothing and say so once", async () => {
  mocks({ checked: false, problems: [] });
  renderQueue([makeCase("First")]);
  await waitFor(() => expect(toast.warning).toHaveBeenCalledTimes(1));
  expect(vi.mocked(toast.warning).mock.calls[0][0]).toMatch(/could not be checked/);
  expect(screen.getByRole("button", { name: /Review 1 test case/ })).toBeEnabled();
});

test("after a submit with a rule error the row and the summary give the plain reason", async () => {
  const refused = (index: number, title: string): SubmitItemResult => ({
    index,
    title,
    action: "failed",
    id: null,
    error: "Module is required",
    attached: [],
  });
  const state = mocks({ checked: true, problems: [] }, (q) => q.map((tc, i) => refused(i, tc.title)));
  renderQueue([makeCase("First"), makeCase("Second")]);

  fireEvent.click(screen.getByRole("button", { name: /Review 2 test cases/ }));
  fireEvent.click(await screen.findByRole("button", { name: /Yes — create 2/ }));
  await waitFor(() => expect(state.submitted).toBe(1));

  // Grouped: one line for the two refusals.
  const why = await screen.findByRole("list", { name: "Why cases failed" });
  expect(within(why).getByText("2 failed: Module is required")).toBeInTheDocument();
  // And each failed row carries the reason, not a bare HTTP status.
  const rows = screen.getAllByRole("row");
  expect(within(rows[0]).getByText("Module is required")).toBeInTheDocument();
  expect(within(rows[1]).getByText("Module is required")).toBeInTheDocument();
  expect(screen.queryByText(/HTTP 400/)).not.toBeInTheDocument();
});
