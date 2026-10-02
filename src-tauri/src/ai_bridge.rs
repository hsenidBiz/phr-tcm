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

#[derive(Clone, Default, Serialize)]
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
    /// The database chosen under Database Read Access, by id, or None when none
    /// is - in which case both database tools refuse. Only the id travels
    /// here: the login it stands for is resolved from `db_secrets` each time
    /// a database tool runs, so a login saved a moment ago applies to the
    /// very next call.
    pub db_id: Option<String>,
    /// Where `db_id` is resolved: the app's own store, set by
    /// `set_bridge_context`. None in a context nobody set up, which the
    /// database tools read as "nothing chosen". What it holds are
    /// passwords, so it is never serialised and `Debug` names only whether
    /// it is there.
    #[serde(skip)]
    pub db_secrets: Option<std::sync::Arc<dyn crate::db::SecretStore>>,
    /// Whether the person has switched create, update and delete on. It is
    /// half the permission: `/db-query` also needs the connection's own
    /// user to be one that may write.
    pub db_writes: bool,
    /// Whether the person has switched API templates on: proving and
    /// running one writes to the application, so both are refused
    /// (`API_WRITES_OFF`) until they do. Off by default, like `db_writes`.
    pub api_writes: bool,
    /// The AI Bridge tab's "Risk-tiered test design (trial)" switch: the
    /// writing guide carries the risk-tiered rules instead of the plain
    /// granularity and edge-case sections. Off by default.
    pub risk_tiered: bool,
}

/// Hand-written so the store - and therefore every password in it - cannot
/// reach a log line, a panic message or a bug report through a stray
/// `{:?}` on the context (and a trait object has no `Debug` to derive).
impl std::fmt::Debug for BridgeContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BridgeContext")
            .field("org", &self.org)
            .field("project", &self.project)
            .field("module_ref", &self.module_ref)
            .field("preconditions_ref", &self.preconditions_ref)
            .field("disabled_tools", &self.disabled_tools)
            .field("working_dir", &self.working_dir)
            // The id comes from the webview; one this build does not know
            // could be anything the webview sent, so only a known id is shown.
            .field(
                "db_id",
                &self.db_id.as_deref().map(|id| {
                    let known = self
                        .db_secrets
                        .as_deref()
                        .is_some_and(|store| crate::db::credentials::is_known(store, id));
                    if known { id } else { "(unknown)" }
                }),
            )
            .field("db_secrets", &self.db_secrets.as_ref().map(|_| "(hidden)"))
            .field("db_writes", &self.db_writes)
            .field("api_writes", &self.api_writes)
            .field("risk_tiered", &self.risk_tiered)
            .finish()
    }
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

/// Where the API template routes say a template's file or run history
/// changed, so the API Templates tab can reload without polling. A
/// process-wide sink for the same reason as `INTAKE_SINK`; `None` in a
/// test that never installs one.
type TemplatesSink = Box<dyn Fn(String) + Send + Sync>;
static TEMPLATES_SINK: std::sync::OnceLock<TemplatesSink> = std::sync::OnceLock::new();

/// Called once by the app when the bridge starts. Later calls are ignored.
pub fn set_templates_sink(f: TemplatesSink) {
    let _ = TEMPLATES_SINK.set(f);
}

fn templates_changed(id: &str) {
    if let Some(f) = TEMPLATES_SINK.get() {
        f(id.to_string());
    }
}

type WatchSource = Box<dyn Fn() -> Vec<String> + Send + Sync>;
static WATCH_SOURCE: std::sync::OnceLock<WatchSource> = std::sync::OnceLock::new();

/// Called once by the app when the bridge starts: the files the app is
/// following right now. Unset in tests, where nothing is followed.
pub fn set_watch_source(f: WatchSource) {
    let _ = WATCH_SOURCE.set(f);
}

fn watched_now() -> Vec<String> {
    WATCH_SOURCE.get().map(|f| f()).unwrap_or_default()
}

/// Whether an in-place tool run may rewrite `path`: a file the app is
/// following, or one under the working repository's `.test-cases` folder.
/// Paths are compared canonicalised, so `..` and case/format differences
/// cannot walk out. A path that does not exist is never writable here.
pub fn bridge_may_write(path: &str, working_dir: Option<&str>, watched: &[String]) -> bool {
    let Ok(file) = std::fs::canonicalize(path) else {
        return false;
    };
    if watched
        .iter()
        .any(|w| std::fs::canonicalize(w).map(|c| c == file).unwrap_or(false))
    {
        return true;
    }
    let Some(root) = working_dir.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    // The one containment rule (workspace::is_inside), the same one the
    // intake and the merge route apply to their output paths.
    crate::workspace::is_inside(&crate::workspace::cases_dir(std::path::Path::new(root)), &file)
}

/// The refusal for an in-place write outside what the app owns.
const IN_PLACE_REFUSAL: &str = "in_place only writes a draft the app is following or one under the working repository's .test-cases folder - call again without in_place and write the returned JSON yourself.";

/// Per-launch shared secret for the handshake file (32 hex chars).
pub fn new_token() -> String {
    let mut rng = rand::rng();
    (0..32)
        .map(|_| char::from_digit(rng.random_range(0..16), 16).unwrap())
        .collect()
}

/// Query-string value by key from "a=1&b=2", percent-decoded ('+' -> space,
/// arbitrary %XX -> the raw byte). `pub` so `tests/suite/ai_bridge.rs` can
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
    // Every Auto Run route is offered only where the Auto Run tools are -
    // a development build, or a release build whose optional extras are
    // unlocked - and the check runs ONCE here, by path, before the
    // router's match, so a route added later is covered by the shape of
    // its name rather than by somebody remembering to repeat the guard on
    // its own arm.
    if let Some(refused) = autorun_guard_for(path, crate::ai_tools::autorun_offered()) {
        return refused;
    }
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
        ("POST", "/optimize") => optimize_json(body, target, ctx),
        ("POST", "/transform") => transform_json(body, ctx),
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
        // The Auto Run routes. Only ONE of them reaches Azure DevOps, and
        // only to read: the save route looks the test cases up so the
        // floor has something to check the script against. The rest
        // document a format, drive the browser the person opened, or read
        // files this machine already wrote - so none of them needs a
        // signed-in client. All were let through by the guard above.
        ("GET", "/autorun-guide") => (200, autorun_guide_with_quirks(ctx)),
        ("POST", "/autorun-script") => save_autorun_scripts(ctx, client, body).await,
        ("GET", "/autorun-page") => autorun_page(target).await,
        ("POST", "/autorun-probe") => autorun_probe(body).await,
        ("POST", "/autorun-try") => autorun_try(ctx, body).await,
        ("GET", "/autorun-failures") => autorun_failures(target),
        ("POST", "/autorun-quirk") => autorun_quirk(ctx, body),
        ("POST", "/autorun-quirk-retire") => autorun_quirk_retire(ctx, body),
        // The active environment's accounts: the assistant proposes logins
        // (never passwords) for a person to add, and reads the ones there -
        // passwords included only in an environment marked as a test one.
        ("POST", "/accounts-propose") => accounts_propose(body),
        ("GET", "/accounts") => accounts_read(ctx),
        // The API template routes: gated with the Auto Run ones by the
        // guard above. Proving and running write to the application, so
        // both also need the person's own switch (`ctx.api_writes`); the
        // guide and the list are reads and answer either way.
        ("GET", "/api-template-guide") => (200, api_template_guide(ctx)),
        ("GET", "/api-templates") => api_template_list(ctx),
        ("POST", "/api-template-prove") => {
            api_template_prove(
                ctx,
                body,
                real_template_browsers,
                real_stage_db,
                &crate::commands::autorun_replay::replay_timing(false),
            )
            .await
        }
        ("POST", "/api-template-run") => {
            api_template_run(
                ctx,
                body,
                real_template_browsers,
                real_stage_db,
                &crate::commands::autorun_replay::replay_timing(false),
            )
            .await
        }
        // Flows: saving one writes a local file and reads the database, and
        // progress only reads - neither writes to the application, so
        // neither needs the API templates switch.
        ("POST", "/api-template-flow-save") => api_template_flow_save(ctx, body, stage_db).await,
        ("POST", "/api-template-flow-progress") => api_template_flow_progress(ctx, body, stage_db).await,
        // The database routes. Not Auto Run and not dev-only: they are
        // switchable like any ordinary tool, and what they may do is
        // decided by the connection the person chose and the write switch
        // beside it - never by the build kind.
        ("POST", "/db-lookup") => db_lookup(ctx, body).await,
        ("POST", "/db-query") => db_query(ctx, body).await,
        // The proxy asks for this before listing tools, so a toggle in the
        // app takes effect on the assistant's next tools/list. `autorun`
        // says whether the Auto Run tools are offered at all: the proxy is
        // a separate process and cannot read the optional-extras switch.
        ("GET", "/tools") => (
            200,
            serde_json::json!({
                "disabled": ctx.disabled_tools,
                "autorun": crate::ai_tools::autorun_offered(),
                // Only while writes are on too: without them there is no
                // change to run, asked about or not.
                "db_no_ask": crate::app_settings::current().db_auto_approve && ctx.db_writes,
            })
            .to_string(),
        ),
        ("GET", "/run-results") => match client {
            Some(c) => run_results(ctx, c, target).await,
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
/// spelling; "not possible, by design" ends the attempt. `pub` so a test
/// can hold every real route name to it: a route whose name contained one
/// of these words would still be reached (its arm matches first), but it
/// would read as a write to anyone checking what the bridge refuses.
pub fn smells_like_a_write(method: &str, target: &str) -> bool {
    if matches!(method, "PUT" | "PATCH" | "DELETE") {
        return true;
    }
    let path = target.split('?').next().unwrap_or("").to_ascii_lowercase();
    ["create", "update", "delete", "edit", "write", "add-", "remove", "submit"]
        .iter()
        .any(|w| path.contains(w))
}

/// The guard every autorun route runs before doing anything else: where the
/// Auto Run tools are not offered (a release build whose optional extras
/// are locked) there is no switch that can turn them back on, so they
/// refuse unconditionally rather than falling through to whatever
/// `ctx.disabled_tools` says. `offered` is explicit so both branches are
/// testable without a release build.
pub fn autorun_route_guard(offered: bool) -> Option<(u16, String)> {
    if offered {
        None
    } else {
        Some((404, "not available in this build".to_string()))
    }
}

/// The same guard, applied by PATH rather than by arm. `route` calls this
/// once, before its match, so every `/autorun-` route - every
/// `/api-template` one, which rides on the same signed-in browser and is
/// offered exactly where Auto Run is, and `/accounts` and every
/// `/accounts-` one, the Auto Run accounts - is covered by the shape of its
/// name: a route added later cannot be left ungated by forgetting to
/// repeat the check. `offered` is explicit for the same reason
/// `autorun_route_guard`'s is: both branches stay testable.
pub fn autorun_guard_for(path: &str, offered: bool) -> Option<(u16, String)> {
    if path.starts_with("/autorun-")
        || path.starts_with("/api-template")
        || path == "/accounts"
        || path.starts_with("/accounts-")
    {
        autorun_route_guard(offered)
    } else {
        None
    }
}

/// Said when a prove or a run arrives while the person's API templates
/// switch is off - before anything else about the call is looked at.
pub const API_WRITES_OFF: &str =
    "API templates are switched off - turn them on under API templates on the AI Bridge tab";

/// Said when a prove or a run arrives while another one holds the
/// process-wide slot (`runner::claim`).
const API_TEMPLATE_BUSY: &str = "another API template is running - wait for it to finish";

/// The browsers a real prove or run opens: one, headless - nobody watches
/// a template run.
fn real_template_browsers(which: crate::browser::launch::Browser) -> crate::commands::autorun_replay::RealBrowsers {
    crate::commands::autorun_replay::RealBrowsers::new(which, false)
}

/// The origin this project signs in at in the active environment - the
/// environment's address, else the recipe's - or None without a recipe.
fn recipe_origin(root: &std::path::Path, org: &str, project: &str) -> Option<String> {
    crate::autorun::recipe::load_effective_recipe(root, org, project)
        .ok()
        .flatten()
        .and_then(|r| crate::autorun::recipe::origin_of(&r.start_url))
}

/// The API template guide, with this project's account keys and origin.
/// Answers without a data root too - with the format alone and a sentence
/// saying what is not set up yet.
fn api_template_guide(ctx: &BridgeContext) -> String {
    let Some(root) = crate::autorun::store::configured_root() else {
        return crate::api_templates::guide::text(&[], None);
    };
    // Keys only: the guide never sees a username or a password.
    let keys: Vec<String> = crate::autorun::accounts::load_accounts(&root)
        .unwrap_or_default()
        .into_iter()
        .map(|a| a.key)
        .collect();
    let origin = recipe_origin(&root, &ctx.org, &ctx.project);
    let mut out = crate::api_templates::guide::text(&keys, origin.as_deref());
    match crate::environments::active(&root) {
        Ok(env) => out.push_str(&crate::api_templates::guide::active_environment_line(&env.name)),
        Err(e) => crate::applog::warn(format!("Guide: the active environment could not be read: {e}")),
    }
    out.push_str(&crate::test_files::guide_section(&project_test_files(&root, ctx)));
    // The project's quirks - the same list, and the same section, the Auto
    // Run guide ends with: active notes only, each with its evidence.
    if !ctx.org.trim().is_empty() && !ctx.project.trim().is_empty() {
        let quirks = crate::autorun::quirks::load_quirks(&root, &ctx.org, &ctx.project).unwrap_or_default();
        let section = crate::autorun::quirks::quirks_section(&quirks);
        if !section.is_empty() {
            out.push('\n');
            out.push_str(&section);
        }
    }
    out
}

/// This project's Test files - names and sizes, for an assistant to pick
/// from. One that cannot be read is logged by `test_files::list` and reads
/// as none.
fn project_test_files(root: &std::path::Path, ctx: &BridgeContext) -> Vec<crate::test_files::TestFile> {
    if ctx.org.trim().is_empty() || ctx.project.trim().is_empty() {
        return Vec::new();
    }
    crate::test_files::list(&crate::test_files::folder(root, &ctx.org, &ctx.project)).unwrap_or_default()
}

/// Every saved template for this project, as a summary: what it is, what
/// it takes and gives back, the flow stage it performs, whether it is
/// proven on this site (an imported one is not - `unproven` says what to
/// do about it), and its newest run
/// (null before its first - the prove that saved it is history, not a
/// run). Beside them, every saved flow with its stages and the templates
/// on each - a flow file that no longer parses is left out (and logged by
/// `flow_store::list`), never the whole answer.
fn api_template_list(ctx: &BridgeContext) -> (u16, String) {
    use crate::api_templates::{flow_store, gate, store};
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let saved = match store::list(&root, &ctx.org, &ctx.project) {
        Ok(saved) => saved,
        Err(e) => {
            crate::applog::warn(format!("api templates: the list could not be read: {e}"));
            return (500, "the saved templates could not be read - see Settings, Logs".to_string());
        }
    };
    let flows = flow_store::list(&root, &ctx.org, &ctx.project).unwrap_or_else(|e| {
        crate::applog::warn(format!("api template flows: the list could not be read: {e}"));
        Vec::new()
    });
    let flow_rows: Vec<serde_json::Value> = flows
        .iter()
        .map(|f| {
            let stages: Vec<serde_json::Value> = f
                .stages
                .iter()
                .map(|s| {
                    let on: Vec<&str> =
                        gate::templates_on(&saved, &f.id, &s.id).iter().map(|t| t.template.id.as_str()).collect();
                    serde_json::json!({
                        "id": s.id,
                        "title": s.title,
                        "requires": s.requires,
                        "optional": s.optional,
                        "creates": s.creates,
                        "templates": on,
                    })
                })
                .collect();
            serde_json::json!({
                "id": f.id,
                "title": f.title,
                "module": f.module,
                "subject": f.subject,
                "stages": stages,
            })
        })
        .collect();
    let rows: Vec<serde_json::Value> = saved
        .into_iter()
        .map(|s| {
            let t = s.template;
            serde_json::json!({
                "id": t.id,
                "title": t.title,
                "module": t.module,
                "effect": t.effect,
                "params": t.params,
                "outputs": t.outputs,
                "stage": t.stage,
                // An imported template arrives unproven (api_templates::share):
                // runs of it are allowed, so the assistant is told plainly.
                "proven": t.proven.is_some(),
                "unproven": t.proven.is_none().then_some(crate::api_templates::share::UNPROVEN_FOR_ASSISTANT),
                "last_run": s.runs.into_iter().find(|r| r.mode == store::MODE_RUN),
            })
        })
        .collect();
    // Names and sizes only: what a template's `files` may name.
    let test_files: Vec<serde_json::Value> = project_test_files(&root, ctx)
        .into_iter()
        .map(|f| serde_json::json!({ "name": f.name, "size": f.size }))
        .collect();
    (200, serde_json::json!({ "templates": rows, "flows": flow_rows, "test_files": test_files }).to_string())
}

/// Every saved template for this project, for naming the ones on a stage.
/// One that cannot be listed leaves that part of a sentence short rather
/// than refusing the call, and is logged.
fn saved_templates(root: &std::path::Path, org: &str, project: &str) -> Vec<crate::api_templates::store::SavedTemplate> {
    crate::api_templates::store::list(root, org, project).unwrap_or_else(|e| {
        crate::applog::warn(format!("api templates: the list could not be read: {e}"));
        Vec::new()
    })
}

/// An argument that may arrive as JSON or as a JSON string - the shape
/// every sibling tool on this server takes its payload in. A string that
/// does not parse is handed on as the string, so the complaint about it
/// names what was actually sent.
fn json_arg(v: Option<&serde_json::Value>) -> Option<serde_json::Value> {
    match v {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(s)) => {
            Some(serde_json::from_str(s).unwrap_or_else(|_| serde_json::Value::String(s.clone())))
        }
        Some(other) => Some(other.clone()),
    }
}

/// What a prove and a run both carry besides the template: the account
/// key, the param values, and the browser.
struct TemplateCall {
    account: String,
    values: serde_json::Map<String, serde_json::Value>,
    browser: crate::browser::launch::Browser,
}

fn template_call(v: &serde_json::Value, shape: &str) -> Result<TemplateCall, (u16, String)> {
    let account = match v.get("account") {
        Some(serde_json::Value::String(s)) if !s.trim().is_empty() => s.trim().to_string(),
        _ => {
            return Err((
                400,
                format!(
                    "this call needs an \"account\" - one of the account keys get_api_template_guide lists. Expected {shape}."
                ),
            ))
        }
    };
    let values = match json_arg(v.get("values")) {
        None => serde_json::Map::new(),
        Some(serde_json::Value::Object(m)) => m,
        Some(_) => return Err((400, "\"values\" is an object of param names to values".to_string())),
    };
    let browser = match v.get("browser") {
        None | Some(serde_json::Value::Null) => crate::browser::launch::Browser::Edge,
        Some(serde_json::Value::String(s)) if matches!(s.trim().to_ascii_lowercase().as_str(), "edge" | "chrome") => {
            crate::browser::launch::Browser::from_name(s)
        }
        Some(_) => return Err((400, "\"browser\" is \"edge\" or \"chrome\"".to_string())),
    };
    Ok(TemplateCall { account, values, browser })
}

const PROVE_SHAPE: &str = "{ \"template\": <the draft>, \"account\": \"<account key>\", \"values\": { <param>: <value> }, \"replace\"?: true, \"why\"?: \"<reason>\", \"browser\"?: \"edge\" | \"chrome\" }";
const RUN_SHAPE: &str = "{ \"id\": \"<template id>\", \"account\": \"<account key>\", \"values\": { <param>: <value> }, \"browser\"?: \"edge\" | \"chrome\" }";

/// `POST /api-template-prove`: run a draft and save it only if every step
/// passed - and, for a template on a flow, only if its own stage is done
/// afterwards. The route arm with the browser factory (`open`) and the
/// flow database (`open_db`, asked for only by a template on a flow)
/// handed in, so a test reaches all of it but a real browser and server.
pub async fn api_template_prove<B: crate::autorun::replay::Browsers, D: crate::api_templates::gate::StageDb>(
    ctx: &BridgeContext,
    body: &str,
    open: impl FnOnce(crate::browser::launch::Browser) -> B,
    open_db: impl FnOnce(&BridgeContext) -> Result<D, (u16, String)>,
    timing: &crate::browser::timing::Timing,
) -> (u16, String) {
    use crate::api_templates::runner::{Mode, RunRequest};
    use crate::api_templates::{store, valid_id, ApiTemplate};
    if !ctx.api_writes {
        return (400, API_WRITES_OFF.to_string());
    }
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return (400, format!("that is not readable JSON: {e}. Expected {PROVE_SHAPE}.")),
    };
    let Some(draft) = json_arg(v.get("template")) else {
        return (400, format!("this call needs a \"template\". Expected {PROVE_SHAPE}."));
    };
    let template: ApiTemplate = match serde_json::from_value(draft) {
        Ok(t) => t,
        Err(e) => return (400, format!("that is not a template: {e} - call get_api_template_guide for the format")),
    };
    let call = match template_call(&v, PROVE_SHAPE) {
        Ok(c) => c,
        Err(refused) => return refused,
    };
    let replace = v["replace"].as_bool().unwrap_or(false);
    let why = v["why"].as_str().map(str::to_string);
    let existing = match store::load(&root, &ctx.org, &ctx.project, &template.id) {
        Ok(found) => found,
        // A saved file that no longer reads still stands for a template of
        // that id, so replacing it needs the same reason a readable one
        // does. An invalid id is one of `check`'s own problems.
        Err(e) if valid_id(&template.id) => {
            crate::applog::warn(format!("api template {}: the saved copy could not be read: {e}", template.id));
            Some(template.clone())
        }
        Err(_) => None,
    };
    let req = RunRequest {
        org: ctx.org.clone(),
        project: ctx.project.clone(),
        account: call.account,
        values: call.values,
        mode: Mode::Prove { replace, why },
        template,
    };
    run_api_template_request(ctx, &root, req, existing.as_ref(), call.browser, open, open_db, timing).await
}

/// `POST /api-template-run`: run a saved template and return its outputs,
/// or the failing step and what had been created. As
/// `api_template_prove`, with the browser factory and the flow database
/// handed in.
pub async fn api_template_run<B: crate::autorun::replay::Browsers, D: crate::api_templates::gate::StageDb>(
    ctx: &BridgeContext,
    body: &str,
    open: impl FnOnce(crate::browser::launch::Browser) -> B,
    open_db: impl FnOnce(&BridgeContext) -> Result<D, (u16, String)>,
    timing: &crate::browser::timing::Timing,
) -> (u16, String) {
    use crate::api_templates::runner::{Mode, RunRequest};
    use crate::api_templates::store;
    if !ctx.api_writes {
        return (400, API_WRITES_OFF.to_string());
    }
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return (400, format!("that is not readable JSON: {e}. Expected {RUN_SHAPE}.")),
    };
    let id = match v.get("id") {
        Some(serde_json::Value::String(s)) if !s.trim().is_empty() => s.trim().to_string(),
        _ => return (400, format!("this call needs an \"id\". Expected {RUN_SHAPE}.")),
    };
    let call = match template_call(&v, RUN_SHAPE) {
        Ok(c) => c,
        Err(refused) => return refused,
    };
    let template = match store::load(&root, &ctx.org, &ctx.project, &id) {
        Ok(Some(t)) => t,
        Ok(None) => {
            return (
                400,
                format!("no template called \"{id}\" is saved for this project - list_api_templates shows the ones that are"),
            )
        }
        Err(e) => return (400, e),
    };
    let req = RunRequest {
        org: ctx.org.clone(),
        project: ctx.project.clone(),
        account: call.account,
        values: call.values,
        mode: Mode::Run,
        template,
    };
    run_api_template_request(ctx, &root, req, None, call.browser, open, open_db, timing).await
}

/// An assistant's free text as one app-log line: every run of whitespace
/// (newlines included) one space, and at most 200 characters.
fn one_short_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(200).collect()
}

/// What a prove and a run share once the call is read: the one-at-a-time
/// slot, taken first so no environment switch can come between the checks
/// and the run; every check together; for a template on a flow, the gate -
/// the stages before its own, asked of the database - before the browser
/// opens; the run, then the bookkeeping - a proven template saved with its
/// evidence (on a flow, only once its own stage checks done), the run
/// appended to the template's history, and the tab told. 200 with the
/// report when it passed, 502 with the report when it did not.
///
/// `open_db` is called at most once, and never for a template on no flow:
/// those run exactly as they did before flows existed, database or not.
#[allow(clippy::too_many_arguments)]
async fn run_api_template_request<B: crate::autorun::replay::Browsers, D: crate::api_templates::gate::StageDb>(
    ctx: &BridgeContext,
    root: &std::path::Path,
    req: crate::api_templates::runner::RunRequest,
    existing: Option<&crate::api_templates::ApiTemplate>,
    which: crate::browser::launch::Browser,
    open: impl FnOnce(crate::browser::launch::Browser) -> B,
    open_db: impl FnOnce(&BridgeContext) -> Result<D, (u16, String)>,
    timing: &crate::browser::timing::Timing,
) -> (u16, String) {
    use crate::api_templates::flow::{check_stage_ref, creating_stage};
    use crate::api_templates::gate::{self, CheckFor, StageState};
    use crate::api_templates::runner::{claim, preflight, run_template, stage_flow, Mode};
    use crate::api_templates::store::{self, RunRecord};
    use crate::api_templates::{ApiTemplate, Proven};
    // The slot first, before the template, its flow or the database is
    // looked at: holding it is what refuses an environment switch, so none
    // can land between those checks and the run. Every return below drops
    // it, as the run's end does.
    let Some(_claim) = claim() else {
        return (409, API_TEMPLATE_BUSY.to_string());
    };
    if let Err(problems) = preflight(root, &req, existing) {
        return (400, problems.join("\n"));
    }
    let id = req.template.id.as_str();
    let (org, project) = (req.org.as_str(), req.project.as_str());

    // The flow preflight has just found. Gone in between - replaced by
    // another call - is refused as preflight would have refused it.
    let on_flow = match &req.template.stage {
        None => None,
        Some(r) => match stage_flow(root, org, project, &req.template) {
            Some(f) => Some((f, r.id.clone())),
            None => return (400, check_stage_ref(&req.template, None).join("\n")),
        },
    };
    // The creating stage has no record to gate on; its prove still needs
    // the database afterwards, so that is asked for before the browser too,
    // rather than after the steps have already written.
    let mut db = None;
    if let Some((f, stage_id)) = &on_flow {
        let creates = creating_stage(f).is_some_and(|s| &s.id == stage_id);
        if !creates || matches!(req.mode, Mode::Prove { .. }) {
            let d = match open_db(ctx) {
                Ok(d) => d,
                Err(refused) => return refused,
            };
            if !creates {
                let value = req.values.get(&f.subject.name).cloned().unwrap_or(serde_json::Value::Null);
                let templates = saved_templates(root, org, project);
                if let Err(sentence) = gate::gate(&d, f, stage_id, &value, &templates, id).await {
                    return (400, sentence);
                }
            }
            db = Some(d);
        }
    }

    let mut browsers = open(which);
    let report = run_template(&mut browsers, root, &req, timing).await;
    drop(browsers);

    // A proven template on a flow must have done its own stage: the
    // subject from the values, or - creating it - from what was captured.
    // The flow is read again first: one replaced or removed while the steps
    // ran decides what is saved, and a stage it no longer has is not.
    let mut incomplete = None;
    if let (true, Mode::Prove { .. }, Some((_, stage_id)), Some(d)) = (report.ok, &req.mode, &on_flow, &db) {
        let reloaded = stage_flow(root, org, project, &req.template);
        match reloaded.as_ref().and_then(|f| f.stages.iter().find(|s| &s.id == stage_id).map(|s| (f, s))) {
            None => {
                let flow_id = req.template.stage.as_ref().map(|r| r.flow.as_str()).unwrap_or_default();
                let gone = match &reloaded {
                    None => format!("this template's flow {flow_id} is no longer saved"),
                    Some(_) => format!("stage \"{stage_id}\" is no longer in flow {flow_id}"),
                };
                crate::applog::info(format!("api template {id}: every step passed but {gone}, so it was not saved"));
                incomplete =
                    Some(format!("every step passed, but {gone}, so the template was not saved; {}", report.message()));
            }
            Some((f, stage)) => {
                let name = &f.subject.name;
                let value = if stage.creates { report.outputs.get(name) } else { req.values.get(name) }
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                // A creating stage's subject is what a step captured: one of
                // the wrong type is said as that, never as a check that could
                // not run - a retry would only create another record with the
                // same capture.
                let wrong_type = if stage.creates { gate::validate_value(f, &value).err() } else { None };
                if let Some(e) = wrong_type {
                    let kind = f.subject.kind.word();
                    crate::applog::info(format!(
                        "api template {id}: every step passed but the captured {name} is not a {kind}, so it was not saved"
                    ));
                    incomplete = Some(format!(
                        "every step passed, but the captured {name} is not a {kind} ({e}), so the template was not saved; {}",
                        report.message()
                    ));
                } else {
                    let why = CheckFor { purpose: "prove", template: Some(id) };
                    // A check that could not run - a database error, a
                    // timeout, no row count - is never "not done": either way
                    // nothing is saved, but each says what it is.
                    match gate::stage_state(d, f, stage, &value, &why).await {
                        StageState::Done => {}
                        StageState::NotDone => {
                            crate::applog::info(format!(
                                "api template {id}: every step passed but stage {} is not done, so it was not saved",
                                stage.id
                            ));
                            incomplete = Some(format!(
                                "every step passed, but {} is still not done for {name} {}, so the template was not saved; {}",
                                stage.title,
                                gate::shown(&value),
                                report.message()
                            ));
                        }
                        StageState::CouldNotRun => {
                            crate::applog::info(format!(
                                "api template {id}: every step passed but the check for stage {} could not be run, so it was not saved",
                                stage.id
                            ));
                            incomplete = Some(format!(
                                "every step passed, but the check for {} could not be run - see the activity folder in Settings, Logs, so the template was not saved; {}",
                                stage.title,
                                report.message()
                            ));
                        }
                    }
                }
            }
        }
    }

    let mut changed = false;
    let mut not_saved = None;
    if let (true, Mode::Prove { replace, why }, None) = (report.ok, &req.mode, &incomplete) {
        let proven = Proven {
            at: crate::applog::stamp(),
            origin: recipe_origin(root, org, project).unwrap_or_default(),
            account: req.account.clone(),
            outputs: report.outputs.clone(),
            // The run held the template slot, so no switch happened since
            // it signed in: the active environment is the one it ran in.
            environment: crate::environments::active(root).ok().map(|e| e.name),
        };
        let t = ApiTemplate { proven: Some(proven), ..req.template.clone() };
        match store::save(root, org, project, &t) {
            Ok(()) => {
                changed = true;
                if existing.is_some() && *replace {
                    crate::applog::info(format!("api template {id} replaced: {}", one_short_line(why.as_deref().unwrap_or(""))));
                }
            }
            Err(e) => {
                crate::applog::warn(format!("api template {id}: the proven template could not be saved: {e}"));
                not_saved = Some(format!(
                    "{} - but the template could not be saved; see Settings, Logs",
                    report.message()
                ));
            }
        }
    }
    // Every run gets a line in its template's history, and so does the
    // prove that saved it. A failed prove saved nothing - over an existing
    // id the saved template is still the one proven before - so it gets
    // no line: the assistant already has the failure, and a red "last
    // run" on a template that never ran would be a lie.
    let saved_by_this_prove = changed;
    if matches!(req.mode, Mode::Run) || saved_by_this_prove {
        let record = RunRecord {
            at: crate::applog::stamp(),
            mode: req.mode.label().to_string(),
            account: req.account.clone(),
            ok: report.ok,
            failed_step: report.failed.clone(),
            detail: (!report.ok).then(|| report.message()),
            // What the run actually left behind: its outputs, or - when it
            // stopped - everything it had captured by then.
            outputs: if report.ok { report.outputs.clone() } else { report.created.clone() },
        };
        match store::append_run(root, org, project, id, record) {
            Ok(()) => changed = true,
            Err(e) => crate::applog::warn(format!("api template {id}: the run could not be added to its history: {e}")),
        }
    }
    if changed {
        templates_changed(id);
    }
    if let Some(sentence) = incomplete {
        return (502, sentence);
    }
    if let Some(sentence) = not_saved {
        return (500, sentence);
    }
    let text = serde_json::to_string(&report).unwrap_or_default();
    (if report.ok { 200 } else { 502 }, text)
}

const FLOW_SAVE_SHAPE: &str = "{ \"flow\": <the flow>, \"sample\": <the subject of a real record>, \"replace\"?: true, \"why\"?: \"<reason>\" }";
const FLOW_PROGRESS_SHAPE: &str = "{ \"flow\": \"<flow id>\", \"subject\": <the record's id> }";

/// `POST /api-template-flow-save` (design doc "API template flows" §6): a
/// flow is saved only once every stage's check has run on `sample`, a real
/// record - done or not done both fine, a check that could not run is not.
/// Every problem with the call comes back together, before the database is
/// asked for. Replacing a saved flow needs `replace: true` and a `why`, and
/// the answer names the saved templates whose stage it no longer has: they
/// are refused until a replacement is proven. The tab is told through the
/// same sink a prove or a run uses. Writes nothing to the application, so
/// the API templates switch is not needed.
pub async fn api_template_flow_save<D: crate::api_templates::gate::StageDb>(
    ctx: &BridgeContext,
    body: &str,
    open_db: impl FnOnce(&BridgeContext) -> Result<D, (u16, String)>,
) -> (u16, String) {
    use crate::api_templates::flow::{parse_flow, FlowSaved};
    use crate::api_templates::flow_store;
    use crate::api_templates::gate::{self, CheckFor, StageState};
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return (400, format!("that is not readable JSON: {e}. Expected {FLOW_SAVE_SHAPE}.")),
    };
    let Some(draft) = json_arg(v.get("flow")) else {
        return (400, format!("this call needs a \"flow\". Expected {FLOW_SAVE_SHAPE}."));
    };
    let sample = match v.get("sample") {
        None | Some(serde_json::Value::Null) => None,
        Some(s) => Some(s.clone()),
    };
    let replace = v["replace"].as_bool().unwrap_or(false);
    let why = v["why"].as_str().map(str::to_string);
    let (org, project) = (ctx.org.as_str(), ctx.project.as_str());

    let mut problems = Vec::new();
    let parsed = match parse_flow(&draft) {
        Ok(f) => Some(f),
        Err(p) => {
            problems.extend(p);
            None
        }
    };
    match (&parsed, &sample) {
        (_, None) => problems.push(format!(
            "this call needs a \"sample\": the subject of a real record, found with db_query. Expected {FLOW_SAVE_SHAPE}."
        )),
        (Some(f), Some(s)) => {
            if let Err(e) = gate::validate_value(f, s) {
                problems.push(e);
            }
        }
        (None, Some(_)) => {}
    }
    let mut replacing = false;
    if let Some(f) = &parsed {
        replacing = match flow_store::load(&root, org, project, &f.id) {
            Ok(found) => found.is_some(),
            // A saved file that no longer reads still stands for a flow of
            // that id, so replacing it needs the same reason.
            Err(e) => {
                crate::applog::warn(format!("api template flow {}: the saved copy could not be read: {e}", f.id));
                true
            }
        };
        let has_why = why.as_deref().is_some_and(|w| !w.trim().is_empty());
        if replacing && !(replace && has_why) {
            problems.push(format!(
                "a flow called \"{}\" already exists - send replace: true and a why to change it",
                f.id
            ));
        }
    }
    let (Some(mut f), Some(sample), true) = (parsed, sample, problems.is_empty()) else {
        return (400, problems.join("\n"));
    };

    let db = match open_db(ctx) {
        Ok(d) => d,
        Err(refused) => return refused,
    };
    let checked = CheckFor { purpose: "save", template: None };
    let mut results: Vec<(String, String, StageState)> = Vec::with_capacity(f.stages.len());
    for s in &f.stages {
        let state = gate::stage_state(&db, &f, s, &sample, &checked).await;
        results.push((s.id.clone(), s.title.clone(), state));
    }
    let failed: Vec<String> = results
        .iter()
        .filter(|(_, _, st)| *st == StageState::CouldNotRun)
        .map(|(_, title, _)| {
            format!(
                "the check for {title} could not be run on {} {} - see the activity folder in Settings, Logs",
                f.subject.name,
                gate::shown(&sample)
            )
        })
        .collect();
    if !failed.is_empty() {
        return (400, failed.join("\n"));
    }

    f.saved = Some(FlowSaved { at: crate::applog::stamp(), sample: sample.clone() });
    if let Err(e) = flow_store::save(&root, org, project, &f) {
        crate::applog::warn(format!("api template flow {}: it could not be saved: {e}", f.id));
        return (500, "the flow could not be saved - see Settings, Logs".to_string());
    }
    if replacing {
        crate::applog::info(format!(
            "api template flow {} replaced: {}",
            f.id,
            one_short_line(why.as_deref().unwrap_or(""))
        ));
    }

    let saved = saved_templates(&root, org, project);
    // The rest of the map: saved templates no flow places yet. A flow is
    // meant to take in every template found for its record, so the answer
    // hands the assistant the ones still outside one.
    let loose: Vec<(String, String, String)> = saved
        .iter()
        .filter(|s| s.template.stage.is_none())
        .map(|s| (s.template.id.clone(), s.template.title.clone(), s.template.module.clone()))
        .collect();
    let orphaned: Vec<(String, String, String)> = saved
        .into_iter()
        .filter_map(|s| {
            let r = s.template.stage?;
            (r.flow == f.id && !f.stages.iter().any(|st| st.id == r.id))
                .then_some((s.template.id, s.template.title, r.id))
        })
        .collect();
    let mut message = if orphaned.is_empty() {
        format!("flow {} saved; every check ran on {} {}", f.id, f.subject.name, gate::shown(&sample))
    } else {
        let ids: Vec<&str> = orphaned.iter().map(|(id, _, _)| id.as_str()).collect();
        format!(
            "flow {} saved. These templates perform a stage it no longer has, so they are refused until a replacement is proven: {}",
            f.id,
            ids.join(", ")
        )
    };
    if !loose.is_empty() {
        let ids: Vec<&str> = loose.iter().map(|(id, _, _)| id.as_str()).collect();
        message.push_str(&format!(
            ". {} saved template{} on no flow yet: {}. Place each one acting on this record on a stage of this flow (add the stage if it is missing), save the flow again, then re-prove the template with its stage set; one acting on another record belongs in that record's own flow.",
            loose.len(),
            if loose.len() == 1 { " is" } else { "s are" },
            ids.join(", ")
        ));
    }
    templates_changed(&f.id);
    let stages: Vec<serde_json::Value> = results
        .iter()
        .map(|(id, title, st)| serde_json::json!({ "id": id, "title": title, "done": *st == StageState::Done }))
        .collect();
    let orphaned: Vec<serde_json::Value> = orphaned
        .into_iter()
        .map(|(id, title, stage)| serde_json::json!({ "id": id, "title": title, "stage": stage }))
        .collect();
    let loose: Vec<serde_json::Value> = loose
        .into_iter()
        .map(|(id, title, module)| serde_json::json!({ "id": id, "title": title, "module": module }))
        .collect();
    (
        200,
        serde_json::json!({
            "saved": f.id,
            "sample": sample,
            "stages": stages,
            "orphaned": orphaned,
            "not_on_a_flow": loose,
            "message": message,
        })
        .to_string(),
    )
}

/// `POST /api-template-flow-progress`: every stage of a saved flow for one
/// record, each checked once - `done`, `next`, `blocked`, `skippable` or
/// `could_not_check` - with the templates on each (design doc §7). Only
/// reads, so the API templates switch is not needed.
pub async fn api_template_flow_progress<D: crate::api_templates::gate::StageDb>(
    ctx: &BridgeContext,
    body: &str,
    open_db: impl FnOnce(&BridgeContext) -> Result<D, (u16, String)>,
) -> (u16, String) {
    use crate::api_templates::{flow_store, gate, valid_id};
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return (400, format!("that is not readable JSON: {e}. Expected {FLOW_PROGRESS_SHAPE}.")),
    };
    let id = match v.get("flow") {
        Some(serde_json::Value::String(s)) if !s.trim().is_empty() => s.trim().to_string(),
        _ => return (400, format!("this call needs a \"flow\" id. Expected {FLOW_PROGRESS_SHAPE}.")),
    };
    let subject = match v.get("subject") {
        None | Some(serde_json::Value::Null) => {
            return (400, format!("this call needs a \"subject\": the record's id. Expected {FLOW_PROGRESS_SHAPE}."))
        }
        Some(s) => s.clone(),
    };
    let not_saved =
        || format!("no flow called \"{id}\" is saved for this project - list_api_templates shows the ones that are");
    if !valid_id(&id) {
        return (400, not_saved());
    }
    let f = match flow_store::load(&root, &ctx.org, &ctx.project, &id) {
        Ok(Some(f)) => f,
        Ok(None) => return (400, not_saved()),
        Err(e) => {
            crate::applog::warn(format!("api template flow {id}: it could not be read: {e}"));
            return (400, format!("flow {id} could not be read - save it again with save_api_flow"));
        }
    };
    // The wrong type is said before any database is asked for.
    if let Err(e) = gate::validate_value(&f, &subject) {
        return (400, e);
    }
    let db = match open_db(ctx) {
        Ok(d) => d,
        Err(refused) => return refused,
    };
    let templates = saved_templates(&root, &ctx.org, &ctx.project);
    match gate::progress(&db, &f, &subject, &templates).await {
        Ok(stages) => (200, serde_json::json!({ "flow": f.id, "subject": subject, "stages": stages }).to_string()),
        Err(e) => (400, e),
    }
}

/// Said when a page route arrives with no browser behind it. The person
/// opens one; an assistant cannot, and telling it which button to name
/// is the difference between a dead end and a sentence it can pass on.
const NO_SUPERVISED_BROWSER: &str =
    "no supervised browser is open - the person opens one with Open browser on the Auto Run tab";

/// What every page route checks before it reaches for the browser at all:
/// an unattended run owns the browser while it is going, and the two
/// kinds of session are never open at once.
fn unattended_run_is_using_the_browser() -> Option<(u16, String)> {
    if crate::commands::autorun_replay::replay_is_running() {
        Some((409, "an unattended run is going - wait for it".to_string()))
    } else {
        None
    }
}

/// Where scripts, runs and quirks live, or the refusal to guess.
fn autorun_root() -> Result<std::path::PathBuf, (u16, String)> {
    crate::autorun::store::configured_root().ok_or((
        503,
        "the app could not set up its data directory this session - restart the app".to_string(),
    ))
}

/// The active environment's live guide section, or None when the list
/// cannot be read (said in the log; a guide without it still teaches the
/// format).
fn active_environment_section(root: &std::path::Path) -> Option<String> {
    match crate::environments::active(root) {
        Ok(env) => Some(crate::autorun::guide::active_environment_section(&env)),
        Err(e) => {
            crate::applog::warn(&format!("Guide: the active environment could not be read: {e}"));
            None
        }
    }
}

/// The guide's own text, plus this project's sections when it has any: the
/// module-screen rule while "Scripts may open pages by address" is off,
/// then the recorded quirks. The constant (`autorun::guide::autorun_guide`)
/// only says a quirks section exists; this reads what is actually on file,
/// so the guide can never go stale on a live project.
fn autorun_guide_with_quirks(ctx: &BridgeContext) -> String {
    let mut out = crate::autorun::guide::autorun_guide();
    let Some(root) = crate::autorun::store::configured_root() else {
        return out;
    };
    // The environment is the app's, not a project's: named even with no
    // project open.
    if let Some(section) = active_environment_section(&root) {
        out.push('\n');
        out.push_str(&section);
    }
    if ctx.project.trim().is_empty() {
        return out;
    }
    let nav = crate::autorun::nav::load_nav(&root, &ctx.org, &ctx.project).unwrap_or_default();
    let quirks = crate::autorun::quirks::load_quirks(&root, &ctx.org, &ctx.project).unwrap_or_default();
    let files = project_test_files(&root, ctx);
    for section in [
        crate::autorun::nav::guide_section(&nav),
        crate::autorun::quirks::quirks_section(&quirks),
        crate::test_files::guide_section(&files),
    ] {
        if !section.is_empty() {
            out.push('\n');
            out.push_str(&section);
        }
    }
    out
}

/// The page in the browser the person opened, as text: Chrome's own
/// accessibility tree with a locator on every line.
async fn autorun_page(target: &str) -> (u16, String) {
    if let Some(busy) = unattended_run_is_using_the_browser() {
        return busy;
    }
    // Capped as well as defaulted: a snapshot is text an assistant has to
    // read, and a limit of a million turns "see the page" into a reply
    // nothing can use. Ten times the default is already a very long page.
    let limit = q(target, "limit")
        .and_then(|s| s.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(crate::browser::snapshot::DEFAULT_LIMIT)
        .min(crate::browser::snapshot::DEFAULT_LIMIT * 10);
    // The lock is held for exactly one protocol job - whoever holds it
    // holds the browser, and the person may be using it.
    let mut slot = crate::commands::autorun::supervised().lock().await;
    let Some(session) = slot.as_mut() else {
        return (409, NO_SUPERVISED_BROWSER.to_string());
    };
    match crate::browser::snapshot::snapshot(&mut session.cdp, limit).await {
        Ok(text) => (200, text),
        Err(e) => (503, format!("the browser did not answer: {e}")),
    }
}

/// One named field out of a small JSON body, or a refusal that says what
/// the body should have carried.
fn body_field(body: &str, key: &str, shape: &str) -> Result<serde_json::Value, (u16, String)> {
    let v: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| (400, format!("that is not readable JSON: {e}. Expected {shape}.")))?;
    match v.get(key) {
        Some(found) if !found.is_null() => Ok(found.clone()),
        _ => Err((400, format!("this call needs a \"{key}\". Expected {shape}."))),
    }
}

/// What a locator matches on the open page right now.
async fn autorun_probe(body: &str) -> (u16, String) {
    let selector = match body_field(body, "selector", "{ \"selector\": <a script's target> }") {
        Ok(v) => v,
        Err(refused) => return refused,
    };
    let target: crate::browser::locator::Target = match serde_json::from_value(selector) {
        Ok(t) => t,
        Err(e) => return (400, format!("that is not a locator: {e}")),
    };
    if let Err(why) = target.validate() {
        return (400, why);
    }
    if let Some(busy) = unattended_run_is_using_the_browser() {
        return busy;
    }
    let mut slot = crate::commands::autorun::supervised().lock().await;
    let Some(session) = slot.as_mut() else {
        return (409, NO_SUPERVISED_BROWSER.to_string());
    };
    match crate::browser::snapshot::probe(&mut session.cdp, &target).await {
        Ok(text) => (200, text),
        Err(e) => (503, format!("the browser did not answer: {e}")),
    }
}

/// The applog line for a tried action: its kind, what it points at (a
/// locator's own words, or a navigate's url / a check_url's address), and
/// whether it worked. Pure and separate from the route so it can be
/// tested without a browser. A `fill`'s VALUE is never in it - only its
/// selector, the same as every other selector-carrying kind.
pub fn describe_try(action: &crate::browser::actions::Action, ok: bool) -> String {
    use crate::browser::actions::Action;
    let kind = serde_json::to_value(action)
        .ok()
        .and_then(|v| v["kind"].as_str().map(str::to_string))
        .unwrap_or_default();
    let what = match action {
        Action::Navigate { url } => url.clone(),
        Action::CheckUrl { contains } => contains.clone(),
        Action::CheckText { .. } | Action::SignIn { .. } => String::new(),
        // Never a query string: a fragment or a path can carry a token there.
        Action::ExpectResponse { url_contains: address, .. } | Action::ApiRequest { path: address, .. } => {
            crate::autorun::report::without_query(address.trim()).to_string()
        }
        Action::Click { selector }
        | Action::Fill { selector, .. }
        | Action::WaitFor { selector, .. }
        | Action::ExpectVisible { selector, .. }
        | Action::ExpectHidden { selector, .. }
        | Action::ExpectText { selector, .. }
        | Action::ExpectContainsText { selector, .. }
        | Action::ExpectCount { selector, .. }
        | Action::ExpectAttribute { selector, .. }
        | Action::Upload { selector, .. } => selector.describe(),
    };
    let target = if what.is_empty() { String::new() } else { format!(" {what}") };
    format!(
        "AI tried {kind}{target} in the supervised browser: {}",
        if ok { "ok" } else { "failed" }
    )
}

/// Run ONE action against the open page, so an assistant can find out
/// whether a repair works before it writes the repair down.
async fn autorun_try(ctx: &BridgeContext, body: &str) -> (u16, String) {
    let raw = match body_field(body, "action", "{ \"action\": <one script action> }") {
        Ok(v) => v,
        Err(refused) => return refused,
    };
    let action: crate::browser::actions::Action = match serde_json::from_value(raw) {
        Ok(a) => a,
        Err(e) => {
            return (
                400,
                format!("that is not an action: {e} - call get_autorun_guide for the vocabulary."),
            )
        }
    };
    // Signing in needs the tester's own accounts and the project's
    // recipe. It is theirs to drive, and a script never carries a login.
    if matches!(action, crate::browser::actions::Action::SignIn { .. }) {
        return (400, "sign_in is not a thing an assistant does - the person signs in".to_string());
    }
    if let Err(why) = action.validate() {
        return (400, why);
    }
    // `validate()` allows `file://` (a saved script may need it for the
    // live fixture, a development-only tab) but a TRIED action runs
    // against whatever page the person actually has open - sending their
    // browser to a local file is never something a rehearsal should do.
    if let crate::browser::actions::Action::Navigate { url } = &action {
        if url.trim().to_ascii_lowercase().starts_with("file:") {
            return (400, "a tried navigate goes to http or https only".to_string());
        }
    }
    if let Some(busy) = unattended_run_is_using_the_browser() {
        return busy;
    }
    // Everything that can fail without the browser is settled BEFORE the
    // lock: whoever holds it holds the browser the person is using, and
    // a session held open to look up a path is a session held for no
    // reason.
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let mut slot = crate::commands::autorun::supervised().lock().await;
    let Some(session) = slot.as_mut() else {
        return (409, NO_SUPERVISED_BROWSER.to_string());
    };
    // A step of one, numbered 0 - it belongs to no case, and nothing
    // records it. `run_step` is still what carries it out, so a tried
    // action behaves exactly as it will inside a script - the runner's own
    // kinds included: a tried `expect_response` takes its mark as the try
    // starts, and checks a request the page makes while it waits.
    //
    // `{{username}}` and `{{password}}` are refused where a script is
    // SAVED, not here: a tried `fill` is not on its way into a file, and
    // it types the literal text it was given rather than standing in for
    // anything a recipe would have substituted.
    let step = crate::autorun::StepScript { step_number: 0, actions: vec![action.clone()], unchecked: None };
    let outcomes = match crate::autorun::runner::run_step(
        &mut session.cdp,
        &root,
        &ctx.org,
        &ctx.project,
        &step,
        &crate::browser::timing::Timing::default(),
        &mut session.account,
    )
    .await
    {
        Ok(v) => v,
        Err(why) => return (500, why),
    };
    let Some(outcome) = outcomes.first() else {
        return (500, "the action produced no outcome".to_string());
    };
    // Shown to a person reading Settings -> Logs, never returned to the
    // assistant - and never a `fill`'s VALUE, which `describe_try` never
    // even looks at.
    crate::applog::info(describe_try(&action, outcome.ok));
    let mut text =
        format!("{}: {}", if outcome.ok { "ok" } else { "failed" }, outcome.detail);
    if let Some(shot) = &outcome.screenshot {
        text.push_str(&format!(" (picture: {shot})"));
    }
    (200, text)
}

/// A run's failed cases, as text an assistant can act on - and, when it
/// must not touch the script at all, the reason why.
fn autorun_failures(target: &str) -> (u16, String) {
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    // A `case_id` that was sent but is not a number is a mistake worth
    // naming: read as "no case given" it would quietly answer about the
    // newest run instead, which is a different question.
    let case_id = match q(target, "case_id") {
        None => None,
        Some(raw) => match raw.trim().parse::<i32>() {
            Ok(id) => Some(id),
            Err(_) => return (400, "case_id must be a number".to_string()),
        },
    };
    let run = match q(target, "run_id") {
        Some(id) => match crate::autorun::store::load_run(&root, &id) {
            Ok(Some(run)) => run,
            Ok(None) => return (404, format!("no run {id}")),
            // The store's own message names the file's path, and a path
            // is nothing the reader can act on.
            Err(_) => return (400, "the run file could not be read".to_string()),
        },
        None => match crate::autorun::failures::latest_run(&root, case_id) {
            Some(run) => run,
            None => {
                return match case_id {
                    Some(id) => (404, format!("no run on this machine has case {id}")),
                    None => (404, "no run on this machine yet".to_string()),
                }
            }
        },
    };
    // The scripts are what turn "action 2 failed" into the action's own
    // JSON. A case with no script on disk is reported as such rather than
    // dropped, so this gathers whatever is there and lets the describer
    // say what is missing.
    let scripts: Vec<crate::autorun::CaseScript> = run
        .cases
        .iter()
        .filter_map(|c| crate::autorun::store::load_script(&root, c.case_id).ok().flatten())
        .collect();
    (200, crate::autorun::failures::describe_failures(&run, &scripts))
}

/// Record something learned about the application, attributed, so the
/// next script does not rediscover the same surprise.
///
/// `from` says which assistant's work it came out of: "autorun" (the
/// default) or "api", for one building API templates. Both read the same
/// list - one per project.
fn autorun_quirk(ctx: &BridgeContext, body: &str) -> (u16, String) {
    use crate::autorun::quirks::{record_quirk, Recorded, FROM_API, FROM_AUTORUN};
    let text = match body_field(body, "text", "{ \"text\": \"one line about this application\" }") {
        Ok(serde_json::Value::String(s)) => s,
        Ok(_) => return (400, "\"text\" is one line of text".to_string()),
        Err(refused) => return refused,
    };
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    let from = match v.get("from") {
        None | Some(serde_json::Value::Null) => FROM_AUTORUN,
        Some(serde_json::Value::String(s)) if s == FROM_AUTORUN => FROM_AUTORUN,
        Some(serde_json::Value::String(s)) if s == FROM_API => FROM_API,
        Some(_) => return (400, format!("\"from\" is \"{FROM_AUTORUN}\" or \"{FROM_API}\"")),
    };
    // The cases and steps it is about, when the assistant names them -
    // each step checked against the newest run of its case, so a note can
    // only be tied to steps that really failed. That tie is what later
    // runs count evidence against.
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct NamedCase {
        case_id: i32,
        steps: Vec<i32>,
    }
    let named: Vec<NamedCase> = match v.get("cases") {
        None | Some(serde_json::Value::Null) => Vec::new(),
        Some(c) => match serde_json::from_value(c.clone()) {
            Ok(list) => list,
            Err(e) => {
                return (400, format!("\"cases\" is a list of {{ case_id, steps: [number] }}: {e}"))
            }
        },
    };
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let mut sources: Vec<crate::autorun::quirks::QuirkSource> = Vec::with_capacity(named.len());
    for c in &named {
        let run = crate::autorun::failures::latest_run(&root, Some(c.case_id));
        let script = crate::autorun::store::load_script(&root, c.case_id).ok().flatten();
        match crate::autorun::quirks::source_from_run(run.as_ref(), script.as_ref(), c.case_id, &c.steps) {
            Ok(s) => sources.push(s),
            Err(why) => return (400, why),
        }
    }
    match record_quirk(
        &root,
        &ctx.org,
        &ctx.project,
        &text,
        "assistant",
        from,
        sources,
        crate::autorun::sessions::now_ms(),
    ) {
        Ok(Recorded::Added(id)) => (200, format!("recorded as {id}")),
        Ok(Recorded::AlreadyKnown(id)) => (200, format!("already known, as {id}")),
        Ok(Recorded::Reactivated(id, reason)) => (200, reactivated_reply(&id, reason.as_deref())),
        Err(why) => (400, why),
    }
}

/// What an assistant is told when its line brought a retired note back.
fn reactivated_reply(id: &str, reason: Option<&str>) -> String {
    match reason {
        Some(r) => format!("{id} had been retired (\"{r}\") - it is back on the list; check that reason no longer holds"),
        None => format!("{id} had been retired - it is back on the list"),
    }
}

/// Retire one of the assistant's own quirks, with a reason - and, when
/// `replacement` is given, file the better note in the same call, keeping
/// the old one's sources. A person's note is refused: it is theirs to
/// remove, in the app.
fn autorun_quirk_retire(ctx: &BridgeContext, body: &str) -> (u16, String) {
    use crate::autorun::quirks::{retire_in, update_quirks, Recorded};
    let shape = "{ \"id\": \"q1a2b3c\", \"reason\": \"why it no longer helps\", \"replacement\": \"optional better note\" }";
    let id = match body_field(body, "id", shape) {
        Ok(serde_json::Value::String(s)) => s,
        Ok(_) => return (400, "\"id\" is a quirk's id, as the guide shows it".to_string()),
        Err(refused) => return refused,
    };
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    let reason = match v.get("reason") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(_) => return (400, "\"reason\" is one sentence".to_string()),
    };
    let replacement = match v.get("replacement") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(_) => return (400, "\"replacement\" is one line of text".to_string()),
    };
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let now = crate::autorun::sessions::now_ms();
    match update_quirks(&root, &ctx.org, &ctx.project, |list| {
        retire_in(list, &id, reason.as_deref(), replacement.as_deref(), true, now)
    }) {
        Ok((None, _)) => (200, format!("retired {}", id.trim())),
        Ok((Some(Recorded::Added(new)), _)) => (200, format!("retired {}, replaced by {new}", id.trim())),
        Ok((Some(Recorded::AlreadyKnown(new) | Recorded::Reactivated(new, _)), _)) => {
            (200, format!("retired {} - the replacement is already on the list as {new}", id.trim()))
        }
        Err(why) => (400, why),
    }
}

// ------------------------------------------------- the environment's accounts

/// `POST /accounts-propose`: the assistant's proposed logins for the active
/// environment, REPLACING whatever it proposed before. Never a password -
/// a field the shape does not have is refused, `password` included - and
/// nothing reaches the accounts until a person adds it in the app.
fn accounts_propose(body: &str) -> (u16, String) {
    use crate::environments::{active, save_proposals, ProposedAccount};
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Proposal {
        key: String,
        #[serde(default)]
        label: String,
        username: String,
        #[serde(default)]
        role: Option<String>,
    }
    let shape = "{ \"accounts\": [{ \"key\": \"hr.supervisor\", \"label\": \"HR supervisor\", \"username\": \"sup1\", \"role\"?: \"Supervisor\" }] }";
    let raw = match body_field(body, "accounts", shape) {
        Ok(v) => v,
        Err(refused) => return refused,
    };
    let raw = json_arg(Some(&raw)).unwrap_or(raw);
    let list: Vec<Proposal> = match serde_json::from_value(raw) {
        Ok(l) => l,
        Err(e) => {
            return (400, format!("\"accounts\" is a list of {{ key, label, username, role? }} - never a password: {e}"))
        }
    };
    if list.is_empty() {
        return (400, format!("\"accounts\" needs at least one account. Expected {shape}."));
    }
    let proposals: Vec<ProposedAccount> = list
        .into_iter()
        .map(|p| {
            let key = p.key.trim().to_string();
            let label = match p.label.trim() {
                "" => key.clone(),
                l => l.to_string(),
            };
            let role = p.role.map(|r| r.trim().to_string()).filter(|r| !r.is_empty());
            ProposedAccount { key, label, username: p.username.trim().to_string(), role }
        })
        .collect();
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let env = match active(&root) {
        Ok(e) => e,
        Err(e) => return (500, e),
    };
    if let Err(why) = save_proposals(&root, &env.id, &proposals) {
        return (400, why);
    }
    crate::applog::info(format!(
        "Environments: the assistant proposed {} account(s) for {}",
        proposals.len(),
        env.name
    ));
    (
        200,
        format!(
            "proposed {} account(s) for the environment {} - a person picks which to add, and gives each its password, in the app. Another call replaces this proposal.",
            proposals.len(),
            env.name
        ),
    )
}

/// Said when `GET /accounts` arrives while the person has switched the
/// `get_accounts` tool off.
pub const GET_ACCOUNTS_OFF: &str =
    "reading the accounts is switched off - turn on get_accounts on the AI Bridge tab";

/// `GET /accounts`: the active environment's accounts - key, label and
/// username, and the password ONLY when the environment is marked as a
/// test environment. The one place a password leaves the app; it is never
/// logged, and the log line says only how many were read.
///
/// Refused while the person has switched `get_accounts` off on the AI
/// Bridge tab. The MCP proxy already hides a switched-off tool; this route
/// checks again itself, as the database routes do (`stage_db`), because it
/// is the one that can hand out passwords.
fn accounts_read(ctx: &BridgeContext) -> (u16, String) {
    if ctx.disabled_tools.iter().any(|t| t == "get_accounts") {
        return (409, GET_ACCOUNTS_OFF.to_string());
    }
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let env = match crate::environments::active(&root) {
        Ok(e) => e,
        Err(e) => return (500, e),
    };
    // Read by the id just read, so the accounts and the test-environment
    // mark are the same environment's even if a switch lands in between.
    let accounts = match crate::autorun::accounts::load_accounts_for(&root, &env.id) {
        Ok(a) => a,
        Err(e) => return (500, e),
    };
    let shown = env.test_environment;
    let list: Vec<serde_json::Value> = accounts
        .iter()
        .map(|a| {
            let mut one = serde_json::json!({ "key": a.key, "label": a.label, "username": a.username });
            if shown {
                one["password"] = serde_json::Value::String(a.password.clone());
            }
            one
        })
        .collect();
    let mut out = serde_json::json!({
        "environment": env.name,
        "test_environment": shown,
        "accounts": list,
    });
    if !shown {
        out["note"] = serde_json::Value::String(format!(
            "{} is not marked as a test environment, so no password is shown - scripts and templates name an account by its key",
            env.name
        ));
    }
    crate::applog::info(format!(
        "AI bridge: the assistant read the {} account(s) of {}{}",
        accounts.len(),
        env.name,
        if shown { ", passwords included" } else { "" }
    ));
    (200, out.to_string())
}

// ------------------------------------------------------- the company database

/// The two things both database routes need before they can do anything:
/// a connection the person chose, and a sqlcmd to run it with. Neither is
/// a failure of the call - both are 409, "the app is not set up for this
/// yet" - and each says which of the two is missing.
fn db_ready(
    ctx: &BridgeContext,
) -> Result<(crate::db::Connection, std::path::PathBuf), (u16, String)> {
    let nothing_chosen = || (409, crate::db::query::NO_CONNECTION.to_string());
    // An id this build does not know - one saved for a preset a later
    // release removed - is nothing chosen too: the person's next step is
    // the same, pick a database.
    let store = ctx.db_secrets.as_deref().ok_or_else(nothing_chosen)?;
    let id = ctx
        .db_id
        .as_deref()
        .map(str::trim)
        .filter(|id| crate::db::credentials::is_known(store, id))
        .ok_or_else(nothing_chosen)?;
    // Resolved now, not when the context was pushed: a login saved since
    // is the one this call signs in with. `own` with nothing saved HAS been
    // chosen, so it gets its own sentence - "pick one" would send the person
    // back to a choice they already made. A store that cannot be read keeps
    // its own sentence too: picking again won't help.
    let chosen = crate::db::credentials::resolve(store, id)
        .map_err(|why| (409, why))?
        .ok_or_else(|| (409, crate::db::query::NO_LOGIN_SAVED.to_string()))?;
    // The error names the missing key and nothing else - the rest of that
    // string is a credential, and this sentence is shown to a person.
    let connection = crate::db::parse_connection(&chosen)
        .map_err(|why| (409, format!("{why} - choose a connection under Database Read Access on the AI Bridge tab")))?;
    let exe = crate::db::sqlcmd_path()
        .ok_or_else(|| (409, crate::db::NOT_INSTALLED.to_string()))?;
    Ok((connection, exe))
}

/// Said when a template on a flow is proven or run with no database chosen:
/// its gate is a set of database checks, so without one it cannot run.
pub const FLOW_NEEDS_DB: &str =
    "this template belongs to a flow, and flow checks need a database: choose one on the AI Bridge tab";

/// Said when a flow check would run while the person has switched off
/// Database Read Access: every check is an assistant-written read of
/// that database, so none runs until reading is switched on again.
pub const FLOW_NEEDS_READING: &str =
    "flow checks read the company database: switch on Database Read Access on the AI Bridge tab";

/// The chosen database as the place flow checks are asked, with
/// `db_ready`'s refusals as they are - what saving a flow and a flow's
/// progress answer with. Refused first while database reading is switched
/// off (`db_query` among the disabled tools), before any connection is
/// looked at.
fn stage_db(
    ctx: &BridgeContext,
) -> Result<crate::api_templates::gate::SqlcmdStageDb<crate::db::RealRunner>, (u16, String)> {
    if ctx.disabled_tools.iter().any(|t| t == "db_query") {
        return Err((409, FLOW_NEEDS_READING.to_string()));
    }
    let (conn, exe) = db_ready(ctx)?;
    Ok(crate::api_templates::gate::SqlcmdStageDb { runner: crate::db::RealRunner, exe, conn })
}

/// The database a prove or a run of a template on a flow gates on. With
/// nothing chosen the answer says why a template needs one at all
/// (`FLOW_NEEDS_DB`), with `db_ready`'s own status; reading switched off is
/// `stage_db`'s `FLOW_NEEDS_READING`; every other refusal - no login saved,
/// a store that cannot be read, no sqlcmd - is `db_ready`'s, unchanged.
pub fn real_stage_db(
    ctx: &BridgeContext,
) -> Result<crate::api_templates::gate::SqlcmdStageDb<crate::db::RealRunner>, (u16, String)> {
    stage_db(ctx).map_err(|(status, why)| {
        if why == crate::db::query::NO_CONNECTION {
            (status, FLOW_NEEDS_DB.to_string())
        } else {
            (status, why)
        }
    })
}

/// A body's JSON, parsed once, or the refusal that names what could not be
/// read. Both database routes need more than one field out of the same
/// body (`db_lookup` reads `query` and `limit`), so parsing happens here
/// and every field after this is read from the one `Value`.
fn db_body(body: &str, shape: &str) -> Result<serde_json::Value, (u16, String)> {
    serde_json::from_str(body)
        .map_err(|e| (400, format!("that is not readable JSON: {e}. Expected {shape}.")))
}

/// One named string out of an already-parsed body, trimmed, or the
/// refusal that says what the body should have carried. Separate from
/// `body_field` because these two routes want the empty string and the
/// missing key to read the same way: neither is a question.
fn db_body_text(parsed: &serde_json::Value, key: &str, shape: &str) -> Result<String, (u16, String)> {
    let text = parsed.get(key).and_then(|v| v.as_str()).unwrap_or_default().trim().to_string();
    if text.is_empty() {
        return Err((400, format!("this call needs a \"{key}\". Expected {shape}.")));
    }
    Ok(text)
}

/// The tables and columns behind some words, or one table's own columns.
async fn db_lookup(ctx: &BridgeContext, body: &str) -> (u16, String) {
    const SHAPE: &str = "{ \"query\": \"leave request\", \"limit\": 10 }";
    let parsed = match db_body(body, SHAPE) {
        Ok(v) => v,
        Err(refused) => return refused,
    };
    let query = match db_body_text(&parsed, "query", SHAPE) {
        Ok(q) => q,
        Err(refused) => return refused,
    };
    let limit = crate::db::query::lookup_limit(parsed.get("limit").and_then(|l| l.as_i64()));
    let (connection, exe) = match db_ready(ctx) {
        Ok(ready) => ready,
        Err(refused) => return refused,
    };
    // Kept here, not in `run_lookup`, so the lookup itself stays a plain
    // function of what sqlcmd answers - and only an answer is kept, never
    // a failure, so a server that was briefly unreachable is asked again.
    let key = crate::cache::keys::db_lookup(
        &connection.server,
        &connection.database,
        limit,
        &query.to_lowercase(),
    );
    if let Some(hit) = crate::cache::session_fresh::<String>(&key, crate::cache::keys::DB_LOOKUP_TTL) {
        return (200, hit);
    }
    match crate::db::query::run_lookup(
        &crate::db::RealRunner,
        &exe,
        &connection,
        &query,
        limit,
    )
    .await
    {
        Ok(text) => {
            crate::cache::session_put(&key, text.clone());
            (200, text)
        }
        Err(refused) => refused,
    }
}

/// The statements of a batch body: each either `{ "sql": ..., "expect_rows":
/// N }` or a bare string. Anything else is named back rather than skipped -
/// a dropped statement would change what the transaction does.
fn batch_statements(list: &[serde_json::Value]) -> Result<Vec<crate::db::BatchStatement>, (u16, String)> {
    list.iter()
        .enumerate()
        .map(|(i, item)| {
            let n = i + 1;
            let (sql, expect_rows) = match item {
                serde_json::Value::String(s) => (s.clone(), None),
                serde_json::Value::Object(o) => {
                    let sql = o.get("sql").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                    let expect_rows = match o.get("expect_rows") {
                        None | Some(serde_json::Value::Null) => None,
                        Some(v) => Some(v.as_i64().ok_or_else(|| {
                            (400, format!("statement {n}: expect_rows must be a whole number"))
                        })?),
                    };
                    (sql, expect_rows)
                }
                _ => {
                    return Err((
                        400,
                        format!("statement {n} must be an object with \"sql\" (and optionally \"expect_rows\")"),
                    ))
                }
            };
            if sql.trim().is_empty() {
                return Err((400, format!("statement {n} has no \"sql\"")));
            }
            Ok(crate::db::BatchStatement { sql: sql.trim().to_string(), expect_rows })
        })
        .collect()
}

/// One statement, or a batch of them as one transaction, if the guard and
/// the two write doors allow it.
async fn db_query(ctx: &BridgeContext, body: &str) -> (u16, String) {
    const SHAPE: &str = "{ \"sql\": \"SELECT TOP (10) * FROM dbo.LeaveRequest\" } or { \"statements\": [{ \"sql\": \"UPDATE ...\", \"expect_rows\": 2 }], \"dry_run\": true }";
    let parsed = match db_body(body, SHAPE) {
        Ok(v) => v,
        Err(refused) => return refused,
    };
    let dry_run = parsed.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(false);
    // A single `sql` with dry_run is a batch of one: ignoring the flag
    // would run for real what the caller asked only to try.
    let batch = match (parsed.get("statements"), parsed.get("sql")) {
        (Some(_), Some(_)) => {
            return (400, format!("send \"sql\" or \"statements\", not both. Expected {SHAPE}."))
        }
        (Some(list), None) => match list.as_array() {
            Some(list) => Some(list.clone()),
            None => return (400, format!("\"statements\" must be a list. Expected {SHAPE}.")),
        },
        (None, Some(sql)) if dry_run => Some(vec![sql.clone()]),
        _ => None,
    };
    if let Some(list) = batch {
        let statements = match batch_statements(&list) {
            Ok(s) => s,
            Err(refused) => return refused,
        };
        let (connection, exe) = match db_ready(ctx) {
            Ok(ready) => ready,
            Err(refused) => return refused,
        };
        return match crate::db::query::run_batch_query(
            &crate::db::RealRunner,
            &exe,
            &connection,
            ctx.db_writes,
            &statements,
            dry_run,
        )
        .await
        {
            Ok(text) => (200, text),
            Err(refused) => refused,
        };
    }
    let sql = match db_body_text(&parsed, "sql", SHAPE) {
        Ok(s) => s,
        Err(refused) => return refused,
    };
    let (connection, exe) = match db_ready(ctx) {
        Ok(ready) => ready,
        Err(refused) => return refused,
    };
    match crate::db::query::run_query(
        &crate::db::RealRunner,
        &exe,
        &connection,
        ctx.db_writes,
        &sql,
    )
    .await
    {
        Ok(text) => (200, text),
        Err(refused) => refused,
    }
}

/// The body keys `save_autorun_script` reads. Anything else is named
/// back rather than dropped - a misspelled "edits" that was silently
/// ignored would let an undeclared repair through as if it were a new
/// script.
const SAVE_BODY_KEYS: [&str; 2] = ["scripts", "edits"];

struct SaveRequest {
    scripts: Vec<crate::autorun::CaseScript>,
    edits: Vec<crate::autorun::edits::Edit>,
}

fn bad_scripts(e: serde_json::Error) -> String {
    format!(
        "that is not a list of action scripts: {e}. Expected an array of {{ case_id, title, steps: [{{ step_number, actions }}] }} - call get_autorun_guide for the format."
    )
}

/// Either shape: the bare array a new bundle has always been, or the
/// object that carries the declarations a repair needs alongside it.
fn parse_save_request(body: &str) -> Result<SaveRequest, String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(bad_scripts)?;
    let map = match v {
        serde_json::Value::Array(_) => {
            return Ok(SaveRequest {
                scripts: serde_json::from_value(v).map_err(bad_scripts)?,
                edits: vec![],
            })
        }
        serde_json::Value::Object(map) => map,
        _ => {
            return Err(
                "that is not a list of action scripts. Send the array, or { \"scripts\": [...], \"edits\": [...] }."
                    .to_string(),
            )
        }
    };
    let unknown: Vec<String> = map
        .keys()
        .filter(|k| !SAVE_BODY_KEYS.contains(&k.as_str()))
        .map(|k| format!("\"{k}\""))
        .collect();
    if !unknown.is_empty() {
        return Err(format!(
            "this body carries {} save_autorun_script does not read: {}. It reads \"scripts\" and \"edits\".",
            if unknown.len() == 1 { "a key" } else { "keys" },
            unknown.join(", ")
        ));
    }
    let scripts_value = map.get("scripts").cloned().ok_or_else(|| {
        "this body has no \"scripts\". Send { \"scripts\": [...], \"edits\": [...] }.".to_string()
    })?;
    let edits = match map.get("edits") {
        None => vec![],
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| {
            format!(
                "that is not a list of declared edits: {e}. Each is {{ case_id, steps: [number], why, area (optional: true when the area changed), quirk (optional) }}."
            )
        })?,
    };
    Ok(SaveRequest { scripts: serde_json::from_value(scripts_value).map_err(bad_scripts)?, edits })
}

/// The case and steps a repair's quirk is about, with the class of the
/// failure that led to it (read from the newest run of that case).
fn repair_source(
    root: &std::path::Path,
    old: &crate::autorun::CaseScript,
    edit: &crate::autorun::edits::Edit,
) -> crate::autorun::quirks::QuirkSource {
    let run = crate::autorun::failures::latest_run(root, Some(edit.case_id));
    crate::autorun::quirks::source_for_repair(run.as_ref(), old, edit.case_id, &edit.steps)
}

/// Is the script sent word for word the one already on disk?
///
/// Compared the way the declared-edit gate compares steps - by
/// `step_signature`, so JSON formatting does not count as a change -
/// plus the three fields outside the steps a save can carry, `title`,
/// `account` and `area` (a blank area is no area; case does not tell two
/// area names apart). Positional rather than keyed by step number, so a bundle
/// that merely REORDERS the same steps counts as a change and goes
/// through the gate rather than around it. `repairs` is deliberately
/// not compared: it is never the sender's to set.
fn unchanged_script(old: &crate::autorun::CaseScript, sent: &crate::autorun::CaseScript) -> bool {
    old.title == sent.title
        && old.account == sent.account
        && old.area_name().map(crate::autorun::nav::module_key) == sent.area_name().map(crate::autorun::nav::module_key)
        && old.steps.len() == sent.steps.len()
        && old.steps.iter().zip(&sent.steps).all(|(a, b)| {
            a.step_number == b.step_number
                && crate::autorun::edits::step_signature(a)
                    == crate::autorun::edits::step_signature(b)
        })
}

/// Save one or many Auto Run action scripts, as an assistant writes them.
///
/// A BUNDLE by design: the body is an array, so a whole PBI's worth of
/// cases lands in one call - and the same shape is what the Auto Run
/// screen's Import button reads from a file. One case is a bundle of one.
///
/// ALL OR NOTHING, for real. Every gate below runs for every script
/// before a single byte is written: the body parses, each change to a
/// script that already exists is declared and takes a repair off the
/// cap, and each script meets its test case's expected results. Only
/// then does `store::save_scripts_atomically` run - and it validates and
/// serialises every entry before writing any of them, so a bad case id
/// or a filesystem error on entry 16 of 30 can never leave the other 29
/// half-applied. An unknown action `kind` fails here rather than
/// mid-run, with the browser already open in front of them.
///
/// `repairs` NEVER comes from the body. It is the count of changes made
/// without a person looking, and a sender that could set it could also
/// set it back to zero, which is the whole of what the cap prevents. A
/// script that comes back word for word unchanged costs nothing from
/// that count: re-saving a whole PBI after fixing one case is exactly
/// that, for every other case in the bundle.
async fn save_autorun_scripts(
    ctx: &BridgeContext,
    client: Option<&crate::ado::AdoClient>,
    body: &str,
) -> (u16, String) {
    let SaveRequest { scripts, edits } = match parse_save_request(body) {
        Ok(r) => r,
        Err(e) => return (400, e),
    };
    if scripts.is_empty() {
        return (400, "no scripts in the bundle".to_string());
    }
    // Capped before any lookup - disk or Azure DevOps - runs at all: the
    // floor's own lookup sends every id in the bundle through
    // `get_test_cases_by_ids` in one call, and `/test-cases` already caps
    // ITS ids list at the same number for the same reason.
    if scripts.len() > MAX_CASE_IDS {
        return (400, format!("a bundle can carry at most {MAX_CASE_IDS} scripts"));
    }
    // A declaration for a case this bundle is not saving changes nothing
    // and declares nothing - but it would still file its quirk, and it
    // usually means the wrong case id was typed into one of the two
    // lists. Named back before anything is read or written.
    let stray: Vec<String> = {
        let mut ids: Vec<i32> = edits
            .iter()
            .map(|e| e.case_id)
            .filter(|id| !scripts.iter().any(|s| s.case_id == *id))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids.into_iter().map(|id| id.to_string()).collect()
    };
    if !stray.is_empty() {
        return (
            400,
            format!(
                "edits names {} {}, which {} not in this bundle",
                if stray.len() == 1 { "case" } else { "cases" },
                stray.join(", "),
                if stray.len() == 1 { "is" } else { "are" }
            ),
        );
    }
    let root = match autorun_root() {
        // `set_root` runs exactly once, during app setup, and only when
        // `app_data_dir()` resolves - so a missing root is not something
        // this process will ever recover from on its own. Refuse rather
        // than invent a path: a script written somewhere the app does
        // not read would look saved and never appear.
        Err(refused) => return refused,
        Ok(r) => r,
    };

    // Gate 0: the project's own rules, before anything is read from disk
    // or Azure DevOps - a project whose runs start on the module screen
    // refuses a script that opens pages by address, and every project
    // refuses an `area` it has not recorded.
    if let Err(why) = crate::autorun::nav::check_project_rules(&root, &ctx.org, &ctx.project, &scripts) {
        return (400, why);
    }

    // Gate 1: what this bundle does to the scripts already on disk.
    let mut prepared: Vec<crate::autorun::CaseScript> = Vec::with_capacity(scripts.len());
    let mut lines: Vec<String> = Vec::with_capacity(scripts.len());
    let mut repair_sources: Vec<crate::autorun::quirks::QuirkSource> = Vec::new();
    for sent in &scripts {
        let existing = match crate::autorun::store::load_script(&root, sent.case_id) {
            Ok(v) => v,
            Err(e) => return (500, e),
        };
        let declared = edits.iter().find(|e| e.case_id == sent.case_id);
        let mut script = sent.clone();
        match existing {
            // A re-send that changes nothing is not a repair. The count
            // is of changes made without a person looking, so an
            // identical bundle - which is what saving a whole PBI again
            // after fixing one case IS, for every other case in it -
            // must neither raise it nor run into the cap. A declaration
            // for such a case is still refused, by `check_edits`' own
            // rule 2: it names a step nothing happened to.
            Some(old) if unchanged_script(&old, sent) => {
                if declared.is_some() {
                    if let Err(why) = crate::autorun::edits::check_edits(&old, sent, declared) {
                        return (400, why);
                    }
                }
                script.repairs = old.repairs;
                script.last_repair = old.last_repair.clone();
                lines.push(format!("case {} (unchanged)", script.case_id));
            }
            Some(old) => {
                if let Err(why) = crate::autorun::edits::check_edits(&old, sent, declared) {
                    return (400, why);
                }
                match crate::autorun::edits::next_repairs(&old) {
                    Ok(n) => script.repairs = n,
                    Err(why) => return (400, why),
                }
                // `check_edits` above only returns Ok when a real change
                // was fully declared, so `declared` is always Some here -
                // there is no path where a script actually differs from
                // disk and this branch is reached with nothing declared.
                if let Some(e) = declared {
                    let why = e.why.trim();
                    crate::applog::info(format!(
                        "AI repaired case {} steps {:?}: {why}",
                        script.case_id, e.steps
                    ));
                    script.last_repair = Some(why.to_string());
                    // Read now, while the script on disk is still the
                    // one that ran: its targets classify the failure.
                    if e.quirk.as_deref().is_some_and(|t| !t.trim().is_empty()) {
                        repair_sources.push(repair_source(&root, &old, e));
                    }
                }
                lines.push(format!(
                    "case {} (repaired, {} of {} used)",
                    script.case_id,
                    script.repairs,
                    crate::autorun::edits::MAX_REPAIRS
                ));
            }
            None => {
                if declared.is_some() {
                    return (
                        400,
                        format!(
                            "case {} has no script yet - \"edits\" is for changing one that exists",
                            sent.case_id
                        ),
                    );
                }
                script.repairs = 0;
                script.last_repair = None;
                lines.push(format!("case {} (new)", script.case_id));
            }
        }
        prepared.push(script);
    }

    // Gate 2: the expected-result floor. This is the one place the save
    // route reaches Azure DevOps, and only to READ the cases - a script
    // is judged against what its test case says should happen, never
    // against what the application happens to do.
    let Some(client) = client else {
        // 503, like every other route that needs a signed-in client - not
        // 400: nothing about the bundle itself is wrong, the app just has
        // nowhere to check it against yet.
        return (
            503,
            "sign in to Test Case Manager first - a script is checked against its test case before it is saved"
                .to_string(),
        );
    };
    let ids: Vec<i32> = prepared.iter().map(|s| s.case_id).collect();
    let cases = match client
        .get_test_cases_by_ids(
            &ctx.org,
            &ids,
            ctx.module_ref.as_deref(),
            ctx.preconditions_ref.as_deref(),
        )
        .await
    {
        Ok(cases) => cases,
        // Azure DevOps refuses the whole batch when any id is not a work
        // item, so it cannot say which - the ids are named back instead.
        Err(crate::ado::AdoError::NotFound) => {
            return (
                400,
                format!(
                    "Azure DevOps has no work item for at least one of {} - a script is saved against a real test case",
                    ids.iter().map(|i| format!("#{i}")).collect::<Vec<_>>().join(", ")
                ),
            )
        }
        Err(e) => return (502, format!("Azure DevOps error: {e:?}")),
    };
    for script in &prepared {
        let Some(case) = cases.iter().find(|c| c.id == script.case_id) else {
            return (
                400,
                format!("case {} is not a test case in this organization", script.case_id),
            );
        };
        let shortfalls =
            crate::autorun::floor::check_floor(script, &crate::autorun::floor::expected_of(&case.steps));
        if !shortfalls.is_empty() {
            return (400, format!("case {}: {}", script.case_id, shortfalls.join("; ")));
        }
    }

    // Everything has passed; now the disk.
    match crate::autorun::store::save_scripts_atomically(&root, &prepared) {
        Ok(()) => {}
        Err(crate::autorun::store::SaveScriptsError::Invalid(e)) => return (400, e),
        Err(crate::autorun::store::SaveScriptsError::Io(e)) => {
            return (500, format!("could not save the bundle: {e}"))
        }
    }
    crate::applog::info(format!("AI saved {} auto-run script(s)", prepared.len()));

    let mut report = vec![format!("saved {} script(s): {}", prepared.len(), lines.join(", "))];
    // The quirks come last, after the scripts are safely down: a quirk
    // the list will not take (too long, or the fortieth) is worth saying
    // so about, but it is not worth losing a good repair over.
    // Each quirk keeps the case and steps of its repair, and the class of
    // the failure that led to it, so later runs can say whether it helped.
    for edit in &edits {
        let Some(text) = edit.quirk.as_deref().filter(|t| !t.trim().is_empty()) else {
            continue;
        };
        let sources: Vec<crate::autorun::quirks::QuirkSource> =
            repair_sources.iter().filter(|s| s.case_id == edit.case_id).cloned().collect();
        match crate::autorun::quirks::record_quirk(
            &root,
            &ctx.org,
            &ctx.project,
            text,
            "assistant",
            crate::autorun::quirks::FROM_AUTORUN,
            sources,
            crate::autorun::sessions::now_ms(),
        ) {
            Ok(crate::autorun::quirks::Recorded::Added(id)) => {
                report.push(format!("quirk recorded as {id}: {}", text.trim()))
            }
            Ok(crate::autorun::quirks::Recorded::AlreadyKnown(id)) => {
                report.push(format!("quirk already known, as {id}: {}", text.trim()))
            }
            Ok(crate::autorun::quirks::Recorded::Reactivated(id, reason)) => {
                report.push(format!("quirk {}: {}", reactivated_reply(&id, reason.as_deref()), text.trim()))
            }
            Err(why) => report.push(format!("quirk not recorded: {why}")),
        }
    }
    (200, report.join("\n"))
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
fn optimize_json(body: &str, target: &str, ctx: &BridgeContext) -> (u16, String) {
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
    // An in-place rewrite is a read-patch-write, like a comment save, and a
    // comment autosave on the same file must not interleave with it. Take
    // the same lock BEFORE the read and hold it until the write is done.
    let _serialised = in_place
        .then(|| crate::filewatch::NOTE_WRITE.lock().unwrap_or_else(|e| e.into_inner()));
    // The exact text the cases were parsed from. The write-back merges into
    // this, never into a second read of a file that may have moved on.
    let mut draft_text: Option<String> = None;
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
            match crate::import_parser::read_draft(path) {
                Ok((text, v)) => {
                    draft_text = Some(text);
                    (v.cases, v.warnings)
                }
                Err(e) => return (400, serde_json::json!({ "error": e }).to_string()),
            }
        }
        None => match parse_cases_with_warnings(body) {
            Ok(v) => v,
            Err(e) => return (400, serde_json::json!({ "error": e }).to_string()),
        },
    };
    let file_specs = draft_text
        .as_deref()
        .map(crate::import_parser::specs::read_specs)
        .unwrap_or_default();
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
    let json = match draft_text_for(draft_text.as_deref(), &optimized) {
        Ok(j) => j,
        Err(e) => return (500, serde_json::json!({ "error": e }).to_string()),
    };
    if in_place {
        // Temp file + rename: a half-written draft under a watched path is
        // worse than no write.
        let path = from_path.expect("guarded above");
        if !bridge_may_write(&path, ctx.working_dir.as_deref(), &watched_now()) {
            return (400, serde_json::json!({ "error": IN_PLACE_REFUSAL }).to_string());
        }
        if let Err(e) = crate::ai_tools::atomic_write(std::path::Path::new(&path), &json) {
            return (500, serde_json::json!({ "error": e }).to_string());
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
    let mut response = serde_json::json!({
        "test_cases": doc.get("test_cases").cloned().unwrap_or(doc),
        "report": report,
        // Anything the importer could not read. A case it skipped is
        // simply not in the output, so silence here read as success
        // over a draft that had quietly got shorter.
        "import_warnings": import_warnings,
        "note": "Hand this JSON to the developer as the file to import. Every case now carries spec_order and tester_order - keep both fields exactly as set; the app flips the queue between the two readings. The report explains what was reordered and why. Check import_warnings - a case listed there was NOT read and is not in this output.",
    });
    // The file's `specs` travel with the cases the caller writes back.
    if !file_specs.is_empty() {
        response["specs"] = serde_json::json!(file_specs);
    }
    (200, response.to_string())
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
fn transform_json(body: &str, ctx: &BridgeContext) -> (u16, String) {
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

    // Same lock as a comment save, taken before the read (see optimize_json).
    let _serialised = in_place
        .then(|| crate::filewatch::NOTE_WRITE.lock().unwrap_or_else(|e| e.into_inner()));
    let mut draft_text: Option<String> = None;
    let (cases, import_warnings) = match &from_path {
        Some(path) => {
            if !std::path::Path::new(path).is_file() {
                return (
                    400,
                    serde_json::json!({ "error": format!("{path} does not exist or is not a file") })
                        .to_string(),
                );
            }
            match crate::import_parser::read_draft(path) {
                Ok((text, v)) => {
                    draft_text = Some(text);
                    (v.cases, v.warnings)
                }
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
    let file_specs = draft_text
        .as_deref()
        .map(crate::import_parser::specs::read_specs)
        .unwrap_or_default();
    let json = match draft_text_for(draft_text.as_deref(), &out) {
        Ok(j) => j,
        Err(e) => return (500, serde_json::json!({ "error": e }).to_string()),
    };

    if in_place {
        // Temp file + rename: a half-written draft under a watched path is
        // worse than no write.
        let path = from_path.expect("guarded above");
        if !bridge_may_write(&path, ctx.working_dir.as_deref(), &watched_now()) {
            return (400, serde_json::json!({ "error": IN_PLACE_REFUSAL }).to_string());
        }
        if let Err(e) = crate::ai_tools::atomic_write(std::path::Path::new(&path), &json) {
            return (500, serde_json::json!({ "error": e }).to_string());
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
    let mut response = serde_json::json!({
        "test_cases": parsed.get("test_cases").cloned().unwrap_or(parsed),
        "report": report,
        "import_warnings": import_warnings,
    });
    // The caller writes the file back itself, so the file's `specs` travel
    // with the cases - or the rewrite would drop them. Absent, not empty,
    // when the draft came inline: nothing to carry.
    if !file_specs.is_empty() {
        response["specs"] = serde_json::json!(file_specs);
    }
    (200, response.to_string())
}

/// The text to write back over a draft file: the new cases inside the
/// file's OWN document (`old` is the exact text they were parsed from), so
/// its `specs`, its whole-set `comments` and any key this app has never
/// heard of survive the rewrite - a tool owns the cases, never the file.
/// Without a file (the draft came inline) it is the standard wrapper.
fn draft_text_for(old: Option<&str>, cases: &[crate::model::TestCase]) -> Result<String, String> {
    match old {
        Some(old) => crate::import_parser::merge_cases_into_draft(old, cases),
        None => crate::import_parser::queue_to_json_string(cases),
    }
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
            crate::import_parser::parse_file(path).map(|p| (p.cases, p.warnings))
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
                         an assistant never writes it. If this is a problem you found, move it to the \
                         case's `findings` list.",
                        i + 1,
                        tc.title
                    ));
                }
                if let Some(why) = finding_like_note(&tc.reviewer_notes) {
                    advisories.push(format!(
                        "Test case {} ('{}'): its reviewer_notes read like a problem report ({why}). \
                         reviewer_notes say only what the case checks and where the requirement \
                         lives; a problem is a finding - move it to the case's `findings` list and \
                         take it out of the note.",
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
            Ok(parsed) => parsed.cases,
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
    // The slices' documents, once each in first-seen order: the merged file
    // names what every slice named, so its review page has a spec pane.
    let mut specs: Vec<String> = Vec::new();
    // Title -> the slice files it appeared in. A fan-out makes title
    // collisions likely precisely because no slice-writer sees another's
    // output, and `dedupe` is the wrong repair (first-wins would delete a
    // real case) - so the merge REPORTS the collision for a human to
    // settle (round 6 §4: two legitimate different cases converged on one
    // title in 373, found only by hand).
    let mut title_slices: std::collections::BTreeMap<String, Vec<(String, String)>> =
        std::collections::BTreeMap::new();
    // Each slice's whole-set note, under the name of the slice it came from.
    // The response calls the slices "safe to remove", so nothing they hold
    // may be left behind in them.
    let mut notes: Vec<String> = Vec::new();
    let output_dir = std::path::Path::new(&req.output_path)
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default();

    for path in &req.paths {
        match crate::import_parser::read_draft(path) {
            Ok((text, parsed)) => {
                let cases = parsed.cases;
                let file_warnings = parsed.warnings;
                let slice_dir = std::path::Path::new(path)
                    .parent()
                    .map(std::path::Path::to_path_buf)
                    .unwrap_or_default();
                for s in parsed.specs {
                    // A relative entry names a file beside the SLICE; the
                    // merged file may live in another folder.
                    let s = crate::spec_pane::rebase_spec_entry(&s, &slice_dir, &output_dir);
                    if !specs.iter().any(|have| have.eq_ignore_ascii_case(&s)) {
                        specs.push(s);
                    }
                }
                let note = crate::import_parser::comments::general_comment(&text);
                if !note.trim().is_empty() {
                    let name = std::path::Path::new(path)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.clone());
                    notes.push(format!("{name}:\n{}", note.trim()));
                }
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

    let text = match crate::import_parser::queue_to_json_string(&merged)
        .and_then(|t| if specs.is_empty() { Ok(t) } else { crate::import_parser::specs::patch_specs(&t, &specs) })
        .and_then(|t| {
            if notes.is_empty() {
                Ok(t)
            } else {
                crate::import_parser::comments::patch_general_comment(&t, &notes.join("\n\n"))
            }
        })
    {
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
    parsed.map(|p| (p.cases, p.warnings))
}

/// Run a draft through the app's REAL importer to get `TestCase`s, so
/// these tools accept exactly what the Import Test Cases tab accepts (bare
/// array, `test_cases` wrapper, the lot).
/// The project's existing tag names, for suggesting tags that match what
/// the team already uses instead of inventing near-duplicates.
///
/// Reads the shared reference cache the app fills (cache/mod.rs) - the
/// whole point is that an assistant asking for tags does NOT repeat a
/// request the app has already made. Only a completely cold cache (the AI
/// asked before the developer opened a tag field) fetches, and it stores
/// the result so the app doesn't pay for it either.
async fn tags(
    ctx: &BridgeContext,
    client: Option<&crate::ado::AdoClient>,
    target: &str,
) -> (u16, String) {
    let key = crate::cache::keys::tags(&ctx.org, &ctx.project);
    let (values, source) = match crate::cache::get::<Vec<String>>(&key) {
        Some(v) => (v, "cache"),
        None => match client {
            Some(c) => match c.get_tags(&ctx.org, &ctx.project).await {
                Ok(v) => {
                    crate::cache::put(&key, &v);
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
/// Whether the writing guide carries the risk-tiered rules: the person's
/// switch, and only where Auto Run is offered (a development build, or a
/// release build whose extras are unlocked) - a switch left on before the
/// extras were reset must not apply. Both inputs explicit, so each case is
/// testable from this development test binary.
pub fn risk_tiered_for(requested: bool, offered: bool) -> bool {
    requested && offered
}

/// The writing guide's granularity section, as it reads with the
/// risk-tiered trial switched off.
const GRANULARITY: &str = "\
        ## Granularity - quality over quantity\n\
        Similar checks belong in ONE case, not several. Checking a\n\
        notification's title and checking its body is one case with two\n\
        steps (or one step with both in the expected result) - not two\n\
        cases. Split only when the checks need different setup or data, or\n\
        can fail independently in a way the tester must record separately.\n\
        A padded case count is not coverage; every extra case is another\n\
        row someone has to execute and maintain.\n\n\
";

/// The writing guide's edge-case section, as it reads with the risk-tiered
/// trial switched off.
const EDGE_CASES: &str = "\
        ## Edge cases worth writing\n\
        A set that only walks the happy path is not finished. For each\n\
        feature, add the edge cases a tester can run from the application\n\
        itself in a few minutes, each as its own case with the branch in\n\
        its title:\n\n\
        - Access: open the page's address without signing in, or as a role\n\
        that should not see it; the expected result is what the application\n\
        shows instead (the sign-in page, a permission message), named\n\
        exactly.\n\
        - Required and empty: submit with a required field blank, with only\n\
        spaces, at the field's maximum length, and one over it.\n\
        - Boundaries the form shows: the smallest and largest value a field\n\
        accepts, a date at the edge of the allowed range, zero and a\n\
        negative number where the field is numeric.\n\
        - State: the same action twice (double submit, refresh after\n\
        saving, back button after a save), and an item edited by someone\n\
        else in between when the application shows that.\n\
        - Absence: the list with nothing in it, a search with no matches, a\n\
        filter that removes everything; the expected result is the empty\n\
        state's own words.\n\n\
        Do NOT write cases that need developer tools, a modified request,\n\
        a database change, a disconnected network, or a clock change: a\n\
        tester cannot run them from the application, and a case nobody can\n\
        run is worse than none. If a spec names such a behaviour, put it in\n\
        `reviewer_notes` as a note for the developers instead.\n\n\
";

/// The risk-tiered trial (the AI Bridge tab's "Risk-tiered test design"
/// switch): the team's Test Risk-Tiering Policy and test-generation rules,
/// adapted to test cases in Azure DevOps. It replaces the granularity
/// section; the rules for code-level tests (data-row attributes, method-style
/// names, mutation testing) have no meaning here and are left out.
const RISK_TIERED_DESIGN: &str = "\
        ## Risk-tiered design (trial rules)\n\
        These rules replace the plain granularity and edge-case guidance while\n\
        the developer trials them. They cut the number of cases without\n\
        cutting coverage: every case earns its place against an acceptance\n\
        criterion or a named risk.\n\n\
        ### Tier every scenario\n\
        - T1 - Critical: financial, legal or statutory, data isolation between\n\
        companies, or security. Payroll calculations, tax, EPF/ETF, access\n\
        control.\n\
        - T2 - Core: a core business workflow whose failure is visible but\n\
        recoverable. Leave, attendance, onboarding, integrations.\n\
        - T3 - Low: cosmetic or configuration, with a small blast radius.\n\
        Labels, report layout, settings screens.\n\n\
        Tier each SCENARIO, not the whole story. A check that shows or depends\n\
        on a T1 behaviour - the label that displays a calculated figure, the\n\
        report column that carries it - is RELATED: it belongs in the T1 case\n\
        and is T1. A change that stands on its own - an unrelated label renamed\n\
        in the same story - is UNRELATED: its own case, at its own tier.\n\n\
        ### Design techniques\n\
        - Equivalence partitioning: one case per partition, not per value.\n\
        - Boundary values: the minimum, the maximum and just outside - nothing\n\
        in between.\n\
        - Pairwise for three or more interacting inputs. Never every\n\
        combination unless the developer asks for it on a T1 scenario.\n\
        - T1 combinations: when three or more inputs feed one calculation or\n\
        rule, say so in the scenario list with the count - for example \"tax\n\
        band x employee type x join date feed the EPF calculation: pairwise\n\
        covers 12 of 48 combinations - generate all 48?\" - and let the\n\
        developer decide.\n\
        - Negative cases: one per distinct validation rule, not one per\n\
        invalid input.\n\n\
        ### Budget per story\n\
        New scenarios at most: T1 25, T2 12, T3 5. When that is not enough,\n\
        STOP and list the extra scenarios with a one-line justification each,\n\
        instead of writing them.\n\n\
        ### Consolidate\n\
        - Before adding a case, read the PBI's existing cases (`get_test_cases`)\n\
        and extend a matching one - keeping its `id` - rather than writing a\n\
        near-duplicate.\n\
        - Checks that differ only in their data are ONE case: one step per data\n\
        row, each with its own expected result. Different branches still stay\n\
        separate cases (see One branch per case).\n\
        - Similar checks on the same screen with the same setup belong in one\n\
        case. A padded case count is not coverage.\n\n\
        ### Tags on every case\n\
        These three are required; a genuinely new tag is fine for them.\n\
        - A trace: the acceptance criterion or named risk the case covers, as a\n\
        tag like `AC-3` or `Risk-payroll-rounding`. Never write a case without\n\
        one.\n\
        - Its tier: exactly one of `T1`, `T2`, `T3`.\n\
        - Exactly one run category: `Smoke` (the critical path, runnable in a\n\
        few minutes with no special data), `Regression` (the default) or\n\
        `Extended` (slow, data-heavy, or across companies).\n\n\
        ### Ready to automate\n\
        - Deterministic: name exact data and fixed dates in preconditions and\n\
        steps - never \"today\", \"any employee\" or an order the tester cannot\n\
        see.\n\
        - Isolated: a case sets up what it needs and never depends on another\n\
        case having run first.\n\
        - Data isolation: a case that reads or changes one company's data also\n\
        checks another company's data is not shown or touched (T1).\n\n\
";

/// The risk-tiered trial's edge cases: the same kinds of edge case as the
/// plain guide, chosen by the design techniques instead of listed per input.
const RISK_TIERED_EDGE_CASES: &str = "\
        ## Edge cases, the tiered way\n\
        Choose edge cases with the techniques above, not by habit - each one\n\
        still traced, tiered and inside the budget:\n\n\
        - Access: open the page's address without signing in, or as a role\n\
        that should not see it; the expected result is what the application\n\
        shows instead (the sign-in page, a permission message), named\n\
        exactly.\n\
        - Validation: one negative case per validation rule. A blank required\n\
        field and a value over the maximum length are two rules; if the\n\
        application trims input, blank and only spaces are one.\n\
        - Boundaries: the minimum, the maximum and just outside, for each field\n\
        the form bounds.\n\
        - State: the same action twice (double submit, refresh after saving,\n\
        back button after a save), where the flow allows it.\n\
        - Absence: the list with nothing in it, a search with no matches; the\n\
        expected result is the empty state's own words.\n\n\
        Do NOT write cases that need developer tools, a modified request,\n\
        a database change, a disconnected network, or a clock change: a\n\
        tester cannot run them from the application, and a case nobody can\n\
        run is worse than none. If a spec names such a behaviour, put it in\n\
        `reviewer_notes` as a note for the developers instead.\n\n\
";

/// The risk-tiered trial's extra workflow step: the scenario list, approved
/// before any case is drafted.
const RISK_TIERED_SCENARIO_STEP: &str = "\
        1.5. Before drafting, write the SCENARIO LIST in the conversation - one\n\
        line each: the scenario, its trace (`AC-n` or the named risk), its\n\
        tier, RELATED or UNRELATED where the story mixes tiers, and any T1\n\
        combination question. Wait for the developer to approve it, unless\n\
        they said the scenarios are pre-approved. Then write cases ONLY from\n\
        the approved list.\n\
";

/// The risk-tiered trial's closing step: what was covered, and what was not.
const RISK_TIERED_SUMMARY_STEP: &str = "\
        6. End with a summary: each approved scenario and the acceptance\n\
        criterion or risk it covers; cases added against existing cases\n\
        extended; and every scenario deferred over the budget, with its\n\
        justification.\n\
";

async fn guide(ctx: &BridgeContext, client: &crate::ado::AdoClient) -> String {
    let risk_tiered = risk_tiered_for(ctx.risk_tiered, crate::ai_tools::autorun_offered());
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
    let tag_lines = match crate::cache::get::<Vec<String>>(&crate::cache::keys::tags(&ctx.org, &ctx.project)) {
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
        Import Test Cases tab, reviews, then creates - you never write to Azure DevOps.\n\n\
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
        - `db_lookup` and `db_query` check real data: which table a screen reads\n\
        from, and what a value is today. The expected result still comes from the\n\
        specification. The database only tells you the current state, never what\n\
        it should be.\n\n\
        ## Format\n\
        Each case: `title` (required, <=255 chars), `steps` (required, each\n\
        `{{\"action\", \"expected\"}}`, or `{{\"shared\": N}}` for a Shared Steps reference - keep those exactly as exported, never invent one),\n\
        `tags` (semicolon-separated, never commas),\n\
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
        {granularity}\
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
        boost, skyrocketing, opened up, powerful, inquiries, ever-evolving.\n\
        - MUST put the name of anything the tester looks for on screen in\n\
        double quotation marks: the \"Save\" button, the \"Leave Requests\"\n\
        page, the \"Search employees\" placeholder, the \"Status\" column,\n\
        the \"Approved\" tab, the \"Your changes were saved\" message. The\n\
        name inside the quotes is the exact text on screen, capitalised as\n\
        the application shows it. Without quotes a tester cannot tell the\n\
        word \"save\" from the button \"Save\".\n\n\
        IMPORTANT: review every case before handing it back and make sure\n\
        there are no em dashes.\n\n\
        ## area\n\
        Give every case an `area`: where it sits on the page or in the\n\
        feature, as a path with `/` between the levels, page or screen\n\
        first: `\"Manage Events / Create / Validation\"`. Reuse the same spelling\n\
        for the same place across the set, so its cases stack under one\n\
        node in the app's Test map instead of two. This is the app's own grouping path, not the work item's Area Path. Never sent to Azure DevOps. Set or move it in bulk with `transform_cases` (`set_area`, and `where.area_is` to pick an area).\n\n\
        ## specs\n\
        Put the documents these cases were written from in the file's\n\
        top-level `\"specs\"` list, beside `test_cases`: each entry a path to\n\
        a markdown file (relative to the JSON file, or absolute) or an Azure\n\
        DevOps wiki page URL as copied from the browser. These are the\n\
        documents named at intake. The developer reads them beside the cases\n\
        in the browser, and every `Spec:` citation in `reviewer_notes` links\n\
        to its heading there - so name the document in a citation the way its\n\
        file or wiki page is named.\n\n\
        ## Findings\n\
        When something you read is WRONG - a case that contradicts its spec,\n\
        a spec that contradicts itself, code that does what neither says -\n\
        put it in that case's `findings` list in the file: `{{\"kind\":\n\
        \"test_case\"|\"spec\"|\"code\", \"subject\": \"<spec section or code\n\
        symbol>\", \"title\": \"<one line>\", \"detail\": \"<markdown>\"}}`.\n\
        `kind` is one of test_case, spec or code. Write it into the file\n\
        directly, or with `transform_cases` and its `set_findings` op. A\n\
        finding about the spec or the code goes on the case it affects; if\n\
        several, on the first. The developer reads findings in the browser\n\
        page under each case. Do this on your own when it applies; nobody\n\
        will ask you to. And never write `comment` for this or for anything\n\
        else, and never put it in reviewer_notes: the first is the\n\
        developer's field, the second says where a case came from and\n\
        nothing more. Do not write a case around a defect as if the defect\n\
        were the requirement - record the finding and say so in the\n\
        conversation.\n\n\
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
        That is a finding: put it in the case's `findings` list and keep\n\
        the note to what the case checks and where its requirement lives.\n\
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
        {edge_cases}\
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
        {scenario_step}\
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
        `transform_cases` instead of rewriting the file yourself.\n{summary_step}",
        org = ctx.org,
        project = ctx.project,
        granularity = if risk_tiered { RISK_TIERED_DESIGN } else { GRANULARITY },
        edge_cases = if risk_tiered { RISK_TIERED_EDGE_CASES } else { EDGE_CASES },
        scenario_step = if risk_tiered { RISK_TIERED_SCENARIO_STEP } else { "" },
        summary_step = if risk_tiered { RISK_TIERED_SUMMARY_STEP } else { "" },
    )
}

/// How many case ids one call may name. The batch read chunks at 200, but a
/// list longer than this is a whole suite - get_suite_test_cases reads that.
const MAX_CASE_IDS: usize = 200;

/// Real cases in the import JSON record shape, so they double as format
/// demonstrations. Three ways in:
/// - `pbi` alone: every case the PBI is tested by;
/// - `ids` alone: those cases, in the order asked - no PBI needed;
/// - both: the PBI's cases narrowed to those ids, with `not_on_pbi` naming
///   any id the PBI is not tested by.
async fn test_cases(
    ctx: &BridgeContext,
    client: &crate::ado::AdoClient,
    target: &str,
) -> (u16, String) {
    let pbi = match q(target, "pbi") {
        None => None,
        Some(v) => match v.parse::<i32>() {
            Ok(id) => Some(id),
            Err(_) => return (400, format!("pbi must be a work item id, got {v:?}")),
        },
    };
    let ids = match q(target, "ids") {
        None => None,
        Some(v) => match parse_case_ids(&v) {
            Ok(ids) => Some(ids),
            Err(msg) => return (400, msg),
        },
    };
    let (limit, offset, titles_only) = paging(target);
    let (module_ref, preconditions_ref) = (ctx.module_ref.as_deref(), ctx.preconditions_ref.as_deref());

    match (pbi, ids) {
        (None, None) => (
            400,
            "pass ?pbi=<PBI work item id> for the cases a PBI is tested by (find one with search_pbis), \
             or ?ids=<test case ids, comma-separated> to read cases by their own ids"
                .into(),
        ),
        (Some(pbi), ids) => {
            let cases = match client.get_pbi_test_cases_full(&ctx.org, pbi, module_ref, preconditions_ref).await {
                Ok(c) => c,
                Err(e) => return (502, format!("Azure DevOps error: {e:?}")),
            };
            let Some(ids) = ids else {
                return (200, case_page(&cases, limit, offset, titles_only).to_string());
            };
            let mut picked: Vec<_> = cases.into_iter().filter(|c| ids.contains(&c.id)).collect();
            picked.sort_by_key(|c| ids.iter().position(|&i| i == c.id).unwrap_or(usize::MAX));
            let not_on_pbi: Vec<i32> =
                ids.iter().copied().filter(|i| !picked.iter().any(|c| c.id == *i)).collect();
            let mut out = case_page(&picked, limit, offset, titles_only);
            if !not_on_pbi.is_empty() {
                out["not_on_pbi"] = serde_json::json!(not_on_pbi);
                out["note_not_on_pbi"] = serde_json::json!(format!(
                    "PBI #{pbi} is not tested by these ids; call get_test_cases with case_ids alone (no pbi_id) to read them wherever they are."
                ));
            }
            (200, out.to_string())
        }
        (None, Some(ids)) => match client.get_test_cases_by_ids(&ctx.org, &ids, module_ref, preconditions_ref).await {
            Ok(mut cases) => {
                // The batch read answers in id order; give back the order asked.
                cases.sort_by_key(|c| ids.iter().position(|&i| i == c.id).unwrap_or(usize::MAX));
                (200, case_page(&cases, limit, offset, titles_only).to_string())
            }
            // ADO refuses the whole batch when any id is not a work item.
            Err(crate::ado::AdoError::NotFound) => (
                404,
                format!(
                    "Azure DevOps has no work item for at least one of {} - check the ids, or find the case through its PBI or suite",
                    ids.iter().map(|i| format!("#{i}")).collect::<Vec<_>>().join(", ")
                ),
            ),
            Err(e) => (502, format!("Azure DevOps error: {e:?}")),
        },
    }
}

/// `ids=12,34` as a de-duplicated list in the order given. A token that is
/// not a whole number is refused rather than dropped, so a typo cannot
/// quietly return fewer cases than were asked for.
fn parse_case_ids(raw: &str) -> Result<Vec<i32>, String> {
    let mut ids: Vec<i32> = vec![];
    for token in raw.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        let id = token
            .trim_start_matches('#')
            .parse::<i32>()
            .map_err(|_| format!("ids must be test case work item ids, got {token:?}"))?;
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    if ids.is_empty() {
        return Err("ids is empty - pass one or more test case ids, comma-separated".into());
    }
    if ids.len() > MAX_CASE_IDS {
        return Err(format!(
            "at most {MAX_CASE_IDS} ids per call - for a whole suite use get_suite_test_cases"
        ));
    }
    Ok(ids)
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
                    "steps": c.steps.iter().map(crate::import_parser::step_json).collect::<Vec<_>>(),
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

type SuiteTree = Vec<crate::ado_testplan::PlanWithSuites>;

/// The plans and suites the Test Suites tab shows, cached in memory per org
/// and project for `cache::keys::SUITE_TREE_TTL`; `refresh=true` reads them
/// again. Session tier: the tree is large, and TestPlan's `#[serde(skip)]`
/// fields would not survive a trip through the disk.
async fn suite_tree(
    ctx: &BridgeContext,
    client: &crate::ado::AdoClient,
    refresh: bool,
) -> Result<SuiteTree, crate::ado::AdoError> {
    let key = crate::cache::keys::suite_tree(&client.base_url, &ctx.org, &ctx.project);
    if !refresh {
        if let Some(tree) = crate::cache::session_fresh::<SuiteTree>(&key, crate::cache::keys::SUITE_TREE_TTL) {
            return Ok(tree);
        }
    }
    let tree = client.list_plans_with_suites(&ctx.org, &ctx.project).await?;
    crate::cache::session_put(&key, tree.clone());
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

/// How many matching results get their comment + linked bugs fetched. Each
/// one is its own request; a suite with 80 failures is a suite with a
/// bigger problem than missing detail text.
const RUN_FAILURE_DETAIL_CAP: usize = 10;
/// How many matching results are listed at all - id, title, configuration
/// and outcome come with the points, so listing costs no extra request.
const RUN_RESULT_LIST_CAP: usize = 200;

/// Every outcome a test point can carry, in the order the summary lists
/// them: key (Azure DevOps' own value, lowercased), the label an assistant
/// and a person read, and other spellings accepted when asking for it.
/// "Never run" is a point with no verdict yet - the Test Plans UI calls it
/// Active. The last seven are the automated-test outcomes; a manual suite
/// rarely has them, but a filter for one should not be refused as a typo.
pub const RUN_OUTCOMES: &[(&str, &str, &[&str])] = &[
    ("failed", "Failed", &["fail"]),
    ("blocked", "Blocked", &[]),
    ("paused", "Paused", &[]),
    ("inprogress", "In progress", &[]),
    ("notapplicable", "Not applicable", &["na"]),
    ("passed", "Passed", &["pass"]),
    ("neverrun", "Never run", &["notrun", "never", "active", "unspecified", "none"]),
    ("error", "Error", &[]),
    ("timeout", "Timeout", &[]),
    ("aborted", "Aborted", &[]),
    ("inconclusive", "Inconclusive", &[]),
    ("warning", "Warning", &[]),
    ("notexecuted", "Not executed", &[]),
    ("notimpacted", "Not impacted", &[]),
];

fn outcome_word(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase()
}

/// A point's outcome as a RUN_OUTCOMES key. `last_outcome` is empty for a
/// point with no verdict; anything this table does not know keeps its own
/// (normalised) spelling, so it is counted, never dropped.
pub fn outcome_key(last_outcome: &str) -> String {
    if last_outcome.trim().is_empty() {
        "neverrun".into()
    } else {
        outcome_word(last_outcome)
    }
}

/// The label for a key: the table's, or the raw key for one it lacks.
pub fn outcome_label(key: &str) -> String {
    RUN_OUTCOMES
        .iter()
        .find(|(k, _, _)| *k == key)
        .map_or_else(|| key.to_string(), |(_, label, _)| label.to_string())
}

/// The outcomes asked for - a comma-separated list in any spelling
/// ("Not Applicable", "not_applicable", "notapplicable"), or "all". None or
/// blank is Failed, what this tool returned before it took a filter. An
/// unknown word is refused with the list, not silently matched to nothing:
/// an empty answer to a typo would read as "nothing had that outcome".
pub fn parse_outcomes(raw: Option<&str>) -> Result<Vec<&'static str>, String> {
    let Some(raw) = raw.filter(|r| !r.trim().is_empty()) else {
        return Ok(vec!["failed"]);
    };
    let mut keys: Vec<&'static str> = Vec::new();
    for word in raw.split(',').map(outcome_word).filter(|w| !w.is_empty()) {
        if word == "all" {
            return Ok(RUN_OUTCOMES.iter().map(|(k, _, _)| *k).collect());
        }
        let Some((key, _, _)) = RUN_OUTCOMES
            .iter()
            .find(|(k, _, aliases)| *k == word || aliases.contains(&word.as_str()))
        else {
            let known: Vec<&str> = RUN_OUTCOMES.iter().map(|(_, l, _)| *l).collect();
            return Err(format!(
                "\"{word}\" is not a test outcome. Use one or more of: {}, or \"all\"",
                known.join(", ")
            ));
        };
        if !keys.contains(key) {
            keys.push(key);
        }
    }
    if keys.is_empty() {
        return Ok(vec!["failed"]);
    }
    Ok(keys)
}

/// The cases from a PBI's latest runs with the outcomes asked for (Failed
/// unless told otherwise), each with its comment and linked bugs, plus a
/// count of every outcome in the suite - what an assistant needs to draft
/// regression cases, or to see where a PBI's testing stands.
///
/// GET-only end to end: `find_pbi_requirement_suite` is the find-ONLY
/// scan, never the find-or-create one. A PBI with no suite is an answer
/// ("this PBI has never had a run"), not a reason to create anything -
/// this is the bridge, and the bridge does not write to Azure DevOps.
/// The resolved suite comes from the cache shared with the upload and Run
/// Tests (`ado_testplan::cached_suite`): resolving scans every plan and
/// took about a minute on a large org, which is what made the MCP proxy
/// give up at 30s and blame the connection.
async fn run_results(
    ctx: &BridgeContext,
    client: &crate::ado::AdoClient,
    target: &str,
) -> (u16, String) {
    let Some(pbi) = q(target, "pbi").and_then(|v| v.parse::<i32>().ok()) else {
        return (400, "pass ?pbi=<work item id> (find one with search_pbis)".into());
    };
    let wanted = match parse_outcomes(q(target, "outcome").as_deref()) {
        Ok(w) => w,
        Err(why) => return (400, why),
    };
    let mut retried = false;
    let (suite, points) = loop {
        let cached = crate::ado_testplan::cached_suite(&client.base_url, &ctx.org, &ctx.project, pbi);
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
                        crate::ado_testplan::remember_suite(&client.base_url, &ctx.org, &ctx.project, pbi, &s);
                        (s, false)
                    }
                    Ok(None) => {
                        return (
                            200,
                            serde_json::json!({
                                "pbi": pbi,
                                "summary": [],
                                "results": [],
                                "note": "This PBI has no test suite, so it has never had a test run - there are no results to read.",
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
                crate::ado_testplan::forget_suite(&client.base_url, &ctx.org, &ctx.project, pbi);
                retried = true;
            }
            Err(e) => return (502, format!("Azure DevOps error: {e:?}")),
        }
    };

    let total = points.len();
    // Every outcome in the suite, counted - the table's order first, then
    // anything it does not know, so nothing in the suite goes uncounted.
    let mut counts: Vec<(String, usize)> = Vec::new();
    for p in &points {
        let key = outcome_key(&p.last_outcome);
        match counts.iter_mut().find(|(k, _)| *k == key) {
            Some((_, n)) => *n += 1,
            None => counts.push((key, 1)),
        }
    }
    let rank = |k: &str| RUN_OUTCOMES.iter().position(|(key, _, _)| *key == k).unwrap_or(usize::MAX);
    counts.sort_by_key(|(k, _)| rank(k));
    let summary: Vec<serde_json::Value> = counts
        .iter()
        .map(|(k, n)| serde_json::json!({ "outcome": outcome_label(k), "count": n }))
        .collect();

    let matching: Vec<_> = points
        .into_iter()
        .filter(|p| wanted.contains(&outcome_key(&p.last_outcome).as_str()))
        .collect();
    let matched = matching.len();

    let mut results: Vec<serde_json::Value> = vec![];
    for (i, p) in matching.iter().take(RUN_RESULT_LIST_CAP).enumerate() {
        // The comment is where the tester wrote what actually happened -
        // fetched per result, best-effort, for the first few only: a result
        // whose detail cannot be read is still a result worth naming. A
        // point that never ran has no result to read.
        let detail = match (p.last_run_id, p.last_result_id) {
            (Some(run), Some(res)) if i < RUN_FAILURE_DETAIL_CAP => Some(
                client
                    .get_result_report_info(&ctx.org, &ctx.project, run, res)
                    .await
                    .unwrap_or_default(),
            ),
            _ => None,
        };
        let mut row = serde_json::json!({
            "case_id": p.test_case_id,
            "title": p.test_case_name,
            "configuration": p.config_name,
            "outcome": outcome_label(&outcome_key(&p.last_outcome)),
            "run_id": p.last_run_id,
        });
        if let Some((comment, bug_ids)) = detail {
            row["comment"] = serde_json::json!(comment);
            row["bug_ids"] = serde_json::json!(bug_ids);
        }
        results.push(row);
    }

    let mut out = serde_json::json!({
        "pbi": pbi,
        "plan": { "id": suite.plan_id, "name": suite.plan_name },
        "cases_in_suite": total,
        "summary": summary,
        "outcomes": wanted.iter().map(|k| outcome_label(k)).collect::<Vec<_>>(),
        "matched": matched,
        "results": results,
        "note": "`summary` counts every outcome in the PBI's suite; `results` lists the cases with the outcomes asked for. A result's `comment` is what the tester wrote and `bug_ids` are the bugs they linked. To write regression cases from failures, start with begin_test_case_writing as usual - and read the failed case itself via get_test_cases so the regression case extends it instead of restating it.",
    });
    let mut limits: Vec<String> = Vec::new();
    if matched > RUN_RESULT_LIST_CAP {
        limits.push(format!("{matched} cases matched; the first {RUN_RESULT_LIST_CAP} are listed."));
    }
    if matched > RUN_FAILURE_DETAIL_CAP {
        limits.push(format!(
            "Comments and linked bugs were read for the first {RUN_FAILURE_DETAIL_CAP} only - ask for fewer outcomes to see the others' detail."
        ));
    }
    if !limits.is_empty() {
        out["truncated"] = serde_json::json!(limits.join(" "));
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
    // One wiki name or id - never a route. The decoded value used to reach
    // the URL raw, so `..%2F` climbed out to any GET endpoint.
    if wiki_id.contains('/') || wiki_id.contains('\\') || wiki_id.contains("..") {
        return (400, "the wiki parameter is a wiki name or id - it cannot contain '/', '\\' or '..'".into());
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
        let mut backoff = crate::note_server::AcceptBackoff::default();
        loop {
            let (mut sock, _) = match listener.accept().await {
                Ok(pair) => {
                    backoff.succeeded();
                    pair
                }
                Err(e) => {
                    let wait = backoff.failed();
                    crate::applog::warn(format!(
                        "AI bridge could not accept a connection, retrying in {} ms: {e}",
                        wait.as_millis()
                    ));
                    tokio::time::sleep(wait).await;
                    continue;
                }
            };
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
                        Parsed::Complete { method, target, token, body, proxy_version } => {
                            break (method, target, token, body, proxy_version)
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
                let (method, target, tok, body, proxy_version) = parsed;
                let (status, payload) = if tok.as_deref() != Some(state.token.as_str()) {
                    (401, String::new())
                } else {
                    note_proxy_version(proxy_version.as_deref(), &state.version);
                    let ctx = state.ctx.lock().unwrap().clone();
                    let client = match &make_client {
                        Some(f) => f().await,
                        None => None,
                    };
                    route(&ctx, client.as_ref(), &method, &target, &body, &state.version).await
                };
                let reason = match status {
                    200 => "OK",
                    400 => "Bad Request",
                    401 => "Unauthorized",
                    403 => "Forbidden",
                    409 => "Conflict",
                    500 => "Internal Server Error",
                    502 => "Bad Gateway",
                    503 => "Unavailable",
                    _ => "Not Found",
                };
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
    Complete {
        method: String,
        target: String,
        token: Option<String>,
        body: String,
        /// The `x-tcm-proxy-version` header: the build of the MCP proxy that
        /// sent this. None from a proxy older than the header.
        proxy_version: Option<String>,
    },
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
    let mut proxy_version = None;
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
        if k.eq_ignore_ascii_case(crate::mcp::PROXY_VERSION_HEADER) {
            proxy_version = Some(v.to_string());
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
        proxy_version,
    }
}

/// Logs - once per proxy version, not once per call - an MCP proxy that is
/// not this app's build. The assistant hears it from the proxy itself; this
/// is where a person hears it: Settings → Logs, which a bug report ships.
/// A proxy older than the header says nothing and is not flagged here.
fn note_proxy_version(proxy: Option<&str>, app: &str) {
    // The versions already reported. A log de-duplication, not cached data.
    static WARNED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    let Some(proxy) = proxy else { return };
    let Some(warning) = crate::mcp::version_warning(proxy, app) else { return };
    let mut warned = WARNED.lock().unwrap_or_else(|e| e.into_inner());
    if warned.iter().any(|v| v == proxy) {
        return;
    }
    warned.push(proxy.to_string());
    crate::applog::warn(format!("AI bridge: {warning}"));
}
