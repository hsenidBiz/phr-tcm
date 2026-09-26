// Getting started: signing in and the parts of the app every screen shares -
// the sidebar, the context bar (organisation, project, PBI), the bell, the
// Work Manager switch and the command palette.
//
// Locates come from the real components: Sidebar.tsx (data-tour="nav-*",
// "Close sidebar"), ContextBar.tsx (aria-label "Organization"/"Project",
// data-tour="pbi"/"work", "Settings"), PbiPicker.tsx ("Clear PBI",
// "Find PBI"), NotificationBell.tsx and CommandPalette.tsx. The capture
// boots every shot with PBI #1001 selected (src/dev/demo.ts,
// seedCaptureScene), so a route that clears it cannot affect the next.

import type { Screen } from "../types";

const SHELL = "getting-started-shell";
const PBI = "getting-started-pbi";
const BELL = "getting-started-bell";
const PALETTE = "getting-started-palette";
const WORK = "getting-started-work";

export const gettingStarted: Screen = {
  id: "getting-started",
  title: "Getting started",
  group: "Getting started",
  summary:
    "Sign in once with your Microsoft work account, then choose the organisation, project and PBI you are working on in the bar across the top. " +
    "Every test case screen works on that PBI until you choose another, and the app remembers all three the next time you open it.",
  shots: [
    { id: SHELL, route: [{ nav: "Manual Entry" }], alt: "The app with the sidebar on the left and the context bar across the top" },
    {
      id: PBI,
      route: [{ nav: "Manual Entry" }, { click: { role: "button", name: "Clear PBI" } }, { click: { role: "textbox", name: "Find PBI" } }],
      alt: "Choosing a PBI: the search box with recently used PBIs",
    },
    {
      id: BELL,
      route: [{ nav: "Manual Entry" }, { click: { role: "button", nameRe: "^Notifications" } }],
      alt: "The notification panel opened from the bell",
    },
    {
      id: PALETTE,
      // Scrolled to the end, so the actions and projects are in view with
      // the screen rows above them.
      route: [
        { nav: "Manual Entry" },
        { press: "Control+K" },
        { waitFor: { role: "dialog", name: "Command palette" } },
        { scrollTo: { role: "option", name: "Customer Portal" } },
      ],
      alt: "The command palette",
    },
    {
      id: WORK,
      route: [{ nav: "Manual Entry" }, { click: { testId: "work" } }, { waitFor: { testId: "nav-board" } }],
      alt: "Work Manager, with its own sidebar",
    },
  ],
  controls: [
    // --- The shell --------------------------------------------------------
    {
      id: "sidebar",
      shot: SHELL,
      locate: { testId: "nav-manual" },
      name: "Sidebar",
      does:
        "Lists the test case screens: Manual Entry, Import File, Update Test Cases, View Test Cases, Run Tests, Search Suites, Suite Management and AI Bridge. " +
        "Click one to open it. The highlighted row is the screen you are on.",
      tips: ["[[Ctrl+1]] to [[Ctrl+8]] open the screens in the order they are listed."],
    },
    {
      id: "organization",
      shot: SHELL,
      locate: { role: "combobox", name: "Organization" },
      name: "Organization",
      does: "The Azure DevOps organisation you work in, from every organisation your account can open. Choosing another one clears the project and PBI.",
    },
    {
      id: "project",
      shot: SHELL,
      locate: { role: "combobox", name: "Project" },
      name: "Project",
      does: "The project inside that organisation. Choosing another one clears the PBI.",
    },
    {
      id: "pbi",
      shot: SHELL,
      locate: { testId: "pbi" },
      name: "PBI",
      does:
        "The Product Backlog Item you are working on, with its number and title. Manual Entry, Import File, Update Test Cases, View Test Cases and Run Tests all work on this PBI.",
    },
    {
      id: "clear-pbi",
      shot: SHELL,
      locate: { role: "button", name: "Clear PBI" },
      name: "Clear PBI (x)",
      does: "Clears the PBI so you can search for another one.",
    },
    {
      id: "bell",
      shot: SHELL,
      locate: { role: "button", nameRe: "^Notifications" },
      name: "Notifications",
      does:
        "The red number counts what happened since you last looked: work assigned to you, pull requests with conflicts, reviews or comments, and mentions. Click it to open the list.",
    },
    {
      id: "work-manager",
      shot: SHELL,
      locate: { testId: "work" },
      name: "Work Manager",
      does: "Switches the app to Work Manager: pull requests, the board and new work items. The same button, then named Test Case Manager, brings you back.",
      tips: ["Hover it to see how many pull requests have conflicts or comments to resolve.", "[[Ctrl+Shift+M]] switches too."],
    },
    {
      id: "account",
      shot: SHELL,
      locate: { text: "Alex Tester" },
      name: "Your account",
      does: "The account you are signed in with. It is shown when the window is wide enough.",
    },
    {
      id: "settings",
      shot: SHELL,
      locate: { role: "button", name: "Settings" },
      name: "Settings",
      does: "Opens Settings. Click it again to go back to the screen you came from.",
    },
    {
      id: "palette-hint",
      shot: SHELL,
      locate: { text: "Ctrl+K for commands" },
      name: "Ctrl+K for commands",
      does: "A reminder that [[Ctrl+K]] opens the command palette from anywhere.",
    },
    {
      id: "close-sidebar",
      shot: SHELL,
      locate: { role: "button", name: "Close sidebar" },
      name: "Close",
      does: "Folds the sidebar to a strip of icons, so the screen gets more room. Hover an icon to see its name; the same button opens the sidebar again.",
    },

    // --- Choosing a PBI ---------------------------------------------------
    {
      id: "find-pbi",
      shot: PBI,
      locate: { role: "textbox", name: "Find PBI" },
      name: "Find PBI",
      does:
        "Type a PBI number or words from its title. Matching PBIs appear as you type; [[Enter]] searches at once. Click a result to choose it.",
    },
    {
      id: "recently-used",
      shot: PBI,
      locate: { text: "Recently used" },
      name: "Recently used",
      does: "Before you type, the PBIs you chose most recently in this project. Click one to choose it again; the x beside it removes it from the list.",
    },
    {
      id: "recent-pbis",
      shot: PBI,
      locate: { text: "Recent PBIs" },
      name: "Recent PBIs",
      does: "While no PBI is chosen, the screen offers the same recent PBIs as cards. Click one to choose it.",
    },

    // --- Notifications ----------------------------------------------------
    {
      id: "notification-panel",
      shot: BELL,
      locate: { role: "dialog", name: "Notifications" },
      name: "Notification list",
      does:
        "Everything that needs you, newest first. Opening the list marks it all as read; items stay until you dismiss them. Click anywhere else or press [[Esc]] to close it.",
    },
    {
      id: "clear-all",
      shot: BELL,
      locate: { role: "button", name: "Clear all" },
      name: "Clear all",
      does: "Removes every notification from the list.",
    },
    {
      id: "kind",
      shot: BELL,
      locate: { text: "Mention" },
      name: "Kind and time",
      does: "What the notification is about (Assigned, Conflicts, Review, Comments or Mention) and how long ago it happened.",
    },
    {
      id: "open-in-browser",
      shot: BELL,
      locate: { role: "button", nameRe: "^Open in Azure DevOps: " },
      name: "Open in Azure DevOps",
      does: "Opens the item in Azure DevOps in your browser.",
    },
    {
      id: "dismiss",
      shot: BELL,
      locate: { role: "button", nameRe: "^Dismiss: " },
      name: "Dismiss (x)",
      does: "Removes this one notification.",
    },
    {
      id: "notification-title",
      shot: BELL,
      locate: { role: "button", nameRe: "^Sam Doyle mentioned you on" },
      name: "Notification title",
      does: "Takes you to the work item or pull request inside the app, in Work Manager. When the app cannot open it there, it opens in your browser instead.",
    },

    // --- Command palette ----------------------------------------------------
    {
      id: "palette-search",
      shot: PALETTE,
      locate: { role: "combobox", name: "Type a command or search" },
      name: "Type a command or search",
      does:
        "Filters the list as you type. The letters only need to appear in order, so typing rn tst finds Run Tests. Pick a row with the arrow keys and [[Enter]], or click it.",
    },
    {
      id: "go-to",
      shot: PALETTE,
      locate: { role: "option", nameRe: "^Run Tests" },
      name: "Go to",
      does: "One row per screen, with its shortcut beside it ([[Ctrl+1]] for Manual Entry up to [[Ctrl+8]] for AI Bridge), and Settings at the end.",
    },
    {
      id: "toggle-work",
      shot: PALETTE,
      locate: { role: "option", nameRe: "^Toggle Work Manager" },
      name: "Toggle Work Manager",
      does: "Switches between the test case screens and Work Manager, the same as [[Ctrl+Shift+M]].",
    },
    {
      id: "toggle-theme",
      shot: PALETTE,
      locate: { role: "option", name: "Toggle theme" },
      name: "Toggle theme",
      does: "Switches between the light and the dark theme.",
    },
    {
      id: "check-updates",
      shot: PALETTE,
      locate: { role: "option", name: "Check for updates" },
      name: "Check for updates",
      does: "Asks whether a newer version is out. If one is, a bar at the top offers **Restart to update**; if not, a message says you are on the latest version.",
    },
    {
      id: "switch-project",
      shot: PALETTE,
      // The one project in the capture's organisation.
      locate: { role: "option", name: "Customer Portal" },
      name: "Switch project",
      does: "The projects in your organisation. Pick one to move to it.",
    },

    // --- Work Manager -------------------------------------------------------
    {
      id: "back-to-test-cases",
      shot: WORK,
      locate: { testId: "work" },
      name: "Test Case Manager",
      does: "In Work Manager the switch takes you back to the test case screens.",
    },
    {
      id: "nav-prs",
      shot: WORK,
      locate: { testId: "nav-prs" },
      name: "Pull Requests",
      does: "The project's pull requests, with their checks and comments.",
    },
    {
      id: "nav-board",
      shot: WORK,
      locate: { testId: "nav-board" },
      name: "Board",
      does: "The team's board. A red number on it counts work newly assigned to you; opening the board clears it.",
    },
    {
      id: "nav-create",
      shot: WORK,
      locate: { testId: "nav-create" },
      name: "New Work Item",
      does: "Creates a new work item in the project.",
    },
  ],
  tips: [
    "Keyboard: [[Ctrl+K]] opens the command palette, [[Ctrl+1]] to [[Ctrl+8]] open the test case screens, and [[Ctrl+Shift+M]] switches to Work Manager and back.",
    "The organisation, project, PBI and the screen you were on are remembered, so the app opens where you left it.",
    "If your session runs out, a window asks you to sign in again. What is already on screen stays readable in the meantime.",
    "Until a PBI is chosen, the test case screens show a short prompt and your recent PBIs instead of their usual content.",
    "Settings has an interface tour that walks you through the app one screen at a time.",
  ],
  howTo: [
    {
      title: "Sign in",
      steps: [
        "Open the app and press **Sign in with Microsoft**.",
        "Finish signing in in the browser window that opens. Until you do, the button reads **Waiting for browser**.",
        "Back in the app, choose your organisation, project and PBI in the bar across the top.",
      ],
    },
    {
      title: "Choose the PBI to work on",
      steps: [
        "Press the **x** on the PBI in the bar, or start from a screen with no PBI chosen.",
        "Click **Find PBI** and pick one of your recent PBIs, or type its number or part of its title.",
        "Click the PBI. Every test case screen now works on it.",
      ],
    },
  ],
};
