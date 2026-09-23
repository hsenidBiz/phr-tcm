// Grouping and bulk selection on the Auto Run case list.
//
// The screen reads the case list from Azure DevOps and writes nothing back
// to it, so every assertion here is about local state: which rows are
// ticked, which groups are shut, and what the Run button hands to RunPane.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { toast } from "../../lib/toast";
import AutoRun from "./index";

vi.mock("../../lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() } }));

afterEach(() => {
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
    if (cmd === "auto_run_load_recipe") return null;
    if (onCommand) return onCommand(cmd, args);
    return null;
  });
}

function renderScreen() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <AutoRun org="acme" project="proj" pbi={pbi as never} />
    </QueryClientProvider>,
  );
}

test("the Accounts and Sign-in recipe buttons open their own dialogs", async () => {
  mockList([caseRow(1, "Login - valid credentials")], [1]);
  renderScreen();
  await screen.findByText("Login - valid credentials");

  fireEvent.click(screen.getByRole("button", { name: "Accounts" }));
  expect(await screen.findByRole("heading", { name: "Accounts" })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  await waitFor(() =>
    expect(screen.queryByRole("heading", { name: "Accounts" })).not.toBeInTheDocument(),
  );

  fireEvent.click(screen.getByRole("button", { name: "Sign-in recipe" }));
  expect(await screen.findByRole("heading", { name: "Sign-in recipe" })).toBeInTheDocument();
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
  // inert, and Run is not offered at all.
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

test("Clear drops the selection without opening a run", async () => {
  mockList([caseRow(1, "Alpha check")], [1]);
  renderScreen();
  await screen.findByText("Alpha check");

  fireEvent.click(screen.getByRole("checkbox", { name: "Select #1" }));
  expect(await screen.findByRole("button", { name: "Run 1 selected" })).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Clear" }));
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

// ---- Clear scripts / Clear results (development build only) ------------

test("Clear scripts and Clear results are disabled when there is nothing to clear", async () => {
  // No case has a script, and no run exists on this machine.
  mockList([caseRow(1, "Alpha check"), caseRow(2, "Beta check")], []);
  renderScreen();
  await screen.findByText("Alpha check");

  expect(screen.getByRole("button", { name: "Clear scripts" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Clear results" })).toBeDisabled();
});

test("Clear scripts opens its confirm with the exact sentence; Cancel calls nothing", async () => {
  let calls = 0;
  mockList([caseRow(1, "Alpha check"), caseRow(2, "Beta check")], [1], [], (cmd) => {
    if (cmd === "auto_run_clear_scripts") calls += 1;
    return null;
  });
  renderScreen();
  await screen.findByText("Alpha check");

  fireEvent.click(screen.getByRole("button", { name: "Clear scripts" }));
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

  fireEvent.click(screen.getByRole("button", { name: "Clear scripts" }));
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

  fireEvent.click(screen.getByRole("button", { name: "Clear results" }));
  expect(
    await screen.findByText(
      "This removes every Auto Run result and picture on this machine, including runs already sent to Azure DevOps (those stay there). Nothing in Azure DevOps changes.",
    ),
  ).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Clear 1 run" }));
  await waitFor(() => expect(toast.success).toHaveBeenCalledWith("1 run removed."));
});
