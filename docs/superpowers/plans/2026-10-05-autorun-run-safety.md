# Auto Run run safety Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** add three run-safety features:
- no-save scripts whose save requests are stopped inside the browser;
- preconditions checked before step 1;
- one sign-in per account at a time across Auto Run and API templates.

**Architecture:**
- `CaseScript` gains `no_save` and `preconditions`.
- The no-save guard is CDP `Fetch` interception, handled where the CDP driver processes events (`browser/cdp.rs`), so a paused request is answered without waiting for the runner's next call.
- Preconditions reuse `api_templates::gate::stage_state`.
- The lease is a small process-wide registry in `autorun/lease.rs`, used by the replay, the supervised session and the template runner.

**Tech Stack:** Rust (tokio, serde, CDP), React 19 + TypeScript, vitest.

**Spec:** docs/superpowers/specs/2026-10-05-autorun-run-safety-design.md

## Global Constraints

- Auto Run and API Templates stay gated. No changelog, help-site or README edits, and never mention a secret unlock.
- Rust tests live only in `src-tauri/tests/suite/` (one binary; a test that sets process-wide state, such as the lease registry, takes its lock from `suite/serial.rs`).
- Live tests are `#[ignore = "starts a real headless Edge"]`.
- Never hand-edit `src/bindings.ts`. Regenerate it with `cd src-tauri && cargo test --test bindings`.
- Colours come from tokens and icons from `src/lib/actionIcons.ts`. `ui-consistency` and `a11y` must stay unchanged.
- Plain sentences, no em dashes. No password, cookie, header, host or query string in any outcome, log or event.
- Run one test command at a time. Commit with a Bash heredoc ending `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. A no-save case must not hang the page: every paused request that is not a save must be continued at once, including while the runner is between calls.
2. A save fired during the page's own load (before step 1's first action) is still blocked and still fails the case.
3. A lease must be released when a case panics or the run is stopped mid-case. Otherwise the next case waits 60 s for nothing.
4. A precondition value with quotes, or a SQL-like value, goes through the flow's existing substitution and is never pasted raw.
5. An old script file without the new fields loads unchanged and behaves exactly as before.

---

### Task 1: No-save scripts (spec §1)

**Files:**
- `src-tauri/src/autorun/mod.rs` (`CaseScript.no_save`)
- `src-tauri/src/browser/cdp.rs` (Fetch interception, answered in event handling)
- a new `src-tauri/src/browser/save_guard.rs` (pure matching: `is_save(method, url, patterns) -> bool`, the sentence)
- `src-tauri/src/autorun/runner.rs` and `replay.rs` (enable the guard for no-save cases, fail the case on a block)
- `src-tauri/src/autorun/patterns.rs` (`ErrorClass::SaveBlocked`)
- `src-tauri/src/autorun/edits.rs` (a repair cannot clear `no_save`)
- the project settings store for patterns, plus IPC to read and edit them
- `src-tauri/src/autorun/guide.rs`
- `src/screens/AutoRun/ScriptEditor.tsx` (`Must not save`)
- `src/screens/AutoRun/index.tsx` (the Setup tab row `Save words` and its dialog)
- the fixture `src-tauri/tests/fixtures/autorun-save.html`
- tests

- [ ] **Step 1:** Write failing tests.
  - `is_save`: methods × words × patterns, the query ignored, case-insensitive.
  - The sentence.
  - A repair that clears `no_save` is refused, and an editor save may clear it.
  - The guide mentions the flag.
  - Live, fixture page plus a tiny local server:
    - with the guard, a click that POSTs `/api/Save` is blocked: the server never receives it, and the case fails with the sentence;
    - `/api/Search` (POST) passes;
    - a project pattern `search` blocks it;
    - a save fired on page load is blocked (Review Focus 2);
    - without `no_save`, nothing is intercepted.
- [ ] **Step 2:** Implement.
  - The guard is enabled only for no-save cases.
  - Paused requests are answered inside the driver's event processing (Review Focus 1).
  - Regenerate the bindings.
- [ ] **Step 3:** Webview.
  - The ScriptEditor checkbox, with the accessible name `Must not save`.
  - The Setup tab row `Save words`: the built-in words shown as fixed, plus an `Edit save words` dialog for the project's own (add or remove), saved through IPC.
  - Cover both with vitest.
- [ ] **Step 4:** Run the focused tests, the live test once with `--ignored --test-threads=1`, then `npx tsc --noEmit`, `npm test` and `cargo test --tests`, one at a time. Commit `feat(v2): no-save Auto Run scripts have their save requests stopped in the browser`.

### Task 2: Preconditions (spec §2)

**Files:**
- `src-tauri/src/autorun/mod.rs` (`CaseScript.preconditions`)
- a new `src-tauri/src/autorun/preconditions.rs` (`check_all<D: StageDb>(db, flows, preconditions) -> Result<(), String>`, returning the Blocked sentence)
- `replay.rs` and the supervised start (check before sign-in)
- save-time validation in `ai_bridge.rs` (the assistant's save), the editor save and import
- `guide.rs`
- tests

- [ ] **Step 1:** Write failing tests with the fake `StageDb`:
  - done: the case proceeds;
  - not done: Blocked with the exact sentence, with and without `why`;
  - could not run;
  - no database;
  - save refuses an unknown flow, an unknown stage or a missing value;
  - a value with quotes goes through the substitution (Review Focus 4);
  - an old script loads (Review Focus 5).
- [ ] **Step 2:** Implement and regenerate the bindings.
- [ ] **Step 3:** Run the focused tests, `cargo test --tests` and `npm test`, one at a time. Commit `feat(v2): Auto Run checks a script's preconditions before step 1`.

### Task 3: One sign-in per account at a time (spec §3)

**Files:**
- a new `src-tauri/src/autorun/lease.rs` (`acquire(env, key, holder, wait) -> Result<Lease, String>`, `try_acquire`, the drop-released `Lease`)
- `replay.rs` (per case)
- the supervised session (`commands/autorun.rs`)
- `api_templates/runner.rs` (run and prove)
- tests, which take `serial.rs`'s lock

- [ ] **Step 1:** Write failing tests:
  - a second acquire waits, and gets the lease once the first is dropped;
  - a timeout gives the exact sentence;
  - the lease is released on drop, on error, on stop and on panic (with `catch_unwind`; Review Focus 3);
  - a supervised sign-in on a held account is refused at once;
  - an API template run waits for an unattended case and then proceeds;
  - different accounts never block each other.
- [ ] **Step 2:** Implement.
- [ ] **Step 3:** Run the focused tests, `cargo test --tests` and `npm test`, one at a time. Commit `feat(v2): Auto Run and API templates never sign in as one account at the same time`.
