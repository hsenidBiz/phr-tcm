// AI Bridge: connecting AI coding assistants to this app - the working
// repository, which assistants have its tools, which tools they may use,
// and the company database those tools read.
//
// Locates come from screens/AiBridge.tsx, components/DbCredentialsModal.tsx
// and components/BridgeStatusBadge.tsx. In capture mode the sample data
// seeds one working repository, two detected tools (one registered, one
// with a machine-wide copy left over) and two databases with the dev one
// chosen (src/dev/demo.ts). The options shot turns on the two Settings
// switches that change this tab; every capture boot turns them off again.

import type { Screen, Step } from "../types";

const TAB = "ai-bridge-tab";
const OTHER = "ai-bridge-other-tools";
const SWITCHED_OFF = "ai-bridge-switched-off";
const CREDENTIALS = "ai-bridge-credentials";
const OPTIONS = "ai-bridge-options";

const NAV: Step = { nav: "AI Bridge" };
const REPO = "C:\\Projects\\customer-portal";

export const aiBridge: Screen = {
  id: "ai-bridge",
  title: "AI Bridge",
  group: "AI tools",
  summary:
    "Connect an AI coding assistant, such as Claude Code or VS Code, to this app. The assistant can then read your test cases, suites, " +
    "run results, tags, PBIs and wiki pages, and check the drafts it writes, while you stay in charge of what reaches Azure DevOps: " +
    "none of these tools can write to it. The tools are registered per working repository; the other settings here apply to the whole app.",
  shots: [
    { id: TAB, route: [NAV, { waitFor: { role: "button", name: "Unregister" } }], alt: "The AI Bridge tab with a working repository, the AI tools and the company database" },
    {
      id: OTHER,
      route: [NAV, { click: { text: "Other tools" } }, { waitFor: { role: "button", name: "Copy command" } }],
      alt: "Other tools opened, with the command line and the config to copy",
    },
    {
      id: SWITCHED_OFF,
      route: [
        NAV,
        { click: { role: "switch", name: "Run failures" } },
        { scrollTo: { role: "button", name: "Turn all back on" } },
      ],
      alt: "One tool switched off, with Turn all back on under the list",
    },
    {
      id: CREDENTIALS,
      route: [NAV, { click: { role: "button", name: "Manage credentials" } }, { waitFor: { role: "button", name: "Test connection" } }],
      alt: "The credentials window of the chosen database",
    },
    {
      id: OPTIONS,
      route: [
        { click: { role: "button", name: "Settings" } },
        { click: { role: "switch", name: "Allow registering AI tools machine-wide" } },
        { click: { role: "switch", name: "Offer the PHR X database server on the AI Bridge tab" } },
        NAV,
        { waitFor: { role: "textbox", name: "Database server path" } },
      ],
      alt: "AI Bridge with the machine-wide choice and the PHR X server settings switched on in Settings",
    },
  ],
  controls: [
    // --- The tab --------------------------------------------------------------
    {
      id: "status",
      shot: TAB,
      locate: { role: "status", nameRe: "^Bridge listening" },
      name: "Running",
      does:
        "Beside the title: whether the bridge the assistants connect to is up. It runs only while this app is open and you are signed in; " +
        "when it is not, it reads **Not running**. Hover it to see the port it listens on.",
    },
    {
      id: "repository",
      shot: TAB,
      locate: { role: "radio", name: `Use ${REPO}` },
      name: "Working repository",
      does:
        "The repositories your test cases belong to. The dot marks the current one: files written or imported for it go to its **.test-cases** folder, and the AI tools register into it. Click another dot to make that one current.",
      tips: ["Until a repository is added, this card is all the tab shows."],
    },
    {
      id: "repository-switch",
      shot: TAB,
      locate: { role: "switch", name: `AI tools for ${REPO}` },
      name: "AI tools switch",
      does: "Turns the AI tools on or off for that repository. Switching off the current one turns them off without forgetting the folder.",
    },
    {
      id: "remove-repository",
      shot: TAB,
      locate: { role: "button", name: `Remove ${REPO}` },
      name: "Remove (x)",
      does: "Takes the repository off the list. Nothing in the folder is changed.",
    },
    {
      id: "add-repository",
      shot: TAB,
      locate: { role: "button", name: "Add repository" },
      name: "Add repository",
      does: "Opens a folder picker. The folder you pick is added to the list and becomes the current repository.",
    },
    {
      id: "rescan",
      shot: TAB,
      locate: { role: "button", name: "Rescan" },
      name: "Rescan",
      does: "Looks again for the AI tools installed on this computer and says how many it found. Use it after installing one, without restarting the app.",
    },
    {
      id: "registered",
      shot: TAB,
      locate: { text: "Registered ✓" },
      name: "Installed AI tools",
      does:
        "Each AI tool found on this computer, with where its settings are read from (**in this repo**, or **global** for machine-wide) and whether this app's tools are registered with it.",
    },
    {
      id: "unregister",
      shot: TAB,
      locate: { role: "button", name: "Unregister" },
      name: "Unregister",
      does: "Removes this app's tools from that AI tool's settings.",
    },
    {
      id: "register",
      shot: TAB,
      locate: { role: "button", name: "Register" },
      name: "Register",
      does:
        "Adds this app's tools to that AI tool's settings, in the current repository (or machine-wide, when you chose that). An assistant that is already running may need its session restarted to see them.",
    },
    {
      id: "retire-global",
      shot: TAB,
      locate: { role: "button", name: "Retire global copies" },
      name: "Retire global copies",
      does:
        "Shown under a tool that still has this app's tools registered machine-wide from before. A machine-wide copy can hide the repository's own in most assistants; this removes it.",
    },
    {
      id: "other-tools",
      shot: TAB,
      locate: { text: "Other tools" },
      name: "Other tools",
      does: "For an assistant the app does not detect: opens a command line and a settings snippet you can copy and add yourself.",
    },
    {
      id: "tool-count",
      shot: TAB,
      locate: { text: "6 of 6 on" },
      name: "Tools an assistant may use",
      does:
        "How many of the tools below are switched on. The tools every writing job needs (the writing guide, reading cases, checking a draft and similar) are always on and not listed.",
    },
    {
      id: "tool-switch",
      shot: TAB,
      locate: { role: "switch", name: "Test Suites" },
      name: "Tool switch",
      does:
        "Switch a tool off to keep it out of an assistant's reach: Test Suites, Run failures, Company database (read), Project tags, Find a PBI and Project wiki. A connected assistant sees the change without being restarted.",
    },
    {
      id: "database",
      shot: TAB,
      locate: { role: "combobox", name: "Database" },
      name: "Database",
      does:
        "The company database this app's own database tools read, when **Company database (read)** is switched on. The x clears the choice.",
    },
    {
      id: "signs-in-as",
      shot: TAB,
      locate: { text: "Signs in as portal_devlogin" },
      name: "Signs in as",
      does: "The user the chosen database signs in as, or **No login saved**. The password is never shown.",
    },
    {
      id: "manage-credentials",
      shot: TAB,
      locate: { role: "button", name: "Manage credentials" },
      name: "Manage credentials",
      does: "Opens the login of the chosen database. Greyed out until a database is chosen.",
    },
    {
      id: "writes",
      shot: TAB,
      locate: { role: "switch", name: "Create, update and delete" },
      name: "Create, update and delete",
      does:
        "Lets an assistant change data in the chosen database, not only read it. It is off until you turn it on, it can only be turned on for a dev login database (a user ending in _devlogin), and every statement is written to the app's log.",
    },
    {
      id: "forget",
      shot: TAB,
      locate: { role: "button", name: "Forget them" },
      name: "Forget them",
      does:
        "Removes the database logins saved on this computer (they are kept in Windows Credential Manager), clears the card's settings and the chosen database, and turns **Create, update and delete** off.",
    },
    {
      id: "breakdown",
      shot: TAB,
      locate: { role: "heading", name: "AI Tools Breakdown" },
      name: "AI Tools Breakdown",
      does: "What each tool you can switch does, and the recommended way to have an assistant write test cases for a PBI.",
    },

    // --- Other tools ------------------------------------------------------------
    {
      id: "copy-command",
      shot: OTHER,
      locate: { role: "button", name: "Copy command" },
      name: "Copy (command line)",
      does: "Copies the command that registers this app's tools with Claude Code. Run it inside the repository.",
    },
    {
      id: "copy-config",
      shot: OTHER,
      locate: { role: "button", name: "Copy config" },
      name: "Copy (config)",
      does: "Copies the settings snippet to paste into another assistant's tool settings.",
    },

    // --- A tool switched off ------------------------------------------------------
    {
      id: "switched-off",
      shot: SWITCHED_OFF,
      locate: { role: "switch", name: "Run failures" },
      name: "A switched-off tool",
      does: "Its name is greyed out and the count above drops by one.",
    },
    {
      id: "turn-all-on",
      shot: SWITCHED_OFF,
      locate: { role: "button", name: "Turn all back on" },
      name: "Turn all back on",
      does: "Switches every tool on again. It shows while any tool is off.",
    },

    // --- Credentials --------------------------------------------------------------
    {
      id: "credentials",
      shot: CREDENTIALS,
      locate: { role: "dialog", name: "Credentials for Dev - dev login" },
      name: "Credentials window",
      does:
        "For the databases that come with the app, the server and database are shown and cannot be changed. For any other database you can edit the server, port and database, and choose whether to trust the server certificate.",
    },
    {
      id: "user",
      shot: CREDENTIALS,
      locate: { role: "textbox", name: "User" },
      name: "User",
      does: "Who the database signs in as.",
    },
    {
      id: "password",
      shot: CREDENTIALS,
      locate: { role: "textbox", name: "Password" },
      name: "Password",
      does: "Type a new password, or leave it blank to keep the saved one. A saved password is never shown back.",
    },
    {
      id: "test-connection",
      shot: CREDENTIALS,
      locate: { role: "button", name: "Test connection" },
      name: "Test connection",
      does: "Tries to sign in with what is in the form, without saving it, and shows the result under the fields.",
    },
    {
      id: "reset-default",
      shot: CREDENTIALS,
      locate: { role: "button", name: "Reset to default" },
      name: "Reset to default",
      does: "Puts back the login the database came with. It shows only on a database that comes with the app, once its login has been changed.",
    },
    {
      id: "credentials-cancel",
      shot: CREDENTIALS,
      locate: { role: "button", name: "Cancel" },
      name: "Cancel",
      does: "Closes the window without saving.",
    },
    {
      id: "credentials-save",
      shot: CREDENTIALS,
      locate: { role: "button", name: "Save" },
      name: "Save",
      does: "Saves the login in Windows Credential Manager on this computer and closes the window.",
    },

    // --- Options from Settings ------------------------------------------------------
    {
      id: "register-in-repository",
      shot: OPTIONS,
      locate: { role: "button", name: "This repository" },
      name: "Register in: This repository",
      does: "Registers the AI tools into the current repository. The choice shows only when **Allow registering AI tools machine-wide** is on in Settings.",
    },
    {
      id: "register-machine-wide",
      shot: OPTIONS,
      locate: { role: "button", name: "Machine-wide" },
      name: "Register in: Machine-wide",
      does: "Registers the AI tools for the whole computer instead, for a machine that does not work from a repository. Writing test cases still needs a repository.",
    },
    {
      id: "server-path",
      shot: OPTIONS,
      locate: { role: "textbox", name: "Database server path" },
      name: "Server path",
      does:
        "Shown when **Offer the PHR X database server on the AI Bridge tab** is on in Settings: registers the company's own database server beside this app's tools. Type or pick where its PeoplesHR.DBMCPServer.exe is. It is no longer needed for looking things up.",
    },
    {
      id: "server-file",
      shot: OPTIONS,
      locate: { role: "button", name: "File" },
      name: "File",
      does: "Picks the server's file.",
    },
    {
      id: "server-folder",
      shot: OPTIONS,
      locate: { role: "button", name: "Folder" },
      name: "Folder",
      does: "Picks the folder the server is in instead.",
    },
    {
      id: "db-type",
      shot: OPTIONS,
      locate: { role: "combobox", name: "Database type" },
      name: "DB_TYPE",
      does: "The kind of database the server talks to: mssql or sqlserver.",
    },
    {
      id: "schema-filter",
      shot: OPTIONS,
      locate: { role: "textbox", name: "Schema filter" },
      name: "SCHEMA_FILTER",
      does: "Which schemas the server reads, separated by commas. Leave it blank for the server's default.",
    },
    {
      id: "phrx-register",
      shot: OPTIONS,
      locate: { text: "Choose a database and fill in the server path to enable registration." },
      name: "Register the server",
      does:
        "Once a database is chosen and the server path filled in, each AI tool gets a **Register** button here. Registering writes the chosen database's login into that tool's settings file; **Unregister** takes it out again.",
    },
  ],
  tips: [
    "None of the tools an assistant gets through this app can create, update or delete anything in Azure DevOps. You import what it writes yourself, on Import File.",
    "The AI Bridge only works while this app is open and signed in.",
    "When the PHR X server is switched off in Settings but still registered with a tool, the database card lists that tool with an **Unregister** button, so its saved login can be removed.",
    "Choosing another database while the PHR X server is switched on also updates the login in every tool it is registered with. Restart the assistant's session afterwards.",
  ],
  howTo: [
    {
      title: "Connect an AI assistant",
      steps: [
        "Press **Add repository** and pick the repository your test cases belong to.",
        "Under **Connect your AI tools**, press **Register** beside your assistant.",
        "Start (or restart) the assistant's session in that repository. It can now use this app's tools while the app is open.",
      ],
    },
    {
      title: "Let an assistant read the company database",
      steps: [
        "Pick the database in **Database**. If it has no login yet, press **Manage credentials**, fill it in, **Test connection**, then **Save**.",
        "Make sure **Company database (read)** is switched on in the tool list.",
      ],
    },
  ],
};
