# Auto Run backlog 44-53 (and what is still open before them) Implementation Plan

> **Owner decisions (2026-10-30), all as recommended:** Task 4 - under no_save stop any POST/PUT/PATCH/DELETE within 5 s of a save-named click, whatever its path (path only named). Task 7 - a named assistant's holds (incl. the main browser, only when an assistant opened it) are let go after 10 quiet minutes; a person's browser never. Task 8 - a `needs_records` list checked through saved setups' DB find rules before sign-in; missing -> Blocked; an undeclared record gets a note, not a refusal. Task 11 - browser reset of an expired test password only on a test environment with a new per-environment switch, off by default; the API route stays unbuilt. Naming: request/landing paths only (no host, no query), via save_guard::path_of.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build backlog items 44-53 from `docs/autorun/backlog-2026-10.md` (commit cba177cd) and the earlier Auto Run items the audit below finds still open or partly done (40, 46, 47, 49, 51).

**Architecture:** Each task is a small, self-contained change to the Rust Auto Run code (`src-tauri/src/autorun/`, `src-tauri/src/browser/`, the bridge `src-tauri/src/ai_bridge.rs`, the MCP tool texts `src-tauri/src/mcp.rs`), with its guide (`src-tauri/src/autorun/guide.rs`) and role (`src-tauri/src/autorun/roles/*.md`) text changed in the same task. Pure decisions live in pure functions the suite tests directly; the browser-facing part only calls them. Tasks 1-7 touch nothing the per-environment branch changes heavily and can land first; tasks 8-11 must be cut after `feat/per-env-autorun` merges.

**Tech Stack:** Rust (Tauri 2, tokio, serde, specta / tauri-specta, windows-sys 0.61), CDP over WebSocket, React 19 + TypeScript (vitest, jsdom).

**Spec:** `docs/autorun/backlog-2026-10.md` as on `origin/main` (read it with `git show origin/main:docs/autorun/backlog-2026-10.md`; the working tree's copy may be older), items 40, 44-53 and the "Smaller" section. Project rules: `CLAUDE.md`.

Line numbers below are `origin/main` at cba177cd.

---

## Part 1 - Audit

Every numbered item and every "Smaller" bullet, judged from the code on `origin/main`.

| # | Item | Status | Evidence (origin/main) |
|---|---|---|---|
| 1 | Dismiss-if-present in scripts | PARTLY | `when_visible` (`autorun/action_help.rs:72`), recipe `close_when_shown` (`autorun/popups.rs:1`, `builtin_recipe.json:20`). Left: the session prompt and "Performance Method Changed" alert are still guarded per script - a recipe list entry, no code. |
| 2 | Read-only guard on shared drafts | DONE | `CaseScript::no_save` `autorun/mod.rs:276`, `browser/save_guard.rs` (but see 45) |
| 3 | Preconditions before step 1 | DONE | `CaseScript::preconditions` `autorun/mod.rs:283`, `autorun/preconditions.rs` |
| 4 | Retry the module path once | DONE | `autorun/nav.rs:1522` `TRIED_TWICE`, `:1542-1590` |
| 5 | Download checks | DONE | `action_help.rs:85`, `autorun/downloads.rs:1-12` (xlsx/csv/pdf) |
| 6 | Replay to step N | DONE | `autorun/replay_to.rs`, `mcp.rs:721` |
| 7 | Re-run failed, retry transient | DONE | `autorun/transient.rs:51`, `autorun/replay.rs:1070`; re-run failed by filter by decision (`src/screens/RunPanel/index.tsx:273`) |
| 8 | Fixtures | DONE | `api_templates/fixture.rs`, `fixture_run.rs`, `fixture_store.rs` |
| 9 | Script setup runs a fixture | DONE | `CaseScript::setup` `autorun/mod.rs:298` |
| 10 | Clean up test-made drafts | DONE | `autorun/cleanup.rs:1,359`, `autorun/test_made.rs` |
| 11 | One session per account | DONE | `autorun/lease.rs:1` |
| 12 | Tabs | DONE | `action_help.rs:86-90` |
| 13 | Browser dialogs | DONE | `action_help.rs:91`, `browser/dialogs.rs`, `mod.rs:327` |
| 14 | Table checks | DONE | `action_help.rs:92-108`, `browser/table.rs` |
| 15 | Page errors as a check | DONE | `CaseScript::page_errors` `mod.rs:332`, `browser/page_errors.rs` |
| 16 | PDF downloads | DONE | `autorun/downloads.rs:1-12` |
| 17 | Reuse a signed-in browser for templates | DONE | `api_templates/held.rs:1` |
| 18 | Refusal fails the discovery action | DONE | `autorun/refusals.rs:1` |
| 19 | Remember refusals per area | DONE | `record_validation_rule` `mcp.rs:857` |
| 20-26 | Export hand-over to Playwright | DONE | `pw_export/handover.rs` (baseline, From TCM), `pw_export/test_case.rs:147` (assumed marker), `pw_export/app_quirks.rs:82` (`tcm:` tags), `pw_export/export.rs` (order notes, warnings, Uses) |
| 27 | Replays without re-signing in | DONE but one gap | per-call `no_save` `mcp.rs:747,762`, `reset_autorun_browser` `mcp.rs:800`, takeover `ai_bridge.rs:5101`. Gap: a second replay of the same case signs in again (the print is retaken only after the trip, `replay_to.rs:797`) - planned as 44b |
| 28 | Repairs edit preconditions | DONE | `PreconditionRepair` `mod.rs:418` |
| 29 | Request/response capture, network and cookie tools | DONE | `autorun/capture_tools.rs:1`, `capture_redact.rs`, `browser/session.rs` |
| 30 | Element inspection | DONE | `browser/inspect.rs`, `probe_autorun_locator` `show` `mcp.rs:687` |
| 31 | Shared-steps checkpoint | DONE | `replay_to.rs:1065` (`carry_across_why`), `:1528` |
| 32 | Menu preferences and mode | DONE | `autorun/menu_setup.rs:1,208`, `recipe::MenuMode` |
| 33 | Never bend a check | DONE | `autorun/divergences.rs`, `case_values.rs`, `floor.rs`, `forbidden_fixes.rs`, quirk classes `quirks.rs:600` |
| 34 | Script-a-PBI skill | DONE | `/tcm:gen-tests` `ai_tools.rs:682` (replaced `script-pbi`, `:1133`) |
| 35 | Per-role files, rules in answers | DONE | `autorun/roles.rs:183-184`, `roles/*.md` |
| 36 | Timing on a request | DONE | `browser/actions.rs:211-214`, `autorun/api_checks.rs:64-77` |
| 37 | page_until, as written, same_as, absence | DONE | `browser/page_until.rs`, `action_help.rs:41`, `browser/locator.rs:625` |
| 38 | Assistant rules a divergence | DONE | `mcp.rs:890`, `divergences.rs:133` (undo) |
| 39 | Unchecked, forced click, armed checks | DONE | `autorun/unchecked.rs`, `browser/no_change.rs`, `api_checks.rs:433` |
| 40 | Expired passwords, active sessions, Check sign-in | OPEN | no expiry query, no reset-page detection, no active-session check, no per-account Check sign-in anywhere in `src-tauri/src` or `src/` (the only `check_sign_in`, `commands/autorun_record_signin.rs:164`, proves a recorded recipe) |
| 41 | First three-writer run | DONE (2.4.2-beta.2) | `action_help.rs:1-24` (refusal, waits, call budget), discovery area walk `mcp.rs:732`, `tcm_mcp.rs:3931` agent-list test |
| 42 | Second three-writer run | DONE (2.4.2-beta.3) | `save_autorun_area` `variant` `mcp.rs:783`, `autorun/page_compact.rs`, probe `html` `mcp.rs:697` |
| 43 | Data is made, not held back | DONE (2.4.2-beta.3) | `roles/writer.md` data bullet |
| 44 | Chrome Password Manager prompts | OPEN | `browser/launch.rs:103-118`: no `--disable-features`, no profile Preferences |
| 44b | Second replay of a case signs in again | OPEN | print retaken only after the trip (`replay_to.rs:797`) and after discovery calls (`ai_bridge.rs:5959`), never after a replay's steps |
| 45 | `no_save` lets `/wizard` saves through | OPEN | the guard judges the path's words only (`save_guard.rs:65-79`); `POST /hr/PMSV10/wizard` has none |
| 46 | Edit clears suspected-defect mark | PARTLY | the save says it cleared it (`ai_bridge.rs:13114-13123`) but clears on any edit of the step (`ai_bridge.rs:12769`) |
| 47 | Clean-up after a failing check | PARTLY | a clean-up step that runs after a failed step exists (`mod.rs:185-192`, `guide.rs:366-380`); no rule tells writers to put deletes there, and the save says nothing about a delete after a check |
| 48 | Positional deletes on shared lists | OPEN | no save note for `nth` on a delete; not in `roles/writer.md` |
| 49 | Kept data not declared | PARTLY | `preconditions::undeclared_record_note` (`autorun/preconditions.rs:144-215`) notes prefix-named records, only when there are no preconditions and no setup; the only declaration is an API-flow precondition; nothing blocks a run on missing kept data |
| 50 | Sign-in on a cold site | OPEN | `autorun/signin.rs:509-531`: one marker wait, no retry, failure does not say where the browser ended |
| 51 | Holds outlive their agent | PARTLY | extra slots idle-released at 30 min (`commands/autorun_slots.rs:388,417`), claims 20 min (`autorun/claims.rs:25`); the main browser never; `reset_autorun_browser` lists no holds |
| 52 | Hung orphan browser | OPEN | `browser/tree.rs:202-209` sweep "never looks at processes"; the offscreen failure (`autorun/patterns.rs:135`) suggests nothing |
| 53 | Which logins reach an area | OPEN | needs the Check sign-in of item 40, which does not exist |
| S1 | MCP save drops `edits` | DONE | `mcp.rs:648` |
| S2 | `list_api_templates` too large | DONE | filter and paging `mcp.rs:1140-1147` |
| S3 | Iframe follow-ups | DONE but a measurement | `css_quoted` `browser/snapshot.rs:800-816`; `ErrorClass::FrameUnreachable` `patterns.rs:75`; clipping re-centred `browser/input.rs:90-109`; sentence only when no frame entered `browser/locator.rs:615`; depth note `snapshot.rs:381,390`; `check_text` reads frames `guide.rs:298`. Left: the per-match round trip was never measured (no code) |
| S4 | Cookie banner | DONE | recipe `close_when_shown`; discovery closes it since 41 |

**Counts:** 53 numbered items: 41 DONE, 5 PARTLY (1, 46, 47, 49, 51), 7 OPEN (40, 44, 45, 48, 50, 52, 53), plus 44b (from 27/44) OPEN. Smaller: 4 DONE (S3 leaves a measurement).

### Not in this plan, and why

- **1 (remaining prompts)** - a recipe change (`close_when_shown` / `after_sign_in` entries), no code. Do it on a live run.
- **S3 (frame round trip)** - a measurement on a live page, no code until it shows a cost.
- **41/42 live leftovers** (record My Assessments' slow load and the cycle-selection race as quirks, 144580 not reproduced, the upstream hand-back refusal) - live-run quirks or Claude Code's, no TCM code.
- **Owed hand checks** in `docs/backlog.html` (three-writer run, menu shapes, overlays, reset keeping the sign-in, office, Halloween, all-projects phases) - live-org checks, not code.
- **Manage Cycle extras** ("yours to pick" in `docs/backlog.html`) - the owner has not picked them.
- **40's API password reset** (`PasswordResetVerification` with RSA) - the backlog says prove it as an API template first; Task 11 builds the browser route the backlog names as the fallback.

---

## Global Constraints

- Rust tests only in `src-tauri/tests/suite/`, one module per file, each declared with a `mod` line in `src-tauri/tests/suite/main.rs`; never a `#[cfg(test)]` module in `src/`, never a new file directly under `tests/`.
- A test that touches process-wide state (statics, env vars, the log tail, the agents registry, the pacer) takes its lock from `tests/suite/serial.rs` (`serial::autorun()`, `serial::agents()`, `serial::log_tail()`, ...).
- `src/bindings.ts` is regenerated with `cd src-tauri && cargo test --test bindings`, never hand-edited.
- One build or test command at a time on this machine; wait for it. One build cache, `src-tauri/target`; never `CARGO_TARGET_DIR` / `--target-dir`.
- Run one module: `cd src-tauri && cargo test --test suite <module>::`. Frontend: `npx vitest run <file>`; typecheck `npx tsc --noEmit`.
- User-facing errors and log lines name no host, no query string and no full URL; a transport error never reaches the user through `to_string()` (use `ado/transport.rs` sentences). Where an item asks to name a request or a landing page, name the path only, through `browser::save_guard::path_of` (as the guard's sentences already do). No password, token or cookie value in any answer, log or error.
- Role files (`src-tauri/src/autorun/roles/*.md`) stay within `ROLE_MAX_LINES` = 45 and `ROLE_MAX_CHARS` = 4000 (`autorun/roles.rs:183-184`); `writer.md` has about 700 characters left, `healer.md` about 500.
- The guide (`autorun/guide.rs`), the role files, the MCP tool descriptions (`mcp.rs`) and the skill texts (`ai_tools.rs`) say the same thing about any behaviour a task changes: change them in the same task.
- Never weaken a gate test (`ui-consistency.test.ts`, `tests/bindings.rs`, `ado_network.rs`, the role caps, `tcm_mcp.rs::every_tool_routed_through_a_browser_slot_takes_an_agent`). A new tool that takes a browser slot goes in the agent lists that test reads.
- No Azure DevOps DELETE, ever. No new DELETE anywhere.
- The changelog (`src/lib/changelog.ts`) is end-user facing: no repo paths, no backlog numbers, no dev-process lines. The version lives in three places (`tauri.conf.json`, `Cargo.toml`, `changelog.ts`); this plan does not bump it.
- Never name the hidden features (the equalizer easter egg, the secret unlock) in code comments, commits, changelog or docs.
- Colours in UI code come from tokens (`text-text`, `bg-surface`...); icons from `src/lib/actionIcons.ts`.
- Commits: Bash heredoc, `git commit -F - <<'EOF' ... EOF`, ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; confirm with `git log -1`.

**Merge order with `feat/per-env-autorun`:** that branch rewrites `autorun/store.rs`, `autorun/roots.rs`, `environments.rs`, `commands/environments.rs`, `lib.rs`, `backup.rs`, `commands/misc.rs` and touches `autorun/recipe.rs` (33 lines), `autorun/accounts.rs`, `autorun/mod.rs`, `commands/autorun.rs`. Tasks 1-7 touch none of the heavy files. Tasks 8-11 are marked **[after per-env]**: cut them from `main` only after it merges.

## Review Focus

1. **A browser profile whose Preferences cannot be written or read** (locked, corrupt, a folder in its place): the browser must still start; the prompts are a nuisance, a refused launch stops every run. Pinned by Task 1's `password_prefs_replace_unreadable_file` and `launch_goes_on_when_prefs_cannot_be_written`.
2. **A Chrome or Edge that is not a hung Auto Run browser** (the person's own browser, an answering Auto Run browser of this app, a child process of one): never ended. Pinned by Task 2's `orphan_of_ignores_other_profiles`, `orphan_of_ignores_child_processes`, `close_hung_keeps_an_answering_orphan`.
3. **The no-save window after a save-named click stopping what is not a save**: a GET, a click on "Search" or "Next", the sign-in's own posts. Pinned by Task 4's `click_saves_false_for_search_next_and_address`, `verdict_lets_reads_through_in_the_window`, `sign_in_posts_are_never_stopped`.
4. **A sign-in retry that types the password twice or retries what is not slowness** (a refused session, a browser that stopped answering, the reset page). Pinned by Task 3's `refused_session_is_not_retried`, `harness_failure_is_not_retried`, `retry_reloads_and_never_types_again`.
5. **Idle release taking a browser someone is using** (the person's main browser, one with a replay or a call going). Pinned by Task 7's `main_slot_opened_by_person_never_idle_released`, `slot_with_replay_going_is_not_idle`.

---

## Task 1: Chrome's password prompts never show (44)

**Files:**
- Modify: `src-tauri/src/browser/launch.rs` (`launch_args` `:103`, `args_with` `:122`, `launch_with` `:366`)
- Modify: `src-tauri/src/autorun/guide.rs` (one sentence in "Refreshing, sessions and the keyboard", `:414`)
- Test: `src-tauri/tests/suite/browser_launch.rs`

**Interfaces:**
- Produces: `pub const PASSWORD_FEATURES_OFF: &str = "PasswordLeakDetection";`
- Produces: `pub fn password_prefs(existing: Option<&str>) -> serde_json::Value` - `existing` is the file's text when there is one; the answer is that JSON (or `{}` when absent or not a JSON object) with `credentials_enable_service: false`, `profile.password_manager_enabled: false`, `profile.password_manager_leak_detection: false` set, every other key kept.
- Produces: `pub fn write_password_prefs(profile_dir: &Path) -> std::io::Result<()>` - creates `<profile_dir>/Default/` and writes `Default/Preferences` from `password_prefs`.

- [ ] **Step 1: Write the failing tests** in `browser_launch.rs`:
  - `launch_args_disable_password_leak_detection_once`: `launch_args(9222, dir)` has exactly one arg starting `--disable-features=`, and it contains `PasswordLeakDetection`; the last arg is still `about:blank`.
  - `args_with_folds_extra_disable_features_into_one_switch`: `args_with(9222, dir, &["--disable-features=Foo"])` has exactly one `--disable-features=` arg, equal to `--disable-features=PasswordLeakDetection,Foo`; last arg `about:blank`.
  - `password_prefs_on_empty_profile`: `password_prefs(None)` gives `v["credentials_enable_service"] == false`, `v["profile"]["password_manager_enabled"] == false`, `v["profile"]["password_manager_leak_detection"] == false`.
  - `password_prefs_merge_keeps_existing_keys`: input `{"other":1,"profile":{"name":"x","password_manager_enabled":true},"credentials_enable_service":true}` keeps `other == 1` and `profile.name == "x"`, and sets all three to false.
  - `password_prefs_replace_unreadable_file`: input `"not json"` and input `"[1,2]"` each give the three keys false.
  - `write_password_prefs_creates_default_preferences`: on a temp dir, the file `Default/Preferences` exists and parses with the three keys false; a second call on that file keeps a key written between the calls.
  - `launch_goes_on_when_prefs_cannot_be_written`: with `Default` created as a FILE (so the folder cannot be made), `write_password_prefs` returns `Err` - and `launch_with` must treat that as a log line, not a failure (assert on the helper `prefs_failure_is_logged_only(&e) -> String` that it names no path: does not contain the temp dir).
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite browser_launch::` - expect FAIL (functions not defined).
- [ ] **Step 3: Implement** the three functions above plus `pub fn prefs_failure_is_logged_only(e: &std::io::Error) -> String` (the log line: "Auto Run could not switch off the browser's password prompts: <kind>", no path). `launch_args` gains `format!("--disable-features={PASSWORD_FEATURES_OFF}")` before the start page; `args_with` merges any `--disable-features=` in `extra` into that one switch (comma-joined, order: ours first) - Chromium honours only one. `launch_with` calls `write_password_prefs(&profile_dir)` after `fresh_profile()` and before spawning; an `Err` is `applog::warn(prefs_failure_is_logged_only(&e))` and the launch goes on. Unattended runs' browser contexts (`one_browser`) use the same launched profile, so nothing more is needed there.
- [ ] **Step 4: Guide**: one sentence: the Auto Run browser starts with the browser's own password saving and breach warnings off, so no "Save password?" or "Change your password" bubble covers the page. Update any guide test that pins the section.
- [ ] **Step 5: Run** `cd src-tauri && cargo test --test suite browser_launch::` then `page_size::` (also uses `launch_args`) - expect PASS.
- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/browser/launch.rs src-tauri/src/autorun/guide.rs src-tauri/tests/suite/browser_launch.rs
git commit -F - <<'EOF'
fix(v2): the Auto Run browser never shows its password save or breach prompts

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

Hand check owed (add to `docs/backlog.html` in Task 12): Chrome and Edge on the Philippines dev site after a weak-password sign-in show no Password Manager bubble.

---

## Task 2: A hung browser left from an earlier run is closed (52)

**Files:**
- Modify: `src-tauri/src/browser/tree.rs` (beside `sweep_leftover_profiles` `:209`; the Windows module that already has `command_line` `:616` and `TerminateProcess` `:776`)
- Modify: `src-tauri/src/browser/launch.rs` (`launch_with` `:366`)
- Modify: `src-tauri/src/browser/input.rs` (`OFFSCREEN` `:360`)
- Modify: `src-tauri/src/ai_bridge.rs` (discovery / try / replay failure answers)
- Test: `src-tauri/tests/suite/browser_tree.rs`, `src-tauri/tests/suite/browser_launch.rs`, `src-tauri/tests/suite/autorun_discovery.rs`

**Interfaces:**
- Produces in `tree.rs`: `pub struct Orphan { pub pid: u32, pub profile: String, pub port: Option<u16> }` (`profile` is the folder NAME, `tcm-autorun-<digits>`).
- Produces: `pub fn orphan_of(command_line: &str, temp: &Path) -> Option<(String, Option<u16>)>` - pure: the browser process (no `--type=` switch) whose `--user-data-dir` (quoted or not) is `<temp>\<name>` with `is_profile_name(name)`; the port from `--remote-debugging-port=`.
- Produces: `pub fn find_orphans(temp: &Path) -> Vec<Orphan>` - Windows: a `TH32CS_SNAPPROCESS` walk, each process's command line read as `command_line` does, minus every profile in `held_profiles()`; empty off Windows.
- Produces: `pub fn end_orphan(o: &Orphan) -> bool` - ends every process whose command line `carries_profile` that profile; true when none is left within `END_WITHIN`.
- Produces in `launch.rs`: `pub fn close_hung(orphans: Vec<Orphan>, answers: &dyn Fn(u16) -> bool, end: &mut dyn FnMut(&Orphan) -> bool) -> Vec<String>` - for each orphan with a port that does not answer, `end`; returns the profile names closed. An orphan that answers, or has no port, is left. `pub fn devtools_answers(port: u16) -> bool` - `GET http://127.0.0.1:<port>/json/version` with a 1500 ms connect+read limit.
- Produces in `input.rs`: `pub const OFFSCREEN_RESET_HINT: &str = " If it stays outside the visible part once the page settles, reset_autorun_browser gives a clean page with the sign-in kept.";` and `pub fn with_offscreen_hint(detail: &str) -> String` (appends the hint once, only when `detail` contains `OFFSCREEN`).

- [ ] **Step 1: Failing tests** in `browser_tree.rs`:
  - `orphan_of_reads_profile_and_port`: `"chrome.exe --remote-debugging-port=51234 --user-data-dir=C:\\T\\tcm-autorun-51234 about:blank"` with temp `C:\T` gives `("tcm-autorun-51234", Some(51234))`.
  - `orphan_of_handles_quoted_dir`: `--user-data-dir="C:\\T\\tcm-autorun-7"` gives `("tcm-autorun-7", None)` when no port switch.
  - `orphan_of_ignores_child_processes`: the same with `--type=renderer` gives `None`.
  - `orphan_of_ignores_other_profiles`: another temp dir, `tcm-autorun-x1`, the person's `User Data` gives `None` each.
  In `browser_launch.rs`:
  - `close_hung_ends_only_silent_ones`: orphans A (port 1, silent), B (port 2, answers), C (no port); `close_hung` calls `end` for A only and returns `["tcm-autorun-1"]`.
  - `close_hung_keeps_an_answering_orphan`: B alone, `end` never called, empty answer.
  In `autorun_discovery.rs`:
  - `offscreen_failure_suggests_reset_once`: `with_offscreen_hint("button X is outside the visible part of the page")` ends with `OFFSCREEN_RESET_HINT`; applying it twice adds it once; a detail without `OFFSCREEN` is unchanged.
- [ ] **Step 2: Run** `cargo test --test suite browser_tree::` then `browser_launch::` then `autorun_discovery::offscreen` - FAIL.
- [ ] **Step 3: Implement.** `launch_with` calls `close_hung(find_orphans(&long_temp_dir()), &devtools_answers, &mut end_orphan)` before `fresh_profile()`; each closed one is logged `applog::warn("Auto Run closed a browser left from an earlier run that did not answer (profile <name>)")` - the folder name only. The bridge's discovery, try and replay failure lines pass through `with_offscreen_hint`.
- [ ] **Step 4: Run** the three modules again - PASS.
- [ ] **Step 5: Commit** (`fix(v2): a hung Auto Run browser left from an earlier run is closed before a new one starts`).

---

## Task 3: Sign-in survives a cold site and a second replay keeps it (50, 44b)

**Files:**
- Modify: `src-tauri/src/autorun/signin.rs` (`SignInOutcome` `:24`, `sign_in_held` `:389`, marker wait `:509-531`)
- Modify: `src-tauri/src/autorun/replay_to.rs` (end of a replay; `retake_sign_in_for` `:995`)
- Modify: `src-tauri/src/ai_bridge.rs` (move `call_touches_sign_in` used at `:5976` into `replay_to.rs`)
- Modify: `src-tauri/src/autorun/guide.rs` ("Repairing a script that failed": a replay of the same case again needs no reset)
- Test: `src-tauri/tests/suite/autorun_signin.rs`, `src-tauri/tests/suite/autorun_replay_to.rs`

**Interfaces:**
- Produces in `signin.rs`: `SignInOutcome` gains `pub landed: Option<String>` (the page's path at the end, via `save_guard::path_of`, never host or query).
- Produces: `pub fn landing_words(current_url: &str, start_url: &str) -> String` - `"the browser ended on <path>"`, plus `" - another portal than the sign-in's (/<a>, not /<b>)"` when the first path segments differ.
- Produces: `pub const COLD_SITE_RETRY: &str = "the signed-in marker did not show; reloaded once (a slow site)";`
- Produces in `replay_to.rs`: `pub fn touches_sign_in(actions: &[Action], recipe: &SignInRecipe) -> Option<String>` (moved, unchanged; ai_bridge calls it).
- Consumes: `retake_sign_in_for(d, root, org, project, lease, by)` (`replay_to.rs:995`), `signin::already_signed_in_why` (`signin.rs:217`).

- [ ] **Step 1: Failing tests** in `autorun_signin.rs` (with the file's fake driver):
  - `cold_site_marker_retried_once_after_reload`: marker absent on the first wait, present after a reload: outcome ok, one `reload` among the steps, the log has `COLD_SITE_RETRY`, the password was typed once.
  - `retry_reloads_and_never_types_again`: marker never shows: exactly one reload, the login fields filled once, detail starts with `MARKER_NEVER_APPEARED` and contains `the browser ended on /hr/security/login`.
  - `refused_session_is_not_retried`: the page reports a refused session: no reload, `session_refused_again` sentence as today.
  - `harness_failure_is_not_retried`: the marker check answers as a harness failure: no reload, `harness` true.
  - `landing_words_names_path_only`: `landing_words("https://x.test/sshr/home?t=1", "https://x.test/hr/security/login")` == `"the browser ended on /sshr/home - another portal than the sign-in's (/sshr, not /hr)"`; it contains neither `x.test` nor `t=1`; same portal gives no " - another portal" part.
  - `signed_in_in_another_portal_says_so`: marker shows on `/sshr/home`: ok stays true, `landed == Some("/sshr/home")`, detail ends with the portal words.
  In `autorun_replay_to.rs`:
  - `second_replay_of_same_case_keeps_the_sign_in`: the fake re-issues one HttpOnly cookie during step 1; replay 1 to the whole case, then replay 2 of the same case: no second sign-in, answer contains `signed_in_already_line(<area>)`.
  - `replay_that_signs_in_itself_is_not_reprinted`: a script with a `sign_in` action: after replay 1 the print is not retaken; replay 2 signs in (as today).
- [ ] **Step 2: Run** `cargo test --test suite autorun_signin::` then `autorun_replay_to::` - FAIL.
- [ ] **Step 3: Implement.** In `sign_in_held`, when the marker check fails, is not a harness failure and the page is not a refusal: run `Action::Reload` once, log `COLD_SITE_RETRY`, wait for the marker again (`timing.nav_ms`); only then fail, appending `landing_words` to the failure. Set `landed` on every outcome. A success whose `landing_words` names another portal stays ok, with the words after "signed in as X". In `replay_to`, after a replay's steps end (stopped before a step, or the whole case) with the browser signed in as the case's account and `touches_sign_in(&steps' actions, &recipe)` is `None`, and `already_signed_in_why` is ok: `retake_sign_in_for(..., "the replay's steps")`.
- [ ] **Step 4: Guide**: in "Repairing a script that failed": replaying the same case again needs no reset - the replay keeps its sign-in; a sign-in that stops says where the browser ended (a slow site is reloaded once first).
- [ ] **Step 5: Run** both modules, then `autorun_discovery::` and `autorun_session_refused::` (they use `signin`) - PASS.
- [ ] **Step 6: Commit** (`fix(v2): a sign-in waits out a cold site once and says where it ended; a second replay keeps the sign-in`).

---

## Task 4: A no-save guard stops what a save button sends (45)

**Files:**
- Modify: `src-tauri/src/browser/save_guard.rs`
- Modify: `src-tauri/src/browser/cdp.rs` (tab guard `:267`, `answer_paused` `:1562-1612`, `Driver` trait `:2948-2990`)
- Modify: `src-tauri/src/browser/actions.rs` (the `Click` execution)
- Modify: `src-tauri/src/ai_bridge.rs` (discovery answer's written requests, near `net_posted_paths` use)
- Modify: `src-tauri/src/autorun/guide.rs` ("Scripts that must not save" `:1113-1140`), `src-tauri/src/mcp.rs` (`discover_autorun_actions` and `start_autorun_discovery` texts)
- Test: `src-tauri/tests/suite/save_guard.rs`

**Interfaces:**
- Produces in `save_guard.rs`: `pub const SAVE_CLICK_WINDOW_MS: u64 = 5_000;`
- Produces: `pub fn click_saves(target: &Target, patterns: &[String]) -> bool` - any link's `name`, `text` or `css` (id and class tokens) holds a `SAVE_WORDS` word or a project word as a phrase, or a `WORD_SAVES` word as a word of its own (`adds`).
- Produces: `pub fn verdict(method: &str, url: &str, patterns: &[String], save_click_open: bool) -> bool` - `is_save(..) || (save_click_open && SAVE_METHODS.contains(method))`.
- Produces: `pub fn let_through_line(posted: &[(String, String)]) -> Option<String>` - each `(method, path)`, path as `path_of` gives it; `"went through the no-save guard (no save word in the path): POST /a, PUT /b - if one of them saves, a person adds a save word on the Auto Run Setup tab"`; `None` for none.
- Produces on `Driver`: `fn open_save_click_window(&mut self)` (default: nothing) and `Cdp`'s own: sets the guarded tab's `save_click_until = now + SAVE_CLICK_WINDOW_MS`; `answer_paused` uses `verdict(.., save_click_until > now)` for the tab guard and the `armed` path. The sign-in's hold (`hold_saves`) still wins: nothing is stopped while it holds.

- [ ] **Step 1: Failing tests** in `save_guard.rs`:
  - `click_saves_on_save_and_continue_button`: `{"role":"button","name":"Save & Continue"}` -> true; `{"text":"Submit"}` -> true.
  - `click_saves_on_css_id`: `"#btnSaveContinue"` -> true; `".btn-add-row"` -> true.
  - `click_saves_false_for_search_next_and_address`: `"Search"`, `"Next"`, `"Address book"`, `"#padding"` -> false.
  - `click_saves_project_word`: name `"Finish"` with patterns `["finish"]` -> true; without -> false.
  - `verdict_blocks_any_write_in_save_click_window`: `verdict("POST", "https://x/hr/PMSV10/wizard", &[], true)` -> true; `false` window -> false.
  - `verdict_lets_reads_through_in_the_window`: GET and HEAD with the window open -> false.
  - `let_through_line_names_method_and_path_only`: `[("POST", "/hr/PMSV10/wizard")]` gives a line containing `POST /hr/PMSV10/wizard` and no host or query; an empty list gives `None`.
  - `sign_in_posts_are_never_stopped`: with the file's existing paused-request harness, a POST during `hold_saves(true)` with the window open is continued.
  - `wizard_post_after_save_click_is_stopped`: same harness: guard on, `open_save_click_window()`, a paused `POST /hr/PMSV10/wizard` is answered `Fetch.failRequest` and the tab's `blocked` sentence is `blocked("POST", url)`; the same POST with no window is continued.
- [ ] **Step 2: Run** `cargo test --test suite save_guard::` - FAIL.
- [ ] **Step 3: Implement** the functions; in `actions.rs`, before dispatching a `Click` (not `expect_no_change`, which already fails on any write) on a driver that `is_guarding_saves()`, call `open_save_click_window()` when `click_saves(selector, <the guard's patterns>)`. In the bridge, a guarded discovery or try answer adds `let_through` (from `let_through_line`) for writing requests the guard continued.
- [ ] **Step 4: Guide and MCP texts**: a click on a control named like a save (Save, Save & Continue, Submit, Add...) also stops every POST, PUT, PATCH or DELETE the page sends in the next 5 seconds, whatever its path; a write that went through is named in `let_through`.
- [ ] **Step 5: Run** `save_guard::`, then `autorun_discovery::`, then `autorun_guide::` - PASS.
- [ ] **Step 6: Commit** (`fix(v2): a must-not-save browser stops what a save button sends, whatever its address`).

---

## Task 5: A repair that leaves a step's checks alone keeps its defect mark (46)

**Files:**
- Modify: `src-tauri/src/autorun/defects.rs`
- Modify: `src-tauri/src/ai_bridge.rs` (`repaired_marks` `:12769`, report `:13114-13131`)
- Modify: `src-tauri/src/autorun/roles/case-rewriter.md` only if it says a repair clears the mark (check; keep within the cap)
- Test: `src-tauri/tests/suite/autorun_defects.rs`

**Interfaces:**
- Produces in `defects.rs`: `pub fn checks_changed(old: &StepScript, new: &StepScript) -> bool` - the ordered list of `Action::each()` items with `is_check()`, compared as JSON; a step missing on either side is changed.
- Produces: `pub fn kept_line(case_id: i32, step: i32) -> String` - `"case {case_id}: suspected defect at step {step} kept - the repair left that step's checks as they were"`.

- [ ] **Step 1: Failing tests** in `autorun_defects.rs`:
  - `checks_changed_false_when_only_a_click_locator_changes`.
  - `checks_changed_true_when_a_check_value_changes` (an `expect_text` `equals` differs).
  - `checks_changed_true_when_a_check_is_removed`.
  - `repair_of_a_locator_keeps_the_mark`: a script marked at step 8, a save with `edits` `{steps:[8]}` changing only a click's locator: the mark is still on disk and the answer contains `kept_line(case, 8)`.
  - `repair_of_the_check_clears_the_mark`: the same edit changing the step's `expect_text`: cleared, answer says "cleared - the step was repaired" (today's sentence).
- [ ] **Step 2: Run** `cargo test --test suite autorun_defects::` - FAIL.
- [ ] **Step 3: Implement**: `repaired_marks` takes a mark only when `checks_changed(old step, new step)`; otherwise the report gets `kept_line`.
- [ ] **Step 4: Run** `autorun_defects::` then `autorun_edits::` - PASS.
- [ ] **Step 5: Commit** (`fix(v2): a repair that leaves a step's checks alone keeps its suspected-defect mark`).

---

## Task 6: The save warns about deletes that can go wrong (47, 48)

**Files:**
- Create: `src-tauri/src/autorun/script_notes.rs` (declare in `autorun/mod.rs` - one `pub mod` line)
- Modify: `src-tauri/src/ai_bridge.rs` (the save notes chain, `:13000-13005`)
- Modify: `src-tauri/src/autorun/guide.rs` (clean-up paragraph `:366-380`, "Selectors" `:1528`)
- Modify: `src-tauri/src/autorun/roles/writer.md`, `src-tauri/src/autorun/roles/case-rewriter.md` (one line each, within the caps)
- Test: create `src-tauri/tests/suite/autorun_script_notes.rs` (+ `mod autorun_script_notes;` in `tests/suite/main.rs`); `src-tauri/tests/suite/autorun_bridge.rs`

**Interfaces:**
- Produces: `pub fn is_delete_click(a: &Action) -> bool` - a `Click` whose target's name, text or css holds `delete`, `remove` or `discard` as a word (folded case).
- Produces: `pub fn delete_after_check_notes(script: &CaseScript, only_steps: Option<&[i32]>) -> Vec<String>` - for a case step (never the clean-up) where a delete click comes after a check: `"step {n} deletes after a check: when the check fails the delete never runs and the record stays for the next run - move the delete into the clean-up step"`.
- Produces: `pub fn positional_delete_notes(script: &CaseScript, only_steps: Option<&[i32]>) -> Vec<String>` - a delete click any of whose links has `nth`, or any click with `nth` followed in the same step by `expect_dialog`: `"step {n}: the delete picks by position (nth {k}) - on a shared list another record can sit there; find the row by its own name before deleting"`.
- Consumes: `CaseScript::case_steps`, `cleanup_step` (`autorun/mod.rs:486-501`), `LocatorStep::nth` (`browser/locator.rs`).

- [ ] **Step 1: Failing tests** in `autorun_script_notes.rs`:
  - `delete_after_failing_check_is_noted` (step 8: fill, expect_text, click "Delete"): one note naming step 8.
  - `delete_in_cleanup_step_is_not_noted`.
  - `delete_before_any_check_is_not_noted`.
  - `nth_delete_is_noted` (`{"role":"button","name":"Delete","nth":2}`): one note with `nth 2`.
  - `nth_click_then_confirm_dialog_is_noted` (`{"css":".icon-trash","nth":1}` then `expect_dialog`).
  - `delete_found_by_name_is_not_noted` (a chain: row named "AUTOTEST Marks Range Scale", then button "Delete").
  - `only_steps_limits_notes`.
  In `autorun_bridge.rs`: `save_answer_notes_a_positional_delete`: a save of such a script answers a `case N: note: step ...` line, and the save still succeeds.
- [ ] **Step 2: Run** `cargo test --test suite autorun_script_notes::` - FAIL.
- [ ] **Step 3: Implement**; chain both functions into the `seen_scope` notes after `undeclared_record_note` (they count toward `MAX_SAVE_NOTES`).
- [ ] **Step 4: Guide and roles.** Clean-up paragraph: "Put every delete of what the case made in the clean-up step, never after a check in a case step: a failing check stops the step and the record stays, so the next run fails on it." Selectors: "Before deleting, find the row by its own name, never by nth on a list others share." `writer.md` and `case-rewriter.md`: one bullet "Deletes go in the clean-up step and find their row by its own name, never by nth." Run `autorun_roles::` (caps) and `autorun_guide::`.
- [ ] **Step 5: Run** `autorun_script_notes::`, `autorun_bridge::`, `autorun_roles::`, `autorun_guide::` - PASS.
- [ ] **Step 6: Commit** (`feat(v2): the script save warns about a delete after a check and a delete by position`).

---

## Task 7: Holds end with their agent, and a reset lists them (51)

**Files:**
- Modify: `src-tauri/src/commands/autorun_slots.rs` (`IDLE_RELEASE_MS` `:388`, `idle_extras`/`idle_slots` `:417-440`, `on_sweep` `:506`)
- Modify: `src-tauri/src/autorun/claims.rs` (`CLAIM_TIMEOUT_MS` `:25`)
- Modify: `src-tauri/src/ai_bridge.rs` (`/autorun-reset` `:752`)
- Modify: `src-tauri/src/mcp.rs` (`reset_autorun_browser` `:800`), `src-tauri/src/autorun/guide.rs` (the several-assistants section)
- Test: `src-tauri/tests/suite/autorun_slots.rs`, `src-tauri/tests/suite/autorun_claims.rs`, `src-tauri/tests/suite/autorun_bridge.rs` (all under `serial::agents()`)

**Interfaces:**
- Produces in `autorun_slots.rs`: `pub const NAMED_IDLE_RELEASE_MS: u64 = 10 * 60 * 1000;` and `pub fn idle_after(key: &AgentKey) -> u64` (named subagent: `NAMED_IDLE_RELEASE_MS`; else `IDLE_RELEASE_MS`).
- Changes: `idle_slots(now_ms)` judges each slot by `idle_after(bound key)` and includes the MAIN slot when it is bound to an assistant and its browser was opened by an assistant (`opened_by_assistant`), never when the person opened it or holds its guard. A slot with a replay, a batch or a held session lock stays not idle (as today).
- Produces: `pub struct LiveHold { pub agent: String, pub browser: String /* "main" | "extra 1" | "extra 2" */, pub account: Option<String>, pub cases: Vec<i32>, pub quiet_for_s: u64 }` (`serde::Serialize`) and `pub fn live_holds(now_ms: u64) -> Vec<LiveHold>` - every slot bound to an assistant, with its claims (`claims`), oldest first. `agent` is the label the card shows ("Assistant 2 (dw-levels)"), never an id.
- Produces in `claims.rs`: `pub fn release_of(key: &AgentKey) -> usize` (drops that agent's claims; called with the idle release).

- [ ] **Step 1: Failing tests:**
  - `named_agent_slot_released_after_ten_quiet_minutes` (extra slot, agent named, last call 11 min ago: idle; 9 min: not).
  - `unnamed_agent_keeps_thirty_minutes` (11 min: not idle).
  - `main_slot_bound_to_named_agent_is_released_when_idle`.
  - `main_slot_opened_by_person_never_idle_released`.
  - `slot_with_replay_going_is_not_idle`.
  - `idle_release_frees_the_agents_case_claims` (claims of that key gone, another agent's kept).
  - `live_holds_lists_agent_browser_account_cases`.
  - in `autorun_bridge.rs`: `reset_answer_lists_live_holds` (the answer JSON has `holds` with the other agent's entry; no ids, no cookies).
- [ ] **Step 2: Run** `cargo test --test suite autorun_slots::` then `autorun_claims::` - FAIL.
- [ ] **Step 3: Implement**; the sweep's release of an idle slot also calls `claims::release_of(&key)` (the account lease goes with the browser's close, as today) and logs "Auto Run released <label>'s browser after <n> minutes without a call".
- [ ] **Step 4: MCP and guide**: `reset_autorun_browser` answers `holds` (who holds which browser, account and cases, and for how long without a call); a named assistant's browser and cases are let go after 10 minutes without a call, any other assistant's after 30; the person's own browser never.
- [ ] **Step 5: Run** the three modules, then `tcm_mcp::` - PASS.
- [ ] **Step 6: Commit** (`fix(v2): an assistant's browser and cases are let go once it stops calling, and a reset lists who holds what`).

**Owner decision flagged:** phase 5 decided "the main browser is never released this way". This task releases it only when an assistant opened it and a named assistant holds it; confirm, and confirm 10 minutes for named subagents.

---

## Task 8: Kept data a script relies on is declared and checked before step 1 (49) [after per-env]

**Files:**
- Modify: `src-tauri/src/autorun/mod.rs` (`CaseScript` `:235`; new `NeededRecord`)
- Create: `src-tauri/src/autorun/needs_records.rs`
- Modify: `src-tauri/src/autorun/preconditions.rs` (`undeclared_record_note` `:154`: quiet when `needs_records` is set; its sentence names `needs_records`)
- Modify: the run's precondition check (where `preconditions` are checked before sign-in, `commands/autorun.rs:2712` `auto_run_check_preconditions` and the runner) to check `needs_records` too
- Modify: `src-tauri/src/ai_bridge.rs` (save validation), `src-tauri/src/commands/autorun.rs` (the `CaseScript` literals `:3085`, `:3122`)
- Modify: `src-tauri/src/autorun/guide.rs` ("Preconditions" `:1187`), `roles/writer.md`
- Modify: `src/screens/AutoRun/ScriptEditor.tsx` (keeps the field on save), `src/screens/AutoRun/CaseCard.tsx` (one "Needs: <setup>" line)
- Regenerate: `src/bindings.ts`
- Test: create `src-tauri/tests/suite/autorun_needs_records.rs` (+ `mod`), `src/screens/AutoRun/ScriptEditor.test.tsx`

**Interfaces:**
- Produces: `pub struct NeededRecord { pub setup: String, pub why: String }` (serde, specta); `CaseScript::needs_records: Vec<NeededRecord>` with `#[serde(default, skip_serializing_if = "Vec::is_empty")]`.
- Produces in `needs_records.rs`: `pub fn check_declared(needed: &[NeededRecord], setups: &SetupFile) -> Vec<String>` (save refusals: an unknown setup - `"needs_records names \"X\", which is not a saved setup - save it with save_setup first"`; a setup with no database find rule - `"... has no database find rule, so a run cannot look for its record before signing in"`).
- Produces: `pub async fn check_needed<S: setup_find::RowSource>(rows: &S, needed: &[NeededRecord], setups: &SetupFile, prefix: &str, database_ready: bool) -> NeedsOutcome` with `pub enum NeedsOutcome { Met, Missing(String), NotChecked(String) }`: `Missing` is the Blocked sentence `"Blocked: the case needs <why> (setup \"X\"), which is not there - run_setup X builds it"`; `NotChecked` when database read access is off (`"needs_records not checked: database read access is off"`, said on the case, never blocking).
- Consumes: `setups::find_setup` (`autorun/setups.rs:145`), `setup_find::find_in_database` (`autorun/setup_find.rs:291`).

- [ ] **Step 1: Failing tests** in `autorun_needs_records.rs`: `save_refuses_unknown_setup`, `save_refuses_setup_without_database_rule`, `missing_record_blocks_with_sentence`, `found_record_lets_case_run`, `database_off_is_said_not_blocked`, `undeclared_note_quiet_when_needs_records_set`, `old_script_without_field_reads_and_writes_the_same` (a script JSON without the key round-trips byte-identical). Vitest: `the editor keeps needs_records when saving`.
- [ ] **Step 2: Run** `cargo test --test suite autorun_needs_records::` - FAIL.
- [ ] **Step 3: Implement**; the run checks `needs_records` with the preconditions, before sign-in; Blocked uses the existing Blocked path with the sentence.
- [ ] **Step 4: Regenerate bindings**: `cd src-tauri && cargo test --test bindings`; then `npx tsc --noEmit`, `npx vitest run src/screens/AutoRun/ScriptEditor.test.tsx`.
- [ ] **Step 5: Guide and role.** Preconditions: a record the case relies on that it did not make (kept by earlier work) is declared in `needs_records` with a saved setup that finds it; a run without it is Blocked, not failed halfway. `writer.md`: "Data a case relies on and did not make is declared at the time it is made: a fixture, a precondition or needs_records." Run `autorun_roles::`, `autorun_guide::`.
- [ ] **Step 6: Run** `autorun_needs_records::`, `autorun_preconditions::`, `autorun_bridge::` - PASS.
- [ ] **Step 7: Commit** (`feat(v2): a script declares the kept records it relies on, and a run without them is Blocked`).

**Owner decision flagged:** declaring kept data through saved setups' database find rules (this task) vs. requiring API-flow preconditions; and whether `undeclared_record_note` should become a refusal.

---

## Task 9: Expired passwords and other sign-ins are seen before a sign-in (40, part 1) [after per-env]

**Files:**
- Modify: `src-tauri/src/environments.rs` (`Environment` `:22`: two query fields), `src-tauri/src/commands/environments.rs` (save validation)
- Create: `src-tauri/src/autorun/password_expiry.rs`, `src-tauri/src/autorun/active_login.rs`
- Modify: `src-tauri/src/autorun/recipe.rs` (`SignInRecipe` `:258`: `password_reset`), `src-tauri/src/autorun/builtin_recipe.json`
- Modify: `src-tauri/src/autorun/signin.rs` (the reset page fails as "password expired")
- Modify: `src-tauri/src/ai_bridge.rs` (discovery start refuses an account signed in elsewhere; `propose_accounts` marks expired logins)
- Modify: `src-tauri/src/autorun/guide.rs` ("Who the case runs as" `:1041`, "Environments" `:1068`), `src-tauri/src/ai_tools.rs` (`/tcm:setup` step 3 text)
- Regenerate: `src/bindings.ts`
- Test: create `src-tauri/tests/suite/autorun_password_expiry.rs` (+ `mod`); `environments.rs`; `autorun_signin.rs`

**Interfaces:**
- Produces on `Environment`: `#[serde(default)] pub password_expiry_query: String` and `#[serde(default)] pub active_session_query: String` - empty means not checked. Each must be one SELECT the database guard classifies as a read; `{users}` (expiry) and `{user}` (active) are the only placeholders, written by the app as quoted literals the way `setup_find::sql_with_prefix` writes `{prefix}`.
- Produces in `password_expiry.rs`: `pub const DUE_SOON_DAYS: i64 = 7;`, `pub struct ExpiryRow { pub user: String, pub reset_due: Option<String>, pub prompt_change: bool }`, `pub enum Expiry { Ok { due: String, days_left: i64 }, DueSoon { due: String, days_left: i64 }, Expired { due: String }, ForcedChange, Unknown(String) }`, `pub fn judge(row: Option<&ExpiryRow>, today: &str) -> Expiry`, `pub fn query_with_users(sql: &str, users: &[String]) -> Result<String, String>`, `pub fn card_words(e: &Expiry) -> String` ("password due 2027-02-02, 97 days").
- Produces in `active_login.rs`: `pub struct ActiveRow { pub since: String, pub logged_to: String }`, `pub fn signed_in_elsewhere(key: &str, rows: &[ActiveRow], ours: bool) -> Option<String>` - `ours` true when this app's browser holds the account's lease (`lease::holder_of`); the sentence `"<key> is signed in elsewhere since 09:40 (ehrm) - signing in will log that session out"`.
- Produces on `SignInRecipe`: `#[serde(default)] pub password_reset: Option<PasswordResetPage>` with `pub struct PasswordResetPage { pub path_contains: String, pub field: Target }` (built-in: `/security/PasswordReset`, `#txtOldPassword`). `signin.rs`: `pub const PASSWORD_EXPIRED: &str = "password expired - the site sent the account to its password reset page";`.

- [ ] **Step 1: Failing tests** in `autorun_password_expiry.rs`: `judge_expired_when_due_is_past`, `judge_due_soon_under_seven_days`, `judge_forced_change_wins`, `judge_unknown_when_no_row_or_no_database` (never Expired), `query_with_users_quotes_and_doubles_quotes`, `query_refuses_a_write_or_a_second_statement`, `signed_in_elsewhere_ignores_our_own_session`, `flag_zero_rows_never_count` (the query filters; a row passed in counts - the test pins the documented query text in the guide uses `ACTLOG_ACT_FLG = 1`). In `environments.rs` suite: `old_environment_file_reads_with_empty_queries`, `save_refuses_a_query_that_writes`. In `autorun_signin.rs`: `reset_page_fails_as_password_expired` (path `/hr/security/PasswordReset` with `#txtOldPassword`: detail starts with `PASSWORD_EXPIRED`, no retry of Task 3).
- [ ] **Step 2: Run** `cargo test --test suite autorun_password_expiry::` - FAIL.
- [ ] **Step 3: Implement.** A discovery start (bridge) runs the active-session query for its account when set: a row not ours refuses with the sentence (409); the expiry query warns in the answer when `DueSoon`/`Expired`. `propose_accounts` runs the expiry and active-session queries on the logins it is sent and marks each (`expired`, `due soon`, `signed in elsewhere`) in its answer, listing usable ones first. A DB that cannot be reached is `Unknown`, said once, never blocking.
- [ ] **Step 4: Regenerate bindings**, `npx tsc --noEmit`.
- [ ] **Step 5: Guide and setup skill** text: the two environment queries (the backlog's SELECTs as the examples, reading only `Password_Valid_Days` from `HS_PR_PARAMETERS`), what each answer means, never read `HS_PR_PARAMETERS` whole.
- [ ] **Step 6: Run** `autorun_password_expiry::`, `environments::`, `autorun_signin::`, `autorun_bridge::` - PASS.
- [ ] **Step 7: Commit** (`feat(v2): an expired password and another sign-in are seen before an Auto Run sign-in`).

---

## Task 10: Check sign-in on each account, as far as an area (40 part 2, 53) [after per-env]

**Files:**
- Create: `src-tauri/src/autorun/sign_in_check.rs`, `src-tauri/src/commands/autorun_check_signin.rs` (+ `pub mod` in `commands/mod.rs`, registered in `lib.rs`'s command list)
- Modify: `src-tauri/src/ai_bridge.rs` (`/autorun-check-sign-in`), `src-tauri/src/mcp.rs` (tool `check_autorun_sign_in`, in the agent lists `tcm_mcp.rs:3931` reads)
- Modify: `src/screens/AutoRun/AccountsDialog.tsx` (a Check sign-in button per account, an optional area picker, the result line); `src/screens/AutoRun/RunPane.tsx` (warning line for accounts expired or due within 7 days, from Task 9)
- Modify: `src-tauri/src/autorun/guide.rs` ("Who the case runs as"), `src-tauri/src/ai_tools.rs` (`/tcm:gen-tests` Several writers: check each account reaches the area before the split), `docs-site/` Accounts text
- Regenerate: `src/bindings.ts`
- Test: create `src-tauri/tests/suite/autorun_check_signin.rs` (+ `mod`); `tcm_mcp.rs`; `src/screens/AutoRun/AccountsDialog.test.tsx`

**Interfaces:**
- Produces: `pub enum AreaReach { Reached { area: String }, Stopped { area: String, at: String } }` and `pub struct SignInCheck { pub account: String, pub signed_in: bool, pub landed: Option<String>, pub stopped: Option<String>, pub password: Option<String>, pub area: Option<AreaReach> }` (serde, specta; no password, no host).
- Produces: `pub fn summary(c: &SignInCheck) -> String` - e.g. `"signed in, landed on /hr/home/index; reaches Definition Wizard"`, `"signed in, landed on /hr/home/index; stopped on the way to Definition Wizard: <walk sentence>"`, `"stopped on the password reset page"`, `"password refused"`.
- Produces: `#[tauri::command] pub async fn auto_run_check_sign_in(app, account_key: String, area: Option<String>) -> Result<SignInCheck, String>` - refused while a run, a replay or a discovery holds the main browser (the same sentences as `auto_run_open_browser`, `commands/autorun.rs:573`); signs in with `sign_in_leased`, runs Task 9's queries when set, walks to `area` the way a run does (`nav::find_area` + the route walk, every menu path variant), and leaves the browser open where it stopped.
- Produces: bridge `POST /autorun-check-sign-in {account, area?}` in the caller's slot (like `start_autorun_discovery`), answering `SignInCheck` plus `summary`; MCP `check_autorun_sign_in` with `agent`.
- Consumes: `signin::SignInOutcome::landed` (Task 3), `password_expiry::card_words` (Task 9), `signin::PASSWORD_EXPIRED` (Task 9).

- [ ] **Step 1: Failing tests:** `summary_reaches_area`, `summary_stops_on_the_way_with_walk_sentence`, `summary_password_reset_page`, `summary_names_path_only` (no host); bridge with the fake driver: `check_walks_to_area_and_leaves_browser_open`, `check_refused_while_a_replay_runs`, `check_names_no_password` (the account's password string appears nowhere in the answer or log tail - `serial::log_tail()`); `tcm_mcp::every_tool_routed_through_a_browser_slot_takes_an_agent` stays green with the new tool listed. Vitest: `Check sign-in shows the summary under the account`, `the area picker sends the area`, `Check sign-in is disabled while a run is going`.
- [ ] **Step 2: Run** `cargo test --test suite autorun_check_signin::` - FAIL.
- [ ] **Step 3: Implement** the pure part, the command, the bridge route, the MCP tool, the UI. Icons from `actionIcons.ts`, colours from tokens.
- [ ] **Step 4: Regenerate bindings**; `npx tsc --noEmit`; `npx vitest run src/screens/AutoRun/AccountsDialog.test.tsx`; `npx vitest run src/ui-consistency.test.ts`.
- [ ] **Step 5: Guide, skills, How To Use text.** Shots of the Accounts dialog are owed (`npm run docs:dev` / `docs:shots --only <shot>` / `docs:build` per `CLAUDE.md`) - do them in this task if the owner's `tauri dev` is not running, else list them as owed in Task 12.
- [ ] **Step 6: Run** `autorun_check_signin::`, `tcm_mcp::`, `ai_tools::`, `autorun_guide::` - PASS.
- [ ] **Step 7: Commit** (`feat(v2): Check sign-in for each account, as far as a chosen area, for the person and the assistant`).

---

## Task 11: Reset an expired password on a test environment (40 part 3) [after per-env]

**Files:**
- Modify: `src-tauri/src/environments.rs` (`reset_expired_passwords: bool`, `#[serde(default)]`, false)
- Modify: `src-tauri/src/autorun/signin.rs` (on `PASSWORD_EXPIRED`), `src-tauri/src/autorun/accounts.rs` (store the new password)
- Modify: `src-tauri/src/autorun/recipe.rs` (`PasswordResetPage` gains `new`, `confirm`, `submit` targets; built-in `#txtNewPassword`, `#txtConfirmPassword`, `#btnsubmit`)
- Modify: the Environments settings UI (one switch, shown only for a test environment), `src/screens/AutoRun/AccountsDialog.tsx` ("Reset the password" on an expired account when the switch is on)
- Modify: `src-tauri/src/autorun/guide.rs`
- Regenerate: `src/bindings.ts`
- Test: `src-tauri/tests/suite/autorun_signin.rs`, `src-tauri/tests/suite/autorun_accounts.rs`, `src-tauri/tests/suite/environments.rs`

**Interfaces:**
- Produces in `signin.rs`: `pub const RESET_PASSWORDS: [&str; 2] = ["Abc@123", "Test@1234"];` and `pub fn next_password(current: &str, refused: &[String]) -> Option<&'static str>` (the first of the list that is neither the current password nor refused).
- Produces in `accounts.rs`: `pub fn set_password_for(root: &Path, env_id: &str, key: &str, new: &str) -> Result<(), String>` (drops the account's saved session, as a login change does; never logs the value).
- Behaviour: on the reset page, only when the environment is `test_environment && reset_expired_passwords`: fill old (the saved password), new and confirm, press submit (never `#btnreset`), wait for the sign-in page; the site's refusal (the "Info!" label) tries the next password once; success stores the new password, logs `"password reset for <key>"`, and signs in again with it. Anywhere else: the `PASSWORD_EXPIRED` failure, untouched.

- [ ] **Step 1: Failing tests:** `next_password_skips_current_and_refused`; `reset_on_test_environment_saves_new_password_and_signs_in` (fake page; the account file holds `Abc@123` after; log has `password reset for automation51` and no password); `no_reset_on_non_test_environment` (fails `PASSWORD_EXPIRED`, account file unchanged); `no_reset_when_switch_off`; `refused_new_password_tries_the_next_once`; `set_password_for_drops_saved_session`; `old_environment_reads_reset_switch_off`.
- [ ] **Step 2: Run** `cargo test --test suite autorun_signin::` - FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Regenerate bindings**; `npx tsc --noEmit`; the touched vitest files.
- [ ] **Step 5: Run** `autorun_signin::`, `autorun_accounts::`, `environments::` - PASS.
- [ ] **Step 6: Commit** (`feat(v2): an expired password is reset on a test environment when switched on`).

**Owner decisions flagged:** the switch defaults off (the backlog says "With the reset switched on"); the two passwords are the backlog's own and both sit in breach lists (Task 1 silences the browser's warning); the API route stays unbuilt until proven as a template.

---

## Task 12: Changelog, backlog status, owed checks

**Files:**
- Modify: `src/lib/changelog.ts` (lines for the next version entry the controller creates; end-user words only)
- Modify: `docs/autorun/backlog-2026-10.md` (a "Done in <version>" note under 44-53, 40, 46, 47, 49, 51, as 41-43 have)
- Modify: `docs/backlog.html` (shipped section and the owed hand checks)

- [ ] **Step 1:** Changelog lines, e.g.: "The Auto Run browser no longer shows password save or breach warnings over the page." / "A must-not-save case now stops what a Save button sends, whatever its address." / "A slow sign-in is given a second look, and a failed one says where the browser ended up." / "Check sign-in on each account, as far as an area." / "An expired password is spotted before a run, and can be reset on a test environment." No backlog numbers, no paths.
- [ ] **Step 2:** Owed hand checks in `docs/backlog.html`: Chrome and Edge show no Password Manager bubble (Task 1); a killed-by-hand orphan is closed at the next launch and logged (Task 2); the Definition Wizard Save & Continue is stopped in a no_save discovery (Task 4); three cold sign-ins at once on the Philippines dev site (Task 3); a stopped subagent's browser is let go after 10 minutes (Task 7); Check sign-in on the seven HR-admin logins to Definition Wizard (Task 10); one expired account reset on QA Automation (Task 11); the Accounts dialog shots if not retaken in Task 10.
- [ ] **Step 3:** `npx vitest run src/lib` (changelog tests), then commit (`docs(v2): changelog and backlog status for backlog 44-53`).

---

## Self-review notes

- Coverage: 44 (T1, T3), 45 (T4), 46 (T5), 47-48 (T6), 49 (T8), 50 (T3), 51 (T7), 52 (T2), 53 (T10), 40 (T9-T11). Item 1 and S3 are listed as skipped with reasons.
- Names used across tasks: `SignInOutcome::landed` (T3 -> T10), `signin::PASSWORD_EXPIRED` and `password_expiry::card_words` (T9 -> T10, T11), `PasswordResetPage` (T9, extended in T11).
- Owner decisions open: T4 blocks every write after a save-named click (the backlog's "block"); T7 main-browser idle release and 10 minutes; T8 the declaration mechanism; T11 switch default and passwords; and the global rule on naming paths (path only, as the guard already does) where 45 and 50 ask to name the request and the landing page.
