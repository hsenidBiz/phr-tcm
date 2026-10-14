# Network Pause Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An unattended Auto Run pauses when the active environment's database server stops answering, waits for the VPN to come back (Stop available, 30-minute limit), and resumes by itself, re-running the case the drop interrupted.

**Architecture:** A small `autorun::network` module resolves the environment's database server (no login) and checks it with a TCP connect. The run's existing pause gate (`replay::ResetGate`) gains two default methods - "is the network there?" and "wait for it" - that do nothing for every existing gate, so only the app's gate (`reset_wait::AppGate`) changes behaviour. The run loop asks before each case and after a Failed/Blocked one. The wait has its own registry, event, command and run record; the webview shows it in the run pill and a panel beside the reset pause, and notifies; the assistant gets one read-only tool.

**Tech Stack:** Rust (Tauri 2, tokio, tauri-specta), React 19 + TypeScript, vitest.

**Spec:** `docs/superpowers/specs/2026-10-14-network-pause-design.md`

## Global Constraints

- Only the unattended run (`auto_run_replay` → `replay::run_cases_planned`) pauses. Supervised runs, replay-to-step and discovery are untouched.
- Active only when `ai_tools::autorun_offered()` is true AND the active environment's database resolves to a host and port; otherwise no check is made and runs behave exactly as today.
- Reachable = a TCP connection to the database server's host:port succeeds within **3 s**. No login, no query, no `sqlcmd`.
- Port: `DbDatabase.port`, else a `host,port` in `server`, else **1433**. A `tcp:` prefix is stripped. A named instance (`host\instance`) with no port anywhere → no check.
- Recheck every **5 s**; give up after **30 minutes** per pause; a case is re-run after a network pause at most **3** times.
- Reasons, verbatim:
  - stopped: `the run was stopped while the company network was unreachable`
  - gave up: `the company network was unreachable for 30 minutes`
  - re-run cap note appended to a kept record's reason: ` (the company network was unreachable each time this case ran)`
- Never a host, port, address or password in an event, a run file, a notification or an assistant answer.
- Rust tests only under `src-tauri/tests/suite/<module>.rs` with a `mod` line in `suite/main.rs`. `src/bindings.ts` is generated (`cargo test --test bindings`), never hand-edited. Colours from tokens; icons from `src/lib/actionIcons.ts`.
- Build/test with `CARGO_TARGET_DIR=/d/azure-devops-test-case-creator/src-tauri/target-gate`; one build at a time. Commits via Bash heredoc ending `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. A case that fails while the database answers is recorded exactly as today, transient retry included - the network rule must never swallow a real failure.
2. No database, an unresolvable one, or a named instance without a port: no check is ever made and no time is added to the run.
3. When the server answers, the check costs one fast connect per case; when it hangs, never more than 3 s.
4. Stop during a wait, the 30-minute limit and app exit all end the run cleanly: the run file is saved, every remaining case is recorded as not run with the right reason, and no wait is left in the registry.
5. Nothing the feature writes or emits (event, run file, notification text, assistant answer) contains the host or port.

---

### Task 1: Find and knock on the database server

**Files:**
- Create: `src-tauri/src/autorun/network.rs` (add `pub mod network;` to `src-tauri/src/autorun/mod.rs`)
- Test: `src-tauri/tests/suite/autorun_network.rs`

**Interfaces:**
- Produces:
  - `pub const CONNECT_TIMEOUT: Duration = 3 s; RECHECK: Duration = 5 s; GIVE_UP: Duration = 30 min; MAX_NETWORK_RERUNS: u32 = 3;`
  - `pub fn target_of(server: &str, port: Option<u16>) -> Option<(String, u16)>`
  - `pub fn db_target(store: &dyn crate::db::SecretStore, db_id: &str) -> Option<(String, u16)>` - finds `db_id` in `crate::db::credentials::databases(store)` and applies `target_of(&d.server, d.port)`; `None` for a blank or unknown id.
  - `pub async fn answers(host: &str, port: u16, timeout: Duration) -> bool` - `tokio::net::TcpStream::connect` under `tokio::time::timeout`.

- [ ] **Step 1: Failing tests**
  - `target_of`: `("db1", None)` → `("db1", 1433)`; `("db1", Some(1444))` → 1444; `("db1,1500", None)` → 1500; `("tcp:db1,1500", None)` → `("db1", 1500)`; `("tcp:db1", None)` → 1433; `("db1\\SQL2019", None)` → `None`; `("db1\\SQL2019", Some(1600))` → `("db1", 1600)`; `("", None)` and `("  ", Some(1))` → `None`; surrounding whitespace trimmed.
  - `db_target` with `MemoryStore`: a saved custom database resolves; an unknown id and `""` → `None`.
  - `answers`: a `TcpListener` bound on `127.0.0.1:0` → `true`; a port just released → `false`, within `CONNECT_TIMEOUT + 1 s`.
- [ ] **Step 2: Run** `cargo test --test suite autorun_network::` - FAIL (module missing).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** - PASS.
- [ ] **Step 5: Commit** `feat(v2): find and knock on the environment's database server`.

### Task 2: The wait itself

**Files:**
- Modify: `src-tauri/src/autorun/network.rs`
- Test: `src-tauri/tests/suite/autorun_network.rs`

**Interfaces:**
- Produces:
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Network { Reachable, Unreachable, Unchecked }`
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum WaitEnd { Resumed, Stopped, GaveUp }` with `pub fn outcome(self) -> &'static str` → `"resumed" | "stopped" | "gave_up"`.
  - `pub async fn wait_until<F, Fut>(mut check: F, stop: &AtomicBool, cancel: &AtomicBool, recheck: Duration, limit: Duration) -> WaitEnd where F: FnMut() -> Fut, Fut: Future<Output = bool>` - checks at once, then every `recheck`; `Resumed` when a check is true; `Stopped` as soon as `stop` or `cancel` is set (polled at least every 50 ms, as `reset_wait` does); `GaveUp` when `limit` passes.
  - `pub struct NetworkWaits` (single slot, `Mutex`) with `pub fn begin(&self, run_id: &str, before_case_id: i32, limit: Duration) -> Arc<AtomicBool>` (the stop flag), `pub fn end(&self, run_id: &str)`, `pub fn stop(&self, run_id: &str) -> bool`, `pub fn stop_for_exit(&self) -> bool`, `pub fn current(&self) -> Option<WaitView>`; `pub fn waits() -> &'static NetworkWaits`.
  - `#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)] pub struct WaitView { pub run_id: String, pub before_case_id: i32, pub waited_s: u32, pub give_up_in_s: u32 }`

- [ ] **Step 1: Failing tests** (millisecond durations; no real 30 minutes):
  - `wait_until` returns `Resumed` when the check turns true on the third call; `Stopped` when `stop` is set mid-wait (returns within 200 ms); `Stopped` on `cancel`; `GaveUp` after `limit` with the check never true.
  - `NetworkWaits`: `begin` then `current()` names the run and case; `stop(run_id)` sets the returned flag and is `false` for another run id; `end` clears `current()`; `stop_for_exit` stops whatever waits and is `false` when nothing does.
- [ ] **Step 2: Run** - FAIL. **Step 3: Implement.** **Step 4: Run** - PASS.
- [ ] **Step 5: Commit** `feat(v2): an unattended run's wait for the network`.

### Task 3: The run loop pauses, resumes and re-runs

**Files:**
- Modify: `src-tauri/src/autorun/replay.rs` (`ResetGate`, `run_cases_planned`), `src-tauri/src/autorun/mod.rs` (`LocalRun`, new record)
- Test: `src-tauri/tests/suite/autorun_network_pause.rs` (reuse the fakes `tests/suite/autorun_reset.rs` uses for `run_cases_planned`)

**Interfaces:**
- Consumes: `network::{Network, WaitEnd, MAX_NETWORK_RERUNS}`.
- Produces:
  - `ResetGate` gains default methods (existing gates unchanged):
    - `fn network(&mut self) -> impl Future<Output = Network> { std::future::ready(Network::Unchecked) }`
    - `fn wait_for_network(&mut self, before_case_id: i32, cancel: &AtomicBool) -> impl Future<Output = WaitEnd> { std::future::ready(WaitEnd::Resumed) }`
  - `#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize, specta::Type)] pub struct NetworkPauseRecord { pub before_case_id: i32, pub waited_ms: u32, pub outcome: String }` and `LocalRun.network_pauses: Vec<NetworkPauseRecord>` (`#[serde(default, skip_serializing_if = "Vec::is_empty")]`).
  - Reason constants in `replay.rs`: `NETWORK_STOPPED`, `NETWORK_GAVE_UP`, `NETWORK_EVERY_TIME` (values in Global Constraints).
- Rules (in `run_cases_planned`, per case, after the reset pause):
  1. Before the case: `gate.network()`; `Unreachable` → wait (rule 4).
  2. Run the case exactly as today, transient retry included, to its final record.
  3. If that record's `proposed` is `"Failed"` or `"Blocked"` and `gate.network()` is `Unreachable`: if this case has been re-run for the network fewer than `MAX_NETWORK_RERUNS` times → discard the record, wait (rule 4), run the case again from rule 2 (setup included); else keep the record with `NETWORK_EVERY_TIME` appended to its `reason`.
  4. A wait pushes a `NetworkPauseRecord` (waited ms, outcome). `Resumed` → carry on; `Stopped`/`GaveUp` → this case and every remaining one recorded the way `stopped_at_reset` records them, with `NETWORK_STOPPED` / `NETWORK_GAVE_UP` as the reason, then the run ends.
  - `Unchecked` anywhere = today's behaviour.

- [ ] **Step 1: Failing tests** (a scripted gate: a queue of `Network` answers and `WaitEnd`s, recording what it was asked):
  - `unchecked_runs_exactly_as_before` - same records as a `NeverPauses` run.
  - `unreachable_before_a_case_waits_then_runs_it` - one pause record "resumed", case recorded once.
  - `a_failure_while_unreachable_is_rerun_not_recorded` - first go Failed + `Unreachable`, wait, second go Passed → only the Passed record; one pause.
  - `a_failure_while_reachable_is_kept` - Failed + `Reachable` → Failed record, no pause.
  - `reruns_stop_after_three` - always Failed + `Unreachable` → three pauses, final record Failed with the appended note.
  - `stop_and_give_up_end_the_run` - `Stopped` and `GaveUp` each: remaining cases not run with the matching reason, pause outcome recorded, run file saved.
- [ ] **Step 2: Run** - FAIL. **Step 3: Implement.** **Step 4: Run** this module, then `autorun_reset::` and `autorun_replay::` (unchanged behaviour) - PASS.
- [ ] **Step 5: Commit** `feat(v2): an unattended run waits out a dropped network and re-runs the case it interrupted`.

### Task 4: The app's gate, event, Stop and exit

**Files:**
- Modify: `src-tauri/src/autorun/reset_wait.rs` (`AppGate`), `src-tauri/src/commands/autorun_replay.rs`, `src-tauri/src/events.rs`, `src-tauri/src/lib.rs`
- Test: `src-tauri/tests/suite/autorun_network_pause.rs`; bindings regenerated

**Interfaces:**
- Produces:
  - `AppGate` gains `pub network: Option<(String, u16)>` and `pub notify_network: &'a (dyn Fn(&AutorunNetworkWait) + Send + Sync)`; implements `network()` (`Unchecked` when `None`, else `answers(.., CONNECT_TIMEOUT)`) and `wait_for_network` (`waits().begin`, emit `waiting: true`, `wait_until(answers, flag, cancel, RECHECK, GIVE_UP)`, `waits().end`, emit `waiting: false` with the outcome).
  - Event `#[derive(... tauri_specta::Event)] pub struct AutorunNetworkWait { pub run_id: String, pub before_case_id: i32, pub waiting: bool, pub give_up_after_s: u32, pub ended: Option<String> /* "resumed"|"stopped"|"gave_up" */ }` registered in `collect_events!`. No host, no port.
  - `auto_run_replay` resolves `network` once: `None` unless `crate::ai_tools::autorun_offered()`, else `environments::active(&root)` → `network::db_target(secrets, &env.db_id)`.
  - Command `#[tauri::command] pub fn auto_run_stop_network_wait(run_id: String) -> bool` (registered); the app-exit hook (`lib.rs`, beside `stop_paused_run_on_exit`) also calls `network::waits().stop_for_exit()`.

- [ ] **Step 1: Failing tests:** `AppGate` with `network: None` never checks; with a target on a closed local port reports `Unreachable`, and a wait that is stopped through `waits().stop(run_id)` returns `Stopped` and emits `waiting: true` then `waiting: false, ended: Some("stopped")`; with a live local listener, `Reachable`. The emitted payloads, serialised, contain neither the host nor the port.
- [ ] **Step 2: Run** - FAIL. **Step 3: Implement**, then `cargo test --test bindings`. **Step 4: Run** the module + bindings - PASS.
- [ ] **Step 5: Commit** `feat(v2): the app pauses an unattended run for the network and lets the person stop it`.

### Task 5: The assistant can see the wait

**Files:**
- Modify: `src-tauri/src/ai_bridge.rs` (route), `src-tauri/src/mcp.rs` (tool list + call), `src-tauri/src/ai_tools.rs` (`DEV_ONLY_TOOLS`), `src/lib/mcpTools.ts` (mirror)
- Test: the existing bridge/MCP test modules for tools (find where `get_autorun_page` is tested) plus `src/lib/mcpTools.test.ts`

**Interfaces:**
- Produces: route `("GET", "/autorun-network-wait")` → `{"waiting": false}` or `{"waiting": true, "run_id", "before_case_id", "waited_s", "give_up_in_s"}` from `network::waits().current()`; MCP tool `get_autorun_network_wait` (no inputs; description: whether an unattended run is paused because the company network is unreachable, since when and when it gives up; read only - only the network returning, Stop or the limit end it). The `/autorun-` prefix puts it behind the existing Auto Run gate.

- [ ] **Step 1: Failing tests:** the route answers both shapes; the tool is listed only when Auto Run is offered; the TS mirror lists it; the answer contains no host/port.
- [ ] **Step 2–4:** run (FAIL), implement, run (PASS).
- [ ] **Step 5: Commit** `feat(v2): the assistant can see an unattended run waiting for the network`.

### Task 6: The person sees it, is told, and can stop it

**Files:**
- Modify: `src/lib/backgroundRun.ts`, `src/components/RunPill.tsx`, `src/screens/AutoRun/index.tsx`, `src/screens/AutoRun/ReplayPane.tsx`, `src/screens/AutoRun/RunPane.tsx`, the run review header (`src/screens/AutoRun/RunReview.tsx`)
- Create: `src/screens/AutoRun/NetworkWaitPanel.tsx` (+ test)
- Test: `src/lib/backgroundRun.test.ts`, `src/components/RunPill.test.tsx`, `NetworkWaitPanel.test.tsx`, `RunReview.test.tsx`

**Interfaces:**
- Consumes: `events.autorunNetworkWait`, `commands.autoRunStopNetworkWait`, `LocalRun.network_pauses`, `announce` (`src/lib/assignedAlerts.ts`).
- Produces: `BackgroundRun.networkWait: AutorunNetworkWait | null` (set on `waiting: true`, cleared on `waiting: false`); `stopNetworkWait()` in `backgroundRun.ts`.
- Copy, verbatim:
  - Panel title: `Paused - the company network is unreachable`
  - Panel body: `Reconnect the VPN; the run carries on by itself. Gives up in {m} min.` plus the time waited, and a `Stop` button (icon from `actionIcons`).
  - Pill: `Waiting for the network` (warning tone, as the reset pause).
  - Notification on `waiting: true`: title `Auto Run paused`, body `The company network is unreachable. Reconnect the VPN - the run carries on by itself.`
  - Notification on `ended: "gave_up"`: title `Auto Run stopped`, body `The company network was unreachable for 30 minutes.`
  - Review line per pause: `Waited {n} min for the network before #{case}` + ` (stopped)` / ` (gave up)` when not resumed.
- [ ] **Step 1: Failing tests:** the store sets and clears `networkWait` from events and announces once per pause and once on give-up (never on resume); the pill shows the waiting label; the panel's Stop calls the command with the run id and disables while pending; the review lists pauses.
- [ ] **Step 2: Run** the files - FAIL. **Step 3: Implement**; render `NetworkWaitPanel` wherever `ResetNeededPanel` is rendered. **Step 4: Run** the files, `src/ui-consistency.test.ts`, `npx tsc --noEmit` - PASS.
- [ ] **Step 5: Commit** `feat(v2): an unattended run's network pause shows, notifies and can be stopped`.

### Task 7: Verify the whole branch

- [ ] **Step 1:** `cd src-tauri && cargo test --tests --no-fail-fast`, then `npm test`, then `npx tsc --noEmit` - all green (re-run a lone `App.test.tsx`/timing failure alone before believing it).
- [ ] **Step 2: Hand check with Avin (human step):** start an unattended run of a few cases with FortiClient connected; disconnect FortiClient mid-run; confirm the pause, the notification and the pill; reconnect; confirm it resumes and the interrupted case re-runs; confirm the review shows the pause. Then the same with Stop.
- [ ] **Step 3:** Commit any fixes, each on its own.
