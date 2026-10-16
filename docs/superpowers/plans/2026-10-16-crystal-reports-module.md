# Crystal Reports Module Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Crystal Reports module that lets an AI assistant, through TCM's MCP server, open, read, edit, compare, check, create and preview `.rpt` files, change stored procedures/views/functions on the Dev login, and lets the person watch the PDF preview in TCM and open the file in Crystal Reports.

**Architecture:** The owner's re-implementation (`rpt-core` + vendored rpt-rs) is copied unchanged into `crystal-reports/` and used by path from `src-tauri`. A new `src-tauri/src/crystal/` module holds the one open report (a process-wide `Mutex`), the edit-save-reload loop, the compare routine and the preview. MCP tools forward to new AI Bridge routes under `/crystal-`; the DB guard gains an object-statement allowance; a new screen shows the preview.

**Tech Stack:** Rust (Tauri 2, tauri-specta, tokio), `rpt-core`/`rpt-db-mssql` from the bundle, React 19 + TypeScript, vitest.

**Spec:** `docs/superpowers/specs/2026-10-16-crystal-reports-module-design.md`

## Global Constraints

- The bundle copy is `Out` file for file minus `target\` and `work\`; **no file inside `crystal-reports/` is ever edited**. Refreshed only on the owner's word. The original `Out` folder is never modified.
- MPL-2.0: the copy's `vendor/LICENSE`, `vendor/THIRD-PARTY-NOTICES.md`, `vendor/VENDORED.md` and the font licence texts travel unchanged.
- One build cache: `src-tauri/target`. Never `CARGO_TARGET_DIR`, never build inside `crystal-reports/`.
- Rust tests only in `src-tauri/tests/suite/<module>.rs` with a `mod` line in `suite/main.rs`. `src/bindings.ts` is generated (`cargo test --test bindings`).
- No hardcoded colours; icons from `src/lib/actionIcons.ts`; ui-consistency gate never weakened.
- No connection string, password or host in a tool reply, log line or webview payload. User-facing errors name no URL.
- No HTTP DELETE; no `DROP` reaches any database.
- AI Bridge route paths must not contain `create`, `update`, `delete`, `edit`, `write`, `add-`, `remove`, `submit` (`smells_like_a_write`, ai_bridge.rs:461).
- Never name hidden features in copy, commits or docs.
- Tests that edit a sample report copy it into a `tempfile::tempdir()` first; the vendored samples are never written.

## Review Focus

1. A report path that does not exist, is not a `.rpt`, is locked by Crystal Reports, or is read-only on disk → a plain sentence naming the file, nothing half-written (Task 3 test `a_locked_or_missing_file_is_refused_in_a_sentence`).
2. An edit batch the bundle refuses → the file on disk is unchanged and the reply carries the refusal plus the IR re-read from disk (Task 3 test `a_refused_batch_leaves_the_file_and_reloads`).
3. A procedure body over 20 000 characters (real report procedures are long) → accepted up to 200 000 and sent to sqlcmd through a temp file, never the command line (Task 5 test `a_long_object_body_goes_through_a_file`).
4. A report whose data source is a SQL command or an unsupported table, or that names another database → the live preview refuses before connecting, naming the table (Task 6 test `a_report_naming_another_database_is_refused_before_connecting`).
5. The screen opened with no report, after the module is switched off, or after the open file was deleted outside TCM → it says so rather than showing a stale preview (Task 7 test `the_screen_says_when_there_is_nothing_to_show`).

---

### Task 1: Copy the bundle in and build against it

**Files:**
- Create: `crystal-reports/` (copy of `C:\Users\Admin\Desktop\Main Menu\Software\Personal Projects\CrystalReports ReImplementation\Out`, excluding `target\` and `work\`)
- Create: `docs/crystal-reports-copy.md` (where/when copied, the source `git rev-parse HEAD` if `Out` is a git repo — read-only command — and the refresh rule)
- Modify: `src-tauri/Cargo.toml` (deps), `.gitignore` (`crystal-reports/target/`)
- Create: `src-tauri/tests/suite/crystal_bundle.rs`; Modify: `src-tauri/tests/suite/main.rs`

**Interfaces:**
- Produces: crates `rpt_core` and `rpt_db_mssql` usable from `v2_lib`; fixture dir `crystal-reports/Sample Report/` (six `PMSV10_*.rpt`).

- [ ] **Step 1:** Copy with `robocopy "<Out>" "crystal-reports" /E /XD target work` (robocopy exit codes below 8 are success). Measure: `du -sh crystal-reports`. **If over 60 MB, stop and ask the owner** (do not trim).
- [ ] **Step 2:** Add to `src-tauri/Cargo.toml` `[dependencies]`: `rpt-core = { path = "../crystal-reports/rpt-core" }` and `rpt-db-mssql = { path = "../crystal-reports/rpt-db-mssql" }`. Run `cargo metadata --format-version 1 --manifest-path src-tauri/Cargo.toml >/dev/null` — expected success (the crates inherit from their own workspace root).
- [ ] **Step 3: Write the failing test** `the_vendored_bundle_opens_every_sample_report` in `crystal_bundle.rs`: for each `.rpt` in `crystal-reports/Sample Report` (path from `env!("CARGO_MANIFEST_DIR")/../crystal-reports/Sample Report`), `rpt_core::open(path)` is Ok and `build_ir(&rpt, &rpt_core::locate::locate(&rpt), path).objects` is non-empty; assert exactly 6 files were seen.
- [ ] **Step 4:** `cd src-tauri && cargo test --test suite crystal_bundle::` → PASS (first build is long).
- [ ] **Step 5:** `cargo test --test bindings` and confirm `git diff --stat src/bindings.ts` is empty. Commit (`chore(v2): Crystal Reports - copy of the rpt re-implementation, built by path`).

### Task 2: The Crystal Reports module

**Files:**
- Modify: `src-tauri/src/modules.rs` (enum `CrystalReports`, `ALL: [Module; 6]` with it last, `parent` → `None`, `name` "Crystal Reports", `key` "crystal_reports"; `initial`: on only in dev builds), `src/lib/modules.ts`, `src/screens/Modules/modulesCopy.ts`, `src/screens/Modules/ModuleTree.tsx` (`AT` slot on the board, `ICON`), `src/lib/newNav.ts`, `docs-site/src/modules.ts`
- Test: `src-tauri/tests/suite/modules.rs`, `src/lib/modules.test.ts`, `src/screens/Modules/index.test.tsx`, `src/lib/newNav.test.ts`, `docs-site/src/modules.test.ts`

**Interfaces:**
- Produces: `Module::CrystalReports`, key `"crystal_reports"`; `modules::on(Module::CrystalReports)`.

- [ ] **Step 1: Failing tests:** in `the_tree_has_the_designed_parents` add `assert_eq!(Module::CrystalReports.parent(), None)`; an initial-state test that a non-dev install starts with it off; frontend `MODULE_INFO.crystal_reports.name === "Crystal Reports"`.
- [ ] **Step 2:** Run `cargo test --test suite modules::` and `npx vitest run src/lib/modules src/screens/Modules` → FAIL.
- [ ] **Step 3:** Implement the variant at every site listed above. Description copy: "Lets your AI assistant open, format and preview Crystal Reports files, and change the stored procedures behind them on the Dev login." Sidebar/nav entry is added in Task 7; here `moduleNavItem` returns the `"crystal-reports"` section id.
- [ ] **Step 4:** Regenerate bindings; run the tests from Step 2 plus `npx tsc --noEmit` → PASS.
- [ ] **Step 5:** Commit.

### Task 3: The open report (Rust core)

**Files:**
- Create: `src-tauri/src/crystal/mod.rs` (state + operations), `src-tauri/src/crystal/compare.rs` (port of `rpt-cli` `diff_values`, crystal-reports/rpt-cli/src/main.rs:322-381, with an attribution comment), `src-tauri/src/crystal/guide.rs`
- Modify: `src-tauri/src/lib.rs` (`pub mod crystal;`)
- Test: `src-tauri/tests/suite/crystal_reports.rs`

**Interfaces:**
- Produces (all `Result<_, String>`, all in `crate::crystal`):
  - `pub fn open(path: &str) -> Result<serde_json::Value, String>` — opens, stores, returns the IR JSON.
  - `pub fn read(filter: &ReadFilter) -> Result<serde_json::Value, String>`; `pub struct ReadFilter { pub section: Option<String>, pub object: Option<String> }` — whole IR, or only the named section (its entry plus its objects) or one object by id or name.
  - `pub fn edit(ops: Option<serde_json::Value>, plan: Option<serde_json::Value>) -> Result<serde_json::Value, String>` — exactly one of the two; native ops deserialize to `Vec<rpt_core::edit::EditOp>`, a plan to `LayoutPlan` then `plan::lower`; `apply_edits_on`, then `std::fs::write` over the open path, then `open_bytes` + IR; returns the IR. On refusal: reload from disk and return `Err(format!("{refusal}\n\nThe report was read again from disk: nothing in this batch was saved."))`.
  - `pub fn new_report(path: &str, opts: rpt_core::blank::NewReportOptions) -> Result<serde_json::Value, String>` — refuses an existing file; writes `create_report` bytes; opens it.
  - `pub fn check(path: Option<&str>) -> Result<serde_json::Value, String>` — `validate::validate`; JSON `{ "errors": [{check, message}], "warnings": [...] }` built by hand (types are not Serialize).
  - `pub fn compare(a: &str, b: &str) -> Result<Vec<String>, String>` — `path: a -> b` lines.
  - `pub fn preview_saved() -> Result<PreviewInfo, String>`; `pub struct PreviewInfo { pub path: String, pub page_count: usize, pub diagnostics: Vec<String>, pub live: Option<String> /* connection name */ }`, Serialize + specta::Type. Stores the PDF bytes in state.
  - `pub fn last_pdf() -> Option<(PreviewInfo, Vec<u8>)>`; `pub fn current_path() -> Option<String>`; `pub fn close()`.
  - `pub fn guide() -> &'static str` (text in guide.rs).
- State: `static OPEN: Mutex<Option<Open>>` with `Open { path, rpt, ir, preview: Option<(PreviewInfo, Vec<u8>)> }`. After every successful `edit`/`open`/`new_report`, call `preview_saved()` internally so the screen always has a current render (its failure is kept as a diagnostic, never fails the edit).

- [ ] **Step 1: Failing tests** (`crystal_reports.rs`, each copies a sample into a tempdir; the module's state is process-wide, so every test takes the lock from `suite/serial.rs`):
  - `opening_returns_the_ir_and_reading_can_narrow_it` — `open` IR `format == "crpt-ir/1"`; `read(object: Some(<first object's name>))` has exactly that one object.
  - `an_edit_is_saved_to_disk_and_only_that_changes` — `set_text` (or `set_font` via a plan) on a named text object; reopen the file independently: `compare(original_copy, edited)` lines all mention that object's path and nothing else; returned IR shows the new value.
  - `a_refused_batch_leaves_the_file_and_reloads` — batch `[valid op, op on object "no-such"]`; file bytes identical to before; error contains "nothing in this batch was saved".
  - `a_locked_or_missing_file_is_refused_in_a_sentence` — missing path, a `.txt`, and a read-only copy (`set_readonly(true)`) for `edit`: each an `Err` naming the file name; read-only file unchanged.
  - `new_refuses_an_existing_file_and_opens_a_blank_one` / `check_reports_no_errors_on_a_sample` / `a_saved_preview_has_pages`.
- [ ] **Step 2:** `cargo test --test suite crystal_reports::` → FAIL.
- [ ] **Step 3:** Implement. The guide text covers: units (twips, 1440 = 1 inch; points; `#RRGGBB`, `set_text_color` takes r/g/b), the two op vocabularies (native snake_case `op`, Vibey camelCase plan), "ids come from the IR returned by the last call", every edit saves over the file (git is the undo), the limits from the spec's Non-goals, the loop (open → read → edit → preview → check), and the database rules from Tasks 5-6.
- [ ] **Step 4:** Tests → PASS. Commit.

### Task 4: MCP tools and bridge routes

**Files:**
- Modify: `src-tauri/src/mcp.rs` (tool entries + `tools_call` forwards), `src-tauri/src/ai_bridge.rs` (routes, `module_of_route` for prefix `/crystal-`), `src-tauri/src/ai_tools.rs` (`CRYSTAL_TOOLS`, `module_of_tool`, `module_tools`), `src/lib/mcpTools.ts` (lists, groups, cards)
- Test: `src-tauri/tests/suite/tcm_mcp.rs`, `src-tauri/tests/suite/ai_tools.rs`, `src-tauri/tests/suite/ai_bridge.rs`

**Interfaces:**
- Consumes: Task 3's functions.
- Produces: tools → routes (all POST unless noted):

| tool | route | args |
|---|---|---|
| `open_report` | `/crystal-open` | `path` (req) |
| `read_report` | `/crystal-read` | `section?`, `object?` |
| `edit_report` | `/crystal-change` | `ops?` (array), `plan?` (object) |
| `compare_reports` | `/crystal-compare` | `a`, `b` (req) |
| `check_report` | `/crystal-check` | `path?` |
| `new_report` | `/crystal-new` | `path` (req), `title?`, `author?`, `comments?` |
| `preview_report` | `/crystal-preview` | — |
| `preview_report_live` | `/crystal-preview-live` | `params?` (object name→value) — Task 6 |
| `get_report_guide` | GET `/crystal-guide` | — |

Every route runs the Task 3 call inside `tokio::task::spawn_blocking`; success is `200` with JSON text, a refusal `400` with the sentence. (Task 7 adds the screen's change event and its emits on these routes.)

- [ ] **Step 1: Failing tests:** `each_tool_belongs_to_its_module` gains the nine tools under `CrystalReports`; `each_module_off_removes_its_tools_from_the_list_and_refuses_them` covers it via `Module::ALL`; a bridge test that `/crystal-open` returns 404 with "Crystal Reports is switched off in Modules" when the module is off; `the_frontend_tool_list_matches` (existing comparison) passes once `mcpTools.ts` is updated.
- [ ] **Step 2:** Run those suites + `npx vitest run src/lib/mcpTools` → FAIL. **Step 3:** Implement. **Step 4:** PASS. **Step 5:** Commit.

### Task 5: Procedures, views and functions on the Dev login

**Files:**
- Modify: `src-tauri/src/db/guard.rs`, `src-tauri/src/db/query.rs`, `src-tauri/src/db/sqlcmd.rs`
- Test: `src-tauri/tests/suite/db_guard.rs` (or the existing guard test module — use the one that already tests `classify`), `src-tauri/tests/suite/db_query.rs` equivalent

**Interfaces:**
- Produces: `Access::DevObjects` (Dev login **and** Crystal Reports on **and** Database Access on); `Verdict::ObjectChange`; `pub const MAX_OBJECT_SQL_CHARS: usize = 200_000;`.
- `query.rs` picks `DevObjects` instead of `DevWrites` when `access_for_user` is Dev and `modules::on(CrystalReports) && modules::on(DbAccess)`. `allowed`: `ObjectChange` passes only on `DevObjects` **with writes switched on** (`WRITES_OFF` otherwise); on `DevWrites` it is refused with: "creating or altering procedures, views and functions needs the Crystal Reports module switched on in Modules"; on `ReadOnly` → `READ_ONLY_SENTENCE`.

- [ ] **Step 1: Failing tests** (pure `classify`/`allowed`, no database):
  - `an_object_statement_is_recognised` — `CREATE PROCEDURE dbo.p AS SELECT 1`, `ALTER PROC p`, `CREATE OR ALTER VIEW v AS …`, `ALTER FUNCTION f() RETURNS INT AS BEGIN RETURN 1 END` → `ObjectChange`.
  - `a_body_may_hold_ordinary_t_sql_and_dynamic_sql` — a body with `DECLARE`, `SET`, `IF/ELSE`, `WHILE`, `BEGIN TRAN … COMMIT`, `BEGIN TRY … END CATCH`, a cursor, `#temp` create/insert/`DROP TABLE #temp`, `EXEC dbo.other @a = 1`, `EXEC sp_executesql @sql`, `EXEC(@sql)` → `ObjectChange`.
  - `a_body_refuses_what_reaches_outside` — one case each: `xp_cmdshell`, `OPENROWSET`, `OPENQUERY`, `OPENDATASOURCE`, `otherdb.dbo.t`, `srv.db.dbo.t`, `GRANT`, `REVOKE`, `DENY`, `BACKUP`, `RESTORE`, `KILL`, `SHUTDOWN`, `RECONFIGURE`, `DBCC`, `EXECUTE AS`, `SETUSER`, `REVERT`, `DROP TABLE t`, `TRUNCATE TABLE t` → `Refused` naming the word.
  - `only_these_objects_may_be_made` — `CREATE TABLE`, `ALTER TABLE`, `CREATE TRIGGER`, `CREATE INDEX`, `CREATE SCHEMA`, `CREATE LOGIN`, `CREATE USER`, `CREATE PROCEDURE otherdb.dbo.p`, two objects in one statement, a `GO` line → `Refused`.
  - `object_changes_need_the_dev_login_and_the_module` — `allowed(sql, ReadOnly)` → read-only sentence; `DevWrites` → the module sentence; `DevObjects` → Ok.
  - `a_long_object_body_goes_through_a_file` — 150 000-char procedure classifies as `ObjectChange`; `sqlcmd_args` for it contains `-i <path>` and no `-Q`; 200 001 chars → refused naming the limit.
  - Existing guard tests still pass unchanged (DDL still refused for every non-object statement).
- [ ] **Step 2:** FAIL. **Step 3:** Implement: in `classify`, before the `REFUSED_WORDS` loop, detect `^(CREATE( OR ALTER)?|ALTER)\s+(PROC|PROCEDURE|VIEW|FUNCTION)\s+<name>` on the comment/literal-stripped text; the name may be `obj`, `schema.obj` (bracketed or not), never three or more parts. Then run the body check: the refused-in-body list above as whole words outside literals (`DROP`/`TRUNCATE` allowed only when followed by `TABLE #`), plus any `a.b.c` / `a.b.c.d` multi-part name → refused. A body over `MAX_SQL_CHARS` is allowed up to `MAX_OBJECT_SQL_CHARS`. In `sqlcmd`, an `ObjectChange` statement is written to a temp file under the app's data folder (deleted after the run) and passed with `-i`.
- [ ] **Step 4:** PASS (also run the whole existing db suite). **Step 5:** Commit.

### Task 6: Live preview

**Files:**
- Modify: `src-tauri/src/crystal/mod.rs` (add `preview_live`), `src-tauri/src/ai_bridge.rs` (route), `src-tauri/src/ai_tools.rs` (allow-file entries), `src-tauri/src/mcp.rs` (tool + `DB_NO_ASK` suffix when `db_no_ask`)
- Test: `src-tauri/tests/suite/crystal_reports.rs`, `src-tauri/tests/suite/ai_tools.rs`

**Interfaces:**
- Produces: `pub fn live_target(report: &rpt_model::Report, own_db: &str) -> Result<String, String>` (the procedure name it will run, checked) and `pub fn preview_live(conn: &crate::db::sqlcmd::Connection, connection_name: &str, params: Vec<(String, String)>) -> Result<PreviewInfo, String>`.
- Route `/crystal-preview-live`: `db_ready(ctx)` for the connection; require `modules::on(DbAccess)`, `ctx.db_writes` (else `WRITES_OFF`), `access_for_user(&conn.user) == Dev` (else `READ_ONLY_SENTENCE`); `live_target` first; then `rpt_core::preview::preview_live_report` with `LiveDb { connection_string, params }` inside `spawn_blocking` (it uses its own tokio runtime and must not run on a runtime thread). Connection string built from `Connection` as `Server=..;Database=..;User Id=..;Password=..;TrustServerCertificate=True|False` and dropped after the call; errors pass through `rpt_db_mssql` redaction and are never logged with it.
- `live_target` uses `rpt_db_mssql::plan::plan_for_report`: first fetchable table must be `StoredProcedure`; strip `;1`; `db.schema.proc` allowed only if `db` equals `own_db` case-insensitively (drop it); 4 parts, other db, `sp_`/`xp_` prefix → refused; `Command{..}` → "this report runs a SQL command, not a stored procedure: live preview only runs stored procedures"; `NotSupported{table,reason}` → names table and reason.
- Auto-approve: `AllowFile`/`write_allow` carry a list of entries; the DB entries become `mcp__tcm__db_query` + `mcp__tcm__preview_report_live` (Claude) and `tcm:db_query` + `tcm:preview_report_live` (Cursor). Turning auto-approve off removes both.

- [ ] **Step 1: Failing tests:** `a_report_naming_another_database_is_refused_before_connecting` (build the check over `TableFetch` values: own db stripped, other db refused, 4-part refused, `xp_x` refused, Command refused, NotSupported names the table); `live_preview_needs_the_dev_login_and_writes` (route-level with a fake ctx: writes off → `WRITES_OFF`; non-dev user → read-only sentence; no connection call made — assert via a runner/flag that nothing connected); `auto_approve_covers_the_live_preview_too` (allow file gains and loses both entries).
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS. **Step 5:** Commit.

### Task 7: The Crystal Reports screen

**Files:**
- Modify: `src-tauri/src/events.rs` (`CrystalReportChanged { path: Option<String> }`), `src-tauri/src/lib.rs` (`collect_events!`, commands), `src-tauri/src/ai_bridge.rs` (emit after open/change/new/preview/preview-live), `src-tauri/tauri.conf.json` (CSP `frame-src 'self' blob:`)
- Create: `src-tauri/src/commands/crystal.rs` — `crystal_current() -> Result<Option<PreviewInfo>, String>`, `crystal_preview_pdf() -> Result<Option<String>, String>` (base64 of the last PDF, no data-URL prefix), `crystal_open_in_designer() -> Result<(), String>` (`tauri_plugin_opener::open_path(path, None)`; if the file is gone: "The report file is no longer there."; if no program is registered for .rpt: "Windows has no program set to open .rpt files. Install Crystal Reports, or open the file from it."). All three refuse with the module-off sentence when `CrystalReports` is off.
- Create: `src/screens/CrystalReports/index.tsx`, `src/screens/CrystalReports/index.test.tsx`
- Modify: `src/components/Sidebar.tsx` (`Section` `"crystal-reports"`, `CASE_ITEMS` entry, `NO_SHORTCUT`), `src/App.tsx` (`TITLES`, render under `modules.has("crystal_reports")`), `src/lib/prefs.ts` (valid section), `src/lib/modules.ts` (`sectionModule`), `src/components/navGlyphs.tsx` (glyph), `docs-site/src/content/` (a Crystal Reports page), then `npm run docs:build`

**Interfaces:**
- Consumes: `PreviewInfo` (Task 3), event from Task 4/6 routes.
- Screen behaviour: listens to `events.crystalReportChanged`; on mount and on each event calls `crystalCurrent` and `crystalPreviewPdf`; turns the base64 into a `Blob` (`application/pdf`) and shows it in an `<iframe title="Report preview">` from `URL.createObjectURL` (revoked on change/unmount). Header: file name + folder (`title` = full path), "Live data from <name>" or "Saved data", page count, diagnostics list. Button **Open in Crystal Reports** (icon from `actionIcons`, e.g. `IconOpenExternal` or the nearest existing "open" icon) → `crystalOpenInDesigner`, error shown under it. No report: "No report open. Ask your AI assistant to open one." 

- [ ] **Step 1: Failing tests** (`index.test.tsx`, mockIPC): `the_screen_says_when_there_is_nothing_to_show` (current → null: the sentence, no iframe, no button); `it_shows_the_open_report_and_refreshes_on_change` (current → info with 3 pages + one diagnostic: file name, "Saved data", "3 pages", the diagnostic, an iframe; emit the event → commands called again); `live_previews_say_where_the_data_came_from`; `open_in_crystal_shows_a_refusal` (command throws → sentence shown). Rust: `the_screen_commands_are_refused_while_the_module_is_off`.
- [ ] **Step 2:** FAIL. **Step 3:** Implement; regenerate bindings. **Step 4:** `npx vitest run src/screens/CrystalReports src/ui-consistency src/lib docs-site` + `npx tsc --noEmit` + the Rust test → PASS. **Step 5:** Commit.

### Task 8: Gate and hand-check list

- [ ] Full gate one at a time: `cd src-tauri && cargo test --tests`, then `npm test`, then `npm run build`. All green.
- [ ] Changelog entry for the next beta (end-user wording; no repo paths).
- [ ] Owner hand-checks (cannot run here): a live preview on the Dev login against a real report procedure; Open in Crystal Reports opens the saved file; the PDF shows inside TCM (WebView2 PDF viewer in an iframe — jsdom cannot see it).
