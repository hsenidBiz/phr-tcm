// New Work Item (Work Manager): creating a task, bug or PBI with every
// field set up front, optionally under a parent PBI.
//
// Locates come from screens/CreateWorkItem.tsx, components/TagsField.tsx
// and components/PbiPicker.tsx. A route cannot type a title, so the page
// after creating is described in the tips.

import type { Screen } from "../types";

const FORM = "new-work-item-form";

export const newWorkItem: Screen = {
  id: "new-work-item",
  title: "New Work Item",
  group: "Work Manager",
  summary:
    "Create a task, bug or PBI in the project with everything set before it is created: who it is for, where it sits, its priority, tags, description and, if you like, the PBI it belongs under.",
  shots: [{ id: FORM, route: [{ click: { testId: "work" } }, { nav: "New Work Item" }, { waitFor: { role: "textbox", name: "Title" } }], alt: "The New Work Item form" }],
  controls: [
    {
      id: "type",
      shot: FORM,
      locate: { role: "combobox", name: "Work item type" },
      name: "Type",
      does: "Task, Bug or Product Backlog Item.",
    },
    {
      id: "title",
      shot: FORM,
      locate: { role: "textbox", name: "Title" },
      name: "Title",
      does: "What needs doing. It is the only field you must fill in.",
    },
    {
      id: "assign-to",
      shot: FORM,
      locate: { role: "combobox", name: "Assign to" },
      name: "Assign to",
      does: "A member of the project's team, or **Unassigned**.",
    },
    {
      id: "priority",
      shot: FORM,
      locate: { role: "combobox", name: "Priority" },
      name: "Priority",
      does: "1 to 4, or **Default** to leave it to Azure DevOps.",
    },
    {
      id: "area",
      shot: FORM,
      locate: { role: "combobox", name: "Area" },
      name: "Area",
      does: "Where the item belongs in the project. Type to search; left empty, it goes in the project's own area.",
    },
    {
      id: "iteration",
      shot: FORM,
      locate: { role: "combobox", name: "Iteration" },
      name: "Iteration",
      does: "The sprint, listed with its dates. Left empty, the item goes to the backlog.",
    },
    {
      id: "tags",
      shot: FORM,
      locate: { role: "textbox", name: "Tags" },
      name: "Tags",
      does: "Type a tag and press [[Enter]]; the project's existing tags are suggested as you type.",
    },
    {
      id: "parent",
      shot: FORM,
      locate: { role: "textbox", name: "Find PBI" },
      name: "Parent PBI",
      does: "Optional. Search for a PBI by number or title and pick it: the new item is linked under it, so it sits beneath that PBI on boards and backlogs.",
    },
    {
      id: "description",
      shot: FORM,
      locate: { role: "textbox", name: "Description" },
      name: "Description",
      does: "Context, acceptance criteria and links.",
    },
    {
      id: "create",
      shot: FORM,
      locate: { role: "button", name: "Create Task" },
      name: "Create (type)",
      does: "Creates the work item in Azure DevOps. It is named for the type you chose, and greyed out until there is a title.",
    },
  ],
  tips: [
    "After creating, the page shows the new item's number with **Open in Azure DevOps**, **Copy link** and **Create another**. Create another brings the form back with the title, tags and description cleared and everything else kept.",
    "A half-filled form is kept while you visit other screens, until the app is closed.",
    "The new item shows on the board the next time you open it, if it is in the board's scope.",
  ],
};
