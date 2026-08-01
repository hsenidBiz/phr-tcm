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

/// Where `/begin` announces the path the assistant is about to write to.
///
/// The developer answers "where should the finished JSON go?" in the intake
/// questions, so the app knows the exact path before a single case exists.
/// Telling the UI lets it start watching that path immediately, and the
/// watcher then folds the file into the queue the moment it appears -
/// turning "write the file, then go and import it by hand" into just
/// writing the file.
///
/// A process-wide sink rather than a parameter on `route`, because `route`
/// is the seam every bridge test calls and threading a handle through it
/// would rewrite all of them for one arm. There is exactly one bridge per
/// process (`start_bridge` holds a `running` lock), and this is `None` in
/// tests, which is what keeps a test run from emitting into a live app.
type IntakeSink = Box<dyn Fn(String) + Send + Sync>;
static INTAKE_SINK: std::sync::OnceLock<IntakeSink> = std::sync::OnceLock::new();

/// Called once by the app when the bridge starts. Later calls are ignored.
pub fn set_intake_sink(f: IntakeSink) {
    let _ = INTAKE_SINK.set(f);
}

fn announce_intake_path(path: &str) {
    if let Some(f) = INTAKE_SINK.get() {
        f(path.to_string());
    }
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
        ("GET", "/test-cases") => match client {
            Some(c) => test_cases(ctx, c, target).await,
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
    // Warnings are not failures - a long title or a comma in a tag is worth
    // saying and not worth refusing over - but they must reach the caller,
    // because some of them mean a case was dropped.
    let (cases, import_warnings) = match parse_cases_with_warnings(body) {
        Ok(v) => v,
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
                "import_warnings": import_warnings,
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
            // Anything the importer could not read. A case it skipped is
            // simply not in the output, so silence here read as success
            // over a draft that had quietly got shorter.
            "import_warnings": import_warnings,
            "note": "Hand this JSON to the developer as the file to import. The report explains what was reordered and why. Check import_warnings - a case listed there was NOT read and is not in this output.",
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
    let (cases, import_warnings) = match parse_cases_with_warnings(&draft) {
        Ok(v) => v,
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
            "import_warnings": import_warnings,
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
    // The tri-state is kept, not flattened. `.known()` returns an empty
    // list for BOTH "this organization has no Module field" and "the
    // request for its values failed", and `problems` skips the module
    // check entirely on an empty list - so a failed fetch silently turned
    // off the very check this tool exists for and still said "ready".
    let allowed = match client {
        Some(c) => allowed_modules(ctx, c).await,
        None => Modules::Unavailable("not signed in to Test Case Manager".into()),
    };
    let modules: Vec<String> = allowed.known().to_vec();

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

    // Not a `problem` - those block, and a transient fetch failure must not
    // stop a developer whose module is perfectly correct. But it has to be
    // SAID, or "ready" claims a check that never ran.
    let mut unchecked: Vec<String> = vec![];
    if let Modules::Unavailable(why) = &allowed {
        if !answers.module.trim().is_empty() {
            unchecked.push(format!(
                "module '{}' could not be checked against this organization's allowed values ({why}) - confirm it with the developer.",
                answers.module.trim()
            ));
        }
    }

    // The answers are validated by here - `output_path` ends in .json and
    // its folder exists - so this is a real place a file is about to
    // appear. Watching starts now, before the assistant has written a line.
    announce_intake_path(answers.output_path.trim());

    let plan = crate::intake::plan_markdown(&answers, &feature);
    let plan_path = crate::intake::plan_path(&answers.output_path);
    // `fs::write` TRUNCATES, and this path is derived from a name the
    // developer typed - so it can land on a file that was never ours.
    // Re-running begin after refining the answers has to keep working, so
    // one of our own plans is replaced; anything else is left alone.
    // Blowing away somebody's notes to leave a plan in their place is not
    // a trade this tool gets to make on its own.
    let refused = matches!(
        std::fs::read_to_string(&plan_path),
        Ok(existing) if !existing.starts_with(crate::intake::PLAN_HEADING)
    );
    let written = !refused && std::fs::write(&plan_path, &plan).is_ok();
    (
        200,
        serde_json::json!({
            "status": "ready",
            "unchecked": unchecked,
            "plan": plan,
            "plan_path": if written { serde_json::json!(plan_path) } else { serde_json::Value::Null },
            "plan_write_error": if written {
                serde_json::Value::Null
            } else if refused {
                serde_json::json!(format!(
                    "{plan_path} already exists and was not written by this tool, so it was left \
                     untouched - the plan is in this response instead. Ask the developer for a \
                     different output_path if it should be saved."
                ))
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
            // Text that a round trip through Azure DevOps will read as
            // markup and remove. `<cycleId>` and friends now survive, but a
            // spec quote naming a real element - "<div>", "<img>" - is
            // genuinely indistinguishable from the rich-text editor's own
            // output, so the author gets told here instead of finding out
            // by diffing an export against their own source.
            for (i, tc) in cases.iter().enumerate() {
                let mut hits: Vec<String> = Vec::new();
                for (n, s) in tc.steps.iter().enumerate() {
                    for (what, text) in [("action", &s.action), ("expected", &s.expected)] {
                        if crate::steps_xml::contains_html_markup(text) {
                            hits.push(format!("step {} {what}", n + 1));
                        }
                    }
                }
                if crate::steps_xml::contains_html_markup(&tc.preconditions) {
                    hits.push("preconditions".to_string());
                }
                if !hits.is_empty() {
                    warnings.push(format!(
                        "Test case {} ('{}'): {} contains an HTML tag name in angle brackets, \
                         which Azure DevOps stores as markup and will drop on the way back. \
                         Placeholders like <cycleId> are safe; a literal element name is not - \
                         write it as `code`, or in braces.",
                        i + 1,
                        tc.title,
                        hits.join(", ")
                    ));
                }

                // A negative folded into its positive is covered but not
                // visible: nobody auditing by title can see it was tested.
                if let Some(why) = crate::branchcheck::both_branches_reason(tc) {
                    warnings.push(format!(
                        "Test case {} ('{}') appears to cover both branches of a condition - {}. \
                         Consider splitting it into a positive and a negative, each with a title \
                         stating its own branch. If the transition IS the behaviour under test, \
                         leave it.",
                        i + 1,
                        tc.title,
                        why
                    ));
                }
            }
            if let Some(c) = client {
                let allowed = allowed_modules(ctx, c).await;
                // Say so rather than pass silently: "no warnings" has to
                // mean "checked and fine", not "could not look".
                if let Modules::Unavailable(why) = &allowed {
                    warnings.push(format!(
                        "Module values could not be read from Azure DevOps ({why}), so the                          Module on each case was NOT checked. Everything else was."
                    ));
                }
                let known = allowed.known();
                if !known.is_empty() {
                    for (i, tc) in cases.iter().enumerate() {
                        let m = tc.module_value.trim();
                        if !m.is_empty() && !known.iter().any(|a| a.eq_ignore_ascii_case(m)) {
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
/// What is known about the Module values a case may carry.
///
/// Three states, not one list. Collapsing them lost the only one that
/// matters: a failed lookup used to come back as an empty Vec, which every
/// caller read as "no constraint to check", so validate_cases answered a
/// dropped connection or a 403 with a clean bill of health.
pub enum Modules {
    /// This organization has no Module field at all.
    NotConfigured,
    /// The values a case may carry. Empty means the field is free text.
    Known(Vec<String>),
    /// The lookup failed, so nothing can be said about Module either way.
    Unavailable(String),
}

async fn allowed_modules(ctx: &BridgeContext, client: &crate::ado::AdoClient) -> Modules {
    let Some(fref) = &ctx.module_ref else {
        return Modules::NotConfigured;
    };
    match client
        .get_field_allowed_values(&ctx.org, &ctx.project, "Test Case", fref)
        .await
    {
        // A picklist with entries is the answer.
        Ok(list) if !list.is_empty() => Modules::Known(list),
        // No picklist: the field is free text, so fall back to what the
        // project actually uses. That call failing is still a failure.
        Ok(_) => match client.field_values_in_use(&ctx.org, &ctx.project, fref).await {
            Ok(used) => Modules::Known(used),
            Err(e) => Modules::Unavailable(e.to_string()),
        },
        Err(e) => Modules::Unavailable(e.to_string()),
    }
}

impl Modules {
    /// The values to check against, when there are any to check against.
    pub fn known(&self) -> &[String] {
        match self {
            Modules::Known(v) => v,
            _ => &[],
        }
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
    let module_lines = match allowed_modules(ctx, client).await {
        Modules::Known(v) if !v.is_empty() => {
            v.iter().map(|m| format!("- `{m}`")).collect::<Vec<_>>().join("\n")
        }
        Modules::Known(_) | Modules::NotConfigured => {
            "This organization has no Module values to choose from - leave it blank.".to_string()
        }
        // Deliberately not the same sentence as above: one says there is
        // nothing to pick, the other says we could not find out. An
        // assistant told the first will confidently leave Module blank.
        Modules::Unavailable(why) => format!(
            "Module values could not be read from Azure DevOps ({why}) - ask the developer \
             rather than guessing."
        ),
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
        ## reviewer_notes\n\
        Optional, also never sent to Azure DevOps, and the most useful thing\n\
        you can add - but ONLY if it stays short. It is a POINTER to where\n\
        the requirement lives, not an explanation of it. A reviewer reads one\n\
        of these per case while checking dozens, so a paragraph costs more\n\
        time than it saves and buries the citation that made it worth\n\
        reading.\n\n\
        ONE OR TWO LINES. Cite the source and stop:\n\
        - `Spec: Step10-ManagePerformanceCycle.md 7.7 (AC-3)`\n\
        - `Code: IndexModel.CanCopyFromPreviousCycle`\n\
        - `Out of scope: SSO` - only when you deliberately left something out\n\
        - one short quote ONLY when the exact wording IS the requirement\n\n\
        Do NOT restate the test, explain your reasoning, describe what the\n\
        app does, or justify a decision at length. The steps already say what\n\
        is being tested; anything longer is prose the reviewer must read\n\
        before reaching the reference they actually wanted. If a case really\n\
        does need an argument made, that is a `comment`, not this.\n\n\
        Cite; never paraphrase from memory. Rendered as MARKDOWN, so a wiki\n\
        link works - but a bare citation line needs no formatting at all.\n\n\
        ## One branch per case\n\
        When the spec says a thing is shown ONLY when X, that is two test\n\
        cases, not one. Write the positive and the negative separately, each\n\
        with a title stating its own branch, and have each name its\n\
        counterpart in `reviewer_notes` so neither reads as a duplicate.\n\n\
        Never fold the negative in as an extra step of the positive - do not\n\
        turn a setting off half way through a case to check the other branch.\n\
        It is covered that way, but it is INVISIBLE: someone auditing titles\n\
        against a large backlog cannot see the negative was tested, and the\n\
        case leaves the environment different from how it found it.\n\
        `validate_cases` warns when a case looks like both branches at once.\n\n\
        Two things a merged case hides, both real: a title claiming a button\n\
        is hidden from the Manager AND the Reviewer when the case only ever\n\
        signs in as one of them, and a weak negative - one unrated goal out\n\
        of two, where four out of five would have caught an implementation\n\
        that passes on any rating.\n\n\
        ## Allowed Module values (live)\n{module_lines}\n\n\
        ## Tags this project already uses\n\
        Reuse these wherever one fits - a near-duplicate ('smoke-test' next to\n\
        an existing 'smoke') fragments the project's tags. A genuinely new tag\n\
        is allowed when nothing here matches.\n\n{tag_lines}\n\n\
        ## Workflow\n\
        0. Call `begin_test_case_writing` FIRST and put its questions to the\n\
        developer. Where the file goes, which specs are authoritative and what\n\
        is out of scope are theirs to decide, not yours to assume.\n\
        1. Call `get_test_cases` for the PBI you're writing for and mimic\n\
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
async fn test_cases(
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
            // Titles are cheap: a fixed 200, not the caller's limit, so one
            // call can cover a whole PBI when all it needs is duplicate
            // checking. (Was written `limit.max(200).min(200)`, which is
            // the same 200 by a longer route - and a deny-level clippy lint,
            // because that shape is usually a mistake rather than a
            // deliberate constant.)
            let page = if titles_only { 200 } else { limit };
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
                // Read until headers+body are complete (or the cap). Every
                // read is bounded: a client that opens a connection, sends
                // half a request and stops used to hold this task and its
                // socket for the life of the process, one per attempt.
                let deadline = std::time::Duration::from_secs(15);
                let parsed = loop {
                    let read = tokio::time::timeout(deadline, sock.read(&mut buf[used..])).await;
                    let Ok(Ok(n)) = read else { return };
                    if n == 0 { return; }
                    used += n;
                    match parse_http(&buf[..used]) {
                        Parsed::Complete { method, target, token, body } => {
                            break (method, target, token, body)
                        }
                        // Answer rather than close in silence: the proxy on
                        // the other end is waiting on a response, and "the
                        // connection went away" tells it nothing it can act
                        // on or show the user.
                        Parsed::Malformed => {
                            let _ = sock
                                .write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                                .await;
                            return;
                        }
                        Parsed::Incomplete => {}
                    }
                    if used >= buf.len() {
                        let _ = sock
                            .write_all(b"HTTP/1.1 413 Payload Too Large\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                            .await;
                        return;
                    }
                };
                let (method, target, tok, body) = parsed;
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

/// What a buffer of bytes off the socket amounts to so far.
///
/// The old version returned an Option, which conflated the two ways of not
/// having a request: "keep reading" and "this will never be one". A
/// malformed header read as "keep reading", so the connection sat there
/// until the 64 KB cap - or, if the client simply stopped sending, until
/// the end of the process, holding a task and a socket per attempt.
pub enum Parsed {
    Complete { method: String, target: String, token: Option<String>, body: String },
    Incomplete,
    Malformed,
}

/// Parse a request from the raw BYTES.
///
/// Offsets used to come from `String::from_utf8_lossy(raw)` and then index
/// `raw`. Those are not the same string: every invalid byte becomes a
/// three-byte U+FFFD, so one bad byte anywhere in the headers slid the body
/// offset and the request was read from the wrong place. The head is found
/// by byte, and only then decoded.
pub fn parse_http(raw: &[u8]) -> Parsed {
    let Some(head_end) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
        // No blank line yet. A request line is short; if this much has
        // arrived without one, it is not HTTP.
        return if raw.len() > 16 * 1024 { Parsed::Malformed } else { Parsed::Incomplete };
    };
    let Ok(head) = std::str::from_utf8(&raw[..head_end]) else {
        return Parsed::Malformed; // headers are not text
    };
    let mut lines = head.lines();
    let Some(mut req) = lines.next().map(str::split_whitespace) else {
        return Parsed::Malformed;
    };
    let (Some(method), Some(target)) = (req.next(), req.next()) else {
        return Parsed::Malformed;
    };
    let mut token = None;
    let mut content_len = 0usize;
    for line in lines {
        // A header we cannot read is not a reason to reject the request -
        // only Content-Length has to be right, because it decides where
        // the body ends.
        let Some((k, v)) = line.split_once(':') else { continue };
        let v = v.trim();
        if k.eq_ignore_ascii_case("x-bridge-token") {
            token = Some(v.to_string());
        }
        if k.eq_ignore_ascii_case("content-length") {
            match v.parse() {
                Ok(n) => content_len = n,
                Err(_) => return Parsed::Malformed,
            }
        }
    }
    let body_start = head_end + 4;
    let Some(end) = body_start.checked_add(content_len).filter(|e| *e <= raw.len()) else {
        return Parsed::Incomplete; // body still arriving
    };
    Parsed::Complete {
        method: method.to_string(),
        target: target.to_string(),
        token,
        body: String::from_utf8_lossy(&raw[body_start..end]).to_string(),
    }
}
