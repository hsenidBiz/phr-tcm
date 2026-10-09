# Auto Run Navigation Speed Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Make Auto Run trips and steps faster without changing anything a run checks. Area trips skip the home reload. The prompt window after a trip home is shorter. Must not save stops pausing static files. The run's files are read once. Step pictures no longer hold up the next step. The highlight pause can be turned off.

**Architecture:**
- `autorun/nav.rs` `go_to_module` gains a quick first try from the current page, with a fallback to today's path.
- `autorun/signin.rs` and `autorun/timing.rs` gain a 500 ms home window.
- `browser/save_guard.rs` `fetch_enable_params` lists resource types.
- `autorun/runner.rs` keeps a per-run cache of the recipe and areas file, keyed on each file's modified time and size.
- `autorun/replay.rs` and `runner::picture` move the picture off the step loop, with a barrier before the next step's first page-changing action and before the record is saved.

**Tech Stack:** Rust (Tauri 2, CDP driver).

**Spec:** `docs/superpowers/specs/2026-10-09-autorun-nav-speed-design.md`

## Global Constraints

- `QUICK_TRY_MS = 3000`
- `HOME_PROMPT_WINDOW_MS = 500`
- `FRESH_LOGIN_WINDOW_MS` stays 5000, and `PROMPT_WINDOW_MS` stays 1500 for a saved-session reuse.
- Patterns that are not paused: `Image`, `Stylesheet`, `Script`, `Font`, `Media`, `Manifest`, `TextTrack`.
- Nothing a run checks changes. The Must not save guard blocks every save-shaped request as before.
- Rust tests are integration tests in `src-tauri/tests/suite/`, with a `mod` line and serial locks. `src/bindings.ts` is generated only.
- No hosts or query strings in logs. No HTTP DELETE. No em or en dashes. Run one build or test command at a time.
- Commit with a Bash heredoc, staged by name, ending with the implementer's `Co-Authored-By`.

## Review Focus

1. A sidebar toggle must never close a menu that is already open. Test: `an_open_menu_keeps_its_toggle_unclicked`.
2. A trip started from a screen whose menu is covered (a modal or a full-screen grid) must still arrive, through the fallback. Test: `a_covered_menu_falls_back_to_home_and_arrives`.
3. A form post or a beacon on a Must not save case must still be blocked once static types are not paused. Test: `a_beacon_and_a_form_post_are_still_blocked`.
4. A step picture must show the page after its own step, never after the next step's click. Test: `the_next_click_waits_for_the_outstanding_picture`.
5. A case whose browser dies with a picture outstanding must save its record without a dangling picture. Test: `a_dead_browser_with_a_picture_outstanding_saves_a_clean_record`.

---

### Task 1: Trips from the current page and the shorter home window (A, B)

**Files:**
- `autorun/nav.rs` (`go_to_module`, a new quick-try helper)
- `autorun/timing.rs`
- `autorun/signin.rs`: the `Prompts::Together` window used after a reload home, around line 359

**Interfaces:**
- `pub const QUICK_TRY_MS: u64 = 3000;` and `pub const HOME_PROMPT_WINDOW_MS: u64 = 500;` in `autorun/timing.rs`.
- `go_to_module`, when it is not the first trip after a sign-in:
  1. Try the path's clicks from the current page. Skip a click when the next click's target is visible and enabled now. Each click gets a `QUICK_TRY_MS` limit.
  2. If every click passed and the arrived check succeeds, return `Ok`.
  3. Otherwise, log one INFO line ("went home and tried <module> again"; module name only), then run today's `go_home` plus the full path plus the arrived check. Its failure reports as today.

- [ ] Tests:
  - `a_trip_from_inside_the_app_does_not_reload_home`
  - `an_open_menu_keeps_its_toggle_unclicked`
  - `a_covered_menu_falls_back_to_home_and_arrives`
  - `the_first_trip_after_sign_in_is_unchanged`
  - `a_trip_home_watches_prompts_for_half_a_second`
  - `a_fresh_login_still_watches_for_five_seconds`
- [ ] Update any existing nav or speed test that pinned 1500 ms after a trip home, as the plan says. Never weaken a test for any other reason.
- [ ] Run the tests one at a time: `autorun_nav::`, then `autorun_replay::`, then the sign-in suite, then `cargo test --tests`.
- [ ] Commit `perf(v2): an Auto Run trip opens the menu from where the page is, and a trip home waits half a second for prompts`.

### Task 2: Save checks for save-capable requests only, and files read once (C, D)

**Files:**
- `browser/save_guard.rs` (`fetch_enable_params`)
- `autorun/runner.rs` (the per-step `load_effective_recipe_if_any` and `load_nav`)

**Interfaces:**
- `fetch_enable_params()` returns one pattern per resource type: `Document`, `XHR`, `Fetch`, `Ping`, `EventSource`, `Other`, plus any type the guard's code already relies on. It never includes the static types.
- A small per-run file cache in `autorun/runner.rs` (or a sibling module), which reads a file again only when its modified time or size changed.
  - It must pass the cache tripwire. It is not a data cache in CLAUDE.md's sense: it is per run, held in memory and dropped with the run. If the tripwire objects, add a named `NOT_CACHES` entry with its reason.

- [ ] Tests:
  - `the_fetch_patterns_hold_save_capable_types_only`
  - `a_beacon_and_a_form_post_are_still_blocked`
  - `the_recipe_is_read_once_per_run`
  - `a_recipe_changed_between_steps_is_read_again`
- [ ] Run the tests one at a time: `save_guard::`, then `autorun_discovery::`, then `autorun_replay::`, then `cargo test --tests`. If a real Edge is available, also run `cargo test --test suite browser_live -- --ignored save` for the save-guard live tests.
- [ ] Commit `perf(v2): Must not save pauses only requests that can save, and a run reads its recipe and areas once`.

### Task 3: Step pictures not waited for (E)

**Files:**
- `autorun/replay.rs` (around line 518)
- `autorun/runner.rs` (`picture`)
- the driver or `browser/page.rs` capture, if the capture can be split into "send" and "collect"
- `autorun/store.rs` (`save_shot`)

**Interfaces:**
- **Writing pictures.** The disk write moves to `spawn_blocking`.
- **Splitting the capture.** If the CDP driver supports an outstanding command whose answer is collected later, split the capture into "ask" and "collect". If it does not, keep the capture awaited, move only the write, and say so in the report.
- **The barrier.** The next step's first page-changing action (click, fill, press, drag, navigate, reload, select) waits for any outstanding picture. A check or read does not wait.
- **Before the record is saved,** wait for every outstanding picture. A picture that failed or timed out is recorded as no picture.
- **Blocked saves.** `take_save_blocked` is still read after the step's last action, at the same point as today.

- [ ] Tests:
  - `the_next_click_waits_for_the_outstanding_picture`
  - `a_check_runs_while_the_picture_is_taken`
  - `the_record_waits_for_every_picture`
  - `a_dead_browser_with_a_picture_outstanding_saves_a_clean_record`
  - `a_failed_capture_leaves_no_picture`
- [ ] Run the tests one at a time: `autorun_replay::`, then `autorun_runner::` (or the matching suite), then `cargo test --tests`.
- [ ] Commit `perf(v2): a step's picture is taken without holding up the next step`.

### Task 4: Highlight each action, optional (F)

**Files:**
- the app settings (Rust settings struct, getter and setter command)
- `commands/autorun_replay.rs` (watched timing)
- the supervised browser's `Timing::default()` call sites (`commands/autorun.rs` around 778 and 880, `autorun/replay_to.rs` around 269)
- `src/screens/AutoRun/ReplayPane.tsx` (the tick box beside Watch the browser)
- the regenerated `src/bindings.ts`

**Interfaces:**
- The setting is `autorun_highlight: bool`, serde default true. It is set by a `set_autorun_highlight(on: bool)` command, or by the settings setter the app already uses for its other switches.
- `highlight_ms` is 350 when the setting is on and the browser is watched or supervised. Otherwise it is 0.
- The tick box is labelled "Highlight each action". It shows beside Watch the browser and is saved through the command. Use theme tokens only, and follow the pattern of the Watch the browser control.

- [ ] Tests:
  - Rust:
    - `highlight_defaults_on`
    - `highlight_off_skips_the_pause_for_a_watched_run`
    - `highlight_off_skips_the_pause_in_the_supervised_browser`
  - vitest:
    - `the_highlight_box_saves_the_setting`
- [ ] Run the focused tests first. Then run `cargo test --tests`, `cargo test --test bindings`, `npx tsc --noEmit` and `npm test`.
- [ ] The How To Use entry for the run options gets one sentence. Run `npm run docs:build` and `npx vitest run docs-site`. Retake no screenshots unless a positions test requires it.
- [ ] Commit `feat(v2): the highlight pause before each action can be turned off`.

### Final gate

- [ ] Run one at a time: `cd src-tauri && cargo test --tests`, then `npx tsc --noEmit`, `npm test` and `npm run build`.
- [ ] Hand check owed: one real unattended run of 3 cases, compared with the 2.1.2 phase timings.
