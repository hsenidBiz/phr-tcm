# Playwright Tab Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** A Playwright tab, gated by Advanced Features, that:
- reads the team's Playwright repo from the clone folder;
- runs its tests the way `npm run qa` does;
- manages its accounts through `npm run users`;
- keeps the repo's copies of our Auto Run scripts up to date.

**Architecture:**
- A Rust module `pw_repo/` holds:
  - `structure.rs`: reads the repo;
  - `process.rs`: runs repo commands in a job, streams output, one at a time;
  - `runner.rs`: builds the tree, the run list, the dry count and the arguments for each action;
  - `users.rs`: arguments for `npm run users` and parsing its answers;
  - `sync.rs`: the export record and case states.
- Tauri commands live in `commands/pw_repo.rs`, and the generated bindings carry them to the webview.
- The webview screen is `src/screens/Playwright/`, with Runner, Accounts and Sync sections. It reuses the Export dialog.

**Tech Stack:** Rust (Tauri 2), React 19 + TS, vitest.

**Spec:** `docs/superpowers/specs/2026-10-12-playwright-tab-design.md` (approved 2026-10-12).

## Global Constraints

- **Never run git.** The app only ever runs `npx playwright …` from the repo's own `node_modules`, and `npm run users -- …`, both in the clone.
- **What the app writes in the repo:**
  - It never writes `navigation.json`, `qa-options.json`, data files, or refactored specs under `specs/`.
  - It writes `users.json` only through `npm run users … --apply`.
  - The only runner file it writes is `.qa-cli/run.list`.
- **Repo processes.**
  - Every process runs in its own Windows job (`browser/tree.rs` pattern), ended on Stop or app exit.
  - Only one repo command runs at a time.
- **Secrets.** Passwords are never logged and never sent to the webview except on an explicit Show. No secrets, hosts or query strings in logs. No HTTP DELETE to Azure DevOps.
- **Advanced Features off** means no tab and every command refused.
- **Tests.**
  - Rust tests are integration tests in `src-tauri/tests/suite/`, with serial locks where needed.
  - Normalise CRLF in tests that read source.
- **Code rules.**
  - `src/bindings.ts` is generated only.
  - Use theme tokens and icons from `actionIcons.ts`. Never weaken `ui-consistency`.
- **No em or en dashes.** Run one build or test command at a time.
- **Commits** use a Bash heredoc, are staged by name, and end with the implementer's `Co-Authored-By`.

## Review Focus

1. A run whose dry count disagrees with the picked tests never starts. Test: `a_count_mismatch_refuses_to_run`.
2. Stop ends every process of a run, and quitting the app ends a running one. Tests: `stop_ends_the_whole_run`, `exit_ends_a_running_repo_command`.
3. A password never reaches a log or the webview without Show. Test: `a_password_is_never_logged_or_sent_unasked`.
4. Sync Update never changes an Amendment, "Added by engineer" block, data file, `users.json`, `navigation.json` or a refactored spec. Test: `update_leaves_the_repos_own_files_alone`.
5. A clone without `node_modules` or Node is told so, and nothing is started. Test: `a_clone_without_node_modules_starts_nothing`.

---

### Task 1: The tab, the repo's structure, and the health line

**Files:**
- `src-tauri/src/pw_repo/{mod.rs,structure.rs}`
- `commands/pw_repo.rs`
- the bindings
- `src/screens/Playwright/` (shell and health line)
- app navigation and gating
- `src/screens/AutoRun` (the Export menu item opens the tab)

**Interfaces:**
- `pw_repo_structure(clone) -> RepoStructure`, where `RepoStructure` holds:
  - `features[]`, each with country, side, module, feature, and its counts of specs and test cases;
  - `generated_ids`;
  - `health: Vec<HealthIssue>`.
- `HealthIssue` is one of `NotTheRepo`, `NoNodeModules`, `NoNode`, `NoQaOptions`.
- The clone folder setting and recognition reuse `pw_export/clone.rs`.
- The tab:
  - shows only with Advanced Features on;
  - lets the person pick or change the clone folder;
  - shows the health line and a structure summary;
  - holds the Export dialog.

- [ ] Tests:
  - Rust: reading a fixture repo; each health issue; refusal with Advanced Features off.
  - vitest: the tab is hidden when off; the health line; the Export item opens the tab.
- [ ] Commit `feat(v2): a Playwright tab that reads the team's repo from its clone folder`.

### Task 2: Running a repo command safely

**Files:** `src-tauri/src/pw_repo/process.rs`, `commands/pw_repo.rs`, the bindings.

**Interfaces:**
- `start(clone, program, args) -> RunId`:
  - spawns in a new Windows job;
  - streams stdout and stderr, ANSI stripped, as `PwOutput { run, line }` events;
  - ends with `PwDone { run, code }`.
- Only `npx` and `npm` are allowed as programs, resolved from the repo or PATH. Anything else is refused.
- `stop(run)` ends the job.
- One run at a time. A second start is refused with a sentence naming the running one.
- App exit ends a running command, through the exit path `close_autorun_browsers` uses.

- [ ] Tests:
  - `stop_ends_the_whole_run`, using a test seam that spawns a child that spawns a grandchild;
  - `exit_ends_a_running_repo_command`;
  - a second start is refused;
  - only `npx` and `npm` are allowed;
  - output is ANSI stripped.
- [ ] Commit `feat(v2): repo commands run in their own job, one at a time, with their output streamed`.

### Task 3: The test runner

**Files:** `src-tauri/src/pw_repo/runner.rs`, `commands/pw_repo.rs`, `src/screens/Playwright/Runner*.tsx`.

**Interfaces:**
- `pw_list(clone) -> TestTree`. It runs `npx playwright test --list --reporter=json`, then parses the result into country, side, module, feature, spec and test. Each test carries its ADO id, from the `// ADO <id>` marker or `index.json`, and its last result if the last report has one.
- `pw_run(clone, picked, action, mode)`:
  1. Write `.qa-cli/run.list` the way `qa.mjs:60-81` does.
  2. Dry count with `--list --test-list` and compare with the picked count. A mismatch refuses with both numbers.
  3. Run the action, using the argument building of `qa.mjs:83-99`, `--last-failed` for re-run, and the options from `qa-options.json`. Use the repo's defaults when that file is missing; never write it.
- **Modes** are hidden, shown and debug. Debug with more than one test needs `confirm: true`.
- `pw_preview(...)` returns the run list and the command without running anything.
- `pw_reports(clone)` lists `reports/<runId>/` folders by date. Opening one uses the system browser on the report's `index.html`, the same way the app opens files elsewhere.
- **The screen:**
  - the tree with ticks remembered per clone, plus Tick all and Clear;
  - action buttons and the mode picker;
  - the output panel with Stop;
  - the report list.

- [ ] Tests:
  - Rust:
    - building the tree from a saved list JSON fixture;
    - ADO ids from markers and from the index;
    - the run list's content;
    - `a_count_mismatch_refuses_to_run`, with a fake process seam;
    - the arguments for every action and mode;
    - debug with more than one test refused without `confirm`;
    - `qa-options.json` read but never written.
  - vitest: ticks; buttons disabled while running; the output panel; Stop; the report list.
- [ ] Commit `feat(v2): run the repo's tests from the Playwright tab, with the same count check as qa`.

### Task 4: Test accounts

**Files:** `src-tauri/src/pw_repo/users.rs`, `commands/pw_repo.rs`, `src/screens/Playwright/Accounts*.tsx`.

**Interfaces:**
- `pw_users_list(clone) -> Vec<RepoAccount>`. It runs `npm run users -- list --json`.
- `RepoAccount` holds key, username, employee number, display name, title and references. It holds no password.
- `pw_users_password(clone, key) -> String` exists only for Show. It reads through `npm run users`, or by reading `users.json` if the CLI has no read. The app keeps it in memory only and never logs it.
- `pw_users_change(clone, change, apply: bool)`:
  - `change` is one of `Add`, `Set`, `Remove` or `Rename { cascade: true }`.
  - It builds `npm run users -- <cmd> … [--apply]`.
  - Without `apply` it returns the command's dry-run output.
  - With `apply` it runs the change and then lists the accounts again.
  - The command's own refusals come back as they are.
- Passwords are passed to the command in a way that keeps them out of logs and process listings where possible. Check how the users CLI accepts a password (an argument, stdin or an env var); use stdin or env if supported. If only an argument works, document that the password is visible in the process list for the command's lifetime.
- **The screen:**
  - the list;
  - Add and Edit forms;
  - Rename, with a preview of what the cascade rewrites;
  - Delete;
  - Show password.

- [ ] Tests:
  - Rust:
    - the arguments for each change;
    - parsing the list JSON;
    - `a_password_is_never_logged_or_sent_unasked`;
    - a refusal passed through;
    - dry run without `apply`.
  - vitest: the forms; the preview step before applying; Show password.
- [ ] Commit `feat(v2): manage the repo's test accounts from the Playwright tab through npm run users`.

### Task 5: Sync

**Files:**
- `src-tauri/src/pw_repo/sync.rs`
- `pw_export/export.rs`, so each export records what it wrote
- `commands/pw_repo.rs`
- `src/screens/Playwright/Sync*.tsx`

**Interfaces:**
- **The export record.** A per-clone, per-project store at `<app data>/pw-sync/<clone-hash>-<project>.json`, written atomically. For each case it holds the script fingerprint, the time, and the files written. Every export through the existing all-or-nothing path updates it.
- `pw_sync_states(clone, org, project) -> Vec<SyncRow>`, where each row is one of `Current`, `OutOfDate { why }`, `NotInRepo`, `NeverExported`.
  - The fingerprint is a hash of the saved script's steps, area, account and marks.
  - `NotInRepo` means the repo has no section or raw spec for the case.
- `pw_sync_update(clone, org, project, cases)` re-exports the given cases through the existing export, so that:
  - test-case sections are spliced;
  - raw specs are replaced;
  - `index.json` entries are updated.
- **The screen:** the list with states; Update out of date; Update ticked; and per updated case, "needs /gen-test stages 5 and 6 in the repo".

- [ ] Tests:
  - the states: a repair makes a case out of date, and an unchanged one stays current;
  - `NotInRepo`;
  - `NeverExported`;
  - update re-exports only the given cases;
  - `update_leaves_the_repos_own_files_alone`, which checks Amendment and "Added by engineer" blocks, data, users, navigation and `specs/` byte for byte;
  - the record is written atomically;
  - vitest: states; Update.
- [ ] Commit `feat(v2): a Sync view keeps the repo's copies of our scripts up to date`.

### Task 6: How To Use

- [ ] `docs-site/src/content/`: add a Playwright section, hidden with Advanced Features off. This uses the Round C mechanism; if Round C has not landed yet, add the section with the same gating marker the Auto Run sections use.
- [ ] Run `npm run docs:build`, then `npx vitest run docs-site`. Do not retake shots unless a positions test needs it.
- [ ] Commit `docs(v2): the guide covers the Playwright tab`.

### Final gate

- [ ] Run, one at a time: `cd src-tauri && cargo test --tests`, then `cargo test --test bindings`, `npx tsc --noEmit`, `npm test` and `npm run build`.
- [ ] Opt-in live check: one small run in a real clone of the repo (`#[ignore]`).
- [ ] Hand check owed:
  - a real run of a few tests from the tab;
  - an account rename that cascades;
  - a Sync update after a repair.
