# Step Attachments Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Files can be attached to, listed on, and removed from individual test-case steps in every step editor and by assistants, and land in Azure DevOps as step attachments that stay on the right step.

**Architecture:** Attachments are data on each `Step` (`attachments: Vec<StepAttachment>`), so they move with the step in the webview. At upload the final Steps XML is computed as today; `parse_step_ids(final_xml)` gives each step's id, and one pure function turns (final ids, current relations, draft attachments, `removed_attachments`, uploaded blobs) into JSON-patch relation ops appended to the existing create/update documents. Relations nobody carried or removed are never touched.

**Tech Stack:** Rust (Tauri 2, reqwest, serde, specta/tauri-specta rc.25, wiremock in tests), React 19 + TypeScript, vitest/jsdom, `@tauri-apps/plugin-dialog`.

**Spec:** `C:\Users\Admin\AppData\Local\Temp\claude\C--Users-Admin-Desktop-Main-Menu-Software-Personal-Projects-Automated-Test-Cases-Creator-v2\745a29a5-7630-44a4-bf8d-16a538a04432\scratchpad\step-attachments-design.md` (copy it to `docs/superpowers/specs/2026-11-03-step-attachments-design.md` in Task 1 so it travels with the branch).

## Global Constraints

- Branch `release/2.4.3` created from tag `v2.4.2`. No beta work is merged in. Paths below are as they are at `v2.4.2`.
- **No HTTP DELETE anywhere.** Removal is a JSON-patch `{"op":"remove","path":"/relations/<i>"}` in a PATCH; `tests/suite/ado.rs`'s DELETE scan must keep passing.
- Every index-based relation op (`replace`, `remove`) is preceded in the same document by `{"op":"test","path":"/rev","value":<rev>}`; removes are emitted in descending index order.
- Relation comment written exactly as `[TestStep=<id>]:` followed by the kept comment text; parsed leniently with `^\s*\[TestStep=(\d+)\]:` (rest = comment).
- Relation for a new file: `{"rel":"AttachedFile","url":<blob url>,"attributes":{"name":<file name>,"comment":"[TestStep=<id>]:"}}`.
- File cap: `STEP_ATTACHMENT_MAX_BYTES = 25 * 1024 * 1024` (25 MB) per file. Paths must pass `crate::test_files::is_plain_local_path`.
- Rust owns all file reads and network I/O; the webview only holds paths, names and sizes.
- User-facing errors name no URL; transport errors go through `network_error` / `AdoError::user_text`, raw error to `applog`.
- `src/bindings.ts` is regenerated with `cd src-tauri && cargo test --test bindings`, never hand-edited.
- No new cache on either side.
- Icons only from `src/lib/actionIcons.ts` (`IconAttach`, `IconRemove`); colours only from theme tokens; `src/ui-consistency.test.ts` is never weakened.
- Rust tests go in `src-tauri/tests/suite/<module>.rs` with a `mod` line in `src-tauri/tests/suite/main.rs`; never `#[cfg(test)]` in `src/`. Frontend tests sit beside the component (`*.test.tsx`), as the repo does.
- One build or test command at a time; one cache `src-tauri/target`; never `CARGO_TARGET_DIR`.
- Commits via Bash heredoc `git commit -F - <<'EOF'`, ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Never name hidden features in commits/docs.
- Old drafts, exports and caches (no `attachments` / `remove_attachments` keys) load and upload exactly as in 2.4.2.

## Review Focus

1. **An old draft or old JSON (no attachment keys) uploaded over a case that HAS step attachments in ADO** - expected: no relation op at all, attachments untouched. Pinned in Task 3 (`no_attachment_work_means_no_ops`) and Task 5 (`old_queue_update_sends_no_relation_ops`).
2. **A step with an attachment is moved AND edited so `merge_steps_xml` gives it a fresh id** - expected: the comment is re-pointed to the new id, not orphaned. Pinned in Task 3 (`moved_and_edited_step_repoints_comment`).
3. **The file was deleted or grew past 25 MB between picking and uploading** - expected: only that row fails, before any request, with a sentence naming the file. Pinned in Task 5 (`missing_file_fails_row_before_sending`).
4. **Uploading the same queue twice** - expected: the second upload adds no duplicate relation (paths were swapped for GUIDs from `attached`). Pinned in Task 8 (`second_upload_carries_ids_not_paths`).
5. **An attachment-only change on an update row** - expected: the row is not skipped as unchanged and the review shows the change. Pinned in Task 8 (`attachment_only_change_is_a_change`).

---

### Task 1: Model, marker and the pure relation-ops function

**Files:**
- Create: `src-tauri/src/step_attachments.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod step_attachments;` near `pub mod steps_xml;` line 66)
- Modify: `src-tauri/src/steps_xml.rs:39-49` (`Step` gains `attachments`)
- Modify: `src-tauri/src/model.rs:51-127` (`TestCase` gains `removed_attachments`; `is_valid` refuses attachments on a shared step)
- Modify: every `Step { .. }` struct literal in `src-tauri/src` and `src-tauri/tests/suite` (about 118; add `..Default::default()` or `attachments: vec![]`)
- Create: `src-tauri/tests/suite/step_attachments.rs`; Modify: `src-tauri/tests/suite/main.rs`
- Create: `docs/superpowers/specs/2026-11-03-step-attachments-design.md` (copy of the spec)

**Interfaces:**
- Produces:
  - `pub struct StepAttachment { id: Option<String>, path: Option<String>, name: String, size: Option<u64>, comment: String }` (serde/specta attrs as in the spec).
  - `pub const STEP_ATTACHMENT_MAX_BYTES: u64 = 25 * 1024 * 1024;`
  - `pub fn marker(step_id: &str, comment: &str) -> String` -> `"[TestStep=7]:" + comment`
  - `pub fn parse_marker(comment: &str) -> Option<(String /*step id*/, String /*rest*/)>`
  - `pub fn attachment_guid(url: &str) -> Option<String>` (last path segment, query stripped)
  - `pub struct CaseRelation { pub index: usize, pub guid: String, pub name: String, pub size: Option<u64>, pub comment: String }`
  - `pub struct CaseRelations { pub rev: i64, pub attachments: Vec<CaseRelation> }` (AttachedFile relations only, index = position in the work item's full `relations` array)
  - `pub struct Uploaded { pub path: String, pub guid: String, pub url: String, pub name: String }`
  - `pub struct AttachmentOps { pub ops: Vec<serde_json::Value>, pub notes: Vec<String> }`
  - `pub fn relation_ops(final_ids: &[String], steps: &[Step], removed: &[String], current: Option<&CaseRelations>, uploaded: &[Uploaded]) -> AttachmentOps` - rules 1-4 of the spec; `current: None` = a create.
  - `pub fn has_attachment_work(tc: &TestCase) -> bool` (any step attachment, or any removed GUID)
  - `Step.attachments: Vec<StepAttachment>`, `TestCase.removed_attachments: Vec<String>`

- [ ] **Step 1: Live check (read-only).** With the owner's sign-in, GET one Test Case that has a step attachment added through the ADO web form (`?$expand=relations`) and record the relation's exact `attributes` in the test file's top comment. If the marker differs from `[TestStep=N]:`, change the constant and the Global Constraint before going on, and tell the controller.
- [ ] **Step 2: Write the failing tests** in `tests/suite/step_attachments.rs`:
  - `marker_round_trips`: `marker("3","")=="[TestStep=3]:"`, `parse_marker("[TestStep=3]:see log")==Some(("3","see log"))`, `parse_marker("Linked by X")==None`.
  - `guid_from_url`: `attachment_guid("https://x/_apis/wit/attachments/aaaa-1?fileName=a.png")==Some("aaaa-1")`.
  - `no_attachment_work_means_no_ops`: steps without attachments, `removed` empty, current has 2 relations -> `ops.is_empty()`.
  - `new_file_on_create_adds_relation`: create (`current: None`), step 2 of 2 carries `{path:"C:\\a.png"}`, `uploaded` has it -> exactly one op `add /relations/-` with comment `"[TestStep=3]:"` and `name` `"a.png"`, no `test` op.
  - `moved_and_edited_step_repoints_comment`: relation index 4 says `[TestStep=2]:note`, the step carrying it now has final id `"9"` -> ops `[test /rev, replace /relations/4/attributes/comment "[TestStep=9]:note"]`.
  - `removed_guid_is_removed_desc`: removed `["g1","g3"]` at indexes 1 and 5 -> `[test, remove /relations/5, remove /relations/1]`.
  - `carried_and_removed_keeps`: GUID both carried and removed -> no remove op.
  - `carried_guid_missing_on_item_notes`: carried GUID not in current -> no op, one note.
  - `existing_guid_on_create_notes`: create with `{id:"g"}` -> no op, one note.
  - `old_json_step_has_no_attachments_key`: `serde_json::to_value(Step{action,expected,..Default})` has no `attachments` key; same for `TestCase` and `removed_attachments`.
  - `shared_step_with_attachment_is_invalid`: `TestCase::is_valid` errors.
- [ ] **Step 3: Run** `cd src-tauri && cargo test --test suite step_attachments::` - expect compile failure / FAIL.
- [ ] **Step 4: Implement** the module, the two fields, the `is_valid` rule and the literal sweep. In `relation_ops`, ops order: `test` (only if any index op), `replace`s ascending, `remove`s descending, `add`s.
- [ ] **Step 5: Run** `cd src-tauri && cargo test --test suite step_attachments::` then the whole `cargo test --tests` - PASS (proves the literal sweep broke nothing).
- [ ] **Step 6: Commit** `feat(steps): step attachments in the model, the TestStep marker, and the relation ops`.

### Task 2: Azure DevOps reads and blob upload for step attachments

**Files:**
- Create: `src-tauri/src/ado/attachments.rs` (`impl AdoClient`); Modify: `src-tauri/src/ado/mod.rs` (`pub mod attachments;` with the other mods at 12-21; `TestCaseFull` at 96-139 gains `pub attachments_read: bool`)
- Modify: `src-tauri/src/ado/endpoints.rs:1149-1170` (set `attachments_read: false` where `TestCaseFull` is built)
- Create: `src-tauri/tests/suite/ado_step_attachments.rs`; Modify: `src-tauri/tests/suite/main.rs`

**Interfaces:**
- Consumes: Task 1 types, `AdoClient::upload_wi_attachment` (`endpoints.rs:1348`, returns the blob URL).
- Produces:
  - `pub async fn case_relations(&self, org: &str, ids: &[i32]) -> Result<HashMap<i32, CaseRelations>, AdoError>` - `GET {org}/_apis/wit/workitems?ids=..&$expand=relations&errorPolicy=omit&api-version=7.1`, chunked at `WORKITEM_BATCH_SIZE`.
  - `pub fn fill_step_attachments(cases: &mut [TestCaseFull], rel: &HashMap<i32, CaseRelations>)` - each relation whose marker id equals `step_ids[j]` becomes a `StepAttachment{id,name,size,comment}` on `steps[j]`; sets `attachments_read = true` for cases present in `rel`. Relations without a marker or for an unknown step id are not shown.
  - `pub async fn load_step_attachments(&self, org: &str, cases: &mut [TestCaseFull])` - calls the two above; on error logs and leaves `attachments_read=false`.
  - `pub fn read_step_file(path: &str) -> Result<(String /*name*/, Vec<u8>), String>` - local-path rule, exists, is a file, <= 25 MB; sentences name the file, never a URL.
  - `pub async fn upload_step_files(&self, org: &str, project: &str, steps: &[Step]) -> Result<Vec<Uploaded>, String>` - every `path` attachment once, via `read_step_file` then `upload_wi_attachment`; the first failure returns its sentence (transport errors through `user_text`).

- [ ] **Step 1: Write the failing tests** (wiremock, as `tests/suite/ado_share.rs` does):
  - `case_relations_reads_attached_files_with_indexes`: mock returns relations `[TestedBy, AttachedFile(comment "[TestStep=2]:")]` with `rev: 7` -> one `CaseRelation{index:1,..}`, `rev==7`.
  - `fill_puts_attachment_on_its_step`: `step_ids ["2","3"]`, relation marker 3 -> `steps[1].attachments.len()==1`, `steps[0]` empty, `attachments_read`.
  - `read_step_file_refuses_over_cap_and_unc` (temp file of 25 MB + 1 byte; `\\\\server\\x.png`) - errors, neither message contains `http`.
  - `upload_step_files_posts_octet_stream_once_per_path` - mock `POST /o/p/_apis/wit/attachments` hit twice for two paths; returned GUIDs from the mock `url`.
  - `no_delete_requests`: rerun the existing DELETE scan in `tests/suite/ado.rs` (it already covers new files under `src/ado/`; confirm it scans `attachments.rs`).
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite ado_step_attachments::` - FAIL.
- [ ] **Step 3: Implement** the functions above.
- [ ] **Step 4: Run** the module, then `cargo test --test suite ado::` - PASS.
- [ ] **Step 5: Commit** `feat(ado): read step attachments with their step, and upload step files`.

### Task 3: Ops in the create and update documents

**Files:**
- Modify: `src-tauri/src/ado/endpoints.rs:559-623` (`create_test_case_doc`), `:789-866` (`update_test_case_doc`), `:1889-1920` (`steps_patch` stays; add `final_step_ids`)
- Modify: `src-tauri/src/commands/cases.rs:79-112` (`update_test_case` command) and `:20-55` (loaders call `load_step_attachments`)
- Test: `src-tauri/tests/suite/step_attachments.rs` (pure doc tests), `src-tauri/tests/suite/ado_step_attachments.rs` (command-level wiremock)

**Interfaces:**
- Consumes: `relation_ops`, `upload_step_files`, `case_relations`, `load_step_attachments`.
- Produces:
  - `pub fn final_step_ids(steps: &[Step], original_xml: Option<&str>) -> Vec<String>` = `parse_step_ids` of `steps_patch(..)` or else the original (create: of `build_steps_xml`).
  - `create_test_case_doc(.., attach: &AttachmentInput)` and `update_test_case_doc(.., attach: &AttachmentInput)` append `relation_ops(..).ops`; where `pub struct AttachmentInput<'a> { pub current: Option<&'a CaseRelations>, pub uploaded: &'a [Uploaded], pub skip: bool }` with `AttachmentInput::NONE` for callers that do no attachment work. If `final_ids.len() != steps.len()`, no ops and a note.
  - Both docs now return `(Vec<serde_json::Value>, Vec<String> /*notes*/)`; update the call sites (`commands/queue.rs:1183,1205`, `update_test_case_from_model`, and any test in `tests/suite/` that calls them).
  - `update_test_case` command: when `has_attachment_work(&tc)`, reads `case_relations` for the id, uploads files, passes them in; returns `Result<Vec<String> /*notes*/, String>`.
  - `get_pbi_test_cases_full` / `get_test_cases_by_ids` commands in `commands/cases.rs` call `load_step_attachments` before returning (the shared `endpoints.rs` functions do NOT, so Auto Run and Playwright export make no extra request).

- [ ] **Step 1: Write the failing tests:**
  - `create_doc_links_new_file_to_built_step_id`: 2 steps, second has an uploaded file -> doc contains `"[TestStep=3]:"`.
  - `unchanged_steps_use_original_ids`: original XML ids `["2","5"]`, steps unchanged, carried relation says 5 -> no op.
  - `update_doc_without_attachment_input_is_byte_identical_to_2_4_2` (compare against the doc built with `AttachmentInput::NONE` for a fixture case).
  - `update_command_rev_mismatch_message_has_no_url`: wiremock PATCH answers 412/400 with a `test` failure -> error text says the case changed in Azure DevOps and contains no `http`.
- [ ] **Step 2: Run** `cargo test --test suite step_attachments:: ado_step_attachments::` - FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the two modules, then `cargo test --tests` - PASS.
- [ ] **Step 5: Commit** `feat(cases): saving a case writes its step attachments`.

### Task 4: describe_step_files command and bindings

**Files:**
- Modify: `src-tauri/src/commands/cases.rs` (new command); `src-tauri/src/lib.rs` (register in the command list near line 172)
- Regenerate: `src/bindings.ts`
- Test: `src-tauri/tests/suite/step_attachments.rs`

**Interfaces:**
- Produces: `#[tauri::command] #[specta::specta] pub fn describe_step_files(paths: Vec<String>) -> Vec<StepFileCheck>` with `pub struct StepFileCheck { pub path: String, pub name: String, pub size: Option<u64>, pub refused: Option<String> }` (metadata only, no read; same rules as `read_step_file`). TS: `commands.describeStepFiles`, `Step.attachments?: StepAttachment[]`, `TestCase.removed_attachments?: string[]`, `TestCaseFull.attachments_read: boolean`.

- [ ] **Step 1: Failing test** `describe_refuses_folder_missing_and_big`: a dir, a missing path, a 26 MB temp file -> each `refused.is_some()`; a small file -> `size==Some(n)`, `name` is the file name.
- [ ] **Step 2: Run** - FAIL. **Step 3: Implement.** **Step 4: Run** the module, then `cd src-tauri && cargo test --test bindings`, then `npx tsc --noEmit` - PASS; fix any TS errors from the new required `attachments_read`.
- [ ] **Step 5: Commit** `feat(cases): check picked step files; regenerate bindings`.

### Task 5: Queue upload

**Files:**
- Modify: `src-tauri/src/commands/queue.rs:22-32` (`SubmitItemResult` gains `#[serde(default)] pub attached: Vec<AttachedFile>`), `:715-1158` (`submit_queue`), `:1162-1230` (`queue_item_request` takes `&AttachmentInput`), near `:1234` (new `note_attachments_not_saved`)
- Test: `src-tauri/tests/suite/submit_attachments.rs` (new), `src-tauri/tests/suite/main.rs`; check `tests/suite/submit_mapping.rs` still passes

**Interfaces:**
- Consumes: Tasks 1-3.
- Produces: `pub struct AttachedFile { pub step: u32 /*0-based*/, pub path: String, pub id: String, pub name: String }` (serde + specta), filled for rows whose action is `created`/`updated`.
- Flow: before the chunk loop, one `case_relations` read for update rows with `has_attachment_work`; failure -> those rows go with `AttachmentInput{skip:true}` and `note_attachments_not_saved` adds the row note. Inside the loop, per row with path attachments: `upload_step_files` first; an `Err` makes that row `failed_item(i, tc, sentence)` and it is not sent.

- [ ] **Step 1: Failing tests** (wiremock `$batch` like `tests/suite/upload_timeout.rs` / `submit_mapping.rs`):
  - `missing_file_fails_row_before_sending`: row 0 has a missing path -> row 0 `failed`, its error names `login.png`, and the batch body has only row 1.
  - `old_queue_update_sends_no_relation_ops`: update row without attachment keys -> no `$expand=relations` request and the batch body has no `/relations/`.
  - `created_row_reports_attached`: one new file on step 2 -> result `attached == [{step:1, id:<mock guid>, ..}]`.
  - `relations_read_failure_notes_row`: relations GET 500 -> row `updated`, error/notice text says its attachments were not changed, no `http` in it.
- [ ] **Step 2: Run** `cargo test --test suite submit_attachments::` - FAIL. **Step 3: Implement.** **Step 4: Run** module, then `cargo test --tests` - PASS.
- [ ] **Step 5: Regenerate bindings** (`cargo test --test bindings`), `npx tsc --noEmit`, commit `feat(queue): upload step attachments with the batch`.

### Task 6: Import, export, transform ops, validation and the assistant surface

**Files:**
- Modify: `src-tauri/src/import_parser/mod.rs:221-300` (step reader accepts `attachments`; case reader accepts `remove_attachments`), `src-tauri/src/import_parser/export.rs:17-60,62-100` (`AI_INSTRUCTIONS`, `step_json`, `case_record`)
- Modify: `src-tauri/src/transform.rs:20-140` (`attach_file`, `remove_attachment`; `SUPPORTED_OPS: [&str; 27]`; `ops_reference`), `:500-560,1040-1100` (split/remove keep attachments), `src-tauri/src/optimize.rs` (any step rebuild keeps attachments)
- Modify: `src-tauri/src/ai_bridge.rs:14620-14640` (writing guide Format), `:14880-14930` (`get_test_cases` read calls `load_step_attachments`), the `validate_cases` handler (attachment warnings)
- Test: `src-tauri/tests/suite/import_parser.rs`, `transform.rs`, `optimize.rs`, `ai_bridge.rs`, `mcp.rs` (op list in the tool description)

**Interfaces:**
- Consumes: Task 1 types, `is_plain_local_path`, `STEP_ATTACHMENT_MAX_BYTES`, `load_step_attachments`.
- Produces: JSON shapes `steps[].attachments[] = {path} | {id, name, size?, comment?}`, `remove_attachments: [guid]`; ops `attach_file {path, step, where}`, `remove_attachment {name, step?, where}`.

- [ ] **Step 1: Failing tests:**
  - `import_parser::old_file_loads_identically` - an existing fixture without the keys parses to the same `TestCase` list as before (no attachments, no removed).
  - `import_parser::path_attachment_and_bad_entries` - `{path}` kept with `name` from the file name; `{}` dropped with a warning; an attachment on `{"shared":N}` dropped with a warning; a non-local path warns.
  - `import_parser::export_round_trip_keeps_attachments` - export then import equals input.
  - `transform::attach_file_adds_to_step_and_remove_attachment_moves_existing_to_removed`.
  - `transform::split_step_keeps_attachments_on_first_part` and `remove_step_matching_moves_existing_ids_to_removed`.
  - `optimize::attachments_survive_optimize` - a case with an attached step comes back with the same attachment on the step with the same text.
  - `mcp::transform_ops_listed` - tool description contains `attach_file` and `remove_attachment`.
- [ ] **Step 2: Run** each module (one command at a time) - FAIL. **Step 3: Implement**, including the writing-guide lines (end-user plain words: absolute local path, 25 MB, never on a shared step, keep `id` entries as exported). **Step 4: Run** modules, then `cargo test --tests` - PASS.
- [ ] **Step 5: Commit** `feat(writing): step attachments in case files, transforms and the assistant guide`.

### Task 7: Step editor UI

**Files:**
- Modify: `src/components/StepsEditor.tsx` (chips + Attach), `src/components/QueueCaseEditor.tsx:110`, `src/screens/ManualEntry.tsx:161`, `src/screens/ExistingCases/CaseEditor.tsx:76-96,165` (pass the save notes to a toast; show "Attachments could not be read" when `!original.attachments_read`)
- Create: `src/lib/stepAttachments.ts`, `src/lib/stepAttachments.test.ts`
- Test: `src/components/StepsEditor.test.tsx`

**Interfaces:**
- Consumes: `commands.describeStepFiles`, `IconAttach`, `IconRemove`, `open` from `@tauri-apps/plugin-dialog`.
- Produces: `export function withSteps(tc: TestCase, steps: Step[]): TestCase` (adds GUIDs of existing attachments no longer on any step to `removed_attachments`, de-duplicated) - every host's `onChange` goes through it; ManualEntry has no existing attachments and may use it too. `StepsEditor` props unchanged.

- [ ] **Step 1: Failing tests:**
  - `stepAttachments.test.ts`: `withSteps` after deleting a step holding `{id:"g1"}` -> `removed_attachments == ["g1"]`; after removing a `{path}` chip -> unchanged list; reorder -> unchanged list.
  - `StepsEditor.test.tsx`: `Attach` on step 2 with mocked `open` -> `["C:\\a.png"]` and mocked `describeStepFiles` -> `onChange` gets step 2 with `{path,name:"a.png",size}`; a refused file shows its sentence and adds nothing; button `Remove a.png from step 2` removes it; a shared row has no `Attach` button.
- [ ] **Step 2: Run** `npx vitest run src/lib/stepAttachments.test.ts src/components/StepsEditor.test.tsx` - FAIL. **Step 3: Implement** with tokens only. **Step 4: Run** those, then `npx vitest run src/ui-consistency.test.ts`, `npx tsc --noEmit` - PASS.
- [ ] **Step 5: Commit** `feat(ui): attach, list and remove files on a step`.

### Task 8: Review diff and post-upload stamping

**Files:**
- Modify: `src/lib/caseDiff.ts:45-60,85-240` (attachment change counts; a diff line), `src/lib/queueUploaded.ts:77` (`keepUploaded` swaps `path` for `id` from `attached`), `src/lib/queueStamp.ts:40` (stamp the watched file the same way), `src/lib/fileSync.ts:62-130` (`changedSteps` ignores attachment-only differences in content keys; `caseKey`/`keysFor` unchanged)
- Test: `src/lib/caseDiff.test.ts`, `src/lib/queueUploaded.test.ts`, `src/lib/queueStamp.test.ts`

**Interfaces:**
- Consumes: `SubmitItemResult.attached` (`AttachedFile{step,path,id,name}`), `TestCaseFull.steps[].attachments`.
- Produces: `diffCase` reports `attachments: { added: string[]; removed: string[]; moved: string[] }` and `diffSummary` mentions it; a row with any of them is changed.

- [ ] **Step 1: Failing tests:** `attachment_only_change_is_a_change` (caseDiff: same text, one new path -> not unchanged; summary contains `a.png`); `existing_attachment_on_other_step_is_moved`; `second_upload_carries_ids_not_paths` (keepUploaded: result `attached` turns step 1's `{path}` into `{id,name}`); stamping writes the same into the file slice.
- [ ] **Step 2: Run** `npx vitest run src/lib/caseDiff.test.ts src/lib/queueUploaded.test.ts src/lib/queueStamp.test.ts` - FAIL. **Step 3: Implement.** **Step 4: Run** them, then the full `npm test` (App.test.tsx has a known load flake: one failure that passes on re-run is that) and `npx tsc --noEmit` - PASS.
- [ ] **Step 5: Hand check** in `npm run tauri dev` against a scratch PBI (owner's org): attach two files to a new case's step 2, upload, open the case in ADO and see them on step 2; reorder steps in Update Test Cases, upload, see them still on that step; remove one, upload, see it gone; upload the same queue again, see no duplicates. Record the outcome in the commit message body.
- [ ] **Step 6: Commit** `feat(queue): attachment changes show in review and are stamped after upload`.

### Task 9: How To Use guide, changelog, version

**Files:**
- Modify: `docs-site/src/content/manual-entry.ts`, `docs-site/src/content/update-test-cases.ts`, `docs-site/src/content/import-file.ts`, `docs-site/src/content/ai-tools.ts`; rebuilt `src-tauri/help/`
- Modify: `src/lib/changelog.ts` (new 2.4.3 entry first), `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` (version 2.4.3)

- [ ] **Step 1:** Add a short "Attach files to a step" passage to each page above (Attach button, 25 MB per file, Shared Steps cannot carry them, removing one removes it from the case on upload, case files: `{ "path": ... }`). No new screenshots unless a shot id's accessible names changed; if `npm run docs:shots -- --dry-run` reports a changed shot, follow CLAUDE.md's docs flow (`docs:dev` signed out, `docs:shots --only <id>`).
- [ ] **Step 2:** Changelog 2.4.3 items, end-user wording, e.g. "You can attach files to a test case step: use Attach on the step in Manual Entry, the queue or Update Test Cases. Each step lists its files, and removing one takes it off the test case when you upload." and "Assistants can attach a file to a step of a case they write or update." Bump the three version places to `2.4.3`.
- [ ] **Step 3: Run** `npm run docs:build`, then `npx vitest run docs-site src/lib/changelog.test.ts` - PASS (guard tests: built page fresh, no hidden feature named).
- [ ] **Step 4: Gate:** `cd src-tauri && cargo test --tests`, then `npm test`, then `npm run build` - PASS, one at a time.
- [ ] **Step 5: Commit** `docs: step attachments in How To Use; 2.4.3 changelog and version`. Do not release; the owner runs `scripts/release-v2.ps1 -Version 2.4.3`.
