// Grouping and bulk selection on the Auto Run case list.
//
// The screen reads the case list from Azure DevOps and writes nothing back
// to it, so every assertion here is about local state: which rows are
// ticked, which groups are shut, and what the Run button hands to RunPane.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { Profiler } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { toast } from "../../lib/toast";
import AutoRun from "./index";

vi.mock("../../lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() } }));

/** What auto_run_load_recipe answers; a test that needs a recipe sets it. */
let savedRecipe: unknown = null;

afterEach(() => {
  savedRecipe = null;
  clearMocks();
  localStorage.clear();
  vi.restoreAllMocks();
  vi.clearAllMocks();
});

const pbi = { id: 42, title: "Login work", state: "Active", work_item_type: "Product Backlog Item" };

function caseRow(id: number, title: string) {
  return {
    id,
    title,
    tags: "",
    automation_status: "Not Automated",
    steps: [],
    step_ids: [],
    steps_xml: "",
    module_value: "",
    preconditions: "",
  };
}

const STEPS = [{ step_number: 1, actions: [{ kind: "check_text", value: "ok" }] }];

/** `scripted` names the case ids that have a saved script; every other
 * case answers null the way an unscripted one does. `runs` defaults to
 * none on this machine, and `onCommand` lets a test observe or answer a
 * call the shared handler does not know about (Clear scripts/results). */
function mockList(
  cases: ReturnType<typeof caseRow>[],
  scripted: number[],
  runs: unknown[] = [],
  onCommand?: (cmd: string, args: unknown) => unknown,
) {
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") {
      const id = (args as { caseId?: number; case_id?: number }).caseId ?? (args as { case_id: number }).case_id;
      return scripted.includes(id) ? { case_id: id, title: "s", steps: STEPS } : null;
    }
    if (cmd === "auto_run_list_runs") return runs;
    if (cmd === "auto_run_list_accounts") return [];
    if (cmd === "auto_run_load_recipe") return savedRecipe;
    if (onCommand) return onCommand(cmd, args);
    return null;
  });
}

/** Switches the screen to its tab of that name, the way a person would. */
function openTab(name: "Test cases" | "Past runs") {
  fireEvent.click(screen.getByRole("tab", { name: new RegExp(`^${name}`) }));
}

/** Shows the Setup panel's full rows, the way a person would. The panel
 * already shows them when something a run needs is missing (as in most of
 * these mocks), and then there is nothing to press. */
function openSetup() {
  const show = screen.queryByRole("button", { name: "Show setup details" });
  if (show) fireEvent.click(show);
}

/** Opens a case's card, which is where its Script and Run buttons are. */
function openCard(id: number) {
  fireEvent.click(screen.getByRole("button", { name: `Show details for #${id}` }));
}

/** Opens the Test cases tab's More menu and returns one of its items. */
function moreItem(name: "Import scripts" | "Clear scripts") {
  fireEvent.click(screen.getByRole("button", { name: "More" }));
  return screen.getByRole("menuitem", { name });
}

/** Renders the screen on its Test cases tab, where the screen always opens.
 * The mocks here set nothing up, so the Setup panel opens its full rows
 * by itself. A test about the past runs switches tab first. */
function renderScreen() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(
    <QueryClientProvider client={qc}>
      <AutoRun org="acme" project="proj" pbi={pbi as never} />
    </QueryClientProvider>,
  );
  openTab("Test cases");
  return view;
}

test("the Setup card's Accounts and Sign-in buttons open their own dialogs", async () => {
  mockList([caseRow(1, "Login - valid credentials")], [1]);
  renderScreen();
  await screen.findByText("Login - valid credentials");
  openSetup();

  fireEvent.click(screen.getByRole("button", { name: "Edit accounts" }));
  expect(await screen.findByRole("heading", { name: "Accounts" })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  await waitFor(() =>
    expect(screen.queryByRole("heading", { name: "Accounts" })).not.toBeInTheDocument(),
  );

  // The Sign-in row has two ways in: Edit opens the recipe as JSON...
  fireEvent.click(screen.getByRole("button", { name: "Edit sign-in recipe" }));
  expect(await screen.findByRole("heading", { name: "Sign-in recipe" })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  await waitFor(() =>
    expect(screen.queryByRole("heading", { name: "Sign-in recipe" })).not.toBeInTheDocument(),
  );

  // ...and Record opens the recorder.
  fireEvent.click(within(row("Sign-in")).getByRole("button", { name: "Record sign-in" }));
  expect(await screen.findByRole("heading", { name: "Record sign-in" })).toBeInTheDocument();
  expect(screen.queryByRole("heading", { name: "Sign-in recipe" })).not.toBeInTheDocument();
});

test("Group by title folds cases sharing a prefix into one heading", async () => {
  mockList(
    [
      caseRow(1, "Login - valid credentials"),
      caseRow(2, "Login - locked account"),
      caseRow(3, "Reports - export to CSV"),
    ],
    [1, 2, 3],
  );
  renderScreen();

  // Flat by default: no heading, every case its own row.
  expect(await screen.findByText("Login - valid credentials")).toBeInTheDocument();
  expect(screen.queryByText("Login (2)")).not.toBeInTheDocument();

  fireEvent.click(screen.getByRole("checkbox", { name: "Group by title" }));

  expect(await screen.findByText("Login (2)")).toBeInTheDocument();
  // A one-member bucket is not a folder - it lands in Ungrouped.
  expect(screen.getByText("Ungrouped (1)")).toBeInTheDocument();
});

test("the header checkbox ticks only the scripted cases under it", async () => {
  // Three in the group, one of them without a script: the checkbox must
  // take two, not three, or "Run 3 selected" would queue a case the
  // runner has nothing to run.
  mockList(
    [
      caseRow(1, "Login - valid credentials"),
      caseRow(2, "Login - locked account"),
      caseRow(3, "Login - expired password"),
    ],
    [1, 3],
  );
  renderScreen();
  await screen.findByText("Login - valid credentials");
  fireEvent.click(screen.getByRole("checkbox", { name: "Group by title" }));

  fireEvent.click(await screen.findByRole("checkbox", { name: "Select all in Login" }));
  expect(await screen.findByRole("button", { name: "Run 2 selected" })).toBeInTheDocument();
});

test("clicking a fully ticked header checkbox clears the group; the title folds it", async () => {
  mockList(
    [caseRow(1, "Login - valid credentials"), caseRow(2, "Login - locked account")],
    [1, 2],
  );
  renderScreen();
  await screen.findByText("Login - valid credentials");
  fireEvent.click(screen.getByRole("checkbox", { name: "Group by title" }));

  const box = await screen.findByRole("checkbox", { name: "Select all in Login" });
  fireEvent.click(box);
  expect(await screen.findByRole("button", { name: "Run 2 selected" })).toBeInTheDocument();

  fireEvent.click(box);
  await waitFor(() =>
    expect(screen.queryByRole("button", { name: "Run 2 selected" })).not.toBeInTheDocument(),
  );

  // The TITLE is a fold control now - same contract as every other
  // grouped screen, so a habit learned there cannot mis-tick runs here.
  fireEvent.click(screen.getByText("Login (2)"));
  expect(screen.queryByText("Login - valid credentials")).not.toBeInTheDocument();
});

test("collapsing a group keeps its ticked cases, shown by the header checkbox", async () => {
  mockList(
    [caseRow(1, "Login - valid credentials"), caseRow(2, "Login - locked account")],
    [1, 2],
  );
  renderScreen();
  await screen.findByText("Login - valid credentials");
  fireEvent.click(screen.getByRole("checkbox", { name: "Group by title" }));
  fireEvent.click(await screen.findByRole("checkbox", { name: "Select all in Login" }));
  await screen.findByRole("button", { name: "Run 2 selected" });

  // Collapsing hides the rows; the count has to survive, or a person
  // cannot tell what a "Run 2 selected" is about to run - it lives in
  // the run button's own label now. The whole-group tick stays on the
  // header checkbox - no separate dot marker.
  fireEvent.click(screen.getByRole("button", { name: "Collapse group Login" }));
  expect(screen.queryByText("Login - valid credentials")).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Run 2 selected" })).toBeInTheDocument();
  expect(screen.getByRole("checkbox", { name: "Select all in Login" })).toHaveAttribute(
    "aria-checked",
    "true",
  );
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
});

test("the grouping choice and the collapsed groups outlive a remount", async () => {
  const cases = [caseRow(1, "Login - valid credentials"), caseRow(2, "Login - locked account")];
  mockList(cases, [1, 2]);
  const first = renderScreen();
  await screen.findByText("Login - valid credentials");
  fireEvent.click(screen.getByRole("checkbox", { name: "Group by title" }));
  fireEvent.click(await screen.findByRole("button", { name: "Collapse group Login" }));
  first.unmount();

  mockList(cases, [1, 2]);
  renderScreen();
  // Still grouped, still shut - both read back from storage on mount.
  expect(await screen.findByText("Login (2)")).toBeInTheDocument();
  expect(screen.queryByText("Login - valid credentials")).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Expand group Login" })).toBeInTheDocument();
});

test("an unscripted case cannot be ticked, so a bulk run never queues one", async () => {
  mockList([caseRow(1, "Login - valid credentials"), caseRow(2, "Login - locked account")], [1]);
  renderScreen();
  await screen.findByText("Login - locked account");

  // Case 2 has no script: its checkbox is present for row alignment but
  // inert, and Run is not offered at all, not even on its open card.
  openCard(2);
  expect(await screen.findByRole("button", { name: "Add script for #2" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Run #2" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("checkbox", { name: "Select #2" }));
  expect(screen.queryByText(/selected/)).not.toBeInTheDocument();

  fireEvent.click(screen.getByRole("checkbox", { name: "Select #1" }));
  expect(await screen.findByRole("button", { name: "Run 1 selected" })).toBeInTheDocument();
});

test("Run N selected opens one pane for the whole selection, in list order", async () => {
  mockList(
    [caseRow(3, "Alpha check"), caseRow(1, "Beta check"), caseRow(2, "Gamma check")],
    [1, 2, 3],
  );
  renderScreen();
  await screen.findByText("Alpha check");

  // Tick out of list order on purpose - the run must still read top-down.
  fireEvent.click(screen.getByRole("checkbox", { name: "Select #2" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Select #3" }));

  fireEvent.click(screen.getByRole("button", { name: "Run 2 selected" }));

  // The pane opens on the FIRST case in list order (#3 "Alpha check"),
  // not the first one clicked.
  const progress = await screen.findByText("case 1 of 2");
  expect(progress.closest("h2")).toHaveTextContent("#3 Alpha check");
});

test("Clear selection drops the selection without opening a run", async () => {
  mockList([caseRow(1, "Alpha check")], [1]);
  renderScreen();
  await screen.findByText("Alpha check");

  fireEvent.click(screen.getByRole("checkbox", { name: "Select #1" }));
  expect(await screen.findByRole("button", { name: "Run 1 selected" })).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Clear selection" }));
  await waitFor(() =>
    expect(screen.queryByRole("button", { name: "Run 1 selected" })).not.toBeInTheDocument(),
  );
  expect(screen.queryByText("Open browser")).not.toBeInTheDocument();
});

test("finishing a run clears the selection it ran", async () => {
  mockList([caseRow(1, "Alpha check")], [1]);
  renderScreen();
  await screen.findByText("Alpha check");

  fireEvent.click(screen.getByRole("checkbox", { name: "Select #1" }));
  fireEvent.click(await screen.findByRole("button", { name: "Run 1 selected" }));

  // Leaving without a verdict still ends the run - the ticks must go with
  // it, or the next click runs the same case again by accident.
  fireEvent.click(await screen.findByRole("button", { name: /Close/ }));
  await waitFor(() =>
    expect(screen.queryByRole("button", { name: "Run 1 selected" })).not.toBeInTheDocument(),
  );
});

test("with cases ticked the bar offers both a supervised and an unattended run", async () => {
  mockList([caseRow(1, "Alpha check"), caseRow(2, "Beta check")], [1, 2]);
  renderScreen();
  await screen.findByText("Alpha check");

  fireEvent.click(screen.getByRole("checkbox", { name: "Select #1" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Select #2" }));

  expect(await screen.findByRole("button", { name: "Run 2 selected" })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Run 2 unattended" }));
  expect(await screen.findByRole("heading", { name: "Unattended run" })).toBeInTheDocument();
});

// ---- Execution order: both run buttons use the plan ----

const PLANNED = (order: number[]) => ({ order, phases: [order], resets: [], counts: null, saved: true });

test("Run selected starts from the planned order, not list order", async () => {
  mockList([caseRow(1, "Alpha check"), caseRow(2, "Beta check")], [1, 2], [], (cmd) =>
    cmd === "auto_run_plan" ? PLANNED([2, 1]) : null,
  );
  renderScreen();
  await screen.findByText("Alpha check");
  fireEvent.click(screen.getByRole("checkbox", { name: "Select #1" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Select #2" }));
  fireEvent.click(await screen.findByRole("button", { name: "Run 2 selected" }));

  const progress = await screen.findByText("case 1 of 2");
  expect(progress.closest("h2")).toHaveTextContent("#2 Beta check");
});

test("Run unattended starts from the planned order and shows no plan for a single phase", async () => {
  const replays: { cases: { case_id: number }[] }[] = [];
  mockList([caseRow(1, "Alpha check"), caseRow(2, "Beta check")], [1, 2], [], (cmd, args) => {
    if (cmd === "auto_run_plan") return PLANNED([2, 1]);
    if (cmd === "auto_run_replay") {
      replays.push(args as never);
      return new Promise(() => {});
    }
    return null;
  });
  renderScreen();
  await screen.findByText("Alpha check");
  fireEvent.click(screen.getByRole("checkbox", { name: "Select #1" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Select #2" }));
  fireEvent.click(await screen.findByRole("button", { name: "Run 2 unattended" }));
  await screen.findByRole("heading", { name: "Unattended run" });
  expect(screen.queryByLabelText("Run plan")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Start" }));
  await waitFor(() => expect(replays).toHaveLength(1));
  expect(replays[0].cases.map((c) => c.case_id)).toEqual([2, 1]);
});

test("a double click on a run button asks once and starts one run", async () => {
  let plans = 0;
  mockList([caseRow(1, "Alpha check")], [1], [], (cmd) => {
    if (cmd !== "auto_run_plan") return null;
    plans += 1;
    return new Promise((r) => setTimeout(() => r(PLANNED([1])), 30));
  });
  renderScreen();
  await screen.findByText("Alpha check");
  fireEvent.click(screen.getByRole("checkbox", { name: "Select #1" }));
  const run = await screen.findByRole("button", { name: "Run 1 selected" });
  fireEvent.click(run);
  fireEvent.click(run);
  expect(run).toBeDisabled();
  await screen.findByRole("heading", { name: /#1 Alpha check/ });
  expect(plans).toBe(1);
});

test("when the plan fails no run starts and a fixed sentence is shown, not the raw error", async () => {
  mockList([caseRow(1, "Alpha check")], [1], [], (cmd) => {
    if (cmd === "auto_run_plan") throw "the store is unreadable";
    return null;
  });
  renderScreen();
  await screen.findByText("Alpha check");
  fireEvent.click(screen.getByRole("checkbox", { name: "Select #1" }));
  fireEvent.click(await screen.findByRole("button", { name: "Run 1 selected" }));
  await waitFor(() =>
    expect(toast.error).toHaveBeenCalledWith("Could not work out the order. Try again, or see Settings → Logs."),
  );
  expect(JSON.stringify(vi.mocked(toast.error).mock.calls)).not.toContain("unreadable");
  expect(screen.queryByText(/case 1 of/)).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Run 1 selected" })).toBeEnabled();
});

test("a run is not started if the PBI changed while the plan was on its way", async () => {
  let resolvePlan: (v: unknown) => void = () => {};
  mockList([caseRow(1, "Alpha check")], [1], [], (cmd) =>
    cmd === "auto_run_plan" ? new Promise((r) => (resolvePlan = r)) : null,
  );
  const view = renderScreen();
  await screen.findByText("Alpha check");
  fireEvent.click(screen.getByRole("checkbox", { name: "Select #1" }));
  fireEvent.click(await screen.findByRole("button", { name: "Run 1 selected" }));
  view.rerender(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <AutoRun org="acme" project="proj" pbi={{ ...pbi, id: 43 } as never} />
    </QueryClientProvider>,
  );
  await act(async () => {
    resolvePlan(PLANNED([1]));
    await new Promise((r) => setTimeout(r, 20));
  });
  expect(screen.queryByText(/case 1 of/)).not.toBeInTheDocument();
  expect(screen.queryByRole("heading", { name: /#1 Alpha check/ })).not.toBeInTheDocument();
});

test("a plan that is not these cases runs in list order", async () => {
  mockList([caseRow(1, "Alpha check"), caseRow(2, "Beta check")], [1, 2], [], (cmd) =>
    cmd === "auto_run_plan" ? PLANNED([2, 7]) : null,
  );
  renderScreen();
  await screen.findByText("Alpha check");
  fireEvent.click(screen.getByRole("checkbox", { name: "Select #1" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Select #2" }));
  fireEvent.click(await screen.findByRole("button", { name: "Run 2 selected" }));
  const progress = await screen.findByText("case 1 of 2");
  expect(progress.closest("h2")).toHaveTextContent("#1 Alpha check");
});

test("the Execution order item in More opens the dialog", async () => {
  mockList([caseRow(1, "Alpha check")], [1]);
  renderScreen();
  await screen.findByText("Alpha check");
  fireEvent.click(screen.getByRole("button", { name: "More" }));
  fireEvent.click(screen.getByRole("menuitem", { name: /Execution order/ }));
  expect(await screen.findByRole("heading", { name: "Execution order" })).toBeInTheDocument();
});

// ---- Clear scripts / Clear results (shown wherever Auto Run is: dev, or unlocked) ----

test("Clear scripts and Clear results are disabled when there is nothing to clear", async () => {
  // No case has a script, and no run exists on this machine.
  mockList([caseRow(1, "Alpha check"), caseRow(2, "Beta check")], []);
  renderScreen();
  await screen.findByText("Alpha check");

  expect(moreItem("Clear scripts")).toBeDisabled();
  openTab("Past runs");
  expect(await screen.findByRole("button", { name: "Clear results" })).toBeDisabled();
});

test("Clear scripts opens its confirm with the exact sentence; Cancel calls nothing", async () => {
  let calls = 0;
  mockList([caseRow(1, "Alpha check"), caseRow(2, "Beta check")], [1], [], (cmd) => {
    if (cmd === "auto_run_clear_scripts") calls += 1;
    return null;
  });
  renderScreen();
  await screen.findByText("Alpha check");

  fireEvent.click(moreItem("Clear scripts"));
  expect(
    await screen.findByText(
      "This removes the scripts of the 2 cases listed for this PBI from this machine. Nothing in Azure DevOps changes.",
    ),
  ).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  await waitFor(() =>
    expect(screen.queryByRole("button", { name: "Clear 2 scripts" })).not.toBeInTheDocument(),
  );
  expect(calls).toBe(0);
});

test("confirming Clear scripts calls the command, toasts the count and refreshes the script badges", async () => {
  mockList([caseRow(1, "Alpha check")], [1], [], (cmd, args) => {
    if (cmd === "auto_run_clear_scripts") {
      expect((args as { caseIds: number[] }).caseIds).toEqual([1]);
      return 1;
    }
    return null;
  });
  renderScreen();
  await screen.findByText("Alpha check");

  fireEvent.click(moreItem("Clear scripts"));
  fireEvent.click(await screen.findByRole("button", { name: "Clear 1 script" }));

  await waitFor(() => expect(toast.success).toHaveBeenCalledWith("1 script removed."));
  expect(screen.queryByText(/This removes the scripts of/)).not.toBeInTheDocument();
});

test("Clear results opens its confirm with the exact sentence and, once confirmed, toasts the count", async () => {
  mockList(
    [caseRow(1, "Alpha check")],
    [1],
    [{ id: "run-1", pbi_id: 42, started_at: "1", cases: [], mode: "unattended", published: null }],
    (cmd) => (cmd === "auto_run_clear_runs" ? 1 : null),
  );
  renderScreen();
  await screen.findByText("Alpha check");
  openTab("Past runs");

  fireEvent.click(await screen.findByRole("button", { name: "Clear results" }));
  expect(
    await screen.findByText(
      "This removes every Auto Run result and picture on this machine, including runs already sent to Azure DevOps (those stay there). Nothing in Azure DevOps changes.",
    ),
  ).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Clear 1 run" }));
  await waitFor(() => expect(toast.success).toHaveBeenCalledWith("1 run removed."));
});

test("the Setup card's Areas button opens its dialog", async () => {
  mockList([caseRow(1, "Login - valid credentials")], [1], [], (cmd) =>
    cmd === "auto_run_load_nav" ? { direct_urls: true, modules: [] } : null,
  );
  renderScreen();
  await screen.findByText("Login - valid credentials");
  openSetup();
  fireEvent.click(screen.getByRole("button", { name: "Edit areas" }));
  expect(await screen.findByRole("heading", { name: "Areas" })).toBeInTheDocument();
});

test("the Setup card's Discovery row says what is mapped, and View opens the Discovery dialog", async () => {
  const day = 24 * 60 * 60 * 1000;
  const areaOf = (area: string, stale: boolean) => ({
    area,
    explored_at: Date.now() - day,
    account: "admin",
    stale,
    stale_reason: stale ? "A script failed there since it was explored" : null,
    pages: 1,
    elements: 2,
    writes: [],
  });
  mockList([caseRow(1, "Login - valid credentials")], [1], [], (cmd) =>
    cmd === "auto_run_load_map" ? { areas: [areaOf("Leave", false), areaOf("Payroll", true)] } : null,
  );
  renderScreen();
  await screen.findByText("Login - valid credentials");
  openSetup();
  expect(await within(row("Discovery")).findByText("2 areas explored, 1 stale")).toBeInTheDocument();
  fireEvent.click(within(row("Discovery")).getByRole("button", { name: "View discovery" }));
  expect(await screen.findByRole("heading", { name: "Discovery" })).toBeInTheDocument();
  expect(await screen.findByRole("listitem", { name: "Payroll" })).toHaveTextContent("Stale");
});

test("end_discovery_shows_while_discovery_is_active_and_ends_it", async () => {
  let active = true;
  const ended: string[] = [];
  mockList([caseRow(1, "Login - valid credentials")], [1], [], (cmd) => {
    if (cmd === "auto_run_discovery_active") return active;
    if (cmd === "auto_run_end_discovery") {
      ended.push(cmd);
      active = false;
      return null;
    }
    return null;
  });
  renderScreen();
  await screen.findByText("Login - valid credentials");
  openSetup();
  const end = await within(row("Discovery")).findByRole("button", { name: "End discovery" });
  fireEvent.click(end);
  // No confirm: ending loses nothing, so the one click ends it.
  await waitFor(() => expect(ended).toEqual(["auto_run_end_discovery"]));
});

test("end_discovery_is_not_offered_while_no_discovery_is_going", async () => {
  mockList([caseRow(1, "Login - valid credentials")], [1], [], (cmd) =>
    cmd === "auto_run_discovery_active" ? false : null,
  );
  renderScreen();
  await screen.findByText("Login - valid credentials");
  openSetup();
  await within(row("Discovery")).findByRole("button", { name: "View discovery" });
  expect(within(row("Discovery")).queryByRole("button", { name: "End discovery" })).not.toBeInTheDocument();
});

// ---- Last result filter ----

const rec = (case_id: number, verdict: string, proposed = "") => ({
  case_id,
  title: `case ${case_id}`,
  verdict,
  proposed,
  note: "",
  steps: [],
});
const savedRun = (id: string, started_at: string, cases: ReturnType<typeof rec>[]) => ({
  id,
  pbi_id: 42,
  started_at,
  cases,
  mode: "unattended",
  published: null,
});

/** Six cases over two title groups. Their last results: #1 Failed (a newer
 * run overrides its older Passed), #2 Blocked, #3 Failed, #4 Passed, #5 never
 * run, #6 Passed. #3 has no script. */
function mockFilterList() {
  mockList(
    [
      caseRow(1, "Login - alpha"),
      caseRow(2, "Login - bravo"),
      caseRow(3, "Login - charlie"),
      caseRow(4, "Reports - delta"),
      caseRow(5, "Reports - echo"),
      caseRow(6, "Login - foxtrot"),
    ],
    [1, 2, 4, 5, 6],
    [
      savedRun("a", "1000", [
        rec(1, "Passed"),
        rec(2, "", "Blocked"),
        rec(3, "Failed"),
        rec(4, "Passed"),
        rec(6, "Passed"),
      ]),
      savedRun("b", "2000", [rec(1, "", "Failed")]),
    ],
  );
}

const lastResultGroup = () => screen.findByRole("group", { name: "Filter by last result" });
const rowOf = (title: string) => screen.getByText(title).closest("li") as HTMLElement;
const visibleTitles = () => screen.queryAllByText(/^(Login|Reports) - /).map((e) => e.textContent);
// The row's buttons carry their counts only once the runs are read.
const press = async (name: string) => fireEvent.click(await screen.findByRole("button", { name }));

test("the Last result row counts every case, and pressing buckets shows only them", async () => {
  mockFilterList();
  renderScreen();
  const group = await lastResultGroup();
  await waitFor(() =>
    expect(within(group).getByRole("button", { name: "Failed (2)" })).toBeInTheDocument(),
  );
  expect(within(group).getByRole("button", { name: "Passed (2)" })).toBeInTheDocument();
  expect(within(group).getByRole("button", { name: "Blocked (1)" })).toBeInTheDocument();
  expect(within(group).getByRole("button", { name: "Not run (1)" })).toBeInTheDocument();
  // Nothing pressed: every case shows, as it always did.
  expect(visibleTitles()).toHaveLength(6);
  expect(within(group).getByRole("button", { name: "Failed (2)" })).toHaveAttribute(
    "aria-pressed",
    "false",
  );

  await press("Failed (2)");
  expect(within(group).getByRole("button", { name: "Failed (2)" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  expect(visibleTitles()).toEqual(["Login - alpha", "Login - charlie"]);

  // Multi-select: Blocked adds to Failed rather than replacing it.
  await press("Blocked (1)");
  expect(visibleTitles()).toEqual(["Login - alpha", "Login - bravo", "Login - charlie"]);

  await press("Failed (2)");
  await press("Blocked (1)");
  expect(visibleTitles()).toHaveLength(6);
});

test("the filter applies when grouped, and a group with nothing to show is hidden", async () => {
  mockFilterList();
  renderScreen();
  await screen.findByText("Login - alpha");
  fireEvent.click(screen.getByRole("checkbox", { name: "Group by title" }));
  expect(await screen.findByText("Login (4)")).toBeInTheDocument();
  expect(screen.getByText("Reports (2)")).toBeInTheDocument();

  await press("Failed (2)");
  expect(screen.getByText("Login (2)")).toBeInTheDocument();
  expect(screen.queryByText(/^Reports/)).not.toBeInTheDocument();
  expect(visibleTitles()).toEqual(["Login - alpha", "Login - charlie"]);
});

test("Select all shown ticks only the visible cases that have a script", async () => {
  mockFilterList();
  renderScreen();
  await screen.findByText("Login - alpha");
  const all = screen.getByRole("checkbox", { name: "Select all shown" });

  await press("Failed (2)");
  // Visible: #1 (scripted) and #3 (not).
  fireEvent.click(all);
  expect(await screen.findByRole("button", { name: "Run 1 selected" })).toBeInTheDocument();
  expect(screen.getByRole("checkbox", { name: "Select #1" })).toHaveAttribute(
    "aria-checked",
    "true",
  );
  expect(all).toHaveAttribute("aria-checked", "true");

  // Widen the filter: the ticked case stays, and now only some are ticked.
  await press("Passed (2)");
  expect(all).toHaveAttribute("aria-checked", "mixed");
  fireEvent.click(all);
  expect(await screen.findByRole("button", { name: "Run 3 selected" })).toBeInTheDocument();

  // Ticked all the way, a click clears just what is shown.
  fireEvent.click(all);
  await waitFor(() =>
    expect(screen.queryByRole("button", { name: /selected$/ })).not.toBeInTheDocument(),
  );
});

test("Select all shown is off while no visible case has a script", async () => {
  mockList(
    [caseRow(1, "Login - alpha"), caseRow(3, "Login - charlie")],
    [1],
    [savedRun("a", "1000", [rec(1, "Passed"), rec(3, "Failed")])],
  );
  renderScreen();
  await screen.findByText("Login - alpha");
  const all = screen.getByRole("checkbox", { name: "Select all shown" });
  await waitFor(() => expect(all).not.toHaveAttribute("aria-disabled", "true"));
  await press("Failed (1)");
  expect(all).toHaveAttribute("aria-disabled", "true");
});

test("pressing a filter that hides a ticked case unticks it", async () => {
  mockFilterList();
  renderScreen();
  await screen.findByText("Login - alpha");
  fireEvent.click(screen.getByRole("checkbox", { name: "Select #1" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Select #4" }));
  expect(await screen.findByRole("button", { name: "Run 2 selected" })).toBeInTheDocument();

  await press("Failed (2)"); // #4 (Passed) is hidden now
  expect(await screen.findByRole("button", { name: "Run 1 selected" })).toBeInTheDocument();

  // And it stays off when the filter is lifted: it was dropped, not parked.
  await press("Failed (2)");
  expect(screen.getByRole("checkbox", { name: "Select #4" })).toHaveAttribute(
    "aria-checked",
    "false",
  );
  expect(screen.getByRole("button", { name: "Run 1 selected" })).toBeInTheDocument();
});

test("a group's Select all covers only the rows of it that are shown", async () => {
  mockFilterList();
  renderScreen();
  await screen.findByText("Login - alpha");
  fireEvent.click(screen.getByRole("checkbox", { name: "Group by title" }));
  await screen.findByText("Login (4)");

  await press("Failed (2)"); // Login shows #1 and #3; #2 and #6 are hidden
  fireEvent.click(screen.getByRole("checkbox", { name: "Select all in Login" }));
  expect(await screen.findByRole("button", { name: "Run 1 selected" })).toBeInTheDocument();

  await press("Failed (2)");
  expect(screen.getByRole("checkbox", { name: "Select #2" })).toHaveAttribute(
    "aria-checked",
    "false",
  );
  expect(screen.getByRole("checkbox", { name: "Select #6" })).toHaveAttribute(
    "aria-checked",
    "false",
  );
  expect(screen.getByRole("checkbox", { name: "Select all in Login" })).toHaveAttribute(
    "aria-checked",
    "mixed",
  );
});

test("a filter that shows nothing says so", async () => {
  mockList([caseRow(1, "Login - alpha")], [1]);
  renderScreen();
  await screen.findByText("Login - alpha");
  await press("Passed (0)");
  expect(screen.getByText("No cases match this filter.")).toBeInTheDocument();
  expect(screen.queryByText("Login - alpha")).not.toBeInTheDocument();
  await press("Passed (0)");
  expect(screen.queryByText("No cases match this filter.")).not.toBeInTheDocument();
});

test("each card names its last result, and a case never run says Not run", async () => {
  mockFilterList();
  renderScreen();
  await screen.findByText("Login - alpha");
  // The visible word, with the words a screen reader hears before it.
  await waitFor(() =>
    expect(within(rowOf("Login - alpha")).getByText("Failed")).toHaveTextContent("Last result: Failed"),
  );
  expect(within(rowOf("Login - bravo")).getByText("Blocked")).toHaveTextContent(
    "Last result: Blocked",
  );
  expect(within(rowOf("Reports - delta")).getByText("Passed")).toHaveTextContent(
    "Last result: Passed",
  );
  // A case no run reached says so in the same words the Not run filter uses.
  expect(within(rowOf("Reports - echo")).getByText("Not run")).toHaveTextContent("Last result: Not run");
  // The mark sits beside the card's own controls, it does not replace them:
  // they are on the open card.
  expect(within(rowOf("Login - alpha")).queryByRole("button", { name: "Run #1" })).not.toBeInTheDocument();
  openCard(1);
  expect(within(rowOf("Login - alpha")).getByRole("button", { name: "Run #1" })).toBeInTheDocument();
  expect(within(rowOf("Login - alpha")).getByText("Failed")).toHaveTextContent("Last result: Failed");
});

test("a run stopped before a case leaves that case's older result standing", async () => {
  mockList(
    [caseRow(1, "Login - alpha"), caseRow(2, "Login - bravo")],
    [1, 2],
    [
      savedRun("a", "1000", [rec(1, "Passed"), rec(2, "Failed")]),
      savedRun("b", "2000", [rec(1, "", ""), rec(2, "", "Passed")]),
    ],
  );
  renderScreen();
  await screen.findByRole("button", { name: "Passed (2)" });
  expect(screen.getByRole("button", { name: "Not run (0)" })).toBeInTheDocument();
});

const CASES = [caseRow(1, "Login - alpha"), caseRow(2, "Login - bravo")];

/** The screen with `autoRunListRuns` answered by `answer`, whatever it does. */
function mockRunsAnswer(answer: () => unknown) {
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return CASES;
    if (cmd === "auto_run_load_script") {
      const id = (args as { caseId?: number; case_id?: number }).caseId ?? (args as { case_id: number }).case_id;
      return { case_id: id, title: "s", steps: STEPS };
    }
    if (cmd === "auto_run_list_runs") return answer();
    if (cmd === "auto_run_list_accounts") return [];
    if (cmd === "auto_run_load_recipe") return null;
    return null;
  });
}

test("until the runs are read the Last result buttons have no counts and cannot be pressed", async () => {
  mockRunsAnswer(() => new Promise(() => {})); // never answers
  renderScreen();
  await screen.findByText("Login - alpha");
  const group = await lastResultGroup();
  for (const name of ["Passed", "Failed", "Blocked", "Not run"]) {
    expect(within(group).getByRole("button", { name })).toBeDisabled();
  }
  fireEvent.click(within(group).getByRole("button", { name: "Failed" }));
  // No filter was applied: both cases are still there.
  expect(visibleTitles()).toHaveLength(2);
});

test("runs that cannot be read leave every case showing and say so in place of the row", async () => {
  mockRunsAnswer(() => {
    throw new Error("disk");
  });
  renderScreen();
  await screen.findByText("Login - alpha");
  expect(await screen.findByText("Past results could not be read")).toBeInTheDocument();
  expect(screen.queryByRole("group", { name: "Filter by last result" })).not.toBeInTheDocument();
  expect(visibleTitles()).toHaveLength(2);
  expect(screen.getByRole("checkbox", { name: "Select all shown" })).not.toHaveAttribute(
    "aria-disabled",
    "true",
  );
});

test("a refresh of the runs that hides a ticked case unticks it", async () => {
  let runs = [savedRun("a", "1000", [rec(1, "Passed"), rec(2, "Passed")])];
  mockRunsAnswer(() => runs);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <AutoRun org="acme" project="proj" pbi={pbi as never} />
    </QueryClientProvider>,
  );
  openTab("Test cases");
  await screen.findByText("Login - alpha");
  await press("Passed (2)");
  fireEvent.click(screen.getByRole("checkbox", { name: "Select all shown" }));
  expect(await screen.findByRole("button", { name: "Run 2 selected" })).toBeInTheDocument();

  // A newer run fails #1: under the Passed filter it is hidden now.
  runs = [...runs, savedRun("b", "2000", [rec(1, "Failed")])];
  await qc.invalidateQueries({ queryKey: ["autorun-runs"] });
  expect(await screen.findByRole("button", { name: "Run 1 selected" })).toBeInTheDocument();
  expect(visibleTitles()).toEqual(["Login - bravo"]);
});

test("the Last result choice survives a tab switch", async () => {
  mockFilterList();
  renderScreen();
  await screen.findByText("Login - alpha");
  await press("Failed (2)");
  openTab("Past runs");
  openTab("Test cases");
  expect(await screen.findByRole("button", { name: "Failed (2)" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  expect(visibleTitles()).toEqual(["Login - alpha", "Login - charlie"]);
});

test("the result filters are pressed and released one by one, each with its count", async () => {
  mockFilterList();
  renderScreen();
  const group = await lastResultGroup();
  const failed = await within(group).findByRole("button", { name: "Failed (2)" });
  const passed = within(group).getByRole("button", { name: "Passed (2)" });
  fireEvent.click(failed);
  expect(failed).toHaveAttribute("aria-pressed", "true");
  expect(passed).toHaveAttribute("aria-pressed", "false");
  // The count is part of the name, and stays when the button is pressed.
  expect(failed).toHaveAccessibleName("Failed (2)");
  fireEvent.click(failed);
  expect(failed).toHaveAttribute("aria-pressed", "false");
});

// ---- Search ----

const searchBox = () => screen.getByRole("textbox", { name: "Search test cases" });
const typeSearch = (text: string) => fireEvent.change(searchBox(), { target: { value: text } });

test("the search finds a case by its id, with or without #, and by its title in any case", async () => {
  mockFilterList();
  renderScreen();
  await screen.findByText("Login - alpha");

  typeSearch("#4");
  expect(visibleTitles()).toEqual(["Reports - delta"]);
  typeSearch("4");
  expect(visibleTitles()).toEqual(["Reports - delta"]);
  typeSearch("CHARLIE");
  expect(visibleTitles()).toEqual(["Login - charlie"]);
  typeSearch("reports");
  expect(visibleTitles()).toEqual(["Reports - delta", "Reports - echo"]);
});

test("the search narrows the list together with the result filters, grouped or not", async () => {
  mockFilterList();
  renderScreen();
  await screen.findByText("Login - alpha");
  await press("Failed (2)");
  typeSearch("alpha");
  expect(visibleTitles()).toEqual(["Login - alpha"]);

  fireEvent.click(screen.getByRole("checkbox", { name: "Group by title" }));
  expect(await screen.findByText("Login (1)")).toBeInTheDocument();
  expect(visibleTitles()).toEqual(["Login - alpha"]);
});

test("a search that matches nothing says so in its own words", async () => {
  mockFilterList();
  renderScreen();
  await screen.findByText("Login - alpha");
  typeSearch("  zulu ");
  expect(screen.getByText('No test cases match "zulu".')).toBeInTheDocument();
  expect(screen.queryByText("No cases match this filter.")).not.toBeInTheDocument();
  expect(visibleTitles()).toEqual([]);
});

test("Escape in the search box and the clear button both empty it", async () => {
  mockFilterList();
  renderScreen();
  await screen.findByText("Login - alpha");
  // Nothing to clear yet: no clear button.
  expect(screen.queryByRole("button", { name: "Clear search" })).not.toBeInTheDocument();

  typeSearch("alpha");
  expect(visibleTitles()).toHaveLength(1);
  fireEvent.keyDown(searchBox(), { key: "Escape" });
  expect(searchBox()).toHaveValue("");
  expect(visibleTitles()).toHaveLength(6);

  typeSearch("bravo");
  expect(visibleTitles()).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: "Clear search" }));
  expect(searchBox()).toHaveValue("");
  expect(visibleTitles()).toHaveLength(6);
});

test("Select all shown covers only the cases the search leaves, and a search that hides a ticked case unticks it", async () => {
  mockFilterList();
  renderScreen();
  await screen.findByText("Login - alpha");

  typeSearch("Login");
  fireEvent.click(screen.getByRole("checkbox", { name: "Select all shown" }));
  // #1, #2 and #6 have scripts; #3 does not, and Reports are not shown.
  expect(await screen.findByRole("button", { name: "Run 3 selected" })).toBeInTheDocument();

  typeSearch("alpha");
  expect(await screen.findByRole("button", { name: "Run 1 selected" })).toBeInTheDocument();
  typeSearch("");
  expect(screen.getByRole("checkbox", { name: "Select #2" })).toHaveAttribute("aria-checked", "false");
  expect(screen.getByRole("checkbox", { name: "Select #4" })).toHaveAttribute("aria-checked", "false");
});

// ---- Case cards ----

test("a collapsed card has no Script, Run or Add script button; opening it shows them", async () => {
  mockList([caseRow(1, "Alpha check"), caseRow(2, "Beta check")], [1]);
  renderScreen();
  await screen.findByText("Alpha check");
  expect(screen.queryByRole("button", { name: /script for #/ })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /^Run #/ })).not.toBeInTheDocument();

  // The title opens it, as the chevron does.
  fireEvent.click(screen.getByRole("button", { name: "Alpha check" }));
  expect(await screen.findByRole("button", { name: "Run #1" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Hide details for #1" })).toHaveAttribute("aria-expanded", "true");
  expect(screen.queryByRole("button", { name: "Add script for #2" })).not.toBeInTheDocument();

  openCard(2);
  expect(screen.getByText("No script yet")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Add script for #2" })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Hide details for #1" }));
  expect(screen.queryByRole("button", { name: "Run #1" })).not.toBeInTheDocument();
});

// One sticky control, bottom left like the other screens' own: Collapse all
// while any card is open, Expand all when none is.
test("Expand all opens every card shown, and Collapse all shuts them", async () => {
  mockFilterList();
  renderScreen();
  await screen.findByText("Login - alpha");
  const expand = await screen.findByRole("button", { name: "Expand all" });
  // Nothing is open, so there is nothing to collapse.
  expect(screen.queryByRole("button", { name: "Collapse all" })).not.toBeInTheDocument();

  // Only what the filter shows is opened.
  await press("Failed (2)");
  fireEvent.click(expand);
  expect(screen.getAllByRole("button", { name: /^Hide details for #/ }).map((b) => b.getAttribute("aria-label"))).toEqual([
    "Hide details for #1",
    "Hide details for #3",
  ]);
  // Cards are open, so the control now collapses, and cannot expand.
  expect(screen.queryByRole("button", { name: "Expand all" })).not.toBeInTheDocument();
  await press("Failed (2)");
  expect(screen.getByRole("button", { name: "Show details for #2" })).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Collapse all" }));
  expect(screen.queryByRole("button", { name: /^Hide details for #/ })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Collapse all" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Expand all" })).toBeInTheDocument();
});

test("the sticky Collapse all is pinned to the window, outside the screen", async () => {
  mockFilterList();
  renderScreen();
  await screen.findByText("Login - alpha");
  openCard(1);
  const collapse = await screen.findByRole("button", { name: "Collapse all" });
  const dock = collapse.parentElement as HTMLElement;
  expect(dock.parentElement).toBe(document.body);
  expect(dock).toHaveClass("fixed", "bottom-6");
  // Gone from the row above the list: the dock is the only one.
  expect(screen.getAllByRole("button", { name: "Collapse all" })).toHaveLength(1);
});

test("Collapse all still shuts cards a search or a filter hides", async () => {
  mockFilterList();
  renderScreen();
  await screen.findByText("Login - alpha");
  openCard(1);
  // The only open card is hidden now, but it is still open.
  typeSearch("bravo");
  expect(screen.queryByRole("button", { name: "Hide details for #1" })).not.toBeInTheDocument();
  const collapse = screen.getByRole("button", { name: "Collapse all" });
  expect(collapse).toBeEnabled();
  fireEvent.click(collapse);
  typeSearch("");
  expect(screen.getByRole("button", { name: "Show details for #1" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Collapse all" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Expand all" })).toBeInTheDocument();
});

test("the open cards are remembered for each PBI while the screen is mounted", async () => {
  mockList([caseRow(1, "Alpha check"), caseRow(2, "Beta check")], [1, 2]);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const other = { ...pbi, id: 43, title: "Other work" };
  const view = render(
    <QueryClientProvider client={qc}>
      <AutoRun org="acme" project="proj" pbi={pbi as never} />
    </QueryClientProvider>,
  );
  await screen.findByText("Alpha check");
  openCard(1);

  view.rerender(
    <QueryClientProvider client={qc}>
      <AutoRun org="acme" project="proj" pbi={other as never} />
    </QueryClientProvider>,
  );
  // Another PBI's list starts shut.
  expect(await screen.findByRole("button", { name: "Show details for #1" })).toBeInTheDocument();

  view.rerender(
    <QueryClientProvider client={qc}>
      <AutoRun org="acme" project="proj" pbi={pbi as never} />
    </QueryClientProvider>,
  );
  expect(await screen.findByRole("button", { name: "Hide details for #1" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Show details for #2" })).toBeInTheDocument();
});

test("an open card shows the script's facts and its numbered steps", async () => {
  mockList([caseRow(1, "Alpha check")], [1]);
  renderScreen();
  await screen.findByText("Alpha check");
  openCard(1);
  expect(await screen.findByRole("button", { name: "Run #1" })).toBeInTheDocument();
  expect(screen.getByText("1 step")).toBeInTheDocument();
  const steps = screen.getByRole("list", { name: "Script steps of #1" });
  expect(within(steps).getAllByRole("listitem")).toHaveLength(1);
  // A script with no account, area or marks prints no empty labels.
  expect(screen.queryByText("Runs as")).not.toBeInTheDocument();
  expect(screen.queryByText("Changes")).not.toBeInTheDocument();
  expect(screen.queryByText("Files")).not.toBeInTheDocument();
});

// ---- Setup card, header line, site address ----

const RECIPE = {
  start_url: "https://hr.example.internal/login",
  steps: [{ kind: "click", selector: { role: "button", name: "Login" } }],
  after_sign_in: [{ kind: "click", selector: { css: "#menu" } }],
  signed_in: { css: "#m" },
  allowed_origins: ["https://sso.example.internal"],
  session_minutes: 90,
};

/** A project that is fully set up: a recipe, two accounts, one module
 * path. `saves` records every recipe written, and the mock hands back the
 * last one written - the way the file on disk would. */
function mockSetUp(env?: ReturnType<typeof envList>) {
  const saves: { recipe: typeof RECIPE }[] = [];
  const envSaves: { env: Record<string, unknown> }[] = [];
  let envView = env;
  let recipe: unknown = RECIPE;
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return [caseRow(1, "Alpha check")];
    if (cmd === "auto_run_load_script") return null;
    if (cmd === "auto_run_list_runs") return [];
    if (cmd === "auto_run_list_accounts") {
      return [
        { key: "a", label: "A", username: "u", password: "p" },
        { key: "b", label: "B", username: "u", password: "p" },
      ];
    }
    if (cmd === "auto_run_load_nav") {
      return { direct_urls: true, modules: [{ module: "Leave", clicks: [], arrived: "", recorded: "" }] };
    }
    if (cmd === "env_list") return envView ?? null;
    if (cmd === "env_save") {
      const a = args as { env: Record<string, unknown> };
      envSaves.push(a);
      envView = { active: envView!.active, environments: [{ ...envView!.environments[0], ...a.env }] } as never;
      return envView;
    }
    if (cmd === "auto_run_load_recipe") return recipe;
    if (cmd === "auto_run_save_recipe") {
      const a = args as { recipe: typeof RECIPE };
      saves.push(a);
      recipe = a.recipe;
      return null;
    }
    return null;
  });
  return Object.assign(saves, { envSaves });
}

const row = (name: string) => screen.getByRole("group", { name });

function envList(start_url: string, allowed_origins: string[] = []) {
  return {
    active: "qa",
    environments: [
      { id: "qa", name: "QA", start_url, allowed_origins, db_id: "db", test_environment: false, has_default_password: false },
    ],
  };
}

test("the header names the active environment and the host of its address", async () => {
  savedRecipe = RECIPE;
  mockList([caseRow(1, "Login - valid credentials")], [1], [], (cmd) =>
    cmd === "env_list" ? envList("https://qa.example.com/start", ["https://sso.qa.example.com"]) : undefined,
  );
  renderScreen();
  expect(await screen.findByText("QA - qa.example.com")).toBeInTheDocument();
  expect(screen.getByText("QA - qa.example.com").parentElement).toHaveTextContent("Environment QA - qa.example.com");
  // The Setup card says the same address, not the recipe's.
  openSetup();
  const site = row("Site address");
  expect(await within(site).findByText("https://qa.example.com/start")).toBeInTheDocument();
  expect(within(site).queryByText("https://hr.example.internal/login")).not.toBeInTheDocument();
  expect(within(site).getByText(/\+1 allowed site/)).toBeInTheDocument();
});

test("an environment with no address of its own shows the recipe's host", async () => {
  savedRecipe = RECIPE;
  mockList([caseRow(1, "Login - valid credentials")], [1], [], (cmd) =>
    cmd === "env_list" ? envList("") : undefined,
  );
  renderScreen();
  expect(await screen.findByText("QA - hr.example.internal")).toBeInTheDocument();
  openSetup();
  expect(within(row("Site address")).getByText("https://hr.example.internal/login")).toBeInTheDocument();
});


test("the Setup card's Test files row counts the project's files, and Manage opens them", async () => {
  const listed: unknown[] = [];
  mockList([caseRow(1, "Alpha check")], [1], [], (cmd, args) => {
    if (cmd !== "test_files_list") return null;
    listed.push(args);
    return [
      { name: "appraisal.pdf", size: 1536, modified: "1" },
      { name: "cv.txt", size: 5, modified: "1" },
    ];
  });
  renderScreen();
  await screen.findByText("Alpha check");
  openSetup();

  expect(await within(row("Test files")).findByText("2 files")).toBeInTheDocument();
  expect(listed[0]).toEqual(expect.objectContaining({ organization: "acme", project: "proj" }));
  fireEvent.click(within(row("Test files")).getByRole("button", { name: "Manage test files" }));
  const dialog = await screen.findByRole("dialog", { name: "Test files" });
  expect(await within(dialog).findByText("appraisal.pdf")).toBeInTheDocument();
  fireEvent.click(within(dialog).getByRole("button", { name: "Close" }));
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Test files" })).not.toBeInTheDocument());
});

test("the Test files row reads None yet for a project with none", async () => {
  mockList([caseRow(1, "Alpha check")], [1]);
  renderScreen();
  await screen.findByText("Alpha check");
  openSetup();
  expect(await within(row("Test files")).findByText("None yet")).toBeInTheDocument();
  expect(within(row("Test files")).getByRole("button", { name: "Manage test files" })).toBeEnabled();
});

test("the Setup card shows each row's state for a project with nothing set up", async () => {
  mockList([caseRow(1, "Alpha check")], [1]);
  renderScreen();
  await screen.findByText("Alpha check");
  // The strip on Test cases says there is no address yet.
  expect(await screen.findByText("no site set yet")).toBeInTheDocument();
  openSetup();

  expect(screen.getByRole("heading", { name: "Setup" })).toBeInTheDocument();
  await waitFor(() => expect(within(row("Site address")).getByText("Not set up yet")).toBeInTheDocument());
  // No saved recipe: the app's own sign-in is used.
  expect(within(row("Sign-in")).getByText("Built-in")).toBeInTheDocument();
  expect(within(row("Accounts")).getByText("None yet")).toBeInTheDocument();
  // The nav query answers null in this mock: an answer, so the row reads
  // "none" rather than sitting on "Loading…" forever.
  expect(await within(row("Areas")).findByText("None recorded yet")).toBeInTheDocument();
  expect(within(row("Areas")).queryByText("Loading…")).not.toBeInTheDocument();
  expect(within(row("Areas")).getByRole("button", { name: "Edit areas" })).toBeEnabled();

  // The address is the environment's, so it is set here with or without a
  // saved recipe - the built-in sign-in needs nothing else.
  expect(within(row("Site address")).queryByRole("button", { name: "Set up sign-in" })).not.toBeInTheDocument();
  fireEvent.click(within(row("Site address")).getByRole("button", { name: "Edit site address" }));
  expect(await screen.findByRole("heading", { name: "Site address" })).toBeInTheDocument();
  expect(screen.queryByRole("heading", { name: "Record sign-in" })).not.toBeInTheDocument();
});

test("with no saved recipe the built-in signs in at the environment's address", async () => {
  mockList([caseRow(1, "Alpha check")], [1], [], (cmd) =>
    cmd === "env_list" ? envList("https://qa.example.com/start") : undefined,
  );
  renderScreen();
  await screen.findByText("Alpha check");
  expect(await screen.findByText("QA - qa.example.com")).toBeInTheDocument();
  openSetup();
  expect(await within(row("Site address")).findByText("https://qa.example.com/start")).toBeInTheDocument();
  expect(within(row("Sign-in")).getByText("Built-in")).toBeInTheDocument();
  // Recording or editing saves the project's own recipe, which replaces it.
  expect(within(row("Sign-in")).getByRole("button", { name: "Record sign-in" })).toBeEnabled();
  expect(within(row("Sign-in")).getByRole("button", { name: "Edit sign-in recipe" })).toBeEnabled();
});

test("the Setup card and the readiness strip read a project that is set up", async () => {
  mockSetUp();
  renderScreen();
  await screen.findByText("Alpha check");

  // The strip on Test cases: the host the runs go to.
  expect(await screen.findByText("hr.example.internal")).toBeInTheDocument();
  // The counts are the Setup panel's, not the strip's.
  expect(screen.queryByText("2 accounts")).not.toBeInTheDocument();
  expect(screen.queryByText("1 area")).not.toBeInTheDocument();
  // The old header line's project name is gone.
  expect(screen.queryByText("proj")).not.toBeInTheDocument();

  openSetup();

  const site = row("Site address");
  expect(await within(site).findByText("https://hr.example.internal/login")).toBeInTheDocument();
  expect(within(site).getByText("+1 allowed site")).toBeInTheDocument();
  expect(within(row("Sign-in")).getByText("Recipe saved")).toBeInTheDocument();
  expect(within(row("Sign-in")).getByRole("button", { name: "Edit sign-in recipe" })).toBeInTheDocument();
  expect(within(row("Sign-in")).getByRole("button", { name: "Record sign-in" })).toBeInTheDocument();
  expect(await within(row("Accounts")).findByText("2 accounts on this machine")).toBeInTheDocument();
  // One wording for the same count, in the row and in the header.
  expect(await within(row("Areas")).findByText("1 area recorded")).toBeInTheDocument();

});

test("saving a new site address writes it to the active environment, not the recipe, and updates the Setup row and the readiness strip", async () => {
  const saves = mockSetUp(envList(""));
  renderScreen();
  await screen.findByText("Alpha check");
  // No address of its own yet: the strip shows the recipe's host.
  expect(await screen.findByText("QA - hr.example.internal")).toBeInTheDocument();

  openSetup();
  fireEvent.click(await screen.findByRole("button", { name: "Edit site address" }));
  expect(await screen.findByRole("heading", { name: "Site address" })).toBeInTheDocument();
  const start = screen.getByRole("textbox", { name: "Start address" }) as HTMLInputElement;
  expect(await screen.findByText("Using the sign-in recipe's address")).toBeInTheDocument();
  fireEvent.change(start, { target: { value: "https://people.example.org/" } });
  fireEvent.change(screen.getByRole("textbox", { name: "Also allowed" }), {
    target: { value: "https://sso.example.org\n\n  https://cdn.example.org  " },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));

  await waitFor(() => expect(saves.envSaves).toHaveLength(1));
  expect(saves.envSaves[0].env).toEqual(
    expect.objectContaining({
      id: "qa",
      start_url: "https://people.example.org/",
      allowed_origins: ["https://sso.example.org", "https://cdn.example.org"],
    }),
  );
  // The recipe file is untouched.
  expect(saves).toHaveLength(0);
  await waitFor(() =>
    expect(screen.queryByRole("heading", { name: "Site address" })).not.toBeInTheDocument(),
  );
  expect(await within(row("Site address")).findByText("https://people.example.org/")).toBeInTheDocument();
  expect(within(row("Site address")).getByText("+2 allowed sites")).toBeInTheDocument();
  openTab("Test cases");
  expect(await screen.findByText("QA - people.example.org")).toBeInTheDocument();
});

test("the selection's actions live in the shared dock, in place and as an aria-hidden floating copy", async () => {
  mockList([caseRow(1, "Alpha check"), caseRow(2, "Beta check")], [1, 2]);
  renderScreen();
  await screen.findByText("Alpha check");

  // No selection, no dock - neither copy.
  expect(document.querySelector("[data-sticky-action]")).toBeNull();

  fireEvent.click(screen.getByRole("checkbox", { name: "Select #1" }));
  const run = await screen.findByRole("button", { name: "Run 1 selected" });
  const unattended = screen.getByRole("button", { name: "Run 1 unattended" });
  const clear = screen.getByRole("button", { name: "Clear selection" });
  // The in-place row holds all three, together.
  expect(run.parentElement).toContainElement(unattended);
  expect(run.parentElement).toContainElement(clear);
  // No sticky bar at the top any more: the in-place row comes after the list.
  const lastCase = screen.getByText("Beta check").closest("li")!;
  expect(lastCase.compareDocumentPosition(run) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();

  // ActionDock's floating copy: portalled, labelled for tooling, and never
  // in the accessibility tree (the in-place row already is).
  const floating = document.querySelector("[data-sticky-action]")!;
  expect(floating).toHaveAttribute("aria-label", "Run selection");
  expect(floating).toHaveAttribute("aria-hidden", "true");
  expect(within(floating as HTMLElement).getByText("Run 1 selected")).toBeInTheDocument();
  expect(screen.getAllByRole("button", { name: "Run 1 selected" })).toHaveLength(1);

  fireEvent.click(clear);
  await waitFor(() => expect(document.querySelector("[data-sticky-action]")).toBeNull());
});

test("Clear results lives in the Past runs section, and Clear scripts and Import scripts in the test cases' More menu", async () => {
  mockList([caseRow(1, "Alpha check")], [1]);
  renderScreen();
  await screen.findByText("Alpha check");

  const testCases = screen.getByRole("heading", { name: /^Test cases/ }).closest("section")!;
  expect(within(testCases).getByRole("button", { name: "More" })).toBeInTheDocument();
  expect(moreItem("Clear scripts")).toBeInTheDocument();
  expect(screen.getByRole("menuitem", { name: "Import scripts" })).toBeInTheDocument();
  expect(within(testCases).queryByRole("button", { name: "Clear results" })).not.toBeInTheDocument();
  expect(screen.queryByRole("menuitem", { name: "Clear results" })).not.toBeInTheDocument();

  openTab("Past runs");
  const pastRuns = (await screen.findByRole("heading", { name: "Past runs" })).closest("section")!;
  expect(within(pastRuns).getByRole("button", { name: "Clear results" })).toBeInTheDocument();
  // The "saved on this machine" note belongs with the runs it describes.
  expect(within(pastRuns).getByText(/nothing goes to azure devops unless you press send/i)).toBeInTheDocument();
});

test("with no project picked, the project-bound Setup buttons are disabled and say why", async () => {
  mockList([caseRow(1, "Alpha check")], [1]);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <AutoRun org="acme" project="" pbi={pbi as never} />
    </QueryClientProvider>,
  );
  openSetup();
  await screen.findByRole("button", { name: "Edit site address" });

  const why = "Pick an organization and project first";
  for (const name of ["Edit site address", "Record sign-in", "Edit sign-in recipe", "Edit areas"]) {
    const b = screen.getByRole("button", { name });
    expect(b).toBeDisabled();
    expect(b).toHaveAttribute("title", why);
  }
  // Accounts belong to this machine, not a project - never gated on one.
  expect(screen.getByRole("button", { name: "Edit accounts" })).toBeEnabled();
  expect(within(row("Site address")).getByText(why)).toBeInTheDocument();
});

// The screen used to be two columns from xl (setup and cases left, Past
// runs right), then three tabs. It is two tabs now: the Setup panel lives
// on the Test cases tab beside the list, and Past runs is a panel of its
// own. Where the panel sits is layout, which jsdom cannot see; which panel
// holds it is not.
test("the case list with its Setup panel, and Past runs, are one tab panel each, shown one at a time", async () => {
  mockList([caseRow(1, "Login - valid credentials")], [1]);
  renderScreen();
  await screen.findByText("Login - valid credentials");

  const only = (name: string) => {
    const panels = screen.getAllByRole("tabpanel");
    expect(panels).toHaveLength(1);
    expect(panels[0]).toHaveAccessibleName(new RegExp(`^${name}`));
    return panels[0];
  };
  const cases = only("Test cases");
  expect(within(cases).getByRole("heading", { name: /^Test cases/ })).toBeInTheDocument();
  expect(within(cases).getByRole("region", { name: "Setup" })).toBeInTheDocument();
  expect(screen.queryByRole("tab", { name: /^Setup/ })).not.toBeInTheDocument();
  expect(screen.queryByRole("heading", { name: "Past runs" })).not.toBeInTheDocument();

  openTab("Past runs");
  expect(within(only("Past runs")).getByRole("heading", { name: "Past runs" })).toBeInTheDocument();
  expect(within(only("Past runs")).getByRole("region", { name: "Setup" })).toBeInTheDocument();

  expect(document.querySelector('[class*="xl:grid-cols-"]')).toBeNull();
});

test("Past runs has the Setup panel beside it, with a working toggle, and its open state carries across tabs", async () => {
  mockList([caseRow(1, "Login - valid credentials")], [1]);
  renderScreen();
  await screen.findByText("Login - valid credentials");

  openTab("Past runs");
  const panel = within(screen.getByRole("tabpanel")).getByRole("region", { name: "Setup" });
  expect(within(panel).getByRole("button", { name: "Hide setup details" })).toHaveAttribute("aria-expanded", "true");
  fireEvent.click(within(panel).getByRole("button", { name: "Hide setup details" }));
  expect(within(panel).getByRole("button", { name: "Show setup details" })).toHaveAttribute("aria-expanded", "false");

  // Shut on one tab, shut on the other.
  openTab("Test cases");
  expect(screen.getByRole("button", { name: "Show setup details" })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Show setup details" }));
  openTab("Past runs");
  expect(screen.getByRole("button", { name: "Hide setup details" })).toBeInTheDocument();
});

// A store answering (a script, the runs) redraws the screen. The check that
// drops hidden cases from the selection runs after every one of those draws,
// and it used to set the selection each time even with nothing to drop: one
// more update queued per draw. With hundreds of scripts answering one after
// another, React counted that chain as an endless loop (error #185) and the
// screen crashed. Nothing to drop must mean no update at all.
test("a redraw with no hidden case ticked queues no selection update", async () => {
  mockList([caseRow(1, "Login - valid credentials"), caseRow(2, "Login - locked account")], [1, 2]);
  let draws = 0;
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <Profiler id="autorun" onRender={() => draws++}>
        <AutoRun org="acme" project="proj" pbi={pbi as never} />
      </Profiler>
    </QueryClientProvider>,
  );
  openTab("Test cases");
  fireEvent.click(await screen.findByRole("checkbox", { name: "Select #1" }));
  await waitFor(() => expect(screen.getByRole("checkbox", { name: "Select #1" })).toBeChecked());
  await new Promise((r) => setTimeout(r, 50));

  const before = draws;
  act(() => {
    qc.setQueryData(["autorun-runs"], [{ id: "r1", started_at: "1", cases: [{ case_id: 2, verdict: "Passed" }] }]);
  });
  await new Promise((r) => setTimeout(r, 50));
  expect(draws - before).toBe(1);
  expect(screen.getByRole("checkbox", { name: "Select #1" })).toBeChecked();
});

test("the Save words row shows the built-in words and the project's own, and Edit opens its dialog", async () => {
  const builtIn = ["save", "update", "delete", "submit", "approve", "publish", "assign"];
  mockList([caseRow(1, "Login - valid credentials")], [1], [], (cmd) =>
    cmd === "auto_run_load_nav"
      ? { direct_urls: true, modules: [], save_words: ["recalc"], built_in_save_words: builtIn }
      : null,
  );
  renderScreen();
  await screen.findByText("Login - valid credentials");
  openSetup();
  const words = row("Save words");
  await waitFor(() => expect(words).toHaveTextContent(`${builtIn.join(", ")}, recalc`));
  fireEvent.click(within(words).getByRole("button", { name: "Edit save words" }));
  expect(await screen.findByRole("heading", { name: "Save words" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Remove save word recalc" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Remove save word save" })).not.toBeInTheDocument();
});

/** An unattended run of this PBI in which case 1 failed at step 2. */
const FAILED_RUN = {
  id: "run-1",
  pbi_id: 42,
  started_at: "1786000200000",
  mode: "unattended",
  published: null,
  cases: [
    {
      case_id: 1,
      title: "Alpha check",
      verdict: "",
      note: "",
      proposed: "Failed",
      reason: "step 2: button not found",
      steps: [
        { step_number: 1, outcomes: [{ ok: true, detail: "clicked Edit" }] },
        { step_number: 2, outcomes: [{ ok: false, detail: "button not found" }] },
      ],
    },
  ],
};

function mockReplayList() {
  const replays: unknown[] = [];
  mockList([caseRow(1, "Alpha check")], [1], [FAILED_RUN], (cmd, args) => {
    if (cmd === "auto_run_load_run") return FAILED_RUN;
    if (cmd === "auto_run_replay_to_step") {
      replays.push(args);
      return new Promise(() => {});
    }
    return null;
  });
  return replays;
}

test("Replay in Past runs opens the supervised pane on that case and starts the replay", async () => {
  const replays = mockReplayList();
  renderScreen();
  await screen.findByText("Alpha check");
  openTab("Past runs");

  fireEvent.click(await screen.findByRole("button", { name: "Replay to step 2 for case 1" }));

  expect(await screen.findByRole("button", { name: "Stop replay" })).toBeInTheDocument();
  expect(screen.getByRole("heading", { name: "#1 Alpha check" })).toBeInTheDocument();
  await waitFor(() =>
    expect(replays).toEqual([{ organization: "acme", project: "proj", caseId: 1, step: 2, dbReadAccess: true }]),
  );
});

test("Replay in the review closes the review and shows the pane replaying", async () => {
  const replays = mockReplayList();
  renderScreen();
  await screen.findByText("Alpha check");
  openTab("Past runs");

  fireEvent.click(await screen.findByRole("button", { name: "Review" }));
  const review = await screen.findByRole("listitem", { name: "Case #1 Alpha check" });
  fireEvent.click(within(review).getByRole("button", { name: "Replay to step 2 for case 1" }));

  expect(await screen.findByRole("button", { name: "Stop replay" })).toBeInTheDocument();
  await waitFor(() =>
    expect(screen.queryByRole("listitem", { name: "Case #1 Alpha check" })).not.toBeInTheDocument(),
  );
  await waitFor(() => expect(replays).toHaveLength(1));
});

/** A run paused before case 2 to put "cycle published" back. */
const WAITING = {
  run_id: "run-7",
  before_case_id: 2,
  names: ["cycle published"],
  changed_by: [["cycle published", [1]]],
  remaining: [2, 9],
};

/** The screen's own answers, plus a run waiting at a reset point when
 * `waiting` says so. Answers to the pause go to `answers`. */
function mockWaiting(waiting: { current: unknown }, answers: { runId: string; continueRun: boolean }[]) {
  mockIPC(
    (cmd, args) => {
      if (cmd === "list_test_case_fields") return [];
      if (cmd === "pbi_test_cases_full") return [caseRow(1, "Publish the cycle"), caseRow(2, "Edit a draft cycle")];
      if (cmd === "auto_run_list_runs") return [];
      if (cmd === "auto_run_list_accounts") return [];
      if (cmd === "auto_run_waiting_reset") return waiting.current;
      if (cmd === "auto_run_answer_reset") {
        answers.push(args as { runId: string; continueRun: boolean });
        waiting.current = null;
        return null;
      }
      return null;
    },
    { shouldMockEvents: true },
  );
}

test("coming back to Auto Run finds a run paused at a reset point, and Continue answers it", async () => {
  const waiting = { current: WAITING as unknown };
  const answers: { runId: string; continueRun: boolean }[] = [];
  mockWaiting(waiting, answers);
  const first = renderScreen();
  await screen.findByRole("region", { name: "Reset needed" });
  // The person leaves Auto Run and comes back: the pause is found again.
  first.unmount();
  renderScreen();
  const panel = await screen.findByRole("region", { name: "Reset needed" });
  expect(
    within(panel).getByText('Reset: revert "cycle published" (changed by #1 Publish the cycle)'),
  ).toBeInTheDocument();
  // A case the list does not show falls back to its id.
  const left = within(panel).getByRole("list", { name: "Cases still to run" });
  expect(within(left).getAllByRole("listitem").map((li) => li.textContent)).toEqual([
    "#2 Edit a draft cycle",
    "#9",
  ]);
  fireEvent.click(within(panel).getByRole("button", { name: "Continue after reset" }));
  await waitFor(() => expect(answers).toEqual([{ runId: "run-7", continueRun: true }]));
  await waitFor(() => expect(screen.queryByRole("region", { name: "Reset needed" })).not.toBeInTheDocument());
});

test("Stop on a paused run found again answers Stop", async () => {
  const answers: { runId: string; continueRun: boolean }[] = [];
  mockWaiting({ current: WAITING }, answers);
  renderScreen();
  fireEvent.click(await screen.findByRole("button", { name: "Stop the run at this reset" }));
  await waitFor(() => expect(answers).toEqual([{ runId: "run-7", continueRun: false }]));
  await waitFor(() => expect(screen.queryByRole("region", { name: "Reset needed" })).not.toBeInTheDocument());
});

test("a pause that comes while the run's own dialog is closed shows the panel", async () => {
  mockWaiting({ current: null }, []);
  renderScreen();
  await screen.findByText("Publish the cycle");
  expect(screen.queryByRole("region", { name: "Reset needed" })).not.toBeInTheDocument();
  const { emit } = await import("@tauri-apps/api/event");
  await act(async () => {
    await emit("autorun-reset-needed", WAITING);
  });
  expect(await screen.findByRole("region", { name: "Reset needed" })).toBeInTheDocument();
});

// ---- Discovery holds the Auto Run browser ----

test("run_buttons_are_disabled_while_discovery_is_active", async () => {
  let active = true;
  mockIPC(
    (cmd, args) => {
      if (cmd === "auto_run_discovery_active") return active;
      if (cmd === "list_test_case_fields") return [];
      if (cmd === "pbi_test_cases_full") return [caseRow(1, "Alpha check")];
      if (cmd === "auto_run_load_script") return { case_id: 1, title: "s", steps: STEPS };
      if (cmd === "auto_run_list_runs") return [];
      if (cmd === "auto_run_list_accounts") return [];
      void args;
      return null;
    },
    { shouldMockEvents: true },
  );
  renderScreen();
  await screen.findByText("Alpha check");

  fireEvent.click(screen.getByRole("checkbox", { name: "Select #1" }));
  const supervised = await screen.findByRole("button", { name: "Run 1 selected" });
  const unattended = screen.getByRole("button", { name: "Run 1 unattended" });
  await waitFor(() => expect(supervised).toBeDisabled());
  expect(unattended).toBeDisabled();
  expect(supervised).toHaveAttribute("title", "Discovery is using the Auto Run browser");
  expect(unattended).toHaveAttribute("title", "Discovery is using the Auto Run browser");

  openCard(1);
  const cardRun = screen.getByRole("button", { name: "Run #1" });
  expect(cardRun).toBeDisabled();
  expect(cardRun).toHaveAttribute("title", "Discovery is using the Auto Run browser");
  fireEvent.click(cardRun);
  expect(screen.queryByRole("button", { name: "Open browser" })).not.toBeInTheDocument();

  // The discovery ends: the runs are back.
  active = false;
  const { emit } = await import("@tauri-apps/api/event");
  await act(async () => {
    await emit("autorun-discovery-changed", { active: false });
  });
  await waitFor(() => expect(screen.getByRole("button", { name: "Run 1 selected" })).toBeEnabled());
  expect(screen.getByRole("button", { name: "Run 1 unattended" })).toBeEnabled();
  expect(screen.getByRole("button", { name: "Run #1" })).toBeEnabled();
  expect(screen.getByRole("button", { name: "Run #1" })).not.toHaveAttribute("title");
});
