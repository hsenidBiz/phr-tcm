import { clearMocks } from "@tauri-apps/api/mocks";
import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { toast } from "sonner";
import { expandSuite, mountScreen } from "./testSupport";

vi.mock("sonner", () => ({
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

  fireEvent.click(within(l).getByRole("checkbox", { name: "Select #201" }));
  fireEvent.click(within(l).getByRole("checkbox", { name: "Select #203" }));
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

test("selecting in a second plan starts a new selection", async () => {
  mountScreen();
  const l1 = await expandSuite("Regression");
  fireEvent.click(within(l1).getByRole("checkbox", { name: "Select #201" }));
  const l2 = await expandSuite("Invoices");
  fireEvent.click(within(l2).getByRole("checkbox", { name: "Select #202" }));
  const auth = screen.getByRole("region", { name: "Auth - Test Plan" });
  const billing = screen.getByRole("region", { name: "Billing - Test Plan" });
  expect(within(billing).getByText("1 selected")).toBeInTheDocument();
  expect(within(auth).queryByText(/selected/)).not.toBeInTheDocument();
  expect(within(l1).getByRole("checkbox", { name: "Select #201" })).not.toBeChecked();
});

test("New test suite offers root and static suites as parents; with cases selected it creates and copies", async () => {
  const { calls } = mountScreen((cmd, args) => {
    if (cmd === "create_static_suite")
      return { id: 94, name: (args as { name: string }).name, suite_type: "staticTestSuite", requirement_id: null, parent_id: 90 };
    if (cmd === "add_cases_to_suite") return [202];
    return undefined;
  });
  const l = await expandSuite("Regression");
  fireEvent.click(within(l).getByRole("checkbox", { name: "Select #202" }));
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
  fireEvent.click(within(l).getByRole("checkbox", { name: "Select #202" }));
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
  fireEvent.click(within(l).getByRole("checkbox", { name: "Select #201" }));
  const auth = screen.getByRole("region", { name: "Auth - Test Plan" });
  expect(within(auth).getByText("1 selected")).toBeInTheDocument();
  fireEvent.click(within(auth).getByRole("button", { name: "Clear selection" }));
  expect(within(auth).queryByText(/selected/)).not.toBeInTheDocument();
  expect(within(l).getByRole("checkbox", { name: "Select #201" })).not.toBeChecked();
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
  fireEvent.click(within(regression).getByRole("checkbox", { name: "Select #201" }));
  fireEvent.click(within(regression).getByRole("checkbox", { name: "Select #202" }));
  await expandSuite("Smoke");
  expect(screen.getByText("2 test cases")).toBeInTheDocument();
  expect(screen.getByText("2 of 3 selected")).toBeInTheDocument();
});
