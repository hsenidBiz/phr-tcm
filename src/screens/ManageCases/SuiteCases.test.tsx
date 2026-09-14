import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { toast } from "sonner";
import type { SuiteCase } from "../../lib/suiteOrder";
import SuiteCases from "./SuiteCases";

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));
const { openDialog } = vi.hoisted(() => ({ openDialog: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: openDialog }));

afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.clearAllMocks();
});

const ENTRIES = [
  { id: 95, sequence_number: 0, entry_type: "suite" },
  { id: 201, sequence_number: 1, entry_type: "testCase" },
  { id: 202, sequence_number: 2, entry_type: "testCase" },
  { id: 203, sequence_number: 3, entry_type: "testCase" },
];
const point = (id: number, name: string, config = "Windows 10") => ({
  point_id: id * 10, test_case_id: id, test_case_name: name, config_name: config,
  tester: "", last_outcome: "none", last_run_id: null, last_result_id: null,
});

function Harness({ onToggle }: { onToggle?: (cases: SuiteCase[], on: boolean) => void }) {
  const [selected, setSelected] = useState<Set<number>>(new Set());
  return (
    <SuiteCases
      org="acme"
      project="Web"
      planId={9}
      suiteId={91}
      suiteName="Regression"
      selected={selected}
      onToggle={(cases, on) => {
        onToggle?.(cases, on);
        setSelected((s) => {
          const next = new Set(s);
          for (const c of cases) on ? next.add(c.id) : next.delete(c.id);
          return next;
        });
      }}
    />
  );
}

function mount(extra: (cmd: string, args: unknown) => unknown = () => undefined, onToggle?: (c: SuiteCase[], on: boolean) => void) {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "list_suite_entries") return ENTRIES;
    if (cmd === "list_test_points")
      return [point(201, "Valid login"), point(201, "Valid login", "Windows 11"), point(202, "Bad password"), point(203, "Locked out")];
    return extra(cmd, args);
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const { unmount } = render(
    <QueryClientProvider client={qc}>
      <Harness onToggle={onToggle} />
    </QueryClientProvider>,
  );
  return { calls, unmount };
}

async function list() {
  const l = await screen.findByRole("list", { name: "Test cases in Regression" });
  // Let the passive effects that follow the first paint (order mirror,
  // mutation option sync) settle before a caller fires an action.
  await new Promise((r) => setTimeout(r, 0));
  return l;
}

test("lists the suite's cases once each in entry order; nothing to apply at first", async () => {
  mount();
  const l = await list();
  const rows = within(l).getAllByRole("listitem");
  expect(rows).toHaveLength(3);
  expect(rows[0]).toHaveTextContent("#201");
  expect(rows[0]).toHaveTextContent("Valid login");
  expect(rows[2]).toHaveTextContent("#203");
  expect(within(l).queryByText(/#95/)).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Apply order" })).toBeDisabled();
});

test("drag re-orders; Apply order sends the ids, disables at once, and reloads", async () => {
  const { calls } = mount((cmd) => {
    if (cmd === "reorder_suite_cases") return [203, 201, 202];
  });
  const l = await list();
  const rows = within(l).getAllByRole("listitem");
  fireEvent.dragStart(rows[2]);
  fireEvent.dragOver(rows[0]);
  fireEvent.drop(rows[0]);
  expect(within(l).getAllByRole("listitem")[0]).toHaveTextContent("#203");
  // Dirty now, so the sticky bar has joined the inline row's own Apply
  // order button - take the inline one (it renders first).
  const apply = screen.getAllByRole("button", { name: "Apply order" })[0];
  expect(apply).toBeEnabled();
  fireEvent.click(apply);
  await waitFor(() => {
    const call = calls.find((c) => c.cmd === "reorder_suite_cases");
    expect(call?.args).toEqual({ organization: "acme", project: "Web", suiteId: 91, caseIds: [203, 201, 202] });
  });
  await waitFor(() => expect(apply).toBeDisabled());
  await waitFor(() => expect(calls.filter((c) => c.cmd === "list_suite_entries").length).toBeGreaterThan(1));
  expect(toast.success).toHaveBeenCalledWith("Order saved.");
});

test("Move up and down step a row; Reset restores the server order", async () => {
  mount();
  const l = await list();
  fireEvent.click(within(l).getByRole("button", { name: "Move #202 up" }));
  expect(within(l).getAllByRole("listitem")[0]).toHaveTextContent("#202");
  fireEvent.click(within(l).getByRole("button", { name: "Move #202 down" }));
  expect(within(l).getAllByRole("listitem")[0]).toHaveTextContent("#201");
  fireEvent.click(within(l).getByRole("button", { name: "Move #201 down" }));
  // Dirty now, so the sticky bar has joined the inline row's own buttons -
  // take the inline ones (they render first).
  expect(screen.getAllByRole("button", { name: "Apply order" })[0]).toBeEnabled();
  fireEvent.click(screen.getAllByRole("button", { name: "Reset" })[0]);
  expect(within(l).getAllByRole("listitem")[0]).toHaveTextContent("#201");
  expect(screen.getByRole("button", { name: "Apply order" })).toBeDisabled();
});

test("the sticky bar appears only while the order is unsaved, and names its suite", async () => {
  mount();
  const l = await list();
  // Clean: the inline row is there, the sticky bar is not.
  expect(screen.queryByRole("region", { name: /unsaved order/i })).not.toBeInTheDocument();

  fireEvent.click(within(l).getByRole("button", { name: "Move #202 up" }));

  const bar = await screen.findByRole("region", { name: /unsaved order/i });
  expect(within(bar).getByRole("button", { name: "Apply order" })).toBeEnabled();
  expect(within(bar).getByRole("button", { name: "Reset" })).toBeEnabled();
  expect(bar).toHaveTextContent("Regression");

  fireEvent.click(within(bar).getByRole("button", { name: "Reset" }));
  await waitFor(() => expect(screen.queryByRole("region", { name: /unsaved order/i })).not.toBeInTheDocument());
});

test("Apply tester order from file re-orders from the file's tester_order", async () => {
  openDialog.mockResolvedValue("C:\\drafts\\auth.json");
  const base = { steps: [], tags: "", automation_status: "Not Automated", module_value: "", preconditions: "" };
  mount((cmd) => {
    if (cmd === "parse_import_file")
      return { cases: [{ ...base, title: "c", update_id: 203, tester_order: 1 }, { ...base, title: "a", update_id: 201, tester_order: 2 }], warnings: [] };
  });
  const l = await list();
  fireEvent.click(screen.getByRole("button", { name: "Apply tester order from file" }));
  await waitFor(() => expect(within(l).getAllByRole("listitem")[0]).toHaveTextContent("#203"));
  expect(within(l).getAllByRole("listitem")[1]).toHaveTextContent("#201");
  expect(toast.info).toHaveBeenCalledWith("Placed 2 of 3 test cases from the file. Apply order to save.");
  // Dirty now, so the sticky bar has joined the inline row's own Apply
  // order button - take the inline one (it renders first).
  expect(screen.getAllByRole("button", { name: "Apply order" })[0]).toBeEnabled();
});

test("a file naming none of the suite's cases changes nothing; cancelling the picker does nothing", async () => {
  openDialog.mockResolvedValueOnce("C:\\drafts\\other.json").mockResolvedValueOnce(null);
  const { calls } = mount((cmd) => {
    if (cmd === "parse_import_file")
      return { cases: [{ title: "x", steps: [], tags: "", automation_status: "Not Automated", module_value: "", preconditions: "", update_id: null, tester_order: 1 }], warnings: [] };
  });
  const l = await list();
  fireEvent.click(screen.getByRole("button", { name: "Apply tester order from file" }));
  await waitFor(() =>
    expect(toast.warning).toHaveBeenCalledWith("No test case in that file is in this suite. The file needs ids from an upload."),
  );
  expect(within(l).getAllByRole("listitem")[0]).toHaveTextContent("#201");
  const parses = calls.filter((c) => c.cmd === "parse_import_file").length;
  fireEvent.click(screen.getByRole("button", { name: "Apply tester order from file" }));
  await waitFor(() => expect(openDialog).toHaveBeenCalledTimes(2));
  expect(calls.filter((c) => c.cmd === "parse_import_file").length).toBe(parses);
});

test("checking a row reports the case to the parent; unchecking reports it back", async () => {
  const toggles: Array<[number[], boolean]> = [];
  mount(undefined, (cases, on) => toggles.push([cases.map((c) => c.id), on]));
  const l = await list();
  fireEvent.click(within(l).getByRole("checkbox", { name: "Select #202" }));
  expect(toggles[toggles.length - 1]).toEqual([[202], true]);
  // The "select all" checkbox is CaseOrderList's header row, a sibling of
  // the <ol> rather than one of its <li>s, so it is outside `l`'s scope.
  fireEvent.click(screen.getByRole("checkbox", { name: "Select all test cases" }));
  expect(toggles[toggles.length - 1]).toEqual([[201, 203], true]);
  fireEvent.click(within(l).getByRole("checkbox", { name: "Select #202" }));
  expect(toggles[toggles.length - 1]).toEqual([[202], false]);
});

test("a suite already read once paints from the cache instead of loading again", async () => {
  // First visit: the two reads happen and the rows appear.
  const first = mount();
  expect(await screen.findByText(/Case 201|#201/)).toBeInTheDocument();
  const readsFirst = first.calls.filter((c) => c.cmd === "list_suite_entries").length;
  expect(readsFirst).toBe(1);
  first.unmount();

  // Second visit, same suite: rows are there in the first paint, with no
  // "Loading test cases" and no fresh read.
  const second = mount();
  expect(screen.getByText(/Case 201|#201/)).toBeInTheDocument();
  expect(screen.queryByText("Loading test cases")).not.toBeInTheDocument();
  expect(second.calls.filter((c) => c.cmd === "list_suite_entries")).toHaveLength(0);
});

test("an empty suite says so", async () => {
  mockIPC((cmd) => {
    if (cmd === "list_suite_entries") return [];
    if (cmd === "list_test_points") return [];
    return undefined;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <Harness />
    </QueryClientProvider>,
  );
  expect(await screen.findByText("No test cases in this suite.")).toBeInTheDocument();
});
