//! The AI bridge: a 127.0.0.1-only service that lets the tcm-mcp stdio
//! binary (and therefore AI tools) read a live writing guide, fetch real
//! example cases, reorganise a draft into a run sheet, and search PBIs.
//! Reads and pure transforms ONLY - no writes, no raw ADO passthrough, and
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
    /// Tools the user has switched off in the AI Bridge tab. Empty means
    /// everything is available - the default - so an unset context can
    /// never accidentally disable the whole server.
    pub disabled_tools: Vec<String>,
}

/// Per-launch shared secret for the handshake file (32 hex chars).
pub fn new_token() -> String {
    let mut rng = rand::rng();
    (0..32)
        .map(|_| char::from_digit(rng.random_range(0..16), 16).unwrap())
        .collect()
}

/// Query-string value by key from "a=1&b=2", percent-decoded ('+' -> space,
/// arbitrary %XX -> the raw byte). `pub` so `tests/ai_bridge.rs` can
/// exercise it directly.
pub fn q(target: &str, key: &str) -> Option<String> {
    let qs = target.split_once('?')?.1;
    for pair in qs.split('&') {
        let Some((k, v)) = pair.split_once('=') else {
            continue;
        };
        if k == key {
            return Some(percent_decode(v));
        }
    }
    None
}

/// Minimal RFC 3986 percent-decoder: '+' -> space (form convention), %XX ->
/// the decoded byte, invalid/truncated escapes pass through literally.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 3 <= bytes.len() && s.is_char_boundary(i + 3) => {
                match u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    Ok(byte) => {
                        out.push(byte);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
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
    version: &str,
) -> (u16, String) {
    let path = target.split_once('?').map(|(p, _)| p).unwrap_or(target);
    match (method, path) {
        ("GET", "/ping") => (
            200,
            serde_json::json!({
                "app": "tcm",
                "version": version,
                "org": ctx.org,
                "project": ctx.project,
            })
            .to_string(),
        ),
        ("POST", "/optimize") => optimize_json(body, target),
        ("POST", "/transform") => transform_json(body),
        ("POST", "/validate") => (200, validate_json(body, target, ctx, client).await),
        ("POST", "/begin") => begin_writing(body, target, ctx, client).await,
        ("GET", "/guide") => match client {
            Some(c) => (200, guide(ctx, c).await),
            None => (503, "sign in to Test Case Manager first".into()),
        },
        ("GET", "/examples") => match client {
            Some(c) => examples(ctx, c, target).await,
            None => (503, "sign in to Test Case Manager first".into()),
        },
        ("GET", "/tags") => tags(ctx, client, target).await,
        // The proxy asks for this before listing tools, so a toggle in the
        // app takes effect on the assistant's next tools/list.
        ("GET", "/tools") => (
            200,
            serde_json::json!({ "disabled": ctx.disabled_tools }).to_string(),
        ),
        ("GET", "/search-pbis") => match client {
            Some(c) => search_pbis(ctx, c, target).await,
            None => (503, "sign in to Test Case Manager first".into()),
        },
        ("GET", "/search-wiki") => match client {
            Some(c) => search_wiki(ctx, c, target).await,
            None => (503, "sign in to Test Case Manager first".into()),
        },
        ("GET", "/wiki-page") => match client {
            Some(c) => wiki_page(ctx, c, target).await,
            None => (503, "sign in to Test Case Manager first".into()),
        },
        _ => (404, String::new()),
    }
}

/// Reorganise a draft into a run sheet: navigation spelled out as steps,
/// cases ordered so the tester switches environment as little as
/// possible, expected results reduced to the outcome. Pure - it reads the
/// JSON the assistant sends and hands back a new one. Nothing is written
/// anywhere, and Azure DevOps is never touched.
fn optimize_json(body: &str, target: &str) -> (u16, String) {
    let cases = match parse_cases(body) {
        Ok(c) => c,
        Err(e) => return (400, serde_json::json!({ "error": e }).to_string()),
    };
    let entry = q(target, "entry");
    let dry_run = matches!(q(target, "dry_run").as_deref(), Some("true") | Some("1"));
    let (optimized, report) = crate::optimize::optimize(cases, entry.as_deref());
    if dry_run {
        // Report only: the caller inspects what WOULD change before
        // committing to the transformed JSON.
        return (
            200,
            serde_json::json!({
                "report": report,
                "note": "Dry run - no test_cases returned. Call again without dry_run to get the transformed JSON.",
            })
            .to_string(),
        );
    }
    let json = match crate::import_parser::queue_to_json_string(&optimized) {
        Ok(j) => j,
        Err(e) => return (500, serde_json::json!({ "error": e }).to_string()),
    };
    let doc: serde_json::Value = serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);
    (
        200,
        serde_json::json!({
            "test_cases": doc.get("test_cases").cloned().unwrap_or(doc),
            "report": report,
            "note": "Hand this JSON to the developer as the file to import. The report explains what was reordered and why.",
        })
        .to_string(),
    )
}

/// Apply declarative edits to a draft - the restructuring an assistant
/// would otherwise write a throwaway script for. Pure, like `optimize`.
fn transform_json(body: &str) -> (u16, String) {
    let doc: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return (400, serde_json::json!({ "error": format!("invalid JSON: {e}") }).to_string()),
    };
    // The draft arrives as a JSON *string* (the tool's `json` argument),
    // but a caller posting the array inline should work too.
    let draft = match &doc["test_cases"] {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let cases = match parse_cases(&draft) {
        Ok(c) => c,
        Err(e) => return (400, serde_json::json!({ "error": e }).to_string()),
    };
    let ops = match crate::transform::parse_ops(&doc["operations"]) {
        Ok(o) => o,
        Err(e) => return (400, serde_json::json!({ "error": e }).to_string()),
    };
    let (out, report) = crate::transform::apply(cases, &ops);
    let json = match crate::import_parser::queue_to_json_string(&out) {
        Ok(j) => j,
        Err(e) => return (500, serde_json::json!({ "error": e }).to_string()),
    };
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);
    (
        200,
        serde_json::json!({
            "test_cases": parsed.get("test_cases").cloned().unwrap_or(parsed),
            "report": report,
        })
        .to_string(),
    )
}

/// Hand control to the developer before a single case is written.
///
/// Phase 1 (empty body): the checklist to put to them in chat, with the
/// context this app already knows - org/project, the org's real Module
/// values, a suggested output folder - so they correct defaults instead
/// of composing answers from nothing.
///
/// Phase 2 (answers in the body): the answers are CHECKED - spec files
/// must exist on disk, the output folder must exist, the module must be
/// a real one - and only then is a plan file written. An assistant that
/// invents a plausible path is caught by name here, which is most of
/// what a file picker would have bought.
async fn begin_writing(
    body: &str,
    target: &str,
    ctx: &BridgeContext,
    client: Option<&crate::ado::AdoClient>,
) -> (u16, String) {
    let feature = q(target, "feature").unwrap_or_default();
    let modules = match client {
        Some(c) => allowed_modules(ctx, c).await,
        None => vec![],
    };

    // Phase 1: nothing sent, so hand back the questions.
    if body.trim().is_empty() || body.trim() == "{}" {
        return (
            200,
            serde_json::json!({
                "status": "questions",
                "feature": feature,
                "ask_the_developer": crate::intake::questions(),
                "context": {
                    "organization": ctx.org,
                    "project": ctx.project,
                    "allowed_modules": modules,
                    "tags_hint": "Call get_tags (with a query filter) to reuse existing tags.",
                },
                "note": "Put these to the developer in chat - one at a time, and let them \
                         paste or drop file paths. Do NOT answer them yourself, and do not \
                         start writing. When they have answered, call this tool again with \
                         their answers to get the plan.",
            })
            .to_string(),
        );
    }

    let answers: crate::intake::IntakeAnswers = match serde_json::from_str(body) {
        Ok(a) => a,
        Err(e) => {
            return (
                400,
                serde_json::json!({ "status": "error", "error": format!("could not read the answers: {e}") })
                    .to_string(),
            )
        }
    };

    let problems = crate::intake::problems(&answers, &modules);
    if !problems.is_empty() {
        return (
            200,
            serde_json::json!({
                "status": "needs_answers",
                "problems": problems,
                "allowed_modules": modules,
                "note": "Go back to the developer with these - do not guess a value or a \
                         path to get past them, and do not start writing.",
            })
            .to_string(),
        );
    }

    let plan = crate::intake::plan_markdown(&answers, &feature);
    let plan_path = crate::intake::plan_path(&answers.output_path);
    let written = std::fs::write(&plan_path, &plan).is_ok();
    (
        200,
        serde_json::json!({
            "status": "ready",
            "plan": plan,
            "plan_path": if written { serde_json::json!(plan_path) } else { serde_json::Value::Null },
            "plan_write_error": if written {
                serde_json::Value::Null
            } else {
                serde_json::json!(format!("could not write {plan_path} - the plan is in this response instead"))
            },
            "answers": answers,
            "note": "Show this plan to the developer and get their agreement before writing \
                     any case. Then write only what it covers, put the JSON exactly at \
                     output_path, and follow the steps at the end of the plan.",
        })
        .to_string(),
    )
}

/// Validate a draft with the app's REAL importer: case count, warnings,
/// errors. With a signed-in client the Module values are also checked
/// against the org's picklist. Reinstated after field feedback rated it
/// the most trustworthy tool in the set.
///
/// Large drafts: `?path=` reads the draft from a local file instead of
/// the request body, so a 166 KB file needs no splitting. An empty body
/// with no path is an explicit error, never a silent pass - a caller must
/// not be able to mistake "nothing arrived" for "nothing wrong".
async fn validate_json(
    body: &str,
    target: &str,
    ctx: &BridgeContext,
    client: Option<&crate::ado::AdoClient>,
) -> String {
    let from_path = q(target, "path");
    let parsed = match &from_path {
        Some(path) => {
            if !std::path::Path::new(path).is_file() {
                return serde_json::json!({
                    "cases": 0, "warnings": [],
                    "error": format!("{path} does not exist or is not a file"),
                })
                .to_string();
            }
            crate::import_parser::parse_file(path)
        }
        None => {
            if body.trim().is_empty() {
                return serde_json::json!({
                    "cases": 0, "warnings": [],
                    "error": "empty draft - pass the JSON in the body, or a local file via ?path= for large drafts",
                })
                .to_string();
            }
            match parse_cases_with_warnings(body) {
                Ok(v) => Ok(v),
                Err(e) => Err(e),
            }
        }
    };
    let out = match parsed {
        Ok((cases, mut warnings)) => {
            if let Some(c) = client {
                let allowed = allowed_modules(ctx, c).await;
                if !allowed.is_empty() {
                    for (i, tc) in cases.iter().enumerate() {
                        let m = tc.module_value.trim();
                        if !m.is_empty() && !allowed.iter().any(|a| a.eq_ignore_ascii_case(m)) {
                            warnings.push(format!(
                                "Test case {} ('{}'): Module '{}' is not an allowed value in \
                                 this organization - pick one from get_writing_guide.",
                                i + 1,
                                tc.title,
                                m
                            ));
                        }
                    }
                }
            }
            serde_json::json!({
                "cases": cases.len(),
                "warnings": warnings,
                "error": serde_json::Value::Null,
            })
        }
        Err(e) => serde_json::json!({ "cases": 0, "warnings": [], "error": e }),
    };
    out.to_string()
}

/// Like `parse_cases`, but keeps the importer's warnings too.
fn parse_cases_with_warnings(
    body: &str,
) -> Result<(Vec<crate::model::TestCase>, Vec<String>), String> {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = std::env::temp_dir().join("tcm-v2-bridge");
    let _ = std::fs::create_dir_all(&dir);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = dir.join(format!("validate-{}-{}.json", std::process::id(), seq));
    std::fs::write(&path, body).map_err(|_| "could not stage the draft".to_string())?;
    let parsed = crate::import_parser::parse_file(path.to_str().unwrap_or_default());
    let _ = std::fs::remove_file(&path);
    parsed
}

/// Run a draft through the app's REAL importer to get `TestCase`s, so
/// these tools accept exactly what the Import File tab accepts (bare
/// array, `test_cases` wrapper, the lot).
fn parse_cases(body: &str) -> Result<Vec<crate::model::TestCase>, String> {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = std::env::temp_dir().join("tcm-v2-bridge");
    let _ = std::fs::create_dir_all(&dir);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = dir.join(format!("draft-{}-{}.json", std::process::id(), seq));
    std::fs::write(&path, body).map_err(|_| "could not stage the draft".to_string())?;
    let parsed = crate::import_parser::parse_file(path.to_str().unwrap_or_default());
    let _ = std::fs::remove_file(&path);
    parsed.map(|(cases, _warnings)| cases)
}

/// The project's existing tag names, for suggesting tags that match what
/// the team already uses instead of inventing near-duplicates.
///
/// Reads the shared reference cache the app fills (refcache.rs) - the
/// whole point is that an assistant asking for tags does NOT repeat a
/// request the app has already made. Only a completely cold cache (the AI
/// asked before the developer opened a tag field) fetches, and it stores
/// the result so the app doesn't pay for it either.
async fn tags(
    ctx: &BridgeContext,
    client: Option<&crate::ado::AdoClient>,
    target: &str,
) -> (u16, String) {
    let key = crate::refcache::tags_key(&ctx.org, &ctx.project);
    let (values, source) = match crate::refcache::any(&key) {
        Some(v) => (v, "cache"),
        None => match client {
            Some(c) => match c.get_tags(&ctx.org, &ctx.project).await {
                Ok(v) => {
                    crate::refcache::put(&key, &v);
                    (v, "fetched")
                }
                Err(e) => return (502, format!("could not read tags: {e}")),
            },
            None => return (503, "sign in to Test Case Manager first".into()),
        },
    };
    // A project can carry thousands of tags; ?query= filters to a
    // case-insensitive substring match so the common call stays cheap.
    let total = values.len();
    let values: Vec<String> = match q(target, "query").filter(|f| !f.trim().is_empty()) {
        Some(f) => {
            let f = f.to_lowercase();
            values
                .into_iter()
                .filter(|t| t.to_lowercase().contains(&f))
                .collect()
        }
        None => values,
    };
    (
        200,
        serde_json::json!({
            "tags": values,
            "count": values.len(),
            "total": total,
            "source": source,
            "note": "Prefer an existing tag over a new one. Tags are \
                     semicolon-separated in the import JSON, never commas.",
        })
        .to_string(),
    )
}

/// How many tag names the writing guide inlines before it stops and
/// points at the dedicated tool. Long enough to be genuinely useful,
/// short enough not to drown the guide.
const GUIDE_TAG_LIMIT: usize = 60;

/// The org's Module values: configured picklist first, observed values as
/// the fallback - the same discovery the app's own module picker uses.
async fn allowed_modules(ctx: &BridgeContext, client: &crate::ado::AdoClient) -> Vec<String> {
    match &ctx.module_ref {
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
    }
}

/// Live writing guide: format rules from the importer's own constants +
/// the org's Module values, fetched fresh (no snapshot staleness).
async fn guide(ctx: &BridgeContext, client: &crate::ado::AdoClient) -> String {
    let statuses = crate::model::VALID_STATUSES
        .iter()
        .map(|v| format!("\"{v}\""))
        .collect::<Vec<_>>()
        .join(" or ");
    let modules = allowed_modules(ctx, client).await;
    let module_lines = if modules.is_empty() {
        "Module values could not be discovered - ask the developer.".to_string()
    } else {
        modules.iter().map(|m| format!("- `{m}`")).collect::<Vec<_>>().join("\n")
    };
    // Cache-only: the guide must not become another request. If nothing is
    // cached yet, `get_tags` will fill it on demand.
    let tag_lines = match crate::refcache::any(&crate::refcache::tags_key(&ctx.org, &ctx.project)) {
        Some(tags) if !tags.is_empty() => {
            let shown = tags
                .iter()
                .take(GUIDE_TAG_LIMIT)
                .map(|t| format!("`{t}`"))
                .collect::<Vec<_>>()
                .join(", ");
            if tags.len() > GUIDE_TAG_LIMIT {
                format!(
                    "{shown}\n\n(showing {GUIDE_TAG_LIMIT} of {} - call `get_tags` for all of them)",
                    tags.len()
                )
            } else {
                shown
            }
        }
        _ => "Call `get_tags` for the list this project already uses.".to_string(),
    };
    format!(
        "# Writing test cases for Test Case Manager ({org}/{project})\n\n\
        Produce a JSON array of test cases. The developer imports it via the\n\
        Import File tab, reviews, then creates - you never write to Azure DevOps.\n\n\
        ## Format\n\
        Each case: `title` (required, <=255 chars), `steps` (required, each\n\
        `{{\"action\", \"expected\"}}`), `tags` (semicolon-separated, never commas),\n\
        `automation_status` (exactly {statuses}), `module` (ONLY from the list\n\
        below), `preconditions` (state, not steps), and optionally `comment` -\n\
        an in-app note that round-trips through the file but is never sent\n\
        to Azure DevOps. Include `id` ONLY to update that exact work item;\n\
        omit it to create.\n\n\
        ## Allowed Module values (live)\n{module_lines}\n\n\
        ## Tags this project already uses\n\
        Reuse these wherever one fits - a near-duplicate ('smoke-test' next to\n\
        an existing 'smoke') fragments the project's tags. A genuinely new tag\n\
        is allowed when nothing here matches.\n\n{tag_lines}\n\n\
        ## Workflow\n\
        0. Call `begin_test_case_writing` FIRST and put its questions to the\n\
        developer. Where the file goes, which specs are authoritative and what\n\
        is out of scope are theirs to decide, not yours to assume.\n\
        1. Call `get_example_cases` for the PBI you're writing for and mimic\n\
        their style and granularity.\n\
        2. Draft your cases.\n\
        3. Call `optimize_cases` with the JSON: it spells navigation out as\n\
        steps, trims expected results to the outcome, and reorders the cases so\n\
        the tester changes environment as few times as possible. Hand back the\n\
        JSON it returns.\n\
        4. Call `validate_cases` and fix every warning. For a large draft,\n\
        pass a local file via its `path` argument instead of inlining the\n\
        JSON.\n\
        5. For later edits - retagging, retitling, setting a module - call\n\
        `transform_cases` instead of rewriting the file yourself.\n",
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
    let offset = q(target, "offset").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
    // Titles-only mode exists for duplicate checking: comparing titles
    // against a PBI with 60 cases must not cost 60 cases of step text.
    let titles_only = matches!(q(target, "titles_only").as_deref(), Some("true") | Some("1"));
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
            let total = cases.len();
            // Titles are cheap: cap at 200, not 20, so one call can cover
            // a whole PBI when all the caller needs is duplicate checking.
            let page = if titles_only { limit.max(200).min(200) } else { limit };
            let records: Vec<serde_json::Value> = cases
                .iter()
                .skip(offset)
                .take(page)
                .map(|c| {
                    if titles_only {
                        serde_json::json!({ "id": c.id, "title": c.title })
                    } else {
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
                    }
                })
                .collect();
            let returned = records.len();
            let mut out = serde_json::json!({
                "test_cases": records,
                "total": total,
                "offset": offset,
            });
            if offset + returned < total {
                // Say so explicitly: a silently incomplete page is how a
                // duplicate check quietly misses cases.
                out["note"] = serde_json::json!(format!(
                    "{} of {} cases returned - pass offset={} for the next page.",
                    returned,
                    total,
                    offset + returned
                ));
            }
            (200, out.to_string())
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

/// Wiki documentation search, so the AI can ground its output in the
/// project's own docs instead of guessing.
async fn search_wiki(
    ctx: &BridgeContext,
    client: &crate::ado::AdoClient,
    target: &str,
) -> (u16, String) {
    let Some(query) = q(target, "q").filter(|s| !s.trim().is_empty()) else {
        return (400, "pass ?q=<search text>".into());
    };
    let top = q(target, "top")
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(10)
        .min(25);
    match client.search_wiki(&ctx.org, &ctx.project, &query, top).await {
        Ok(hits) => (
            200,
            serde_json::json!({
                "results": hits.iter().map(|h| serde_json::json!({
                    "file_name": h.file_name,
                    "path": h.path,
                    "wiki_name": h.wiki_name,
                    "wiki_id": h.wiki_id,
                    "highlights": h.highlights,
                })).collect::<Vec<_>>()
            })
            .to_string(),
        ),
        Err(e) => (502, format!("Azure DevOps error: {e:?}")),
    }
}

/// Full markdown content of one wiki page - call after `search_wiki` finds
/// the right `wiki_id` + `path`.
async fn wiki_page(
    ctx: &BridgeContext,
    client: &crate::ado::AdoClient,
    target: &str,
) -> (u16, String) {
    let Some(wiki_id) = q(target, "wiki").filter(|s| !s.trim().is_empty()) else {
        return (400, "pass ?wiki=<wiki id>&path=<page path>".into());
    };
    let Some(path) = q(target, "path").filter(|s| !s.trim().is_empty()) else {
        return (400, "pass ?wiki=<wiki id>&path=<page path>".into());
    };
    match client.get_wiki_page(&ctx.org, &ctx.project, &wiki_id, &path).await {
        Ok(page) => (
            200,
            serde_json::json!({ "path": page.path, "content": page.content }).to_string(),
        ),
        Err(e) => (502, format!("Azure DevOps error: {e:?}")),
    }
}

// ---------------------------------------------------------------- server

use std::sync::{Arc, Mutex as StdMutex};

/// Context shared between the TCP loop and the set_bridge_context command.
pub struct BridgeState {
    pub ctx: StdMutex<BridgeContext>,
    pub token: String,
    /// The running app's version (from tauri.conf.json via
    /// `AppHandle::package_info`), reported on `/ping` and in the handshake
    /// file so tcm-mcp can echo the real version without its own Cargo.toml
    /// needing to stay in sync.
    pub version: String,
}
pub type SharedBridge = Arc<BridgeState>;

impl BridgeState {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(ctx: BridgeContext, version: String) -> SharedBridge {
        Arc::new(BridgeState { ctx: StdMutex::new(ctx), token: new_token(), version })
    }
}

/// Optional per-request ADO client factory: the Tauri layer passes one
/// that mints a fresh token; tests pass None (ping/validate only).
pub type ClientFactory =
    Arc<dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<crate::ado::AdoClient>> + Send>> + Send + Sync>;

/// Where the app announces the running bridge to `v2.exe --mcp` proxies.
pub fn handshake_path() -> std::path::PathBuf {
    std::env::temp_dir().join("tcm-v2-mcp-bridge.json")
}

/// Bind 127.0.0.1:0, optionally write the handshake file, serve forever on
/// the tokio runtime. Returns (port, token). Requests: tiny HTTP/1.1, one
/// request per connection, 64 KiB body cap. `handshake` is Some in the real
/// app and None in tests - a test run must never clobber a live app's
/// handshake file (it did once: proxies then saw a dead port + test token).
pub async fn start_listener(
    state: SharedBridge,
    make_client: Option<ClientFactory>,
    handshake: Option<std::path::PathBuf>,
) -> Result<(u16, String), String> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let token = state.token.clone();
    if let Some(path) = handshake {
        std::fs::write(
            path,
            serde_json::json!({ "port": port, "token": token, "version": state.version }).to_string(),
        )
        .map_err(|e| e.to_string())?;
    }

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
                    route(&ctx, client.as_ref(), &method, &target, &body, &state.version).await
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
    Ok((port, token))
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
