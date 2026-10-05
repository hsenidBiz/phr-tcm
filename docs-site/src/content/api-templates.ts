// API Templates: the operations an assistant has built from the tested
// application's code and proven on this computer - each one a way to create
// or change test data through the site's own endpoints - and the flows that
// put them in order. Read and remove only: assistants build, prove and run
// them, never this screen.
//
// Locates come from screens/ApiTemplates/index.tsx, TemplateRow.tsx and
// FlowMap.tsx. The sample data (src/dev/demo.ts) has five templates in two
// modules (one imported and not proven yet) and one flow of three stages,
// the last of them with no template yet. The View flow page is written for
// the browser and is described, not captured.

import type { Screen, Step } from "../types";

const LIST = "api-templates-list";
const DETAILS = "api-templates-details";
const FLOWS = "api-templates-flows";

const NAV: Step = { nav: "API Templates" };
const READY: Step = { waitFor: { role: "tab", nameRe: "^Templates" } };

export const apiTemplates: Screen = {
  id: "api-templates",
  title: "API Templates",
  group: "AI tools",
  summary:
    "An API template is a recipe for creating or changing test data through the site's own requests, the ones its pages send: create a user, lock an account, add an address. " +
    "An assistant builds each one from the application's code and proves it on your site before it appears here; after that, a script's precondition or the assistant itself can run it for the data a test needs. " +
    "This screen shows them, and lets you read and remove them. " +
    "API Templates is part of the advanced features: turn on **Enable Advanced Features** in Settings, under General, and it appears at the end of the sidebar.",
  shots: [
    { id: LIST, route: [NAV, READY], alt: "API Templates: the templates grouped by module, each with what it does and when it last ran" },
    {
      id: DETAILS,
      route: [
        NAV,
        READY,
        { click: { role: "button", name: "Show details of Create a user" } },
        { waitFor: { role: "table", name: "Parameters" } },
        { scrollTo: { role: "list", name: "Runs" } },
      ],
      alt: "One template opened: its parameters, steps, proof and runs",
    },
    {
      id: FLOWS,
      route: [NAV, READY, { click: { role: "tab", nameRe: "^Flows" } }, { waitFor: { role: "heading", name: "Set up a locked account" } }],
      alt: "The Flows tab: a flow of three stages drawn as a map, with the templates that perform each stage",
    },
  ],
  groups: [
    { id: "templates", title: "The templates", summary: "Every template of the project, by module, and what each one does." },
    { id: "details", title: "Inside a template", summary: "Its parameters, its requests, its proof, and every time it ran." },
    { id: "flows", title: "Flows", summary: "The stages a record goes through, in the order the application allows, and the templates that perform them." },
  ],
  controls: [
    // --- The list ------------------------------------------------------------
    {
      id: "runs-against",
      shot: LIST,
      group: "templates",
      locate: { text: "portal.example.test" },
      name: "Runs against",
      does: "The site templates run on: the site address of the active environment, set on the Auto Run tab's **Setup**.",
    },
    {
      id: "writes-switch",
      shot: LIST,
      group: "templates",
      locate: { role: "button", name: "API templates on" },
      name: "API templates on / off",
      does:
        "Whether an assistant may prove and run templates, which writes test data to your site. It is switched on and off on the AI Bridge tab, where this button takes you, and is off until you turn it on. " +
        "Templates run as your Auto Run accounts.",
    },
    {
      id: "test-files",
      shot: LIST,
      group: "templates",
      locate: { role: "button", name: "Test files" },
      name: "Test files",
      does: "The documents and pictures templates and scripts upload, kept on this computer. The same list as on the Auto Run tab.",
    },
    {
      id: "export",
      shot: LIST,
      group: "templates",
      locate: { role: "button", name: "Export" },
      name: "Export",
      does: "Saves every template and flow of the project to one file, to share with a colleague. The proof from your site and the run history are left out.",
    },
    {
      id: "import",
      shot: LIST,
      group: "templates",
      locate: { role: "button", name: "Import" },
      name: "Import",
      does:
        "Adds the templates and flows from a file someone exported, after a warning. One with the same id as yours replaces it. Imported templates are marked **Unproven** until they are proven on your site.",
    },
    {
      id: "tab-templates",
      shot: LIST,
      group: "templates",
      locate: { role: "tab", nameRe: "^Templates" },
      name: "Templates",
      does: "The templates, grouped by the module of the application they belong to, with how many there are.",
    },
    {
      id: "tab-flows",
      shot: LIST,
      group: "flows",
      locate: { role: "tab", nameRe: "^Flows" },
      name: "Flows",
      does: "The flows, each drawn as a map of its stages.",
    },
    {
      id: "search",
      shot: LIST,
      group: "templates",
      locate: { role: "textbox", name: "Search templates" },
      name: "Search",
      does: "Shows only the templates whose title, module, id, stage or flow contains what you type. Groups open while you search.",
    },
    {
      id: "module-group",
      shot: LIST,
      group: "templates",
      locate: { role: "button", name: "Collapse group Accounts" },
      name: "Module",
      does: "Folds the module's templates away, or opens them again. Clicking the module's name does the same.",
    },
    {
      id: "show-details",
      shot: LIST,
      group: "templates",
      locate: { role: "button", name: "Show details of Create a user" },
      name: "Details (>)",
      does: "Opens everything about the template below its line.",
    },
    {
      id: "template",
      shot: LIST,
      group: "templates",
      locate: { role: "listitem", name: "Lock a user account" },
      name: "Template",
      does:
        "One template: its title, what it does to the data (**create**, **edit** or **delete**), the flow stage it performs, and when it last ran, with a green or red dot for how it went.",
    },
    {
      id: "stage",
      shot: LIST,
      group: "templates",
      locate: { text: "Stage: Create a user" },
      name: "Stage",
      does: "The stage of a flow the template performs. A stage whose flow is no longer saved is marked, and the template cannot run until the flow is saved again.",
    },
    {
      id: "unproven",
      shot: LIST,
      group: "templates",
      locate: { text: "Unproven" },
      name: "Unproven",
      does:
        "An imported template that has not been proven on your site yet. It can run, but ask the assistant to prove it before relying on it; the runs it lists may be from the version it replaced.",
    },
    {
      id: "remove",
      shot: LIST,
      group: "templates",
      locate: { role: "button", name: "Remove Create a user" },
      name: "Remove",
      does: "Removes the template from this computer, after asking. The data it created on your site stays.",
    },
    {
      id: "collapse-all",
      shot: LIST,
      group: "templates",
      locate: { role: "button", nameRe: "^Collapse all \\(\\d+\\)$" },
      name: "Collapse all",
      does: "Folds every module in one press, and opens them all again once they are folded.",
    },

    // --- Details ---------------------------------------------------------------
    {
      id: "parameters",
      shot: DETAILS,
      group: "details",
      locate: { role: "table", name: "Parameters" },
      name: "Parameters",
      does:
        "What a run of the template needs: each value's name and type, whether it is required, what an optional one is left as, what it is for, and a hint the assistant uses to find a real value.",
    },
    {
      id: "steps",
      shot: DETAILS,
      group: "details",
      locate: { role: "list", name: "Steps" },
      name: "Steps",
      does: "The requests the template sends, in order: the address, the fields it sends, any test file it uploads, what it expects back, and the values it keeps from the answer.",
    },
    {
      id: "sources",
      shot: DETAILS,
      group: "details",
      locate: { role: "list", name: "Sources" },
      name: "Sources",
      does: "The files of the application's code the template was built from.",
    },
    {
      id: "proven",
      shot: DETAILS,
      group: "details",
      locate: { role: "heading", name: "Proven" },
      name: "Proven",
      does:
        "When the template was proven, as which account and in which environment, and what the proof created. Proving runs the template end to end on your site; it is saved only if every step passed.",
    },
    {
      id: "runs",
      shot: DETAILS,
      group: "details",
      locate: { role: "list", name: "Runs" },
      name: "Runs",
      does: "Every proof and run, newest first: when, which kind, as which account, whether it worked or the step it failed at and why, and the values it created.",
    },
    {
      id: "hide-details",
      shot: DETAILS,
      group: "details",
      locate: { role: "button", name: "Hide details of Create a user" },
      name: "Hide details (v)",
      does: "Folds the details away again.",
    },

    // --- Flows -----------------------------------------------------------------
    {
      id: "flow",
      shot: FLOWS,
      group: "flows",
      locate: { role: "heading", name: "Set up a locked account" },
      name: "Flow",
      does:
        "A flow is the path a record takes through a part of the application, such as a wizard, as stages in the order the application allows. The assistant maps it from the code, with a check for each stage that tells whether it is done for a record.",
    },
    {
      id: "tracks",
      shot: FLOWS,
      group: "flows",
      locate: { text: "Tracks user_id" },
      name: "Tracks",
      does: "The record the flow follows. Before running a template, the assistant asks which stages are already done for that record, so templates go in an order the application accepts.",
    },
    {
      id: "zoom",
      shot: FLOWS,
      group: "flows",
      locate: { role: "button", name: "Zoom in" },
      name: "Zoom",
      does: "Zooms the map in or out. The map can also be dragged, and zoomed with [[Ctrl]] and the mouse wheel.",
    },
    {
      id: "view-flow",
      shot: FLOWS,
      group: "flows",
      locate: { role: "button", name: "View flow Set up a locked account in the browser" },
      name: "View flow",
      does: "Opens the flow on a page of its own in your browser: its stages, the check each one runs, and the templates that perform them.",
    },
    {
      id: "remove-flow",
      shot: FLOWS,
      group: "flows",
      locate: { role: "button", name: "Remove flow Set up a locked account" },
      name: "Remove flow",
      does: "Removes the flow from this computer, after asking. Its templates stay, but cannot run until a flow with their stage is saved again.",
    },
    {
      id: "stage-box",
      shot: FLOWS,
      group: "flows",
      locate: { role: "group", name: "Lock the account" },
      name: "Stage",
      does: "One stage of the flow. The arrows come in from the stages it needs first; a dashed box is a stage that can be skipped.",
    },
    {
      id: "stage-template",
      shot: FLOWS,
      group: "flows",
      locate: { role: "button", nameRe: "^Lock a user account" },
      name: "Template on a stage",
      does: "A template that performs the stage, with what it does to the data. Click it to open its details on the Templates tab.",
    },
    {
      id: "no-template",
      shot: FLOWS,
      group: "flows",
      locate: { text: "No template yet" },
      name: "No template yet",
      does: "A stage no template performs. Ask the assistant to build one when your tests need it.",
    },
  ],
  tips: [
    "This screen only shows templates and removes them. Building, proving and running them is your assistant's work, through the app's AI tools on the AI Bridge tab.",
    "Proving or running a template writes real test data to your site, so it is allowed only while **API templates** is switched on, on the AI Bridge tab.",
    "Templates and flows are kept on this computer, per project. Use **Export** and **Import** to share them with your team.",
  ],
  howTo: [
    {
      title: "Get your first templates",
      steps: [
        "Turn on **Enable Advanced Features** in Settings, under General.",
        "Set up the site address and your accounts on the Auto Run tab's **Setup**.",
        "On the AI Bridge tab, connect your assistant and switch **API templates** on.",
        "Ask the assistant to build a template for the data you need. It proves the template on your site, and it appears here once it worked.",
      ],
    },
    {
      title: "Check a template before relying on it",
      steps: [
        "Press **>** beside the template to open its details.",
        "Read **Proven**: when it last worked on your site and what it created.",
        "Read **Runs** for any failure since, with the step it failed at and why.",
        "An **Unproven** template came from an import: ask the assistant to prove it on your site.",
      ],
    },
    {
      title: "Read a flow",
      steps: [
        "Open the **Flows** tab.",
        "Follow the arrows from left to right: each stage needs the stages that point to it.",
        "Each stage lists the templates that perform it; **No template yet** marks a gap.",
        "Press **View flow** to see the flow on a page of its own, with the check each stage runs.",
      ],
    },
  ],
};
