# Five Improvements: PBI Glow, Browser Bookmarks, Tour, Writing Guide, Database Tools Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Five requests from the owner on 2026-09-22: (1) the PBI chip stops glowing after an upload that mixed new and updated cases; (2) the View in Browser page lets a reviewer bookmark the case they stopped at, kept in the browser only, and folds its four buttons into one menu; (3) the tour visits Suite Management and the review-and-upload part of the queue, and its theme stop walks the person into Settings, lets them pick a theme there, and shows them round; (4) the writing guide asks for reasonable edge cases and puts the names of buttons, pages and fields in quotation marks; (5) the app's own MCP server gains two database tools, a schema lookup and a query runner over `sqlcmd`, using the connection presets the app already ships, so the separate PHR-X server is no longer needed.

**Architecture:** Each item is independent; each is one task with its own tests and commit, except the database work, which is two (the Rust tools and guard, then the MCP surface and settings). The database tools are the one new capability: a pure `db/` module (statement guard, sqlcmd discovery, result parsing, schema search SQL) called by two bridge routes and two MCP tools; the connection string reaches the bridge through `BridgeContext` from the same setting the AI Bridge tab already keeps.

**Tech Stack:** Rust (Tauri 2, tokio process spawning for `sqlcmd`, wiremock-free tests: the sqlcmd runner is faked through a trait), React 19 + TypeScript, vitest, the served review page's plain JavaScript (`src-tauri/web/`).

**Owner decisions (2026-09-22):** database tools live in `tcm-testcases` (no new screen); table lookup is our own search over `INFORMATION_SCHEMA`/`sys.*`, PHR-X is not proxied; `db_query` allows writes only on the "Dev — dev login" preset, with the statement echoed in the log, SELECT-only everywhere else; `sqlcmd` is found on PATH or in the SQL tools folders and its absence is explained (nothing bundled).

## Global Constraints

- Every Rust test is an integration test under `src-tauri/tests/`. One build or test command at a time on this shared machine; Rust from `src-tauri/` with `CARGO_TARGET_DIR=target/gate`; frontend `npx vitest run --exclude "**/.claude/**"`. No new crates. `src/bindings.ts` is generated; never hand-edit it.
- GET/POST/PATCH only against Azure DevOps; nothing here touches Azure DevOps at all.
- **The database tools never run a statement they have not classified.** `db_query` refuses anything but one SELECT (or `WITH … SELECT`) unless the connection is the dev-login preset; on that preset INSERT/UPDATE/DELETE/MERGE are allowed and every statement is logged whole before it runs; DROP/TRUNCATE/ALTER/CREATE/EXEC/GRANT/`xp_`/`sp_` and batch separators (`GO`) are refused on every preset. Results are capped (rows and characters) and the run has a timeout.
- **Credentials never appear in a log line, a tool result or an error.** `sqlcmd` receives the password through its own argument, never through a shell; the app's log names the server and database only.
- The review page's bookmark is browser-only: a localStorage key on the page's origin, never sent to the app, never in a run file or a bug report.
- The tour's house rules hold (`src/tour/tourScript.ts` header: two stops per tab, AI Bridge four; short sentences; nothing about internals) and `src/tour/tourScript.test.ts` / `tourAnchors.test.ts` are the gate; do not weaken them.
- The writing guide's existing tests in `src-tauri/tests/ai_bridge.rs` slice the document by heading; new text goes in its own sections or at the end of an existing one so those slices keep passing.
- No em dashes in user-facing text. Colours via tokens; icons from `src/lib/actionIcons.ts`; `src/ui-consistency.test.ts` must not be weakened. The changelog is end-user-facing and is written at release time by the owner; each task's commit subject states its user-visible change.
- Commits via Bash heredoc with the model's own `Co-Authored-By` trailer.

**Out of scope:** a person-facing DB screen; bundling `sqlcmd`; removing the PHR-X registration code (it stays available; the tour and AI Bridge copy stop presenting it as required); bookmarks in the app itself.

## Tasks

1. The PBI glow ends when the upload does
2. Bookmarks and a menu on the review page
3. The tour: Suite Management, review and upload, and a theme picked in Settings
4. The writing guide: edge cases and quoted names
5. Database tools: the guard, sqlcmd, the schema search
6. Database tools: bridge routes, MCP tools, settings and the guides
7. Auto Run: clear the scripts, clear the results (development build)

---

### Task 1: The PBI glow ends when the upload does

**Files:**
- Modify: `src/components/QueueSection.tsx`
- Test: `src/components/QueueSection.test.tsx` (or the file that already tests the queue's submit; Grep `auto_run\|submit_queue\|QueueSection` under `src/**/*.test.tsx` and add to the one that mounts `QueueSection` with a mocked `submit_queue`)

**What is wrong.** `arm(on)` (`QueueSection.tsx` around line 300) sets the `armed` state and dispatches the PBI glow (`setPbiGlow`). Reviewing a queue that contains at least one new case arms it (`openReview` and the floating confirm). The submit's `onSuccess` (around line 627) calls `setReviewing(false)` but never `arm(false)`, so after a mixed upload the chip keeps glowing and the floating action button stays hidden (`!armed` gates it) until the component unmounts. The duplicate gate's "Stop, take me back" button (around line 1424) has the same omission. After a success the uploaded rows gain ids, `pureUpdates` becomes true, and no later path ever re-arms or disarms.

- [ ] **Step 1: Write the failing tests**

In the queue test file, with the existing mocks for `submit_queue` (returning one `created` and one `updated` result), `set_bridge_context`, etc.:

```tsx
test("the PBI stops glowing once a mixed upload has succeeded", async () => {
  const glows: boolean[] = [];
  window.addEventListener("tcm-pbi-glow", (e) => glows.push((e as CustomEvent<boolean>).detail));
  // mount QueueSection with a queue of one new case (update_id null) and one update
  // open the review (the button the existing tests use), then confirm the upload
  await waitFor(() => expect(glows.at(-1)).toBe(true)); // armed while reviewing
  // ... confirm, wait for the results list
  await waitFor(() => expect(glows.at(-1)).toBe(false));
});

test("stopping at the duplicate gate also stops the glow", async () => {
  // a queue whose title duplicates an existing case (the existing duplicate-gate test shows how)
  // open review -> armed; press "Stop - take me back" -> the last glow event is false
});
```

Write both in full against the file's existing helpers; keep the assertions on the event stream, not on component state.

- [ ] **Step 2: Run:** `npx vitest run <that file> --exclude "**/.claude/**"`; expected: both fail (the last event stays `true`).

- [ ] **Step 3: Implement.** In `onSuccess`, replace `setReviewing(false)` with `arm(false); setReviewing(false);` and add a one-sentence comment (the glow says "about to create on this PBI"; once it has happened there is nothing to warn about). In the duplicate gate's Stop button, `onClick={() => { arm(false); setReviewing(false); }}`. Also make `onError` of the same mutation call `arm(false)` if it does not already (a failed upload leaves nothing armed either; read it and decide, and say which in the report).

- [ ] **Step 4: Run** the file, then `npx vitest run src/components src/ui-consistency.test.ts --exclude "**/.claude/**"` and `npx tsc --noEmit`.

- [ ] **Step 5: Commit**

```bash
git add src/components/QueueSection.tsx src/components/*.test.tsx
git commit -q -F - <<'EOF'
fix(v2): the PBI stops glowing once an upload with new cases has gone through

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 2: Bookmarks and a menu on the review page

**Files:**
- Modify: `src-tauri/src/import_parser/html.rs` (the searchbar's buttons, a stable key per case, a page scope), `src-tauri/src/commands/queue.rs` (pass the scope in), `src-tauri/web/cases-page.js` (the menu and the bookmark), `src-tauri/web/cases.css` or wherever the page's styles live (Grep `tc-notes` under `src-tauri/web`)
- Test: `src-tauri/tests/html_report.rs` or whichever test renders the page (Grep `tc-notes` under `src-tauri/tests`); `src-tauri/tests/web_assets.rs` if the JS is pinned by a test

**How it looks.** The four controls (Show/Hide reviewer notes, Show/Hide findings, View as Tree, Show/Hide spec) move into one `Options` button in the searchbar that opens a small menu (a `<details class='tc-menu'>` with the four items inside; native disclosure, no framework, closes on Escape and on a click outside). Each item keeps its id and behaviour, so the existing JS keeps working. Beside the search box a `Bookmark` item is NOT in the menu: bookmarking is per case, so each case heading gets a small bookmark button at its right edge (`<button class='tc-mark' aria-pressed='false' title='Bookmark: where the review stopped'>`), and the searchbar gets one `Go to bookmark` button that scrolls to the marked case (hidden when nothing is marked). One bookmark per page scope: marking a case unmarks the previous one. The marked case's heading shows the mark filled and the case gets a left border in the accent colour.

**Storage.** localStorage on the page's origin, key `tcm-report-mark:<scope>`, value the case key. `scope` is written by the renderer as `<body data-scope='…'>`: `pbi-<id>` for a PBI's cases and `draft-<hash of the file path>` for a draft (`html.rs` gets a `scope: &str` parameter; `render_queue_html`/`render_draft_html` supply it; the hash is the existing FNV-1a helper if one exists in the crate, else 8 hex digits of FNV-1a written inline as `project_slug` does). Each `.case` gets `data-key`: the ADO id for a case that has one (`wid`), else `d<slot>` (the same identity the comment boxes use). All storage access is try/catch-guarded like `remember`/`recall` in `cases-page.js` (a `file://` origin may refuse storage: the button still toggles for the session).

The live in-place swap (`cases-page.js` around lines 146-172) re-runs wiring after replacing the case list; the bookmark wiring must be re-runnable the same way: expose `window.__tcmWireMarks()` and call it there.

- [ ] **Step 1: Write the failing tests**

Rust (the render test file): `the_review_page_folds_its_controls_into_one_menu` asserts the searchbar contains one `<details class='tc-menu'>` whose summary reads `Options`, and that `tc-notes`, `tc-findings`, `tc-tree` and `tc-spec` are INSIDE it (string order check), and that `data-scope='pbi-42'` is on `<body>` for a PBI render and `data-scope='draft-` for a draft; `every_case_carries_its_key`: `data-key='157957'` for an ADO case and `data-key='d0'` for the first draft slot; `each_case_has_a_bookmark_button_and_the_bar_a_go_to`: one `tc-mark` per case, one `tc-goto`.

JS: if `src-tauri/web` has a test harness (Grep `describe(` / `vitest` under `src-tauri/web` and `src`), add `cases-page.marks.test.ts` with a jsdom page containing three cases: clicking a mark stores the key under `tcm-report-mark:<scope>`, marks only that case, un-marks the previous; `Go to bookmark` calls `scrollIntoView` on the marked case; with storage throwing, the toggle still works and nothing is thrown. If there is no harness, say so in the report and cover the JS with the Rust `web_assets` pin (the file contains the function names) plus a hand check.

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement.** Keep the JS in the style of `cases-page.js` (plain functions, guarded storage, idempotent wiring). The menu's summary is a button-styled element using the page's existing button styles; the four items are the existing elements moved inside a vertical list. `Go to bookmark` and `tc-mark` use the page's existing palette (`var(--accent)` or whatever the page defines; read `webtheme.rs`).
- [ ] **Step 4: Run** the Rust render tests, `cargo test --test web_assets` (if it exists), the JS test, then open the page once by hand from a dev build (View in Browser) and check: the menu opens/closes, the four items still work, a bookmark survives a reload of the same page, `Go to bookmark` scrolls.
- [ ] **Step 5: Commit**

```bash
git add src-tauri/src src-tauri/web src-tauri/tests
git commit -q -F - <<'EOF'
feat(v2): the review page keeps a bookmark where the review stopped, and its controls sit in one menu

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 3: The tour: Suite Management, review and upload, and a theme picked in Settings

**Files:**
- Modify: `src/tour/tourScript.ts` (steps, anchors, `TourWhere`/`tourControl` for Settings), `src/tour/UiTour.tsx` (the waiting card for the gear; drop the in-card theme picker), `src/screens/ManageCases/index.tsx` (anchor), `src/screens/ImportFile.tsx` (anchor), `src/components/QueueSection.tsx` (anchor on the review/upload controls), `src/screens/Settings.tsx` (anchors), `src/tour/tourData.ts` (fake data for Suite Management if the screen needs it to render a table)
- Test: `src/tour/tourScript.test.ts`, `src/tour/tourAnchors.test.ts`, `src/tour/UiTour.test.tsx` (whichever exists), `src/screens/Settings.test.tsx`

**The new script**, replacing steps 10 to 18 (keep 1 to 9 as they are):

| # | where | anchor | Title / body |
| --- | --- | --- | --- |
| 10 | cases `suites` | `plans-tree` | (unchanged) "Find any set of tests" |
| 11 | cases `manage` | `manage-plans` | "Arrange a PBI's suite" / "Every plan that holds the PBI's cases, with the suites and cases underneath. Tick cases to copy them into another suite or a new one, and drag to set the order testers see." |
| 12 | cases `import` | `queue-review` | "Review before you upload" / "Nothing goes to Azure DevOps until you have looked. Review shows what will be created and what updated, warns about duplicates, and Upload is the one button that sends." |
| 13 | cases `ai` | `ai-repos` | (unchanged) |
| 14 | ″ | `ai-tools` | (unchanged) |
| 15 | ″ | `ai-toolset` | (unchanged) |
| 16 | ″ | `ai-db` | "Company database" / "Your assistant can look up tables and check real data while it writes, using the connection you choose here." (reworded: the DB is now our own tools, Task 6) |
| 17 | work `board` | `board-columns` | (unchanged) |
| 18 | ″ | `nav-prs` | (unchanged) |
| 19 | cases `settings` | `theme` | "Make it yours" / "Pick a theme and an accent colour. The whole app follows, and this choice stays after the tour." (the card's button reads Continue; no in-card picker any more) |
| 20 | ″ | `settings-backup` | "Take it with you" / "Backup and transfer packs your settings and queue into one file for another machine." |
| 21 | ″ | `settings-updates` | "Kept up to date" / "The app checks for updates on its own; this is where you see the version and check by hand." |
| 22 | – | – | (unchanged) "That is the tour" |

Two stops on Suite Management is within budget (one); Import File now has two (`import-drop`, `queue-review`); Settings has three, so the budget rule in `tourScript.ts` gains `settings` beside AI Bridge as an allowed exception (three), with its reason in the comment (it is the one tab the tour asks the person to act in). The `queue` stop on Manual Entry (step 5) stays and loses its last sentence ("The same panel appears on the Import File tab.") since the Import File tab now has its own stop.

**Reaching Settings.** `TourWhere` for `cases("settings")` already type-checks if `Section` includes `"settings"` (it does: `App.tsx` uses `section === "settings"`). `tourControl` returns `{ kind: "case", section: "settings" }`, whose control anchor today would be `nav-settings`, which does not exist: the gear is `data-tour="settings"` in the context bar. Add to `UiTour.tsx`'s `controlAnchor`: a `case` control for section `settings` rings `settings`, and its waiting card reads `Click the Settings gear at the top right to carry on.` Back from a Settings stop navigates as Back does elsewhere. `onAwait`'s "leave one rail row live" must leave the gear live for this control (read how App handles `onAwait` and extend it for the gear).

**Anchors to add** (and to `TOUR_ANCHORS`): `manage-plans` on the plan table wrapper in `src/screens/ManageCases/index.tsx`; `queue-review` on the element in `QueueSection.tsx` that holds the Review/Upload controls (the sticky bar or the review header: pick the element that exists both before and during review; if the floating button is portalled, anchor the queue header instead and say so); `theme` on the Appearance section's theme + accent block in `Settings.tsx`; `settings-backup` on the Backup & transfer section; `settings-updates` on the Updates section. `ImportFile.tsx` keeps `import-drop` and gets nothing else (the queue anchor lives inside `QueueSection`).

**The picker.** Delete `TourThemePicker` and the `picker` field from `TourStep`; the Settings theme block is the real one. `tourData.ts`: Suite Management renders from the fake backend; if `ManageCases` needs a fake plan tree to show anything, add one plan with two suites and three cases under it in the tour data (read `tourBackend.ts` to see which commands it fakes; the screen's queries must be answered, else the anchor never appears and the tour's 1.5 s wait times out).

- [ ] **Step 1: Write the failing tests:** `tourScript.test.ts`: the budget rule allows three on `settings`; the script has a `manage` stop with anchor `manage-plans`, a second `import` stop with `queue-review`, three `settings` stops ending on `settings-updates`, no `picker` field anywhere; `tourAnchors.test.ts` covers the new names (it scans `src/` for `data-tour="..."`); a `UiTour` test: at a `settings` destination from `cases/manual`, the waiting card says `Click the Settings gear` and rings `settings`; a `Settings.test.tsx` assertion that the three anchors exist on the named sections.
- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement.** Copy stays within the house rules (one or two sentences, tester's words, nothing about internals).
- [ ] **Step 4: Run:** `npx vitest run src/tour src/screens/Settings.test.tsx src/screens/ManageCases src/screens/ImportFile.test.tsx src/ui-consistency.test.ts --exclude "**/.claude/**"`, `npx tsc --noEmit`, then walk the whole tour once in a dev build (Settings, Show UI tour) and confirm every stop rings something and the gear is clickable while the tour waits for it.
- [ ] **Step 5: Commit**

```bash
git add src/tour src/screens src/components
git commit -q -F - <<'EOF'
feat(v2): the tour visits Suite Management and the upload review, and picks the theme in Settings

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 4: The writing guide: edge cases and quoted names

**Files:**
- Modify: `src-tauri/src/ai_bridge.rs` (`guide()`), `src-tauri/src/optimize.rs` only if its quoted-text handling needs to learn the new convention (read `optimize.rs` around lines 242-243 and 374-388 first: it already treats quoted alert text carefully)
- Test: `src-tauri/tests/ai_bridge.rs`

**The two additions.**

1. A new section `## Edge cases worth writing`, placed after `## One branch per case` and before `## Allowed Module values (live)`:

```
## Edge cases worth writing

A set that only walks the happy path is not finished. For each feature, add the edge cases a tester can run from the application itself in a few minutes, each as its own case with the branch in its title:

- Access: open the page's address without signing in, or as a role that should not see it; the expected result is what the application shows instead (the sign-in page, a permission message), named exactly.
- Required and empty: submit with a required field blank, with only spaces, at the field's maximum length, and one over it.
- Boundaries the form shows: the smallest and largest value a field accepts, a date at the edge of the allowed range, zero and a negative number where the field is numeric.
- State: the same action twice (double submit, refresh after saving, back button after a save), and an item edited by someone else in between when the application shows that.
- Absence: the list with nothing in it, a search with no matches, a filter that removes everything; the expected result is the empty state's own words.

Do NOT write cases that need developer tools, a modified request, a database change, a disconnected network, or a clock change: a tester cannot run them from the application, and a case nobody can run is worse than none. If a spec names such a behaviour, put it in `reviewer_notes` as a note for the developers instead.
```

2. In `## Writing style - sound like a tester, not a model`, a new rule added at the END of that section's list:

```
- MUST put the name of anything the tester looks for on screen in double quotation marks: the "Save" button, the "Leave Requests" page, the "Search employees" placeholder, the "Status" column, the "Approved" tab, the "Your changes were saved" message. The name inside the quotes is the exact text on screen, capitalised as the application shows it. Without quotes a tester cannot tell the word "save" from the button "Save".
```

Also update the worked example(s) in the guide (if any step text names a button without quotes) so the guide practises what it teaches, and the intro sentence of `## Granularity` if it contradicts the edge-case section (it does not; check).

- [ ] **Step 1: Write the failing tests** in `src-tauri/tests/ai_bridge.rs`, beside `guide_carries_format_rules_and_live_modules`:

```rust
#[tokio::test]
async fn the_guide_asks_for_reasonable_edge_cases_and_quoted_names() {
    // build ctx + client the way guide_carries_format_rules_and_live_modules does
    let g = /* GET /guide body */;
    let edge = section(&g, "## Edge cases worth writing");
    assert!(edge.contains("without signing in"), "the one edge case the owner named must be there");
    assert!(edge.contains("Do NOT write cases that need developer tools"), "the boundary of a reasonable edge case must be stated");
    assert!(edge.contains("reviewer_notes"));
    let style = section(&g, "## Writing style");
    assert!(style.contains("double quotation marks"));
    assert!(style.contains("the \"Save\" button"));
    // Order: edge cases come after one-branch and before the live modules.
    let a = g.find("## One branch per case").unwrap();
    let b = g.find("## Edge cases worth writing").unwrap();
    let c = g.find("## Allowed Module values").unwrap();
    assert!(a < b && b < c);
}
```

where `section(g, heading)` is the slicing helper the file already uses (reuse it; do not add a second).

- [ ] **Step 2: Run to verify failure:** `cargo test --test ai_bridge the_guide_asks_for_reasonable_edge_cases_and_quoted_names`.
- [ ] **Step 3: Implement.** Also read `src-tauri/src/optimize.rs`'s handling of quoted text in steps (`trim_expected`, alert messages): the new convention puts more quotes into actions and expected results; make sure the optimizer neither strips them nor treats every quoted phrase as an alert. Add one optimizer test if its behaviour on a step like `Click the "Save" button` was not already pinned.
- [ ] **Step 4: Run:** `cargo test --test ai_bridge`, `cargo test --test optimize` (or whichever file covers `optimize.rs`), `cargo test --test ai_tools` (it asserts the slash-command files do not duplicate the guide), warnings check.
- [ ] **Step 5: Commit**

```bash
git add src-tauri/src src-tauri/tests
git commit -q -F - <<'EOF'
feat(v2): the writing guide asks for runnable edge cases and puts on-screen names in quotation marks

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 5: Database tools: the guard, sqlcmd, the schema search

**Files:**
- Create: `src-tauri/src/db/mod.rs`, `src-tauri/src/db/guard.rs`, `src-tauri/src/db/sqlcmd.rs`, `src-tauri/src/db/schema.rs`; add `pub mod db;` in `src-tauri/src/lib.rs`
- Test: `src-tauri/tests/db_guard.rs`, `src-tauri/tests/db_sqlcmd.rs`, `src-tauri/tests/db_schema.rs`

**Interfaces (all in `v2_lib::db`):**

```rust
// guard.rs
/// What a statement is allowed to do on this connection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Access { ReadOnly, DevWrites }

/// The dev-login preset is the only connection that may write: recognised by
/// its User Id, compared case-insensitively, never by the password.
pub fn access_for(connection_string: &str) -> Access; // user id ends with "_devlogin" -> DevWrites, else ReadOnly

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict { Read, Write, Refused(String) }

/// Classifies ONE statement. Comments and string literals are stripped
/// before the first keyword is read; `GO`, a second statement (`;` followed by
/// anything but whitespace), and every DDL/EXEC/procedure form are refused
/// outright with a sentence naming what was found.
pub fn classify(sql: &str) -> Verdict;
// Read: SELECT, WITH ... SELECT, (SELECT ...)
// Write: INSERT, UPDATE, DELETE, MERGE
// Refused: DROP, TRUNCATE, ALTER, CREATE, EXEC/EXECUTE, GRANT/REVOKE/DENY, BACKUP/RESTORE, xp_/sp_ anywhere as a call,
//          GO on its own line, a second statement, an empty statement, a statement over MAX_SQL_CHARS (20_000)
pub fn allowed(sql: &str, access: Access) -> Result<Verdict, String>;
// Read always Ok; Write Ok only for DevWrites, else Err("this connection is read only: INSERT/UPDATE/DELETE need the Dev - dev login connection, chosen in the AI Bridge tab"); Refused -> Err(the sentence)

// sqlcmd.rs
pub const ROW_CAP: usize = 200;
pub const CHAR_CAP: usize = 60_000;
pub const TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Clone, PartialEq)]
pub struct Connection { pub server: String, pub database: String, pub user: String, pub password: String, pub trust_cert: bool }
pub fn parse_connection(connection_string: &str) -> Result<Connection, String>; // "Server=;Database=;User Id=;Password=;TrustServerCertificate=" keys, case-insensitive; a missing key names itself in the error; the password never appears in an error
impl std::fmt::Debug for Connection { /* password shown as (hidden) */ }

/// Where sqlcmd is: PATH first, then the Microsoft SQL tools folders.
pub fn find_sqlcmd(path_var: &str, program_files: &str, program_files_x86: &str) -> Option<PathBuf>;
// candidates in order: each PATH entry joined with "sqlcmd.exe"; "{pf}\Microsoft SQL Server\Client SDK\ODBC\{170,180}\Tools\Binn\sqlcmd.exe" and the same under pf x86; "{pf}\sqlcmd\sqlcmd.exe"
pub const NOT_INSTALLED: &str = "sqlcmd is not installed on this machine - install it with `winget install sqlcmd` (or the SQL Server command line tools), then try again";

/// The one process this app spawns for SQL. Faked in tests through the trait.
pub trait Runner {
    fn run(&self, exe: &Path, args: &[String], stdin: &str, timeout: Duration) -> impl std::future::Future<Output = Result<Output, String>>;
}
pub struct Output { pub status: i32, pub stdout: String, pub stderr: String }
pub struct RealRunner;

/// The argument list for sqlcmd: server, database, user, password, -C when trust_cert, -s "\t" -W (tab separated, trimmed),
/// -h -1 off (keep headers), -t <timeout>, -b (exit on error), -Q for the statement text, and -y 0 -Y 0 so wide columns are not truncated.
/// The statement goes through -Q, not stdin, so a multi-line SELECT is one batch. The password is an argument to sqlcmd itself, never a shell.
pub fn sqlcmd_args(c: &Connection, sql: &str) -> Vec<String>;

/// Runs one already-allowed statement and returns the tab-separated text, capped: at most ROW_CAP data rows,
/// at most CHAR_CAP characters, with a last line "... N more rows (capped)" / "... output capped at CHAR_CAP characters" when it was.
/// A non-zero exit: Err with sqlcmd's stderr/stdout message, with the password replaced by "(hidden)" should it ever appear.
pub async fn run_sql<R: Runner>(r: &R, exe: &Path, c: &Connection, sql: &str) -> Result<String, String>;

// schema.rs
/// A ranked lookup over the database's own catalogue. One SELECT (so it is a Read under the guard), built from
/// INFORMATION_SCHEMA.TABLES/COLUMNS and sys.foreign_keys, scored: table name equals a term 100, contains 60,
/// column name equals 40, contains 20; terms are the words of `query` (lower-cased, non-alphanumerics split), each
/// escaped as a T-SQL string literal (' doubled). `schema_filter` non-empty limits TABLE_SCHEMA. Returns the top `limit` tables
/// with their matching columns, then a second SELECT-free description of each's foreign keys is folded into the same SELECT via
/// STRING_AGG when the server supports it (SQL Server 2017+); the SQL is a single statement.
pub fn lookup_sql(query: &str, schema_filter: &str, limit: usize) -> String;
/// Renders sqlcmd's tab-separated answer to `lookup_sql` as text an assistant reads: one block per table,
/// "schema.table (N rows est.)", its matching columns with types, its foreign keys as "column -> other.table(column)".
pub fn render_lookup(tsv: &str) -> String;
```

- [ ] **Step 1: Write the failing tests**

`tests/db_guard.rs` (pure): `access_for` on the three shipped presets (`db_defaults::DB_PRESETS`): only the dev-login one is `DevWrites`; `classify`: `SELECT 1` Read; `  with x as (select 1) select * from x` Read; `(SELECT 1)` Read; `-- comment\nSELECT 1` Read; `/* DROP */ SELECT 1` Read (the word inside a comment does not count); `SELECT 'DROP TABLE' AS s` Read (a literal does not count); `INSERT INTO t VALUES (1)` Write; `UPDATE t SET a = 1` Write; `DELETE FROM t` Write; `MERGE ...` Write; `DROP TABLE t` Refused naming DROP; `SELECT 1; DELETE FROM t` Refused ("a second statement"); `SELECT 1\nGO\nSELECT 2` Refused (GO); `EXEC sp_who` Refused; `SELECT * FROM t; ` (trailing semicolon and spaces) Read; `` (empty) Refused; 20_001 chars Refused; `allowed(write, ReadOnly)` is the read-only sentence; `allowed(write, DevWrites)` Ok(Write); `allowed(refused, DevWrites)` still Err.

`tests/db_sqlcmd.rs`: `parse_connection` on the three presets (server, database, user, trust_cert true; password not in `{:?}`); a string missing `Password=` errs naming `Password` without the rest; `find_sqlcmd` with a temp dir laid out as the ODBC 180 folder and an empty PATH finds it; with PATH containing a dir that has `sqlcmd.exe` finds that first; nothing -> None; `sqlcmd_args` contains `-S server`, `-d database`, `-U user`, `-P password`, `-C`, `-Q <sql>`, `-t 30`, `-b`, and no shell metacharacter handling is needed (assert the args are separate strings); `run_sql` with a `FakeRunner` returning 250 tab-separated rows: output has 200 data rows plus the header and the "... 50 more rows (capped)" line; a 100 000-char stdout is cut at CHAR_CAP with the cap line; a non-zero status with stderr `Login failed for user 'x'. Password=abc` returns Err whose text contains `Login failed` and not `abc` (the redaction: replace the connection's password wherever it appears); the FakeRunner records the timeout it was given (30 s).

`tests/db_schema.rs`: `lookup_sql("leave request", "PeoplesHR", 10)` contains `INFORMATION_SCHEMA.TABLES`, `TABLE_SCHEMA = N'PeoplesHR'`, both terms lower-cased as literals, a `TOP (10)`, and is one statement (`classify` says Read); a term with a quote `o'brien` is doubled; `render_lookup` on a hand-written TSV of two tables renders the blocks and the FK lines; an empty TSV renders `no table or column matches those words`.

- [ ] **Step 2: Run to verify failure** (compile errors).
- [ ] **Step 3: Implement.** `guard.rs`'s stripping: remove `--` to end of line, `/* ... */` blocks, and the contents of single-quoted literals (keeping the quotes), then upper-case and take the first word for the verb; refused keywords are searched as whole words in the stripped text; `GO` is a line that is only `GO` (case-insensitive) in the ORIGINAL text; a second statement is a `;` in the stripped text followed by any non-whitespace. `sqlcmd.rs`'s `RealRunner` uses `tokio::process::Command` with `.args(args)` (no shell), `kill_on_drop(true)`, and `tokio::time::timeout`. `schema.rs`'s SQL: one SELECT over a CTE of matching tables and a CTE of matching columns joined to `sys.foreign_keys`/`sys.foreign_key_columns` for the FK text; test it against the guard in the test so it can never be refused by its own gate.
- [ ] **Step 4: Run** the three test files, `cargo test --test ado` (its no-DELETE source scan may need `db/` excluded: the word DELETE appears in `guard.rs` as a classified verb, not an HTTP method; read the scan's exclusions and add the module the way `boards.rs` was added to the INCLUDE list only if the scan is about HTTP verbs in `ado/`; say what you did), warnings check.
- [ ] **Step 5: Commit**

```bash
git add src-tauri/src src-tauri/tests
git commit -q -F - <<'EOF'
feat(v2): a SQL guard, a sqlcmd runner and a schema lookup, ready for the assistant's database tools

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 6: Database tools: bridge routes, MCP tools, settings and the guides

**Files:**
- Modify: `src-tauri/src/ai_bridge.rs` (routes `POST /db-lookup`, `POST /db-query`; `BridgeContext.db_connection_string: Option<String>`), `src-tauri/src/commands/ai_bridge.rs` (`set_bridge_context` gains `db_connection_string: Option<String>`), `src-tauri/src/mcp.rs` (tools `db_lookup`, `db_query`), `src-tauri/src/ai_tools.rs` (`CORE_TOOLS`? no: they are switchable; add to the ordinary list), `src/lib/mcpTools.ts` (two entries, one row "Company database"), `src/App.tsx` (push the stored connection string into the context), `src/screens/AiBridge.tsx` (the Company database card: the PHR-X server path becomes optional and secondary; the presets and connection string are what the tools use; copy says so), `src/tour/tourScript.ts` copy for `ai-db` (Task 3 already reworded it), `src-tauri/src/autorun/guide.rs` (the "database: verifying effects" line names the tools), the writing guide's `## Use these tools` section (names `db_lookup`/`db_query` for checking real data)
- Test: `src-tauri/tests/ai_bridge.rs`, `src-tauri/tests/tcm_mcp.rs`, `src-tauri/tests/ai_tools.rs`, `src/lib/mcpTools.test.ts`, `src/screens/AiBridge.test.tsx`, `src-tauri/tests/autorun_guide.rs`
- Generated: `src/bindings.ts`

**Routes** (both need a connection: with `ctx.db_connection_string` `None`, `(409, "no database connection is chosen - pick one under Company database on the AI Bridge tab")`; with `sqlcmd` not found, `(409, NOT_INSTALLED)`):

| Method, path | Body | Answer |
| --- | --- | --- |
| `POST /db-lookup` | `{ "query": "leave request", "limit"?: 10 }` | 200, `render_lookup` text; 400 for an empty query; `limit` clamped 1..=30 |
| `POST /db-query` | `{ "sql": "SELECT ..." }` | 200, the capped TSV text with a first line `rows: N (capped)` when capped; 400 with the guard's sentence for a refused or not-allowed statement; 502 `the database refused the statement: <redacted message>` for a sqlcmd failure |

Every `/db-query` call logs `db query (<Read|Write>) on <server>/<database>: <first 200 chars of the statement>`; a Write logs the whole statement. Never the user or password. `/db-lookup` logs the words.

**Tools** (after `save_autorun_script`'s Auto Run block in `tools_list`, before `optimize_cases`; not dev-only; switchable as one row):
- `db_lookup` (`query: string`, `limit?: number`): "Find the tables and columns behind a topic in the company database: table and column names, types, foreign keys, ranked by how well they match the words. Use it before writing a query, and to check which table a screen reads from."
- `db_query` (`sql: string`): "Run one SQL statement on the chosen company database through sqlcmd and read the result (200 rows at most). SELECT on every connection; INSERT, UPDATE and DELETE only when the person has switched writes on in the AI Bridge tab and the connection is the Dev - dev login one; never DROP, ALTER, CREATE or EXEC. Use it to verify what a test case expects against real data, or to set up test data on the dev database."

**TS mirror and settings (amended 2026-09-22, owner):** two switches, both on the AI Bridge tab's Company database card, one for reading and one for writing.
- *Reading* is the tools' row: `MCP_TOOLS` gains `db_lookup` and `db_query`; `TOOL_PAIRS` gains `["db_lookup", "db_query"]` with `PAIR_ROWS.db_lookup = { label: "Company database (read)", summary: "Look up tables and run SELECT on the connection chosen below." }`. Off means both tools are refused the ordinary way ("switched off in Test Case Manager").
- *Create, update, delete* is a second switch, OFF by default, stored as `tcm-v2-db-writes` ("1" only when on; absent means off, so a fresh profile and a cleared one both read off) in `src/lib/dbServer.ts`, and pushed to the bridge as `BridgeContext.db_writes: bool` through `set_bridge_context` alongside the connection string. `/db-query` allows INSERT/UPDATE/DELETE/MERGE only when BOTH hold: the switch is on AND the connection's user is the dev login (the guard's `Access::DevWrites`); with the switch off the refusal reads `create, update and delete are switched off - turn them on under Company database on the AI Bridge tab`; with the switch on but a read-only connection, the guard's own read-only sentence. The card shows the write switch under the read one with one line of copy: `Only on the Dev - dev login connection, and every statement is written to the log.` and the switch is disabled with that explanation when the chosen connection is not the dev login.
- `App.tsx` reads both the stored `connection_string` (blank means none) and the writes flag and passes them to `setBridgeContext`, re-pushing when either changes (the `useSyncExternalStore` pattern the disabled set uses; `saveDbConfig`/the new `saveDbWrites` notify it).
- The Company database card's heading loses "(PHR-X)"; a one-line note says the PHR-X server registration is optional and no longer needed for lookups.

**Guides.** Writing guide `## Use these tools`: one bullet: `db_lookup` and `db_query` check real data (which table a screen reads, what a value is today); the expected result still comes from the spec, the database only tells you the current state. Auto Run guide: the "verifying effects" bullet names the two tools. Add one drift assertion each.

- [ ] **Step 1: Write the failing tests.** `tests/ai_bridge.rs`: the two routes with a `FakeRunner` (the route needs a way to be given one: make the runner a `BridgeContext`-independent injectable, for example a `pub static SQL_RUNNER: OnceLock<Box<dyn Runner>>`-free design is hard with async traits; simplest: the routes call `db::query::run_lookup(ctx, runner, ...)`/`run_query(...)` pure-ish functions in a new `db/query.rs` that take `&impl Runner` and the found exe path, and the tests call THOSE with the fake, while the route's own test only covers the 409 branches (no connection; sqlcmd missing via an env override `TCM_SQLCMD` that the finder honours first, pointing at a nonexistent path)). Cover: read-only preset + INSERT -> 400 with the read-only sentence and no runner call; dev-login + INSERT with `db_writes: false` -> 400 with the switched-off sentence and no runner call; dev-login + INSERT with `db_writes: true` -> runner called, log line contains the statement; DROP on dev-login -> 400, no call; lookup renders; sqlcmd failure -> 502 without the password. `tests/tcm_mcp.rs`: order assertion extended; both tools switchable and refused the ordinary way. `mcpTools.test.ts`: the new row. `AiBridge.test.tsx`: the heading no longer says PHR-X; saving a preset calls `set_bridge_context` with the connection string; the write switch is off by default, disabled with its explanation on a read-only connection, and turning it on (dev login chosen) pushes `db_writes: true`; `src/lib/dbServer.test.ts` (or the file that tests it): the writes flag round-trips and defaults to off.
- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement.** Bindings regenerated (the `set_bridge_context` signature changes).
- [ ] **Step 4: Run:** `cargo test --tests`, `cargo test --test bindings` + line-ending check, `npx tsc --noEmit`, `npx vitest run --exclude "**/.claude/**"`, `npm run build`. Then, by hand in a dev build with `sqlcmd` installed (`winget install sqlcmd`): choose "Dev — read only" on the AI Bridge tab, ask the connected assistant to `db_lookup` "leave request" and to `db_query` a SELECT; confirm the read-only refusal of an INSERT; switch to the dev login and confirm the INSERT runs and is in Settings, Logs whole. Record the results in the report.
- [ ] **Step 5: Commit**

```bash
git add src-tauri/src src-tauri/tests src src/bindings.ts
git commit -q -F - <<'EOF'
feat(v2): the assistant can look up tables and run SQL on the chosen company database, writes only on the dev login

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 7: Auto Run: clear the scripts, clear the results (development build)

**Files:**
- Modify: `src-tauri/src/autorun/store.rs` (`clear_scripts`, `clear_runs`), `src-tauri/src/commands/autorun.rs` (two commands), `src-tauri/src/lib.rs` (register)
- Modify: `src/screens/AutoRun/index.tsx` (two toolbar buttons with a confirm), `src/lib/actionIcons.ts`
- Test: `src-tauri/tests/autorun_store.rs`, `src-tauri/tests/autorun_commands.rs`, `src/screens/AutoRun/index.test.tsx` (or `src/screens/AutoRun.test.tsx`)
- Generated: `src/bindings.ts`

**Owner request (2026-09-22):** in the dev version of Auto Run, a button to clear the scripts and one to clear the results that have already been executed.

**Interfaces:**
- `store::clear_scripts(root, case_ids: &[i32]) -> Result<usize, String>`: removes `scripts/case-<id>.json` for each id that exists; returns how many were removed; a missing file is not an error; any other I/O error is returned (the message names the first failure).
- `store::clear_runs(root) -> Result<usize, String>`: removes every `runs/*.json` and every file under `shots/`; returns the number of runs removed. Runs that were sent to Azure DevOps are removed too (the record in Azure DevOps is the durable one; the confirm says so).
- Commands: `auto_run_clear_scripts(case_ids: Vec<i32>) -> Result<usize, String>` and `auto_run_clear_runs() -> Result<usize, String>`; both refuse while `replay_is_running()` or while a supervised browser is open, with the sentences `auto_run_open_browser` and `auto_run_replay` already use (a run in progress reads scripts and writes runs).
- UI: two `Button size="sm" variant="outline"` in the toolbar after "Sign-in recipe": `<IconClearScripts aria-hidden /> Clear scripts` (disabled when the PBI's listed cases have no scripts) and `<IconClearResults aria-hidden /> Clear results` (disabled when there are no runs). Each opens the shared `Modal` confirm: `This removes the scripts of the N cases listed for this PBI from this machine. Nothing in Azure DevOps changes.` / `This removes every Auto Run result and picture on this machine, including runs already sent to Azure DevOps (those stay there). Nothing in Azure DevOps changes.` with Cancel and a danger-toned confirm button. On success: toast `N scripts removed` / `N runs removed`, invalidate the case list's script queries and `["autorun-runs"]`. Icons: two unused lucide glyphs in `src/lib/actionIcons.ts` that mean the ACTION (for example `FileX2 as IconClearScripts`, `Eraser as IconClearResults`); check they are not already aliased.

- [ ] **Step 1: Write the failing tests.** Rust: `clear_scripts` removes only the named ids and leaves others; a missing id is fine; the count is right; `clear_runs` removes runs and shots and returns the run count, a run with `published` set included; both on an empty root return 0. Frontend: the two buttons appear; each opens its confirm with the exact sentence; Cancel calls nothing; confirm calls the command and toasts the count; the buttons are disabled when there is nothing to clear.
- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run:** `cargo test --test autorun_store`, `cargo test --test autorun_commands`, `cargo test --test bindings` + line-ending check, `npx tsc --noEmit`, `npx vitest run src/screens/AutoRun src/screens/AutoRun.test.tsx src/ui-consistency.test.ts --exclude "**/.claude/**"`.
- [ ] **Step 5: Commit**

```bash
git add src-tauri/src src-tauri/tests src/bindings.ts src/screens/AutoRun src/lib/actionIcons.ts
git commit -q -F - <<'EOF'
feat(v2): Auto Run can clear a PBI's scripts and every result on this machine

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

## After execution

Hand checks, in a dev build: (1) upload a queue with one new and one updated case; the PBI chip stops glowing when the results appear. (2) View in Browser: the Options menu holds the four controls; bookmark a case, reload, it is still marked; Go to bookmark scrolls to it. (3) Show UI tour from Settings: Suite Management and the upload review each ring; the tour asks you to click the gear, waits, then rings the theme block, Backup and Updates. (4) Ask the assistant for the writing guide: the edge-case section and the quotes rule are there; a written set uses quotes. (5) With sqlcmd installed: lookup, SELECT, refused INSERT on read only, INSERT on the dev login, all logged without credentials; with sqlcmd absent: the install sentence.
