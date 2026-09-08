// Grouping and bulk selection on the Auto Run case list.
//
// The screen reads the case list from Azure DevOps and writes nothing back
// to it, so every assertion here is about local state: which rows are
// ticked, which groups are shut, and what the Run button hands to RunPane.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import AutoRun from "./index";

afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.restoreAllMocks();
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
 * case answers null the way an unscripted one does. */
function mockList(cases: ReturnType<typeof caseRow>[], scripted: number[]) {
  mockIPC((cmd, args) => {
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "pbi_test_cases_full") return cases;
    if (cmd === "auto_run_load_script") {
      const id = (args as { caseId?: number; case_id?: number }).caseId ?? (args as { case_id: number }).case_id;
      return scripted.includes(id) ? { case_id: id, title: "s", steps: STEPS } : null;
    }
    if (cmd === "auto_run_list_runs") return [];
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
