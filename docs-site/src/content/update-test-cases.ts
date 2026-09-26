// Update Test Cases: the PBI's test cases as they are in Azure DevOps, each
// one editable in place, with bulk actions for a selection.
//
// Locates come from screens/ExistingCases/index.tsx and CaseEditor.tsx,
// StepsEditor.tsx, TagsField, components/BulkEditDialog.tsx,
// RelinkDialog.tsx and DeleteConfirm.tsx. The shots are of PBI #1001's five
// sample cases (src/dev/demo.ts); a click on a case's title selects it,
// the way a click anywhere on its row does. The editor shot opens #5002 and
// changes its automation status, so Save and Discard are both live.

import type { Screen } from "../types";

const LIST = "update-test-cases-list";
const EDITOR = "update-test-cases-editor";
const GROUPED = "update-test-cases-grouped";
const SELECTED = "update-test-cases-selected";
const BULK = "update-test-cases-bulk-edit";
const MOVE = "update-test-cases-move";
const DELETE = "update-test-cases-delete";

const FIRST = "Login - valid credentials";
const NAV = { nav: "Update Test Cases" };
const SELECT_FIRST = { click: { text: FIRST } };

export const updateTestCases: Screen = {
  id: "update-test-cases",
  title: "Update Test Cases",
  group: "Test cases",
  summary:
    "Change test cases that are already in Azure DevOps. Every case linked to the PBI is listed; open one to edit it in place, " +
    "or select several to change them together, rename them, move them to another PBI or export them.",
  shots: [
    { id: LIST, route: [NAV], alt: "Update Test Cases listing the five test cases of the PBI" },
    {
      id: EDITOR,
      route: [
        NAV,
        { click: { role: "button", name: "Expand #5002" } },
        { click: { role: "combobox", name: "Automation status" } },
        { click: { role: "option", name: "Planned" } },
      ],
      alt: "A test case open for editing, with one change made",
    },
    {
      id: GROUPED,
      route: [NAV, { click: { role: "checkbox", name: "Group by title" } }],
      alt: "The test cases grouped by the start of their titles",
    },
    { id: SELECTED, route: [NAV, SELECT_FIRST], alt: "One test case selected, with the selection actions" },
    {
      id: BULK,
      route: [NAV, SELECT_FIRST, { click: { role: "button", name: "Bulk edit" } }, { waitFor: { role: "button", nameRe: "^Apply to \\d+$" } }],
      alt: "The Bulk edit window for the selected test case",
    },
    {
      id: MOVE,
      route: [
        NAV,
        SELECT_FIRST,
        { click: { role: "button", name: "Move to PBI" } },
        { waitFor: { role: "textbox", name: "Search for the destination PBI" } },
      ],
      alt: "The window for moving test cases to another PBI",
    },
    {
      id: DELETE,
      route: [NAV, SELECT_FIRST, { click: { role: "button", name: "Delete" } }, { waitFor: { role: "button", nameRe: "^Permanently delete \\d+$" } }],
      alt: "The confirmation before test cases are deleted",
    },
  ],
  controls: [
    // --- The list ----------------------------------------------------------
    {
      id: "count",
      shot: LIST,
      locate: { role: "heading", nameRe: "Total Test Cases$" },
      name: "Total Test Cases",
      does: "How many test cases are linked to the PBI. While the search box has text, it shows how many of them match.",
    },
    {
      id: "refresh",
      shot: LIST,
      locate: { role: "button", name: "Refresh" },
      name: "Refresh",
      does: "Reads the test cases from Azure DevOps again, for changes made somewhere else.",
    },
    {
      id: "search",
      shot: LIST,
      locate: { role: "textbox", name: "Search test cases" },
      name: "Filter by name or id",
      does: "Shows only the cases whose title, id or tags contain what you type.",
    },
    {
      id: "group-by-title",
      shot: LIST,
      locate: { role: "checkbox", name: "Group by title" },
      name: "Group by title",
      does: "Gathers cases whose titles start the same way, such as every case beginning with **Login**, under one heading. The choice is remembered.",
    },
    {
      id: "expand",
      shot: LIST,
      locate: { role: "button", name: "Expand #5001" },
      name: "Open the editor (>)",
      does: "Opens the case for editing right in the list. Press it again to close it. Double-clicking the row does the same.",
    },
    {
      id: "case",
      shot: LIST,
      locate: { text: FIRST },
      name: "Test case",
      does:
        "One test case: its id, title, number of steps and automation status. Click it to select it; click it again to clear. " +
        "[[Ctrl]]+click adds or removes one case, [[Shift]]+click selects a range.",
    },

    // --- Editing one case ---------------------------------------------------
    {
      id: "case-title",
      shot: EDITOR,
      locate: { role: "textbox", name: "Case title" },
      name: "Title",
      does: "The case's title. Pressing [[Enter]] here saves, the same as **Save changes**.",
    },
    {
      id: "case-status",
      shot: EDITOR,
      locate: { role: "combobox", name: "Automation status" },
      name: "Automation status",
      does: "**Not Automated** or **Planned**.",
    },
    {
      id: "remove-tag",
      shot: EDITOR,
      locate: { role: "button", name: "Remove regression" },
      name: "Remove tag (x)",
      does: "Takes that tag off the case when you save.",
    },
    {
      id: "tags",
      shot: EDITOR,
      locate: { role: "textbox", name: "Tags" },
      name: "Tags",
      does: "Add a tag: type to search the project's existing tags, or type a new one.",
      tips: ["When the project has a Module field or a Preconditions field, it shows in the editor too."],
    },
    {
      id: "reorder-step",
      shot: EDITOR,
      locate: { role: "button", name: "Reorder step 1" },
      name: "Drag handle",
      does: "Drag a step by its handle to move it, or click the handle and use the up and down arrow keys.",
    },
    {
      id: "step-action",
      shot: EDITOR,
      locate: { role: "textbox", name: "Step 1 action" },
      name: "Action",
      does: "What the tester does in this step.",
    },
    {
      id: "step-expected",
      shot: EDITOR,
      locate: { role: "textbox", name: "Step 1 expected" },
      name: "Expected result",
      does: "What should happen after the action.",
    },
    {
      id: "remove-step",
      shot: EDITOR,
      locate: { role: "button", name: "Remove step 1" },
      name: "Remove step (x)",
      does: "Removes the step.",
    },
    {
      id: "add-step",
      shot: EDITOR,
      locate: { role: "button", name: "Add Step" },
      name: "Add Step",
      does: "Adds an empty step at the end.",
    },
    {
      id: "save",
      shot: EDITOR,
      locate: { role: "button", name: "Save changes" },
      name: "Save changes",
      does:
        "Saves the case to Azure DevOps. It is greyed out until something has changed, and while the case has a problem, which is named beside it. " +
        "Steps you did not touch are left exactly as they were, formatting and pictures included.",
    },
    {
      id: "discard",
      shot: EDITOR,
      locate: { role: "button", name: "Discard changes" },
      name: "Discard changes",
      does: "Shows once something has changed. It puts the case back as it was when you opened it (or last saved it). A message offers **Undo** for ten seconds, in case you pressed it by mistake.",
    },
    {
      id: "collapse-all",
      shot: EDITOR,
      locate: { role: "button", nameRe: "^Collapse all \\(\\d+\\)$" },
      name: "Collapse all",
      does: "Closes the open editor and folds every open group in one press. It stays in the corner while anything is open.",
    },

    // --- Grouped ------------------------------------------------------------
    {
      id: "group-fold",
      shot: GROUPED,
      locate: { role: "button", name: "Collapse group Login" },
      name: "Fold group (v)",
      does: "Folds the group away, or opens it again. An open editor stays on screen when its group is folded.",
    },
    {
      id: "group-select",
      shot: GROUPED,
      locate: { role: "checkbox", name: "Select all in Login" },
      name: "Select the group",
      does: "Selects every case in the group, or clears them when they are all selected.",
    },
    {
      id: "group-name",
      shot: GROUPED,
      locate: { role: "button", name: "Login (3)" },
      name: "Group name",
      does: "The words the titles share and how many cases are in the group. Clicking it folds or opens the group. Cases that share nothing sit under **Ungrouped**.",
    },

    // --- A selection --------------------------------------------------------
    {
      id: "bulk-edit",
      shot: SELECTED,
      locate: { role: "button", name: "Bulk edit" },
      name: "Bulk edit",
      does: "Opens a window for setting the same automation status, tags, module or preconditions on every selected case.",
    },
    {
      id: "rename",
      shot: SELECTED,
      locate: { role: "button", name: "Rename" },
      name: "Rename",
      does:
        "Opens the rename window, the same one the queue uses on Import File. Here **Rename** saves the new titles to Azure DevOps straight away, and **Undo rename** puts them back.",
    },
    {
      id: "export-json",
      shot: SELECTED,
      locate: { role: "button", name: "Export JSON" },
      name: "Export JSON",
      does: "Saves the selected cases to a JSON file you choose. The file carries their ids, so importing it later updates these cases instead of creating copies.",
    },
    {
      id: "delete",
      shot: SELECTED,
      locate: { role: "button", name: "Delete" },
      name: "Delete",
      does: "Opens the confirmation for deleting the selected cases for good. It only shows when Azure DevOps allows you to delete test cases in this PBI's area.",
    },
    {
      id: "move-to-pbi",
      shot: SELECTED,
      locate: { role: "button", name: "Move to PBI" },
      name: "Move to PBI",
      does: "Opens a window for moving the selected cases to a different PBI, for cases that were linked to the wrong one.",
    },
    {
      id: "clear",
      shot: SELECTED,
      locate: { role: "button", name: "Clear" },
      name: "Clear",
      does: "Clears the selection.",
    },
    {
      id: "selection-hint",
      shot: SELECTED,
      locate: { text: "Ctrl+click to toggle · Shift+click for range" },
      name: "Selection",
      does:
        "The bar appears once a case is selected: it starts with how many are selected, and its buttons act on them. " +
        "When it scrolls away, the buttons float at the bottom of the window. [[Ctrl]]+click adds or removes one case, [[Shift]]+click selects everything between two cases.",
    },

    // --- Bulk edit ----------------------------------------------------------
    {
      id: "bulk-status",
      shot: BULK,
      locate: { role: "combobox", name: "Automation status" },
      name: "Automation status",
      does: "The status to give every selected case, or **Leave unchanged**.",
    },
    {
      id: "bulk-module",
      shot: BULK,
      locate: { role: "checkbox", name: "Set module" },
      name: "Set module",
      does: "Tick it to show a Module field; the value you pick goes on every selected case.",
    },
    {
      id: "bulk-tags",
      shot: BULK,
      locate: { role: "combobox", name: "Tags" },
      name: "Tags",
      does: "**Add tags** adds the tags you type to each case's own. **Replace tags** swaps each case's tags for them. **Leave unchanged** keeps them.",
    },
    {
      id: "bulk-preconditions",
      shot: BULK,
      locate: { role: "checkbox", name: "Set preconditions" },
      name: "Set preconditions",
      does: "Tick it to show a Preconditions box; its text replaces the preconditions of every selected case.",
    },
    {
      id: "bulk-cancel",
      shot: BULK,
      locate: { role: "button", name: "Cancel" },
      name: "Cancel",
      does: "Closes the window without changing anything.",
    },
    {
      id: "bulk-apply",
      shot: BULK,
      locate: { role: "button", nameRe: "^Apply to \\d+$" },
      name: "Apply to N",
      does:
        "Saves the chosen fields to each selected case in Azure DevOps, one after another. Titles and steps are never touched. " +
        "It is greyed out until you choose at least one change. Any case that fails is named with the reason.",
    },

    // --- Move to PBI --------------------------------------------------------
    {
      id: "move-search",
      shot: MOVE,
      locate: { role: "textbox", name: "Search for the destination PBI" },
      name: "Search PBIs",
      does: "Type a title or an id to find the PBI to move the cases to, then click it in the list. **change** beside it picks a different one.",
    },
    {
      id: "move-cancel",
      shot: MOVE,
      locate: { role: "button", name: "Cancel" },
      name: "Cancel",
      does: "Closes the window without moving anything.",
    },
    {
      id: "move-confirm",
      shot: MOVE,
      locate: { role: "button", name: "Pick a PBI first" },
      name: "Move N to #PBI",
      does:
        "Reads **Pick a PBI first** until you choose one. Then it moves the cases: their link goes from this PBI to the new one, so they leave this list and show under the new PBI's suite. " +
        "Moving them back is the same move the other way.",
    },

    // --- Delete -------------------------------------------------------------
    {
      id: "delete-confirm-tick",
      shot: DELETE,
      locate: { role: "checkbox", name: "I understand these test cases will be permanently deleted" },
      name: "I understand",
      does: "Tick it to confirm you have read the list above it. The delete button stays greyed out until you do.",
    },
    {
      id: "delete-cancel",
      shot: DELETE,
      locate: { role: "button", name: "Cancel" },
      name: "Cancel",
      does: "Closes the window. Nothing is deleted.",
    },
    {
      id: "delete-confirm",
      shot: DELETE,
      locate: { role: "button", nameRe: "^Permanently delete \\d+$" },
      name: "Permanently delete N",
      does:
        "Deletes the listed cases in Azure DevOps. This cannot be undone: there is no recycle bin for test cases, and their run history goes with them. " +
        "If some cannot be deleted, the window stays open and names each one with the reason.",
    },
  ],
  tips: [
    "Cases picked with **Edit cases** in Search Suites open here with **Back to PBI cases** above them, which returns to the PBI's own list. **Move to PBI** is greyed out for them.",
    "A case you open keeps what you type until you save or discard it, even if you fold its group.",
  ],
  howTo: [
    {
      title: "Change one test case",
      steps: [
        "Choose the PBI in the bar at the top.",
        "Press **>** on the case, or double-click it.",
        "Make the change: title, status, tags or steps.",
        "Press **Save changes**.",
      ],
    },
    {
      title: "Tag several cases at once",
      steps: [
        "Click the first case, then [[Ctrl]]+click the others (or [[Shift]]+click the last one of a run).",
        "Press **Bulk edit**.",
        "Set **Tags** to **Add tags** and type the tags.",
        "Press **Apply to N**.",
      ],
    },
  ],
};
