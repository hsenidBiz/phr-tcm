// AI Bridge: connecting AI coding assistants to this app - the working
// repository, which assistants have its tools, which tools they may use,
// and the company database those tools read.
//
// Locates come from screens/AiBridge.tsx, components/DbCredentialsModal.tsx
// and components/BridgeStatusBadge.tsx. In capture mode the sample data
// seeds one working repository, two detected tools (one registered, one
// with a machine-wide copy left over) and two databases with the dev one
// chosen (src/dev/demo.ts). The options shot turns on the Settings switch
// that changes this tab; every capture boot turns it off again.

import type { Screen, Step } from "../types";

const TAB = "ai-bridge-tab";
const OTHER = "ai-bridge-other-tools";
const SWITCHED_OFF = "ai-bridge-switched-off";
const CREDENTIALS = "ai-bridge-credentials";
const OPTIONS = "ai-bridge-options";
const API = "ai-bridge-api-templates";

const NAV: Step = { nav: "AI Bridge" };
const REPO = "C:\\Projects\\customer-portal";

export const aiBridge: Screen = {
  id: "ai-bridge",
  title: "AI Bridge",
  group: "AI tools",
  summary:
    "Connect an AI coding assistant, such as Claude Code or VS Code, to this app. The assistant can then read your test cases, suites, " +
    "run results, tags, Product Backlog Items and wiki pages, and check the drafts it writes, while you stay in charge of what reaches Azure DevOps: " +
    "none of these tools can write to it. The tools are registered per working repository; the other settings here apply to the whole app.",
  shots: [
    { id: TAB, route: [NAV, { waitFor: { role: "button", name: "Unregister" } }], alt: "The AI Bridge tab with a working repository, the AI tools, the environment and Database Read Access" },
    {
      id: OTHER,
      route: [NAV, { click: { text: "Other tools" } }, { waitFor: { role: "button", name: "Copy command" } }],
      alt: "Other tools opened, with the command line and the config to copy",
    },
    {
      id: SWITCHED_OFF,
      route: [
        NAV,
        { click: { role: "switch", name: "Run results" } },
        { scrollTo: { role: "button", name: "Turn all back on" } },
      ],
      alt: "One tool switched off, with Turn all back on under the list",
    },
    {
      id: CREDENTIALS,
      route: [NAV, { click: { role: "button", name: "Edit Dev - dev login" } }, { waitFor: { role: "button", name: "Test connection" } }],
      alt: "The credentials window of the chosen database",
    },
    {
      id: API,
      route: [NAV, { scrollTo: { role: "switch", name: "API templates (create, edit and delete)" } }],
      alt: "The API templates card, with the switch that lets an assistant prove and run templates",
    },
    {
      id: OPTIONS,
      route: [
        { click: { role: "button", name: "Settings" } },
        { click: { role: "switch", name: "Allow registering AI tools machine-wide" } },
        NAV,
        { waitFor: { role: "button", name: "Machine-wide" } },
      ],
      alt: "AI Bridge with the machine-wide choice switched on in Settings",
    },
  ],
  groups: [
    { id: "repositories", title: "Working repositories", summary: "The folders your test cases belong to, and whether the bridge is running." },
    { id: "connect", title: "Connect your AI tools", summary: "Register the bridge with the AI tools installed on this computer." },
    { id: "tools", title: "Tools an assistant may use", summary: "Switch off any tool you do not want an assistant to call." },
    {
      id: "environment",
      title: "Environment",
      summary: "The site and database Auto Run and API Templates work against. Shown while Enable Advanced Features is on, in Settings.",
    },
    {
      id: "api-templates",
      title: "API templates",
      summary: "Whether an assistant may create and change test data on your site with API templates. Shown while Enable Advanced Features is on, in Settings.",
    },
    { id: "company-database", title: "Database Read Access", summary: "The one database the assistants' tools work with, and how they sign in to it." },
    { id: "tools-breakdown", title: "AI Tools Breakdown", summary: "What each tool does, in plain words." },
  ],
  controls: [
    // --- The tab --------------------------------------------------------------
    {
      id: "status",
      shot: TAB,
      group: "repositories",
      locate: { role: "status", nameRe: "^Bridge listening" },
      name: "Running",
      does:
        "Beside the title: whether the bridge the assistants connect to is up. It runs only while this app is open and you are signed in; " +
        "when it is not, it reads **Not running**. Hover it to see the port it listens on.",
    },
    {
      id: "repository",
      shot: TAB,
      group: "repositories",
      locate: { role: "radio", name: `Use ${REPO}` },
      name: "Working repository",
      does:
        "The repositories your test cases belong to. The dot marks the current one: files written or imported for it go to its **.test-cases** folder, and the AI tools register into it. Click another dot to make that one current.",
      tips: ["Until a repository is added, this card is all the tab shows."],
    },
    {
      id: "repository-switch",
      shot: TAB,
      group: "repositories",
      locate: { role: "switch", name: `AI tools for ${REPO}` },
      name: "AI tools switch",
      does: "Turns the AI tools on or off for that repository. Switching off the current one turns them off without forgetting the folder.",
    },
    {
      id: "remove-repository",
      shot: TAB,
      group: "repositories",
      locate: { role: "button", name: `Remove ${REPO}` },
      name: "Remove (x)",
      does: "Takes the repository off the list. Nothing in the folder is changed.",
    },
    {
      id: "add-repository",
      shot: TAB,
      group: "repositories",
      locate: { role: "button", name: "Add repository" },
      name: "Add repository",
      does: "Opens a folder picker. The folder you pick is added to the list and becomes the current repository.",
    },
    {
      id: "rescan",
      shot: TAB,
      group: "connect",
      locate: { role: "button", name: "Rescan" },
      name: "Rescan",
      does: "Looks again for the AI tools installed on this computer and says how many it found. Use it after installing one, without restarting the app.",
    },
    {
      id: "registered",
      shot: TAB,
      group: "connect",
      locate: { text: "Registered ✓" },
      name: "Installed AI tools",
      does:
        "Each AI tool found on this computer, with where its settings are read from (**in this repo**, or **global** for machine-wide) and whether this app's tools are registered with it.",
    },
    {
      id: "unregister",
      shot: TAB,
      group: "connect",
      locate: { role: "button", name: "Unregister" },
      name: "Unregister",
      does: "Removes this app's tools from that AI tool's settings.",
    },
    {
      id: "register",
      shot: TAB,
      group: "connect",
      locate: { role: "button", name: "Register" },
      name: "Register",
      does:
        "Adds this app's tools to that AI tool's settings, in the current repository (or machine-wide, when you chose that). An assistant that is already running may need its session restarted to see them.",
    },
    {
      id: "retire-global",
      shot: TAB,
      group: "connect",
      locate: { role: "button", name: "Retire global copies" },
      name: "Retire global copies",
      does:
        "Shown under a tool that still has this app's tools registered machine-wide from before. A machine-wide copy can hide the repository's own in most assistants; this removes it.",
    },
    {
      id: "other-tools",
      shot: TAB,
      group: "connect",
      locate: { text: "Other tools" },
      name: "Other tools",
      does: "For an assistant the app does not detect: opens a command line and a settings snippet you can copy and add yourself.",
    },
    {
      id: "tool-count",
      shot: TAB,
      group: "tools",
      locate: { text: "6 of 6 on" },
      name: "Tools an assistant may use",
      does:
        "How many of the tools below are switched on. The tools every writing job needs (the writing guide, reading cases, checking a draft and similar) are always on and not listed.",
    },
    {
      id: "tool-switch",
      shot: TAB,
      group: "tools",
      locate: { role: "switch", name: "Test Suites" },
      name: "Tool switch",
      does:
        "Switch a tool off to keep it out of an assistant's reach: Test Suites, Run results, Database Read Access, Project tags, Find a Product Backlog Item and Project wiki. A connected assistant sees the change without being restarted.",
    },
    {
      id: "environment-pick",
      shot: TAB,
      group: "environment",
      locate: { role: "combobox", name: "Environment" },
      name: "Environment",
      does:
        "The environment Auto Run and API Templates work in: a site address, the company database below, and its own accounts and saved sign-ins. " +
        "Switching moves the **Database** below with it, and the environment's name shows in the title bar.",
    },
    {
      id: "edit-environments",
      shot: TAB,
      group: "environment",
      locate: { role: "button", name: "Edit environments" },
      name: "Edit environments",
      does:
        "Adds, renames or removes environments, and sets each one's site address, database and test name prefix. " +
        "The **Test name prefix**, **AUTOTEST** unless you change it, starts the name of every draft a fixture or a script makes in that environment, so **Clean up test-made drafts** can tell the tests' drafts from yours. It is 3 to 20 letters, digits or -.",
    },
    {
      id: "api-templates-switch",
      shot: API,
      group: "api-templates",
      locate: { role: "switch", name: "API templates (create, edit and delete)" },
      name: "API templates (create, edit and delete)",
      does:
        "Lets a connected assistant prove and run API templates: build a template for the test data a case needs, prove it end to end on your site, and run a saved one for the records it creates. " +
        "Proving and running write real test data through the site's own requests, as your Auto Run accounts, against the active environment's site address, so this is off until you turn it on. " +
        "The card is shown while **Enable Advanced Features** is on, in Settings, under General. The templates themselves are listed on the API Templates tab.",
    },
    {
      id: "database",
      shot: TAB,
      group: "company-database",
      locate: { role: "combobox", name: "Database" },
      name: "Database",
      does:
        "The company database this app's own database tools read, when **Database Read Access** is switched on. The x clears the choice.",
    },
    {
      id: "signs-in-as",
      shot: TAB,
      group: "company-database",
      locate: { text: "Signs in as portal_devlogin" },
      name: "Signs in as",
      does: "The user the chosen database signs in as, or **No login saved**. The password is never shown.",
    },
    {
      id: "edit-database",
      shot: TAB,
      group: "company-database",
      locate: { role: "button", name: "Edit Dev - dev login" },
      name: "Edit (database)",
      does:
        "Every database is listed under the picker. **Edit** opens that database's login. **Add database** above the list adds one of your own, which can be removed again; the ones that come with the app cannot.",
    },
    {
      id: "writes",
      shot: TAB,
      group: "company-database",
      locate: { role: "switch", name: "Create, update and delete" },
      name: "Create, update and delete",
      does:
        "Lets an assistant change data in the chosen database, not only read it. It is off until you turn it on, it can only be turned on for a dev login database (a user ending in _devlogin), and every statement is written to the app's log.",
    },
    {
      id: "no-ask",
      shot: TAB,
      group: "company-database",
      locate: { role: "switch", name: "Run database changes without asking" },
      name: "Run changes without asking",
      does:
        "Lets your AI tools change the database without stopping to ask you first. Available while **Create, update and delete** is on. " +
        "The app sets each registered tool's own \"always allow\" for the database tool where that tool keeps it in a file (Claude Code and Cursor), and names below the switch any tool you need to allow it in yourself. " +
        "Assistants still try each set of changes as a dry run before saving it. Off until you turn it on.",
    },
    {
      id: "forget",
      shot: TAB,
      group: "company-database",
      locate: { role: "button", name: "Forget them" },
      name: "Forget them",
      does:
        "Removes the database logins saved on this computer (they are kept in Windows Credential Manager), clears the card's settings and the chosen database, and turns **Create, update and delete** off.",
    },
    {
      id: "breakdown",
      shot: API,
      group: "tools-breakdown",
      locate: { role: "heading", name: "AI Tools Breakdown" },
      name: "AI Tools Breakdown",
      does: "What each tool you can switch does, and the recommended way to have an assistant write test cases for a Product Backlog Item.",
    },

    // --- Other tools ------------------------------------------------------------
    {
      id: "copy-command",
      shot: OTHER,
      group: "connect",
      locate: { role: "button", name: "Copy command" },
      name: "Copy (command line)",
      does: "Copies the command that registers this app's tools with Claude Code. Run it inside the repository.",
    },
    {
      id: "copy-config",
      shot: OTHER,
      group: "connect",
      locate: { role: "button", name: "Copy config" },
      name: "Copy (config)",
      does: "Copies the settings snippet to paste into another assistant's tool settings.",
    },

    // --- A tool switched off ------------------------------------------------------
    {
      id: "switched-off",
      shot: SWITCHED_OFF,
      group: "tools",
      locate: { role: "switch", name: "Run results" },
      name: "A switched-off tool",
      does: "Its name is greyed out and the count above drops by one.",
    },
    {
      id: "turn-all-on",
      shot: SWITCHED_OFF,
      group: "tools",
      locate: { role: "button", name: "Turn all back on" },
      name: "Turn all back on",
      does: "Switches every tool on again. It shows while any tool is off.",
    },

    // --- Credentials --------------------------------------------------------------
    {
      id: "credentials",
      shot: CREDENTIALS,
      group: "company-database",
      locate: { role: "dialog", name: "Credentials for Dev - dev login" },
      name: "Credentials window",
      does:
        "For the databases that come with the app, the server and database are shown and cannot be changed. For any other database you can edit the server, port and database, and choose whether to trust the server certificate.",
    },
    {
      id: "user",
      shot: CREDENTIALS,
      group: "company-database",
      locate: { role: "textbox", name: "User" },
      name: "User",
      does: "Who the database signs in as.",
    },
    {
      id: "password",
      shot: CREDENTIALS,
      group: "company-database",
      locate: { role: "textbox", name: "Password" },
      name: "Password",
      does: "Type a new password, or leave it blank to keep the saved one. A saved password is never shown back.",
    },
    {
      id: "test-connection",
      shot: CREDENTIALS,
      group: "company-database",
      locate: { role: "button", name: "Test connection" },
      name: "Test connection",
      does: "Tries to sign in with what is in the form, without saving it, and shows the result under the fields.",
    },
    {
      id: "reset-default",
      shot: CREDENTIALS,
      group: "company-database",
      locate: { role: "button", name: "Reset to default" },
      name: "Reset to default",
      does: "Puts back the login the database came with. It shows only on a database that comes with the app, once its login has been changed.",
    },
    {
      id: "credentials-cancel",
      shot: CREDENTIALS,
      group: "company-database",
      locate: { role: "button", name: "Cancel" },
      name: "Cancel",
      does: "Closes the window without saving.",
    },
    {
      id: "credentials-save",
      shot: CREDENTIALS,
      group: "company-database",
      locate: { role: "button", name: "Save" },
      name: "Save",
      does: "Saves the login in Windows Credential Manager on this computer and closes the window.",
    },

    // --- Options from Settings ------------------------------------------------------
    {
      id: "register-in-repository",
      shot: OPTIONS,
      group: "connect",
      locate: { role: "button", name: "This repository" },
      name: "Register in: This repository",
      does: "Registers the AI tools into the current repository. The choice shows only when **Allow registering AI tools machine-wide** is on in Settings.",
    },
    {
      id: "register-machine-wide",
      shot: OPTIONS,
      group: "connect",
      locate: { role: "button", name: "Machine-wide" },
      name: "Register in: Machine-wide",
      does: "Registers the AI tools for the whole computer instead, for a machine that does not work from a repository. Writing test cases still needs a repository.",
    },
  ],
  tips: [
    "None of the tools an assistant gets through this app can create, update or delete anything in Azure DevOps. You import what it writes yourself, on Import Test Cases.",
    "The AI Bridge only works while this app is open and signed in.",
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
        "Pick the database in **Database**. If it has no login yet, press **Edit** beside it in the list, fill it in, **Test connection**, then **Save**.",
        "Make sure **Database Read Access** is switched on in the tool list.",
      ],
    },
    {
      title: "Use your own writing style",
      steps: [
        "In the **Writing style** card, type your team's rules for designing test cases in Markdown, or press **Upload .md** to load them from a file.",
        "Switch on **Use my writing style** and press **Save**. **Discard changes** goes back to what you saved last.",
        "The assistant follows your style the next time it reads the writing guide. It replaces the standard advice on how many cases to write and which edge cases to cover. The case format and import rules always apply.",
      ],
    },
  ],
};
