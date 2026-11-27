# Video Player with Step Markers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Failed-case videos and discovery recordings open in a full player over the app, and in the run report, with step ticks, a red failed stretch, a step list that opens into actions, and keys. Every marker comes from a recorded start time.

**Architecture:** During a recorded case, the runner reads the clock once per step and once per action against the recorder's own start. It stores those offsets on the case record as `CaseRecord::video_marks`. A discovery recording stores one mark per tried action on its index entry (`RecordingEntry::marks`). Labels are the step highlight's words: `browser::highlight::Say`, or the label the highlight actually drew when it drew one. The app draws the player in React inside the existing `Modal`. The report draws the same player from its own script (`web/autorun-report-player.js`, allowed by hash) and its own tokens. It reads the timeline JSON that Rust puts in the page's data blob.

**Tech Stack:** Rust (Tauri 2, serde, specta/tauri-specta), React 19 + TypeScript, TanStack Query, Tailwind tokens, vitest + Testing Library (jsdom), the report's plain ES5 page script.

**Spec:** `docs/superpowers/specs/2026-11-27-video-player-design.md` (including "Added 27 November 2026": the report player).

## Global Constraints

- The new timing fields deserialize with defaults, so old records still load. Each one is `#[serde(default, skip_serializing_if = ...)]`, so a record with no marks writes no new key.
- No hardcoded colours: tokens only. The ui-consistency gate (`tests/ui-consistency.test.ts`) passes. Icons come from `src/lib/actionIcons.ts`.
- Report colours come only from the report's token variables (`web/autorun-report-tokens.css`). The accent comes from `PagePalette`. `tests/suite/autorun_report_smart.rs`'s stylesheet scan must pass.
- The app player uses `src/components/ui/modal.tsx` `Modal` (portal, focus trap, Esc). It works with keyboard and screen reader. The step list is a real `<ol>` of `<button>`s with accessible names.
- No secret value appears in any marker or label. Labels come from `browser::highlight::Say` or the label `highlight::show` drew.
- Markers come from recorded start times, never from summed durations.
- Recording timings must not slow a run: one clock read per step and one per action, and only while recording.
- Rust tests go only in `src-tauri/tests/suite/`. A new module needs a `mod` line in `suite/main.rs`. Webview tests go in `tests/`, mirroring `src/`.
- `src/bindings.ts` is regenerated with `cd src-tauri && cargo test --test bindings`. Never hand-edit it.
- Run one cargo command at a time, and never two suites at once (the machine is shared). Never set `CARGO_TARGET_DIR` or `--target-dir`.
- The report page never contains the literal text `<video`, `<embed` or `<iframe`. This is `the_report_links_the_video_beside_the_pictures_and_never_embeds_it`. The player builds its element with `document.createElement('video')`.
- Commit with a Bash heredoc: `git commit -F - <<'EOF' ... EOF`. End with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Confirm with `git log -1`.
- User-facing strings are exactly as written in this plan.

## Review Focus

1. **A video whose `duration` is `NaN`/`Infinity`, or not loaded yet.** Ticks and the red stretch are placed on `length_ms`, and are re-placed on the video's own duration once that is finite. No style is ever `NaN%`. Pinned in Task 4 (`positions use length_ms until the video knows its duration`) and Task 6.
2. **A mark later than the video's end.** `length_ms` is read just before the stop, and the browser may cut a little. Positions clamp to 0–100%, and a jump clamps to the duration. Pinned in Task 4 (`timelinePercent clamps to 0..100`) and Task 6.
3. **Two steps that start at the same millisecond, or a step at 0.** This happens with a reused browser's sign-in at 0 and a module trip that does nothing. `currentAt` picks the later of equal starts, and the list stays in recorded order. Pinned in Task 4 (`currentAt`).
4. **Space or Enter on a focused step or action button.** The button activates as a normal button, and play/pause does not also toggle. Pinned in Task 4 (`Space on a focused button presses it and does not toggle play`) and Task 6.
5. **The player closed before the video bytes arrive.** No object URL leaks, and the player does not reopen when the bytes land. Pinned in Task 5 (`closing before the video loads leaves it closed`).

---

## File map

| File | Responsibility | Task |
|---|---|---|
| `src-tauri/src/autorun/mod.rs` | `VideoMark`, `VideoStep`, `VideoMarks`; `CaseRecord::video_marks`; `pub mod video_marks;` | 1 |
| `src-tauri/src/autorun/video_marks.rs` (new) | `offset_ms`, `mark_label`, `mark_failures`, `case_timeline` + `VideoTimeline`/`TimelineStep` | 1 |
| `src-tauri/src/autorun/recordings.rs` | `RecordingEntry::marks`, `add_with_marks`, `MAX_MARKS`, `Running::marks` | 1, 3 |
| `src-tauri/src/browser/recording.rs` | `Recording::started()` | 1 |
| `src-tauri/src/browser/cdp.rs` | `Driver::note_drawn_label` / `take_drawn_label`; `Cdp` field | 1 |
| `src-tauri/src/browser/highlight.rs` | `show` notes the label it drew; labels for page-level actions | 1 |
| `src-tauri/src/autorun/runner.rs` | `InRun::clock` / `InRun::marks`; one mark per action | 2 |
| `src-tauri/src/autorun/replay.rs` | `run_case_in` records step offsets, length and failures | 2 |
| `src-tauri/src/ai_bridge/discovery_recording.rs` | `recording_now`, `note_recording_mark`; marks reach the index | 3 |
| `src-tauri/src/ai_bridge/discovery.rs`, `autorun_try.rs` | call the two helpers around each tried action | 3 |
| `src/screens/AutoRun/videoTimeline.ts` (new) | `Timeline` type, mappers, maths, `stepLabel` (moved) | 4 |
| `src/screens/AutoRun/VideoPlayer.tsx` (new) | the modal player | 4 |
| `src/lib/actionIcons.ts` | 5 new icon names | 4 |
| `src/screens/AutoRun/CaseVideo.tsx`, `RecordingsCard.tsx`, `PastRuns.tsx`, `RunReview.tsx` | Watch / Play open the player | 5 |
| `src-tauri/web/autorun-report-player.js` (new), `autorun-report.css`, `autorun-report.js` | the report's player | 6 |
| `src-tauri/src/autorun/report.rs` | timelines in the blob; player script + CSP hash | 7 |
| `src/dev/demo.ts`, `docs-site/src/content/auto-run.ts` | sample data and docs | 8 |

---

### Task 1 (Rust): Timing data, labels and the recorder's clock

**Files:**
- Modify: `src-tauri/src/autorun/mod.rs`. Add the types beside `StepRecord` (~line 588). Add `CaseRecord::video_marks` after `video` (~line 709). Add `pub mod video_marks;`.
- Create: `src-tauri/src/autorun/video_marks.rs`
- Modify: `src-tauri/src/autorun/recordings.rs` (`RecordingEntry`, `add`)
- Modify: `src-tauri/src/autorun/replay.rs:222`: make `was_run` `pub(crate)`
- Modify: `src-tauri/src/browser/recording.rs` (`Recording`, `start`)
- Modify: `src-tauri/src/browser/cdp.rs`. Add the trait methods near `step_began` (~line 3135). Add the `Cdp` field and the overrides in `impl Driver for Cdp` (~line 3196).
- Modify: `src-tauri/src/browser/highlight.rs` (`show` ~line 447, `label_of` ~line 209)
- Modify: every `CaseRecord { .. }` literal in `src-tauri/src` and `src-tauri/tests/suite`. Each gets `video_marks: None`. Find them with Grep `video: (None|Some)` and `CaseRecord \{`; there are about 40 sites in 24 files.
- Modify: `src-tauri/tests/suite/common.rs` (`ScriptedDriver`: field + overrides)
- Create: `src-tauri/tests/suite/video_marks.rs`; add `mod video_marks;` in `tests/suite/main.rs`
- Test: `src-tauri/tests/suite/browser_highlight.rs`
- Regenerate: `src/bindings.ts`

**Interfaces:**
- Produces (in `v2_lib::autorun`, all `Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type`):
  ```rust
  pub struct VideoMark { pub at_ms: u32, pub label: String }
  pub struct VideoStep {
      pub step_number: i32,
      #[serde(default, skip_serializing_if = "std::ops::Not::not")] pub cleanup: bool,
      pub at_ms: u32,
      #[serde(default, skip_serializing_if = "std::ops::Not::not")] pub failed: bool,
      #[serde(default, skip_serializing_if = "Vec::is_empty")] pub actions: Vec<VideoMark>,
  }
  pub struct VideoMarks { pub length_ms: u32, #[serde(default)] pub steps: Vec<VideoStep> }
  // CaseRecord:
  #[serde(default, skip_serializing_if = "Option::is_none")] pub video_marks: Option<VideoMarks>,
  ```
- Produces (in `v2_lib::autorun::video_marks`):
  ```rust
  pub const FILL_FORM_LABEL: &str = "Fill the form";
  pub const SHOT_LABEL: &str = "Take a picture";
  pub fn offset_ms(start: std::time::Instant, at: std::time::Instant) -> u32; // saturating, capped at u32::MAX
  pub fn mark_label(action: &Action, drawn: Option<&str>) -> String;
  pub fn mark_failures(marks: &mut VideoMarks, steps: &[StepRecord], cleanup: Option<&StepRecord>);
  #[derive(Serialize)] pub struct TimelineStep { pub name: String, pub at_ms: u32, pub failed: bool, pub actions: Vec<VideoMark> }
  #[derive(Serialize)] pub struct VideoTimeline { pub title: String, pub length_ms: Option<u32>, pub steps: Vec<TimelineStep>, pub marks: Vec<VideoMark> }
  pub fn case_timeline(case: &CaseRecord) -> VideoTimeline;
  ```
- Produces (`recordings`): `RecordingEntry` gains `#[serde(default, skip_serializing_if = "Vec::is_empty")] pub marks: Vec<VideoMark>`. Also `pub const MAX_MARKS: usize = 2_000;` and `pub fn add_with_marks(root: &Path, title: &str, duration_ms: u32, bytes: &[u8], marks: Vec<VideoMark>) -> Result<RecordingEntry, String>`. `add` stays and calls it with `Vec::new()`. Marks beyond `MAX_MARKS` are dropped.
- Produces (`browser::recording`): `Recording` gains a private `started: Instant`, set right after the START call succeeds, and `pub fn started(&self) -> Instant`.
- Produces (`browser::cdp::Driver`): `fn note_drawn_label(&mut self, _label: &str) {}` and `fn take_drawn_label(&mut self) -> Option<String> { None }`. `Cdp` overrides both with a `drawn_label: Option<String>` field, initialised `None`. `take` empties it.

Rules the tests pin:
- `mark_label` checks these in order:
  1. A `fill_form` stand-in (`action_help::fill_form_ask(a).is_some()`) gives `FILL_FORM_LABEL`.
  2. A screenshot stand-in (`action_help::screenshot_ask(a).is_some()`) gives `SHOT_LABEL`.
  3. `drawn` `Some(l)` gives `l`.
  4. Otherwise, from `Say::of(a)`: when `if_password` is `Some`, that hidden label (the element was never looked at, so it may be a password field); otherwise `label`.
- `highlight::show` calls `d.note_drawn_label(label)` only when the page answered `true`. `label` is the one it drew, after the password swap.
- `label_of` gains these arms, before its catch-all. Each was the bare kind word before:
  - `Navigate` → `"Open a page"`
  - `Reload` → `"Reload the page"`
  - `ExpireSession` → `"End the session"`
  - `ReturnToArea` → `"Go back to the area"`
  - `OpenTab` → `"Open a tab"`
  - `SwitchTab` → `"Switch tab"`
  - `CloseTab` → `"Close a tab"`
  - `SignIn` → `"Sign in"`

  None of these is drawn over an element, so no on-screen box changes.
- `mark_failures`: a `VideoStep` is `failed` when its record has an outcome with `!o.ok && replay::was_run(o)`. A step that is not `cleanup` is matched to `steps` by `step_number`. A `cleanup` step is matched to `cleanup`.
- `case_timeline`:
  - `title` is `format!("Video of #{} {}", case.case_id, case.title)`.
  - Each step's `name` is `report::step_label(n)`, or `CLEANUP_LABEL` for a clean-up step.
  - `length_ms` comes from `video_marks`. Without `video_marks`: `length_ms: None` and `steps: []`.
  - `marks` is always `[]` (only recordings have a flat list).

- [ ] **Step 1: Write the failing tests** in `tests/suite/video_marks.rs`:

```rust
#[test]
fn old_case_records_and_recording_entries_load_without_marks() {
    let case: CaseRecord = serde_json::from_value(json!({ "case_id": 1, "title": "t", "verdict": "", "note": "", "steps": [], "video": "video-1-1.mp4" })).unwrap();
    assert_eq!(case.video_marks, None);
    assert!(!serde_json::to_string(&case).unwrap().contains("video_marks"));
    let e: RecordingEntry = serde_json::from_value(json!({ "id": "r", "title": "t", "created": "c", "duration_ms": 5, "file": "recording-1-1.mp4" })).unwrap();
    assert!(e.marks.is_empty());
    assert!(!serde_json::to_string(&e).unwrap().contains("marks"));
}

#[test]
fn marks_write_only_what_they_hold() {
    let m = VideoMarks { length_ms: 90_000, steps: vec![VideoStep { step_number: 1, cleanup: false, at_ms: 0, failed: false, actions: vec![] }] };
    assert_eq!(serde_json::to_value(&m).unwrap(), json!({ "length_ms": 90000, "steps": [{ "step_number": 1, "at_ms": 0 }] }));
}

#[test]
fn an_offset_is_measured_from_the_recording_start_and_never_negative() {
    let t0 = Instant::now();
    assert_eq!(offset_ms(t0, t0 + Duration::from_millis(1500)), 1500);
    assert_eq!(offset_ms(t0 + Duration::from_millis(10), t0), 0);
}

#[test]
fn a_mark_label_is_the_highlight_label_and_never_a_secret() {
    let a = |v: Value| -> Action { serde_json::from_value(v).unwrap() };
    let fill = a(json!({ "kind": "fill", "selector": "#surname", "value": "Smith" }));
    assert_eq!(mark_label(&fill, Some("Type 'Smith'")), "Type 'Smith'");
    assert_eq!(mark_label(&fill, None), TYPE_PASSWORD, "never drawn, so it may have been a password field");
    assert_eq!(mark_label(&a(json!({ "kind": "fill", "selector": "#a", "value": "x", "sensitive": true })), None), TYPE_HIDDEN);
    assert_eq!(mark_label(&a(json!({ "kind": "click", "selector": "#a" })), None), "Click");
    assert_eq!(mark_label(&a(json!({ "kind": "check_text", "value": "Saved" })), None), "Check: shows 'Saved'");
    assert_eq!(mark_label(&a(json!({ "kind": "navigate", "url": "https://app.test/x?token=abc" })), None), "Open a page");
    assert_eq!(mark_label(&a(json!({ "kind": "sign_in", "account": "admin" })), None), "Sign in");
    let form = action_help::fill_form_from(&json!({ "kind": "fill_form", "fields": { "#pw": "hunter2" } })).unwrap().unwrap();
    assert_eq!(mark_label(&form, None), FILL_FORM_LABEL);
    assert_eq!(mark_label(&action_help::screenshot_action(false), None), SHOT_LABEL);
}

#[test]
fn mark_failures_marks_the_steps_that_failed() { /* steps: 1 ok, 2 failed, 3 "not run: ..." ; cleanup failed */
    // assert flags == [false, true, false] for steps 1,2,3 and the cleanup VideoStep failed == true
}

#[test]
fn the_case_timeline_names_its_steps_like_the_review() {
    // video_marks steps 0, -1, 1, cleanup(step 2) -> names ["Sign in", "Module", "Step 1", "Clean-up"]
    // title == "Video of #7 Leave request"; length_ms == Some(20_000); failed carried over
    // a case without video_marks -> steps empty, length_ms None, marks empty
}
```

Add this to `browser_highlight.rs`:

```rust
#[tokio::test]
async fn the_label_drawn_is_noted_for_the_video() {
    let mut d = FakePage { password: true, ..FakePage::default() }.driver();
    execute_with(&mut d, &act(json!({ "kind": "fill", "selector": "#a", "value": "hunter2" })), &off()).await;
    assert_eq!(d.take_drawn_label().as_deref(), Some(TYPE_PASSWORD));
    assert_eq!(d.take_drawn_label(), None, "taking it empties it");
    let mut d = FakePage::default().driver();
    execute_with(&mut d, &act(json!({ "kind": "fill", "selector": "#a", "value": "Smith" })), &off()).await;
    assert_eq!(d.take_drawn_label().as_deref(), Some("Type 'Smith'"));
}
```

`ScriptedDriver` gets a `drawn_label: Option<String>` field (initialised `None` in `ScriptedDriver::new`) and overrides both trait methods.

- [ ] **Step 2: Run them to verify they fail**

Run: `cd src-tauri && cargo test --test suite video_marks::`
Expected: compile errors (`VideoMarks`, `offset_ms`, `mark_label` not found).

- [ ] **Step 3: Implement** the types, `video_marks.rs`, the `recordings` additions, `Recording::started`, the two `Driver` methods, and the `highlight` changes. Add `video_marks: None` to every `CaseRecord` literal.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test --test suite video_marks::`, then `cd src-tauri && cargo test --test suite browser_highlight::`
Expected: PASS.

- [ ] **Step 5: Regenerate the bindings and run the whole Rust suite**

Run: `cd src-tauri && cargo test --test bindings`, then `cd src-tauri && cargo test --tests`
Expected: PASS. `src/bindings.ts` now has `VideoMark`, `VideoStep`, `VideoMarks`, `video_marks?` on `CaseRecord`, and `marks?` on `RecordingEntry`. Then `npx tsc --noEmit` passes.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src src-tauri/tests/suite src/bindings.ts
git commit -F - <<'EOF'
feat(v2): timing data for video markers

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 2 (Rust): A recorded case notes where each step and action began

**Files:**
- Modify: `src-tauri/src/autorun/runner.rs`:
  - `InRun` (~line 425): add the 2 fields.
  - `run_step_in_run` (~line 1017): reset `run.marks`.
  - `run_expanded`'s action loop: take the offset after `p.began` (~line 1256); push the mark after `p.ended` (~line 1555).
- Modify: `src-tauri/src/autorun/replay.rs` `run_case_in` (lines 535–822)
- Test: `src-tauri/tests/suite/autorun_video.rs`

**Interfaces:**
- Consumes: `VideoMark`, `VideoStep`, `VideoMarks`, `video_marks::{offset_ms, mark_label, mark_failures}`, `Recording::started`, `Driver::take_drawn_label` (Task 1).
- Produces: `InRun` gains `pub clock: Option<Instant>` (input: the recording's start; `None` records nothing) and `pub marks: Vec<VideoMark>` (learned back: one per action the step started).

Where the clock is read:
- Action loop, right after `p.began(...)`: `let mark_at = run.clock.map(|c| offset_ms(c, Instant::now()));`. When `Some`, first call `d.take_drawn_label()` to drop any older label. After the action's outcome is final (beside `p.ended`), push `VideoMark { at_ms, label: mark_label(action, d.take_drawn_label().as_deref()) }`. An action skipped with `continue` before it runs gets no mark.
- `run_case_in`:
  - `let clock = recording.as_ref().map(|r| r.started());`.
  - A reused browser's step 0 gets `at_ms: 0`.
  - The sign-in step gets `offset_ms(c, signing_in)`.
  - The module step gets `offset_ms(c, reaching)`.
  - Each run step gets `offset_ms(c, asked_at)`, with `cleanup: step.cleanup` and `actions: in_run.marks`. Move `marks` out beside `tab, dialog, components` at line 702.
  - These reuse the `Instant`s already read; a step that was not run gets no `VideoStep`.
  - `length_ms = offset_ms(c, Instant::now())` is read just before `recording::stop` (line 770).
  - After `record.video` is set, `record.video_marks = Some(VideoMarks { length_ms, steps })` only when the video was kept. Then call `mark_failures(marks, &record.steps, record.cleanup.as_ref())`. A passed case, or one with no video, keeps `None`.

- [ ] **Step 1: Write the failing tests** in `autorun_video.rs`:

```rust
#[tokio::test]
async fn a_kept_video_knows_where_each_step_and_action_began() {
    let dir = tempfile::tempdir().unwrap();
    let mut gate = DropsOnce(vec![]);
    let run = plan_run(dir.path(), vec![driver(Recorder::working(), false)], &mut gate, false).await;
    let marks = run.cases[0].video_marks.clone().expect("a kept video has its marks");
    let steps: Vec<(i32, bool)> = marks.steps.iter().map(|s| (s.step_number, s.failed)).collect();
    assert_eq!(steps, vec![(1, false), (2, true)]);
    assert_eq!(marks.steps[0].actions.iter().map(|a| a.label.as_str()).collect::<Vec<_>>(), ["Open a page"]);
    assert_eq!(marks.steps[1].actions.iter().map(|a| a.label.as_str()).collect::<Vec<_>>(), ["Check: shows 'x'"]);
    assert!(marks.steps[0].at_ms <= marks.steps[1].at_ms && marks.steps[1].at_ms <= marks.steps[1].actions[0].at_ms);
    assert!(marks.steps[1].actions[0].at_ms <= marks.length_ms);
}

#[tokio::test]
async fn a_step_that_never_ran_has_no_marker() {
    // script: step 1 check_text "x" (fails), step 2 check_text "y" (not run)
    // assert video_marks.steps == [step 1 only], failed == true
}

#[tokio::test]
async fn a_passed_case_and_a_run_without_video_keep_no_marks() {
    // go(dir, true, vec![driver(Recorder::working(), true)], &[2]) -> video_marks None
    // go(dir, false, vec![driver(Recorder::working(), false)], &[3]) -> video_marks None
}
```

Extend `the_recording_starts_before_the_sign_in_and_stops_after_the_last_step` with `assert_eq!(case.video_marks.as_ref().unwrap().steps[0].step_number, 0, "the sign-in is the first marker");`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cd src-tauri && cargo test --test suite autorun_video::`
Expected: FAIL (`video_marks` is `None`).

- [ ] **Step 3: Implement** `InRun::clock`/`marks` and the `run_case_in` changes described above.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test --test suite autorun_video::`, then `cd src-tauri && cargo test --test suite autorun_replay::`
Expected: PASS.

- [ ] **Step 5: Run the whole Rust suite**

Run: `cd src-tauri && cargo test --tests`
Expected: PASS.

- [ ] **Step 6: Commit** (`feat(v2): a recorded case notes where each step and action began`).

---

### Task 3 (Rust): A discovery recording marks each tried action

**Files:**
- Modify: `src-tauri/src/autorun/recordings.rs` (`Running` gains `pub marks: Vec<VideoMark>`)
- Modify: `src-tauri/src/ai_bridge/discovery_recording.rs`:
  - `Running` literal (line 80): add `marks: Vec::new()`.
  - New helpers.
  - `Video` gains `marks`; `take_and_read` moves them over.
  - `keep_video` calls `recordings::add_with_marks`.
- Modify: `src-tauri/src/ai_bridge/discovery.rs` `discover_action_heard` loop (lines 852–897)
- Modify: `src-tauri/src/ai_bridge/autorun_try.rs` `autorun_try` (lines 245–299)
- Test: `src-tauri/tests/suite/autorun_recordings.rs`

**Interfaces:**
- Consumes: `VideoMark`, `recordings::{add_with_marks, MAX_MARKS}`, `video_marks::mark_label`, `Driver::take_drawn_label` (Task 1).
- Produces (re-exported from `ai_bridge` like the other recording items):
  ```rust
  /// The running recording's clock now (one read), or None with no recording going.
  pub fn recording_now(discovery: &Option<crate::commands::autorun::DiscoveryState>) -> Option<u32>;
  /// One mark at `at_ms` (from `recording_now`) labelled by `mark_label(action, drawn)`. Nothing when `at_ms` is None, no recording is going, or `MAX_MARKS` are held.
  pub fn note_recording_mark(discovery: &mut Option<DiscoveryState>, at_ms: Option<u32>, action: &Action, drawn: Option<&str>);
  ```

Call sites:
- In `discover_action_heard`, for each `one` in `runs` (so a component marks each expanded action):
  - Before `discover_one`: `let mark_at = recording_now(p.discovery);`. When `Some`, also call `d.take_drawn_label()`.
  - After the `Ok(ran)` match: `note_recording_mark(p.discovery, mark_at, one, d.take_drawn_label().as_deref())`.
  - A refused action (the `Err(refused)` return) gets no mark.
- In `autorun_try`:
  - Before `CALL_STARTED.scope(...)`: `let mark_at = recording_now(&session.discovery);`. When `Some`, also call `session.cdp.take_drawn_label()`.
  - After `answer`: `if answer.0 == 200 { note_recording_mark(&mut session.discovery, mark_at, &action, session.cdp.take_drawn_label().as_deref()) }`. A tried action that ran answers 200, passing or failing.
  - A `fill_form` gives one mark, `"Fill the form"`.

- [ ] **Step 1: Write the failing tests** in `autorun_recordings.rs`:

```rust
#[tokio::test]
async fn a_discovery_recording_keeps_a_mark_for_each_action_tried() {
    let _lock = crate::serial::autorun();
    let dir = root();
    let (mut b, _) = slot(working(), exploring("Leave"));
    assert_eq!(recording_start_in(&mut b, dir.path(), &start_body(TITLE), T0).await.0, 200);
    let click: Action = serde_json::from_value(json!({ "kind": "click", "selector": "#save" })).unwrap();
    let _ = discover_action_in(&mut b, dir.path(), "Acme", "Web", &click, None, None).await;
    assert_eq!(recording_stop_in(&mut b).await.0, 200);
    let e = &list(dir.path())[0];
    assert_eq!(e.marks.iter().map(|m| m.label.as_str()).collect::<Vec<_>>(), ["Click"]);
    assert!(e.marks[0].at_ms <= e.duration_ms);
}

#[test]
fn a_mark_needs_a_running_recording_and_names_no_secret() {
    // recording_now(&None) == None; note_recording_mark(&mut None, Some(5), ..) adds nothing
    // with a Running in a DiscoveryState: a fill_form stand-in with "hunter2" -> label "Fill the form", no "hunter2" anywhere in the marks
}

#[test]
fn marks_stop_at_the_cap() {
    // note MAX_MARKS + 5 marks -> running.marks.len() == MAX_MARKS
}

#[test]
fn a_recording_written_with_marks_reads_them_back() {
    // add_with_marks(dir, "t", 10, MP4, vec![VideoMark { at_ms: 3, label: "Click".into() }]) -> list()[0].marks == that
}
```

(`slot` and `exploring` come from `crate::autorun_discovery`. `discover_action_in` is `v2_lib::ai_bridge::discover_action_in`.)

- [ ] **Step 2: Run them to verify they fail**

Run: `cd src-tauri && cargo test --test suite autorun_recordings::`
Expected: compile errors (`recording_now`, `note_recording_mark` not found).

- [ ] **Step 3: Implement** `Running::marks`, the two helpers, the `Video` carry-over, and the call sites.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test --test suite autorun_recordings::`, then `cd src-tauri && cargo test --test suite autorun_discovery::`
Expected: PASS.

- [ ] **Step 5: Run the whole Rust suite**, then regenerate the bindings and confirm nothing changed

Run: `cd src-tauri && cargo test --tests`, then `cd src-tauri && cargo test --test bindings`, then `git diff --stat src/bindings.ts`
Expected: PASS; no change to `src/bindings.ts` (`Running` is not exposed).

- [ ] **Step 6: Commit** (`feat(v2): a discovery recording marks each tried action`).

---

### Task 4 (webview): The timeline model and the player

**Files:**
- Create: `src/screens/AutoRun/videoTimeline.ts`
- Create: `src/screens/AutoRun/VideoPlayer.tsx`
- Modify: `src/screens/AutoRun/RunReview.tsx:93-97`. Move `stepLabel` out to `videoTimeline.ts`, then import it and add `export { stepLabel } from "./videoTimeline";` so `tests/screens/AutoRun/RunReview.test.tsx` still imports it.
- Modify: `src/lib/actionIcons.ts`. Add these in the "Moving through a flow" block with one-line comments:
  - `Pause as IconPauseVideo`
  - `SkipBack as IconPreviousStep`
  - `SkipForward as IconNextStep`
  - `ChevronRight as IconShowActions`
  - `ChevronDown as IconHideActions`
- Test: `tests/screens/AutoRun/videoTimeline.test.ts`, `tests/screens/AutoRun/VideoPlayer.test.tsx`

**Interfaces:**
- Consumes: `CaseRecord`, `RecordingEntry`, `VideoMarks` from `src/bindings.ts` (Task 1).
- Produces (`videoTimeline.ts`). The shape matches Rust's `VideoTimeline` JSON (Task 7), snake_case:
  ```ts
  export type TimelineMark = { label: string; at_ms: number };
  export type TimelineStep = { name: string; at_ms: number; failed: boolean; actions: TimelineMark[] };
  export type Timeline = { title: string; length_ms: number | null; steps: TimelineStep[]; marks: TimelineMark[] };
  export const CLEANUP_LABEL = "Clean-up";
  export const SKIP_MS = 5000;
  export const PREV_GRACE_MS = 1000;
  export const SPEEDS = [0.5, 1, 2] as const;
  export function stepLabel(stepNumber: number): string;            // moved, unchanged
  export function caseTimeline(c: CaseRecord): Timeline;           // title `Video of #${id} ${title}`
  export function recordingTimeline(e: RecordingEntry): Timeline;  // title `Recording: ${title}`, length_ms = duration_ms, steps [], marks = e.marks ?? []
  export function failedFrom(t: Timeline): number | null;          // first failed step's at_ms
  export function currentAt(starts: number[], ms: number): number; // index of the last start <= ms (later wins on ties); -1 before the first
  export function stepTarget(t: Timeline, ms: number, dir: -1 | 1): number | null; // ] next start > ms; [ last start < ms - PREV_GRACE_MS
  export function timelinePercent(ms: number, lengthMs: number): number; // clamped 0..100; 0 when lengthMs is not a positive finite number
  export function clockText(ms: number): string;                   // 83000 -> "1:23", negative -> "0:00"
  ```
  `stepTarget` uses `steps` when there are any, else `marks`.
- Produces (`VideoPlayer.tsx`):
  ```ts
  export const NO_MARKERS = "No step markers: this video was recorded before they were kept.";
  export const VIDEO_WONT_PLAY = "This video cannot be played here - save it instead.";
  export const VIDEO_LOADING = "Loading the video...";
  export default function VideoPlayer(props: {
    timeline: Timeline; src: string | null; loading: boolean; error: boolean;
    saving: boolean; onSave: () => void; onClose: () => void;
  }): JSX.Element;
  ```

Layout and copy, all inside `<Modal onClose={onClose} label={timeline.title} className={MODAL_LARGE + " flex flex-col gap-3 p-4"}>`:
- **Header:** `timeline.title` as `h2`, and a button `aria-label="Close player"` (`IconCancel`).
- **Stage:** `<video aria-label={timeline.title}>` with no native `controls`, on `bg-surface`. While `loading`, show `VIDEO_LOADING`. On `error` (prop, or the element's `error` event), show `VIDEO_WONT_PLAY`; the markers and list stay.
- **Timeline bar:** a `div` with `role="slider" aria-label="Position"`, `aria-valuemin=0`, `aria-valuemax={length in s}`, `aria-valuenow`, and `aria-valuetext={`${clockText(now)} of ${clockText(len)}${stepName ? `, ${stepName}` : ""}`}`. Clicking it seeks. Inside the bar:
  - one tick per step (or per mark), with `title` = the step's `name` (or the mark's label), at `timelinePercent(at_ms, len)`;
  - the failed stretch `data-testid="failed-stretch"` in `bg-danger/40`, `left: timelinePercent(failedFrom)`, `right: 0`;
  - the playhead in `bg-accent`.

  `len` = the video's `duration*1000` when finite and > 0, else `timeline.length_ms`.
- **Controls:**
  - `Play`/`Pause` button (`IconPlayVideo`/`IconPauseVideo`).
  - `Previous step` (`IconPreviousStep`) and `Next step` (`IconNextStep`).
  - The time text `0:42 / 1:30`.
  - A `role="group" aria-label="Speed"` of buttons `0.5x`, `1x`, `2x` with `aria-pressed`.
  - `Save video` (`IconExport`), disabled while `saving`.
- **List** (beside the video, `lg:` two columns):
  - With steps: heading `Steps` and `<ol aria-label="Steps">`. Each `li` holds:
    - a jump button with `aria-label={`${name} at ${clockText(at_ms)}${failed ? ", failed" : ""}`}`, and visible text name + time + `Failed` badge (`text-danger`);
    - when it has actions, a toggle `aria-label={`Actions of ${name}`}` with `aria-expanded` (`IconShowActions`/`IconHideActions`);
    - when open, an `<ol>` of action buttons `aria-label={`${label} at ${clockText(at_ms)}`}`.

    The failed step starts open. The step playing now has `aria-current="step"` and `bg-accent/10`. On open, the failed step's `li` is scrolled into view (`el.scrollIntoView?.({ block: "nearest" })`).
  - Without steps but with marks: heading `Actions` and `<ol aria-label="Actions">` of the action buttons, the playing one `aria-current="true"`.
  - With neither: no list, and `NO_MARKERS` under the video.
- **Keys** (`onKeyDown` on the panel's content wrapper):
  - `" "` toggles play;
  - `ArrowLeft`/`ArrowRight` skip ∓/± `SKIP_MS` (clamped to 0..len);
  - `[`/`]` jump to `stepTarget(..., -1/1)`.

  Each handled key calls `preventDefault`. Space and Enter are left alone when `e.target` is a `button`, so a focused button presses as usual.

- [ ] **Step 1: Write the failing tests**

`videoTimeline.test.ts`:

```ts
test("caseTimeline names steps like the review and carries failures", () => {
  const t = caseTimeline(caseWith({ length_ms: 20000, steps: [
    { step_number: 0, at_ms: 0 }, { step_number: 1, at_ms: 4000, actions: [{ at_ms: 4200, label: "Click" }] },
    { step_number: 2, at_ms: 9000, failed: true }, { step_number: 3, cleanup: true, at_ms: 15000 } ] }));
  expect(t.title).toBe("Video of #7 Leave request");
  expect(t.steps.map((s) => s.name)).toEqual(["Sign in", "Step 1", "Step 2", "Clean-up"]);
  expect(t.steps[1].actions).toEqual([{ at_ms: 4200, label: "Click" }]);
  expect(failedFrom(t)).toBe(9000);
});
test("a case without marks gives an empty timeline", () => { /* steps [], marks [], length_ms null, failedFrom null */ });
test("recordingTimeline is a flat list with the recording's length", () => { /* title "Recording: Bug 1", length_ms 94000, marks copied */ });
test("currentAt picks the last start at or before now, later on ties", () => {
  expect(currentAt([0, 0, 4000], 0)).toBe(1);
  expect(currentAt([1000, 4000], 500)).toBe(-1);
  expect(currentAt([1000, 4000], 4000)).toBe(1);
});
test("stepTarget moves to the next start, and back past the grace", () => {
  // starts 0, 4000, 9000: (4500, 1) -> 9000; (9200, -1) -> 4000; (9000 + 1500, -1) -> 9000; (9500, 1) -> null
});
test("timelinePercent clamps to 0..100", () => {
  expect(timelinePercent(25000, 20000)).toBe(100);
  expect(timelinePercent(5000, 20000)).toBe(25);
  expect(timelinePercent(5000, Number.NaN)).toBe(0);
});
test("clockText", () => { expect(clockText(83000)).toBe("1:23"); expect(clockText(-5)).toBe("0:00"); });
```

`VideoPlayer.test.tsx`. In `beforeEach`, stub `HTMLMediaElement.prototype.play` (resolved promise) and `pause`. Also define configurable `currentTime` and `playbackRate` (backed by fields) and `duration` (default `NaN`) on the prototype. Fixture: steps Sign in 0, Step 1 4000 (action "Click" 4200), Step 2 9000 failed (action "Check: shows 'x'" 9100); length_ms 20000.

```ts
test("a dialog named by the title lists the steps and marks the failed one", () => {
  render(<VideoPlayer {...props} />);
  const dialog = screen.getByRole("dialog", { name: "Video of #7 Leave request" });
  const list = within(dialog).getByRole("list", { name: "Steps" });
  expect(within(list).getByRole("button", { name: "Step 2 at 0:09, failed" })).toBeInTheDocument();
  expect(within(list).getByRole("button", { name: "Check: shows 'x' at 0:09" })).toBeInTheDocument(); // failed step open
  expect(within(list).queryByRole("button", { name: "Click at 0:04" })).toBeNull();                 // others closed
});
test("positions use length_ms until the video knows its duration", () => {
  // failed-stretch style.left === "45%"; set duration 30 + fireEvent.loadedMetadata(video) -> "30%"
});
test("each step has a tick titled with its name", () => { /* 3 elements with title "Sign in"/"Step 1"/"Step 2" in the slider */ });
test("clicking a step or an action jumps the video there", () => {
  // click "Step 1 at 0:04" -> video.currentTime === 4; open "Actions of Step 1", click "Click at 0:04" -> 4.2
});
test("Space plays and pauses, arrows skip 5 s, [ and ] move between steps", () => {
  // keyDown " " on the dialog -> play called; currentTime 4: ArrowRight -> 9, ArrowLeft -> 4; "]" -> 9; at 9.2 "[" -> 4
});
test("Space on a focused button presses it and does not toggle play", () => {
  // focus "Step 1 at 0:04"; keyDown " " -> play not called
});
test("the playing step is marked as the video plays", () => {
  // currentTime 9.5; fireEvent.timeUpdate(video) -> "Step 2 at 0:09, failed" has aria-current="step"
});
test("speed buttons set the playback rate", () => { /* click "2x" -> playbackRate 2, aria-pressed true; "1x" aria-pressed false */ });
test("Save video calls onSave; Close player and Escape call onClose", () => { /* ... */ });
test("a recording gets a flat list of actions", () => { /* list "Actions" with "Click at 0:03" */ });
test("an old video opens without markers", () => { /* empty timeline: NO_MARKERS shown, no list, no failed-stretch */ });
test("a video that will not play says so and keeps the list", () => { /* error -> VIDEO_WONT_PLAY and the Steps list */ });
```

- [ ] **Step 2: Run them to verify they fail**

Run: `npx vitest run tests/screens/AutoRun/videoTimeline.test.ts tests/screens/AutoRun/VideoPlayer.test.tsx`
Expected: FAIL (modules not found).

- [ ] **Step 3: Implement** `videoTimeline.ts`, the icons, and `VideoPlayer.tsx`. Move `stepLabel`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `npx vitest run tests/screens/AutoRun/videoTimeline.test.ts tests/screens/AutoRun/VideoPlayer.test.tsx tests/screens/AutoRun/RunReview.test.tsx tests/ui-consistency.test.ts`
Expected: PASS.

- [ ] **Step 5: Typecheck** with `npx tsc --noEmit`. Expected: no errors.

- [ ] **Step 6: Commit** (`feat(v2): the video player with step markers`).

---

### Task 5 (webview): Watch and Play open the player

**Files:**
- Modify: `src/screens/AutoRun/CaseVideo.tsx`
- Modify: `src/screens/AutoRun/RecordingsCard.tsx`
- Modify: `src/screens/AutoRun/PastRuns.tsx:332` and `src/screens/AutoRun/RunReview.tsx:657` (pass the timeline)
- Test: `tests/screens/AutoRun/CaseVideo.test.tsx`, `tests/screens/AutoRun/RecordingsCard.test.tsx`

**Interfaces:**
- Consumes: `VideoPlayer`, `caseTimeline`, `recordingTimeline`, `clockText` (Task 4); `useVideoFile` (unchanged).
- Produces: `CaseVideo({ name, className, timeline }: { name?: string | null; className?: string; timeline: Timeline })`. PastRuns and RunReview pass `timeline={caseTimeline(c)}`.

Behaviour:
- **CaseVideo:**
  - The play control becomes `Watch video` (`IconPlayVideo`), with title `"Watch the video with its steps"`. A parallel change is turning this row into icon buttons; if it has landed, keep its button and change only what it does, and keep the accessible name `Watch video`.
  - Clicking it sets `open` and calls `play.mutate()`.
  - While `open`, it renders `<VideoPlayer timeline src={src} loading={play.isPending} error={play.isError} saving={save.isPending} onSave={() => save.mutate()} onClose={() => { setOpen(false); close(); }} />`.
  - The inline `<video>` and `Close video` are removed. `Download video` stays.
  - A video too large to play keeps `Watch video` disabled with the existing too-large title.
- **RecordingsCard:**
  - `Play ${title}` opens the player the same way, with `recordingTimeline(entry)`.
  - The inline `<video>` and `Close ${title}` are removed.
  - `lengthText` is replaced by the import `clockText`.
- **Closing before the bytes land:** `src` may arrive after close. The player renders only while `open`, and `close()` revokes any URL that is set.

- [ ] **Step 1: Write the failing tests** (update the existing ones that named `Play video` / the inline video):

```ts
test("Watch video opens the player over the app with the case's steps", async () => {
  mockIPC((cmd) => (cmd === "auto_run_video_info" ? { size: 9, playable: true, attachable: true } : cmd === "auto_run_video" ? btoa("mp4") : null));
  show(NAME, timelineWithSteps);
  fireEvent.click(await screen.findByRole("button", { name: "Watch video" }));
  const dialog = await screen.findByRole("dialog", { name: "Video of #7 Leave request" });
  expect(within(dialog).getByLabelText("Video of #7 Leave request")).toHaveAttribute("src", "blob:the-video");
  expect(within(dialog).getByRole("list", { name: "Steps" })).toBeInTheDocument();
});
test("closing the player goes back to the row and lets the video go", async () => {
  // click "Close player" -> no dialog; URL.revokeObjectURL called with "blob:the-video"; "Watch video" focused again (Modal restores focus)
});
test("closing before the video loads leaves it closed", async () => {
  // auto_run_video resolves after a deferred; click Watch, click Close player, then resolve -> no dialog, revokeObjectURL called
});
test("a video that cannot be fetched opens the player with the will-not-play note", async () => {
  // auto_run_video rejects -> dialog shows VIDEO_WONT_PLAY and the Steps list
});
```

In `RecordingsCard.test.tsx`: `Play <title>` opens `dialog` named `Recording: <title>`, with an `Actions` list when the entry has `marks`. The old inline-video test is replaced.

- [ ] **Step 2: Run them to verify they fail**

Run: `npx vitest run tests/screens/AutoRun/CaseVideo.test.tsx tests/screens/AutoRun/RecordingsCard.test.tsx`
Expected: FAIL (no `Watch video`, no dialog).

- [ ] **Step 3: Implement** the CaseVideo and RecordingsCard changes, and pass `timeline` from PastRuns and RunReview.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `npx vitest run tests/screens/AutoRun/`
Expected: PASS.

- [ ] **Step 5: Run the whole webview suite and the typecheck**

Run: `npx tsc --noEmit`, then `npm test`
Expected: PASS. One `tests/App.test.tsx` failure that passes on a re-run is the documented load flake.

- [ ] **Step 6: Commit** (`feat(v2): Watch and Play open the video player`).

---

### Task 6 (webview): The report's own player

**Files:**
- Create: `src-tauri/web/autorun-report-player.js`
- Modify: `src-tauri/web/autorun-report.css`. Add a `/* Player */` block, colours only from the token variables. Hide `.vp` in the print block at line 310.
- Modify: `src-tauri/web/autorun-report.js`:
  - the `a[data-video]` click (lines 150–157);
  - `renderGallery`'s video tile (line 350): `g.k` opens the player, and pictures open the lightbox over the pictures only;
  - `overlayOpen` (line 421) and the keydown handler (line 427): return early while the player is open.
- Test: `tests/lib/reportPlayer.test.ts`

**Interfaces:**
- Consumes: the timeline JSON shape of Task 4 / Task 7, `D.videos[name]`.
- Produces (ES5, no network calls, no `eval`/`new Function`, and no literal `<video`):
  ```js
  window.ReportPlayer = {
    open: function (timeline, src, opener) {}, // builds #vp on first use, focuses Play
    close: function () {},                     // pauses, hides, focuses opener
    isOpen: function () {},
    at: currentAt, target: stepTarget, percent: timelinePercent, clock: clockText // same rules as Task 4
  };
  ```
  Opening a video without a timeline uses `{ title: 'Video', length_ms: null, steps: [], marks: [] }`.
  `autorun-report.js` calls `window.ReportPlayer.open(D.videos && D.videos[name] || EMPTY, '../shots/' + name, el)`.

The DOM it builds uses the same copy as Task 4:
- `div.overlay.vp#vp` with `role="dialog" aria-modal="true" aria-label=<title>`;
- `Close player`, `Play`/`Pause`, `Previous step`, `Next step`, the `Speed` group (`0.5x`/`1x`/`2x`, `aria-pressed`), and a `Save video` link (`<a download href=src>`);
- the slider `Position`, with ticks (`title`) and `.vp__fail`;
- the `Steps` or `Actions` `<ol>` with the same button names;
- `NO_MARKERS` and `VIDEO_WONT_PLAY` text.

Keys and a Tab focus trap live on `document` while it is open. Esc closes.

CSS:
- `.vp__fail` uses `var(--fail-fg)` at reduced opacity.
- The playhead and the current step use `var(--accent)`.
- The bar uses `var(--border)`/`var(--bg-subtle)`.
- Focus uses `var(--focus-ring)`.

- [ ] **Step 1: Write the failing tests** (pattern of `tests/lib/flowPage.test.ts`: read the file, `new Function(src)()` in `beforeAll`; stub `play`/`pause`/`currentTime`/`duration` as in Task 4):

```ts
test("the script has no network call, no eval and never writes a video tag", () => {
  for (const banned of ["fetch(", "XMLHttpRequest", "WebSocket", "eval(", "new Function", "<video"]) expect(src).not.toContain(banned);
});
test("open builds a dialog with the steps and the failed one marked", () => {
  player().open(timeline, "../shots/video-1-1.mp4", opener);
  const dialog = document.getElementById("vp")!;
  expect(dialog.getAttribute("aria-label")).toBe("Video of #7 Leave request");
  expect(buttonNamed("Step 2 at 0:09, failed")).toBeTruthy();
  expect((dialog.querySelector(".vp__fail") as HTMLElement).style.left).toBe("45%");
});
test("positions follow the video's duration once it is known, and clamp", () => { /* duration 30 + loadedmetadata -> 30%; a mark at 25000 of 20000 -> 100% */ });
test("clicking a step or an action seeks there", () => { /* currentTime 4, 4.2 */ });
test("keys: Space, arrows, [ ] act; Space on a button is left to the button; Esc closes and focus returns", () => { /* ... */ });
test("a video with no timeline opens without markers", () => { /* NO_MARKERS text, no ol */ });
test("the same maths as the app", () => {
  expect(player().at([0, 0, 4000], 0)).toBe(1);
  expect(player().target(timeline, 9200, -1)).toBe(4000);
  expect(player().percent(5000, Number.NaN)).toBe(0);
  expect(player().clock(83000)).toBe("1:23");
});
```

- [ ] **Step 2: Run them to verify they fail**

Run: `npx vitest run tests/lib/reportPlayer.test.ts`
Expected: FAIL (file not found).

- [ ] **Step 3: Implement** `autorun-report-player.js`, the CSS block, and the three `autorun-report.js` changes.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `npx vitest run tests/lib/reportPlayer.test.ts tests/lib/webPages.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit** (`feat(v2): the run report's own video player`).

---

### Task 7 (Rust): The report carries each video's timeline and the player script

**Files:**
- Modify: `src-tauri/src/autorun/report.rs`:
  - add `const PLAYER: &str = include_str!("../../web/autorun-report-player.js");` beside `SCRIPT` (~line 932);
  - the gallery loop (~lines 1143–1148): `videos.insert(name, case_timeline(case))` for each linked video;
  - the blob (~line 1401): `"videos"`;
  - the CSP (~line 1424) and the script tags (~line 1454).
- Test: `src-tauri/tests/suite/autorun_video.rs`, `src-tauri/tests/suite/autorun_report.rs`, `src-tauri/tests/suite/autorun_report_smart.rs`

**Interfaces:**
- Consumes: `video_marks::case_timeline` (Task 1); `web/autorun-report-player.js` (Task 6).
- Produces:
  - The blob holds `"videos": { "<video name>": VideoTimeline }`, only for a video the page links (safe name, file present).
  - The player script is LF-normalised like `SCRIPT`. It is written as `<script>{player}</script>` just before the page's own script, and hashed: `csp(&[head_js, player_js, app_js])`.

- [ ] **Step 1: Write the failing tests**

In `autorun_video.rs`:

```rust
#[test]
fn the_report_carries_each_videos_timeline() {
    use v2_lib::autorun::report::build_with_downloads;
    let name = "video-1700000000000-000001.mp4";
    let mut run = run_naming(name, false);
    run.cases[0].video_marks = Some(VideoMarks { length_ms: 20_000, steps: vec![
        VideoStep { step_number: 1, cleanup: false, at_ms: 4_000, failed: true, actions: vec![VideoMark { at_ms: 4_200, label: "Click".into() }] } ] });
    let html = build_with_downloads(&run, &[], "now", &|_| true, &|_| None);
    let blob = report_blob(&html); // text between `id="report-data">` and `</script>`, parsed as JSON
    let t = &blob["videos"][name];
    assert_eq!(t["title"], format!("Video of #{} {}", run.cases[0].case_id, run.cases[0].title));
    assert_eq!(t["length_ms"], 20000);
    assert_eq!(t["steps"][0], json!({ "name": "Step 1", "at_ms": 4000, "failed": true, "actions": [{ "at_ms": 4200, "label": "Click" }] }));
    assert!(!html.contains("<video"), "still never embedded");
}

#[test]
fn an_old_video_has_an_empty_timeline_and_a_missing_one_none() {
    // without video_marks: videos[name] == { title, length_ms: null, steps: [], marks: [] }
    // exists -> false: blob has no "videos" entry for it
}
```

In `autorun_report.rs`: update `the_policy_loads_nothing_and_names_the_pages_own_scripts_by_hash` to `assert_eq!(scripts.len(), 3, "the head script, the player and the page's own")` and `assert_eq!(script_src.matches("'sha256-").count(), 3)`.

In `autorun_report_smart.rs`: the network-ban test (line ~512) also reads `web/autorun-report-player.js` and applies the same banned list.

- [ ] **Step 2: Run them to verify they fail**

Run: `cd src-tauri && cargo test --test suite autorun_video::`
Expected: FAIL (no `"videos"` in the blob).

- [ ] **Step 3: Implement** the blob entry, `PLAYER`, the CSP and the script tag.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test --test suite autorun_video::`, then `cd src-tauri && cargo test --test suite autorun_report::`, then `cd src-tauri && cargo test --test suite autorun_report_smart::`
Expected: PASS, the stylesheet colour scan included.

- [ ] **Step 5: Run the whole Rust suite** with `cd src-tauri && cargo test --tests`. Expected: PASS.

- [ ] **Step 6: Commit** (`feat(v2): the run report plays each video with its steps`).

- [ ] **Step 7: Check it by hand in a browser.** Open a report with a failed case's video, via the dev app's Report button. Confirm:
  - the tile opens the player;
  - the ticks, the red stretch and the step list show;
  - Space, ←, →, [ and ] work;
  - Esc returns focus to the tile;
  - the gallery's video opens the player;
  - pictures still open the lightbox.

  jsdom does no layout, so only a real page shows overlap or a covered control.

---

### Task 8 (webview + docs): Sample data, How To Use text and shots

**Files:**
- Modify: `src/dev/demo.ts`:
  - the demo run's failed case (`proposed: "Failed"`, ~line 453) gets `video: "video-1700000000000-000001.mp4"` and a `video_marks` with 4 steps: Sign in 0, Step 1 3000 with actions "Click" 3200 and "Type 'Annual'" 3900, Step 2 8000 with action "Check: shows 'Saved'" 8300 and `failed: true`, Clean-up 14000; `length_ms: 18000`;
  - `autoRunVideoInfo` answers `{ size: 2_100_000, playable: true, attachable: true }` for that name too;
  - the demo recording `rec-1.webm` gets `marks`: "Click" 2000, "Type 'Leave'" 5500, "Take a picture" 9000.

  Demo mode has no `autoRunVideo`, so the player shows `VIDEO_WONT_PLAY` over its markers. That is acceptable for the shot, which is about the timeline and list.
- Modify: `docs-site/src/content/auto-run.ts`

Docs text:
- **New shot** `const PLAYER = "auto-run-video-player";` with route `[...TO_RUNS, { click: { role: "button", name: "Watch video" } }, { waitFor: { role: "list", name: "Steps" } }]` and alt `"The video player over Past runs: step markers on the timeline, the failed step in red and the step list beside the video"`.
- **New items** on `PLAYER`, group `runs`:
  - `video-timeline` (locate `{ role: "slider", name: "Position" }`), name "Timeline", does: "A mark for each step of the case (hover one for its name). The failed step is red from where it started to the end of the video. Click anywhere to go there."
  - `video-steps` (locate `{ role: "list", name: "Steps" }`), name "Steps", does: "The case's steps beside the video, the one playing now highlighted. Click a step to go to it, or open it to see its actions (\"Click Save\", \"Type 'Smith'\") and click one. The failed step is marked and opens first. Passwords and other secret values read **Type a hidden value**."
  - `video-controls` (locate `{ role: "group", name: "Speed" }`), name "Play, speed and save", does: "**Play** and **Pause**, the speed (0.5x, 1x, 2x) and **Save video**. Keys: [[Space]] plays or pauses, [[Left]] and [[Right]] skip 5 seconds, [[\\[]] and [[\\]]] go to the previous or next step, [[Escape]] closes the player."
- **New item** on `RUNS`: `watch-video` (locate `{ role: "button", name: "Watch video" }`), name "Watch video", does: "Opens the video of a failed or blocked case in a player over the app, with its steps marked. **Download video** saves the file."
- **`recordings-card` does:** replace "**Play** watches one here" with "**Play** opens it in the player, with a mark for each action the assistant tried".
- **`report` does:** "Opens a report of the run in your browser: each case, its steps, what failed, the screenshots, and each failed case's video in a player with its steps marked. It changes nothing."

Check the key notation `[[...]]` against the other items in `auto-run.ts`, and use the form they use.

- [ ] **Step 1:** Make the demo and docs content changes above.
- [ ] **Step 2: Run the guards**

Run: `npx vitest run docs-site/src` and `npm test -- tests/dev`
Expected: PASS. A stale-built-page guard failure is expected until Step 4.
- [ ] **Step 3: The controller takes the shots.**
  1. Close `tauri dev`.
  2. `npm run docs:dev` (signed out).
  3. `npm run docs:shots -- --dry-run`.
  4. `npm run docs:shots -- --only auto-run-video-player --only auto-run-past-runs --only auto-run-recordings --only auto-run-review`. The Past runs row and the review now show **Watch video**.
- [ ] **Step 4:** `npm run docs:build`, then `npx vitest run docs-site/src`. Expected: PASS.
- [ ] **Step 5: Commit** the demo, content, shots, positions and built site together (`docs(v2): the video player in How To Use`).

---

## Dependencies

| Task | Kind | Needs |
|---|---|---|
| 1 Timing data, labels, recorder clock | Rust | none |
| 2 Case run timings | Rust | 1 |
| 3 Discovery marks | Rust | 1 |
| 4 Timeline model + player | webview | 1 (bindings) |
| 5 Watch/Play open the player | webview | 4 |
| 6 Report's own player | webview | none (shape fixed here) |
| 7 Report wiring | Rust | 1, 6 |
| 8 Docs | webview + docs | 5, 7 |

Task 6 can run beside 1–3. Tasks 4–5 can run beside 2–3. Run Rust tasks one at a time in this checkout: one cargo command at a time, and never two fix agents in one tree.
