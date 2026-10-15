# Modules: switch on the parts of the app you need

Date: 2026-10-15. Status: design, from the conversation with Avin; awaiting his review.

## Why

TCM has grown well beyond test case management: database access for the assistant,
writing style rules, Auto Run, API Templates and the Playwright tab. Colleagues who only
write and manage test cases see all of it, or none of it behind one switch ("Enable
Advanced Features"). Avin wants colleagues to start with a lean app and switch on only the
areas they need, the way a player unlocks a game's tech tree, and to retire the
Advanced Features switch.

## Goals

- A **Modules** page, opened from Settings, where each optional area is a module the person
  switches on or off.
- Shown as a **tech tree**, top to bottom: a module that needs another can only be switched
  on once that one is on.
- **Fresh installs start lean**: no module on.
- **Updates take nothing away**: an existing install keeps everything it uses today.
- Switching a module off hides it everywhere: its screen, its cards, its assistant tools and
  slash commands, its help pages.
- It should feel good: paths light up when a module comes on.

## Non-goals

- Test case management itself is never optional (Manual Entry, Import, Update, View, Run
  Tests, Search Suites, Suite Management, the Work Manager board, sign-in, Settings, and the
  AI Bridge screen with the core writing tools).
- No per-user or per-organisation policy: modules are a per-machine choice.
- Splitting today's features into finer modules than listed below.

## Anti-goals

- An update that makes something a colleague uses disappear.
- Switching a module off deleting anything (scripts, templates, database logins, writing
  style, environments). Off means hidden; on again brings it all back.
- A module reachable while its parent is off (by a saved tab, a shortcut, the command
  palette, an assistant tool or a help link).
- The Extras Easter egg changing (see below).

## The tree (decided)

Top to bottom, starting from **Core**:

| Module | Needs | What it contains |
|---|---|---|
| **Core** (root, always on) | - | Test case management. Not a switch. |
| **Database Access** | Core | The AI Bridge's database card (choose, add, edit databases); the `db_lookup` / `db_query` assistant tools and their switches (writes, auto-approve); Auto Run precondition reading. |
| **Writing Style** | Core | The AI Bridge's Writing Style card, and the custom style carried by `get_writing_guide`. |
| **Auto Run** | Database Access | The Auto Run screen; the AI Bridge's Environment card and the environment name in the title bar; test files; the 25 Auto Run assistant tools and the `setup`, `discover`, `map-menus`, `heal` slash commands; the network pause. |
| **API Templates** | Auto Run | The API Templates screen; the AI Bridge's "API templates (create, edit and delete)" card; the 11 API template tools. |
| **Playwright** | Auto Run | The Playwright screen and Auto Run's "Export to Playwright" menu item. |

Each module's description on the page (two or three lines, as in the approved mockup):

- **Database Access:** Connect TCM to your test environments' databases. Your assistant can
  look data up, and Auto Run uses it to check preconditions and to notice when the VPN drops.
- **Writing Style:** Your own rules for how test cases are worded and split into steps. Your
  assistant follows them instead of the standard guide when it writes cases.
- **Auto Run:** Script a test case once, then run it in a browser, watched or unattended, and
  review the results before they go to Azure DevOps. Includes environments, accounts, areas
  and test files.
- **API Templates:** Create the test data a case needs, such as cycles, drafts and settings,
  through the app's own pages before the case runs, so every run starts from a known state.
- **Playwright:** Export passing Auto Run scripts into the team's Playwright repo, run its
  tests, manage its test accounts and keep exported cases in sync.

## The Modules page (decided, from the approved mockups)

- **Opened from Settings** by a "Modules" entry; it is its own full page (with a way back to
  Settings), not a card on the Settings grid.
- **Layout B, top to bottom**, in a scrolling area so a longer tree still fits.
- **Core** at the top: a glowing **power core** with rings pulsing outward, named "Core", with
  a short line under it ("Test cases, suites, runs and the board"). The animation runs
  continuously and gently.
- **Module nodes**: a wide card with an icon tile, the name, a status line and the
  description. Status is sentence case, never capitals: "On", "Off · click to switch on",
  "Locked · needs Auto Run".
- **Switching on**: click the node. A light runs along the path from its parent to it, then the
  path stays lit and the node glows; its icon tile takes the accent colour.
- **Locked node**: dimmed; clicking it gives a small shake and leaves the status line saying
  what it needs.
- **Switching off**: the node and its path dim. Switching off a module that others depend on
  switches those off too.
- The mockups are in `.superpowers/brainstorm/1159-1792043373/content/` (`module-tree-v2.html`
  and `root-node.html`, option B).

## Upgrade and fresh installs (decided)

- **Fresh install** (no earlier app data on the machine): every module off.
- **Existing install**, on the first start of the version with modules:
  - Database Access and Writing Style on (everyone has them today).
  - If Advanced Features was on, or the Extras unlock was on: Auto Run, API Templates and
    Playwright on as well.
- After that, only the person's own choices on the Modules page change them.

## Advanced Features and Extras (decided)

- The **"Enable Advanced Features"** switch is removed from Settings; Modules replaces it.
- The **Extras Easter egg stays as it is**: the hidden key sequence on Settings still reveals
  the Extras card with the games and Reset to default.

## My calls (Claude), for review

- **Extras no longer switches features on.** Today the unlock also shows Auto Run, API
  Templates and Playwright. With modules, only the Modules page decides what is on; the
  unlock only reveals the Extras card. (On the upgrade, an unlocked install keeps those
  modules on, so nothing disappears.)
- **Switching off a parent asks first** when it would also switch off modules under it:
  "Switching off Auto Run also switches off API Templates and Playwright." Switching off a
  module with nothing under it does not ask.
- **Switching a module off is refused while it is busy**: Auto Run during a run, Playwright
  while a repo command runs, with a sentence saying why.
- **API Templates off does not stop Auto Run scripts' setups and fixtures from running.**
  Those are part of running a script; turning API Templates off hides the screen and the
  assistant's template tools, not what saved scripts already do.
- **Where the choice is stored:** per machine, in Rust, beside today's Advanced Features flag;
  like that flag, it is not part of a backup.
- **Development builds** start with every module on (as today), but the switches still work so
  the tree can be tried. The documentation capture mode shows every module.
- **Reduced motion**: with the system's reduced-motion setting on, the core does not pulse and
  switching shows the end state at once, with no travelling light.
- **Accessibility**: each node is a switch a keyboard can reach, with its name, state and what
  it needs announced; the paths are decoration.
- **The How To Use guide** shows the pages of the modules that are on (today it shows or hides
  three screens with one flag), and gains a Modules page.

## Left to the builder

- The module names in code and how the single "features on" check becomes a per-module check
  in both Rust and the webview, keeping the existing gates (sidebar, command palette, saved
  tab redirect, assistant tool lists, bridge routes, slash commands, help) driven from it.
- How the page draws the tree (the mockups use positioned cards and SVG paths).
- How "earlier app data exists" is detected for the upgrade.
- Which existing tests change and how (Advanced Features, sidebar, AI Bridge, help-site
  guards all pin today's single switch).

## Testing

- The tree rules: a locked module cannot be switched on; switching a parent off switches its
  descendants off; the upgrade rules for fresh, existing, advanced and unlocked installs.
- Each module off hides every surface listed in the table and refuses its assistant tools and
  routes; each on shows them.
- The page: statuses, locking, the confirmation, refusal while busy, reduced motion,
  keyboard use.
- The help guide follows the modules, and its guard terms still hold (no mention of the
  Easter egg).
