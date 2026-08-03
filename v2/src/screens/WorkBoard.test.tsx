import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import WorkBoard from "./WorkBoard";

afterEach(() => clearMocks());

function renderBoard() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <WorkBoard org="acme" project="Web" />
    </QueryClientProvider>,
  );
}

const boardData = {
  items: [
    {
      id: 11,
      title: "Write docs",
      work_item_type: "Task",
      state: "To Do",
      state_color: "b2b2b2",
      column: "To Do",
      assigned_to: "Avin",
      tags: "",
      priority: 2,
      changed_date: "2026-07-11T00:00:00Z",
    },
    {
      id: 12,
      title: "Fix bug",
      work_item_type: "Bug",
      state: "Done",
      state_color: "339933",
      column: "Done",
      assigned_to: "Avin",
      tags: "",
      priority: 1,
      changed_date: "2026-07-11T01:00:00Z",
    },
  ],
  states_by_type: {
    Task: [
      { name: "To Do", color: "b2b2b2", category: "Proposed" },
      { name: "In Progress", color: "007acc", category: "InProgress" },
      { name: "Done", color: "339933", category: "Completed" },
    ],
  },
};

test("items land in their columns", async () => {
  mockIPC((cmd) => {
    if (cmd === "fetch_board") return boardData;
    if (cmd === "classification_paths") return [];
  });
  renderBoard();
  const todo = await screen.findByTestId("col-To Do");
  expect(within(todo).getByText("Write docs")).toBeInTheDocument();
  const done = screen.getByTestId("col-Done");
  expect(within(done).getByText("Fix bug")).toBeInTheDocument();
});

test("drop moves card and applies the returned state", async () => {
  let moved: unknown = null;
  mockIPC((cmd, args) => {
    if (cmd === "fetch_board") return boardData;
    if (cmd === "classification_paths") return [];
    if (cmd === "move_board_item") {
      moved = args;
      return "In Progress";
    }
  });
  renderBoard();
  const card = await screen.findByText("Write docs");
  fireEvent.dragStart(card.closest("[draggable]")!);
  const target = screen.getByTestId("col-In Progress");
  fireEvent.drop(target);

  // Optimistically moved, then state text updates from the command result.
  const movedTitle = await within(target).findByText("Write docs");
  const movedCard = movedTitle.closest("[draggable]") as HTMLElement;
  expect(await within(movedCard).findByText("In Progress")).toBeInTheDocument();
  expect(moved).toMatchObject({ itemId: 11, workItemType: "Task", column: "In Progress" });
});

test("text filter narrows visible cards", async () => {
  mockIPC((cmd) => {
    if (cmd === "fetch_board") return boardData;
    if (cmd === "classification_paths") return [];
  });
  renderBoard();
  await screen.findByText("Write docs");
  fireEvent.change(screen.getByLabelText("Filter items"), { target: { value: "bug" } });
  expect(screen.queryByText("Write docs")).not.toBeInTheDocument();
  expect(screen.getByText("Fix bug")).toBeInTheDocument();
});

test("card click opens the drawer; save patches only dirty fields", async () => {
  let patched: { patches?: Array<{ reference_name: string; value: string }> } = {};
  mockIPC((cmd, args) => {
    if (cmd === "fetch_board") return boardData;
    if (cmd === "classification_paths") return [];
    if (cmd === "work_item_detail")
      return {
        id: 11,
        title: "Write docs",
        work_item_type: "Task",
        state: "To Do",
        assigned_to: "Avin",
        assigned_to_unique: "a@x.com",
        activity: "",
        tags: "",
        area_path: "P",
        iteration_path: "P\\S1",
        remaining_work: null,
        completed_work: null,
        original_estimate: null,
        start_date: "",
        finish_date: "",
        description_text: "old text",
        description_html: "<div>old text</div>",
        description_field: "System.Description",
        extra_pages: [],
        extra_pages_error: null,
        inline_images: [],
      };
    if (cmd === "list_team_members")
      return [{ display_name: "Avin", unique_name: "a@x.com" }];
    if (cmd === "activity_values") return [];
    if (cmd === "work_item_comments") return [];
    if (cmd === "update_work_item") {
      patched = args as typeof patched;
      return null;
    }
  });
  renderBoard();
  fireEvent.click(await screen.findByText("Write docs"));

  const title = await screen.findByLabelText(/Title/);
  fireEvent.change(title, { target: { value: "Write better docs" } });
  fireEvent.click(screen.getByRole("button", { name: "Save changes" }));

  await vi.waitFor(() => expect(patched.patches).toBeTruthy());
  expect(patched.patches).toEqual([
    { reference_name: "System.Title", value: "Write better docs" },
  ]);
});

test("bug drawer shows RCA / Preventive Measures tabs and saves their edits", async () => {
  let patched: { patches?: Array<{ reference_name: string; value: string }> } = {};
  mockIPC((cmd, args) => {
    if (cmd === "fetch_board") return boardData;
    if (cmd === "classification_paths") return [];
    if (cmd === "work_item_detail")
      return {
        id: 12,
        title: "Fix bug",
        work_item_type: "Bug",
        state: "Done",
        assigned_to: "Avin",
        assigned_to_unique: "a@x.com",
        activity: "",
        tags: "",
        area_path: "P",
        iteration_path: "P\S1",
        remaining_work: null,
        completed_work: null,
        original_estimate: null,
        start_date: "",
        finish_date: "",
        description_text: "repro",
        description_html: "<div>repro</div>",
        description_field: "Microsoft.VSTS.TCM.ReproSteps",
        extra_pages: [
          {
            name: "RCA",
            fields: [
              {
                label: "Initial Findings",
                reference_name: "Custom.InitialFindings",
                section: 0,
                kind: "html",
                allowed: [],
                value: "<div>null ref</div>",
              },
              {
                label: "Root Cause Category",
                reference_name: "Custom.RootCauseCategory",
                section: 1,
                kind: "pick",
                allowed: ["Code Defect", "Design/Requirement"],
                value: "",
              },
            ],
          },
          {
            name: "Preventive Measures",
            fields: [
              {
                label: "Lessons Learned",
                reference_name: "Custom.LessonsLearned",
                section: 0,
                kind: "html",
                allowed: [],
                value: "",
              },
            ],
          },
        ],
        extra_pages_error: null,
        inline_images: [],
      };
    if (cmd === "list_team_members") return [{ display_name: "Avin", unique_name: "a@x.com" }];
    if (cmd === "activity_values") return [];
    if (cmd === "work_item_comments") return [];
    if (cmd === "update_work_item") {
      patched = args as typeof patched;
      return null;
    }
  });
  renderBoard();
  fireEvent.click(await screen.findByText("Fix bug"));

  // The RCA page shows ALL its fields: rich text as markdown + a picklist.
  // Rich text opens in Preview (rendered); switch to Write to edit.
  const rcaTab = await screen.findByRole("button", { name: "RCA" });
  fireEvent.click(rcaTab);
  expect(screen.queryByLabelText("Initial Findings (markdown)")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Write" }));
  const findings = screen.getByLabelText("Initial Findings (markdown)");
  expect(findings).toHaveValue("null ref");
  fireEvent.change(findings, { target: { value: "null ref in save path" } });
  fireEvent.click(screen.getByLabelText("Root Cause Category"));
  fireEvent.click(screen.getByRole("option", { name: "Code Defect" }));

  // The empty Preventive Measures page is editable too.
  fireEvent.click(screen.getByRole("button", { name: "Preventive Measures" }));
  fireEvent.change(screen.getByLabelText("Lessons Learned (markdown)"), {
    target: { value: "add a guard test" },
  });

  fireEvent.click(screen.getByRole("button", { name: "Save changes" }));
  await vi.waitFor(() => expect(patched.patches).toBeTruthy());
  expect(patched.patches).toHaveLength(3);
  expect(patched.patches![0].reference_name).toBe("Custom.InitialFindings");
  expect(patched.patches![0].value).toContain("null ref in save path");
  // Picklist saves the raw value, not HTML.
  expect(patched.patches![1]).toEqual({
    reference_name: "Custom.RootCauseCategory",
    value: "Code Defect",
  });
  expect(patched.patches![2].reference_name).toBe("Custom.LessonsLearned");
  expect(patched.patches![2].value).toContain("add a guard test");
});

test("PBI scope waits for a pick, then fetches with pbiId", async () => {
  const calls: Array<Record<string, unknown>> = [];
  mockIPC((cmd, args) => {
    if (cmd === "fetch_board") {
      calls.push(args as Record<string, unknown>);
      return boardData;
    }
    if (cmd === "classification_paths") return [];
    if (cmd === "search_pbis")
      return [{ id: 4242, title: "Login flow", work_item_type: "Product Backlog Item" }];
  });
  renderBoard();
  await screen.findByTestId("col-To Do");

  // Switch scope to By PBI: the board stops fetching and prompts instead.
  fireEvent.click(screen.getByLabelText("Board scope"));
  fireEvent.click(await screen.findByText("By PBI…"));
  expect(await screen.findByText(/Pick a PBI above/)).toBeInTheDocument();

  // Pick one via the search picker: the fetch carries its id.
  const find = screen.getByLabelText("Find PBI");
  fireEvent.change(find, { target: { value: "Login" } });
  fireEvent.keyDown(find, { key: "Enter" });
  fireEvent.click(await screen.findByText(/Login flow/));
  await screen.findByText(/items? under/);
  const last = calls[calls.length - 1];
  expect(last.pbiId).toBe(4242);
  expect(last.area).toBeNull();
});

test("stale cards get the warning edge; Done cards never do", async () => {
  const staleDate = new Date(Date.now() - 12 * 86_400_000).toISOString();
  mockIPC((cmd) => {
    if (cmd === "fetch_board")
      return {
        items: [
          { ...boardData.items[0], changed_date: staleDate }, // To Do, stale
          { ...boardData.items[1], changed_date: staleDate }, // Done, stale age but exempt
        ],
        states_by_type: boardData.states_by_type,
      };
    if (cmd === "classification_paths") return [];
    if (cmd === "board_pr_links") return [];
  });
  renderBoard();
  const staleCard = (await screen.findByText("Write docs")).closest("[draggable]")!;
  expect(staleCard).toHaveAttribute("title", "No changes in 12 days");
  const doneCard = screen.getByText("Fix bug").closest("[draggable]")!;
  expect(doneCard).not.toHaveAttribute("title");
});

test("PR chips render from board links and active outranks completed", async () => {
  mockIPC((cmd) => {
    if (cmd === "fetch_board") return boardData;
    if (cmd === "classification_paths") return [];
    if (cmd === "board_pr_links")
      return [
        { work_item_id: 11, pr_id: 9, status: "completed", title: "Old", repo: "database", web_url: "https://x/9" },
        { work_item_id: 11, pr_id: 12, status: "active", title: "New", repo: "web", web_url: "https://x/12" },
      ];
  });
  renderBoard();
  const card = (await screen.findByText("Write docs")).closest("[draggable]")!;
  // Chips are labelled by REPO (active first) so multi-repo items read apart.
  const active = await within(card as HTMLElement).findByTitle("Active PR !12 in web: New");
  const done = within(card as HTMLElement).getByTitle("Completed PR !9 in database: Old");
  expect(active.textContent).toContain("web");
  expect(done.textContent).toContain("database");
  expect(
    active.compareDocumentPosition(done) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
});

test("This sprint toggle refetches with currentSprint=true", async () => {
  const calls: Array<Record<string, unknown>> = [];
  mockIPC((cmd, args) => {
    if (cmd === "fetch_board") {
      calls.push(args as Record<string, unknown>);
      return boardData;
    }
    if (cmd === "classification_paths") return [];
    if (cmd === "board_pr_links") return [];
  });
  renderBoard();
  await screen.findByText("Write docs");
  expect(calls[calls.length - 1].currentSprint).toBe(false);

  fireEvent.click(screen.getByRole("checkbox", { name: /This sprint/ }));
  await vi.waitFor(() =>
    expect(calls[calls.length - 1].currentSprint).toBe(true),
  );
  expect(localStorage.getItem("tcm-v2-this-sprint")).toBe("on");
  localStorage.removeItem("tcm-v2-this-sprint");
});

test("the legacy Hide Done preference migrates to a collapsed Done rail (no cards rendered)", async () => {
  localStorage.setItem("tcm-v2-hide-done", "on");
  mockIPC((cmd) => {
    if (cmd === "fetch_board") return boardData;
    if (cmd === "classification_paths") return [];
  });
  renderBoard();
  const done = await screen.findByTestId("col-Done");
  // Rail: a restore control, no cards - so it can't stretch the board.
  expect(within(done).getByLabelText("Open Done")).toBeInTheDocument();
  expect(within(done).queryByText("Fix bug")).not.toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-hidden-cols")).toBe(JSON.stringify(["Done"]));
  expect(localStorage.getItem("tcm-v2-hide-done")).toBeNull();
  localStorage.removeItem("tcm-v2-hidden-cols");
});

test("any column can hide via its eye, but the last visible one is protected", async () => {
  mockIPC((cmd) => {
    if (cmd === "fetch_board") return boardData;
    if (cmd === "classification_paths") return [];
  });
  renderBoard();
  await screen.findByTestId("col-To Do");

  // Hide two columns. The rail only appears after the content's fade-out
  // phase (the column shrinks empty, so card text never squishes) - await it.
  fireEvent.click(screen.getByLabelText("Hide To Do"));
  fireEvent.click(screen.getByLabelText("Hide In Progress"));
  expect(await screen.findByLabelText("Open To Do")).toBeInTheDocument();
  expect(await screen.findByLabelText("Open In Progress")).toBeInTheDocument();

  // The third column's eye is disabled - all three can never hide at once.
  const lastHide = screen.getByLabelText("Hide Done");
  expect(lastHide).toBeDisabled();
  fireEvent.click(lastHide);
  expect(screen.queryByLabelText("Open Done")).not.toBeInTheDocument();

  // Restoring one re-enables hiding the rest.
  fireEvent.click(screen.getByLabelText("Open To Do"));
  expect(screen.getByLabelText("Hide Done")).toBeEnabled();
  expect(localStorage.getItem("tcm-v2-hidden-cols")).toBe(JSON.stringify(["In Progress"]));
  localStorage.removeItem("tcm-v2-hidden-cols");
});

test("areas under a Scrum Archive node are hidden from the scope picker", async () => {
  mockIPC((cmd) => {
    if (cmd === "fetch_board") return boardData;
    if (cmd === "classification_paths")
      return ["HRM", "HRM\\Gamma Guardians", "HRM\\Scrum Archive\\Old Team"];
  });
  renderBoard();
  await screen.findByTestId("col-To Do");
  fireEvent.click(screen.getByLabelText("Board scope"));
  expect(await screen.findByText("Area: HRM\\Gamma Guardians")).toBeInTheDocument();
  expect(screen.queryByText(/Scrum Archive/)).not.toBeInTheDocument();
});

test("area boards get an assignee filter built from the items; My work does not", async () => {
  mockIPC((cmd) => {
    if (cmd === "fetch_board")
      return {
        items: [
          boardData.items[0], // Avin
          { ...boardData.items[1], id: 13, column: "To Do", state: "To Do", assigned_to: "Kim", title: "Kim's task" },
          { ...boardData.items[1], id: 14, column: "To Do", state: "To Do", assigned_to: "", title: "Nobody's task" },
        ],
        states_by_type: boardData.states_by_type,
      };
    if (cmd === "classification_paths") return ["HRM\\Gamma Guardians"];
  });
  renderBoard();
  await screen.findByTestId("col-To Do");
  // My work: no assignee filter offered.
  expect(screen.queryByLabelText("Filter assignee")).not.toBeInTheDocument();

  fireEvent.click(screen.getByLabelText("Board scope"));
  fireEvent.click(await screen.findByText("Area: HRM\\Gamma Guardians"));
  // The scope switch refetches (new query key) - wait for the area board's
  // items before opening the picker, or its options are still empty.
  await screen.findByText("Kim's task");
  const picker = await screen.findByLabelText("Filter assignee");
  fireEvent.click(picker);
  // "Kim" also appears on the board card - target the dropdown's option row.
  const optionFor = (name: string) =>
    screen.getAllByText(name).find((el) => el.closest("label"))!;
  fireEvent.click(optionFor("Kim"));

  // Only Kim's card stays; Avin's and the unassigned one filter out.
  expect(screen.getByText("Kim's task")).toBeInTheDocument();
  expect(screen.queryByText("Write docs")).not.toBeInTheDocument();
  expect(screen.queryByText("Nobody's task")).not.toBeInTheDocument();

  // Unassigned is a first-class option.
  fireEvent.click(optionFor("Kim")); // untick
  fireEvent.click(optionFor("Unassigned"));
  expect(screen.getByText("Nobody's task")).toBeInTheDocument();
  expect(screen.queryByText("Kim's task")).not.toBeInTheDocument();
});

test("a move blocked by required fields opens the item with those fields named", async () => {
  mockIPC((cmd) => {
    if (cmd === "fetch_board") return boardData;
    if (cmd === "classification_paths") return [];
    if (cmd === "move_board_item")
      // The REAL shape: an AdoError::Http whose body carries ADO's rule
      // message - describeAdoError must surface it, the parser must mine it.
      throw {
        kind: "Http",
        detail: {
          status: 400,
          body: JSON.stringify({
            message:
              "TF401320: Rule Error for field Remaining Work. Error code: Required, InvalidEmpty.",
          }),
        },
      };
    if (cmd === "work_item_detail")
      return {
        id: 11, title: "Write docs", work_item_type: "Task", state: "To Do",
        assigned_to: "Avin", assigned_to_unique: "a@x.com", activity: "", tags: "",
        area_path: "P", iteration_path: "P\S1",
        remaining_work: null, completed_work: null, original_estimate: null,
        start_date: "", finish_date: "",
        description_text: "", description_html: "", description_field: "System.Description",
        extra_pages: [], extra_pages_error: null, inline_images: [],
      };
    if (cmd === "list_team_members") return [];
    if (cmd === "activity_values") return [];
    if (cmd === "work_item_comments") return [];
  });
  renderBoard();
  const card = await screen.findByText("Write docs");
  fireEvent.dragStart(card.closest("[draggable]")!);
  fireEvent.drop(screen.getByTestId("col-In Progress"));

  // The drawer opens with the blocking field called out in the banner.
  expect(
    await screen.findByText(/requires these fields before the state can change/),
  ).toBeInTheDocument();
  expect(screen.getByText("Remaining Work")).toBeInTheDocument();
});
