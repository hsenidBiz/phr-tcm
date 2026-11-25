# Auto Test on a test suite - Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Auto Test can be opened on a test suite (no PBI): list its cases, script, run, review, send results to the suite's test points, and file bugs. The assistant knows the current suite.

**Architecture:** A `Scope` (PBI or suite) replaces the bare PBI wherever Auto Test is keyed. Rust gets a suite-cases command, a `suite` field on `LocalRun`, scope-keyed order files, a suite branch in publish, and a `current_suite` in the bridge context. The webview carries `Scope` from App state (top bar picker and Search Suites' More menu) into Auto Test.

**Tech Stack:** Rust (Tauri 2, tauri-specta), React 19 + TypeScript, vitest.

**Spec:** `docs/superpowers/specs/2026-11-23-auto-test-on-a-suite-design.md`

## Global Constraints

- Only a suite's OWN cases (non-recursive), as Run Tests' folder mode.
- Results go to the suite's own test points, every configuration's point, as the PBI path does.
- Old `LocalRun` records (no `suite`) must read as PBI runs unchanged.
- A PBI and a suite are never both current: choosing one clears the other.
- The assistant learns the suite through `get_current_pbi`'s answer. No new MCP tool.
- Azure DevOps: GET / POST / PATCH only (CLAUDE.md); no new DELETE.
- User-facing errors name no URL (`ado/transport.rs` sentences).
- `src/bindings.ts` is regenerated, never hand-edited.
- Rust tests are integration tests in `src-tauri/tests/suite/`; process-wide state takes `suite/serial.rs` locks in the order written there.
- No hardcoded colours; `tests/ui-consistency.test.ts` passes unchanged.
- Changelog lines are end-user facing. Never name hidden features.

## Review Focus

1. A suite run published when one case was removed from the suite since the run: that case is skipped with "not in this suite any more", the rest are sent.
2. A finished background run whose scope differs from the screen's: the review opens on the run's own scope (PBI or suite), never on the wrong one.
3. Switching from a suite to a PBI (or back) while a run setup is open cancels the setup, as a PBI change does today.
4. An old run record without `suite` in Past runs: it is labelled "for PBI #id" and Send works as before.
5. `get_current_pbi` with neither a PBI nor a suite answers exactly as today.

---

### Task 1: Rust - suite cases, the run's scope, scope-keyed order

**Files:**
- Modify: `src-tauri/src/commands/cases.rs` (new command)
- Modify: `src-tauri/src/autorun/mod.rs` (`LocalRun`)
- Modify: `src-tauri/src/autorun/store.rs` (order files ~1949-2105)
- Modify: `src-tauri/src/commands/autorun.rs` (`auto_run_save_order`, `auto_run_clear_order`, `plan_at` ~3486-3529)
- Modify: `src-tauri/src/commands/autorun_replay.rs` (`autoRunReplay` ~290, `planned_cases` ~432)
- Test: `src-tauri/tests/suite/autorun_suite_scope.rs` (+ mod line)

**Interfaces:**
- Produces:
  - `#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, PartialEq)] pub struct SuiteRef { pub plan_id: i32, pub suite_id: i32, pub title: String }` in `autorun/mod.rs`.
  - `pub enum RunScope { Pbi(i32), Suite(SuiteRef) }` with serde `{"kind":"pbi","pbi_id":N}` / `{"kind":"suite",...}` and specta. It has `fn key(&self) -> String`, returning `pbi:<id>` / `suite:<plan>:<suite>`.
  - `LocalRun { ..., #[serde(default)] pub suite: Option<SuiteRef> }`, where `pbi_id` is 0 for a suite run. Add `fn scope(&self) -> RunScope`.
  - The command `suite_test_cases_full(org: String, project: String, plan_id: i32, suite_id: i32, module_ref: Option<String>, preconditions_ref: Option<String>) -> Result<Vec<TestCaseFull>, AdoError>`. It reads the suite's own points (`get_test_points_in(.., recursive=false)`), de-duplicates case ids in suite order, then calls `get_test_cases_by_ids` and `load_step_attachments`, as `pbi_test_cases_full` does.
  - Order files are keyed by `RunScope`: `orders/<pbi_id>.json` for a PBI (unchanged path, so existing orders keep working) and `orders/suite-<plan>-<suite>.json` for a suite. The commands `auto_run_save_order` / `auto_run_clear_order` / the plan read take `scope: RunScope` instead of `pbi_id`.
  - `autoRunReplay` takes `scope: RunScope` instead of `pbi_id`, and writes `pbi_id` / `suite` on the `LocalRun` accordingly.

- [ ] **Step 1: Write the failing tests:**
  - `suite_cases_are_own_points_deduplicated_in_order`, against a wiremock suite with 2 configurations per case and a child suite whose case must not appear;
  - `old_run_record_reads_as_pbi_run`, a JSON with no `suite` key giving `suite == None` and `scope() == Pbi(id)`;
  - `suite_run_record_round_trips`;
  - `order_files_are_keyed_by_scope`, where a PBI and a suite with the same number do not collide and the PBI path is unchanged;
  - `replay_records_the_suite_scope`, using the fake browsers from `autorun_replay.rs`.
- [ ] **Step 2:** Run `cd src-tauri && cargo test --test suite autorun_suite_scope::`. Expect FAIL.
- [ ] **Step 3:** Implement. Regenerate bindings with `cargo test --test bindings`.
- [ ] **Step 4:** Run `autorun_suite_scope::`, then `autorun_replay::`, then `run_order` / any order module. Expect PASS. Update call sites of the changed commands in the webview only as far as needed to compile (`npx tsc --noEmit`), passing `{ kind: "pbi", pbi_id }`. Task 4 does the real UI.
- [ ] **Step 5:** Commit `feat(v2): runs, order and cases can belong to a test suite`.

### Task 2: Rust - publish a suite run to its own test points; bugs without a PBI

**Files:**
- Modify: `src-tauri/src/commands/autorun_publish.rs` (~6-79, `publish_run` ~251-437)
- Modify: `src-tauri/src/commands/bugs.rs` (only if needed; `create_work_item` already skips id 0)
- Test: extend `src-tauri/tests/suite/autorun_suite_scope.rs`

**Interfaces:**
- Consumes: `LocalRun.suite`, `RunScope` (Task 1).
- Produces: `autoRunPublish(org, project, run_id, name, cases)`.
  - The `pbi_id` argument is dropped: the scope comes from the stored run. If you keep the signature for the PBI path, ignore the passed id when the run has a suite. Keep it the smaller change and say which.
  - For a suite run, `EnsuredSuite { plan_id, suite_id }` comes from `run.suite`: no area lookup and no requirement-suite search.
  - A case without a point in that suite is skipped with the exact reason `not in this suite any more`. The PBI path keeps `not in this PBI's test suite`.

- [ ] **Step 1: Write the failing tests:**
  - `suite_run_publishes_to_its_suite_points`: the wiremock asserts the test run's `pointIds` come from that plan/suite, and that no work-item read for a PBI happens;
  - `suite_run_skips_a_case_no_longer_in_the_suite`;
  - `pbi_run_publish_unchanged`, which may reuse an existing test if one covers it;
  - `bug_with_pbi_zero_links_only_the_case` (likely already true: assert the relations sent).
- [ ] **Step 2:** Run `cargo test --test suite autorun_suite_scope::`. Expect FAIL.
- [ ] **Step 3:** Implement. Regenerate bindings if the signature changed.
- [ ] **Step 4:** Run `autorun_suite_scope::` and the existing publish/bug modules (`grep -l autoRunPublish\|publish_run src-tauri/tests/suite`). Expect PASS.
- [ ] **Step 5:** Commit `feat(v2): a suite run's results go to the suite's own test points`.

### Task 3: Rust - the assistant knows the current suite

**Files:**
- Modify: `src-tauri/src/ai_bridge/mod.rs` (`CurrentPbi` ~105, `BridgeContext` ~175, `current_pbi_answer` ~550-570)
- Modify: `src-tauri/src/commands/ai_bridge.rs` (switches struct ~230, apply ~437, `clear_bridge_pbi` ~246)
- Modify: `src-tauri/src/ai_bridge/autorun_review.rs` (`/autorun-order` ~640-716, `note_pbi_in` ~490)
- Modify: `src-tauri/src/mcp.rs` (the `get_current_pbi` description ~659)
- Modify: prompt/guide text: `src-tauri/src/ai_tools.rs` (~136, 338, 446, 488, 722), `src-tauri/src/autorun/guide.rs` (~1121-1131), `ai_bridge/guide_routes.rs` ~533, `ai_bridge/writing_routes.rs` ~373 (only where it is about Auto Test, not test-case writing)
- Test: `src-tauri/tests/suite/autorun_suite_scope.rs`, plus the existing current_pbi tests module

**Interfaces:**
- Produces:
  - `pub struct CurrentSuite { pub plan_id: i32, pub suite_id: i32, pub title: String, pub plan_name: String, pub project: Option<String> }` (serde + specta).
  - `BridgeContext.current_suite: Option<CurrentSuite>`. The bridge switches struct gains `suite: Option<CurrentSuite>`, applied like `pbi`, and cleared by `clear_bridge_pbi`.
  - `current_pbi_answer`:
    - with a PBI, unchanged;
    - with a suite and no PBI: `{"pbi": null, "suite": {plan_id, suite_id, title, plan_name, project}, "note": "No PBI is selected. The current suite is \"<title>\" (plan <plan_id>, suite <suite_id>). Read its cases with get_suite_test_cases.", autorun_mode, app_simple_mode}`;
    - with neither, exactly today's answer.
  - `/autorun-order` validates the case ids against the current scope's cases: the PBI's as today, or the suite's own points via the Task 1 helper. It saves to the scope-keyed order file.
  - `note_pbi_in` is skipped when there is no PBI.
- One line added to each Auto Test prompt/guide: when `get_current_pbi` names a suite, the cases come from that suite (`get_suite_test_cases` with its plan and suite ids).

- [ ] **Step 1: Write the failing tests:**
  - `current_pbi_answer_with_a_suite` (exact JSON keys and the note);
  - `current_pbi_answer_with_neither_unchanged`;
  - `context_push_sets_and_clears_the_suite`;
  - `autorun_order_validates_against_suite_cases`.
- [ ] **Step 2:** Run. Expect FAIL.
- [ ] **Step 3:** Implement. Regenerate bindings.
- [ ] **Step 4:** Run `autorun_suite_scope::`, `current_pbi::`, `tcm_mcp::` and `ai_bridge::`. Expect PASS.
- [ ] **Step 5:** Commit `feat(v2): the assistant knows the current test suite`.

### Task 4: Webview - choose a suite, and Auto Test on it

**Files:**
- Modify: `src/App.tsx` (state ~93, 108, `setPbi` ~221-234; run-finished navigation ~271-298)
- Modify: `src/components/ContextBar.tsx` and the `PbiPicker` it renders (PBI / Suite switch)
- Modify: `src/screens/Suites.tsx` (More menu ~455-478: add "Auto Test"); `src/app/CaseScreens.tsx` (~140-194: wire it and pass the scope to AutoRun)
- Modify: `src/screens/AutoRun/index.tsx` (cases query ~139, open cards, run setup, `startPlanned` ~716, the no-PBI message ~798), `src/lib/backgroundRun.ts` (`BackgroundRun.pbi`, `ReviewRequest.pbi`, `openRunSetup`, `autoRunReplay` call ~352), `src/screens/AutoRun/PastRuns.tsx` (~199-220), `src/screens/AutoRun/RunReview.tsx` (~111, 374-410, 470, 784), `src/components/BugDialog.tsx` (footer ~395)
- Modify: `src/app/useBridgeContextSync.ts` (push the `suite`)
- Modify: `src/dev/demo.ts` (demo answers for `suiteTestCasesFull`, a demo suite in the picker)
- Test: `tests/screens/AutoRun/*.test.tsx`, `tests/components/ContextBar*.test.tsx`, `tests/screens/Suites*.test.tsx`, `tests/app/*`

**Interfaces:**
- Consumes: Tasks 1-3 bindings (`suiteTestCasesFull`, `RunScope`, `LocalRun.suite`, the publish signature, the bridge `suite` switch).
- Produces (`src/lib/scope.ts`):
  - `type Scope = { kind: "pbi"; pbi: PbiHit } | { kind: "suite"; suite: SuiteScope }`, where `SuiteScope = { planId: number; suiteId: number; title: string; planName: string; project?: string }`;
  - `scopeKey(s: Scope): string`;
  - `toRunScope(s: Scope): RunScope`.
- App state:
  - `suite: SuiteScope | null` beside `pbi`. Setting either clears the other. It is remembered across restarts the way the PBI is (same prefs mechanism) and cleared on sign-out or an org change.
  - Auto Test receives `scope: Scope | null`.
- **Top bar:**
  - The picker has a PBI / Suite segmented switch. Suite mode searches suites by name through the existing suite search command that Search Suites uses (read Suites.tsx for it) and lists plan › suite path.
  - The chip shows a folder icon, the suite title and the plan name muted. Its clear button clears the suite.
- **Search Suites:** the More menu gains "Auto Test" for every suite. It sets the suite and goes to Auto Test.
- **Auto Test:**
  - With `scope.kind === "suite"`, cases come from `suiteTestCasesFull`.
  - Everything else (open cards, setup, review matching, cancel-on-scope-change) uses `scopeKey`.
  - The no-scope message says "Pick a PBI or a suite in the bar above".
- **Past runs:** a suite run says "for suite <title>"; the other-scope dimming compares scope keys.
- **Review:**
  - the mismatch compares scope keys;
  - the run name is `<suite title> - Auto Test`;
  - publish uses the new signature;
  - the bug link uses `pbi_id` 0 for a suite run.
- **BugDialog footer:** "Links to test case #id" when `pbiId` is 0, else as today.
- **App, after a run finishes:** it moves to the run's own scope (suite or PBI).

- [ ] **Step 1: Write the failing tests:**
  - choosing a suite in the picker sets the chip and clears a PBI, and the reverse;
  - Search Suites' Auto Test opens Auto Test on the suite;
  - Auto Test lists suite cases (calls `suiteTestCasesFull`);
  - Run setup and replay are called with the suite scope;
  - review Send is enabled for a matching suite run and calls publish;
  - Past runs labels a suite run;
  - BugDialog has no PBI in the footer for 0;
  - the bridge push carries the suite;
  - a scope change cancels an open setup.
- [ ] **Step 2:** Run the affected vitest files. Expect FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `npx tsc --noEmit`, the affected files, then the full `npm test`. Expect PASS (a docs built-page failure only if docs text changed).
- [ ] **Step 5:** Commit `feat(v2): open Auto Test on a test suite`.

### Task 5: Docs, backlog and the full gate

**Files:**
- Modify: `docs-site/src/content/auto-run.ts`, `search-suites.ts` and `getting-started.ts` (the top bar): one short paragraph each.
- Modify: `docs/backlog.html` (a done note at release time; leave it to release).

- [ ] **Step 1:** Write the docs text. List the shot ids showing the top bar picker, the Search Suites More menu and Auto Test's empty state.
- [ ] **Step 2:** Run the full `cd src-tauri && cargo test --test suite` and `npm test`. Expect 0 failed (docs built-page excepted until shots).
- [ ] **Step 3:** Commit `docs(v2): Auto Test on a test suite`.
