// View Test Cases: the PBI's test cases, read-only, with personal comments
// kept on this computer and the browser page for reading them all.
//
// Locates come from screens/ViewCases/index.tsx, CaseDetail.tsx and
// CommentModal.tsx. The sample data carries one local comment, on #5002
// (CAPTURE_NOTES in src/dev/demo.ts), so its Comment chip shows.

import type { Screen } from "../types";

const LIST = "view-test-cases-list";
const DETAIL = "view-test-cases-detail";
const NOTE = "view-test-cases-add-comment";
const COMMENT = "view-test-cases-comment";
const SELECTED = "view-test-cases-selected";

const FIRST = "Login - valid credentials";
const NAV = { nav: "View Test Cases" };
const OPEN_FIRST = { click: { role: "button", name: "Expand #5001" } };

export const viewTestCases: Screen = {
  id: "view-test-cases",
  title: "View Test Cases",
  group: "Test cases",
  summary:
    "Read the PBI's test cases without any risk of changing them. Open a case to see its steps, keep a personal comment on it, " +
    "and open the cases as a page in your browser or save them to a file.",
  shots: [
    { id: LIST, route: [NAV], alt: "View Test Cases listing the test cases of the PBI" },
    { id: DETAIL, route: [NAV, OPEN_FIRST], alt: "A test case opened to its tags, preconditions and steps" },
    {
      id: NOTE,
      route: [NAV, OPEN_FIRST, { click: { role: "button", name: "Add comment" } }],
      alt: "Writing a local comment on a test case",
    },
    {
      id: COMMENT,
      route: [NAV, { click: { role: "button", name: "Has a local comment" } }, { waitFor: { role: "button", name: "Close comment" } }],
      alt: "A test case's local comment, opened from its Comment chip",
    },
    { id: SELECTED, route: [NAV, { click: { text: FIRST } }], alt: "One test case selected" },
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
      does: "Reads the test cases from Azure DevOps again. The list stays on screen while it does; the arrow spins until it is done.",
    },
    {
      id: "export-json",
      shot: LIST,
      locate: { role: "button", name: "Export JSON" },
      name: "Export JSON",
      does:
        "Saves the cases to a JSON file in the Import File format: the selected ones, or every case shown when none is selected. " +
        "Their ids are included, so importing the file later updates these cases instead of creating copies.",
    },
    {
      id: "view-in-browser",
      shot: LIST,
      locate: { role: "button", name: "View All Test Cases in Browser" },
      name: "View All Test Cases in Browser",
      does:
        "Opens the cases as a page in your browser: the selected ones, or every case shown when none is selected. It is the same page as **View in browser** on Import File. " +
        "Comments typed on it are kept as the cases' local comments here.",
      tips: ["While the page is open, it keeps up with what you select here and offers to refresh."],
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
      does:
        "Gathers cases whose titles start the same way under one heading, as on Update Test Cases. The heading's arrow and name fold the group; its box selects every case in it. The choice is remembered.",
    },
    {
      id: "expand",
      shot: LIST,
      locate: { role: "button", name: "Expand #5001" },
      name: "Open (>)",
      does: "Opens the case below its row. Several can be open at once, to compare them. Double-clicking the row does the same.",
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
    {
      id: "comment-chip",
      shot: LIST,
      locate: { role: "button", name: "Has a local comment" },
      name: "Comment",
      does: "Shows on a case that has a local comment. Hover it to read the comment, or click it to open it in a small window.",
    },

    // --- An open case ---------------------------------------------------------
    {
      id: "tags",
      shot: DETAIL,
      locate: { text: "smoke" },
      name: "Tags",
      does: "The case's tags.",
    },
    {
      id: "preconditions",
      shot: DETAIL,
      locate: { text: "Preconditions:" },
      name: "Preconditions",
      does: "What must be true before the first step, when the case has any.",
    },
    {
      id: "steps",
      shot: DETAIL,
      locate: { role: "columnheader", name: "Action" },
      name: "Steps",
      does: "Each step's action and expected result, in order. A shared step shows as one line naming it.",
    },
    {
      id: "add-comment",
      shot: DETAIL,
      locate: { role: "button", name: "Add comment" },
      name: "Add comment",
      does:
        "Starts a local comment on the case: a note for yourself, such as a step that needs updating. It is saved on this computer only and never written to Azure DevOps. " +
        "On a case that has one, the comment shows here with **Edit**.",
    },
    {
      id: "collapse-all",
      shot: DETAIL,
      locate: { role: "button", nameRe: "^Collapse all \\(\\d+\\)$" },
      name: "Collapse all",
      does: "Closes every open case and folds every open group in one press. It stays in the corner while anything is open.",
    },

    // --- Writing a comment ----------------------------------------------------
    {
      id: "comment-field",
      shot: NOTE,
      locate: { role: "textbox", name: "Comment for #5001" },
      name: "Comment",
      does: "Type the comment.",
    },
    {
      id: "comment-save",
      shot: NOTE,
      locate: { role: "button", name: "Save comment" },
      name: "Save comment",
      does: "Saves the comment. It is greyed out until the text has changed. Saving an empty comment removes it.",
    },
    {
      id: "comment-cancel",
      shot: NOTE,
      locate: { role: "button", name: "Cancel" },
      name: "Cancel",
      does: "Closes the box without saving.",
    },
    {
      id: "saved-locally",
      shot: NOTE,
      locate: { text: "Saved locally" },
      name: "Saved locally",
      does: "A reminder that the comment stays on this computer.",
    },

    // --- The comment window ---------------------------------------------------
    {
      id: "comment-close",
      shot: COMMENT,
      locate: { role: "button", name: "Close comment" },
      name: "Close (x)",
      does: "Closes the window. [[Esc]] does the same.",
    },
    {
      id: "comment-text",
      shot: COMMENT,
      locate: { text: "Check the error wording once the new copy lands." },
      name: "The comment",
      does: "The case's local comment, under the case it belongs to.",
    },
    {
      id: "comment-edit",
      shot: COMMENT,
      locate: { role: "button", name: "Edit" },
      name: "Edit",
      does: "Turns the comment into a box you can change, with **Save comment** and **Cancel**.",
    },
    {
      id: "comment-remove",
      shot: COMMENT,
      locate: { role: "button", name: "Remove" },
      name: "Remove",
      does: "Deletes the comment and closes the window. The Comment chip goes with it.",
    },

    // --- A selection ----------------------------------------------------------
    {
      id: "selected",
      shot: SELECTED,
      locate: { text: "1 selected" },
      name: "Selected",
      does: "How many cases are selected. **Export** and **View in Browser** above then act on the selection only.",
    },
    {
      id: "clear",
      shot: SELECTED,
      locate: { role: "button", name: "Clear" },
      name: "Clear",
      does: "Clears the selection.",
    },
    {
      id: "export-selected",
      shot: SELECTED,
      locate: { role: "button", nameRe: "^Export \\d+ JSON$" },
      name: "Export N JSON",
      does: "The **Export JSON** button while cases are selected: it saves only those.",
    },
    {
      id: "view-selected",
      shot: SELECTED,
      locate: { role: "button", nameRe: "^View \\d+ Test Cases? in Browser$" },
      name: "View N Test Cases in Browser",
      does: "The browser button while cases are selected: the page shows only those.",
    },
  ],
  tips: [
    "Nothing on this screen changes a test case. To edit one, use Update Test Cases.",
    "Local comments belong to this computer and this organisation. They are not shared with anyone.",
  ],
  howTo: [
    {
      title: "Keep a note on a case",
      steps: [
        "Press **>** on the case.",
        "Press **Add comment**, type the note and press **Save comment**.",
        "The case now shows a **Comment** chip in the list. Click it any time to read, edit or remove the note.",
      ],
    },
  ],
};
