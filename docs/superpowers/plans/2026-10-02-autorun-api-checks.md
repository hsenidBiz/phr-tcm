# Auto Run API Checks Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Two new Auto Run script steps - `expect_response` (check a request the page itself made during the step) and `api_request` (a GET sent from the signed-in page) - each checking status and JSON fields.

**Architecture:** A bounded network record (`browser/net_record.rs`) is fed every `Network.*` event the DevTools driver reads, beside the existing page log; the `Driver` trait exposes a sequence marker and the entries since a marker. The runner carries both new steps out itself (like `sign_in` and `upload`): it takes a marker when a script step starts, `expect_response` polls the record from that marker, and `api_request` runs a small in-page GET. Checks reuse the API templates' JSON partial match, excerpt and token scrubbing.

**Tech Stack:** Rust (Tauri 2, serde, specta), Chrome DevTools Protocol, React 19/TS (one small floor.ts change), integration tests in `src-tauri/tests/suite/`.

**Spec:** `docs/superpowers/specs/2026-10-02-autorun-api-checks-design.md`

## Global Constraints

- `expect_response`: `url_contains` required non-empty, matched case-insensitively against path + query (never the host); `method` optional; `status` default 200; `json` optional; `timeout_ms` optional, default `Timing::expect_ms` (10 s). Looks only at requests started since the current script step began; when several match, the most recent FINISHED one is checked.
- `api_request`: GET only; `path` must pass `api_templates::is_safe_relative_path`; `query` map percent-encoded the way API templates encode it; `expect.status` default 200; `expect.json` optional. Sent by the page (`fetch`, `credentials: "same-origin"`, no anti-forgery token); timeout `Timing::action_ms`.
- JSON check = the API templates' partial match (listed keys must match, nested objects recurse, arrays and scalars by equality, extra keys ignored). Bodies at most 64 KB.
- Failure detail = one sentence + (when a body exists) `excerpt(scrub_tokens(body, None))`. Stored outcomes never contain request headers, cookies, tokens, or any URL query string or host.
- Network record bounded to the last 400 requests; keeps id, method, path + query, start sequence, status, content type, finished / failed - never headers, request bodies or cookies.
- Both kinds are checks (`Action::is_check`, frontend `floor.ts`); the assertion-source rule is unchanged.
- Auto Run stays gated as today. Never name the secret unlock. No changelog, help-site or README changes.
- Rust tests only in `src-tauri/tests/suite/` (one module per file, `mod` line in `suite/main.rs`). Never hand-edit `src/bindings.ts`; regenerate with `cd src-tauri && cargo test --test bindings`. `Driver` changes must keep the existing fakes compiling (`tests/suite/common.rs` ScriptedDriver, `api_templates_runner.rs`, `autorun_recorder.rs`).
- One test command at a time. Commits via Bash heredoc ending `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Branch `feat/autorun-api-checks`.

## Review Focus

1. **A matching request that never finishes** (long-poll, hung save). Expected: at the timeout, the sentence says it had not finished, not that there was none. Pinned in Task 3.
2. **A JSON check against a response that is not JSON or whose body Chrome no longer holds** (HTML error page, evicted body). Expected: its own sentence, never a panic or a pass. Pinned in Task 3.
3. **`api_request` silently redirected to the sign-in page** (expired session answers 200 HTML). Expected: fails, naming the path it was redirected to (no host). Pinned in Task 4.
4. **Secrets in addresses** (`?access_token=...`). Expected: no query string ever appears in a stored outcome. Pinned in Tasks 3 and 4.
5. **A navigation or upload inside the same step** before `expect_response`. Expected: the record still holds the step's requests (only `forget_events` is cleared). Pinned in Task 1.

---

### Task 1: The network record

**Files:**
- Create: `src-tauri/src/browser/net_record.rs` (+ `pub mod net_record;` in `browser/mod.rs`)
- Modify: `src-tauri/src/browser/cdp.rs` (field + `on_event` feeds it; `Driver` trait methods), `src-tauri/src/commands/autorun.rs` (~100: the supervised session calls `page_log::watch`)
- Test: `src-tauri/tests/suite/net_record.rs`

**Interfaces:**
- Produces:
  - `pub enum NetState { Pending, Finished, Failed(String) }`
  - `pub struct NetEntry { pub seq: u64, pub id: String, pub method: String, pub path_query: String, pub status: Option<u16>, pub mime: Option<String>, pub state: NetState }`
  - `#[derive(Default)] pub struct NetRecord` with `pub fn observe(&mut self, ev: &Event)` (never claims the event), `pub fn mark(&self) -> u64` (the next seq), `pub fn since(&self, mark: u64) -> Vec<NetEntry>` (oldest first); `pub const MAX_REQUESTS: usize = 400`.
  - `Driver` gains default methods `fn net_mark(&self) -> u64 { 0 }` and `fn net_since(&self, _mark: u64) -> Vec<NetEntry> { Vec::new() }`; `Cdp` implements them from its record. ScriptedDriver (tests/suite/common.rs) gains an optional `NetRecord` it feeds from the events it emits, so later tasks can test against it.
- Events read: `Network.requestWillBeSent` (http(s) only; new id -> new entry with the next seq; a repeat id = redirect -> update `path_query`, keep seq), `Network.responseReceived` (`response.status`, `response.mimeType`), `Network.loadingFinished` (Finished), `Network.loadingFailed` (Failed(`errorText`)). `path_query` = the URL without scheme/host/fragment.

- [ ] **Step 1: Failing tests** in `tests/suite/net_record.rs`: `a_request_is_recorded_without_its_host`, `status_and_mime_arrive_with_the_response`, `finished_and_failed_are_recorded`, `a_redirect_keeps_its_place`, `only_the_last_400_are_kept`, `since_returns_only_newer_entries`, `non_http_requests_are_ignored`, and (Review Focus 5) `forgetting_events_keeps_the_record` - feed events through a `Cdp` over the suite's fake transport (or the ScriptedDriver), call `forget_events()`, assert `net_since(mark)` still has them.
- [ ] **Step 2: Run** `cd src-tauri && cargo test --test suite net_record::` - FAIL (module missing).
- [ ] **Step 3: Implement** the record and the `Cdp` wiring (`on_event` calls `net_record.observe(&ev)` before `page_log.observe`); call `page_log::watch` when the supervised session opens (best effort, as `RealBrowsers::open` does).
- [ ] **Step 4: Run** `cargo test --test suite net_record::` then `cargo test --test suite autorun` - PASS.
- [ ] **Step 5: Commit** `feat(v2): Auto Run browsers keep a bounded record of the page's requests`.

### Task 2: The two step kinds - shape, validation, checks, editor

**Files:**
- Modify: `src-tauri/src/browser/actions.rs` (variants, `validate`, `is_check`, `run()` arm), `src-tauri/src/autorun/guide.rs` (`ACTION_KINDS` + action docs), `src-tauri/src/ai_bridge.rs` (~1357 `describe_try`), `src-tauri/src/autorun/patterns.rs` (~303 exhaustive match), `src/screens/AutoRun/floor.ts` (count `api_request` as a check)
- Test: `src-tauri/tests/suite/autorun_actions.rs` (or the module that tests `Action` serde/validate - find it), `tests/suite/autorun_guide.rs`, `src/screens/AutoRun/floor.test.ts` (or its existing test)

**Interfaces:**
- Produces:
  - `Action::ExpectResponse { #[serde(default, skip_serializing_if = "Option::is_none")] method: Option<String>, url_contains: String, #[serde(default = "ok_status")] status: u16, #[serde(default, skip_serializing_if = "Option::is_none")] json: Option<Value>, #[serde(default, skip_serializing_if = "Option::is_none")] timeout_ms: Option<u32> }` (kind `expect_response`)
  - `Action::ApiRequest { path: String, #[serde(default, skip_serializing_if = "BTreeMap::is_empty")] query: BTreeMap<String, String>, #[serde(default)] expect: ApiExpect }` (kind `api_request`), `pub struct ApiExpect { #[serde(default = "ok_status")] pub status: u16, #[serde(default, skip_serializing_if = "Option::is_none")] pub json: Option<Value> }`
  - `Value` fields use the same specta override API templates use for `Expect::json` (`#[specta(type = ...Unknown)]`).
  - `run()` arm for both: `ActionOutcome::failed("<kind> is carried out by the runner")` (as `sign_in`), so only the runner executes them.
- Validation sentences: empty `url_contains` -> `expect_response needs url_contains`; `method` not one of GET POST PUT PATCH DELETE HEAD OPTIONS (case-insensitive) -> `expect_response method "<m>" is not an HTTP method`; unsafe path -> `api_request path "<p>" is not a safe path on this site`; status outside 100-599 -> `status <n> is not an HTTP status`.

- [ ] **Step 1: Failing tests:** serde round trip for both kinds (defaults applied: status 200, empty query omitted on write); each validation sentence; `is_check()` true for both; guide `ACTION_KINDS` contains both (existing drift test); `floor.ts` counts a step whose only action is `api_request` as checked.
- [ ] **Step 2: Run** `cargo test --test suite autorun` and `npx vitest run src/screens/AutoRun/floor` - FAIL.
- [ ] **Step 3: Implement**; add both to every exhaustive `match` on `Action`; document both in the guide's action list (one line each, the shapes from the spec) so the drift test passes.
- [ ] **Step 4: Run** the same + `cargo test --test bindings` + `npx tsc --noEmit` - PASS.
- [ ] **Step 5: Commit** `feat(v2): Auto Run scripts can declare expect_response and api_request steps`.

### Task 3: Carrying out expect_response

**Files:**
- Create: `src-tauri/src/autorun/api_checks.rs` (+ `pub mod api_checks;` in `autorun/mod.rs`)
- Modify: `src-tauri/src/autorun/runner.rs` (`run_step_routed`: `let mark = d.net_mark();` before the step's first action; intercept `ExpectResponse`), `src-tauri/src/api_templates/exec.rs` (`partial_match` -> `pub(crate)`), `src-tauri/src/autorun/patterns.rs` (`ErrorClass::Api` for the sentences below)
- Test: `src-tauri/tests/suite/autorun_api_checks.rs`

**Interfaces:**
- Consumes: Task 1 (`Driver::net_mark/net_since`, `NetEntry`, `NetState`), Task 2 (`Action::ExpectResponse`).
- Produces:
  - `pub fn pick(entries: &[NetEntry], method: Option<&str>, url_contains: &str) -> Pick` where `pub enum Pick { Finished(NetEntry), Failed(NetEntry), PendingOnly(NetEntry), None { seen: usize } }` - most recent finished match wins; else the most recent failed; else a pending match; else none (with the step's request count).
  - `pub fn judge(entry: &NetEntry, status: u16, json: Option<&Value>, body: Option<&str>) -> Result<(), String>` (pure; the sentences).
  - `pub async fn expect_response<D: Driver>(d: &mut D, a: &Action, mark: u64, timing: &Timing) -> ActionOutcome` - polls with a light call (`Runtime.evaluate` of `1`) every `timing.poll_ms` until `Pick::Finished`/`Failed` or the deadline (sets/clears `d.set_deadline` like the existing wait loops); fetches the body with `Network.getResponseBody { requestId }` only when `json` is set (decode base64 when `base64Encoded`).
- Sentences (method/path shown WITHOUT query): `no request matching "<pattern>" in <s> s (this step made <n> requests)`; `<METHOD> <path> had not finished after <s> s`; `<METHOD> <path> failed: <errorText>`; `<METHOD> <path> answered <status>, expected <expected>`; `the response to <METHOD> <path> was not JSON`; `the response body was no longer available`; field mismatch = the partial-match sentence prefixed `the response to <METHOD> <path>: `. Success detail: `<METHOD> <path> answered <status>`.

- [ ] **Step 1: Failing tests:** pure `pick` (method filter, case-insensitive pattern, host never matched, most recent finished wins, pending-only); pure `judge` (each sentence; partial match pass/fail; non-JSON body); driver-level with ScriptedDriver: a mark + emitted request/response/finished events -> pass; no events -> the "no request" sentence after a short `timeout_ms`; (Review Focus 1) a pending-only match at the timeout -> the "had not finished" sentence; (Review Focus 2) `getResponseBody` error -> "no longer available", HTML body with a JSON check -> "was not JSON"; (Review Focus 4) a request `/Save?access_token=abc` -> no outcome detail contains `access_token`; runner-level: requests made BEFORE the step's mark are ignored; `patterns::classify` puts these sentences in the Api class.
- [ ] **Step 2: Run** `cargo test --test suite autorun_api_checks::` - FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same, then `cargo test --test suite autorun` - PASS.
- [ ] **Step 5: Commit** `feat(v2): expect_response checks the request the page made during the step`.

### Task 4: Carrying out api_request

**Files:**
- Modify: `src-tauri/src/autorun/api_checks.rs` (`api_request`), `src-tauri/src/autorun/runner.rs` (intercept `ApiRequest`), `src-tauri/src/api_templates/exec.rs` (extract `pub fn encode_query(q: &BTreeMap<String, String>) -> String` from `build_request` and use it there - one implementation)
- Test: `src-tauri/tests/suite/autorun_api_checks.rs`

**Interfaces:**
- Consumes: Task 2 (`Action::ApiRequest`, `ApiExpect`), Task 3 (`judge`-style sentences; reuse the same status/JSON checking code - do not duplicate it).
- Produces:
  - `pub const GET_FN: &str` - an in-page `async function (url, limitMs)` doing `fetch(url, { credentials: "same-origin", headers: { Accept: "application/json" }, signal })` and returning `{ status, contentType, finalPath, redirected, text }` (text capped at 64 KB; `finalPath` = path + query of `response.url`) or `{ error }`.
  - `pub async fn api_request<D: Driver>(d: &mut D, a: &Action, timing: &Timing) -> ActionOutcome` - re-checks the path, builds `path + "?" + encode_query(query)`, calls `GET_FN` on a fresh `page::document(d)` handle with `timing.action_ms`.
- Sentences: `GET <path> failed: <error>`; (Review Focus 3) `GET <path> was redirected to <finalPath without query>`; then the Task 3 status/JSON sentences with method `GET`.

- [ ] **Step 1: Failing tests:** `encode_query` matches what `build_request` produced before (an existing API-templates test keeps passing, plus one direct test); with ScriptedDriver answering the `Runtime.callFunctionOn` for `GET_FN`: 200 + matching JSON -> pass; wrong status; field mismatch; not JSON; `{ error: "timeout" }`; `redirected: true` to `/Account/Login?ReturnUrl=...` -> the redirect sentence with no query; an unsafe path is refused before any call; (Review Focus 4) no query string in any outcome.
- [ ] **Step 2: Run** `cargo test --test suite autorun_api_checks::` - FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same, `cargo test --test suite api_template`, `cargo test --test suite autorun` - PASS.
- [ ] **Step 5: Commit** `feat(v2): api_request reads the site's API from the signed-in page`.

### Task 5: Guide, assistant reference, live check

**Files:**
- Modify: `src-tauri/src/autorun/guide.rs` (a "Checking the API" section), any assistant-facing tool description that lists action kinds (find it in `src-tauri/src/mcp.rs`)
- Test: `src-tauri/tests/suite/autorun_guide.rs`, `src-tauri/tests/suite/browser_live.rs` (the existing ignored real-browser module)

- [ ] **Step 1: Failing tests:** the guide contains `## Checking the API`, both kinds with their limits (GET only for `api_request`; requests since the step started for `expect_response`; status default 200; partial JSON match), the instruction to look up real endpoints, paths and response fields with `list_api_templates` (reference only, never to run them from a script), and that what is checked must come from the case's expected result. An ignored live test (`#[ignore]`, like the others in `browser_live.rs`) serving a local page that `fetch`es a JSON endpoint on click: `expect_response` passes and a wrong status fails; `api_request` reads the endpoint.
- [ ] **Step 2: Run** `cargo test --test suite autorun_guide::` - FAIL.
- [ ] **Step 3: Implement** the guide text (plain sentences, no em dashes) and the live test.
- [ ] **Step 4: Run** `cargo test --test suite autorun_guide::`, then the live test once with `--ignored`, then `npm test` and `cd src-tauri && cargo test --tests` once each - PASS.
- [ ] **Step 5: Commit** `feat(v2): the Auto Run guide explains API checks and points at proven templates`.
