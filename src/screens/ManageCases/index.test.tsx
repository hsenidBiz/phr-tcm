import { clearMocks } from "@tauri-apps/api/mocks";
import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { caseRow, expandSuite, mountScreen } from "./testSupport";

vi.mock("sonner", () => ({
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
  mountScreen(undefined, { id: 42, title: "Login", work_item_type: "Product Backlog Item" });
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
  const { rerender } = mountScreen(undefined, { id: 42, title: "Login", work_item_type: "Product Backlog Item" });
  expect(await screen.findByRole("list", { name: "Test cases in PBI 42 suite" })).toBeInTheDocument();
  rerender({ id: 55, title: "Reset password", work_item_type: "Product Backlog Item" });
  expect(await screen.findByRole("list", { name: "Test cases in PBI 55 suite" })).toBeInTheDocument();
  expect(screen.getByRole("list", { name: "Test cases in PBI 42 suite" })).toBeInTheDocument();
});

test("a picked PBI with no suite in any plan falls back to every plan and says so", async () => {
  mountScreen(undefined, { id: 77, title: "Orphan", work_item_type: "Product Backlog Item" });
  expect(await screen.findByRole("region", { name: "Billing - Test Plan" })).toBeInTheDocument();
  expect(screen.getByText("PBI #77 has no test suite yet. Showing every plan.")).toBeInTheDocument();
});

test("a suite handed over from Search Suites opens, whatever PBI is in the bar", async () => {
  // The PBI in the bar belongs to the Auth plan; the focus points at the
  // Billing plan's own suite. The focus is what the user just clicked, so
  // it wins.
  mountScreen(
    undefined,
    { id: 42, title: "Login work", work_item_type: "Product Backlog Item" },
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

test("narrowing back to the PBI's plan while another plan holds a selection shows a notice; Clear selection there drops it", async () => {
  mountScreen(undefined, { id: 42, title: "Login", work_item_type: "Product Backlog Item" });
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
