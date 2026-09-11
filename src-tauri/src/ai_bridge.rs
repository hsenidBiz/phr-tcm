//! The AI bridge: a 127.0.0.1-only service that lets the tcm-mcp stdio
//! binary (and therefore AI tools) read a live writing guide, fetch real
//! example cases, reorganise a draft into a run sheet, and search PBIs.
//! Reads and pure transforms ONLY - no writes, no raw ADO passthrough, and
//! the bearer token never leaves the app. Modeled on note_server.rs; the
//! TCP loop is thin, all logic lives in `route` so tests need no sockets.

use rand::RngExt;
use serde::Serialize;

static INTAKE_SINK: std::sync::OnceLock<IntakeSink> = std::sync::OnceLock::new();

/// How many tag names the writing guide inlines before it stops and
/// points at the dedicated tool. Long enough to be genuinely useful,
/// short enough not to drown the guide.
const GUIDE_TAG_LIMIT: usize = 60;

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
    /// The working repository picked on the AI Bridge tab, or None when
    /// none is set - in which case a writing job cannot start, because
    /// there is nowhere agreed for its file to go.
    pub working_dir: Option<String>,
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
        ("POST", "/check-coverage") => check_coverage_route(body).await,
        ("POST", "/merge-cases") => merge_cases_route(body, ctx),
        ("POST", "/begin") => begin_writing(body, target, ctx, client).await,
        ("GET", "/guide") => match client {
            Some(c) => (200, guide(ctx, c).await),
            None => (503, "sign in to Test Case Manager first".into()),
        },
        ("GET", "/test-cases") => match client {
            Some(c) => test_cases(ctx, c, target).await,
            None => (503, "sign in to Test Case Manager first".into()),
        },
        ("GET", "/suites") => match client {
            Some(c) => suites(ctx, c, target).await,
            None => (503, "sign in to Test Case Manager first".into()),
        },
        ("GET", "/suite-cases") => match client {
            Some(c) => suite_cases(ctx, c, target).await,
            None => (503, "sign in to Test Case Manager first".into()),
        },
        ("GET", "/tags") => tags(ctx, client, target).await,
        // Both autorun routes deliberately ignore `client`: one documents
        // a format, the other writes local files. Neither reaches Azure
        // DevOps, so neither should demand a sign-in first.
        ("GET", "/autorun-guide") => (200, crate::autorun::guide::autorun_guide()),
        ("POST", "/autorun-script") => save_autorun_scripts(body),
        // Findings are local app data: no client needed, like the autorun
        // routes - a problem noticed while signed out is still a problem.
        ("POST", "/findings") => record_finding(ctx, body),
        ("GET", "/findings") => list_findings(ctx, target),
        // The proxy asks for this before listing tools, so a toggle in the
        // app takes effect on the assistant's next tools/list.
        ("GET", "/tools") => (
            200,
            serde_json::json!({ "disabled": ctx.disabled_tools }).to_string(),
        ),
        ("GET", "/run-failures") => match client {
            Some(c) => run_failures(ctx, c, target).await,
            None => (503, "sign in to Test Case Manager first".into()),
        },
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
        _ if smells_like_a_write(method, target) => (403, WRITE_REFUSAL.into()),
        _ => (404, String::new()),
    }
}

/// The exact sentence an assistant is handed when it tries to write. One
/// constant, shared with the MCP layer's unknown-tool path, so the two
/// doors give the same answer.
pub const WRITE_REFUSAL: &str = "This action is not possible and must be done through the app itself. Giving an automated agent access to directly edit or create cases can be unsafe. This bridge only reads from Azure DevOps - draft cases into the watched JSON file and a person imports them from the app.";

/// A request that was going to 404 anyway, but whose shape says the
/// assistant wanted to WRITE - a mutating verb, or a path named after
/// one. Those deserve the refusal sentence rather than a bare 404,
/// because "not found" invites the assistant to retry with a different
/// spelling; "not possible, by design" ends the attempt.
fn smells_like_a_write(method: &str, target: &str) -> bool {
    if matches!(method, "PUT" | "PATCH" | "DELETE") {
        return true;
    }
    let path = target.split('?').next().unwrap_or("").to_ascii_lowercase();
    ["create", "update", "delete", "edit", "write", "add-", "remove", "submit"]
        .iter()
        .any(|w| path.contains(w))
}

/// Save one or many Auto Run action scripts, as an assistant writes them.
///
/// A BUNDLE by design: the body is an array, so a whole PBI's worth of
/// cases lands in one call - and the same shape is what the Auto Run
/// screen's Import button reads from a file. One case is a bundle of one.
///
/// ALL OR NOTHING, for real: parsing is one gate, but
/// `store::save_scripts_atomically` is the one that actually makes the
/// claim true - it validates and serialises every entry before a single
/// file is written, so a bad case id or a filesystem error on entry 16 of
/// 30 can never leave the other 29 half-applied. An unknown action `kind`
/// fails here rather than mid-run, with the browser already open in front
/// of them.
fn save_autorun_scripts(body: &str) -> (u16, String) {
    let scripts: Vec<crate::autorun::CaseScript> = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => {
            return (
                400,
                format!(
                    "that is not a list of action scripts: {e}. Expected an array of {{ case_id, title, steps: [{{ step_number, actions }}] }} - call get_autorun_guide for the format."
                ),
            )
        }
    };
    if scripts.is_empty() {
        return (400, "no scripts in the bundle".to_string());
    }

    let Some(root) = crate::autorun::store::configured_root() else {
        // `set_root` runs exactly once, during app setup, and only when
        // `app_data_dir()` resolves - so `None` here is not something
        // this process will ever recover from on its own; "try again"
        // would never help. Refuse rather than invent a path: a script
        // written somewhere the app does not read would look saved and
        // never appear.
        return (
            503,
            "the app could not set up its data directory this session - restart the app"
                .to_string(),
        );
    };

    match crate::autorun::store::save_scripts_atomically(&root, &scripts) {
        Ok(()) => {
            let saved: Vec<String> = scripts.iter().map(|sc| sc.case_id.to_string()).collect();
            crate::applog::info(format!("AI saved {} auto-run script(s)", saved.len()));
            (
                200,
                serde_json::json!({
                    "saved": saved.len(),
                    "case_ids": saved,
                    "note": "Open Auto Run in the app - these cases now show a Run button.",
                })
                .to_string(),
            )
        }
        Err(crate::autorun::store::SaveScriptsError::Invalid(e)) => (400, e),
        Err(crate::autorun::store::SaveScriptsError::Io(e)) => {
            (500, format!("could not save the bundle: {e}"))
        }
    }
}

/// The store root as app setup published it; `None` outside the app.
fn findings_root() -> Result<std::path::PathBuf, (u16, String)> {
    crate::findings::configured_root().ok_or((
        503,
        "Test Case Manager has not finished starting - the findings store has no location yet".into(),
    ))
}

/// `record_finding`: one note about one thing, for the open org and project.
fn record_finding(ctx: &BridgeContext, body: &str) -> (u16, String) {
    let root = match findings_root() {
        Ok(r) => r,
        Err(e) => return e,
    };
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => {
            return (
                400,
                format!("that is not a finding: {e}. Expected {{ kind: test_case|spec|code, subject, title, detail }}."),
            )
        }
    };
    let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
    match crate::findings::record(
        &root,
        crate::findings::NewFinding {
            org: ctx.org.clone(),
            project: ctx.project.clone(),
            kind: s("kind"),
            subject: s("subject"),
            title: s("title"),
            detail: s("detail"),
        },
    ) {
        Ok(f) => {
            crate::applog::info(format!("AI recorded a {} finding: {}", f.kind, f.title));
            let open = crate::findings::list_open(&root, &ctx.org, &ctx.project).len();
            (200, serde_json::json!({ "id": f.id, "open": open }).to_string())
        }
        Err(e) => (400, e),
    }
}

/// `list_findings`: open by default; `status=resolved` or `status=all`.
fn list_findings(ctx: &BridgeContext, target: &str) -> (u16, String) {
    let root = match findings_root() {
        Ok(r) => r,
        Err(e) => return e,
    };
    let want = q(target, "status").unwrap_or_else(|| "open".into());
    let all = crate::findings::list(&root, &ctx.org, &ctx.project);
    let rows: Vec<&crate::findings::Finding> =
        all.iter().filter(|f| want == "all" || f.status == want).collect();
    (200, serde_json::json!({ "findings": rows, "total": rows.len() }).to_string())
}

/// Reorganise a draft into a run sheet: navigation spelled out as steps,
/// cases ordered so the tester switches environment as little as
/// possible, expected results reduced to the outcome. Azure DevOps is
/// never touched.
///
/// Round 7 §1: the draft may come as a `path` instead of inline JSON -
/// the run-sheet ordering is inherently GLOBAL (one tester_order across
/// the whole set), so this was the one tool where "too big to inline"
/// could not be chunked around: five shards give five sequences all
/// starting at 1, a wrong answer that looks like an answer. `path` and
/// `in_place` match transform_cases exactly; with `in_place: true` the
/// response is the report alone, which removes the return half of the
/// cost as well as the send half.
fn optimize_json(body: &str, target: &str) -> (u16, String) {
    let from_path = q(target, "path").filter(|p| !p.trim().is_empty());
    let in_place = matches!(q(target, "in_place").as_deref(), Some("true") | Some("1"));
    if from_path.is_some() && !body.trim().is_empty() {
        return (
            400,
            serde_json::json!({ "error": "pass the draft as \"json\" OR as \"path\", not both - \
                a silently preferred source is how the wrong draft gets optimized." })
            .to_string(),
        );
    }
    if in_place && from_path.is_none() {
        return (
            400,
            serde_json::json!({ "error": "in_place needs a \"path\" - there is no file to write back to." })
                .to_string(),
        );
    }
    // Warnings are not failures - a long title or a comma in a tag is worth
    // saying and not worth refusing over - but they must reach the caller,
    // because some of them mean a case was dropped.
    let (cases, import_warnings) = match &from_path {
        Some(path) => {
            if !std::path::Path::new(path).is_file() {
                return (
                    400,
                    serde_json::json!({ "error": format!("{path} does not exist or is not a file") })
                        .to_string(),
                );
            }
            match crate::import_parser::parse_file(path) {
                Ok(v) => v,
                Err(e) => return (400, serde_json::json!({ "error": e }).to_string()),
            }
        }
        None => match parse_cases_with_warnings(body) {
            Ok(v) => v,
            Err(e) => return (400, serde_json::json!({ "error": e }).to_string()),
        },
    };
    let entry = q(target, "entry");
    let dry_run = matches!(q(target, "dry_run").as_deref(), Some("true") | Some("1"));
    // Default true: regrouping for the tester is what this tool is mostly
    // for. `reorder=false` is what a spec-ordered set passes, so it still
    // gets the navigation and expected-result work without being shuffled.
    let reorder = !matches!(q(target, "reorder").as_deref(), Some("false") | Some("0"));
    // Default true as well. Round 8 §14: an arithmetic set wants the
    // navigation and ordering work WITHOUT its expected results rewritten,
    // and welding the two together made the whole tool unusable there - the
    // reported workaround was to skip it entirely and lose both halves.
    let trim_expected =
        !matches!(q(target, "trim_expected").as_deref(), Some("false") | Some("0"));
    let (optimized, report) =
        crate::optimize::optimize_full(cases, entry.as_deref(), reorder, trim_expected);
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
    if in_place {
        // Temp-in-same-directory + rename, like transform_cases: a half-
        // written draft under a watched path is worse than no write.
        let path = from_path.expect("guarded above");
        let tmp = format!("{path}.tmp");
        if let Err(e) = std::fs::write(&tmp, &json) {
            let _ = std::fs::remove_file(&tmp);
            return (500, serde_json::json!({ "error": format!("could not write {tmp}: {e}") }).to_string());
        }
        if let Err(e) = std::fs::rename(&tmp, &path) {
            let _ = std::fs::remove_file(&tmp);
            return (500, serde_json::json!({ "error": format!("could not replace {path}: {e}") }).to_string());
        }
        return (
            200,
            serde_json::json!({
                "written_to": path,
                "cases": optimized.len(),
                "report": report,
                "import_warnings": import_warnings,
                "note": "The optimized draft was written back in place - no JSON is echoed. Every case now carries spec_order and tester_order; the app's file watch will pick the change up.",
            })
            .to_string(),
        );
    }
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
            "note": "Hand this JSON to the developer as the file to import. Every case now carries spec_order and tester_order - keep both fields exactly as set; the app flips the queue between the two readings. The report explains what was reordered and why. Check import_warnings - a case listed there was NOT read and is not in this output.",
        })
        .to_string(),
    )
}

/// Apply declarative edits to a draft - the restructuring an assistant
/// would otherwise write a throwaway script for.
///
/// Round 5 §10: the draft may come as a `path` instead of inline JSON -
/// the same escape `validate_cases` has, because a 245 KB draft inlined
/// both ways is a 489 KB round-trip to reword some steps, and sharding it
/// forces the hand-rolled reassembly the guide forbids. `path` and inline
/// together is a 400 naming both - the old behaviour, building the result
/// from the inline draft while the path sat ignored, is the exact silent-
/// discard §15 condemns. `in_place: true` writes the transformed draft
/// back to the path atomically and skips echoing the JSON, which for a
/// bulk retag is the whole cost.
fn transform_json(body: &str) -> (u16, String) {
    let doc: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return (400, serde_json::json!({ "error": format!("invalid JSON: {e}") }).to_string()),
    };
    let from_path = doc["path"].as_str().filter(|p| !p.trim().is_empty()).map(str::to_string);
    let has_inline = !doc["test_cases"].is_null();
    let in_place = doc["in_place"].as_bool().unwrap_or(false);
    if from_path.is_some() && has_inline {
        return (
            400,
            serde_json::json!({ "error": "pass the draft as \"json\" OR as \"path\", not both - \
                a silently preferred source is how edits land on the wrong draft." })
            .to_string(),
        );
    }
    if in_place && from_path.is_none() {
        return (
            400,
            serde_json::json!({ "error": "in_place needs a \"path\" - there is no file to write back to." })
                .to_string(),
        );
    }

    let (cases, import_warnings) = match &from_path {
        Some(path) => {
            if !std::path::Path::new(path).is_file() {
                return (
                    400,
                    serde_json::json!({ "error": format!("{path} does not exist or is not a file") })
                        .to_string(),
                );
            }
            match crate::import_parser::parse_file(path) {
                Ok(v) => v,
                Err(e) => return (400, serde_json::json!({ "error": e }).to_string()),
            }
        }
        None => {
            // The draft arrives as a JSON *string* (the tool's `json`
            // argument), but a caller posting the array inline works too.
            let draft = match &doc["test_cases"] {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Null => {
                    return (
                        400,
                        serde_json::json!({ "error": "empty draft - pass the JSON as \"json\", or a local file via \"path\" for large drafts" })
                            .to_string(),
                    )
                }
                other => other.to_string(),
            };
            match parse_cases_with_warnings(&draft) {
                Ok(v) => v,
                Err(e) => return (400, serde_json::json!({ "error": e }).to_string()),
            }
        }
    };
    let (ops, mut ignored) = match crate::transform::parse_ops_full(&doc["operations"]) {
        Ok(o) => o,
        Err(e) => return (400, serde_json::json!({ "error": e }).to_string()),
    };
    // Body arguments this route does not read - echoed, not swallowed.
    if let Some(obj) = doc.as_object() {
        for k in obj.keys() {
            if !["test_cases", "operations", "path", "in_place"].contains(&k.as_str()) {
                ignored.push(format!("request argument \"{k}\" is not read by transform_cases."));
            }
        }
    }
    let (out, mut report) = crate::transform::apply(cases, &ops);
    report.ignored.extend(ignored);
    let json = match crate::import_parser::queue_to_json_string(&out) {
        Ok(j) => j,
        Err(e) => return (500, serde_json::json!({ "error": e }).to_string()),
    };

    if in_place {
        // Temp-in-same-directory + rename, like merge_case_files: a half-
        // written draft under a watched path is worse than no write.
        let path = from_path.expect("guarded above");
        let tmp = format!("{path}.tmp");
        if let Err(e) = std::fs::write(&tmp, &json) {
            let _ = std::fs::remove_file(&tmp);
            return (500, serde_json::json!({ "error": format!("could not write {tmp}: {e}") }).to_string());
        }
        if let Err(e) = std::fs::rename(&tmp, &path) {
            let _ = std::fs::remove_file(&tmp);
            return (500, serde_json::json!({ "error": format!("could not replace {path}: {e}") }).to_string());
        }
        return (
            200,
            serde_json::json!({
                "written_to": path,
                "cases": out.len(),
                "report": report,
                "import_warnings": import_warnings,
                "note": "The transformed draft was written back in place - no JSON is echoed. The app's file watch will pick the change up.",
            })
            .to_string(),
        );
    }

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

    // No repository, no job: the whole point of the intake is agreeing
    // where the file goes, and that place is now `<repo>/.test-cases`.
    let root = ctx
        .working_dir
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from);
    let Some(root) = root else {
        return (
            409,
            serde_json::json!({
                "status": "blocked",
                "error": "No working repository is set in Test Case Manager. Ask the developer to open the AI Bridge tab and pick the repository these cases belong to, then call this tool again.",
            })
            .to_string(),
        );
    };
    let cases_dir = match crate::workspace::ensure_cases_dir(&root) {
        Ok(d) => d,
        Err(e) => {
            return (500, serde_json::json!({ "status": "error", "error": e }).to_string());
        }
    };
    let cases_dir_str = cases_dir.to_string_lossy().to_string();

    // Phase 1: nothing sent, so hand back the questions.
    if body.trim().is_empty() || body.trim() == "{}" {
        return (
            200,
            serde_json::json!({
                "status": "questions",
                "feature": feature,
                "ask_the_developer": crate::intake::questions_for(Some(&cases_dir_str)),
                "context": {
                    "organization": ctx.org,
                    "project": ctx.project,
                    "allowed_modules": modules,
                    "output_dir": cases_dir_str,
                    "suggested_output_path": crate::workspace::default_output_path(&root, &feature),
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

    let mut answers: crate::intake::IntakeAnswers = match serde_json::from_str(body) {
        Ok(a) => a,
        Err(e) => {
            return (
                400,
                serde_json::json!({ "status": "error", "error": format!("could not read the answers: {e}") })
                    .to_string(),
            )
        }
    };

    answers.output_path = crate::workspace::resolve_output(&root, &answers.output_path);
    let problems = crate::intake::problems_in(&answers, &modules, Some(&cases_dir));
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
    // Advice, not a gate: `None` (never an error) when no spec file could
    // be read at all, which must not block an otherwise-ready intake.
    let scale = crate::intake::job_scale(&answers.spec_paths, &answers.sections);
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
            "scale": scale,
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
            "note": "Show this plan to the developer and say, in so many words: check the \
                     plan and tell me if anything needs changing, or say to go ahead. Do \
                     not write a single case until they answer. Then write only what the \
                     plan covers, put the JSON exactly at output_path, and follow the \
                     steps at the end of the plan.",
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
/// A reviewer note that reports a problem rather than a provenance. The
/// phrases are the ones assistants actually used when they wrote defects
/// into notes; a hit is an advisory, never a block.
fn finding_like_note(notes: &str) -> Option<String> {
    const PHRASES: [&str; 10] = [
        "contradict", "does not match", "doesn't match", "mismatch", "discrepan",
        "inconsisten", "bug:", "defect", "the code does not", "spec says",
    ];
    let lower = notes.to_lowercase();
    PHRASES
        .iter()
        .find(|p| lower.contains(*p))
        .map(|p| format!("it says \"{p}\""))
}

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
            // Judgement calls, kept OUT of `warnings` so "fix every
            // warning" stays followable as written.
            let mut advisories: Vec<String> = Vec::new();
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
                //
                // An ADVISORY, not a warning - round 3 feedback. The tool's
                // own instruction is "fix every warning", and a judgement
                // call that cannot be cleanly cleared devalues the channel
                // it shares: authors learn to skim past all of it,
                // including the import-blocking warnings that were always
                // reliable.
                if let Some(why) = crate::branchcheck::both_branches_reason(tc) {
                    advisories.push(format!(
                        "Test case {} ('{}') appears to cover both branches of a condition - {}. \
                         Consider splitting it into a positive and a negative, each with a title \
                         stating its own branch. If the transition IS the behaviour under test, \
                         leave it.",
                        i + 1,
                        tc.title,
                        why
                    ));
                }

                // A `Spec:` citation with no verbatim quote and no exemption
                // is a judgement call, not a defect the importer can catch -
                // same register as the both-branches check above. Uses
                // `speccov::parse_citations` (the one parser Task 1 built)
                // rather than a second regex over `reviewer_notes`, so the
                // guide's citation grammar and this check can never drift
                // apart. A code-only citation (no `specs` at all) never
                // trips this - the rule is about quoting a SPEC, not code.
                if let Some(citations) = crate::speccov::parse_citations(&tc.reviewer_notes) {
                    if citations.specs.iter().any(|s| s.quote.is_none() && s.exemption.is_none()) {
                        advisories.push(format!(
                            "Test case {} ('{}') cites a Spec: section with no verbatim quote and \
                             no exemption - {}.",
                            i + 1,
                            tc.title,
                            crate::speccov::bare_citation_hint(&tc.reviewer_notes)
                        ));
                    }
                }

                // The two human fields. A comment on a case with NO id was
                // written by the assistant - a case with an id may carry the
                // developer's own, round-tripped through the file - and a
                // note that reports a problem is a finding in the wrong
                // place. Both judgement calls: said, not blocked.
                if tc.update_id.is_none() && !tc.comment.trim().is_empty() {
                    advisories.push(format!(
                        "Test case {} ('{}') carries a `comment`. That field is the developer's and \
                         an assistant never writes it. If this is a problem you found, remove it here \
                         and call record_finding.",
                        i + 1,
                        tc.title
                    ));
                }
                if let Some(why) = finding_like_note(&tc.reviewer_notes) {
                    advisories.push(format!(
                        "Test case {} ('{}'): its reviewer_notes read like a problem report ({why}). \
                         reviewer_notes say only what the case checks and where the requirement \
                         lives; a problem is a finding - call record_finding and take it out of the note.",
                        i + 1,
                        tc.title
                    ));
                }
            }
            if let Some(c) = client {
                let allowed = allowed_modules(ctx, c).await;
                // Say so rather than pass silently: "no warnings" has to
                // mean "checked and fine", not "could not look".
                if let Modules::Unavailable(why) = &allowed {
                    warnings.push(format!(
                        "Module values could not be read from Azure DevOps ({why}), so the Module on each case was NOT checked. Everything else was."
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
            let mut doc = serde_json::json!({
                "cases": cases.len(),
                "warnings": warnings,
                "error": serde_json::Value::Null,
            });
            // Only present when there are any, and self-describing: the
            // register matters. A warning must be fixed; an advisory must
            // be READ, decided, and the decision said out loud.
            if !advisories.is_empty() {
                doc["advisories"] = serde_json::json!(advisories);
                doc["advisories_note"] = serde_json::json!(
                    "Advisories are judgement calls, not defects: read each one, decide, and \
                     tell the developer what you decided and why. They do not need to be \
                     'fixed' the way warnings do."
                );
            }
            doc
        }
        Err(e) => serde_json::json!({ "cases": 0, "warnings": [], "error": e }),
    };
    out.to_string()
}

/// Request body for `/check-coverage`: which draft to score against which
/// spec documents. `path` and `json` are a strict XOR - never both, never
/// neither - see `check_coverage_route`'s doc comment for why.
#[derive(serde::Deserialize, Default)]
struct CoverageRequest {
    json: Option<String>,
    path: Option<String>,
    #[serde(default)]
    spec_paths: Vec<String>,
    #[serde(default)]
    sections: String,
    #[serde(default)]
    out_of_scope: String,
}

/// Score a draft's `Spec:` citations against one or more spec documents -
/// the coverage math itself lives in `speccov::check_coverage`; this route
/// is only the I/O around it.
///
/// `path` XOR `json` names the draft: both present is refused rather than
/// silently preferring one, because a silently-preferred source is exactly
/// the defect class this whole feature exists to catch (round 5, item 10).
/// Neither present is refused the same way, for the same reason. Every
/// `spec_paths` entry is read from disk; one that does not exist or cannot
/// be read is a 400 naming that path, never a silently empty inventory -
/// an unreadable spec must not read as "fully covered because it has no
/// sections".
async fn check_coverage_route(body: &str) -> (u16, String) {
    let req: CoverageRequest = match serde_json::from_str(body) {
        Ok(r) => r,
        Err(e) => {
            return (
                400,
                serde_json::json!({ "error": format!("could not read the request body: {e}") }).to_string(),
            )
        }
    };

    let has_path = req.path.as_deref().is_some_and(|p| !p.trim().is_empty());
    let has_json = req.json.as_deref().is_some_and(|j| !j.trim().is_empty());
    if has_path && has_json {
        return (
            400,
            serde_json::json!({
                "error": "both `path` and `json` were given - pass exactly one, naming the draft \
                          to score. A silently-preferred source is the defect this tool exists to catch."
            })
            .to_string(),
        );
    }
    if !has_path && !has_json {
        return (
            400,
            serde_json::json!({
                "error": "neither `path` nor `json` was given - pass exactly one, naming the draft to score"
            })
            .to_string(),
        );
    }

    let cases = if has_path {
        let path = req.path.as_deref().unwrap_or_default();
        if !std::path::Path::new(path).is_file() {
            return (
                400,
                serde_json::json!({ "error": format!("{path} does not exist or is not a file") }).to_string(),
            );
        }
        match crate::import_parser::parse_file(path) {
            Ok((cases, _warnings)) => cases,
            Err(e) => return (400, serde_json::json!({ "error": e }).to_string()),
        }
    } else {
        match parse_cases_with_warnings(req.json.as_deref().unwrap_or_default()) {
            Ok((cases, _warnings)) => cases,
            Err(e) => return (400, serde_json::json!({ "error": e }).to_string()),
        }
    };

    let mut inventories: Vec<(String, crate::speccov::Inventory)> = Vec::with_capacity(req.spec_paths.len());
    for spec_path in &req.spec_paths {
        match std::fs::read_to_string(spec_path) {
            Ok(text) => inventories.push((spec_path.clone(), crate::speccov::parse_inventory(&text))),
            Err(e) => {
                return (
                    400,
                    serde_json::json!({
                        "error": format!("{spec_path} does not exist or could not be read: {e}")
                    })
                    .to_string(),
                )
            }
        }
    }

    let report = crate::speccov::check_coverage(crate::speccov::CoverageInput {
        inventories,
        cases: &cases,
        sections_scope: &req.sections,
        out_of_scope: &req.out_of_scope,
    });
    (200, report.to_string())
}

/// Request body for `/merge-cases`: which slice files to concatenate, and
/// where the merged draft goes.
#[derive(serde::Deserialize, Default)]
struct MergeRequest {
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    output_path: String,
}

/// Merge slice files from a fan-out into one draft - the other half of
/// `check_spec_coverage`: that finds gaps in a single draft, this is how a
/// spec too large for one writer gets back to being a single draft at all.
///
/// Every slice is read through `crate::import_parser::parse_file` - the
/// REAL importer, same as every other route that reads a draft - never
/// hand-parsed, so a slice a subagent wrote is judged by the exact rules
/// the app itself will apply on import. One unreadable or unparsable slice
/// fails the WHOLE merge with a 400 naming that path; nothing is written,
/// because a merge missing one slice silently is worse than no merge.
/// `output_path` already existing is refused rather than overwritten - the
/// caller picks a new name rather than this route guessing whether the
/// existing file was meant to survive. Cases are concatenated as-is, with
/// no cross-slice deduplication - a caller merging slices that may overlap
/// should run `optimize_cases` or `transform_cases`' dedupe on the merged
/// file afterward.
///
/// With a working repository set, the merged draft obeys the same rule the
/// intake does: a bare name resolves into `<repo>/.test-cases`, and a path
/// outside that folder is refused. A fan-out's merged file is the file the
/// app then watches and imports, so letting it land anywhere would put the
/// end of the writing job outside the workspace the rest of it lives in.
/// Without a repository nothing changes - a merge is a file operation, not
/// an intake, and it still works.
fn merge_cases_route(body: &str, ctx: &BridgeContext) -> (u16, String) {
    let mut req: MergeRequest = match serde_json::from_str(body) {
        Ok(r) => r,
        Err(e) => {
            return (
                400,
                serde_json::json!({ "error": format!("could not read the request body: {e}") }).to_string(),
            )
        }
    };

    if req.paths.is_empty() {
        return (
            400,
            serde_json::json!({ "error": "no `paths` given - name the slice files to merge" }).to_string(),
        );
    }
    if req.output_path.trim().is_empty() {
        return (
            400,
            serde_json::json!({ "error": "no `output_path` given - name where the merged draft goes" })
                .to_string(),
        );
    }
    // The workspace rules, when there is a workspace - before the
    // already-exists check, so a bare name is judged where it will land.
    if let Some(root) = ctx
        .working_dir
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
    {
        let cases_dir = match crate::workspace::ensure_cases_dir(&root) {
            Ok(d) => d,
            Err(e) => return (500, serde_json::json!({ "error": e }).to_string()),
        };
        req.output_path = crate::workspace::resolve_output(&root, &req.output_path);
        if !crate::workspace::is_inside(&cases_dir, std::path::Path::new(&req.output_path)) {
            return (
                400,
                serde_json::json!({
                    "error": format!(
                        "output_path must be inside the working repository's .test-cases folder \
                         ({}) - give a file name, or a path under that folder.",
                        cases_dir.display()
                    )
                })
                .to_string(),
            );
        }
    }

    if std::path::Path::new(&req.output_path).exists() {
        return (
            400,
            serde_json::json!({
                "error": format!(
                    "{} already exists - merge_case_files refuses to overwrite it; pick a new \
                     output_path or remove the existing file first",
                    req.output_path
                )
            })
            .to_string(),
        );
    }

    let mut merged: Vec<crate::model::TestCase> = Vec::new();
    let mut per_file: Vec<serde_json::Value> = Vec::with_capacity(req.paths.len());
    let mut warnings: Vec<String> = Vec::new();
    // Title -> the slice files it appeared in. A fan-out makes title
    // collisions likely precisely because no slice-writer sees another's
    // output, and `dedupe` is the wrong repair (first-wins would delete a
    // real case) - so the merge REPORTS the collision for a human to
    // settle (round 6 §4: two legitimate different cases converged on one
    // title in 373, found only by hand).
    let mut title_slices: std::collections::BTreeMap<String, Vec<(String, String)>> =
        std::collections::BTreeMap::new();

    for path in &req.paths {
        match crate::import_parser::parse_file(path) {
            Ok((cases, file_warnings)) => {
                per_file.push(serde_json::json!({ "path": path, "cases": cases.len() }));
                // Prefixed with the slice's own file name - a warning
                // aggregated across several slices is useless if it can't
                // be traced back to which one produced it.
                warnings.extend(file_warnings.into_iter().map(|w| format!("{path}: {w}")));
                for c in &cases {
                    // Keyed case-insensitively, but the warning shows the
                    // AUTHOR'S casing - a lowercased title in the message
                    // reads like the tool mangled it (round 8 dogfooding).
                    title_slices
                        .entry(c.title.trim().to_lowercase())
                        .or_default()
                        .push((c.title.trim().to_string(), path.clone()));
                }
                merged.extend(cases);
            }
            Err(e) => {
                return (
                    400,
                    serde_json::json!({ "error": format!("{path}: {e}") }).to_string(),
                )
            }
        }
    }

    for slices in title_slices.values() {
        if slices.len() > 1 {
            let display_title = &slices[0].0;
            let mut named: Vec<&str> = slices.iter().map(|(_, p)| p.as_str()).collect();
            named.dedup();
            warnings.push(format!(
                "duplicate title: '{display_title}' appears {} times ({}) - if these are \
                 different cases, disambiguate the titles; dedupe would keep the first and \
                 delete the rest.",
                slices.len(),
                named.join(", ")
            ));
        }
    }

    let text = match crate::import_parser::queue_to_json_string(&merged) {
        Ok(t) => t,
        Err(e) => return (400, serde_json::json!({ "error": e }).to_string()),
    };

    // Atomic write: write to a sibling temp file, then rename it into
    // place. A crash or error partway through a plain `fs::write` would
    // leave `output_path` existing but truncated, which is worse than the
    // merge never having run at all - the caller would find a file, not
    // know it was cut short, and hand a half-draft to the next step. There
    // is no existing atomic-write helper on this branch (the autorun
    // store's version lives on an unmerged sibling), so it is inlined
    // here rather than borrowed from elsewhere.
    let tmp_path = format!("{}.tmp", req.output_path);
    if let Err(e) = std::fs::write(&tmp_path, &text) {
        let _ = std::fs::remove_file(&tmp_path);
        return (
            400,
            serde_json::json!({ "error": format!("could not write {}: {e}", req.output_path) }).to_string(),
        );
    }
    if let Err(e) = std::fs::rename(&tmp_path, &req.output_path) {
        let _ = std::fs::remove_file(&tmp_path);
        return (
            400,
            serde_json::json!({ "error": format!("could not write {}: {e}", req.output_path) }).to_string(),
        );
    }

    (
        200,
        serde_json::json!({
            "cases": merged.len(),
            "per_file": per_file,
            "warnings": warnings,
            "superseded": req.paths,
            "note": format!(
                "The {} slice file(s) above are now superseded by {} and safe to remove from .test-cases - they are importable and id-less, and the importer will happily offer them.",
                req.paths.len(),
                req.output_path
            ),
        })
        .to_string(),
    )
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
    let (values, capped): (Vec<String>, bool) =
        match q(target, "query").filter(|f| !f.trim().is_empty()) {
            Some(f) => {
                let f = f.to_lowercase();
                (
                    values.into_iter().filter(|t| t.to_lowercase().contains(&f)).collect(),
                    false,
                )
            }
            // No query on a 2,000-tag project used to dump the whole list
            // (~30 KB) into the assistant's context (round 8 dogfooding) -
            // capped, with the cap saying how to get the rest.
            None if values.len() > NO_QUERY_TAG_LIMIT => {
                (values.into_iter().take(NO_QUERY_TAG_LIMIT).collect(), true)
            }
            None => (values, false),
        };
    let note = if capped {
        format!(
            "Showing {NO_QUERY_TAG_LIMIT} of {total} tags - pass ?query= (a substring) to \
             search all of them. Prefer an existing tag over a new one. Tags are \
             semicolon-separated in the import JSON, never commas."
        )
    } else {
        "Prefer an existing tag over a new one. Tags are semicolon-separated in the import \
         JSON, never commas."
            .to_string()
    };
    (
        200,
        serde_json::json!({
            "tags": values,
            "count": values.len(),
            "total": total,
            "source": source,
            "note": note,
        })
        .to_string(),
    )
}

/// The most tags a query-less get_tags answers with. High enough that a
/// small project's whole list still comes back in one call, low enough
/// that a 2,229-tag org cannot flood the context by accident.
const NO_QUERY_TAG_LIMIT: usize = 300;

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
        ## Use these tools; do not rebuild them\n\
        Do NOT write your own script, generator or one-off parser to produce,\n\
        transform, validate or reformat test cases. Use the tools: they carry\n\
        THIS project's live rules - the allowed Module values, the tags\n\
        already in use, the exact field contract, and what a kept `id` means\n\
        on the way back in. A hand-rolled equivalent gets those subtly wrong,\n\
        and wrongly in a way nobody sees: the file still looks right.\n\n\
        This has already cost a real draft. A set too large for `validate_cases`\n\
        led to a workaround being written instead, and it reported a pass over\n\
        cases it had never checked.\n\n\
        If a tool genuinely cannot do what you need, STOP and say so. Name the\n\
        tool, what you needed it to do, and what it did instead - then ask the\n\
        developer whether a tool should be added for it. That is a decision for\n\
        them, and a missing capability they hear about gets fixed for everyone.\n\
        Routing around it silently fixes it for nobody and hides the gap.\n\n\
        ## Format\n\
        Each case: `title` (required, <=255 chars), `steps` (required, each\n\
        `{{\"action\", \"expected\"}}`), `tags` (semicolon-separated, never commas),\n\
        `automation_status` (exactly {statuses}), `module` (ONLY from the list\n\
        below), `preconditions` (state, not steps), and `comment` - the developer's own note, which round-trips\n\
        through the file and is never sent to Azure DevOps. You never write\n\
        `comment`: leave it exactly as you found it, and never add one.\n\
        Include `id` ONLY to update that exact work item;\n\
        omit it to create.\n\n\
        Reuse preconditions VERBATIM wherever the environment genuinely is\n\
        the same - do not reword the same setup per case. The run-sheet\n\
        ordering groups cases by shared preconditions so the tester changes\n\
        environment as little as possible; 192 bespoke wordings of the same\n\
        few setups leave it nothing to group, and the ordering buys nothing.\n\n\
        ## Granularity - quality over quantity\n\
        Similar checks belong in ONE case, not several. Checking a\n\
        notification's title and checking its body is one case with two\n\
        steps (or one step with both in the expected result) - not two\n\
        cases. Split only when the checks need different setup or data, or\n\
        can fail independently in a way the tester must record separately.\n\
        A padded case count is not coverage; every extra case is another\n\
        row someone has to execute and maintain.\n\n\
        ## Writing style - sound like a tester, not a model\n\
        This applies to the TEST CASES THEMSELVES: every title, step,\n\
        expected result, precondition and reviewer note. What you say to the\n\
        developer in the conversation is not bound by it. Inside the cases,\n\
        FOLLOW THIS WRITING STYLE:\n\n\
        - SHOULD use clear, simple language.\n\
        - SHOULD be spartan and informative.\n\
        - SHOULD use short, impactful sentences.\n\
        - SHOULD use active voice; avoid passive voice.\n\
        - SHOULD focus on practical, actionable checks.\n\
        - SHOULD use data and examples to support claims when possible: the\n\
        exact value entered, the exact text expected.\n\
        - SHOULD use \"you\" and \"your\" to directly address the tester.\n\
        - AVOID em dashes (—) anywhere. Use only commas, periods, or other\n\
        standard punctuation. To connect ideas, use a period; never an em dash.\n\
        - AVOID constructions like \"...not just this, but also this\".\n\
        - AVOID metaphors and clichés.\n\
        - AVOID generalizations.\n\
        - AVOID setup language in any sentence: in conclusion, in closing, etc.\n\
        - AVOID warnings or notes to the reader inside a case; write the\n\
        content requested and nothing around it.\n\
        - AVOID unnecessary adjectives and adverbs.\n\
        - AVOID hashtags.\n\
        - AVOID semicolons.\n\
        - AVOID markdown and asterisks in titles, steps, expected results and\n\
        preconditions (reviewer_notes is the one field rendered as markdown,\n\
        and it needs none).\n\
        - AVOID these words: can, may, just, that, very, really, literally,\n\
        actually, certainly, probably, basically, could, maybe, delve, embark,\n\
        enlightening, esteemed, shed light, craft, crafting, imagine, realm,\n\
        game-changer, unlock, discover, skyrocket, abyss, not alone, in a world\n\
        where, revolutionize, disruptive, utilize, utilizing, dive deep,\n\
        tapestry, illuminate, unveil, pivotal, intricate, elucidate, hence,\n\
        furthermore, however, harness, exciting, groundbreaking, cutting-edge,\n\
        remarkable, it, remains to be seen, glimpse into, navigating,\n\
        landscape, stark, testament, in summary, in conclusion, moreover,\n\
        boost, skyrocketing, opened up, powerful, inquiries, ever-evolving.\n\n\
        IMPORTANT: review every case before handing it back and make sure\n\
        there are no em dashes.\n\n\
        ## Findings\n\
        When something you read is WRONG - a case that contradicts its spec,\n\
        a spec that contradicts itself, code that does what neither says -\n\
        call `record_finding` with `kind` (test_case, spec or code), the\n\
        `subject` (the work item id, the spec file and section, or the file\n\
        and member), a one-line `title` and the `detail` in markdown. The\n\
        developer reads findings on the AI Bridge tab and in the browser\n\
        report, and resolves them there. Call `list_findings` first so you do\n\
        not record what is already known. Do this on your own when it applies;\n\
        nobody will ask you to. And never write `comment` for this or for anything\n\
        else, and never put it in reviewer_notes: the first is the developer's\n\
        field, the second says where a case came from and nothing more. Do not\n\
        write a case around a defect as if the defect were the requirement -\n\
        record the finding and say so in the conversation.\n\n\
        ## reviewer_notes\n\
        Optional, never sent to Azure DevOps, and the most useful thing you\n\
        can add. Two parts, in this order, and nothing else:\n\n\
        1. **What this case checks**, in one or two plain sentences that\n\
        someone who has not read the spec would understand. Not the steps\n\
        retold - the point, in ordinary words.\n\
        2. **Where the requirement lives**: `Spec: Step10-ManagePerformanceCycle.md\n\
        7.7 (AC-3)`, `Code: IndexModel.CanCopyFromPreviousCycle`, or both.\n\n\
        Add `Out of scope: SSO` only when THIS case deliberately leaves\n\
        something out. The `Spec:` pointer comes FIRST, with the quote\n\
        directly beneath it as `> \"<the source sentence>\"` - the checker\n\
        reads a quote only in that position, so a quote placed above the\n\
        pointer is reported as missing. Quote the source sentence\n\
        verbatim, or it must not be presented as a quote: when you cannot\n\
        quote (a table, a diagram, code), state the exemption in the\n\
        fixed form\n\
        `Spec: <file> <section> - no quotable text (<why>)`,\n\
        naming why as one of code-not-prose, absence,\n\
        table/diagram, or synthesis. Never rewrite inside quotation marks;\n\
        elide with an ellipsis (`...`) instead - check_spec_coverage\n\
        honours an elided quote by requiring every fragment verbatim, in\n\
        order.\n\n\
        Leave OUT, every time:\n\
        - Where the cases came from as a body of work - \"Source:\n\
        implementation (authority = app)\", \"written from the spec\". The\n\
        developer chose that at intake and already knows it; repeating it\n\
        per case is the same sentence on every case.\n\
        - The SET's scope. That was agreed once, in the plan. A note is\n\
        about ONE case.\n\
        - A walk through the steps. They are directly above the note.\n\
        - Anything WRONG that you noticed - a contradiction, a gap, a bug.\n\
        That is a finding: call `record_finding` and keep the note to what\n\
        the case checks and where its requirement lives.\n\
        - Your reasoning, or a decision argued at length. If a case really\n\
        needs an argument made, that belongs in the conversation, not here - and never in `comment`.\n\n\
        Cite; never paraphrase from memory. Rendered as MARKDOWN, so a wiki\n\
        link works - but two sentences and a citation need no formatting.\n\n\
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
        developer. What the file is called (it lives in the repository's\n\
        .test-cases folder), which specs are authoritative and what is out of\n\
        scope are theirs to decide, not yours to assume.\n\
        1. Call `get_test_cases` for the PBI you're writing for and mimic\n\
        their style and granularity.\n\
        2. Draft your cases. Write your draft IN SPEC ORDER - cases walking\n\
        down the document, so a reviewer can scroll the spec and the file\n\
        together, and so `check_spec_coverage` (next) can reason about it\n\
        against the document in the order you wrote it.\n\
        2.5. Call `check_spec_coverage` with the draft and the plan's spec\n\
        paths, while it is still in spec order. Report `uncovered` to the\n\
        developer and account for every entry - \"out of scope for this batch\"\n\
        is a fine answer, silence is not.\n\
        3. Call `optimize_cases` with the JSON: it spells navigation out as\n\
        steps, trims expected results to the outcome, and reorders the cases so\n\
        the tester changes environment as few times as possible. Hand back the\n\
        JSON it returns. The optimizer then stamps every case with BOTH\n\
        orders: `spec_order` (the order you wrote) and `tester_order` (its\n\
        grouped run sequence). Keep those two fields exactly as it set them -\n\
        do not renumber them by hand, and do not strip them; the app uses\n\
        them to flip the queue between the two readings.\n\
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
    let (limit, offset, titles_only) = paging(target);
    match client
        .get_pbi_test_cases_full(
            &ctx.org,
            pbi,
            ctx.module_ref.as_deref(),
            ctx.preconditions_ref.as_deref(),
        )
        .await
    {
        Ok(cases) => (200, case_page(&cases, limit, offset, titles_only).to_string()),
        Err(e) => (502, format!("Azure DevOps error: {e:?}")),
    }
}

/// The paging arguments every case-listing route reads the same way:
/// `limit` (default 5, cap 20), `offset`, and `titles_only`.
fn paging(target: &str) -> (usize, usize, bool) {
    let limit = q(target, "limit")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(5)
        .min(20);
    let offset = q(target, "offset").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
    // Titles-only mode exists for duplicate checking: comparing titles
    // against a set of 60 cases must not cost 60 cases of step text.
    let titles_only = matches!(q(target, "titles_only").as_deref(), Some("true") | Some("1"));
    (limit, offset, titles_only)
}

/// One page of cases in the import JSON record shape, with the total and
/// a note when the page did not reach the end. Shared by the PBI and the
/// suite listings so the two never drift apart in shape.
fn case_page(
    cases: &[crate::ado::TestCaseFull],
    limit: usize,
    offset: usize,
    titles_only: bool,
) -> serde_json::Value {
    let total = cases.len();
    // Titles are cheap: a fixed 200, not the caller's limit, so one call
    // can cover a whole set when all it needs is duplicate checking.
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
    out
}

/// How long a scanned plan tree is reused before it is read again. A
/// project can hold hundreds of plans, and the scan is one request per
/// plan; an assistant that lists suites and then reads three of them must
/// not pay for the scan three times.
const SUITE_TREE_TTL: std::time::Duration = std::time::Duration::from_secs(10 * 60);

type SuiteTree = Vec<crate::ado_testplan::PlanWithSuites>;

fn suite_tree_cache() -> &'static std::sync::Mutex<
    std::collections::HashMap<(String, String, String), (std::time::Instant, SuiteTree)>,
> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<(String, String, String), (std::time::Instant, SuiteTree)>>,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The plans and suites the Test Suites tab shows, cached per org and
/// project for `SUITE_TREE_TTL`; `refresh=true` reads them again.
async fn suite_tree(
    ctx: &BridgeContext,
    client: &crate::ado::AdoClient,
    refresh: bool,
) -> Result<SuiteTree, crate::ado::AdoError> {
    let key = (client.base_url.clone(), ctx.org.clone(), ctx.project.clone());
    if !refresh {
        if let Some((at, tree)) = suite_tree_cache().lock().unwrap().get(&key) {
            if at.elapsed() < SUITE_TREE_TTL {
                return Ok(tree.clone());
            }
        }
    }
    let tree = client.list_plans_with_suites(&ctx.org, &ctx.project).await?;
    suite_tree_cache()
        .lock()
        .unwrap()
        .insert(key, (std::time::Instant::now(), tree.clone()));
    Ok(tree)
}

/// Every suite in the project, one row each with its plan, filtered by
/// `q` against the plan name, the suite name, or a requirement id. The
/// plan id and suite id in a row are what `get_suite_test_cases` takes.
async fn suites(
    ctx: &BridgeContext,
    client: &crate::ado::AdoClient,
    target: &str,
) -> (u16, String) {
    const CAP: usize = 200;
    let query = q(target, "q").map(|s| s.trim().to_lowercase()).unwrap_or_default();
    let refresh = matches!(q(target, "refresh").as_deref(), Some("true") | Some("1"));
    let tree = match suite_tree(ctx, client, refresh).await {
        Ok(t) => t,
        Err(e) => return (502, format!("Azure DevOps error: {e:?}")),
    };
    let mut rows: Vec<serde_json::Value> = vec![];
    for p in &tree {
        let plan_hit = query.is_empty() || p.plan.name.to_lowercase().contains(&query);
        for s in &p.suites {
            let hit = plan_hit
                || s.name.to_lowercase().contains(&query)
                || s.requirement_id.map(|r| r.to_string() == query).unwrap_or(false);
            if !hit {
                continue;
            }
            rows.push(serde_json::json!({
                "plan_id": p.plan.id,
                "plan_name": p.plan.name,
                "suite_id": s.id,
                "suite_name": s.name,
                "suite_type": s.suite_type,
                "requirement_id": s.requirement_id,
                "parent_suite_id": s.parent_id,
            }));
        }
    }
    let total = rows.len();
    rows.truncate(CAP);
    let mut out = serde_json::json!({ "suites": rows, "total": total });
    if total > CAP {
        out["note"] = serde_json::json!(format!(
            "{CAP} of {total} suites returned - pass q=<plan or suite name> to narrow the list."
        ));
    }
    (200, out.to_string())
}

/// The cases in one suite, in the import JSON record shape, by way of the
/// suite's test points - the same read the Run Tests tab makes. With
/// `children=true` the endpoint's own `isRecursive` flag brings in every
/// child suite's cases, so a folder reads in one call.
async fn suite_cases(
    ctx: &BridgeContext,
    client: &crate::ado::AdoClient,
    target: &str,
) -> (u16, String) {
    let (Some(plan), Some(suite)) = (
        q(target, "plan").and_then(|v| v.parse::<i32>().ok()),
        q(target, "suite").and_then(|v| v.parse::<i32>().ok()),
    ) else {
        return (400, "pass ?plan=<plan id>&suite=<suite id> (find them with search_test_suites)".into());
    };
    let (limit, offset, titles_only) = paging(target);
    let children = matches!(q(target, "children").as_deref(), Some("true") | Some("1"));
    let points = match client
        .get_test_points_in(&ctx.org, &ctx.project, plan, suite, &[], children)
        .await
    {
        Ok(p) => p,
        Err(e) => return (502, format!("Azure DevOps error: {e:?}")),
    };
    // One point per case per configuration: a suite run on two browsers
    // lists every case twice. Keep the suite's order, each case once.
    let mut ids: Vec<i32> = vec![];
    for p in &points {
        if let Some(id) = p.test_case_id {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    match client
        .get_test_cases_by_ids(&ctx.org, &ids, ctx.module_ref.as_deref(), ctx.preconditions_ref.as_deref())
        .await
    {
        Ok(mut cases) => {
            // The work item read answers in id order; put the suite's own
            // order back so the page reads like the suite does.
            cases.sort_by_key(|c| ids.iter().position(|&i| i == c.id).unwrap_or(usize::MAX));
            let mut out = case_page(&cases, limit, offset, titles_only);
            out["plan_id"] = serde_json::json!(plan);
            out["suite_id"] = serde_json::json!(suite);
            (200, out.to_string())
        }
        Err(e) => (502, format!("Azure DevOps error: {e:?}")),
    }
}

/// How many failures get their comment + linked bugs fetched. Each one is
/// its own request; a suite with 80 failures is a suite with a bigger
/// problem than missing detail text.
const RUN_FAILURE_DETAIL_CAP: usize = 10;

/// The failed cases from a PBI's latest runs, with each failure's comment
/// and linked bugs - what an assistant needs to draft regression cases.
///
/// GET-only end to end: `find_pbi_requirement_suite` is the find-ONLY
/// scan, never the find-or-create one. A PBI with no suite is an answer
/// ("this PBI has never had a run"), not a reason to create anything -
/// this is the bridge, and the bridge does not write to Azure DevOps.
/// pbi -> resolved suite, held for the app's lifetime. Resolving a suite
/// scans EVERY test plan in the project - throttle-paced, that took ~60s
/// against a large org, and the MCP proxy used to give up at 30s and
/// blame the connection. The ids are stable once found (same reason the
/// Run Tests tab seeds them from disk); a suite deleted in Azure DevOps
/// is caught by the 404 on its points and re-resolved once. The client's
/// base_url is in the key so parallel tests on different mock servers
/// cannot poison each other.
fn suite_cache() -> &'static std::sync::Mutex<
    std::collections::HashMap<(String, String, String, i32), crate::ado_testplan::EnsuredSuite>,
> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<
            std::collections::HashMap<(String, String, String, i32), crate::ado_testplan::EnsuredSuite>,
        >,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}

async fn run_failures(
    ctx: &BridgeContext,
    client: &crate::ado::AdoClient,
    target: &str,
) -> (u16, String) {
    let Some(pbi) = q(target, "pbi").and_then(|v| v.parse::<i32>().ok()) else {
        return (400, "pass ?pbi=<work item id> (find one with search_pbis)".into());
    };
    let key = (
        client.base_url.clone(),
        ctx.org.clone(),
        ctx.project.clone(),
        pbi,
    );
    let mut retried = false;
    let (suite, points) = loop {
        let cached = suite_cache().lock().unwrap().get(&key).cloned();
        let (suite, from_cache) = match cached {
            Some(s) => (s, true),
            None => {
                // No area path here: the scan only uses it to order plans,
                // and it checks every plan regardless, so "" costs at most
                // a slower hit.
                match client
                    .find_pbi_requirement_suite(&ctx.org, &ctx.project, pbi, "")
                    .await
                {
                    Ok(Some(s)) => {
                        suite_cache().lock().unwrap().insert(key.clone(), s.clone());
                        (s, false)
                    }
                    Ok(None) => {
                        return (
                            200,
                            serde_json::json!({
                                "pbi": pbi,
                                "failures": [],
                                "note": "This PBI has no test suite, so it has never had a test run - there are no failures to read.",
                            })
                            .to_string(),
                        )
                    }
                    Err(e) => return (502, format!("Azure DevOps error: {e:?}")),
                }
            }
        };
        match client
            .get_test_points(&ctx.org, &ctx.project, suite.plan_id, suite.suite_id, &[])
            .await
        {
            Ok(p) => break (suite, p),
            // A cached suite that 404s was deleted in Azure DevOps since
            // it was resolved: forget it and scan once from scratch.
            Err(crate::ado::AdoError::NotFound) if from_cache && !retried => {
                suite_cache().lock().unwrap().remove(&key);
                retried = true;
            }
            Err(e) => return (502, format!("Azure DevOps error: {e:?}")),
        }
    };

    let total = points.len();
    let failed: Vec<_> = points
        .into_iter()
        .filter(|p| p.last_outcome.eq_ignore_ascii_case("failed"))
        .collect();
    let failed_total = failed.len();

    let mut failures: Vec<serde_json::Value> = vec![];
    for p in failed.iter().take(RUN_FAILURE_DETAIL_CAP) {
        // The comment is where the tester wrote what actually went wrong -
        // fetched per result, best-effort: a failure whose detail cannot be
        // read is still a failure worth naming.
        let (comment, bug_ids) = match (p.last_run_id, p.last_result_id) {
            (Some(run), Some(res)) => client
                .get_result_report_info(&ctx.org, &ctx.project, run, res)
                .await
                .unwrap_or_default(),
            _ => Default::default(),
        };
        failures.push(serde_json::json!({
            "case_id": p.test_case_id,
            "title": p.test_case_name,
            "configuration": p.config_name,
            "run_id": p.last_run_id,
            "comment": comment,
            "bug_ids": bug_ids,
        }));
    }

    let mut out = serde_json::json!({
        "pbi": pbi,
        "plan": { "id": suite.plan_id, "name": suite.plan_name },
        "cases_in_suite": total,
        "failed": failed_total,
        "failures": failures,
        "note": "Each failure's `comment` is what the tester wrote when it failed, and `bug_ids` are the bugs they linked. To write regression cases for these, start with begin_test_case_writing as usual - and read the failed case itself via get_test_cases so the regression case extends it instead of restating it.",
    });
    if failed_total > RUN_FAILURE_DETAIL_CAP {
        out["truncated"] = serde_json::json!(format!(
            "{failed_total} cases are failed; details fetched for the first {RUN_FAILURE_DETAIL_CAP}."
        ));
    }
    (200, out.to_string())
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

/// Full markdown content of one wiki page. `path` is a page path, a
/// `search_wiki` hit's path, or the URL from the browser - see
/// `AdoClient::get_wiki_page`. `wiki` is required for all but the URL.
async fn wiki_page(
    ctx: &BridgeContext,
    client: &crate::ado::AdoClient,
    target: &str,
) -> (u16, String) {
    let Some(path) = q(target, "path").filter(|s| !s.trim().is_empty()) else {
        return (400, "pass ?path=<page path, search path, or wiki url>".into());
    };
    // A wiki URL names its own wiki, so asking for `wiki` as well would
    // refuse the one thing a person has to hand. Every other form still
    // needs it.
    let trimmed = path.trim();
    let is_url = trimmed.starts_with("http://") || trimmed.starts_with("https://");
    let wiki_id = q(target, "wiki").filter(|s| !s.trim().is_empty()).unwrap_or_default();
    if wiki_id.is_empty() && !is_url {
        return (400, "pass ?wiki=<wiki id>&path=<page path>, or ?path=<wiki url>".into());
    }
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
