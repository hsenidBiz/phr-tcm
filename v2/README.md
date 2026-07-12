# Azure DevOps Test Case Manager — V2

A Windows desktop app for authoring, importing, editing, and **running** Azure
DevOps test cases against a Product Backlog Item (PBI), plus a lightweight
**Work Manager** for triaging your work items — all without leaving one window.

Built with **Tauri 2 + Rust + React/TypeScript**. Signs in with your own
Microsoft account; ships and auto-updates via Velopack.

> Internal tool. Current version: **1.2.0** (on `feat/tauri-rewrite`; the
> shipping product for existing users is still the PyQt5 v1 — see the repo-root
> `README.md`).

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

### Edit Test Cases
- Pull a PBI's linked cases; click to select, **ctrl/⌘+click** to toggle,
  **shift+click** for a range.
- **Bulk edit** selected cases (status, module, tags add/replace,
  preconditions) — titles and steps are never touched.
- **Group by title** (v1 smart grouping — shared delimiter/word prefixes),
  refresh, and view/export the selection.

### Run Tests & the Runner
- A results table tinted by last outcome, filterable by name/id/outcome, with
  **Group by title** and **shift+click** range selection.
- Record quick outcomes inline, or open the compact **always-on-top runner**
  for a step-by-step player (optionally scoped to a selected subset).
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
- **View** any suite or folder as an HTML report, hand a folder's cases to
  **Edit**, or jump a requirement suite straight to **Edit / Run**.
- Plan scans run concurrently and are cached for the session (with a refresh).

### Work Manager
- A **To Do / In Progress / Done** board of your (or a team's) work items with
  drag-drop between columns and a **Hide Done** toggle for more editing room.
- A **detail drawer** (slides in, drag-resizable from its left edge) to edit
  state, assignee, activity, effort, dates, and a **markdown** description
  (Write / Preview), plus comments and quick-create.
- A **shadcn-style calendar** for the date fields (themes correctly).

### Appearance
- Full **theme system** — the whole palette swaps via one setting: **Light,
  Slate, Midnight, Graphite, Ocean, OLED** (true-black), plus **System**.
- **Accent** presets (or "Theme default") layered on top; app-matched
  scrollbars; a startup splash; collapsible icon sidebar; frameless window
  chrome; a **Ctrl+K** command palette and **Ctrl+1..5 / Ctrl+Shift+M**
  shortcuts.

---

## Security model

The app talks to Azure DevOps with your credentials, so it is deliberately
conservative — the same invariants as v1, now enforced structurally:

- **No DELETE calls — anywhere.** The Rust client only issues GET / POST /
  PATCH; there is no delete method to call (guard-tested).
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
    ado.rs                  Azure DevOps REST client (GET/POST/PATCH only)
    ado_testplan.rs         Test plans, suites, points, runs; suite detection
    auth.rs                 MSAL PKCE loopback sign-in + in-memory token
    steps_xml.rs            Steps XML build/parse (golden-tested)
    import_parser.rs        Import parsing (golden-tested)
    model.rs                TestCase / Step domain types
    work_board.rs           Work Manager board, details, comments, teams
    updater.rs              Velopack auto-update
    lib.rs                  tauri-specta commands + typed events
  tests/                    Integration tests (wiremock + golden vectors)

src/                        React / TypeScript frontend
  App.tsx                   Shell: title bar, sidebar, context bar, routing
  bindings.ts               Generated typed IPC (tauri-specta)
  screens/                  ManualEntry, ImportFile, ExistingCases, RunPanel,
                            RunnerWindow, Suites, WorkBoard, Settings
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
