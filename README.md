# Test Case Manager

A Windows desktop app for writing, uploading, managing, reviewing and
**running** Azure DevOps test cases against a Product Backlog Item — in
bulk, from one window — plus a Work Manager board for the work items and
pull requests around them.

Built with **Tauri 2 + Rust + React/TypeScript**. Signs in with your own
Microsoft account; installs and updates itself through Velopack. Current
version: **1.23.2**. The app lives in [`v2/`](v2/).

## The problem it solves

Authoring test cases in the Azure DevOps web UI is slow and per-case: forty
cases means forty forms, and updating them means finding each one again.
Teams end up drafting in spreadsheets and copying them in by hand — which
is where cases get lost, duplicated, or quietly drift from the spec.

This app makes **the batch** the unit of work: draft many, review them
together, write them in one pass, and manage them as a set afterwards.

## What it does

### Writing
- **By hand** — title, tags, automation status, module, preconditions and
  ordered action / expected steps. Module comes from the organisation's own
  picklist; tags autocomplete from the project's existing tags.
- **With an AI assistant** — the app runs a local bridge (`tcm-testcases`,
  an MCP server) that the coding assistants already on the machine can
  use: `begin_test_case_writing` puts a checklist to the tester and writes
  an approved plan before any case exists; `get_writing_guide` serves the
  live format rules; `check_spec_coverage` finds the parts of a
  specification with no case yet; `transform_cases`, `optimize_cases` and
  `validate_cases` reshape and check a draft through the app's own
  importer. **None of these tools can write to Azure DevOps** — the
  assistant proposes, a person presses the button.

### Uploading
- **JSON round-trip.** The import format is the file the app exports, so it
  is AI-editable and human-diffable. A case carrying an `id` updates that
  exact work item; a case without one is created. Title matching is only
  ever a duplicate *warning*, never an update.
- **Files stay live.** An imported file is watched; edits made outside the
  app flow into the queue with a report of what moved.
- **A review gate that shows the change.** Every queued item gets a
  git-style diff preview — creates carry a NEW badge, updates show field
  old → new with word-level diffs inside each step, no-op updates are
  flagged and skipped.
- **Two-stage confirm.** The button spells out *create X · update Y*, then
  a final warning spotlights the target PBI before anything is sent. A
  results panel afterwards lists each case with its new work item number;
  anything that failed keeps a red outline in the queue.
- **Board visibility, handled.** The app finds or creates the PBI's
  requirement-based test suite so new cases actually count on the board.

### Managing
- Pull a PBI's cases; live search by title, id or tag; click / ctrl+click /
  shift+click selection; smart grouping by shared title prefix.
- **Bulk edit** status, module, tags and preconditions across a selection.
  Titles and steps are deliberately never touched in bulk.
- **Suite browser** — the plan's full folder tree with search; view any
  folder as an HTML report, hand its cases to Edit, or jump a requirement
  suite straight to Run.
- **Blank fields never wipe data** — an update skips fields left blank.

### Reviewing and running
- A read-only tab with **per-case comments saved locally** (never written
  to Azure DevOps), and a shareable browser report whose comment boxes
  autosave back into the app while it is open.
- A compact **always-on-top runner**: Passed / Failed / Blocked / N/A per
  step and overall, screenshot capture (snip, paste or attach), File a Bug
  from a failure, results submitted as a test run under the existing plan.
- **History** — each case's last five outcomes as coloured dots; failures
  expand in place to show the result comment and linked bugs.
- **Execution reports** — pass rate, outcome bar, failures first — for a
  chosen set, a suite, or a folder.

### Work Manager
- A **To Do / In Progress / Done** board with drag-and-drop. Every move is
  **verified against what Azure DevOps actually saved**; a rule-blocked
  transition rolls back and shows the rule's own message.
- An Azure-DevOps-style full-window editor with **every process tab drawn
  from the organisation's own layout** — a Bug shows its RCA and Preventive
  Measures pages — and rich text that renders properly: markdown, tables,
  inline attachments.
- **Pull requests** — awaiting your review, yours, and everything active on
  a repo, with descriptions, linked work items and the build that ran.
  Read-only; voting and completing stay in Azure DevOps.

## Why it is safe to hand to a team

- **One DELETE, and it says so.** A test fails the build if any file in the
  Rust client issues a DELETE, with a single carved-out exception: deleting
  a test case, permission-gated and **permanent** — Azure DevOps has no
  recoverable deletion for test artifacts, and the app's own confirmation
  dialog says exactly that. Plans, suites, runs, attachments, comments,
  board items and pull requests are never removed.
- **The token never leaves the Rust core.** In memory for the session only;
  never returned to the UI, written to disk or logged. A build-time test
  fails if a token-shaped field ever appears in the generated interface.
- **No admin setup.** Interactive Microsoft sign-in through the system
  browser via the well-known Azure CLI public client — no PAT, no app
  registration. Tokens refresh silently; a mid-batch expiry re-auths and
  resumes.
- **Reads are free; writes are gated and rate-limited.**

## Install & update

Download `AzureDevOpsTestCaseManager.V2-win-Setup.exe` from the latest
release and run it — no toolchain needed. Installed apps check for updates
at launch and hourly, show a **Restart to update** banner, and apply the
update only when you click it. The update feed is the public
[releases repository](https://github.com/AvinAlwis/azure-devops-test-case-manager-v2-releases);
each release is also mirrored on this repository's Releases page.

## Repository

| Path | What |
| --- | --- |
| [`v2/`](v2/) | The app: Rust backend (`src-tauri/`), React frontend (`src/`), release scripts |
| [`v2/README.md`](v2/README.md) | Architecture, security model, authentication, develop / build / release |
| [`docs/`](docs/) | Design specs, implementation plans and the presentation brief |
| [`CLAUDE.md`](CLAUDE.md) | Working notes for coding assistants — conventions and the invariants the tests enforce |

```sh
cd v2
npm install
npm run tauri dev            # run the app
npm test                     # frontend suite (vitest)
cd src-tauri && cargo test --tests   # Rust suite
```

Releases ship with `v2/scripts/release-v2.ps1 -Version X.Y.Z`, which gates
on both suites, pushes source first, builds, packs with Velopack and
publishes.
