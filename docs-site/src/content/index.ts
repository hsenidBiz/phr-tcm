// The registry: every documented screen, in page order, and the Common
// tasks recipes. Each screen lives in its own file beside this one and is
// added to `screens` here.

import type { Intro, Screen } from "../types";
import { aiBridge } from "./ai-tools";
import { board } from "./board";
import { gettingStarted } from "./getting-started";
import { importFile } from "./import-file";
import { manualEntry } from "./manual-entry";
import { newWorkItem } from "./new-work-item";
import { pullRequests } from "./pull-requests";
import { runTests } from "./run-tests";
import { searchSuites } from "./search-suites";
import { settings } from "./settings";
import { suiteManagement } from "./suite-management";
import { updateTestCases } from "./update-test-cases";
import { viewTestCases } from "./view-test-cases";

export { recipes } from "./recipes";

export const screens: Screen[] = [
  gettingStarted,
  manualEntry,
  importFile,
  updateTestCases,
  viewTestCases,
  runTests,
  searchSuites,
  suiteManagement,
  pullRequests,
  board,
  newWorkItem,
  aiBridge,
  settings,
];

/** The hero and the Quick start strip. A step links to its section once
 *  that section exists; until then it shows as plain text. */
export const intro: Intro = {
  promise: "Every screen and every button, in plain words.",
  lead:
    "Write test cases by hand or import them, review the queue, upload them to your PBI, then run them and follow the work. " +
    "Pick a screen from the contents, or press [[Ctrl+K]] to search.",
  heroShot: "import-file-queue",
  quickStart: [
    { label: "Sign in", hint: "With your Microsoft work account", link: "getting-started" },
    { label: "Pick a PBI", hint: "Organisation, project, backlog item", link: "getting-started/find-pbi" },
    { label: "Write or import", hint: "By hand, or from a JSON file", link: "import-file/import-json" },
    { label: "Review", hint: "Check every case in the queue", link: "import-file/review" },
    { label: "Upload", hint: "Linked to the PBI and its suite", link: "import-file/confirm" },
    { label: "Run", hint: "Step through and record results", link: "run-tests" },
  ],
};
