// Board (Work Manager): work items in To Do / In Progress / Done columns,
// optionally in swimlanes by parent, and the work item window that opens
// from a card.
//
// Locates come from screens/WorkBoard.tsx, lib/boardLanes.ts,
// components/WorkItemDrawer.tsx, CommentsPanel.tsx, HistoryPanel.tsx and
// MarkdownField.tsx. Every capture boot turns swimlanes off, shows every
// column and clears the type filter (src/dev/demo.ts), so a route that
// changes one does not carry into the next shot. The window's routes end
// by clicking the Planning heading, which moves the pointer off the
// description (hovering it shows a tooltip).

import type { Screen, Step } from "../types";

const COLUMNS = "board-columns";
const SWIMLANES = "board-swimlanes";
const HIDDEN = "board-hidden-column";
const SCOPE = "board-scope";
const AREA = "board-area";
const ITEM = "board-item";
const STATE = "board-item-state";
const HISTORY = "board-item-history";
const RCA = "board-item-rca";

const NAV: Step[] = [{ click: { testId: "work" } }, { nav: "Board" }];
const CARD = "Session timeout is not enforced";
const OPEN_ITEM: Step[] = [
  ...NAV,
  { click: { text: CARD } },
  { waitFor: { role: "button", name: "Post" } },
  { click: { role: "heading", name: "Planning" } },
];

export const board: Screen = {
  id: "board",
  title: "Board",
  group: "Work Manager",
  summary:
    "Your work items as cards in To Do, In Progress and Done. Drag a card to another column to change its state, or click it to open the work item: " +
    "edit its fields, read and add comments, and see its history. Switch the board to an area or a PBI to see more than your own work.",
  shots: [
    { id: COLUMNS, route: [...NAV, { waitFor: { text: CARD } }], alt: "The board with its three columns and a work item's pull request chips" },
    { id: SWIMLANES, route: [...NAV, { click: { role: "switch", name: "Swimlanes" } }, { waitFor: { role: "button", name: "Collapse all" } }], alt: "The board in swimlanes, one lane per parent work item" },
    {
      id: HIDDEN,
      route: [...NAV, { click: { role: "button", name: "Hide Done" } }, { waitFor: { role: "button", name: "Open Done" } }],
      alt: "The Done column folded to a narrow strip",
    },
    {
      id: SCOPE,
      route: [...NAV, { click: { role: "combobox", name: "Board scope" } }, { waitFor: { role: "option", name: "My work" } }],
      alt: "The board scope list: my work, by PBI, or an area",
    },
    {
      id: AREA,
      route: [
        ...NAV,
        { click: { role: "combobox", name: "Board scope" } },
        { click: { role: "option", nameRe: "^Area: .*\\\\" } },
        { waitFor: { role: "button", name: "Filter assignee" } },
      ],
      alt: "An area board, with the assignee filter and This sprint",
    },
    { id: ITEM, route: OPEN_ITEM, alt: "A work item opened from its card" },
    {
      id: STATE,
      route: [...OPEN_ITEM, { click: { role: "combobox", name: "State" } }, { waitFor: { role: "option", name: "In Progress" } }],
      alt: "The work item's state list",
    },
    {
      id: HISTORY,
      route: [...OPEN_ITEM, { click: { role: "button", name: "History" } }, { waitFor: { text: "Time in each state" } }],
      alt: "The work item's history",
    },
    {
      id: RCA,
      route: [...OPEN_ITEM, { click: { role: "button", name: "RCA" } }, { waitFor: { role: "combobox", name: "Root Cause Category" } }],
      alt: "A bug's RCA page",
    },
  ],
  controls: [
    // --- The toolbar and the columns --------------------------------------------------
    {
      id: "scope",
      shot: COLUMNS,
      locate: { role: "combobox", name: "Board scope" },
      name: "Board scope",
      does: "Whose work the board shows: **My work** (assigned to you, the default), **By PBI** or an area. Type to find an area in a long list.",
    },
    {
      id: "filter",
      shot: COLUMNS,
      locate: { role: "textbox", name: "Filter items" },
      name: "Filter by title, id, tag",
      does: "Shows only the cards whose title, number or tags contain what you type.",
    },
    {
      id: "filter-type",
      shot: COLUMNS,
      locate: { role: "button", name: "Filter type" },
      name: "All types",
      does: "Shows only the work item types you tick, such as Bug or Task. The choice is remembered.",
    },
    {
      id: "refresh",
      shot: COLUMNS,
      locate: { role: "button", name: "Refresh work items" },
      name: "Refresh",
      does: "Reads the board again from Azure DevOps. The board otherwise shows what it read last and updates itself in the background.",
    },
    {
      id: "swimlanes",
      shot: COLUMNS,
      locate: { role: "switch", name: "Swimlanes" },
      name: "Swimlanes",
      does: "Groups the cards under the item they belong to, such as tasks under their PBI. The choice is remembered on this computer.",
    },
    {
      id: "hide-column",
      shot: COLUMNS,
      locate: { role: "button", name: "Hide Done" },
      name: "Hide",
      does: "Folds a column to a narrow strip, with its name and count, so the others get more room. At least one column always stays open. Hidden columns are remembered.",
    },
    {
      id: "card",
      shot: COLUMNS,
      locate: { text: "Wire the login form to the session service" },
      name: "Card",
      does:
        "A work item: its type, number, who it is assigned to, title, state and tags. Click it to open it. Drag it to another column to change its state in Azure DevOps.",
      tips: [
        "If Azure DevOps needs fields filled in before the state can change, the card goes back, the work item opens and those fields are marked.",
        "A card with an amber left edge has not changed for a week or more (cards in Done never get it). Hover it to see how long.",
      ],
    },
    {
      id: "pr-chip",
      shot: COLUMNS,
      locate: { role: "button", name: "portal-web" },
      name: "Pull request chip",
      does: "A pull request linked to the work item, named by its repository. A dot means it is active. Hover it for the title; click it to open it in your browser.",
    },
    {
      id: "completed-pr-chip",
      shot: COLUMNS,
      locate: { role: "button", name: "portal-db" },
      name: "Completed pull request",
      does: "A tick instead of the dot means the pull request is completed.",
    },

    // --- Swimlanes -------------------------------------------------------------------------
    {
      id: "collapse-all",
      shot: SWIMLANES,
      locate: { role: "button", name: "Collapse all" },
      name: "Collapse all",
      does: "Folds every lane to its heading.",
    },
    {
      id: "expand-all",
      shot: SWIMLANES,
      locate: { role: "button", name: "Expand all" },
      name: "Expand all",
      does: "Opens every lane again.",
    },
    {
      id: "lane-toggle",
      shot: SWIMLANES,
      locate: { role: "button", name: "Login and session flow, 2 cards, collapse" },
      name: "Fold a lane",
      does: "Folds or opens this lane on its own. Folded lanes are remembered for each project.",
    },
    {
      id: "lane-title",
      shot: SWIMLANES,
      locate: { role: "button", name: "#1001 Login and session flow" },
      name: "Lane heading",
      does: "The parent work item, with its type and how many cards are under it. Click its title to open it, as you would a card.",
    },
    {
      id: "no-parent",
      shot: SWIMLANES,
      locate: { text: "No parent" },
      name: "No parent",
      does: "The last lane holds the cards with no parent. In swimlanes, a card can only be dropped into a column of its own lane.",
    },

    // --- A hidden column ----------------------------------------------------------------------
    {
      id: "open-column",
      shot: HIDDEN,
      locate: { role: "button", name: "Open Done" },
      name: "Open",
      does: "Opens the hidden column again.",
    },

    // --- Scope --------------------------------------------------------------------------------
    {
      id: "my-work",
      shot: SCOPE,
      locate: { role: "option", name: "My work" },
      name: "My work",
      does: "The work items assigned to you in this project.",
    },
    {
      id: "by-pbi",
      shot: SCOPE,
      locate: { role: "option", name: "By PBI…" },
      name: "By PBI",
      does: "Everything parented under one PBI, and the PBI itself. A PBI search box appears in the toolbar; pick the PBI there.",
    },
    {
      id: "area",
      shot: SCOPE,
      locate: { role: "option", name: "Area: Customer Portal" },
      name: "Area",
      does: "Everything in that area of the project, whoever it is assigned to.",
    },

    // --- An area board --------------------------------------------------------------------------
    {
      id: "filter-assignee",
      shot: AREA,
      locate: { role: "button", name: "Filter assignee" },
      name: "All assignees",
      does: "On an area board: shows only the cards of the people you tick, or **Unassigned**.",
    },
    {
      id: "this-sprint",
      shot: AREA,
      locate: { role: "checkbox", name: "This sprint" },
      name: "This sprint",
      does: "On an area board: shows only the items in the current sprint of the project's default team. Remembered.",
    },

    // --- The work item ---------------------------------------------------------------------------
    {
      id: "copy-link",
      shot: ITEM,
      locate: { role: "button", name: "Copy link" },
      name: "Copy link",
      does: "Copies the work item's Azure DevOps link.",
    },
    {
      id: "open-in-ado",
      shot: ITEM,
      locate: { role: "button", name: "Open in Azure DevOps" },
      name: "Open in Azure DevOps",
      does: "Opens the work item in your browser.",
    },
    {
      id: "close-details",
      shot: ITEM,
      locate: { role: "button", name: "Close details" },
      name: "Close (x)",
      does: "Closes the work item. [[Esc]] does the same. Changes you have not saved are dropped; clicking outside the window does not close it.",
    },
    {
      id: "title",
      shot: ITEM,
      locate: { role: "textbox", name: "Title" },
      name: "Title",
      does: "The work item's title.",
    },
    {
      id: "state",
      shot: ITEM,
      locate: { role: "combobox", name: "State" },
      name: "State",
      does: "The states this type of work item can be in.",
    },
    {
      id: "assigned-to",
      shot: ITEM,
      locate: { role: "combobox", name: "Assigned to" },
      name: "Assigned to",
      does: "Who the work item is assigned to. Type to find a team member; the x leaves it unassigned.",
    },
    {
      id: "activity",
      shot: ITEM,
      locate: { role: "combobox", name: "Activity" },
      name: "Activity",
      does: "The kind of work, such as Development or Testing, when the work item type has one.",
    },
    {
      id: "pages",
      shot: ITEM,
      locate: { role: "button", name: "Description" },
      name: "Description and other pages",
      does: "The description, and any other pages your process adds to this type, such as **RCA** and **Preventive Measures** on a bug. Click one to show it.",
    },
    {
      id: "write-preview",
      shot: ITEM,
      locate: { role: "button", name: "Write" },
      name: "Write / Preview",
      does: "**Write** opens every text field on the page for editing, in markdown with a toolbar and a live preview. **Preview** shows them as they will look.",
    },
    {
      id: "edit-description",
      shot: ITEM,
      locate: { role: "button", name: "Edit Description" },
      name: "Edit",
      does: "Opens this one field for editing. Clicking the text does the same.",
    },
    {
      id: "discussion-history",
      shot: ITEM,
      locate: { role: "button", name: "Discussion" },
      name: "Discussion / History",
      does: "Switches between the comments and the work item's history.",
    },
    {
      id: "new-comment",
      shot: ITEM,
      locate: { role: "textbox", name: "New comment (markdown)" },
      name: "New comment",
      does: "Write a comment in markdown; the buttons above add bold, italic, code, links, headings, lists and quotes. The preview under it shows how it will look.",
    },
    {
      id: "post",
      shot: ITEM,
      locate: { role: "button", name: "Post" },
      name: "Post",
      does: "Adds the comment to the work item in Azure DevOps straight away, without **Save changes**. Greyed out until you type something.",
    },
    {
      id: "edit-comment",
      shot: ITEM,
      locate: { role: "button", name: "Edit" },
      name: "Edit (comment)",
      does: "Shown on your own comments: opens the comment for editing, with **Cancel** and **Update**. **Update** saves it in Azure DevOps.",
    },
    {
      id: "remaining",
      shot: ITEM,
      locate: { role: "spinbutton", name: "Remaining" },
      name: "Planning",
      does: "Remaining, Completed and Original work, as numbers.",
    },
    {
      id: "start-date",
      shot: ITEM,
      locate: { role: "button", name: "Start date" },
      name: "Start date / Target date",
      does: "Pick a date from the calendar.",
    },
    {
      id: "classification",
      shot: ITEM,
      locate: { role: "heading", name: "Classification" },
      name: "Classification",
      does: "The work item's area and iteration. They are shown, not changed, here.",
    },
    {
      id: "close",
      shot: ITEM,
      locate: { role: "button", name: "Close" },
      name: "Close",
      does: "Closes the work item without saving.",
    },
    {
      id: "save",
      shot: ITEM,
      locate: { role: "button", name: "Save changes" },
      name: "Save changes",
      does:
        "Saves the fields you changed to Azure DevOps, and only those, then refreshes the board. Greyed out until something changed. If Azure DevOps refuses, its reason is shown.",
    },

    // --- State ---------------------------------------------------------------------------------------
    {
      id: "state-option",
      shot: STATE,
      locate: { role: "option", name: "In Progress" },
      name: "Pick a state",
      does: "Choose the new state, then press **Save changes**. To change only the state, you can also drag the card to another column.",
    },

    // --- History ---------------------------------------------------------------------------------------
    {
      id: "time-in-state",
      shot: HISTORY,
      locate: { text: "Time in each state" },
      name: "Time in each state",
      does: "Every state the work item has been in, how many times it entered each one and how long it spent there in total. **now** marks the current state.",
    },
    {
      id: "history-filter",
      shot: HISTORY,
      locate: { role: "button", name: "Everything" },
      name: "Everything / State / Field edits / Links",
      does: "Shows every change, or only state changes, field edits or links added and removed.",
    },
    {
      id: "history-details",
      shot: HISTORY,
      locate: { role: "button", name: "Show details of Sam Doyle's change" },
      name: "details",
      does: "Each change, grouped by day, says who made it, when and what changed. **details** lists each field with its old and new value, with the changed words marked.",
    },

    // --- A bug's RCA page ---------------------------------------------------------------------------------
    {
      id: "rca-field",
      shot: RCA,
      locate: { role: "button", name: "Edit Initial Findings" },
      name: "Text fields",
      does: "The page's fields, laid out as in Azure DevOps. Rich text fields edit like the description; plain text fields are simple boxes.",
    },
    {
      id: "rca-pick",
      shot: RCA,
      locate: { role: "combobox", name: "Root Cause Category" },
      name: "Pick lists",
      does: "Fields with a fixed set of values are lists. Like every field here, they are saved with **Save changes**.",
    },
  ],
  tips: [
    "Clicking a work item in the notification bell opens the board with that work item open.",
    "Work items are created on **New Work Item**, not on the board.",
  ],
  howTo: [
    {
      title: "Move a work item on",
      steps: [
        "Find its card; type part of its title in **Filter by title, id, tag** if the board is busy.",
        "Drag the card into the next column. The new state is saved to Azure DevOps.",
        "If Azure DevOps asks for more fields first, the work item opens with them marked: fill them in, choose the new **State** and press **Save changes**.",
      ],
    },
    {
      title: "Comment on a work item",
      steps: ["Click the card.", "Type in **New comment** under Discussion.", "Press **Post**."],
    },
  ],
};
