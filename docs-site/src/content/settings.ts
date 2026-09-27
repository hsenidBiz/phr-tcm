// Settings: the look of the app, how fast it talks to Azure DevOps, this
// guide, the interface tour, the AI tool options, the changelog and log,
// bug reports, backup and updates.
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

const OPEN: Step = { click: { role: "button", name: "Settings" } };

export const settings: Screen = {
  id: "settings",
  title: "Settings",
  group: "Settings",
  summary:
    "Open Settings with the gear at the top right; press it again to go back. Everything here is saved on this computer as soon as you change it.",
  shots: [
    { id: MAIN, route: [OPEN], alt: "Settings: appearance and request rate on the left, the changelog, backup and updates on the right" },
    {
      id: MORE,
      route: [OPEN, { scrollTo: { role: "switch", name: "Start with Windows" } }],
      alt: "Settings scrolled down to How To Use, the interface tour, AI tools and Background",
    },
    { id: LOGS, route: [OPEN, { click: { role: "button", name: "Logs" } }, { waitFor: { role: "button", name: "Copy log" } }], alt: "The app log in place of the changelog" },
    {
      id: BUG,
      route: [OPEN, { click: { role: "button", name: "Report a bug" } }, { waitFor: { role: "textbox", name: "Bug title" } }],
      alt: "The Report a bug window",
    },
  ],
  groups: [
    { id: "appearance", title: "Appearance", summary: "The theme and the accent colour." },
    { id: "rate", title: "Azure DevOps request rate", summary: "How fast the app may call Azure DevOps." },
    { id: "changelog-logs", title: "Changelog, logs and bug reports", summary: "What changed in each version, the app's own log, and a way to report a problem." },
    { id: "backup-updates", title: "Backup and updates", summary: "Move your settings to another computer, and check for a new version." },
    { id: "help-and-options", title: "Help, the tour and AI tools options", summary: "Further down: this guide, the interface tour, and a switch for AI Bridge." },
    { id: "background", title: "Running in the background", summary: "Keep the app running in the tray when its window is closed, and start it with Windows." },
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
      group: "rate",
      locate: { role: "button", nameRe: "^Full speed" },
      name: "Azure DevOps request rate",
      does:
        "How quickly the app sends requests. Azure DevOps limits requests per person, and your browser uses the same allowance. **Full speed** is the default; " +
        "choose **Balanced** if Azure DevOps warns you about usage, or **Gentle** while you are working in Azure DevOps too.",
    },

    // --- Changelog, backup, updates -------------------------------------------
    {
      id: "report-bug",
      shot: MAIN,
      group: "changelog-logs",
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
      shot: MAIN,
      group: "backup-updates",
      locate: { role: "button", name: "Export to file" },
      name: "Export to file",
      does:
        "Saves your settings and local data (theme, default tags, drafts, cached lists) to one file, for moving to another computer. " +
        "Your Microsoft sign-in and the database logins are never included.",
    },
    {
      id: "import-backup",
      shot: MAIN,
      group: "backup-updates",
      locate: { role: "button", name: "Import from file" },
      name: "Import from file",
      does:
        "Picks a backup file, then asks before going on: **Import and reload** replaces this computer's settings and local data with the backup's and reloads the app. " +
        "Anything changed here since the backup was made is overwritten.",
    },
    {
      id: "check-updates",
      shot: MAIN,
      group: "backup-updates",
      locate: { role: "button", name: "Check for updates" },
      name: "Check for updates",
      does:
        "Asks whether a newer version is out. The version you have is shown above it. The app also checks by itself when it starts and every hour; when a version is ready, a bar at the top offers **Restart to update**.",
    },
    {
      id: "beta-builds",
      shot: MAIN,
      group: "backup-updates",
      locate: { role: "switch", name: "Download beta builds" },
      name: "Download beta builds",
      does:
        "Off by default. On: the app also installs beta builds, which bring new features sooner. Turn it off and a beta build stays until the next stable release arrives.",
    },

    // --- How To Use, tour, AI tools -------------------------------------------------
    {
      id: "how-to-use",
      shot: MORE,
      group: "help-and-options",
      locate: { role: "button", name: "How To Use" },
      name: "How To Use",
      does: "Opens this guide in your browser. It comes with the app, so it works offline and always matches the version you have.",
    },
    {
      id: "show-tour",
      shot: MORE,
      group: "help-and-options",
      locate: { role: "button", name: "Show UI tour" },
      name: "Show UI tour",
      does: "Replays the walkthrough that highlights each area of the app.",
    },
    {
      id: "allow-machine-wide",
      shot: MORE,
      group: "help-and-options",
      locate: { role: "switch", name: "Allow registering AI tools machine-wide" },
      name: "Allow registering AI tools machine-wide",
      does:
        "For a computer that does not work from a repository: the AI Bridge tab then offers **Register in: This repository** or **Machine-wide**. Writing test cases still needs a repository.",
    },

    // --- Background ---------------------------------------------------------------
    {
      id: "close-to-tray",
      shot: MORE,
      group: "background",
      locate: { role: "switch", name: "Keep running in the tray when closed" },
      name: "Keep running in the tray when closed",
      does:
        "On by default. Closing the window leaves the app running in the notification area (the ^ on the taskbar), so your AI assistant's tools stay available. Click the icon to open the window again; right-click it and choose Quit to close the app. Off: closing the window closes the app.",
    },
    {
      id: "start-with-windows",
      shot: MORE,
      group: "background",
      locate: { role: "switch", name: "Start with Windows" },
      name: "Start with Windows",
      does: "Starts the app in the notification area when you sign in to Windows, without opening its window.",
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
