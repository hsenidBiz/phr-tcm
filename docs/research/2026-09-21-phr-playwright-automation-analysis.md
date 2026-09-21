# PHR-PLAYWRIGHT-AUTOMATION: what it does, and what it would take to do the same over Chrome DevTools

Date: 2026-09-21
Subject repo: `C:\Users\Admin\Desktop\Main Menu\Software\Personal Projects\PHR-PLAYWRIGHT-AUTOMATION` (HEAD `634ab5d`)
Target app: this repository (Test Case Manager), specifically the Auto Run tab and its CDP layer in `src-tauri/src/browser/`
Scope: core logic only. The subject repo's console UI (`cli/render/`, `cli/input/`, the painting half of the `*-mode.mjs` files) was deliberately not analysed.
Method: five parallel read-only passes (runtime, suites and lint, AI generation pipeline, runner orchestration, this app's Auto Run), then spot checks of the claims the recommendations rest on. No code was run in either repo. No credentials, emails or employee numbers were copied out of `users.json`.

---

## 1. Summary

PHR-PLAYWRIGHT-AUTOMATION is a Playwright + TypeScript end-to-end framework for the PeoplesHR web application, wrapped in two things that are more interesting than the tests themselves:

1. **An AI authoring pipeline** (`/gen-test`). An Azure DevOps Test Case id goes in. An assistant fetches the case, a human helps it capture the menu path once, a generator agent drives a real browser through the steps and writes a raw spec, a healer agent repairs it within strict bounds, and a refactorer turns it into a clean spec + page object + data file. Every stage has a mechanical gate so the assistant cannot make a test pass by weakening it.
2. **A runner** (`qa`) that discovers tests, lets a tester select some, runs them, keeps every report, and manages test accounts and test data.

The repo currently holds very few real tests (two Performance features and a smoke suite, nine raw generated specs). Almost all of its weight is the machinery around them: 124 `.mjs` files of tooling and tests against 36 `.ts` files of framework and suites. Read it as a **method for getting reliable browser tests out of an AI**, not as a test suite.

Nothing in it writes back to Azure DevOps. ADO is read-only input; results live in local HTML and JSON reports.

For this app the transferable value, in order:

1. The **guard rails** that stop an assistant faking a pass (assertion floor, blast-radius diff, bounded healing, locators verified against the live page, a ban on sleeps and in-page JavaScript in tests).
2. The **test-account model**: a keyed registry, lazily minted and cached login sessions, and a second actor in the same test.
3. The **application quirks catalogue**: 17 browser-level behaviours of PeoplesHR that any driver has to survive, Playwright or not.
4. The **data model**: per-case data with provenance and a `{{unique}}` token.

What does not transfer is the execution substrate. Auto Run today sends exactly one CDP method (`Runtime.evaluate`) and has six action kinds. The specs in the subject repo lean on Playwright's auto-waiting, role and accessible-name locators, retrying assertions, storage-state sessions and multiple browser contexts. Section 8 lists each of those with the CDP calls that replace it.

---

## 2. Architecture at a glance

| Layer | Where | What it is |
| --- | --- | --- |
| Written test cases | `suites/**/test-cases/*.md` | One `## <ADO id> - <title>` section per Azure DevOps Test Case, with metadata, preconditions, data, navigation path, a Step / Action / Expected Result table and cleanup. The stored section is also the baseline `--sync` diffs against. |
| Test data | `suites/**/data/<feature>.json` | One file per feature, keyed by spec stem then case name. `_shared` merges into every case. `_provenance` records where each value came from. `{{unique}}` is replaced per load. |
| Page objects | `suites/**/pages/*.page.ts` | Locators and actions only. No `expect(` is allowed here. |
| Specs | `suites/**/specs/*.spec.ts` | One literal `test()` per ADO id, tagged `// ADO <id>`. All assertions live here so they can be counted. |
| Raw generator output | `suites/_generated/` | Untouched specs straight from the generator, one per ADO id, indexed in `index.json`. Exempt from lint. They are the assertion-count baseline. |
| Framework runtime | `src/` | Fixtures, the user registry and auth cache, navigation, shared components (app shell, dialog, toast), data loading, uniqueness. |
| Config | `playwright.config.ts` | Projects, timeouts, reporters, trace policy. |
| AI pipeline | `.claude/` + `CLAUDE.md` | The `/gen-test` command, six agents, three skills, two hooks. |
| Quality gates | `scripts/lint-tests.mjs`, `cli/spec/*` | AST-based lint with no warning level, assertion floor, blast-radius diff. |
| Runner | `cli/` | Discovery, selection, argv building, spawn, report history, users, data, Excel round trip. |

Suite path convention: `suites/<country>/<side>/<module>/<feature>/{test-cases,data,pages,specs}`, where country is `sl`, `in` or `ph` and side is `admin` or `self`. Shared components sit under `suites/common-components/` with no country or side.

---

## 3. How one test case becomes an executed test

`/gen-test <ADO id...>` runs seven steps (`.claude/commands/gen-test.md`, governed by `CLAUDE.md`).

1. **Fetch.** The main session reads the Test Case through an Azure DevOps MCP server, asking explicitly for `Microsoft.VSTS.TCM.Steps`, and writes the markdown section. `--describe` replaces the fetch with an interview, and the expected outcome is captured before the assistant sees the application, so an existing bug cannot be written down as correct behaviour. Ticketless cases get local ids from 900000001 up.
2. **Navigation and account.** Main session only. If `src/navigation.json` has no entry for `<module>/<feature>`, a browser opens on a cached session, a human clicks to the screen, and the assistant reconstructs the menu chain from network requests and an accessibility snapshot, then reads it back in plain text before saving it. The reason given is blunt: the sidebar is role-less `<div>`s that an agent loops against blind. The same handoff fixes which test account the spec runs as.
3. **Generate.** A vendor `playwright-test-generator` agent with browser tools but no Edit and no Bash drives the live app and writes one raw spec per ticket into `suites/_generated/`.
4. **Verify raw and heal.** A failure caused by a missing precondition is never healed: the run stops and reports it. Otherwise the vendor `playwright-test-healer` runs. Its stock instructions say "repeat until it passes" with no cap, so every dispatch carries an injected bounds block: three cycles per dispatch, a short-circuit when the same failure signature repeats, hard stops on a login screen, a missing precondition or a product defect, two confirmation runs before calling something a flake, a list of forbidden fixes (no dropped or weakened `expect`, no fixed sleeps, no `networkidle`, no in-test `evaluate`, no unauthorised retry wrapper), `test.fixme()` plus a report if unresolved, and a mandatory "quirk candidate" section. Ceiling: two dispatches or five runs.
5. **Refactor.** A project-owned `test-refactorer` drops the raw navigation, extracts a page object and a data file with provenance, and may combine several tickets into one spec (`--spec`) or one serial chain (`--chain`).
6. **Verify refactored.** A real run plus `lint:tests --require-specs`. Both must pass.
7. **Run summary.** Lists every value that was invented or assumed rather than taken from the ticket, the heal record, and any disclosures.

`--sync` re-fetches a ticket, classifies the drift as wording, data and preconditions, behaviour, or wholesale, and only opens a browser for the last two, regenerating only the one `// ADO <id>` test. `verifySplice` then proves nothing else in the file moved. The classification is a human judgement, and the docs admit a wrong call silently re-enshrines stale behaviour.

A separate `spec-editor` agent applies human-approved edits and is deliberately given no Bash and no browser. The stated reason: an agent that can run tests iterates toward green, and the cheapest way to green is deleting the step QA just asked for.

---

## 4. Runtime core (`src/`, `playwright.config.ts`)

**Config.** `fullyParallel: false`, four workers locally and two in CI, so parallelism is per spec file. Timeouts: test 90 s, expect 10 s, action 15 s, navigation 30 s. Retries 0 locally, 1 in CI. Reporters: list, HTML into `reports/<runId>/`, JSON as a sibling file (the HTML reporter empties its own folder at start). Trace kept on failure, screenshot on failure, no video. Projects `SL`, `PH`, `IN` select suite folders by country; PH and IN inherit the SL suites minus anything tagged `@sl-only`. There is deliberately no global `storageState`.

**Accounts.** A key is `<environment>.<module>.<role>[.<discriminator>]`. `users.json` is a flat map from key to username, password and optional identity fields. `TEST_ENV` remaps the environment segment of every key. An unresolved key throws; there is no silent fallback. A spec declares its account once with `test.use({ userKey })`, and admin and self specs never share one.

**Sessions.** `ensureAuthFile` looks for `.auth/<env>/<module>.<role>.json`. If it is fresh by mtime (default 8 hours) it is reused; otherwise a throwaway context logs in and the storage state is written with temp-then-rename. Login is lazy, so only the first test needing an account pays for it. The `page` fixture re-logs in and refreshes the file if a cached session has died mid-run.

**Login sequence.** Fill username and password by role, click the login button, wait up to 30 s for `#sidebar-toggle-menu`, dismiss the optional "another active session" modal, then read and if necessary switch off the per-account "Group Menu Items" preference so captured menu chains still match.

**Second actor.** `actingAs(key)` opens a second independent browser context with its own storage state and its own manually started trace, and refuses a key from a different environment. There is no runtime lock on accounts: two concurrent specs using the same account kill each other's session, and the framework treats that as a registry assignment problem rather than something a wait could fix.

**Fixtures.** `userKey` to `user` to `storageState` to `page` to `shell`, `dialog`, `toast`, plus `actingAs` and an auto fixture `autoNavigate` that walks the recorded menu chain for the spec's feature before the test body runs.

**Navigation.** `navigation.json` maps `<module>/<feature>` to `{side, menu[], moduleNav?[], verifiedOn, capturedBy}` under a strict schema. Entries must come from a live capture with a human. Only three files in the whole repo may call `page.goto`, all for cold start. Everything else is click-driven, because the product bounces direct URLs to home.

**Shared components.** `AppShell.switchTo(side)` is idempotent. `openMenu` expands the sidebar and clicks each label filtered to the first visible match, with an explicit visibility wait per step. `goHome` is a reload, because the product always lands on home. `revisit` is reload plus re-navigate and is the only way to check that something persisted. `Dialog` and `Toast` always filter to visible nodes, because the app keeps hidden duplicates of both in the DOM at all times.

**Data and uniqueness.** `loadCase(__filename, caseKey)` merges base `_shared`, base case, country delta `_shared`, country delta case, then replaces every `{{unique}}` with one token per call (`Date.now()` and a counter, base 36), so several fields in one case share a token. It has to be called inside the test body; a chained test that reloaded its data would get a new token and act on a record that does not exist.

---

## 5. What a test actually does to the browser

Measured across the page objects and specs in scope.

| Primitive | Count |
| --- | --- |
| `click()` | 133 |
| `fill()` | 69 |
| read `innerText()` then `parseInt` (counter before and after) | about 9 |
| `waitForEvent('filechooser')` then `setFiles()` | 2 |
| check, press, hover, native `selectOption`, drag and drop, downloads | 0 |

| Assertion | Count |
| --- | --- |
| `toBeVisible` | 232 |
| `toBeHidden` | 60 |
| `toContainText` | 33 |
| `toHaveCount` | 32 |
| `toHaveText` | 27 |
| `toHaveAttribute` | rare |
| `expect(async () => {...}).toPass()` retry wrapper | used around known flaky saves |

| Locator strategy | Count |
| --- | --- |
| `getByRole(role, { name })` | 154 |
| `getByText` | 124 |
| `.locator(css)` | 114 |
| `.filter({ visible, hasText })` | 53 |
| XPath ancestor climbs | about 13 |
| `.nth()` | 12 |
| `getByLabel`, `getByPlaceholder`, `getByTestId`, `frameLocator` | 0 |

So the verb set is small: click, fill, read text or attribute, count matches, wait for visible or hidden, handle a file chooser, and retry a block until it passes. The locator set is what is demanding: most locators are **role plus accessible name, scoped inside another locator, filtered to visible**. Dropdowns in this app are button-and-link popovers, not native selects. There are no iframes in scope.

---

## 6. The guard rails

**Lint (`scripts/lint-tests.mjs`).** Parses with the TypeScript compiler, never regex over code (three hand-rolled lexers desynced silently before that was adopted). Every rule is an error. Rules: spec outside a `specs/` folder, raw selector in a spec, spec with no written test case, no declared account, no navigation entry, missing common-component import, assertion count below floor, six data-file rules (orphan data, parse error, format, shape, provenance mismatch, orphan case), no URL navigation, no bare reload, no `waitForTimeout`, no `networkidle`, no `evaluate`, duplicate page object, and a fail-closed parse-error backstop.

**Assertion floor.** For each ADO id the refactored test must contain at least as many `expect(` calls as its raw generated counterpart. A recorded floor moves arithmetically when the raw spec grows. It catches dropped assertions, not weakened ones; the docs say so.

**Blast radius (`cli/spec/diff.mjs`).** Before and after models of a spec are diffed, and an edit is accepted only if every changed, added, removed or unchained test was declared by exact title beforehand. A `userKey` change can never be authorised. An assertion-drop allowance is forfeited if anything else was added in the same edit. `--sync` adds identity checks: marker moved, describe or position or serial changed, or a chained test silently stopped reading a shared variable.

**Hooks.** A pre-edit hook blocks writes to `users.json`, `.env*` and the vendor agent files. A post-edit hook runs the lint for `.ts` under `suites/` and `src/`, or the sibling unit test for tooling files.

**Assistant rules in `CLAUDE.md`.** Never guess a selector or URL; verify against the real DOM first. Role-based locators before CSS or XPath. Locators only in page objects. The retry wrapper is allowed under four documented conditions only. Navigation, data and spec edits happen only through human-gated skills in the main session.

---

## 7. Runner orchestration (the non-UI half of `cli/`)

**Generic, carries over to any driver:**

- Selection as a derived tri-state set over a tree of country, side, module, feature, spec, test.
- Run options where `null` means "say nothing" and a tolerant loader ignores unknown keys (`qa-options.json`).
- Keep-every-run report history indexed by a local timestamp, with a navigator over past runs.
- The whole test-data model (`cli/data/`): refusals computed at parse time for shapes that cannot be shown as a grid, nine pure mutations, and an Excel round trip whose read-back passes nine ordered gates (stamp, schema, source hash, columns, case rows, type, unique token, from-ticket attention, human confirm) where any failure means zero changes.
- The user registry operations (`cli/users/`): validation of key grammar, roles, credentials, supervisor cycles and discriminator collisions; rename with cascade across `test.use`, `actingAs` literals and a test case's `**User:**` line; removal refused while anything references the account; cached session deleted on any credential change; CSV import from Katalon.
- Local id allocation that reads both the index and every markdown heading, because a heading is written before the index.

**Playwright-specific, does not carry over:**

- Discovery through `playwright test --list --reporter=json`, with a fingerprint cache because a bare list takes about 4.4 s.
- The `--test-list` file, and `verify-count`, which exists because Playwright's "no tests found" guard is bypassed for test-list runs, so every real run is preceded by a dry list whose count must match.
- The argv vocabulary, `show-report`, and the `test()`, `describe`, `expect` syntax assumptions in `cli/spec/model.mjs`.

One inconsistency worth knowing: the run-options design doc specifies an in-console options screen and session-only options. The code has a JSON file instead, because that screen corrupted on the user's real terminal. The design doc was not updated.

---

## 8. The 17 application quirks

These are properties of PeoplesHR, not of Playwright. A CDP driver meets every one of them. Source: `docs/app-quirks.md`, each entry with symptom, cause, fix and class.

| # | Quirk | What the framework does |
| --- | --- | --- |
| 1 | A sidebar label also matches an invisible duplicate | Filter to visible, take the first |
| 2 | `switchTo` hangs immediately after login | Wait for the shell marker before switching |
| 3 | A sidebar module entry resolves to two elements | Visible filter |
| 4 | An in-page module link also matches a "Recent Activities" shortcut | Exact role and name match, deliberately no `.first()` |
| 5 | `#phr-sidebar` cannot be hovered until expanded | Expand first |
| 6 | Hidden dialog and toast nodes are always pre-rendered | Every dialog and toast locator filters to visible |
| 7 | Two nested `<main>` elements | Scope explicitly |
| 8 | A rating method save intermittently returns 400 although it succeeded (a real double-submit product bug) | Retry-until-pass around the save |
| 9 | A second login for one account kills the first session | One account per spec file; treated as an assignment problem |
| 10 | A row's icon toggle sits three ancestors above its content | Ancestor climb |
| 11 | A rating method needs at least two items to save | Business rule, encoded in data |
| 12 | A rating method's Type locks once it has items | Business rule, ordering in the chain |
| 13 | The shared sandbox is too slow or unreliable to trust a save | Deliberately not retried; root cause undiagnosed |
| 14 | Login blocked by the "another active session" modal, which appears 187 to 288 ms after the shell looks ready | Optional dismiss with a 5 s window |
| 15 | The per-account "Group Menu Items" preference changes the module nav | Read first, switch off at login |
| 16 | A sidebar flyout click races the accordion expand animation | Explicit visibility wait per menu step |
| 17 | Retrying a wizard "Save & Continue" can double-submit the step already reached | A smarter retry that first checks whether the destination state is already reached |

Quirks 8 and 17 together are the important lesson: a blind retry wrapper is not enough. The retry has to be **state-aware**.

---

## 9. Where Auto Run stands today

Verified in this repo (`src-tauri/src/browser/`, `src-tauri/src/autorun/`, `src-tauri/src/commands/autorun.rs`).

- **Launch.** Edge or Chrome from a short fixed path list. A fresh throwaway profile in `%TEMP%` per open. An OS-assigned debugging port. Never headless (a test asserts it). One global session; opening another kills the first.
- **CDP client.** 91 lines over `tokio-tungstenite`. It takes the first `page` target from `/json/list` once. The **only** method sent anywhere in the codebase is `Runtime.evaluate` (confirmed by grep: one hit, `browser/cdp.rs:76`). Events are skipped. There is no timeout at this layer, so an `eval` can wait forever. Errors are plain strings.
- **Actions.** Six kinds: `Navigate` (sets `location.href`), `Click`, `Fill` (property setter plus `input` and `change` events, not real keystrokes), `WaitFor` (200 ms poll), `CheckText` (substring of `body.innerText`), `CheckUrl`. An element is one string: CSS, or `text=` substring over a fixed tag list where the last match wins. Before Click and Fill there is a 350 ms highlight pause for the human. The only gate is existence: no visibility, enabled or stability check.
- **Run model.** `CaseScript` per case and `LocalRun` with per-step outcomes, stored as JSON in the app data folder with atomic writes. The verdict is always set by a person. Nothing is written to Azure DevOps. No screenshots, video or trace are kept.
- **AI path.** The bridge routes `GET /autorun-guide` and `POST /autorun-script` work, but their MCP tools sit in `HIDDEN_TOOLS` and are refused even on a direct call. The slash command is gone. The guide text already says the right things (locators from the page, assertions only from the case) but nothing enforces them at run time.
- **Gaps that are also findings.** Passwords are plain strings inside the script JSON. `Navigate` has no origin allowlist.

What Auto Run already has that the subject repo does not: a saved, replayable script per Azure DevOps case, and, elsewhere in this app, the ability to create test runs and write per-step outcomes to Azure DevOps.

---

## 10. Capability gap and the CDP calls that close it

| Playwright capability the specs depend on | Auto Run today | CDP replacement |
| --- | --- | --- |
| Auto-waiting with actionability (attached, visible, stable, enabled, receives events) | Absent | Resolve the node, `DOM.scrollIntoViewIfNeeded`, `DOM.getContentQuads` twice a frame apart for stability, hit-test the centre with `DOM.getNodeForLocation`, read `disabled` and `aria-disabled`. Loop until all pass or the action timeout. |
| Real input | Absent (synthetic events) | `Input.dispatchMouseEvent` (move, press, release), `Input.insertText` or `Input.dispatchKeyEvent`. Frameworks that ignore synthetic events then behave. |
| Role and accessible-name locators | Partial (`text=` substring) | `Accessibility.queryAXTree` with `role` and `accessibleName` returns backend node ids computed by Chrome itself. Verify on this app first: whether it returns hidden or ignored nodes decides where the visible filter goes. |
| Scoped locators, `filter({visible, hasText})`, `nth` | Absent | A locator becomes a small chain evaluated fresh on every retry: root, then role or text or CSS step, then filters, then index. Never cache a node id across retries. |
| Retrying assertions | Partial (`WaitFor` only) | One polling loop shared by visible, hidden, text, contains-text, count and attribute, with the expect timeout. |
| Retry-until-pass block | Absent | A block step with a mandatory "already reached?" probe (quirks 8 and 17), not a bare loop. |
| Session reuse (`storageState`) | Absent (fresh profile each time) | `Storage.getCookies` and `Storage.setCookies` per browser context, plus `localStorage` per origin captured and restored through a page script before first navigation. |
| Second actor, isolated session | Absent (one global session) | `Target.createBrowserContext` then `Target.createTarget` inside it. Needs flattened sessions: `Target.attachToTarget` with `flatten: true`, and a `sessionId` on every message. |
| File chooser | Absent | `Page.setInterceptFileChooserDialog`, the `Page.fileChooserOpened` event, then `DOM.setFileInputFiles`. |
| JavaScript dialogs | Absent; an `alert` would hang the run | `Page.javascriptDialogOpening` and `Page.handleJavaScriptDialog`. |
| New tabs and popups | Absent | `Target.setDiscoverTargets` or `Target.setAutoAttach`. |
| Navigation and load waits | `location.href` only | `Page.navigate`, `Page.reload`, `Page.lifecycleEvent`. |
| Evidence | Text only | `Page.captureScreenshot` per failed step, and optionally every step. `Page.startScreencast` if video is wanted. A Playwright-style trace is not worth rebuilding; screenshot, URL, the locator and the accessibility snippet around it cover most debugging. |
| Downloads | Absent | `Browser.setDownloadBehavior` with its progress events. Not used by any test in scope, so it can wait. |
| iframes | Disclaimed in the guide | Same-process frames through execution contexts; out-of-process frames need auto-attach. Not used by any test in scope. |
| Parallel workers | Absent | One browser context per worker, with an account lease so two workers can never hold the same account (quirk 9). |
| Reporter | Local JSON | Already have the better target: Azure DevOps runs with per-step outcomes. |

Two prerequisites sit under all of it, both in `browser/cdp.rs`: an **event pump** (the client currently discards events, and file choosers, dialogs, lifecycle and new targets are all events) and **timeouts with a typed error** instead of strings.

---

## 11. Recommended shape for this app

**Keep the script as data, not code.** The subject repo needs a spec, a page object, a data file and a markdown file per feature because its artefact is TypeScript. Auto Run's artefact is already a JSON script bound to an Azure DevOps case. Extend that rather than imitate page objects:

- A step carries the Azure DevOps step id it implements. Expected results in the case become the assertion floor for free: a script may not have fewer checks than the case has expected results, and a step with an expected result may not have zero checks. This is stronger than the subject repo's floor, which can only compare against a first draft.
- Locators become structured (`role` and `name`, `text`, `css`, `within`, `visible`, `nth`) instead of one string.
- Named locators can be declared once per script or per feature and referenced by steps. That is the useful part of a page object without the code.
- Data moves out of the steps: a case-level value map with `{{unique}}` and provenance, so the same script can run with another data row.
- An account key replaces inline credentials. Rust owns the secret: store test-account passwords in the operating system credential store and resolve them at run time, never in the script JSON. This fixes an existing weakness regardless of anything else here.
- Navigation chains are stored per project and feature, captured once with a human, and reused by every script for that feature.

**Carry over the rules, enforce them in Rust.** No fixed sleeps in a script. No in-page JavaScript step. No direct URL step for an app that forbids it, as a per-project setting. A healer that is bounded by count, stops on a login screen or a missing precondition, may never delete or loosen a check, and must report a quirk candidate. An edit to a script is accepted only if the steps it touched were declared beforehand. `transform.rs` and the validator already work this way for test cases, so the pattern exists in this codebase.

**Record the quirks per project.** A short, structured list the driver and the assistant both read: always filter dialogs and toasts to visible, one live session per account, dismiss the active-session modal, state-aware save retry. It is the single most reusable thing in the subject repo and it is application knowledge, not framework code.

**Do not carry over** the TypeScript spec and page-object model, Playwright's list and test-list and verify-count machinery, the HTML reporter, the console UI, and for now the Excel round trip.

**Suggested order.** Each phase is useful on its own.

1. CDP foundations: event pump, timeouts, typed errors, flattened sessions, `Page` and `Input` and `DOM` domains, screenshot on failure. Replace synthetic click and fill with real input plus actionability checks. Existing scripts keep working and get more reliable.
2. Locators and assertions: structured locators with role and name through the accessibility tree, scoping, visible filter, `nth`; the shared retrying assertion loop with visible, hidden, text, count and attribute.
3. Accounts and sessions: registry, credential store, lazy login with cached cookies, the login sequence including quirks 14 and 15 as project settings, one account lease at a time.
4. Unattended replay: run a whole script, then a selection of cases, without a click per step; per-step evidence; results offered to Azure DevOps as a run with step outcomes, behind the same opt-in as today.
5. Authoring with an assistant: reopen the hidden tools with an accessibility-snapshot tool, the bounded healer, the expected-result floor and the declared-edit gate.
6. Second actor, file chooser, dialogs, popups, then parallel runs with account leases.

---

## 12. Risks and open questions

- **The honest cost.** Auto-waiting and role locators are the two things Playwright spent years on. Building them on the accessibility tree is feasible and keeps the installer free of a Node runtime and browser binaries, which is the reason recorded in `Cargo.toml`. It is still the bulk of the work, and phases 1 and 2 are where it sits.
- **`Accessibility.queryAXTree` behaviour on this application is unverified.** The PeoplesHR sidebar is role-less `<div>`s, so role locators will not reach it; the subject repo falls back to filtered text there, and this app would too.
- **Account contention is a product rule, not a test problem.** Anything parallel needs a lease on accounts from day one.
- **The sandbox is unreliable** (quirk 13, undiagnosed). Unattended runs will report failures that are not defects. Evidence per step is what makes those cheap to dismiss.
- **The subject repo's own admitted gaps** apply to any copy of its method: the floor misses weakened assertions, there is no teardown so a failed chain leaks records, `--sync` classification is a judgement call, and one malformed data file breaks every spec in its feature.
- **This machine.** Auto Run is never headless today. Unattended runs on a shared desktop will steal focus unless headless is allowed or the window is kept off-screen; and each new unsigned build is already blocked once by the antivirus here, which does not change.
- **Not determined.** Whether checkboxes, key presses, hover, native selects, drag and drop or downloads appear in PeoplesHR flows outside the two features in scope. The verb counts in section 5 describe this repo as it is, not the application as a whole.
