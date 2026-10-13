// Playwright: the team's Playwright repo, read from a clone on this
// computer. The clone folder and its health line, what the clone holds, the
// Runner, the repo's test accounts, Sync, and the Export to Playwright
// window, which opens here from Auto Run's More menu.
//
// Words come from screens/Playwright/index.tsx, HealthLine.tsx, Runner.tsx,
// RunnerTree.tsx, Accounts.tsx, Sync.tsx, PlaywrightExportDialog.tsx and
// FinishInRepo.tsx.
// The screen has no shots yet: it needs a clone of the repo, which the
// capture's sample data does not have, so it is described in tips and
// how-tos only. Shown only while Enable Advanced Features is on
// (../advanced.ts).

import type { Screen } from "../types";

export const playwright: Screen = {
  id: "playwright",
  title: "Playwright",
  group: "Running tests",
  summary:
    "Work with your clone of the team's Playwright repo without leaving the app: run its tests, manage its test accounts, and keep the repo's copies of your Auto Run scripts up to date. " +
    "The app runs only the repo's own commands, in the clone, and never commits, pulls or pushes. " +
    "Playwright is part of the advanced features: turn on **Enable Advanced Features** in Settings, under General, and it appears in the sidebar under Auto Run.",
  shots: [],
  controls: [],
  tips: [
    "**Clone folder** shows the folder in use. Press **Choose…** (or **Change…** once one is set) and pick your clone of the PHR-PLAYWRIGHT-AUTOMATION repo. It is the same folder Export to Playwright uses, so changing it in one place changes it in both. **Refresh** reads the clone again.",
    "The health line under the folder reads **The clone is ready.** when nothing is wrong. Otherwise it says what stops the tab working: the folder is not a clone of the repo; the clone has no node_modules, so run npm install in it; or Node was not found, so install Node.js and restart the app. Until those are fixed the Runner and Accounts are not shown.",
    "A missing qa-options.json is only a warning: runs use the repo's default options. The app reads that file and never writes it.",
    "**In the repo** sums up the clone: its features, specs, test cases, raw specs, accounts and navigation entries, then lists each feature as country, side, module and feature with its specs and test cases.",
    "Only one repo command runs at a time. While a run, a listing or an account change is going, the buttons that would start another are greyed out.",
  ],
  howTo: [
    {
      title: "Pick and run tests",
      steps: [
        "Under **Runner**, the tests are listed the way the repo's own console lists them: country, side, module, feature, spec, then each test. Fold or unfold a branch with the arrow beside it. Each test shows its Azure DevOps case number and, after a run, whether it passed, failed, was flaky or was skipped.",
        "Tick the tests you want, or tick a branch to take everything under it. **Tick all** ticks every test and **Clear** clears them. The ticks are remembered for this clone.",
        "If the specs changed in the repo, press **List again** to read the tests afresh.",
        "Choose how the browser shows: **Browser hidden**, **Browser shown** or **Debug**. Debug pauses at the start of every test, so a debug run of more than one test asks first; press **Run in debug** to go ahead or **Cancel**.",
        "Press **Run picked**, which shows how many are ticked. **Run all** runs every test in the clone, and **Re-run failures** runs again the ones that failed last time.",
        "To check first, press **Preview**: it shows how many tests would run, the run list and the exact command, without running anything.",
      ],
    },
    {
      title: "Watch a run, stop it, and read the report",
      steps: [
        "Before a run starts, the app counts what Playwright would run. If that count does not match the tests you picked, the run does not start, and the message gives both numbers. Use **List again**, then run again.",
        "The output appears in the panel under the buttons as it happens. **Stop** ends the run and every process it started.",
        "When the run ends, the line above the output says whether it passed, ended with failures or was stopped, and the run's report opens in your browser.",
        "Under **Reports**, every past run is listed by date with its passed and failed counts. Press **Open** beside one to read its report again.",
      ],
    },
    {
      title: "Manage the repo's test accounts",
      steps: [
        "Under **Accounts**, each account shows its key, how many specs and other files use it, and its username, display name, employee number, title and supervisor. **List again** reads them afresh.",
        "**Show password** shows that account's password; **Hide password** hides it again. Passwords are hidden until you ask, and hidden again after any change.",
        "Press **Add account** and fill in its key, username, password and the rest, or press **Edit**, **Rename** or **Delete** on an account. Edit changes one field at a time: pick the field and type the new value, or leave it empty to clear an optional field.",
        "Press **Preview** to see what the repo's npm run users command will change. Nothing is written yet. Press **Apply** to make the change, or **Cancel**. Editing the form again drops the preview, so preview again before you apply.",
        "The repo's own checks apply: a rename rewrites every spec and test case that names the account, an account still in use cannot be deleted, and a changed username or password clears that account's saved session. When the repo refuses, its own message is shown.",
        "While an add or a password change runs, the password is visible in the Windows process list for a few seconds, because the repo's command takes it on its command line. It is never kept in a file or in the app's log.",
      ],
    },
    {
      title: "Keep the repo's copies of your scripts current",
      steps: [
        "Under **Sync**, every case of the project with an Auto Run script is listed with its state. **Current**: the script has not changed since it was exported. **Out of date**: the script changed after the export, and the line says what changed. **Not in the repo**: the clone no longer has its section or raw spec. **Never exported**: it has not been exported yet. **In the repo (not from this app)**: the repo already has it, but this app did not export it, for example because someone wrote it by hand. **Excluded in the repo**: the team left it out of the repo on purpose, so nothing here ever writes it.",
        "Press **Update out of date** to export every out-of-date case again, or tick cases and press **Update ticked**. **Refresh** reads the states again.",
        "Press **Add new** to export every case with a script that the repo does not have yet. A case counts as in the repo when a test-case file has its heading, the generated index lists it, or a spec has its ADO marker. A new case that cannot be exported yet is left out, and the summary lists it with why.",
        "Update out of date and Add new never touch a case that is in the repo but not from this app. To replace one on purpose, tick it and press **Update ticked**.",
        "Update replaces each case's raw spec and rewrites its section in the test-case file from Azure DevOps. The repo's Added by engineer, Also and Assertion floor blocks stay as they are, and so does every other section and file.",
        "Each updated case is listed with its file. An updated raw spec still needs /gen-test steps 5, 6 and 7 in the repo to become a finished spec: **Finish in the repo**, below the list, gives the prompt for that.",
      ],
    },
    {
      title: "Export scripts to the repo",
      steps: [
        "Export to Playwright now lives in this tab. On **Auto Run**, pick the Product Backlog Item, press **More** and choose **Export to Playwright**: the Playwright tab opens with the export window over it.",
        "Check the clone folder, then where each area's specs go (side, module and feature) and which repo user each of your accounts signs in as. Press **Save mappings** when you change them.",
        "Under **Cases**, each case says **New** or **In the repo**, with where the repo has it. Cases already in the repo are skipped. Tick one to replace it. A case the repo marks as excluded cannot be ticked.",
        "Untick any new case you do not want. A case that cannot go says why, with a command to copy when the repo is missing its user.",
        "Press **Export**. The summary lists the files written and the steps still to do in the repo. Nothing is run, committed or pushed.",
      ],
    },
    {
      title: "Finish exported cases in the repo",
      steps: [
        "After an export, an Update or Add new, **Finish in the repo** shows a ready-made prompt for each feature folder the cases went to. It asks Claude Code to run steps 5, 6 and 7 of the repo's /gen-test on exactly those cases: the refactor, the lint and live check, and the run summary. It tells Claude Code not to fetch the tickets again or regenerate the raw specs.",
        "Pick where the cases go: add them to one of the feature's specs, one spec per case, or **New spec…** and type a name in lower-case letters, digits and hyphens. When the feature has exactly one spec, that one is picked for you.",
        "Press **Copy**, open Claude Code in your clone and paste the prompt. The app only copies the text: it does not start Claude Code or anything else.",
      ],
    },
  ],
};
