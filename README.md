# Azure DevOps Test Case Manager

A Windows desktop app for writing, importing, editing and running Azure
DevOps test cases against a Product Backlog Item (PBI). A Work Manager
board for your work items and pull requests sits in the same window.

Built with Tauri 2, Rust and React with TypeScript. You sign in with your
own Microsoft account. The app ships as a Velopack installer and updates
on launch.

Internal tool. The version lives in three places: src-tauri/tauri.conf.json,
src-tauri/Cargo.toml and src/lib/changelog.ts. The release script refuses
to ship unless all three agree.

---

## What the app does

Pick an organization, a project and a PBI in the context bar. Every screen
scopes to your choice. The Rust core detects the area and iteration paths
and the PBI's requirement-based test suite, so cases you create appear in
the board's test count.

### Manual Entry
- Write one test case: title, tags, automation status, module,
  preconditions and ordered steps (action and expected result) in a
  numbered step grid.
- Module is a dropdown fed by your organization's picklist. Free entry
  applies when none is configured. Tags autocomplete from the tags your
  project already uses.

### Import File
- JSON is the import and export format. Export JSON writes the same file
  you import, so an AI assistant edits the same shape you read. A kept id
  updates a work item. A blank id creates a new case.
- Queue cases before writing: View in browser opens an HTML report, Export
  JSON saves the queue, Remove all clears the queue, and each row has a
  remove action.
- Edit queued cases in place. Every queued row opens the same form as the
  case editor and saves back into the queue. A kept id still updates the
  work item on submit.
- Before you write, the review gate shows a diff for every queued item.
  Creates carry a NEW badge. Updates show each field old and new, with
  word-level diffs per step. No-op updates are flagged. The diff shows as
  soon as the queue holds an update, before you press Review.
- Writing takes two steps. The confirm button spells out create X and
  update Y. A final warning spotlights the target PBI in the context bar
  before anything is sent. Cancel stops the batch after the current chunk.
- Uploads go to Azure DevOps in batches of 25, each new case linked to the
  PBI in the same call. Two hundred cases take seconds. A case Azure
  DevOps rejects stays in the queue with the server's own message.
- If no test suite exists for the PBI and the app fails to create one, a
  message names the plan and explains what to do.
- Comments sync both ways between the file and View Test Cases, keyed by
  case id.

### Update Test Cases
- Pull a PBI's linked cases. Click to select, ctrl or cmd click to toggle,
  shift click for a range.
- A search box filters by title, id or tag. The header shows matched and
  total counts.
- Bulk edit the selected cases: status, module, tags (add or replace) and
  preconditions. Titles and steps are never touched.
- Group by title, refresh, and view or export the selection. Expanded and
  collapsed groups are remembered.

### View Test Cases
- A read-focused list of the PBI's cases with selection, search and View
  in browser for the chosen subset.
- Per-case comments are saved on your machine, per organization. Nothing
  is written to Azure DevOps. A chip marks commented cases.
- The browser report includes a comment box under each case. Comments
  autosave back into the app while the report is open.

### Run Tests and the Runner
- A read-only overview table tinted by last outcome. Filter by name, id or
  outcome. Group by title. Select all with the button or ctrl A. Outcomes
  are recorded only through the runner.
- The History column shows each case's last five outcomes as coloured
  dots, in the table and in the runner.
- Rows expand in place to show steps and, for failures, the latest result
  comment and linked bugs.
- Refresh reloads outcomes and history. Shift click Refresh to detect the
  suite again.
- Execution report: select cases and build a shareable HTML summary with
  pass rate, outcome bar and failure details, opened in your browser.
- Select rows and open the compact always-on-top runner. Mark Passed,
  Failed, Blocked or Not Applicable per step and overall. Snip a region,
  paste a screenshot or attach a file. File a Bug from a failure. Submit
  as a new test run under the existing plan.

### Test Suites
- The plan's suite folder tree with a search box which prunes and expands
  matches.
- View any suite or folder as an HTML report, generate an execution
  report, or hand a folder's cases to Update or Run.
- Plan scans run concurrently and are cached for the session.

### AI Bridge
- The app runs a local bridge while you are signed in. Coding assistants
  such as Claude Code, VS Code, Cursor and Windsurf register the app as an MCP server
  per repository and read the same live rules the importer uses.
- Tools: find a PBI, read a PBI's cases, read the project's tags, search
  and read the wiki, check spec coverage, optimise a draft into a run
  sheet, validate a draft with the real importer, and read the run
  failures a tester recorded.
- A writing guide tells the assistant the exact field contract, the
  allowed module values, the tags in use and the writing style test cases
  follow: short active sentences, exact values, no em dashes.
- A badge beside the tab title shows whether the bridge is running.
- An optional company database server registers alongside the bridge.
  Change the default connection in the app and the registered tool
  configs update with you.

### Work Manager
- A To Do, In Progress and Done board of your work items, or a team's,
  with drag and drop between columns and a Hide Done toggle. A This
  sprint filter appears when you pick an area.
- Moves are checked against what Azure DevOps saved. If a rule blocks a
  transition, the card rolls back and the rule message shows.
- Opening a card shows a full-window editor laid out like Azure DevOps:
  title, State, Assigned and Activity on top, description and discussion
  on the left, Planning and Classification on the right. Save writes only
  the fields you touched.
- Every process tab comes from your organization's own process layout.
  Text, HTML and picklist fields are editable.
- Rich text renders as markdown with GFM tables and inline attached
  images. Comments support markdown, and you edit your own comments in
  place.
- Pull Requests lists your PRs and the ones waiting for your review, with
  conflicts and unresolved comments called out.
- A notification bell collects what happened while you were away: new
  assignments, PRs with conflicts or comments to resolve, and PRs waiting
  for your review. Opening the bell marks everything read.

### Appearance
- Themes: Light, Slate, Midnight, Graphite, Ocean, OLED and System. Accent
  presets sit on top.
- App-matched scrollbars, a startup splash, a collapsible icon sidebar,
  frameless window chrome, a Ctrl K command palette, Ctrl 1 to 8 tab
  shortcuts and Ctrl Shift M for Work Manager.
- An animated sign-in screen. Tab bodies fade in on switch. A guided tour
  covers every tab.

### Settings
- Theme, accent and request rate. Backup and restore of app data.
- The changelog, the app's own log, Copy log, Open log folder and Report
  a bug. Report a bug opens a prefilled GitHub issue with your log
  attached. Organization, project and work item names are removed from
  the log first.

---

## Security model

The app talks to Azure DevOps with your credentials, so the rules below
are enforced in code and by tests.

- One DELETE, and only one. Every file in the Rust client fails the build
  if it issues a DELETE, with one exception: src/ado/deletion.rs deletes a
  Test Case through the Test Management API. Azure DevOps offers no
  recoverable deletion for test artifacts, so this removes the case and
  its run history for good, and the dialog says so before you proceed.
  The delete is permission-gated and fails closed. Plans, suites, runs,
  attachments, comments, board items and pull requests are never removed.
- The token never crosses the IPC boundary. Your Microsoft access token
  lives in Rust memory for the session. The app never returns the token to
  the web layer, writes the token to disk or logs the token. A test fails
  the build if a token-shaped field appears in the generated bindings.
- Confirm before every write. A review gate precedes any create or update.
  Reads happen freely.
- Rate-limited writes. A pacer keeps requests within the budget you set,
  and the app slows down whenever Azure DevOps asks.
- Blank fields never wipe data. An update skips fields left blank.
- Comments and reviewer notes stay in the app and in your files. The app
  never sends them to Azure DevOps.

## Authentication

Interactive MSAL with PKCE through your system browser, using the Azure
CLI public client. No Personal Access Token and no app registration. The
token refreshes silently. A 401 in the middle of a batch prompts you to
sign in again and resumes the in-flight item.

---

## Install and update

The app ships as a Velopack installer and updates from this repository's
GitHub Releases. Install once. New versions apply on launch. End users
need no toolchain.

## Develop

```sh
npm install
npm run tauri dev        # run the app (Rust + Vite HMR)

npm test                 # vitest (frontend)
npx tsc --noEmit         # typecheck
npm run build            # type-check + Vite build
cargo test --tests       # Rust (run inside src-tauri/)
```

### Dev-only features

`npm run tauri dev` runs the developer version. The title bar shows DEV
and a Developer Panel offers debugging tools: context dump, query cache
refetch and drop, storage clearing, UI triggers and forced failures.
Dev-only code gates on `import.meta.env.DEV` in the frontend and
`#[cfg(debug_assertions)]` in Rust. Both are compile-time constants, so
`tauri build` removes them. Released binaries carry no trace. Put new
debugging tools behind the same gates.

Rust tests live in `src-tauri/tests/` as integration targets. A Windows
manifest quirk crashes tauri-linked unit-test binaries.

`src/bindings.ts` is generated by `cargo test --test bindings`. Change the
Rust and regenerate. Never edit the file by hand.

## Build and release

```powershell
# gates (cargo test + vitest + production build), pushes source FIRST,
# builds, packs with Velopack, publishes to the releases repo:
.\scripts\release-v2.ps1 -Version X.Y.Z
```

Add an entry to `src/lib/changelog.ts` before you run the script. The
post-update What's new dialog only opens when an entry newer than the
last-seen version exists.

---

## Architecture

Rust owns I/O, secrets and wire formats. TypeScript owns everything you
see.

```
src-tauri/                  Rust core
  src/
    lib.rs                  Crate root: module tree, specta builder, app wiring
    commands/               The IPC surface, one thin module per domain
                            (auth, discovery, queue, cases, testplan, runs,
                            bugs, board, misc)
    events.rs               Typed tauri-specta events
    state.rs                Managed state and the shared token-refresh helper
    ado/                    Azure DevOps REST client (GET, POST, PATCH,
                            $batch): transport, endpoints, throttle
    ado_testplan/           Test plans and suites, runs and results, history
    work_board/             Board pipeline, item detail, process layout
    import_parser/          Parsing (golden-tested), exports, HTML report
    ai_bridge.rs, mcp.rs    The local bridge and the MCP server the
                            assistants talk to
    auth.rs                 MSAL PKCE loopback sign-in and in-memory token
    steps_xml.rs            Steps XML build and parse (golden-tested)
    model.rs                TestCase and Step domain types
    updater/mod.rs          Velopack auto-update
  tests/                    Integration tests (wiremock and golden vectors)

src/                        React and TypeScript frontend
  App.tsx                   Shell: title bar, sidebar, context bar, routing
  bindings.ts               Generated typed IPC (tauri-specta)
  screens/                  ManualEntry, ImportFile, EditCases, ViewCases,
                            RunPanel, RunnerWindow, Suites, AiBridge,
                            WorkBoard, PrPanel, Settings
  components/               TitleBar, Sidebar, ContextBar, PbiPicker,
                            QueueSection, NotificationBell, ErrorBoundary,
                            ui/ (calendar, datefield, button, input, select)
  lib/                      theme, grouping, ipc, notifications, changelog
```

- Typed IPC and events through tauri-specta. Progress streams over typed
  events such as SuiteScanProgress and SubmitProgress.
- TanStack Query holds all server state. No hand-rolled loading flags.
- Tailwind v4 with CSS-variable design tokens. Themes are token sets keyed
  off data-theme and data-accent on the html element. A consistency test
  fails the build on a hardcoded colour.
- A render crash shows a fallback with a Reload button and writes the
  stack to the app log.

## Tech stack

Tauri 2, Rust (tokio, reqwest, quick-xml), tauri-specta, React 19, Vite,
TypeScript, Tailwind v4, TanStack Query, Vitest, Velopack.
