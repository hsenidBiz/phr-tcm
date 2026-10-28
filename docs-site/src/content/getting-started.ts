// Getting started: signing in and the parts of the app every screen shares -
// the sidebar, the context bar (the Product Backlog Item), the bell, the
// Work Manager switch and the command palette.
//
// Locates come from the real components: Sidebar.tsx (data-tour="nav-*",
// "Close sidebar"), ContextBar.tsx (data-tour="pbi"/"work", "Settings"),
// PbiPicker.tsx ("Clear PBI", "Find PBI"), NotificationBell.tsx and
// CommandPalette.tsx. The capture boots every shot with PBI #1001 selected
// (src/dev/demo.ts, seedCaptureScene), so a route that clears it cannot
// affect the next. It runs with one organization and Treat all projects as
// one on, so neither the Organization nor the Project picker renders: both
// are described in prose (the where-you-work group), not located.

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
    "Sign in once with your Microsoft work account, then choose the Product Backlog Item you are working on in the bar across the top. " +
    "Every test case screen works on that Product Backlog Item until you choose another, and the app remembers it the next time you open it.",
  shots: [
    { id: SHELL, route: [{ nav: "Manual Entry" }], alt: "The app with the sidebar on the left and the context bar across the top" },
    {
      id: PBI,
      route: [{ nav: "Manual Entry" }, { click: { role: "button", name: "Clear PBI" } }, { click: { role: "textbox", name: "Find PBI" } }],
      alt: "Choosing a Product Backlog Item: the search box with recently used Product Backlog Items",
    },
    {
      id: BELL,
      route: [{ nav: "Manual Entry" }, { click: { role: "button", nameRe: "^Notifications" } }],
      alt: "The notification panel opened from the bell",
    },
    {
      id: PALETTE,
      // Scrolled to the end, so the actions are in view with the screen
      // rows above them.
      route: [
        { nav: "Manual Entry" },
        { press: "Control+K" },
        { waitFor: { role: "dialog", name: "Command palette" } },
        { scrollTo: { role: "option", name: "Check for updates" } },
      ],
      alt: "The command palette",
    },
    {
      id: WORK,
      route: [{ nav: "Manual Entry" }, { click: { testId: "work" } }, { waitFor: { testId: "nav-board" } }],
      alt: "Work Manager, with its own sidebar",
    },
  ],
  groups: [
    { id: "move-around", title: "Move between screens", summary: "The sidebar lists every screen; the command palette reaches them from the keyboard." },
    {
      id: "where-you-work",
      title: "Pick the Product Backlog Item",
      summary:
        "The context bar across the top says which Product Backlog Item every screen works on. The Product Backlog Item can come from any project: with **Treat all projects as one** on in Settings, under General (it is on by default), every project shows together in one search and each Product Backlog Item brings its own project. " +
        "When your account has only one organisation it is chosen for you; with several, an **Organization** picker shows first, and choosing another organisation clears the Product Backlog Item. " +
        "Switch **Treat all projects as one** off and a **Project** picker appears beside it (unless the organisation has only one project, which is chosen for you); choosing another project then clears the Product Backlog Item.",
    },
    { id: "top-right", title: "Notifications, Work Manager and your account", summary: "The corner of the context bar: what changed, the switch to Work Manager, and who you are signed in as." },
    { id: "commands", title: "Run a command", summary: "The command palette finds any screen or action by name." },
  ],
  controls: [
    // --- The shell --------------------------------------------------------
    {
      id: "sidebar",
      shot: SHELL,
      group: "move-around",
      locate: { testId: "nav-manual" },
      name: "Sidebar",
      does:
        "Lists the test case screens under **Test Case Management**: Manual Entry, Import Test Cases, Update Test Cases, View Test Cases, Run Tests, Search Suites, Suite Management and AI Bridge. " +
        "With the matching modules switched on in Settings, a second group at the bottom, below them, lists Auto Run, Playwright and API Templates. " +
        "Click one to open it. The highlighted row is the screen you are on.",
      tips: ["[[Ctrl+1]] to [[Ctrl+8]] open the screens in the order they are listed, and [[Ctrl+9]] opens Auto Run when it is switched on. Playwright and API Templates have no shortcut."],
    },
    {
      id: "pbi",
      shot: SHELL,
      group: "where-you-work",
      locate: { testId: "pbi" },
      name: "Product Backlog Item",
      does:
        "The Product Backlog Item you are working on, with its number and title. It can come from any project. Manual Entry, Import Test Cases, Update Test Cases, View Test Cases and Run Tests all work on this Product Backlog Item, and test cases you create go to its own project.",
    },
    {
      id: "clear-pbi",
      shot: SHELL,
      group: "where-you-work",
      locate: { role: "button", name: "Clear PBI" },
      name: "Clear Product Backlog Item (x)",
      does: "Clears the Product Backlog Item so you can search for another one.",
    },
    {
      id: "bell",
      shot: SHELL,
      group: "top-right",
      locate: { role: "button", nameRe: "^Notifications" },
      name: "Notifications",
      does:
        "The red number counts what happened since you last looked: work assigned to you, pull requests with conflicts, reviews or comments, and mentions. With Treat all projects as one on, it covers every project of the organization. Click it to open the list.",
    },
    {
      id: "work-manager",
      shot: SHELL,
      group: "top-right",
      locate: { testId: "work" },
      name: "Work Manager",
      does: "Switches the app to Work Manager: pull requests, the board and new work items. The same button, then named Test Case Manager, brings you back.",
      tips: ["Hover it to see how many pull requests have conflicts or comments to resolve.", "[[Ctrl+Shift+M]] switches too."],
    },
    {
      id: "account",
      shot: SHELL,
      group: "top-right",
      locate: { text: "Alex Tester" },
      name: "Your account",
      does: "The account you are signed in with. It is shown when the window is wide enough.",
    },
    {
      id: "settings",
      shot: SHELL,
      group: "top-right",
      locate: { role: "button", name: "Settings" },
      name: "Settings",
      does: "Opens Settings. Click it again to go back to the screen you came from.",
    },
    {
      id: "palette-hint",
      shot: SHELL,
      group: "move-around",
      locate: { text: "Ctrl+K for commands" },
      name: "Ctrl+K for commands",
      does: "A reminder that [[Ctrl+K]] opens the command palette from anywhere.",
    },

    // --- Choosing a PBI ---------------------------------------------------
    {
      id: "find-pbi",
      shot: PBI,
      group: "where-you-work",
      locate: { role: "textbox", name: "Find PBI" },
      name: "Find Product Backlog Item",
      does:
        "Type a Product Backlog Item number or words from its title. Matching Product Backlog Items appear as you type; [[Enter]] searches at once. With **Treat all projects as one** on, every project is searched and a result names its project when the results come from more than one. Click a result to choose it.",
    },
    {
      id: "recently-used",
      shot: PBI,
      group: "where-you-work",
      locate: { text: "Recently used" },
      name: "Recently used",
      does: "Before you type, the Product Backlog Items you chose most recently (in this project, when Treat all projects as one is off). Click one to choose it again; the x beside it removes it from the list.",
    },
    {
      id: "recent-pbis",
      shot: PBI,
      group: "where-you-work",
      locate: { text: "Recent PBIs" },
      name: "Recent Product Backlog Items",
      does: "While no Product Backlog Item is chosen, the screen offers the same recent Product Backlog Items as cards. Click one to choose it.",
    },

    // --- Notifications ----------------------------------------------------
    {
      id: "notification-panel",
      shot: BELL,
      group: "top-right",
      locate: { role: "dialog", name: "Notifications" },
      name: "Notification list",
      does:
        "Everything that needs you, newest first. Opening the list marks it all as read; items stay until you dismiss them. Click anywhere else or press [[Esc]] to close it.",
    },
    {
      id: "clear-all",
      shot: BELL,
      group: "top-right",
      locate: { role: "button", name: "Clear all" },
      name: "Clear all",
      does: "Removes every notification from the list.",
    },
    {
      id: "kind",
      shot: BELL,
      group: "top-right",
      locate: { text: "Mention" },
      name: "Kind and time",
      does: "What the notification is about (Assigned, Conflicts, Review, Comments or Mention) and how long ago it happened.",
    },
    {
      id: "open-in-browser",
      shot: BELL,
      group: "top-right",
      locate: { role: "button", nameRe: "^Open in Azure DevOps: " },
      name: "Open in Azure DevOps",
      does: "Opens the item in Azure DevOps in your browser.",
    },
    {
      id: "dismiss",
      shot: BELL,
      group: "top-right",
      locate: { role: "button", nameRe: "^Dismiss: " },
      name: "Dismiss (x)",
      does: "Removes this one notification.",
    },
    {
      id: "notification-title",
      shot: BELL,
      group: "top-right",
      locate: { role: "button", nameRe: "^Sam Doyle mentioned you on" },
      name: "Notification title",
      does: "Takes you to the work item or pull request inside the app, in Work Manager, in its own project. When the app cannot open it there, it opens in your browser instead.",
    },

    // --- Command palette ----------------------------------------------------
    {
      id: "palette-search",
      shot: PALETTE,
      group: "commands",
      locate: { role: "combobox", name: "Type a command or search" },
      name: "Type a command or search",
      does:
        "Filters the list as you type. The letters only need to appear in order, so typing rn tst finds Run Tests. Pick a row with the arrow keys and [[Enter]], or click it.",
    },
    {
      id: "go-to",
      shot: PALETTE,
      group: "commands",
      locate: { role: "option", nameRe: "^Run Tests" },
      name: "Go to",
      does:
        "One row per screen, with its shortcut beside it ([[Ctrl+1]] for Manual Entry up to [[Ctrl+8]] for AI Bridge, or [[Ctrl+9]] with Auto Run switched on), and Settings at the end.",
    },
    {
      id: "toggle-work",
      shot: PALETTE,
      group: "commands",
      locate: { role: "option", nameRe: "^Toggle Work Manager" },
      name: "Toggle Work Manager",
      does: "Switches between the test case screens and Work Manager, the same as [[Ctrl+Shift+M]].",
    },
    {
      id: "toggle-theme",
      shot: PALETTE,
      group: "commands",
      locate: { role: "option", name: "Toggle theme" },
      name: "Toggle theme",
      does: "Switches between the light and the dark theme.",
    },
    {
      id: "check-updates",
      shot: PALETTE,
      group: "commands",
      locate: { role: "option", name: "Check for updates" },
      name: "Check for updates",
      does: "Asks whether a newer version is out. If one is, a bar at the top offers **Restart to update**; if not, a message says you are on the latest version.",
    },

    // --- Work Manager -------------------------------------------------------
    {
      id: "back-to-test-cases",
      shot: WORK,
      group: "top-right",
      locate: { testId: "work" },
      name: "Test Case Manager",
      does: "In Work Manager the switch takes you back to the test case screens.",
    },
    {
      id: "nav-prs",
      shot: WORK,
      group: "top-right",
      locate: { testId: "nav-prs" },
      name: "Pull Requests",
      does: "The pull requests of every project (of the project picked in the bar, when Treat all projects as one is off), with their checks and comments.",
    },
    {
      id: "nav-board",
      shot: WORK,
      group: "top-right",
      locate: { testId: "nav-board" },
      name: "Board",
      does: "The team's board. A red number on it counts work newly assigned to you; opening the board clears it.",
    },
    {
      id: "nav-create",
      shot: WORK,
      group: "top-right",
      locate: { testId: "nav-create" },
      name: "New Work Item",
      does: "Creates a new work item in the project.",
    },
  ],
  tips: [
    "The button at the bottom of the sidebar folds it to a strip of icons for more room; hover an icon to see its name, and the same button opens it again.",
    "Keyboard: [[Ctrl+K]] opens the command palette, [[Ctrl+1]] to [[Ctrl+8]] open the test case screens ([[Ctrl+9]] with Auto Run switched on), and [[Ctrl+Shift+M]] switches to Work Manager and back.",
    "The Product Backlog Item and the screen you were on are remembered, so the app opens where you left it.",
    "With **Treat all projects as one** switched off, the palette also lists your projects under **Switch project**; pick one to move to it. With it on there is no project to switch to.",
    "On a fresh install with **Treat all projects as one** on, the app sets your home project for you, from the only project, the one that already has Auto Run files, or the first Product Backlog Item you pick (a message says so when the organisation has several projects). You can change it in Settings.",
    "If your session runs out, a window asks you to sign in again. What is already on screen stays readable in the meantime.",
    "Until a Product Backlog Item is chosen, the test case screens show a short prompt and your recent Product Backlog Items instead of their usual content.",
    "Settings has an interface tour that walks you through the app one screen at a time.",
  ],
  howTo: [
    {
      title: "Sign in",
      steps: [
        "Open the app and press **Sign in with Microsoft**.",
        "Finish signing in in the browser window that opens. Until you do, the button reads **Waiting for browser**.",
        "Back in the app, choose your Product Backlog Item in the bar across the top.",
      ],
    },
    {
      title: "Choose the Product Backlog Item to work on",
      steps: [
        "Press the **x** on the Product Backlog Item in the bar, or start from a screen with no Product Backlog Item chosen.",
        "Click **Find Product Backlog Item** and pick one of your recent Product Backlog Items, or type its number or part of its title.",
        "Click the Product Backlog Item. Every test case screen now works on it.",
      ],
    },
  ],
};
