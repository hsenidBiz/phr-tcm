# Auto Run replay to step N Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The person (a button) or the assistant (a tool) can have the supervised Auto Run browser replay a case's saved steps 1 to N-1 and stop before step N, ready for healing.

**Architecture:**
- One engine function in a new `autorun/replay_to.rs` does the work, in the spec's order: checks, browser, preconditions, guard, sign-in, area trip, steps. It runs on the supervised session that `commands/autorun.rs` holds.
- A Tauri command serves the person's button.
- An AI bridge route plus an MCP tool serve the assistant. A must-not-save script first asks the person in the app, through an event and an answer command.

**Tech Stack:** Rust (tokio, CDP driver, the AI bridge's HTTP routes, MCP), React 19 + TypeScript, vitest.

**Spec:** docs/superpowers/specs/2026-10-05-autorun-replay-to-step-design.md

## Global Constraints

- Rust tests only in `src-tauri/tests/suite/`, with a `mod` line in `suite/main.rs`. Process-wide state takes a lock from `suite/serial.rs`. Live tests are `#[ignore = "starts a real headless Edge"]`.
- Never hand-edit `src/bindings.ts`. Regenerate it with `cd src-tauri && cargo test --test bindings`, and restore line-ending-only drift.
- Colours from tokens, icons from `src/lib/actionIcons.ts`. `ui-consistency` and `a11y` stay as they are.
- Plain sentences, no em dashes. No password, cookie, host, query string or SQL in any sentence, event or log line.
- Every sentence in spec §1 to §3 is verbatim.
- The lease, the no-save guard, preconditions and the Database Read Access rule apply exactly as in a supervised run.
- One test command at a time. Commit with a Bash heredoc ending `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Stage by name.

## Review Focus

1. **A replay started while the person is mid-way through a watched run** in the same supervised browser. The replay refuses with `a replay is already running - wait for it to finish` only if a replay runs. Otherwise it signs in afresh and travels, and the person's pane shows it.
2. **Step N equal to the last step plus 1** replays every step and reports it.
3. **The supervised browser closed by the person mid-replay:** the engine ends with `the replay was stopped at step <k>` and never panics or hangs on the session lock.
4. **The Allow modal and a closed app window:** the app stays in the tray and the modal shows when the window reopens. With no answer in 2 minutes, the assistant hears `the person did not answer within 2 minutes`.
5. **A script whose area cannot be built** (no area saved): the replay stops before step 1 with the area sentence the watched run already uses (`NEEDS_SCRIPT_AREA` or `area_route`'s error), never a panic.

---

### Task 1: The engine and the person's command

**Files:**
- A new `src-tauri/src/autorun/replay_to.rs`.
- `src-tauri/src/autorun/mod.rs`.
- `src-tauri/src/commands/autorun.rs`:
  - the new command `auto_run_replay_to_step`;
  - the stop command;
  - a shared "open if none" helper;
  - a remembered last browser name, kept wherever the app keeps small Auto Run settings (one string, default `edge`, set whenever `auto_run_open_browser` or an unattended run picks one).
- `src-tauri/src/lib.rs`: register the commands.
- Tests: a new `tests/suite/autorun_replay_to.rs`, plus `browser_live.rs`.

**Interfaces:**
- Produces:
  - `pub struct ReplayRequest { pub case_id: i32, pub step: i32, pub db_read_access: bool }`
  - `pub enum ReplayEnd { Ready { step: i32, notice: Option<String> }, StoppedAt { step: i32, why: String, outcomes: Vec<ActionOutcome> }, Stopped { step: i32 }, Refused(String) }`
  - `ReplayEnd::sentence(&self) -> String`, giving the spec §1 sentences verbatim.
  - `pub async fn replay_to<D: Driver>(d: &mut D, root: &Path, org: &str, project: &str, req: &ReplayRequest, account: &mut Option<String>, lease: &mut Held, cancel: &AtomicBool, progress: impl FnMut(i32, i32)) -> ReplayEnd`. `progress(k, total)` is called before each step.
  - The command `auto_run_replay_to_step(app, organization, project, case_id, step, db_read_access) -> Result<ReplayEnd, String>`.
    - It takes the supervised session, opening one first if none is open, with the remembered browser.
    - It emits `autorun-replay-progress { case_id, step, of }` per step.
    - Only one at a time, through a static flag (`a replay is already running - wait for it to finish`).
  - The command `auto_run_stop_replay()`, which sets the cancel flag. Closing the supervised browser sets it too.
  - `ReplayEnd` derives `specta::Type` (the bindings are regenerated).

- [ ] **Step 1:** Write failing tests, with the fake driver and a temp store.
  - The refusals:
    - `case <id> has no saved script`;
    - `step <n> is not in case <id>'s script (it has steps 1 to <last>)` for 0, for a negative step, and for last+2.
  - Steps 1..N-1 run and step N does not (assert the clicks).
  - N = last+1 runs every step (Review Focus 2).
  - A failure at step k gives `replay stopped at step <k>: <sentence>`, with its outcomes.
  - Cancel set mid-replay gives `the replay was stopped at step <k>`.
  - A precondition not met gives its Blocked sentence, and nothing is signed in.
  - Database Read Access off: no database is asked, and the notice is carried on `Ready`.
  - A held account gives the lease sentence.
  - A no-save script sets the guard before step 1.
  - No saved area gives the area sentence before step 1 (Review Focus 5).
  - A second concurrent replay is refused.
  - Live: a three-step fixture replayed to step 3 leaves the page where step 2 left it.
- [ ] **Step 2:** Implement.
  - Reuse: `preconditions::check_case`/`for_run` (the supervised precondition path), `guard_for_case`, the supervised sign-in path with the session lease, and the unattended run's trip to the module (`replay.rs`, the `MODULE_STEP` code) driven by `area_route`. Then `run_step_in_run` per step, with `InRun` carrying `cancel`.
  - Factor out what `replay.rs` and `auto_run_step` already do, rather than copy it.
- [ ] **Step 3:** Regenerate the bindings. Run the focused tests, the live module once (`--ignored --test-threads=1`), `cargo test --tests`, `npx tsc --noEmit` and `npm test`, one at a time. Commit `feat(v2): Auto Run can replay a case to the step before the one being healed`.

### Task 2: The assistant's tool and the Allow prompt

**Files:**
- `src-tauri/src/ai_bridge.rs`: the route `POST /autorun-replay` with body `{ case_id, step }`.
- `src-tauri/src/mcp.rs`: the tool `replay_autorun_to_step`. Its description says what it does, that a must-not-save script asks the person, and never to replay past the failing step.
- `src-tauri/src/ai_tools.rs`: the tool is in the Auto Run set (offered and switched off the same way).
- A new `src-tauri/src/autorun/replay_ask.rs`: the pending-request registry. One at a time, with a `tokio::sync::oneshot` and a timeout parameter (2 minutes in production).
- `src-tauri/src/commands/autorun.rs`: the command `auto_run_answer_replay_request(id: String, allow: bool)`.
- The webview:
  - a new `src/screens/AutoRun/ReplayRequestModal.tsx`, mounted once at the app level (wherever other global listeners live in `App.tsx`);
  - it listens for `autorun-replay-request { id, case_id, title, step }`;
  - it shows the spec §3 text verbatim, with Allow and Deny.
- `src-tauri/src/autorun/guide.rs`: the healing section's three points (spec §3).
- Tests:
  - `tests/suite/autorun_replay_ask.rs` (new);
  - `ai_bridge` tests: the route;
  - `tcm_mcp` tests: the tool is listed and gated;
  - `autorun_guide`;
  - vitest for the modal.

**Interfaces:**
- Consumes: Task 1's `replay_to`, `ReplayEnd`, the open-if-none helper and the one-at-a-time flag.
- The bridge answer:
  - on `Ready`: `{ "sentence": ..., "page": <get_autorun_page's snapshot shape> }`;
  - otherwise: `{ "sentence": ... }`.
- `db_read_access` for the assistant is `!ctx.disabled_tools.contains("db_query")`.

- [ ] **Step 1:** Write failing tests.
  - A must-not-save request:
    - emits the event and waits;
    - Allow runs the replay;
    - Deny gives `the person declined the replay`;
    - the timeout (as a test parameter) gives `the person did not answer within 2 minutes`;
    - a second request while one waits gives `a replay request is already waiting for the person`.
  - A normal script runs with no event.
  - The tool is listed with the Auto Run tools, and its absence or switching off follows them.
  - The guide contains the three points.
  - The modal renders the exact text and calls the answer command with allow or deny.
- [ ] **Step 2:** Implement.
- [ ] **Step 3:** Regenerate the bindings. Run the focused tests, `cargo test --tests`, `npx tsc --noEmit` and `npm test`, one at a time. Commit `feat(v2): the assistant can replay a case to the failing step, and asks first on a must-not-save script`.

### Task 3: The person's button and the pane

**Files:**
- `src/screens/AutoRun/RunReview.tsx` and `PastRuns.tsx`: a `Replay to step <n>` button, with the accessible name `Replay case <id> to step <n>`, on a failed or blocked case whose failing step is known. Find how the failing step is already worked out for a case (the review's step marks, or `proposed`/`reason`). Pick the first failed step. Hide the button when none is known.
- `src/screens/AutoRun/index.tsx` and `RunPane.tsx`:
  - pressing the button opens the supervised pane on that case, if it is not already showing, and starts the replay through `auto_run_replay_to_step` (passing `dbReadAccessOn()`);
  - the pane shows `replaying step K of N-1` from `autorun-replay-progress`, then the final sentence;
  - it marks steps 1..N-1 as run, or the failing one as failed, the way a watched run marks them;
  - step N is the next one, so the person can continue by hand;
  - a Stop control calls `auto_run_stop_replay`.
- Tests: vitest in `RunReview.test.tsx`, `PastRuns.test.tsx` and `RunPane.test.tsx`.

**Interfaces:**
- Consumes: the Task 1 commands, `ReplayEnd` and the progress event.

- [ ] **Step 1:** Write failing vitest tests.
  - The button appears only when a failing step is known, with the right n and name.
  - Pressing it opens the pane and calls the command with that case and step.
  - Progress text updates from events.
  - Each `ReplayEnd` shows its sentence.
  - After `Ready`, step N is the next step and earlier ones are marked.
  - Stop calls the stop command.
- [ ] **Step 2:** Implement.
- [ ] **Step 3:** Run `npx tsc --noEmit`, the focused vitest files, ui-consistency and a11y, then `npm test`, one at a time. Commit `feat(v2): a failed case can be replayed to its failing step from the run review and Past runs`.
