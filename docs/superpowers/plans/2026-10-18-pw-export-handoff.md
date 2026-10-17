# Export to Playwright: hand over what TCM knows - Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The export carries what TCM learned making a script pass (data a case needs, what it must not touch, order, quirks, page behaviour) into the places the Playwright pipeline already reads.

**Architecture:** A new `pw_export::handover` module turns a case's (fixture-resolved) script, the project's protected baseline, quirks, recipe, component mapping and run order into a `Handover` value. `test_case.rs` renders it into the case section (TCM-owned rows/blocks) while keeping repo-authored content; the export also writes two TCM-owned docs in the same commit phase, the handoff prompt repeats the essentials, the preview shows warnings, and the Sync fingerprint includes the handover.

**Tech Stack:** Rust (Tauri 2, serde, specta), React 19 + TS, vitest.

**Spec:** `docs/superpowers/specs/2026-10-17-pw-export-handoff-design.md` (E1-E11, its "Decisions", "Ownership on re-export", "Constraints" and "Calls I made" are binding).

## Global Constraints

- Everything is additive: never change how the repo runs, heals, refactors or lints; never edit `.claude/agents/playwright-test-healer.md`.
- TCM owns: its case section, the `**From TCM:**` block, Preconditions/Cleanup rows ending ` (from TCM)`, `docs/tcm-protected-baseline.md`, `docs/app-quirks.md` entries tagged `<!-- tcm:… -->`, and the md's `<!-- tcm:order -->` block. It never edits `**Also:**`, `**Added by engineer (not from ADO):**`, `**Assertion floor:**`, untagged quirks, or rows not ending ` (from TCM)`.
- Repo formats (confirmed in the clone 2026-10-18): `**User:** <key>  <!-- assumed -->` (two spaces before the comment); `**Uses:** common-components/<name>` one line per component beside `**User:**`/`**Navigation:**`; `docs/app-quirks.md` entries are `### <title>` then bold-labelled paragraphs `**Symptom in a failing run:**`, `**Cause:**`, `**Fix pattern (raw spec):**`, `**Established live:**` (YYYY-MM-DD), `**Permanent fix:**`, `**Class:**`, entries separated by a line `---`.
- Exportability changes only by E2 (setup cases refused; leftover placeholders refused; a shared-draft fixture with no current output refused). The newest-run-Passed rule stays.
- New docs files go through the existing `commit(root, &outs)` phase in `export.rs` and are named in its failure message.
- Sync fingerprint: the new handover content goes in a new part hashed ONLY when non-empty, so existing exports do not flip to Out of date.
- Inferred writes are worded "appears to …".
- Rust tests only in `src-tauri/tests/suite/<module>.rs` (+ `mod` line in `suite/main.rs`); `src/bindings.ts` regenerated, never hand-edited; one build cache; no hardcoded colours; icons from `src/lib/actionIcons.ts`; never name hidden features.

## Review Focus

1. A re-export over an md the repo pipeline edited (step-4 Preconditions rows, engineer blocks, a hand-edited quirk) → nothing repo-authored lost or reordered; only TCM-tagged content replaced (Task 2 `a_re_export_keeps_every_row_the_repo_added`, Task 4 `untagged_quirks_are_never_touched`).
2. A fixture rebuilt between preview and write → the written spec uses the value read at write time and Sync shows Out of date afterwards (Task 1 `a_rebuilt_shared_draft_makes_the_export_out_of_date`).
3. Two exported cases in one md, one look-only and one that publishes → each case's `**From TCM:**` block names only its own facts, and the md's order block names both (Task 5 `two_cases_in_one_file_keep_their_own_facts`).
4. A protected baseline that is empty, or a project with no quirks/recipe popups → no empty headings or empty docs written; existing repo docs untouched (Task 3 `an_empty_baseline_writes_nothing`, Task 4 `no_quirks_no_change`).
5. A case whose ADO title/steps contain `|` or a line break → the table rows the export writes stay one row each (Task 2 `a_pipe_in_a_value_stays_in_its_cell`).

---

### Task 1: E2 - resolve shared drafts, refuse setup cases and leftover placeholders

**Files:**
- Modify: `src-tauri/src/pw_export/export.rs` (`load_expanded` ~273-284, `evaluate` ~355-498, `Fingerprinter::of` ~335-352), `src-tauri/src/pw_repo/sync.rs` (`SyncState` ~213-231, `states` ~277-333, `add_new` ~341)
- Test: `src-tauri/tests/suite/pw_export_write.rs`, `src-tauri/tests/suite/pw_repo_sync.rs`

**Interfaces:**
- Consumes: `autorun::setup::{fixture_values, resolve}` (setup.rs ~201, ~265), `api_templates::fixture_store::current_outputs` (~148), `api_templates::exec::placeholders`.
- Produces: `pub const SETUP_STAYS_IN_TCM: &str` (refusal text) and `pub fn resolved_script(root, org, project, script: CaseScript) -> Result<CaseScript, String>` in export.rs (returns the script with every `{{fixture.<id>.<output>}}` substituted; Err naming the fixture when it has no current output, or naming any placeholder left). `Ready.script` is the resolved script; `pub struct SharedDraft { fixture: String, name: String }` list on `Ready` (`shared_drafts: Vec<SharedDraft>`) for Task 2's Preconditions rows. Sync: `SyncState::NotExportable { reason: String }`.

- [ ] **Step 1: Failing tests** in `pw_export_write.rs`:
  - `a_case_with_a_setup_is_refused_and_says_it_stays_in_tcm` — script with `setup: Some(Setup{fixture:"f1"})`: preview `exportable == false`, `reason == Some(format!("builds its own draft with TCM fixture `f1`; Playwright has no fixture mechanism - stays TCM-only"))`.
  - `a_shared_draft_value_is_written_into_the_spec` — fixture `f2` with a newest ok run whose outputs have `name = "Shared FY26 Draft"`, a fill of `{{fixture.f2.name}}`: exportable; the written raw spec contains `Shared FY26 Draft` and no `{{`.
  - `a_shared_draft_with_no_current_output_is_refused` — fixture never run: refused, reason names `f2`.
  - `a_leftover_placeholder_is_refused_never_written` — `{{something.x}}` in a step: refused, reason names `{{something.x}}`; nothing written.
  - `a_rebuilt_shared_draft_makes_the_export_out_of_date` (`pw_repo_sync.rs`) — export, then a newer fixture run with a different `name`: state is `OutOfDate`.
  - `a_setup_case_is_listed_as_not_exportable` (`pw_repo_sync.rs`) — `SyncState::NotExportable` with the same reason, never `OutOfDate`.
- [ ] **Step 2:** `cd src-tauri && cargo test --test suite pw_export_write:: pw_repo_sync::` → FAIL.
- [ ] **Step 3:** Implement. Refusal order: right after the verdict check (~392). `load_expanded` → resolve after expansion; the fingerprint is taken from the resolved steps. Sync renders `NotExportable` in the webview like the existing excluded state (find where SyncState is shown in `src/screens/Playwright/Sync.tsx`; add the row text "Stays in TCM: <reason>").
- [ ] **Step 4:** PASS (+ bindings regenerated, `npx vitest run src/screens/Playwright`). **Step 5:** Commit `feat(v2): Playwright export - shared drafts substituted, setup cases stay in TCM`.

### Task 2: E1 + E3 - the handover module, Preconditions rows, the From TCM block, Cleanup rows

**Files:**
- Create: `src-tauri/src/pw_export/handover.rs` (+ `pub mod handover;` in `pw_export/mod.rs`)
- Modify: `src-tauri/src/pw_export/test_case.rs` (`CaseDoc` ~10-24, `section` ~52-99, preservation `local_blocks`/`with_local_blocks` ~184-251), `export.rs` write loop (~820-836)
- Test: Create `src-tauri/tests/suite/pw_export_handover.rs`; modify `pw_export_test_case.rs`

**Interfaces:**
- Produces (Tasks 3-9 add to it):
  ```rust
  #[derive(Debug, Clone, Default, PartialEq)]
  pub struct Handover {
      pub preconditions: Vec<String>, // Condition text, renderer appends " (from TCM)"
      pub cleanup: Vec<String>,       // Cleanup action text, renderer appends " (from TCM)"
      pub from_tcm: Vec<String>,      // bullet lines of the **From TCM:** block
      pub uses: Vec<String>,          // Task 6
      pub warnings: Vec<String>,      // Task 8
  }
  pub struct HandoverInput<'a> {
      pub script: &'a CaseScript,               // resolved
      pub shared_drafts: &'a [SharedDraft],     // Task 1
      pub protected: &'a [ProtectedRecord],     // Task 3 (empty until then)
  }
  pub fn handover_for(input: &HandoverInput) -> Handover;
  ```
  `CaseDoc` gains `pub handover: Handover` (default empty; `read_docs` leaves it default, the write loop fills it).
- Rules (E1): one precondition row per `script.preconditions` entry: `"<flow> at <stage> stage"` (+ `": <why>"` when set); per `needs_unchanged` item `"requires: <item> unchanged"`; per `needs_state` entry `"requires: <key> = <value>"`; per shared draft `"shared draft \"<name>\", rebuilt by TCM fixture \`<fixture>\`"`. ADO Preconditions rows stay first.
- Rules (E3) `from_tcm` lines, in this order, each only when it applies:
  - `no_save` → `"Look-only: must not save anything."`
  - each step whose actions click/press a control named (case-insensitive) Delete/Remove → `"step N appears to delete data (\"<name>\")"`; Save/Submit/Create/Add/Update → `"step N appears to save data (\"<name>\")"`; Publish → `"step N appears to publish - irreversible; run on demand only"` (the publish line also goes into `warnings` in Task 8)
  - each `changes` item → `"changes shared state: <item>"`
  - protected records present (Task 3) → `"must never touch the protected records in docs/tcm-protected-baseline.md"`
  - suspected defect → `"step <N> is a known product defect (TCM mark <YYYY-MM-DD>): <note>"` (date from `marked_at` epoch ms, UTC)
  - cleanup: a step whose actions delete a record the case created earlier (a delete-named click after a fill whose value appears in the delete's locator text) → `cleanup` row `"step N deletes \"<value>\" it created"` and `from_tcm` line `"relies on step N to put data back"`. Described only - no `afterEach`.
- Rendering in `section()`: `**From TCM:**` block (heading line `**From TCM:**` then `- ` bullets) right after the `**Navigation:**` line; omitted when `from_tcm` is empty. Preconditions/Cleanup rows numbered after ADO rows.
- Preservation: on `splice`, rows in the old Preconditions and Cleanup tables NOT ending ` (from TCM)` and not equal to an ADO row are kept, after the ADO rows and before the new TCM rows, renumbered. The old `**From TCM:**` block is replaced wholesale (never added to `local_blocks`).

- [ ] **Step 1: Failing tests** (`pw_export_handover.rs` for `handover_for` rules; `pw_export_test_case.rs` for rendering):
  - `preconditions_come_from_the_script` (flow/stage/why, needs_unchanged, needs_state, shared draft → exact strings above)
  - `a_look_only_case_says_so_first`, `delete_save_publish_steps_are_described_as_appearing_to`, `a_suspected_defect_names_its_step_date_and_note`, `a_created_record_deleted_later_is_the_cleanup`
  - `the_from_tcm_block_sits_after_navigation_and_is_omitted_when_empty`
  - `tcm_rows_end_with_from_tcm_and_follow_the_ado_rows`
  - `a_re_export_keeps_every_row_the_repo_added` (old md with an extra Preconditions row and Cleanup row added by the repo + an engineer block; re-splice keeps them, replaces only `(from TCM)` rows and the From TCM block)
  - `a_pipe_in_a_value_stays_in_its_cell` (`|` escaped as `\|`, newlines → space, in every row TCM writes)
- [ ] **Step 2:** FAIL. **Step 3:** Implement; fill `doc.handover` in the write loop from `r.script` + `r.shared_drafts`. **Step 4:** PASS (`pw_export_handover:: pw_export_test_case:: pw_export_write::`). **Step 5:** Commit `feat(v2): Playwright export - preconditions, safety facts and cleanup from TCM`.

### Task 3: E4 - protected baseline (setting, Setup dialog, repo doc)

**Files:**
- Modify: `src-tauri/src/autorun/nav.rs` (`NavFile` ~90-102, `NavView` ~122-130; add `set_protected` beside `set_save_words` ~365), `src-tauri/src/commands/autorun.rs` (command beside `auto_run_set_save_words` ~2603), `src-tauri/src/lib.rs` (register), `src-tauri/src/pw_export/export.rs` (`Ctx`, `outs`), `src-tauri/src/pw_export/handover.rs`
- Create: `src/screens/AutoRun/ProtectedRecordsDialog.tsx` (+ test), opened from `src/screens/AutoRun/SetupPanel.tsx` beside Save words (~792-808)
- Test: `src-tauri/tests/suite/pw_export_write.rs`, a nav test module that already covers `set_save_words` (grep), `src/screens/AutoRun/ProtectedRecordsDialog.test.tsx`

**Interfaces:**
- Produces: `#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, PartialEq)] pub struct ProtectedRecord { pub kind: ProtectedKind, pub name: String }`, `pub enum ProtectedKind { GoalGroup, RatingMethod, ProficiencyScale, CompetencyGroup, Cycle, Other }` (serde snake_case); `NavFile.protected: Vec<ProtectedRecord>` (`#[serde(default)]`); command `auto_run_set_protected(organization, project, records: Vec<ProtectedRecord>) -> Result<NavView, String>` (trimmed names, empty names dropped, duplicates by kind+name case-insensitive dropped).
- Seed (offered by the dialog's "Add the usual PMS records" button, never written automatically): GoalGroup "Customer Focus", GoalGroup "Operational Excellence", RatingMethod "5-Point Scale", RatingMethod "Four Level Scale", ProficiencyScale "6-level proficiency scale", CompetencyGroup "Core Competencies", CompetencyGroup "Technical Competencies", Cycle "FY2026 (published)", Cycle "FY2025 (unpublished)".
- Repo doc `docs/tcm-protected-baseline.md` (TCM-owned, whole file): heading `# Protected baseline (from TCM)`, one sentence that tests must never edit or delete these records and that TCM rewrites this file on export, then one `## <Kind>` section per kind present with `- <name>` lines. Written in the export's `outs` when the list is non-empty; when empty and the file exists from an earlier export, it is left as is (never deleted - no DELETE-like behaviour on repo files).

- [ ] **Step 1: Failing tests:** `the_baseline_is_kept_per_project_and_tidied`; `an_export_writes_the_baseline_doc_and_each_case_points_at_it` (doc content exact; From TCM line from Task 2's rule present); `an_empty_baseline_writes_nothing`; dialog test: add/remove rows, kind picker (use `src/components/ui/select.tsx`), "Add the usual PMS records" fills the seed without duplicates, Save calls the command.
- [ ] **Step 2:** FAIL. **Step 3:** Implement; regenerate bindings. **Step 4:** PASS (+ `npx vitest run src/screens/AutoRun src/ui-consistency`). **Step 5:** Commit `feat(v2): a protected baseline per project, handed to the Playwright repo`.

### Task 4: E5 + E6 - quirks and page behaviour into docs/app-quirks.md

**Files:**
- Create: `src-tauri/src/pw_export/app_quirks.rs`
- Modify: `export.rs` (`Ctx` loads quirks via `autorun::quirks::load_quirks`; `outs` gains the updated `docs/app-quirks.md`)
- Test: Create `src-tauri/tests/suite/pw_export_app_quirks.rs`

**Interfaces:**
- Produces: `pub fn merge_app_quirks(existing: Option<&str>, entries: &[TcmQuirkEntry]) -> Option<String>` (None = no change needed) and `pub struct TcmQuirkEntry { pub tag: String, pub title: String, pub symptom: String, pub cause: String, pub fix: String, pub established: String, pub permanent: String, pub class: String }`; `pub fn quirk_entries(quirks: &[Quirk], case_ids: &[i32], recipe: Option<&SignInRecipe>) -> Vec<TcmQuirkEntry>`.
- Rules: relevant quirk = `is_active()` and a `sources` entry for an exported case id. Tag `tcm:<quirk id>`; title = first sentence of `text` (≤ 80 chars); symptom = "a step on this page fails without this handling (from TCM)"; cause = `text`; fix = `text` for the fix pattern line prefixed "Handle as TCM does: "; established = quirk `at` date YYYY-MM-DD; permanent = `not yet promoted`; class = `app quirk`. Recipe (E6): each `close_when_shown` → tag `tcm:popup:<name>`, title `<name> pops up and blocks the page`, fix "close it with <close target as text> whenever <shown target as text> is visible"; each `wait_while_shown` → tag `tcm:loading:<name>`, title `<name> loading indicator`, fix "wait while <shown target> is visible, up to <limit_ms/1000> s". Same other fields.
- Format: each entry exactly in the repo's entry shape (Global Constraints) with `<!-- <tag> -->` on the line right after `### <title>`. Merge: an existing entry whose tag line matches is replaced in place; new entries are appended at the end of the file, each preceded by `---`. If the file does not exist it is created with heading `# Known application quirks` and the entries. Untagged content is never touched.

- [ ] **Step 1: Failing tests:** `a_tagged_entry_is_replaced_in_place`, `new_entries_are_appended_after_a_rule`, `untagged_quirks_are_never_touched`, `no_quirks_no_change` (returns None), `recipe_popups_and_loaders_become_entries`, `only_quirks_of_exported_cases_travel`, and in `pw_export_write.rs` `an_export_with_a_quirk_updates_app_quirks_in_the_same_commit`.
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS. **Step 5:** Commit `feat(v2): Playwright export - TCM quirks and page behaviour into app-quirks.md`.

### Task 5: E7 - run order and grouping notes

**Files:**
- Modify: `export.rs` (preview/write take `pbi_id: Option<i32>`; `Ctx` loads `autorun::store::load_order(root, pbi)`), `src-tauri/src/commands/pw_export.rs` (`pw_export_preview`, `pw_export_write` gain `pbi_id: Option<i32>`), `src/screens/Playwright/PlaywrightExportDialog.tsx` (pass `pbiId`), `test_case.rs` (order block), `handover.rs`
- Test: `pw_export_test_case.rs`, `pw_export_write.rs`

**Interfaces:**
- Produces: `pub fn order_note(cases: &[(i32, &CaseScript)], saved_order: Option<&[i32]>) -> Vec<String>` in handover.rs, using `autorun::plan::Marks::of` / `needs_of` for dependencies. Lines: `"run <a> before <b> (<b> needs what <a> sets up)"`; `"run <id> alone and last: it publishes"` (publish inferred as in Task 2) / `"run <id> alone and last: it changes shared state (<item>)"`; when a saved order exists, `"saved TCM order: <ids joined by ', '>"`; `"suggested group: <ids> (run together, in this order)"` for chains of dependent cases. Plus one fixed line: `"Proposed for the repo owner: tag publishing cases @publishes and destructive ones @destructive and keep them out of routine runs."`
- md: a TCM-owned block between `<!-- tcm:order -->` and `<!-- /tcm:order -->` placed right after the file's `**User:**` line, heading `**Order (from TCM):**` with `- ` lines; rewritten on each export for the cases in that file; omitted when there are no lines besides the proposal line.

- [ ] **Step 1: Failing tests:** `order_lines_from_marks_and_saved_order`, `two_cases_in_one_file_keep_their_own_facts` (Review Focus 3), `the_order_block_is_replaced_not_duplicated`, `no_order_block_for_independent_cases`.
- [ ] **Step 2:** FAIL. **Step 3:** Implement; regenerate bindings. **Step 4:** PASS (+ `npx vitest run src/screens/Playwright`). **Step 5:** Commit `feat(v2): Playwright export - run order notes`.

### Task 6: E8 + E9 - provenance marker and `**Uses:**` from mapped components

**Files:**
- Modify: `test_case.rs` (`new_file` ~101-107: `<!-- from-tcm -->` → `<!-- assumed -->`; existing files' header lines whose comment is exactly `<!-- from-tcm -->` are rewritten to `<!-- assumed -->`, nothing else in the header touched), `pw_export/mapping.rs` (`ExportMap.components: BTreeMap<String, String>` `#[serde(default)]`), `export.rs` (`load_expanded` keeps the component names from `expand_step`'s `Option<String>` tags), `handover.rs` (`uses`), `section()` renders one `**Uses:** common-components/<x>` per mapped component right after `**Navigation:**`, `src/screens/Playwright/PlaywrightExportDialog.tsx` (a "Components" mapping section in the Mappings card: one Combobox per TCM component used by the listed cases, options = folder names under the clone's `suites/common-components/` that contain a content dir, plus "Not mapped")
- Test: `pw_export_test_case.rs` (update the header test ~65-68), `pw_export_mapping.rs`, `pw_export_write.rs`, `PlaywrightExportDialog.test.tsx`

**Interfaces:**
- Produces: `Handover.uses`; the preview gains `components: Vec<String>` (TCM component names used by the listed cases) and the clone's common components list (`common_components: Vec<String>`) for the dialog.

- [ ] **Step 1: Failing tests:** `a_new_file_marks_the_user_as_assumed` (`**User:** KEY  <!-- assumed -->`), `an_old_from_tcm_marker_becomes_assumed`, `a_mapped_component_writes_a_uses_line`, `an_unmapped_component_writes_none`, `the_component_map_round_trips`, dialog test for the Components section saving into `map.components`.
- [ ] **Step 2:** FAIL. **Step 3:** Implement; regenerate bindings. **Step 4:** PASS. **Step 5:** Commit `feat(v2): Playwright export - assumed user marker and Uses lines for mapped components`.

### Task 7: E10 - the Finish in the repo prompt carries the rest

**Files:**
- Modify: `src-tauri/src/pw_repo/handoff.rs` (`PromptInput` ~86-98, `prompt` ~101-154, `cards` ~320-373)
- Test: `src-tauri/tests/suite/pw_repo_handoff.rs` (update the full-prompt test ~104-124)

**Interfaces:**
- `PromptInput` gains `has_from_tcm: bool`, `order: Vec<String>`, `uses_written: bool`. `cards` reads them from the md file it already locates (the `**From TCM:**` presence for the cases in the card, the `<!-- tcm:order -->` block's lines, and whether every case in the card has a `**Uses:**` line) - no new caller inputs.
- New sentences (inserted after the missing-precondition sentence ~130): when `has_from_tcm`: `"Read each case's **From TCM:** block before running: it says what the case must not save, what it appears to write or publish, and the cleanup it relies on."`; always: `"Read docs/tcm-protected-baseline.md and the entries tagged tcm: in docs/app-quirks.md first; never fix a test by editing or deleting a record named in the protected baseline."`; always: `"A failure on a step the From TCM block lists as a known product defect is a product defect, not something to heal."`; when `order` non-empty: `"Order: "` + lines joined by `"; "` + `". Keep publishing and destructive cases out of routine runs."` and the "The tests are independent (no --chain)." sentence is replaced by `"Run them in the order above."` When `uses_written`, the `**Uses:**` checkpoint sentence (~137-140) is dropped.

- [ ] **Step 1: Failing tests:** update the exact full-prompt assertion for a card with none of the new facts (only the two "always" sentences added); `a_card_with_from_tcm_facts_and_order_says_so`; `written_uses_lines_drop_the_checkpoint`.
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS. **Step 5:** Commit `feat(v2): Finish in the repo - safety, order and baseline in the prompt`.

### Task 8: E11 - export warnings in the dialog

**Files:**
- Modify: `export.rs` (`PreviewCase` ~33-55 gains `pub warnings: Vec<String>`), `handover.rs` (`warnings`), `src/screens/Playwright/PlaywrightExportDialog.tsx` (`caseRow` ~383-425), docs `docs-site/src/content/playwright.ts`
- Test: `pw_export_write.rs`, `PlaywrightExportDialog.test.tsx`

**Interfaces:**
- Warning strings (each only when it applies; computed in preview for exportable AND refused-for-other-reasons cases that have a script):
  - a delete-named click whose locator uses an `nth`/first-row choice: `"step N appears to delete a row it did not name"`
  - `repeat_while_visible` without `except`: `"step N appears to delete every matching row (no protected rows listed)"`
  - publish: `"step N appears to publish - irreversible"`
  - needs state another listed case changes: `"needs <item>, which #<other id> changes"`
  - quirks attached (Task 4 relevance): `"<n> TCM quirk(s) travel with this case"`
  - "run twice" (spec decision 3): the newest TWO runs holding the case were not both Passed → `"not yet passed twice in a row in TCM"`
- UI: under the case row's head, each warning as a `text-warning` line with the warning icon from `actionIcons` (aria-hidden); never blocks Export.

- [ ] **Step 1: Failing tests:** one Rust test per warning string, and a dialog test that warnings render and Export stays enabled.
- [ ] **Step 2:** FAIL. **Step 3:** Implement; regenerate bindings; update the guide's export steps; `npm run docs:build`. **Step 4:** PASS (+ `npx vitest run src/screens/Playwright src/ui-consistency docs-site`). **Step 5:** Commit `feat(v2): Export to Playwright - warnings before writing`.

### Task 9: Sync fingerprint includes the handover

**Files:**
- Modify: `src-tauri/src/pw_repo/sync.rs` (`Parts` ~32-38 gains `#[serde(default)] pub handover: String`; `FingerprintInput` ~52-64 gains `handover: String`; `fingerprint()` ~90-97 includes `"handover"` in the hashed JSON ONLY when non-empty; `changed()` ~248-267 names "TCM facts" when it differs), `export.rs` (`Fingerprinter::of` fills it with the rendered `**From TCM:**` lines + Preconditions TCM rows + this case's order lines, joined by `\n`)
- Test: `src-tauri/tests/suite/pw_repo_sync.rs` (rewrite the assertion at ~187-192: a marks-only edit now flips to `OutOfDate` with "TCM facts" named; add `an_export_from_before_this_change_stays_current`)

- [ ] **Step 1: Failing tests** as above. **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS (`pw_repo_sync:: pw_export_write::`). **Step 5:** Commit `feat(v2): Sync - a changed TCM fact makes an export out of date`.

### Task 10: Gate

- [ ] `cd src-tauri && cargo test --tests`, then `npm test`, then `npm run build` - one at a time, all green.
- [ ] Owner hand checks (cannot run here): a real export into the clone, then `npm run lint:tests -- --require-specs` in the clone passes; the Finish in the repo prompt in Claude Code reads the new blocks; app-quirks.md diff shows only tagged entries.
