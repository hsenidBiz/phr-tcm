// Settings: cards of settings on the left (appearance, general, AI tools,
// updates, backup, help and support) and the changelog and log on the right.
//
// Locates come from screens/Settings.tsx and lib/adoRate.ts. Settings opens
// from the gear in the context bar, not the sidebar. No route clicks a
// theme, accent or request rate: those are saved at once and would carry
// into the shots after them.

import type { Screen, Step } from "../types";

const MAIN = "settings-main";
const MORE = "settings-more";
const LOGS = "settings-logs";
const BUG = "settings-report-bug";
const ADVANCED = "settings-advanced";

const OPEN: Step = { click: { role: "button", name: "Settings" } };

export const settings: Screen = {
  id: "settings",
  title: "Settings",
  group: "Settings",
  summary:
    "Open Settings with the gear at the top right; press it again to go back. Everything here is saved on this computer as soon as you change it.",
  shots: [
    { id: MAIN, route: [OPEN], alt: "Settings: Appearance, General and AI tools on the left; the changelog with Updates, Backup and Help under it on the right" },
    {
      id: MORE,
      route: [OPEN, { scrollTo: { role: "button", name: "Show UI tour" } }],
      alt: "Settings scrolled down to the AI tools, Updates, Backup and Help cards",
    },
    {
      id: ADVANCED,
      route: [OPEN, { scrollTo: { role: "switch", name: "Enable Advanced Features" } }],
      alt: "The end of the General card, with Enable Advanced Features turned on, and the AI tools card under it",
    },
    // From the top of Settings: the log is shorter than the changelog, so a page
    // that arrived scrolled snaps up when it opens, and the two theme passes
    // caught it in different places.
    {
      id: LOGS,
      route: [OPEN, { scrollTo: { role: "heading", name: "Appearance" } }, { click: { role: "button", name: "Logs" } }, { waitFor: { role: "button", name: "Copy log" } }],
      alt: "The app log in place of the changelog",
    },
    {
      id: BUG,
      route: [OPEN, { click: { role: "button", name: "Report a bug" } }, { waitFor: { role: "textbox", name: "Bug title" } }],
      alt: "The Report a bug window",
    },
  ],
  groups: [
    { id: "appearance", title: "Appearance", summary: "The theme and the accent colour." },
    {
      id: "general",
      title: "General",
      summary: "Running in the tray, starting with Windows, how fast the app may call Azure DevOps, and the advanced features.",
    },
    { id: "ai-tools", title: "AI tools", summary: "Where AI tools may be registered." },
    { id: "updates", title: "Updates", summary: "The version you have, beta builds, and checking for a new version." },
    { id: "backup", title: "Backup and transfer", summary: "Move your settings to another computer." },
    { id: "help", title: "Help and support", summary: "This guide, the interface tour, and reporting a problem with the app." },
    { id: "changelog-logs", title: "Changelog and logs", summary: "What changed in each version, and the app's own log." },
  ],
  controls: [
    // --- Appearance and request rate ------------------------------------------
    {
      id: "theme",
      shot: MAIN,
      group: "appearance",
      locate: { role: "button", name: "Theme Light" },
      name: "Theme",
      does: "Changes the whole app's colours: Light, Slate, Midnight, Graphite, Ocean or OLED.",
    },
    {
      id: "theme-system",
      shot: MAIN,
      group: "appearance",
      locate: { role: "button", name: "Theme System" },
      name: "System",
      does: "Follows Windows: light when Windows is light, dark when it is dark.",
    },
    {
      id: "accent",
      shot: MAIN,
      group: "appearance",
      locate: { role: "button", name: "Accent Theme default" },
      name: "Accent",
      does: "The highlight colour. The first swatch keeps the theme's own; the others (green, blue, violet, amber, rose) replace it in any theme.",
    },
    {
      id: "request-rate",
      shot: MAIN,
      group: "general",
      locate: { role: "button", nameRe: "^Full speed" },
      name: "Azure DevOps request rate",
      does:
        "How quickly the app sends requests: **Full speed**, **Balanced** or **Gentle**, with the chosen one explained underneath. Azure DevOps limits requests per person, and your browser uses the same allowance. " +
        "**Full speed** is the default; choose **Balanced** if Azure DevOps warns you about usage, or **Gentle** while you are working in Azure DevOps too.",
    },
    {
      id: "advanced-features",
      shot: ADVANCED,
      group: "general",
      locate: { role: "switch", name: "Enable Advanced Features" },
      name: "Enable Advanced Features",
      does:
        "Off by default. On: the sidebar adds **Auto Run**, under Run Tests, and **API Templates**, at the end, and the AI Bridge tab adds the environments they work in and the AI tools that go with them. " +
        "Off again: they are hidden, and your scripts, runs and templates are kept on this computer for when you turn it back on.",
      tips: ["With the advanced features on, the sidebar's shortcuts run from [[Ctrl+1]] to [[Ctrl+9]]: Auto Run takes [[Ctrl+6]] and the screens after it move down one."],
    },

    // --- Changelog, backup, updates -------------------------------------------
    {
      id: "report-bug",
      shot: MORE,
      group: "help",
      locate: { role: "button", name: "Report a bug" },
      name: "Report a bug",
      does: "Opens a window for reporting a problem with this app (see below). Problems in the product you test are filed from the runner instead.",
    },
    {
      id: "changelog",
      shot: MAIN,
      group: "changelog-logs",
      locate: { role: "button", name: "Changelog" },
      name: "Changelog",
      does: "Shows what changed in the latest version of the app.",
    },
    {
      id: "logs",
      shot: MAIN,
      group: "changelog-logs",
      locate: { role: "button", name: "Logs" },
      name: "Logs",
      does: "Shows the app's own log in the same place, for when something needs reporting.",
    },
    {
      id: "show-more",
      shot: MAIN,
      group: "changelog-logs",
      locate: { role: "button", nameRe: "^Show more \\(" },
      name: "Show more",
      does: "Opens the notes of every earlier version, in a box of their own. **Show less** folds them away again.",
    },
    {
      id: "export-backup",
      shot: MORE,
      group: "backup",
      locate: { role: "button", name: "Export to file" },
      name: "Export to file",
      does:
        "Saves your settings and local data (theme, default tags, drafts, cached lists) to one file, for moving to another computer. " +
        "Your Microsoft sign-in and the database logins are never included.",
    },
    {
      id: "import-backup",
      shot: MORE,
      group: "backup",
      locate: { role: "button", name: "Import from file" },
      name: "Import from file",
      does:
        "Picks a backup file, then asks before going on: **Import and reload** replaces this computer's settings and local data with the backup's and reloads the app. " +
        "Anything changed here since the backup was made is overwritten.",
    },
    {
      id: "check-updates",
      shot: MORE,
      group: "updates",
      locate: { role: "button", name: "Check for updates" },
      name: "Check for updates",
      does:
        "Asks whether a newer version is out. The version you have is shown beside it. The app also checks by itself when it starts and every hour; when a version is ready, a bar at the top offers **Restart to update**.",
    },
    {
      id: "beta-builds",
      shot: MORE,
      group: "updates",
      locate: { role: "switch", name: "Download beta builds" },
      name: "Download beta builds",
      does:
        "Off by default. On: the app also installs beta builds, which bring new features sooner. Turn it off and a beta build stays until the next stable release arrives.",
    },

    // --- How To Use, tour, AI tools -------------------------------------------------
    {
      id: "how-to-use",
      shot: MORE,
      group: "help",
      locate: { role: "button", name: "How To Use" },
      name: "How To Use",
      does: "Opens this guide in your browser. The first time, it downloads the guide (a one-off download - the button shows its size) and then opens it. After that it opens straight away, works offline, and stays when the app updates. When an app update brings a newer guide, an Update Guide button appears beside it.",
    },
    {
      id: "show-tour",
      shot: MORE,
      group: "help",
      locate: { role: "button", name: "Show UI tour" },
      name: "Show UI tour",
      does: "Replays the walkthrough that highlights each area of the app.",
    },
    {
      id: "allow-machine-wide",
      shot: ADVANCED,
      group: "ai-tools",
      locate: { role: "switch", name: "Allow registering AI tools machine-wide" },
      name: "Allow registering AI tools machine-wide",
      does:
        "For a computer that does not work from a repository: the AI Bridge tab then offers **Register in: This repository** or **Machine-wide**. Writing test cases still needs a repository.",
    },

    // --- Background ---------------------------------------------------------------
    {
      id: "close-to-tray",
      shot: MAIN,
      group: "general",
      locate: { role: "switch", name: "Keep running in the tray when closed" },
      name: "Keep running in the tray when closed",
      does:
        "On by default. Closing the window leaves the app running in the notification area (the ^ on the taskbar), so your AI assistant's tools stay available. Click the icon to open the window again; right-click it and choose Quit to close the app. Off: closing the window closes the app.",
    },
    {
      id: "start-with-windows",
      shot: MAIN,
      group: "general",
      locate: { role: "switch", name: "Start with Windows" },
      name: "Start with Windows",
      does: "Starts the app when you sign in to Windows. **Start minimized** below decides whether it opens its window or waits in the notification area.",
    },
    {
      id: "start-minimized",
      shot: MAIN,
      group: "general",
      locate: { role: "switch", name: "Start minimized" },
      name: "Start minimized",
      does:
        "On by default, and only available while **Start with Windows** is on. On: the app starts at sign-in in the notification area (the ^ on the taskbar), without opening its window. Off: the window opens as usual.",
    },

    // --- The app log ------------------------------------------------------------------
    {
      id: "log",
      shot: LOGS,
      group: "changelog-logs",
      locate: { role: "heading", name: "App log" },
      name: "App log",
      does: "What the app has been doing, refreshed every few seconds while it is open. Include it when you report a bug. A file is kept for each day, for a week.",
    },
    {
      id: "copy-log",
      shot: LOGS,
      group: "changelog-logs",
      locate: { role: "button", name: "Copy log" },
      name: "Copy log",
      does: "Copies the whole log shown, to paste into a message.",
    },
    {
      id: "open-log-folder",
      shot: LOGS,
      group: "changelog-logs",
      locate: { role: "button", name: "Open log folder" },
      name: "Open log folder",
      does: "Opens the folder with the daily log files.",
    },

    // --- Report a bug -----------------------------------------------------------------
    {
      id: "bug-title",
      shot: BUG,
      group: "changelog-logs",
      locate: { role: "textbox", name: "Bug title" },
      name: "Bug title",
      does: "One line for the issue. Leave it blank to take it from the description.",
    },
    {
      id: "what-happened",
      shot: BUG,
      group: "changelog-logs",
      locate: { role: "textbox", name: "What happened" },
      name: "What happened",
      does: "What you were doing, and what happened instead.",
    },
    {
      id: "bug-cancel",
      shot: BUG,
      group: "changelog-logs",
      locate: { role: "button", name: "Cancel" },
      name: "Cancel",
      does: "Closes the window without reporting anything.",
    },
    {
      id: "open-issue",
      shot: BUG,
      group: "changelog-logs",
      locate: { role: "button", name: "Open the issue" },
      name: "Open the issue",
      does:
        "Opens a filled-in issue on GitHub in your browser, for you to check and submit, and opens the folder with a copy of the log. Drag that log file onto the issue before submitting. " +
        "Nothing is sent from the app, and your organisation, project and work item names are removed from the log first.",
    },
  ],
  tips: [
    "Settings can also be opened from the command palette: press [[Ctrl+K]] and pick **Settings**.",
  ],
};
