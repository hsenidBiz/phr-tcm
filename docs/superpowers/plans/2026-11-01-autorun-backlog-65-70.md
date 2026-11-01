# Auto Run backlog 65-70 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the six problems the first rewriters on 2.4.3-beta.10 hit (backlog 65-70).

**Architecture:** Each item is a contained change in `src-tauri/src/autorun/` (or the bridge / replay commands that call it), with its own integration tests in `src-tauri/tests/suite/` and its own guide text.

**Tech Stack:** Rust (Tauri 2), integration tests in one binary (`cargo test --test suite <module>::`).

**Spec:** `docs/autorun/backlog-2026-10.md`, section "The fix round on 2.4.3-beta.10" (items 65-70). Code map: the scratchpad findings handed to each implementer.

## Global Constraints

- Never send HTTP DELETE to Azure DevOps.
- No secrets, URLs, or paths in user-facing text, answers to assistants, or logs.
- Never hand-edit `src/bindings.ts`; regenerate with `cargo test --test bindings` if a specta type changes.
- Never weaken a gate test (`src/ui-consistency.test.ts`, the docs guard, role caps of 4000 characters in `src-tauri/src/autorun/roles/*.md`).
- Every Rust test is in `src-tauri/tests/suite/` (a module per file, declared in `suite/main.rs`); process-wide state takes its lock from `suite/serial.rs`.
- One build or test command at a time; never set `CARGO_TARGET_DIR` or `--target-dir`.
- Never stage `.test-cases/`.
- Guide text (`src-tauri/src/autorun/guide.rs`, `src-tauri/src/api_templates/guide.rs`, `mcp.rs` tool descriptions, `ai_tools.rs`) that states the old rule changes with the rule, in the same plain style.

## Rulings

- R1 (65): The cap rises from 60 to 100 active quirks. Past it, the existing refusal (which already names the best candidates to retire and the oldest others) stays. A repair's quirk refused at the cap is no longer one line among many: the save answer opens with it, quotes the refused quirk's text, and says to retire one and record it again with `record_quirk`. Cost if wrong: about 12 KB more run guide at worst.
- R2 (66): A `gives_up` entry is valid where the repair removes a check of that kind, even when another check takes its place. The "removes {lost}" test counts removed kinds (the multiset difference the per-entry check already uses), not the net change in number. The record shows the trade: `GivenUp` gains `replaced_by: Vec<String>` (the check kinds the repair added on that step, empty when none; omitted from JSON when empty, default when absent), and the report line adds " (replaced by KIND, KIND)" when not empty.
- R3 (67): An empty `"unchecked": []` means "none", in a patch and in a whole save: it is stored as no list (as `null` is). The refusal for a list of only invalid entries is unchanged.
- R4 (68): No log of the beta.10 resets exists on this machine, so the fix covers both likely causes: (a) a reset no longer loads the sign-in recipe's start page (on PeoplesHR that is the login page, whose load can end the session) while the sign-in stands; it loads the page the browser landed on after its last sign-in (recorded on the session), falling back to the start page when none is known; (b) when the sign-in is gone, the answer names why, in the same plain words the log uses (the reasons hold no URL or secret; verify that).
- R5 (69): A replay to past a case's last step (a whole-case replay) always starts from the beginning: no carry-on from a stand and no carry-across. Replays to an earlier step keep carrying on.
- R6 (70): Two narrow additions to the seen gate, nothing wider:
  - (a) A locator whose text/name/has_text is a prefix (at least 20 characters after folding) of a value an earlier step typed counts as typed, as the whole-words rule already does (lists and grids cut long names).
  - (b) In the clean-up step only, a locator inside the `then` of a `when_visible` guard that has never been seen saves, with a save note that it is checked the first time its guard fires. When a guard fires in a run or replay, its `then` targets that worked are filed as seen (matched 1, the script's area). Every other locator keeps its rule.

---

### Task 1 (65): quirk cap 100 and a refused repair quirk leads the answer

**Files:** `src-tauri/src/autorun/quirks.rs` (MAX_QUIRKS), `src-tauri/src/ai_bridge.rs` (the repair save's quirk loop near the comment "a quirk the list will not take"), guide text: `autorun/guide.rs` (~2862), `api_templates/guide.rs` (~306), the `record_quirk` description in `mcp.rs` (~845). Tests: `tests/suite/autorun_quirks.rs` (`assert_eq!(MAX_QUIRKS, 60)` and "60 active quirks" become 100), `tests/suite/tcm_mcp.rs:1458`, `tests/suite/autorun_bridge.rs` (new test).

- [ ] Test: `a_repair_quirk_refused_at_the_cap_leads_the_answer`: a full list (100 active), a repair save with `quirk` set. The save succeeds; the answer's first line starts `quirk not recorded` and quotes the quirk text and names `record_quirk`; the scripts are saved.
- [ ] Update the existing tests that pin 60 to pin 100. Run them and see them fail.
- [ ] Implement. Run `cargo test --test suite autorun_quirks:: autorun_bridge:: tcm_mcp:: autorun_forbidden_fixes::`.
- [ ] Commit `fix(autorun): quirks keep 100 per project; a refused repair quirk leads the answer`.

### Task 2 (66): gives_up accepts a check replaced by another kind

**Files:** `src-tauri/src/autorun/case_values.rs` (`check_repair_values`, the per-step count loop), `src-tauri/src/autorun/mod.rs` (`GivenUp`), `src-tauri/src/ai_bridge.rs` (where `GivenUp` is built and the "check given up" report line). Regenerate bindings if `GivenUp` is a specta type. Guide: the gives_up paragraph in `autorun/guide.rs`. Tests: `tests/suite/autorun_case_values.rs`, `tests/suite/autorun_bridge.rs`.

- [ ] Test `gives_up_naming_checks_replaced_by_another_kind_passes`: old step 5 has five `expect_visible`; new step 5 has five `expect_text same_as`; gives_up names the five `expect_visible`. It passes.
- [ ] Change `gives_up_naming_more_than_the_repair_removes_is_refused` (line ~1435) so it still refuses naming more than the removed kinds (e.g. naming two `expect_visible` when one was removed and none replaced) with the same message.
- [ ] Test (bridge): the saved `given_up` entry carries `replaced_by: ["expect_text"]` and the report line ends `(replaced by expect_text)`. An entry where nothing was added has no `replaced_by` in the JSON.
- [ ] Implement, run `cargo test --test suite autorun_case_values:: autorun_bridge:: autorun_guide::`, commit `fix(autorun): gives_up takes a check replaced by another kind and records what replaced it`.

### Task 3 (67): an empty unchecked list means none

**Files:** `src-tauri/src/autorun/store.rs` (`check_unchecked_items`) and wherever a step is normalized before writing (store an empty `Items` as `None`), `src-tauri/src/ai_bridge.rs` (`apply_patch_steps` if normalizing there is simpler). Tests: `tests/suite/autorun_store.rs` (`an_empty_unchecked_list_is_refused` becomes `an_empty_unchecked_list_means_none`), `tests/suite/ai_bridge.rs` (patch).

- [ ] Tests: a whole save with `"unchecked": []` saves and the written step has no `unchecked`; a repair patch with `"unchecked": []` clears a step's stale list exactly as `null` does.
- [ ] Implement, run `cargo test --test suite autorun_store:: ai_bridge::`, commit `fix(autorun): an empty unchecked list clears it`.

### Task 4 (68): a reset keeps the sign-in

**Files:** `src-tauri/src/commands/autorun.rs` (`reset_in_for_assistant` ~1863, `SIGN_IN_GONE_AT_RESET` ~1754), `src-tauri/src/autorun/replay_to.rs` (`sign_in_stands`, the `NotSignedIn` reasons ~920), the session struct that holds `signed_in`, `src-tauri/src/autorun/nav.rs` (`load_home` ~846), and the sign-in path that completes a sign-in (record the landing page there). Guide: `autorun/guide.rs` ~186 and ~1947. Tests: `tests/suite/autorun_replay_to.rs` (reset tests ~2347-3002).

- [ ] Test `a_reset_does_not_reload_a_login_page_that_ends_the_session`: a fake site whose start (login) page clears the session cookie when loaded while signed in; after a sign-in that lands on `/home`, a reset keeps the sign-in (answer says "still signed in as"), and the next replay does not sign in again.
- [ ] Test: with no landing page known, the reset loads the start page as before (existing tests keep passing).
- [ ] Test `a_reset_whose_sign_in_has_gone_says_why`: the gone answer includes the reason's plain words (e.g. the cookie-changed reason); check that no URL appears.
- [ ] Implement, run `cargo test --test suite autorun_replay_to:: autorun_held_browser:: autorun_bridge::`, commit `fix(autorun): a reset goes back to the signed-in page and says why a sign-in went`.

### Task 5 (69): a whole-case replay starts from the beginning

**Files:** `src-tauri/src/commands/autorun.rs` (~2482-2511, the `carry_from` match), `src-tauri/src/autorun/replay_to.rs` (`whole_case`). Guide: `autorun/guide.rs` ~2695-2712 (carry-on paragraph). Tests: `tests/suite/autorun_replay_to.rs`.

- [ ] Test `a_whole_case_replay_never_carries_on`: replay case C to step 4 (stand left), then replay C to last+1: the answer does not start "Carried on", steps 1-3 ran again, and `checked` is set.
- [ ] Test: replaying to `before == last+1` with a stand there runs from the beginning, not "already standing".
- [ ] Keep `a_replay_carries_on_from_where_the_last_one_stood` green (it replays to an earlier step).
- [ ] Implement, run `cargo test --test suite autorun_replay_to:: autorun_held_browser::`, commit `fix(autorun): a whole-case replay always starts from the beginning`.

### Task 6 (70): the seen gate takes a cut typed name and a guarded clean-up's then

**Files:** `src-tauri/src/autorun/seen_check.rs` (`exempt` ~1367, `typed_by`/`has_phrase`, `scan` ~2406, `guards` ~2333; save notes), `src-tauri/src/ai_bridge.rs` (the save notes), `src-tauri/src/autorun/replay_to.rs` (~1420 beside `file_component_finds`) and the run's guard path in `src-tauri/src/autorun/replay.rs` / `browser/repeat.rs` (file a fired guard's worked `then` targets), `discovery_map::record_matched_counts`. Guide: the seen-gate paragraph in `autorun/guide.rs`. Tests: `tests/suite/seen_check.rs`, `tests/suite/autorun_when_visible.rs`.

- [ ] Test `a_row_holding_the_start_of_a_typed_name_is_exempt`: step 2 fills a 150-character name; step 4 clicks `.row` has_text its first 60 characters. Saves. A 19-character prefix does not.
- [ ] Test `a_guarded_clean_up_then_saves_unseen_with_a_note`: clean-up `when_visible` (seen) with `then` click on a never-seen button saves and the answer notes it. The same `then` in a normal step is still refused.
- [ ] Test `a_fired_guard_files_its_then_as_seen`: a replay or run in which the guard fires and the `then` click works files the click's locator as seen matching 1 in the script's area.
- [ ] Implement, run `cargo test --test suite seen_check:: autorun_when_visible:: autorun_replay_to::`, commit `fix(autorun): the seen gate takes a cut typed name and checks a guarded clean-up when it fires`.

### Task 7: docs

- [ ] `docs/autorun/backlog-2026-10.md`: mark 65-70 "Done in 2.4.3-beta.11", each with one line on what was done.
- [ ] `docs/backlog.html`: add the owed hand checks (a reset on PeoplesHR keeps the sign-in; a whole-case replay starts over).
- [ ] Commit `docs(autorun): backlog 65-70 status`.
