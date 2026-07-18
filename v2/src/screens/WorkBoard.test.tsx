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
  fireEvent.change(screen.getByLabelText("Root Cause Category"), {
    target: { value: "Code Defect" },
  });

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
