// Import File: bringing cases in (a JSON file, a share link), the queue and
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
  title: "Import File",
  group: "Test cases",
  summary:
    "Bring test cases in from a JSON file or a teammate's share link, check them in the queue, then upload them to the PBI. " +
    "A case that carries an id updates that exact work item; a case without one is created new.",
  shots: [
    { id: QUEUE, route: [{ nav: "Import File" }], alt: "Import File with three cases in the queue" },
    {
      id: OPEN,
      route: [{ nav: "Import File" }, { click: { role: "button", name: `Expand steps of ${FIRST}` } }],
      alt: "A queued case opened to its steps",
    },
    {
      id: CHANGES,
      route: [{ nav: "Import File" }, { click: { role: "button", nameRe: "^Click to view" } }],
      alt: "An update opened to what will change",
    },
    {
      id: EDIT,
      route: [
        { nav: "Import File" },
        { click: { role: "button", name: `Edit ${FIRST}` } },
        { waitFor: { role: "button", name: "Save to queue" } },
        { scrollTo: { role: "button", name: "Save to queue" } },
      ],
      alt: "A queued case open for editing in the queue",
    },
    {
      id: SELECTED,
      route: [
        { nav: "Import File" },
        { click: { role: "checkbox", name: `Select ${FIRST}` } },
        { click: { role: "checkbox", name: `Select ${THIRD}` } },
      ],
      alt: "Two queued cases ticked, with the bulk actions",
    },
    {
      id: BULK,
      route: [
        { nav: "Import File" },
        { click: { role: "checkbox", name: `Select ${FIRST}` } },
        { click: { role: "checkbox", name: `Select ${THIRD}` } },
        { click: { role: "button", name: "Bulk edit" } },
        { waitFor: { role: "button", nameRe: "^Apply to \\d+$" } },
      ],
      alt: "The Bulk edit window for two queued cases",
    },
    {
      id: RENAME,
      route: [{ nav: "Import File" }, { click: { role: "button", name: "Rename" } }, { waitFor: { role: "textbox", name: "Find" } }],
      alt: "The rename window for the queued cases",
    },
    {
      id: REVIEW,
      route: [
        { nav: "Import File" },
        { click: { role: "button", nameRe: "^Review \\d+ test cases?$" } },
        { waitFor: { role: "button", name: "Create duplicates anyway" } },
        { scrollTo: { role: "button", nameRe: "^Yes . create" } },
      ],
      alt: "Reviewing the queue before upload, stopped by a duplicate title",
    },
    {
      id: RESULTS,
      route: [
        { nav: "Import File" },
        { click: { role: "checkbox", name: `Select ${FIRST}` } },
        { click: { role: "checkbox", name: `Select ${THIRD}` } },
        { click: { role: "button", name: "Remove 2" } },
        { click: { role: "button", nameRe: "^Review \\d+ test cases?$" } },
        { click: { role: "button", nameRe: "^Confirm & update" } },
        { waitFor: { role: "button", name: "Clear results" } },
      ],
      alt: "The results of an upload",
    },
  ],
  controls: [
    // --- Bringing cases in, and the queue ---------------------------------
    {
      id: "import-json",
      shot: QUEUE,
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
      locate: { role: "textbox", name: "Share link" },
      name: "Share link",
      does: "Paste a share link a teammate sent you. A link works once: after the import it expires.",
    },
    {
      id: "import-shared",
      shot: QUEUE,
      locate: { role: "button", name: "Import shared" },
      name: "Import shared",
      does:
        "Fetches the shared draft and adds its cases to the queue. A copy is saved on this computer and watched like an imported file, so the ids an upload creates are kept.",
      tips: [
        "A draft shared for a different PBI asks first: stay on the PBI you have chosen, or switch to the one it was shared for.",
      ],
    },
    {
      id: "queue-title",
      shot: QUEUE,
      locate: { text: "Queue for PBI #1001 (3 queued)" },
      name: "Queue for PBI",
      does: "Every case waiting to be uploaded to this PBI, and how many there are. The queue is shared with Manual Entry and kept on this computer for each PBI.",
    },
    {
      id: "view-in-browser",
      shot: QUEUE,
      locate: { role: "button", name: "View in browser" },
      name: "View in browser",
      does: "Opens the queue as a review page in your browser, where you can read every case and leave comments. See **Use the review page** below.",
    },
    {
      id: "share-for-review",
      shot: QUEUE,
      locate: { role: "button", name: "Share for review" },
      name: "Share for review",
      does:
        "Attaches the draft to the PBI in Azure DevOps as a file and copies a one-time link to it, ready to send to a reviewer, who imports it with **Import shared**. " +
        "No test cases are created. It needs a connection, so it is greyed out while you are offline.",
    },
    {
      id: "export-json",
      shot: QUEUE,
      locate: { role: "button", name: "Export JSON" },
      name: "Export JSON",
      does: "Saves the queue to a JSON file you choose.",
    },
    {
      id: "rename",
      shot: QUEUE,
      locate: { role: "button", name: "Rename" },
      name: "Rename",
      does: "Opens the rename window for every queued title, or only the ticked ones when some are ticked.",
    },
    {
      id: "remove-all",
      shot: QUEUE,
      locate: { role: "button", name: "Remove all" },
      name: "Remove all",
      does: "Empties the queue and stops watching the imported files. Nothing in Azure DevOps is touched.",
    },
    {
      id: "order-testing",
      shot: QUEUE,
      locate: { role: "button", name: "For testing" },
      name: "Order: For testing",
      does:
        "Sorts the queue so cases that share a setup sit together. Offered when the cases carry an order, which an AI assistant adds when it optimises a draft. The queue's order is the order the cases are created in.",
    },
    {
      id: "order-spec",
      shot: QUEUE,
      locate: { role: "button", name: "Down the spec" },
      name: "Order: Down the spec",
      does: "Sorts the queue to follow the specification document, so a reviewer can read both side by side.",
    },
    {
      id: "select-all",
      shot: QUEUE,
      locate: { role: "checkbox", name: "Select all queued cases" },
      name: "Select cases for bulk actions",
      does: "Ticks every case, ready for a bulk action.",
    },
    {
      id: "row-select",
      shot: QUEUE,
      locate: { role: "checkbox", name: `Select ${FIRST}` },
      name: "A queued case",
      does:
        "One row per case. Its box ticks it for a bulk action; hold [[Shift]] and click another box to tick everything in between.",
    },
    {
      id: "expand-steps",
      shot: QUEUE,
      locate: { role: "button", name: `Expand steps of ${FIRST}` },
      name: "Show steps (>)",
      does: "Opens the case to its steps, with the preconditions and reviewer notes, exactly as they will be written.",
    },
    {
      id: "update-badge",
      shot: QUEUE,
      locate: { text: "UPDATE #5002" },
      name: "NEW or UPDATE",
      does: "**NEW** means the case will be created. **UPDATE** and a number means it will change that existing test case.",
    },
    {
      id: "what-changes",
      shot: QUEUE,
      locate: { role: "button", nameRe: "^Click to view" },
      name: "What will change",
      does:
        "For an update, how many fields, steps and step types will change compared with the case in Azure DevOps. Click it to see each change. A **no-op** mark means the upload would change nothing.",
    },
    {
      id: "comment",
      shot: QUEUE,
      locate: { text: "Asked the team how long the sign-in should last." },
      name: "Comment",
      does:
        "A note about the case, from the file, the review page or the case's editor (**Edit**). It stays in the app and the file, and is never sent to Azure DevOps.",
    },
    {
      id: "row-edit",
      shot: QUEUE,
      locate: { role: "button", name: `Edit ${FIRST}` },
      name: "Edit",
      does: "Opens the case for changes right in the queue. See the editor below.",
    },
    {
      id: "row-remove",
      shot: QUEUE,
      locate: { role: "button", name: `Remove ${FIRST} from the queue` },
      name: "Remove",
      does: "Takes the case out of the queue, and out of the file it came from, so the file and the queue still agree.",
    },
    {
      id: "review",
      shot: QUEUE,
      locate: { role: "button", nameRe: "^Review \\d+ test cases?$" },
      name: "Review",
      does: "Starts the final check before anything is written. It stays within reach at the bottom right while you scroll a long queue.",
    },

    // --- A case opened --------------------------------------------------------
    {
      id: "hide-steps",
      shot: OPEN,
      locate: { role: "button", name: `Collapse steps of ${FIRST}` },
      name: "Hide steps (v)",
      does: "Closes the case again.",
    },
    {
      id: "case-preconditions",
      shot: OPEN,
      locate: { text: "Preconditions:" },
      name: "Preconditions",
      does: "What must be true before the first step.",
    },
    {
      id: "reviewer-notes",
      shot: OPEN,
      locate: { text: "Reviewer notes" },
      name: "Reviewer notes",
      does: "Notes for whoever reviews the draft, for example which part of the spec the case covers. They stay in the app and the review page.",
    },
    {
      id: "steps-table",
      shot: OPEN,
      locate: { text: "Action" },
      name: "Steps",
      does: "Every step with its action and expected result, numbered in order.",
    },
    {
      id: "collapse-all",
      shot: OPEN,
      locate: { role: "button", nameRe: "^Collapse all" },
      name: "Collapse all",
      does: "Closes every opened case at once. It shows while anything in the queue is open, with how many are.",
    },
    {
      id: "what-changes-open",
      shot: CHANGES,
      locate: { role: "button", nameRe: "^Click to view" },
      name: "What will change, opened",
      does: "Click it again to close the changes.",
    },
    {
      id: "change-detail",
      shot: CHANGES,
      locate: { text: "Title:" },
      name: "The changes",
      does: "Each field that will change, with the old and new wording marked word by word, and the steps that change.",
    },

    // --- Editing a queued case -------------------------------------------------
    {
      id: "editor-title",
      shot: EDIT,
      locate: { role: "textbox", name: "Case title" },
      name: "Title",
      does: "The case's title. [[Enter]] here saves, like **Save to queue**.",
    },
    {
      id: "editor-status",
      shot: EDIT,
      locate: { role: "combobox", name: "Automation status" },
      name: "Automation status",
      does: "**Not Automated** or **Planned**.",
    },
    {
      id: "editor-tags",
      shot: EDIT,
      locate: { role: "textbox", name: "Tags" },
      name: "Tags",
      does: "The case's tags. Type to search the project's tags or add a new one.",
    },
    {
      id: "editor-module",
      shot: EDIT,
      locate: { role: "combobox", name: "Module" },
      name: "Module",
      does: "The module the case belongs to.",
    },
    {
      id: "editor-preconditions",
      shot: EDIT,
      locate: { role: "textbox", name: "Preconditions" },
      name: "Preconditions",
      does: "What must be true before the first step.",
    },
    {
      id: "editor-comment",
      shot: EDIT,
      locate: { role: "textbox", name: "Comment (in-app only)" },
      name: "Comment (in-app only)",
      does: "A note about the case. It is saved in the JSON file and never sent to Azure DevOps.",
    },
    {
      id: "editor-steps",
      shot: EDIT,
      locate: { role: "textbox", name: "Step 1 action" },
      name: "Steps",
      does: "The steps, edited the same way as on Manual Entry: drag to reorder, **Add Step**, and the x to remove one.",
    },
    {
      id: "editor-save",
      shot: EDIT,
      locate: { role: "button", name: "Save to queue" },
      name: "Save to queue",
      does:
        "Keeps your changes in the queue and in the file the case came from; nothing goes to Azure DevOps until the upload. It is greyed out until something changes, or while the case has a problem, which is named beside it.",
    },
    {
      id: "editor-cancel",
      shot: EDIT,
      locate: { role: "button", name: "Cancel" },
      name: "Cancel",
      does: "Closes the editor and drops the changes.",
    },
    {
      id: "editor-close",
      shot: EDIT,
      locate: { role: "button", name: `Close the editor for ${FIRST}` },
      name: "Close",
      does: "While the editor is open, the row's **Edit** reads **Close** and closes it the same way.",
    },

    // --- Bulk actions -----------------------------------------------------------
    {
      id: "selected-count",
      shot: SELECTED,
      locate: { text: "2 of 3 selected" },
      name: "Selected",
      does: "How many cases are ticked.",
    },
    {
      id: "bulk-edit",
      shot: SELECTED,
      locate: { role: "button", name: "Bulk edit" },
      name: "Bulk edit",
      does: "Changes the automation status, module, tags or preconditions of every ticked case at once.",
    },
    {
      id: "rename-selected",
      shot: SELECTED,
      locate: { role: "button", name: "Rename 2" },
      name: "Rename",
      does: "Opens the rename window for the ticked cases only.",
    },
    {
      id: "remove-selected",
      shot: SELECTED,
      locate: { role: "button", name: "Remove 2" },
      name: "Remove",
      does: "Takes the ticked cases out of the queue.",
    },
    {
      id: "bulk-status",
      shot: BULK,
      locate: { role: "combobox", name: "Automation status" },
      name: "Automation status",
      does: "Leave it unchanged, or set every ticked case to **Not Automated** or **Planned**.",
    },
    {
      id: "bulk-module",
      shot: BULK,
      locate: { role: "checkbox", name: "Set module" },
      name: "Set module",
      does: "Tick it to choose one module for all of them.",
    },
    {
      id: "bulk-tags",
      shot: BULK,
      locate: { role: "combobox", name: "Tags" },
      name: "Tags",
      does: "Leave tags unchanged, add tags to the ones each case already has, or replace them.",
    },
    {
      id: "bulk-preconditions",
      shot: BULK,
      locate: { role: "checkbox", name: "Set preconditions" },
      name: "Set preconditions",
      does: "Tick it to give them all the same preconditions.",
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
      name: "Apply",
      does: "Makes the changes. Titles and steps are never touched, and the file each case came from is updated to match.",
    },

    // --- Rename -----------------------------------------------------------------
    {
      id: "rename-find",
      shot: RENAME,
      locate: { role: "textbox", name: "Find" },
      name: "Find",
      does: "The text to look for in each title.",
    },
    {
      id: "rename-replace",
      shot: RENAME,
      locate: { role: "textbox", name: "Replace with" },
      name: "Replace with",
      does: "What to put in its place. Leave it empty to delete the text found.",
    },
    {
      id: "rename-regex",
      shot: RENAME,
      locate: { role: "checkbox", name: "Regular expression" },
      name: "Regular expression",
      does: "Treats **Find** as a pattern, so parts of it can be reused in the replacement as $1, $2 and so on.",
    },
    {
      id: "rename-match-case",
      shot: RENAME,
      locate: { role: "checkbox", name: "Match case" },
      name: "Match case",
      does: "Only finds text with the same capital and small letters.",
    },
    {
      id: "rename-first-only",
      shot: RENAME,
      locate: { role: "checkbox", name: "First match only" },
      name: "First match only",
      does: "Replaces only the first match in each title.",
    },
    {
      id: "rename-prefix",
      shot: RENAME,
      locate: { role: "textbox", name: "Prefix" },
      name: "Prefix",
      does: "Text added to the start of every title.",
    },
    {
      id: "rename-suffix",
      shot: RENAME,
      locate: { role: "textbox", name: "Suffix" },
      name: "Suffix",
      does: "Text added to the end of every title.",
    },
    {
      id: "rename-capitalisation",
      shot: RENAME,
      locate: { role: "combobox", name: "Capitalisation" },
      name: "Capitalisation",
      does: "Leave titles as they are, or change them to Title Case, UPPERCASE or lowercase.",
    },
    {
      id: "rename-number-from",
      shot: RENAME,
      locate: { role: "spinbutton", name: "Number from" },
      name: "Number from",
      does: "Where numbering starts. Put ${n} in the replacement, prefix or suffix to number the cases in the order shown.",
    },
    {
      id: "rename-digits",
      shot: RENAME,
      locate: { role: "spinbutton", name: "Number digits" },
      name: "Digits",
      does: "How many digits each number has, with zeros in front: 3 digits gives 001, 002 and so on.",
    },
    {
      id: "rename-summary",
      shot: RENAME,
      locate: { text: "0 will change" },
      name: "Preview",
      does:
        "The table above it shows every title now and after the rename, exactly as it will be saved. This line counts the changes, and warns when a new title would repeat another or cannot be saved.",
    },
    {
      id: "rename-cancel",
      shot: RENAME,
      locate: { role: "button", name: "Cancel" },
      name: "Cancel",
      does: "Closes the window without renaming. After a rename it reads **Done**.",
    },
    {
      id: "rename-apply",
      shot: RENAME,
      locate: { role: "button", nameRe: "^Rename \\d+$" },
      name: "Rename",
      does: "Renames the titles shown in the preview. Afterwards **Undo rename** puts them back.",
    },

    // --- Review -------------------------------------------------------------------
    {
      id: "check-pbi",
      shot: REVIEW,
      locate: { testId: "pbi" },
      name: "Highlighted PBI",
      does: "While you confirm, the PBI glows, so you can check the new cases are going to the right place.",
    },
    {
      id: "duplicate-hint",
      shot: REVIEW,
      locate: { text: "A test case with this title already exists on the PBI - this will create a duplicate, not update it." },
      name: "Duplicate warning",
      does: "Marks a new case whose title is already used by a case on the PBI. Uploading it would make a second case, not update the first.",
    },
    {
      id: "stop-back",
      shot: REVIEW,
      locate: { role: "button", nameRe: "^Stop . take me back$" },
      name: "Stop, take me back",
      does:
        "Leaves the review so you can fix the queue: remove the duplicate, or import a file that includes the existing case's id so it is updated instead.",
    },
    {
      id: "create-anyway",
      shot: REVIEW,
      locate: { role: "button", name: "Create duplicates anyway" },
      name: "Create duplicates anyway",
      does: "Accepts the duplicates. Nothing is written yet; it only makes the upload button available.",
    },
    {
      id: "confirm",
      shot: REVIEW,
      locate: { role: "button", nameRe: "^Yes . create" },
      name: "Yes, create and update",
      does:
        "Uploads the queue. New cases are created, linked to the PBI and added to its test suite; cases with an id are updated. " +
        "The button says how many of each, and stays greyed out while a duplicate waits for an answer or a case has a problem to fix. " +
        "With only updates in the queue there is no PBI to check, so it reads **Confirm & update** and the number, and uploads at once.",
    },
    {
      id: "back",
      shot: REVIEW,
      locate: { role: "button", name: "Back" },
      name: "Back",
      does: "Leaves the review without uploading anything.",
    },

    // --- Results --------------------------------------------------------------------
    {
      id: "uploaded",
      shot: RESULTS,
      locate: { text: "UPLOADED" },
      name: "UPLOADED",
      does: "Marks a case the last upload wrote. It stays in the queue, now carrying its id, until you remove it.",
    },
    {
      id: "results",
      shot: RESULTS,
      locate: { text: "1 test case uploaded - 1 updated" },
      name: "Results",
      does:
        "What the upload did, in one line, then every case with its id, marked **NEW**, **UPDATED**, **FAILED** or **UNKNOWN** (it may have been written; see the tips on held cases).",
    },
    {
      id: "copy-changes",
      shot: RESULTS,
      locate: { role: "button", name: "Copy changes" },
      name: "Copy changes",
      does:
        "Copies, for a tester, each updated case's id with what changed in it, followed by the ids and titles of the new cases, so they know what to run again.",
    },
    {
      id: "clear-results",
      shot: RESULTS,
      locate: { role: "button", name: "Clear results" },
      name: "Clear results",
      does: "Hides the results and the marks they left on the rows.",
    },
  ],
  tips: [
    "Only a case id updates a work item. A title that matches an existing case is a warning, never an update.",
    "While a file is watched, a line under **Import JSON** names it and how many cases it added. **Stop** stops following that file (**Remove all** there stops every one) and asks whether to keep the cases it added or remove them too. Cases you typed by hand are never removed.",
    "Under each watched file, **Attach spec…** adds specification documents (Markdown or text files) and **Add wiki link** adds an Azure DevOps wiki page. Both open beside the cases on the review page.",
    "**General comments**, under the watched files, holds notes about the whole set, such as a question you asked a developer. They are saved into the file.",
    "When a watched file changes, a panel says what was added, changed or removed, and the rows it touched are outlined in the queue. **Show details** lists each change; the x closes the panel.",
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
        "Choose the PBI in the bar at the top.",
        "Press **Import JSON** and pick the file.",
        "Check the queue: open a case with **>** to read its steps, and open **what will change** on each update.",
        "Press **Review**. Deal with any duplicate or flagged case.",
        "Check the highlighted PBI, then press the button that says what it will create and update.",
      ],
    },
    {
      title: "Use the review page",
      steps: [
        "Press **View in browser**. The page opens in your browser, in the app's theme, with a switch for light and dark.",
        "Every case is listed with its **NEW** or **UPDATE** mark. Search, and narrow the search to one field (title, ID, prerequisites, steps, tags or module).",
        "Type in the comment box under a case, or in the notes about the whole set. They save by themselves into the file the cases came from.",
        "Press the bookmark on a case to mark where you stopped; **Go to bookmark** brings you back to it later.",
        "**Options** hides the reviewer notes, the findings or the spec pane, and opens the Test map with **View as Tree**.",
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
