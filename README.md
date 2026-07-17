# Azure DevOps Test Case Manager

> ### 🚀 V2 is the current product
> The app has been rewritten on **Tauri 2 + Rust + React/TypeScript** and
> lives in [`v2/`](v2/) — smaller install, faster start, the same
> no-DELETE / in-memory-token safety, plus a Work Manager board with an
> ADO-style work-item editor (all process tabs, markdown, inline images),
> selectable full-UI themes, smart grouping, a multi-level Test Suites
> browser, a View tab with locally-saved per-case comments, per-case run
> history, git-style update diff previews with a two-stage confirm, and
> one-click HTML execution reports.
> **Start with [`v2/README.md`](v2/README.md)** for features, architecture,
> and the dev/release workflow. Installers ship from
> [azure-devops-test-case-manager-v2-releases](https://github.com/AvinAlwis/azure-devops-test-case-manager-v2-releases).
>
> The PyQt5 app documented below is **V1 — in feature freeze** since v3.3.0
> (which prompts its users to move to V2). It keeps working and still
> receives critical fixes only.

A Windows desktop app for creating, editing, importing, and **executing** Azure
DevOps test cases against a Product Backlog Item (PBI) — without leaving a single
window. Built with PyQt5; signs in with your own Microsoft account; ships and
updates itself via Velopack.

> Internal tool. V1 current version: **3.3.0** (feature freeze).

---

## What it does

Point it at a PBI/work item ID and it auto-detects the organization, project,
area path, iteration path, and whether a test plan/suite already exists — then
lets you author, import, edit, and run test cases for it.

### Authoring
- **Create test cases** — title, preconditions, and ordered steps (action /
  expected result).
- **Import & bulk-update** — load test cases from Excel/CSV using a downloadable
  template; create new cases or update existing ones by Test Case ID.
- **Edit existing cases** — pull a PBI's cases, edit steps/preconditions, set
  *Assigned To*, rename, and export.
- **Review & confirm** — a full review screen before anything is written.
  Nothing is sent to Azure DevOps without explicit confirmation.

### Execution (Test Runner)
- Assemble a **session** of cases and run them in an always-on-top runner.
- Mark **Pass / Fail / Blocked / Not Applicable** per step and overall.
- Attach **screenshots** — paste from the clipboard at full quality, or capture
  the screen (multi-monitor and mixed-DPI aware).
- Add per-case **comments**; previously uploaded screenshots are fetched and
  shown as you move through cases.
- **Create a Bug** work item directly from a failure.
- **Resume** an interrupted run.
- **Submit results** as a new test run under the existing test plan, with a
  blocking progress overlay. Cases whose data didn't change are skipped, so a
  re-run doesn't record duplicate identical results.

### Interface
- Light and dark themes.
- Custom frameless window chrome (dark title bar, rounded corners, app-wide
  border) applied to the main window, the Test Runner, and dialogs.
- Modern flat tables with a subtle row-hover highlight, themed SVG icons, and a
  recently-used configurations list.

---

## Security model

This app talks to Azure DevOps with your credentials, so it is deliberately
conservative:

- **No DELETE calls — anywhere.** The app never deletes work items, test cases,
  runs, or attachments.
- **Token in memory only.** The Microsoft access token is held in memory for the
  session; it is never written to disk and never logged.
- **Confirm before every write.** A review/confirm screen precedes any create or
  update; reads happen freely, writes never do silently.
- **Rate-limited creates.** A short delay between create calls avoids hammering
  the API.

## Authentication

Sign-in is **interactive MSAL** (a browser window) using the well-known Azure CLI
public client — no Personal Access Token, no app registration, and no SSH key
required. The token refreshes silently during a session; if a long batch hits a
401 mid-run, you're prompted to re-authenticate and the queue is preserved.

---

## Install & update

The app is distributed as a **Velopack** installer and updates itself
automatically from its releases feed — install once and new versions apply on
launch. No Python or manual setup needed for end users.

## Run from source (development)

Requires Python 3.x on Windows.

```sh
pip install -r requirements.txt
python main.py
```

## Build a release

`build.ps1` reads the version from `app/version.py`, packages the app with
PyInstaller, and produces a Velopack release.

```powershell
# 1. bump VERSION in app/version.py
# 2. commit + push the source
.\build.ps1            # PyInstaller one-folder -> vpk pack
vpk upload github      # publish the delta to the public releases repo
```

---

## Project structure

```
main.py                     App entry point (QApplication, font, logging, icon)
app/
  version.py                Single source of truth for the version
  auth/                     MSAL sign-in + in-memory token manager
  api/devops_client.py      Azure DevOps REST client (read + create/update only)
  models/test_case.py       Test case / step data models
  gui/
    main_window.py          Window chrome, header, tabs, queue pill, theming
    auth_screen.py          Sign-in screen
    config_screen.py        PBI lookup, org/area/iteration detection, recents
    manual_entry.py         Create test cases by hand
    import_screen.py        Excel/CSV import + bulk update
    edit_screen.py          Edit existing cases
    review_screen.py        Review & confirm before writes
    progress_screen.py      Batched create/update with resume + bug-on-fail
    run_screen.py           Build an execution session
    test_runner.py          Execute tests, screenshots, submit results
    frameless.py            Custom window chrome (frameless + rounded corners)
    delegates.py            Shared item-view delegates (row hover highlight)
  utils/
    theme.py                Palette, tokens, QSS builders
    icons.py                Themed SVG icon rendering
    settings.py             Persisted settings (atomic writes)
    import_parser.py        Excel/CSV parsing + template generation
    worker.py               QThreadPool worker + signals
    updater.py              Velopack update integration
resources/                  App icon + bundled SVG icon set
scripts/make_app_icon.py    Regenerates the app icon
```

## Tech stack

PyQt5 · MSAL · Azure DevOps REST API · openpyxl · PyInstaller · Velopack
