// Import Test Cases: bringing cases in (a JSON file, a share link), the queue and
// everything it does, the review before upload, and the results.
//
// Locates come from ImportFile.tsx, QueueSection.tsx, QueueRow.tsx,
// QueueBulkEditDialog.tsx and PowerRenameDialog.tsx. Every shot starts from
// the same three queued cases for PBI #1001 (src/dev/demo.ts,
// CAPTURE_QUEUE): a new case with a comment and reviewer notes, an update
// of #5002 whose title changed, and a new case titled like #5001, which the
// duplicate check stops. Each row's Edit and Remove buttons are named for
// their case ("Edit <title>", "Remove <title> from the queue").
//
// Watched files, specs, general comments, change reports and Recent JSON
// Imports only appear after a real file has been imported from disk, which
// the capture cannot do; they are covered in the tips and how-tos below.
// Button names that carry an em dash in the app are matched with "." so no
// dash has to be written here.

import type { Screen } from "../types";

const QUEUE = "import-file-queue";
const OPEN = "import-file-open-case";
const CHANGES = "import-file-changes";
const SELECTED = "import-file-selected";
const BULK = "import-file-bulk-edit";
const RENAME = "import-file-rename";
const REVIEW = "import-file-review";
const RESULTS = "import-file-results";
const EDIT = "import-file-edit";

const FIRST = "Login - remember me keeps you signed in";
const THIRD = "Login - valid credentials";

export const importFile: Screen = {
  id: "import-file",
  title: "Import Test Cases",
  group: "Test cases",
  summary:
    "Bring test cases in from a JSON file or a colleague's share link, check them in the queue, then upload them to the Product Backlog Item. " +
    "A case that carries an id updates that exact work item; a case without one is created new.",
  shots: [
    { id: QUEUE, route: [{ nav: "Import Test Cases" }], alt: "Import Test Cases with three cases in the queue" },
    {
      id: OPEN,
      route: [{ nav: "Import Test Cases" }, { click: { role: "button", name: `Expand steps of ${FIRST}` } }],
      alt: "A queued case opened to its steps",
    },
    {
      id: CHANGES,
      route: [{ nav: "Import Test Cases" }, { click: { role: "button", nameRe: "^Click to view" } }],
      alt: "An update opened to what will change",
    },
    {
      id: EDIT,
      route: [
        { nav: "Import Test Cases" },
        { click: { role: "button", name: `Edit ${FIRST}` } },
        { waitFor: { role: "button", name: "Save to queue" } },
        { scrollTo: { role: "button", name: "Save to queue" } },
      ],
      alt: "A queued case open for editing in the queue",
    },
    {
      id: SELECTED,
      route: [
        { nav: "Import Test Cases" },
        { click: { role: "row", name: FIRST } },
        { ctrlClick: { role: "row", name: THIRD } },
      ],
      alt: "Two queued cases selected, with the bulk actions",
    },
    {
      id: BULK,
      route: [
        { nav: "Import Test Cases" },
        { click: { role: "row", name: FIRST } },
        { ctrlClick: { role: "row", name: THIRD } },
        { click: { role: "button", name: "Bulk edit" } },
        { waitFor: { role: "button", nameRe: "^Apply to \\d+$" } },
      ],
      alt: "The Bulk edit window for two queued cases",
    },
    {
      id: RENAME,
      route: [
        { nav: "Import Test Cases" },
        { click: { role: "row", name: FIRST } },
        { ctrlClick: { role: "row", name: THIRD } },
        { click: { role: "button", nameRe: "^Rename \\d+$" } },
        { waitFor: { role: "textbox", name: "Find" } },
      ],
      alt: "The rename window for two selected queued cases",
    },
    {
      id: REVIEW,
      route: [
        { nav: "Import Test Cases" },
        { click: { role: "button", nameRe: "^Review \\d+ test cases?$" } },
        { waitFor: { role: "button", name: "Create duplicates anyway" } },
        { scrollTo: { role: "button", nameRe: "^Yes . create" } },
      ],
      alt: "Reviewing the queue before upload, stopped by a duplicate title",
    },
    {
      id: RESULTS,
      route: [
        { nav: "Import Test Cases" },
        { click: { role: "row", name: FIRST } },
        { ctrlClick: { role: "row", name: THIRD } },
        { click: { role: "button", name: "Remove 2" } },
        { click: { role: "button", nameRe: "^Review \\d+ test cases?$" } },
        { click: { role: "button", nameRe: "^Confirm & update" } },
        { waitFor: { role: "button", name: "Clear results" } },
      ],
      alt: "The results of an upload",
    },
  ],
  groups: [
    { id: "bring-in", title: "Import or open a shared file", summary: "Bring test cases in from a JSON file, or from a link a colleague shared." },
    { id: "the-queue", title: "The queue", summary: "Everything waiting to be uploaded, with what you can do to all of it." },
    { id: "queued-cases", title: "A queued case", summary: "Each row is one case: open it to see its steps, or to see what an update will change." },
    { id: "edit-case", title: "Edit a queued case", summary: "Change a case in the queue before it is uploaded." },
    { id: "many-at-once", title: "Change many cases at once", summary: "Select cases to edit, rename or remove them together." },
    { id: "upload", title: "Review and upload", summary: "A last look at every case, then the upload and its results." },
  ],
  controls: [
    // --- Bringing cases in, and the queue ---------------------------------
    {
      id: "import-json",
      shot: QUEUE,
      group: "bring-in",
      locate: { role: "button", name: "Import JSON" },
      name: "Import JSON",
      does:
        "Opens a JSON file of test cases and adds its cases to the queue. The file is then watched: save a change to it and the change flows into the queue by itself.",
      tips: [
        "With a working repository set on AI Bridge, a file from anywhere else is first copied into the repository's .test-cases folder, and the copy is the one watched.",
      ],
    },
    {
      id: "share-link",
      shot: QUEUE,
      group: "bring-in",
      locate: { role: "textbox", name: "Share link" },
      name: "Share link",
      does: "Paste a share link a colleague sent you. A link works once: after the import it expires.",
    },
    {
      id: "import-shared",
      shot: QUEUE,
      group: "bring-in",
      locate: { role: "button", name: "Import shared" },
      name: "Import shared",
      does:
        "Fetches the shared draft and adds its cases to the queue. A copy is saved on this computer and watched like an imported file, so the ids an upload creates are kept.",
      tips: [
        "A draft shared for a different Product Backlog Item asks first: stay on the Product Backlog Item you have chosen, or switch to the one it was shared for.",
      ],
    },
    {
      id: "queue-title",
      shot: QUEUE,
      group: "the-queue",
      locate: { text: "Queue for PBI #1001 (3 queued)" },
      name: "Queue for Product Backlog Item",
      does: "Every case waiting to be uploaded to this Product Backlog Item, and how many there are. The queue is shared with Manual Entry and kept on this computer for each Product Backlog Item.",
    },
    {
      id: "view-in-browser",
      shot: QUEUE,
      group: "the-queue",
      locate: { role: "button", name: "View in browser" },
      name: "View in browser",
      does: "Opens the queue as a review page in your browser, where you can read every case and leave comments. See **Use the review page** below.",
    },
    {
      id: "share-for-review",
      shot: QUEUE,
      group: "the-queue",
      locate: { role: "button", name: "Share for review" },
      name: "Share for review",
      does:
        "Attaches the draft to the Product Backlog Item in Azure DevOps as a file and copies a one-time link to it, ready to send to a reviewer, who imports it with **Import shared**. " +
        "No test cases are created. It needs a connection, so it is greyed out while you are offline.",
    },
    {
      id: "remove-all",
      shot: QUEUE,
      group: "the-queue",
      locate: { role: "button", name: "Clear queue" },
      name: "Clear queue",
      does: "Empties the queue and stops watching the imported files. Nothing in Azure DevOps is touched.",
    },
    {
      id: "order-testing",
      shot: QUEUE,
      group: "the-queue",
      locate: { role: "button", name: "For testing" },
      name: "Order: For testing",
      does:
        "Sorts the queue so cases that share a setup sit together. Offered when the cases carry an order, which an AI assistant adds when it optimises a draft. The queue's order is the order the cases are created in.",
    },
    {
      id: "order-spec",
      shot: QUEUE,
      group: "the-queue",
      locate: { role: "button", name: "Down the spec" },
      name: "Order: Down the spec",
      does: "Sorts the queue to follow the specification document, so a reviewer can read both side by side.",
    },
    {
      id: "group-by-area",
      shot: QUEUE,
      group: "the-queue",
      locate: { role: "switch", name: "Group by area" },
      name: "Group by area",
      does:
        "Shows the queue under a heading for each area, one level for each part of the area's path, in the order the cases come. " +
        "Each heading counts the cases under it: click its name to select them all (again to clear), or its arrow to fold it. Cases with no area sit under **No area**, last. " +
        "Only the view changes: the cases are still uploaded in queue order, and **View in browser** groups its page the same way.",
      tips: ["The switch and the groups you fold are remembered on this computer."],
    },
    {
      id: "select-all",
      shot: QUEUE,
      group: "queued-cases",
      locate: { role: "checkbox", name: "Select all queued cases" },
      name: "Select cases for bulk actions",
      does: "Selects every case, ready for a bulk action.",
    },
    {
      id: "row-select",
      shot: QUEUE,
      group: "queued-cases",
      locate: { role: "row", name: FIRST },
      name: "A queued case",
      does:
        "One row per case. Click it to select it; click it again to clear. [[Ctrl]]+click adds or removes one case, [[Shift]]+click selects a range. " +
        "Its buttons and links do their own job and leave the selection alone.",
      tips: [
        "A row reached with [[Tab]] is selected with [[Space]], with [[Ctrl]] or [[Shift]] held just as for a click.",
        "When the queue is grouped by area, a [[Shift]]+click range runs over the cases on screen and skips folded groups.",
      ],
    },
    {
      id: "expand-steps",
      shot: QUEUE,
      group: "queued-cases",
      locate: { role: "button", name: `Expand steps of ${FIRST}` },
      name: "Show steps (>)",
      does: "Opens the case to its steps, with the preconditions and reviewer notes, exactly as they will be written.",
    },
    {
      id: "update-badge",
      shot: QUEUE,
      group: "queued-cases",
      locate: { text: "UPDATE #5002" },
      name: "NEW or UPDATE",
      does: "**NEW** means the case will be created. **UPDATE** and a number means it will change that existing test case.",
    },
    {
      id: "what-changes",
      shot: QUEUE,
      group: "queued-cases",
      locate: { role: "button", nameRe: "^Click to view" },
      name: "What will change",
      does:
        "For an update, how many fields, steps and step types will change compared with the case in Azure DevOps. Click it to see each change. A **no-op** mark means the upload would change nothing.",
    },
    {
      id: "comment",
      shot: QUEUE,
      group: "queued-cases",
      locate: { text: "Asked the team how long the sign-in should last." },
      name: "Comment",
      does:
        "A note about the case, from the file, the review page or the case's editor (**Edit**). It stays in the app and the file, and is never sent to Azure DevOps.",
    },
    {
      id: "row-edit",
      shot: QUEUE,
      group: "queued-cases",
      locate: { role: "button", name: `Edit ${FIRST}` },
      name: "Edit",
      does: "Opens the case for changes right in the queue. See the editor below.",
    },
    {
      id: "row-remove",
      shot: QUEUE,
      group: "queued-cases",
      locate: { role: "button", name: `Remove ${FIRST} from the queue` },
      name: "Remove",
      does: "Takes the case out of the queue, and out of the file it came from, so the file and the queue still agree.",
    },
    {
      id: "review",
      shot: QUEUE,
      group: "upload",
      locate: { role: "button", nameRe: "^Review \\d+ test cases?$" },
      name: "Review",
      does: "Starts the final check before anything is written. It stays within reach at the bottom right while you scroll a long queue.",
    },

    // --- A case opened --------------------------------------------------------
    {
      id: "hide-steps",
      shot: OPEN,
      group: "queued-cases",
      locate: { role: "button", name: `Collapse steps of ${FIRST}` },
      name: "Hide steps (v)",
      does: "Closes the case again.",
    },
    {
      id: "case-preconditions",
      shot: OPEN,
      group: "queued-cases",
      locate: { text: "Preconditions:" },
      name: "Preconditions",
      does: "What must be true before the first step.",
    },
    {
      id: "reviewer-notes",
      shot: OPEN,
      group: "queued-cases",
      locate: { text: "Reviewer notes" },
      name: "Reviewer notes",
      does: "Notes for whoever reviews the draft, for example which part of the spec the case covers. They stay in the app and the review page.",
    },
    {
      id: "steps-table",
      shot: OPEN,
      group: "queued-cases",
      locate: { text: "Action" },
      name: "Steps",
      does: "Every step with its action and expected result, numbered in order.",
    },
    {
      id: "collapse-all",
      shot: OPEN,
      group: "queued-cases",
      locate: { role: "button", nameRe: "^Collapse all" },
      name: "Collapse all",
      does: "Closes every opened case at once. It shows while anything in the queue is open, with how many are.",
    },
    {
      id: "what-changes-open",
      shot: CHANGES,
      group: "queued-cases",
      locate: { role: "button", nameRe: "^Click to view" },
      name: "What will change, opened",
      does: "Click it again to close the changes.",
    },
    {
      id: "change-detail",
      shot: CHANGES,
      group: "queued-cases",
      locate: { text: "Title:" },
      name: "The changes",
      does: "Each field that will change, with the old and new wording marked word by word, and the steps that change.",
    },

    // --- Editing a queued case -------------------------------------------------
    {
      id: "editor-title",
      shot: EDIT,
      group: "edit-case",
      locate: { role: "textbox", name: "Case title" },
      name: "Title",
      does: "The case's title. [[Enter]] here saves, like **Save to queue**.",
    },
    {
      id: "editor-status",
      shot: EDIT,
      group: "edit-case",
      locate: { role: "combobox", name: "Automation status" },
      name: "Automation status",
      does: "**Not Automated** or **Planned**.",
    },
    {
      id: "editor-tags",
      shot: EDIT,
      group: "edit-case",
      locate: { role: "textbox", name: "Tags" },
      name: "Tags",
      does: "The case's tags. Type to search the project's tags or add a new one.",
    },
    {
      id: "editor-module",
      shot: EDIT,
      group: "edit-case",
      locate: { role: "combobox", name: "Module" },
      name: "Module",
      does: "The module the case belongs to.",
    },
    {
      id: "editor-preconditions",
      shot: EDIT,
      group: "edit-case",
      locate: { role: "textbox", name: "Preconditions" },
      name: "Preconditions",
      does: "What must be true before the first step.",
    },
    {
      id: "editor-comment",
      shot: EDIT,
      group: "edit-case",
      locate: { role: "textbox", name: "Comment (in-app only)" },
      name: "Comment (in-app only)",
      does: "A note about the case. It is saved in the JSON file and never sent to Azure DevOps.",
    },
    {
      id: "editor-steps",
      shot: EDIT,
      group: "edit-case",
      locate: { role: "textbox", name: "Step 1 action" },
      name: "Steps",
      does: "The steps, edited the same way as on Manual Entry: drag to reorder, **Add Step**, and the x to remove one.",
    },
    {
      id: "editor-save",
      shot: EDIT,
      group: "edit-case",
      locate: { role: "button", name: "Save to queue" },
      name: "Save to queue",
      does:
        "Keeps your changes in the queue and in the file the case came from; nothing goes to Azure DevOps until the upload. It is greyed out until something changes, or while the case has a problem, which is named beside it.",
    },
    {
      id: "editor-cancel",
      shot: EDIT,
      group: "edit-case",
      locate: { role: "button", name: "Cancel" },
      name: "Cancel",
      does: "Closes the editor and drops the changes.",
    },

    // --- Bulk actions -----------------------------------------------------------
    {
      id: "selected-count",
      shot: SELECTED,
      group: "many-at-once",
      locate: { text: "2 of 3 selected" },
      name: "Selected",
      does: "How many cases are selected.",
    },
    {
      id: "bulk-edit",
      shot: SELECTED,
      group: "many-at-once",
      locate: { role: "button", name: "Bulk edit" },
      name: "Bulk edit",
      does: "Changes the automation status, module, tags or preconditions of every selected case at once.",
    },
    {
      id: "rename-selected",
      shot: SELECTED,
      group: "many-at-once",
      locate: { role: "button", name: "Rename 2" },
      name: "Rename",
      does: "Opens the rename window for the selected cases only.",
    },
    {
      id: "remove-selected",
      shot: SELECTED,
      group: "many-at-once",
      locate: { role: "button", name: "Remove 2" },
      name: "Remove",
      does: "Takes the selected cases out of the queue.",
    },
    {
      id: "bulk-status",
      shot: BULK,
      group: "many-at-once",
      locate: { role: "combobox", name: "Automation status" },
      name: "Automation status",
      does: "Leave it unchanged, or set every selected case to **Not Automated** or **Planned**.",
    },
    {
      id: "bulk-module",
      shot: BULK,
      group: "many-at-once",
      locate: { role: "checkbox", name: "Set module" },
      name: "Set module",
      does: "Tick it to choose one module for all of them.",
    },
    {
      id: "bulk-tags",
      shot: BULK,
      group: "many-at-once",
      locate: { role: "combobox", name: "Tags" },
      name: "Tags",
      does: "Leave tags unchanged, add tags to the ones each case already has, or replace them.",
    },
    {
      id: "bulk-preconditions",
      shot: BULK,
      group: "many-at-once",
      locate: { role: "checkbox", name: "Set preconditions" },
      name: "Set preconditions",
      does: "Tick it to give them all the same preconditions.",
    },
    {
      id: "bulk-cancel",
      shot: BULK,
      group: "many-at-once",
      locate: { role: "button", name: "Cancel" },
      name: "Cancel",
      does: "Closes the window without changing anything.",
    },
    {
      id: "bulk-apply",
      shot: BULK,
      group: "many-at-once",
      locate: { role: "button", nameRe: "^Apply to \\d+$" },
      name: "Apply",
      does: "Makes the changes. Titles and steps are never touched, and the file each case came from is updated to match.",
    },

    // --- Rename -----------------------------------------------------------------
    {
      id: "rename-find",
      shot: RENAME,
      group: "many-at-once",
      locate: { role: "textbox", name: "Find" },
      name: "Find",
      does: "The text to look for in each title.",
    },
    {
      id: "rename-replace",
      shot: RENAME,
      group: "many-at-once",
      locate: { role: "textbox", name: "Replace with" },
      name: "Replace with",
      does: "What to put in its place. Leave it empty to delete the text found.",
    },
    {
      id: "rename-regex",
      shot: RENAME,
      group: "many-at-once",
      locate: { role: "checkbox", name: "Regular expression" },
      name: "Regular expression",
      does: "Treats **Find** as a pattern, so parts of it can be reused in the replacement as $1, $2 and so on.",
    },
    {
      id: "rename-match-case",
      shot: RENAME,
      group: "many-at-once",
      locate: { role: "checkbox", name: "Match case" },
      name: "Match case",
      does: "Only finds text with the same capital and small letters.",
    },
    {
      id: "rename-first-only",
      shot: RENAME,
      group: "many-at-once",
      locate: { role: "checkbox", name: "First match only" },
      name: "First match only",
      does: "Replaces only the first match in each title.",
    },
    {
      id: "rename-prefix",
      shot: RENAME,
      group: "many-at-once",
      locate: { role: "textbox", name: "Prefix" },
      name: "Prefix",
      does: "Text added to the start of every title.",
    },
    {
      id: "rename-suffix",
      shot: RENAME,
      group: "many-at-once",
      locate: { role: "textbox", name: "Suffix" },
      name: "Suffix",
      does: "Text added to the end of every title.",
    },
    {
      id: "rename-capitalisation",
      shot: RENAME,
      group: "many-at-once",
      locate: { role: "combobox", name: "Capitalisation" },
      name: "Capitalisation",
      does: "Leave titles as they are, or change them to Title Case, UPPERCASE or lowercase.",
    },
    {
      id: "rename-number-from",
      shot: RENAME,
      group: "many-at-once",
      locate: { role: "spinbutton", name: "Number from" },
      name: "Number from",
      does: "Where numbering starts. Put ${n} in the replacement, prefix or suffix to number the cases in the order shown.",
    },
    {
      id: "rename-digits",
      shot: RENAME,
      group: "many-at-once",
      locate: { role: "spinbutton", name: "Number digits" },
      name: "Digits",
      does: "How many digits each number has, with zeros in front: 3 digits gives 001, 002 and so on.",
    },
    {
      id: "rename-summary",
      shot: RENAME,
      group: "many-at-once",
      locate: { text: "0 will change" },
      name: "Preview",
      does:
        "The table above it shows every title now and after the rename, exactly as it will be saved. This line counts the changes, and warns when a new title would repeat another or cannot be saved.",
    },
    {
      id: "rename-cancel",
      shot: RENAME,
      group: "many-at-once",
      locate: { role: "button", name: "Cancel" },
      name: "Cancel",
      does: "Closes the window without renaming. After a rename it reads **Done**.",
    },
    {
      id: "rename-apply",
      shot: RENAME,
      group: "many-at-once",
      // The button's text sits in its centring span, so match the button by
      // what it reads as a whole, not by a text node of its own.
      locate: { css: `xpath=//*[@role="dialog"]//button[starts-with(normalize-space(.), "Rename ")]` },
      name: "Rename",
      does: "Renames the titles shown in the preview. Afterwards **Undo rename** puts them back.",
    },

    // --- Review -------------------------------------------------------------------
    {
      id: "check-pbi",
      shot: REVIEW,
      group: "upload",
      locate: { testId: "pbi" },
      name: "Highlighted Product Backlog Item",
      does: "While you confirm, the Product Backlog Item glows, so you can check the new cases are going to the right place.",
    },
    {
      id: "duplicate-hint",
      shot: REVIEW,
      group: "upload",
      locate: { text: "A test case with this title already exists on the PBI - this will create a duplicate, not update it." },
      name: "Duplicate warning",
      does: "Marks a new case whose title is already used by a case on the Product Backlog Item. Uploading it would make a second case, not update the first.",
    },
    {
      id: "stop-back",
      shot: REVIEW,
      group: "upload",
      locate: { role: "button", name: "Go back" },
      name: "Go back",
      does:
        "Leaves the review so you can fix the queue: remove the duplicate, or import a file that includes the existing case's id so it is updated instead.",
    },
    {
      id: "create-anyway",
      shot: REVIEW,
      group: "upload",
      locate: { role: "button", name: "Create duplicates anyway" },
      name: "Create duplicates anyway",
      does: "Accepts the duplicates. Nothing is written yet; it only makes the upload button available.",
    },
    {
      id: "confirm",
      shot: REVIEW,
      group: "upload",
      locate: { role: "button", nameRe: "^Yes . create" },
      name: "Yes, create and update",
      does:
        "Uploads the queue. New cases are created, linked to the Product Backlog Item and added to its test suite; cases with an id are updated. " +
        "The button says how many of each, and stays greyed out while a duplicate waits for an answer or a case has a problem to fix. " +
        "With only updates in the queue there is no Product Backlog Item to check, so it reads **Confirm & update** and the number, and uploads at once.",
    },
    {
      id: "back",
      shot: REVIEW,
      group: "upload",
      locate: { role: "button", name: "Back" },
      name: "Back",
      does: "Leaves the review without uploading anything.",
    },

    // --- Results --------------------------------------------------------------------
    {
      id: "uploaded",
      shot: RESULTS,
      group: "upload",
      locate: { text: "UPLOADED" },
      name: "UPLOADED",
      does: "Marks a case the last upload wrote. It stays in the queue, now carrying its id, until you remove it.",
    },
    {
      id: "results",
      shot: RESULTS,
      group: "upload",
      locate: { text: "1 test case uploaded - 1 updated" },
      name: "Results",
      does:
        "What the upload did, in one line, then every case with its id, marked **NEW**, **UPDATED**, **FAILED** or **UNKNOWN** (it may have been written; see the tips on held cases).",
    },
    {
      id: "copy-changes",
      shot: RESULTS,
      group: "upload",
      locate: { role: "button", name: "Copy changes" },
      name: "Copy changes",
      does:
        "Copies, for a tester, each updated case's id with what changed in it, followed by the ids and titles of the new cases, so they know what to run again.",
    },
    {
      id: "clear-results",
      shot: RESULTS,
      group: "upload",
      locate: { role: "button", name: "Clear results" },
      name: "Clear results",
      does: "Hides the results and the marks they left on the rows.",
    },
  ],
  tips: [
    "Only a case id updates a work item. A title that matches an existing case is a warning, never an update.",
    "While a file is watched, a line under **Import JSON** names it and how many cases it added. **Stop** stops following that file (**Stop all** there stops every one) and asks whether to keep the cases it added or remove them too. Cases you typed by hand are never removed.",
    "Under each watched file, **Attach spec…** adds specification documents (Markdown files) and **Add wiki link** adds an Azure DevOps wiki page. Both open beside the cases on the review page. Nothing else can be a spec: any other file or link is refused with the reason.",
    "**General comments**, under the watched files, holds notes about the whole set, such as a question you asked a developer. They are saved into the file.",
    "When a watched file changes, a panel says what was added, changed or removed, and the rows it touched are outlined in the queue. **Show details** lists each change; the x closes the panel. A new case the file changed also gets a link on its own row, such as \"Title, 2 steps changed\", that opens the old and new wording right there; closing the panel removes the link.",
    "With the queue empty, **Recent JSON Imports** lists files you imported before. **Open** imports one again as it is now; the x takes it off the list.",
    "Warnings about an imported file, such as a field it could not read, are listed under the share link.",
    "While an upload runs, a bar under the queue says what it is doing (checking what changed, then uploading) and how many cases are done.",
    "If an upload is cut off before Azure DevOps answers, the cases it may have created are marked and uploading is held until you press **Check with Azure DevOps**, so nothing is created twice.",
    "When a held case's title matches more than one test case in Azure DevOps, the app cannot tell which is yours. Check in Azure DevOps, then press **Release** and confirm, and the case can be uploaded again.",
  ],
  howTo: [
    {
      title: "Import a file and upload it",
      steps: [
        "Choose the Product Backlog Item in the bar at the top.",
        "Press **Import JSON** and pick the file.",
        "Check the queue: open a case with **>** to read its steps, and open **what will change** on each update.",
        "Press **Review**. Deal with any duplicate or flagged case.",
        "Check the highlighted Product Backlog Item, then press the button that says what it will create and update.",
      ],
    },
    {
      title: "Use the review page",
      steps: [
        "Press **View in browser**. The page opens in your browser, in the app's theme, with a switch for light and dark.",
        "Every case is listed with its **NEW** or **UPDATE** mark. Search, and narrow the search to one field (title, ID, prerequisites, steps, tags or module). What it finds is highlighted in each case.",
        "The switches at the end of the search box change how it matches: [[Aa]] matches case, [[ab]] matches whole words only, and [[.*]] searches with a regular expression ([[Alt+C]], [[Alt+W]] and [[Alt+R]] while typing). Put words in quotes to find them together, as written.",
        "Type in the comment box under a case, or in the notes about the whole set. They save by themselves into the file the cases came from.",
        "To have them dealt with, ask your AI assistant: **Address the comments I made for the test cases**. It reads each comment and fixes the cases in the file. The Review page section has every part of the page.",
        "Press the bookmark on a case to mark where you stopped; **Go to bookmark** brings you back to it later.",
        "**Options** hides the reviewer notes, the findings or the spec pane, and opens the Test map with **View as Tree**. Under **Show on cards**, untick Automation Status, Module or Tags to hide them from every case.",
        "Keep the page open: when the queue changes, a bar offers to refresh it.",
      ],
    },
    {
      title: "See the cases as a Test map",
      steps: [
        "On the review page, open **Options** and press **View as Tree**. It is there when at least one case has an area in its file.",
        "The cases are drawn as a tree of the areas they cover. Zoom out for the area names; zoom in for ids and then titles.",
        "Click an area to fold it, and click a case to see its steps. A link on the map goes back to the review page.",
      ],
    },
  ],
};
