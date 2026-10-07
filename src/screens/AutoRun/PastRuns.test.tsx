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
function WithFilter({
  pbiId,
  onReview,
  onReplay,
}: {
  pbiId: number | null;
  onReview: (id: string) => void;
  onReplay: (caseId: number, title: string, step: number) => void;
}) {
  const [filter, setFilter] = useState<ResultFilter>("All");
  return (
    <PastRuns pbiId={pbiId} onReview={onReview} onReplay={onReplay} filter={filter} onFilterChange={setFilter} />
  );
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
  const onReplay = vi.fn();
  render(
    <QueryClientProvider client={qc}>
      <WithFilter pbiId={pbiId} onReview={onReview} onReplay={onReplay} />
    </QueryClientProvider>,
  );
  return { onReview, onReplay };
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

  // Badge text sits in its centring span; the badge is its parent.
  const badge = (await screen.findByText("QA")).parentElement!;
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

  fireEvent.click(await screen.findByRole("button", { name: /open a report of the run/i }));

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

  fireEvent.click(await screen.findByRole("button", { name: /open a report of the run/i }));

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

test("a case run a second time after a transient failure is labelled Retried", async () => {
  const first = "step 1: GET /hr/api/cycles answered 503, expected 200";
  renderPastRuns(
    [
      runOf({
        cases: [
          { case_id: 201, title: "Valid login", verdict: "", note: "", proposed: "Passed", retried: first },
          { case_id: 202, title: "Locked account", verdict: "", note: "", proposed: "Passed" },
        ],
      }),
    ],
    7,
  );

  const retried = await screen.findByRole("listitem", { name: "Run of Valid login" });
  const label = within(retried).getByText("Retried").parentElement!;
  expect(label).toHaveAttribute("title", `Run a second time after a transient failure: ${first}`);
  expect(label).toHaveClass("text-warning");
  const plain = screen.getByRole("listitem", { name: "Run of Locked account" });
  expect(within(plain).queryByText("Retried")).not.toBeInTheDocument();
});

test("a case whose script flagged page errors carries the count", async () => {
  renderPastRuns(
    [
      runOf({
        cases: [
          { case_id: 201, title: "Valid login", verdict: "", note: "", proposed: "Passed", page_errors_seen: 3 },
          { case_id: 202, title: "Locked account", verdict: "", note: "", proposed: "Passed" },
        ],
      }),
    ],
    7,
  );

  const flagged = await screen.findByRole("listitem", { name: "Run of Valid login" });
  const label = within(flagged).getByText("page errors seen: 3").parentElement!;
  expect(label).toHaveClass("text-warning");
  expect(label).toHaveAttribute("title", expect.stringContaining("5xx"));
  const plain = screen.getByRole("listitem", { name: "Run of Locked account" });
  expect(within(plain).queryByText(/page errors seen/)).not.toBeInTheDocument();
});

test("a case whose preconditions were not checked is labelled Not checked, with the sentence", async () => {
  const notice = "preconditions were not checked: Database Read Access is off on the AI Bridge tab";
  renderPastRuns(
    [
      runOf({
        cases: [
          { case_id: 201, title: "Valid login", verdict: "", note: "", proposed: "Passed", notice },
          { case_id: 202, title: "Locked account", verdict: "", note: "", proposed: "Passed" },
        ],
      }),
    ],
    7,
  );

  const unchecked = await screen.findByRole("listitem", { name: "Run of Valid login" });
  const label = within(unchecked).getByText("Not checked").parentElement!;
  expect(label).toHaveAttribute("title", notice);
  expect(label).toHaveClass("text-warning");
  const plain = screen.getByRole("listitem", { name: "Run of Locked account" });
  expect(within(plain).queryByText("Not checked")).not.toBeInTheDocument();
});

test("a case with downloads lists each with its size and opens it by name", async () => {
  const opened: unknown[] = [];
  renderPastRuns(
    [
      runOf({
        pbi_id: 42,
        cases: [
          {
            case_id: 201,
            title: "Valid login",
            verdict: "",
            note: "",
            proposed: "Passed",
            steps: [
              { step_number: 1, outcomes: [], downloads: ["Template.xlsx"] },
              { step_number: 2, outcomes: [], downloads: ["errors.csv"] },
            ],
          },
          { case_id: 202, title: "No files", verdict: "", note: "", proposed: "Passed", steps: [] },
        ],
      }),
    ],
    42,
    (cmd, args) => {
      if (cmd === "auto_run_download_sizes") return [{ name: "Template.xlsx", size: 5427 }];
      if (cmd === "auto_run_open_download") {
        opened.push(args);
        return null;
      }
      return undefined;
    },
  );

  const row = await screen.findByRole("listitem", { name: "Run of Valid login" });
  const list = within(row).getByRole("list", { name: "Downloads" });
  expect(within(list).getByText("Template.xlsx")).toBeInTheDocument();
  expect(await within(list).findByText("5.3 KB")).toBeInTheDocument();
  // Gone from the folder: said so, and it cannot be opened.
  expect(within(list).getByText("no longer on this machine")).toBeInTheDocument();
  expect(within(list).getByRole("button", { name: "Open errors.csv" })).toBeDisabled();

  fireEvent.click(within(list).getByRole("button", { name: "Open Template.xlsx" }));
  await waitFor(() => expect(opened).toEqual([{ runId: "run-1", name: "Template.xlsx" }]));

  const other = screen.getByRole("listitem", { name: "Run of No files" });
  expect(within(other).queryByRole("list", { name: "Downloads" })).not.toBeInTheDocument();
});

test("a download Rust refuses to open says why", async () => {
  renderPastRuns(
    [
      runOf({
        cases: [
          {
            case_id: 201,
            title: "Valid login",
            verdict: "",
            note: "",
            proposed: "Passed",
            steps: [{ step_number: 1, outcomes: [], downloads: ["a.csv"] }],
          },
        ],
      }),
    ],
    7,
    (cmd) => {
      if (cmd === "auto_run_download_sizes") return [{ name: "a.csv", size: 12 }];
      if (cmd === "auto_run_open_download") throw "that file is not one of this run's downloads";
      return undefined;
    },
  );

  fireEvent.click(await screen.findByRole("button", { name: "Open a.csv" }));
  await waitFor(() =>
    expect(toast.error).toHaveBeenCalledWith("Could not open a.csv: that file is not one of this run's downloads"),
  );
});

/** A failed case whose step 2 failed after step 1 passed, and a passed one. */
const FAILED_AT_2 = runOf({
  pbi_id: 42,
  cases: [
    {
      case_id: 201,
      title: "Valid login",
      verdict: "",
      note: "",
      proposed: "Failed",
      steps: [
        { step_number: 0, outcomes: [{ ok: true, detail: "signed in as tester1" }] },
        { step_number: 1, outcomes: [{ ok: true, detail: "clicked Login" }] },
        { step_number: 2, outcomes: [{ ok: false, detail: 'button "Save" not found' }] },
        { step_number: 3, outcomes: [{ ok: false, detail: "not run: an earlier step of this case failed" }] },
      ],
    },
    {
      case_id: 202,
      title: "Locked account",
      verdict: "Passed",
      note: "",
      proposed: "Passed",
      steps: [{ step_number: 1, outcomes: [{ ok: true, detail: "page contains Locked out" }] }],
    },
    {
      case_id: 203,
      title: "Blocked before it began",
      verdict: "",
      note: "",
      proposed: "Blocked",
      reason: "precondition not met",
      steps: [],
    },
  ],
});

test("a case with a failed step offers Replay to that step; one without a known failing step does not", async () => {
  const { onReplay } = renderPastRuns([FAILED_AT_2], 42);

  const failed = await screen.findByRole("listitem", { name: "Run of Valid login" });
  const replay = within(failed).getByRole("button", { name: "Replay to step 2 for case 201" });
  expect(replay).toHaveTextContent("Replay to step 2");

  expect(
    within(screen.getByRole("listitem", { name: "Run of Locked account" })).queryByRole("button", { name: /Replay/ }),
  ).not.toBeInTheDocument();
  expect(
    within(screen.getByRole("listitem", { name: "Run of Blocked before it began" })).queryByRole("button", {
      name: /Replay/,
    }),
  ).not.toBeInTheDocument();

  fireEvent.click(replay);
  expect(onReplay).toHaveBeenCalledWith(201, "Valid login", 2);
});

test("a run for another PBI offers no Replay", async () => {
  renderPastRuns([{ ...FAILED_AT_2, pbi_id: 7 }], 42);

  expect(await screen.findByText("for PBI #7")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /Replay/ })).not.toBeInTheDocument();
});

test("a case the person called Passed offers no Replay, whatever its steps say", async () => {
  renderPastRuns(
    [
      {
        ...FAILED_AT_2,
        cases: [{ ...FAILED_AT_2.cases[0], verdict: "Passed" }],
      },
    ],
    42,
  );

  await screen.findByRole("listitem", { name: "Run of Valid login" });
  expect(screen.queryByRole("button", { name: /Replay/ })).not.toBeInTheDocument();
});

test("a run that stopped at a reset point shows the reset line between its cases", async () => {
  renderPastRuns(
    [
      runOf({
        cases: [
          { case_id: 201, title: "Publish the cycle", verdict: "", note: "", proposed: "Passed" },
          { case_id: 202, title: "Edit a draft cycle", verdict: "", note: "", proposed: "", reason: "not run: the run stopped at a reset point" },
        ],
        resets: [{ before_case_id: 202, names: ["cycle published", "rota"], waited_ms: 5, outcome: "stopped" }],
      }),
    ],
    7,
  );
  const line = await screen.findByText('Reset: revert "cycle published" - stopped');
  expect(screen.getByText('Reset: revert "rota" - stopped')).toBeInTheDocument();
  const items = Array.from(line.closest("ul")!.children);
  const at = items.indexOf(line);
  expect(items.indexOf(screen.getByRole("listitem", { name: "Run of Publish the cycle" }))).toBeLessThan(at);
  expect(items.indexOf(screen.getByRole("listitem", { name: "Run of Edit a draft cycle" }))).toBe(at + 2);
});

test("a run with no reset points shows no reset line", async () => {
  renderPastRuns([runOf()], 7);
  await screen.findByText("Valid login");
  expect(screen.queryByText(/Reset: revert/)).not.toBeInTheDocument();
});
