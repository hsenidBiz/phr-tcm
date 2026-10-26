// Playwright: the team's Playwright repo, read from a clone on this
// computer. The clone folder and its health badges, then four tabs: what the
// clone holds (Overview), the Runner, the repo's test accounts and Sync; and
// the Export to Playwright window, which opens here from Auto Run's More menu.
//
// Words come from screens/Playwright/index.tsx, HealthLine.tsx, Runner.tsx,
// RunnerTree.tsx, FeatureTree.tsx, Accounts.tsx, Sync.tsx, PlaywrightExportDialog.tsx and
// FinishInRepo.tsx.
// The screen has no shots yet: it needs a clone of the repo, which the
// capture's sample data does not have, so it is described in tips and
// how-tos only. Shown only while the Playwright module is on
// (../modules.ts).

import type { Screen } from "../types";

export const playwright: Screen = {
  id: "playwright",
  title: "Playwright",
  group: "Running tests",
  summary:
    "Work with your clone of the team's Playwright repo without leaving the app: run its tests, manage its test accounts, and keep the repo's copies of your Auto Run scripts up to date. " +
    "The app runs only the repo's own commands, in the clone, and never commits, pulls or pushes. " +
    "Playwright is a module: switch it on in Settings, under General, on the **Modules** page (it needs Auto Run), and it appears in the sidebar under Auto Run.",
  shots: [],
  controls: [],
  tips: [
    "The top of the tab shows the clone folder in use. Press **Choose…** (or **Change…** once one is set) and pick your clone of the PHR-PLAYWRIGHT-AUTOMATION repo. It is the same folder Export to Playwright uses, so changing it in one place changes it in both. **Refresh** reads the clone again.",
    "Under the folder, a badge reads **All good** when nothing is wrong. Otherwise each badge names what stops the tab working, and pointing at it says what to do: the folder is not a clone of the repo; the clone has no node_modules, so run npm install in it; or Node was not found, so install Node.js and restart the app. Until those are fixed the Runner and Accounts tabs say why instead of showing their lists.",
    "A missing qa-options.json is only a warning: runs use the repo's default options. The app reads that file and never writes it.",
    "The rest of the tab is split into four tabs: **Overview**, **Runner**, **Accounts** and **Sync**. The tab opens on the one you chose last.",
    "**Overview** sums up the clone: its features, specs, test cases, raw specs, accounts and navigation entries, then a tree shows what is under the repo's suites folder: each folder beneath its parent, at any depth, and each feature beneath its folder with its specs and test cases. A folder shows the totals of everything under it; fold or unfold it with the arrow beside it, and point at a name to see its full path. Right-click a folder (or press Shift+F10 on it) to make a feature folder inside it (with its specs and test-cases folders) or to rename it. To make a folder at the top, directly under suites, right-click the empty space below the tree (or the line that says there are no features yet), or focus it and press Shift+F10; before a rename that would break an import, a path alias, a config path or a navigation entry, the app lists what will break, and it never edits those files for you. common-components cannot be renamed here.",
    "**New feature folder** makes a feature folder in the clone: choose it from the right-click menu, type the name and press [[Enter]]. It gets test-cases and specs folders, each with a .gitkeep so git keeps them when you commit, and it is listed and offered as an export destination straight away. A folder that is already there, or that differs from one there only in capitals, is refused. The export window has a **New folder** button under **Areas** that takes a whole path, such as sl/admin/performance/new-feature.",
    "Only one repo command runs at a time. While a run, a listing or an account change is going, the buttons that would start another are greyed out.",
  ],
  howTo: [
    {
      title: "Pick and run tests",
      steps: [
        "On the **Runner** tab, the tests are listed the way the repo's own console lists them: one level for each folder on the way down (however many the repo has), then the feature, the spec and each test. Fold or unfold a branch with the arrow beside it. Each test shows its Azure DevOps case number and, after a run, whether it passed, failed, was flaky or was skipped.",
        "Tick the tests you want, or tick a branch to take everything under it. **Tick all** ticks every test and **Clear** clears them. The ticks are remembered for this clone.",
        "If the specs changed in the repo, press **Refresh** above the list to read the tests afresh.",
        "Choose how the browser shows: **Browser hidden**, **Browser shown** or **Debug**. Debug pauses at the start of every test, so a debug run of more than one test asks first; press **Run in debug** to go ahead or **Cancel**.",
        "Press **Run picked**, which shows how many are ticked. **Run all** runs every test in the clone, and **Re-run failures** runs again the ones that failed last time.",
        "To check first, press **Preview**: it shows how many tests would run, the run list and the exact command, without running anything.",
      ],
    },
    {
      title: "Watch a run, stop it, and read the report",
      steps: [
        "Before a run starts, the app counts what Playwright would run. If that count does not match the tests you picked, the run does not start, and the message gives both numbers. Use **Refresh** above the list, then run again.",
        "The output appears in the **Output** panel as it happens, and the panel follows the newest line until you scroll up. **Stop**, at the top of the panel, ends the run and every process it started.",
        "When the run ends, the top of the Output panel says whether it passed, ended with failures or was stopped, and the run's report opens in your browser.",
        "Under **Reports**, every past run is listed by date with its passed and failed counts. Press **Open** beside one to read its report again.",
      ],
    },
    {
      title: "Manage the repo's test accounts",
      steps: [
        "On the **Accounts** tab, a table shows each account's key, username, employee number, display name (with its supervisor), title, and how many specs and other files use it. **Refresh** on the tab reads them afresh.",
        "The eye button at the end of an account's row (**Show password**) shows that account's password; press it again (**Hide password**) to hide it. Passwords are hidden until you ask, and hidden again after any change.",
        "Press **Add account** and fill in its key, username, password and the rest, or use the **Edit**, **Rename** or **Delete** button at the end of an account's row. The form opens in a window. Edit changes one field at a time: pick the field and type the new value, or leave it empty to clear an optional field.",
        "Press **Preview** to see what the repo's npm run users command will change. Nothing is written yet. Press **Apply** to make the change, or **Cancel**. Editing the form again drops the preview, so preview again before you apply.",
        "The repo's own checks apply: a rename rewrites every spec and test case that names the account, an account still in use cannot be deleted, and a changed username or password clears that account's saved session. When the repo refuses, its own message is shown.",
        "While an add or a password change runs, the password is visible in the Windows process list for a few seconds, because the repo's command takes it on its command line. It is never kept in a file or in the app's log.",
      ],
    },
    {
      title: "Keep the repo's copies of your scripts current",
      steps: [
        "On the **Sync** tab, every case of the project with an Auto Run script is listed with its state. **Current**: the script has not changed since it was exported. **Out of date**: the script changed after the export, and the line under it says what changed. **Not in the repo**: the clone no longer has its section or raw spec. **Never exported**: it has not been exported yet. **In the repo (not from this app)**: the repo already has it, but this app did not export it, for example because someone wrote it by hand. **Excluded in the repo**: the team left it out of the repo on purpose, so nothing here ever writes it.",
        "Press **Update out of date** to export every out-of-date case again, or tick cases and press **Update ticked**. **Refresh** reads the states again.",
        "Press **Add new** to export every case with a script that the repo does not have yet. A case counts as in the repo when a test-case file has its heading, the generated index lists it, or a spec has its ADO marker. A new case that cannot be exported yet is left out, and the summary lists it with why.",
        "Update out of date and Add new never touch a case that is in the repo but not from this app. To replace one on purpose, tick it and press **Update ticked**.",
        "Update replaces each case's raw spec and rewrites its section in the test-case file from Azure DevOps. The repo's Added by engineer, Also and Assertion floor blocks stay as they are, and so does every other section and file.",
        "Each updated case is listed with its file. An updated raw spec still needs /gen-test steps 4, 5, 6 and 7 in the repo to become a finished spec: **Finish in the repo**, below the list, gives the prompt for that.",
      ],
    },
    {
      title: "Export scripts to the repo",
      steps: [
        "Export to Playwright now lives in this tab. On **Auto Run**, pick the Product Backlog Item, press **More** and choose **Export to Playwright**: the Playwright tab opens with the export window over it.",
        "Check the clone folder, then, under **Mappings**, where each area's specs go (one **Folder for** field per area: the feature's folder path under suites, which lists the repo's own feature paths as you type, and takes a path that is not listed too) and which repo user each of your accounts signs in as. Press **Save mappings** when you change them.",
        "Under **Cases**, the cases are grouped by module, and each says **New** or **In the repo**, with where the repo has it. Cases already in the repo are skipped. Tick one to replace it. A case the repo marks as excluded cannot be ticked. A case with no Auto Run script is not listed: the window only says how many there are.",
        "Each case shows the folder it goes to, which is its area's. To send one case somewhere else, press **Own folder** beside it and type or pick its folder; **Area's folder** sends it back with the rest of its area. Own folders are kept when you press **Save mappings**. When a case's title names one of the clone's folders beside its area's folder (or inside it), its folder line reads **Suggested:** with that folder's name, and **Use** sends the case there. **Use** and the **Use N suggested folders** button above the cases (shown when two or more cases have one) only change what you are editing: press **Save mappings** to keep them. A case that already has its own folder is not offered one. A folder name can be anything the repo can hold, except one starting with _ or . or with a character Windows does not allow in a name.",
        "Untick any new case you do not want. A case that cannot go says why, with a command to copy when the repo is missing its user.",
        "Read the warnings under each case before you export. A warning is a line with a triangle icon, and it never stops Export: it says what to check first. They are: a step that appears to delete a row it did not name, or every matching row (a repeat that deletes rows with no protected rows listed); a step that appears to publish, which cannot be undone; state the case needs that another listed case changes (the line names the case); the number of TCM quirks that travel with the case; and a case that has not passed twice in a row in TCM yet. TCM works out what a step does from the names of the controls it clicks, so these say \"appears to\". A case that cannot be exported still shows its warnings.",
        "Press **Export**, which shows how many cases it sends. The summary lists the files written and the steps still to do in the repo. Nothing is run, committed or pushed.",
      ],
    },
    {
      title: "Finish exported cases in the repo",
      steps: [
        "After an export, an Update or Add new, **Finish in the repo** shows a ready-made prompt for each test-case file the cases went to. It asks Claude Code to run steps 4, 5, 6 and 7 of the repo's /gen-test on exactly those cases: run the raw specs (the export never runs them) and fix them in place if they fail, then the refactor, the lint and live check, and the run summary. It tells Claude Code not to fetch the tickets again or regenerate the raw specs.",
        "The cases go into the spec named like their test-case file, which is how the repo pairs a spec with its test cases: it is created if it does not exist yet, and added to if it does.",
        "Before the raw specs run, the prompt has Claude Code set the repo's seed to the user each case was exported to run as, and put the seed back afterwards.",
        "One spec runs as one account, so the card says so instead of giving a prompt when it cannot. When the cases of one file were exported to run as different accounts, map them to the same account, or export them in separate runs. When they run as a different account than the spec already uses, map them to the spec's account, then export them again. A case whose account is not in the repo's users.json is refused the same way.",
        "A case a spec already has is left out of the prompt and listed: change that test with /edit-spec in the repo.",
        "Press **Copy**, open Claude Code in your clone and paste the prompt. The app only copies the text: it does not start Claude Code or anything else.",
      ],
    },
  ],
};
