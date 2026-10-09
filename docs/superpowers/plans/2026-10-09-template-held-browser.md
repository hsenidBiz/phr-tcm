# API Template Held Browser Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Single API template runs on the same environment and account reuse one signed-in browser for 2 minutes after each run, skipping the launch and the sign-in.

**Architecture:**
- A small held-browser store, `api_templates/held.rs`, keeps one signed-in driver and its `Session` per (environment id, account key), with an idle deadline and a lease generation.
- `run_template_within` takes the account lease as today, then reuses a valid held entry through the existing fixture split (`open_session` / `run_in_session`). It puts the browser back when the run ends.
- `autorun::lease::take` bumps a per-key generation whenever a holder other than `Holder::Template` takes the lease, so a held entry knows it has given way.

**Tech Stack:** Rust (Tauri 2, CDP driver).

**Spec:** `docs/superpowers/specs/2026-10-09-template-held-browser-design.md`

## Global Constraints

- `HELD_IDLE = Duration::from_secs(120)`. Each reuse restarts it.
- An idle held browser holds **no** lease. A run takes the lease exactly as today (`account_lease`), for its whole run.
- Fixtures (`fixture_run.rs`) keep their own browser and are unchanged.
- Log lines name the account key only. No login, password, recipe text, host or query string. No HTTP DELETE.
- Rust tests are integration tests in `src-tauri/tests/suite/`, with a `mod` line. Process-wide state (the held store, the lease table) takes its lock from `suite/serial.rs`.
- `src/bindings.ts` is generated only; nothing here should change it.
- No em or en dashes. Run one build or test command at a time.
- Commits use a Bash heredoc, are staged by name, and end with the implementer's `Co-Authored-By` line.

## Review Focus

1. A held browser must never be reused after another holder has signed the account in. Test: `a_case_taking_the_account_makes_the_held_browser_give_way`.
2. A reused browser whose session silently died (an empty 400, or the sign-in page) must sign in fresh once, not fail. Test: `a_dropped_session_on_a_reused_browser_signs_in_again_once`.
3. A timeout or panic mid-run must not leave a held entry that points at a half-used browser. Test: `a_run_that_timed_out_keeps_nothing`.
4. The idle timer firing while a run has taken the entry must not close the browser under it. Test: `the_idle_close_never_closes_a_browser_in_use`.
5. Changing the recipe or the account's login between runs must not reuse the old session. Test: `a_changed_recipe_is_not_reused`.

---

### Task 1: The held-browser store and lease generations

**Files:**
- Create `src-tauri/src/api_templates/held.rs` (register it in `api_templates/mod.rs`).
- Modify `src-tauri/src/autorun/lease.rs`.
- Add `src-tauri/tests/suite/template_held.rs` and its `mod` line.

**Interfaces:**
- `lease.rs`:
  - `pub fn generation(env: &str, key: &str) -> u64`. It is bumped inside `take` whenever the lease is taken by a holder other than `Holder::Template`.
- `held.rs`, generic over the driver type used by `Browsers`:
  - `pub struct HeldEntry<D> { pub driver: D, pub session: runner::Session, pub fingerprint: u64, pub generation: u64, pub page: Option<String> }`, where `page` is the path of the page the browser is on.
  - `pub fn take<D>(env, key, fingerprint) -> Option<HeldEntry<D>>` returns the entry only when its fingerprint matches and its generation equals `lease::generation(env, key)`. Otherwise it removes the entry and hands it back for closing. The implementer picks the exact return shape, but it must allow the caller to close a stale driver.
  - `pub fn put<D>(env, key, entry)` stores the entry with deadline now plus `HELD_IDLE`, and returns any entry it replaced so the caller can close it.
  - `pub fn expired<D>() -> Vec<HeldEntry<D>>` drains entries past their deadline.
  - `pub fn drain_all<D>() -> Vec<HeldEntry<D>>` serves app exit, environment change and sign out.
- **The store and type erasure:** a process-wide `Mutex` map. If the driver type makes a static store awkward, store `Box<dyn Any + Send>` and downcast. Tests can use the fake driver.
- **Fingerprint:** a hash of the recipe JSON plus the account key and login. It is never logged.

- [ ] Tests:
  - `take_after_put_returns_the_entry_once`
  - `a_case_taking_the_account_makes_the_held_browser_give_way` (a lease taken as `Holder::Case` bumps the generation, so `take` refuses)
  - `a_template_lease_does_not_bump_the_generation`
  - `a_changed_recipe_is_not_reused`
  - `expired_drains_only_entries_past_their_deadline` (use an injectable clock or a test-only short idle)
  - `put_replacing_an_entry_hands_the_old_one_back`
- [ ] Implement. Run `cargo test --test suite template_held::`, then `cargo test --test suite autorun_lease::` (or whichever module covers the lease), then `cargo test --tests`.
- [ ] Commit `feat(v2): a held signed-in browser per account, which gives way when anything else takes the account`

### Task 2: Single runs reuse the held browser

**Files:**
- `api_templates/runner.rs`:
  - `run_template_within`;
  - `run_signed_in` (skip the token-page navigation when already on that path);
  - the dropped-session retry;
  - the report's sign-in line.
- `ai_bridge.rs` around line 1022: the `run_template` caller, which today drops `browsers` straight after the run.
- An idle sweeper: a tokio task, spawned once, that every 10 s closes `held::expired()` entries through their own close.
- App exit (`close_autorun_browsers` or the same exit path Auto Run uses), environment change and sign-out call `held::drain_all()` and close each entry.

**Interfaces:**
- **With a valid held entry:**
  1. take the lease;
  2. `held::take`;
  3. `run_in_session` with the entry's session;
  4. on success or failure, if the browser is alive and the run did not time out, `held::put` it back with the page it is now on;
  5. close whatever `put` replaced.
- **Without a held entry:** open, `open_session`, `run_in_session`, then `put` as above. This replaces today's `drive` plus `close` for single runs. The `close` path stays for a browser that died, a timeout, or a failed sign-in.
- **Dropped session:**
  - It applies only when the session was reused, and only to the first request of the run. That request comes back as an empty-bodied 400, or the token page lands on the recipe's sign-in page. The detection must not catch a template whose expected answer is a 400 with a body.
  - Then: close the held driver, open a fresh one, sign in, and run the template from its first step.
  - This happens at most once per run.
- **The report's sign-in record:** a reused session says "Signed in earlier, reused" in the sign-in line, in the same shape `progress.signed_in` writes.
- **Timeout or panic:** the driver is closed, never put back.

- [ ] Tests (fake `Browsers` and `Driver`):
  - `a_second_run_within_two_minutes_opens_no_browser_and_signs_in_once`
  - `the_token_page_is_not_reloaded_when_already_on_it`
  - `a_different_token_page_is_loaded`
  - `a_dropped_session_on_a_reused_browser_signs_in_again_once`
  - `an_empty_400_on_a_fresh_browser_fails_as_before`
  - `a_400_with_a_body_on_a_reused_browser_is_the_templates_own_answer`
  - `a_run_that_timed_out_keeps_nothing`
  - `a_failed_sign_in_keeps_nothing`
  - `the_idle_close_never_closes_a_browser_in_use`
  - `quitting_changing_environment_or_signing_out_closes_the_held_browser`
  - `a_reused_run_reports_its_sign_in_as_reused`
- [ ] Implement. Run `template_held::`, then the api template runner suites (`api_templates_runner::`, `api_fixtures::`), then `cargo test --tests`.
- [ ] Commit `perf(v2): back-to-back API template runs reuse one signed-in browser for two minutes`

### Final gate

- [ ] Run, one at a time: `cd src-tauri && cargo test --tests`, then `npx tsc --noEmit`, `npm test` and `npm run build`.
- [ ] Hand check owed: two real template runs within 2 minutes; the second skips the sign-in.
