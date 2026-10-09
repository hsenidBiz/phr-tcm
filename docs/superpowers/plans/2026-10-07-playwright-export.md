# Playwright Export Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Export a PBI's passing Auto Run scripts into a local PHR-PLAYWRIGHT-AUTOMATION clone at the raw `/gen-test` stage (raw spec, `index.json` entry, test-case section), ready for the repo's own refactorer and linter.

**Architecture:** A new Rust module `src-tauri/src/pw_export/` holds pure parts (selector and action translation to TypeScript, test-case markdown render and splice, `index.json` update) and one impure writer that stages every file and renames them all or none. Two Tauri commands — a preview (what can be exported, and why not) and a write — sit in `commands/pw_export.rs`. The UI is one dialog opened from Auto Run's More menu, which also holds the clone folder and the area/account mappings.

**Tech Stack:** Rust (serde_json, existing `AdoClient`), React 19 + TanStack Query, tauri-specta bindings, vitest + mockIPC, Rust integration tests in `src-tauri/tests/suite/`.

**Spec:** `docs/superpowers/specs/2026-10-07-playwright-export-design.md`

## Global Constraints

- TCM never runs git, npm, Playwright or Claude Code in the clone, never commits, never pushes.
- TCM never writes `src/users/users.json`, `src/navigation.json`, `suites/_generated/seed.spec.ts` or `auth.setup.ts`; it reads only the KEYS of `users.json`, never a value.
- Raw spec header is exactly `// spec: suites/<seg>/test-cases/<stem>.md` then `// seed: suites/_generated/seed.spec.ts` then a blank line; imports exactly `import { test, expect } from '@playwright/test';`; one `test.describe('<Feature Title>')` holding one `test('<case title>', async ({ page }) => { ... })`.
- `index.json`: flat object `"<id>": "<file>"`, 2-space indent, existing order kept, new keys appended, trailing newline.
- Test-case section heading is `## <id> — <title>` (em dash, U+2014); no other line the export writes may match `^##\s+\d+\s`.
- `<seg>` = `sl/<side>/<module>/<feature>`; `<stem>` = `<feature>`; side ∈ {admin, self}; module and feature kebab-case `[a-z0-9]+(-[a-z0-9]+)*`.
- Never emit `test.fixme` or `test.skip`. Every string literal from a script is escaped (see Task 3).
- A case is exportable only if its newest run's `verdict == "Passed"`, every action translates, its area is mapped and its account maps to an existing repo key.
- Correction to the spec, decided here: the clone folder and the mappings live in the Export dialog, not under Settings; the clone path is stored in `AppSettings` (per machine).
- Commits: Bash heredoc `git commit -F - <<'EOF' … EOF`, ending `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. A selector or value containing `'`, `` ` ``, `\`, `${` or a newline — the raw spec must still parse and say exactly the script's text (Task 3 tests).
2. Re-exporting a case already in the clone — same raw file name reused, its section replaced, nothing duplicated (Task 7 test).
3. A test-case file that already holds other cases (written by `/gen-test`) — every other section left byte-for-byte, including one with an `**Assertion floor:**` line (Task 5 test).
4. A clone folder that is not the repo, or whose `index.json` is malformed — refused with a sentence, nothing written (Task 2 tests).
5. A write that fails partway (a file locked by an editor) — no file changed (Task 7 test).

---

### Task 1: Settings and mappings storage

**Files:**
- Modify: `src-tauri/src/app_settings.rs` (add field + Default), `src-tauri/src/commands/app_settings.rs`
- Create: `src-tauri/src/pw_export/mod.rs`, `src-tauri/src/pw_export/mapping.rs`
- Modify: `src-tauri/src/lib.rs` (`pub mod pw_export;`)
- Test: `src-tauri/tests/suite/pw_export_mapping.rs` (+ `mod` line in `suite/main.rs`)

**Interfaces:**
- Produces:
  - `AppSettings.playwright_clone: String` (serde default `""`); command `set_playwright_clone(path: String) -> Result<AppSettings, String>`.
  - `pw_export::mapping::Placement { side: String, module: String, feature: String }` (Serialize, Deserialize, specta::Type, Clone, PartialEq).
  - `pw_export::mapping::ExportMap { areas: BTreeMap<String, Placement>, accounts: BTreeMap<String, BTreeMap<String, String>> }` — `accounts[env_id][tcm_account_key] = repo_user_key`. Serde default on both.
  - `mapping::path(root: &Path, org: &str, project: &str) -> PathBuf` = `<root>/projects/<recipe::project_slug(org,project)>.pwexport.json`.
  - `mapping::load(root, org, project) -> Result<ExportMap, String>` (missing → default, BOM stripped); `mapping::save(root, org, project, &ExportMap) -> Result<(), String>` via `ai_tools::atomic_write`.
  - `Placement::validate(&self) -> Result<(), String>`; `Placement::seg(&self) -> String` (`"sl/<side>/<module>/<feature>"`); `Placement::suggest(area_name: &str) -> Placement` = side `"admin"`, module `"performance"`, feature = kebab-case of the area name (a first guess the person confirms).

- [ ] **Step 1: Write failing tests** in `pw_export_mapping.rs`:
  - `a_missing_map_is_empty_and_a_saved_one_reads_back` — load on empty temp root is `ExportMap::default()`; save with `areas{"Definition Wizard": Placement{admin, performance, definition-wizard}}` and `accounts{"env-1": {"automation": "AutomationSL.performance-management.general"}}`; load equals it.
  - `placement_rules` — `validate()` ok for `admin/performance/proficiency-levels`; errors naming the field for side `"Admin"`, module `"Perf Mgmt"`, feature `"-x"`, empty feature; `seg()` = `"sl/admin/performance/proficiency-levels"`.
  - `suggest_kebabs_the_area_name` — `suggest("Definition Wizard").feature == "definition-wizard"`, side `"admin"`.
  - `the_clone_path_is_a_setting` — `AppSettings::default().playwright_clone == ""`; an old settings JSON without the field loads with `""`.
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite pw_export_mapping::` — expect compile failure (module missing).
- [ ] **Step 3: Implement** the types and functions above; register `set_playwright_clone` in `lib.rs` `collect_commands`.
- [ ] **Step 4: Run** the same command — PASS.
- [ ] **Step 5: Commit** `feat(v2): Playwright export - clone setting and per-project area/account mappings`.

### Task 2: Reading the clone

**Files:**
- Create: `src-tauri/src/pw_export/clone.rs`
- Test: `src-tauri/tests/suite/pw_export_clone.rs`

**Interfaces:**
- Produces:
  - `clone::Clone { root: PathBuf, user_keys: Vec<String>, navigation_keys: Vec<String>, index: Vec<(String, String)>, generated_files: Vec<String> }` (index in file order).
  - `clone::open(path: &Path) -> Result<Clone, String>` — requires `playwright.config.ts`, `suites/_generated/index.json`, `src/navigation.json`, `src/users/users.json`; otherwise `Err(NOT_A_CLONE)` with `pub const NOT_A_CLONE: &str = "that folder is not a PHR-PLAYWRIGHT-AUTOMATION clone - it needs playwright.config.ts, suites/_generated/index.json, src/navigation.json and src/users/users.json"`. Malformed JSON in any of the three → `Err` naming the file. `user_keys` = top-level keys of users.json only (values never kept). `navigation_keys` = keys of `entries`. `generated_files` = `*.spec.ts` file names in `suites/_generated/`.
  - `clone::index_with(index: &[(String, String)], id: i32, file: &str) -> String` — the new `index.json` text: existing pairs in order, `id` replaced in place if present else appended; 2-space indent; trailing `\n`.

- [ ] **Step 1: Failing tests** (build a fake clone in a temp dir per test):
  - `a_real_looking_clone_opens` — keys, nav keys, index order, generated files as written.
  - `user_values_are_never_read` — users.json values are non-strings (e.g. `{"K": 42}`); `open` still succeeds with `user_keys == ["K"]` (proves values are not parsed as accounts).
  - `a_folder_that_is_not_the_repo_is_refused` — missing `playwright.config.ts` → `Err(NOT_A_CLONE)`.
  - `a_malformed_index_is_refused_by_name` — index `[]` → error containing `suites/_generated/index.json`.
  - `index_appends_and_keeps_order` / `index_replaces_in_place` — exact expected strings, e.g. `index_with(&[("1","a.spec.ts")], 2, "b.spec.ts") == "{\n  \"1\": \"a.spec.ts\",\n  \"2\": \"b.spec.ts\"\n}\n"`.
- [ ] **Step 2: Run** `cargo test --test suite pw_export_clone::` — FAIL.
- [ ] **Step 3: Implement.** `serde_json` is built with `preserve_order`, so `serde_json::Map` keeps the file's key order; a non-string value in `index.json` is a malformed index.
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(v2): Playwright export - read a clone's user keys, navigation and generated index`.

### Task 3: Selectors to TypeScript

**Files:**
- Create: `src-tauri/src/pw_export/ts.rs` (string literal + selector translation)
- Test: `src-tauri/tests/suite/pw_export_ts.rs`

**Interfaces:**
- Produces:
  - `ts::lit(s: &str) -> String` — single-quoted TS literal escaping `\`, `'`, newline (`\n`), CR, tab, U+2028/U+2029.
  - `ts::locator(page: &str, t: &Target) -> String` — expression on the page variable `page` (e.g. `"cur"`).
    - `Legacy(css)` → `page.locator(<lit>)`; `Legacy("text=…")` → `page.locator(<lit>).last()` (TCM's legacy text form is the last match).
    - Step `{role, name?, exact}` → `.getByRole(<lit role>, { name: <lit>, exact: true })` (omit `exact` when false, omit options when no name).
    - `{text}` → `.getByText(<lit>)` / `{ exact: true }`; `{css}` → `.locator(<lit>)`.
    - `visible` absent or true → append `.filter({ visible: true })`; `Some(false)` → nothing.
    - `nth: Some(n)` → `.nth(n)`.
    - Chain: left to right on the previous expression; a step that is an iframe (`css` starting with `iframe` or `frame`, or `role` equal ignoring case to `iframe`) is followed by `.contentFrame()` before the next step.

- [ ] **Step 1: Failing tests** with exact expected strings, e.g.:
  - `lit("it's \\ a `x` ${y}\nz") == "'it\\'s \\\\ a `x` ${y}\\nz'"` (single quotes make `` ` `` and `${` inert).
  - `locator("cur", role button "Save" exact) == "cur.getByRole('button', { name: 'Save', exact: true }).filter({ visible: true })"`.
  - `locator("cur", {css:"#a", visible:false, nth:2}) == "cur.locator('#a').nth(2)"`.
  - iframe chain `[{css:"iframe[title='Employee Search']"},{role:"button",name:"Search"}]` == `"cur.locator('iframe[title=\\'Employee Search\\']').filter({ visible: true }).contentFrame().getByRole('button', { name: 'Search' }).filter({ visible: true })"`.
  - legacy `"text=Objectives"` == `"cur.locator('text=Objectives').last()"`.
- [ ] **Step 2: Run** `cargo test --test suite pw_export_ts::` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(v2): Playwright export - selectors and string literals as TypeScript`.

### Task 4: Actions and whole scripts to a raw spec

**Files:**
- Create: `src-tauri/src/pw_export/translate.rs`
- Test: `src-tauri/tests/suite/pw_export_translate.rs`, fixture `src-tauri/tests/fixtures/pw_export/raw-135560.spec.ts`

**Interfaces:**
- Consumes: `ts::lit`, `ts::locator` (Task 3); `CaseScript`, `StepScript`, `Action`, `RecipeStep` (existing).
- Produces:
  - `translate::Untranslatable(pub String)` — the reason a case cannot be exported.
  - `translate::check(script: &CaseScript) -> Result<(), Untranslatable>` — every action translatable.
  - `translate::raw_spec(input: &RawSpecInput) -> Result<String, Untranslatable>` where `RawSpecInput<'a> { script: &'a CaseScript, md_path: String /* suites/<seg>/test-cases/<stem>.md */, feature_title: String, after_sign_in: &'a [RecipeStep], area_clicks: &'a [Target], area_clicks_by_name: &'a BTreeMap<String, Vec<Target>> /* for return_to_area{area} */, step_texts: &'a BTreeMap<i32, String> /* the case's own step action text, by step number */ }`.
- Rules (one decision each; the body is the implementer's):
  - Opening, before step 1: `await page.goto('/');`, `await expect(page).toHaveURL(/\/hr\/home\/index/);`, then `after_sign_in` translated (a recipe `when_visible` like a script one), then each area click as a click. A `let cur = page;` precedes everything; all actions act on `cur`.
  - Per step: `// <n>. <step_texts[n]>` (newlines in the text become spaces; a missing text gives `// <n>.`), then `// Not checked: <reason>` when the step has `unchecked`, then its actions.
  - `click` → `await <loc>.click();` `fill` → `.fill(<lit>)`; `wait_for` → `.waitFor({ timeout: N })`; `drag` with `position` None or `Onto` → `.dragTo(<loc to>)`; `Before`/`After` → `Untranslatable("drag before/after has no Playwright equivalent")`.
  - expects → `toBeVisible/toBeHidden/toHaveText/toContainText/toHaveCount/toHaveAttribute/toBeFocused`, `{ timeout: N }` when `timeout_ms` set.
  - `check_text` → `await expect(cur.locator('body')).toContainText(<lit>, { ignoreCase: true });` `check_url` → `await expect(cur).toHaveURL(new RegExp(<escaped>))` — escape regex metacharacters of `contains`.
  - `when_visible` → `if (await <loc>.waitFor({ timeout: W }).then(() => true, () => false)) { <then> }` with `W` = `within_ms` or 2000.
  - `expect_response`: for each one in a step, emit before the step's first action `const resp<step>_<i> = cur.waitForResponse(r => <url test> && <method test>);` with the url test `r.url().toLowerCase().includes(<lit lowercased fragment>)`; at its own place: `const r = await resp…; expect(r.status()).toBe(S);` and with `json` `expect(await r.json()).toMatchObject(<json as TS object literal>);`. A `json` holding an array of objects → `Untranslatable("expect_response json with a list of objects is compared differently by Playwright")`.
  - `api_request` → `const a = await cur.request.get(<lit path>, { params: {…} }); expect(a.status()).toBe(S);` plus `toMatchObject` as above (same array rule).
  - `reload` → `await cur.reload();` `return_to_area{area}` → `await page.goto('/')`, URL check, `after_sign_in`, then that area's clicks (missing area → `Untranslatable`).
  - `expire_session` → `await cur.context().clearCookies();` `press_key{key, times}` → `await cur.keyboard.press(<key>)` repeated `times` (key names: `Shift+Tab` stays, `Space` → `' '` is NOT used — Playwright accepts `'Space'`).
  - Tabs: `expect_tab{name}` → `const tab_<name> = await cur.context().waitForEvent('page', { timeout });` + optional `toHaveURL` contains; `open_tab` → `tab_<name> = await cur.context().newPage(); await tab_<name>.goto(<lit url>); cur = tab_<name>;`; `switch_tab` → `cur = tab_<name>;` (`main` → `page`); `close_tab` → `await tab_<name>.close(); cur = page;` when it was current; `expect_tab_closed` → `await tab_<name>.waitForEvent('close', { timeout });`. Tab names sanitized to `[A-Za-z0-9_]`.
  - Not exportable, each with its own sentence: `upload`, `sign_in`, `expect_dialog`, `expect_download` with any of `sheet/headers/cells/contains_text/pdf`, `expect_row`, `expect_no_row`, `expect_sorted`, `expect_row_count`. `expect_download` with `name` only → `const dl = await cur.waitForEvent('download', { timeout }); expect(dl.suggestedFilename()).toMatch(<glob as anchored RegExp>);` armed at the step start like `expect_response`.
- [ ] **Step 1: Failing tests:**
  - `a_sibling_script_becomes_the_golden_raw_spec` — the script of case 135560 (copy it into the test as JSON) with its case's step texts, a one-click area and the PeoplesHR `after_sign_in` toggle produces exactly `fixtures/pw_export/raw-135560.spec.ts` (write the fixture by hand from the rules above first; it is the contract).
  - One test per rule group: response armed before the click and awaited after; `when_visible`; tab switching changes `cur`; `return_to_area` replays the named area; `press_key` times 3 → three presses.
  - `each_unexportable_kind_says_why` — table of the not-exportable kinds → `Untranslatable` text contains the kind name.
- [ ] **Step 2: Run** `cargo test --test suite pw_export_translate::` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(v2): Playwright export - Auto Run scripts as raw /gen-test specs`.

### Task 5: Test-case markdown

**Files:**
- Create: `src-tauri/src/pw_export/test_case.rs`
- Test: `src-tauri/tests/suite/pw_export_test_case.rs`

**Interfaces:**
- Produces:
  - `test_case::CaseDoc { id: i32, title: String, state: String, area_path: String, iteration_path: String, project: String, module: String, tags: String, preconditions: String, steps: Vec<(String, String)>, side: String, navigation_captured: bool, feature_title: String }`.
  - `test_case::section(doc: &CaseDoc) -> String` — `## <id> — <title>`, blank line, `### Metadata` table (ID, Title, State, Area Path, Iteration, Project, Module, Tags), `**Side:** <side> — mapped in TCM.`, `**Navigation:** captured in src/navigation.json.` or `**Navigation:** not yet captured - capture it in this repo before refactoring.`, `### Preconditions` (one numbered row per non-empty precondition line), `### Test Data & Validation` (header row only), `### Navigation Path` (text block with the area name), `### Test Steps` (`| Step | Action | Expected Result |`, `|` and newlines in cells escaped as `\|` and `<br>`), `### Cleanup / Postconditions` (header row only). Ends with one blank line.
  - `test_case::new_file(feature_title: &str, stem: &str, user_key: &str) -> String` — `# Test Case Set: <Feature Title>` + the repo's intro paragraph naming `specs/<stem>.spec.ts` + `**User:** <user_key>  <!-- from-tcm -->`.
  - `test_case::splice(existing: &str, id: i32, section: &str) -> String` — replace the section starting at the line matching `^##\s+<id>\s` up to (not including) the next line matching `^##\s` (level 2 only) or EOF; append at the end (preceded by one blank line) when absent; never touch any other byte.
- [ ] **Step 1: Failing tests:** `a_section_in_the_repos_layout` (exact string for a two-step case with a `|` in a step); `splice_replaces_only_its_own_section` (file with sections 1, 2, 3 where 2 holds `**Assertion floor:** 72 (raw sum 80)` and `### Test Steps`; splicing 1 and 3 leaves 2 byte-identical); `splice_appends_a_new_case`; `a_new_file_header`; `nothing_written_matches_a_stray_id_heading` (no line other than the heading matches `^##\s+\d+\s`).
- [ ] **Step 2: Run** `cargo test --test suite pw_export_test_case::` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(v2): Playwright export - test-case sections in the repo's markdown layout`.

### Task 6: Case metadata from Azure DevOps

**Files:**
- Modify: `src-tauri/src/ado/endpoints.rs` (new method beside `get_test_cases_by_ids`)
- Test: `src-tauri/tests/suite/ado_case_meta.rs` (wiremock, like the other ADO tests)

**Interfaces:**
- Produces: `ado::CaseMeta { id: i32, state: String, area_path: String, iteration_path: String }` and `AdoClient::get_case_meta(&self, organization: &str, ids: &[i32]) -> Result<Vec<CaseMeta>, AdoError>` — one batch `workitemsbatch` POST per 200 ids with fields `System.Id, System.State, System.AreaPath, System.IterationPath`. GET/POST only (the no-DELETE invariant).
- [ ] **Step 1: Failing test** `case_meta_reads_state_area_and_iteration` — wiremock answers a batch; result fields match; request body lists exactly those four fields.
- [ ] **Step 2–4:** run (FAIL), implement, run (PASS).
- [ ] **Step 5: Commit** `feat(v2): read a test case's state, area and iteration`.

### Task 7: Preview and write commands

**Files:**
- Create: `src-tauri/src/pw_export/export.rs`, `src-tauri/src/commands/pw_export.rs`
- Modify: `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs` (register commands)
- Test: `src-tauri/tests/suite/pw_export_write.rs`

**Interfaces:**
- Consumes: Tasks 1–6; `store::load_script`, `store::list_runs`, `nav::load_nav`, `recipe::load_effective_recipe_if_any`, `environments::active_id`.
- Produces (all `specta::Type`, no `u64`):
  - `PreviewCase { case_id: i32, title: String, exportable: bool, reason: Option<String>, seg: Option<String>, user_key: Option<String>, add_user_command: Option<String> }`.
  - `Preview { clone_ok: bool, clone_problem: Option<String>, user_keys: Vec<String>, areas: Vec<String>, accounts: Vec<String>, map: ExportMap, cases: Vec<PreviewCase> }`.
  - `ExportResult { files: Vec<String> /* relative to the clone */, cases: Vec<(i32, String)> /* id → raw spec file */, missing_navigation: Vec<String> /* <module>/<feature> */ }`.
  - `export::preview_with(root, org, project, case_ids: &[i32], clone_path: &str) -> Preview` — reasons, in this order: no script; newest run with the case not `Passed` (`"latest run: <verdict or not reviewed>"`); area not mapped; account not mapped / key not in clone (`add_user_command` = `npm run users -- add <key> --username <username> --password <password> --apply`, the literal word `<password>`); `translate::check` failure.
  - `export::write_with(root, org, project, case_ids, clone_path, docs: &BTreeMap<i32, CaseDoc>) -> Result<ExportResult, String>` — refuses (nothing written) unless every id is exportable; builds every file's new content first; writes each to `<target>.tcm-export-tmp`; renames all; on any error removes the temps and returns the error. Raw file name: the index's existing file for the id, else `kebab(title)` cut to 60 chars, made unique against `generated_files` with `-2`, `-3`…
  - Commands: `pw_export_preview(app, organization, project, case_ids) -> Result<Preview, String>`; `pw_export_save_map(app, organization, project, map: ExportMap) -> Result<(), String>` (validates every Placement); `pw_export_write(app, organization, project, case_ids, module_ref: Option<String>, preconditions_ref: Option<String>) -> Result<ExportResult, String>` — fetches `get_test_cases_by_ids` + `get_case_meta`, builds `CaseDoc`s, calls `write_with`. Transport errors go through `network_error` (no URLs to the user).
- [ ] **Step 1: Failing tests** (temp TCM root + temp fake clone; scripts, runs, nav, recipe and map written by the test):
  - `preview_lists_each_reason` — one case per reason, plus one exportable; asserts each `reason` and the `add_user_command` text (contains the username, contains `<password>`, contains no real password).
  - `write_creates_raw_spec_index_and_section`; `reexport_reuses_the_file_and_replaces_the_section`; `other_sections_and_index_entries_are_untouched`; `missing_navigation_is_reported_not_written` (`src/navigation.json` byte-identical after); `users_json_is_never_written` (byte-identical after).
  - `a_failed_write_changes_nothing` — make the test-case target a directory so its rename fails; assert raw spec absent, index byte-identical, no `.tcm-export-tmp` left.
- [ ] **Step 2: Run** `cargo test --test suite pw_export_write::` — FAIL.
- [ ] **Step 3: Implement**, then regenerate bindings: `cd src-tauri && cargo test --test bindings`.
- [ ] **Step 4: Run** the module, then `cargo test --test bindings` — PASS.
- [ ] **Step 5: Commit** `feat(v2): Playwright export - preview and an all-or-nothing write into the clone`.

### Task 8: Export dialog

**Files:**
- Create: `src/screens/AutoRun/PlaywrightExportDialog.tsx`, `src/screens/AutoRun/PlaywrightExportDialog.test.tsx`
- Modify: `src/screens/AutoRun/index.tsx` (More menu entry "Export to Playwright" + `exportOpen` state, rendered beside `ExecutionOrderDialog`)

**Interfaces:**
- Consumes: `commands.pwExportPreview`, `pwExportSaveMap`, `pwExportWrite`, `setPlaywrightClone`, `getAppSettings`; `open({ directory: true })` from `@tauri-apps/plugin-dialog`; `useFieldRefs(org, project).prefs`.
- Produces: `PlaywrightExportDialog({ org, project, pbiId, caseIds, onClose })`.
- Layout, top to bottom: **Clone folder** (path + Choose…, saved through `setPlaywrightClone`, the clone problem shown under it); **Areas** (one row per area: side select admin/self, module, feature inputs; Save mappings); **Accounts** (one row per TCM account: a select of the clone's user keys); **Cases** (a checkbox per exportable case, ticked by default; the others listed greyed with their reason, and any `add_user_command` in a copyable code line); **Export** button → on success a summary: files written, missing navigation entries, and the next steps from the spec (capture navigation; run the raw spec in the `generated` project with the seed set to the user key; ask Claude Code to run `test-refactorer`; `npm run lint:tests -- --require-specs`; commit).
- [ ] **Step 1: Failing tests** (mockIPC): `lists_reasons_and_only_ticks_exportable`; `choosing_a_clone_saves_it`; `saving_a_mapping_sends_the_map`; `export_sends_the_ticked_ids_and_shows_the_summary`; `a_refused_export_shows_the_reason_and_writes_nothing_else`.
- [ ] **Step 2: Run** `npx vitest run src/screens/AutoRun/PlaywrightExportDialog.test.tsx` — FAIL.
- [ ] **Step 3: Implement**; colours from tokens only (the consistency gate).
- [ ] **Step 4: Run** the file, `src/ui-consistency.test.ts`, and `npx tsc --noEmit` — PASS.
- [ ] **Step 5: Commit** `feat(v2): Export to Playwright dialog on the Auto Run screen`.

### Task 9: Whole-branch verification and the hand check

- [ ] **Step 1:** `cd src-tauri && cargo test --tests --no-fail-fast` (capture to a file; all green), then `npm test` (capture; file count = all test files), `npx tsc --noEmit`.
- [ ] **Step 2: Hand check with the person:** export 135520's sibling set into a scratch copy of the clone; in that copy run `npx playwright test suites/_generated/<file> --project=generated` with the seed set to the user key; confirm it passes; confirm `node scripts/lint-tests.mjs suites src` reports no new `parse-error` for the exported ids. Record what was seen in the PR/commit message.
- [ ] **Step 3: Commit** any fixes from the hand check, each as its own commit.
