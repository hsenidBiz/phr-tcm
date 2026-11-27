# Discovery recordings and account changes - Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** While discovering, the assistant can record its checks as a video that is kept in the app and listed on a Recordings card. Every Auto Test video is recorded at the Auto Test page size. Separately, the writer and the case rewriter may move a saved script to another saved account by declaring it, and the healer may not.

**Architecture:** The recorder in `browser/recording.rs` reads its size from `viewport::current()`. A new store, `autorun/recordings.rs`, keeps recordings in their own folder under the environment root, with an index file. Failed-case videos never touch that folder. Two bridge routes start and stop a recording held on `DiscoveryState`. Every way a discovery ends stops and keeps that recording first, and a 10-minute timer stops it too. The account change is a new `Edit.account` flag checked in `edits::check_edits` and against the saved accounts in the save route. The MCP connector refuses the flag from a session that has been given the healer's rules.

**Tech Stack:** Rust (Tauri 2, tauri-specta, tokio), React 19 + TypeScript, TanStack Query, vitest.

**Spec:** `docs/superpowers/specs/2026-11-27-discovery-recordings-design.md`

## Global Constraints

- Recorder: `Page.startScreenRecording` / `Page.stopScreenRecording`, read back with `IO.read`. The browser needs Chromium (Edge) 153 or later.
- Every recording, failed-case videos included, uses `browser::viewport::current()` for `maxWidth` / `maxHeight`, instead of `recording::MAX_WIDTH` / `MAX_HEIGHT` (both consts are removed). `frameRate` stays 30 and `audio` stays false.
- A recording must never fail or slow discovery. Recorder errors are logged and mean "no video", as today.
- One recording at a time per discovery browser. A second start is refused with a sentence.
- A recording stops, and its video is kept, when the discovery ends or is stopped, and at a 10-minute cap (`600_000` ms).
- Titles are sanitised and at most 80 characters. Titles and tool replies name no URL or secret.
- Recordings are kept in the app only: no Azure DevOps attachment and no test-result upload. They are kept until removed and are never pruned with failed-case videos.
- The guide tells the assistant to record only when the prompt asks for evidence or a recording, or says a fix is being verified. It never records by default. It starts just before the checks that prove the point, never during sign-in or navigation, and stops right after them.
- The tools are always on in Basic mode, and the Recordings card shows in Basic mode too.
- Account change: allowed only when declared with `"account": true` and its `why`. An undeclared change is refused with a sentence that says how to declare it. A declared no-op is refused, as `area` is. The new key must be one of the environment's saved accounts. The connector refuses the declaration in a session given the healer's rules, before it reaches the app.
- Azure DevOps: GET / POST / PATCH only. No new DELETE (`tests/suite/ado.rs` scans for one).
- No hardcoded colours (`tests/ui-consistency.test.ts` passes unchanged). Icons come from `src/lib/actionIcons.ts`.
- `src/bindings.ts` is regenerated with `cd src-tauri && cargo test --test bindings`, never hand-edited.
- Rust tests live in `src-tauri/tests/suite/` only. A new module gets a `mod` line in `suite/main.rs`. A test that calls `store::set_root` takes `crate::serial::autorun()`.
- Never set `CARGO_TARGET_DIR` or `--target-dir`. Run one build or test command at a time.
- Commits use a Bash heredoc (`git commit -F - <<'EOF'`), ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Confirm each with `git log -1`.
- No `src/lib/changelog.ts` entry: the release adds it. Changelog lines are end-user facing.
- No em dashes in any text an assistant reads (the guide tests check for them).

## Review Focus

1. **The person ends the discovery while a recording runs** (End discovery on the Assistants card, or the assistant's release). Expect the video to be kept and listed. Test: `a_persons_end_discovery_keeps_the_recording` (Task 3).
2. **The browser is closed by hand mid-recording.** Expect it to be let go as today, with nothing listed, no panic, and one log line. Test: `a_gone_browser_loses_its_recording_quietly` (Task 3).
3. **The environment is switched between start and stop.** Expect the video never to be written into the newly active environment, and the write to be refused. Test: `a_recording_for_an_environment_no_longer_active_is_not_written` (Task 2).
4. **A title that is a URL, blank, multi-line or over 80 characters.** A URL or a blank title is refused. Whitespace and control characters are folded, and the title is cut at 80. Test: `titles_are_cleaned_and_bad_ones_refused` (Task 2).
5. **A corrupt index file.** Expect the next recording still to be kept: the bad file is moved aside, and the listing never panics. Test: `an_unreadable_index_is_set_aside_and_recording_goes_on` (Task 2).

---

### Task 1: Record every video at the Auto Test page size

**Files:**
- Modify: `src-tauri/src/browser/recording.rs` (consts ~35-38, `start_params` ~57, `start` ~108)
- Test: `src-tauri/tests/suite/autorun_video.rs` (`recording_is_asked_for_small_files` ~157), `src-tauri/tests/suite/page_size.rs` (`it_ships_as_1280_by_720_and_a_fresh_install_has_it` ~82, `a_saved_size_is_loaded_at_start_up_and_every_user_of_it_follows` ~169)

**Interfaces:**
- Produces:
  - `pub fn start_params_for(size: crate::browser::viewport::PageSize) -> serde_json::Value`, which gives `{ frameRate: 30, maxWidth: size.width, maxHeight: size.height, audio: false }`.
  - `pub fn start_params() -> Value`, which is `start_params_for(viewport::current())`. `start` keeps its signature and sends `start_params()`.
  - `MAX_WIDTH` and `MAX_HEIGHT` are removed. Update the module doc's "Kept small on purpose" comment to say the video is the page size in force.

- [ ] **Step 1: Write the failing tests**
  - In `autorun_video.rs`, replace `recording_is_asked_for_small_files` with `recording_is_asked_at_the_page_size`. It asserts `start_params_for(PageSize { width: 1920, height: 1080 })` gives `maxWidth` 1920, `maxHeight` 1080, `frameRate` 30 and `audio` false. It also asserts the result for `800 x 600` is 800 / 600.
  - In `page_size.rs`, `it_ships_...` asserts `recording::start_params()["maxWidth"] == 1280` and `["maxHeight"] == 720`. `a_saved_size_is_loaded_...` (it already runs in its own process) asserts `recording::start_params() == json!({ "frameRate": 30, "maxWidth": 1600, "maxHeight": 900, "audio": false })`.
- [ ] **Step 2:** Run `cd src-tauri && cargo test --test suite autorun_video::`. Expect a FAIL: `start_params_for` is not defined.
- [ ] **Step 3:** Implement as in Interfaces.
- [ ] **Step 4:** Run `cargo test --test suite autorun_video::`, then `cargo test --test suite page_size::`. Expect PASS.
- [ ] **Step 5:** Commit: `feat(v2): videos are recorded at the Auto Test page size`.

### Task 2: The recordings store and its index

**Files:**
- Create: `src-tauri/src/autorun/recordings.rs` (and add `pub mod recordings;` in `src-tauri/src/autorun/mod.rs`)
- Modify: `src-tauri/src/autorun/store.rs` (`load_video` ~1570, `video_size` ~1586: delegate recording names)
- Modify: `src-tauri/src/events.rs` (`AutorunDataKind` ~65: add `Recordings`)
- Modify: `src-tauri/src/commands/autorun.rs` (beside `auto_run_save_video` ~3137) and `src-tauri/src/lib.rs` (`collect_commands!` ~305)
- Regenerate: `src/bindings.ts`
- Test: `src-tauri/tests/suite/autorun_recordings.rs` (new; add `mod autorun_recordings;` to `suite/main.rs`)

**Interfaces:**
- Produces, in `autorun::recordings`:
  - `pub const CAP_MS: u64 = 600_000;`, `pub const CAP: std::time::Duration` (the same 10 minutes), and `pub const MAX_TITLE: usize = 80;`.
  - `#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, PartialEq)] pub struct RecordingEntry { pub id: String, pub title: String, pub created: String, pub duration_ms: u32, pub file: String }`.
    - `id` is `recording-<ms>-<seq>`. `file` is `id + ".mp4"`. `created` is `crate::applog::iso_stamp()`.
  - `pub fn recordings_dir(root: &Path) -> PathBuf`, which is `root.join("recordings")`. The index is `recordings/index.json`, shaped `{ "recordings": [RecordingEntry] }`.
  - `pub fn safe_recording_name(name: &str) -> bool`, which accepts only `recording-<digits>-<digits>.mp4`. It is written the way `store::safe_video_name` is, and its prefix keeps it apart from `video-...` names.
  - `pub fn clean_title(raw: &str) -> Result<String, String>`:
    - It folds every run of whitespace or control characters to one space, trims, and cuts to `MAX_TITLE` characters.
    - It refuses a blank title with `"a recording needs a short title, e.g. \"Bug 4521: leave balance fix\""`.
    - It refuses a title containing `://` or `www.` with `"a recording's title names no address"`.
  - `pub fn add(root: &Path, title: &str, duration_ms: u32, bytes: &[u8]) -> Result<RecordingEntry, String>`:
    - It holds `roots::guard(root)`, writes the file create-new (the way `store::save_video` does), then rewrites the index atomically (`roots::write_file_held`).
    - If the index write fails, it removes the file.
    - It refuses an empty video with `"the browser gave no video"`.
    - It calls `data_changed::notify(root, AutorunDataKind::Recordings)` after the write lands.
  - `pub fn list(root: &Path) -> Vec<RecordingEntry>` lists newest first and leaves out entries whose file is not a regular file.
    - An unreadable index is renamed to `index.unreadable-<ms>.json`, logged with `applog::warn`, and read as empty. `add` does the same before writing.
  - `pub fn remove(root: &Path, id: &str) -> Result<(), String>` deletes the file and the entry. An unknown id gives `Err("that recording is gone")`. It notifies `Recordings`.
  - `pub fn load(root: &Path, file: &str) -> Result<Vec<u8>, String>` and `pub fn size(root: &Path, file: &str) -> Option<u64>` check `safe_recording_name` and `store::MAX_VIDEO_BYTES`, as `store::load_video` / `video_size` do.
  - `#[derive(Debug)] pub struct Running { pub recording: crate::browser::recording::Recording, pub title: String, pub started_ms: u64, pub root: PathBuf }`, with `pub fn deadline_ms(&self) -> u64 { self.started_ms + CAP_MS }`. It is used in Task 3.
- `store::load_video` and `store::video_size` hand a `safe_recording_name` to `recordings::load` / `recordings::size`. The commands `auto_run_video`, `auto_run_video_info` and `auto_run_save_video` then serve recordings unchanged.
- New commands (`#[tauri::command(async)] #[specta::specta]`, root as the video commands take it):
  - `auto_run_recordings(app) -> Result<Vec<RecordingEntry>, String>`.
  - `auto_run_remove_recording(app, id: String) -> Result<(), String>`.

- [ ] **Step 1: Write the failing tests** in `autorun_recordings.rs`. Each takes `crate::serial::autorun()` and calls `set_root(tempdir)`.
  - `titles_are_cleaned_and_bad_ones_refused`:
    - `"  Bug 4521:\n leave\tbalance fix "` gives `"Bug 4521: leave balance fix"`.
    - 120 `x`s give 80.
    - `""` and `"   "` give the blank sentence.
    - `"see https://app/x"` and `"www.app.com"` give the address sentence.
  - `a_recording_is_kept_listed_newest_first_and_read_back`:
    - Two `add`s are listed in reverse order, with `safe_recording_name(&e.file)` true and `duration_ms` as given.
    - `store::load_video(root, &e.file)` returns the bytes, and `store::video_size` returns their length.
  - `removing_a_recording_deletes_its_file_and_entry`: after `remove`, the list is empty, the file is gone, and a second `remove` gives `"that recording is gone"`.
  - `recordings_are_never_pruned_or_cleared_with_failed_case_videos`: `add` one recording, then call `store::prune_videos_to(root, 0)` and `store::clear_runs(root)`. The recording is still listed and its file is still there.
  - `a_recording_name_is_never_a_failed_case_video_name`: `store::safe_video_name` is false for a recording file, and `safe_recording_name` is false for `video-1-1.mp4`, `../x.mp4` and `recording-1-1.mp4.exe`.
  - `an_unreadable_index_is_set_aside_and_recording_goes_on`: write `{not json` to `recordings/index.json`. `list` is empty, and an `add` is then listed. An `index.unreadable-*.json` exists.
  - `a_recording_for_an_environment_no_longer_active_is_not_written`: create two env roots and `set_root` to the first. `add` to the second gives `Err`, and its `recordings/` folder has no `.mp4`.
  - `an_empty_video_is_not_kept`: `add(root, "t", 1, &[])` gives `Err("the browser gave no video")`, and nothing is listed.
- [ ] **Step 2:** Run `cd src-tauri && cargo test --test suite autorun_recordings::`. Expect a FAIL: the module does not exist.
- [ ] **Step 3:** Implement the store, the delegation in `store.rs`, `AutorunDataKind::Recordings`, and the two commands, and register them in `lib.rs`.
- [ ] **Step 4:** Run `cargo test --test suite autorun_recordings::`, then `cargo test --test suite autorun_video::`. Expect PASS.
- [ ] **Step 5:** Run `cargo test --test bindings`, then `npx tsc --noEmit`. Expect PASS, and `src/bindings.ts` gains `autoRunRecordings`, `autoRunRemoveRecording`, `RecordingEntry` and `"recordings"` in `AutorunDataKind`. Add `recordings: ["autorun-recordings"]` to `DATA_KIND_KEYS` in `src/hooks/useAutoRunDataSync.ts` (the `Record` type needs it to compile).
- [ ] **Step 6:** Commit: `feat(v2): a recordings store kept apart from failed-case videos`.

### Task 3: Start and stop a recording in a discovery, with the safety stops

**Files:**
- Modify: `src-tauri/src/browser/recording.rs` (split `keep` ~178 into a read step and the write)
- Modify: `src-tauri/src/commands/autorun.rs` (`DiscoveryState` ~334, its construction ~701, `spawn_recording_cap`, the `.await` call sites ~1480, ~1544, ~1747, ~1751, ~2700)
- Create: `src-tauri/src/ai_bridge/discovery_recording.rs` (add `mod discovery_recording;` and `pub use self::discovery_recording::*;` in `ai_bridge/mod.rs`)
- Modify: `src-tauri/src/ai_bridge/discovery_end.rs` (`close_discovery`, `end_discovery_closing`, `close_browser_in` become `async`; `end_discovery_keeping` and each of those three call `finish_recording` first, beside `finish_mapping` / `finish_explore`)
- Modify: `src-tauri/src/ai_bridge/discovery_browser.rs` (`forget_gone_browser` ~67, `make_way_for_discovery` ~167)
- Modify: `src-tauri/src/ai_bridge/discovery.rs` (the `DiscoveryState` literal ~172)
- Modify: `src-tauri/src/ai_bridge/explore.rs` (~838: `.await`)
- Modify: `src-tauri/src/ai_bridge/mod.rs` (route arms beside `/autorun-discover-end` ~860)
- Modify: `src-tauri/src/commands/autorun_slots.rs` (`need_of` ~44: both routes are `Need::Act`)
- Modify: `src-tauri/src/assistant_activity.rs` (`route_action` ~99)
- Test: extend `src-tauri/tests/suite/autorun_recordings.rs`. Update `tests/suite/autorun_discovery.rs` (`opened_for_discovery` ~149 and `exploring` ~167 get `recording: None`; the 4 `close_browser_in` / close calls get `.await`), `autorun_explore.rs`, `autorun_kept.rs`, `autorun_slots.rs` (`.await`, plus the two routes in `need_of_names_every_browser_route`) and `assistant_activity.rs`.

**Interfaces:**
- Consumes: `recordings::{add, clean_title, Running, CAP, CAP_MS, RecordingEntry}` (Task 2), and `recording::{support, start, stop}`.
- Produces:
  - `recording::read_video<D: Driver>(d: &mut D, stopped: Stopped, who: &str) -> Option<Vec<u8>>`. It reads the stream, closes it, logs, and gives `None` for an empty video. `keep` becomes `read_video` plus `store::save_video`, with no change in behaviour (`autorun_video::` stays green).
  - `DiscoveryState { ..., pub recording: Option<crate::autorun::recordings::Running> }`.
  - In `ai_bridge::discovery_recording`, these constants (exact copy):
    - `RECORDING_NO_DISCOVERY = "No discovery is going: start one with start_autorun_discovery, then start the recording just before the checks."`
    - `RECORDING_NOT_AVAILABLE = "Recording is not available: this browser version cannot record. Carry on with the checks without a recording."`
    - `ALREADY_RECORDING = "A recording is already going in this browser: stop it with stop_autorun_recording before you start another."`
    - `NOT_RECORDING = "No recording is going in this browser."`
    - `RECORDING_NOT_STARTED = "The recording could not be started; carry on with the checks without one."`
    - `RECORDING_LOST = "The recording stopped, but the browser gave no video, so none was kept; the checks are not affected."`
  - `pub fn recording_started(title: &str) -> String` gives `Recording "<title>" started: stop it with stop_autorun_recording right after the checks. It stops by itself when the discovery ends or after 10 minutes.`
  - `pub fn recording_kept(title: &str, duration_ms: u32) -> String` gives `Recording kept: "<title>" (<m:ss>). It is ready in the app under Auto Test → Recordings, on the Past runs tab.`
  - `pub async fn recording_start_in<B: DiscoveryBrowser>(slot: &mut Option<B>, root: &Path, body: &str, now_ms: u64) -> (u16, String)`:
    - 400 for a bad body or a bad title (the `clean_title` sentence).
    - 409 with `RECORDING_NO_DISCOVERY` when there is no slot, no `discovery`, or a discovery not yet signed in (`account` is `None`).
    - 409 with `ALREADY_RECORDING`.
    - 409 with `RECORDING_NOT_AVAILABLE` when `support` is `No`.
    - 409 with `RECORDING_NOT_STARTED` when `start` gives `None`.
    - Otherwise it stores `Running` and answers 200 with `recording_started`.
  - `pub async fn recording_stop_in<B: DiscoveryBrowser>(slot: &mut Option<B>, now_ms: u64) -> (u16, String)` gives 409 with `NOT_RECORDING`, or 200 with `recording_kept(..)`, or 200 with `RECORDING_LOST`.
  - `pub async fn finish_recording<B: DiscoveryBrowser>(slot: &mut Option<B>, now_ms: u64) -> Option<RecordingEntry>`. It is the safety stop: it takes `Running`, stops, reads and calls `recordings::add(&running.root, ..)`, and logs every failure. `duration_ms = now_ms - started_ms`, capped at `CAP_MS`.
  - `pub async fn stop_recording_over_cap_in<B: DiscoveryBrowser>(slot: &mut Option<B>, now_ms: u64) -> Option<RecordingEntry>` calls `finish_recording` only when `now_ms >= running.deadline_ms()`.
  - `forget_gone_browser`: when the dying browser's discovery has a recording, it logs `"Auto Test recording: lost, the browser had gone"`, sends no call, and keeps nothing.
  - `commands::autorun::spawn_recording_cap(n: SlotNo)` uses `tauri::async_runtime::spawn`. It sleeps `recordings::CAP`, locks `session_slot(n)`, and calls `stop_recording_over_cap_in(&mut slot, now_ms())`. The deadline check means a later recording is never stopped early.
  - Routes in `ai_bridge/mod.rs`, both behind `replay_is_holding_the_browser(slot)` and `autorun_root()`, locking `session_slot(slot)` as `/autorun-explore-change` does:
    - `("POST", "/autorun-recording-start")` calls `recording_start_in`. On 200 it also calls `spawn_recording_cap(slot)`.
    - `("POST", "/autorun-recording-stop")` calls `recording_stop_in`.
  - `assistant_activity::route_action`: `/autorun-recording-start` gives `(Tried, "Started a recording")`, and `/autorun-recording-stop` gives `(Saved, "Kept a recording")`.

- [ ] **Step 1: Write the failing tests** in `autorun_recordings.rs`:
  - Use the fake slot from `crate::autorun_discovery::{slot, exploring, opened_for_discovery}`.
  - Use a recorder driver built like `autorun_video.rs`'s `driver_on`, with a `ScriptedDriver` answering `Browser.getVersion`, the start and stop calls, `IO.read` (one base64 MP4 chunk, then eof) and `IO.close`.
  - Tests:
    - `a_discovery_records_its_checks_and_keeps_the_video`: start gives 200 containing `Recording "Bug 4521: leave balance fix" started`. Stop at `now + 83_000` gives 200 equal to `recording_kept("Bug 4521: leave balance fix", 83_000)` and contains `(1:23)`. The entry is listed, and its bytes equal the MP4.
    - `a_second_start_is_refused_while_one_is_going`: 409 with `ALREADY_RECORDING`, and only one `Page.startScreenRecording` is sent.
    - `no_recording_without_a_signed_in_discovery`: an empty slot, `opened_for_discovery()` and a person's browser (`discovery: None`) all give 409 with `RECORDING_NO_DISCOVERY`.
    - `an_old_browser_says_recording_is_not_available`: product `Edg/152.0.1.1` gives 409 with `RECORDING_NOT_AVAILABLE`, no start is sent, and the discovery is still `Some`.
    - `stopping_with_none_going_says_so`: 409 with `NOT_RECORDING`.
    - `a_stop_the_browser_refuses_keeps_nothing_and_says_so`: when `Page.stopScreenRecording` errors, the answer is 200 with `RECORDING_LOST` and nothing is listed.
    - `ending_the_discovery_keeps_the_recording`: `end_discovery_in(&mut slot).await` and `end_discovery_releasing` each leave one entry.
    - `a_persons_end_discovery_keeps_the_recording`: `end_discovery_as(&mut slot, EndedBy::Person).await` leaves one entry, and the slot is closed.
    - `closing_the_browser_keeps_the_recording`: `close_browser_in(&mut slot).await` leaves one entry.
    - `a_gone_browser_loses_its_recording_quietly`: with `gone` set, `forget_gone_browser` is true, nothing is listed, and no `Page.stopScreenRecording` is sent.
    - `the_cap_stops_a_recording_at_ten_minutes`: `stop_recording_over_cap_in` at `start + 599_999` gives `None` and keeps recording. At `start + 600_000` it gives an entry with `duration_ms == 600_000`.
    - `the_cap_never_stops_a_newer_recording_early`: stop, then start again at `t2`. A cap check at `first start + 600_000` gives `None`.
    - `the_recording_routes_act_on_the_browser`: `need_of("POST", "/autorun-recording-start")` and `need_of("POST", "/autorun-recording-stop")` are `Some(Need::Act)`.
  - In `assistant_activity.rs`, `the_table_names_writes_only` asserts the two new rows.
- [ ] **Step 2:** Run `cd src-tauri && cargo test --test suite autorun_recordings::`. Expect a FAIL: `recording_start_in` is not defined.
- [ ] **Step 3:** Implement as in Interfaces. Making the three close functions async also changes their call sites: add `.await` at each one listed under Files.
- [ ] **Step 4:** Run, one at a time: `cargo test --test suite autorun_recordings::`, `autorun_discovery::`, `autorun_explore::`, `autorun_kept::`, `autorun_slots::`, `assistant_activity::`, `autorun_video::`. Expect PASS.
- [ ] **Step 5:** Commit: `feat(v2): a discovery can record its checks, kept when it ends`.

### Task 4: The assistant's two recording tools and the guide

**Files:**
- Modify: `src-tauri/src/mcp.rs` (the tool list after `end_autorun_discovery` ~899; the dispatch beside `"end_autorun_discovery"` ~2888)
- Modify: `src-tauri/src/ai_tools.rs` (`AUTORUN_TOOLS` ~162, after `"end_autorun_discovery"`; `CLAUDE_AUTORUN_RULES` ~1821 and `CURSOR_AUTORUN_ENTRIES` ~1848)
- Modify: `src/lib/mcpTools.ts` (the tool entries ~97 and the `AUTORUN_TOOLS` mirror ~394)
- Modify: `src-tauri/src/autorun/guide.rs` (a new `### Recording your checks` inside `## Discovering the app`, before `### What the application refuses` ~2214)
- Test: `src-tauri/tests/suite/tcm_mcp.rs`, `src-tauri/tests/suite/ai_tools.rs` (`each_tool_belongs_to_its_module` ~557), `src-tauri/tests/suite/autorun_guide.rs`, `tests/lib/mcpTools.test.ts`

**Interfaces:**
- Consumes: the routes `/autorun-recording-start` and `/autorun-recording-stop` (Task 3).
- Produces:
  - `start_autorun_recording`, with schema `{ title: string }` (required `["title"]`). It dispatches as `call("POST", "/autorun-recording-start", &args.to_string())`.
  - `stop_autorun_recording`, with schema `{}`. It dispatches as `call("POST", "/autorun-recording-stop", &args.to_string())`.
  - Neither tool is in `SIMPLE_HIDDEN_TOOLS`, so both stay on in Basic mode. Neither starts a role (`roles::role_for_call` is unchanged).
  - The `start_autorun_recording` description must say:
    - only when the person's prompt asks for evidence or a recording, or says a fix is being verified, never by default;
    - start just before the checks that prove the point, never during sign-in or navigation, and stop right after them;
    - `title` is short, e.g. "Bug 4521: leave balance fix", at most 80 characters, with no address or secret;
    - one recording at a time;
    - it stops by itself when the discovery ends or after 10 minutes;
    - the video stays in the app.
  - The `stop_autorun_recording` description must say that it keeps the video and that its answer says it is ready under Auto Test → Recordings.
  - The guide section, as copy:

    ```
    ### Recording your checks

    Record only when the person's prompt asks for evidence or a recording, or says
    a fix is being verified. Never record by default. With `start_autorun_recording`
    and a short `title` ("Bug 4521: leave balance fix"), start just before the checks
    that prove the point, never during the sign-in or the trip to the screen, and
    call `stop_autorun_recording` right after them. One recording at a time; it stops
    by itself when your discovery ends or after 10 minutes. The video stays in the
    app: tell the person it is under Auto Test → Recordings, on the Past runs tab.
    A browser that cannot record says so: carry on without one.
    ```

  - `mcpTools.ts` entries (labels and summaries):
    - `start_autorun_recording` has the label "Start a recording" and the summary "Record the assistant's checks in its discovery browser, when you asked for video evidence."
    - `stop_autorun_recording` has the label "Stop the recording" and the summary "Stop the recording and keep its video under Auto Test, Recordings."

- [ ] **Step 1: Write the failing tests**
  - `tcm_mcp.rs`: `the_recording_tools_ride_with_the_auto_run_tools_and_reach_their_routes` calls `rides_with_the_auto_run_tools` with:
    - `("start_autorun_recording", "/autorun-recording-start", json!({ "title": "Bug 4521: leave balance fix" }), json!(["title"]))`
    - `("stop_autorun_recording", "/autorun-recording-stop", json!({}), json!([]))`
  - `tcm_mcp.rs`: `the_recording_tools_say_when_to_record`. Both descriptions contain no `\u{2014}`. The start description contains "never by default", "10 minutes" and "80 characters".
  - `ai_tools.rs`: the `AUTORUN_TOOLS` pin gains both names after `end_autorun_discovery`. Add `the_recording_tools_stay_on_in_basic_mode`, asserting `!SIMPLE_HIDDEN_TOOLS.contains(..)` for both, and that `CLAUDE_AUTORUN_RULES` has `mcp__tcm__start_autorun_recording` and `mcp__tcm__stop_autorun_recording`.
  - `autorun_guide.rs`: `the_guide_says_when_to_record`. The `Discovering the app` part contains `### Recording your checks`, "Never record by default", "never during the sign-in" and "Auto Test → Recordings", and has no em dash.
  - `tests/lib/mcpTools.test.ts`: `summary("start_autorun_recording")` and `summary("stop_autorun_recording")` equal the strings above. The existing Rust-mirror comparison then covers `AUTORUN_TOOLS`.
- [ ] **Step 2:** Run `cd src-tauri && cargo test --test suite tcm_mcp::`. Expect a FAIL: the tools are unknown.
- [ ] **Step 3:** Implement as in Interfaces.
- [ ] **Step 4:** Run, one at a time: `cargo test --test suite tcm_mcp::`, `ai_tools::`, `autorun_guide::`, then `npx vitest run tests/lib/mcpTools.test.ts`. Expect PASS.
- [ ] **Step 5:** Commit: `feat(v2): start_autorun_recording and stop_autorun_recording for the assistant`.

### Task 5: The Recordings card

**Files:**
- Create: `src/screens/AutoRun/useVideoFile.ts` (the play and save logic lifted out of `CaseVideo.tsx`)
- Modify: `src/screens/AutoRun/CaseVideo.tsx` (use the hook, with no visible change)
- Create: `src/screens/AutoRun/RecordingsCard.tsx`
- Modify: `src/screens/AutoRun/index.tsx` (the Past runs panel ~1161, above `<PastRuns>`; and the no-scope branch ~824, after `<AssistantsCard />`)
- Test: `tests/screens/AutoRun/RecordingsCard.test.tsx` (new). `tests/screens/AutoRun/CaseVideo.test.tsx` must pass unchanged.

**Interfaces:**
- Consumes: `commands.autoRunRecordings`, `autoRunRemoveRecording`, `autoRunVideo`, `autoRunVideoInfo`, `autoRunSaveVideo` and `RecordingEntry` (Task 2), and the `recordings` data kind (Task 2).
- Produces:
  - `export function useVideoFile(name: string | null | undefined, saveAs?: string)`. It returns `{ info, src, close, play, save }`, with the same queries and mutations `CaseVideo` has today (`videoInfoKey`, `pickSavePath(saveAs ?? name)`, the same toasts).
  - `export const RECORDINGS_KEY = ["autorun-recordings"] as const;`. It is a plain `useQuery` of a local file list, as `PastRuns`' `["autorun-runs"]` is. It needs no `persistentQuery` cache. It is environment-scoped by its `autorun-` prefix (`isEnvironmentScopedKey`).
  - `export const REMOVE_CONFIRM = "Remove this recording? It cannot be undone.";`
  - `export default function RecordingsCard()`. It draws nothing while loading, after an error, or with no recordings. Otherwise it shows a section labelled by an `h3` "Recordings" (the `AssistantsCard` heading style).
    - Each row shows the title, `new Date(created).toLocaleString()`, the length as `m:ss`, and three buttons:
      - "Play" (`IconPlayVideo`, `aria-label` `Play <title>`);
      - "Save" (`IconExport`, `aria-label` `Save <title>`; default file name `<title with characters not allowed in a file name removed>.mp4`);
      - "Remove" (`IconRemove`, `aria-label` `Remove <title>`).
    - Remove opens an inline confirm with `REMOVE_CONFIRM`, "Remove recording" and "Keep". After removal it invalidates `RECORDINGS_KEY`.
    - Play shows `<video aria-label="Recording: <title>">` as `CaseVideo` does.
  - It is not gated on `simple`: it shows in Basic mode.

- [ ] **Step 1: Write the failing tests** in `RecordingsCard.test.tsx`, using `mockIPC` as `CaseVideo.test.tsx` does:
  - `nothing shows while there are no recordings`: with `autoRunRecordings` returning `[]`, no "Recordings" heading appears.
  - `each recording shows its title, time and length`: one entry `{ id: "recording-1764238500000-000001", title: "Bug 4521: leave balance fix", created: "2026-11-27T10:15:00Z", duration_ms: 83000, file: "recording-1764238500000-000001.mp4" }`. The heading "Recordings", the title and `1:23` are shown.
  - `Play shows the recording in the app`: `auto_run_video` is asked with that file, and `getByLabelText("Recording: Bug 4521: leave balance fix")` exists.
  - `Save asks where and copies the file there`: `dialog.save` gets the default path `Bug 4521 leave balance fix.mp4`, and `auto_run_save_video` is called with the file and the chosen path.
  - `Remove asks first and then removes it`: clicking `Remove Bug 4521...` shows `REMOVE_CONFIRM`. "Keep" sends nothing. "Remove recording" calls `auto_run_remove_recording` with the id and asks `auto_run_recordings` again.
- [ ] **Step 2:** Run `npx vitest run tests/screens/AutoRun/RecordingsCard.test.tsx`. Expect a FAIL: the module is not found.
- [ ] **Step 3:** Implement the hook, refactor `CaseVideo`, add the card, and place it in `index.tsx` in both places.
- [ ] **Step 4:** Run `npx vitest run tests/screens/AutoRun/RecordingsCard.test.tsx tests/screens/AutoRun/CaseVideo.test.tsx tests/ui-consistency.test.ts tests/hooks/useAutoRunDataSync.test.tsx`, then `npx tsc --noEmit`. Expect PASS.
- [ ] **Step 5:** Commit: `feat(v2): a Recordings card on Auto Test to play, save and remove recordings`.

### Task 6: A declared account change in the edit gate and the save

**Files:**
- Modify: `src-tauri/src/autorun/edits.rs` (`Edit` ~17; rule 7 ~291; rule 5 ~311)
- Modify: `src-tauri/src/ai_bridge/autorun_save.rs` (`bad_edits` ~26; the repair branch after `check_edits` ~585)
- Modify: `src-tauri/src/mcp.rs` (the `edits` schema description ~752: add `account? (true when you changed the account the script runs as: a saved account's key)`)
- Test: `src-tauri/tests/suite/autorun_edits.rs`, `src-tauri/tests/suite/autorun_bridge.rs`. Add `account: false` to every full `Edit { .. }` literal: `autorun_edits.rs:17` and `:527`, `ai_bridge.rs:4853` and `:4891`, `components.rs:963`.

**Interfaces:**
- Produces:
  - `Edit { ..., #[serde(default)] pub account: bool }`, documented like `area`.
  - Rule 7: `precondition_only` also needs `!e.account`.
  - Rule 5 is replaced by these checks:
    - Changed and not declared:
      `format!("case {}: the account changed from {} to {} but was not declared - add \"account\": true with its \"why\" to the case's \"edits\" entry, or send the account as saved", id, say(old), say(new))`, where `say` gives `"\"key\""`, or `the case's own (none)` for `None`.
    - Declared and unchanged: `pub const ACCOUNT_DECLARED_UNCHANGED: &str = "the account was declared but not changed";`
  - `pub fn check_new_account(case_id: i32, new: Option<&str>, saved_keys: &[String]) -> Result<(), String>` is pure:
    - `None` or blank gives `format!("case {case_id}: a declared account change needs the key of a saved account (get_accounts lists them)")`.
    - A key not in `saved_keys` gives `format!("case {case_id}: there is no saved account \"{key}\" in this environment - use a key get_accounts lists")`.
  - The save route checks the new account only on the repair branch. When `declared.is_some_and(|e| e.account)` after `check_edits` is Ok, it reads `accounts::load_accounts(&root)` (once per save; an unreadable file refuses with its error), then calls `check_new_account`. A refusal joins `refused`.
    - When not a dry run, it logs `applog::info("AI changed case N's account from X to Y: <why>")`. The `why` is kept as `last_repair`, as for other declared edits.

- [ ] **Step 1: Write the failing tests**
  - In `autorun_edits.rs`, replace `the_account_a_script_runs_as_cannot_be_changed_by_a_repair` with:
    - `an_undeclared_account_change_is_refused_with_how_to_declare_it`: `tester1` to `tester2` with no edit gives exactly `case 1: the account changed from "tester1" to "tester2" but was not declared - add "account": true with its "why" to the case's "edits" entry, or send the account as saved`.
    - `a_declared_account_change_passes_the_gate`: `Edit { account: true, ..edit(&[], "case 1 needs the approver's login") }` gives `Ok(())`.
    - `a_declared_account_change_that_changes_nothing_is_refused`: the same account gives `ACCOUNT_DECLARED_UNCHANGED`.
    - `an_account_only_edit_still_needs_a_reason`: `account: true` with a blank `why` gives `"an edit needs a reason"`.
    - `the_new_account_must_be_a_saved_one`: `check_new_account(1, Some("ghost"), &["hr_admin".into()])` gives the no-saved-account sentence. `None` gives the needs-a-key sentence. `Some("hr_admin")` gives `Ok`.
  - In `autorun_bridge.rs` (pattern: `a_resend_that_only_changes_the_area_is_a_repair` ~3260), save the accounts `hr_admin` and `hr_user` with `save_accounts`, and set `sc[0]["account"]` on `case_7`:
    - `a_declared_account_change_by_a_writer_or_rewriter_is_saved`: first save as `hr_admin` gives 200. A resend as `hr_user` with no edits gives 400 containing `the account changed from "hr_admin" to "hr_user" but was not declared`, and the disk is still `hr_admin`. With `edits: [{ case_id: 7, steps: [], account: true, why: "the case is the approver's" }]` it gives 200 whose first line is `saved 1 script(s): case 7 (repaired)`. The disk account is `hr_user`, `repairs == 1`, and `last_repair == Some("the case is the approver's")`.
    - `a_declared_change_to_an_unsaved_account_is_refused`: `ghost` gives 400 containing `there is no saved account "ghost"`, and the disk is unchanged.
    - `a_declared_account_change_that_changes_nothing_is_refused_by_the_save`: 400 containing `the account was declared but not changed`.
- [ ] **Step 2:** Run `cd src-tauri && cargo test --test suite autorun_edits::`. Expect a FAIL: there is no field `account`.
- [ ] **Step 3:** Implement as in Interfaces, and update the struct literals listed under Files.
- [ ] **Step 4:** Run, one at a time: `cargo test --test suite autorun_edits::`, `autorun_bridge::`, `ai_bridge::`, `components::`, `autorun_forbidden_fixes::`. Expect PASS.
- [ ] **Step 5:** Commit: `feat(v2): a repair may move a script to another saved account when it declares it`.

### Task 7: The connector keeps the healer out; role and guide text

**Files:**
- Modify: `src-tauri/src/autorun/roles.rs` (beside `cases_repaired_by` ~265: a shared edits walker and `declares_account_change`; the new sentence)
- Modify: `src-tauri/src/mcp_session.rs` (`SessionBook::delivered`)
- Modify: `src-tauri/src/mcp.rs` (`RoleNotes` ~283: `fn delivered(&mut self, agent: Option<&str>, role: Role) -> bool` for both impls; `answer_with`'s `tools/call` arm ~387, before `tools_call_outcome`)
- Modify: `src-tauri/src/autorun/roles/writer.md`, `case-rewriter.md` (line 8), `healer.md`
- Modify: `src-tauri/src/autorun/guide.rs` (the refusal list ~3041 and the paragraph before it ~3031)
- Test: `src-tauri/tests/suite/mcp_session.rs`, `src-tauri/tests/suite/autorun_roles.rs`, `src-tauri/tests/suite/autorun_guide.rs` (`the_guide_lists_every_refusal_the_edit_gate_can_give` ~442)

**Interfaces:**
- Consumes: `Edit.account`, the gate's sentences and `ACCOUNT_DECLARED_UNCHANGED` (Task 6).
- Produces:
  - `pub fn declares_account_change(args: &serde_json::Value) -> bool` in `autorun::roles`. It walks the same shapes `cases_repaired_by` does: `edits` beside `scripts`, a `{ scripts, edits }` bundle sent as `scripts`, and `edits` nested in a script, each possibly JSON text. It is true when any entry's `account` is true or `"true"`. Refactor `collect_repairs` into one walker over entries that both functions use.
  - `pub const HEALER_KEEPS_ACCOUNT: &str = "A healer never changes the account a script runs as: send the account as saved and leave \"account\" out of the edit. If the case needs another account, tell the person; the account is a person's or a writer's call.";` (in `autorun::roles`).
  - `SessionBook::delivered(&self, agent: Option<&str>, role: Role) -> bool`.
  - In `answer_with`: when `name == "save_autorun_script" && roles::declares_account_change(&args) && session.delivered(agent, Role::Healer)`, the result is `{ content: [{ type: "text", text: HEALER_KEEPS_ACCOUNT }], isError: true }`. Nothing reaches the app and no role note is added. An untracked `Session` never refuses, because `delivered` is false.
  - Role text. This is a hard limit: rules plus tool note are capped at `ROLE_MAX_CHARS` 4000 and `ROLE_MAX_LINES` 45 (`autorun_roles::` and `autorun_pw_rules::` tests).
    - Today `writer` is at 3996 chars and 27 lines, `healer` at 3998 chars and 45 lines, and `case-rewriter` at 3594 chars and 43 lines.
    - Fit each change by tightening wording in the same file without dropping a phrase any test pins. Never raise the caps. If it cannot fit, stop and report.
    - `writer.md` extends the "Never invent an account..." bullet with: ``A saved script moves to another saved account only with `"account": true` and its why in the case's `edits` entry.``
    - `case-rewriter.md` replaces "A repair never changes the account a script runs as." with: ``To move a script to another saved account, declare `"account": true` with its why; a healer never does.``
    - `healer.md` changes the "How a repair is made" bullet, in place with no new line, to: `- Repair by the case rewriter's rules (with your first save), and never change the account.`
  - Guide:
    - The paragraph before the list gains: ``When the repair changes the account the script runs as (a writer's or a case rewriter's call, never a healer's), the entry says `"account": true` and the account is a saved key.``
    - The list's account line becomes three lines: the undeclared sentence (`the account changed from ... but was not declared`), `the account was declared but not changed`, and `there is no saved account "..." in this environment`.

- [ ] **Step 1: Write the failing tests**
  - In `mcp_session.rs`, with a `repair_account_args()` of `{ scripts: [{ case_id: 7, title: "t", account: "hr_user", steps: [] }], edits: [{ case_id: 7, steps: [], account: true, why: "w" }] }`:
    - `a_healer_session_cannot_declare_an_account_change`: `Session::new()`, tick, `failures(..)`, then `send(.., "save_autorun_script", repair_account_args())`. The result has `isError == true`, `texts == [HEALER_KEEPS_ACCOUNT]`, and `fake.calls` has no `/autorun-script`.
    - `a_writer_session_may_declare_an_account_change`: a `start_autorun_discovery` first. The save reaches `/autorun-script`.
    - `a_healer_may_still_save_a_repair_that_keeps_the_account`: after `failures`, `repair_args()` (no account) reaches `/autorun-script`.
    - `only_the_healer_agent_is_refused_in_a_shared_book`: as `each_agent_name_gets_the_role_rules_once` does, through `handle_message_shared` with `agent` "healer-1" (given `get_autorun_failures`) and "writer-1". Only healer-1's save is refused.
  - In `autorun_roles.rs`, `an_account_declaration_is_found_in_every_shape`:
    - true for edits beside scripts, for edits as JSON text, inside a bundle sent as `scripts`, nested in a script, and with `"account": "true"`;
    - false for `account: false`, for no `account`, and for `{}`.
  - Also in `autorun_roles.rs`, `the_roles_say_who_may_change_the_account`: flat `writer` text contains `"account": true`, `case-rewriter` contains `a healer never does`, and `healer` contains `never change the account`.
  - In `autorun_guide.rs`, the refusal pin replaces `"the account a script runs as cannot be changed by a repair"` with `"but was not declared - add \"account\": true"`, `"the account was declared but not changed"` and `"there is no saved account"`.
- [ ] **Step 2:** Run `cd src-tauri && cargo test --test suite mcp_session::`. Expect a FAIL: `HEALER_KEEPS_ACCOUNT` is not defined.
- [ ] **Step 3:** Implement as in Interfaces.
- [ ] **Step 4:** Run, one at a time: `cargo test --test suite mcp_session::`, `autorun_roles::`, `autorun_pw_rules::`, `autorun_guide::`, `tcm_mcp::`. Expect PASS.
- [ ] **Step 5:** Commit: `feat(v2): the connector keeps the healer from changing a script's account`.

### Task 8: How To Use, the backlog, and the full suites

**Files:**
- Modify: `docs-site/src/content/auto-run.ts` (a new item in the `runs` group, with `shot: RUNS`, after the Past runs items)
- Modify: `src/dev/demo.ts` (~1875: `autoRunRecordings` gives one sample recording; `autoRunVideoInfo` answers for its file)
- Modify: `docs/backlog.html` (a "Built" line with the hand checks owed)
- Regenerate (via the CLAUDE.md docs flow): `docs-site/shots/{light,dark}/auto-run-past-runs.jpg`, `docs-site/shots/positions.json`, `src-tauri/help/`

**Interfaces:**
- Consumes: `RecordingsCard`, which renders on the Past runs tab (Task 5).
- Shots that show the card: `auto-run-past-runs` (`RUNS`). Check `auto-run-review` (`REVIEW`, which starts on the same tab) in the dry run. No top-of-screen shot changes, because the card is not on the Test cases tab.
- Docs item: `{ id: "recordings-card", shot: RUNS, group: "runs", locate: { role: "heading", name: "Recordings" }, name: "Recordings", does: <one sentence> }`. The sentence: "Videos an assistant recorded of its checks while discovering, when you asked it for evidence, each with its title, date and length: **Play** watches one here, **Save** copies it where you choose, and **Remove** deletes it from this computer."
  - Name no hidden feature (`docs-site/src/guard.test.ts`).

- [ ] **Step 1:** Add the demo recording and the docs item. Run `npx vitest run docs-site/src/content.test.ts docs-site/src/guard.test.ts`. Expect PASS, or a stale-shot failure that Step 2 clears.
- [ ] **Step 2:** Run the docs flow from CLAUDE.md:
  - Close `tauri dev`, then run `npm run docs:dev`, signed out.
  - Run `npm run docs:shots -- --dry-run`, then `npm run docs:shots -- --only auto-run-past-runs`, adding `auto-run-review` if the dry run lists it.
  - Run `npm run docs:build`.
  - Expect `npx vitest run docs-site/src/built.test.ts docs-site/src/help-site.test.ts` to PASS.
- [ ] **Step 3:** Add to `docs/backlog.html` a "Built 27 November 2026" line: discovery recordings, page-size videos, and declared account changes. The hand checks owed:
  - a recording on a real discovery shows the step highlight box and label, and no secret in a label;
  - a recording at a 1920 x 1080 page;
  - End discovery while recording keeps the video;
  - Play, Save and Remove on the card;
  - a writer's declared account change in the script editor.
- [ ] **Step 4:** Run the full suites, one at a time:
  - `cd src-tauri && cargo test --tests` (the whole `suite` binary plus `bindings`);
  - `git diff --exit-code src/bindings.ts` (the bindings are fresh);
  - `npm test`;
  - `npx tsc --noEmit`;
  - `npm run build`.
  - Expect all to PASS. One `App.test.tsx` failure that passes on a re-run is the documented flake.
- [ ] **Step 5:** Commit the shots, positions, built site, demo, docs content and backlog together: `docs(v2): How To Use and backlog for discovery recordings`.
