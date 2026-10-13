# A Playwright tab: run, accounts and sync for the team's Playwright repo

Date: 2026-10-12
Status: design approved in conversation; this file is the written spec for review.
Builds on: `docs/superpowers/specs/2026-10-07-playwright-export-design.md` (Export to Playwright).

## Goal

The app becomes a friendlier front end for the PHR-PLAYWRIGHT-AUTOMATION repo:
- it reads the repo's structure from the clone folder;
- it runs the repo's tests the way its `npm run qa` console does;
- it manages the repo's test accounts through `npm run users`;
- it keeps the repo's copies of our Auto Run scripts up to date.

## Owner decisions

| Question | Decision |
|---|---|
| Which repo tools get a screen | The test runner (`npm run qa`) and test accounts (`npm run users`). Test data and spec tools stay in the repo. The AI steps (`/gen-test`, `/edit-spec`, `/test-data`) stay in Claude Code. |
| Sync direction | App to repo only. Our scripts stay the source. The app tracks which repo specs came from which script, shows which are out of date, and re-exports only those. |
| Running the repo's commands | The app runs the repo's own commands in the clone (`npx playwright`, `npm run users`) and streams their output. It never runs git. |
| Placement | A new Playwright tab beside Auto Run, shown only with Advanced Features on. Export to Playwright moves into it. |

## Out of scope

- Repo to app sync: importing or merging specs edited in the repo.
- Test data screens (`data:*`) and spec tools (`spec:*`, `lint:tests`).
- Running `/gen-test` or any AI step.
- Any git operation: commit, pull or push.
- Writing `navigation.json`. Writing `users.json` other than through `npm run users`.

## 1. The tab and the repo folder

- **Where.** A Playwright tab beside Auto Run, shown only when Advanced Features is on.
- **Clone folder.** It uses the existing clone-folder setting and recognition (`pw_export/clone.rs`, four marker files). It can be set or changed from the tab.
- **Reading the structure.** On opening, the app reads:
  - the `suites/<country>/<side>/<module>/<feature>/` folders;
  - `src/users/users.json` (read only);
  - `src/navigation.json` (read only);
  - `suites/_generated/index.json`;
  - the `## <id>` headings in the test-case files.
- **Health line.** One line flags anything that stops the rest working:
  - not the repo;
  - no `node_modules` (with a hint to run `npm install` there);
  - Node not found;
  - `qa-options.json` missing (with defaults used).
- **Export to Playwright** moves into the tab. The Auto Run menu item stays as a shortcut that opens it.

## 2. The test runner

- **The tree.** The app runs the repo's own `npx playwright test --list --reporter=json` in the clone, as `cli/inventory/playwright-list.mjs` does, and shows the result as a folding tree: country, side, module, feature, spec, test.
  - Each test shows its ADO id, read from the `// ADO <id>` marker or `index.json`, and its last result.
  - Refresh lists the tests again.
- **Picking.** Tick tests or branches, Tick all and Clear. The ticks are remembered per clone.
- **Actions.**
  - Run picked.
  - Run all.
  - Re-run failures (`--last-failed`).
  - Browser mode: hidden, shown or debug.
  - Preview, which shows the run list and the command.
- **The same safety as `qa`.**
  - The app writes `.qa-cli/run.list` exactly as `qa.mjs` does.
  - It dry-counts with `--list --test-list`, and refuses to run when the count does not match (`cli/run/verify-count.mjs`).
  - A debug run of more than one test asks first.
  - Run options come from the repo's `qa-options.json`. The app never writes that file.
- **While it runs.** Output streams into a panel, ANSI colours are stripped, and there is a Stop button that ends the process tree.
  - When the run ends, the run's `reports/<runId>/` HTML report opens.
  - Past reports are listed by date to reopen.
- **What the app runs.** Only `npx playwright …` from the repo's own `node_modules`, in the clone. Never git. The repo's own files are never changed by the runner apart from `.qa-cli/run.list`, which is gitignored.

## 3. Test accounts

- **The list.** It comes from `npm run users -- list --json`: key, username, employee number, display name, title, and where each is referenced.
- **Changes** all go through `npm run users -- <add|set|rm|rename --cascade> … --apply`, so the repo's own checks apply:
  - a rename rewrites every reference;
  - a referenced account cannot be deleted;
  - a changed username or password clears that account's saved session.

  Before applying, the app shows what the command will change (a dry run without `--apply`, where the command supports it).
- **Passwords** are hidden until Show. They are never logged and never sent to the webview except on Show.
- **Errors.** The command's own refusal is shown as it is.

## 4. Sync: keeping the repo's copies current

- **What is recorded.** On every export, the app records for each case a fingerprint of the script, the time, and the files it wrote. This is a small store per clone and project, written atomically.
- **The Sync view** lists every exported case with its state:
  - **Current.** The script is unchanged since export.
  - **Out of date.** The script changed after export: a repair, an added step, a new area.
  - **Not in the repo.** The repo no longer has its section or raw spec.
  - **Never exported.**
- **Update** re-exports only the out-of-date cases (or the ticked ones) through the existing all-or-nothing export. A test-case section is spliced, so the other sections stay byte-for-byte, and a raw spec is replaced.
- **Never touched:**
  - the repo's `## Amendment` and "Added by engineer" blocks;
  - data files;
  - `users.json`;
  - `navigation.json`;
  - refactored specs under `specs/`.
- **What still needs the repo.** An updated raw spec still needs the repo's refactorer (`/gen-test` stages 5 and 6) to become a finished spec. The Sync view says so for each updated case.

## 5. Errors and limits

- No HTTP DELETE to Azure DevOps. No secrets, hosts or query strings in logs.
- **Processes.** A process the app starts in the repo runs in its own Windows job (`browser/tree.rs`) and is ended with it on Stop or app exit.
- **One at a time.** Only one runner or account command runs at a time. While one runs, the others are greyed out with the reason.
- **Advanced Features off.** No tab, and no commands.

## 6. Testing

- **Rust:**
  - reading the structure from a fixture repo;
  - building the tree from a saved `--list` JSON;
  - the run-list and dry-count refusal;
  - argument building for each runner action;
  - the `users` command arguments and parsing their JSON;
  - the sync store and its states;
  - re-export of only the out-of-date cases;
  - the job-object stop.
- **Frontend:**
  - the tab is gated by Advanced Features;
  - the tree's ticks;
  - the run panel and Stop;
  - the accounts list and edit flows;
  - the Sync view's states and Update.
- **Live, opt-in:** one small run in a real clone of the repo.
- **How To Use** gains a Playwright section, which is hidden with Advanced Features off.
