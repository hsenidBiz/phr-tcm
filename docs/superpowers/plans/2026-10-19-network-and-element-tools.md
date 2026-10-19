# Network, Cookie And Element Tools Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** the assistant can read the supervised browser's captured requests and responses (and export/import them), set test headers and cookies under guards, and inspect an element's styles, listeners, animations and structure; scripts gain `expect_style`.

**Architecture:** a per-browser capture store fed by the existing CDP event routing (beside `net_record`), with a capture-specific redaction; MCP tools in the Auto Run group read it and act on the browser through raw CDP; overrides are recorded on `Session` and named in every answer until reset; element inspection runs page JS plus `DOMDebugger.getEventListeners`.

**Tech Stack:** Rust (Tauri 2, the CDP driver in `src-tauri/src/browser`), MCP tools via `mcp.rs` + `ai_bridge.rs`, vitest for the tool list.

**Spec:** `docs/superpowers/specs/2026-10-19-network-and-element-tools-design.md` (read its "Decisions after the builder check").

## Global Constraints

- No secret shown, logged or written: passwords, tokens, session values, `cookie`/`set-cookie`/`authorization`/`proxy-authorization` headers, sign-in cookie values (spec decisions 2, 3).
- TCM sends no request of its own to the application, no DELETE; a capture is never replayed into the browser.
- `set_autorun_headers` uses `Network.setExtraHTTPHeaders` only - never `Fetch` (the save guard's interceptor).
- Acting tools refuse (409) in the person's browser or during a watched run (spec decision 4).
- Budgets: 1000 requests; response body ≤ 2 MB each, 32 MB per browser, oldest bodies first; request body ≤ 256 KB.
- Raw CDP cookie commands (`Network.getCookies`/`getAllCookies`/`setCookie`/`deleteCookies`), not typed parsers.
- New tools registered everywhere `record_validation_rule` is (mcp.rs tool + routing, bridge route table, `ai_tools.rs` `AUTORUN_TOOLS`, `src/lib/mcpTools.ts`, `tcm_mcp.rs` lists, `src/lib/mcpTools.test.ts`).
- Rust tests only in `src-tauri/tests/suite/`; one test run at a time; never `CARGO_TARGET_DIR`.

## Review Focus

1. The sign-in POST (`UserName=a&Password=hunter2`) captured → listing, body read and HAR export never contain `hunter2`. Test in Task 2.
2. A long discovery producing 1500 requests with 50 MB of JSON → memory stays within the budgets; oldest bodies dropped, requests listed. Test in Task 1.
3. `set_autorun_cookie` on `.ASPXAUTH` or an HttpOnly cookie → refused; on `culture` → set. Test in Task 4.
4. Headers set, then a blocked save → still blocked (no `Fetch` change). Test in Task 4.
5. A replay that passes with overrides set → its answer says so; after `reset_autorun_browser` the overrides are gone and the answer is plain. Test in Task 4.

---

### Task 1: The capture store

**Files:** Create `src-tauri/src/browser/net_capture.rs` (+ `pub mod net_capture;` in `browser/mod.rs`); modify `src-tauri/src/browser/cdp.rs` (route `Network.*` events to it beside `net_record.observe`, per tab and for new tabs; read bodies at `loadingFinished`); tests `src-tauri/tests/suite/net_capture.rs`.

**Interfaces (produces):**
- `pub struct Captured { pub id: u64, pub request_id: String, pub tab: String, pub method: String, pub url_path_query: String, pub kind: Option<String>, pub status: Option<u16>, pub mime: Option<String>, pub req_headers: Vec<(String,String)>, pub resp_headers: Vec<(String,String)>, pub req_body: Option<String>, pub resp_body: Option<Body>, pub started_ms: u64, pub duration_ms: Option<u64>, pub failed: Option<String>, pub redirect: Option<String> }` (`Body` = text or "dropped"/"not text"/"too big")
- `pub struct CaptureFilter { pub include_types: Vec<String>, pub exclude_types: Vec<String>, pub path_contains: Option<String> }`
- `NetCapture::{observe(&Event), list(offset, limit) -> (Vec<&Captured>, usize), get(id) -> Option<&Captured>, set_filter, filter, clear}`; on the driver: `capture(&self) -> Option<&NetCapture>` / `capture_mut`.

Rules: same-site XHR/Fetch/Document, not polls (reuse `NetRecord`'s poll mark); allow-listed headers only (spec Part 1); request body from `postData` or `Network.getRequestPostData` when `hasPostData` and omitted; response body via `Network.getResponseBody` at `loadingFinished` for text-like mimes (spec list) within the budgets. The body read is a CDP call: queue the finished ids and read them where the driver already awaits events (no new background task); a body gone by then is marked "dropped".

- [ ] Tests first: `a_same_site_xhr_is_kept_with_its_bodies`; `polls_other_sites_images_are_not_kept`; `a_redirect_keeps_its_first_hop`; `budgets_drop_oldest_bodies_and_requests` (Review Focus 2); `a_filter_decides_what_is_kept_from_now_on`; `cookie_and_authorization_headers_are_never_kept`; `a_new_tab_is_captured`. Then implement; `cd src-tauri && timeout 1800 cargo test --test suite -- net_capture:: net_record::` PASS; commit `feat(v2): capture the page's requests and responses in the supervised browser`.

### Task 2: Redaction and HAR

**Files:** Create `src-tauri/src/autorun/capture_redact.rs`; create `src-tauri/src/autorun/har.rs`; tests `capture_redact.rs`, `har.rs` in `tests/suite/`.

**Interfaces:** `pub fn redact_body(mime: Option<&str>, body: &str) -> String` (JSON secret keys via `api_checks`' redact, form-urlencoded and multipart fields, no length cut); `pub fn redact_path_query(pq: &str) -> String` (secret query params hidden); `pub fn to_har(entries: &[&Captured]) -> serde_json::Value` (HAR 1.2, redacted); `pub fn from_har(v: &Value) -> Result<Vec<Captured>, String>`.

- [ ] Tests first: `the_sign_in_password_never_shows` (Review Focus 1: form body, JSON body, multipart, query); `addresses_inside_bodies_are_kept`; `a_har_round_trips`; `a_har_from_elsewhere_with_secrets_is_redacted_on_import`. Implement; PASS; commit `feat(v2): capture redaction and HAR`.

### Task 3: Read tools

**Files:** `src-tauri/src/mcp.rs`, `src-tauri/src/ai_bridge.rs` (routes + handlers), `src-tauri/src/ai_tools.rs`, `src/lib/mcpTools.ts`; tests `tcm_mcp.rs`, `autorun_bridge.rs` (or a new `autorun_capture_tools.rs`), `src/lib/mcpTools.test.ts`.

Tools (spec Part 2 table): `list_autorun_requests {offset?, limit? (≤100), from?}`, `search_autorun_requests {path?, method?, status?, type?, request_contains?, response_contains?, offset?, limit?, from?}`, `get_autorun_request {id, from?}`, `get_autorun_response {id, from?}`, `get_autorun_response_body {id, offset?, from?}` (pages of 20 000 chars), `set_autorun_capture {include_types?, exclude_types?, path_contains?}`, `get_autorun_capture`, `export_autorun_requests {name}` (`<autorun root>/captures/<name>.har`, name validated as a file stem), `import_autorun_requests {name}` (only from that folder; loaded as set `from: name`). Every answer redacted (Task 2). No browser → 404 "No browser is open." for live reads.

- [ ] Tests first per tool (bridge level with the scripted driver), the export file holds no secret, import refuses a name outside the folder (`..`, a path), registration lists. Implement; PASS (`autorun_`, `tcm_mcp::`, vitest mcpTools); commit `feat(v2): the assistant reads, searches, exports and imports captured requests`.

### Task 4: Headers and cookies

**Files:** `src-tauri/src/commands/autorun.rs` (`Session.overrides: Overrides { headers: BTreeMap<String,String>, cookies: BTreeSet<String> }`, reset undoes them, release drops them), `src-tauri/src/ai_bridge.rs` (tools; the answer line on try/discovery/replay), `src-tauri/src/browser/cdp.rs` (`set_extra_headers` per tab and on new tabs), registration files; tests `autorun_overrides.rs` (new) + registration tests.

Tools: `set_autorun_headers {headers: {name: value|null}}`, `get_autorun_cookies {url?}`, `set_autorun_cookie {name, value, path?, expires?, secure?, same_site?}` (site = the current page's), `clear_autorun_cookies {}`. Rules: spec decisions 1, 2, 4. The answer line verbatim: `Changed for testing: header <names>; cookie <names>.`; a passing replay with overrides: `Passed with these changes in place: header <names>; cookie <names>.`

- [ ] Tests first: Review Focus 3, 4, 5; `refused_header_names` (cookie, authorization, proxy-authorization, host); `acting_tools_refuse_in_the_persons_browser`; `clear_cookies_signs_out_and_lets_the_lease_go` (account None, fingerprint None, pane event, discovery kept); `reset_removes_only_what_the_assistant_set`; `a_cookie_value_of_the_sign_in_is_hidden_when_read`. Implement; PASS (`autorun_`); commit `feat(v2): test headers and cookies, guarded and named in every answer`.

### Task 5: `inspect_autorun_element`

**Files:** `src-tauri/src/ai_bridge.rs` (tool + handler), `src-tauri/src/browser/` (an `inspect.rs` with the JS and `DOMDebugger.getEventListeners` via `DOM.resolveNode`/`Runtime` object ids), registration files; tests `autorun_inspect.rs`.

Input `{locator, area?, show?: ["styles","listeners","animations","structure"], properties?: [..]}`; the locator resolved as a try resolves it (same refusals for not found / matched many). Styles: all computed or the listed properties; listeners: type, useCapture, once, passive, handler's script location (no source text), for the element and - when another element is at its click centre - that one too, named; animations: `getAnimations()` name, duration, delay, iterations, playState, and computed `transition-*`; structure: tag, attributes (values of `value` on password inputs hidden), text (≤500 chars), child count by tag, frame. Read-only.

- [ ] Tests first with the scripted driver for each part, a covered element names its cover, a password input's value hidden. Implement; PASS; commit `feat(v2): inspect_autorun_element - styles, listeners, animations, structure`.

### Task 6: `expect_style`

**Files:** `src-tauri/src/browser/actions.rs` (`Action::ExpectStyle { target, property, equals?, contains?, not_equals? }`, executed like other expects with their wait/retry; colours normalised to `rgb()`/`rgba()`), `src-tauri/src/autorun/seen_check.rs` (target treated like a check's), `src-tauri/src/autorun/guide.rs` (vocabulary line), the script editor's action list in `src/` (find where actions are listed), bindings regenerated; tests `browser_actions.rs`, `autorun_seen_check.rs`, `autorun_guide.rs`, the editor's test.

- [ ] Tests first: equals/contains/not_equals pass and fail lines; `red` equals `rgb(255, 0, 0)`; waits for a transition to settle within the timeout; seen check accepts a seen target. Implement; `cargo test --test bindings`; PASS; commit `feat(v2): expect_style - a script checks a computed style`.

### Task 7: Guide and gate

- [ ] `src-tauri/src/autorun/guide.rs`: a short section naming each tool, "build API templates from captured requests (`get_autorun_request` / `get_autorun_response_body`)", the overrides rule ("reset or remove them before proving a fix"), what is hidden; quick rules stay ≤ 45 lines; How To Use: one sentence under Auto Run, `npm run docs:build`.
- [ ] Gate: `cd src-tauri && cargo test --tests`, then `npm test`, then `npm run build` - one at a time, all green.
- [ ] Owner hand checks: capture a real Save and build an API template from it; a feature cookie set and reset; inspect a disabled-looking button.
