//! The AI bridge: a 127.0.0.1-only service that lets the tcm-mcp stdio
//! binary (and therefore AI tools) read a live writing guide, fetch real
//! example cases, validate drafts against the REAL importer, and search
//! PBIs. Read + validate ONLY - no writes, no raw ADO passthrough, and
//! the bearer token never leaves the app. Modeled on note_server.rs; the
//! TCP loop is thin, all logic lives in `route` so tests need no sockets.

use rand::RngExt;
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
/// `pub` so `tests/ai_bridge.rs` can exercise it directly.
pub fn q(target: &str, key: &str) -> Option<String> {
    let qs = target.split_once('?')?.1;
    for pair in qs.split('&') {
        let Some((k, v)) = pair.split_once('=') else {
            continue;
        };
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

/// Process-wide counter so concurrent /validate calls never share a temp
/// file (the pid alone is constant for the app's lifetime).
static VALIDATE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Run the REAL importer on the draft: write to a temp file (parse_file
/// dispatches on extension) and report cases/warnings/error.
fn validate_json(body: &str) -> String {
    let dir = std::env::temp_dir().join("tcm-v2-bridge");
    let _ = std::fs::create_dir_all(&dir);
    let seq = VALIDATE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = dir.join(format!("validate-{}-{}.json", std::process::id(), seq));
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
