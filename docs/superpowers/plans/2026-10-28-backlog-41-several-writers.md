# Backlog 41: Several Writers Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix what the first three-writer Auto Run run hit (backlog item 41): unclear action refusals and hung waits, menus that differ by account, unexplained refused sessions, the cookie banner eating the first discovery click, discovery starting at home instead of its area, subagent reports lost on a failed hand-back, and splitting cases between writers by hand.

**Architecture:** Rust changes in the MCP proxy (`src-tauri/src/mcp.rs`), the bridge (`src-tauri/src/ai_bridge.rs`), Auto Run navigation and sign-in (`src-tauri/src/autorun/{nav,replay,runner,signin,popups}.rs`), and the generated assistant text (`src-tauri/src/ai_tools.rs`, `src-tauri/src/autorun/guide.rs`). No webview change, no new dependency, no ADO call.

**Tech Stack:** Rust (tokio, serde_json), integration tests in `src-tauri/tests/suite/` (one binary, one module per file).

**Spec:** `docs/autorun/backlog-2026-10.md`, item 41 (section "Several writers at once"). Owner rulings 2026-10-28: a differing menu gets a **clear failure sentence**, not a path per menu shape; the coordinator's per-account split **is built now**; the item's "end_autorun_discovery takes no agent" was an outdated installed build (2.3.1-beta.4) - the source already adds `agent` to every Auto Run and API template tool and `tests/suite/tcm_mcp.rs::every_auto_run_and_template_tool_takes_an_agent` enforces it, so only a gap test is added. "My Assessments stays blank" is a site quirk recorded at run time, not code. The hand-back refusal itself is Claude Code's; only the TCM side (report files) is built.

## Global Constraints

- Never an HTTP DELETE to Azure DevOps; no secret, token, URL or file path in any user-facing or assistant-facing sentence or applog line.
- Never hand-edit `src/bindings.ts`. If a command signature changes, regenerate with `cargo test --test bindings` (none is expected to).
- Every Rust test is an integration test under `src-tauri/tests/suite/` (one module per file, `mod` line in `suite/main.rs`); tests touching process-wide state take a lock from `suite/serial.rs`.
- Saved scripts keep the rule that `wait_for` needs `timeout_ms` (`autorun/guide.rs` "no defaults ... leave it out and the save is rejected"): the default applies only to discovery / try actions, never to save validation or the `Action` type.
- Assistant-facing sentences are plain English, no em or en dashes inside new sentences other than the existing " - " house separator.
- One test command at a time; never set `CARGO_TARGET_DIR`. Run one module: `cd src-tauri && cargo test --test suite <module>::`.
- Commit with a bash heredoc (`git commit -F - <<'EOF'`), ending `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Never stage `src-tauri/Cargo.toml` drift, `src/bindings.ts` drift, or `.test-cases/`.

## Review Focus

- A `wait_for` (or any wait kind with a required `timeout_ms`) sent to discovery without `timeout_ms` runs with the discovery timing's `expect_ms`; the same action in `save_autorun_script` is still rejected.
- A discovery action whose own wait is huge (`timeout_ms: 4000000000`) answers within that action's capped wait plus the margin, never hangs the call.
- A wrong field on `click` (`"target"` instead of `"selector"`) is refused with a sentence naming `selector` and showing a click example; an unknown kind lists the kinds.
- An area walk failing because a click never found its target, with an account known, says the account's menu has no such control; an ambiguous-match failure (`AREA_PATH_NOT_EXACT`) does not get the menu sentence.
- `start_autorun_discovery` with an `area` that is not a saved area behaves as today (goes home, files under that label) and says so; with a saved area whose walk fails, the discovery stays started and its browser stays open where it stopped.

---

### Task 1: The agent gap test and clearer action refusals, with bounded waits

**Files:**
- Modify: `src-tauri/src/mcp.rs` (`replay_call` ~1825), `src-tauri/src/ai_bridge.rs` (the discovery parse arms ~8133 and ~8214, the try route `/autorun-try` ~554 and its parser, `discover_actions_paged` ~6004)
- Possibly create: a small pure module for the examples table and the default-filling, e.g. `src-tauri/src/autorun/action_help.rs` (declared in `autorun/mod.rs`)
- Test: `src-tauri/tests/suite/tcm_mcp.rs` (gap test), new `src-tauri/tests/suite/action_help.rs` (+ `mod action_help;` in `suite/main.rs`)

**Interfaces (produces):**
- `pub fn fill_wait_defaults(action: &mut serde_json::Value, default_ms: u32) -> bool` - for any action object whose kind has a required `timeout_ms` (find every such variant of `browser::actions::Action`; `wait_for` at least) and no `timeout_ms`, inserts `default_ms`; returns whether it filled one.
- `pub fn refusal_for(action: &serde_json::Value, serde_error: &str) -> String` - the refusal sentence: for a known kind, `that is not a <kind> action: <serde error>. A <kind> action looks like <example JSON>`; for a missing or unknown kind, `"<kind>" is not an action kind; the kinds are <comma list>`; always ending ` - call get_guide (topic autorun) for the vocabulary.`
- `pub fn example_for(kind: &str) -> Option<&'static str>` - one compact valid JSON example per action kind, taken from the guide's vocabulary (`autorun/guide.rs` "## The actions", ~242-300). A test parses every example as an `Action` so the table cannot drift.
- `pub const WAIT_MARGIN_MS: u64 = 5_000;` and a cap `pub const MAX_DISCOVERY_WAIT_MS: u32 = 120_000;` - a discovery or try action's `timeout_ms` above the cap is lowered to it (the answer notes "timeout_ms lowered to 120000"), and each action's execution is wrapped in `tokio::time::timeout(its wait + WAIT_MARGIN_MS)`; on expiry the batch stops with `action <n> did not answer within <s> s; the batch stopped there`.

- [ ] **Step 1: Tests.**
  - `tcm_mcp.rs`: `every_tool_routed_through_a_browser_slot_takes_an_agent` - for every tool in `tools_list` whose bridge route is one `commands::autorun_slots` gives a slot need (`need_of` returns Some, expose a small pub helper if needed, or a pub list of routes), assert the tool is in `AUTORUN_TOOLS` or `API_TEMPLATE_TOOLS`.
  - `action_help.rs`: `fill_wait_defaults` fills `{"kind":"wait_for","selector":{"css":"#x"}}` with 10000 and leaves one that has `timeout_ms` alone; every `example_for` kind parses as `Action`; `refusal_for({"kind":"click","target":"#x"}, ..)` contains `selector` and `"kind":"click"`; `refusal_for({"kind":"clik"}, ..)` lists `click` and `wait_for`.
  - A save of a script with a `wait_for` lacking `timeout_ms` is still rejected (find the existing test; if it exists, leave it; if not, add it to the save-validation test module).
  - `replay_call` with `{"case_id":1,"step_index":2}` answers an error naming `step_index` and listing `case_id, step, page, stop` (mirror the `probe_call` / `SHOW_NEEDS_A_SELECTOR` test style).
  - A discovery with one `wait_for` whose `timeout_ms` is above the cap answers with the "lowered" note (use the bridge test harness the existing discovery tests use; if no driver-free path exists, test the clamp in `action_help` as a pure `clamp_wait(&mut Value) -> bool` and the timeout wrapper by a unit-level future that never resolves).
- [ ] **Step 2: Run** `cargo test --test suite action_help::` and the new `tcm_mcp::` test; expect FAIL.
- [ ] **Step 3: Implement.** Fill defaults and clamp on the parsed JSON before `serde_json::from_value::<Action>` in the discovery (one and several) and try paths only, with the default `Timing::supervised().expect_ms` (or the timing those paths already use). Replace the two `that is not an action` messages with `refusal_for`. `replay_call` refuses keys other than `case_id`, `step`, `page`, `stop` (and the `agent` already stripped earlier). Update the guide's discovery notes (`autorun/guide.rs` timeout notes ~360-383) to say discovery fills a missing wait timeout with 10 s but a saved script still needs it.
- [ ] **Step 4: Run** `cargo test --test suite action_help:: tcm_mcp:: autorun_guide::` (one at a time per module if the filter takes one), then the modules touching discovery (`grep -l discover src-tauri/tests/suite/*.rs` to find them). Expect PASS.
- [ ] **Step 5: Commit** `fix(v2): a refused Auto Run action names the field it expected and shows an example; discovery waits have a default and a ceiling`.

### Task 2: The menu-shape sentence and refused-session logging

**Files:**
- Modify: `src-tauri/src/autorun/nav.rs` (`PathFailure`, `UNREACHED_PREFIX` ~657, `for_run` ~722), callers that know the account: `autorun/replay.rs` ~88, `autorun/runner.rs` ~2004/~2013, `ai_bridge.rs` ~7683; `autorun/runner.rs` ~1712-1738 and `autorun/signin.rs` ~157/~385 for logging
- Test: `src-tauri/tests/suite/` - the existing nav test module (find it by `PathFailure` / `UNREACHED_PREFIX` in tests) and the sign-in module

**Interfaces (produces):**
- `pub fn menu_differs(account: &str, locator: &str) -> String` in `nav.rs`, giving exactly: `the menu as <account> has no <locator> - this account's menu differs from the one the area was recorded with; script this area as an account whose menu has it`.
- A failed area walk whose failing click never found its target (not the ambiguous-match case that appends `AREA_PATH_NOT_EXACT`), with an account key known at the caller, has `menu_differs(..)` appended to the run sentence after the existing reason. Without an account key, unchanged.
- Refused-session applog lines (both sites) also say which browser (the slot's label, "the main Auto Run browser" / "Assistant N's browser" as used elsewhere, or "an unattended run's browser"), the refusal kind (`SessionRefusal` variant in words: "the session was tampered with" / "an empty Bad Request"), and the saved session's age in minutes when known. Pass what is needed down as a small `&str`/struct argument; no secrets, cookies or URLs.

- [ ] **Step 1: Tests.** `menu_differs("gabriel", "button \"Self Service\"")` is the exact sentence; a `PathFailure` for a never-found click plus an account gives the run sentence ending with it; the `AREA_PATH_NOT_EXACT` case does not include it; the refused-session log line contains the browser words, the kind words and `min` (use the applog tail helper the sign-in tests use, under the `serial.rs` lock).
- [ ] **Step 2: Run** the touched modules; expect FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the nav, replay, runner and sign-in test modules; expect PASS.
- [ ] **Step 5: Commit** `fix(v2): a failed area walk says when the account's menu differs; a refused session's log says which browser and why`.

### Task 3: Discovery starts at its area, with the cookie banner closed

**Files:**
- Modify: `src-tauri/src/ai_bridge.rs` (`discover_start_in` ~5009, `autorun_discover_start` ~7921), `src-tauri/src/autorun/popups.rs`, `src-tauri/src/mcp.rs` (`start_autorun_discovery` description, which today says it goes home), `src-tauri/src/autorun/guide.rs` (the discovery start text)
- Test: the discovery start test module(s) and a popups test

**Interfaces (produces):**
- `pub async fn close_shown_within(driver, recipe: &SignInRecipe, within_ms: u64) -> usize` in `popups.rs`: polls (at the timing's `poll_ms`) up to `within_ms` for any `close_when_shown` entry whose `shown` selector is visible and clicks its `close` selector (never another button); returns how many it closed. Used with `within_ms = 3_000`.
- `start_autorun_discovery` with `area`:
  - If `area` names a saved area (the same lookup `discovery_area_route` / `runner.rs` ~150 uses), after the sign-in (fresh: `TripFrom::SignIn`; kept browser after `went_home`: `TripFrom::SignIn` too, since `load_home` leaves it on the starting page) walk there with `nav::reach_module_noted` exactly as a run does, so the menu ends in the state a run leaves it. The answer's `path`/`page` describe the area's page and add `"at_area": true`.
  - If the walk fails: the discovery stays started, the browser stays open where it stopped, and the answer adds `"at_area": false` and `"area_walk": "<the run sentence>"`.
  - If `area` is not a saved area: unchanged (home), with `"at_area": false` and `"area_walk": "no saved area is called <area>; the discovery starts at home and files what it sees under that name"`.
  - In every case, once landed (home or area), call `close_shown_within(.., 3_000)` before reading the page, so the first action is never covered by the banner.
- [ ] **Step 1: Tests.** With the existing fake-driver / bridge harness the discovery-start tests use: a saved area makes the start walk its clicks (assert the clicks the driver received and `at_area: true`); an unknown area gives the home landing and the exact `area_walk` sentence; a failing walk keeps the discovery state and returns `at_area: false`; `close_shown_within` clicks `#btnCookieClose` when `#cookieBanner.show` becomes visible after a delay and never clicks anything else. If the harness cannot drive one of these, say so in the report and test the pure part (lookup + answer shaping) instead.
- [ ] **Step 2: Run** those modules; expect FAIL.
- [ ] **Step 3: Implement**, and update the mcp.rs description and guide text (discovery naming a saved area starts there, banner closed).
- [ ] **Step 4: Run** the discovery, popups, `tcm_mcp::` and `autorun_guide::` modules; expect PASS.
- [ ] **Step 5: Commit** `feat(v2): a discovery naming a saved area starts there, with the cookie banner already closed`.

### Task 4: Report files for subagents, and splitting cases between writers

**Files:**
- Modify: `src-tauri/src/ai_tools.rs` (`begin-test-case-writing` ~324-330, `gen-tests` ~656-719), `src-tauri/src/autorun/guide.rs` (Subagents ~1972), `src-tauri/src/autorun/roles/writer.md`
- Test: `src-tauri/tests/suite/ai_tools.rs`, `src-tauri/tests/suite/autorun_guide.rs`

**Produces (text, exact sentences where quoted):**
- Everywhere a TCM text tells the assistant to start subagents (`begin-test-case-writing` slices, the guide's Subagents paragraph, `gen-tests`' new section): `Tell each subagent to write its report to a file of its own (<name>-report.md, beside its output) before it hands back. If a hand-back fails, read that file instead.`
- The writer role (`roles/writer.md`): `Before you hand back, write your report to the file the coordinator named (or <your agent name>-report.md); a hand-back can be refused, the file cannot.`
- `gen-tests` gains a section **Several writers** (only when the person asks for more than one writer, or more than one account is free and the cases need different accounts):
  1. Call `get_accounts` and keep the accounts no other assistant holds.
  2. Find, with one `db_query` (read the module's quirks and areas first for its table names), what each free account can do in the cases' module: its role in each record the cases need (for example which cycles it is in as Self, manager or reviewer). Show it as one table: account, roles, records.
  3. Split the cases so each writer gets cases its account can do; list every case held back and why (no free account has the role it needs).
  4. Start one writer per account, each named (`writer-1`...), told its account, its cases and its report file; every Auto Run call passes its name as `agent`.
  5. When all are done, read every report file (also when a hand-back failed) and summarise.
- [ ] **Step 1: Tests.** `ai_tools.rs`: `begin-test-case-writing` and `gen-tests` texts contain the report-file sentence; `gen-tests` contains `Several writers`, `get_accounts`, `db_query` and `held back`; the writer agent file contains the writer sentence. `autorun_guide.rs`: the Subagents paragraph contains the report-file sentence. Keep every existing text test passing (some assert exact paragraphs; update them only where the new sentence is inserted).
- [ ] **Step 2: Run** `cargo test --test suite ai_tools::` and `autorun_guide::`; expect FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same modules; expect PASS.
- [ ] **Step 5: Commit** `feat(v2): subagents leave their report in a file, and a several-writer run splits cases by what each account can do`.

### Task 5: Whole-suite gate

- [ ] `cd src-tauri && cargo test --tests` (whole Rust suite), then `npm test`, then `npx tsc --noEmit`, one at a time, all green. Fix anything red at its source.
