// A run reviewed under a different PBI must never be offered for review
// from a screen currently showing another PBI - the review dialog would
// send it with the wrong title and no per-step marks (every `step_ids`
// would be `[]`), silently.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import PastRuns from "./PastRuns";

afterEach(() => {
  clearMocks();
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

function renderPastRuns(runs: unknown[], pbiId: number | null) {
  mockIPC((cmd) => {
    if (cmd === "auto_run_list_runs") return runs;
    return null;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const onReview = vi.fn();
  render(
    <QueryClientProvider client={qc}>
      <PastRuns pbiId={pbiId} onReview={onReview} />
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
