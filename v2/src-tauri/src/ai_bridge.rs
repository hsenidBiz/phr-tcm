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
        `{{\"action\", \"expected\"}}`), `tags` (semicolon-separated, never commas),\n\
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
        .get_pbi_test_cases_full(
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
    match client.search_pbis(&ctx.org, &ctx.project, &query, 20).await {
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
