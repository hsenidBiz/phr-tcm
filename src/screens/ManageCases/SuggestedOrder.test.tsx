import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { toast } from "sonner";
import { cacheKeys, cacheWrite } from "../../lib/cache";
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

function tree(cases: SuiteCase[], qc: QueryClient) {
  return (
    <QueryClientProvider client={qc}>
      <SuggestedOrder org="acme" project="Web" planId={9} suiteId={91} pbiId={42} suiteName="Regression" cases={cases} />
    </QueryClientProvider>
  );
}

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
  const { rerender: rtlRerender } = render(tree(cases, qc));
  /** Re-render with a different `cases` prop, same query client - simulates
   * the suite's cases changing under the editor (a suite-cases refetch, or
   * an Apply order in the Azure DevOps editor). */
  const rerender = (nextCases: SuiteCase[]) => rtlRerender(tree(nextCases, qc));
  return { calls, qc, rerender };
}

/** Seeds the persistent (disk) cache the way a previous app run would have
 * left it, aged so React Query treats it as stale and refetches on mount -
 * a fresh write is within `staleMs` and would not, which would make a test
 * of "the background refetch reaches the screen" pass for the wrong reason
 * (no refetch ever happens). */
function primeDiskSeed(file: unknown, ageMs: number) {
  const key = `tcm-v2-cache:${cacheKeys.runOrder("acme", "Web", 42)}`;
  cacheWrite(cacheKeys.runOrder("acme", "Web", 42), { state: "found", file });
  const raw = JSON.parse(localStorage.getItem(key)!);
  raw.at -= ageMs;
  localStorage.setItem(key, JSON.stringify(raw));
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

test("Save sends the edited order, carries groups over, toasts, updates the cache, and disables Save", async () => {
  // Stateful, like the real backend: once saved, a later read (the
  // invalidate-triggered refetch below) sees what was just written, not
  // what was there before.
  let currentFile = FILE([{ id: 201, group: "Auth" }, { id: 202 }, { id: 203 }]);
  const NEW_SAVED = {
    format: "tcm-run-order",
    version: 1,
    saved_by: "me@example.com",
    saved_at: "2026-09-24T08:00:00Z",
    cases: [{ id: 202 }, { id: 201, group: "Auth" }, { id: 203 }],
  };
  const { calls, qc } = mount((cmd) => {
    if (cmd === "get_run_order") return { state: "found", file: currentFile };
    if (cmd === "save_run_order") {
      currentFile = NEW_SAVED;
      return NEW_SAVED;
    }
  });
  const list = await orderList();
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
  // The shared cache entry - the one Run Tests reads - carries the new file.
  await vi.waitFor(() =>
    expect(qc.getQueryData(["run-order", "acme", "Web", 42])).toEqual({ state: "found", file: NEW_SAVED }),
  );
  await screen.findByText(`Saved by me@example.com on ${localDate("2026-09-24T08:00:00Z")}`);
  // The list now equals the (just-saved) saved order.
  await vi.waitFor(() => expect(screen.getByRole("button", { name: "Save suggested order" })).toBeDisabled());
  // The invalidate after the save (paired with setQueryData, as
  // SuiteCases.tsx does for Apply order) forced a refetch that rewrote the
  // DISK copy too - otherwise Run Tests would paint the pre-save order from
  // disk on its next launch, before ever asking Azure DevOps again.
  const diskKey = `tcm-v2-cache:${cacheKeys.runOrder("acme", "Web", 42)}`;
  await vi.waitFor(() => {
    const raw = JSON.parse(localStorage.getItem(diskKey) ?? "null");
    expect(raw?.data).toEqual({ state: "found", file: NEW_SAVED });
  });
});

test("an unreadable file is worded like Run Tests' note, and still allows Save", async () => {
  mount((cmd) => {
    if (cmd === "get_run_order") return { state: "unreadable", reason: "the run-order file is damaged" };
  });
  await orderList();
  // Exactly `useRunOrder`'s `noteFor`: the brief's leading sentence, plus
  // the same trailing pointer Run Tests gives for the same reason.
  expect(
    await screen.findByText(
      "The suggested run order could not be read: the run-order file is damaged. See Settings → Logs.",
    ),
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

// ---- Fix round 1: following a newer file, and the suite's own cases ----

test("a newer saved file (e.g. a colleague's save) reaches the list while untouched", async () => {
  const OLD = FILE([{ id: 201 }, { id: 202 }, { id: 203 }]);
  primeDiskSeed(OLD, 10 * 60_000);
  const NEW = { ...FILE([{ id: 203 }, { id: 202 }, { id: 201 }]), saved_by: "other@example.com" };
  mount((cmd) => {
    if (cmd === "get_run_order") return { state: "found", file: NEW };
  });
  await orderList();
  // The disk copy (OLD) paints first; the background refetch then lands.
  await screen.findByText(`Saved by other@example.com on ${localDate(NEW.saved_at)}`);
  expect(await idsOnScreen()).toEqual([203, 202, 201]);
});

test("once touched, the list does not follow a later refetch", async () => {
  const OLD = FILE([{ id: 201 }, { id: 202 }, { id: 203 }]);
  primeDiskSeed(OLD, 10 * 60_000);
  const NEW = { ...FILE([{ id: 203 }, { id: 202 }, { id: 201 }]), saved_by: "other@example.com" };
  const { qc } = mount((cmd) => {
    if (cmd === "get_run_order") return { state: "found", file: NEW };
  });
  const list = await orderList();
  fireEvent.click(within(list).getByRole("button", { name: "Move #202 up" }));
  expect(await idsOnScreen()).toEqual([202, 201, 203]);
  // Confirm the refetch really landed in the shared cache before asserting
  // the screen ignored it - otherwise a pass could just mean it never ran.
  await vi.waitFor(() =>
    expect(qc.getQueryData(["run-order", "acme", "Web", 42])).toEqual({ state: "found", file: NEW }),
  );
  expect(await idsOnScreen()).toEqual([202, 201, 203]);
});

test("editing while the read is still pending survives once it resolves", async () => {
  let resolveRead: (value: unknown) => void = () => {};
  const pending = new Promise((resolve) => {
    resolveRead = resolve;
  });
  mount((cmd) => {
    if (cmd === "get_run_order") return pending;
  });
  const list = await orderList();
  fireEvent.click(within(list).getByRole("button", { name: "Move #202 up" }));
  expect(await idsOnScreen()).toEqual([202, 201, 203]);
  resolveRead({ state: "found", file: FILE([{ id: 203 }, { id: 202 }, { id: 201 }]) });
  // The read landing is visible in the note (never gated by `touched`); the
  // edited order must still be on screen once it has.
  await screen.findByText(`Saved by lead@example.com on ${localDate("2026-09-23T10:15:00Z")}`);
  expect(await idsOnScreen()).toEqual([202, 201, 203]);
});

test("a case added to the suite appears in the list and the save payload, while untouched", async () => {
  const { calls, rerender } = mount((cmd) => {
    if (cmd === "save_run_order") return FILE([{ id: 201 }, { id: 202 }, { id: 203 }, { id: 204 }]);
  });
  await orderList();
  // Wait for the read to settle BEFORE the suite gains a case: otherwise the
  // still-pending read's effect can pick up the new case by accident when it
  // finally fires, passing for the wrong reason.
  await screen.findByText("No suggested run order yet.");
  rerender([...CASES, { id: 204, title: "New case" }]);
  await vi.waitFor(async () => expect(await idsOnScreen()).toEqual([201, 202, 203, 204]));
  fireEvent.click(screen.getByRole("button", { name: "Save suggested order" }));
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await vi.waitFor(() => expect(toast.success).toHaveBeenCalledWith("Suggested run order saved."));
  const saveCall = calls.find((c) => c.cmd === "save_run_order");
  expect(saveCall?.args).toEqual({
    organization: "acme",
    project: "Web",
    pbiId: 42,
    cases: [{ id: 201 }, { id: 202 }, { id: 203 }, { id: 204 }],
  });
});

test("a case removed from the suite disappears from the list and the save payload, once touched", async () => {
  const FOUR: SuiteCase[] = [...CASES, { id: 204, title: "New case" }];
  const { calls, rerender } = mount((cmd) => {
    if (cmd === "save_run_order") return FILE([{ id: 202 }, { id: 201 }, { id: 203 }]);
  }, FOUR);
  const list = await orderList();
  // Touch: this is now the tester's own list, not a mirror of the read.
  fireEvent.click(within(list).getByRole("button", { name: "Move #202 up" }));
  expect(await idsOnScreen()).toEqual([202, 201, 203, 204]);
  // 204 leaves the suite (deleted, or moved elsewhere).
  rerender(CASES);
  await vi.waitFor(async () => expect(await idsOnScreen()).toEqual([202, 201, 203]));
  fireEvent.click(screen.getByRole("button", { name: "Save suggested order" }));
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await vi.waitFor(() => expect(toast.success).toHaveBeenCalledWith("Suggested run order saved."));
  const saveCall = calls.find((c) => c.cmd === "save_run_order");
  expect(saveCall?.args).toEqual({
    organization: "acme",
    project: "Web",
    pbiId: 42,
    cases: [{ id: 202 }, { id: 201 }, { id: 203 }],
  });
});
