import { clearMocks } from "@tauri-apps/api/mocks";
import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { toast } from "../../lib/toast";
import { caseRow, expandSuite, mountScreen } from "./testSupport";

vi.mock("../../lib/toast", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.clearAllMocks();
});

// Scoped to a plan's own region: two plans each carry a "Copy to" combobox
// with the same accessible name, so an unscoped query would be ambiguous.
async function openSelect(scope: HTMLElement, name: string) {
  const trigger = within(scope).getByRole("combobox", { name });
  fireEvent.click(trigger);
  return trigger;
}

test("selecting cases in a plan enables Copy to suite for that plan only; copying adds them and keeps them", async () => {
  const { calls } = mountScreen((cmd) => (cmd === "add_cases_to_suite" ? [201, 203] : undefined));
  const l = await expandSuite("PBI 42 suite");
  const auth = screen.getByRole("region", { name: "Auth - Test Plan" });
  const billing = screen.getByRole("region", { name: "Billing - Test Plan" });
  expect(within(auth).getByRole("button", { name: "Copy to suite" })).toBeDisabled();

  fireEvent.click(caseRow(l, 201), { ctrlKey: true });
  fireEvent.click(caseRow(l, 203), { ctrlKey: true });
  expect(within(auth).getByText("2 selected")).toBeInTheDocument();
  expect(within(billing).queryByText(/selected/)).not.toBeInTheDocument();
  // A target is needed: static suites of this plan (root included), never
  // the PBI suite. The list is searchable - a project's suites run long.
  await openSelect(auth, "Copy to");
  const labels = screen.getAllByRole("option").map((o) => o.textContent?.trim());
  expect(labels).toEqual(["Plan root", "Regression", "Smoke"]);
  fireEvent.change(screen.getByPlaceholderText("Search…"), { target: { value: "regr" } });
  expect(screen.getAllByRole("option").map((o) => o.textContent?.trim())).toEqual(["Regression"]);
  fireEvent.click(screen.getByRole("option", { name: "Regression" }));
  fireEvent.click(within(auth).getByRole("button", { name: "Copy to suite" }));

  await waitFor(() => {
    const add = calls.find((c) => c.cmd === "add_cases_to_suite");
    expect(add?.args).toEqual({ organization: "acme", project: "Web", planId: 9, suiteId: 91, caseIds: [201, 203] });
  });
  expect(toast.success).toHaveBeenCalledWith("Copied 2 test cases to Regression. They stay where they were.");
  // The selection is spent, and the target suite is read again if it is open.
  await waitFor(() => expect(within(auth).queryByText(/selected/)).not.toBeInTheDocument());
});

test("changing the picker mid-copy still names and invalidates the suite that was actually targeted (regression)", async () => {
  let resolveAdd: (cases: number[]) => void = () => {};
  const pending = new Promise<number[]>((resolve) => {
    resolveAdd = resolve;
  });
  const { calls } = mountScreen((cmd) => (cmd === "add_cases_to_suite" ? pending : undefined));
  const l = await expandSuite("PBI 42 suite");
  const auth = screen.getByRole("region", { name: "Auth - Test Plan" });

  fireEvent.click(caseRow(l, 201), { ctrlKey: true });
  fireEvent.click(caseRow(l, 203), { ctrlKey: true });
  await openSelect(auth, "Copy to");
  fireEvent.click(screen.getByRole("option", { name: "Regression" }));
  fireEvent.click(within(auth).getByRole("button", { name: "Copy to suite" }));

  await waitFor(() => {
    const add = calls.find((c) => c.cmd === "add_cases_to_suite");
    expect(add?.args).toEqual({ organization: "acme", project: "Web", planId: 9, suiteId: 91, caseIds: [201, 203] });
  });

  // Copy is still in flight (the mock hasn't resolved yet): change the picker.
  await openSelect(auth, "Copy to");
  fireEvent.click(screen.getByRole("option", { name: "Smoke" }));

  resolveAdd([201, 203]);
  await waitFor(() => expect(toast.success).toHaveBeenCalled());
  expect(toast.success).toHaveBeenCalledWith("Copied 2 test cases to Regression. They stay where they were.");
});

test("selecting in a second plan starts a new selection", async () => {
  mountScreen();
  const l1 = await expandSuite("Regression");
  fireEvent.click(caseRow(l1, 201), { ctrlKey: true });
  const l2 = await expandSuite("Invoices");
  fireEvent.click(caseRow(l2, 202), { ctrlKey: true });
  const auth = screen.getByRole("region", { name: "Auth - Test Plan" });
  const billing = screen.getByRole("region", { name: "Billing - Test Plan" });
  expect(within(billing).getByText("1 selected")).toBeInTheDocument();
  expect(within(auth).queryByText(/selected/)).not.toBeInTheDocument();
  expect(caseRow(l1, 201)).not.toHaveAttribute("data-selected");
});

test("New test suite offers root and static suites as parents; with cases selected it creates and copies", async () => {
  const { calls } = mountScreen((cmd, args) => {
    if (cmd === "create_static_suite")
      return { id: 94, name: (args as { name: string }).name, suite_type: "staticTestSuite", requirement_id: null, parent_id: 90 };
    if (cmd === "add_cases_to_suite") return [202];
    return undefined;
  });
  const l = await expandSuite("Regression");
  fireEvent.click(caseRow(l, 202), { ctrlKey: true });
  const auth = screen.getByRole("region", { name: "Auth - Test Plan" });
  fireEvent.click(within(auth).getByRole("button", { name: "New test suite" }));
  const dialog = await screen.findByRole("dialog");
  fireEvent.click(within(dialog).getByRole("combobox", { name: "Create inside" }));
  expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual([
    "Plan root (Auth - Test Plan)",
    "Regression",
    "    Smoke",
  ]);
  fireEvent.click(screen.getByRole("option", { name: "Plan root (Auth - Test Plan)" }));
  fireEvent.change(within(dialog).getByLabelText("Suite name"), { target: { value: "  Nightly  " } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Create suite and add 1 test case" }));
  await waitFor(() => {
    const create = calls.find((c) => c.cmd === "create_static_suite");
    expect(create?.args).toEqual({ organization: "acme", project: "Web", planId: 9, parentSuiteId: 90, name: "Nightly" });
  });
  await waitFor(() => {
    const add = calls.find((c) => c.cmd === "add_cases_to_suite");
    expect(add?.args).toEqual({ organization: "acme", project: "Web", planId: 9, suiteId: 94, caseIds: [202] });
  });
  expect(toast.success).toHaveBeenCalledWith('Created suite "Nightly" and added 1 test case. They stay in their suites too.');
  await waitFor(() => expect(calls.filter((c) => c.cmd === "list_plans_with_suites").length).toBeGreaterThan(1));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});

test("New test suite with nothing selected only creates; an empty name is refused", async () => {
  const { calls } = mountScreen((cmd) =>
    cmd === "create_static_suite"
      ? { id: 94, name: "Nightly", suite_type: "staticTestSuite", requirement_id: null, parent_id: 100 }
      : undefined,
  );
  const billing = await screen.findByRole("region", { name: "Billing - Test Plan" });
  fireEvent.click(within(billing).getByRole("button", { name: "New test suite" }));
  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).getByRole("button", { name: "Create suite" })).toBeDisabled();
  fireEvent.change(within(dialog).getByLabelText("Suite name"), { target: { value: "   " } });
  expect(within(dialog).getByRole("button", { name: "Create suite" })).toBeDisabled();
  fireEvent.change(within(dialog).getByLabelText("Suite name"), { target: { value: "Nightly" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Create suite" }));
  await waitFor(() => {
    const create = calls.find((c) => c.cmd === "create_static_suite");
    expect(create?.args).toEqual({ organization: "acme", project: "Web", planId: 10, parentSuiteId: 100, name: "Nightly" });
  });
  expect(calls.some((c) => c.cmd === "add_cases_to_suite")).toBe(false);
  expect(toast.success).toHaveBeenCalledWith('Created suite "Nightly".');
});

test("the suite is created but the copy fails: the suite still shows up and the toast says what happened", async () => {
  const { calls } = mountScreen((cmd) => {
    if (cmd === "create_static_suite")
      return { id: 94, name: "Nightly", suite_type: "staticTestSuite", requirement_id: null, parent_id: 90 };
    if (cmd === "add_cases_to_suite") throw new Error("boom");
    return undefined;
  });
  const l = await expandSuite("Regression");
  fireEvent.click(caseRow(l, 202), { ctrlKey: true });
  const auth = screen.getByRole("region", { name: "Auth - Test Plan" });
  fireEvent.click(within(auth).getByRole("button", { name: "New test suite" }));
  const dialog = await screen.findByRole("dialog");
  fireEvent.change(within(dialog).getByLabelText("Suite name"), { target: { value: "Nightly" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Create suite and add 1 test case" }));
  await waitFor(() => expect(toast.warning).toHaveBeenCalledWith(expect.stringContaining('Created suite "Nightly", but the test cases could not be added'), { duration: 20000 }));
  await waitFor(() => expect(calls.filter((c) => c.cmd === "list_plans_with_suites").length).toBeGreaterThan(1));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});

test("Clear selection in a plan's header drops the selection and unchecks its rows", async () => {
  mountScreen();
  const l = await expandSuite("Regression");
  fireEvent.click(caseRow(l, 201), { ctrlKey: true });
  const auth = screen.getByRole("region", { name: "Auth - Test Plan" });
  expect(within(auth).getByText("1 selected")).toBeInTheDocument();
  fireEvent.click(within(auth).getByRole("button", { name: "Clear selection" }));
  expect(within(auth).queryByText(/selected/)).not.toBeInTheDocument();
  expect(caseRow(l, 201)).not.toHaveAttribute("data-selected");
});

test("New test suite is hidden only when Azure DevOps says no", async () => {
  // A clear no: the control goes away.
  mountScreen((cmd) => (cmd === "can_create_test_suites" ? false : undefined));
  const auth = await screen.findByRole("region", { name: "Auth - Test Plan" });
  await waitFor(() =>
    expect(within(auth).queryByRole("button", { name: /New test suite/i })).not.toBeInTheDocument(),
  );
});

test("an unanswerable permission check leaves New test suite in place", async () => {
  // null = could not ask. The button stays; the create itself refuses.
  const { calls } = mountScreen((cmd) => (cmd === "can_create_test_suites" ? null : undefined));
  const auth = await screen.findByRole("region", { name: "Auth - Test Plan" });
  // Wait for the answer to actually land before asserting: right after the
  // region appears, the query is still loading (data === undefined), so an
  // assertion here would pass just as well against a falsiness bug like
  // `!mayCreate.data`. Only once the `null` answer has been applied does
  // this prove the fail-open posture rather than the loading state.
  await waitFor(() => expect(calls.some((c) => c.cmd === "can_create_test_suites")).toBe(true));
  await new Promise((r) => setTimeout(r, 0));
  expect(within(auth).getByRole("button", { name: /New test suite/i })).toBeInTheDocument();
});

test("a suite's header count is its own, not the whole plan's selection", async () => {
  mountScreen();
  const regression = await expandSuite("Regression");
  fireEvent.click(caseRow(regression, 201), { ctrlKey: true });
  fireEvent.click(caseRow(regression, 202), { ctrlKey: true });
  await expandSuite("Smoke");
  expect(screen.getByText("2 test cases")).toBeInTheDocument();
  expect(screen.getByText("2 of 3 selected")).toBeInTheDocument();
});

// ---- Projects together: every write lands in the plan's own project -----
//
// The home project is Web. The Payslip plan lives in Payroll, so anything
// written to it that carries "Web" would be a write in the wrong project.

const TOGETHER = { allProjects: true };

/** The project each call of one command named, in order. */
const projectsOf = (calls: Array<{ cmd: string; args: unknown }>, cmd: string) =>
  calls.filter((c) => c.cmd === cmd).map((c) => (c.args as { project: string }).project);

test("reordering a suite writes to its plan's project", async () => {
  const { calls } = mountScreen(
    (cmd) => (cmd === "reorder_suite_cases" ? [203, 201, 202] : undefined),
    null,
    null,
    undefined,
    TOGETHER,
  );
  const l = await expandSuite("Slips");
  // The cases of the suite were read in Payroll as well.
  expect(projectsOf(calls, "list_suite_entries")).toEqual(["Payroll"]);
  expect(projectsOf(calls, "list_test_points")).toEqual(["Payroll"]);

  const rows = within(l).getAllByRole("listitem");
  fireEvent.dragStart(rows[2]);
  fireEvent.dragOver(rows[0]);
  fireEvent.drop(rows[0]);
  fireEvent.click(screen.getByRole("button", { name: "Apply order" }));
  await waitFor(() => {
    const call = calls.find((c) => c.cmd === "reorder_suite_cases");
    expect(call?.args).toEqual({ organization: "acme", project: "Payroll", suiteId: 121, caseIds: [203, 201, 202] });
  });
  expect(calls.filter((c) => c.cmd === "reorder_suite_cases")).toHaveLength(1);
});

test("a new static suite is created in its plan's project", async () => {
  const { calls } = mountScreen(
    (cmd, args) => {
      if (cmd === "create_static_suite")
        return { id: 123, name: (args as { name: string }).name, suite_type: "staticTestSuite", requirement_id: null, parent_id: 120 };
      if (cmd === "add_cases_to_suite") return [202];
      return undefined;
    },
    null,
    null,
    undefined,
    TOGETHER,
  );
  const l = await expandSuite("Slips");
  fireEvent.click(caseRow(l, 202), { ctrlKey: true });
  const payslip = screen.getByRole("region", { name: "Payslip - Test Plan" });
  fireEvent.click(within(payslip).getByRole("button", { name: "New test suite" }));
  const dialog = await screen.findByRole("dialog");
  fireEvent.change(within(dialog).getByLabelText("Suite name"), { target: { value: "Nightly" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Create suite and add 1 test case" }));

  await waitFor(() => {
    const create = calls.find((c) => c.cmd === "create_static_suite");
    expect(create?.args).toEqual({ organization: "acme", project: "Payroll", planId: 12, parentSuiteId: 120, name: "Nightly" });
  });
  await waitFor(() => {
    const add = calls.find((c) => c.cmd === "add_cases_to_suite");
    expect(add?.args).toEqual({ organization: "acme", project: "Payroll", planId: 12, suiteId: 123, caseIds: [202] });
  });
  // The permission to create was asked in the plan's own project, too.
  expect(projectsOf(calls, "can_create_test_suites").filter((p) => p === "Payroll")).toHaveLength(1);
  // Only the project that changed is read again, and the new suite shows.
  await waitFor(() => expect(projectsOf(calls, "list_plans_with_suites").filter((p) => p === "Payroll")).toHaveLength(2));
  expect(projectsOf(calls, "list_plans_with_suites").filter((p) => p === "Web")).toHaveLength(1);
  await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
});

test("copying cases writes to the plan's project", async () => {
  const { calls } = mountScreen((cmd) => (cmd === "add_cases_to_suite" ? [201] : undefined), null, null, undefined, TOGETHER);
  const l = await expandSuite("Slips");
  fireEvent.click(caseRow(l, 201), { ctrlKey: true });
  const payslip = screen.getByRole("region", { name: "Payslip - Test Plan" });
  await openSelect(payslip, "Copy to");
  fireEvent.click(screen.getByRole("option", { name: "Plan root" }));
  fireEvent.click(within(payslip).getByRole("button", { name: "Copy to suite" }));
  await waitFor(() => {
    const add = calls.find((c) => c.cmd === "add_cases_to_suite");
    expect(add?.args).toEqual({ organization: "acme", project: "Payroll", planId: 12, suiteId: 120, caseIds: [201] });
  });
});

test("copy targets never include another plan's suites", async () => {
  // Every plan of every project is on screen at once. A selection belongs to
  // one plan, and its copy targets are that plan's suites alone: a copy could
  // otherwise cross projects.
  mountScreen(undefined, null, null, undefined, TOGETHER);
  const l = await expandSuite("Slips");
  fireEvent.click(caseRow(l, 201), { ctrlKey: true });
  const payslip = screen.getByRole("region", { name: "Payslip - Test Plan" });
  await openSelect(payslip, "Copy to");
  expect(screen.getAllByRole("option").map((o) => o.textContent?.trim())).toEqual(["Plan root", "Slips"]);
  fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });

  // The selection is Payroll's: the other projects' plans cannot take it.
  for (const name of ["Auth - Test Plan", "Billing - Test Plan"]) {
    const plan = screen.getByRole("region", { name });
    expect(within(plan).getByRole("button", { name: "Copy to suite" })).toBeDisabled();
    expect(within(plan).queryByText(/selected/)).not.toBeInTheDocument();
  }
});
