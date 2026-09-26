// Search Suites: every test plan of the project as a tree of its suites,
// with each suite's test points and what can be done with it.
//
// Locates come from screens/Suites.tsx and components/MoreActionsMenu.tsx.
// A suite row is one button holding smaller ones, so it is found by the
// start of its name ("PBI 1001 ..."); the chips beside it are named for
// their suite ("View <suite>", "Edit cases in <suite>"). The sample plan
// has one folder, "Web app", holding the two PBIs' suites. The menu shot
// waits for the opened suite's points first: a fold still animating
// scrolls, and a scroll closes the menu.

import type { Screen, Step } from "../types";

const TREE = "search-suites-tree";
const MORE = "search-suites-more";

const SUITE = "Login and session flow";
const OPEN_SUITE: Step[] = [
  { nav: "Search Suites" },
  { click: { text: "Web app" } },
  { click: { role: "button", nameRe: "^PBI 1001 " } },
  { waitFor: { role: "table", name: `Test points in ${SUITE}` } },
];

export const searchSuites: Screen = {
  id: "search-suites",
  title: "Search Suites",
  group: "Running tests",
  summary:
    "Browse every test plan in the project and the suites inside it. Find a suite by name, see its test cases and their last outcomes, " +
    "and open it where you need it: in the browser, in Update Test Cases, in Run Tests or in Suite Management.",
  shots: [
    { id: TREE, route: OPEN_SUITE, alt: "A test plan's folder opened, with one suite showing its test points" },
    {
      id: MORE,
      route: [...OPEN_SUITE, { click: { role: "button", name: `More actions for ${SUITE}` } }, { waitFor: { role: "menuitem", name: "Report" } }],
      alt: "The More menu of a PBI's suite",
    },
  ],
  controls: [
    {
      id: "refresh",
      shot: TREE,
      locate: { role: "button", name: "Refresh test plans" },
      name: "Refresh test plans",
      does:
        "Reads every plan and suite from Azure DevOps again, with fresh outcomes. The tree is kept on this computer, so it opens at once; press this after suites change.",
    },
    {
      id: "search",
      shot: TREE,
      locate: { role: "textbox", name: "Search suites" },
      name: "Search plans and suites",
      does:
        "Shows only the plans and suites whose names contain what you type, with the folders they sit in. Results start folded; a plan whose name matches keeps all its suites.",
    },
    {
      id: "folder",
      shot: TREE,
      locate: { role: "button", nameRe: "^Web app Open " },
      name: "Folder",
      does: "A suite that holds other suites. Click it to open or fold it. Each plan starts with its folders folded, under the plan's name and area.",
    },
    {
      id: "pbi-badge",
      shot: TREE,
      locate: { text: "PBI 1001" },
      name: "PBI badge",
      does: "Marks a PBI's own suite, with the PBI's number.",
    },
    {
      id: "suite",
      shot: TREE,
      locate: { role: "button", nameRe: "^PBI 1001 " },
      name: "Suite",
      does: "Click a suite to show or hide its test points below it.",
    },
    {
      id: "open-in-ado",
      shot: TREE,
      locate: { role: "button", name: `Open ${SUITE} in Azure DevOps` },
      name: "Open in Azure DevOps",
      does: "Opens the suite in Azure DevOps' Test Plans, in your browser.",
    },
    {
      id: "copy-link",
      shot: TREE,
      locate: { role: "button", name: `Copy link to ${SUITE}` },
      name: "Copy link",
      does: "Copies the suite's Azure DevOps link.",
    },
    {
      id: "view",
      shot: TREE,
      locate: { role: "button", name: `View ${SUITE}` },
      name: "View",
      does: "Opens the test cases as a page in your browser. On a folder, every suite inside it is included.",
    },
    {
      id: "edit-cases",
      shot: TREE,
      locate: { role: "button", name: `Edit cases in ${SUITE}` },
      name: "Edit cases",
      does:
        "Opens the cases in Update Test Cases. On a PBI's suite, that PBI becomes the current one. On any other suite or folder, the cases under it are handed over on their own.",
    },
    {
      id: "more",
      shot: TREE,
      locate: { role: "button", name: `More actions for ${SUITE}` },
      name: "More",
      does: "Opens the other options for the suite. Hovering it opens them too.",
    },
    {
      id: "points",
      shot: TREE,
      locate: { role: "table", name: `Test points in ${SUITE}` },
      name: "Test points",
      does: "The suite's test cases, one row per configuration they run on, with the last outcome of each.",
    },
    {
      id: "use-as-pbi",
      shot: MORE,
      locate: { role: "menuitem", name: "Use as current PBI" },
      name: "Use as current PBI",
      does: "Makes the suite's PBI the one chosen in the bar at the top, without leaving this screen. Offered on a PBI's suite.",
    },
    {
      id: "manage",
      shot: MORE,
      locate: { role: "menuitem", name: "Manage" },
      name: "Manage",
      does: "Opens Suite Management on this suite's plan, with the suite open.",
    },
    {
      id: "run",
      shot: MORE,
      locate: { role: "menuitem", name: "Run Tests" },
      name: "Run Tests",
      does:
        "On a PBI's suite, opens Run Tests for that PBI. On any other suite, opens the runner straight away with the suite's own cases (not those of the suites inside it).",
    },
    {
      id: "report",
      shot: MORE,
      locate: { role: "menuitem", name: "Report" },
      name: "Report",
      does: "Opens an execution report of the suite and every suite inside it, in your browser.",
    },
  ],
  tips: [
    "Plans with no suites are not listed.",
    "While the app collects the cases for **View**, **Edit cases** or a run, the buttons are greyed out and a note says so.",
  ],
  howTo: [
    {
      title: "Find a suite and run it",
      steps: [
        "Type part of the suite's name in **Search plans and suites**.",
        "Open the folders down to the suite; click it to check its cases.",
        "Open **More** and choose **Run Tests**.",
      ],
    },
  ],
};
