// A run reviewed under a different PBI must never be offered for review
// from a screen currently showing another PBI - the review dialog would
// send it with the wrong title and no per-step marks (every `step_ids`
// would be `[]`), silently.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { toast } from "../../lib/toast";
import PastRuns from "./PastRuns";
import type { ResultFilter } from "./verdicts";

/** The screen holds the filter; this stands in for it. */
function WithFilter({ pbiId, onReview }: { pbiId: number | null; onReview: (id: string) => void }) {
  const [filter, setFilter] = useState<ResultFilter>("All");
  return <PastRuns pbiId={pbiId} onReview={onReview} filter={filter} onFilterChange={setFilter} />;
}

// The report opens in the browser; no dialog is ever involved.
const saveDialog = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: saveDialog }));
vi.mock("../../lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() } }));

afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
});

function runOf(overrides: Partial<Record<string, unknown>> = {}) {
  return {
    id: "run-1",
    pbi_id: 7,
    started_at: "1786000200000",
    mode: "unattended",
    published: null,
    cases: [{ case_id: 201, title: "Valid login", verdict: "", note: "", proposed: "Passed" }],
    ...overrides,
  };
}

function renderPastRuns(
  runs: unknown[],
  pbiId: number | null,
  onCommand?: (cmd: string, args: unknown) => unknown,
) {
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_list_runs") return runs;
    return onCommand?.(cmd, args) ?? null;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const onReview = vi.fn();
  render(
    <QueryClientProvider client={qc}>
      <WithFilter pbiId={pbiId} onReview={onReview} />
    </QueryClientProvider>,
  );
  return { onReview };
}

test("a run for another PBI shows which PBI it belongs to and offers no Review button", async () => {
  renderPastRuns([runOf({ pbi_id: 7 })], 42);

  expect(await screen.findByText("for PBI #7")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /review/i })).not.toBeInTheDocument();
});

test("a run for the currently selected PBI still offers Review as before", async () => {
  const { onReview } = renderPastRuns([runOf({ pbi_id: 42 })], 42);

  expect(await screen.findByRole("button", { name: "Review" })).toBeInTheDocument();
  expect(screen.queryByText(/for PBI #/)).not.toBeInTheDocument();
  screen.getByRole("button", { name: "Review" }).click();
  expect(onReview).toHaveBeenCalledWith("run-1");
});

test("a supervised run of another PBI keeps its rows exactly as today", async () => {
  renderPastRuns([runOf({ pbi_id: 7, mode: "" })], 42);

  // Supervised runs never offer Review regardless of PBI, and the
  // "for PBI #N" note is only ever shown for the unattended case this
  // finding is about.
  expect(await screen.findByText("Valid login")).toBeInTheDocument();
  expect(screen.queryByText(/for PBI #/)).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /review/i })).not.toBeInTheDocument();
});

test("a run names the environment it was made in, when it has one", async () => {
  renderPastRuns([runOf({ environment: "QA" }), runOf({ id: "run-2", started_at: "1786000100000" })], 42);

  const badge = await screen.findByText("QA");
  expect(badge).toHaveAttribute("title", "The environment this run was made in");
  // The older run, saved before environments existed, shows nothing extra.
  expect(screen.getAllByText("unattended")).toHaveLength(2);
  expect(screen.getAllByText("QA")).toHaveLength(1);
});

// Each run card counts its cases by result; the filter row shows only the
// runs with at least one case in a bucket, and inside them only those cases.
const MIXED = runOf({
  id: "run-mixed",
  cases: [
    { case_id: 201, title: "Valid login", verdict: "", note: "", proposed: "Passed" },
    // Confirmed Failed over a Passed proposal: counts as Failed.
    { case_id: 202, title: "Locked account", verdict: "Failed", note: "", proposed: "Passed" },
    { case_id: 203, title: "Password reset", verdict: "", note: "", proposed: "" },
  ],
});
const ALL_PASSED = runOf({
  id: "run-green",
  started_at: "1786000100000",
  cases: [{ case_id: 301, title: "Logout", verdict: "Passed", note: "" }],
});

test("a run card shows its counts by result, coloured", async () => {
  renderPastRuns([MIXED], 42);

  const results = await screen.findByRole("group", { name: "Results" });
  expect(within(results).getByText("1 passed")).toHaveClass("text-success");
  expect(within(results).getByText("1 failed")).toHaveClass("text-danger");
  expect(within(results).getByText("1 not run")).toHaveClass("text-faint");
  // A bucket with nothing in it is left out.
  expect(within(results).queryByText(/blocked/)).not.toBeInTheDocument();
});

test("the filter shows only runs with a case in that bucket, and only those cases", async () => {
  renderPastRuns([MIXED, ALL_PASSED], 42);
  const row = await screen.findByRole("group", { name: "Filter by result" });

  // Counted in runs: both have a Passed case, only one a Failed one.
  expect(within(row).getByRole("button", { name: "All (2)" })).toHaveAttribute("aria-pressed", "true");
  expect(within(row).getByRole("button", { name: "Passed (2)" })).toBeInTheDocument();
  expect(within(row).getByRole("button", { name: "Failed (1)" })).toBeInTheDocument();
  expect(within(row).getByRole("button", { name: "Blocked (0)" })).toBeInTheDocument();
  expect(within(row).getByRole("button", { name: "Not run (1)" })).toBeInTheDocument();

  fireEvent.click(within(row).getByRole("button", { name: "Failed (1)" }));
  expect(screen.getByText("Locked account")).toBeInTheDocument();
  expect(screen.queryByText("Valid login")).not.toBeInTheDocument();
  expect(screen.queryByText("Logout")).not.toBeInTheDocument();
  // The card still counts its whole run.
  expect(screen.getByText("1 passed")).toBeInTheDocument();

  fireEvent.click(within(row).getByRole("button", { name: "Blocked (0)" }));
  expect(screen.getByText("No run on this machine has a case with that result.")).toBeInTheDocument();

  fireEvent.click(within(row).getByRole("button", { name: "All (2)" }));
  expect(screen.getByText("Logout")).toBeInTheDocument();
  expect(screen.getByText("Valid login")).toBeInTheDocument();
});

// Report: one run opened as a page in the browser, written and opened by Rust.
test("Report opens the run's report in the browser and says so", async () => {
  const calls: unknown[] = [];
  renderPastRuns([runOf({ pbi_id: 42 })], 42, (cmd, args) => {
    if (cmd === "auto_run_open_report") {
      calls.push(args);
      return null;
    }
    return null;
  });

  fireEvent.click(await screen.findByRole("button", { name: /save a report of the run/i }));

  await waitFor(() => expect(calls).toHaveLength(1));
  expect(calls[0]).toEqual({ runId: "run-1", ranAt: new Date(1786000200000).toLocaleString() });
  await waitFor(() => expect(toast.success).toHaveBeenCalledWith("Report opened in your browser"));
  // Nothing is saved anywhere the person has to pick.
  expect(saveDialog).not.toHaveBeenCalled();
  expect(toast.error).not.toHaveBeenCalled();
});

test("a report that cannot be opened shows the error toast", async () => {
  renderPastRuns([runOf({ pbi_id: 42 })], 42, (cmd) => {
    if (cmd === "auto_run_open_report") {
      throw "the report could not be opened in your browser - see Settings, Logs";
    }
    return null;
  });

  fireEvent.click(await screen.findByRole("button", { name: /save a report of the run/i }));

  await waitFor(() =>
    expect(toast.error).toHaveBeenCalledWith(
      "Could not open the report: the report could not be opened in your browser - see Settings, Logs",
    ),
  );
  expect(toast.success).not.toHaveBeenCalled();
  expect(saveDialog).not.toHaveBeenCalled();
});

test("Past runs' filter buttons say they count runs, not cases", async () => {
  renderPastRuns([MIXED, ALL_PASSED], 42);
  const row = await screen.findByRole("group", { name: "Filter by result" });
  expect(within(row).getByRole("button", { name: "Failed (1)" })).toHaveAttribute("title", "Runs with a failed case");
  expect(within(row).getByRole("button", { name: "Not run (1)" })).toHaveAttribute(
    "title",
    "Runs with a case that was not run",
  );
  expect(within(row).getByRole("button", { name: "All (2)" })).toHaveAttribute("title", "Every run on this machine");
});
