// Pull Requests (Work Manager): the pull requests waiting on you, your
// own, and the active ones on the repositories you track, with their
// comments, checks and linked work items.
//
// Locates come from screens/PrPanel.tsx, components/PrThreads.tsx,
// components/PipelineDialog.tsx and components/ui/multiselect.tsx. Each
// route switches to Work Manager first (every capture boot starts on the
// test case screens). In capture mode one repository is tracked beside
// your own pull requests, and the open review thread's reply carries a
// pasted screenshot (src/dev/demo.ts).

import type { Screen, Step } from "../types";

const LIST = "pull-requests-list";
const OPEN = "pull-requests-open";
const PICKER = "pull-requests-picker";
const PIPELINE = "pull-requests-pipeline";
const LOG = "pull-requests-log";

const NAV: Step[] = [{ click: { testId: "work" } }, { nav: "Pull Requests" }];
const OPEN_ROW: Step[] = [
  ...NAV,
  { click: { role: "button", nameRe: "^!501 " } },
  { waitFor: { role: "button", name: "Resolve" } },
];
const OPEN_HISTORY: Step[] = [
  ...OPEN_ROW,
  { click: { role: "button", name: "View history" } },
  { waitFor: { role: "button", name: "Close pipeline history" } },
];

export const pullRequests: Screen = {
  id: "pull-requests",
  title: "Pull Requests",
  group: "Work Manager",
  summary:
    "The pull requests that need you: the ones waiting for your review, your own, and the active ones on any repository you choose to follow. " +
    "Open one to read its description and comments, see its checks and linked work items, and resolve comment threads. Voting and completing stay in Azure DevOps.",
  shots: [
    { id: LIST, route: [...NAV, { waitFor: { role: "button", nameRe: "^!501 " } }], alt: "Pull Requests: awaiting your review, your pull requests and one followed repository" },
    { id: OPEN, route: OPEN_ROW, alt: "A pull request opened, with its comments, reviewers, last pipeline run and work items" },
    {
      id: PICKER,
      route: [...NAV, { click: { role: "button", name: "Repositories" } }, { waitFor: { role: "checkbox", name: "Your Pull Requests" } }],
      alt: "The list of what to show: your pull requests and the project's repositories",
    },
    { id: PIPELINE, route: OPEN_HISTORY, alt: "The pipeline history of a pull request" },
    {
      id: LOG,
      route: [...OPEN_HISTORY, { click: { role: "button", nameRe: "^Run Unit Test" } }, { waitFor: { role: "button", name: "Close log" } }],
      alt: "One step's log from a pipeline run",
    },
  ],
  controls: [
    // --- The list -----------------------------------------------------------------
    {
      id: "repositories",
      shot: LIST,
      locate: { role: "button", name: "Repositories" },
      name: "What to show",
      does:
        "Picks what the page lists besides the pull requests waiting for your review: **Your Pull Requests**, and any of the project's repositories. " +
        "It shows what is picked, or how many. The choice is remembered for each project.",
    },
    {
      id: "clear-repositories",
      shot: LIST,
      locate: { role: "button", name: "Clear selection" },
      name: "Clear (x)",
      does: "Unticks everything, leaving only the pull requests waiting for your review.",
    },
    {
      id: "active-completed",
      shot: LIST,
      locate: { role: "button", name: "active" },
      name: "Active / Completed",
      does: "Whether the repositories you follow show their active or their completed pull requests. It does not change the two lists above them.",
    },
    {
      id: "refresh",
      shot: LIST,
      locate: { role: "button", name: "Refresh pull requests" },
      name: "Refresh",
      does: "Reads the lists again from Azure DevOps.",
    },
    {
      id: "awaiting",
      shot: LIST,
      locate: { role: "heading", nameRe: "^Awaiting your review" },
      name: "Awaiting your review",
      does: "Active pull requests you are a reviewer on and have not voted on yet, with how many there are. The heading lights up while there are any.",
    },
    {
      id: "row",
      shot: LIST,
      locate: { role: "button", nameRe: "^!501 " },
      name: "Pull request",
      does:
        "Its number and title, the repository, the branch it merges and the branch it merges into, and who opened it. The dots on the right are the reviewers: green approved, amber waiting for the author, " +
        "red rejected, grey not voted yet (hover a dot for the name). A pill counts the comment threads still to resolve. Click the row to open it.",
    },
    {
      id: "pipeline-in-progress",
      shot: LIST,
      locate: { text: "Pipeline In Progress" },
      name: "Pipeline In Progress",
      does: "The pull request's validation build is still running. A build that passed shows nothing, so a quiet row is a good sign.",
    },
    {
      id: "open-in-ado",
      shot: LIST,
      locate: { role: "button", name: "Open !501 in Azure DevOps" },
      name: "Open in Azure DevOps",
      does: "Opens the pull request in your browser, for voting, replying or completing it.",
    },
    {
      id: "yours",
      shot: LIST,
      locate: { role: "heading", nameRe: "^Your pull requests" },
      name: "Your pull requests",
      does: "The active pull requests you opened. Shown while **Your Pull Requests** is ticked.",
    },
    {
      id: "draft",
      shot: LIST,
      locate: { text: "Draft" },
      name: "Draft",
      does: "The pull request is still a draft.",
    },
    {
      id: "conflicts",
      shot: LIST,
      locate: { text: "Conflicts" },
      name: "Conflicts",
      does: "The pull request has merge conflicts to resolve.",
    },
    {
      id: "pipeline-error",
      shot: LIST,
      locate: { text: "Pipeline Error" },
      name: "Pipeline Error",
      does: "The pull request's validation build failed. Open the row to see where.",
    },
    {
      id: "followed-repository",
      shot: LIST,
      locate: { role: "heading", nameRe: "^Active on " },
      name: "Active on (repository)",
      does:
        "Every other active pull request on a repository you follow (or **Completed on**, with Completed chosen). With Active chosen, a pull request already listed above is not repeated. " +
        "A long list shows part at a time, with **Load more** under it.",
    },

    // --- An open pull request -------------------------------------------------------
    {
      id: "description",
      shot: OPEN,
      locate: { text: "What changed and why." },
      name: "Description",
      does: "The pull request's description. A long one is cut short with **View more**, which opens the whole text in a window.",
    },
    {
      id: "comments",
      shot: OPEN,
      locate: { text: "1 unresolved of 2" },
      name: "Comments",
      does: "How many comment threads are still open. The open ones come first; resolved ones stay below them as a record.",
    },
    {
      id: "thread",
      shot: OPEN,
      locate: { text: "/src/components/LoginForm.tsx:42" },
      name: "Comment thread",
      does:
        "One conversation: the file and line it is about (or **General comment**), its status, and each comment with its author and when it was written. Links open in your browser.",
    },
    {
      id: "image",
      shot: OPEN,
      locate: { role: "img", name: "sign-in error" },
      name: "Pictures",
      does: "Screenshots pasted into a comment show in place. If one cannot be loaded, a note says so.",
    },
    {
      id: "resolve",
      shot: OPEN,
      locate: { role: "button", name: "Resolve" },
      name: "Resolve",
      does: "Marks the thread resolved in Azure DevOps. Resolving and reactivating threads are the only changes this page makes there.",
    },
    {
      id: "reactivate",
      shot: OPEN,
      locate: { role: "button", name: "Reactivate" },
      name: "Reactivate",
      does: "Puts a resolved thread back to active in Azure DevOps.",
    },
    {
      id: "view-history",
      shot: OPEN,
      locate: { role: "button", name: "View history" },
      name: "View history",
      does: "Opens every pipeline run of this pull request, with its stages and steps.",
    },
    {
      id: "last-run",
      shot: OPEN,
      locate: { role: "button", nameRe: "^Open build " },
      name: "Last Run Pipeline",
      does:
        "The latest build: its result, name and number, whether it is the PR validation build or CI, its stages and the environments it was deployed to. " +
        "A failed one names the step it failed at. The arrow button opens the build in Azure DevOps. Above it, the reviewers are listed with their votes.",
    },
    {
      id: "work-item",
      shot: OPEN,
      locate: { role: "button", nameRe: "^2005 " },
      name: "Work items",
      does: "The work items linked to the pull request, with their state. Click one to open it in your browser.",
    },

    // --- What to show ------------------------------------------------------------------
    {
      id: "your-pull-requests",
      shot: PICKER,
      locate: { role: "checkbox", name: "Your Pull Requests" },
      name: "Your Pull Requests",
      does: "Ticked, the page lists the pull requests you opened. It starts ticked.",
    },
    {
      id: "repository",
      shot: PICKER,
      locate: { role: "checkbox", name: "portal-api" },
      name: "A repository",
      does:
        "Tick a repository to follow it: its pull requests get a list of their own. What you ticked is listed first each time the list opens. With many repositories, a search box appears at the top.",
    },

    // --- Pipeline history ------------------------------------------------------------------
    {
      id: "close-history",
      shot: PIPELINE,
      locate: { role: "button", name: "Close pipeline history" },
      name: "Close",
      does: "Closes the pipeline history. The line under the title says how many runs there were and how far the change got, such as the last environment it reached.",
    },
    {
      id: "search-steps",
      shot: PIPELINE,
      locate: { role: "textbox", name: "Search stages and steps" },
      name: "Search stages, jobs and steps",
      does: "Shows only the stages, jobs and steps whose names contain what you type, with every run opened.",
    },
    {
      id: "failures-only",
      shot: PIPELINE,
      locate: { role: "button", name: "Failures only" },
      name: "Failures only",
      does: "Shows only the steps that failed. Greyed out when nothing failed.",
    },
    {
      id: "run",
      shot: PIPELINE,
      locate: { role: "button", nameRe: "^in progress " },
      name: "Run",
      does:
        "One run, newest first: its result, number and kind, when it started and how long it took, and where it failed or what is running now. " +
        "Click it to open or fold its stages. The run that needs attention opens by itself.",
    },
    {
      id: "step",
      shot: PIPELINE,
      locate: { role: "button", nameRe: "^Restore The Solution" },
      name: "Step",
      does: "A step of the run, with how long it took. A failed step shows its error under it. Click a step to read its log.",
    },
    {
      id: "open-run",
      shot: PIPELINE,
      locate: { role: "button", name: "Open run in Azure DevOps" },
      name: "Open run in Azure DevOps",
      does: "Opens the run in your browser. A run that was deployed also lists its environments, each with a button to open the release.",
    },

    // --- A step's log ---------------------------------------------------------------------
    {
      id: "close-log",
      shot: LOG,
      locate: { role: "button", name: "Close log" },
      name: "Close",
      does: "Closes the log. The log of a step that is still running keeps filling in while it is open.",
    },
    {
      id: "copy-log",
      shot: LOG,
      locate: { role: "button", name: "Copy log" },
      name: "Copy log",
      does: "Copies the whole log.",
    },
  ],
  tips: [
    "Clicking a pull request in the notification bell opens this page with that pull request open. If no list here holds it, a message says what to tick to see it.",
  ],
  howTo: [
    {
      title: "Review a pull request",
      steps: [
        "Switch to Work Manager and open **Pull Requests**.",
        "Click a pull request under **Awaiting your review** to open it.",
        "Read the description and the comments, and check **Last Run Pipeline**.",
        "Resolve the threads that are dealt with, then press **Open in Azure DevOps** to vote.",
      ],
    },
  ],
};
