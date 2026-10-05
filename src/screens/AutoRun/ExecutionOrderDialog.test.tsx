// Auto Run's Execution order dialog: the planned order with its reset
// points, moving cases, and the two ways of keeping or dropping an order.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ExecutionOrderDialog from "./ExecutionOrderDialog";

afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
});

const CASES = [
  { id: 1, title: "Publish the cycle" },
  { id: 2, title: "Open the report" },
  { id: 3, title: "Close the cycle" },
];

const PLAN = {
  order: [1, 2, 3],
  phases: [[1], [2, 3]],
  resets: [{ before_case_id: 2, names: ["Cycle"], changed_by: [["Cycle", [1]]] }],
  counts: null,
  saved: false,
};

function mount(plan: unknown = PLAN) {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    return cmd === "auto_run_plan" ? plan : null;
  });
  const onClose = vi.fn();
  render(<ExecutionOrderDialog org="acme" project="Web" pbiId={42} cases={CASES} onClose={onClose} />);
  return { calls, onClose };
}

const rows = () => screen.getAllByRole("listitem").map((li) => li.textContent ?? "");

test("lists the planned order with the reset line between its rows", async () => {
  mount();
  expect(await screen.findByText('Reset: revert "Cycle" (changed by #1 Publish the cycle)')).toBeInTheDocument();
  expect(rows()[0]).toContain("Publish the cycle");
  expect(rows()[2]).toContain("Close the cycle");
  expect(screen.queryByText(/this order needs/)).not.toBeInTheDocument();
});

test("up and down reorder, Save order sends the new order and closes", async () => {
  const { calls, onClose } = mount();
  await screen.findByText(/Reset: revert/);
  expect(screen.getByRole("button", { name: "Save order" })).toBeDisabled();

  fireEvent.click(screen.getByRole("button", { name: "Move #3 up" }));
  expect(rows()[1]).toContain("Close the cycle");
  fireEvent.click(screen.getByRole("button", { name: "Save order" }));

  await waitFor(() => expect(onClose).toHaveBeenCalled());
  expect(calls.find((c) => c.cmd === "auto_run_save_order")?.args).toEqual({ pbiId: 42, caseIds: [1, 3, 2] });
});

test("Use suggested order is offered only when an order is saved", async () => {
  mount();
  await screen.findByText(/Reset: revert/);
  expect(screen.queryByRole("button", { name: "Use suggested order" })).not.toBeInTheDocument();
});

test("a saved order that needs more resets says so, and Use suggested order calls clear_order", async () => {
  const { calls, onClose } = mount({ ...PLAN, saved: true, counts: [3, 1] });
  expect(await screen.findByText("this order needs 3 resets; the suggested order needs 1")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Use suggested order" }));
  await waitFor(() => expect(onClose).toHaveBeenCalled());
  expect(calls.find((c) => c.cmd === "auto_run_clear_order")?.args).toEqual({ pbiId: 42 });
});

test("reset lines wait for a save once the list has been moved", async () => {
  mount();
  await screen.findByText(/Reset: revert/);
  fireEvent.click(screen.getByRole("button", { name: "Move #3 up" }));
  expect(screen.queryByText(/Reset: revert/)).not.toBeInTheDocument();
  expect(screen.getByText("Reset points are worked out again once the order is saved.")).toBeInTheDocument();
});

test("a case whose title is unknown falls back to its number alone", async () => {
  mount({ ...PLAN, resets: [{ before_case_id: 2, names: ["Cycle"], changed_by: [["Cycle", [99]]] }] });
  expect(await screen.findByText('Reset: revert "Cycle" (changed by #99)')).toBeInTheDocument();
});
