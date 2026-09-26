// Suite Management: a test plan's suites with their cases, for putting a
// suite's cases in order and copying cases into other suites.
//
// Locates come from screens/ManageCases/index.tsx, PlanTable.tsx,
// SuiteCases.tsx, CaseOrderList.tsx and NewSuiteDialog.tsx. With PBI #1001
// chosen, the screen opens on the plan that holds its suite, with that
// suite open. In capture mode the sample data lists each PBI suite's cases
// (listSuiteEntries in src/dev/demo.ts). Apply order from files needs files
// picked from disk, so its window is described in the tips.

import type { Screen, Step } from "../types";

const PLAN = "suite-management-plan";
const SELECTED = "suite-management-selected";
const GROUPED = "suite-management-grouped";
const NEW_SUITE = "suite-management-new-suite";

const NAV: Step = { nav: "Suite Management" };
const FIRST = "Login - valid credentials";

export const suiteManagement: Screen = {
  id: "suite-management",
  title: "Suite Management",
  group: "Running tests",
  summary:
    "Organise a test plan's suites. Put a suite's cases in the order they should be run and save that order to Azure DevOps, " +
    "copy cases into another suite, or make a new suite for them.",
  shots: [
    { id: PLAN, route: [NAV], alt: "Suite Management showing the PBI's plan with its suite open" },
    {
      id: SELECTED,
      route: [
        NAV,
        { click: { text: FIRST } },
        { click: { role: "combobox", name: "Copy to" } },
        { click: { role: "option", name: "Web app" } },
      ],
      alt: "One test case selected, with a suite picked to copy it to",
    },
    { id: GROUPED, route: [NAV, { click: { role: "switch", name: "Group by title" } }], alt: "A suite's cases grouped by the start of their titles" },
    {
      id: NEW_SUITE,
      route: [NAV, { click: { role: "button", name: "New test suite" } }, { waitFor: { role: "textbox", name: "Suite name" } }],
      alt: "The New test suite window",
    },
  ],
  controls: [
    // --- The plan -----------------------------------------------------------
    {
      id: "scope",
      shot: PLAN,
      locate: { text: "Showing the plan that holds PBI #1001." },
      name: "Which plans",
      does: "With a PBI chosen, only the plan that holds its suite is shown, with that suite open. When the PBI has no suite yet, every plan is shown.",
    },
    {
      id: "show-all",
      shot: PLAN,
      locate: { role: "button", name: "Show all plans" },
      name: "Show all plans",
      does: "Shows every plan in the project. **Show only this PBI's plan** goes back.",
    },
    {
      id: "new-suite",
      shot: PLAN,
      locate: { role: "button", name: "New test suite" },
      name: "New test suite",
      does: "Opens a window for creating a suite in this plan. With cases selected, they are copied into it too. It only shows when you may create suites in the plan's area.",
    },
    {
      id: "folder",
      shot: PLAN,
      locate: { role: "button", name: "Expand Web app" },
      name: "Suite",
      does: "Every suite of the plan, in tree order. Click one to open it and see its cases; click again to close it. Several can be open at once.",
    },
    {
      id: "open-suite",
      shot: PLAN,
      locate: { role: "button", name: "Collapse Login and session flow" },
      name: "Open suite",
      does: "An open suite. A PBI's own suite carries a badge with the PBI's number.",
    },
    {
      id: "group-by-title",
      shot: PLAN,
      locate: { role: "switch", name: "Group by title" },
      name: "Group by title",
      does:
        "Shows a heading over each run of cases whose titles start the same way, and puts each group's cases together. That rearranges the list, so **Apply order** lights up when anything moved. Turning it off only hides the headings. The choice is remembered.",
    },
    {
      id: "apply-order",
      shot: PLAN,
      locate: { role: "button", name: "Apply order" },
      name: "Apply order",
      does: "Saves the order on screen to the suite in Azure DevOps. It is greyed out until the order has changed.",
    },
    {
      id: "reset",
      shot: PLAN,
      locate: { role: "button", name: "Reset" },
      name: "Reset",
      does: "Puts the list back in the suite's saved order. It is greyed out until the order has changed.",
    },
    {
      id: "order-from-files",
      shot: PLAN,
      locate: { role: "button", name: "Apply order from files" },
      name: "Apply order from files",
      does: "Orders the suite by draft files you pick: each file's cases become one block, in the file's own order. See the tips below.",
    },
    {
      id: "count",
      shot: PLAN,
      locate: { text: "5 test cases" },
      name: "Cases",
      does: "How many cases the suite holds, or how many of them are selected.",
    },
    {
      id: "select-all",
      shot: PLAN,
      locate: { role: "button", name: "Select all" },
      name: "Select all",
      does: "Selects every case in the suite.",
    },
    {
      id: "case",
      shot: PLAN,
      locate: { text: FIRST },
      name: "Case",
      does:
        "One case, numbered by its place. Drag it to move it; a selected case carries the whole selection with it. Click to select it, [[Ctrl]]+click to add more, [[Shift]]+click for a range.",
    },
    {
      id: "move",
      shot: PLAN,
      locate: { role: "button", name: "Move #5001 down" },
      name: "Move up / down",
      does: "Moves the case, or the selection it belongs to, one place up or down.",
    },

    // --- A selection ----------------------------------------------------------
    {
      id: "selected",
      shot: SELECTED,
      locate: { text: "1 selected" },
      name: "Selected",
      does: "How many cases are selected in this plan. A selection can take cases from several suites of the same plan; picking a case in another plan starts a new one.",
    },
    {
      id: "clear-selection",
      shot: SELECTED,
      locate: { role: "button", name: "Clear selection" },
      name: "Clear selection",
      does: "Clears the selection in every suite of the plan.",
    },
    {
      id: "copy-to",
      shot: SELECTED,
      locate: { role: "combobox", name: "Copy to" },
      name: "Copy to",
      does: "The suite to copy the selected cases into: the plan's root or one of its static suites. Type to search the list; the x empties the choice.",
    },
    {
      id: "copy",
      shot: SELECTED,
      locate: { role: "button", name: "Copy to suite" },
      name: "Copy to suite",
      does: "Adds the selected cases to the chosen suite. They stay in the suites they were in; a case can belong to many suites. It is greyed out until cases are selected and a suite is picked.",
    },
    {
      id: "list-selected",
      shot: SELECTED,
      locate: { text: "1 of 5 selected" },
      name: "Selected in this suite",
      does: "How many of this suite's cases are selected. **Clear** beside it clears the selection in this suite only.",
    },

    // --- Grouped --------------------------------------------------------------
    {
      id: "a-z",
      shot: GROUPED,
      locate: { role: "button", name: "A-Z groups" },
      name: "A-Z groups",
      does: "Puts every group's cases together and the groups in A to Z order.",
    },
    {
      id: "collapse-groups",
      shot: GROUPED,
      locate: { role: "button", name: "Collapse groups" },
      name: "Collapse groups",
      does: "Folds every group; **Expand groups** opens them all again.",
    },
    {
      id: "group-fold",
      shot: GROUPED,
      locate: { role: "button", name: "Collapse group Login" },
      name: "Group heading",
      does:
        "Click a group's heading or its arrow to fold it. [[Ctrl]]+click the heading to select or clear the whole group. Drag the heading to move the group as one block.",
    },
    {
      id: "group-move",
      shot: GROUPED,
      locate: { role: "button", name: "Move group Login down" },
      name: "Move group up / down",
      does: "Moves the whole group past the group above or below it.",
    },

    // --- New test suite -------------------------------------------------------
    {
      id: "suite-name",
      shot: NEW_SUITE,
      locate: { role: "textbox", name: "Suite name" },
      name: "Suite name",
      does: "The new suite's name.",
    },
    {
      id: "create-inside",
      shot: NEW_SUITE,
      locate: { role: "combobox", name: "Create inside" },
      name: "Create inside",
      does: "Where the suite goes: the plan's root or another static suite, the only places Azure DevOps allows.",
    },
    {
      id: "new-cancel",
      shot: NEW_SUITE,
      locate: { role: "button", name: "Cancel" },
      name: "Cancel",
      does: "Closes the window without creating anything.",
    },
    {
      id: "create",
      shot: NEW_SUITE,
      locate: { role: "button", nameRe: "^Create suite" },
      name: "Create suite",
      does:
        "Creates the suite in Azure DevOps. With cases selected it reads **Create suite and add N test cases** and copies them in as well. It is greyed out until the suite has a name.",
    },
  ],
  tips: [
    "Nothing moves in Azure DevOps until you press **Apply order**. When you scroll down a long suite, its order buttons float at the bottom of the window, and they stay there while the order has unsaved changes.",
    "**Apply order from files** asks for one or more draft JSON files and lists them in a window. Put the files in the order their blocks should take (drag them, or use the arrows); each shows how many of the suite's cases it places. **Add more files** adds another; **Apply** arranges the list, and **Apply order** then saves it. Cases in no file stay after the blocks, in their current order.",
    "**Manage** in Search Suites opens this screen on the suite you picked, with **Show this PBI's plan** or **Show all plans** to go back to the usual view.",
    "When the selected cases sit in a plan that is no longer shown, a line says how many are out of view, with **Clear selection**.",
    "The order saved here is the suite's own order in Azure DevOps, which is what **Spec order** means in Run Tests.",
  ],
  howTo: [
    {
      title: "Put a suite's cases in order",
      steps: [
        "Choose the PBI in the bar at the top; its suite opens by itself.",
        "Drag the cases into order, or select several and move them together with the arrows.",
        "Press **Apply order**.",
      ],
    },
    {
      title: "Copy cases into another suite",
      steps: [
        "Open the suites that hold the cases and select them ([[Ctrl]]+click to add more).",
        "Pick the suite in **Copy to**.",
        "Press **Copy to suite**. To copy them into a suite that does not exist yet, press **New test suite** instead.",
      ],
    },
  ],
};
