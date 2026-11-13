# File bug required fields + Test Runner file handling Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Filing a bug from the runner works in orgs whose Bug type has required custom fields (asked for, remembered, and a rule rejection names the fields instead of failing); the Test Runner can download every file, lists all of the last result's attachments, attaches several files at once, and accepts dropped files.

**Architecture:** Rust reads the bug type's field definitions (GET only) into `ExtraField`s and sends the person's values in the create PATCH; a TF401320 body becomes `FileBugError::RuleViolations(Vec<FieldViolation>)` and nothing is created. Result attachments are listed with metadata (images also carry a thumbnail); a download is a GET streamed by Rust straight to the path the save dialog returned, and a session file is written by Rust from the b64 the runner already holds. The runner reads several picked or dropped paths through the existing `read_file_b64`.

**Tech Stack:** Tauri 2, Rust (serde, reqwest 0.13, tokio, tauri-specta rc.25, wiremock 0.6, tempfile), React 19 + TypeScript, TanStack Query, `@tauri-apps/plugin-dialog`, vitest + jsdom (`mockIPC`).

**Spec:** `docs/superpowers/specs/2026-11-13-bug-fields-runner-files-design.md`

## Global Constraints

- No HTTP DELETE to Azure DevOps: GET / POST / PATCH only. `tests/suite/ado.rs::client_source_has_no_delete_calls` scans every `.rs` under `src/` for the text `.delete(` / `Method::DELETE`. Remove a local `.part` file with `std::fs::remove_file`.
- Tokens live in Rust. Downloaded bytes go from ADO to disk in Rust and never cross IPC (thumbnails excepted, as today).
- User-facing errors name no URL and no path. Transport failures use the `ado/transport.rs` sentences through `network_error`; raw bodies go to the log (`applog`, `transport::said`). The per-field message shown to the person is scrubbed (no `://`, no `http`, capped).
- `src/bindings.ts` is generated: `cd src-tauri && cargo test --test bindings`. Never hand-edit it.
- Every Rust test lives in `src-tauri/tests/suite/`, one module per file with a `mod` line in `suite/main.rs`. One module: `cargo test --test suite <module>::`.
- A tauri-specta command takes at most 10 arguments (`AppHandle` counts); `file_bug` goes to 9.
- `src/ui-consistency.test.ts` must pass unweakened. Colours from tokens only. Icons from `src/lib/actionIcons.ts` (`IconDownload`, `IconAttach` exist), each `aria-hidden`, never hand-sized. Icon-only buttons carry `aria-label`. Small pills use `label-trim`.
- One cache implementation per side. Webview: `src/lib/cache.ts` - a new key goes in `cacheKeys`, shelf life from `CACHE`; never change an existing key string. No new Rust cache entries.
- Demo mode answers every AdoError command (`src/dev/demoCoverage.test.ts`): patch `src/dev/demo.ts` in the same task that adds the command.
- Guide: text changes in `docs-site/src/content/*`, then `npm run docs:build`, commit the rebuilt `src-tauri/help`. No new screenshots unless a documented accessible name changes.
- One build or test command at a time on this machine. One cargo cache (`src-tauri/target`); never set `CARGO_TARGET_DIR`.
- Commits: Bash heredoc `git commit -F - <<'EOF' ... EOF`, last line `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Confirm with `git log -1`. Never stage `.test-cases/`.

## Review Focus

1. **A rule rejection creates nothing and names every field.** ADO's body carries all 7 errors in `customProperties.RuleValidationErrors`; the top-level message names only the first. Tests: Task 1 `every_rule_violation_in_the_body_is_returned`, `a_rule_rejection_creates_nothing_and_returns_the_fields`, `a_message_carrying_a_url_is_replaced_by_the_flag_sentence`.
2. **A field value cannot steer the JSON-patch path or override Title/repro.** Task 1 `field_values_never_override_title_or_repro_and_odd_refs_are_dropped`.
3. **A failed fields lookup must not block filing** (RunReview and older tests mock no `bug_fields`). Task 2 "a failed field lookup still lets the bug be filed".
4. **A download that fails part-way leaves no half file at the destination**, and a 401 is a re-sign-in, not "saved". Task 3 `a_failed_download_leaves_no_file`, `a_download_is_streamed_to_the_chosen_path`, `an_expired_session_is_unauthorized_not_saved`.
5. **The drop hint never swallows clicks, and a drop goes to the case on screen at drop time.** jsdom cannot see covering, so a source tripwire plus the walk-through. Task 5 "the drop hint is pointer-events-none and only while dragging", "a drop after moving to the next case attaches to the new case".

---

### Task 1: Rust - bug field definitions, field values on create, typed rule-violation error

**Files:**
- Create: `src-tauri/src/ado/rule_errors.rs` (+ `pub mod rule_errors;` in `src-tauri/src/ado/mod.rs`)
- Modify: `src-tauri/src/work_board/layout.rs` (new `bug_field_definitions`), `src-tauri/src/work_board/mod.rs` (`ExtraField.kind` doc adds `"date"`; `pub const BUG_AUTO_FILLED`)
- Modify: `src-tauri/src/commands/bugs.rs` (`bug_fields`, `file_bug` signature, `file_bug_with`, `FileBugError`, `BugFieldValue`), `src-tauri/src/lib.rs` (register `bugs::bug_fields`)
- Modify: `src/bindings.ts` (regenerated), `src/dev/demo.ts` (`bugFields: () => ok({ wi_type: "Bug", fields: [] })`; `fileBug` keeps its shape)
- Modify (minimal, keeps tsc green): `src/components/BugDialog.tsx` - pass `[]` as `fieldValues`; replace `unwrapStr` with a local unwrap using `fileBugErrorText` (Task 2 builds the UI)
- Test: `src-tauri/tests/suite/bugs.rs` (extend; already has a `mod` line)

**Interfaces:**
```rust
// ado/rule_errors.rs
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct FieldViolation { pub reference_name: String, pub message: String }
/// Every field a TF401320 / RuleValidationException body names. Reads
/// customProperties.RuleValidationErrors[] (fieldReferenceName, fieldStatusFlags,
/// errorMessage); falls back to customProperties.FieldReferenceName/ErrorMessage.
/// message = ADO's errorMessage with "TFnnnnnn: " stripped, capped at 200 chars,
/// unless it contains "://" or "http" -> the sentence for the flags
/// ("Required"/"InvalidEmpty" -> "This field is required.";
///  "LimitedToValues"/"InvalidListValue" -> "Pick one of the allowed values.";
///  else "Azure DevOps did not accept this value."). Only plain dotted refs kept. Empty if not a rule error.
pub fn rule_violations(body: &str) -> Vec<FieldViolation>;

// work_board/mod.rs
/// Fields ADO fills on create, never asked for.
pub const BUG_AUTO_FILLED: &[&str] = &["System.Title","System.AreaPath","System.IterationPath",
  "System.State","System.Reason","System.CreatedBy","System.CreatedDate","System.ChangedBy",
  "System.ChangedDate","System.AuthorizedAs","System.AuthorizedDate","System.RevisedDate",
  "System.Id","System.Rev","System.TeamProject","System.WorkItemType","System.Watermark",
  "System.BoardColumn","System.BoardColumnDone","System.BoardLane","System.CommentCount","System.History"];

// work_board/layout.rs (impl AdoClient)
/// GET {org}/{project}/_apis/wit/workitemtypes/{type}/fields?$expand=all, keep
/// alwaysRequired && no defaultValue && not BUG_AUTO_FILLED && != repro_field,
/// plus every ref in `extra_refs` (rule-violation fields, shown even if not alwaysRequired).
/// Each kept field's type from GET {org}/{project}/_apis/wit/fields/{ref} (get_json_each):
/// allowedValues non-empty -> "pick"; dateTime -> "date"; html -> "html"; else "text".
/// A picklist type with no allowedValues falls back to get_field_allowed_values.
pub async fn bug_field_definitions(&self, org: &str, project: &str, wi_type: &str,
    repro_field: &str, extra_refs: &[String]) -> Result<Vec<ExtraField>, AdoError>;

// commands/bugs.rs
#[derive(serde::Serialize, specta::Type)]
pub struct BugFields { pub wi_type: String, pub fields: Vec<crate::work_board::ExtraField> }
#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
pub struct BugFieldValue { pub reference_name: String, pub value: String }
#[derive(Debug, serde::Serialize, specta::Type)]
#[serde(tag = "kind", content = "detail")]
pub enum FileBugError { RuleViolations(Vec<crate::ado::rule_errors::FieldViolation>), Ado(crate::ado::AdoError) }
impl From<crate::ado::AdoError> for FileBugError { /* Http 4xx with violations -> RuleViolations, else Ado */ }

#[tauri::command] #[specta::specta]
pub async fn bug_fields(app: tauri::AppHandle, organization: String, project: String,
    extra_refs: Vec<String>) -> Result<BugFields, crate::ado::AdoError>;
#[tauri::command] #[specta::specta]
pub async fn file_bug(app: tauri::AppHandle, organization: String, project: String, title: String,
    repro_text: String, test_case_id: i32, pbi_id: i32, screenshots_b64: Vec<String>,
    field_values: Vec<BugFieldValue>) -> Result<work_bug::FiledBug, FileBugError>;
/// The body of file_bug, testable against wiremock. Drops blank values, the Title and
/// repro refs, and any ref not matching ^[A-Za-z][A-Za-z0-9_]*(\.[A-Za-z0-9_]+)+$.
pub async fn file_bug_with(client: &crate::ado::AdoClient, organization: &str, project: &str,
    title: &str, repro_text: &str, test_case_id: i32, pbi_id: i32, screenshots_b64: &[String],
    field_values: &[BugFieldValue]) -> Result<work_bug::FiledBug, FileBugError>;
```
```ts
// src/components/BugDialog.tsx (exported for tests)
export function fileBugErrorText(e: FileBugError): string; // Ado -> describeAdoError; RuleViolations -> "Azure DevOps needs: <refs>"; unknown shape (dev fault injector) -> String(e)
```

- [ ] **Step 1: Write the failing tests** in `tests/suite/bugs.rs` (wiremock + `AdoClient::with_base_urls("tok".into(), uri, uri, ...)` as in `ado_testplan.rs`):
  - `every_rule_violation_in_the_body_is_returned`: a body with 7 `RuleValidationErrors` -> 7 `FieldViolation`s in order, first `Custom.BugType`, message not starting with `TF401320`.
  - `a_message_carrying_a_url_is_replaced_by_the_flag_sentence`: errorMessage `"... See https://dev.azure.com/x?secret=1"`, flags `"required, invalidEmpty"` -> `"This field is required."`, no `://`.
  - `a_body_that_is_not_a_rule_error_has_no_violations`: `{"message":"VS402323: ..."}`, non-JSON, `""` -> `[]`.
  - `required_fields_skip_what_ado_fills_and_fields_with_defaults`: type-fields mock with System.Title, System.AreaPath (required), Microsoft.VSTS.TCM.ReproSteps (required), Microsoft.VSTS.Common.Severity (required, defaultValue "3 - Medium"), Custom.BugType (required, allowedValues ["UI","API"]), Custom.FoundOn (required) + `fields/Custom.FoundOn` mock type `dateTime` -> exactly `[Custom.BugType kind "pick" allowed [UI,API], Custom.FoundOn kind "date"]`.
  - `extra_refs_are_returned_even_when_not_always_required`: `extra_refs = ["Custom.RootCause"]` (not alwaysRequired) included, kind `"text"`.
  - `field_values_go_in_the_create_patch`: wiremock matcher on POST `/_apis/wit/workitems/$Bug` asserts the body has `{"op":"add","path":"/fields/Custom.BugType","value":"UI"}`.
  - `field_values_never_override_title_or_repro_and_odd_refs_are_dropped`: values for `System.Title`, `Microsoft.VSTS.TCM.ReproSteps`, `Custom.X/../relations`, `""`-valued `Custom.Y` do not appear; exactly one `/fields/System.Title` op carrying the dialog's title.
  - `a_rule_rejection_creates_nothing_and_returns_the_fields`: POST mock 400 with the TF401320 body -> `Err(FileBugError::RuleViolations(v))`, `v.len() == 7`; attachment upload mock `.expect(0)`.
  - `other_failures_stay_ado_errors`: 403 -> `Err(FileBugError::Ado(AdoError::Forbidden))`.
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite bugs::` - Expected: FAIL (unresolved items).
- [ ] **Step 3: Implement** as pinned. `file_bug` = token, client, `detect_bug_type`, `file_bug_with`; create then screenshots exactly as today. `bug_fields` = detect_bug_type then `bug_field_definitions(&info.wi_type, &info.repro_field, &extra_refs)`. Build URLs with `percent_encode_segment` / `urlencoding::encode` like the neighbouring code. The raw 400 body is logged only through the existing `handle_json` path.
- [ ] **Step 4: Run** `cd src-tauri && cargo test --test suite bugs::`, then `cargo test --test suite ado::` (DELETE tripwire), then `cargo test --test bindings`, then from root `npx tsc --noEmit` and `npx vitest run src/components/BugDialog.test.tsx src/dev/demoCoverage.test.ts src/screens/AutoRun/RunReview.test.tsx` - Expected: PASS (existing BugDialog tests asserting `filed.screenshotsB64` still pass; `filed.fieldValues` is `[]`).
- [ ] **Step 5: Commit** `feat(v2): file bug sends required fields and names rule violations` (heredoc + trailer).

### Task 2: BugDialog - required fields, remembered values, violations highlighted

**Files:**
- Modify: `src/components/BugDialog.tsx`, `src/lib/cache.ts` (`cacheKeys.bugFieldValues`)
- Test: `src/components/BugDialog.test.tsx`

**Interfaces:**
```ts
// src/lib/cache.ts
bugFieldValues: (org: string, project: string) => `bug-fields:${org}/${project}`, // shelf: CACHE.picks
// src/components/BugDialog.tsx
export function toAdoDate(ymd: string): string;          // "2026-11-13" -> new Date(2026,10,13).toISOString()
export function missingRequired(fields: ExtraField[], values: Record<string, string>): string[]; // refs with blank value
// state: values: Record<string,string> (seeded from cacheRead(cacheKeys.bugFieldValues(org, project), CACHE.picks.ttlMs)),
//        violations: Record<string,string> (ref -> message), extraRefs: string[]
// query: useQuery({ queryKey: ["bug-fields", org, project, extraRefs], queryFn: () => unwrap(commands.bugFields(org, project, extraRefs)), staleTime: 30 * 60_000, retry: false })
```
Field rendering under Repro steps, in returned order: `pick` -> `Select` (`src/components/ui/select.tsx`) with `aria-label={field.label}`; `date` -> `DateField` with `ariaLabel={field.label}` and its ADO-error ring prop when violated; `text` -> `Input`; `html` -> `Textarea`. A violated field shows its message in `text-danger text-xs` under it and the control gets the danger ring. File stays disabled while `missingRequired(...)` is non-empty, with `title="Fill in: <labels>"`. The paragraph `Links to test case #N and PBI #M... Paste (Ctrl+V) to add more.` keeps its exact text (the guide locates it). On `RuleViolations`: set `violations`, add refs not in the current fields to `extraRefs` (refetch shows them), toast.error `"Azure DevOps needs more fields - fill in the highlighted ones and file again."`, dialog stays open. On success: `cacheWrite(cacheKeys.bugFieldValues(org, project), values)`. Date values go through `toAdoDate` when sent. While the query loads, File is disabled. If the query errors or returns undefined, there are no extra fields and File follows the title rule only.

- [ ] **Step 1: Write the failing tests** (`mockIPC` answering `bug_fields` and `file_bug`; `clearCacheForTests`/`cacheWrite` from `../lib/cache` as cache.test.ts does):
  - "required fields render as dropdown, date and text, and File waits for them": fields `[pick Custom.BugType [UI,API], date Custom.FoundOn, text Custom.Build]`; File disabled; after choosing `UI`, picking a date, typing `1.2` -> enabled; `filed.fieldValues` equals the three refs, date value matches `/^\d{4}-\d{2}-\d{2}T/`.
  - "values are remembered per org and project": after a successful file, a re-render for the same org+project prefills `UI`; a different project does not.
  - "a rule rejection highlights the named fields, fetching ones not shown": first `file_bug` throws `{kind:"RuleViolations", detail:[{reference_name:"Custom.RootCause", message:"This field is required."}]}`; the next `bug_fields` receives `extraRefs: ["Custom.RootCause"]`, the field appears with the message, `onFiled` not called, dialog stays open.
  - "a failed field lookup still lets the bug be filed": `bug_fields` throws `{kind:"Forbidden"}` -> File enabled with a title only, `file_bug` gets `fieldValues: []`.
  - "an Ado error toasts the readable sentence": `file_bug` throws `{kind:"Ado", detail:{kind:"Http", detail:{status:400, body:'{"message":"VS1: nope"}'}}}` -> toast.error contains `VS1: nope`, no `http`.
  - `toAdoDate("2026-11-13")` round-trips to local 13 Nov; `missingRequired` ignores whitespace-only values.
- [ ] **Step 2: Run** `npx vitest run src/components/BugDialog.test.tsx` - Expected: FAIL.
- [ ] **Step 3: Implement** as pinned.
- [ ] **Step 4: Run** `npx vitest run src/components/BugDialog.test.tsx src/lib/cache.test.ts src/screens/AutoRun/RunReview.test.tsx src/ui-consistency.test.ts`, then `npx tsc --noEmit` - Expected: PASS.
- [ ] **Step 5: Commit** `feat(v2): the bug window asks for the fields Azure DevOps requires` (heredoc + trailer).

### Task 3: Rust - list result attachments, stream a download to a file, save a session file

**Files:**
- Modify: `src-tauri/src/ado_testplan/mod.rs` (`ResultAttachment`, `attachment_kind`), `src-tauri/src/ado_testplan/runs.rs` (private `list_result_attachments`, `get_result_attachments`, `download_result_attachment`; `get_result_screenshots` reuses the lister, behaviour unchanged)
- Modify: `src-tauri/src/ado/transport.rs` (`get_to_file`), `src-tauri/src/commands/runs.rs` (`result_attachments`, `save_result_attachment`), `src-tauri/src/commands/bugs.rs` (`save_attachment_b64`, `write_file_atomically`), `src-tauri/src/lib.rs` (register the three)
- Modify: `src/bindings.ts` (regenerated), `src/dev/demo.ts` (`resultAttachments: () => ok([])`, `saveResultAttachment: () => err("Demo mode: downloads are disabled")`)
- Create: `src-tauri/tests/suite/result_files.rs` + `mod result_files;` in `src-tauri/tests/suite/main.rs`

**Interfaces:**
```rust
// ado_testplan/mod.rs
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct ResultAttachment {
    pub id: i32, pub file_name: String,
    pub size_bytes: u32,          // ADO "size", saturating; u32 because specta refuses u64 over IPC
    pub kind: String,             // "image" | "video" | "file"
    pub thumb_b64: Option<String> // images only; None when the thumbnail could not be read
}
pub fn attachment_kind(file_name: &str) -> &'static str; // png/jpg/jpeg/gif/bmp/webp -> image; webm/mp4/mov/mkv/avi -> video

// ado/transport.rs (impl AdoClient)
/// GET streamed with resp.chunk() into "<dest>.part" (std::fs::File, write_all per chunk),
/// renamed onto dest on success; on any failure the .part is removed with std::fs::remove_file
/// and dest is untouched. Status mapping as get_bytes. A local I/O failure is
/// AdoError::Http { status: 0, body: "Could not save the file: <io::Error>" } (no path).
pub(crate) async fn get_to_file(&self, url: String, dest: &std::path::Path,
    deadline: std::time::Duration) -> Result<u64, AdoError>;

// ado_testplan/runs.rs (impl AdoClient)
pub async fn get_result_attachments(&self, org: &str, project: &str, run_id: i32,
    result_id: i32) -> Result<Vec<ResultAttachment>, AdoError>; // listing failure = Err; thumbnail 401/429 stops thumbnails, list still returned
pub async fn download_result_attachment(&self, org: &str, project: &str, run_id: i32,
    result_id: i32, attachment_id: i32, size_bytes: u32, dest: &std::path::Path)
    -> Result<u64, AdoError>; // deadline = crate::ado::upload_timeout(size_bytes as usize)

// commands/runs.rs
#[tauri::command] #[specta::specta]
pub async fn result_attachments(app: tauri::AppHandle, organization: String, project: String,
    run_id: i32, result_id: i32) -> Result<Vec<ado_testplan::ResultAttachment>, ado::AdoError>;
#[tauri::command] #[specta::specta]
pub async fn save_result_attachment(app: tauri::AppHandle, organization: String, project: String,
    run_id: i32, result_id: i32, attachment_id: i32, size_bytes: u32, dest_path: String)
    -> Result<(), ado::AdoError>;

// commands/bugs.rs
pub fn write_file_atomically(dest: &std::path::Path, bytes: &[u8]) -> std::io::Result<()>; // .part + rename
#[tauri::command(async)] #[specta::specta]
pub fn save_attachment_b64(dest_path: String, b64: String) -> Result<(), String>; // "Could not save the file: ..." / "The file data is damaged."
```

- [ ] **Step 1: Write the failing tests** in `tests/suite/result_files.rs` (wiremock, `tempfile::tempdir()`):
  - `attachments_are_listed_with_size_and_kind_and_images_get_thumbnails`: list mock with `shot.png` (69), `rec.webm` (5000000), `log.txt`, png bytes mock -> 3 entries kinds image/video/file, `size_bytes` 69/5000000, thumb only on the png, no request for the webm's bytes (`.expect(0)`).
  - `a_thumbnail_that_fails_still_lists_the_file`: png bytes 500 (after retries) -> entry present, `thumb_b64 == None`.
  - `a_download_is_streamed_to_the_chosen_path`: 300 KB body -> `dest` holds identical bytes, length returned, no `.part` left.
  - `a_failed_download_leaves_no_file`: 404 -> `Err(AdoError::NotFound)`, neither `dest` nor `dest.part` exists; a pre-existing `dest` keeps its old bytes.
  - `an_expired_session_is_unauthorized_not_saved`: 401 -> `Err(AdoError::Unauthorized)`.
  - `a_download_error_names_no_url`: unreachable base -> `AdoError::Network(s)` with `!s.contains("://")`.
  - `a_session_file_is_written_by_rust`: `save_attachment_b64(path, b64("abc"))` writes `abc`; bad base64 -> `Err` without `://`, nothing written.
  - `screenshots_for_the_preview_are_unchanged`: `get_result_screenshots` on the same mocks still returns only image b64s.
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite result_files::` - Expected: FAIL.
- [ ] **Step 3: Implement** as pinned. Use `self.send(GET, ..., |r| r.timeout(deadline).header("Accept","application/octet-stream"))`; the read retry in `send_as` applies as for any GET. No `.delete(` text anywhere.
- [ ] **Step 4: Run** `cd src-tauri && cargo test --test suite result_files::`, then `cargo test --test suite ado::`, `cargo test --test suite ado_network::`, `cargo test --test suite ado_testplan::`, then `cargo test --test bindings`, then `npx tsc --noEmit` and `npx vitest run src/dev/demoCoverage.test.ts` - Expected: PASS.
- [ ] **Step 5: Commit** `feat(v2): list every result attachment and download files through Rust` (heredoc + trailer).

### Task 4: Runner - Download on every file, full "Previously uploaded" list

**Files:**
- Create: `src/lib/runnerFiles.ts`, `src/lib/runnerFiles.test.ts`
- Modify: `src/screens/RunnerWindow.tsx` (query ~374, chips ~1177-1242)
- Test: `src/screens/RunnerWindow.test.tsx` (mocks `result_screenshots` -> `result_attachments`; add `vi.mock("@tauri-apps/plugin-dialog")`)

**Interfaces:**
```ts
// src/lib/runnerFiles.ts
/** Save dialog seeded from the last export folder (lib/exportDir); remembers the folder. null = cancelled. */
export async function pickSavePath(fileName: string): Promise<string | null>;
/** Session file: b64 -> Rust writes it. true when saved. */
export async function downloadSessionFile(att: RunAttachment): Promise<boolean>;
/** Uploaded file: Rust streams it from Azure DevOps to the chosen path. true when saved. */
export async function downloadResultFile(org: string, project: string, runId: number,
  resultId: number, att: ResultAttachment): Promise<boolean>;
```
Both download helpers toast `Saved <file_name>` on success, `Could not save <file_name>: <reason>` on failure (`unwrap` / `unwrapStr`; reasons never carry a URL - Rust guarantees it). In RunnerWindow:
- `uploaded` becomes `useQuery({ queryKey: ["result-attachments", session?.org, session?.project, run, result], queryFn: () => unwrap(commands.resultAttachments(...)), enabled, staleTime: Infinity, retry: false })`.
- Lightbox `shots` come from the `thumb_b64` entries (mime via `imageMimeForName(file_name)`); alt stays `Uploaded screenshot N`.
- "Previously uploaded:" lists thumbnails as today, then one row per attachment: name (`label-trim`), `formatBytes(size_bytes)` (`src/lib/bytes.ts`), kind word ("Picture"/"Video"/"File"), and `<Button variant="ghost" size="sm" aria-label={`Download ${a.file_name}`}><IconDownload aria-hidden /></Button>`.
- Each session image gets a Download button beside its remove x, and each non-image chip an icon button, both `aria-label={`Download ${a.file_name}`}`.

- [ ] **Step 1: Write the failing tests:**
  - runnerFiles: "cancelling the save dialog writes nothing" (`save` resolves null -> `save_attachment_b64` never invoked); "a session file is saved where the person chose" (args `{destPath, b64}`); "an uploaded file is fetched by Rust, not read into the webview" (`save_result_attachment` receives `attachmentId`, `sizeBytes`, `destPath`; response never base64 decoded); "a failure toasts the reason".
  - RunnerWindow: "previously uploaded lists every file with size and Download" (`result_attachments` returns png + webm + txt -> 3 `Download ...` buttons, `4.8 MB` text for 5000000, one thumbnail); "a session recording can be downloaded" (after a `read_file_b64` attach of `rec.webm`, click `Download rec.webm` -> `save_attachment_b64` invoked).
- [ ] **Step 2: Run** `npx vitest run src/lib/runnerFiles.test.ts src/screens/RunnerWindow.test.tsx` - Expected: FAIL.
- [ ] **Step 3: Implement** as pinned.
- [ ] **Step 4: Run** `npx vitest run src/lib/runnerFiles.test.ts src/screens/RunnerWindow.test.tsx src/ui-consistency.test.ts`, then `npx tsc --noEmit` - Expected: PASS.
- [ ] **Step 5: Commit** `feat(v2): every file in the Test Runner can be downloaded` (heredoc + trailer).

### Task 5: Runner - attach several files, drop files on the window

**Files:**
- Modify: `src/screens/RunnerWindow.tsx` (`attachFile` ~649, new drop effect, root `relative`, hint overlay), `src/lib/runnerFiles.ts` (`readFiles`), `src/lib/openRunner.ts` (`dragDropEnabled: true` with a comment: the runner has no HTML5 drag targets; the main window keeps it off)
- Test: `src/lib/runnerFiles.test.ts`, `src/screens/RunnerWindow.test.tsx` (`windowMock.onDragDropEvent: vi.fn((cb) => { dropHandler = cb; return Promise.resolve(() => {}); })`)

**Interfaces:**
```ts
// src/lib/runnerFiles.ts
export type ReadReport = { read: RunAttachment[]; failed: { name: string; why: string }[] };
/** Reads each path through read_file_b64, one after another; one failure never stops the rest. */
export async function readFiles(paths: string[]): Promise<ReadReport>;
// RunnerWindow
async function attachPaths(caseId: number, paths: string[]): Promise<void>; // addAttachment per read file;
//   toast.success("Attached N file(s)"); toast.error("Could not attach a.txt (File is larger than 25 MB.), b.bin (...)")
// attachFile: const picked = await open({ multiple: true, title: "Attach files to this result" }); string[] | null
// drop: useEffect(() => getCurrentWindow().onDragDropEvent(e => ...), []) with currentIdRef (updated each render);
//   enter/over -> setDragOver(true); leave -> false; drop -> false, then currentIdRef.current == null
//   ? toast.info("Open a case to attach files to it.") : attachPaths(id, e.payload.paths).
//   The unlisten promise is resolved and called on unmount (same pattern as the order listener ~328).
// hint (only while dragOver): <div aria-hidden className="pointer-events-none absolute inset-0 z-40 flex items-center
//   justify-center rounded-md border-2 border-dashed border-accent bg-surface/95 text-sm font-semibold text-text">
//   Drop to attach to case #{id}</div>   (no `fixed`, no `right-`)
```

- [ ] **Step 1: Write the failing tests:**
  - runnerFiles: "every picked file is read and one failure does not stop the others" (3 paths, 2nd rejects `"File is larger than 25 MB."` -> `read.length === 2`, `failed[0].name === "big.bin"`).
  - RunnerWindow: "Attach file takes several files" (`open` resolves `["C:/a.txt","C:/b.png"]` -> 2 chips/thumbnails; `open` called with `multiple: true`); "a failed file is named and the rest still attach"; "dropping files attaches them to the case on screen" (`dropHandler({payload:{type:"drop", paths:[...]}})`); "a drop after moving to the next case attaches to the new case"; "a drop with no case on screen attaches nothing and says so" (empty list -> `toast.info`); "the drop hint is pointer-events-none and only while dragging" (absent at rest, present with class `pointer-events-none` after `type:"enter"`, gone after `leave`; a click on Paste still works while it shows).
- [ ] **Step 2: Run** `npx vitest run src/lib/runnerFiles.test.ts src/screens/RunnerWindow.test.tsx` - Expected: FAIL.
- [ ] **Step 3: Implement** as pinned. Capabilities need no change: the runner (label `runner`) is in `capabilities/default.json`; `core:default` allows the drag-drop event listen; `dialog:default` allows `open`/`save`. Confirm in `src-tauri/gen/schemas` if unsure.
- [ ] **Step 4: Run** `npx vitest run src/lib/runnerFiles.test.ts src/screens/RunnerWindow.test.tsx src/ui-consistency.test.ts`, then `npx tsc --noEmit`, then the full `npm test`, then `cd src-tauri && cargo test --tests` (one at a time) - Expected: PASS. Hand walk-through (owner, live app): drop 3 files (one over 25 MB) on the runner - hint shows, buttons under it still click; download a session recording and an uploaded file; file a bug in an org with required fields.
- [ ] **Step 5: Commit** `feat(v2): attach several files and drop files onto the Test Runner` (heredoc + trailer).

### Task 6: How To Use guide

**Files:**
- Modify: `docs-site/src/content/run-tests.ts` (entries `attach-file`, `bug-links`, `file-bug` and the page `tips`)
- Modify (generated): `src-tauri/help/**`

Text (short active sentences, no em dashes, no URL):
- `attach-file.does`: "Attaches the files you pick to this case's result. You can pick several at once, or drop files anywhere on the runner."
- `attach-file.tips`: "Every attached file has a download button. Pictures show as thumbnails: click one to see it full size, or its x to remove it." and "Previously uploaded lists every file from the last result, with its size, and a download button for each."
- `bug-links.does`: append "If Azure DevOps needs more fields for a bug, the window asks for them above this line, and **File bug** waits until they are filled. Your answers are remembered for the next bug in this project."
- Page tip: "If Azure DevOps still refuses the bug, nothing is created: the fields it named are highlighted with what it said. Fill them in and file again."

No accessible name on a documented screen changes (`Attach file`, `File bug`, `Bug title`, `Repro steps`, the `bug-links` locate text `Links to test case #5001 and PBI #1001. Paste (Ctrl+V) to add more.` are unchanged; the new controls only show with data the sample session lacks), so no screenshots are owed.

- [ ] **Step 1:** Edit the entries as above.
- [ ] **Step 2: Run** `npm run docs:build`, then `npx vitest run docs-site/src/guard.test.ts` - Expected: PASS.
- [ ] **Step 3: Commit** `docs(v2): guide covers bug required fields and runner file handling` with `docs-site/src/content/run-tests.ts` and `src-tauri/help` together (heredoc + trailer).
