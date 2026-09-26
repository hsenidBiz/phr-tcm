// Run Tests: the PBI's test points with their last outcomes, the Execution
// order window, and the runner window a run happens in.
//
// Locates come from screens/RunPanel/index.tsx, CasePreview.tsx,
// ExecutionOrderModal.tsx, ManageCases/CaseOrderList.tsx,
// screens/RunnerWindow.tsx and components/BugDialog.tsx. Rows are picked by
// clicking their "#id" (a click anywhere on a row toggles it). The runner
// shots set `size` to the runner window's real size (src/lib/runnerSize.ts)
// and mark case #5001 Failed first, so File bug shows.
//
// The Execution order window and the bug window open over a screen that
// has a button of the same name (Select all, File bug), so those two are
// described on a neighbouring control instead of marked twice.

import { RUNNER_HEIGHT, RUNNER_WIDTH } from "../../../src/lib/runnerSize";
import type { Screen, Step } from "../types";

const LIST = "run-tests-list";
const SELECTED = "run-tests-selected";
const PREVIEW = "run-tests-preview";
const ORDER = "run-tests-order";
const ORDER_SAVE = "run-tests-order-save";
const GROUPED = "run-tests-grouped";
const RUNNER = "run-tests-runner";
const BUG = "run-tests-bug";

const NAV: Step = { nav: "Run Tests" };
const PICK_TWO: Step[] = [{ click: { text: "#5001" } }, { click: { text: "#5002" } }];
const OPEN_ORDER: Step[] = [
  { click: { role: "button", name: "Set execution order" } },
  { waitFor: { role: "combobox", name: "Start from" } },
];
const TO_RUNNER: Step[] = [
  NAV,
  ...PICK_TWO,
  { click: { role: "button", name: "Run 2 in runner" } },
  { runnerWindow: true },
  { waitFor: { role: "button", name: "Failed" } },
  { click: { role: "button", name: "Failed" } },
];
const RUNNER_SIZE = { w: RUNNER_WIDTH, h: RUNNER_HEIGHT };

export const runTests: Screen = {
  id: "run-tests",
  title: "Run Tests",
  group: "Running tests",
  summary:
    "Run the PBI's test cases and record the results in Azure DevOps. Pick the cases, choose the order to run them in, " +
    "then step through them in the runner: a small window that stays beside whatever you are testing.",
  shots: [
    { id: LIST, route: [NAV], alt: "Run Tests listing the PBI's test cases with their last outcomes" },
    { id: SELECTED, route: [NAV, ...PICK_TWO], alt: "Two test cases selected, ready to run" },
    {
      id: PREVIEW,
      route: [
        NAV,
        { click: { role: "button", name: "Expand test case #5001" } },
        { click: { role: "button", nameRe: "^Execution history" } },
      ],
      alt: "A test case opened to its last result, its earlier results and its steps",
    },
    { id: ORDER, route: [NAV, ...OPEN_ORDER], alt: "The Execution order window" },
    {
      id: GROUPED,
      route: [
        NAV,
        ...OPEN_ORDER,
        { click: { role: "combobox", name: "Group cases" } },
        { click: { role: "option", name: "By title" } },
        { click: { role: "button", name: "Use this order" } },
        { waitFor: { role: "button", name: "Collapse group Login" } },
      ],
      alt: "The test cases grouped by the start of their titles",
    },
    {
      id: ORDER_SAVE,
      route: [NAV, ...OPEN_ORDER, { click: { role: "button", name: "Save for everyone" } }],
      alt: "Confirming a suggested run order for every tester",
    },
    { id: RUNNER, route: TO_RUNNER, size: RUNNER_SIZE, alt: "The runner window on the first of two cases, marked Failed" },
    {
      id: BUG,
      route: [...TO_RUNNER, { click: { role: "button", name: "File bug" } }, { waitFor: { role: "textbox", name: "Bug title" } }],
      size: RUNNER_SIZE,
      alt: "Filing a bug from the runner",
    },
  ],
  controls: [
    // --- The list ----------------------------------------------------------
    {
      id: "execution-report",
      shot: LIST,
      locate: { role: "button", name: "Execution report" },
      name: "Execution report",
      does: "Opens a report of the selected cases' results in your browser. It is greyed out until at least one case is selected, as here.",
    },
    {
      id: "suite",
      shot: LIST,
      locate: { text: 'Plan "Release 2.4" / suite 91' },
      name: "Plan and suite",
      does: "The test plan and the PBI's test suite the results are recorded in. The first time you open a PBI here, the app finds its suite, or creates one when it has none.",
    },
    {
      id: "refresh-outcomes",
      shot: LIST,
      locate: { role: "button", name: "Refresh outcomes" },
      name: "Refresh outcomes",
      does: "Reads the latest outcomes, history and run order from Azure DevOps. [[Shift]]+click looks for the PBI's test suite again from scratch.",
    },
    {
      id: "set-order",
      shot: LIST,
      locate: { role: "button", name: "Set execution order" },
      name: "Set execution order",
      does: "Opens the Execution order window, where you choose the order the list and the runner follow, and how the list is grouped. It is greyed out while the run order is loading.",
    },
    {
      id: "filter-text",
      shot: LIST,
      locate: { role: "textbox", name: "Filter points" },
      name: "Filter by name or id",
      does: "Shows only the cases whose title or id contains what you type. Filtering hides rows; it never changes the order.",
    },
    {
      id: "filter-outcome",
      shot: LIST,
      locate: { role: "combobox", name: "Filter by last outcome" },
      name: "Last outcome filter",
      does: "Shows only the cases whose last result was Passed, Failed, Paused, Blocked or Not Applicable, or that were **Never run**.",
      tips: ["To run the failed cases again: pick **Failed** here, then select them all and run them."],
    },
    {
      id: "select-all",
      shot: LIST,
      locate: { role: "button", name: "Select all" },
      name: "Select all",
      does: "Selects every case the list shows, filters included. [[Ctrl]]+[[A]] does the same. While the list is grouped, the button is hidden and each group's box selects that group.",
    },
    {
      id: "expand",
      shot: LIST,
      locate: { role: "button", name: "Expand test case #5001" },
      name: "Open (>)",
      does: "Opens the case below its row: the last result's comment and bugs, earlier results, and the steps. Several can be open at once.",
    },
    {
      id: "row",
      shot: LIST,
      locate: { role: "row", nameRe: "#5003 Login - locked account" },
      name: "Test case",
      does:
        "One test case, tinted by its last outcome. Click it to select it, click again to clear it; [[Shift]]+click selects everything between two cases. " +
        "The runner only runs the cases you select.",
    },
    {
      id: "last-outcome",
      shot: LIST,
      locate: { role: "columnheader", name: "Last outcome" },
      name: "Last outcome",
      does: "The result the case got the last time it was run. A dash means it has never been run.",
    },
    {
      id: "history",
      shot: LIST,
      locate: { role: "columnheader", name: "History" },
      name: "History",
      does: "The last few results as coloured dots, newest on the left. Hover a dot for its outcome, date and run.",
    },

    // --- A selection --------------------------------------------------------
    {
      id: "run-in-runner",
      shot: SELECTED,
      locate: { role: "button", nameRe: "^Run \\d+ in runner$" },
      name: "Run N in runner",
      does:
        "Opens the runner with the selected cases, in the order the list shows them. If the runner is already open, it comes to the front. " +
        "When the row scrolls away, this button floats at the bottom right.",
    },
    {
      id: "clear-selection",
      shot: SELECTED,
      locate: { role: "button", name: "Clear selection" },
      name: "Clear selection (x)",
      does: "Clears the selection.",
    },

    // --- Grouped -------------------------------------------------------------
    {
      id: "group-fold",
      shot: GROUPED,
      locate: { role: "button", name: "Collapse group Login" },
      name: "Fold group (v)",
      does: "Folds the group away, or opens it again. A case you have opened stays on screen when its group is folded.",
    },
    {
      id: "group-select",
      shot: GROUPED,
      locate: { role: "checkbox", name: "Select all in Login" },
      name: "Select the group",
      does: "Selects every case in the group, or clears them when they are all selected. Folding a group does not unselect its cases.",
    },
    {
      id: "group-name",
      shot: GROUPED,
      locate: { role: "button", name: "Login (3)" },
      name: "Group name",
      does:
        "The group's name and how many cases it holds. Clicking it folds or opens the group. Grouped **By title**, cases whose titles start the same way are together; **By area** uses the areas in the suggested run order.",
    },

    // --- An open case -------------------------------------------------------
    {
      id: "last-result",
      shot: PREVIEW,
      locate: { text: "Last result" },
      name: "Last result",
      does: "The comment recorded with the latest result, and the bugs linked to it.",
    },
    {
      id: "bug-link",
      shot: PREVIEW,
      locate: { role: "button", name: "#2003" },
      name: "Bug",
      does: "Opens that bug in Azure DevOps, in your browser.",
    },
    {
      id: "execution-history",
      shot: PREVIEW,
      locate: { role: "button", name: "Hide execution history" },
      name: "Execution history",
      does:
        "Shows or hides the earlier results: each one's outcome, date, run, who ran it, its comment and its screenshots. Click a screenshot to see it full size. " +
        "It is offered when the case has been run more than once.",
    },
    {
      id: "steps",
      shot: PREVIEW,
      locate: { role: "columnheader", name: "Action" },
      name: "Steps",
      does: "The case's steps, so a failure can be checked without opening the runner.",
    },
    {
      id: "collapse-all",
      shot: PREVIEW,
      locate: { role: "button", nameRe: "^Collapse all \\(\\d+\\)$" },
      name: "Collapse all",
      does: "Closes every open case and folds every open group in one press.",
    },

    // --- Execution order ----------------------------------------------------
    {
      id: "start-from",
      shot: ORDER,
      locate: { role: "combobox", name: "Start from" },
      name: "Start from",
      does:
        "The order to begin with: **Suggested run order** (the one saved for this PBI for every tester, when there is one), **Spec order** (the suite's own order), " +
        "**My order** (one you set before on this computer), or the tester order of a draft file you imported and uploaded.",
    },
    {
      id: "group-cases",
      shot: ORDER,
      locate: { role: "combobox", name: "Group cases" },
      name: "Group cases",
      does:
        "How the list is grouped: **Don't group**, **By title** (cases whose titles start the same way), or **By area** when the suggested run order gives the cases areas.",
    },
    {
      id: "order-note",
      shot: ORDER,
      locate: { text: "No suggested run order yet." },
      name: "Suggested run order note",
      does: "Who saved the suggested run order and when, or that there is none yet. If it cannot be read, this line says why.",
    },
    {
      id: "order-count",
      shot: ORDER,
      locate: { text: "5 test cases" },
      name: "Cases and selection",
      does:
        "How many cases there are, or how many are selected. **Select all** beside it selects every case, and **Clear** drops the selection.",
    },
    {
      id: "order-row",
      shot: ORDER,
      locate: { text: "Login - valid credentials" },
      name: "Case",
      does:
        "Drag a case to where it should run. Click to select it, [[Ctrl]]+click to add more, [[Shift]]+click for a range; dragging a selected case moves the whole selection with it.",
    },
    {
      id: "order-move",
      shot: ORDER,
      locate: { role: "button", name: "Move #5001 down" },
      name: "Move up / down",
      does: "Moves the case (or the selection it belongs to) one place up or down.",
    },
    {
      id: "order-cancel",
      shot: ORDER,
      locate: { role: "button", name: "Cancel" },
      name: "Cancel",
      does: "Closes the window. The list keeps the order and grouping it had.",
    },
    {
      id: "save-for-everyone",
      shot: ORDER,
      locate: { role: "button", name: "Save for everyone" },
      name: "Save for everyone",
      does:
        "Saves this order with the PBI in Azure DevOps as its suggested run order, the one every tester starts from. It asks you to confirm first. " +
        "It is greyed out while the list is the saved suggested order, unchanged.",
    },
    {
      id: "use-this-order",
      shot: ORDER,
      locate: { role: "button", name: "Use this order" },
      name: "Use this order",
      does:
        "Uses the order and grouping on this computer only. An order you changed is kept as **My order**, and an open runner re-sorts the cases after the one on screen to match.",
    },
    {
      id: "order-save-message",
      shot: ORDER_SAVE,
      locate: { text: "Every tester will see this as the suggested run order for this PBI." },
      name: "Confirm",
      does: "Appears after **Save for everyone**, so the order is not shared by accident.",
    },
    {
      id: "order-save-cancel",
      shot: ORDER_SAVE,
      locate: { role: "button", name: "Cancel" },
      name: "Cancel",
      does: "Goes back without saving.",
    },
    {
      id: "order-save",
      shot: ORDER_SAVE,
      locate: { role: "button", name: "Save" },
      name: "Save",
      does: "Saves the suggested run order and switches the list to it.",
    },

    // --- The runner ---------------------------------------------------------
    {
      id: "runner-case-id",
      shot: RUNNER,
      locate: { text: "#5001" },
      name: "Case id",
      does: "The id of the case on screen, the number to quote in a bug.",
    },
    {
      id: "runner-selected",
      shot: RUNNER,
      locate: { text: "2 selected" },
      name: "Selected",
      does: "How many cases this run was opened with.",
    },
    {
      id: "runner-position",
      shot: RUNNER,
      locate: { text: "1/2" },
      name: "Position",
      does: "Which case you are on, out of how many. Hover it to see how many are marked.",
    },
    {
      id: "runner-pin",
      shot: RUNNER,
      locate: { role: "button", nameRe: "^(Unpin|Pin on top)" },
      name: "Pin",
      does: "Keeps the runner on top of other windows, or lets them cover it. The next run opens the way you left it.",
    },
    {
      id: "runner-close",
      shot: RUNNER,
      locate: { role: "button", name: "Close runner" },
      name: "Close (x)",
      does: "Closes the runner. Results already recorded stay recorded; a run that was not finished stays In Progress in Azure DevOps.",
    },
    {
      id: "runner-title",
      shot: RUNNER,
      locate: { text: "Login - valid credentials" },
      name: "Case",
      does: "The case's title, with its last outcome and recent history beside it.",
    },
    {
      id: "runner-preconditions",
      shot: RUNNER,
      locate: { text: "Preconditions" },
      name: "Preconditions",
      does: "What must be true before you start, when the case has any.",
    },
    {
      id: "step-passed",
      shot: RUNNER,
      locate: { role: "button", name: "Mark step 1 passed" },
      name: "P (step passed)",
      does: "Marks this step Passed. Click it again to clear it. Marking steps is optional.",
    },
    {
      id: "step-failed",
      shot: RUNNER,
      locate: { role: "button", name: "Mark step 1 failed" },
      name: "F (step failed)",
      does: "Marks this step Failed, which also offers **File bug**. Click it again to clear it.",
    },
    {
      id: "runner-comment",
      shot: RUNNER,
      locate: { role: "textbox", name: "Comment" },
      name: "Comment",
      does: "A comment for this result. It starts with the last result's comment, if there was one.",
    },
    {
      id: "record",
      shot: RUNNER,
      locate: { role: "button", name: "Record" },
      name: "Record",
      does: "Records your screen: pick a screen or window, then press **Stop recording**. The video is attached to this case's result.",
    },
    {
      id: "snip",
      shot: RUNNER,
      locate: { role: "button", name: "Snip" },
      name: "Snip",
      does:
        "Moves the runner out of the way and opens the Windows snipping tool. The area you capture is attached by itself and the runner comes back. **Cancel snip** stops waiting.",
    },
    {
      id: "paste",
      shot: RUNNER,
      locate: { role: "button", name: "Paste" },
      name: "Paste",
      does: "Attaches the picture on the clipboard. [[Ctrl]]+[[V]] anywhere in the runner does the same.",
    },
    {
      id: "attach-file",
      shot: RUNNER,
      locate: { role: "button", name: "Attach file" },
      name: "Attach file",
      does: "Attaches any file you pick to this case's result.",
      tips: ["Attached pictures show as thumbnails: click one to see it full size, or its x to remove it. Pictures uploaded with the last result show under them."],
    },
    {
      id: "file-bug",
      shot: RUNNER,
      locate: { role: "button", name: "File bug" },
      name: "File bug",
      does: "Opens the bug window, filled in from this case. It shows once the case or one of its steps is marked Failed.",
    },
    {
      id: "passed",
      shot: RUNNER,
      locate: { role: "button", name: "Passed" },
      name: "Passed",
      does: "Marks the case Passed. Every verdict works the same way: click it to mark, click it again to clear.",
      tips: ["The dot on the verdict the case got last time pulses, so marking the same result again is one click."],
    },
    {
      id: "failed",
      shot: RUNNER,
      locate: { role: "button", name: "Failed" },
      name: "Failed",
      does: "Marks the case Failed and offers **File bug**.",
    },
    {
      id: "paused",
      shot: RUNNER,
      locate: { role: "button", name: "Paused" },
      name: "Paused",
      does: "For a case you had to stop half way and mean to come back to.",
    },
    {
      id: "blocked",
      shot: RUNNER,
      locate: { role: "button", name: "Blocked" },
      name: "Blocked",
      does: "For a case that could not be run, for example because something it needs is broken.",
    },
    {
      id: "not-applicable",
      shot: RUNNER,
      locate: { role: "button", name: "Not Applicable" },
      name: "Not Applicable",
      does: "For a case that does not apply to this build.",
    },
    {
      id: "prev",
      shot: RUNNER,
      locate: { role: "button", name: "Prev" },
      name: "Prev",
      does: "Goes back one case, recording the one you leave, as **Next** does.",
    },
    {
      id: "next",
      shot: RUNNER,
      locate: { role: "button", name: "Next" },
      name: "Next",
      does:
        "Goes to the next case and records the one you leave in Azure DevOps straight away: verdict, step marks, comment, attachments and bugs. " +
        "A case you cleared goes back to Active. If recording fails, it is tried again on the next move and at Finish. It is greyed out on the last case, where **Finish** records it.",
    },
    {
      id: "finish",
      shot: RUNNER,
      locate: { role: "button", nameRe: "^Finish \\(\\d+\\)$" },
      name: "Finish (N)",
      does:
        "Records every marked case, completes the run in Azure DevOps and closes the runner. N is how many cases are marked. " +
        "It is greyed out until something is marked, and while you are offline. If any case could not be recorded, the runner stays open and names them.",
    },

    // --- Filing a bug -------------------------------------------------------
    {
      id: "bug-title",
      shot: BUG,
      locate: { role: "textbox", name: "Bug title" },
      name: "Bug title",
      does: "The bug's title, filled in from the case. Change it as you need.",
    },
    {
      id: "bug-repro",
      shot: BUG,
      locate: { role: "textbox", name: "Repro steps" },
      name: "Repro steps",
      does: "The case and its steps, filled in for you. Add what actually happened.",
    },
    {
      id: "bug-links",
      shot: BUG,
      locate: { text: "Links to test case #5001 and PBI #1001. Paste (Ctrl+V) to add more." },
      name: "Links and screenshots",
      does:
        "The bug is linked to the test case and the PBI, and carries the pictures attached to this case in the runner. " +
        "Press [[Ctrl]]+[[V]] in the window to add more. **File bug** creates it in Azure DevOps and notes its number on the case.",
    },
    {
      id: "bug-cancel",
      shot: BUG,
      locate: { role: "button", name: "Cancel" },
      name: "Cancel",
      does: "Closes the window without filing anything.",
    },
  ],
  tips: [
    "A run is opened in Azure DevOps with the first result you record and stays open until **Finish**, so closing the runner never loses a result you moved past.",
    "Offline, the runner keeps your marks and sends them when the connection comes back.",
    "The list colours each row by its last outcome, and the runner's results show here as you record them.",
  ],
  howTo: [
    {
      title: "Run test cases",
      steps: [
        "Choose the PBI in the bar at the top.",
        "Click the cases to run, or press **Select all**.",
        "Press **Run N in runner**.",
        "For each case, follow the steps, mark a verdict, add a comment or a screenshot if needed, and press **Next**.",
        "Press **Finish** when you are done.",
      ],
    },
    {
      title: "Share a run order with the team",
      steps: [
        "Press **Set execution order**.",
        "Pick where to **Start from**, then drag the cases into the order to run them.",
        "Press **Save for everyone** and confirm. Every tester's list now starts from this order.",
      ],
    },
  ],
};
