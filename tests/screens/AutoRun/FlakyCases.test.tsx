// The Flaky cases list on Past runs: rows from `auto_run_flaky_cases`, a toggle that marks or clears a case,
// and nothing at all when no case is flaky.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import FlakyCases from "../../../src/screens/AutoRun/FlakyCases";

vi.mock("../../../src/lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() } }));

afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
});

const ROW = {
  case_id: 201,
  title: "Valid login",
  environment: "QA",
  pattern: [true, false, true],
  last_failure: 'button "Save" not found',
  last_failed_at: "1786000200000",
  marked: false,
};

/** The section starts folded; its header is the one button that says "Flaky cases". */
async function openList() {
  fireEvent.click(await screen.findByRole("button", { name: /Flaky cases/ }));
}

function renderList(cases: unknown[], onCommand?: (cmd: string, args: unknown) => unknown) {
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_flaky_cases") return cases;
    return onCommand?.(String(cmd), args) ?? null;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <FlakyCases />
    </QueryClientProvider>,
  );
}

test("shows nothing when no case is flaky", async () => {
  renderList([]);
  await new Promise((r) => setTimeout(r, 20));
  expect(screen.queryByText("Flaky cases")).toBeNull();
});

test("a row shows the case, its environment, the run-by-run pattern and the last failure", async () => {
  renderList([ROW]);
  // The section starts folded, so it is opened first.
  await openList();
  const row = await screen.findByRole("listitem", { name: "Flaky case Valid login" });
  expect(within(row).getByText("#201")).toBeInTheDocument();
  expect(within(row).getByText("QA")).toBeInTheDocument();
  expect(within(row).getByRole("img", { name: "Recent runs, oldest first: passed, failed, passed" })).toBeInTheDocument();
  const failure = within(row).getByText('button "Save" not found');
  expect(failure).toHaveAttribute("title", 'button "Save" not found');
  expect(within(row).getByRole("button", { name: "Mark case 201 as flaky" })).toHaveTextContent("Mark flaky");
});

test("marking a case calls the set command and refreshes the list", async () => {
  let marked = false;
  const calls: unknown[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_flaky_cases") return [{ ...ROW, marked }];
    if (cmd === "auto_run_set_flaky") {
      calls.push(args);
      marked = (args as { flaky: boolean }).flaky;
    }
    return null;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <FlakyCases />
    </QueryClientProvider>,
  );

  // The section starts folded, so it is opened first.
  await openList();
  fireEvent.click(await screen.findByRole("button", { name: "Mark case 201 as flaky" }));
  await waitFor(() => expect(calls).toEqual([{ caseId: 201, flaky: true }]));
  const unmark = await screen.findByRole("button", { name: "Unmark case 201 as flaky" });
  expect(unmark).toHaveTextContent("Flaky");

  fireEvent.click(unmark);
  await waitFor(() => expect(calls).toEqual([{ caseId: 201, flaky: true }, { caseId: 201, flaky: false }]));
  expect(await screen.findByRole("button", { name: "Mark case 201 as flaky" })).toBeInTheDocument();
});

test("a marked case that no longer alternates is listed and can be unmarked", async () => {
  const calls: unknown[] = [];
  renderList([{ ...ROW, pattern: [false, false], marked: true }], (cmd, args) => {
    if (cmd === "auto_run_set_flaky") calls.push(args);
    return null;
  });
  // The section starts folded, so it is opened first.
  await openList();
  const unmark = await screen.findByRole("button", { name: "Unmark case 201 as flaky" });
  fireEvent.click(unmark);
  await waitFor(() => expect(calls).toEqual([{ caseId: 201, flaky: false }]));
});

test("the section starts folded and still says what is in it", async () => {
  renderList([ROW, { ...ROW, case_id: 202, title: "Other", marked: true }]);
  const header = await screen.findByRole("button", { name: /Flaky cases/ });
  expect(header).toHaveAttribute("aria-expanded", "false");
  expect(header).toHaveTextContent("2 passed and failed recently, 1 marked");
  expect(screen.queryByRole("listitem", { name: "Flaky case Valid login" })).not.toBeInTheDocument();
  fireEvent.click(header);
  expect(header).toHaveAttribute("aria-expanded", "true");
  expect(screen.getByRole("listitem", { name: "Flaky case Valid login" })).toBeInTheDocument();
});
