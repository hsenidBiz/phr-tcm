# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A desktop GUI app (PyQt5) for bulk-creating and editing Azure DevOps **Test Case**
work items and linking them to a Product Backlog Item (PBI). Users sign in with
Microsoft (Entra ID), pick an org/project/PBI, build test cases manually / by
importing a spreadsheet / by editing existing ones, review a queue, then create
them — each linked to the PBI and surfaced on the board via a requirement-based
test suite.

## Commands

```bash
pip install -r requirements.txt     # install runtime deps
pip install -r requirements-dev.txt # install test/lint deps (pytest, ruff)
python main.py                       # run the app
pytest                               # run the unit tests (tests/ — pure logic only, no Qt)
ruff check .                         # lint (config in pyproject.toml)
build.bat                            # build a one-file windowed .exe via PyInstaller (Windows)
```

Tests cover the Qt-free modules only (`import_parser`, `xml_builder`,
`TestCase.is_valid`, `settings`); GUI code has no automated tests. CI
(`.github/workflows/ci.yml`) runs ruff + pytest on `windows-latest` for every
push to `master` — keep it green, since users pull `master` directly. The app
is Windows-first (high-DPI handling, `CREATE_NO_WINDOW` git calls, `build.bat`).

## Distribution & auto-update

The app is distributed as a **git clone of the private GitHub repo on each
machine**, not as a packaged binary for normal use. `app/utils/updater.py` does
an in-app `git fetch` + `--ff-only` merge using the machine's own git
credentials (no tokens embedded), then relaunches. Auto-update is disabled when
frozen by PyInstaller (`sys.frozen`) or when `.git` is absent. Because of this,
a normal `git pull` is how users get changes — keep `master` shippable.

## Architecture

### Layered, GUI-on-top
- `main.py` — builds `QApplication`, enables high-DPI scaling (**must run before
  QApplication is created**), constructs `AppState`, shows `MainWindow`.
- `app/auth/` — Microsoft sign-in (`msal_auth.py`) + in-memory token/JWT handling
  (`token_manager.py`).
- `app/api/devops_client.py` — **all** Azure DevOps REST calls.
- `app/models/test_case.py` — `TestCase` / `Step` dataclasses (the core domain type).
- `app/utils/` — non-GUI helpers: spreadsheet import/export, steps-XML, settings
  persistence, theming, background `Worker`, updater, members cache.
- `app/gui/` — PyQt5 screens/widgets. `main_window.py` is the `QStackedWidget`
  orchestrator (Auth → Config → Main → Review → Progress pages).

### `AppState` is the single shared mutable state
Constructed once in `main.py` and passed into every screen. Holds the
`TokenManager`, `DevOpsClient`, current PBI/area/iteration, the pending `queue`
of `TestCase`s, resolved test plan/suite, and shared caches (`existing_cases`,
`cached_team_members`). **Nothing in `AppState` is written to disk.** Screens
communicate upward by emitting Qt signals that `MainWindow` wires to navigation
and queue management — they do not call each other directly.

### Page/screen flow (the `QStackedWidget` in `main_window.py`)
`PAGE_AUTH` → `PAGE_CONFIG` → `PAGE_MAIN` (tabbed: Manual Entry / Import File /
Edit Test Cases) → `PAGE_REVIEW` → `PAGE_PROGRESS`. The three Main tabs all feed
the same `app_state.queue`; the footer Review button and queue badge live in
`MainWindow`.

### Threading model — never block the GUI thread
Two patterns, both keeping API/network work off the Qt main thread:
- **`app/utils/worker.py` `Worker`** (`QRunnable` on the global `QThreadPool`):
  one-shot calls that emit `result`/`error` signals. Used for org/project
  discovery, update checks, re-auth, etc.
- **`ProgressScreen`'s `CreationWorker`** (`QObject` moved to a `QThread`): the
  long create/update loop, emitting `progress`/`token_needed`/`suite_ready`/
  `finished`. It blocks on `threading.Event`s for re-auth (`token_needed`) and
  abort.

### Safety invariant — this tool never destroys data
`DevOpsClient` only ever issues **GET / POST / PATCH** — there are **no DELETE
calls anywhere**, and several docstrings assert this. PATCH uses the `add` op
(create-or-replace). When changing the client, preserve this: do not introduce
DELETEs, and don't let an update wipe fields the user left blank (see
`update_test_case_from_model` — blank imported values are intentionally skipped).

### Authentication & token refresh
Sign-in uses MSAL via the **well-known Azure CLI public client id** (no app
registration needed in the tenant) with the Azure DevOps `.default` scope. The
token cache is **in-memory only** (`TokenManager` never persists it). The header
getters (`get_json_headers` / `get_patch_headers`) call `_ensure_fresh()`, which
silently refreshes via MSAL when the JWT is near expiry — so API worker threads
get a fresh token transparently. On a hard 401, `DevOpsClient` raises
`TokenExpiredError`, which the create loop turns into an interactive re-sign-in
prompt and then retries the in-flight item (without re-creating it).

### Board visibility via requirement-based test suites
Creating + linking a Test Case isn't enough for it to show on the board's test
count — there must be a **requirement-based test suite** bound to the PBI. Before
creating, `CreationWorker._ensure_suite()` calls
`DevOpsClient.ensure_requirement_suite()` to find-or-create the area-matched test
plan and the PBI's requirement suite. This is **best-effort**: failures only log
a warning and never block creation. Suites auto-populate from the PBI's
`Tested By` links, which `link_to_pbi()` already creates.

### Error types (`app/api/devops_client.py`)
`_handle()` maps HTTP status to typed exceptions the GUI handles distinctly:
`TokenExpiredError` (401 → re-auth), `RateLimitError` (429 → wait `Retry-After`
then retry), `PermissionError` (403), `LookupError` (404), `RuntimeError` (other).

## Important conventions

- **Spreadsheet round-trip (`app/utils/import_parser.py`).** The canonical
  import/export format is a 9-column sheet
  (`TestCaseID, TestCaseName, StepNumber, StepAction, StepExpected, Tags,
  AutomationStatus, ModuleValue, Preconditions`). One row per step; per-case
  fields (name, ID, tags…) sit only on each case's first step row, blank on
  continuation rows. A populated **`TestCaseID` flags the case as an UPDATE**
  (`TestCase.update_id`) of that exact work item; a blank ID creates a new one.
  This is the only reliable way to update — title matching is treated as a
  duplicate warning, never an update.
- **Steps XML.** Azure DevOps stores steps as a specific
  `Microsoft.VSTS.TCM.Steps` XML blob — always build/parse it via
  `app/utils/xml_builder.py` (`build_steps_xml` / `parse_steps_xml`), where step
  ids start at 2.
- **Persistence is deliberately minimal.** `app/utils/settings.py` writes only a
  whitelisted set of keys (`_ALLOWED_KEYS`) to `~/.devops_tc_creator/` — the
  **Bearer token is intentionally excluded**. Also stored there: the draft queue
  (restored on next launch) and the 24 h team-members cache. Add a key to
  `_ALLOWED_KEYS` before expecting `save_settings` to persist it.
- **Theming.** Light/dark is centralized in `app/utils/theme.py` (token dicts +
  `btn_*_qss` builders). Every screen exposes `refresh_theme()`, called by
  `MainWindow._refresh_all_themes()` on toggle. Use the token dict and the shared
  button-style builders rather than hardcoding colors, so static styles and
  `refresh_theme()` can't drift apart.
- **Automation status** is constrained to `"Not Automated"` or `"Planned"`
  (validated in `TestCase.is_valid()` and the import parser).

## Working conventions (this machine)

- **Commits: use a Bash heredoc — `git commit -F - <<'EOF' … EOF` — never
  PowerShell message flags.** All four PS 5.1 failure modes have happened here:
  embedded quotes in `-m` split into pathspecs; a here-string piped to `-F -`
  puts a UTF-8 BOM in the subject; a here-string as an argument parses as a
  pathspec; and a failing cleanup command later in the same block aborts it so
  the commit silently never lands. Confirm with `git log -1` after committing.
- **Releases: run `.\scripts\release.ps1`** (patch bump by default; `-Bump
  minor|major`, `-Version X.Y.Z`). It runs ruff+pytest, bumps `app/version.py`,
  pushes source to private master FIRST, builds via `build.ps1`, verifies the
  frozen-build QtSvg gate, then publishes to the public releases repo using
  `gh auth token` in-process. Never publish without the source pushed.
- **Big GUI modules** (`main_window.py`, `mywork_screen.py`, `test_runner.py`,
  `run_screen.py`, `config_screen.py`) are 50–90 KB: Grep for the region, then
  bounded Read (offset/limit) — don't re-read whole files repeatedly.
- **Lint/format:** `python -m ruff check .` (line-length 120 in pyproject).
- **Headless GUI verification:** offscreen full-window construction; end smoke
  scripts with `os._exit(0)` (Qt's at-exit teardown segfaults offscreen).
  Offscreen has no font rasterizer — text visuals need a real display.
