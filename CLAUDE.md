# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

This repository is a Tauri 2 desktop app (Rust backend, React 19 +
TypeScript frontend) for bulk-creating and editing Azure DevOps **Test
Case** work items against a Product Backlog Item, plus a Work Manager
board. Users sign in with Microsoft (Entra ID), pick an org/project/PBI,
build test cases by hand / by importing JSON / by editing existing ones,
review a queue, then upload them — each linked to the PBI and surfaced on
the board via a requirement-based test suite.

Read `README.md` for the feature set, security model and architecture.

**The old PyQt5 app (v1) is frozen on the `v1` branch** and is no longer
developed. It is not on `main` and takes no further updates; that
branch's own `CLAUDE.md` still describes it. v1 installs update themselves
through Velopack from their own public releases repo, so they are
unaffected by anything here.

## Commands

All of these run from the repository root:

```bash
npm install                  # install frontend deps
npm run tauri dev            # run the app
npm test                     # vitest (jsdom) - the frontend suite
npx tsc --noEmit             # typecheck
npm run build                # production frontend build (tsc + vite)
npm run tauri build          # build the desktop binary
cd src-tauri && cargo test --tests   # the Rust suite
```

**Every Rust test in this crate is an integration test under
`src-tauri/tests/`** — never a `#[cfg(test)]` module inside `src/`.
Only integration-test binaries get the Common-Controls v6 manifest link
args, and a test binary that links tauri dies at startup without them
(`STATUS_ENTRYPOINT_NOT_FOUND`). See the note at the top of
`tests/bindings.rs`.

`src/bindings.ts` is **generated** by `cargo test --test bindings` from the
Rust command signatures via tauri-specta. Never hand-edit it; change the
Rust and regenerate.

There is currently **no CI on `main`** — the workflow that existed tested
v1 and went with it. The suites above are the gate.

## Architecture

`README.md` has the full picture. The essentials:

- **Rust owns every secret.** Tokens live in `AuthState` in memory only,
  never persisted, never crossing the IPC boundary. `tests/bindings.rs`
  fails the build if a token-shaped field ever appears in the generated
  TypeScript.
- **All Azure DevOps calls go through `src-tauri/src/ado/`.** The client
  issues GET / POST / PATCH only — **there are no DELETE calls anywhere**,
  and that is a safety invariant, not an accident. Preserve it.
- **Commands live in `src-tauri/src/commands/`**, one module per area,
  exposed to the frontend through the generated bindings.
- **Screens live in `src/screens/`**, shared UI in `src/components/`,
  module-scope stores (`useSyncExternalStore`) in `src/lib/`.
- **Updates** come from the public GitHub releases repo, checked at launch
  and hourly. `updater/mod.rs` tries the GitHub API first and the
  `latest/download` mirror second - the comment there explains why that
  order matters.

## Testing notes that have cost real time

- **jsdom does no layout and no hit testing.** A full-screen overlay once
  swallowed every click while 655 tests passed, because `fireEvent.click`
  reaches an element a real browser has completely covered. Tests cannot
  catch that class of bug — a manual walk-through can.
- `src/App.test.tsx` mounts the whole app and is slow; it raises both
  vitest's `testTimeout` and Testing Library's `asyncUtilTimeout`, and it
  still has a documented load-induced flake. One failure that passes on a
  re-run is usually that; two different ones are not.
- `src/ui-consistency.test.ts` is a gate on this app's own UI
  conventions. Never weaken it to make code pass.

## Important conventions

- **Import/export format** is JSON (`src-tauri/src/import_parser/`). A
  populated case id flags an **update** of that exact work item; a blank
  one creates. This is the only reliable way to update — title matching is
  treated as a duplicate warning, never an update.
- **Steps XML.** Azure DevOps stores steps as a specific
  `Microsoft.VSTS.TCM.Steps` blob — always build/parse it via
  `src-tauri/src/steps_xml.rs`, where step ids start at 2.
- **Theming.** Colours come from CSS custom properties and Tailwind tokens
  (`text-text`, `bg-surface`, `border-border`, `text-success`…). Never
  hardcode a colour; the consistency gate checks this.
- **Icons** come from the shared vocabulary in `src/lib/actionIcons.ts`,
  named for what the button DOES, not what it looks like.
- **User-facing errors name no URL.** `reqwest`'s `Display` is `error
  sending request for url (https://dev.azure.com/...?$top=500)` - the
  endpoint and its query string, and nothing anyone can act on. Never pass
  a transport error's `to_string()` to the user: log the raw error
  (`applog`, which is what a bug report ships) and return one of the
  sentences in `ado/transport.rs` via `network_error`. Each says what to
  try and points at Settings → Logs. `tests/ado_network.rs` enforces it.
- **Seeing an error state** is what `src/dev/faults.ts` is for: the dev
  panel's "Force a failure" arms the next command - or every command - to
  come back as a chosen `AdoError`. It patches the bindings rather than
  raising a toast, so the failure travels the real path and the screen's
  own handling is what you are looking at.

## Working conventions (this machine)

- **Commits: use a Bash heredoc — `git commit -F - <<'EOF' … EOF` — never
  PowerShell message flags.** All four PS 5.1 failure modes have happened
  here: embedded quotes in `-m` split into pathspecs; a here-string piped
  to `-F -` puts a UTF-8 BOM in the subject; a here-string as an argument
  parses as a pathspec; and a failing cleanup command later in the same
  block aborts it so the commit silently never lands. Confirm with
  `git log -1`.
- **Releases: run `scripts/release-v2.ps1 -Version X.Y.Z`.** It gates
  (cargo test + vitest + a production build), pushes source **first**,
  builds, packs with Velopack, and publishes to GitHub. Never publish
  without the source pushed.
- **The version lives in three places** and the release script refuses if
  they disagree: `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`,
  and a matching entry in `src/lib/changelog.ts`.
- **Big modules** (`QueueSection.tsx`, `App.tsx`, `ImportFile.tsx`,
  `Settings.tsx`) are large: Grep for the region, then a bounded Read —
  don't re-read whole files repeatedly.
- **The machine is shared with the user.** Run one build or test command at
  a time and wait for it; never run two suites at once.
