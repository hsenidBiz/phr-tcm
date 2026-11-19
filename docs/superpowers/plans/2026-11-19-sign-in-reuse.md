# Fewer sign-ins in an Auto Test run - Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** In an unattended Auto Test run, a case reuses the previous case's signed-in browser when it uses the same account, and a case's setup runs in that browser too, instead of every case opening a browser and signing in again.

**Architecture:** A `KeptBrowser` holds the previous case's driver and its account lease (`Held`) across the run loop in `autorun/replay.rs`. A pure decision function (`autorun/kept.rs`) says whether the next case may use it. A reused case checks `replay_to::signed_in_here_why` instead of signing in, and records step 0 as reused. A setup on the same account runs in the kept browser through a new fixture-run entry point that skips opening, leasing and signing in.

**Tech Stack:** Rust (tokio, existing `Driver` / `Browsers` traits), integration tests in `src-tauri/tests/suite/`.

**Spec:** `docs/superpowers/specs/2026-11-19-sign-in-reuse-design.md`

## Global Constraints

- Never more than one signed-in browser per run.
- The account lease is held while a browser is kept, and let go when it closes.
- A fresh browser is used when:
  - the account changes;
  - the case has no module trip (no `route`);
  - the case before was not Passed, was stopped, or failed its sign-in;
  - the case before ran a script `sign_in` step, or had `no_save`;
  - the still-signed-in check fails;
  - a batch pause or a network pause comes between;
  - a transient retry runs.
- A case that is not Passed in a kept browser runs once more in a fresh browser. The fresh run is the record and its video is the case's video; the first go's video is discarded.
- Step 0 of a reused case says it used the browser the case before signed in, with the check's time. The phases show open near 0 and sign-in as the check's time.
- Every tab but the main one is still closed after each case. Downloads stay in the run's folder (`store::downloads_dir(root, run_id)`).
- No new setting.
- Never log a password, fill value or cookie value. User-facing sentences name no URL or path.
- Rust tests are integration tests in `src-tauri/tests/suite/` only, one module per file, with a `mod` line in `suite/main.rs`. Tests touching process-wide state take a lock from `suite/serial.rs`.
- Run one build or test command at a time. Never set `CARGO_TARGET_DIR`.

## Review Focus

1. **Stop pressed while a browser is kept between cases:** the kept browser closes and its lease is let go before the run returns. Test in Task 2.
2. **The run ends on its last case with a kept browser:** it is closed and its lease let go. A later run, or a template run on that account, is not blocked by a leftover lease. Test in Task 2.
3. **A case Blocked before its browser opens** (preconditions, needs-records, divergence) between two reusable cases: the kept browser stays usable. Blocking before start touched no browser. Test in Task 2.
4. **The run account (`run_account`) overrides the script account:** reuse compares the account each case actually signs in as (`run_account.or(script.account)`), not the script's. Test in Task 1.
5. **A setup on the same account while a browser is kept:** the setup must not try to take the account lease a second time (it would wait, then Block the case). Test in Task 3.

---

### Task 1: The reuse decision

**Files:**
- Create: `src-tauri/src/autorun/kept.rs`, declared `pub mod kept;` in `src-tauri/src/autorun/mod.rs`
- Test: `src-tauri/tests/suite/autorun_kept.rs`, with `mod autorun_kept;` in `suite/main.rs`

**Interfaces:**
- Produces:
  - `pub struct Ended { pub account: Option<String>, pub proposed: String, pub stopped: bool, pub signed_in_ok: bool, pub script_signs_in: bool, pub no_save: bool }` describes how the case before ended.
  - `pub struct Next<'a> { pub account: Option<&'a str>, pub has_route: bool, pub pause_between: bool }`
  - `pub enum Fresh { NoBrowser, AccountChanged, NoModuleTrip, BeforeNotPassed, BeforeStopped, BeforeSignInFailed, BeforeSignedInByScript, BeforeNoSave, Paused }`
  - `pub fn may_reuse(before: Option<&Ended>, next: &Next) -> Result<(), Fresh>`
  - `pub fn may_keep(ended: &Ended) -> bool`, true when a browser may be kept after this case at all. Keep it only when every condition holds: `account.is_some()`, `proposed == "Passed"`, `!stopped`, `signed_in_ok`, `!script_signs_in`, `!no_save`.
  - `pub fn ended_from(record: &CaseRecord, script: &CaseScript, account: Option<&str>) -> Ended`
    - `stopped` comes from the record's reason starting with the stop wording the replay uses (`AFTER_STOP`). Expose that constant `pub` if it is private.
    - `signed_in_ok` comes from step `SIGN_IN_STEP`'s last outcome being ok, or true when the case had no sign-in step.
    - `script_signs_in` is any action of kind `sign_in` in `script.steps`.
  - `pub fn reused_detail(check_ms: u64) -> String`, exactly: `used the browser the case before signed in (checked it was still signed in in {check_ms} ms)`

- [ ] **Step 1: Write the failing tests** in `autorun_kept.rs`:
  - `reuses_same_account_with_route`: the case before is Passed as "a"; next is "a" with a route; `may_reuse` gives `Ok(())`.
  - One test per `Fresh` variant, each changing only one field:
    - account "b" gives `AccountChanged`;
    - `has_route: false` gives `NoModuleTrip`;
    - proposed "Failed" gives `BeforeNotPassed`;
    - `stopped` gives `BeforeStopped`;
    - `signed_in_ok: false` gives `BeforeSignInFailed`;
    - `script_signs_in` gives `BeforeSignedInByScript`;
    - `no_save` gives `BeforeNoSave`;
    - `pause_between` gives `Paused`;
    - `before: None` gives `NoBrowser`;
    - next account `None` gives `AccountChanged`.
  - `run_account_wins`: `ended_from` with the script naming "x" and account `Some("a")` gives `Ended.account == Some("a")`.
  - `ended_from_reads_record`: a record with a failed step 0 gives `signed_in_ok == false`; a script with a `sign_in` action gives `script_signs_in == true`.
  - `reused_detail_wording`: `reused_detail(840)` equals the exact sentence above with 840.
- [ ] **Step 2:** Run `cd src-tauri && cargo test --test suite autorun_kept::`. Expect a FAIL to compile.
- [ ] **Step 3:** Implement `kept.rs`. Check `may_keep` first and map each failed condition to its `Fresh` variant. Then check `Paused`, then `NoModuleTrip`, then `AccountChanged`.
- [ ] **Step 4:** Run the same command. Expect PASS.
- [ ] **Step 5:** Commit `feat(v2): the rule for when an Auto Test case may reuse the browser before it`.

### Task 2: Keep and reuse the browser between cases (A2)

**Files:**
- Modify: `src-tauri/src/autorun/replay.rs`: `one_go`, `run_case_as` / `run_case_in`, `run_cases_planned`
- Modify: `src-tauri/src/autorun/kept.rs`: add `KeptBrowser`
- Test: `src-tauri/tests/suite/autorun_kept_run.rs`, with a `mod` line. Copy the fake `Browsers` / `Driver` pattern from `tests/suite/autorun_replay.rs`. The fake counts `open()` and `close()` calls and sign-ins.

**Interfaces:**
- Consumes: Task 1's `may_reuse`, `may_keep`, `ended_from`, `reused_detail`, and `Fresh`.
- Produces:
  - `pub struct KeptBrowser<D> { pub driver: D, pub lease: Held, pub ended: Ended }`
  - `pub async fn close_kept<B: Browsers>(browsers: &mut B, kept: &mut Option<KeptBrowser<B::D>>)` closes the driver, then drops the lease. It does nothing for `None`.

**Behaviour:**
- **The loop owns the kept browser.** `run_cases_planned` holds `let mut kept: Option<KeptBrowser<B::D>> = None;`.
- **When the kept browser is closed with `close_kept`:**
  - before a batch pause's `gate.wait`, and before `pause_for_network`;
  - when `cancel` is seen at the loop top;
  - after the loop (every exit path, including `break 'cases`);
  - before `setup::prepare_case` for a case that has a setup (Task 3 refines this);
  - before a transient retry's second go.
- **`one_go` signature:** it gains `kept: &mut Option<KeptBrowser<B::D>>` and `reuse: bool`. It returns the record and leaves `kept` filled when `may_keep` holds for the case that just ran.
- **A reused go:**
  - Take the kept browser.
  - Call `replay_to::signed_in_here_why(&mut d, root, org, project, key, &Some(key.to_string()), &lease, timing)` and time it.
  - On `Err`: log the reason in the app log, with no account login. Then `browsers.close(d)`, drop the lease, and run a normal fresh go.
  - On `Ok`: call `run_case_as` with a new parameter `reused: Option<u64>` (the check's ms).
    - `run_case_in` then skips the lease wait and the sign-in.
    - It pushes step `SIGN_IN_STEP` with one ok outcome whose detail is `reused_detail(ms)` and `duration_ms: Some(ms)`, and sets `signed_in = None`, so the trip uses `TripFrom::Elsewhere`.
    - It calls `on_step(SIGN_IN_STEP)` as today.
    - It records `phases.open_ms = 0`.
- **The lease after a fresh sign-in:** after a fresh go's successful sign-in, the lease must hold the sign-in's cookie print, or the next check fails with `NoPrint`. If `run_case_in`'s sign-in path does not already call `Held::note_sign_in`, call `replay_to::retake_sign_in(d, &recipe.origins(), lease, "the case's sign-in")` once the sign-in is ok.
- **After a go:** closing other tabs and settling downloads stay as today. If `may_keep(&ended)` holds, the driver and the lease go into `kept` instead of `browsers.close(d)`, and `phases.close_ms = 0`. Otherwise close as today.
- **A go that is not Passed in a reused browser:**
  - Close the kept browser.
  - Discard the first record's video (`store::discard_video`).
  - Run one fresh go; that record is the case's record. The kept state then follows that record.
  - This happens before the transient-retry decision, which is then made on the fresh record.
  - The network re-run loop is unchanged; it already re-runs from the top.
- **The reuse decision** for each case is `may_reuse(kept.as_ref().map(|k| &k.ended), &Next { account, has_route: route.is_some(), pause_between })`. `pause_between` is true when a pause or network wait happened before this case in this iteration. Those already closed `kept`, so `NoBrowser` covers it, but compute it anyway for the log line.
- **The log line:** every case writes one app-log line saying reused, or fresh with the `Fresh` variant's name in `{:?}` form. It names no account login.

- [ ] **Step 1: Write the failing tests** in `autorun_kept_run.rs`, using a fake that passes `signed_in_here_why`. Look at how `autorun_check_signin.rs` fakes a signed-in page and lease print.
  - `two_same_account_cases_open_one_browser`: 2 cases, both "a" with a route, both pass. Expect `open() == 1`, `close() == 1` (at the run's end), one sign-in, and case 2's step 0 detail starts with "used the browser the case before signed in".
  - `account_change_opens_fresh`: "a" then "b". Expect `open() == 2` and two sign-ins.
  - `failed_case_before_opens_fresh`, `no_save_case_before_opens_fresh`, `no_route_case_opens_fresh`, `pause_between_opens_fresh` (with a `Pause` before case 2 and a gate that continues). Each expects `open() == 2`.
  - `check_failing_signs_in_fresh`: the fake's signed-in marker is hidden for case 2. Expect `open() == 2`, two sign-ins, and case 2 recorded as a normal sign-in.
  - `failure_in_kept_browser_reruns_fresh`: case 2 fails on its first go only. Expect `open() == 2`, case 2 Passed, and only one record for case 2.
  - `stop_closes_kept_browser_and_lease`: a stop is set after case 1. Expect `close() == open()`, and `lease::holder_of(env, "a")` is `None` after the run.
  - `run_end_lets_lease_go`: after a 2-case reuse run, `lease::holder_of(env, "a")` is `None`.
  - `blocked_case_between_keeps_browser`: case 2 is Blocked before start by a missing precondition DB, case 3 is "a" with a route. Expect `open() == 1` over cases 1 and 3.
- [ ] **Step 2:** Run `cargo test --test suite autorun_kept_run::`. Expect FAIL.
- [ ] **Step 3:** Implement as described.
- [ ] **Step 4:** Run `cargo test --test suite autorun_kept_run::`, then `autorun_replay::`, `autorun_network_pause::`, `autorun_reset::`, `autorun_lease::`. Expect PASS. Existing tests whose open counts change are updated only where the new rule demands it, and each changed test says so in a comment.
- [ ] **Step 5:** Commit `feat(v2): an Auto Test run keeps the signed-in browser for the next case on the same account`.

### Task 3: A setup runs in the kept browser (A)

**Files:**
- Modify: `src-tauri/src/api_templates/fixture_run.rs`: add an entry point that runs in a given driver
- Modify: `src-tauri/src/api_templates/runner.rs`: `open_session` gains a way to skip the sign-in
- Modify: `src-tauri/src/autorun/setup.rs`: `prepare_case_within` takes an optional kept browser
- Modify: `src-tauri/src/autorun/replay.rs`: the call site
- Test: extend `src-tauri/tests/suite/autorun_kept_run.rs`; see `tests/suite/api_fixtures.rs` for the fixture fakes

**Interfaces:**
- Consumes: `KeptBrowser`, `close_kept` (Task 2).
- Produces:
  - `pub async fn prepare_case_kept<B: Browsers>(browsers: &mut B, kept: &mut Option<KeptBrowser<B::D>>, root, org, project, script, timing, cancel) -> Result<Prepared, String>`
    - If the script has a setup whose fixture account equals the kept browser's account, the fixture runs in the kept driver, under the kept lease, with no sign-in.
    - If the setup's account differs, it calls `close_kept` first and then runs as today (its own browser).
    - With no setup, it behaves like `prepare_case`.
  - In `fixture_run.rs`: `pub async fn run_fixture_in<D: Driver>(d: &mut D, root, org, project, f, timing, limit, retry_pauses, clock, case_id, templates, cancel, source) -> FixtureRun`. It has the same body as `run_fixture_for` minus `account_lease`, `browsers.open` and `browsers.close`. Its session comes from `open_session_signed_in`.
    - Factor the shared body so `run_fixture_for` calls it and the code is not duplicated.
  - In `runner.rs`: `pub(crate) async fn open_session_signed_in<D: Driver>(d, root, req, timing) -> Result<Session, Report>`. It builds `Session` exactly as `open_session` does after its sign-in, without calling `sign_in_in`.
- If the case's setup ran in the kept browser and failed, close the kept browser. The case is Blocked as today.

- [ ] **Step 1: Write the failing tests:**
  - `setup_same_account_runs_in_kept_browser`: case 1 is "a", Passed. Case 2 has a setup whose fixture account is "a". Expect `open() == 1`, one sign-in, and the setup's fixture run reported ok.
  - `setup_other_account_closes_kept_first`: the setup account is "b". Expect the kept browser closed before the setup's browser opens (the fake records the open/close order: close, then open), and never two drivers open at once (the fake asserts its live count is at most 1).
  - `setup_does_not_wait_on_own_lease`: the same-account setup completes within the test's short lease wait (`Timing` lease wait of 200 ms). Without the fix it Blocks with the lease sentence.
- [ ] **Step 2:** Run `cargo test --test suite autorun_kept_run::`. Expect FAIL.
- [ ] **Step 3:** Implement. Replace both `setup::prepare_case` calls in `run_cases_planned` with `prepare_case_kept` (the first go). Leave the transient retry as today: it runs after `close_kept`, so it always uses its own browser.
- [ ] **Step 4:** Run `autorun_kept_run::`, `api_fixtures::`, `api_templates_runner::`. Expect PASS.
- [ ] **Step 5:** Commit `feat(v2): a case's setup runs in the browser the run already signed in`.

### Task 4: Wording and the full gate

**Files:**
- Modify: `src/lib/changelog.ts` (the next beta entry is added at release by `bump.py`; nothing here unless an end-user line is wanted now - leave it to release)
- Modify: `docs/backlog.html`: done note
- Check: the run report / review show step 0's detail as text already. Confirm a reused step 0 renders without a "signing in" label that now misleads. `Grep` `src/screens/AutoRun/` and `src-tauri/src/autorun/report*.rs` for `SIGN_IN_STEP` / "Signing in" labels, and make the label say "Sign-in" if it says "Signed in as".

- [ ] **Step 1:** Run the full Rust suite: `cd src-tauri && cargo test --test suite`. Expect 0 failed.
- [ ] **Step 2:** Run `npx tsc --noEmit` and `npm test` if any webview file changed.
- [ ] **Step 3:** Commit any wording fix and the backlog note.
