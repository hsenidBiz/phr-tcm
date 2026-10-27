import { clearMocks } from "@tauri-apps/api/mocks";
import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { caseRow, expandSuite, mountScreen } from "./testSupport";

vi.mock("../../lib/toast", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.clearAllMocks();
});

test("with no PBI every plan is a table of its suites, collapsed, in tree order", async () => {
  const { calls } = mountScreen();
  expect(await screen.findByRole("region", { name: "Auth - Test Plan" })).toBeInTheDocument();
  expect(screen.getByRole("region", { name: "Billing - Test Plan" })).toBeInTheDocument();
  const auth = screen.getByRole("region", { name: "Auth - Test Plan" });
  const rows = within(auth).getAllByRole("button", { name: /^Expand / });
  expect(rows.map((r) => r.getAttribute("aria-label"))).toEqual([
    "Expand Regression",
    "Expand Smoke",
    "Expand PBI 42 suite",
    "Expand PBI 55 suite",
  ]);
  // Smoke sits under Regression: one level deeper.
  expect(rows[1].style.paddingLeft).not.toBe(rows[0].style.paddingLeft);
  expect(within(auth).getByText("PBI 42")).toBeInTheDocument();
  // Nothing is expanded, so no suite has been read yet.
  expect(calls.some((c) => c.cmd === "list_suite_entries")).toBe(false);
  expect(screen.queryByRole("button", { name: "Show all plans" })).not.toBeInTheDocument();
});

test("expanding a suite loads its cases; collapsing hides them", async () => {
  const { calls } = mountScreen();
  const l = await expandSuite("Regression");
  expect(within(l).getAllByRole("listitem")).toHaveLength(3);
  expect(calls.filter((c) => c.cmd === "list_suite_entries").map((c) => (c.args as { suiteId: number }).suiteId)).toEqual([91]);
  fireEvent.click(screen.getByRole("button", { name: "Collapse Regression" }));
  expect(screen.queryByRole("list", { name: "Test cases in Regression" })).not.toBeInTheDocument();
});

test("a picked PBI shows only its plan with its suite open; Show all plans widens it", async () => {
  mountScreen(undefined, { id: 42, title: "Login", work_item_type: "Product Backlog Item", project: "Web" });
  expect(await screen.findByRole("region", { name: "Auth - Test Plan" })).toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "Billing - Test Plan" })).not.toBeInTheDocument();
  expect(await screen.findByRole("list", { name: "Test cases in PBI 42 suite" })).toBeInTheDocument();
  expect(screen.getByText("Showing the plan that holds PBI #42.")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Show all plans" }));
  expect(screen.getByRole("region", { name: "Billing - Test Plan" })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Show only this PBI's plan" }));
  expect(screen.queryByRole("region", { name: "Billing - Test Plan" })).not.toBeInTheDocument();
});

test("a new PBI in the same plan opens its own suite without closing the old one", async () => {
  const { rerender } = mountScreen(undefined, { id: 42, title: "Login", work_item_type: "Product Backlog Item", project: "Web" });
  expect(await screen.findByRole("list", { name: "Test cases in PBI 42 suite" })).toBeInTheDocument();
  rerender({ id: 55, title: "Reset password", work_item_type: "Product Backlog Item", project: "Web" });
  expect(await screen.findByRole("list", { name: "Test cases in PBI 55 suite" })).toBeInTheDocument();
  expect(screen.getByRole("list", { name: "Test cases in PBI 42 suite" })).toBeInTheDocument();
});

test("a picked PBI with no suite in any plan falls back to every plan and says so", async () => {
  mountScreen(undefined, { id: 77, title: "Orphan", work_item_type: "Product Backlog Item", project: "Web" });
  expect(await screen.findByRole("region", { name: "Billing - Test Plan" })).toBeInTheDocument();
  expect(screen.getByText("PBI #77 has no test suite yet. Showing every plan.")).toBeInTheDocument();
});

test("a suite handed over from Search Suites opens, whatever PBI is in the bar", async () => {
  // The PBI in the bar belongs to the Auth plan; the focus points at the
  // Billing plan's own suite. The focus is what the user just clicked, so
  // it wins.
  mountScreen(
    undefined,
    { id: 42, title: "Login work", work_item_type: "Product Backlog Item", project: "Web" },
    { planId: 10, suiteId: 101 },
  );
  const billing = await screen.findByRole("region", { name: "Billing - Test Plan" });
  expect(await within(billing).findByRole("list", { name: "Test cases in Invoices" })).toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "Auth - Test Plan" })).not.toBeInTheDocument();
});

test("a focus whose plan exists but whose suite no longer does falls back to the ordinary view", async () => {
  const onFocusHandled = vi.fn();
  mountScreen(
    undefined,
    null,
    { planId: 9, suiteId: 999 },
    onFocusHandled,
  );
  expect(await screen.findByRole("region", { name: "Auth - Test Plan" })).toBeInTheDocument();
  expect(screen.getByRole("region", { name: "Billing - Test Plan" })).toBeInTheDocument();
  await waitFor(() => expect(onFocusHandled).toHaveBeenCalledTimes(1));
});

test("no plans yet", async () => {
  mountScreen((cmd) => (cmd === "list_plans_with_suites" ? [] : undefined));
  expect(await screen.findByText("No test plans with test suites in this project yet.")).toBeInTheDocument();
});

/// The tour rings this screen's plan tables. A ring around an empty box
/// says nothing, so the anchor has to hold the tables themselves - and,
/// when there are none, the sentence that explains why.
test("the tour's anchor wraps the plan tables, and never rings an empty box", async () => {
  const { container } = mountScreen();
  await screen.findByRole("region", { name: "Auth - Test Plan" });
  const box = container.querySelector('[data-tour="manage-plans"]') as HTMLElement;
  expect(box, 'no [data-tour="manage-plans"] on Suite Management').not.toBeNull();
  expect(within(box).getByRole("region", { name: "Auth - Test Plan" })).toBeInTheDocument();
  expect(within(box).getByRole("region", { name: "Billing - Test Plan" })).toBeInTheDocument();
});

test("narrowing back to the PBI's plan while another plan holds a selection shows a notice; Clear selection there drops it", async () => {
  mountScreen(undefined, { id: 42, title: "Login", work_item_type: "Product Backlog Item", project: "Web" });
  fireEvent.click(await screen.findByRole("button", { name: "Show all plans" }));
  const l = await expandSuite("Invoices");
  fireEvent.click(caseRow(l, 201), { ctrlKey: true });
  fireEvent.click(screen.getByRole("button", { name: "Show only this PBI's plan" }));
  expect(
    screen.getByText("1 test case selected in Billing - Test Plan is out of view."),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Clear selection" }));
  expect(screen.queryByText(/out of view/)).not.toBeInTheDocument();
});

// ---- Projects together -------------------------------------------------

const TOGETHER = { allProjects: true };

/** The project each call of one command named, in order. */
const asked = (calls: Array<{ cmd: string; args: unknown }>, cmd: string) =>
  calls.filter((c) => c.cmd === cmd).map((c) => (c.args as { project: string }).project);

test("combined Suite Management without a PBI lists every project's plans", async () => {
  const { calls, container } = mountScreen(undefined, null, null, undefined, TOGETHER);
  expect(await screen.findByRole("region", { name: "Payslip - Test Plan" })).toBeInTheDocument();
  expect(screen.getByRole("region", { name: "Auth - Test Plan" })).toBeInTheDocument();
  expect(screen.getByRole("region", { name: "Billing - Test Plan" })).toBeInTheDocument();
  // More than one project has plans: each plan sits under its project, in
  // name order, inside the box the tour rings.
  const headings = screen.getAllByRole("heading", { level: 2 }).map((h) => h.textContent);
  expect(headings).toEqual(["Payroll", "Web"]);
  const box = container.querySelector('[data-tour="manage-plans"]') as HTMLElement;
  expect(within(box).getByRole("region", { name: "Payslip - Test Plan" })).toBeInTheDocument();
  // Every project was read, each in its own project; nothing to apologise for.
  expect(asked(calls, "list_plans_with_suites").sort()).toEqual(["Payroll", "Web"]);
  expect(screen.queryByText(/Showing plans from/)).not.toBeInTheDocument();
});

test("combined Suite Management with plans in one project shows no project header", async () => {
  mountScreen(
    (cmd, args) =>
      cmd === "list_plans_with_suites" && (args as { project: string }).project === "Web" ? [] : undefined,
    null,
    null,
    undefined,
    TOGETHER,
  );
  expect(await screen.findByRole("region", { name: "Payslip - Test Plan" })).toBeInTheDocument();
  expect(screen.queryByRole("heading", { level: 2 })).not.toBeInTheDocument();
});

test("combined Suite Management says so when the organization has no projects", async () => {
  mountScreen((cmd) => (cmd === "list_projects" ? [] : undefined), null, null, undefined, TOGETHER);
  expect(await screen.findByText("This organization has no projects you can read.")).toBeInTheDocument();
});

test("combined Suite Management says so when a project could not be read", async () => {
  mountScreen(
    (cmd, args) => {
      if (cmd === "list_plans_with_suites" && (args as { project: string }).project === "Payroll")
        throw { kind: "Forbidden" };
      return undefined;
    },
    null,
    null,
    undefined,
    TOGETHER,
  );
  expect(await screen.findByRole("region", { name: "Auth - Test Plan" })).toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "Payslip - Test Plan" })).not.toBeInTheDocument();
  expect(screen.getByText("Showing plans from 1 of 2 projects.")).toBeInTheDocument();
});

test("with a PBI picked only the PBI's plan shows, as before", async () => {
  // The PBI is in Payroll, so App's effective project is Payroll: the screen
  // reads that one project, not the organization.
  const { calls } = mountScreen(
    undefined,
    { id: 77, title: "Payslip", work_item_type: "Product Backlog Item", project: "Payroll" },
    null,
    undefined,
    { ...TOGETHER, project: "Payroll" },
  );
  expect(await screen.findByRole("region", { name: "Payslip - Test Plan" })).toBeInTheDocument();
  expect(await screen.findByRole("list", { name: "Test cases in PBI 77 suite" })).toBeInTheDocument();
  expect(screen.getByText("Showing the plan that holds PBI #77.")).toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "Auth - Test Plan" })).not.toBeInTheDocument();
  expect(screen.queryByRole("heading", { level: 2 })).not.toBeInTheDocument();
  expect(asked(calls, "list_plans_with_suites")).toEqual(["Payroll"]);
  expect(calls.some((c) => c.cmd === "list_projects")).toBe(false);
});

test("switched off Suite Management lists the picked project as before", async () => {
  const { calls } = mountScreen();
  expect(await screen.findByRole("region", { name: "Auth - Test Plan" })).toBeInTheDocument();
  expect(screen.getByRole("region", { name: "Billing - Test Plan" })).toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "Payslip - Test Plan" })).not.toBeInTheDocument();
  expect(screen.queryByRole("heading", { level: 2 })).not.toBeInTheDocument();
  expect(asked(calls, "list_plans_with_suites")).toEqual(["Web"]);
  expect(calls.some((c) => c.cmd === "list_projects")).toBe(false);
});

test("a suite handed over from Search Suites opens its plan in the plan's own project", async () => {
  // The home project is Web; the suite lives in Payroll. Its cases are read
  // there, and nothing else is shown.
  const { calls } = mountScreen(undefined, null, { planId: 12, suiteId: 121, project: "Payroll" }, undefined, TOGETHER);
  const payslip = await screen.findByRole("region", { name: "Payslip - Test Plan" });
  expect(await within(payslip).findByRole("list", { name: "Test cases in Slips" })).toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "Auth - Test Plan" })).not.toBeInTheDocument();
  expect(screen.queryByRole("heading", { level: 2 })).not.toBeInTheDocument();
  expect(asked(calls, "list_suite_entries")).toEqual(["Payroll"]);
  expect(asked(calls, "list_test_points")).toEqual(["Payroll"]);
});

test("a handed-over suite without its project is found by its plan", async () => {
  const { calls } = mountScreen(undefined, null, { planId: 12, suiteId: 121 }, undefined, TOGETHER);
  const payslip = await screen.findByRole("region", { name: "Payslip - Test Plan" });
  expect(await within(payslip).findByRole("list", { name: "Test cases in Slips" })).toBeInTheDocument();
  expect(asked(calls, "list_suite_entries")).toEqual(["Payroll"]);
});

test("a handed-over suite that no longer exists in any project falls back to every plan", async () => {
  const onFocusHandled = vi.fn();
  mountScreen(undefined, null, { planId: 12, suiteId: 999, project: "Payroll" }, onFocusHandled, TOGETHER);
  expect(await screen.findByRole("region", { name: "Auth - Test Plan" })).toBeInTheDocument();
  expect(screen.getByRole("region", { name: "Payslip - Test Plan" })).toBeInTheDocument();
  await waitFor(() => expect(onFocusHandled).toHaveBeenCalledTimes(1));
});
