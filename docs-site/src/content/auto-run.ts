// Auto Run: the app drives a real browser through a test case's steps from
// a script, with a person watching (one case or a selection) or unattended,
// and keeps the results on this computer until they are reviewed and sent.
// Two tabs, Test cases and Past runs, with the Setup panel beside the
// cases. A case's Script and Run buttons are on its card once it is open.
//
// Locates come from screens/AutoRun/index.tsx, ScriptEditor.tsx,
// RunPane.tsx, ReplayPane.tsx, PastRuns.tsx, RunReview.tsx,
// SiteAddressDialog.tsx, AccountsDialog.tsx, ReadinessStrip.tsx and
// SuspectedDefectMark.tsx. The sample data (src/dev/demo.ts) gives the
// first Product Backlog Item's cases scripts: #5001 has a setup that changed
// since it was approved, #5002 carries a suspected defect, #5003 a
// precondition, #5004 is marked Must not save, and #5005 has no script. One unattended run holds a case of each result, one of them
// retried. No route sends anything: every shot stops before Save, Start or
// Send.

import type { Screen, Step } from "../types";

const CASES = "auto-run-cases";
const SELECTED = "auto-run-selected";
const SCRIPT = "auto-run-script";
const SCRIPT_JSON = "auto-run-script-json";
const SCRIPT_SETUP = "auto-run-script-setup";
const WATCH = "auto-run-watch";
const UNATTENDED = "auto-run-unattended";
const RUNS = "auto-run-past-runs";
const REVIEW = "auto-run-review";
const SETUP = "auto-run-setup";
const SITE = "auto-run-site-address";
const ACCOUNTS = "auto-run-accounts";

const NAV: Step = { nav: "Auto Run" };

/** Said under both ways of running: a setup that cannot start blocks the case. */
const BLOCKED_BY_SETUP =
  "A case whose script has a setup is **Blocked** before it signs in when the setup is not approved, or when the fixture it needs has never been built. Approve it in the script editor, or build the fixture on the API Templates tab, under **Fixtures**.";
const READY: Step = { waitFor: { role: "checkbox", name: "Select #5001" } };
/** Opens #5003's script, which opens on the script in sentences. */
const TO_SCRIPT: Step[] = [
  NAV,
  READY,
  { click: { role: "button", name: "Show details for #5003" } },
  { waitFor: { role: "button", name: "Hide details for #5003" } },
  { click: { role: "button", name: "Edit script for #5003" } },
  { waitFor: { role: "region", name: "Step 1" } },
];
/** Opens a case's card, where its Script and Run buttons are. */
const OPEN_CARD = (id: number): Step[] => [
  { click: { role: "button", name: `Show details for #${id}` } },
  { waitFor: { role: "button", name: `Hide details for #${id}` } },
];
const PICK_TWO: Step[] = [
  { click: { role: "checkbox", name: "Select #5001" } },
  { click: { role: "checkbox", name: "Select #5004" } },
];
const TO_RUNS: Step[] = [NAV, READY, { click: { role: "tab", nameRe: "^Past runs" } }, { waitFor: { role: "button", name: "Clear results" } }];
const TO_SETUP: Step[] = [
  NAV,
  READY,
  { click: { role: "button", name: "Show setup details" } },
  { waitFor: { role: "button", name: "Edit site address" } },
];

export const autoRun: Screen = {
  id: "auto-run",
  title: "Auto Run",
  group: "Running tests",
  summary:
    "Let the app run a test case for you. A script tells it what to do at each step, and the app does it in a real browser: with you watching and deciding the result, or unattended while you work on something else. " +
    "Results stay on this computer until you review a run and send it to Azure DevOps. " +
    "Auto Run is part of the advanced features: turn on **Enable Advanced Features** in Settings, under General, and it appears in the sidebar under Run Tests.",
  shots: [
    {
      id: CASES,
      route: [NAV, READY, ...OPEN_CARD(5002), ...OPEN_CARD(5005), { waitFor: { role: "button", name: "Run #5002" } }],
      alt: "Auto Run on its Test cases tab: the search, the result filters, the cases with their last results, two of them open, and the Setup panel beside them",
    },
    { id: SELECTED, route: [NAV, READY, ...PICK_TWO], alt: "Two scripted cases selected, with the buttons that run them" },
    {
      id: SCRIPT,
      route: TO_SCRIPT,
      alt: "The script of a case with a precondition, in plain sentences beside the case's own steps",
    },
    {
      id: SCRIPT_JSON,
      route: [
        ...TO_SCRIPT,
        { click: { role: "button", name: "Edit script" } },
        { waitFor: { role: "textbox", name: "Action script JSON" } },
      ],
      alt: "The same script as JSON, after Edit script",
    },
    {
      id: SCRIPT_SETUP,
      route: [
        NAV,
        READY,
        ...OPEN_CARD(5001),
        { click: { role: "button", name: "Edit script for #5001" } },
        { waitFor: { role: "button", name: "Approve setup" } },
        { scrollTo: { role: "button", name: "Approve setup" } },
      ],
      alt: "The script of a case with a setup: the fixture it builds its data from, changed since it was approved",
    },
    {
      id: WATCH,
      route: [
        NAV,
        READY,
        ...OPEN_CARD(5004),
        { click: { role: "button", name: "Run #5004" } },
        { click: { role: "button", name: "Open browser" } },
        { waitFor: { role: "button", name: "Sign in again" } },
        { click: { role: "button", name: "Run step 1" } },
        { waitFor: { text: 'Check the "Dashboard" heading is showing' } },
        { click: { role: "button", name: "Passed" } },
      ],
      alt: "Running one case while you watch: signed in, the first step run, and Passed chosen",
    },
    {
      id: UNATTENDED,
      route: [NAV, READY, ...PICK_TWO, { click: { role: "button", name: "Run 2 unattended" } }, { waitFor: { role: "button", name: "Start" } }],
      alt: "The Unattended run window, before it starts",
    },
    { id: RUNS, route: TO_RUNS, alt: "Past runs: an unattended run with a passed, a failed, a blocked and a retried case" },
    {
      id: REVIEW,
      route: [...TO_RUNS, { click: { role: "button", name: "Review" } }, { waitFor: { role: "button", name: "Accept all" } }],
      alt: "Reviewing an unattended run before sending it to Azure DevOps",
    },
    { id: SETUP, route: TO_SETUP, alt: "The Setup panel opened beside the cases: site address, sign-in, accounts, areas, test files and save words" },
    {
      id: SITE,
      route: [...TO_SETUP, { click: { role: "button", name: "Edit site address" } }, { waitFor: { role: "textbox", name: "Start address" } }],
      alt: "The Site address window",
    },
    {
      id: ACCOUNTS,
      route: [...TO_SETUP, { click: { role: "button", name: "Edit accounts" } }, { waitFor: { role: "button", name: "Save accounts" } }],
      alt: "The Accounts window, with a login the assistant proposed",
    },
  ],
  groups: [
    { id: "cases", title: "Choose what to run", summary: "The Product Backlog Item's test cases, which of them have a script, and how each one did last time." },
    { id: "scripts", title: "Write a script", summary: "What the app does at each step of a case, and the rules the case runs under." },
    { id: "watch", title: "Run while you watch", summary: "The app does the steps; you see them happen and decide the result." },
    { id: "unattended", title: "Run unattended", summary: "The app runs the selection by itself and proposes a result for each case, while you carry on with other work if you like." },
    { id: "runs", title: "Past runs and reviews", summary: "What came of each run, and sending the results you confirm to Azure DevOps." },
    { id: "setup", title: "Set up", summary: "The site, the sign-in, your test accounts, the areas of the site, and the files scripts upload." },
  ],
  controls: [
    // --- Test cases ------------------------------------------------------------
    {
      id: "tab-cases",
      shot: CASES,
      group: "cases",
      locate: { role: "tab", nameRe: "^Test cases" },
      name: "Test cases",
      does: "The Product Backlog Item's test cases, with how many there are, and the Setup panel beside them. Auto Run always opens here.",
    },
    {
      id: "tab-runs",
      shot: CASES,
      group: "runs",
      locate: { role: "tab", nameRe: "^Past runs" },
      name: "Past runs",
      does: "Every run saved on this computer, with how many there are.",
    },
    {
      id: "setup-summary",
      shot: CASES,
      group: "setup",
      locate: { role: "list", name: "Setup summary" },
      name: "Setup",
      does:
        "What a run needs before it can start, one line each: the site address, the sign-in, the accounts, the areas, what discovery has mapped, the test files and the database. " +
        "A green dot means that part is ready, a red one that it is missing, and an amber one that it is worth a look. " +
        "The panel opens by itself when something a run needs is missing, and says **Needs attention**.",
    },
    {
      id: "setup-details",
      shot: CASES,
      group: "setup",
      locate: { role: "button", name: "Show setup details" },
      name: "Show setup details",
      does: "Opens the panel's full rows, each with the button that changes it. **Hide setup details** goes back to the summary.",
    },
    {
      id: "readiness",
      shot: CASES,
      group: "cases",
      locate: { role: "group", name: "Readiness" },
      name: "Readiness",
      does:
        "One line on where runs go: the environment and its site. When something a run needs is missing, such as the sign-in, the accounts or a test file a script uploads, a warning names it here, even with the Setup panel shut. " +
        "What is in place is shown in the Setup panel, not repeated here.",
    },
    {
      id: "search",
      shot: CASES,
      group: "cases",
      locate: { role: "textbox", name: "Search test cases" },
      name: "Search",
      does:
        "Shows only the cases whose id or title holds what you type, with or without the **#** before an id. It works together with the result filters. " +
        "Press [[Escape]] or the clear button to show every case again.",
    },
    {
      id: "expand-all",
      shot: SELECTED,
      group: "cases",
      locate: { role: "button", name: "Expand all" },
      name: "Expand all",
      does:
        "Stuck at the bottom left of the window, so it is there however far you scroll. While no case is open it reads **Expand all** and opens every case the list shows, to read their scripts at once.",
    },
    {
      id: "collapse-all",
      shot: CASES,
      group: "cases",
      locate: { role: "button", name: "Collapse all" },
      name: "Collapse all",
      does: "The same button once a case is open: it reads **Collapse all** and closes every open case, the ones the search or a filter hides too.",
    },
    {
      id: "group-by-title",
      shot: CASES,
      group: "cases",
      locate: { role: "checkbox", name: "Group by title" },
      name: "Group by title",
      does: "Groups the cases whose titles start the same way, as the other test case screens do. Each group can be folded and selected as a whole.",
    },
    {
      id: "select-all-shown",
      shot: CASES,
      group: "cases",
      locate: { role: "checkbox", name: "Select all shown" },
      name: "Select all shown",
      does: "Selects every case the list shows, after the search and the filters, that has a script. Cases without a script cannot be selected.",
    },
    {
      id: "last-result-filter",
      shot: CASES,
      group: "cases",
      locate: { role: "group", name: "Filter by last result" },
      name: "Last result",
      does:
        "Each case shows its last result beside its title: how it did in the newest run that reached it. " +
        "These buttons show only the cases whose last result was **Passed**, **Failed**, **Blocked** or **Not run**, with how many there are of each. Press more than one to see them together, and press one again to drop it.",
      tips: ["To run the failed cases again: press **Failed**, then **Select all shown**."],
    },
    {
      id: "more",
      shot: CASES,
      group: "scripts",
      locate: { role: "button", name: "More" },
      name: "More",
      does:
        "**Import scripts** reads one JSON file that can carry the scripts of every case in the Product Backlog Item, the way an assistant writes them. " +
        "**Clear scripts** removes the scripts of the listed cases from this computer, after asking. Nothing in Azure DevOps changes.",
    },
    {
      id: "select-case",
      shot: CASES,
      group: "cases",
      locate: { role: "checkbox", name: "Select #5001" },
      name: "Select",
      does: "Adds the case to the selection. Only a case with a script has this box.",
    },
    {
      id: "case-details",
      shot: CASES,
      group: "cases",
      locate: { role: "button", name: "Hide details for #5002" },
      name: "Show details",
      does:
        "Opens the case, as a click on its title does. Closed, a case shows only its id, its title and its last result. " +
        "Open, it shows what its script does: how many steps, the account it signs in as, its area, its rules, its setup and whether it is approved, and its last repair, then its steps, the files it uploads and checks, and the last run's downloads, each with **Open**. " +
        "The case's buttons are at the bottom. The cases you open stay open while you stay on Auto Run.",
    },
    {
      id: "card-steps",
      shot: CASES,
      group: "cases",
      locate: { role: "list", name: "Script steps of #5002" },
      name: "Steps",
      does: "The script's steps, numbered, each with the case's own words for it and how many actions it takes. Read only: change them with **Script**.",
    },
    {
      id: "suspected-defect",
      shot: CASES,
      group: "cases",
      locate: { text: "Suspected defect" },
      name: "Suspected defect",
      does:
        "An assistant looked at a failure and found the script was right and the site did not do what the case expects. The open case says at which step and why. " +
        "Worth checking by hand, and worth a bug if it holds.",
    },
    {
      id: "clear-suspected-defect",
      shot: CASES,
      group: "cases",
      locate: { role: "button", name: "Clear suspected defect for #5002" },
      name: "Clear",
      does: "Removes the suspected defect mark once you have looked into it. It asks first; the script itself does not change.",
    },
    {
      id: "edit-script",
      shot: CASES,
      group: "scripts",
      locate: { role: "button", name: "Edit script for #5002" },
      name: "Script",
      does: "Opens the case's script, in plain sentences. On a case without one, the button reads **Add script**.",
    },
    {
      id: "add-script",
      shot: CASES,
      group: "scripts",
      locate: { role: "button", name: "Add script for #5005" },
      name: "Add script",
      does: "Writes a script for a case that has none yet. Until it has one, the case cannot be run or selected.",
    },
    {
      id: "run-one",
      shot: CASES,
      group: "watch",
      locate: { role: "button", name: "Run #5002" },
      name: "Run",
      does: "Runs this one case while you watch. Only a case with a script has it.",
      tips: [BLOCKED_BY_SETUP],
    },

    // --- A selection -------------------------------------------------------------
    {
      id: "run-selected",
      shot: SELECTED,
      group: "watch",
      locate: { role: "button", nameRe: "^Run \\d+ selected$" },
      name: "Run N selected",
      does:
        "Runs the selected cases one after another while you watch, in the order the list shows them. Each case starts in a fresh browser, so one case never passes only because the one before it signed in. " +
        "When the list scrolls away, these buttons float at the bottom right.",
    },
    {
      id: "run-unattended",
      shot: SELECTED,
      group: "unattended",
      locate: { role: "button", nameRe: "^Run \\d+ unattended$" },
      name: "Run N unattended",
      does: "Opens the Unattended run window for the selected cases.",
      tips: [BLOCKED_BY_SETUP],
    },
    {
      id: "clear-selection",
      shot: SELECTED,
      group: "cases",
      locate: { role: "button", name: "Clear selection" },
      name: "Clear selection (x)",
      does: "Clears the selection.",
    },

    // --- The script --------------------------------------------------------------
    {
      id: "case-steps",
      shot: SCRIPT,
      group: "scripts",
      locate: { text: "The case's steps" },
      name: "The case's steps",
      does: "The test case's own steps and expected results, to write the script against. They are read from Azure DevOps and do not change here.",
    },
    {
      id: "runs-as",
      shot: SCRIPT,
      group: "scripts",
      locate: { role: "combobox", name: "Runs as" },
      name: "Runs as",
      does:
        "The account the case signs in as before step 1, picked from your own accounts. A script names an account by its key, so the same script works for every tester with their own login. " +
        "**No sign-in** starts the case signed out.",
    },
    {
      id: "area",
      shot: SCRIPT,
      group: "scripts",
      locate: { role: "combobox", name: "Area" },
      name: "Area",
      does: "The recorded area of the site the case starts in. Left on **the case's Module**, it starts in the area named like the case's Module.",
    },
    {
      id: "must-not-save",
      shot: SCRIPT,
      group: "scripts",
      locate: { role: "checkbox", name: "Must not save" },
      name: "Must not save",
      does:
        "For a case that works on shared data and must never change it. While the case runs, any save the page tries to send is stopped before it reaches the site, and the case fails. " +
        "The save words in the Setup panel decide what counts as a save. Only you can turn it off, here.",
    },
    {
      id: "readable-script",
      shot: SCRIPT,
      group: "scripts",
      locate: { role: "region", name: "Action script" },
      name: "Action script",
      does:
        "What the app does at each step, in plain sentences under each of the case's steps: open a page, click, type, upload a test file, and check what the page shows. " +
        "A step whose expected result is not checked says why. A password is never shown: it reads **the account's password**. " +
        "Most scripts are written by an assistant from the case's steps and imported with **More**, **Import scripts**.",
      tips: [
        "An element the script finds only by its code shows that code in a smaller type. Point at it to see all of it.",
        "Tabs: a script can wait for a link to open a new tab, open one itself, switch between tabs and close one. " +
          "The sentences read, for example, **Wait for a new tab and call it \"report\"** and **Close the \"report\" tab**. " +
          "A step that ran outside the first tab says **in tab** and its name, here and in a saved run.",
        "Keys: a script can press a key with Ctrl, Shift, Alt or Meta held, and press it more than once. It reads, for example, **Press Ctrl+ArrowUp 3 times**.",
        "Drag: a script can drag one item before, after or onto another, for example **Drag the \"Grade C\" row before the \"Grade A\" row**. It is followed by a check of the new order.",
        "Dialogs: a script can check what a browser dialog says, then press OK or Cancel, or type into it first: **Expect a dialog containing \"saved\" and press OK**. A dialog the script does not expect is accepted, and the step says so.",
        "Tables: a script can check that a table or grid has a row, has no such row, is sorted by a column, or has a number of rows: **Check the \"Employees\" table has a row with Status \"Active\"**. Only the rows on screen are read, so filter first.",
        "Page errors: a script can fail a step when the page has a script error or a server error, or only count them. A case that counted some shows **page errors seen** and the number.",
        "PDF downloads: a download check can look for text in a PDF, its number of pages, or text on one page, for example **with the text \"Total\", 2 pages**. A PDF that is locked, scanned or damaged fails the check.",
      ],
    },
    {
      id: "show-json",
      shot: SCRIPT,
      group: "scripts",
      locate: { role: "button", name: "Edit script" },
      name: "Edit script",
      does: "Shows the script as JSON in the same place, with every detail, to change it. The window always opens on the sentences.",
    },
    {
      id: "script-json",
      shot: SCRIPT_JSON,
      group: "scripts",
      locate: { role: "textbox", name: "Action script JSON" },
      name: "Action script JSON",
      does:
        "The script as JSON, the way an assistant writes it. **Save script** saves what is here, from this view or from the sentences.",
    },
    {
      id: "back-to-readable",
      shot: SCRIPT_JSON,
      group: "scripts",
      locate: { role: "button", name: "Back to readable view" },
      name: "Back to readable view",
      does: "Shows the script in sentences again, your changes included. While the JSON is not valid it says what is wrong and stays on the JSON.",
    },
    {
      id: "checks",
      shot: SCRIPT,
      group: "scripts",
      locate: { text: "Checks" },
      name: "Checks",
      does:
        "Whether the script checks each step's expected result, updated as you type. A step whose expected result is not checked says why, or reads **NOT CHECKED**.",
    },
    {
      id: "preconditions",
      shot: SCRIPT,
      group: "scripts",
      locate: { role: "list", name: "Preconditions" },
      name: "Preconditions",
      does:
        "Records the case relies on, each one a stage of a flow on the API Templates tab that must be done for a value. The run checks them before it signs in, and a case whose precondition is not met is **Blocked** and runs no step. " +
        "Preconditions are checked in the company database, so they are skipped, and the case says **Not checked**, while Database Read Access is off.",
    },
    {
      id: "remove-precondition",
      shot: SCRIPT,
      group: "scripts",
      locate: { role: "button", name: "Remove precondition 1" },
      name: "Remove",
      does: "Takes the precondition off the script when you press **Save script**. Preconditions are added by an assistant, not here.",
    },
    {
      id: "script-setup",
      shot: SCRIPT_SETUP,
      group: "scripts",
      locate: { role: "list", name: "Setup steps" },
      name: "Setup",
      does:
        "The fixture the case builds its data from before it signs in: its name, the account it runs as, its steps and what it creates. " +
        "Your assistant writes the setup into the script; you read it here and approve it. Saving the script keeps the setup as the assistant wrote it.",
    },
    {
      id: "setup-changed",
      shot: SCRIPT_SETUP,
      group: "scripts",
      locate: { text: "Changed since you approved it" },
      name: "Changed since you approved it",
      does: "The setup was approved once and has changed since, so it needs approving again before the case can run.",
    },
    {
      id: "approve-setup",
      shot: SCRIPT_SETUP,
      group: "scripts",
      locate: { role: "button", name: "Approve setup" },
      name: "Approve setup",
      does:
        "Approves exactly the setup on screen; if it changed while you were looking, it asks you to review it again. Once approved, the line reads **Approved** and the date, " +
        "with **Withdraw approval** beside it to take the approval back. Only you can approve a setup: your assistant cannot.",
    },
    {
      id: "save-script",
      shot: SCRIPT,
      group: "scripts",
      locate: { role: "button", name: "Save script" },
      name: "Save script",
      does: "Saves the script on this computer. Scripts never go to Azure DevOps.",
    },

    // --- Watching a run ------------------------------------------------------------
    {
      id: "signed-in",
      shot: WATCH,
      group: "watch",
      locate: { text: "Signed in as portal.tester" },
      name: "Sign-in",
      does:
        "The account the case signed in as, once the browser opened. A case with preconditions has them checked first; one that is not met says **Blocked before step 1** and why.",
    },
    {
      id: "sign-in-again",
      shot: WATCH,
      group: "watch",
      locate: { role: "button", name: "Sign in again" },
      name: "Sign in again",
      does: "Forgets the saved sign-in for this account and signs in afresh.",
    },
    {
      id: "run-step",
      shot: WATCH,
      group: "watch",
      locate: { role: "button", name: "Run step 1" },
      name: "Run step N",
      does:
        "Does that step's actions in the browser and lists what each one did. A failed action is shown in red, with **Screenshot** when the browser took one. Run the steps in order, and look at the browser as they run.",
    },
    {
      id: "your-verdict",
      shot: WATCH,
      group: "watch",
      locate: { role: "group", name: "Your verdict" },
      name: "Your verdict",
      does:
        "**Passed**, **Failed** or **Blocked**: the result is yours to decide. Nothing is chosen for you, because a green step can still hide a problem you saw, and a red one can be the script's fault rather than the site's.",
    },
    {
      id: "result-note",
      shot: WATCH,
      group: "watch",
      locate: { role: "textbox", name: "Result note" },
      name: "Result note",
      does: "What you saw, if you want to say. It is saved with the result.",
    },
    {
      id: "save-result",
      shot: WATCH,
      group: "watch",
      locate: { role: "button", name: "Save result" },
      name: "Save result",
      does:
        "Saves the result on this computer and closes the browser. Running a selection, it reads **Save and next case** until the last case, and the whole selection is saved as one run.",
    },
    {
      id: "close-run",
      shot: WATCH,
      group: "watch",
      locate: { role: "button", name: "Close" },
      name: "Close",
      does: "Stops here and closes the browser. Results you already chose are kept.",
    },

    // --- Unattended ----------------------------------------------------------------
    {
      id: "sign-in-as",
      shot: UNATTENDED,
      group: "unattended",
      locate: { role: "combobox", name: "Sign in as" },
      name: "Sign in as",
      does:
        "**Each script's own account** signs every case in as its script says. Pick one of your accounts instead, and that account signs in every case, over the account a script names.",
    },
    {
      id: "browser",
      shot: UNATTENDED,
      group: "unattended",
      locate: { role: "combobox", name: "Browser to run in" },
      name: "Browser",
      does: "Microsoft Edge or Google Chrome. The app remembers your choice.",
    },
    {
      id: "watch-browser",
      shot: UNATTENDED,
      group: "unattended",
      locate: { role: "checkbox", name: "Watch the browser" },
      name: "Watch the browser",
      does: "Off: the browser runs out of sight and you can keep working. On: a window opens for every case.",
    },
    {
      id: "retry-transient",
      shot: UNATTENDED,
      group: "unattended",
      locate: { role: "checkbox", name: "Retry transient failures once" },
      name: "Retry transient failures once",
      does:
        "A case that fails on something passing, such as a gateway error, a dropped connection or a browser that stops answering, runs once more. Its result is labelled **Retried**.",
    },
    {
      id: "start",
      shot: UNATTENDED,
      group: "unattended",
      locate: { role: "button", name: "Start" },
      name: "Start",
      does:
        "Starts the run. The window follows it case by case, and each case can be opened to see its steps as they go; **Stop** ends the run after the step it is on. " +
        "**Run in background** (or Escape) closes the window and the run carries on: a pill in the window's title bar shows how far it has got, such as **Auto Run 3 of 8**, and clicking it opens the window again from any part of the app. " +
        "At a reset point the pill turns amber and reads **Reset needed**; click it to Continue or Stop. " +
        "When the run finishes with its window open, its review opens. Finished in the background, a message and the pill say **Run finished, Review**: click either to open the review on **Past runs**. " +
        "While a run is going, the other Run buttons wait for it. " +
        "While your assistant's discovery is using the Auto Run browser and no run is going, the pill reads **Discovering**, and no run can start until **End discovery** (or the assistant) ends it.",
    },

    // --- Past runs -----------------------------------------------------------------
    {
      id: "clear-results",
      shot: RUNS,
      group: "runs",
      locate: { role: "button", name: "Clear results" },
      name: "Clear results",
      does: "Removes every run and screenshot saved on this computer, after asking. Runs already sent stay in Azure DevOps.",
    },
    {
      id: "runs-filter",
      shot: RUNS,
      group: "runs",
      locate: { role: "group", name: "Filter by result" },
      name: "Filter by result",
      does: "Shows only the runs with a case of that result, and in each of them only those cases. The numbers count runs.",
    },
    {
      id: "run-kind",
      shot: RUNS,
      group: "runs",
      locate: { text: "unattended" },
      name: "Kind of run",
      does: "**supervised** for a run you watched, **unattended** for one the app ran by itself, then the environment it ran in.",
    },
    {
      id: "to-review",
      shot: RUNS,
      group: "runs",
      locate: { text: "3 to review" },
      name: "To review",
      does: "How many cases of an unattended run still need your verdict. A run already sent says **Sent** instead.",
    },
    {
      id: "review",
      shot: RUNS,
      group: "runs",
      locate: { role: "button", name: "Review" },
      name: "Review",
      does: "Opens the run's review, where you confirm each case's result and send the run to Azure DevOps. A supervised run needs no review: you decided each result as it ran.",
    },
    {
      id: "report",
      shot: RUNS,
      group: "runs",
      locate: { role: "button", nameRe: "^Open a report of the run from" },
      name: "Report",
      does: "Opens a report of the run in your browser: each case, its steps, what failed and the screenshots. It changes nothing.",
    },
    {
      id: "run-results",
      shot: RUNS,
      group: "runs",
      locate: { role: "group", name: "Results" },
      name: "Results",
      does: "How many cases of the run passed, failed, were blocked or did not run.",
    },
    {
      id: "retried",
      shot: RUNS,
      group: "runs",
      locate: { text: "Retried" },
      name: "Retried",
      does: "The case failed on something passing and was run once more. Hover it to see why the first try failed. A case that says **Not checked** went on without its preconditions being checked.",
    },

    // --- Review --------------------------------------------------------------------
    {
      id: "review-case",
      shot: REVIEW,
      group: "runs",
      locate: { role: "listitem", name: "Case #5002 Login - wrong password shows an error" },
      name: "Case",
      does:
        "One case of the run, with what the app proposes and why: **Proposed: Failed**, and the step that failed. A proposal is never a result until you confirm it. **Show steps** lists what each step did, with any screenshot.",
    },
    {
      id: "review-verdict",
      shot: REVIEW,
      group: "runs",
      locate: { role: "group", name: "Verdict for #5002" },
      name: "Verdict",
      does: "Confirms the case's result. Press the chosen one again to clear it.",
    },
    {
      id: "review-note",
      shot: REVIEW,
      group: "runs",
      locate: { role: "textbox", name: "Note for #5002" },
      name: "Note",
      does: "What you saw, sent with the result.",
    },
    {
      id: "accept-all",
      shot: REVIEW,
      group: "runs",
      locate: { role: "button", name: "Accept all" },
      name: "Accept all",
      does: "Confirms every case's proposed result in one press. Beside it, how many cases are confirmed so far.",
    },
    {
      id: "save-review",
      shot: REVIEW,
      group: "runs",
      locate: { role: "button", name: "Save review" },
      name: "Save review",
      does: "Saves your verdicts and notes on this computer, to finish the review later.",
    },
    {
      id: "send",
      shot: REVIEW,
      group: "runs",
      locate: { role: "button", name: "Send to Azure DevOps" },
      name: "Send to Azure DevOps",
      does:
        "Creates one test run in Azure DevOps for the Product Backlog Item with the results you confirmed. It asks first, and says how many cases are left out. Save the review before sending. " +
        "Nothing in Azure DevOps is deleted or overwritten, and this is the only way a result leaves this computer.",
    },

    // --- Setup ---------------------------------------------------------------------
    {
      id: "site-address-row",
      shot: SETUP,
      group: "setup",
      locate: { role: "group", name: "Site address" },
      name: "Site address",
      does:
        "Where runs go: the address of the site you test, in the active environment. Environments, each with its own site, accounts and saved sign-ins, are chosen on the AI Bridge tab.",
    },
    {
      id: "edit-site-address",
      shot: SETUP,
      group: "setup",
      locate: { role: "button", name: "Edit site address" },
      name: "Edit (site address)",
      does: "Opens the Site address window.",
    },
    {
      id: "record-sign-in",
      shot: SETUP,
      group: "setup",
      locate: { role: "button", name: "Record sign-in" },
      name: "Record",
      does:
        "For a site whose sign-in page the app does not know: a browser opens, you sign in once by hand, and the app saves how to do it. **Built-in** means the app's own sign-in is used.",
    },
    {
      id: "edit-recipe",
      shot: SETUP,
      group: "setup",
      locate: { role: "button", name: "Edit sign-in recipe" },
      name: "Edit (sign-in)",
      does: "The saved sign-in as JSON, for what a recording cannot capture, and the notes kept about how the site behaves.",
    },
    {
      id: "edit-accounts",
      shot: SETUP,
      group: "setup",
      locate: { role: "button", name: "Edit accounts" },
      name: "Edit (accounts)",
      does: "Opens the Accounts window. The row says how many accounts this computer has for the environment.",
    },
    {
      id: "edit-areas",
      shot: SETUP,
      group: "setup",
      locate: { role: "button", name: "Edit areas" },
      name: "Edit (areas)",
      does:
        "The areas of the site a case can start in. Record one by clicking through the site's menu once in a browser the app opens; the app then finds its way there before step 1. **Try** checks a recorded area in a fresh browser.",
    },
    {
      id: "view-discovery",
      shot: SETUP,
      group: "setup",
      locate: { role: "button", name: "View discovery" },
      name: "View (discovery)",
      does:
        "What your assistant has found out about the live site, area by area. Before it writes a script, the assistant opens the Auto Run browser, signs in and looks through the site itself, so a script is built from what is really on the page. " +
        "The **Discovery** window lists each area with when it was explored and as which account, how many pages and elements were seen, and a **Stale** mark when the map is out of date. " +
        "An area goes stale 30 days after it was explored, or when a script failed there. Expand **save requests** to see the requests the site made when it saved. " +
        "**Forget map** (it asks first) clears one area's map; scripts keep running, and new saves there need the area explored again. " +
        "A script save is refused when it points at something the assistant never saw on the live page. The exceptions are a name that contains, as whole words, something the script typed in an earlier step (typing **AutoTest Leave 7** covers a row **AutoTest Leave 7 Pending**, but not a **Leave** button), a check whose text is a whole word or phrase from the case, and a repair, which is held only to the steps it names. " +
        "While discovery runs, **Open browser** and the Run buttons are greyed out, and hovering one shows **Discovery is using the Auto Run browser**. They come back when your assistant ends discovery. " +
        "If your assistant stopped without ending it, **End discovery** appears beside **View** while discovery runs: it closes the assistant's browser, and what was mapped is kept. " +
        "If the map cannot be read, the window offers **Reset map**, which moves the damaged file aside (it is kept, never deleted) and starts an empty map.",
      tips: ["In your assistant, **/tcm:discover** starts this for a case. The row reads **Not explored yet** until an area has been explored."],
    },
    {
      id: "manage-test-files",
      shot: SETUP,
      group: "setup",
      locate: { role: "button", name: "Manage test files" },
      name: "Manage (test files)",
      does: "The documents and pictures scripts and API templates upload, kept on this computer and named in a script by file name. Add, remove or open their folder.",
    },
    {
      id: "edit-save-words",
      shot: SETUP,
      group: "setup",
      locate: { role: "button", name: "Edit save words" },
      name: "Edit (save words)",
      does:
        "The words that mark a button as a save, which **Must not save** stops: the built-in ones, which cannot be removed, and your project's own.",
    },

    // --- Site address --------------------------------------------------------------
    {
      id: "start-address",
      shot: SITE,
      group: "setup",
      locate: { role: "textbox", name: "Start address" },
      name: "Start address",
      does: "The page runs sign in on and start from. Changing it forgets the environment's saved sign-ins, so the next run signs in afresh.",
    },
    {
      id: "also-allowed",
      shot: SITE,
      group: "setup",
      locate: { role: "textbox", name: "Also allowed" },
      name: "Also allowed",
      does: "Other sites a script may open, such as a separate sign-in page, one per line. A script cannot open anything else.",
    },
    {
      id: "save-site",
      shot: SITE,
      group: "setup",
      locate: { role: "button", name: "Save" },
      name: "Save",
      does: "Saves the address for the environment named at the top.",
    },

    // --- Accounts ------------------------------------------------------------------
    {
      id: "account-key",
      shot: ACCOUNTS,
      group: "setup",
      locate: { role: "textbox", name: "Key for account 1" },
      name: "Key",
      does: "The name a script uses for the account. Keep the keys your team's scripts use, and fill in your own login beside them.",
    },
    {
      id: "account-password",
      shot: ACCOUNTS,
      group: "setup",
      // A password field has no role of its own: found by its label.
      locate: { label: "Password for account 1" },
      name: "Password",
      does: "Your password for the test site, kept on this computer only. **Show passwords** below shows what you typed.",
    },
    {
      id: "proposed",
      shot: ACCOUNTS,
      group: "setup",
      locate: { role: "heading", nameRe: "^Proposed by the assistant" },
      name: "Proposed by the assistant",
      does:
        "Logins an assistant found for this environment. Tick the ones you want and press **Add selected**: leave the password empty to use the one the assistant found in the database, or type one. **Dismiss** drops the list.",
    },
    {
      id: "add-account",
      shot: ACCOUNTS,
      group: "setup",
      locate: { role: "button", name: "Add account" },
      name: "Add account",
      does: "Adds an empty row for another account.",
    },
    {
      id: "save-accounts",
      shot: ACCOUNTS,
      group: "setup",
      locate: { role: "button", name: "Save accounts" },
      name: "Save accounts",
      does: "Saves the accounts for the active environment. A changed login forgets that account's saved sign-in.",
    },
  ],
  tips: [
    "Nothing a script or a run does reaches Azure DevOps by itself. Results go there only when you review a run and press **Send to Azure DevOps**.",
    "Every case starts in a fresh browser with a clean profile, so a result never depends on the case before it.",
    "Scripts, runs, accounts and test files are kept on this computer. Each tester keeps their own accounts; scripts name them by key.",
    "An assistant connected on the AI Bridge tab can write scripts for a whole Product Backlog Item, repair one that broke, and walk you through the setup.",
  ],
  howTo: [
    {
      title: "Turn Auto Run on",
      steps: [
        "Open Settings with the gear at the top right.",
        "Under **General**, turn on **Enable Advanced Features**.",
        "**Auto Run** appears in the sidebar under Run Tests, and **API Templates** at the end.",
      ],
    },
    {
      title: "Set up for the first run",
      steps: [
        "Pick the Product Backlog Item and open **Auto Run**. The Setup panel is beside the cases, and opens by itself when something a run needs is missing; otherwise press **Show setup details**.",
        "Press **Edit** beside **Site address**, type the address of the site you test, and save.",
        "Press **Edit** beside **Accounts** and add your test accounts with the keys your team's scripts use, or add the ones the assistant proposed.",
        "Press **Edit** beside **Areas** and record the areas of the site your cases start in.",
        "Ask your assistant to discover the site before it writes scripts. **View** beside **Discovery** shows what it has mapped.",
        "If your scripts upload files, add them with **Manage** beside **Test files**.",
      ],
    },
    {
      title: "Write or import scripts",
      steps: [
        "On **Test cases**, open a case and press **Add script**, write its actions, check that **Checks** covers every step, and press **Save script**.",
        "Or let an assistant write the scripts for the whole Product Backlog Item, then press **More**, **Import scripts** and pick its file.",
        "A script whose case needs a draft of its own has a setup. Open the script, read its **Setup** and press **Approve setup**.",
      ],
    },
    {
      title: "Run one case while you watch",
      steps: [
        "Open the case with the arrow beside it, or by clicking its title, and press **Run**.",
        "Pick the browser and press **Open browser**. The app signs in as the script's account.",
        "Press **Run step 1**, watch the browser, then the next step, and so on.",
        "Choose **Passed**, **Failed** or **Blocked**, add a note if you want, and press **Save result**.",
      ],
    },
    {
      title: "Run a selection unattended",
      steps: [
        "Select the cases, or filter the list and press **Select all shown**.",
        "Press **Run N unattended**.",
        "Choose who to sign in as and whether to watch, then press **Start**.",
        "To carry on with other work, press **Run in background**. The pill in the title bar shows how far the run has got; click it to open the run again.",
        "If the pill turns amber and reads **Reset needed**, click it, put the named data back, and press **Continue**.",
        "When the run finishes its review opens (from the background, click **Review** on the message or the pill): confirm each case, or press **Accept all**, then **Send to Azure DevOps**.",
      ],
    },
    {
      title: "Read past runs",
      steps: [
        "Open **Past runs** and filter by result if you want.",
        "Press **Review** on an unattended run to confirm its results and send them, or **Report** to read the run in your browser.",
      ],
    },
  ],
};
