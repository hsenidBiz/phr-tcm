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
    if (cmd === "list_teams") return [];
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
    if (cmd === "list_teams") return [];
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
    if (cmd === "list_teams") return [];
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
    if (cmd === "list_teams") return [];
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
