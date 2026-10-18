# Replays Without Re-signing In Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** an assistant's replay, discovery and reset reuse the one supervised browser and its sign-in, so releasing becomes a repair for a broken browser instead of a routine step.

**Architecture:** the save guard gets an owner and a lifetime (the call or discovery that set it); `replay_to_traced` skips the sign-in when the browser is already signed in as the script's account and goes to the area through `nav::reach_module`; discovery and replay hand the browser over instead of closing it; a new `reset_autorun_browser` tool cleans the page state without signing out; the guide changes with it.

**Tech Stack:** Rust (Tauri 2), the CDP driver in `src-tauri/src/browser`, tauri-specta bindings, vitest for the tool list.

**Spec:** `docs/superpowers/specs/2026-10-18-replay-reuse-browser-design.md`

## Global Constraints

- A save never gets through for a case whose script is marked `no_save`, nor during a discovery started with `no_save` (anti-goal).
- A replay never acts as another account or on a refused session to save time (anti-goal).
- The person's browser (`OpenedBy::Person`) is never closed, reset or taken over by the assistant (anti-goal).
- Unattended runs (`OneBrowser`), the recorder, template-held browsers and setup browsers are unchanged.
- No HTTP DELETE; no request of TCM's own to the application.
- Existing tests that pin today's behaviour or wording change with the behaviour; none is weakened to pass.
- New MCP tool listed everywhere `release_autorun_browser` is: `src-tauri/src/mcp.rs` (tool + route), the bridge route table in `ai_bridge.rs`, `AUTORUN_TOOLS` in `src-tauri/src/ai_tools.rs`, `src/lib/mcpTools.ts`, `tests/suite/tcm_mcp.rs` lists, `src/lib/mcpTools.test.ts`.
- Rust tests only in `src-tauri/tests/suite/`; one test run at a time; never `CARGO_TARGET_DIR`.

## Review Focus

1. A `no_save` discovery is running, the assistant replays a case NOT marked no_save → the guard stays on until the discovery ends (a replay inside a no_save discovery never saves). Test in Task 1.
2. The browser is signed in as account A; the script's account is B → full sign-in as B, never a reuse. Test in Task 2.
3. The saved session expired server-side ("Cookie has been tampered" / empty 400) → the reuse check sees it and signs in fresh. Test in Task 2.
4. A setup made way (`setup::make_way` set `Session.account = None`) → not signed in, full sign-in. Test in Task 2.
5. `reset_autorun_browser` on the person's browser, or while a replay runs → 409, nothing touched. Test in Task 4.

---

### Task 1: The save guard belongs to the call that set it

**Files:**
- Modify: `src-tauri/src/commands/autorun.rs` (`Session`: add `guard_owner: Option<GuardOwner>`; `guard_supervised` ~1672; `guard_for_case` ~1692; `used_otherwise`; the replay's `may_lift` ~1295)
- Modify: `src-tauri/src/ai_bridge.rs` (`discovery_for` ~4778, `mark_discovery_for` ~4805, `discovery_must_not_save` ~4819, `guard_a_no_save_discovery` ~3412, the write-backs at ~3335 and ~3691, `end_discovery_in` ~4294)
- Test: `src-tauri/tests/suite/autorun_replay_to.rs`, `autorun_discovery.rs`, `save_guard.rs`

**Interfaces:**
- Produces: `#[derive(Debug, Clone, PartialEq, Eq)] pub enum GuardOwner { Person, Discovery, Call { case_id: Option<i32> } }` in `commands/autorun.rs`; `pub(crate) fn lift_guard(session: &mut Session)` (stops guarding saves on the Cdp, clears `guarded_case` and `guard_owner`).

Rules:
- `start_autorun_discovery {no_save: true}` → `GuardOwner::Discovery`, kept until the discovery ends; `end_discovery_in` lifts a `Discovery` guard.
- A discovery action's or batch's own `no_save` (not the discovery's) → guarded for that call only (`Call`), lifted when the call ends unless the discovery or the case's script still needs it. `DiscoveryState.no_save` is no longer set from an action's `no_save` (`mark_discovery_for` stops OR-ing it in).
- The case's script `no_save` mark → guarded for every call on that case, as today.
- A replay (assistant): guards when its script says `no_save` or a `Discovery` guard is live; otherwise lifts a `Call` guard of the assistant's for another case (replacing today's `guarding_other_case` refusal for that case only); never lifts `Person`.

- [ ] **Step 1: failing tests:** `a_no_save_action_guards_only_its_own_call` (a discovery without no_save; action 1 with `no_save: true` is guarded; action 2 without it may save); `a_no_save_discovery_stays_guarded_until_it_ends` (Review Focus 1, a replay of an unmarked case inside it is still guarded; after `end_autorun_discovery` the next replay may save); `a_replay_lifts_a_guard_the_assistant_left_for_another_case`; `a_replay_never_lifts_the_persons_guard`; `ending_a_discovery_lifts_its_guard`. Update `an_assistants_replay_never_lifts_a_guard_held_for_another_case` to its new meaning (another case's **person** guard stays) - never deleted.
- [ ] **Step 2:** `cd src-tauri && timeout 1800 cargo test --test suite -- autorun_replay_to:: autorun_discovery:: save_guard::` → FAIL.
- [ ] **Step 3:** implement.
- [ ] **Step 4:** PASS, then the `autorun_` filter; commit `feat(v2): the save guard lasts for the call or discovery that set it`.

### Task 2: A replay reuses a signed-in browser

**Files:**
- Modify: `src-tauri/src/autorun/replay_to.rs` (`replay_to_traced` ~417: the sign-in at ~504 and the trip at ~526)
- Modify: `src-tauri/src/autorun/signin.rs` (a check, below)
- Test: `src-tauri/tests/suite/autorun_replay_to.rs`, `autorun_signin.rs` (or the module that tests `sign_in_leased`)

**Interfaces:**
- Produces: `pub async fn already_signed_in<D: Driver>(d: &mut D, recipe: &Recipe, signed_in_as: Option<&str>, wanted: &str, timing: &Timing) -> bool` in `signin.rs` - true only when `signed_in_as == Some(wanted)`, `session_refused(d)` is None, and the recipe's signed-in marker is visible within a short look (no navigation).
- Produces: the replay's answer line `Signed in already: went to <area>.` (verbatim), replacing the sign-in lines on a reuse.

On `ReplayFrom::Beginning`, after preconditions, setup and `guard_for_case`: if the script has an account and `already_signed_in(...)` with `Session.account` as `signed_in_as`, keep the lease (`Held::hold` with the same key is a no-op), skip `sign_in_leased`, and trip with `TripFrom::Elsewhere`; if that trip fails, sign in as today (`sign_in_leased`) and trip with `TripFrom::SignIn` once. Otherwise exactly today's path.

- [ ] **Step 1: failing tests:** `a_replay_in_a_browser_signed_in_as_its_account_skips_the_sign_in` (no Network/Input of the recipe's login, answer says `Signed in already: went to`); `another_account_signs_in` (Review Focus 2); `a_refused_session_signs_in` (Review Focus 3); `after_a_setup_made_way_it_signs_in` (Review Focus 4); `a_failed_trip_signs_in_once_and_goes_again`.
- [ ] **Step 2:** FAIL; **Step 3:** implement; **Step 4:** `autorun_replay_to::` and the `autorun_` filter PASS; commit `feat(v2): a replay goes straight to the area in a signed-in browser`.

### Task 3: Discovery and replay hand the browser over

**Files:**
- Modify: `src-tauri/src/ai_bridge.rs` (`make_way_for_discovery` ~3195, `end_discovery_for_replay` ~4361, `open_for_discovery` path), `src-tauri/src/commands/autorun.rs` (`open_for_discovery` ~380, `replay_supervised` ~1324)
- Test: `src-tauri/tests/suite/autorun_held_browser.rs`, `autorun_discovery.rs`

Rules: starting a discovery in a live browser the assistant's replay left (`OpenedBy::Assistant`, not a person's) no longer closes it: the discovery takes it over - other tabs closed (`close_other_tabs`), the stand and last page change forgotten (`used_otherwise`), the guard re-decided by Task 1's rules, the sign-in kept when it is the discovery's account (else signed in as the discovery's account). A replay after a discovery ends the discovery (mapping summary kept, as today) and keeps the browser. A dead browser is still let go first (`forget_gone_browser`).

- [ ] **Step 1: failing tests:** `a_discovery_takes_over_the_browser_the_replay_left` (replaces `a_discovery_closes_the_browser_the_assistants_replay_left_open`, which is rewritten to the new behaviour); `a_replay_after_a_discovery_keeps_the_browser`; `a_discovery_for_another_account_signs_in_as_it`; `a_dead_browser_is_still_let_go`.
- [ ] **Step 2:** FAIL; **Step 3:** implement; **Step 4:** `autorun_held_browser:: autorun_discovery::` and `autorun_` PASS; commit `feat(v2): discovery and replay keep the browser and its sign-in`.

### Task 4: `reset_autorun_browser`

**Files:**
- Modify: `src-tauri/src/mcp.rs` (tool `reset_autorun_browser`, no args; route `POST /autorun-reset`), `src-tauri/src/ai_bridge.rs` (route + handler), `src-tauri/src/commands/autorun.rs` (`reset_for_assistant`), `src-tauri/src/ai_tools.rs`, `src/lib/mcpTools.ts`
- Test: `src-tauri/tests/suite/autorun_held_browser.rs`, `tcm_mcp.rs`; `src/lib/mcpTools.test.ts`

**Interfaces:**
- Produces: answers verbatim - `Reset: other tabs closed, dialogs cleared, guard lifted, at the home page, still signed in as <account key>.` / `No browser is open.` (404) / `A replay is running: try again when it has finished.` (409) / the existing `PERSONS_BROWSER_OPEN` sentence (409).

Order: refuse while a replay runs (`RUNNING`/replay flag as `release_for_assistant` checks) or for `OpenedBy::Person`; `stop_batch()` before taking `SESSION` (the source test requires it); `forget_gone_browser` first (a dead one answers `No browser is open.`); end a running discovery of the assistant's (mapping summary kept); `close_other_tabs`; `dialog_book().disarm()` + `take_seen()`; `lift_guard` unless a case's script mark still applies (none: no case is in play); `used_otherwise`; `nav::load_home` (keeps cookies, waits for the signed-in marker); if the marker never shows, answer `Reset, but the sign-in has gone: the next replay signs in again.` and set `Session.account = None`.

- [ ] **Step 1: failing tests:** `a_reset_keeps_the_sign_in_and_goes_home`; `a_reset_of_the_persons_browser_is_refused` and `a_reset_during_a_replay_is_refused` (Review Focus 5); `a_reset_with_no_browser_says_so`; `a_reset_whose_sign_in_has_gone_says_so`; the tool is listed with the release tool everywhere (`tcm_mcp.rs`, `mcpTools.test.ts`); the `BATCH_STOP` source test lists `reset_for_assistant`.
- [ ] **Step 2:** FAIL; **Step 3:** implement; **Step 4:** the suites and `npx vitest run src/lib/mcpTools.test.ts` PASS, `npx tsc --noEmit`; commit `feat(v2): reset_autorun_browser - a clean page without signing out`.

### Task 5: The guide

**Files:**
- Modify: `src-tauri/src/autorun/guide.rs` (~163-165 the release line in the quick rules; ~1353-1364 the discovery section's release bullet; ~1766-1782 "Repairing a script that failed"; the `no_save` paragraphs ~157, ~354, ~934-957, ~1288, ~1321), `src-tauri/src/mcp.rs` descriptions of `release_autorun_browser`, `replay_autorun_to_step`, `start_autorun_discovery`, `discover_autorun_action(s)`
- Test: `src-tauri/tests/suite/autorun_guide.rs` (the assert at ~720-722 changes with the wording), `tcm_mcp.rs`

Meaning, in plain words: replays, discovery and resets share one browser and its sign-in; release only when the browser is broken (frozen, wrong account, session gone, "the browser did not answer"), never between replays; `reset_autorun_browser` for a clean page without a sign-in; a discovery's `no_save` lasts until it ends, an action's for that call, a case's mark for every call on that case. The quick rules stay within 45 lines.

- [ ] **Step 1: failing tests:** `the_guide_says_release_only_when_broken`; `the_guide_names_reset`; the old wording assert rewritten to the new sentence; quick rules `<= 45` still holds.
- [ ] **Step 2:** FAIL; **Step 3:** edit; **Step 4:** `autorun_guide:: tcm_mcp::` PASS; commit `docs(v2): the guide keeps the browser between replays`.

### Task 6: Gate

- [ ] `cd src-tauri && cargo test --tests`, then `npm test`, then `npm run build` - one at a time, all green.
- [ ] Owner hand checks: heal one case with three replays in a row - only the first signs in; a no_save discovery then a saving replay after `end_autorun_discovery` saves; `reset_autorun_browser` leaves the browser signed in at home.
