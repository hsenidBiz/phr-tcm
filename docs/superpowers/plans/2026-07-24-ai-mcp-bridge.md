# AI MCP Bridge Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let AI tools (Claude Code, Cursor, Copilot) connect to the running app over MCP and get a live writing guide, real example test cases, validation against the actual importer, and PBI search — so they write import-ready test cases in the org's house style.

**Architecture:** Three layers on one spine: (1) an in-app localhost service (`ai_bridge.rs`, modeled on `note_server.rs` but async) exposing token-guarded read/validate routes that reuse the app's existing auth + ADO client; (2) a tiny `tcm-mcp.exe` stdio binary (second cargo bin target, no new deps) that speaks newline-delimited MCP JSON-RPC and proxies each tool call to the service via a handshake file in the OS temp dir; (3) a Settings section showing bridge status and the copy-able registration command. The app must be running for the bridge to work — that is the accepted trade-off for zero extra sign-in.

**Tech Stack:** Rust (tokio + reqwest + rand, all already in-tree), hand-rolled minimal HTTP/1.1 and JSON-RPC (no new crates), React/TS Settings UI, wiremock + vitest tests.

**Design source:** the conversation design (2026-07-24); supersedes `docs/superpowers/specs/2026-07-20-ai-guide-generator-design.md` as the AI-guide redesign.

## Global Constraints

- All work under `v2/` on `master`; commit per task with Bash heredoc (`git commit -F - <<'EOF' … EOF`), confirm with `git log -1`.
- NO new crate dependencies. NO DELETE calls; the bridge is read + validate only — it must never expose the bearer token, raw ADO passthrough, or any write operation.
- The service binds `127.0.0.1:0` (ephemeral port) ONLY. Every route except none requires header `x-bridge-token` matching the per-launch token; wrong/missing token → 401 with empty body.
- Handshake file: `std::env::temp_dir().join("tcm-v2-mcp-bridge.json")` containing exactly `{"port":<u16>,"token":"<32 hex>"}` — both sides hard-code this path.
- Never hand-edit `v2/src/bindings.ts` — cargo test regenerates it (tests/bindings.rs export).
- If `cargo test` fails with "Access is denied (os error 5)", the dev app is running: use `CARGO_TARGET_DIR=target/gate cargo test` (do NOT use %TEMP% — Defender quarantines build scripts there).
- Gates before each commit: `CARGO_TARGET_DIR=target/gate cargo test` (or plain if no lock) and, for frontend tasks, `npm test` — **check EXIT CODES** (`echo exit=$?`), not grep-filtered lines; vitest can pass all assertions and still fail on an unhandled error.
- jsdom mocks of Tauri APIs must return Promises when the component chains `.then/.catch`.
- MCP protocol level: respond to `initialize` echoing the client's `protocolVersion` (fallback `"2024-11-05"`), advertise only `tools`; transport is newline-delimited JSON-RPC 2.0 on stdio.

---

### Task 1: Bridge router core — context, auth guard, /ping, /validate

**Files:**
- Create: `v2/src-tauri/src/ai_bridge.rs`
- Modify: `v2/src-tauri/src/lib.rs` (add `pub mod ai_bridge;` alphabetically, right after `pub mod ado_testplan;`)
- Test: `v2/src-tauri/tests/ai_bridge.rs`

**Interfaces:**
- Produces (consumed by Tasks 2–3): `BridgeContext { org: String, project: String, module_ref: Option<String>, preconditions_ref: Option<String> }`; `pub async fn route(ctx: &BridgeContext, client: Option<&crate::ado::AdoClient>, method: &str, target: &str, body: &str) -> (u16, String)` where `target` is path+query (e.g. `/examples?pbi=42&limit=3`); `pub fn new_token() -> String` (32 hex chars).
- Consumes: `crate::import_parser::parse_file(path) -> Result<(Vec<TestCase>, Vec<String>), String>` (existing).

- [ ] **Step 1: Write the failing tests**

```rust
// v2/src-tauri/tests/ai_bridge.rs
//! The bridge's router, tested without sockets: routing, validation, and
//! the ADO-backed routes (Task 2) via wiremock. The TCP loop (Task 3) is
//! deliberately thin - everything interesting lives in `route`.

use v2_lib::ai_bridge::{new_token, route, BridgeContext};

fn ctx() -> BridgeContext {
    BridgeContext {
        org: "acme".into(),
        project: "Web".into(),
        module_ref: Some("Custom.Module".into()),
        preconditions_ref: Some("Custom.Preconditions".into()),
    }
}

#[test]
fn tokens_are_32_hex_and_unique() {
    let a = new_token();
    let b = new_token();
    assert_eq!(a.len(), 32);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    assert_ne!(a, b);
}

#[tokio::test]
async fn ping_answers_without_a_client() {
    let (status, body) = route(&ctx(), None, "GET", "/ping", "").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["app"], "tcm");
    assert_eq!(v["org"], "acme");
}

#[tokio::test]
async fn unknown_routes_404() {
    let (status, _) = route(&ctx(), None, "GET", "/secrets", "").await;
    assert_eq!(status, 404);
    let (status, _) = route(&ctx(), None, "DELETE", "/ping", "").await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn validate_runs_the_real_importer() {
    let good = r#"[{"title": "Login works", "steps": [{"action": "Open", "expected": "Shown"}]}]"#;
    let (status, body) = route(&ctx(), None, "POST", "/validate", good).await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["cases"], 1);
    assert_eq!(v["warnings"].as_array().unwrap().len(), 0);

    // Not-JSON is a hard importer error; an empty/foreign wrapper is a
    // lenient zero-case parse - both must be visible to the AI, never a
    // silent success with cases > 0.
    let (status, body) = route(&ctx(), None, "POST", "/validate", "not json at all").await;
    assert_eq!(status, 200); // validation RESULTS are a 200; only transport errors aren't
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v["error"].as_str().is_some(), "hard parse failure must carry an error");

    let (_, body) = route(&ctx(), None, "POST", "/validate", r#"{"not": "a wrapper"}"#).await;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["cases"], 0, "foreign objects must never count as cases");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cd v2/src-tauri && CARGO_TARGET_DIR=target/gate cargo test --test ai_bridge`
Expected: compile FAIL (`ai_bridge` module not found).

- [ ] **Step 3: Implement the module**

```rust
// v2/src-tauri/src/ai_bridge.rs
//! The AI bridge: a 127.0.0.1-only service that lets the tcm-mcp stdio
//! binary (and therefore AI tools) read a live writing guide, fetch real
//! example cases, validate drafts against the REAL importer, and search
//! PBIs. Read + validate ONLY - no writes, no raw ADO passthrough, and
//! the bearer token never leaves the app. Modeled on note_server.rs; the
//! TCP loop is thin, all logic lives in `route` so tests need no sockets.

use rand::Rng;
use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
pub struct BridgeContext {
    pub org: String,
    pub project: String,
    pub module_ref: Option<String>,
    pub preconditions_ref: Option<String>,
}

/// Per-launch shared secret for the handshake file (32 hex chars).
pub fn new_token() -> String {
    let mut rng = rand::rng();
    (0..32)
        .map(|_| char::from_digit(rng.random_range(0..16), 16).unwrap())
        .collect()
}

/// Query-string value by key from "a=1&b=2" (no percent-decoding beyond
/// what the tiny value space needs: %20 and '+' become spaces, %5C -> \).
fn q(target: &str, key: &str) -> Option<String> {
    let qs = target.split_once('?')?.1;
    for pair in qs.split('&') {
        let (k, v) = pair.split_once('=')?;
        if k == key {
            return Some(v.replace('+', " ").replace("%20", " ").replace("%5C", "\\"));
        }
    }
    None
}

/// Dispatch one request. `client` is None only in tests and before sign-in;
/// ADO-backed routes answer 503 without it so the MCP side can say
/// "sign in to Test Case Manager first".
pub async fn route(
    ctx: &BridgeContext,
    client: Option<&crate::ado::AdoClient>,
    method: &str,
    target: &str,
    body: &str,
) -> (u16, String) {
    let path = target.split_once('?').map(|(p, _)| p).unwrap_or(target);
    match (method, path) {
        ("GET", "/ping") => (
            200,
            serde_json::json!({
                "app": "tcm",
                "version": env!("CARGO_PKG_VERSION"),
                "org": ctx.org,
                "project": ctx.project,
            })
            .to_string(),
        ),
        ("POST", "/validate") => (200, validate_json(body)),
        ("GET", "/guide") => match client {
            Some(c) => (200, guide(ctx, c).await),
            None => (503, "sign in to Test Case Manager first".into()),
        },
        ("GET", "/examples") => match client {
            Some(c) => examples(ctx, c, target).await,
            None => (503, "sign in to Test Case Manager first".into()),
        },
        ("GET", "/search-pbis") => match client {
            Some(c) => search_pbis(ctx, c, target).await,
            None => (503, "sign in to Test Case Manager first".into()),
        },
        _ => (404, String::new()),
    }
}

/// Run the REAL importer on the draft: write to a temp file (parse_file
/// dispatches on extension) and report cases/warnings/error.
fn validate_json(body: &str) -> String {
    let dir = std::env::temp_dir().join("tcm-v2-bridge");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("validate-{}.json", std::process::id()));
    if std::fs::write(&path, body).is_err() {
        return serde_json::json!({"error": "could not stage the draft"}).to_string();
    }
    let out = match crate::import_parser::parse_file(path.to_str().unwrap_or_default()) {
        Ok((cases, warnings)) => serde_json::json!({
            "cases": cases.len(),
            "warnings": warnings,
            "error": serde_json::Value::Null,
        }),
        Err(e) => serde_json::json!({"cases": 0, "warnings": [], "error": e}),
    };
    let _ = std::fs::remove_file(&path);
    out.to_string()
}

// guide / examples / search_pbis are implemented in Task 2. Until then,
// compile stubs keep Task 1 green:
async fn guide(_ctx: &BridgeContext, _c: &crate::ado::AdoClient) -> String {
    String::new()
}
async fn examples(
    _ctx: &BridgeContext,
    _c: &crate::ado::AdoClient,
    _target: &str,
) -> (u16, String) {
    (404, String::new())
}
async fn search_pbis(
    _ctx: &BridgeContext,
    _c: &crate::ado::AdoClient,
    _target: &str,
) -> (u16, String) {
    (404, String::new())
}

// `q` is used by Task 2's routes; referenced here so Task 1 compiles
// without dead-code warnings.
#[allow(dead_code)]
fn _keep(target: &str) -> Option<String> {
    q(target, "pbi")
}
```

Add `pub mod ai_bridge;` to `v2/src-tauri/src/lib.rs` after `pub mod ado_testplan;`.

- [ ] **Step 4: Run to verify pass**

Run: `cd v2/src-tauri && CARGO_TARGET_DIR=target/gate cargo test --test ai_bridge`
Expected: 4 passed.

- [ ] **Step 5: Full gate + commit**

Run: `CARGO_TARGET_DIR=target/gate cargo test` — all suites pass.

```bash
git add v2/src-tauri/src/ai_bridge.rs v2/src-tauri/src/lib.rs v2/src-tauri/tests/ai_bridge.rs
git commit -F - <<'EOF'
feat(v2): AI bridge router core - ping + validate against the real importer

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 2: ADO-backed routes — /guide, /examples, /search-pbis

**Files:**
- Modify: `v2/src-tauri/src/ai_bridge.rs` (replace the three stubs)
- Test: `v2/src-tauri/tests/ai_bridge.rs` (append)

**Interfaces:**
- Consumes (all existing on `AdoClient`): `get_field_allowed_values(org, project, "Test Case", field_ref)` + `field_values_in_use(org, project, field_ref)` (the same pair `commands::cases::test_case_field_values` uses — copy that fallback logic); `pbi_test_cases_full` equivalent client method — find it with `grep -n "pub async fn" v2/src-tauri/src/ado/endpoints.rs | grep -i "test_cases\|full"` and use the REAL name (the command in `commands/cases.rs` shows the exact call); `search_pbis(org, project, query)` client method (used by `commands::discovery::search_pbis` — check the real name the same way).
- Produces: `/guide` → markdown string; `/examples?pbi=<id>&limit=<n>` → `{"test_cases":[...]}` in the import JSON record shape (`id`, `title`, `steps[{action,expected}]`, `tags`, `automation_status`, `module`, `preconditions`); `/search-pbis?q=<text>` → `{"pbis":[{"id","title","work_item_type"}]}`.

- [ ] **Step 1: Append the failing tests**

```rust
use v2_lib::ado::AdoClient;
use wiremock::matchers::{method as wm_method, path as wm_path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Wiremock host standing in for ADO; the routes hit the same endpoints
/// the app's own screens use.
async fn ado_stub() -> (MockServer, AdoClient) {
    let server = MockServer::start().await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    (server, client)
}

#[tokio::test]
async fn guide_carries_format_rules_and_live_modules() {
    let (server, client) = ado_stub().await;
    // Module picklist: allowedValues empty -> falls back to values-in-use,
    // exactly like the app's own module picker.
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Web/_apis/wit/workitemtypes/Test Case/fields/Custom.Module"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "allowedValues": ["Login", "Payroll"]
        })))
        .mount(&server)
        .await;

    let (status, body) = route(&ctx(), Some(&client), "GET", "/guide", "").await;
    assert_eq!(status, 200);
    assert!(body.contains("Not Automated"), "statuses come from VALID_STATUSES");
    assert!(body.contains("Planned"));
    assert!(body.contains("semicolon"), "tag separator rule");
    assert!(body.contains("Login") && body.contains("Payroll"), "live modules");
    assert!(body.contains("validate_cases"), "guide tells the AI to validate");
}

#[tokio::test]
async fn examples_return_real_cases_in_import_shape() {
    let (server, client) = ado_stub().await;
    // The same two calls the runner/edit screens make: ids-for-PBI, then
    // batch details. Match loosely on path; the client's own tests pin the
    // exact query strings.
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/_apis/wit/workitems/42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 42,
            "relations": [
                {"rel": "Microsoft.VSTS.Common.TestedBy-Forward",
                 "url": format!("{}/acme/Web/_apis/wit/workitems/201", server.uri())}
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/_apis/wit/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{
                "id": 201,
                "fields": {
                    "System.Title": "Login - valid credentials",
                    "System.Tags": "smoke",
                    "Microsoft.VSTS.TCM.AutomationStatus": "Planned",
                    "Custom.Module": "Login",
                    "Custom.Preconditions": "Account exists",
                    "Microsoft.VSTS.TCM.Steps": "<steps id=\"0\" last=\"2\"><step id=\"2\" type=\"ActionStep\"><parameterizedString isformatted=\"true\">Open page</parameterizedString><parameterizedString isformatted=\"true\">Shown</parameterizedString><description/></step></steps>"
                }
            }]
        })))
        .mount(&server)
        .await;

    let (status, body) =
        route(&ctx(), Some(&client), "GET", "/examples?pbi=42&limit=5", "").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let cases = v["test_cases"].as_array().unwrap();
    assert_eq!(cases.len(), 1);
    assert_eq!(cases[0]["id"], 201);
    assert_eq!(cases[0]["title"], "Login - valid credentials");
    assert_eq!(cases[0]["module"], "Login");
    assert_eq!(cases[0]["steps"][0]["action"], "Open page");
}

#[tokio::test]
async fn examples_without_pbi_400_with_guidance() {
    let (_server, client) = ado_stub().await;
    let (status, body) = route(&ctx(), Some(&client), "GET", "/examples", "").await;
    assert_eq!(status, 400);
    assert!(body.contains("pbi"));
}
```

NOTE for the implementer: before finalizing the `examples_return_real_cases_in_import_shape` mocks, run the app's OWN client method for fetch-cases-for-PBI under wiremock once (see how `v2/src-tauri/tests/` mocks it if a test already exists — `grep -rn "pbi_test_cases_full\|TestedBy" v2/src-tauri/tests/`) and shape the mock endpoints to whatever the real method actually requests. The assertions above are the contract; the mock plumbing must match the client.

- [ ] **Step 2: Run to verify failure**

Run: `cd v2/src-tauri && CARGO_TARGET_DIR=target/gate cargo test --test ai_bridge`
Expected: the three new tests FAIL (stubs return 404/empty).

- [ ] **Step 3: Replace the stubs**

```rust
/// Live writing guide: format rules from the importer's own constants +
/// the org's Module values, fetched fresh (no snapshot staleness).
async fn guide(ctx: &BridgeContext, client: &crate::ado::AdoClient) -> String {
    let statuses = crate::model::VALID_STATUSES
        .iter()
        .map(|v| format!("\"{v}\""))
        .collect::<Vec<_>>()
        .join(" or ");
    let modules = match &ctx.module_ref {
        Some(fref) => {
            let picklist = client
                .get_field_allowed_values(&ctx.org, &ctx.project, "Test Case", fref)
                .await
                .unwrap_or_default();
            if picklist.is_empty() {
                client
                    .field_values_in_use(&ctx.org, &ctx.project, fref)
                    .await
                    .unwrap_or_default()
            } else {
                picklist
            }
        }
        None => vec![],
    };
    let module_lines = if modules.is_empty() {
        "Module values could not be discovered - ask the developer.".to_string()
    } else {
        modules.iter().map(|m| format!("- `{m}`")).collect::<Vec<_>>().join("\n")
    };
    format!(
        "# Writing test cases for Test Case Manager ({org}/{project})\n\n\
        Produce a JSON array of test cases. The developer imports it via the\n\
        Import File tab, reviews, then creates - you never write to Azure DevOps.\n\n\
        ## Format\n\
        Each case: `title` (required, <=255 chars), `steps` (required, each\n\
        `{{\"action\", \"expected\"}}`), `tags` (SEMICOLON-separated, never commas),\n\
        `automation_status` (exactly {statuses}), `module` (ONLY from the list\n\
        below), `preconditions` (state, not steps). Include `id` ONLY to update\n\
        that exact work item; omit it to create.\n\n\
        ## Allowed Module values (live)\n{module_lines}\n\n\
        ## Workflow\n\
        1. Call `get_example_cases` for the PBI you're writing for and mimic\n\
        their style and granularity.\n\
        2. Draft your cases.\n\
        3. Call `validate_cases` with the JSON and fix every warning before\n\
        handing the file to the developer.\n",
        org = ctx.org,
        project = ctx.project,
    )
}

/// Real cases for a PBI, serialized in the import JSON record shape so
/// they double as format demonstrations.
async fn examples(
    ctx: &BridgeContext,
    client: &crate::ado::AdoClient,
    target: &str,
) -> (u16, String) {
    let Some(pbi) = q(target, "pbi").and_then(|v| v.parse::<i32>().ok()) else {
        return (400, "pass ?pbi=<work item id> (find one with search_pbis)".into());
    };
    let limit = q(target, "limit")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(5)
        .min(20);
    match client
        .pbi_test_cases_full(
            &ctx.org,
            pbi,
            ctx.module_ref.as_deref(),
            ctx.preconditions_ref.as_deref(),
        )
        .await
    {
        Ok(cases) => {
            let records: Vec<serde_json::Value> = cases
                .iter()
                .take(limit)
                .map(|c| {
                    serde_json::json!({
                        "id": c.id,
                        "title": c.title,
                        "tags": c.tags,
                        "automation_status": c.automation_status,
                        "module": c.module_value,
                        "preconditions": c.preconditions,
                        "steps": c.steps.iter().map(|s| serde_json::json!({
                            "action": s.action, "expected": s.expected
                        })).collect::<Vec<_>>(),
                    })
                })
                .collect();
            (200, serde_json::json!({ "test_cases": records }).to_string())
        }
        Err(e) => (502, format!("Azure DevOps error: {e:?}")),
    }
}

/// PBI search so the AI can anchor examples/output to the right item.
async fn search_pbis(
    ctx: &BridgeContext,
    client: &crate::ado::AdoClient,
    target: &str,
) -> (u16, String) {
    let Some(query) = q(target, "q").filter(|s| !s.trim().is_empty()) else {
        return (400, "pass ?q=<search text>".into());
    };
    match client.search_pbis(&ctx.org, &ctx.project, &query).await {
        Ok(hits) => (
            200,
            serde_json::json!({
                "pbis": hits.iter().map(|h| serde_json::json!({
                    "id": h.id, "title": h.title, "work_item_type": h.work_item_type
                })).collect::<Vec<_>>()
            })
            .to_string(),
        ),
        Err(e) => (502, format!("Azure DevOps error: {e:?}")),
    }
}
```

Remove the `_keep` helper (q is now genuinely used). IMPORTANT: `pbi_test_cases_full` and `search_pbis` above use the client-method names implied by the commands in `commands/cases.rs` / `commands/discovery.rs` — open those two files first and use the EXACT method names and signatures they call (adjust argument lists if the real methods differ; the commands are one-line wrappers so this is a direct read).

- [ ] **Step 4: Run to verify pass**

Run: `cd v2/src-tauri && CARGO_TARGET_DIR=target/gate cargo test --test ai_bridge`
Expected: 7 passed.

- [ ] **Step 5: Commit**

```bash
git add v2/src-tauri/src/ai_bridge.rs v2/src-tauri/tests/ai_bridge.rs
git commit -F - <<'EOF'
feat(v2): AI bridge serves live guide, real example cases, and PBI search

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 3: TCP server + handshake file + Tauri commands

**Files:**
- Modify: `v2/src-tauri/src/ai_bridge.rs` (append server section)
- Create: `v2/src-tauri/src/commands/ai_bridge.rs`
- Modify: `v2/src-tauri/src/commands/mod.rs` (add `pub mod ai_bridge;` first, before `pub mod auth;`)
- Modify: `v2/src-tauri/src/lib.rs` (register `ai_bridge::bridge_status` and `ai_bridge::set_bridge_context` at the END of `collect_commands![]`, after `discovery::list_iterations`; add `ai_bridge` to the `use commands::{...}` list)
- Test: `v2/src-tauri/tests/ai_bridge.rs` (append an end-to-end socket test)

**Interfaces:**
- Produces: commands `bridge_status() -> BridgeStatus { port: u16, mcp_exe: String }` (starts the server on first call, idempotent) and `set_bridge_context(org, project, module_ref, preconditions_ref) -> ()`; server function `pub async fn serve(state: SharedBridge, get_client: impl Fn() -> ... )` — see code; handshake file at `temp_dir()/tcm-v2-mcp-bridge.json` with `{"port","token"}`.
- Consumes: `route`, `new_token`, `BridgeContext` (Tasks 1–2); `crate::state::get_fresh_token(app)` — note it is `pub(crate)`, callable from the commands module.

- [ ] **Step 1: Append the failing end-to-end test**

```rust
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn tcp_server_guards_with_token_and_serves_ping() {
    let shared = v2_lib::ai_bridge::SharedBridge::new(ctx());
    let (port, token) = v2_lib::ai_bridge::start_listener(Arc::clone(&shared), None)
        .await
        .unwrap();

    async fn send(port: u16, req: String) -> String {
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(req.as_bytes()).await.unwrap();
        let mut buf = Vec::new();
        s.read_to_end(&mut buf).await.unwrap();
        String::from_utf8_lossy(&buf).to_string()
    }

    // Wrong token -> 401, no body.
    let resp = send(
        port,
        "GET /ping HTTP/1.1\r\nHost: x\r\nx-bridge-token: wrong\r\nConnection: close\r\n\r\n".into(),
    )
    .await;
    assert!(resp.starts_with("HTTP/1.1 401"), "got: {resp}");
    assert!(!resp.contains("tcm"));

    // Right token -> 200 with the ping payload.
    let resp = send(
        port,
        format!("GET /ping HTTP/1.1\r\nHost: x\r\nx-bridge-token: {token}\r\nConnection: close\r\n\r\n"),
    )
    .await;
    assert!(resp.starts_with("HTTP/1.1 200"), "got: {resp}");
    assert!(resp.contains("\"app\":\"tcm\""));

    // The handshake file exists and matches.
    let hs: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(std::env::temp_dir().join("tcm-v2-mcp-bridge.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(hs["port"].as_u64().unwrap() as u16, port);
    assert_eq!(hs["token"].as_str().unwrap(), token);
}
```

- [ ] **Step 2: Run to verify failure** — `SharedBridge`/`start_listener` don't exist yet.

- [ ] **Step 3: Append the server section to `ai_bridge.rs`**

```rust
// ---------------------------------------------------------------- server

use std::sync::{Arc, Mutex as StdMutex};

/// Context shared between the TCP loop and the set_bridge_context command.
pub struct BridgeState {
    pub ctx: StdMutex<BridgeContext>,
    pub token: String,
}
pub type SharedBridge = Arc<BridgeState>;

impl BridgeState {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(ctx: BridgeContext) -> SharedBridge {
        Arc::new(BridgeState { ctx: StdMutex::new(ctx), token: new_token() })
    }
}

/// Optional per-request ADO client factory: the Tauri layer passes one
/// that mints a fresh token; tests pass None (ping/validate only).
pub type ClientFactory =
    Arc<dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<crate::ado::AdoClient>> + Send>> + Send + Sync>;

/// Bind 127.0.0.1:0, write the handshake file, serve forever on the tokio
/// runtime. Returns (port, token). Requests: tiny HTTP/1.1, one request
/// per connection, 64 KiB body cap.
pub async fn start_listener(
    state: SharedBridge,
    make_client: Option<ClientFactory>,
) -> Result<(u16, String), String> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let token = state.token.clone();
    std::fs::write(
        std::env::temp_dir().join("tcm-v2-mcp-bridge.json"),
        serde_json::json!({ "port": port, "token": token }).to_string(),
    )
    .map_err(|e| e.to_string())?;

    tauri::async_runtime::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { continue };
            let state = Arc::clone(&state);
            let make_client = make_client.clone();
            tauri::async_runtime::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = vec![0u8; 65536];
                let mut used = 0usize;
                // Read until headers+body are complete (or the cap).
                let (method, target, tok, body) = loop {
                    let Ok(n) = sock.read(&mut buf[used..]).await else { return };
                    if n == 0 { return; }
                    used += n;
                    if let Some(parsed) = parse_http(&buf[..used]) {
                        break parsed;
                    }
                    if used >= buf.len() { return; }
                };
                let (status, payload) = if tok.as_deref() != Some(state.token.as_str()) {
                    (401, String::new())
                } else {
                    let ctx = state.ctx.lock().unwrap().clone();
                    let client = match &make_client {
                        Some(f) => f().await,
                        None => None,
                    };
                    route(&ctx, client.as_ref(), &method, &target, &body).await
                };
                let reason = match status { 200 => "OK", 400 => "Bad Request", 401 => "Unauthorized", 503 => "Unavailable", _ => "Not Found" };
                let resp = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                    payload.len(),
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            });
        }
    });
    Ok((port, state.token.clone()))
}

/// Returns Some((method, target, token-header, body)) once the request is
/// fully buffered; None while incomplete or on garbage.
fn parse_http(raw: &[u8]) -> Option<(String, String, Option<String>, String)> {
    let text = String::from_utf8_lossy(raw);
    let head_end = text.find("\r\n\r\n")?;
    let head = &text[..head_end];
    let mut lines = head.lines();
    let mut req = lines.next()?.split_whitespace();
    let method = req.next()?.to_string();
    let target = req.next()?.to_string();
    let mut token = None;
    let mut content_len = 0usize;
    for line in lines {
        let (k, v) = line.split_once(':')?;
        let v = v.trim();
        if k.eq_ignore_ascii_case("x-bridge-token") { token = Some(v.to_string()); }
        if k.eq_ignore_ascii_case("content-length") { content_len = v.parse().ok()?; }
    }
    let body_start = head_end + 4;
    if raw.len() < body_start + content_len { return None; }
    let body = String::from_utf8_lossy(&raw[body_start..body_start + content_len]).to_string();
    Some((method, target, token, body))
}
```

- [ ] **Step 4: Create `v2/src-tauri/src/commands/ai_bridge.rs`**

```rust
//! AI bridge lifecycle: start-on-demand + context push from the frontend.
//! The bridge itself is read/validate only (see ai_bridge.rs).

use std::sync::Arc;

use crate::ai_bridge::{BridgeContext, BridgeState, SharedBridge};
use crate::state::get_fresh_token;

/// Managed state: the running bridge, if any.
#[derive(Default)]
pub struct BridgeHandle(pub std::sync::Mutex<Option<(SharedBridge, u16)>>);

#[derive(serde::Serialize, specta::Type)]
pub struct BridgeStatus {
    pub port: u16,
    /// Absolute path to tcm-mcp.exe next to the app binary (what the user
    /// registers in their AI tool).
    pub mcp_exe: String,
}

#[tauri::command]
#[specta::specta]
pub async fn bridge_status(app: tauri::AppHandle) -> Result<BridgeStatus, String> {
    use tauri::Manager;
    // Already running? Reuse it.
    {
        let handle = app.state::<BridgeHandle>();
        let guard = handle.0.lock().unwrap();
        if let Some((_, port)) = guard.as_ref() {
            return Ok(BridgeStatus { port: *port, mcp_exe: mcp_exe_path() });
        }
    }
    let shared = BridgeState::new(BridgeContext::default());
    let app_for_client = app.clone();
    let factory: crate::ai_bridge::ClientFactory = Arc::new(move || {
        let app = app_for_client.clone();
        Box::pin(async move {
            get_fresh_token(&app).await.ok().map(crate::ado::AdoClient::new)
        })
    });
    let (port, _token) = crate::ai_bridge::start_listener(Arc::clone(&shared), Some(factory))
        .await?;
    let handle = app.state::<BridgeHandle>();
    *handle.0.lock().unwrap() = Some((shared, port));
    Ok(BridgeStatus { port, mcp_exe: mcp_exe_path() })
}

/// The frontend pushes its current org/project + detected field refs so
/// bridge routes have defaults the AI never has to guess.
#[tauri::command]
#[specta::specta]
pub fn set_bridge_context(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    module_ref: Option<String>,
    preconditions_ref: Option<String>,
) {
    use tauri::Manager;
    let handle = app.state::<BridgeHandle>();
    if let Some((shared, _)) = handle.0.lock().unwrap().as_ref() {
        *shared.ctx.lock().unwrap() = BridgeContext {
            org: organization,
            project,
            module_ref,
            preconditions_ref,
        };
    }
}

fn mcp_exe_path() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("tcm-mcp.exe")))
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}
```

Register in `lib.rs`: add `.manage(commands::ai_bridge::BridgeHandle::default())` next to the other `.manage(...)` calls in `run()`, add `ai_bridge` to the `use commands::{...}` list in `specta_builder()`, and append `ai_bridge::bridge_status, ai_bridge::set_bridge_context` to `collect_commands![]` (after `discovery::list_iterations`).

- [ ] **Step 5: Run gates + commit**

Run: `cd v2/src-tauri && CARGO_TARGET_DIR=target/gate cargo test` — all pass; verify `grep -n "bridgeStatus\|setBridgeContext" ../src/bindings.ts` shows both.

```bash
git add v2/src-tauri/src/ai_bridge.rs v2/src-tauri/src/commands/ai_bridge.rs v2/src-tauri/src/commands/mod.rs v2/src-tauri/src/lib.rs v2/src-tauri/tests/ai_bridge.rs v2/src/bindings.ts
git commit -F - <<'EOF'
feat(v2): AI bridge listener with handshake file + status/context commands

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 4: The tcm-mcp stdio binary

**Files:**
- Create: `v2/src-tauri/src/bin/tcm_mcp.rs`
- Modify: `v2/src-tauri/Cargo.toml` (add the `[[bin]]` section shown below, after the existing `[lib]`/`[[bin]]` sections — check with `grep -n "\[\[bin\]\]\|\[lib\]" Cargo.toml` first)
- Test: `v2/src-tauri/tests/tcm_mcp.rs`

**Interfaces:**
- Consumes: the handshake file `{port, token}`; the bridge HTTP routes (Tasks 1–3).
- Produces: `tcm-mcp.exe` speaking MCP over stdio with tools `get_writing_guide`, `get_example_cases`, `validate_cases`, `search_pbis`. The dispatch logic is a PURE function `handle_message(msg: &str, call: &dyn Fn(&str, &str, &str) -> Result<(u16, String), String>) -> Option<String>` (message in, JSON-RPC response out, None for notifications) so tests never touch stdio or sockets.

Cargo.toml addition:

```toml
[[bin]]
name = "tcm-mcp"
path = "src/bin/tcm_mcp.rs"
```

- [ ] **Step 1: Write the failing tests**

```rust
// v2/src-tauri/tests/tcm_mcp.rs
//! The MCP dispatcher is pure: JSON-RPC string in, response string out,
//! with the bridge call injected - no stdio, no sockets.

use v2_lib::mcp::handle_message;

/// Bridge stub: records the call, returns a canned body.
fn stub(status: u16, body: &str) -> impl Fn(&str, &str, &str) -> Result<(u16, String), String> + '_ {
    move |_method, _path, _payload| Ok((status, body.to_string()))
}

#[test]
fn initialize_echoes_protocol_and_advertises_tools() {
    let req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#;
    let resp = handle_message(req, &stub(200, "")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["id"], 1);
    assert_eq!(v["result"]["protocolVersion"], "2025-03-26");
    assert!(v["result"]["capabilities"]["tools"].is_object());
    assert_eq!(v["result"]["serverInfo"]["name"], "tcm-testcases");
}

#[test]
fn notifications_get_no_response() {
    assert!(handle_message(
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        &stub(200, "")
    )
    .is_none());
}

#[test]
fn tools_list_names_all_four() {
    let resp = handle_message(
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        &stub(200, ""),
    )
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let names: Vec<&str> = v["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        vec!["get_writing_guide", "get_example_cases", "validate_cases", "search_pbis"]
    );
    // Every tool must carry an inputSchema (clients reject tools without one).
    for t in v["result"]["tools"].as_array().unwrap() {
        assert_eq!(t["inputSchema"]["type"], "object");
    }
}

#[test]
fn tools_call_proxies_to_the_bridge_and_wraps_text() {
    let req = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_example_cases","arguments":{"pbi_id":42,"limit":3}}}"#;
    let calls = std::cell::RefCell::new(vec![]);
    let call = |method: &str, path: &str, body: &str| {
        calls.borrow_mut().push((method.to_string(), path.to_string(), body.to_string()));
        Ok((200, r#"{"test_cases":[]}"#.to_string()))
    };
    let resp = handle_message(req, &call).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["content"][0]["type"], "text");
    assert_eq!(v["result"]["content"][0]["text"], r#"{"test_cases":[]}"#);
    let recorded = calls.borrow();
    assert_eq!(recorded[0].0, "GET");
    assert_eq!(recorded[0].1, "/examples?pbi=42&limit=3");
}

#[test]
fn bridge_errors_surface_as_tool_errors_not_crashes() {
    let req = r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"get_writing_guide","arguments":{}}}"#;
    let down = |_: &str, _: &str, _: &str| Err("connection refused".to_string());
    let resp = handle_message(req, &down).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["isError"], true);
    let text = v["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Test Case Manager"), "tells the user to start the app");
}
```

- [ ] **Step 2: Run to verify failure** — `v2_lib::mcp` doesn't exist.

- [ ] **Step 3: Implement**

Add `pub mod mcp;` to `v2/src-tauri/src/lib.rs` (after `pub mod import_parser;` — wherever alphabetical) and create the dispatcher IN THE LIB (`v2/src-tauri/src/mcp.rs`) so the integration test can reach it; the bin is a thin shell around it.

```rust
// v2/src-tauri/src/mcp.rs
//! MCP (Model Context Protocol) dispatcher for the tcm-mcp bridge binary.
//! Pure: JSON-RPC message string in, response string out (None for
//! notifications). The bridge call is injected so tests run without
//! stdio or sockets. Transport framing (newline-delimited stdio) and the
//! HTTP client live in src/bin/tcm_mcp.rs.

type BridgeCall<'a> = &'a dyn Fn(&str, &str, &str) -> Result<(u16, String), String>;

pub fn handle_message(msg: &str, call: BridgeCall) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(msg).ok()?;
    let method = v["method"].as_str()?;
    let id = v.get("id").cloned();
    // Notifications (no id) never get a response.
    id.as_ref()?;
    let id = id.unwrap();

    let result = match method {
        "initialize" => serde_json::json!({
            "protocolVersion": v["params"]["protocolVersion"].as_str().unwrap_or("2024-11-05"),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "tcm-testcases", "version": env!("CARGO_PKG_VERSION") },
        }),
        "tools/list" => tools_list(),
        "tools/call" => tools_call(&v["params"], call),
        _ => {
            return Some(
                serde_json::json!({
                    "jsonrpc": "2.0", "id": id,
                    "error": { "code": -32601, "message": format!("unknown method {method}") },
                })
                .to_string(),
            )
        }
    };
    Some(serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string())
}

fn schema(props: serde_json::Value, required: &[&str]) -> serde_json::Value {
    serde_json::json!({ "type": "object", "properties": props, "required": required })
}

fn tools_list() -> serde_json::Value {
    serde_json::json!({ "tools": [
        {
            "name": "get_writing_guide",
            "description": "The live guide for writing Test Case Manager import JSON: format rules, the org's allowed Module values, and the recommended workflow. Call this first.",
            "inputSchema": schema(serde_json::json!({}), &[]),
        },
        {
            "name": "get_example_cases",
            "description": "Real existing test cases linked to a PBI, in the exact import JSON shape - mimic their style and granularity.",
            "inputSchema": schema(serde_json::json!({
                "pbi_id": { "type": "integer", "description": "Work item id of the PBI" },
                "limit": { "type": "integer", "description": "Max cases (default 5, cap 20)" },
            }), &["pbi_id"]),
        },
        {
            "name": "validate_cases",
            "description": "Validate draft import JSON with Test Case Manager's REAL importer. Returns case count, warnings, and errors - fix every warning before finishing.",
            "inputSchema": schema(serde_json::json!({
                "json": { "type": "string", "description": "The draft import JSON (array or wrapper object)" },
            }), &["json"]),
        },
        {
            "name": "search_pbis",
            "description": "Search the current project's PBIs by title text to find the right work item id.",
            "inputSchema": schema(serde_json::json!({
                "query": { "type": "string" },
            }), &["query"]),
        },
    ]})
}

fn tools_call(params: &serde_json::Value, call: BridgeCall) -> serde_json::Value {
    let name = params["name"].as_str().unwrap_or_default();
    let args = &params["arguments"];
    let outcome = match name {
        "get_writing_guide" => call("GET", "/guide", ""),
        "get_example_cases" => {
            let pbi = args["pbi_id"].as_i64().unwrap_or(0);
            let limit = args["limit"].as_i64().unwrap_or(5);
            call("GET", &format!("/examples?pbi={pbi}&limit={limit}"), "")
        }
        "validate_cases" => call("POST", "/validate", args["json"].as_str().unwrap_or("")),
        "search_pbis" => {
            let q = args["query"].as_str().unwrap_or("");
            call("GET", &format!("/search-pbis?q={}", q.replace(' ', "%20")), "")
        }
        other => Err(format!("unknown tool {other}")),
    };
    match outcome {
        Ok((status, body)) if status < 400 => serde_json::json!({
            "content": [{ "type": "text", "text": body }],
        }),
        Ok((status, body)) => serde_json::json!({
            "isError": true,
            "content": [{ "type": "text", "text": format!("bridge returned {status}: {body}") }],
        }),
        Err(e) => serde_json::json!({
            "isError": true,
            "content": [{ "type": "text", "text": format!(
                "Could not reach Test Case Manager ({e}). Start the app and sign in, then retry."
            )}],
        }),
    }
}
```

```rust
// v2/src-tauri/src/bin/tcm_mcp.rs
//! tcm-mcp: MCP stdio bridge to a RUNNING Test Case Manager. Reads the
//! handshake file for the port + token, proxies each tool call over
//! localhost, and never sees credentials. Register in an AI tool as:
//!   claude mcp add tcm-testcases -- "<install dir>\tcm-mcp.exe"

use std::io::{BufRead, Write};

fn bridge_call(method: &str, path: &str, body: &str) -> Result<(u16, String), String> {
    let hs: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(std::env::temp_dir().join("tcm-v2-mcp-bridge.json"))
            .map_err(|_| "handshake file missing - is the app running?".to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let port = hs["port"].as_u64().ok_or("bad handshake file")? as u16;
    let token = hs["token"].as_str().ok_or("bad handshake file")?;

    let client = reqwest::blocking::Client::new();
    let url = format!("http://127.0.0.1:{port}{path}");
    let req = match method {
        "POST" => client.post(&url).body(body.to_string()),
        _ => client.get(&url),
    };
    let resp = req
        .header("x-bridge-token", token)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let text = resp.text().map_err(|e| e.to_string())?;
    Ok((status, text))
}

fn main() {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(resp) = v2_lib::mcp::handle_message(&line, &bridge_call) {
            let _ = writeln!(stdout, "{resp}");
            let _ = stdout.flush();
        }
    }
}
```

NOTE: `reqwest::blocking` requires reqwest's `blocking` feature. Check `grep -n "reqwest" v2/src-tauri/Cargo.toml`; if `blocking` is not in the feature list, add it to the existing reqwest line's `features = [...]` — this is a feature flag on an existing dependency, not a new dependency. Also confirm the lib name: `grep -n "^name\|\[lib\]" v2/src-tauri/Cargo.toml` — the tests reference `v2_lib`, so the bin uses `v2_lib::mcp` too.

- [ ] **Step 4: Run to verify pass**

Run: `cd v2/src-tauri && CARGO_TARGET_DIR=target/gate cargo test --test tcm_mcp`
Expected: 5 passed. Then full gate: `CARGO_TARGET_DIR=target/gate cargo test`.

- [ ] **Step 5: Smoke the real binary once**

```bash
cd v2/src-tauri && cargo build --bin tcm-mcp 2>&1 | tail -1
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' | ./target/debug/tcm-mcp.exe
```
Expected: two JSON lines — an initialize result naming `tcm-testcases`, then a tools list with 4 tools.

- [ ] **Step 6: Commit**

```bash
git add v2/src-tauri/src/mcp.rs v2/src-tauri/src/bin/tcm_mcp.rs v2/src-tauri/src/lib.rs v2/src-tauri/Cargo.toml v2/src-tauri/Cargo.lock v2/src-tauri/tests/tcm_mcp.rs
git commit -F - <<'EOF'
feat(v2): tcm-mcp stdio binary - MCP tools proxied to the app bridge

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 5: Packaging — ship tcm-mcp.exe with the app

**Files:**
- Modify: `v2/scripts/pack.ps1`

**Interfaces:**
- Consumes: `tcm-mcp.exe` built by `npm run tauri build` (cargo builds every `[[bin]]` in release).
- Produces: the Velopack package contains `tcm-mcp.exe` beside the main exe, which is exactly where `mcp_exe_path()` (Task 3) points.

- [ ] **Step 1: Edit pack.ps1** — after the existing `Copy-Item $exe.FullName $stage` line, add:

```powershell
# The MCP bridge binary ships beside the app (Settings shows its path for
# registration in AI tools). Fail loudly if the build didn't produce it.
$mcp = Join-Path $exeDir "tcm-mcp.exe"
if (-not (Test-Path $mcp)) { throw "tcm-mcp.exe not found in $exeDir - the [[bin]] target did not build." }
Copy-Item $mcp $stage
```

- [ ] **Step 2: Verify locally**

```bash
cd v2 && npm run tauri build 2>&1 | tail -2
ls src-tauri/target/release/tcm-mcp.exe
```
Expected: both exes exist. (Do NOT run vpk/publish — that is release-time only.)

- [ ] **Step 3: Commit**

```bash
git add v2/scripts/pack.ps1
git commit -F - <<'EOF'
build(v2): pack tcm-mcp.exe beside the app in the Velopack payload

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 6: Frontend — context push + Settings "AI Bridge" section

**Files:**
- Modify: `v2/src/App.tsx` (context push effect)
- Modify: `v2/src/screens/Settings.tsx` (new section, after the Changelog section)
- Modify: `v2/src/dev/demo.ts` (fakes for `bridgeStatus` / `setBridgeContext`)
- Test: `v2/src/screens/Settings.test.tsx` (append), `v2/src/App.test.tsx` (append)

**Interfaces:**
- Consumes: `commands.bridgeStatus() -> Result<BridgeStatus{port, mcp_exe}, string>` and `commands.setBridgeContext(organization, project, moduleRef, preconditionsRef)` (Task 3 bindings — check the exact camelCase argument names in `v2/src/bindings.ts` before writing calls); `useFieldRefs(org, project)` for the refs.

- [ ] **Step 1: Write the failing tests**

Append to `v2/src/screens/Settings.test.tsx` (reuse its existing `renderSettings(qc)` helper and imports):

```tsx
test("the AI Bridge section shows the registration command with the shipped exe path", async () => {
  mockIPC((cmd) => {
    if (cmd === "bridge_status")
      return { port: 51234, mcp_exe: "C:\\apps\\tcm\\tcm-mcp.exe" };
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);
  expect(await screen.findByText("AI Bridge")).toBeInTheDocument();
  expect(screen.getByText(/51234/)).toBeInTheDocument();
  // The copy-able registration one-liner embeds the exe path.
  expect(screen.getByText(/tcm-mcp\.exe/)).toBeInTheDocument();
});
```

Append to `v2/src/App.test.tsx` (reuse its existing signed-in render helper — read the file's existing tests first and copy their mock/render pattern exactly):

```tsx
test("the app pushes org/project context to the AI bridge", async () => {
  const pushes: Array<Record<string, unknown>> = [];
  // EXTEND the file's standard mock set with:
  //   if (cmd === "set_bridge_context") { pushes.push(args as Record<string, unknown>); return null; }
  //   if (cmd === "bridge_status") return { port: 1, mcp_exe: "x" };
  // then render signed-in with org "acme" / project "Web" the way the
  // neighbouring tests do.
  await vi.waitFor(() => expect(pushes.length).toBeGreaterThan(0));
  expect(pushes[pushes.length - 1]).toMatchObject({ organization: "acme", project: "Web" });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `cd v2 && npm test > "$TEMP/t.txt" 2>&1; echo exit=$?` — the two new tests fail (`bridge_status` unmocked elsewhere is fine: the query must use `retry: false` and render nothing on error).

- [ ] **Step 3: Implement**

App.tsx — after the existing changelog effect, add:

```tsx
  // Keep the AI bridge's defaults in sync with what the user is looking at:
  // org/project + the detected custom-field refs. Fire-and-forget; the
  // bridge simply serves stale context until the next push.
  const { prefs: bridgePrefs } = useFieldRefs(org, project);
  useEffect(() => {
    if (!signedIn || !org || !project) return;
    commands
      .bridgeStatus()
      .then(() =>
        commands.setBridgeContext(org, project, bridgePrefs.moduleRef, bridgePrefs.preconditionsRef),
      )
      .catch(() => {});
  }, [signedIn, org, project, bridgePrefs.moduleRef, bridgePrefs.preconditionsRef]);
```

(Import `useFieldRefs` from `./hooks/useFieldRefs`. If App already destructures `prefs` elsewhere, rename this binding as shown to avoid a collision — check first with `grep -n "useFieldRefs" v2/src/App.tsx`.)

Settings.tsx — after the Changelog `</section>`, add:

```tsx
      <section className="space-y-3">
        <h2 className="text-sm font-semibold text-text">AI Bridge</h2>
        <p className="text-sm text-muted">
          Lets AI tools (Claude Code, Cursor...) fetch the writing guide, real
          example test cases, and validation from this app while it runs.
          Read-only - AI can never create or change anything in Azure DevOps.
        </p>
        {bridge.data ? (
          <>
            <p className="text-xs text-success">
              Running on 127.0.0.1:{bridge.data.port}
            </p>
            <p className="text-xs text-muted">Register in Claude Code:</p>
            <div className="flex items-center gap-2">
              <code className="id-mono flex-1 truncate rounded bg-surface-2 px-2 py-1 text-xs text-text">
                claude mcp add tcm-testcases -- "{bridge.data.mcp_exe}"
              </code>
              <Button
                size="sm"
                variant="outline"
                onClick={() => {
                  navigator.clipboard
                    .writeText(`claude mcp add tcm-testcases -- "${bridge.data!.mcp_exe}"`)
                    .then(() => toast.success("Copied."));
                }}
              >
                Copy
              </Button>
            </div>
          </>
        ) : (
          <p className="text-xs text-faint">Bridge not running.</p>
        )}
      </section>
```

with, near the other queries in Settings:

```tsx
  const bridge = useQuery({
    queryKey: ["bridge-status"],
    queryFn: () => unwrapStr(commands.bridgeStatus()),
    retry: false,
  });
```

(`bridgeStatus` returns `Result<_, String>` → `unwrapStr` from `../lib/ipc`; add the import. Check `bindings.ts` to confirm and adjust to `unwrap` if the error type came out as AdoError.)

demo.ts — add beside the other fakes, matching the file's `ok()` style:

```ts
    bridgeStatus: () => ok({ port: 51999, mcp_exe: "C:\\demo\\tcm-mcp.exe" }),
    setBridgeContext: () => ok(null),
```

- [ ] **Step 4: Run gates**

Run: `cd v2 && npm test > "$TEMP/t.txt" 2>&1; echo exit=$?; tail -5 "$TEMP/t.txt"`
Expected: exit=0, no `Errors:` line. Then `npm run build` clean and the elimination probe: `grep -rl "tcm-v2-dev-demo" dist/assets/ || echo CLEAN` → CLEAN.

- [ ] **Step 5: Commit**

```bash
git add v2/src/App.tsx v2/src/screens/Settings.tsx v2/src/screens/Settings.test.tsx v2/src/App.test.tsx v2/src/dev/demo.ts v2/src/bindings.ts
git commit -F - <<'EOF'
feat(v2): AI Bridge settings section + live context push from the app

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 7: Live end-to-end check + memory/docs

**Files:**
- Modify: `docs/superpowers/specs/2026-07-20-ai-guide-generator-design.md` (prepend a superseded-by note)

- [ ] **Step 1: End-to-end smoke with the dev app** — start the dev app (`npm run dev` or the packed exe), sign in (or demo mode), open Settings → AI Bridge shows a port. Then from a shell:

```bash
cd v2/src-tauri && cargo build --bin tcm-mcp 2>&1 | tail -1
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"validate_cases","arguments":{"json":"[{\"title\":\"T\",\"steps\":[{\"action\":\"a\",\"expected\":\"\"}]}]"}}}' | ./target/debug/tcm-mcp.exe
```
Expected: the second response's `content[0].text` contains `"cases":1`. (In demo mode `/guide`/`/examples` return 503 or demo data depending on sign-in state — the validate path is the sign-in-free smoke.)

- [ ] **Step 2: Mark the old spec superseded** — prepend to the old spec file:

```markdown
> **SUPERSEDED (2026-07-24)** by the AI MCP bridge:
> `docs/superpowers/plans/2026-07-24-ai-mcp-bridge.md`. Kept for the
> format/drift-gate reasoning the bridge's /guide and /validate reuse.
```

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/specs/2026-07-20-ai-guide-generator-design.md
git commit -F - <<'EOF'
docs(v2): AI-guide spec superseded by the MCP bridge plan

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
git log -1
```
