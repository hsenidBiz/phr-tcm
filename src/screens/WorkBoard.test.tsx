import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { useState, type ComponentProps } from "react";
import WorkBoard from "./WorkBoard";

afterEach(() => {
  clearMocks();
  // The PR-chip query persists to localStorage now - one test's cached
  // answer must not seed the next test's initialData.
  localStorage.clear();
});

function renderBoard(props: Partial<ComponentProps<typeof WorkBoard>> = {}) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <WorkBoard org="acme" project="Web" allProjects={false} homeProject="Web" {...props} />
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

/// Cached-first: flipping away and back inside a minute must render from
/// cache with zero refetch - the tab switch IS the hot path.
test("a remount within staleTime reuses the cached board", async () => {
  let fetches = 0;
  mockIPC((cmd) => {
    if (cmd === "fetch_board") {
      fetches++;
      return boardData;
    }
    if (cmd === "classification_paths") return [];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const renderWithClient = () =>
    render(
      <QueryClientProvider client={qc}>
        <WorkBoard org="acme" project="Web" allProjects={false} homeProject="Web" />
      </QueryClientProvider>,
    );

  const view = renderWithClient(); // first mount
  await screen.findByText("Write docs");
  expect(fetches).toBe(1);
  view.unmount();
  renderWithClient(); // same client = same cache
  await screen.findByText("Write docs"); // renders instantly from cache
  expect(fetches).toBe(1); // FAILS before the fix (2)
});

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
  expect(screen.getByText("Write docs").closest("[draggable]")!.className).toContain("cv-row");
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
        target_date: "",
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
        target_date: "",
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

/// The sprint filter is an AREA-scope tool: "My work" is already a
/// personal slice, so the checkbox hides there and the persisted
/// preference stops applying to the fetch until an area is picked again.
test("This sprint shows only for an area scope, and only then filters the fetch", async () => {
  const calls: Array<Record<string, unknown>> = [];
  mockIPC((cmd, args) => {
    if (cmd === "fetch_board") {
      calls.push(args as Record<string, unknown>);
      return boardData;
    }
    if (cmd === "classification_paths") return ["Web"];
    if (cmd === "board_pr_links") return [];
  });
  renderBoard();
  await screen.findByText("Write docs");

  // My work: no sprint checkbox, no sprint filter.
  expect(screen.queryByRole("checkbox", { name: /This sprint/ })).not.toBeInTheDocument();
  expect(calls[calls.length - 1].currentSprint).toBe(false);

  // Pick an area: the checkbox appears and toggling it filters the fetch.
  fireEvent.click(screen.getByLabelText("Board scope"));
  fireEvent.click(await screen.findByText("Area: Web"));
  fireEvent.click(await screen.findByRole("checkbox", { name: /This sprint/ }));
  await vi.waitFor(() =>
    expect(calls[calls.length - 1].currentSprint).toBe(true),
  );
  expect(localStorage.getItem("tcm-v2-this-sprint")).toBe("on");

  // Back to My work: checkbox gone, and the stored "on" no longer
  // filters - the board key flips to sprint=false, so it re-serves the
  // unfiltered cache and any refetch goes out without the sprint clause.
  const before = calls.length;
  fireEvent.click(screen.getByLabelText("Board scope"));
  fireEvent.click(await screen.findByText("My work"));
  await screen.findByText("Write docs");
  expect(screen.queryByRole("checkbox", { name: /This sprint/ })).not.toBeInTheDocument();
  expect(calls.slice(before).every((c) => c.currentSprint === false)).toBe(true);
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
        start_date: "", target_date: "",
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

/// The bell's handoff: an item arrives as a prop and its drawer opens as
/// soon as the board is there to open it against.
test("a focused item opens its drawer once the board has loaded, then reports handled", async () => {
  mockIPC((cmd) => {
    if (cmd === "fetch_board") return boardData;
    if (cmd === "classification_paths") return [];
    if (cmd === "work_item_detail")
      return {
        id: 12, title: "Fix bug", work_item_type: "Bug", state: "Done",
        assigned_to: "Avin", assigned_to_unique: "a@x.com", activity: "", tags: "",
        area_path: "P", iteration_path: "P\S1",
        remaining_work: null, completed_work: null, original_estimate: null,
        start_date: "", target_date: "",
        description_text: "", description_html: "", description_field: "System.Description",
        extra_pages: [], extra_pages_error: null, inline_images: [],
      };
    if (cmd === "list_team_members") return [];
    if (cmd === "activity_values") return [];
    if (cmd === "work_item_comments") return [];
  });
  const handled = vi.fn();
  renderBoard({ focusItem: 12, onFocusHandled: handled });
  expect(await screen.findByRole("dialog", { name: "Work item 12" })).toBeInTheDocument();
  expect(handled).toHaveBeenCalledTimes(1);
});

// ---- swimlanes --------------------------------------------------------------

const leave = { id: 500, title: "Leave requests", work_item_type: "Product Backlog Item" };
const payroll = { id: 600, title: "Payroll export", work_item_type: "Product Backlog Item" };

function task(id: number, title: string, column: string, changed: string, parent: unknown) {
  return {
    id,
    title,
    work_item_type: "Task",
    state: column,
    state_color: "b2b2b2",
    column,
    assigned_to: "Avin",
    tags: "",
    priority: 2,
    changed_date: changed,
    parent,
  };
}

// Board order is newest change first: 500's newest card, then the loose
// one, then 600, 500 again, then the unreadable parent 900.
const laneData = {
  items: [
    task(21, "Draft the form", "To Do", "2026-07-12T05:00:00Z", leave),
    task(22, "Loose task", "To Do", "2026-07-12T04:00:00Z", null),
    task(23, "Wire the API", "In Progress", "2026-07-12T03:00:00Z", payroll),
    task(24, "Review the form", "Done", "2026-07-12T02:00:00Z", leave),
    task(25, "Orphan task", "To Do", "2026-07-12T01:00:00Z", { id: 900, title: "", work_item_type: "" }),
  ],
  states_by_type: boardData.states_by_type,
};

function mockLanes(extra: (cmd: string, args: unknown) => unknown = () => undefined) {
  mockIPC((cmd, args) => {
    if (cmd === "fetch_board") return laneData;
    if (cmd === "classification_paths") return [];
    return extra(cmd, args);
  });
}

const laneIds = () => screen.getAllByTestId(/^lane-\d+$/).map((el) => el.getAttribute("data-testid"));

test("swimlanes are off by default and the board is exactly as before", async () => {
  mockLanes();
  renderBoard();
  await screen.findByText("Draft the form");
  expect(screen.getByRole("switch", { name: "Swimlanes" })).toHaveAttribute("aria-checked", "false");
  expect(screen.queryByTestId("lane-500")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /^(Collapse|Expand) all/ })).not.toBeInTheDocument();
  expect(within(screen.getByTestId("col-To Do")).getByText("Draft the form")).toBeInTheDocument();
  expect(within(screen.getByTestId("col-To Do")).getByText("Loose task")).toBeInTheDocument();
});

test("turning Swimlanes on groups cards by parent, newest first, No parent last, and is remembered", async () => {
  mockLanes();
  renderBoard();
  await screen.findByText("Draft the form");
  fireEvent.click(screen.getByRole("switch", { name: "Swimlanes" }));

  expect(localStorage.getItem("tcm-v2-board-swimlanes")).toBe("on");
  expect(laneIds()).toEqual(["lane-500", "lane-600", "lane-900", "lane-0"]);
  const lane500 = screen.getByTestId("lane-500");
  expect(within(lane500).getByRole("button", { name: "Leave requests, 2 cards, collapse" })).toBeInTheDocument();
  expect(within(within(lane500).getByTestId("col-To Do")).getByText("Draft the form")).toBeInTheDocument();
  expect(within(within(lane500).getByTestId("col-Done")).getByText("Review the form")).toBeInTheDocument();
  expect(within(lane500).queryByText("Wire the API")).not.toBeInTheDocument();
  // Unreadable parent: the lane reads its id.
  expect(within(screen.getByTestId("lane-900")).getByRole("button", { name: "#900, 1 card, collapse" })).toBeInTheDocument();
  // No parent has no title to open.
  const loose = screen.getByTestId("lane-0");
  expect(within(loose).getByRole("button", { name: "No parent, 1 card, collapse" })).toBeInTheDocument();
  expect(within(loose).queryByRole("button", { name: /^#/ })).not.toBeInTheDocument();
});

test("filters apply first, so a lane they empty is not shown", async () => {
  localStorage.setItem("tcm-v2-board-swimlanes", "on");
  mockLanes();
  renderBoard();
  await screen.findByTestId("lane-500");
  fireEvent.change(screen.getByLabelText("Filter items"), { target: { value: "wire" } });
  expect(laneIds()).toEqual(["lane-600"]);
});

/// With the switch off the same filter state still shows the three
/// (empty) columns - swimlanes has no such fallback, since a lane with no
/// cards is simply not rendered, so filtering out everything used to leave
/// a blank board with no explanation and no way to tell it apart from
/// still loading.
test("filtering out every card in swimlanes view shows the empty-state message, not a blank board", async () => {
  localStorage.setItem("tcm-v2-board-swimlanes", "on");
  mockLanes();
  renderBoard();
  await screen.findByTestId("lane-500");
  fireEvent.change(screen.getByLabelText("Filter items"), { target: { value: "does-not-exist" } });
  expect(screen.queryByTestId(/^lane-/)).not.toBeInTheDocument();
  expect(screen.getByText("No cards match these filters.")).toBeInTheDocument();
  // Collapse all / Expand all act on lanes - none left to act on.
  expect(screen.queryByRole("button", { name: /^(Collapse|Expand) all/ })).not.toBeInTheDocument();
});

test("a lane collapses to its header and stays collapsed next time", async () => {
  localStorage.setItem("tcm-v2-board-swimlanes", "on");
  mockLanes();
  const view = renderBoard();
  const lane500 = await screen.findByTestId("lane-500");
  fireEvent.click(within(lane500).getByRole("button", { name: "Leave requests, 2 cards, collapse" }));

  expect(within(lane500).queryByText("Draft the form")).not.toBeInTheDocument();
  expect(within(lane500).getByRole("button", { name: "Leave requests, 2 cards, expand" })).toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-board-lanes-collapsed:acme/Web")).toBe("[500]");

  view.unmount();
  renderBoard();
  const again = await screen.findByTestId("lane-500");
  expect(within(again).getByRole("button", { name: "Leave requests, 2 cards, expand" })).toBeInTheDocument();
  expect(within(again).queryByText("Draft the form")).not.toBeInTheDocument();
  expect(within(screen.getByTestId("lane-600")).getByText("Wire the API")).toBeInTheDocument();
});

test("one sticky button folds every open lane, then turns into Expand all", async () => {
  localStorage.setItem("tcm-v2-board-swimlanes", "on");
  mockLanes();
  renderBoard();
  const lane500 = await screen.findByTestId("lane-500");

  // It counts the lanes still open, and folding one by hand counts down.
  expect(screen.getByRole("button", { name: "Collapse all (4)" })).toBeInTheDocument();
  fireEvent.click(within(lane500).getByRole("button", { name: "Leave requests, 2 cards, collapse" }));
  expect(screen.getByRole("button", { name: "Collapse all (3)" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /^Expand all/ })).not.toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Collapse all (3)" }));
  expect(screen.getAllByRole("button", { name: /, expand$/ })).toHaveLength(4);
  expect(screen.queryByText("Draft the form")).not.toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-board-lanes-collapsed:acme/Web")).toBe("[0,500,600,900]");
  expect(screen.queryByRole("button", { name: /^Collapse all/ })).not.toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Expand all (4)" }));
  expect(screen.getAllByRole("button", { name: /, collapse$/ })).toHaveLength(4);
  expect(screen.getByText("Draft the form")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Collapse all (4)" })).toBeInTheDocument();
});

test("a card can be dropped only in its own lane's columns", async () => {
  localStorage.setItem("tcm-v2-board-swimlanes", "on");
  const moves: unknown[] = [];
  mockLanes((cmd, args) => {
    if (cmd === "move_board_item") {
      moves.push(args);
      return "In Progress";
    }
  });
  renderBoard();
  const card = (await screen.findByText("Draft the form")).closest("[draggable]")!;

  fireEvent.dragStart(card);
  fireEvent.drop(within(screen.getByTestId("lane-600")).getByTestId("col-In Progress"));
  expect(moves).toEqual([]);
  expect(within(screen.getByTestId("lane-500")).getByText("Draft the form")).toBeInTheDocument();

  fireEvent.dragStart(card);
  fireEvent.drop(within(screen.getByTestId("lane-500")).getByTestId("col-In Progress"));
  await vi.waitFor(() => expect(moves).toHaveLength(1));
  expect(moves[0]).toMatchObject({ itemId: 21, column: "In Progress" });
});

test("clicking a lane's title opens that parent in the drawer", async () => {
  localStorage.setItem("tcm-v2-board-swimlanes", "on");
  const opened: number[] = [];
  mockLanes((cmd, args) => {
    if (cmd === "work_item_detail") {
      const id = (args as { id: number }).id;
      opened.push(id);
      return {
        id,
        title: "Leave requests",
        work_item_type: "Product Backlog Item",
        state: "Committed",
        assigned_to: "",
        assigned_to_unique: "",
        activity: "",
        tags: "",
        area_path: "P",
        iteration_path: "P\\S1",
        remaining_work: null,
        completed_work: null,
        original_estimate: null,
        start_date: "",
        target_date: "",
        description_text: "",
        description_html: "",
        description_field: "System.Description",
        extra_pages: [],
        extra_pages_error: null,
        inline_images: [],
      };
    }
    if (cmd === "list_team_members") return [];
    if (cmd === "activity_values") return [];
    if (cmd === "work_item_comments") return [];
  });
  renderBoard();
  fireEvent.click(await screen.findByRole("button", { name: "#500 Leave requests" }));
  await vi.waitFor(() => expect(opened).toContain(500));
});

test("a hidden column is hidden in every lane alike", async () => {
  localStorage.setItem("tcm-v2-board-swimlanes", "on");
  localStorage.setItem("tcm-v2-hidden-cols", JSON.stringify(["Done"]));
  mockLanes();
  renderBoard();
  await screen.findByTestId("lane-500");
  for (const id of ["lane-500", "lane-600", "lane-900", "lane-0"]) {
    expect(within(screen.getByTestId(id)).getByRole("button", { name: "Open Done" })).toBeInTheDocument();
  }
});

// ---- pinned areas ----------------------------------------------------------

test("an area pinned in the scope list sits at the top, stays there next time, and unpins", async () => {
  mockIPC((cmd) => {
    if (cmd === "fetch_board") return boardData;
    if (cmd === "classification_paths") return ["HRM\\Alpha", "HRM\\Beta", "HRM\\Gamma"];
  });
  const options = () => screen.getAllByRole("option").map((o) => o.textContent);
  const view = renderBoard();
  await screen.findByTestId("col-To Do");
  fireEvent.click(screen.getByLabelText("Board scope"));
  await screen.findByText("Area: HRM\\Gamma");
  expect(options()).toEqual(["My work", "By PBI…", "Area: HRM\\Alpha", "Area: HRM\\Beta", "Area: HRM\\Gamma"]);
  // Only areas can be pinned: My work and By PBI… are one click already.
  expect(screen.queryByRole("button", { name: /^Pin My work/ })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /^Pin By PBI/ })).not.toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Pin Area: HRM\\Gamma" }));
  expect(options()).toEqual(["Area: HRM\\Gamma", "My work", "By PBI…", "Area: HRM\\Alpha", "Area: HRM\\Beta"]);
  expect(screen.getByRole("button", { name: "Unpin Area: HRM\\Gamma" })).toHaveAttribute("aria-pressed", "true");
  // Pinning is not choosing: the list stays open on My work.
  expect(screen.getByRole("combobox", { name: "Board scope" })).toHaveTextContent("My work");
  expect(JSON.parse(localStorage.getItem("tcm-v2-board-pinned-areas:acme/Web")!)).toEqual(["HRM\\Gamma"]);

  view.unmount();
  renderBoard();
  await screen.findByTestId("col-To Do");
  fireEvent.click(screen.getByLabelText("Board scope"));
  await screen.findByText("Area: HRM\\Gamma");
  expect(options()[0]).toBe("Area: HRM\\Gamma");

  fireEvent.click(screen.getByRole("button", { name: "Unpin Area: HRM\\Gamma" }));
  expect(options()).toEqual(["My work", "By PBI…", "Area: HRM\\Alpha", "Area: HRM\\Beta", "Area: HRM\\Gamma"]);
  expect(localStorage.getItem("tcm-v2-board-pinned-areas:acme/Web")).toBe("[]");
});

// ---- open lanes only -------------------------------------------------------

test("Open lanes only hides the lanes whose cards are all Done, says how many, and is remembered", async () => {
  localStorage.setItem("tcm-v2-board-swimlanes", "on");
  const shipped = { id: 700, title: "Shipped feature", work_item_type: "Product Backlog Item" };
  mockIPC((cmd) => {
    if (cmd === "fetch_board")
      return {
        ...laneData,
        items: [
          ...laneData.items,
          task(26, "Ship it", "Done", "2026-07-11T05:00:00Z", shipped),
          task(27, "Announce it", "Done", "2026-07-11T04:00:00Z", shipped),
        ],
      };
    if (cmd === "classification_paths") return [];
  });
  const view = renderBoard();
  await screen.findByTestId("lane-700");
  const toggle = screen.getByRole("switch", { name: "Open lanes only" });
  expect(toggle).toHaveAttribute("aria-checked", "false");

  fireEvent.click(toggle);
  // Leave requests keeps its lane: one card is still To Do beside a Done one.
  expect(laneIds()).toEqual(["lane-500", "lane-600", "lane-900", "lane-0"]);
  expect(screen.getByText("(1 done hidden)")).toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-board-open-lanes")).toBe("on");

  view.unmount();
  renderBoard();
  await screen.findByTestId("lane-500");
  expect(screen.queryByTestId("lane-700")).not.toBeInTheDocument();
});

test("Open lanes only is offered with Swimlanes, and says so when every lane is done", async () => {
  mockLanes();
  renderBoard();
  await screen.findByText("Draft the form");
  expect(screen.queryByRole("switch", { name: "Open lanes only" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("switch", { name: "Swimlanes" }));
  fireEvent.click(screen.getByRole("switch", { name: "Open lanes only" }));
  // Only the Done card is left after the filter, so every lane is done.
  fireEvent.change(screen.getByLabelText("Filter items"), { target: { value: "review" } });
  expect(screen.queryByTestId(/^lane-/)).not.toBeInTheDocument();
  expect(screen.getByText("Every lane here is done. Turn off Open lanes only to see them.")).toBeInTheDocument();
});

// ---- every project as one --------------------------------------------------

const payrollStates = {
  Task: [
    { name: "To Do", color: "b2b2b2", category: "Proposed" },
    { name: "In Progress", color: "ff0000", category: "InProgress" },
    { name: "Done", color: "339933", category: "Completed" },
  ],
};

function inProject(item: ReturnType<typeof task>, project: string) {
  return { ...item, project };
}

const hrmCard = inProject(task(31, "Leave form", "To Do", "2026-07-12T05:00:00Z", null), "HRM");
const payrollCard = inProject(task(32, "Run payroll", "To Do", "2026-07-12T04:00:00Z", null), "Payroll");

const combinedData = {
  items: [hrmCard, payrollCard],
  // The organization-wide board has no project-less states.
  states_by_type: {},
  states_by_project: { HRM: boardData.states_by_type, Payroll: payrollStates },
  parent_projects: {},
};

const noAreas = { paths: [], coverage: { shown: 2, total: 2 } };

/** Answers the organization-wide board and areas; records every command. */
function mockCombined(
  data: unknown = combinedData,
  areas: unknown = noAreas,
  extra: (cmd: string, args: unknown) => unknown = () => undefined,
) {
  const calls: Array<{ cmd: string; args: Record<string, unknown> }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    if (cmd === "fetch_board_all") return data;
    if (cmd === "classification_paths_all") return areas;
    if (cmd === "board_pr_links") return [];
    return extra(cmd, args);
  });
  return calls;
}

const combined = { allProjects: true, homeProject: "HRM", project: "HRM" } as const;

function detailFor(id: number, title: string) {
  return {
    id, title, work_item_type: "Task", state: "To Do", assigned_to: "Avin",
    assigned_to_unique: "a@x.com", activity: "", tags: "", area_path: "P",
    iteration_path: "P\\S1", remaining_work: null, completed_work: null,
    original_estimate: null, start_date: "", target_date: "", description_text: "",
    description_html: "", description_field: "System.Description", extra_pages: [],
    extra_pages_error: null, inline_images: [],
  };
}

/** The drawer's reads, recording the project each asked in. */
function drawerReads(opened: Array<Record<string, unknown>>) {
  return (cmd: string, args: unknown) => {
    if (cmd === "work_item_detail") {
      const a = args as Record<string, unknown>;
      opened.push(a);
      return detailFor(a.id as number, "Opened item");
    }
    if (cmd === "list_team_members") return [];
    if (cmd === "activity_values") return [];
    if (cmd === "work_item_comments") return [];
    return undefined;
  };
}

test("combined board asks every project", async () => {
  const calls = mockCombined(combinedData, { paths: ["HRM\\Alpha", "Payroll\\Beta"], coverage: { shown: 2, total: 2 } });
  renderBoard({ ...combined, project: "" });
  await screen.findByText("Leave form");
  expect(screen.getByText("Run payroll")).toBeInTheDocument();
  const board = calls.filter((c) => c.cmd === "fetch_board_all");
  expect(board).toHaveLength(1);
  expect(board[0].args).toEqual({ organization: "acme", area: null, pbiId: null, currentSprint: false });
  // Nothing asks one project.
  expect(calls.some((c) => c.cmd === "fetch_board" || c.cmd === "classification_paths")).toBe(false);

  // The areas are every project's, and picking one scopes the board to it.
  fireEvent.click(screen.getByLabelText("Board scope"));
  fireEvent.click(await screen.findByText("Area: Payroll\\Beta"));
  await vi.waitFor(() =>
    expect(calls.filter((c) => c.cmd === "fetch_board_all").pop()!.args).toMatchObject({
      organization: "acme",
      area: "Payroll\\Beta",
    }),
  );
});

test("the combined board needs no project, only an organization", () => {
  mockCombined();
  renderBoard({ ...combined, org: "", project: "" });
  expect(screen.getByText("Pick an organization in the bar above first.")).toBeInTheDocument();
});

test("cards name their project only when two projects are on the board", async () => {
  mockCombined({ ...combinedData, items: [hrmCard] });
  const one = renderBoard(combined);
  await screen.findByText("Leave form");
  expect(screen.queryByText("Project:", { exact: false })).not.toBeInTheDocument();
  one.unmount();

  mockCombined();
  renderBoard(combined);
  await screen.findByText("Run payroll");
  const hrm = screen.getByText("Leave form").closest("[draggable]") as HTMLElement;
  const pay = screen.getByText("Run payroll").closest("[draggable]") as HTMLElement;
  expect(within(hrm).getByText("HRM")).toBeInTheDocument();
  expect(within(pay).getByText("Payroll")).toBeInTheDocument();
  expect(within(pay).queryByText("HRM")).not.toBeInTheDocument();
});

test("switched off, cards never name a project even when the items span two", async () => {
  mockIPC((cmd) => {
    if (cmd === "fetch_board") return { ...combinedData, states_by_type: boardData.states_by_type };
    if (cmd === "classification_paths") return [];
  });
  renderBoard();
  await screen.findByText("Run payroll");
  expect(screen.queryByText("Payroll")).not.toBeInTheDocument();
  expect(screen.queryByText("Project:", { exact: false })).not.toBeInTheDocument();
});

test("a lane header names its project under the same rule", async () => {
  localStorage.setItem("tcm-v2-board-swimlanes", "on");
  const leave500 = { id: 500, title: "Leave requests", work_item_type: "Product Backlog Item" };
  const pay600 = { id: 600, title: "Payroll export", work_item_type: "Product Backlog Item" };
  const items = [
    inProject(task(41, "Draft the form", "To Do", "2026-07-12T05:00:00Z", leave500), "HRM"),
    inProject(task(42, "Wire the API", "To Do", "2026-07-12T04:00:00Z", pay600), "Payroll"),
  ];
  const header = (lane: string) =>
    within(screen.getByTestId(lane)).getByRole("button", { name: /, (collapse|expand)$/ }).parentElement as HTMLElement;

  mockCombined({ ...combinedData, items, parent_projects: { 500: "HRM", 600: "Payroll" } });
  const two = renderBoard(combined);
  await screen.findByTestId("lane-500");
  expect(within(header("lane-500")).getByText("HRM")).toBeInTheDocument();
  expect(within(header("lane-600")).getByText("Payroll")).toBeInTheDocument();
  two.unmount();

  mockCombined({ ...combinedData, items: [items[0]] });
  renderBoard(combined);
  await screen.findByTestId("lane-500");
  expect(within(header("lane-500")).queryByText("HRM")).not.toBeInTheDocument();
});

test("moving a card writes to that card's project", async () => {
  const calls = mockCombined(combinedData, noAreas, (cmd) => {
    if (cmd === "move_board_item") return "In Progress";
  });
  // The effective project is HRM; the card belongs to Payroll.
  renderBoard(combined);
  const card = await screen.findByText("Run payroll");
  fireEvent.dragStart(card.closest("[draggable]")!);
  fireEvent.drop(screen.getByTestId("col-In Progress"));

  await vi.waitFor(() => expect(calls.some((c) => c.cmd === "move_board_item")).toBe(true));
  expect(calls.find((c) => c.cmd === "move_board_item")!.args).toEqual({
    organization: "acme",
    project: "Payroll",
    itemId: 32,
    workItemType: "Task",
    column: "In Progress",
  });
  // The colour comes from that project's own states, not a type-wide list.
  const moved = (await within(screen.getByTestId("col-In Progress")).findByText("Run payroll")).closest(
    "[draggable]",
  ) as HTMLElement;
  await vi.waitFor(() =>
    expect(moved.querySelector("span.h-2.w-2")).toHaveStyle({ backgroundColor: "rgb(255, 0, 0)" }),
  );
});

test("the drawer opens in the item's own project, and saves there", async () => {
  const opened: Array<Record<string, unknown>> = [];
  let patched: Record<string, unknown> = {};
  mockCombined(combinedData, noAreas, (cmd, args) => {
    if (cmd === "update_work_item") {
      patched = args as Record<string, unknown>;
      return null;
    }
    return drawerReads(opened)(cmd, args);
  });
  renderBoard(combined);
  fireEvent.click(await screen.findByText("Run payroll"));

  const title = await screen.findByLabelText(/Title/);
  expect(opened[0]).toMatchObject({ organization: "acme", project: "Payroll", id: 32 });
  fireEvent.change(title, { target: { value: "Run payroll twice" } });
  fireEvent.click(screen.getByRole("button", { name: "Save changes" }));
  await vi.waitFor(() => expect(patched.patches).toBeTruthy());
  expect(patched).toMatchObject({ organization: "acme", project: "Payroll", id: 32 });
});

test("a lane parent from another project opens in its project", async () => {
  localStorage.setItem("tcm-v2-board-swimlanes", "on");
  const opened: Array<Record<string, unknown>> = [];
  const parent = { id: 500, title: "Leave requests", work_item_type: "Product Backlog Item" };
  const items = [
    inProject(task(41, "Draft the form", "To Do", "2026-07-12T05:00:00Z", parent), "HRM"),
    inProject(task(42, "Wire the API", "To Do", "2026-07-12T04:00:00Z", null), "Payroll"),
  ];
  mockCombined({ ...combinedData, items, parent_projects: { 500: "Payroll" } }, noAreas, drawerReads(opened));
  renderBoard(combined);
  fireEvent.click(await screen.findByRole("button", { name: "#500 Leave requests" }));
  await vi.waitFor(() => expect(opened).toHaveLength(1));
  expect(opened[0]).toMatchObject({ project: "Payroll", id: 500 });
});

test("a notification's item outside the board opens in the notification's project", async () => {
  const opened: Array<Record<string, unknown>> = [];
  mockCombined(combinedData, noAreas, drawerReads(opened));
  // The caller clears the focus the moment the board reports it handled, as
  // App does - the drawer must keep the project it opened in.
  function Harness() {
    const [focus, setFocus] = useState<{ id: number; project: string } | null>({ id: 99, project: "Payroll" });
    const [qc] = useState(() => new QueryClient({ defaultOptions: { queries: { retry: false } } }));
    return (
      <QueryClientProvider client={qc}>
        <WorkBoard
          org="acme"
          project="HRM"
          allProjects
          homeProject="HRM"
          focusItem={focus?.id ?? null}
          focusProject={focus?.project ?? null}
          onFocusHandled={() => setFocus(null)}
        />
      </QueryClientProvider>
    );
  }
  render(<Harness />);
  expect(await screen.findByRole("dialog", { name: "Work item 99" })).toBeInTheDocument();
  await vi.waitFor(() => expect(opened.length).toBeGreaterThan(0));
  expect(opened.every((o) => o.project === "Payroll" && o.id === 99)).toBe(true);
});

test("a focused item the board holds opens in the board item's project", async () => {
  const opened: Array<Record<string, unknown>> = [];
  mockCombined(combinedData, noAreas, drawerReads(opened));
  renderBoard({ ...combined, focusItem: 32, focusProject: "HRM", onFocusHandled: vi.fn() });
  await screen.findByRole("dialog", { name: "Work item 32" });
  await vi.waitFor(() => expect(opened.length).toBeGreaterThan(0));
  expect(opened[0]).toMatchObject({ project: "Payroll", id: 32 });
});

test("areas left out over the cap are reported", async () => {
  mockCombined(combinedData, { paths: ["HRM\\Alpha"], coverage: { shown: 98, total: 100 } });
  const capped = renderBoard(combined);
  expect(await screen.findByText("Showing areas from 98 of 100 projects.")).toBeInTheDocument();
  capped.unmount();
  // The areas are kept on disk now: the second board starts from nothing, as
  // if the first one's answer had expired.
  localStorage.clear();

  mockCombined(combinedData, { paths: ["HRM\\Alpha"], coverage: { shown: 100, total: 100 } });
  renderBoard(combined);
  await screen.findByText("Leave form");
  fireEvent.click(screen.getByLabelText("Board scope"));
  await screen.findByText("Area: HRM\\Alpha");
  expect(screen.queryByText(/Showing areas from/)).not.toBeInTheDocument();
});

test("an empty My work says no project, and the switched-off board says this one", async () => {
  mockCombined({ ...combinedData, items: [] });
  const on = renderBoard(combined);
  expect(
    await screen.findByText(
      "Nothing assigned to you in any project - quick create one above, or switch to an area board.",
    ),
  ).toBeInTheDocument();
  on.unmount();

  mockIPC((cmd) => {
    if (cmd === "fetch_board") return { items: [], states_by_type: {} };
    if (cmd === "classification_paths") return [];
  });
  renderBoard();
  expect(
    await screen.findByText("Nothing assigned to you in this project - quick create one above, or switch to an area board."),
  ).toBeInTheDocument();
});

test("By PBI in the combined board searches every project and fetches with the pick", async () => {
  const calls = mockCombined(combinedData, noAreas, (cmd) => {
    if (cmd === "search_pbis_all")
      return [{ id: 4242, title: "Login flow", work_item_type: "Product Backlog Item", project: "Payroll" }];
  });
  renderBoard(combined);
  await screen.findByText("Leave form");
  fireEvent.click(screen.getByLabelText("Board scope"));
  fireEvent.click(await screen.findByText("By PBI…"));
  const find = screen.getByLabelText("Find PBI");
  fireEvent.change(find, { target: { value: "Login" } });
  fireEvent.keyDown(find, { key: "Enter" });
  fireEvent.click(await screen.findByText(/Login flow/));
  await vi.waitFor(() =>
    expect(calls.filter((c) => c.cmd === "fetch_board_all").pop()!.args).toMatchObject({
      pbiId: 4242,
      area: null,
    }),
  );
  expect(calls.some((c) => c.cmd === "search_pbis")).toBe(false);
});

test("switched off the board asks the picked project as before", async () => {
  const calls: Array<{ cmd: string; args: Record<string, unknown> }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    if (cmd === "fetch_board") return boardData;
    if (cmd === "classification_paths") return [];
  });
  renderBoard();
  await screen.findByText("Write docs");
  expect(calls.find((c) => c.cmd === "fetch_board")!.args).toEqual({
    organization: "acme",
    project: "Web",
    area: null,
    pbiId: null,
    currentSprint: false,
  });
  expect(calls.find((c) => c.cmd === "classification_paths")!.args).toMatchObject({ project: "Web" });
  expect(calls.some((c) => c.cmd === "fetch_board_all" || c.cmd === "classification_paths_all")).toBe(false);
});

test("the combined board keeps the pins and collapsed lanes of the home project, copied once", async () => {
  localStorage.setItem("tcm-v2-board-swimlanes", "on");
  localStorage.setItem("tcm-v2-board-pinned-areas:acme/HRM", JSON.stringify(["HRM\\Gamma"]));
  localStorage.setItem("tcm-v2-board-lanes-collapsed:acme/HRM", JSON.stringify([500]));
  const parent = { id: 500, title: "Leave requests", work_item_type: "Product Backlog Item" };
  const items = [
    inProject(task(41, "Draft the form", "To Do", "2026-07-12T05:00:00Z", parent), "HRM"),
    inProject(task(42, "Wire the API", "To Do", "2026-07-12T04:00:00Z", null), "Payroll"),
  ];
  mockCombined(
    { ...combinedData, items },
    { paths: ["HRM\\Alpha", "HRM\\Gamma"], coverage: { shown: 2, total: 2 } },
  );
  renderBoard(combined);
  await screen.findByTestId("lane-500");
  // The home project's collapsed lane arrives collapsed; its pin leads the list.
  expect(
    within(screen.getByTestId("lane-500")).getByRole("button", { name: "Leave requests, 1 card, expand" }),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByLabelText("Board scope"));
  await screen.findByText("Area: HRM\\Gamma");
  expect(screen.getAllByRole("option")[0].textContent).toBe("Area: HRM\\Gamma");

  // Copied, never moved; later edits go to the combined key only.
  expect(localStorage.getItem("tcm-v2-board-pinned-areas:acme/*")).toBe('["HRM\\\\Gamma"]');
  expect(localStorage.getItem("tcm-v2-board-lanes-collapsed:acme/*")).toBe("[500]");
  fireEvent.click(screen.getByRole("button", { name: "Unpin Area: HRM\\Gamma" }));
  expect(localStorage.getItem("tcm-v2-board-pinned-areas:acme/*")).toBe("[]");
  expect(localStorage.getItem("tcm-v2-board-pinned-areas:acme/HRM")).toBe('["HRM\\\\Gamma"]');
  expect(localStorage.getItem("tcm-v2-board-lanes-collapsed:acme/HRM")).toBe("[500]");
});

test("the open drawer keeps its item's project when a refetch drops the item from the board", async () => {
  // Reassigned and saved on My work: the next board no longer holds the card,
  // yet the drawer is still open on it, and what it reads, saves and comments
  // must still go to the item's own project, never the home project.
  let dropped = false;
  const asked: Array<{ cmd: string; args: Record<string, unknown> }> = [];
  const opened: Array<Record<string, unknown>> = [];
  const reads = drawerReads(opened);
  mockIPC((cmd, args) => {
    asked.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    if (cmd === "fetch_board_all") return dropped ? { ...combinedData, items: [hrmCard] } : combinedData;
    if (cmd === "classification_paths_all") return noAreas;
    if (cmd === "board_pr_links") return [];
    if (cmd === "update_work_item") {
      dropped = true;
      return null;
    }
    if (cmd === "add_comment") return null;
    return reads(cmd, args);
  });
  renderBoard(combined);
  fireEvent.click(await screen.findByText("Run payroll"));

  fireEvent.change(await screen.findByLabelText(/Title/), { target: { value: "Run payroll twice" } });
  fireEvent.click(screen.getByRole("button", { name: "Save changes" }));
  await vi.waitFor(() => expect(asked.some((c) => c.cmd === "update_work_item")).toBe(true));
  // The refetch has come back without the card; the drawer is still there.
  await vi.waitFor(() => expect(screen.queryByText("Run payroll")).not.toBeInTheDocument());
  expect(screen.getByRole("dialog", { name: "Work item 32" })).toBeInTheDocument();

  fireEvent.change(screen.getByLabelText("New comment (markdown)"), { target: { value: "Reassigned" } });
  fireEvent.click(screen.getByRole("button", { name: /Post/ }));
  await vi.waitFor(() => expect(asked.some((c) => c.cmd === "add_comment")).toBe(true));

  const drawerCommands = [
    "work_item_detail",
    "update_work_item",
    "add_comment",
    "work_item_comments",
    "list_team_members",
    "activity_values",
  ];
  const named = asked.filter((c) => drawerCommands.includes(c.cmd));
  expect(named.some((c) => c.cmd === "add_comment")).toBe(true);
  expect(named.map((c) => [c.cmd, c.args.project])).toEqual(named.map((c) => [c.cmd, "Payroll"]));
});

test("when the areas of every project cannot be listed, the board says so, and asks only once", async () => {
  const asked: string[] = [];
  mockIPC((cmd) => {
    asked.push(cmd);
    if (cmd === "fetch_board_all") return combinedData;
    if (cmd === "board_pr_links") return [];
    if (cmd === "classification_paths_all")
      throw { kind: "Network", detail: "Azure DevOps could not be reached. Try again in a moment." };
  });
  // The app's own client, which retries a failed query three times.
  const qc = new QueryClient();
  render(
    <QueryClientProvider client={qc}>
      <WorkBoard org="acme" project="HRM" allProjects homeProject="HRM" />
    </QueryClientProvider>,
  );
  // Retries would hold the answer back for seconds: a line inside the
  // default wait means the query did not retry.
  const line = await screen.findByText(
    "Could not list the areas of every project: Azure DevOps could not be reached. Try again in a moment.",
  );
  expect(line).toHaveClass("text-danger");
  expect(asked.filter((c) => c === "classification_paths_all")).toHaveLength(1);
  // The board itself is unharmed.
  expect(await screen.findByText("Leave form")).toBeInTheDocument();
});

test("a later visit to the board does not list the areas of every project again", async () => {
  const calls = mockCombined(combinedData, { paths: ["HRM\\Alpha"], coverage: { shown: 2, total: 2 } });
  const first = renderBoard(combined);
  await screen.findByText("Leave form");
  fireEvent.click(screen.getByLabelText("Board scope"));
  await screen.findByText("Area: HRM\\Alpha");
  // Written under the cache's own key, scoped to the organization.
  expect(localStorage.getItem("tcm-v2-cache:areas-all:acme")).not.toBeNull();
  first.unmount();

  // A new visit with nothing in memory, as after the screen was left for a while.
  renderBoard(combined);
  await screen.findByText("Leave form");
  fireEvent.click(screen.getByLabelText("Board scope"));
  await screen.findByText("Area: HRM\\Alpha");
  expect(calls.filter((c) => c.cmd === "classification_paths_all")).toHaveLength(1);
});
