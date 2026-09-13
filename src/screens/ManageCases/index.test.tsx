import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ManageCases from "./index";
import { PLANS, mountWithSuite, pickSuite } from "./testSupport";

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.clearAllMocks();
});

test("the list shows the suite's cases once each, in entry order, with positions", async () => {
  mountWithSuite();
  const l = await pickSuite(91);
  const rows = within(l).getAllByRole("listitem");
  expect(rows).toHaveLength(3);
  expect(rows[0]).toHaveTextContent("1");
  expect(rows[0]).toHaveTextContent("#201");
  expect(rows[0]).toHaveTextContent("Valid login");
  expect(rows[1]).toHaveTextContent("#202");
  expect(rows[2]).toHaveTextContent("#203");
  // The child suite entry (95) is not a case and does not appear.
  expect(within(l).queryByText(/#95/)).not.toBeInTheDocument();
  // Nothing has moved: nothing to apply.
  expect(screen.getByRole("button", { name: "Apply order" })).toBeDisabled();
});

test("dragging a row onto another moves it; Apply order sends the ids and reloads", async () => {
  const { calls } = mountWithSuite((cmd) => {
    if (cmd === "reorder_suite_cases") return [203, 201, 202];
  });
  const l = await pickSuite(91);
  const rows = within(l).getAllByRole("listitem");

  fireEvent.dragStart(rows[2]);
  fireEvent.dragOver(rows[0]);
  fireEvent.drop(rows[0]);

  const after = within(l).getAllByRole("listitem");
  expect(after[0]).toHaveTextContent("#203");
  expect(after[1]).toHaveTextContent("#201");
  expect(after[2]).toHaveTextContent("#202");
  const apply = screen.getByRole("button", { name: "Apply order" });
  expect(apply).toBeEnabled();
  fireEvent.click(apply);

  await waitFor(() => {
    const call = calls.find((c) => c.cmd === "reorder_suite_cases");
    expect(call?.args).toEqual({ organization: "acme", project: "Web", suiteId: 91, caseIds: [203, 201, 202] });
  });
  // The list re-reads from the server after a save.
  await waitFor(() => expect(calls.filter((c) => c.cmd === "list_suite_entries").length).toBeGreaterThan(1));
});

test("Move up and Move down step a row one place; Reset returns to the server order", async () => {
  mountWithSuite();
  const l = await pickSuite(91);
  fireEvent.click(within(l).getByRole("button", { name: "Move #202 up" }));
  expect(within(l).getAllByRole("listitem")[0]).toHaveTextContent("#202");
  fireEvent.click(within(l).getByRole("button", { name: "Move #202 down" }));
  expect(within(l).getAllByRole("listitem")[0]).toHaveTextContent("#201");
  fireEvent.click(within(l).getByRole("button", { name: "Move #201 down" }));
  expect(screen.getByRole("button", { name: "Apply order" })).toBeEnabled();
  fireEvent.click(screen.getByRole("button", { name: "Reset" }));
  expect(within(l).getAllByRole("listitem")[0]).toHaveTextContent("#201");
  expect(screen.getByRole("button", { name: "Apply order" })).toBeDisabled();
});

test("an empty suite says so", async () => {
  mockIPC((cmd) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_plans_with_suites") return PLANS;
    if (cmd === "list_suite_entries") return [];
    if (cmd === "list_test_points") return [];
    return undefined;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <ManageCases org="acme" project="Web" />
    </QueryClientProvider>,
  );
  const plan = await screen.findByLabelText("Test plan");
  await screen.findByText("Auth - Test Plan");
  fireEvent.change(plan, { target: { value: "9" } });
  fireEvent.change(screen.getByLabelText("Test suite"), { target: { value: "91" } });
  expect(await screen.findByText("No test cases in this suite.")).toBeInTheDocument();
});
