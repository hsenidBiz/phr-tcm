# Per-repo workspace — design

**Date:** 2026-09-02 · **Target:** Test Case Manager V2 (`v2/`, master)
**Premise (owner's words):** "constrain the tools per repo rather than globally."
Decisions below were confirmed interactively with the owner on 2026-09-02.

## Problem

Everything the AI side of the app writes or registers is global to the
machine: the `tcm-testcases` and `phr-db-mcp` MCP servers go into user-scope
config (`claude mcp add --scope user`, `~/.claude.json`, each editor's global
config), the `/tcm:*` skills go into `~/.claude/commands/tcm/`, and the
finished test-case JSON goes wherever the assistant was told. A developer
working across several repositories gets one global tool set and files
scattered across arbitrary folders.

## Goals

1. A **working repository** is chosen in the app. Everything below is scoped
   to it.
2. `<repo>/.test-cases/` is **the** home of test-case JSON for that repo:
   - a file picked in Import File is **copied** there (and imported from the
     copy), unless it already lives there;
   - a writing job started through `begin_test_case_writing` creates the
     folder, defaults the output there, and **refuses** an output path
     outside it; the app imports and watches the file from there.
3. The `/tcm:*` skills are written **per repo** to `<repo>/.claude/commands/tcm/`.
4. MCP registration is **per repo** for tools that support a project config:
   Claude Code (`<repo>/.mcp.json`), Cursor (`<repo>/.cursor/mcp.json`),
   VS Code (`<repo>/.vscode/mcp.json`). Claude Desktop and Windsurf have no
   project scope and **stay global**, labelled as such.
5. The **AI Bridge tab is unavailable until a working repository is set**;
   every other tab (Run Tests, View Test Cases, Suites, Work Manager…) works
   without one.
6. Changing the working repository **re-detects** registration state against
   the new repo, so Register is offered again wherever the servers are not
   registered there.

## Decisions (owner-confirmed)

| Question | Decision |
|---|---|
| Where do per-repo skills go? | `<repo>/.claude/commands/tcm/*.md` (project-scoped slash commands). `CLAUDE.md` is not touched. |
| Tools with no project config (Claude Desktop, Windsurf)? | Keep today's global registration; the UI labels the row "global". |
| Existing global registrations when a repo is registered for the first time? | The app's **own** entries are removed automatically: user-scope `tcm-testcases` / `phr-db-mcp`, and marker-stamped files under `~/.claude/commands/tcm/`. Nothing the user wrote is touched. |
| DB connection string (password) landing in a committable `.mcp.json`? | Written with real values so it works; `.mcp.json` is added to `<repo>/.git/info/exclude` (local-only, invisible in the repo). Non-git folders: nothing to exclude. |

Decisions made by the planner (not asked, reasonable defaults):

- **One working repository at a time**, global to the app (persisted in
  localStorage under `tcm-v2-working-dir`), not per org/project — a repo can
  serve several projects, and the owner said "the working directory".
- **Import File without a working repository** behaves exactly as today
  (no copy). With one set, the copy happens and the copy is what is
  imported and watched.
- **Name collisions on copy**: same bytes already there → reuse that file;
  different bytes → `name-2.json`, `name-3.json`… Never overwrite.
- **`begin_test_case_writing` without a working repository** answers a
  blocked status telling the assistant to have the developer pick one on
  the AI Bridge tab. A bare file name as `output_path` resolves into
  `.test-cases/`; a full path is accepted only if it is inside it.
- `.test-cases/` is **not** gitignored by the app — committing the cases
  with the repo is the point of per-repo.

## Behaviour by surface

**AI Bridge tab.** A "Working repository" card sits first. Unset → only that
card renders (with the reason). Set → the rest of the tab as today, with:
detection keyed on the repo; each tool row labelled "in this repo" or
"global"; Register/Unregister acting on the repo's config; the manual
`claude mcp add` snippet shown with `--scope project` and "run inside the
repository".

**Bridge / MCP.** `BridgeContext.working_dir` is pushed from the frontend
with the rest of the context. `/begin` phase 1 returns the questions with
`output_dir` and a `suggested_output_path` (`<repo>/.test-cases/<slug>.json`);
phase 2 resolves a bare name into the folder, refuses paths outside it, and
creates the folder before checking. The announced intake path (watched by
the app) is the resolved one.

**Import File.** After the picker, if a working repository is set and the
file is outside `.test-cases/`, it is copied in and the copy is parsed,
recorded as recent, and watched. The toast says so.

**Registration (Rust).** `register_server` / `unregister_server` take the
working directory. Project-capable tool + repo → project config (Claude Code
via `claude mcp add --scope project` run with the repo as cwd, falling back
to editing `<repo>/.mcp.json`); skills to `<repo>/.claude/commands/tcm/`;
then the global copies of *our* entries are retired. Project-capable tool
without a repo → refused ("pick a working repository first"; the UI never
offers it). Global-only tool → unchanged.

## Non-goals

- No change to how the queue, watches (`tcm-v2-watch:<org>/<pbi>`), drafts or
  recents are keyed — they keep working with the new paths.
- No per-repo *settings* beyond the folder: org/project/PBI selection is
  still global app state.
- No rewriting of `CLAUDE.md`.
- No detection of "which repo the assistant is running in" from the MCP
  proxy's cwd — the app's choice is authoritative.

## Testing approach

Pure Rust rules (`workspace`, `ai_tools`, `intake`) are unit-tested against
temp directories, matching the existing `tests/ai_tools.rs` /
`tests/intake.rs` style. Frontend behaviour is covered by vitest with
`mockIPC` (AiBridge gating and argument passing, ImportFile copy path, App
context push). The I/O of actually running `claude mcp add` is exercised
manually, as today.
