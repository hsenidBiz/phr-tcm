// The Playwright tab's Runner: ticks remembered per clone, the action
// buttons, the output panel as it streams, Stop, and the report list.
// Rust does the run list and the count check (tests/suite/pw_repo_runner.rs);
// here they are a mocked `pw_run`.

import { emit } from "@tauri-apps/api/event";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { StrictMode } from "react";
import { afterEach, expect, test, vi } from "vitest";
import type { ListedTest, PwReport, PwRunning, RunStart, TestTree } from "../../bindings";
import Runner from "./Runner";

/** How many times a tick box has rendered: the tree's own renders. */
const boxes = vi.hoisted(() => ({ renders: 0 }));
vi.mock("../../components/ui/checkbox", async (real) => {
  const m = await real<typeof import("../../components/ui/checkbox")>();
  return {
    ...m,
    Checkbox: (props: Parameters<typeof m.Checkbox>[0]) => {
      boxes.renders += 1;
      return m.Checkbox(props);
    },
  };
});

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

const RM = "sl/admin/performance/rating-methods/specs/rating-methods.spec.ts";

function listed(title: string, over: Partial<ListedTest> = {}): ListedTest {
  const file = over.file ?? RM;
  const title_path = over.title_path ?? ["Rating Methods", title];
  return {
    id: JSON.stringify([file, ...title_path]),
    country: "sl",
    side: "admin",
    module: "performance",
    feature: "rating-methods",
    spec: "rating-methods",
    file,
    title,
    title_path,
    line: 5,
    project: "SL",
    ado_id: null,
    last_result: null,
    ...over,
  };
}

const CREATE = listed("User can create a new rating method", { ado_id: 135152, last_result: "Passed" });
const DELETE = listed("User can delete a rating method", { ado_id: 135179, last_result: "Failed", line: 7 });
const SHELL = listed("the app shell loads", {
  side: "self",
  module: "smoke",
  feature: "framework-smoke",
  spec: "framework-smoke",
  file: "sl/self/smoke/framework-smoke/specs/framework-smoke.spec.ts",
  title_path: ["the app shell loads"],
});
const SHARED = listed("finds an employee by name", {
  country: "common-components",
  side: "",
  module: "",
  feature: "employee-search",
  spec: "employee-search",
  file: "common-components/employee-search/specs/employee-search.spec.ts",
  title_path: ["finds an employee by name"],
});
const TREE: TestTree = { tests: [SHARED, CREATE, DELETE, SHELL], projects: ["SL"], last_report: null, load_errors: [] };

const OLD: PwReport = {
  run_id: "2026-10-12T16-00-00",
  stats: { passed: 3, failed: 1, flaky: 0, skipped: 0, duration_ms: 9000 },
};
const NEW: PwReport = { run_id: "2026-10-13T09-14-00", stats: null };

type Call = { cmd: string; args: Record<string, unknown> };

type Mount = {
  clone?: string;
  start?: (args: Record<string, unknown>) => RunStart | Promise<RunStart>;
  /** What `pw_repo_running` answers. */
  running?: PwRunning | null;
  /** `pw_list` throws this instead. */
  listFails?: string;
  /** What `pw_list` answers instead of TREE. */
  tree?: TestTree;
  /** `pw_reports` throws on its first call. */
  reportsFailFirst?: boolean;
  /** A client with React Query's own defaults (retries on). */
  defaults?: boolean;
};

function mount(opts: Mount = {}) {
  const calls: Call[] = [];
  let reports: PwReport[] = [OLD];
  let current: PwRunning | null = opts.running ?? null;
  let reportsCalls = 0;
  mockIPC(
    (cmd, raw) => {
      const args = (raw ?? {}) as Record<string, unknown>;
      calls.push({ cmd, args });
      if (cmd === "pw_list") {
        if (opts.listFails) throw opts.listFails;
        return opts.tree ?? TREE;
      }
      if (cmd === "pw_reports") {
        reportsCalls += 1;
        if (opts.reportsFailFirst && reportsCalls === 1) throw "the reports could not be read";
        return reports;
      }
      if (cmd === "pw_run") {
        const r = opts.start?.(args) ?? { kind: "Started", run: 7, command: "npx playwright test --project=SL", summary: "2 selected · 2 matched", warnings: [] };
        return r;
      }
      if (cmd === "pw_preview")
        return { run_list: `# header\n[SL] > ${RM}`, command: "npx playwright test --project=SL", expected: 2, warnings: [] };
      if (cmd === "pw_repo_running") return current;
      return null;
    },
    { shouldMockEvents: true },
  );
  const clone = opts.clone ?? "C:/repo";
  const client = opts.defaults ? new QueryClient() : new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(
    <StrictMode>
      <QueryClientProvider client={client}>
        <Runner key={clone} clone={clone} />
      </QueryClientProvider>
    </StrictMode>,
  );
  return Object.assign(calls, {
    unmount: view.unmount,
    addReport: (r: PwReport) => {
      reports = [r, ...reports];
    },
    setRunning: (r: PwRunning | null) => {
      current = r;
    },
  });
}

const button = (name: string | RegExp) => screen.getByRole("button", { name });
const box = (name: string) => screen.getByRole("checkbox", { name });

test("a listing Playwright could not load says why, not that there are no tests", async () => {
  const duplicate =
    'Error: duplicate test title "competencies: parse then emit is byte-identical", first declared in ..\\cli\\data\\roundtrip.test.mjs:25 (cli/data/roundtrip.test.mjs:30)';
  mount({ tree: { tests: [], projects: [], last_report: null, load_errors: [duplicate] } });
  expect(await screen.findByText("Playwright could not load the tests, so none are listed:")).toBeInTheDocument();
  expect(screen.getByText(duplicate)).toBeInTheDocument();
  expect(screen.queryByText("No tests under suites yet.")).not.toBeInTheDocument();
});

test("an empty listing with no errors still says there are no tests", async () => {
  mount({ tree: { tests: [], projects: [], last_report: null, load_errors: [] } });
  expect(await screen.findByText("No tests under suites yet.")).toBeInTheDocument();
});

test("the tree shows each test with its ADO id and last result, under its levels", async () => {
  mount();
  expect(await screen.findByText("User can create a new rating method")).toBeInTheDocument();
  for (const level of ["Common Components", "Employee Search", "SL", "Admin", "Performance", "Rating Methods", "Self", "Smoke"]) {
    expect(screen.getByText(level)).toBeInTheDocument();
  }
  expect(screen.getByText("ADO 135152")).toBeInTheDocument();
  expect(screen.getByText("passed")).toHaveClass("text-success");
  expect(screen.getByText("failed")).toHaveClass("text-danger");
  // A branch folds.
  fireEvent.click(button("Fold Rating Methods"));
  expect(screen.queryByText("User can create a new rating method")).not.toBeInTheDocument();
  fireEvent.click(button("Unfold Rating Methods"));
  expect(screen.getByText("User can create a new rating method")).toBeInTheDocument();
});

test("ticks: one test, a branch, Tick all and Clear, remembered per clone", async () => {
  const first = mount();
  await screen.findByText("User can create a new rating method");
  expect(button("Run picked (0)")).toBeDisabled();

  fireEvent.click(box("Tick the app shell loads"));
  expect(button("Run picked (1)")).toBeEnabled();
  // A branch ticks everything under it, and shows mixed when partly ticked.
  fireEvent.click(box("Tick Rating Methods"));
  expect(button("Run picked (3)")).toBeEnabled();
  expect(box("Tick SL")).toHaveAttribute("aria-checked", "true");
  fireEvent.click(box("Tick User can delete a rating method"));
  expect(box("Tick Rating Methods")).toHaveAttribute("aria-checked", "mixed");
  expect(button("Run picked (2)")).toBeEnabled();

  // Remembered: the same clone comes back with the same ticks.
  first.unmount();
  const again = mount();
  await screen.findByText("User can create a new rating method");
  expect(button("Run picked (2)")).toBeEnabled();
  expect(box("Tick the app shell loads")).toHaveAttribute("aria-checked", "true");
  again.unmount();

  // Another clone has its own.
  const other = mount({ clone: "D:/other" });
  await screen.findByText("User can create a new rating method");
  expect(button("Run picked (0)")).toBeDisabled();
  fireEvent.click(button("Tick all"));
  expect(button("Run picked (4)")).toBeEnabled();
  fireEvent.click(button("Clear"));
  expect(button("Run picked (0)")).toBeDisabled();
  other.unmount();
});

test("a run sends the ticks and the mode, and every action waits while it runs", async () => {
  const calls = mount();
  await screen.findByText("User can create a new rating method");
  fireEvent.click(box("Tick Rating Methods"));
  fireEvent.click(screen.getByRole("combobox", { name: "Browser" }));
  fireEvent.click(screen.getByRole("option", { name: "Browser shown" }));
  expect(button("Stop")).toBeDisabled();

  fireEvent.click(button("Run picked (2)"));
  await waitFor(() => expect(calls.some((c) => c.cmd === "pw_run")).toBe(true));
  const run = calls.find((c) => c.cmd === "pw_run")!;
  expect(run.args).toEqual({ clone: "C:/repo", picked: [CREATE.id, DELETE.id], action: "Picked", mode: "Shown", confirm: false });

  for (const name of [/^Run picked/, "Run all", "Re-run failures", "Preview", "List again"]) {
    await waitFor(() => expect(button(name)).toBeDisabled());
  }
  expect(button("Stop")).toBeEnabled();
});

test("the output panel streams the run's lines after its command and count", async () => {
  mount();
  await screen.findByText("User can create a new rating method");
  fireEvent.click(box("Tick Rating Methods"));
  fireEvent.click(button("Run picked (2)"));
  const log = await screen.findByRole("log", { name: "Run output" });
  await waitFor(() => expect(log).toHaveTextContent("$ npx playwright test --project=SL"));
  expect(log).toHaveTextContent("2 selected · 2 matched");

  await act(async () => {
    await emit("pw-output", { run: 7, line: "Running 2 tests using 1 worker" });
    await emit("pw-output", { run: 7, line: "  ✓  1 [SL] › rating-methods.spec.ts:5:3 › Rating Methods" });
  });
  await waitFor(() => expect(log).toHaveTextContent("Running 2 tests using 1 worker"));
  expect(log.textContent!.indexOf("2 selected")).toBeLessThan(log.textContent!.indexOf("Running 2 tests"));
});

test("Stop ends the run, and its end opens the run's report", async () => {
  const calls = mount();
  await screen.findByText("User can create a new rating method");
  fireEvent.click(box("Tick Rating Methods"));
  fireEvent.click(button("Run picked (2)"));
  await waitFor(() => expect(button("Stop")).toBeEnabled());

  fireEvent.click(button("Stop"));
  await waitFor(() => expect(calls.find((c) => c.cmd === "pw_repo_stop")?.args).toEqual({ run: 7 }));

  // The run's report is new: it opens, and the list shows it.
  calls.addReport(NEW);
  await act(async () => {
    await emit("pw-done", { run: 7, code: null });
  });
  expect(await screen.findByText("The run was stopped.")).toBeInTheDocument();
  await waitFor(() =>
    expect(calls.find((c) => c.cmd === "pw_open_report")?.args).toEqual({ clone: "C:/repo", runId: NEW.run_id }),
  );
  expect(button("Run picked (2)")).toBeEnabled();
  expect(button("Stop")).toBeDisabled();
  expect(await screen.findByText("2026-10-13 09:14:00")).toBeInTheDocument();
});

test("Stop during the listing or the count stops whatever runs", async () => {
  let release: (r: RunStart) => void = () => {};
  const calls = mount({ start: () => new Promise<RunStart>((r) => (release = r)) });
  await screen.findByText("User can create a new rating method");
  fireEvent.click(box("Tick Rating Methods"));
  fireEvent.click(button("Run picked (2)"));
  await waitFor(() => expect(button("Stop")).toBeEnabled());
  // The dry count is going: its id is the slot's, not one this screen knows.
  calls.setRunning({ run: 7, label: "npx playwright test", brief: true });
  fireEvent.click(button("Stop"));
  await waitFor(() => expect(calls.find((c) => c.cmd === "pw_repo_stop")?.args).toEqual({ run: 7 }));
  expect(calls.some((c) => c.cmd === "pw_repo_running")).toBe(true);
  await act(async () => release({ kind: "ConfirmDebug", count: 2 }));
});

test("a run started before the tab was opened is still this Runner's", async () => {
  const calls = mount({ running: { run: 12, label: "npx playwright test", brief: false } });
  await screen.findByText("User can create a new rating method");
  await waitFor(() => expect(button("Stop")).toBeEnabled());
  for (const name of [/^Run picked/, "Run all", "Re-run failures", "Preview", "List again"]) expect(button(name)).toBeDisabled();
  expect(screen.getByRole("combobox", { name: "Browser" })).toBeDisabled();

  // Its lines arrive; another run's do not.
  await act(async () => {
    await emit("pw-output", { run: 12, line: "  ✓  2 [SL] › still going" });
    await emit("pw-output", { run: 99, line: "someone else's line" });
  });
  const log = await screen.findByRole("log", { name: "Run output" });
  await waitFor(() => expect(log).toHaveTextContent("still going"));
  expect(log).not.toHaveTextContent("someone else's line");

  fireEvent.click(button("Stop"));
  await waitFor(() => expect(calls.find((c) => c.cmd === "pw_repo_stop")?.args).toEqual({ run: 12 }));
  // It ends with no new report: nothing opens.
  await act(async () => {
    await emit("pw-done", { run: 12, code: null });
  });
  expect(await screen.findByText("The run was stopped.")).toBeInTheDocument();
  expect(button("Run all")).toBeEnabled();
  await new Promise((r) => setTimeout(r, 20));
  expect(calls.some((c) => c.cmd === "pw_open_report")).toBe(false);
});

test("opened during a dry count, the Runner waits it out, then takes on the run it prepared", async () => {
  const calls = mount({ running: { run: 20, label: "npx playwright test", brief: true } });
  await screen.findByText("User can create a new rating method");
  // Busy, but the dry count is not this screen's run.
  await waitFor(() => expect(button("Run all")).toBeDisabled());
  expect(button("Stop")).toBeEnabled();
  expect(screen.queryByText(/The run/)).not.toBeInTheDocument();

  // The dry count ends and the real run starts.
  calls.setRunning({ run: 21, label: "npx playwright test", brief: false });
  calls.addReport(NEW);
  await act(async () => {
    await emit("pw-done", { run: 20, code: 0 });
  });
  expect(screen.queryByText("The run passed.")).not.toBeInTheDocument();
  await waitFor(() => expect(calls.filter((c) => c.cmd === "pw_repo_running").length).toBeGreaterThanOrEqual(2));
  // Its lines arrive once it is taken on.
  const log = await screen.findByRole("log", { name: "Run output" });
  await waitFor(async () => {
    await act(async () => {
      await emit("pw-output", { run: 21, line: "Running 2 tests using 1 worker" });
    });
    expect(log).toHaveTextContent("Running 2 tests using 1 worker");
  });
  expect(button("Run all")).toBeDisabled();

  await act(async () => {
    await emit("pw-done", { run: 21, code: 1 });
  });
  expect(await screen.findByText("The run ended with failures (exit 1).")).toBeInTheDocument();
  expect(button("Run all")).toBeEnabled();
  // NEW was listed when the run was taken on: it is not this run's.
  await new Promise((r) => setTimeout(r, 20));
  expect(calls.some((c) => c.cmd === "pw_open_report")).toBe(false);
});

test("another kind of repo command going is not taken for a test run", async () => {
  mount({ running: { run: 13, label: "npm run users", brief: true } });
  await screen.findByText("User can create a new rating method");
  await new Promise((r) => setTimeout(r, 20));
  expect(button("Stop")).toBeDisabled();
  expect(button("Run all")).toBeEnabled();
});

test("a failed listing or report read is not retried unasked", async () => {
  const calls = mount({ defaults: true, listFails: "Playwright could not list the tests (exit 1).", reportsFailFirst: true });
  expect(await screen.findByText("Playwright could not list the tests (exit 1).")).toBeInTheDocument();
  expect(await screen.findByText("the reports could not be read")).toBeInTheDocument();
  await new Promise((r) => setTimeout(r, 50));
  expect(calls.filter((c) => c.cmd === "pw_list")).toHaveLength(1);
  expect(calls.filter((c) => c.cmd === "pw_reports")).toHaveLength(1);
});

test("before a run, the reports are read if they were not, so only its own report opens", async () => {
  const calls = mount({ reportsFailFirst: true });
  await screen.findByText("User can create a new rating method");
  fireEvent.click(button("Run all"));
  await waitFor(() => expect(calls.some((c) => c.cmd === "pw_run")).toBe(true));
  const order = calls.map((c) => c.cmd).filter((c) => c === "pw_reports" || c === "pw_run");
  expect(order).toEqual(["pw_reports", "pw_reports", "pw_run"]);
  await waitFor(() => expect(button("Stop")).toBeEnabled());
  // OLD was there before: the run ending without a new report opens nothing.
  await act(async () => {
    await emit("pw-done", { run: 7, code: 0 });
  });
  expect(await screen.findByText("The run passed.")).toBeInTheDocument();
  await new Promise((r) => setTimeout(r, 20));
  expect(calls.some((c) => c.cmd === "pw_open_report")).toBe(false);
});

test("the browser mode is locked while asking, and the answer keeps it", async () => {
  const calls = mount({
    start: (args) =>
      args.confirm
        ? { kind: "Started", run: 8, command: "npx playwright test --debug", summary: "2 selected · 2 matched", warnings: [] }
        : { kind: "ConfirmDebug", count: 2 },
  });
  await screen.findByText("User can create a new rating method");
  fireEvent.click(box("Tick Rating Methods"));
  const mode = screen.getByRole("combobox", { name: "Browser" });
  fireEvent.click(mode);
  fireEvent.click(screen.getByRole("option", { name: "Debug" }));
  fireEvent.click(button("Run picked (2)"));
  const alert = await screen.findByRole("alert");
  expect(mode).toBeDisabled();
  fireEvent.click(within(alert).getByRole("button", { name: "Run in debug" }));
  await waitFor(() => expect(calls.filter((c) => c.cmd === "pw_run").map((c) => c.args.mode)).toEqual(["Debug", "Debug"]));
  await waitFor(() => expect(button("Stop")).toBeEnabled());
  expect(mode).toBeDisabled();
});

test("the report list reopens a past report", async () => {
  const calls = mount();
  expect(await screen.findByText("2026-10-12 16:00:00")).toBeInTheDocument();
  expect(screen.getByText("3 passed, 1 failed")).toBeInTheDocument();
  fireEvent.click(button(`Open the report of ${OLD.run_id}`));
  await waitFor(() =>
    expect(calls.find((c) => c.cmd === "pw_open_report")?.args).toEqual({ clone: "C:/repo", runId: OLD.run_id }),
  );
});

test("a debug run of more than one test asks first", async () => {
  const calls = mount({
    start: (args) =>
      args.confirm
        ? { kind: "Started", run: 8, command: "npx playwright test --debug", summary: "2 selected · 2 matched", warnings: [] }
        : { kind: "ConfirmDebug", count: 2 },
  });
  await screen.findByText("User can create a new rating method");
  fireEvent.click(box("Tick Rating Methods"));
  fireEvent.click(button("Run picked (2)"));
  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent("this runs 2");
  fireEvent.click(within(alert).getByRole("button", { name: "Run in debug" }));
  await waitFor(() => expect(calls.filter((c) => c.cmd === "pw_run").map((c) => c.args.confirm)).toEqual([false, true]));
  await waitFor(() => expect(button("Stop")).toBeEnabled());
});

test("a refused run says why and starts nothing", async () => {
  mount({
    start: () => {
      throw "2 selected, but Playwright matched 1. The run was stopped rather than silently running the wrong set.";
    },
  });
  await screen.findByText("User can create a new rating method");
  fireEvent.click(box("Tick Rating Methods"));
  fireEvent.click(button("Run picked (2)"));
  expect(await screen.findByText(/2 selected, but Playwright matched 1/)).toBeInTheDocument();
  expect(button("Stop")).toBeDisabled();
  expect(button("Run picked (2)")).toBeEnabled();
});

test("Preview shows the run list and the command", async () => {
  mount();
  await screen.findByText("User can create a new rating method");
  fireEvent.click(box("Tick Rating Methods"));
  fireEvent.click(button("Preview"));
  const preview = await screen.findByLabelText("Preview");
  expect(preview).toHaveTextContent(`[SL] > ${RM}`);
  expect(preview).toHaveTextContent("npx playwright test --project=SL");
  expect(preview).toHaveTextContent("Would run 2:");
});

test("a run that ends before its start comes back still ends", async () => {
  mount({
    start: async () => {
      await emit("pw-done", { run: 9, code: 1 });
      return { kind: "Started", run: 9, command: "npx playwright test", summary: "2 selected · 2 matched", warnings: [] };
    },
  });
  await screen.findByText("User can create a new rating method");
  fireEvent.click(box("Tick Rating Methods"));
  fireEvent.click(button("Run picked (2)"));
  expect(await screen.findByText("The run ended with failures (exit 1).")).toBeInTheDocument();
  expect(button("Stop")).toBeDisabled();
  expect(button("Run picked (2)")).toBeEnabled();
});

test("each branch counts its ticked tests, and the output panel waits for a run", async () => {
  mount();
  await screen.findByText("User can create a new rating method");
  expect(screen.getByRole("log", { name: "Run output" })).toHaveTextContent("No run yet");
  fireEvent.click(box("Tick the app shell loads"));
  // SL holds three tests, one of them ticked.
  expect(within(box("Tick SL").parentElement!).getByText("1/3")).toBeInTheDocument();
  expect(within(box("Tick Common Components").parentElement!).getByText("1")).toBeInTheDocument();
});

test("a run's output does not render the tree again, line by line", async () => {
  mount();
  await screen.findByText("User can create a new rating method");
  fireEvent.click(box("Tick Rating Methods"));
  fireEvent.click(button("Run picked (2)"));
  const log = await screen.findByRole("log", { name: "Run output" });
  await waitFor(() => expect(log).toHaveTextContent("2 selected · 2 matched"));
  await waitFor(() => expect(button("Stop")).toBeEnabled());

  const before = boxes.renders;
  await act(async () => {
    for (let i = 1; i <= 40; i++) await emit("pw-output", { run: 7, line: `line ${i}` });
  });
  await waitFor(() => expect(log).toHaveTextContent("line 40"));
  expect(log.textContent!.indexOf("line 1\n")).toBeLessThan(log.textContent!.indexOf("line 40"));
  expect(boxes.renders).toBe(before);
  await act(async () => {
    await emit("pw-done", { run: 7, code: 0 });
  });
});
