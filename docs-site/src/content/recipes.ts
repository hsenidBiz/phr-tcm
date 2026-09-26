// The Common tasks page: short recipes whose steps link into the screen
// sections ("screen" or "screen/control"). A link to a section that is not
// on the page yet renders as plain text, never a dead link.

import type { Recipe } from "../types";

export const recipes: Recipe[] = [
  {
    id: "first-upload",
    title: "Upload your first test cases",
    steps: [
      { text: "Sign in, then choose the organisation, project and PBI in the bar at the top.", link: "getting-started/find-pbi" },
      { text: "Press **Import JSON** and pick your file, or write the cases on Manual Entry.", link: "import-file/import-json" },
      { text: "Check the queue: open a case to read its steps.", link: "import-file/expand-steps" },
      { text: "Press **Review** and deal with any duplicate it finds.", link: "import-file/review" },
      { text: "Check the highlighted PBI and confirm the upload.", link: "import-file/confirm" },
      { text: "Read the results: every case with its new id.", link: "import-file/results" },
    ],
  },
  {
    id: "update-from-json",
    title: "Update cases you already uploaded, from a JSON file",
    steps: [
      { text: "Export the cases with their ids from View Test Cases.", link: "view-test-cases" },
      { text: "Edit the file. Keep each case's id: that is what makes it an update.", link: "import-file" },
      { text: "Press **Import JSON** and pick the file. Updates are marked **UPDATE** and a number.", link: "import-file/update-badge" },
      { text: "Open **what will change** on each one to check the changes.", link: "import-file/what-changes" },
      { text: "Press **Review**, then confirm.", link: "import-file/confirm" },
    ],
  },
  {
    id: "share-draft",
    title: "Share a draft for review",
    steps: [
      { text: "With the cases in the queue, press **Share for review**. The link is copied for you.", link: "import-file/share-for-review" },
      { text: "Send the link to your reviewer. It works once." },
      { text: "The reviewer pastes it into **Share link** on Import File and presses **Import shared**.", link: "import-file/import-shared" },
      { text: "They read the cases and leave comments with **View in browser**.", link: "import-file/view-in-browser" },
    ],
  },
  {
    id: "rerun-failed",
    title: "Run the failed cases again",
    steps: [
      { text: "Open Run Tests with the PBI chosen.", link: "run-tests" },
      { text: "Pick **Failed** in the last outcome filter.", link: "run-tests/filter-outcome" },
      { text: "Press **Select all**.", link: "run-tests/select-all" },
      { text: "Press **Run N in runner**.", link: "run-tests/run-in-runner" },
      { text: "Work through each case, mark its result and press **Next**.", link: "run-tests/next" },
      { text: "Press **Finish** to record the run in Azure DevOps.", link: "run-tests/finish" },
    ],
  },
  {
    id: "raise-bug",
    title: "Raise a bug from a failed step",
    steps: [
      { text: "In the runner, mark the step that went wrong as failed.", link: "run-tests/step-failed" },
      { text: "Press **File bug**.", link: "run-tests/file-bug" },
      { text: "Check the bug's title.", link: "run-tests/bug-title" },
      { text: "Add what actually happened to the repro steps.", link: "run-tests/bug-repro" },
      { text: "Paste any screenshots, then press **File bug**. The bug is linked to the case and the PBI.", link: "run-tests/bug-links" },
    ],
  },
  {
    id: "reorder-suite",
    title: "Put a suite's cases in order",
    steps: [
      { text: "Open Suite Management with the PBI chosen; its suite opens by itself.", link: "suite-management/open-suite" },
      { text: "Drag the cases into order, or select some and move them with the arrows.", link: "suite-management/move" },
      { text: "Press **Apply order** to save the order to the suite in Azure DevOps.", link: "suite-management/apply-order" },
    ],
  },
  {
    id: "find-suite",
    title: "Find a test suite",
    steps: [
      { text: "Open Search Suites and type part of the suite's name.", link: "search-suites/search" },
      { text: "Open the folders down to the suite.", link: "search-suites/folder" },
      { text: "Click the suite to see its test cases and their last outcomes.", link: "search-suites/points" },
      { text: "Open **More** to run it, manage it or open its report.", link: "search-suites/more" },
    ],
  },
  {
    id: "review-pr",
    title: "Review a pull request",
    steps: [
      { text: "Switch to Work Manager.", link: "getting-started/work-manager" },
      { text: "Open **Pull Requests** and find it under **Awaiting your review**.", link: "pull-requests/awaiting" },
      { text: "Click the pull request to open it.", link: "pull-requests/row" },
      { text: "Read the comments and resolve the threads that are dealt with.", link: "pull-requests/resolve" },
      { text: "Check the last pipeline run; **View history** shows where a failed one broke.", link: "pull-requests/view-history" },
      { text: "Press **Open in Azure DevOps** to vote.", link: "pull-requests/open-in-ado" },
    ],
  },
  {
    id: "move-work-item",
    title: "Move a work item on and comment on it",
    steps: [
      { text: "Switch to Work Manager and open **Board**.", link: "board" },
      { text: "Drag the card into the next column to change its state.", link: "board/card" },
      { text: "Click the card to open it.", link: "board/card" },
      { text: "Write a comment and press **Post**.", link: "board/post" },
    ],
  },
  {
    id: "connect-ai",
    title: "Have an AI assistant write test cases",
    steps: [
      { text: "On AI Bridge, press **Add repository** and pick the repository the cases belong to.", link: "ai-bridge/add-repository" },
      { text: "Press **Register** beside your assistant, then start its session in that repository.", link: "ai-bridge/register" },
      { text: "Ask it to write test cases for your PBI. It saves them as a file in the repository's .test-cases folder.", link: "ai-bridge/breakdown" },
      { text: "Import the file on Import File, review the queue and upload it.", link: "import-file/import-json" },
    ],
  },
];
