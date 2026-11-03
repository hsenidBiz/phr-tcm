# Step attachments - design (stable 2.4.3, built from v2.4.2)

## Intent

A tester can attach a file (a screenshot, a sample upload, a data sheet) to one step of a test
case in Test Case Manager, see each step's attachments, and remove one - and an assistant can do
the same through the case JSON it already writes. Attachments land in Azure DevOps exactly where
the web form puts them, so a tester running the case in ADO sees the file on that step.

## How Azure DevOps stores a step attachment

- The blob: `POST {org}/{project}/_apis/wit/attachments?fileName=<name>&api-version=7.1`, body
  `application/octet-stream` -> `{ id, url }`. TCM already does this
  (`AdoClient::upload_wi_attachment`, `src-tauri/src/ado/endpoints.rs:1348`).
- The link: a JSON-patch `add` on `/relations/-` of the Test Case:
  `{ rel: "AttachedFile", url, attributes: { name: <file name>, comment: "[TestStep=<id>]:<text>" } }`.
- `<id>` is the `id` attribute of that step's `<step>` in `Microsoft.VSTS.TCM.Steps`
  (TCM: `steps_xml.rs`, ids from 2). The web form writes `[TestStep=N]:` with nothing after the
  colon when no comment was typed; anything after the colon is the attachment's comment. This
  marker was not confirmed in public docs during design - Task 1's live check reads one real case
  with a step attachment added through the ADO web form and pins the exact bytes. The parser is
  lenient (`^\[TestStep=(\d+)\]:` then any text, whitespace tolerated); the writer writes exactly
  `[TestStep=N]:` + the kept comment text.
- Removing: JSON-patch `{ op: "remove", path: "/relations/<index>" }` in a PATCH, guarded by
  `{ op: "test", path: "/rev", value: <rev> }` first - the pattern `ado_share.rs` already uses.
  ADO has no attachment-delete API; an unreferenced blob is garbage-collected. **No HTTP DELETE.**
- Re-pointing: `{ op: "replace", path: "/relations/<index>/attributes/comment", value: "[TestStep=M]:..." }`.

## Goals

1. Step editor (Manual Entry, Queue case editor, Update/Existing case editor): Attach on each
   non-shared step opens a file picker (several files at once); each step lists its attachments
   (name, size); each can be removed.
2. Upload: new files are uploaded and linked to their step; existing ones are kept; removed ones
   are unlinked; an attachment whose step got a different id is re-pointed.
3. Attachments follow their step through add / remove / reorder / edit before upload.
4. JSON import/export carries step attachments; every existing file loads unchanged.
5. Assistants attach by local path through the JSON they already write, two new
   `transform_cases` ops, `validate_cases`, and `get_test_cases` (which shows existing ones).

## Non-goals

- Previewing, opening or downloading an attachment in TCM (names and sizes only).
- Attachments on Shared Steps (they live in the Shared Steps work item; refused on those rows).
- Case-level (non-step) attachments, editing an attachment's comment text, renaming.
- Showing attachments in the browser review page / draft HTML.
- Copying an existing attachment onto a NEW case (see Open question 1).
- Chunked upload for very large files.

## Approaches considered

**A. Attachments as data on each `Step` (chosen).** `Step` gains `attachments: Vec<StepAttachment>`
(absent from JSON/IPC when empty). The webview already moves `Step` objects around on reorder, so
an attachment travels with its step for free. The step id is decided only at upload: the final
Steps XML is computed (build or `merge_steps_xml`, unchanged), and `parse_step_ids(final_xml)`
gives the id of step *j* in document order, aligned with the draft's steps. Each attachment's
relation comment is then made to name that id. No step-id bookkeeping anywhere in the UI.

**B. Attachments keyed by step id on the case** (`{ step_id: [..] }`). Matches ADO's model, but a
draft step has no id until upload, new steps would need temporary ids, and every reorder/delete in
three editors plus every transform op would have to keep the map in sync. Rejected: many places
to get wrong.

**C. Attachments keyed by step index on the case.** Cheapest to add to the model, but every
reorder, insert, delete, split and optimizer pass must renumber it. Rejected for the same reason.

## Data model (Rust; TypeScript follows from the generated bindings)

`src-tauri/src/step_attachments.rs` (new):

```rust
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct StepAttachment {
    /// An attachment Azure DevOps already holds: its GUID (last segment of the relation URL).
    #[serde(default, skip_serializing_if = "Option::is_none")] pub id: Option<String>,
    /// A new file: a full local path on this machine, read only at upload.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub path: Option<String>,
    /// File name as shown (ADO's `name` attribute; for a new file, the path's file name).
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub size: Option<u64>,
    /// Text after `[TestStep=N]:`, kept verbatim when re-pointed.
    #[serde(default, skip_serializing_if = "String::is_empty")] pub comment: String,
}
```

- `steps_xml::Step` gains `#[serde(default, skip_serializing_if = "Vec::is_empty")] pub attachments: Vec<StepAttachment>`.
- `model::TestCase` gains `#[serde(default, skip_serializing_if = "Vec::is_empty")] pub removed_attachments: Vec<String>`
  (GUIDs the user removed - singly, or by deleting the step that held them).
- `ado::TestCaseFull` gains `pub attachments_read: bool` (false when the relations read failed, so
  the editor can say attachments could not be shown).
- Size cap: **25 MB per file** (`STEP_ATTACHMENT_MAX_BYTES = 25 * 1024 * 1024`), the cap TCM
  already uses for result files (`read_file_b64`) and `save_test_file`; below Azure DevOps
  Services' 60 MB attachment limit, and an on-prem server's lower limit still answers in its own
  words through the row error.

## The upload rules (one pure function, used by queue upload and the Update editor's save)

For a case, given the final step ids, its current `AttachedFile` relations (index, GUID, comment)
and `rev`, the draft's steps and `removed_attachments`, and the GUID/URL of each newly uploaded file:

1. **Carried existing** (step *j* lists GUID *g*): if the relation's marker names a different id
   than `final_ids[j]`, `replace` its comment. If *g* is not on the work item any more, nothing is
   written and the row says so.
2. **Removed** (GUID in `removed_attachments`, carried by no step): `remove` its relation.
   Carried-and-removed: carried wins (logged).
3. **New** (path): uploaded before the batch; `add` relation with `[TestStep=final_ids[j]]:`.
4. **Anything else** - a relation no step carries and nobody removed - is **left exactly as it is**.
   This is what keeps every old draft, old JSON file, and a draft whose attachments were never
   read, from touching attachments at all. `merge_steps_xml` already keeps an unchanged / moved /
   edited-in-place step's id, so their relations stay correct without a write.

Index-based ops (`replace`, `remove`) are emitted after a `test /rev` op, removes in descending
index order, so indexes never shift under a later op. If no index op is needed, no `test` op is
sent. A case with no attachment work produces no ops and costs no extra request.

## How step re-numbering is handled

- Creates: `build_steps_xml` numbers 2..; `parse_step_ids(build)` aligns with the steps.
- Updates: `steps_patch` (unchanged) returns the merged XML, a retyped XML, or `None` (unchanged
  -> the original). `parse_step_ids` of whichever is written gives the final ids. Rule 1 re-points
  only what actually moved id (e.g. a step edited AND moved across an anchor gets a fresh id).
- If `parse_step_ids(final).len() != steps.len()` (only possible when `steps_patch` refused a
  shared-step write), attachment work for that case is skipped with a row note.
- A deleted step: the editor moves the GUIDs of its existing attachments into
  `removed_attachments` (one helper, `withSteps(tc, steps)`, used by every editor host); a
  transform op that removes steps does the same in Rust.

## UI

- `StepsEditor.tsx`: under each non-shared step, a row of attachment chips (`IconAttach`, name,
  size, a remove button labelled "Remove <name> from step N") and a ghost `Attach` button
  (`IconAttach` + label). Picker: `@tauri-apps/plugin-dialog` `open({ multiple: true })`; the paths
  go to a new command `describe_step_files(paths) -> Vec<StepFileCheck>` which answers name,
  size, or a refusal sentence (missing, folder, not local, over 25 MB). Refused files show as a
  toast; accepted ones are added as `{ path, name, size }`.
- Shared-step rows: no Attach.
- `caseDiff.ts`: an update row counts as changed when it adds a file, removes one, or an existing
  attachment now sits on a different step; the diff line reads e.g. "Attachments: + login.png,
  − old.png". Without this an attachment-only edit would be skipped as unchanged.
- After upload, `SubmitItemResult.attached` lets the queue swap each uploaded path for its new
  GUID (and stamp the watched file the same way ids are stamped), so a second upload does not
  attach the file twice.
- Theming tokens only; icons from `actionIcons.ts`; `ui-consistency.test.ts` untouched.

## Assistant tool shape

No new MCP tool. The assistant already writes the case JSON, then calls `transform_cases`,
`validate_cases`, `get_test_cases`:

- JSON: a step may carry `"attachments": [{ "path": "C:\\shots\\login.png" }]` (new) or
  `[{ "id": "<guid>", "name": "login.png" }]` (existing, as exported). A case may carry
  `"remove_attachments": ["<guid>"]`.
- `transform_cases` ops (27 instead of 25): `attach_file { path, step, where }` (step is 1-based)
  and `remove_attachment { name, step?, where }` (an existing one moves to `remove_attachments`,
  a new one is dropped). `remove_step_matching`, `split_step`, `optimize_cases` keep attachments:
  a split step's attachments go to the first replacement step; a removed step's existing ones go
  to `remove_attachments`.
- `validate_cases` warns per bad attachment (not a plain local absolute path -
  `test_files::is_plain_local_path`; missing; over 25 MB; on a shared step; neither id nor path).
- `get_test_cases` returns existing attachments on each step; the writing guide's Format section
  documents both shapes.

## Import / export

- Export (`step_json`, `case_record`) writes `attachments` and `remove_attachments` only when
  non-empty; an existing-only round trip is byte-identical to today for cases without them.
- Import accepts both keys. Old files have neither -> empty -> rule 4 -> nothing touched.
  Path entries are checked on import as warnings (file may be created later); upload re-checks
  and fails only that row, before anything is sent.

## Edge cases

- File moved/deleted between pick and upload -> that row fails before sending, naming the file.
- Batch row fails after blobs uploaded -> blobs are unreferenced and collected by ADO; the row
  keeps its paths and a re-upload uploads again.
- Someone edited the case in ADO between read and upload -> `test /rev` fails the row: "This case
  changed in Azure DevOps after it was read. Upload it again." (no URL).
- Relations read fails -> the update goes without attachment changes and the row says so
  (mirrors `note_prereqs_not_saved`).
- Existing GUID on a NEW case (case copied, `id` nulled) -> not carried, row note (Q1).
- Same file attached to two steps -> uploaded twice, two relations (ADO allows it).
- Upload result "unknown" -> the row is held as today; its paths stay paths.

## Left to the builder

Exact chip layout and copy within the conventions; the exact row-note sentences (plain, no URL,
pointing at Settings -> Logs when logs help); how `keysFor`/queue stamping keys a row after the
path->GUID swap (keep content keys stable: exclude attachments from the key).

## Owner answers (2026-11-03)
1. A copied case does not re-link the original's step attachments; the copy says so.
2. Yes: read (GET only, nothing changed) one existing case that has a step attachment added in the ADO web form, found through the app, to confirm the [TestStep=N] comment format before Task 1 relies on it.
3. 25 MB per file.
