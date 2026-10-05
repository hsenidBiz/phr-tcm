# Auto Run fixtures, script setup and cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** fixtures make drafts reproducibly, scripts get their drafts from fixtures behind the person's approval, and drafts the tests made can be cleaned up on demand.

**Architecture:**
- **Fixtures** are a new API Templates item. They run through the existing template runner in one signed-in browser, under the account lease.
- **The record of test-made drafts** is written only by the fixture runner, and its status is changed only by Cleanup.
- **Approvals** live in the app's store as fingerprints, outside any script file.
- **The webview** gets:
  - a Fixtures tab;
  - a Setup section in the script editor;
  - a prefix field on the environment card;
  - a Cleanup dialog.

**Tech Stack:** Rust (serde, tokio, sha2, tauri-specta, the AI bridge and MCP), React 19 + TypeScript, vitest.

**Spec:** `docs/superpowers/specs/2026-10-06-autorun-fixtures-design.md`

## Global Constraints

**Rust**
- Rust tests go only in `src-tauri/tests/suite/`, one module per file, with a `mod` line in `suite/main.rs`.
- A test that touches process-wide state takes its lock from `suite/serial.rs`.
- Source-reading tests normalise `\r\n` to `\n`.

**Bindings and old files**
- Never hand-edit `src/bindings.ts`. Regenerate it with `cd src-tauri && cargo test --test bindings`, and restore line-ending-only drift.
- Every new field on an existing struct is `serde(default, skip_serializing_if)`, so old scripts, templates, environments and run files load unchanged.

**Copy and UI**
- The spec's sentences are used verbatim.
- Plain sentences, with no em or en dashes in UI text or comments.
- Colours come from tokens and icons from `src/lib/actionIcons.ts`.
- `ui-consistency` and `a11y` stay as they are.

**Safety rules**
- No HTTP DELETE to Azure DevOps outside `ado/deletion.rs` (unchanged).
- No passwords, cookies, hosts or query strings in records, events, logs or sentences.
- User-facing errors are fixed sentences. Raw errors go to `applog` or `logUi`.

**Working rules**
- One test command at a time.
- Commit with a Bash heredoc ending `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`, and stage by name.

## Review Focus

1. **A fixture run that fails after creating something.** The made thing is still recorded `present`, so Cleanup can find it. Rebuild keeps the previous outputs.
2. **Changing a template body used by an approved setup.** The approval stops counting at once, and the case is Blocked with `setup not approved - approve it in the script editor` before anything signs in.
3. **A delete template reached by any path other than Cleanup** is refused:
   - `run_api_template`;
   - a fixture step;
   - a prove on an id not in the record.
4. **Cleanup filters:**
   - the prefix is compared ignoring case;
   - "older than" uses whole days from `created_at`;
   - entries with status `deleted` never show;
   - `delete failed` entries do show again.
5. **The test-made record and approvals cannot be written** by any MCP tool or bridge route. A test lists every route and tool and proves none of them reaches those files.

---

### Task 1: Fixtures, the test prefix and delete templates (model and rules)

**Files:**
- `src-tauri/src/api_templates/fixture.rs` (new): the `Fixture` types and `validate`.
- `src-tauri/src/api_templates/fixture_store.rs` (new):
  - `fixtures/<slug>/<id>.json` and `<id>.runs.json`, holding 20 runs as `store.rs` does;
  - `load`, `save`, `list`, `append_run`, `current_outputs`.
- `src-tauri/src/api_templates/mod.rs`: `ApiTemplate.deletes_kind: Option<String>`, the kind a delete template deletes. Saving a template whose `effect` is `Delete` without it is refused: `a delete template must say which kind it deletes`.
- `src-tauri/src/environments.rs`: `Environment.test_prefix: String`, defaulting to `AUTOTEST` through a serde default fn. `validate` enforces 3 to 20 characters of `[A-Za-z0-9-]` with the sentence `the test name prefix is 3 to 20 letters, digits or -`.
- Tests: `tests/suite/api_fixtures.rs` (new), and the existing environments and templates tests.

**Interfaces:**
- **Produces:**
  - `Fixture { id, name, account, steps: Vec<FixtureStep { template: String, params: BTreeMap<String, String> }>, outputs: BTreeMap<String, String>, creates: Vec<Creates { kind, id, name }> }`;
  - `FixtureRun { at, ok, failed_step: Option<usize>, detail, outputs: BTreeMap<String, Value> }`;
  - `fixture::validate(f: &Fixture, templates: &dyn Fn(&str) -> Option<ApiTemplate>) -> Result<(), Vec<String>>`;
  - `fixture_store::{load, save, list, append_run, current_outputs(root, org, project, id) -> Option<BTreeMap<String, Value>>}`;
  - `ApiTemplate.deletes_kind`;
  - `Environment.test_prefix`.

- [ ] **Step 1:** Write failing tests:
  - Every save refusal in spec §1 "Rules at save", verbatim. `{{steps.<m>...}}` with `m >= n` refuses, and so does a capture the template does not declare.
  - A valid fixture saves and loads.
  - The run history keeps 20, newest first. `current_outputs` is the newest `ok` run's outputs and skips failed ones.
  - An old environment file loads with `test_prefix == "AUTOTEST"`. The prefix limits refuse `AB`, a 21-character prefix and `AUTO TEST`.
  - A Delete template without `deletes_kind` is refused.
- [ ] **Step 2:** Implement, regenerate the bindings, and run the focused tests, `cargo test --tests`, `npx tsc --noEmit` and `npm test`, one at a time.
- [ ] **Step 3:** Commit `feat(v2): fixtures can be saved from proven templates, environments carry a test name prefix, and delete templates say what they delete`.

### Task 2: Running a fixture, the test-made record and the assistant's tools

**Files:**
- `src-tauri/src/api_templates/runner.rs`:
  - Split `drive` so that the part after sign-in (go to the antiforgery page, run the steps) can run again for another template in the same browser. Use a `pub(crate)` fn.
  - Refuse a delete template in `preflight` for `Mode::Run` with `template <id> deletes, and only Clean up test-made drafts runs a delete template`.
- `src-tauri/src/api_templates/fixture_run.rs` (new):
  - `run_fixture<B: Browsers>(browsers, root, org, project, fixture, timing) -> FixtureReport`.
  - The lease and the browser are held once for the whole run.
  - Placeholders are resolved per step: `{{steps.n.x}}`, `{{now:fmt}}` and `{{prefix}}` (the active environment's).
  - It stops at the first failed step with `step <n>: <the runner's sentence>`.
  - It appends a `FixtureRun`.
  - It records the `creates` entries in the test-made record, resolved from whatever was captured, even on failure. Skip an entry whose id placeholder did not resolve.
  - It adds the prefix warning sentence from spec §1 to the report.
- `src-tauri/src/autorun/test_made.rs` (new):
  - `TestMade { environment, kind, id, name, created_at, fixture, run_id, case_id: Option<i32>, status }`, where `status` is `present`, `deleted` or `delete failed: <reason>`.
  - It lives in `test-made.json` in the Auto Run store.
  - `pub(crate) fn record(root, entries)`.
  - `pub fn list(root) -> Vec<TestMade>`.
  - `pub(crate) fn set_status(root, environment, kind, id, status)`.
  - There is no public writer.
- `src-tauri/src/commands/` (the API templates commands module): `api_fixtures_list`, `api_fixture_run` (Run and Rebuild are the same command) and `api_fixture_remove` (the person's, like `RemoveTemplate`).
- The AI bridge plus MCP:
  - `save_api_fixture` (it validates, then saves);
  - `run_api_fixture`;
  - `list_api_fixtures`.

  They sit in the API template tool set, behind `api_writes` for save and run. Mirror them in `ai_tools.rs` and `src/lib/mcpTools.ts`. The sync test covers them.
- The API templates guide (`api_templates/guide.rs`): the spec §1 guide sentences.
- Tests: `tests/suite/api_fixtures.rs`, plus the bridge, MCP and guide tests.

**Interfaces:**
- **Consumes:** Task 1's types and store.
- **Produces:**
  - `FixtureReport { ok, outputs, made: Vec<TestMade>, steps: Vec<RunReport>, failed: Option<String>, warnings: Vec<String> }`;
  - `run_fixture`;
  - `test_made::{list, record, set_status}`.

- [ ] **Step 1:** Write failing tests with the fake browsers:
  - Two steps pass `cycle_id` from step 1 to step 2.
  - `{{now:yyyyMMdd}}` and `{{prefix}}` are substituted.
  - One browser is opened and one sign-in happens for the whole fixture.
  - Step 2 failing gives `step 2: ...`, step 1's made cycle is recorded `present`, and the previous current outputs are kept (Review Focus 1).
  - A made name without the prefix is recorded, and the warning is given.
  - `run_api_template` on a delete template is refused with the sentence.
  - Review Focus 5: every bridge route and MCP tool name is listed, and none of them writes `test-made.json` or `approvals/`. Read the router source, normalising `\r\n`.
- [ ] **Step 2:** Implement, regenerate the bindings, and run the focused tests, `cargo test --tests`, `npx tsc --noEmit` and `npm test`, one at a time.
- [ ] **Step 3:** Commit `feat(v2): a fixture runs its templates in one signed-in browser, and what it makes is recorded as test-made`.

### Task 3: Scripts that use fixtures, and approvals

**Files:**
- `src-tauri/src/autorun/mod.rs`: `CaseScript.setup: Option<Setup { fixture: String }>`.
- `src-tauri/src/autorun/setup.rs` (new):
  - `check_saved(script, fixtures) -> Result<(), Vec<String>>`, with the spec §2 refusals verbatim. Call it from `nav::check_project_rules`, beside the preconditions.
  - `resolve(script, fixture_outputs, setup_outputs) -> Result<CaseScript, String>`. It returns a copy with `{{fixture.<id>.<output>}}` and `{{setup.<output>}}` replaced in every step value. It gives the "has not been built" Blocked sentence verbatim.
  - `prepare_case(...)`: the approval check, then `run_fixture`, then `resolve`. It returns the resolved script or a Blocked sentence: `setup not approved - approve it in the script editor`, or `setup failed: <sentence>`.
  - Made entries carry `case_id`.
- `src-tauri/src/autorun/approvals.rs` (new):
  - `fingerprint(setup, fixture, templates) -> String`: SHA-256 hex over a canonical JSON of the setup, the fixture's `steps`, `outputs`, `creates` and `account`, and each step template's body without `proven`.
  - `approve(root, case_id, fp)`, `withdraw(root, case_id)` and `state(root, case_id, fp) -> Approval { Approved{at} | Changed | None }`.
  - It lives at `approvals/<case id>.json`.
  - Commands, which only the webview calls: `auto_run_setup_view(case_id) -> SetupView`, `auto_run_approve_setup(case_id)` and `auto_run_withdraw_setup(case_id)`.
- **Hooks**, after the preconditions and before sign-in:
  - the unattended path (`replay.rs`, where preconditions are checked);
  - the supervised case start (where it checks preconditions);
  - part 4's `replay_to.rs`.

  The setup's browser and lease are released before the case's sign-in.
- Tests: `tests/suite/autorun_setup.rs` (new).

**Interfaces:**
- **Consumes:** `run_fixture` and `fixture_store::current_outputs` (Task 2).
- **Produces:**
  - `SetupView { fixture_name, account, steps: Vec<String>, creates: Vec<String>, approval: "approved" | "changed" | "none", approved_at: Option<String> }`;
  - the three commands above.

- [ ] **Step 1:** Write failing tests:
  - Save refusals for an unknown fixture, an unknown output and an unknown setup output.
  - `{{fixture...}}` is replaced. A fixture that was never built Blocks with the sentence.
  - A setup runs before sign-in, and its outputs are used as `{{setup.x}}` in a step.
  - A failed setup Blocks with `setup failed: step 1: ...`.
  - Not approved: Blocked, with nothing opened and nothing signed in.
  - Review Focus 2: approve, then change each of the setup, a fixture step and a template body. Each one gives `Changed` and Blocked.
  - Withdraw.
  - The replay to step N also runs the setup.
  - An old script round-trips byte-identical.
- [ ] **Step 2:** Implement, regenerate the bindings, and run the focused tests, `cargo test --tests`, `npx tsc --noEmit` and `npm test`, one at a time.
- [ ] **Step 3:** Commit `feat(v2): an Auto Run script can get its draft from a fixture, once the person approves its setup`.

### Task 4: The Fixtures tab, the prefix field and the editor's Setup section

**Files:**
- `src/screens/ApiTemplates/index.tsx`: a `Fixtures` tab beside Templates and Flows.
- `src/screens/ApiTemplates/FixturesTab.tsx` (new):
  - each fixture with its steps, current outputs and last run (time and outcome);
  - `Run` when it has never been built, else `Rebuild`;
  - Remove, mirroring `RemoveTemplate`;
  - a placeholder slot for the Cleanup button (Task 5).
- The environment card's editor (grep for where `start_url` is edited): a `Test name prefix` field with the Rust limits sentence on error.
- `src/screens/AutoRun/ScriptEditor.tsx`: the spec §2 "Setup" section, verbatim:
  - `Approve setup`;
  - `Approved <date>` with `Withdraw approval`;
  - `Changed since you approved it`.
- Tests: vitest for the tab, the field and the editor section.

**Interfaces:**
- **Consumes:** Task 2's commands and Task 3's `SetupView` and commands.

- [ ] **Step 1:** Write failing vitest tests:
  - The tab lists fixtures. Run calls `apiFixtureRun`, and the button reads Rebuild after a build.
  - A failed Rebuild shows the failure and keeps the outputs shown.
  - The prefix field saves and shows the error.
  - The Setup section's three states, with Approve and Withdraw calling their commands.
- [ ] **Step 2:** Implement. Run `npx tsc --noEmit`, the focused files, ui-consistency and a11y, then `npm test`, one at a time.
- [ ] **Step 3:** Commit `feat(v2): fixtures have their own tab, environments a test name prefix, and the script editor shows a setup to approve`.

### Task 5: Cleanup of test-made drafts

**Files:**
- `src-tauri/src/autorun/cleanup.rs` (new):
  - `preview(root, environment, prefix, older_than_days, now) -> Vec<CleanupLine { entry, deletable: bool, note: Option<String> }>`.
    - It takes record entries with status `present` or starting `delete failed`, in that environment, whose name starts with the prefix ignoring case, and whose age is at least `older_than_days * 24h`.
    - `deletable` is false, with the note `no proven delete template for <kind>`, when no proven template has `effect == Delete` and `deletes_kind == kind`.
  - `run_cleanup<B: Browsers>(browsers, root, org, project, entries, timing, cancel)`. It deletes one at a time with that template, in one signed-in browser under the lease, using `{{id}}` = the entry id. It sets each status, emits `autorun-cleanup-progress { done, total, id, outcome }`, and stops between deletes when `cancel` is set.
- The `prove_api_template` path: for a delete template, refuse with `a delete template is only proven on a draft the tests made` unless `{{id}}` is a `present` entry of that kind in the active environment. A successful proof sets the entry `deleted`.
- Commands, which only the webview calls (no MCP tool, no bridge route):
  - `auto_run_cleanup_preview`;
  - `auto_run_cleanup_run`;
  - `auto_run_cleanup_stop`.
- `src/screens/ApiTemplates/CleanupDialog.tsx` (new):
  - the environment, prefix and "older than" (default 7, minimum 1) controls;
  - the preview, with every deletable line ticked and the others not tickable;
  - the confirm sentence `Delete <n> drafts from <environment>? This cannot be undone.` with Delete and Cancel;
  - live results and a Stop button.

  It is opened from the Fixtures tab's `Clean up test-made drafts` button.
- Tests:
  - `tests/suite/autorun_cleanup.rs` (new);
  - `CleanupDialog.test.tsx`;
  - a Review Focus 5 extension asserting the cleanup commands are not bridge routes or MCP tools.

**Interfaces:**
- **Consumes:** `test_made` (Task 2) and the runner split (Task 2).

- [ ] **Step 1:** Write failing tests:
  - Review Focus 4: prefix case, the age boundary (exactly N days is included, and N days minus 1 minute is not), environment, `deleted` hidden and `delete failed` shown.
  - A kind with no delete template is not deletable and shows the note.
  - The run deletes in order and sets statuses. A failed delete is recorded with its reason.
  - Stop between deletes leaves the rest `present`.
  - A delete template proved off the record is refused, and a proof on the record marks the entry deleted.
  - A fixture step or a `run_api_template` call on a delete template is refused (Review Focus 3).
  - vitest:
    - filters re-preview;
    - untickable lines;
    - the exact confirm sentence;
    - Cancel does nothing;
    - results stream in;
    - Stop calls the command.
- [ ] **Step 2:** Implement, regenerate the bindings, and run the focused tests, `cargo test --tests`, `npx tsc --noEmit` and `npm test`, one at a time.
- [ ] **Step 3:** Commit `feat(v2): drafts the tests made can be previewed and cleaned up, through a proven delete template only`.
