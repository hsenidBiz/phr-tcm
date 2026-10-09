# Auto Run Speed Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Measure every phase of an Auto Run case, then remove the idle time the profile found: about 9 to 18 s per case, without changing what a run checks.

**Architecture:**
- Each phase (open, sign-in, area trip, every step, close) is timed into the run record.
- The sign-in prompts are raced in one window after the app shell appears instead of waited out one by one.
- One browser serves a whole unattended run, with a fresh browser context per case.
- Screenshot pruning happens once per run.
- Waits and the click-stability check stop paying 100 ms polls.

**Tech Stack:** Rust (Tauri 2, CDP driver), React 19 + TS, vitest.

**Spec:** the owner-approved findings in chat (2026-10-09). The evidence is in the profile report, ranked gaps 1 and 3 to 6. Parallel runs (gap 2) are out of scope.

## Global Constraints

- **Run checks unchanged:** nothing a run checks changes. Every check, the Must not save guard, the dialog book, page-error and network logging, downloads, tabs and per-step screenshots keep working for every case.
- **PeoplesHR behaviour stays handled:**
  - the session modal can arrive late, so a fresh login still gives it a window;
  - wait for the app shell before anything else;
  - the sidebar flyout click races its animation, so stability is still checked across frames.
- **Recorded recipes:** they are not rewritten. The faster prompt handling applies to `when_visible` prompts in `after_sign_in` generally, so recorded recipes benefit too.
- **Test layout:**
  - Every Rust test is an integration test in `src-tauri/tests/suite/`, with a `mod` line.
  - A test touching process-wide state takes its lock from `suite/serial.rs`.
  - `src/bindings.ts` is generated only.
- **No HTTP DELETE** to Azure DevOps. **No secrets, hosts or query strings** in logs or records.
- **Shared machine:** run one build or test command at a time. No em or en dashes.
- **Commits:** a Bash heredoc, staged by name, ending with the implementer's `Co-Authored-By` line.

## Review Focus

1. A late session modal after a FRESH login must still be dismissed. Task 2 test: `a_late_session_modal_after_a_fresh_login_is_still_dismissed`.
2. A browser context's own downloads, dialogs, tabs and save guard must not leak into the next case. Task 3 test: `a_second_case_sees_none_of_the_first_cases_tabs_dialogs_or_downloads`.
3. A browser crash mid-run must not end the run. The next case relaunches. Task 3 test: `a_crashed_browser_is_relaunched_for_the_next_case`.
4. An element that appears then moves (an animation) must not be clicked mid-move. Task 5 test: `a_moving_element_is_not_clicked_until_it_settles`.
5. Old run records without phase timings still load and report. Task 1 test: `a_run_without_phase_timings_still_loads`.

---

### Task 1: Phase timings in the run record

**Files:**
- `autorun/mod.rs`: `CaseRecord` gains `#[serde(default)] phases: Option<CasePhases>`, and `StepRecord` gains `#[serde(default)] duration_ms: Option<u64>`.
- `autorun/replay.rs` (`run_case_in`, `one_go`) and the supervised runner, wherever a case is run.
- The run report, if it lists durations.

**Interfaces:**
- `pub struct CasePhases { pub open_ms: u64, pub sign_in_ms: u64, pub area_ms: u64, pub steps_ms: u64, pub close_ms: u64, pub total_ms: u64 }`.
  - `open_ms` and `close_ms` cover browser or context creation and teardown.
  - `total_ms` spans open to close.
  - u64 fields are exported as f64 where specta needs it.
- **The run report** shows one line per case, e.g. "Took 41.2 s: open 1.1, sign-in 12.4, area 3.0, steps 23.9, close 0.8". It shows each step's duration beside its outcome.
- **The applog** gets one info line per case, with the phase numbers only.

- [ ] Tests:
  - `a_case_record_carries_its_phase_timings`
  - `steps_carry_their_duration`
  - `a_run_without_phase_timings_still_loads`
  - Use the runner's fake driver and controllable clock if one exists; otherwise assert the fields are present and ordered.
- [ ] Implement. Run the focused suites, then `cargo test --tests` and `cargo test --test bindings`.
- [ ] Commit `feat(v2): an Auto Run case records how long each phase and step took`

### Task 2: Sign-in prompts raced, not waited out

**Files:** `autorun/signin.rs` (sign-in and `after_sign_in`), `autorun/nav.rs` (`go_home`, the module retry), `browser/actions.rs` (`when_visible`), and `autorun/builtin_recipe.json`.

**Interfaces:**
1. **Run `after_sign_in` as a race.**
   - First wait for the signed-in marker, the app shell, up to its normal timeout.
   - Then run every `when_visible` action in `after_sign_in` within ONE shared window, `PROMPT_WINDOW_MS = 1500` after a saved-session reuse or a trip home, and `FRESH_LOGIN_WINDOW_MS = 5000` after a fresh credential login (late session modal).
   - Poll all their selectors together. Handle each prompt that appears, once, in recipe order. End the window early when all are handled.
   - Non-`when_visible` actions in `after_sign_in` keep their order and behaviour, before and after the race as written.
2. **Sign-in steps' optional prompts** (cookie banner, "Continue here") are checked immediately, without waiting, when the login field is already visible. If not, they wait as today.
3. **`go_home`** and the module retry use the 1500 ms window.
4. **Constants:** `pub const PROMPT_WINDOW_MS: u64 = 1500; pub const FRESH_LOGIN_WINDOW_MS: u64 = 5000;` in `autorun/timing.rs`.

- [ ] Tests (fake driver):
  - `prompts_that_never_appear_cost_one_window_not_five`: total wait at most `PROMPT_WINDOW_MS` plus a small margin, asserted via the fake clock or the counted waits.
  - `a_late_session_modal_after_a_fresh_login_is_still_dismissed`: the modal appears at 3 s.
  - `a_saved_session_reuse_uses_the_short_window`.
  - `prompts_are_handled_in_recipe_order_when_several_appear`.
  - `a_trip_home_uses_the_short_window`.
- [ ] Implement. Run the sign-in, nav and runner suites, then `cargo test --tests`.
- [ ] Commit `perf(v2): sign-in prompts are watched together in one short window instead of waited out one by one`

### Task 3: One browser per run, a fresh context per case

**Files:** the unattended run path `autorun/replay.rs` (`one_go`), `commands/autorun_replay.rs` (launch and connect), `browser/launch.rs`, `browser/cdp.rs` (`Target.createBrowserContext` / `disposeBrowserContext`, and attaching to a target in a context), the close path in `commands/autorun.rs`, plus whatever binds the per-case watchers: page_log, net_record, dialogs, save_guard, downloads, tabs.

**Interfaces:**
- **Per run:** an unattended run launches ONE browser (one profile, deleted at the end of the run).
- **Per case:**
  - `Target.createBrowserContext` creates the context (cookies and storage start empty);
  - the case's page is created in that context, and every per-case watcher attaches to it;
  - the context is disposed when the case ends.
- **Sign-in:** the saved-session reuse keeps working by loading the account's saved cookies into the new context, the way it loads them today.
- **Crash:** if the browser dies, the case is recorded as today, and the next case relaunches the browser.
- **Supervised and replay-to-step browsers are unchanged.**

- [ ] Tests (fake CDP):
  - `a_run_launches_one_browser_and_a_context_per_case`
  - `a_second_case_sees_none_of_the_first_cases_tabs_dialogs_or_downloads`
  - `a_crashed_browser_is_relaunched_for_the_next_case`
  - `the_profile_is_deleted_once_at_the_end_of_the_run`
  - `a_saved_session_is_loaded_into_each_new_context`
- [ ] Implement. Run the replay, runner and browser suites, then `cargo test --tests`.
- [ ] Commit `perf(v2): an unattended run keeps one browser and gives each case a fresh context`

### Task 4: Prune screenshots once per run

**Files:** `autorun/store.rs` (the save-time prune around 694-733, `list_runs` around 421), `autorun/replay.rs`.

**Interfaces:**
- Saving a case no longer prunes.
- The run prunes once, after its last case is saved (and on Stop).
- The rules for what is protected are unchanged.

- [ ] Tests:
  - `saving_a_case_does_not_read_other_runs`, via a counter seam or by timing a fixture with many runs;
  - `a_run_prunes_once_at_the_end`;
  - `protected_shots_survive_the_end_of_run_prune`.
- [ ] Implement. Run the focused suites, then `cargo test --tests`.
- [ ] Commit `perf(v2): old screenshots are pruned once per run, not on every case save`

### Task 5: Faster waits and a one-call stability check

**Files:** `autorun/timing.rs:34-35`, `browser/actions.rs:~1505`, `browser/expect.rs:~279`, `browser/input.rs:~291-298, ~372`, `autorun/nav.rs:~659`.

**Interfaces:**
1. **The poll interval** drops from 100 ms to `POLL_MS = 40`, for every wait, expect and nav poll.
2. **Clicks and fills** check stability in ONE `Runtime.evaluate`. It resolves the element, reads its rect, waits two `requestAnimationFrame`s in the page, and reads the rect again. It is stable when the rects are equal. This replaces the two looks 100 ms apart. If the element is not stable, retry within the existing action timeout.
3. **The watched-run highlight pause** stays at 350 ms by default. Add a per-run "Fast" option only if it is trivial; otherwise leave it unchanged.

- [ ] Tests:
  - `a_moving_element_is_not_clicked_until_it_settles`
  - `a_still_element_is_clicked_after_one_check`
  - `waits_poll_at_the_new_interval`
- [ ] Implement. Run the browser and runner suites, then `cargo test --tests`.
- [ ] Commit `perf(v2): waits check every 40 ms and a click checks stability in one call`

### Final gate

- [ ] Run, one at a time: `cd src-tauri && cargo test --tests`, then `npx tsc --noEmit`, `npm test` and `npm run build`.
- [ ] **Hand check owed:** one real unattended run of 3 cases before and after, comparing the new phase timings.
