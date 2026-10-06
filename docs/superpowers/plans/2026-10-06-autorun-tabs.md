# Auto Run several tabs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** an Auto Run script can follow, open, switch, close and wait for tabs in its one signed-in browser, and every tab is guarded like the first.

**Architecture:**
- **One socket for the whole browser.** The CDP driver moves from one page websocket to the browser's websocket with flattened sessions. `Target.setAutoAttach` (with `waitForDebuggerOnStart` and `flatten`) attaches every new tab.
- **Each tab is set up before it runs.** Every attached tab gets the same setup as the first: the save guard, the page log, dialogs and the seed script. Only then is it released.
- **Actions follow the current tab.** The driver keeps the tabs and a current tab, and `call` goes to the current tab's session, so every existing action becomes tab-aware without changes.

**Tech Stack:** Rust (tokio, the CDP driver), React 19 + TypeScript, vitest.

**Spec:** `docs/superpowers/specs/2026-10-06-autorun-tabs-design.md`

## Global Constraints

- **Tests:**
  - Rust tests go only in `src-tauri/tests/suite/`, with a `mod` line in `suite/main.rs`.
  - Process-wide state takes a lock from `suite/serial.rs`, in the order `activity_log` before `account_leases`.
  - Source-reading tests normalise `\r\n`.
- **Bindings:** never hand-edit `src/bindings.ts`. Regenerate it, and restore any line-ending-only drift.
- **Copy and style:**
  - Spec sentences are used verbatim.
  - Plain sentences, with no em or en dashes.
  - No hosts or query strings in logs, records or sentences.
- **Compatibility:** old scripts and old run files load unchanged.
- **Process:**
  - One test command at a time.
  - Commit with a Bash heredoc ending `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`, and stage by name.

## Review Focus

1. **A tab the page opens before any step expects it.**
   - It is guarded (must-not-save) before its first request.
   - Its dialogs are answered.
   - It is logged as `a tab opened: <address without its query>`.
2. **Closing the current tab.**
   - The current tab becomes `main`.
   - Later steps act in `main`, and nothing hangs on the closed session.
3. **A tab that closes itself while it is current** (for example, a print preview). The next step fails with `there is no tab <name>`. It is never a hang or a panic.
4. **Case end with tabs open.** Every non-main tab is closed before the next case starts, including when the case failed or was stopped.
5. **Downloads, screenshots and the page log from a second tab.**
   - They are attributed to that tab's step.
   - A failure screenshot shows the current tab.

---

### Task 1: One browser socket with a session per tab (no behaviour change)

**Files:**
- `src-tauri/src/browser/cdp.rs`:
  - **Connecting:** `Cdp::connect` connects to `/json/version`'s `webSocketDebuggerUrl`, attaches to the first page with `Target.attachToTarget { flatten: true }`, and sends every command with that `sessionId`.
  - **Per-tab state:** the state each tab owns (`guard`, `page_log`, `net_record`, `dialogs`, events, and the per-target seed script) moves into a `Tab` struct, keyed by `sessionId`.
  - **Event routing:** events carry a `sessionId` and are routed to their tab. Browser-wide events (downloads) go to the tab that started them, where CDP says which. Otherwise they go to the current tab.
  - **New tabs:** `Target.setAutoAttach { autoAttach: true, waitForDebuggerOnStart: true, flatten: true }`. Each tab the page opens gets the same setup as `main` (save guard, page log, dialogs, seed script), and then `Runtime.runIfWaitingForDebugger`.
  - **Current tab:** for this task, only `main`. A tab the page opens is set up and logged, but actions still go to `main`.
  - **The `Driver` trait is unchanged.**
- The fakes (`tests/suite/common.rs` `ScriptedDriver`/`FakePage`, and the `FakeBrowsers` in the runner/replay suites) tolerate a session id, or get one. The existing tests must pass unchanged in what they assert.
- Tests: `tests/suite/browser_tabs.rs` (new).

**Interfaces:**
- **Produces:**
  - `Tab { session_id, target_id, name: Option<String>, url_without_query }`;
  - `Cdp::tabs()`;
  - `Cdp::current()`;
  - the internal `route_event`.

- [ ] **Step 1:** Write failing tests, using a fake browser socket that speaks flattened sessions:
  - commands carry the main session id;
  - events with another session id do not reach `main`;
  - a tab the page opens gets `Fetch.enable` (guard), `Network.enable`/`Runtime.enable` (page log) and the dialog handler before `runIfWaitingForDebugger`, and it is logged.
  - Review Focus 1: with the must-not-save guard on, a save sent by the popup is stopped.
- [ ] **Step 2:** Implement, then run, one at a time:
  1. the focused tests;
  2. `cargo test --tests`;
  3. the live Edge tests (`browser_live`), if this machine runs them today.
- [ ] **Step 3:** Commit `feat(v2): Auto Run drives the whole browser over one connection, and a tab the page opens is guarded like the first`.

### Task 2: The tab actions

**Files:**
- `src-tauri/src/browser/actions.rs`:
  - **New variants:** `ExpectTab { name, url_contains, within_ms }`, `OpenTab { name, url }`, `SwitchTab { name }`, `CloseTab { name }` and `ExpectTabClosed { name, within_ms }`.
  - **`validate`:** the name rules use the spec sentences. `OpenTab`'s url follows `Navigate`'s origin rules and refusal sentences. `main` cannot be closed.
  - **`is_check`:** `ExpectTab` and `ExpectTabClosed` are checks. The other three are actions.
  - **`run`:**
    - `ExpectTab` names the newest unnamed tab opened since the previous step, waiting up to `within_ms` (10 s by default).
    - `SwitchTab` and `CloseTab` change the current tab. Closing the current tab makes `main` current.
- `cdp.rs`:
  - `set_current(name)`;
  - `name_tab`;
  - `close_tab`;
  - `open_tab` (`Target.createTarget` in the same browser context; it is attached through the same set-up path before navigation);
  - when the current tab's target closes, the next call fails with `there is no tab <name>` (Review Focus 3).
- `autorun/runner.rs` and `replay.rs`: at case end, every non-main tab is closed on every path (Review Focus 4). Screenshots use the current tab. Downloads and the page log are attributed per tab (Review Focus 5).
- `autorun/report.rs` `action_words`, `autorun/patterns.rs`, `ai_bridge::describe_try` and `autorun/guide.rs`: words for each new action. The guide gets the three scenarios, with one example each, as the spec asks.
- Regenerate the bindings.
- Tests:
  - `tests/suite/browser_tabs.rs`;
  - the runner, replay and guide suites;
  - an old script round-trips byte-identical.

**Interfaces:**
- **Consumes:** Task 1's `Tab` and session routing.

- [ ] **Step 1:** Write failing tests:
  - each action and each of its failure sentences, verbatim from spec §1 and §2;
  - steps act in the current tab;
  - Review Focus 2, 3, 4 and 5;
  - an unexpected tab is logged and does not fail the case;
  - replay to step N recreates tabs by running the earlier steps.
- [ ] **Step 2:** Implement, then run, one at a time:
  1. the focused tests;
  2. `cargo test --tests`;
  3. `npx tsc --noEmit`;
  4. `npm test`.
- [ ] **Step 3:** Commit `feat(v2): Auto Run scripts can follow, open, switch, close and wait for tabs`.

### Task 3: The webview and the live check

**Files:**
- `src/screens/AutoRun/RunPane.tsx`: `in tab <name>` beside the step, whenever the current tab is not `main`. The current tab comes from the step result. Add `tab: Option<String>` to the step record in Rust if it is not there already, and regenerate.
- `src/screens/AutoRun/describeAction.ts`, from the readable script work: sentences for the five new kinds. Examples:
  - `Wait for a new tab and call it "report"`;
  - `Open a new tab "second" at /hr/...`;
  - `Switch to the "report" tab`;
  - `Close the "report" tab`;
  - `Check the "preview" tab closes`.

  The exhaustive switch makes `tsc` require them.
- Past runs and the review: show `in tab <name>` on step lines where a step ran outside `main`.
- **Live test** (`tests/suite/browser_live.rs`, headless Edge):
  - Use a local fixture page with a `target=_blank` link.
  - The script follows the link, checks the new tab, closes it and returns to `main`.
  - With the guard on, a form post from the new tab is stopped.
- Tests: vitest for the label in the pane and in the review, and the describer sentences.

- [ ] **Step 1:** Write failing tests (vitest and the live test).
- [ ] **Step 2:** Implement, then run, one at a time:
  1. the live test;
  2. `cargo test --tests`;
  3. `npx tsc --noEmit`;
  4. the focused files;
  5. ui-consistency and a11y;
  6. `npm test`.
- [ ] **Step 3:** Commit `feat(v2): the run pane and the review say which tab a step ran in`.
