# Azure DevOps Test Case Manager — V2

A Windows desktop app for authoring, importing, editing, and **running** Azure
DevOps test cases against a Product Backlog Item (PBI), plus a lightweight
**Work Manager** for triaging your work items — all without leaving one window.

Built with **Tauri 2 + Rust + React/TypeScript**. Signs in with your own
Microsoft account; ships and auto-updates via Velopack.

> Internal tool. The version lives in `src-tauri/tauri.conf.json`,
> `src-tauri/Cargo.toml` and `src/lib/changelog.ts`, and the release script
> refuses to ship unless all three agree.

---

## What it does

Pick an organization, project, and PBI in the context bar; every screen scopes
to that choice. The Rust core auto-detects the area/iteration paths and the
PBI's requirement-based test suite so cases you create surface on the board's
test count.

### Manual Entry
- Author a test case: title, tags, automation status, module, preconditions,
  and ordered steps (action / expected) in a numbered step grid.
- **Module** is a dropdown fed by the org's picklist (free entry when none is
  configured); tags autocomplete from the project's existing tags.

### Import File
- **JSON round-trip** is the import/export format (the same file *Export JSON*
  produces — AI-editable). A kept `id` updates that work item; a null id
  creates a new case.
- Queue cases before writing: **View in browser** (an HTML report opened
  directly), **Export JSON**, **Remove all**, or remove individually.
- **Edit queued cases in place**: every queued row (imported or manual) has an
  **Edit** action opening the same form as the case editor — title, tags,
  status, module, preconditions, and the step grid — saving back into the
  queue; a kept `id` still updates that work item on submit.
- The review gate shows a **git-style diff preview** for every queued item:
  creates carry a **NEW** badge, updates show field old → new and per-step
  **word-level diffs** (added words highlighted, removed words struck), steps
  are expandable per row, and **no-op** updates are flagged.
- Writing is **two-stage**: the confirm button spells out *create X · update
  Y*, then a final warning spotlights the target PBI in the context bar (an
  animated border runs around the chip) before anything is sent — removing a
  created case needs delete permission and is permanent, so
  the target gets one last check. A red **Cancel**
  stops the batch mid-run, and a **Clear results** button resets the screen
  afterwards.

### Edit Test Cases
- Pull a PBI's linked cases; click to select, **ctrl/⌘+click** to toggle,
  **shift+click** for a range.
- A **search box** (same placement as Run Tests) filters live by title, id,
  or tag; the header shows *matched / total*.
- **Bulk edit** selected cases (status, module, tags add/replace,
  preconditions) — titles and steps are never touched.
- **Group by title** (smart grouping by shared delimiter/word prefixes),
  refresh, and view/export the selection. Clicking a selected case again
  deselects it, and expanded/collapsed groups are remembered across sessions.

### View Test Cases
- A read-focused tab: the PBI's cases in the same compact grouped list, with
  selection, search, and **View in browser** for the chosen subset.
- **Per-case comments, saved locally** (per organization — nothing is written
  to Azure DevOps): add one from the row or the expanded view; a chip marks
  commented cases and opens the comment in a modal (edit / remove there).
- The browser report includes a **comment box under each case** that
  autosaves back into the app while it's running — annotate during a review
  walkthrough without switching windows.
- Tags moved out of the list rows and into the expanded view to keep the
  list scannable.

### Run Tests & the Runner
- A **read-only overview** table tinted by last outcome, filterable by
  name/id/outcome, with **Group by title** and **shift+click** range
  selection — outcomes are recorded only through the runner.
- A **History** column shows each case's last five outcomes as colored dots
  (hover for date and run number), in both the table and the runner.
- Rows **expand in place** to show the case's steps and, for failures, the
  latest failure detail — result comment and linked bugs — so a fix can be
  checked against the exact failure without opening the runner.
- **Execution report**: highlight the cases to report on (click rows, or a
  group's header checkbox) and one click builds a shareable HTML summary of
  exactly those cases — pass rate, outcome bar, failures-first table, and
  failure details with result comments and linked bugs — and opens it in the
  browser. Whole-suite (and folder) reports live on the Test Suites tab.
- Select rows and open the compact **always-on-top runner** for a
  step-by-step player scoped to that subset (or the whole suite).
- Runner: mark **Passed / Failed / Blocked / Not Applicable** per step and
  overall; a **Pin** toggle; per-case last-outcome badge; auto-loaded
  previously-uploaded screenshots; **Snip** (region capture via the Windows
  overlay + clipboard), **Paste**, or **Attach file**; **File a Bug** from a
  failure; submit as a new test run under the existing plan.
- Suite detection is cached (per PBI) so re-entering Run Tests is instant; a
  re-detect button forces a fresh scan.

### Test Suites
- The plan → multi-level suite **folder tree** (folders collapsed by default),
  with a **search** box that prunes and auto-expands matches.
- **View** any suite or folder as an HTML report, generate an **execution
  report** for it, hand a folder's cases to **Edit**, or jump a requirement
  suite straight to **Edit / Run** (folder actions roll up all descendants).
- Plan scans run concurrently and are cached for the session (with a refresh).

### Work Manager
- A **To Do / In Progress / Done** board of your (or a team's) work items with
  drag-drop between columns and a **Hide Done** toggle for more editing room.
- Moves are **verified against what Azure DevOps actually saved** — if rules
  (e.g. required dates) block a transition, the card rolls back and the rule
  message is shown; the board never displays a state the server rejected.
- Opening a card shows an **Azure-DevOps-style full-window modal** (resizes
  with the window, draggable chrome): title, State / Assigned / Activity up
  top, description and discussion on the left, Planning & Classification on
  the right, Save writes only the fields you touched.
- **Every process tab is fetched from the org's own process layout** — a Bug
  shows its RCA / Preventive Measures pages and so on, each laid out in the
  same section columns as DevOps, with text, HTML, and picklist fields all
  editable.
- Rich text renders properly: **markdown preview by default** (Write to
  edit), **GFM tables**, and **attached images inline** (fetched with your
  credentials, displayed in place).
- A **shadcn-style calendar** for the date fields (themes correctly), and
  **Hide Done** collapses its column smoothly.

### Appearance
- Full **theme system** — the whole palette swaps via one setting: **Light,
  Slate, Midnight, Graphite, Ocean, OLED** (true-black), plus **System**.
- **Accent** presets (or "Theme default") layered on top; app-matched
  scrollbars; a startup splash; collapsible icon sidebar; frameless window
  chrome; a **Ctrl+K** command palette and **Ctrl+1..6 / Ctrl+Shift+M**
  shortcuts.
- The **app icon** is the splash's violet flask mark, and installs create a
  readable **"Test Case Manager"** shortcut.
- An **animated sign-in screen** — the flask mark draws itself in over a
  subtle accent-tinted moving backdrop (skipped automatically where no GPU
  context exists, e.g. over RDP) — plus small touches everywhere: tab bodies
  fade in on switch, case totals count up on load, and a **guided UI tour**
  covers every tab.

---

## Security model

The app talks to Azure DevOps with your credentials, so it is deliberately
conservative, and the invariants are enforced structurally:

- **One DELETE, and it is permanent.** Every file in the Rust client is
  scanned and fails the build if it issues a DELETE — with a single
  carved-out exception, `src/ado/deletion.rs`, which deletes a Test Case
  through the Test Management API (`DELETE _apis/test/testcases/{id}`).
  Azure DevOps offers no recoverable API deletion for test artifacts — the
  work-item recycle bin refuses them outright — so this removes the case
  and its run history irreversibly, and the confirmation dialog says so in
  plain terms before it will proceed. That file is held to a *tighter* rule
  than the others: the work-item endpoint's permanent-erase query parameter,
  which would give this the worse of two already-irreversible semantics, is
  banned from the file by name — including in comments — and a test asserts
  it appears nowhere in it. The delete is permission-gated and fails closed.
  Nothing else — plans, suites, runs, attachments, comments, board items,
  pull requests — is ever removed (guard-tested).
- **Token never crosses the IPC boundary.** The Microsoft access token lives
  only in Rust memory for the session — never returned to the web layer, never
  written to disk, never logged (guard-tested).
- **Confirm before every write.** A review/confirm gate precedes any create or
  update; reads happen freely.
- **Rate-limited writes.** A token-bucket keeps creates within budget.
- **Blank fields never wipe data.** An update skips fields left blank so a
  partial import can't clear existing content.

## Authentication

Interactive **MSAL (PKCE)** through the system browser using the well-known
Azure CLI public client — no Personal Access Token, no app registration. The
token refreshes silently; a mid-batch 401 prompts re-auth and resumes the
in-flight item.

---

## Install & update

Distributed as a **Velopack** installer that auto-updates from its releases
feed — install once, new versions apply on launch. No toolchain needed for end
users. Releases live in a dedicated public repo, separate from the source.

## Develop

```sh
npm install
npm run tauri dev        # run the app (Rust + Vite HMR)

npm test                 # vitest (frontend)
npm run build            # type-check + Vite build
cargo test               # Rust (run inside src-tauri/)
```

### Dev-only features

`npm run tauri dev` runs the developer version: the title bar shows
"— DEV" and a **Developer Panel** (bottom-left) offers debugging tools
(context dump, query-cache refetch/drop, storage clearing, UI triggers).
Dev-only code gates on `import.meta.env.DEV` (frontend, see
`src/dev/DevPanel.tsx` and the `DEV_TOOLS` const in App.tsx) or
`#[cfg(debug_assertions)]` (Rust) - both are compile-time constants, so
`tauri build` (what the release script ships) dead-code-eliminates them:
released binaries carry no trace. Put new debugging/testing tools behind
the same gates and they can never leak into a client build.

Rust tests live in `src-tauri/tests/` (integration targets) because a Windows
manifest quirk crashes tauri-linked unit-test binaries.

## Build & release

```powershell
# bumps version, gates (cargo test + vitest), pushes source FIRST,
# builds, packs with Velopack, publishes to the releases repo:
.\scripts\release-v2.ps1 -Version X.Y.Z
```

---

## Architecture

**Boundary rule: Rust owns I/O, secrets, and wire formats; TypeScript owns
everything user-facing.**

```
src-tauri/                  Rust core
  src/
    lib.rs                  Crate root: module tree, specta builder, app wiring
    commands/               The IPC surface - one thin module per domain
                            (auth, discovery, queue, cases, testplan, runs,
                            bugs, board, misc)
    events.rs               Typed tauri-specta events
    state.rs                Managed state + the shared token-refresh helper
    ado/                    Azure DevOps REST client (GET/POST/PATCH only):
                            transport (HTTP + error mapping) | endpoints
    ado_testplan/           Test plans/suites | runs/results | run history
    work_board/             Board pipeline | item detail | process layout
    import_parser/          Parsing (golden-tested) | exports | HTML report
    auth.rs                 MSAL PKCE loopback sign-in + in-memory token
    steps_xml.rs            Steps XML build/parse (golden-tested)
    model.rs                TestCase / Step domain types
    updater/mod.rs          Velopack auto-update
  tests/                    Integration tests (wiremock + golden vectors)

src/                        React / TypeScript frontend
  App.tsx                   Shell: title bar, sidebar, context bar, routing
  bindings.ts               Generated typed IPC (tauri-specta)
  screens/                  ManualEntry, ImportFile, ExistingCases, ViewCases,
                            RunPanel, RunnerWindow, Suites, WorkBoard, Settings
  components/               TitleBar, Sidebar, PbiPicker, QueueSection,
                            WorkItemDrawer, BulkEditDialog, ui/ (calendar,
                            datefield, button, input, select, …)
  lib/                      theme, grouping, ipc, runnerSession, cn
```

- **Typed IPC & events** via `tauri-specta` (Rust → TS types); progress
  streamed over typed events (`SuiteScanProgress`, `SubmitProgress`).
- **TanStack Query** for all server state (cache, optimistic updates) — no
  hand-rolled loading flags.
- **Tailwind v4** with CSS-variable design tokens; themes are token sets keyed
  off `data-theme` / `data-accent` on `<html>`.

## Tech stack

Tauri 2 · Rust (tokio, reqwest, quick-xml, calamine) · tauri-specta · React 19 ·
Vite · TypeScript · Tailwind v4 · TanStack Query · Vitest · Velopack
