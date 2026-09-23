import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { toast } from "sonner";
import type { SuiteCase } from "../../lib/suiteOrder";
import SuggestedOrder from "./SuggestedOrder";

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.clearAllMocks();
});

const CASES: SuiteCase[] = [
  { id: 201, title: "Valid login" },
  { id: 202, title: "Bad password" },
  { id: 203, title: "Locked out" },
];

/** Whatever locale/timezone this run uses - matches the component's own
 * `toLocaleDateString()` call exactly, so the assertion never hard-codes a
 * format that only holds on one machine. */
const localDate = (iso: string) => new Date(iso).toLocaleDateString();

const FILE = (cases: Array<{ id: number; group?: string }>) => ({
  format: "tcm-run-order",
  version: 1,
  saved_by: "lead@example.com",
  saved_at: "2026-09-23T10:15:00Z",
  cases,
});

function mount(
  extra: (cmd: string, args: unknown) => unknown = () => undefined,
  cases: SuiteCase[] = CASES,
) {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    const answered = extra(cmd, args);
    if (answered !== undefined) return answered;
    if (cmd === "get_run_order") return { state: "none" };
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <SuggestedOrder org="acme" project="Web" planId={9} suiteId={91} pbiId={42} suiteName="Regression" cases={cases} />
    </QueryClientProvider>,
  );
  return { calls, qc };
}

const orderList = () => screen.findByRole("list", { name: "Suggested run order for Regression" });
const startFromPicker = () => screen.getByRole("combobox", { name: "Start from" });

/** The list's case ids top to bottom, in DOM order. */
async function idsOnScreen() {
  const l = await orderList();
  return within(l)
    .getAllByRole("listitem")
    .map((li) => li.textContent ?? "")
    .map((t) => Number(/#(\d+)/.exec(t)?.[1]))
    .filter((n) => Number.isFinite(n));
}

test("default start is the saved order", async () => {
  mount((cmd) => {
    if (cmd === "get_run_order") return { state: "found", file: FILE([{ id: 203 }, { id: 201 }, { id: 202 }]) };
  });
  await orderList();
  await vi.waitFor(() => expect(startFromPicker()).toHaveTextContent("Saved suggested order"));
  expect(await idsOnScreen()).toEqual([203, 201, 202]);
  expect(
    await screen.findByText(`Saved by lead@example.com on ${localDate("2026-09-23T10:15:00Z")}`),
  ).toBeInTheDocument();
});

test("with no saved order the default is Azure DevOps order", async () => {
  mount();
  await orderList();
  expect(startFromPicker()).toHaveTextContent("Order in Azure DevOps");
  expect(await idsOnScreen()).toEqual([201, 202, 203]);
  expect(await screen.findByText("No suggested run order yet.")).toBeInTheDocument();
  fireEvent.click(startFromPicker());
  expect(screen.queryByRole("option", { name: "Saved suggested order" })).not.toBeInTheDocument();
  expect(screen.queryByRole("option", { name: "My order on this machine" })).not.toBeInTheDocument();
});

test("Start from My order uses the local list, reconciled against the suite's cases", async () => {
  localStorage.setItem("tcm-v2-run-order:acme/9/91", JSON.stringify([203, 201, 999]));
  mount((cmd) => {
    if (cmd === "get_run_order") return { state: "found", file: FILE([{ id: 202 }, { id: 201 }, { id: 203 }]) };
  });
  await orderList();
  fireEvent.click(startFromPicker());
  fireEvent.click(screen.getByRole("option", { name: "My order on this machine" }));
  // 999 is not one of this suite's cases and drops out; 202 was not in "my
  // order" and lands at the end, in spec order.
  expect(await idsOnScreen()).toEqual([203, 201, 202]);
});

test("Save opens the modal, Cancel sends nothing", async () => {
  const { calls } = mount();
  await orderList();
  const list = await orderList();
  fireEvent.click(within(list).getByRole("button", { name: "Move #202 up" }));
  fireEvent.click(screen.getByRole("button", { name: "Save suggested order" }));
  expect(
    screen.getByText("Every tester will see this as the suggested run order for this PBI."),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(screen.queryByText("Every tester will see this as the suggested run order for this PBI.")).not.toBeInTheDocument();
  expect(calls.some((c) => c.cmd === "save_run_order")).toBe(false);
});

test("Save sends the edited order, carries groups over, toasts, and updates the saved-by line", async () => {
  const { calls } = mount((cmd) => {
    // 201 carries a group in the currently-saved file; 202 and 203 do not.
    if (cmd === "get_run_order") return { state: "found", file: FILE([{ id: 201, group: "Auth" }, { id: 202 }, { id: 203 }]) };
    if (cmd === "save_run_order") return FILE([{ id: 202 }, { id: 201, group: "Auth" }, { id: 203 }]);
  });
  const list = await orderList();
  // Wait for the read to settle before editing: the default only applies
  // once, but only after that first settle - editing any earlier would race
  // it.
  await vi.waitFor(() => expect(startFromPicker()).toHaveTextContent("Saved suggested order"));
  // Move #202 up, past #201: [202, 201, 203].
  fireEvent.click(within(list).getByRole("button", { name: "Move #202 up" }));
  expect(await idsOnScreen()).toEqual([202, 201, 203]);
  fireEvent.click(screen.getByRole("button", { name: "Save suggested order" }));
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await vi.waitFor(() => expect(toast.success).toHaveBeenCalledWith("Suggested run order saved."));
  const saveCall = calls.find((c) => c.cmd === "save_run_order");
  // The edited order (202 first), with 201's group carried over from the
  // saved file and no group invented for 202 or 203.
  expect(saveCall?.args).toEqual({
    organization: "acme",
    project: "Web",
    pbiId: 42,
    cases: [
      { id: 202 },
      { id: 201, group: "Auth" },
      { id: 203 },
    ],
  });
  await screen.findByText(`Saved by lead@example.com on ${localDate("2026-09-23T10:15:00Z")}`);
});

test("an unreadable file shows its sentence and still allows Save", async () => {
  mount((cmd) => {
    if (cmd === "get_run_order") return { state: "unreadable", reason: "the run-order file is damaged" };
  });
  await orderList();
  expect(
    await screen.findByText("The suggested run order could not be read: the run-order file is damaged"),
  ).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Save suggested order" })).not.toBeDisabled();
  fireEvent.click(startFromPicker());
  expect(screen.queryByRole("option", { name: "Saved suggested order" })).not.toBeInTheDocument();
});

test("Save is disabled while the list equals the saved order", async () => {
  mount((cmd) => {
    if (cmd === "get_run_order") return { state: "found", file: FILE([{ id: 201 }, { id: 202 }, { id: 203 }]) };
  });
  await orderList();
  await vi.waitFor(() => expect(screen.getByRole("button", { name: "Save suggested order" })).toBeDisabled());
});

test("on a save error, toasts the message and keeps the modal's data untouched", async () => {
  mount((cmd) => {
    if (cmd === "save_run_order") return Promise.reject({ kind: "Network", detail: "Could not reach Azure DevOps." });
  });
  const list = await orderList();
  fireEvent.click(within(list).getByRole("button", { name: "Move #202 up" }));
  fireEvent.click(screen.getByRole("button", { name: "Save suggested order" }));
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await vi.waitFor(() =>
    expect(toast.error).toHaveBeenCalledWith("Could not save the suggested run order: Could not reach Azure DevOps."),
  );
});
