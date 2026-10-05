# Auto Run download checks Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Auto Run browsers keep their downloads, and a new `expect_download` check asserts a file's name and, for spreadsheets and text files, its headers, cells or text.

**Architecture:**
- **Capture:** the CDP driver turns downloads on and tracks them in its event handling (`browser/cdp.rs`), exposed through two new `Driver` methods.
- **Checks:** a pure module, `autorun/downloads.rs`, holds the matching, the reading and the sentences. It is testable without a browser.
- **The action:** `expect_download` is an `Action` variant carried out by the runner.
- **Where files are seen:** a download list on each step record feeds Past runs, the review, the report and Open.

**Tech Stack:** Rust (tokio, serde, CDP; the new crates `calamine` and `csv`), React 19 + TypeScript, vitest.

**Spec:** docs/superpowers/specs/2026-10-05-autorun-downloads-design.md

## Global Constraints

- Auto Run and API Templates are public behind Enable Advanced Features. The guide may name them; the changelog is end-user facing.
- Never name the secret unlock, the Extras card or the game.
- Rust tests only in `src-tauri/tests/suite/`: one binary, with a `mod` line in `suite/main.rs`. A test of process-wide state takes its lock from `suite/serial.rs`.
- Live tests are `#[ignore = "starts a real headless Edge"]`.
- Never hand-edit `src/bindings.ts`. Regenerate it with `cd src-tauri && cargo test --test bindings`, and restore line-ending-only drift.
- Colours from tokens, icons from `src/lib/actionIcons.ts`. `src/ui-consistency.test.ts` and `src/a11y.test.tsx` stay as they are.
- Plain sentences, no em dashes. No password, cookie, header, host or query string in any outcome, log line or event.
- A downloaded file is never sent to Azure DevOps.
- One test command at a time. Commit with a Bash heredoc ending `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Stage by name.

## Review Focus

1. **Name collisions.** Two downloads with the same suggested name in one run both survive, as `name.xlsx` and `name (2).xlsx`. Neither overwrites the other.
2. **A hostile suggested name** (`..\..\x.xlsx`, `C:\x`, `con.txt`, `a:b?.csv`) lands inside the run's download folder, under a sanitised name.
3. **A download that started before the step** (an earlier step's export) is never taken as this step's, even when it completes during this step.
4. **A CSV from a European locale** (semicolon-delimited, Windows-1252, with a BOM) reads its headers correctly.
5. **Open in Past runs** only opens files inside that run's own download folder. A crafted name with `..` is refused.

---

### Task 1: Downloads are captured

**Files:**
- `src-tauri/src/browser/cdp.rs`: enable downloads, track the events, rename on completion.
- A new `src-tauri/src/browser/downloads.rs`, for the pure parts:
  - `sanitise_name(&str) -> String`;
  - `unique_name(dir, name) -> String`;
  - `DownloadEntry`.
- `src-tauri/src/autorun/store.rs`:
  - `downloads_dir(root, run_id: &str) -> PathBuf` (`<root>/downloads/<run id>`);
  - `supervised_downloads_dir(root)` (`<root>/downloads/supervised`);
  - `clear_runs` also sweeps `downloads/`.
- Where each browser is launched or attached for a run (`autorun/replay.rs` per case, `commands/autorun.rs` for the supervised session): call the new driver method with the run's folder. The supervised folder is emptied when the session closes.
- Tests: `tests/suite/browser_downloads.rs` (new), `autorun_store.rs`, `browser_live.rs`.
- A new fixture page `tests/fixtures/autorun-download.html`: buttons that download a small csv and an xlsx from data URLs or blob links, and one that downloads twice under the same name.

**Interfaces:**
- Produces:
  - `pub struct DownloadEntry { pub guid: String, pub name: String, pub path: PathBuf, pub started_at: Instant, pub state: DownloadState, pub bytes: u64 }`
  - `pub enum DownloadState { InProgress, Completed, Canceled }`
  - On `Driver`, with defaults for fakes:
    - `fn enable_downloads(&mut self, dir: &Path) -> impl Future<Output = Result<(), CdpError>>`, which sends `Browser.setDownloadBehavior { behavior: "allowAndName", downloadPath, eventsEnabled: true }` (fall back to `Page.setDownloadBehavior` if the page target refuses it);
    - `fn downloads(&self) -> Vec<DownloadEntry>` (default empty).

  A fake driver can push entries for runner tests.

- [ ] **Step 1:** Write failing tests.
  - `sanitise_name`: path parts are stripped, `\ / : * ? " < > |` become `_`, Windows reserved names get a `_` prefix, an empty result becomes `download`, and the length is capped at 150 characters with the extension kept.
  - `unique_name`: ` (2)`, ` (3)` and so on, before the extension.
  - `downloads_dir` and `clear_runs` remove the folder.
  - Live: clicking the fixture's buttons gives two completed entries, the files exist under their sanitised names, and the double download keeps both (Review Focus 1 and 2).
- [ ] **Step 2:** Implement.
  - Events are handled where `Fetch` is (inside `on_event`): `Browser.downloadWillBegin` adds an entry, and `Browser.downloadProgress` updates its state and bytes.
  - On `completed`, the file the browser saved under its guid is renamed to `unique_name(dir, sanitise_name(suggested))`.
- [ ] **Step 3:** Run the focused tests, then the live module once (`--ignored --test-threads=1`), then `cd src-tauri && cargo test --tests`, one at a time. Commit `feat(v2): Auto Run browsers keep their downloads`.

### Task 2: The checks, without a browser

**Files:**
- A new `src-tauri/src/autorun/downloads.rs`.
- `src-tauri/Cargo.toml`: add `calamine` and `csv`, default features only.
- Fixtures under `tests/fixtures/downloads/`:
  - `template.xlsx` (sheet `Employees`; header `Employee No, Name, Department`; B2 `Employee Name`);
  - `legacy.xls`;
  - `comma.csv`;
  - `semicolon-1252-bom.csv`;
  - `errors.txt`.
- Tests: a new `tests/suite/autorun_downloads.rs`.

**Interfaces:**
- Produces, for Task 3:
  - `pub struct DownloadCheck { pub name: String, pub sheet: Option<String>, pub headers: Option<HeaderCheck>, pub cells: Vec<CellCheck>, pub contains_text: Vec<String> }`
  - `pub enum HeaderCheck { Exact(Vec<String>), Contains(Vec<String>) }`
  - `pub struct CellCheck { pub r#ref: String, pub text: String, pub contains: bool }`
  - `pub fn name_matches(pattern: &str, name: &str) -> bool`, case-insensitive, where `*` matches any run of characters.
  - `pub fn parse_a1(r: &str) -> Option<(u32, u32)>`, giving a zero-based row and column (`A1` is `(0, 0)`, `AA10` is `(9, 26)`).
  - `pub fn check_file(path: &Path, shown_name: &str, check: &DownloadCheck) -> Result<String, String>`. `Ok` carries the passed sentence; `Err` carries the failure sentence.
  - `pub const MAX_CHECK_BYTES: u64 = 50 * 1024 * 1024;`

- [ ] **Step 1:** Write failing tests for every outcome sentence in spec §2, verbatim, with the 200-character cap on quoted text. Cover:
  - `name_matches`;
  - `parse_a1`, including the invalid `1A`, `A0` and empty;
  - exact and contains headers on xlsx, xls and both csvs (Review Focus 4);
  - `cells` exact and contains;
  - a missing sheet listing the sheets it has;
  - `contains_text` on txt and csv;
  - a file over the cap (use a sparse or fake length);
  - an unreadable xlsx.
- [ ] **Step 2:** Implement. The CSV delimiter is the one of `,`, `;` and tab that appears most often on the first line. Decode UTF-8 (strip the BOM), else Windows-1252 (`encoding_rs` if already a dependency, else a 1252 table).
- [ ] **Step 3:** Run the focused tests, then `cargo test --tests`. Commit `feat(v2): Auto Run can read a downloaded spreadsheet, CSV or text file and say what it found`.

### Task 3: The `expect_download` action

**Files:**
- `src-tauri/src/browser/actions.rs`:
  - the variant `ExpectDownload { name: String, within_ms: Option<u32>, sheet: Option<String>, headers: Option<HeadersSpec>, cells: Vec<CellSpec>, contains_text: Vec<String> }`, where the serde shape is the spec's JSON (`headers: { exact | contains }`, `cells: [{ ref, text, match }]`);
  - validation with sentences naming the key;
  - `is_check` returns true.
- `src-tauri/src/autorun/runner.rs`: `run_step_routed` carries it out.
- `src-tauri/src/autorun/floor.rs`, and `src/screens/AutoRun/floor.ts` if it lists kinds: counts as a check.
- `src-tauri/src/autorun/guide.rs`: the action, its keys, and the 137540 and 137537 examples from spec §4.
- The script editor's kind list, if the editor validates kinds.
- `mcp.rs` and `ai_bridge.rs`, only if they list action kinds.
- Tests: `browser_actions.rs`, `autorun_runner.rs`, `autorun_guide.rs`, and the live module.

**Interfaces:**
- Consumes:
  - from Task 1: `Driver::downloads()`;
  - from Task 2: `check_file` and the check types.
- Produces: `StepRecord.downloads: Vec<String>`, the file names saved in that step, with `serde(default, skip_serializing_if = "Vec::is_empty")`. Task 4 uses it.

- [ ] **Step 1:** Write failing tests.
  - Validation:
    - refuses a missing `name`, `within_ms` over 120000 or 0, `sheet`/`headers`/`cells` on a name ending `.txt`, `contains_text` on `.xlsx`, a bad `ref` and empty `cells` or `contains_text` arrays;
    - accepts the spec's example.
  - Runner, with the fake driver:
    - a download started during the step and completed passes with the check sentence;
    - one that started before the step is ignored, and the step reports `no download started within <n> s` (Review Focus 3);
    - a canceled download;
    - a wrong name;
    - the files saved during the step are recorded in `StepRecord.downloads`.
  - The floor counts it, and the guide names it.
  - Live: the fixture's xlsx button, then `expect_download` with headers and a cell, passes; a wrong name fails; no click gives "no download started".
- [ ] **Step 2:** Implement. Note the step's start time. Poll `downloads()` every 100 ms within `within_ms` (default 15000). Take the first entry started after that time, wait for it to complete within the same budget, then call `check_file`.
- [ ] **Step 3:** Regenerate the bindings. Run the focused tests, the live module once, `cargo test --tests`, `npx tsc --noEmit` and `npm test`, one at a time. Commit `feat(v2): Auto Run scripts can check a downloaded file`.

### Task 4: Where downloads are seen

**Files:**
- `src-tauri/src/commands/autorun.rs`: a new command, `auto_run_open_download(run_id: String, name: String) -> Result<(), String>`.
  - It resolves `downloads_dir(root, run_id).join(name)`.
  - It refuses with `that file is not one of this run's downloads` unless the canonical path is inside the run's own folder and the name has no path parts (Review Focus 5).
  - It opens the file with the system default app, the way other files are opened (find the existing opener).
- `src-tauri/src/autorun/report.rs`: a `Downloads:` line per case (names and sizes, no link).
- `src-tauri/src/autorun/failures.rs`: `get_autorun_failures` shows names only (already true if it shows outcomes; add the step's download names).
- `src-tauri/src/autorun/publish.rs`: confirm that no download is attached, and add a test.
- `src/screens/AutoRun/RunReview.tsx` and `PastRuns.tsx`: under a case's steps, a Downloads list. Each item shows the name and the size (from a new run-file field, or read lazily with a small command; pick the smaller change) and an `Open` button with the accessible name `Open <name>`.
- Tests: Rust for the open-path refusal, the report line and publish; vitest for the list and Open.

- [ ] **Step 1:** Write the failing tests named above, including the `..` refusal.
- [ ] **Step 2:** Implement, and regenerate the bindings.
- [ ] **Step 3:** Run the focused tests, `cargo test --tests`, `npx tsc --noEmit` and `npm test`, one at a time. Commit `feat(v2): a run's downloads show in Past runs, the review and the report`.
