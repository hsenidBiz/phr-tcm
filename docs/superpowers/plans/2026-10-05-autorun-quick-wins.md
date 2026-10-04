# Auto Run quick wins Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix three tool defects and add three run-engine improvements: dismiss-if-present steps, a module-path retry, and a transient retry.

**Architecture:**
- The tool fixes live in `src-tauri/src/mcp.rs` and `ai_bridge.rs`.
- The iframe fixes live in `browser/snapshot.rs` and `autorun/patterns.rs`.
- `when_visible` becomes an `Action` variant (`browser/actions.rs`), carried out by the runner like `sign_in`.
- The module retry and the transient retry live in `autorun/replay.rs` (the unattended path), and the module retry also in the supervised runner's module trip.
- The webview gains one dialog option and one label.

**Tech Stack:** Rust (tokio, serde, CDP driver), React 19 + TypeScript, vitest.

**Spec:** docs/superpowers/specs/2026-10-05-autorun-quick-wins-design.md

## Global Constraints

- Auto Run and API Templates stay gated as today. No changelog, help-site or README edits, and never mention a secret unlock.
- Rust tests only in `src-tauri/tests/suite/`: one binary, with a `mod` line in `suite/main.rs`. Live browser tests are `#[ignore = "starts a real headless Edge"]`.
- Never hand-edit `src/bindings.ts`. Regenerate it with `cd src-tauri && cargo test --test bindings`, and restore the file if only line endings change.
- Colours from tokens, icons from `src/lib/actionIcons.ts`. `src/ui-consistency.test.ts` and `src/a11y.test.tsx` must not change.
- Plain sentences, no em dashes. No password, cookie, header, host or query string in any outcome, log or event.
- One test command at a time. Commit with a Bash heredoc ending `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. A `when_visible` whose target appears just after `within_ms`: the step must pass silently, and the next step must not be confused by the late banner. Accept that, and test that the outcome says "not shown, skipped".
2. A transient retry must never run twice. A case that fails transiently again is reported once, with both sentences.
3. The module retry must not loop: one reload, then report.
4. A `list_api_templates` call with no arguments on a project of 300 templates must stay under about 20K characters.
5. An iframe title containing `'` and `\` must still produce a step that matches the frame.

---

### Task 1: The three tool fixes (§1, §2, §3)

**Files:** `src-tauri/src/mcp.rs`, `src-tauri/src/ai_bridge.rs` (`api_template_list`, the save route's body parsing), `src-tauri/src/browser/snapshot.rs` (the iframe step), `src-tauri/src/autorun/patterns.rs` (`ErrorClass::FrameUnreachable`), `src-tauri/src/autorun/failures.rs` if it groups by class. Tests: `tcm_mcp.rs`, `ai_bridge.rs` / `api_templates.rs`, `browser_snapshot.rs`, `autorun_patterns.rs` (or whichever module tests patterns).

**Interfaces:**
- Produces:
  - `list_api_templates` arguments `{ module?, search?, flow?, id?, offset?, limit? }` and a paging block `{ total, offset, returned, next_offset? }` in its answer;
  - `ErrorClass::FrameUnreachable`, with key `frame`.

- [ ] **Step 1:** Reproduce the lost `edits`. Write failing tests that drive the real `mcp.rs` dispatch with every payload shape listed in spec §1, and assert that the route receives the edits (a repair is accepted, or is refused for its real reason). Find the shape that loses `edits` and fix the root cause. Also test the refusal sentence for a repair sent without `edits`.
- [ ] **Step 2:** Write failing tests for the `list_api_templates` filters, paging, the compact index, the `id` detail, and Review Focus 4. Use a generated store of 300 templates and assert the no-argument answer is under 20_000 characters. Implement it, and update the tool's description and schema.
- [ ] **Step 3:** Write failing tests for the iframe step escaping (spec §3; values with a space, a quote, a leading digit, a colon and a backslash, where the printed step parses and matches) and for `FrameUnreachable` classification of the frame-unreachable sentence. Implement both.
- [ ] **Step 4:** Run the focused modules, then `cd src-tauri && cargo test --tests` once. Commit: `fix(v2): repairs keep their edits, the template list pages, and frames escape and classify`.

### Task 2: `when_visible` in scripts (§4)

**Files:**
- `src-tauri/src/browser/actions.rs`: the `Action::WhenVisible { selector, within_ms: Option<u32>, then: Vec<Action> }` variant, its validation, `is_check` = false, and `run()` answering "carried out by the runner".
- `src-tauri/src/autorun/runner.rs`: `run_step_routed` carries it out.
- `src-tauri/src/autorun/floor.rs`, plus `src/screens/AutoRun/floor.ts` if it counts checks: inner actions never count.
- `src-tauri/src/autorun/recipe.rs`: reuse its `when_visible` wording and limits where they fit.
- `src-tauri/src/autorun/guide.rs`: the action list and a short section.
- The ScriptEditor JSON validation, if it lists kinds.
- Tests: `browser_actions.rs` / `autorun_runner.rs` / `autorun_guide.rs`, plus a live test.

- [ ] **Step 1:** Write failing tests. Validation:
  - refuses a nested `when_visible`, `sign_in`, checks inside `then`, `within_ms` over 10000, and an empty `then`;
  - accepts the cookie-banner example.

  Runner, with the fake driver:
  - target shown: the `then` actions run, and their outcomes are recorded;
  - target not shown: the outcome is "not shown, skipped" and the step passes;
  - a `then` action fails: the step fails.

  Also: the floor does not count the inner checks, and the guide lists the kind. A live test: a fixture whose banner appears after 500 ms gets dismissed, and the next step works.
- [ ] **Step 2:** Implement it. Regenerate the bindings.
- [ ] **Step 3:** Run the focused tests, the live test once with `--ignored`, then `npm test` and `cargo test --tests`, one at a time. Commit: `feat(v2): Auto Run scripts can dismiss something only if it shows up`.

### Task 3: Module-path retry and transient retry (§5, §6)

**Files:**
- `src-tauri/src/autorun/replay.rs`: the module trip ~231-245 and the per-case loop.
- The supervised module trip, wherever `nav::go_to_module` is called for supervised runs.
- `src-tauri/src/autorun/mod.rs`: `CaseRecord.retried: Option<String>`, with serde default and skip-if-none.
- A new `autorun/transient.rs` (pure classification).
- The replay command's options: a `retry_transient: bool` argument.
- `src/screens/AutoRun/ReplayPane.tsx`: the option, remembered in localStorage.
- The "Retried" label in `RunReview.tsx`, `PastRuns.tsx` and `report.rs`.
- Tests in `tests/suite` and vitest.

**Interfaces:**
- Produces:
  - `transient::is_transient(case: &CaseRecord, script: Option<&CaseScript>) -> Option<String>`, returning the first failure sentence when the case is transient;
  - `CaseRecord.retried`;
  - the replay option `retry_transient`.

- [ ] **Step 1:** Write failing tests for the classifier. It must classify:
  - 502, 503 and 504;
  - a 400 with an empty body;
  - `net::ERR_*`;
  - the browser stopping (harness).

  It must not classify a text mismatch, a 400 with a body, or a 404.
- [ ] **Step 2:** Write failing tests for the replay with the fake browsers.
  - Module retry: the first trip fails, the reload succeeds, the second trip succeeds, and the case proceeds. Both trips failing gives exactly one report, carrying the "tried twice" sentence.
  - Transient retry: a transient first attempt followed by a passing second attempt gives Passed with the reason, and `retried` is set. A transient followed by a failure is reported once, with both sentences. An assertion failure is never retried. With the option off, nothing is retried.
  - Review Focus 2 and 3.
- [ ] **Step 3:** Implement it. Regenerate the bindings.
- [ ] **Step 4:** Webview.
  - The ReplayPane option `Retry transient failures once`: on by default, remembered, and passed to the command.
  - A `Retried` label (warning tone, with an accessible title holding the first sentence) in RunReview and on Past runs cases, plus the report's line.

  Add vitest coverage, and keep ui-consistency and a11y green.
- [ ] **Step 5:** Run the focused tests, `npx tsc --noEmit`, `npm test` and `cd src-tauri && cargo test --tests`, one at a time. Commit: `feat(v2): Auto Run retries a stalled module trip and a transient failure once`.

### Task 4: Built-in recipe cookie dismissal and the further iframe follow-ups (§8, §9)

**Files:**
- `src-tauri/src/autorun/builtin_recipe.json`
- `src-tauri/src/browser/input.rs` (`PROBE_JS`: re-centre the point after clipping)
- `src-tauri/src/browser/locator.rs` (record the unreachable sentence only when no frame at that step was entered)
- `src-tauri/src/browser/snapshot.rs` (the note for frames deeper than 3)
- `src-tauri/src/browser/expect.rs` or wherever `check_text` reads page text (include same-origin frames)
- `src-tauri/src/autorun/guide.rs` (the `check_text` sentence about cross-origin frames)
- Tests: the recipe module, `browser_locator.rs`, `browser_snapshot.rs`, `browser_input.rs`, plus a live test using `tests/fixtures/autorun-iframe.html`, extended if needed.

- [ ] **Step 1:** Write failing tests:
  - the built-in recipe's first `after_sign_in` entry is the exact `#btnCookieClose` `when_visible` from spec §8;
  - the probe's point lies inside the clipped box, with a live test where a button is half-hidden by the frame's edge and the click lands;
  - a step with one entered frame and one unreachable frame records no sentence;
  - the deep-frame note;
  - `check_text` finds text that exists only inside a same-origin frame, with a live test.
- [ ] **Step 2:** Implement, then update the guide sentence.
- [ ] **Step 3:** Run the focused tests, the live tests once with `--ignored --test-threads=1`, then `cargo test --tests` and `npm test`, one at a time. Commit: `fix(v2): the built-in recipe dismisses the cookie bar after every sign-in, and frame follow-ups`.
