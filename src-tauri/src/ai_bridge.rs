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
        ("GET", "/autorun-guide") => {
            let quick = matches!(q(target, "quick").as_deref(), Some("true") | Some("1"));
            (200, autorun_guide_with_quirks(ctx, quick))
        }
        ("POST", "/autorun-script") => save_autorun_scripts(ctx, client, body).await,
        ("GET", "/autorun-page") => autorun_page(ctx, target).await,
        ("POST", "/autorun-probe") => autorun_probe(ctx, body).await,
        ("POST", "/autorun-try") => autorun_try(ctx, body).await,
        // Discovery: the assistant opens the Auto Run browser itself, signs
        // in as a saved account (the app types the password) and explores
        // the live application, one action at a time.
        ("POST", "/autorun-discover-start") => autorun_discover_start(ctx, body).await,
        ("POST", "/autorun-discover-action") => autorun_discover_action(ctx, body).await,
        ("POST", "/autorun-discover-actions") => autorun_discover_actions(ctx, body).await,
        ("POST", "/autorun-discover-end") => crate::commands::autorun::end_discovery().await,
        // Release: every Auto Run browser record the app holds is cleared
        // and the app's own browsers closed, for a browser held after it
        // is gone. Never a process outside the app's own jobs.
        ("POST", "/autorun-release") => crate::commands::autorun::release_for_assistant().await,
        ("POST", "/autorun-discover-area") => autorun_discover_area(ctx, body).await,
        ("POST", "/autorun-replay") => autorun_replay(ctx, body).await,
        ("GET", "/autorun-failures") => autorun_failures(ctx, target),
        ("POST", "/autorun-quirk") => autorun_quirk(ctx, body),
        ("POST", "/autorun-quirk-retire") => autorun_quirk_retire(ctx, body),
        ("POST", "/autorun-defect") => autorun_defect(body),
        // Components: saved only once the open discovery has tried them
        // live, and retired only while no saved script uses them.
        ("POST", "/autorun-component-save") => autorun_component_save(ctx, client, body).await,
        ("POST", "/autorun-component-retire") => autorun_component_retire(ctx, body),
        // Auto Run's own order for a PBI. Reads the PBI's cases from Azure
        // DevOps (a read) to refuse an id the PBI is not tested by.
        ("POST", "/autorun-order") => autorun_order(ctx, client, body).await,
        // The active environment's accounts: the assistant proposes logins
        // (never passwords) for a person to add, and reads the ones there -
        // passwords included only in an environment marked as a test one.
        ("POST", "/accounts-propose") => accounts_propose(body),
        ("GET", "/accounts") => accounts_read(ctx),
        // The project's Test files, by name and size: what a case uploads.
        ("GET", "/autorun-test-files") => autorun_test_files(ctx),
        // The API template routes: gated with the Auto Run ones by the
        // guard above. Proving and running write to the application, so
        // both also need the person's own switch (`ctx.api_writes`); the
        // guide and the list are reads and answer either way.
        ("GET", "/api-template-guide") => (200, api_template_guide(ctx)),
        ("GET", "/api-templates") => api_template_list(ctx, target),
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
        // Fixtures: saving one and running one need the API templates
        // switch - a run writes to the application, and a save is what a
        // run then follows. The list is a read and answers either way.
        // None of these writes the record of test-made drafts itself: only
        // the fixture runner adds to it, and only Clean up changes it.
        ("GET", "/api-template-fixtures") => api_fixture_list(ctx),
        ("POST", "/api-template-fixture-save") => api_fixture_save(ctx, body),
        ("POST", "/api-template-fixture-run") => {
            api_fixture_run(ctx, body, real_template_browsers, &crate::commands::autorun_replay::replay_timing(false))
                .await
        }
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

// Said when a prove, a run or a fixture run arrives while another one
// holds the process-wide slot (`runner::claim`).
use crate::api_templates::runner::API_TEMPLATE_BUSY;

/// The browsers a real prove or run opens: one, headless - nobody watches
/// a template run.
fn real_template_browsers(which: crate::browser::launch::Browser) -> crate::commands::autorun_replay::RealBrowsers {
    crate::commands::autorun_replay::RealBrowsers::new(which, false)
}

/// The origin this project signs in at in the active environment - the
/// environment's address, else the saved recipe's - or None when there is
/// neither.
fn recipe_origin(root: &std::path::Path, org: &str, project: &str) -> Option<String> {
    crate::autorun::recipe::load_effective_recipe(root, org, project)
        .ok()
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

/// Said by `list_test_files` with no project open: Test files belong to one.
const TEST_FILES_NEED_A_PROJECT: &str = "open a project in the app first - Test files belong to a project";

/// `GET /autorun-test-files`: this project's Test files, each by name and
/// human size and nothing else - never a path or a date.
fn autorun_test_files(ctx: &BridgeContext) -> (u16, String) {
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    if ctx.org.trim().is_empty() || ctx.project.trim().is_empty() {
        return (409, TEST_FILES_NEED_A_PROJECT.to_string());
    }
    let files = match crate::test_files::list(&crate::test_files::folder(&root, &ctx.org, &ctx.project)) {
        Ok(f) => f,
        Err(e) => return (500, e),
    };
    let rows: Vec<serde_json::Value> = files
        .into_iter()
        .map(|f| serde_json::json!({ "name": f.name, "size": crate::test_files::human_size(u64::from(f.size)) }))
        .collect();
    (200, serde_json::json!({ "test_files": rows }).to_string())
}

/// How many templates one answer of the list carries when no `limit` is
/// given, and the most it ever carries.
pub const TEMPLATE_PAGE: usize = 25;
pub const TEMPLATE_PAGE_MAX: usize = 100;

/// The saved templates for this project, filtered and paged.
///
/// Arguments, all optional, from the query: `module` (a case-insensitive
/// substring), `search` (a case-insensitive substring of the id, the
/// title, or a param or output name), `flow` (that flow's templates, and
/// that flow alone), `offset` and `limit` (25 by default, 100 at most),
/// and `id` (that one template in full, with the flow it is on).
///
/// A filtered result of at most `limit` templates comes back in full:
/// what each one takes and gives back, the flow stage it performs,
/// whether it is proven on this site (an imported one is not - `unproven`
/// says what to do about it), its newest run (null before its first - the
/// prove that saved it is history, not a run), every flow with its stages
/// and the templates on each, and the project's Test files. A larger one
/// is a compact index: per template the id, title, module, effect, proven
/// state and stage, and per flow its id, title and stage count, with a
/// `note` saying how to narrow it. Every answer carries `paging`. Owner's
/// report: a project's whole list in full was 210K characters, too big to
/// read. A flow file that no longer parses is left out (and logged by
/// `flow_store::list`), never the whole answer.
fn api_template_list(ctx: &BridgeContext, target: &str) -> (u16, String) {
    use crate::api_templates::{flow_store, gate, store};
    let arg = |key: &str| q(target, key).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    let number = |key: &str, min: usize| -> Result<Option<usize>, (u16, String)> {
        match arg(key) {
            None => Ok(None),
            Some(v) => match v.parse::<usize>() {
                Ok(n) if n >= min => Ok(Some(n)),
                _ => Err((400, format!("\"{key}\" is a whole number, {min} or more"))),
            },
        }
    };
    // With `id`, the other filters and the paging are ignored, so an offset
    // or limit sent beside it is not refused either.
    let by_id = arg("id").is_some();
    let offset = match number("offset", 0) {
        _ if by_id => 0,
        Ok(n) => n.unwrap_or(0),
        Err(refused) => return refused,
    };
    let limit = match number("limit", 1) {
        _ if by_id => TEMPLATE_PAGE,
        Ok(n) => n.unwrap_or(TEMPLATE_PAGE).min(TEMPLATE_PAGE_MAX),
        Err(refused) => return refused,
    };
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
    let mut flows = flow_store::list(&root, &ctx.org, &ctx.project).unwrap_or_else(|e| {
        crate::applog::warn(format!("api template flows: the list could not be read: {e}"));
        Vec::new()
    });

    // Which templates this answer is about.
    let lower = |s: &str| s.to_lowercase();
    let matched: Vec<&store::SavedTemplate> = if let Some(id) = arg("id") {
        let Some(one) = saved.iter().find(|s| s.template.id == id) else {
            return (
                404,
                format!("no template called \"{id}\" is saved for this project - list_api_templates shows the ones that are"),
            );
        };
        let on = one.template.stage.as_ref().map(|r| r.flow.clone());
        flows.retain(|f| Some(&f.id) == on.as_ref());
        vec![one]
    } else {
        let flow = arg("flow");
        if let Some(flow) = &flow {
            if !flows.iter().any(|f| &f.id == flow) {
                return (
                    404,
                    format!("no flow called \"{flow}\" is saved for this project - list_api_templates shows the ones that are"),
                );
            }
            flows.retain(|f| &f.id == flow);
        }
        let module = arg("module").map(|m| lower(&m));
        let search = arg("search").map(|s| lower(&s));
        saved
            .iter()
            .filter(|s| {
                let t = &s.template;
                let hit = |text: &str, part: &str| lower(text).contains(part);
                module.as_deref().is_none_or(|m| hit(&t.module, m))
                    && flow.as_ref().is_none_or(|f| t.stage.as_ref().is_some_and(|r| &r.flow == f))
                    && search.as_deref().is_none_or(|q| {
                        hit(&t.id, q)
                            || hit(&t.title, q)
                            || t.params.iter().any(|p| hit(&p.name, q))
                            || t.outputs.iter().any(|o| hit(o, q))
                    })
            })
            .collect()
    };
    let total = matched.len();
    let full = total <= limit;
    let page: Vec<&store::SavedTemplate> = matched.into_iter().skip(offset).take(limit).collect();
    // A narrowed answer in full names the flows its templates are on (a
    // requested `flow` is already the only one), not every flow with every
    // stage - that bulk is what the filters are for.
    let narrowed = !by_id && arg("flow").is_none() && (arg("module").is_some() || arg("search").is_some());
    if full && narrowed {
        flows.retain(|f| page.iter().any(|s| s.template.stage.as_ref().is_some_and(|r| r.flow == f.id)));
    }

    let flow_rows: Vec<serde_json::Value> = flows
        .iter()
        .map(|f| {
            if !full {
                return serde_json::json!({ "id": f.id, "title": f.title, "stage_count": f.stages.len() });
            }
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
    let rows: Vec<serde_json::Value> = page
        .iter()
        .map(|s| {
            let t = &s.template;
            if !full {
                return serde_json::json!({
                    "id": t.id,
                    "title": t.title,
                    "module": t.module,
                    "effect": t.effect,
                    "proven": t.proven.is_some(),
                    "stage": t.stage,
                });
            }
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
                "last_run": s.runs.iter().find(|r| r.mode == store::MODE_RUN),
            })
        })
        .collect();
    let mut paging = serde_json::json!({ "total": total, "offset": offset, "returned": rows.len() });
    if offset + rows.len() < total {
        paging["next_offset"] = serde_json::json!(offset + rows.len());
    }
    let mut answer = serde_json::json!({ "templates": rows, "flows": flow_rows, "paging": paging });
    if full {
        // Names and sizes only: what a template's `files` may name.
        let test_files: Vec<serde_json::Value> = project_test_files(&root, ctx)
            .into_iter()
            .map(|f| serde_json::json!({ "name": f.name, "size": f.size }))
            .collect();
        answer["test_files"] = serde_json::json!(test_files);
    } else {
        let mut note = format!(
            "This is the index of {total} templates. Narrow it with module, search or flow until at most {limit} match, or pass id, for params, outputs, the newest run and the test files. Page with offset and limit (at most {TEMPLATE_PAGE_MAX})."
        );
        // The full answer says it on each unproven row; the index says it
        // once.
        if page.iter().any(|s| s.template.proven.is_none()) {
            note.push_str(&format!(
                " A template whose proven is false was {}.",
                crate::api_templates::share::UNPROVEN_FOR_ASSISTANT
            ));
        }
        answer["note"] = serde_json::json!(note);
    }
    (200, answer.to_string())
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
pub async fn api_template_prove<B: crate::api_templates::held::Keeps, D: crate::api_templates::gate::StageDb>(
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
pub async fn api_template_run<B: crate::api_templates::held::Keeps, D: crate::api_templates::gate::StageDb>(
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
async fn run_api_template_request<B: crate::api_templates::held::Keeps, D: crate::api_templates::gate::StageDb>(
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
    // A delete template is only proven on a draft the tests made: its `id`
    // must be a `present` entry of its kind in the record, in the active
    // environment. The proof that deletes it marks it deleted.
    let deletes = match (&req.mode, req.template.effect) {
        (Mode::Prove { .. }, crate::api_templates::Effect::Delete) => {
            match crate::autorun::cleanup::proof_subject(root, &req.template, &req.values) {
                Ok(entry) => Some(entry),
                Err(sentence) => return (400, sentence),
            }
        }
        _ => None,
    };
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
    // A browser the run kept signed in (`api_templates::held`) was taken
    // out of `browsers` first, so this ends only one it did not keep.
    drop(browsers);
    // Every step passed, so the entry is gone from the application, whether
    // or not the template is saved below.
    if let (true, Some(entry)) = (report.ok, &deletes) {
        crate::autorun::cleanup::proof_deleted(root, entry);
    }

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

const FIXTURE_SAVE_SHAPE: &str = "{ \"fixture\": { \"id\": \"<fixture id>\", \"name\": \"<name>\", \"account\": \"<account key>\", \"steps\": [ { \"template\": \"<template id>\", \"params\": { <param>: \"<text>\" } } ], \"outputs\"?: { <name>: \"{{steps.<n>.<output>}}\" }, \"creates\"?: [ { \"kind\", \"id\", \"name\" } ] } }";
const FIXTURE_RUN_SHAPE: &str = "{ \"id\": \"<fixture id>\", \"browser\"?: \"edge\" | \"chrome\" }";

/// The browser a call asks for: Edge unless it says `"chrome"`.
fn browser_arg(v: &serde_json::Value) -> Result<crate::browser::launch::Browser, (u16, String)> {
    match v.get("browser") {
        None | Some(serde_json::Value::Null) => Ok(crate::browser::launch::Browser::Edge),
        Some(serde_json::Value::String(s)) if matches!(s.trim().to_ascii_lowercase().as_str(), "edge" | "chrome") => {
            Ok(crate::browser::launch::Browser::from_name(s))
        }
        Some(_) => Err((400, "\"browser\" is \"edge\" or \"chrome\"".to_string())),
    }
}

/// `GET /api-template-fixtures`: every saved fixture of the project, with
/// its current outputs (from its newest successful run) and its last run.
/// A read: it answers with the API templates switch off.
fn api_fixture_list(ctx: &BridgeContext) -> (u16, String) {
    use crate::api_templates::fixture_store;
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let saved = match fixture_store::list(&root, &ctx.org, &ctx.project) {
        Ok(s) => s,
        Err(e) => {
            crate::applog::warn(format!("api fixtures: the list could not be read: {e}"));
            return (500, "the fixtures could not be listed - see Settings, Logs".to_string());
        }
    };
    let fixtures: Vec<serde_json::Value> = saved
        .iter()
        .map(|s| {
            let f = &s.fixture;
            serde_json::json!({
                "id": f.id,
                "name": f.name,
                "account": f.account,
                "steps": f.steps,
                "outputs": f.outputs,
                "creates": f.creates,
                "current_outputs": s.runs.iter().find(|r| r.ok).map(|r| &r.outputs),
                "last_run": s.runs.first(),
            })
        })
        .collect();
    (200, serde_json::json!({ "fixtures": fixtures }).to_string())
}

/// `POST /api-template-fixture-save`: checks the fixture against the saved
/// templates (`fixture::validate`) and saves it, or answers 400 with every
/// refusal sentence as it is, one per line. Needs the API templates switch.
pub fn api_fixture_save(ctx: &BridgeContext, body: &str) -> (u16, String) {
    use crate::api_templates::fixture::Fixture;
    use crate::api_templates::fixture_store;
    if !ctx.api_writes {
        return (400, API_WRITES_OFF.to_string());
    }
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return (400, format!("that is not readable JSON: {e}. Expected {FIXTURE_SAVE_SHAPE}.")),
    };
    let Some(raw) = json_arg(v.get("fixture")) else {
        return (400, format!("this call needs a \"fixture\". Expected {FIXTURE_SAVE_SHAPE}."));
    };
    let fixture: Fixture = match serde_json::from_value(raw) {
        Ok(f) => f,
        Err(e) => return (400, format!("that is not a fixture: {e} - call get_api_template_guide for the format")),
    };
    let existed = matches!(fixture_store::load(&root, &ctx.org, &ctx.project, &fixture.id), Ok(Some(_)));
    match fixture_store::save(&root, &ctx.org, &ctx.project, &fixture) {
        Ok(()) => {
            crate::applog::info(format!("api fixture {}: {}", fixture.id, if existed { "replaced" } else { "saved" }));
            templates_changed(&fixture.id);
            (200, serde_json::json!({ "saved": fixture.id, "replaced": existed }).to_string())
        }
        Err(problems) => (400, problems.join("\n")),
    }
}

/// `POST /api-template-fixture-run`: runs a saved fixture - a first build
/// and a Rebuild alike - and answers with its sentence, outputs, what it
/// made and its warnings: 200 when every step passed, 502 when one did not.
/// Needs the API templates switch. The browser factory is handed in, as
/// `api_template_run`'s is, so a test reaches all of it but a real browser.
pub async fn api_fixture_run<B: crate::autorun::replay::Browsers>(
    ctx: &BridgeContext,
    body: &str,
    open: impl FnOnce(crate::browser::launch::Browser) -> B,
    timing: &crate::browser::timing::Timing,
) -> (u16, String) {
    use crate::api_templates::fixture_run::{run_saved, NotRun};
    if !ctx.api_writes {
        return (400, API_WRITES_OFF.to_string());
    }
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return (400, format!("that is not readable JSON: {e}. Expected {FIXTURE_RUN_SHAPE}.")),
    };
    let id = match v.get("id") {
        Some(serde_json::Value::String(s)) if !s.trim().is_empty() => s.trim().to_string(),
        _ => return (400, format!("this call needs an \"id\". Expected {FIXTURE_RUN_SHAPE}.")),
    };
    let which = match browser_arg(&v) {
        Ok(b) => b,
        Err(refused) => return refused,
    };
    let mut browsers = open(which);
    let report = match run_saved(&mut browsers, &root, &ctx.org, &ctx.project, &id, timing).await {
        Ok(r) => r,
        Err(NotRun::Busy) => return (409, API_TEMPLATE_BUSY.to_string()),
        Err(NotRun::Refused(why)) => return (400, why),
    };
    drop(browsers);
    templates_changed(&id);
    let made: Vec<serde_json::Value> =
        report.made.iter().map(|m| serde_json::json!({ "kind": m.kind, "id": m.id, "name": m.name })).collect();
    let out = serde_json::json!({
        "ok": report.ok,
        "sentence": report.message(),
        "outputs": report.outputs,
        "made": made,
        "warnings": report.warnings,
    });
    (if report.ok { 200 } else { 502 }, out.to_string())
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
fn active_environment_section(root: &std::path::Path, ctx: &BridgeContext) -> Option<String> {
    match crate::environments::active(root) {
        Ok(env) => {
            let database = environment_database(ctx, &env);
            Some(crate::autorun::guide::active_environment_section(&env, database.as_ref()))
        }
        Err(e) => {
            crate::applog::warn(&format!("Guide: the active environment could not be read: {e}"));
            None
        }
    }
}

/// The environment's database as the app lists it (`DbDatabase`: label,
/// server and database, never a password), or None when it names none the
/// app knows (the guide then says it is not set up any more, or that there
/// is none when no id is set) - or no store is set up to look it up in,
/// which only a context nobody set up lacks.
fn environment_database(ctx: &BridgeContext, env: &crate::environments::Environment) -> Option<crate::db::DbDatabase> {
    if env.db_id.trim().is_empty() {
        return None;
    }
    let store = ctx.db_secrets.as_deref()?;
    crate::db::credentials::databases(store).into_iter().find(|d| d.id == env.db_id)
}

/// The guide's own text, plus this project's sections when it has any: the
/// module-screen rule while "Scripts may open pages by address" is off,
/// the recorded areas, the areas with no map or a stale one, this
/// project's components, then the recorded quirks. The constant (`autorun::guide::autorun_guide`)
/// only says a quirks section exists; this reads what is actually on file,
/// so the guide can never go stale on a live project.
///
/// `quick` answers the Quick rules section in place of the full text, with
/// the same live sections after it.
fn autorun_guide_with_quirks(ctx: &BridgeContext, quick: bool) -> String {
    let mut out =
        if quick { crate::autorun::guide::quick_rules() } else { crate::autorun::guide::autorun_guide() };
    let Some(root) = crate::autorun::store::configured_root() else {
        return out;
    };
    // The environment is the app's, not a project's: named even with no
    // project open.
    if let Some(section) = active_environment_section(&root, ctx) {
        out.push('\n');
        out.push_str(&section);
    }
    if ctx.project.trim().is_empty() {
        return out;
    }
    let nav = crate::autorun::nav::load_nav(&root, &ctx.org, &ctx.project).unwrap_or_default();
    let quirks = crate::autorun::quirks::load_quirks(&root, &ctx.org, &ctx.project).unwrap_or_default();
    let files = project_test_files(&root, ctx);
    let map = crate::autorun::discovery_map::load_map(&root, &ctx.org, &ctx.project).unwrap_or_else(|e| {
        crate::applog::warn(&format!("Guide: {e}"));
        Default::default()
    });
    let components = crate::autorun::components::load_components(&root, &ctx.org, &ctx.project).unwrap_or_else(|e| {
        crate::applog::warn(&format!("Guide: {e}"));
        Default::default()
    });
    let areas: Vec<&str> = nav.modules.iter().map(|m| m.name()).collect();
    for section in [
        crate::autorun::nav::guide_section(&nav),
        crate::autorun::discovery_map::explore_section(&areas, &map, crate::autorun::sessions::now_ms()),
        crate::autorun::components::guide_section(&components),
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
async fn autorun_page(ctx: &BridgeContext, target: &str) -> (u16, String) {
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
    supervised_page(&ctx.org, &ctx.project, limit).await
}

/// The supervised browser's page as text, `limit` lines at most: what
/// `/autorun-page` answers, and what a replay that reached its step hands
/// back beside its sentence. What it printed is filed in the project's
/// discovery map.
pub async fn supervised_page(organization: &str, project: &str, limit: usize) -> (u16, String) {
    // The lock is held for exactly one protocol job - whoever holds it
    // holds the browser, and the person may be using it.
    let mut slot = crate::commands::autorun::supervised().lock().await;
    let Some(session) = slot.as_mut() else {
        return (409, NO_SUPERVISED_BROWSER.to_string());
    };
    let root = if project.trim().is_empty() { None } else { crate::autorun::store::configured_root() };
    let tabs_case = session.tabs_case;
    let answer = page_read_in(session, root.as_deref(), organization, project, tabs_case, limit).await;
    // A browser that has gone is let go here, not kept to answer 503 again.
    let answer = let_go_if_silent(&mut slot, answer).await;
    crate::commands::autorun::publish_discovery(&slot);
    answer
}

/// Said at the end of a page read during a discovery that has no area yet:
/// nothing was recorded.
pub const READ_WITH_NO_AREA: &str = "Recorded nothing as seen: this discovery has no area yet. Save the area with save_autorun_area, or name it with \"area\" on discover_autorun_action, then read the page again.";

/// Said at the end of a page read during a discovery when the page is not
/// on the application's own origins: nothing was recorded.
pub const READ_OFF_THE_APP: &str =
    "Recorded nothing as seen: this page is not on one of this project's allowed origins.";

/// Said at the end of a page read during a discovery when what it showed
/// could not be filed (the reason is in the log).
pub const READ_NOT_FILED: &str =
    "Recorded nothing as seen: the discovery map could not be written (see Settings -> Logs).";

/// The line a page read during a discovery ends with when it recorded.
pub fn recorded_on(n: usize, area: &str) -> String {
    let what = if n == 1 { "element" } else { "elements" };
    format!("Recorded {n} {what} as seen on {}.", area.trim())
}

/// The page in `browser` as text, `limit` lines at most: what
/// `get_autorun_page` answers. `root` is where the discovery map lives,
/// `None` when there is nowhere to file (no project, no data folder).
///
/// During a discovery a read of the page counts exactly as an action's
/// read of it does (the same page text, filed by the same `read_and_file`),
/// so a read of the whole page explores it. It is filed under the
/// discovery's current area, and the answer ends with one line saying how
/// many elements were recorded as seen there. A discovery with no area yet
/// records nothing and says so (`READ_WITH_NO_AREA`): the case's area or
/// the unattributed bucket would file it where the discovery is not.
///
/// Outside a discovery the read is filed as it always was
/// (`discovery_sighting`: the area of the case the browser last ran, else
/// the unattributed bucket), never explores anything, and the answer is the
/// page text alone.
pub async fn page_read_in<B: DiscoveryBrowser>(
    browser: &mut B,
    root: Option<&std::path::Path>,
    organization: &str,
    project: &str,
    tabs_case: Option<i32>,
    limit: usize,
) -> (u16, String) {
    let p = browser.parts();
    let discovery_area = p.discovery.as_ref().map(|s| named(s.area.as_deref()));
    let at = root.and_then(|root| {
        discovery_sighting(root, organization, project, p.discovery.as_ref(), tabs_case, p.signed_in.as_deref())
    });
    let Some(area) = discovery_area else {
        return read_page(p.driver, limit, at.as_ref()).await;
    };
    let Some(area) = area else {
        let (status, text) = read_page(p.driver, limit, None).await;
        return if status == 200 { (status, format!("{text}\n\n{READ_WITH_NO_AREA}")) } else { (status, text) };
    };
    let (status, text, filed) = read_and_file(p.driver, limit, at.as_ref()).await;
    if status != 200 {
        return (status, text);
    }
    // Named as the map files it.
    let area = match root {
        Some(root) => crate::autorun::discovery_map::canonical_area(root, organization, project, &area),
        None => area,
    };
    let line = match filed {
        Filed::Recorded(n) => recorded_on(n, &area),
        Filed::OffOrigin => READ_OFF_THE_APP.to_string(),
        Filed::Not | Filed::Unrecorded => READ_NOT_FILED.to_string(),
    };
    (status, format!("{text}\n\n{line}"))
}

/// Where what the live page shows is filed in the discovery map
/// (`autorun::discovery_map`): the project, the area it belongs to (`None`
/// is the unattributed bucket), and whether a discovery is under way - in
/// which case a page read stamps the area explored by `account`, an account
/// KEY, never a login, and replaces what the map held for that page.
pub struct Sighting {
    pub root: std::path::PathBuf,
    pub org: String,
    pub project: String,
    pub area: Option<String>,
    pub account: Option<String>,
    /// When the discovery under way started, or `None` outside one.
    pub discovering: Option<u64>,
    /// The application's own origins, as a discovery's `navigate` is held
    /// to them (`runner::policy_for` the project's recipe): a page read
    /// files nothing from a page this does not allow.
    pub policy: crate::browser::actions::Policy,
}

/// The origins a page read may file from: those of the project's recipe in
/// the active environment, as an action's address is checked; any, with no
/// recipe (`runner::policy_for(None)`, as for actions); none, when the
/// recipe cannot be read, so an unreadable recipe never widens what counts.
pub fn recording_policy(root: &std::path::Path, organization: &str, project: &str) -> crate::browser::actions::Policy {
    match crate::autorun::recipe::load_effective_recipe_if_any(root, organization, project) {
        Ok(recipe) => crate::autorun::runner::policy_for(recipe.as_ref()),
        Err(why) => {
            // The error itself can quote the file (an address in it), so
            // only its kind is logged.
            let kind = if why.starts_with("the sign-in recipe is not readable") { "not a valid recipe" } else { "a file error" };
            unrecorded(&format!("the sign-in recipe could not be read ({kind}), so no page read is filed"));
            crate::browser::actions::Policy::only(vec![])
        }
    }
}

/// The area a recording belongs to: the discovery's own area, else the area
/// the saved script of `case_id` names, else none (the unattributed bucket).
/// A blank name counts as none.
pub fn recording_area(root: &std::path::Path, discovery_area: Option<&str>, case_id: Option<i32>) -> Option<String> {
    let named = |a: &str| {
        let a = a.trim();
        (!a.is_empty()).then(|| a.to_string())
    };
    discovery_area.and_then(named).or_else(|| {
        let script = crate::autorun::store::load_script(root, case_id?).ok().flatten()?;
        script.area.as_deref().and_then(named)
    })
}

/// Where the supervised browser's sightings go, or `None` when there is
/// nowhere to file them (no project chosen, no data directory).
fn supervised_sighting(
    session: &crate::commands::autorun::Session,
    organization: &str,
    project: &str,
) -> Option<Sighting> {
    if project.trim().is_empty() {
        return None;
    }
    let root = crate::autorun::store::configured_root()?;
    discovery_sighting(
        &root,
        organization,
        project,
        session.discovery.as_ref(),
        session.tabs_case,
        session.account.as_deref(),
    )
}

/// Where a browser's sightings go under `root`: the discovery's own area
/// and account KEY while one is going (`discovery`), else the area of the
/// case the browser last ran (`tabs_case`) and the account it signed in as
/// (`signed_in`, a key too). `None` when no project is chosen.
pub fn discovery_sighting(
    root: &std::path::Path,
    organization: &str,
    project: &str,
    discovery: Option<&crate::commands::autorun::DiscoveryState>,
    tabs_case: Option<i32>,
    signed_in: Option<&str>,
) -> Option<Sighting> {
    if project.trim().is_empty() {
        return None;
    }
    Some(Sighting {
        area: recording_area(root, discovery.and_then(|s| s.area.as_deref()), tabs_case),
        account: discovery.and_then(|s| s.account.clone()).or_else(|| signed_in.map(str::to_string)),
        discovering: discovery.map(|s| s.started_at),
        policy: recording_policy(root, organization, project),
        root: root.to_path_buf(),
        org: organization.to_string(),
        project: project.to_string(),
    })
}

/// The page the browser is on: its address's path (no host, query or
/// fragment; empty when it cannot be read) and its title. The address is
/// read as `nav::go_to_module` reads it to compare with `arrived`.
pub async fn current_page<D: crate::browser::cdp::Driver>(d: &mut D) -> (String, String) {
    let (href, title) = page_address(d).await;
    let path = if href.trim().is_empty() { String::new() } else { crate::autorun::discovery_map::path_only(&href) };
    (path, title)
}

/// The page's full address (empty when it cannot be read) and its title.
/// The address is only checked against the allowed origins and cut to its
/// path: it is never logged or answered whole.
async fn page_address<D: crate::browser::cdp::Driver>(d: &mut D) -> (String, String) {
    let href = crate::browser::page::eval_value(d, "location.href").await;
    let href = href.ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
    let title = crate::browser::page::eval_value(d, "document.title").await;
    let title = title.ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
    (href, title)
}

/// A recording that could not be written is said in the log and never
/// fails the route that made it.
fn unrecorded(why: &str) {
    crate::applog::warn(format!("Discovery map: what the page showed could not be recorded: {why}"));
}

/// The page as text, `limit` lines at most, with every locator it printed
/// filed at `at`. The text is the same with or without `at`.
pub async fn read_page<D: crate::browser::cdp::Driver>(
    d: &mut D,
    limit: usize,
    at: Option<&Sighting>,
) -> (u16, String) {
    let (status, text, _) = read_and_file(d, limit, at).await;
    (status, text)
}

/// What a page read filed in the discovery map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filed {
    /// Nowhere to file it (`at` was none), or the read failed.
    Not,
    /// This many distinct elements recorded as seen.
    Recorded(usize),
    /// The page is not on the application's own origins (`Sighting::policy`).
    OffOrigin,
    /// It could not be written; the log says why.
    Unrecorded,
}

/// `read_page`, saying what it filed. The single path every page read
/// records through: only the lines the read returned are filed, a page the
/// application's origins do not allow files nothing, and a read the line
/// limit cut short files what it returned without exploring the page
/// (`discovery_map::record_read`).
pub async fn read_and_file<D: crate::browser::cdp::Driver>(
    d: &mut D,
    limit: usize,
    at: Option<&Sighting>,
) -> (u16, String, Filed) {
    let read = match crate::browser::snapshot::snapshot_read(d, limit).await {
        Ok(read) => read,
        Err(e) => return (503, format!("the browser did not answer: {e}"), Filed::Not),
    };
    let Some(at) = at else {
        return (200, read.text, Filed::Not);
    };
    let (href, title) = page_address(d).await;
    if href.trim().is_empty() {
        unrecorded("the page's address could not be read");
        return (200, read.text, Filed::Unrecorded);
    }
    if !may_record(&href, &at.policy) {
        crate::applog::info(format!(
            "Discovery map: a page read was not filed, as the page is not on {}",
            crate::browser::actions::ALLOWED_ORIGINS
        ));
        return (200, read.text, Filed::OffOrigin);
    }
    let recorded = crate::autorun::discovery_map::record_read(
        &at.root,
        &at.org,
        &at.project,
        at.area.as_deref(),
        &crate::autorun::discovery_map::path_only(&href),
        &title,
        &read.lines,
        at.account.as_deref(),
        at.discovering,
        !read.cut,
        crate::autorun::sessions::now_ms(),
    );
    match recorded {
        Ok(n) => (200, read.text, Filed::Recorded(n)),
        Err(why) => {
            unrecorded(&why);
            (200, read.text, Filed::Unrecorded)
        }
    }
}

/// Can a page at `href` (its full address) be filed as seen: only a page
/// of the application counts. An address that is not an http, https or
/// file page (about:blank, a browser error page) never is, even when no
/// recipe limits the origins; one `policy` does not allow never is either.
pub fn may_record(href: &str, policy: &crate::browser::actions::Policy) -> bool {
    crate::autorun::recipe::origin_of(href).is_some() && policy.allows(href)
}

/// Files each locator in `targets` at `at` under the page at `href` (its
/// full address, cut to its path here), only when `may_record` allows that
/// page. Says whether every locator was filed.
fn record_matched_targets(at: &Sighting, href: &str, targets: &[&crate::browser::locator::Target]) -> bool {
    if targets.is_empty() || at.project.trim().is_empty() {
        return false;
    }
    if href.trim().is_empty() {
        unrecorded("the page's address could not be read");
        return false;
    }
    if !may_record(href, &at.policy) {
        crate::applog::info(format!(
            "Discovery map: a matched locator was not filed, as the page is not on {}",
            crate::browser::actions::ALLOWED_ORIGINS
        ));
        return false;
    }
    let path = crate::autorun::discovery_map::path_only(href);
    let now = crate::autorun::sessions::now_ms();
    let mut filed = true;
    for target in targets {
        let area = at.area.as_deref();
        if let Err(why) =
            crate::autorun::discovery_map::record_matched(&at.root, &at.org, &at.project, area, &path, target, now)
        {
            unrecorded(&why);
            filed = false;
        }
    }
    filed
}

/// The locators an action that WORKED must have matched at least once.
/// Kinds that pass while matching nothing give none: `expect_hidden`, an
/// `expect_count` of 0, `expect_no_row`, a `expect_row_count` with no
/// positive bound, and `when_visible`, whose outcome does not say whether
/// its selector was there. Matched without a catch-all, so a new kind will
/// not compile until it is classified here.
fn matched_targets(action: &crate::browser::actions::Action) -> Vec<&crate::browser::locator::Target> {
    use crate::browser::actions::Action;
    match action {
        Action::Click { selector }
        | Action::Fill { selector, .. }
        | Action::Upload { selector, .. }
        | Action::WaitFor { selector, .. }
        | Action::ExpectVisible { selector, .. }
        | Action::ExpectText { selector, .. }
        | Action::ExpectContainsText { selector, .. }
        | Action::ExpectAttribute { selector, .. }
        | Action::ExpectFocused { selector, .. } => vec![selector],
        Action::ExpectCount { selector, equals, .. } => {
            if *equals >= 1 {
                vec![selector]
            } else {
                vec![]
            }
        }
        Action::Drag { from, to, .. } => vec![from, to],
        Action::ExpectRow { table, .. } | Action::ExpectSorted { table, .. } => vec![table],
        Action::ExpectRowCount { table, equals, at_least, .. } => {
            if equals.is_some_and(|n| n >= 1) || at_least.is_some_and(|n| n >= 1) {
                vec![table]
            } else {
                vec![]
            }
        }
        Action::ExpectHidden { .. }
        | Action::UseComponent { .. }
        | Action::ExpectNoRow { .. }
        | Action::WhenVisible { .. }
        | Action::Navigate { .. }
        | Action::CheckText { .. }
        | Action::CheckUrl { .. }
        | Action::SignIn { .. }
        | Action::ExpectResponse { .. }
        | Action::ApiRequest { .. }
        | Action::Reload
        | Action::ExpireSession
        | Action::ReturnToArea { .. }
        | Action::PressKey { .. }
        | Action::ExpectDownload { .. }
        | Action::ExpectTab { .. }
        | Action::OpenTab { .. }
        | Action::SwitchTab { .. }
        | Action::CloseTab { .. }
        | Action::ExpectTabClosed { .. }
        | Action::ExpectDialog { .. } => vec![],
    }
}

/// A probe's answer read back (`snapshot::probe`): how many elements
/// matched (`"matches: N ..."`), and whether the first one listed is
/// visible (its line is `<tag> "<text>" visible at ...`).
struct ProbeAnswer {
    matches: usize,
    first_visible: bool,
}

fn read_probe_answer(text: &str) -> ProbeAnswer {
    let matches = text
        .strip_prefix("matches: ")
        .map(|rest| rest.chars().take_while(char::is_ascii_digit).collect::<String>())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    // After the text's closing quote, which the text itself cannot follow.
    let first_visible = text
        .lines()
        .nth(1)
        .and_then(|line| line.rfind('"').map(|i| &line[i + 1..]))
        .is_some_and(|rest| rest.starts_with(" visible "));
    ProbeAnswer { matches, first_visible }
}

/// How many elements a probe's answer says matched.
fn probe_matches(text: &str) -> usize {
    read_probe_answer(text).matches
}

/// What a locator matches on the page, as the probe answers it; a locator
/// that matched at least once is filed at `at`.
pub async fn probe_page<D: crate::browser::cdp::Driver>(
    d: &mut D,
    target: &crate::browser::locator::Target,
    at: Option<&Sighting>,
) -> (u16, String) {
    let text = match crate::browser::snapshot::probe(d, target).await {
        Ok(text) => text,
        Err(e) => return (503, format!("the browser did not answer: {e}")),
    };
    if let Some(at) = at.filter(|_| probe_matches(&text) > 0) {
        let (href, _) = page_address(d).await;
        record_matched_targets(at, &href, &[target]);
    }
    (200, text)
}

/// Does a probe's answer say it matched exactly one element, and that one
/// visible?
fn one_visible_match(text: &str) -> bool {
    let answer = read_probe_answer(text);
    answer.matches == 1 && answer.first_visible
}

/// Can a locator be probed as it is written: no placeholder (`{{...}}`)
/// and no link left for a component's target input?
fn probeable(target: &crate::browser::locator::Target) -> bool {
    let no_placeholder = serde_json::to_string(target).is_ok_and(|json| !json.contains("{{"));
    no_placeholder && target.links().iter().all(|l| l.input.is_none())
}

/// What a save refused only for unseen locators does while a discovery is
/// open: each refused locator in `targets` that can be probed as written
/// is probed on the discovery browser's current page, as
/// `probe_autorun_locator` probes it (it never clicks or types), and one
/// that matches exactly one visible element is recorded under the
/// discovery's current area. Hands back the locators recorded, as each
/// describes itself; `None` when no discovery is going, and then nothing
/// is probed.
pub async fn record_refused_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    targets: &[crate::browser::locator::Target],
) -> Option<Vec<String>> {
    let browser = slot.as_mut()?;
    let p = browser.parts();
    let state = p.discovery.as_ref()?;
    let at = discovery_sighting(root, organization, project, Some(state), None, p.signed_in.as_deref());
    let mut probed: Vec<String> = Vec::new();
    let mut recorded: Vec<String> = Vec::new();
    // Counted, never named, in the log: a locator is the assistant's
    // writing and can hold an address.
    let mut unchecked = 0usize;
    for target in targets.iter().filter(|t| probeable(t)) {
        let described = target.describe();
        if probed.contains(&described) {
            continue;
        }
        probed.push(described.clone());
        let Ok(text) = crate::browser::snapshot::probe(p.driver, target).await else {
            unchecked += 1;
            continue;
        };
        if !one_visible_match(&text) {
            continue;
        }
        let Some(at) = at.as_ref() else {
            continue;
        };
        let (href, _) = page_address(p.driver).await;
        if record_matched_targets(at, &href, &[target]) {
            recorded.push(described);
        }
    }
    if unchecked > 0 {
        crate::applog::warn(format!("Auto Run save: {unchecked} refused locator(s) could not be checked on the page"));
    }
    if !recorded.is_empty() {
        crate::applog::info(format!(
            "Auto Run save: recorded {} of {} refused locator(s) on the current page",
            recorded.len(),
            targets.len()
        ));
    }
    Some(recorded)
}

/// [`record_refused_in`] for a script save, whose check reads each script's
/// own area: only when every one of `script_areas` is the discovery's
/// current area (trimmed, blank as none, compared as `nav::module_key`
/// compares names). Otherwise a sighting would be filed where the check
/// does not look, so nothing is probed and the answer is `None`.
pub async fn record_refused_for_scripts_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    script_areas: &[Option<&str>],
    targets: &[crate::browser::locator::Target],
) -> Option<Vec<String>> {
    let key = |a: Option<&str>| a.map(str::trim).filter(|a| !a.is_empty()).map(crate::autorun::nav::module_key);
    let here = key(slot.as_mut()?.parts().discovery.as_ref()?.area.as_deref());
    if script_areas.iter().any(|a| key(*a) != here) {
        return None;
    }
    record_refused_in(slot, root, organization, project, targets).await
}

/// [`record_refused_for_scripts_in`] in the supervised browser, holding its
/// lock as `probe_autorun_locator` does. `None` when an unattended run has
/// the browser, no discovery is going, or it is on another area.
async fn record_refused_on_page(
    ctx: &BridgeContext,
    root: &std::path::Path,
    script_areas: &[Option<&str>],
    targets: &[crate::browser::locator::Target],
) -> Option<Vec<String>> {
    if unattended_run_is_using_the_browser().is_some() {
        return None;
    }
    let mut slot = crate::commands::autorun::supervised().lock().await;
    record_refused_for_scripts_in(&mut slot, root, &ctx.org, &ctx.project, script_areas, targets).await
}

/// A save's own answer, after the line that says what a refusal checked on
/// the page recorded.
pub fn after_recording(recorded: &[String], (status, text): (u16, String)) -> (u16, String) {
    let line = if recorded.is_empty() {
        "Recorded on the current page: nothing - no refused locator matched exactly one visible element.".to_string()
    } else {
        format!("Recorded on the current page: {}.", recorded.join(", "))
    };
    (status, format!("{line}\n{text}"))
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
async fn autorun_probe(ctx: &BridgeContext, body: &str) -> (u16, String) {
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
    let at = supervised_sighting(session, &ctx.org, &ctx.project);
    let answer = probe_page(&mut session.cdp, &target, at.as_ref()).await;
    let answer = let_go_if_silent(&mut slot, answer).await;
    crate::commands::autorun::publish_discovery(&slot);
    answer
}

/// The applog line for a tried action: its kind, what it points at (a
/// locator's own words, or a navigate's url / a check_url's address), and
/// whether it worked. Pure and separate from the route so it can be
/// tested without a browser. A `fill`'s VALUE is never in it - only its
/// selector, the same as every other selector-carrying kind.
pub fn describe_try(action: &crate::browser::actions::Action, ok: bool) -> String {
    format!(
        "AI tried {} in the supervised browser: {}",
        describe_action(action),
        if ok { "ok" } else { "failed" }
    )
}

/// An action in a few words: its kind and what it points at, as
/// `describe_try` says it - never a `fill`'s value, a host or a query
/// string. What the log and the discovery map keep of an action.
pub fn describe_action(action: &crate::browser::actions::Action) -> String {
    use crate::browser::actions::Action;
    let kind = serde_json::to_value(action)
        .ok()
        .and_then(|v| v["kind"].as_str().map(str::to_string))
        .unwrap_or_default();
    let what = match action {
        // Its path only: a host and a query string never reach a log line
        // or the discovery map, and a query can carry a token.
        Action::Navigate { url } => crate::autorun::discovery_map::path_only(url),
        Action::CheckUrl { contains } => contains.clone(),
        Action::UseComponent { component, .. } => component.trim().to_string(),
        Action::CheckText { .. }
        | Action::SignIn { .. }
        | Action::Reload
        | Action::ExpireSession
        | Action::ReturnToArea { area: None } => String::new(),
        Action::ReturnToArea { area: Some(area) } => area.trim().to_string(),
        Action::PressKey { key, .. } => key.trim().to_string(),
        Action::ExpectRow { table, .. }
        | Action::ExpectNoRow { table, .. }
        | Action::ExpectSorted { table, .. }
        | Action::ExpectRowCount { table, .. } => table.describe(),
        // The words it checks for are the script's, never a secret.
        Action::ExpectDialog { text, contains, .. } => text.clone().or_else(|| contains.clone()).unwrap_or_default(),
        Action::Drag { from, to, position, .. } => {
            format!("{} {} {}", from.describe(), position.unwrap_or_default().word(), to.describe())
        }
        Action::ExpectDownload { name, .. } => name.trim().to_string(),
        // A tab's name; an opened tab's path, never its host or query.
        Action::ExpectTab { name, .. }
        | Action::SwitchTab { name }
        | Action::CloseTab { name }
        | Action::ExpectTabClosed { name, .. } => name.clone(),
        Action::OpenTab { name, url } => format!("{name} {}", crate::browser::actions::path_only(url)),
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
        | Action::ExpectFocused { selector, .. }
        | Action::Upload { selector, .. }
        | Action::WhenVisible { selector, .. } => selector.describe(),
    };
    let target = if what.is_empty() { String::new() } else { format!(" {what}") };
    format!("{kind}{target}")
}

/// What no action an assistant runs may be: one read from its body by
/// `/autorun-try` or `/autorun-discover-action`.
fn refuse_as_a_tried_action(action: &crate::browser::actions::Action) -> Result<(), (u16, String)> {
    // Signing in needs the tester's own accounts and the project's
    // recipe. It is theirs to drive, and a script never carries a login.
    if matches!(action, crate::browser::actions::Action::SignIn { .. }) {
        return Err((400, "sign_in is not a thing an assistant does - the person signs in".to_string()));
    }
    action.validate().map_err(|why| (400, why))?;
    // `validate()` allows `file://` (a saved script may need it for the
    // live fixture, a development-only tab) but a TRIED action runs
    // against whatever page the person actually has open - sending their
    // browser to a local file is never something a rehearsal should do -
    // nor is one guarded inside a `when_visible`.
    if tried_addresses(action).iter().any(|url| url.trim().to_ascii_lowercase().starts_with("file:")) {
        return Err((400, "a tried navigate goes to http or https only".to_string()));
    }
    Ok(())
}

/// Every address an action would send the browser to: a `navigate`'s or an
/// `open_tab`'s, inside a `when_visible` too.
fn tried_addresses(action: &crate::browser::actions::Action) -> Vec<&str> {
    use crate::browser::actions::Action;
    action
        .each()
        .into_iter()
        .filter_map(|a| match a {
            Action::Navigate { url } | Action::OpenTab { url, .. } => Some(url.as_str()),
            _ => None,
        })
        .collect()
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
    if let Err(refused) = refuse_as_a_tried_action(&action) {
        return refused;
    }
    // The case the try is for: its no-save guard applies, exactly as it
    // does to a step of that case, so a try can never send the save the
    // case itself would have stopped.
    let case_id = match try_case_id(body) {
        Ok(id) => id,
        Err(refused) => return refused,
    };
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
    // Another case's tabs go first, before its unreported stop is read by
    // the guard below (`try_for_case` does this too).
    crate::autorun::runner::tabs_for_case(&mut session.cdp, &mut session.tabs_case, case_id).await;
    // A try adds its case's guard and never lifts one (`guard_for_case`).
    if let Err(why) =
        crate::commands::autorun::guard_supervised(session, &root, &ctx.org, &ctx.project, case_id, false).await
    {
        return (409, why);
    }
    let discovery_area = session.discovery.as_ref().and_then(|s| s.area.clone());
    let answer = try_for_case(
        &mut session.cdp,
        &mut session.tabs_case,
        &mut session.account,
        &mut session.lease,
        &root,
        &ctx.org,
        &ctx.project,
        case_id,
        discovery_area.as_deref(),
        &action,
    )
    .await;
    let answer = let_go_if_silent(&mut slot, answer).await;
    crate::commands::autorun::publish_discovery(&slot);
    answer
}

/// `try_in` for a case in the supervised browser, whose tabs belong to the
/// case it last ran (`tabs_case`). A try for another case starts as that
/// case's first step would: every tab but `main` is closed and `main` is
/// current (`runner::tabs_for_case`), so it never acts in a tab another
/// case left current. `discovery_area` is the area of a discovery under way
/// in that browser: what a try that worked acted on is filed there first.
#[allow(clippy::too_many_arguments)]
pub async fn try_for_case<D: crate::browser::cdp::Driver>(
    d: &mut D,
    tabs_case: &mut Option<i32>,
    account: &mut Option<String>,
    lease: &mut crate::autorun::lease::Held,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    case_id: i32,
    discovery_area: Option<&str>,
    action: &crate::browser::actions::Action,
) -> (u16, String) {
    crate::autorun::runner::tabs_for_case(d, tabs_case, case_id).await;
    try_in_area(d, account, lease, root, organization, project, case_id, discovery_area, action).await
}

// ------------------------------------------------------------ discovery

/// What a discovery works with in the Auto Run browser: its driver, its
/// account lease, the account KEY it is signed in as, and the discovery
/// under way in it (`None` in a browser the person opened).
pub struct DiscoveryParts<'a, D> {
    pub driver: &'a mut D,
    pub lease: &'a mut crate::autorun::lease::Held,
    pub signed_in: &'a mut Option<String>,
    pub discovery: &'a mut Option<crate::commands::autorun::DiscoveryState>,
}

/// The Auto Run browser as a discovery sees it: the app's session
/// (`commands::autorun::Session`), or a test's fake.
pub trait DiscoveryBrowser {
    type D: crate::browser::cdp::Driver;
    fn parts(&mut self) -> DiscoveryParts<'_, Self::D>;
    /// Close the browser, and with it the discovery and its lease.
    fn close(self);
    /// Is the browser still there to drive: its processes run and its own
    /// connection still answers (`commands::autorun::held_browser_alive`)?
    /// A browser the app holds that has gone is let go
    /// (`forget_gone_browser`) rather than kept as busy. A test's fake is
    /// alive unless it says otherwise.
    fn alive(&mut self) -> impl std::future::Future<Output = bool> + Send {
        async { true }
    }
}

/// Said in place of an answer from a held browser that had gone: the app
/// has closed it the normal way and let it go, and none is open now.
pub const BROWSER_GONE: &str = "the Auto Run browser had closed, so the app let it go: no Auto Run browser is open now - start a discovery with start_autorun_discovery, or the person opens one with Open browser on the Auto Run tab";

/// Let go of the browser `slot` holds when it has gone (`alive` says no):
/// a mapping run's summary is kept (`finish_mapping`), the browser is
/// closed through its normal close (which ends what is left of its job and
/// removes its profile) and the slot is emptied. True when it did; a live
/// browser, or none, is left as it is. Called before anything is refused
/// only because a browser is held, and when a held browser fails to
/// answer.
pub async fn forget_gone_browser<B: DiscoveryBrowser>(slot: &mut Option<B>) -> bool {
    match slot.as_mut() {
        None => return false,
        Some(b) => {
            if b.alive().await {
                return false;
            }
        }
    }
    let discovering = slot.as_mut().is_some_and(|b| b.parts().discovery.is_some());
    finish_mapping(slot);
    if let Some(b) = slot.take() {
        crate::browser::tree::blocking(|| b.close());
    }
    let what = if discovering { "discovery browser" } else { "browser" };
    crate::applog::info(format!("Auto Run: the held {what} had closed or stopped answering; it is let go"));
    true
}

/// Refused only while `slot` holds a browser that is really there, with
/// the busy sentence (`busy_browser_sentence`): one that has gone is let go
/// first (`forget_gone_browser`), and nothing is refused for it. Before a
/// discovery opens.
pub async fn refuse_while_held<B: DiscoveryBrowser>(slot: &mut Option<B>) -> Result<(), String> {
    forget_gone_browser(slot).await;
    match slot.as_mut() {
        Some(b) => Err(crate::commands::autorun::busy_browser_sentence(b.parts().discovery.is_some()).to_string()),
        None => Ok(()),
    }
}

/// Does this answer from a held browser say the browser failed, rather
/// than the page: a 503, or the words of a closed browser or a failed
/// DevTools socket?
fn said_browser_failed(answer: &(u16, String)) -> bool {
    answer.0 == 503
        || answer.1.contains(&crate::browser::cdp::CdpError::Closed.to_string())
        || answer.1.contains("DevTools socket failed")
}

/// `answer`, from the browser `slot` holds - unless it says the browser
/// failed and the browser has indeed gone: then it is let go right here
/// (`forget_gone_browser`) and the answer is `BROWSER_GONE`, so the next
/// call does not meet the same dead browser.
pub async fn let_go_if_silent<B: DiscoveryBrowser>(slot: &mut Option<B>, answer: (u16, String)) -> (u16, String) {
    if said_browser_failed(&answer) && forget_gone_browser(slot).await {
        return (409, BROWSER_GONE.to_string());
    }
    answer
}

/// Said to a discovery action, or a page read for one, with no discovery
/// going.
pub const NO_DISCOVERY: &str = "no discovery is going - start one with start_autorun_discovery";

/// Said to a mapping run started with no module named.
pub const MAPPING_NEEDS_A_MODULE: &str = "Name at least one module to map.";

/// Said when the browser a discovery opened was replaced (the person
/// pressed Open browser) before its sign-in.
const DISCOVERY_TAKEN_OVER: &str =
    "the Auto Run browser was taken over before the discovery signed in - start the discovery again";

/// The words of the page's live messages: alerts, statuses and live
/// regions. Read before and after a discovery action, so the ones the
/// action brought up can be told apart.
const STATUS_TEXT_JS: &str = "Array.from(document.querySelectorAll('[role=alert],[role=status],[aria-live=assertive],[aria-live=polite]')).map(e => (e.innerText || '').trim()).filter(t => t.length > 0)";

/// A name the body gave, trimmed; blank is none.
fn named(text: Option<&str>) -> Option<String> {
    text.map(str::trim).filter(|t| !t.is_empty()).map(str::to_string)
}

/// The page's live messages right now (`STATUS_TEXT_JS`), each on one short
/// line. None when the page cannot say.
async fn status_texts<D: crate::browser::cdp::Driver>(d: &mut D) -> Vec<String> {
    match crate::browser::page::eval_value(d, STATUS_TEXT_JS).await {
        Ok(serde_json::Value::Array(items)) => items.iter().filter_map(|v| v.as_str()).map(one_short_line).collect(),
        _ => Vec::new(),
    }
}

/// `text` with every value a `fill` in `action` typed taken out: the map
/// keeps what an action led to, never what was typed to get there.
fn without_typed(text: &str, action: &crate::browser::actions::Action) -> String {
    let mut out = text.to_string();
    for a in action.each() {
        if let crate::browser::actions::Action::Fill { value, .. } = a {
            if !value.trim().is_empty() {
                out = out.replace(value.as_str(), "(typed)");
            }
        }
    }
    out
}

/// Sign the browser a discovery just opened in as `account_key` (a KEY,
/// never a login), and hand back where it landed: `{signed_in, detail,
/// path, page}`, the path with no host or query. The discovery takes the
/// account and `area` once the sign-in arrives, and the landing page is
/// filed under that area, explored by that account. A sign-in that does
/// not arrive - or an account that cannot be signed in at all - closes
/// the browser and says why. A browser the person opened is never signed
/// in or closed here.
pub async fn discover_start_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    account_key: &str,
    area: Option<&str>,
    timing: &crate::browser::timing::Timing,
) -> (u16, String) {
    let key = account_key.trim();
    let failed = {
        let Some(browser) = slot.as_mut() else {
            return (409, NO_SUPERVISED_BROWSER.to_string());
        };
        let p = browser.parts();
        if p.discovery.is_none() {
            return (409, DISCOVERY_TAKEN_OVER.to_string());
        }
        let signed =
            crate::autorun::signin::sign_in_leased(p.driver, root, organization, project, key, p.lease, p.signed_in, timing)
                .await;
        match signed {
            Err(why) => why,
            Ok(out) if !out.ok => out.detail,
            Ok(out) => {
                let area =
                    named(area).map(|a| crate::autorun::discovery_map::canonical_area(root, organization, project, &a));
                let started_at =
                    p.discovery.as_ref().map_or_else(crate::autorun::sessions::now_ms, |s| s.started_at);
                let tried = p.discovery.as_mut().map(|s| std::mem::take(&mut s.tried)).unwrap_or_default();
                // A mapping run's summary is kept under the project it signed in to.
                let mapping = p.discovery.as_mut().and_then(|s| s.mapping.take()).map(|run| {
                    crate::commands::autorun::MappingRun {
                        place: Some(crate::commands::autorun::MappingPlace {
                            root: root.to_path_buf(),
                            organization: organization.to_string(),
                            project: project.to_string(),
                        }),
                        ..run
                    }
                });
                let is_mapping = mapping.is_some();
                *p.discovery = Some(crate::commands::autorun::DiscoveryState {
                    area,
                    account: Some(key.to_string()),
                    started_at,
                    tried,
                    mapping,
                });
                // A mapping run saves nothing: its guard goes on once the
                // sign-in (which itself sends a form) has arrived, and before
                // the page is touched again. A guard that will not go on
                // closes the browser: a mapping run never goes unguarded.
                if let Err(why) = guard_a_mapping_run(p.driver, is_mapping, root, organization, project).await {
                    why
                } else {
                    // The landing page is filed, but does not mark the area
                    // explored: nothing of the area itself has been seen yet.
                    let at =
                        discovery_sighting(root, organization, project, p.discovery.as_ref(), None, p.signed_in.as_deref())
                            .map(|at| Sighting { discovering: None, ..at });
                    let (status, page) =
                        read_page(p.driver, crate::browser::snapshot::DEFAULT_LIMIT, at.as_ref()).await;
                    let (path, _) = current_page(p.driver).await;
                    let mut answer = serde_json::json!({
                        "signed_in": true,
                        "detail": out.detail,
                        "path": path,
                        "page": if status == 200 { page.as_str() } else { "" },
                    });
                    if status != 200 {
                        answer["page_unavailable"] = serde_json::json!(page);
                    }
                    let what = if is_mapping { "mapping run" } else { "discovery" };
                    crate::applog::info(format!("Auto Run {what} started as {key}"));
                    return (200, answer.to_string());
                }
            }
        }
    };
    if let Some(browser) = slot.take() {
        browser.close();
    }
    crate::applog::info(format!("Auto Run discovery as {key} did not start; its browser is closed"));
    (409, failed)
}

/// Switch a mapping run's save guard on, as a Must not save case's is
/// (`commands::autorun::guard_for_case`): the built-in save words and the
/// project's own. Nothing to do for an ordinary discovery.
async fn guard_a_mapping_run<D: crate::browser::cdp::Driver>(
    d: &mut D,
    is_mapping: bool,
    root: &std::path::Path,
    organization: &str,
    project: &str,
) -> Result<(), String> {
    if !is_mapping {
        return Ok(());
    }
    let nav = crate::autorun::nav::load_nav(root, organization, project)
        .map_err(|why| crate::browser::save_guard::setup_failed(&why))?;
    d.guard_saves(&nav.save_words).await.map_err(|e| crate::browser::save_guard::setup_failed(&e.to_string()))
}

/// The saves the guard stopped since the last time this was asked, for a
/// mapping run: added to the run's count and logged by method and path
/// only, with nothing an action in `ran` typed. How many, or `None` for an
/// ordinary discovery.
fn count_blocked_writes<D: crate::browser::cdp::Driver>(
    d: &mut D,
    discovery: Option<&mut crate::commands::autorun::DiscoveryState>,
    ran: &[&crate::browser::actions::Action],
) -> Option<usize> {
    let run = discovery.and_then(|s| s.mapping.as_mut())?;
    let stopped = crate::browser::cdp::Driver::take_saves_stopped(d);
    for (method, path) in &stopped {
        let path = ran.iter().fold(crate::autorun::discovery_map::path_only(path), |p, a| without_typed(&p, a));
        crate::applog::warn(format!("Auto Run mapping run blocked a write: {method} {path}"));
    }
    run.blocked_writes = run.blocked_writes.saturating_add(u32::try_from(stopped.len()).unwrap_or(u32::MAX));
    Some(stopped.len())
}

/// What one action a discovery ran did: its outcome, the dialog it raised,
/// the writes the page sent and the path it ended on.
struct Discovered {
    outcome: crate::browser::actions::ActionOutcome,
    dialogs: Vec<String>,
    writes: Vec<serde_json::Value>,
    after: String,
}

/// One action in the discovery's browser, run as `/autorun-try` runs one
/// (`try_action`, case 0), with what it set off: `{ok, detail, path,
/// dialogs, writes, page}`. Every write request the page sent (`SAVE_METHODS`)
/// is logged in the area's map by method and path - never a query string -
/// the page it ended on is filed, and a line saying what the action led to
/// goes into the area's outcomes. Neither ever holds a value a `fill` typed.
/// A non-blank `area` moves the discovery to that area first.
///
/// A `use_component` is expanded before the browser is touched: from
/// `draft` when one is given, so a component can be tried before it is
/// saved, else from the saved one. Each of its actions then runs as a
/// single action does, and the try stops at the first that fails, as a
/// script step does. Its answer adds `steps: [{action, ok, detail}]`, with
/// `ok` and `detail` the try's as a whole, and a try that worked is kept
/// on the discovery by `components::draft_fingerprint`.
///
/// In a mapping run the answer adds `blocked`: how many saves the guard
/// stopped since the last action (`count_blocked_writes`).
pub async fn discover_action_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    action: &crate::browser::actions::Action,
    draft: Option<&crate::autorun::components::Component>,
    area: Option<&str>,
) -> (u16, String) {
    let Some(browser) = slot.as_mut() else {
        return (409, NO_DISCOVERY.to_string());
    };
    let p = browser.parts();
    let Some(state) = p.discovery.as_mut() else {
        return (409, NO_DISCOVERY.to_string());
    };
    let component = match action {
        crate::browser::actions::Action::UseComponent { component, inputs } => {
            match component_to_try(root, organization, project, component, inputs, draft) {
                Ok(found) => Some(found),
                Err(refused) => return refused,
            }
        }
        _ => None,
    };
    if let Some(moved) = named(area) {
        state.area = Some(crate::autorun::discovery_map::canonical_area(root, organization, project, &moved));
    }
    let area_name = state.area.clone();
    let mapping = state.mapping.is_some();
    let d = p.driver;
    let runs: Vec<&crate::browser::actions::Action> = match &component {
        Some((_, actions)) => actions.iter().collect(),
        None => vec![action],
    };
    let (mut steps, mut writes, mut dialogs) = (Vec::new(), Vec::new(), Vec::new());
    let mut last = None;
    let mut after = String::new();
    for (i, one) in runs.iter().copied().enumerate() {
        let mut ran =
            match discover_one(d, p.signed_in, p.lease, root, organization, project, area_name.as_deref(), mapping, one)
                .await
            {
                Ok(ran) => ran,
                Err(refused) => return refused,
            };
        // A component that fails on its first action says where the
        // browser is: most often it stands on another screen.
        if component.is_some() && i == 0 && !ran.outcome.ok {
            ran.outcome.detail = with_page_where(&ran.outcome.detail, &ran.after);
        }
        steps.push(serde_json::json!({
            "action": describe_action(one),
            "ok": ran.outcome.ok,
            "detail": ran.outcome.detail,
        }));
        writes.extend(ran.writes);
        dialogs.extend(ran.dialogs);
        after = ran.after;
        let failed = !ran.outcome.ok;
        last = Some(ran.outcome);
        if failed {
            break;
        }
    }
    let Some(outcome) = last else {
        return (500, "the action produced no outcome".to_string());
    };
    if let (Some((c, _)), true) = (&component, outcome.ok) {
        let print = crate::autorun::components::draft_fingerprint(c);
        if let Some(state) = p.discovery.as_mut() {
            if !state.tried.contains(&print) {
                state.tried.push(print);
            }
        }
    }
    let at = discovery_sighting(root, organization, project, p.discovery.as_ref(), None, p.signed_in.as_deref());
    let (status, page) = read_page(d, crate::browser::snapshot::DEFAULT_LIMIT, at.as_ref()).await;
    let blocked = count_blocked_writes(d, p.discovery.as_mut(), &runs);
    let mut answer = serde_json::json!({
        "ok": outcome.ok,
        "detail": outcome.detail,
        "path": after,
        "dialogs": dialogs,
        "writes": writes,
        "page": if status == 200 { page.as_str() } else { "" },
    });
    if let Some(n) = blocked {
        answer["blocked"] = serde_json::json!(n);
    }
    if component.is_some() {
        answer["steps"] = serde_json::json!(steps);
    }
    if status != 200 {
        answer["page_unavailable"] = serde_json::json!(page);
    }
    if let Some(shot) = &outcome.screenshot {
        answer["picture"] = serde_json::json!(crate::autorun::store::shot_path(root, shot).display().to_string());
    }
    (200, answer.to_string())
}

/// The batch cap as a literal, so `MAX_BATCH` and the sentences that name
/// it are built from the one number.
macro_rules! max_batch {
    () => {
        20
    };
}

/// The most actions `discover_autorun_actions` runs in one call.
pub const MAX_BATCH: usize = max_batch!();

/// Said to a batch of more than `MAX_BATCH` actions.
pub const BATCH_TOO_LONG: &str = concat!("at most ", max_batch!(), " actions in one call");

/// Said to a batch with no actions in it.
pub const BATCH_EMPTY: &str = "send at least one action in \"actions\"";

/// A batch's size, refused when it holds nothing or more than `MAX_BATCH`.
pub fn refuse_batch_size(n: usize) -> Result<(), (u16, String)> {
    match n {
        0 => Err((400, BATCH_EMPTY.to_string())),
        n if n > MAX_BATCH => Err((400, BATCH_TOO_LONG.to_string())),
        _ => Ok(()),
    }
}

/// The draft a batch's action is tried with: `draft` for a `use_component`
/// that names it, else none (a saved component, or not a component).
fn draft_for<'a>(
    action: &crate::browser::actions::Action,
    draft: Option<&'a crate::autorun::components::Component>,
) -> Option<&'a crate::autorun::components::Component> {
    let key = crate::autorun::nav::module_key;
    match (action, draft) {
        (crate::browser::actions::Action::UseComponent { component, .. }, Some(c)) if key(&c.name) == key(component) => {
            Some(c)
        }
        _ => None,
    }
}

/// One line of a batch's answer for an action that ran, from its single
/// answer (`discover_action_in`'s JSON): ok or failed with the detail, the
/// dialogs it raised, the writes it set off (method and path only), the
/// page path when it changed, whether its own page read failed, and where
/// its picture is.
fn batch_line(n: usize, action: &crate::browser::actions::Action, v: &serde_json::Value, last_path: &mut Option<String>) -> String {
    let ok = v["ok"].as_bool().unwrap_or(false);
    let detail = v["detail"].as_str().map(str::trim).filter(|d| !d.is_empty());
    let mut said = one_short_line(&detail.map(str::to_string).unwrap_or_else(|| describe_action(action)));
    for dialog in v["dialogs"].as_array().into_iter().flatten().filter_map(|d| d.as_str()) {
        said.push_str(&format!("; dialog {}", one_short_line(dialog)));
    }
    for write in v["writes"].as_array().into_iter().flatten() {
        let (method, path) = (write["method"].as_str().unwrap_or(""), write["path"].as_str().unwrap_or(""));
        said.push_str(&format!("; wrote {method} {path}"));
    }
    if let Some(path) = v["path"].as_str().filter(|p| !p.trim().is_empty()) {
        if last_path.as_deref() != Some(path) {
            said.push_str(&format!("; page {path}"));
            *last_path = Some(path.to_string());
        }
    }
    if v.get("page_unavailable").is_some() {
        said.push_str("; page unreadable");
    }
    if let Some(picture) = v["picture"].as_str() {
        said.push_str(&format!("; picture {picture}"));
    }
    format!("{n}. {}: {said}", if ok { "ok" } else { "failed" })
}

/// Said when a batch stopped at a failure: how many actions were not run.
fn not_run_line(n: usize) -> String {
    let what = if n == 1 { "action was" } else { "actions were" };
    format!("Stopped at the failure: {n} {what} not run.")
}

/// Said when a batch gave way to End discovery, Close browser or a
/// release: how many actions were not run, and what may have asked. A
/// release that timed out still asked, so the line never claims the
/// discovery ended.
pub fn ended_line(n: usize) -> String {
    format!("Stopped: {n} not run - End discovery, Close browser or a release asked the batch to stop.")
}

/// The batch's stop control: set by End discovery, Close browser and both
/// releases before they wait for the browser, so a batch holding it gives
/// way before its next action instead of keeping them waiting. Cleared
/// when a batch hears it, and before each batch starts.
pub static BATCH_STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Ask the batch going, if any, to stop before its next action.
pub fn stop_batch() {
    BATCH_STOP.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// A batch's answer when the browser failed under it: `lines` so far, and
/// when the browser has gone (`let_go_if_silent` lets it go right here)
/// `BROWSER_GONE` after them, with 409. A browser still there keeps the
/// failure's own status.
async fn batch_ended_by_the_browser<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    failure: (u16, String),
    mut lines: Vec<String>,
) -> (u16, String) {
    let (status, said) = let_go_if_silent(slot, failure).await;
    if said == BROWSER_GONE {
        lines.push(BROWSER_GONE.to_string());
        return (409, lines.join("\n"));
    }
    (status, lines.join("\n"))
}

/// Several actions in the discovery's browser, in order, each exactly as
/// `discover_action_in` runs one: its own blocking and counting in a
/// mapping run, its own page read and the sightings that read records, and
/// a `use_component` naming `draft` tried with it. `area`, when named,
/// moves the discovery there once, before the first action, so even a
/// first action refused before the browser is touched leaves the batch in
/// the named area. The caller holds the browser for the whole batch, so
/// nothing comes between its actions; `stop` (`BATCH_STOP` from the route)
/// is checked before each action, and when it is set the batch answers what
/// ran and lets the browser go at once.
///
/// It stops at the first action that fails unless `stop_on_failure` is
/// false; a browser that fails stops it either way, and a browser that has
/// gone is let go here, keeping the lines of the actions that ran. The
/// answer is one line per action that ran (`N. ok: ...` or `N. failed:
/// ...`), a line for the actions not run, the saves a mapping run blocked
/// when there were any, and then the page once, as `get_autorun_page`
/// answers it after the last action (`page_read_in`). As a whole read, that
/// last read replaces the page's elements just as `get_autorun_page` after
/// a single action does: something only the last action's own read saw (a
/// message that has since gone) is dropped.
#[allow(clippy::too_many_arguments)]
pub async fn discover_actions_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    actions: &[crate::browser::actions::Action],
    draft: Option<&crate::autorun::components::Component>,
    area: Option<&str>,
    stop_on_failure: bool,
    stop: &std::sync::atomic::AtomicBool,
) -> (u16, String) {
    if let Err(refused) = refuse_batch_size(actions.len()) {
        return refused;
    }
    let Some(state) = slot.as_mut().and_then(|b| b.parts().discovery.as_mut()) else {
        return (409, NO_DISCOVERY.to_string());
    };
    if let Some(moved) = named(area) {
        state.area = Some(crate::autorun::discovery_map::canonical_area(root, organization, project, &moved));
    }
    let mut lines = Vec::new();
    let mut blocked = 0usize;
    let mut last_path = None;
    let mut ran = 0;
    for (i, action) in actions.iter().enumerate() {
        if stop.swap(false, std::sync::atomic::Ordering::SeqCst) {
            lines.push(ended_line(actions.len() - i));
            return (409, lines.join("\n"));
        }
        let (status, body) =
            discover_action_in(slot, root, organization, project, action, draft_for(action, draft), None).await;
        ran = i + 1;
        if status != 200 {
            // The browser failed or the discovery is gone: nothing after
            // it can run.
            let stopped = said_browser_failed(&(status, body.clone())) || body == NO_DISCOVERY;
            lines.push(format!("{}. failed: {}", i + 1, one_short_line(&body)));
            if stopped {
                if ran < actions.len() {
                    lines.push(not_run_line(actions.len() - ran));
                }
                return batch_ended_by_the_browser(slot, (status, body), lines).await;
            }
            if stop_on_failure {
                break;
            }
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
        blocked += v["blocked"].as_u64().map_or(0, |n| usize::try_from(n).unwrap_or(usize::MAX));
        lines.push(batch_line(i + 1, action, &v, &mut last_path));
        if stop_on_failure && !v["ok"].as_bool().unwrap_or(false) {
            break;
        }
    }
    if ran < actions.len() {
        lines.push(not_run_line(actions.len() - ran));
    }
    if blocked > 0 {
        lines.push(format!("Saves blocked by the mapping run: {blocked}."));
    }
    let Some(browser) = slot.as_mut() else {
        return (409, NO_DISCOVERY.to_string());
    };
    let filing = if project.trim().is_empty() { None } else { Some(root) };
    let (status, page) =
        page_read_in(browser, filing, organization, project, None, crate::browser::snapshot::DEFAULT_LIMIT).await;
    if status != 200 {
        lines.push(format!("The page could not be read: {}", one_short_line(&page)));
        return batch_ended_by_the_browser(slot, (status, page), lines).await;
    }
    (200, format!("{}\n\n{page}", lines.join("\n")))
}

/// Said to a discovery's `use_component` naming a component the project
/// has not saved, sent without a `draft`: how a new one is tried.
pub fn not_saved_try_draft(name: &str) -> String {
    format!("\"{}\" is not saved in this project - to try a new one, send it as draft", name.trim())
}

/// A component's first action that failed in a discovery, with where the
/// browser is: the page's path only, never its host or query. A failure
/// there is often the browser on another screen than the component starts
/// on, not a wrong locator.
pub fn with_page_where(detail: &str, path: &str) -> String {
    // Unknown (the address could not be read): no hint at all, rather
    // than the "/" `path_only` makes of nothing.
    if path.trim().is_empty() {
        return detail.to_string();
    }
    let path = crate::autorun::discovery_map::path_only(path);
    format!("{detail} (the page is {path})")
}

/// The component a discovery's `use_component` tries, and its actions with
/// the inputs put in: `draft` when given (it must be the component the
/// action names), else the saved one. Each expanded action is refused as a
/// tried action is, and one that would go to a site outside the sign-in
/// recipe's is refused too, all before the browser is touched.
fn component_to_try(
    root: &std::path::Path,
    organization: &str,
    project: &str,
    name: &str,
    inputs: &serde_json::Map<String, serde_json::Value>,
    draft: Option<&crate::autorun::components::Component>,
) -> Result<(crate::autorun::components::Component, Vec<crate::browser::actions::Action>), (u16, String)> {
    use crate::autorun::components;
    let key = crate::autorun::nav::module_key;
    let c = match draft {
        Some(c) if key(&c.name) == key(name) => c.clone(),
        Some(c) => {
            return Err((400, format!("the draft is {}, but the action uses {}", c.name.trim(), name.trim())));
        }
        None => {
            let file = components::load_components(root, organization, project).map_err(|why| (409, why))?;
            components::find(&file, name).cloned().ok_or_else(|| (400, not_saved_try_draft(name)))?
        }
    };
    let actions = components::expand(&c, inputs).map_err(|why| (400, why))?;
    if actions.is_empty() {
        return Err((400, format!("{} has no actions to try", c.name.trim())));
    }
    for a in &actions {
        refuse_as_a_tried_action(a)?;
    }
    if actions.iter().any(|a| !tried_addresses(a).is_empty()) {
        let recipe =
            crate::autorun::recipe::load_effective_recipe(root, organization, project).map_err(|why| (409, why))?;
        let policy = crate::autorun::runner::policy_for(Some(&recipe));
        for a in &actions {
            refuse_outside_the_recipe(&policy, a)?;
        }
    }
    Ok((c, actions))
}

/// One action of a discovery, with the writes it set off logged and the
/// line saying what it led to filed (`discover_action_in`). In a mapping
/// run (`mapping`) a save the guard stopped never fails the action: it is
/// counted (`count_blocked_writes`), and the action keeps its own outcome.
#[allow(clippy::too_many_arguments)]
async fn discover_one<D: crate::browser::cdp::Driver>(
    d: &mut D,
    signed_in: &mut Option<String>,
    lease: &mut crate::autorun::lease::Held,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    area_name: Option<&str>,
    mapping: bool,
    action: &crate::browser::actions::Action,
) -> Result<Discovered, (u16, String)> {
    let recording = !project.trim().is_empty();
    let bucket = area_name.unwrap_or("");
    let before = current_page(d).await.0;
    let notes_before = status_texts(d).await;
    let mark = crate::browser::cdp::Driver::net_mark(d);
    let tried = try_action(d, signed_in, lease, root, organization, project, 0, area_name, mapping, action).await?;
    let what = describe_action(action);
    let now = crate::autorun::sessions::now_ms();
    let mut writes = Vec::new();
    for sent in crate::browser::cdp::Driver::net_since(d, mark) {
        if !crate::browser::save_guard::SAVE_METHODS.contains(&sent.method.as_str()) {
            continue;
        }
        let path = without_typed(&crate::autorun::discovery_map::path_only(&sent.path_query), action);
        if recording {
            let entry = crate::autorun::discovery_map::WriteEntry {
                method: sent.method.clone(),
                path: path.clone(),
                at: now,
                step: what.clone(),
            };
            if let Err(why) = crate::autorun::discovery_map::record_write(root, organization, project, bucket, entry) {
                unrecorded(&why);
            }
        }
        writes.push(serde_json::json!({ "method": sent.method, "path": path }));
    }
    let after = current_page(d).await.0;
    let new_notes: Vec<String> =
        status_texts(d).await.into_iter().filter(|note| !notes_before.contains(note)).collect();
    let dialogs: Vec<String> = tried.dialog.iter().map(|x| format!("{}: {}", x.kind, x.message)).collect();
    if tried.outcome.ok && recording {
        let led_to = match (&tried.dialog, new_notes.first()) {
            (Some(x), _) => format!("{} \"{}\"", x.kind, one_short_line(&x.message)),
            (None, Some(note)) => format!("\"{note}\""),
            (None, None) if !after.is_empty() && after != before => format!("moved to {after}"),
            (None, None) => "nothing visible changed".to_string(),
        };
        let line = one_short_line(&without_typed(&format!("{what}: {led_to}"), action));
        if let Err(why) = crate::autorun::discovery_map::record_outcome(root, organization, project, bucket, &line) {
            unrecorded(&why);
        }
    }
    Ok(Discovered { outcome: tried.outcome, dialogs, writes, after })
}

/// End the discovery in `slot`: its browser is closed. A browser the
/// person opened is left as it is, and with nothing to end this is still
/// an answer, not an error - ending twice is fine.
///
/// A mapping run's summary is kept first (`finish_mapping`), and the
/// answer is `{detail, summary}`.
pub fn end_discovery_in<B: DiscoveryBrowser>(slot: &mut Option<B>) -> (u16, String) {
    const ENDED: &str = "the discovery is over and its browser is closed";
    match close_discovery(slot) {
        Some(Some(summary)) => (200, serde_json::json!({ "detail": ENDED, "summary": summary }).to_string()),
        Some(None) => (200, ENDED.to_string()),
        None => (200, "no discovery is going".to_string()),
    }
}

/// The one way a discovery in `slot` ends: a mapping run's summary is kept
/// (`finish_mapping`), then its browser is closed. `None` with no discovery
/// going, else the mapping run's summary, if it was one.
fn close_discovery<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
) -> Option<Option<crate::autorun::mapping_summary::MappingSummary>> {
    if !slot.as_mut().is_some_and(|b| b.parts().discovery.is_some()) {
        return None;
    }
    let summary = finish_mapping(slot);
    if let Some(browser) = slot.take() {
        browser.close();
    }
    crate::applog::info("Auto Run discovery ended; its browser is closed");
    Some(summary)
}

/// Close out the mapping run in `slot`, if one is going, however it ends:
/// the end route, End discovery, Close browser, or the app exiting with
/// it open (a browser that died ends one of those ways). The saves its
/// guard blocked since the last action are counted first, then its summary
/// (`mapping_summary::summarize`) is kept under its project, replacing the
/// last, and logged as one line of counts and names. The run is taken off
/// the discovery, so closing it twice keeps one summary. `None` with no
/// mapping run going.
pub fn finish_mapping<B: DiscoveryBrowser>(slot: &mut Option<B>) -> Option<crate::autorun::mapping_summary::MappingSummary> {
    use crate::autorun::mapping_summary::{log_line, save_summary, summarize};
    let browser = slot.as_mut()?;
    let p = browser.parts();
    count_blocked_writes(p.driver, p.discovery.as_mut(), &[]);
    let run = p.discovery.as_mut()?.mapping.take()?;
    let summary = summarize(&run);
    if let Some(place) = &run.place {
        if let Err(why) = save_summary(&place.root, &place.organization, &place.project, &summary) {
            crate::applog::warn(format!("Auto Run mapping run: its summary could not be kept: {why}"));
        }
    }
    crate::applog::info(log_line(&summary));
    Some(summary)
}

/// Refused while a discovery holds the browser in `slot`: a replay, or the
/// person's Open browser, would otherwise take the discovery's browser
/// over. The sentence names `end_autorun_discovery`.
pub fn refuse_while_discovering<B: DiscoveryBrowser>(slot: &mut Option<B>) -> Result<(), String> {
    if slot.as_mut().is_some_and(|b| b.parts().discovery.is_some()) {
        return Err(crate::commands::autorun::busy_browser_sentence(true).to_string());
    }
    Ok(())
}

/// Before the assistant's replay to a step takes the browser in `slot`: a
/// discovery holding it is ended exactly as End discovery ends it
/// (`end_discovery_in`), so what it mapped is kept and a mapping run's
/// summary is saved. Says whether one was ended, with that summary. Called
/// under the session lock the replay then keeps while it opens and runs, so
/// nothing takes the browser between the end and the replay. The person's
/// own Replay to step still goes through `refuse_while_discovering`.
pub fn end_discovery_for_replay<B: DiscoveryBrowser>(slot: &mut Option<B>) -> DiscoveryEnded {
    match close_discovery(slot) {
        Some(summary) => {
            crate::applog::info("Auto Run: the assistant's replay to a step ended its discovery first");
            DiscoveryEnded { ended: true, summary }
        }
        None => DiscoveryEnded::default(),
    }
}

/// Whether the assistant's replay ended a discovery before it took the
/// browser (`end_discovery_for_replay`), and that mapping run's summary
/// when it was one.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DiscoveryEnded {
    pub ended: bool,
    pub summary: Option<crate::autorun::mapping_summary::MappingSummary>,
}

/// Put before the answer to an assistant's replay that ended its discovery
/// first (`end_discovery_for_replay`).
pub const ENDED_DISCOVERY_FIRST: &str =
    "Ended your discovery first (what it mapped is kept); start a new one to explore again.";

/// `said`, after `ENDED_DISCOVERY_FIRST` when the replay ended a discovery:
/// its answer, or the browser that would not open after the end.
pub fn after_ended_discovery(ended: bool, said: &str) -> String {
    match ended {
        true => format!("{ENDED_DISCOVERY_FIRST} {said}"),
        false => said.to_string(),
    }
}

/// Close whatever browser `slot` holds - a discovery ends with it. Whether
/// there was one.
pub fn close_browser_in<B: DiscoveryBrowser>(slot: &mut Option<B>) -> bool {
    finish_mapping(slot);
    match slot.take() {
        Some(browser) => {
            // Closing waits for the browser's processes to go: off the
            // async worker.
            crate::browser::tree::blocking(|| browser.close());
            true
        }
        None => false,
    }
}

/// How many lines of the page a refused area hands back.
const AREA_PAGE_LINES: usize = 40;

/// How many areas one mapping run may save, added and updated together.
pub const MAPPING_CAP: usize = 150;

/// Said to a mapping run's save past `MAPPING_CAP`.
pub const MAPPING_CAP_REACHED: &str = "This mapping run has saved 150 screens; end it and start another for the rest.";

/// Said to a mapping run's save of a name a person's area already has.
fn person_area_kept(name: &str) -> String {
    format!("An area named '{name}' was recorded by a person, so this mapping run leaves it as it is")
}

/// Save the clicks a discovery found as an area named `name` under the
/// test-case Module `module`, once a replay of them from home has arrived
/// where the browser stands now: `{saved, arrived}`, and the discovery
/// moves to the new area. The replay is the run's own trip (`go_to_module`
/// from a fresh home, signed in again as the discovery's account when the
/// session has gone), so a saved area is one a run can take. Clicks that
/// do not arrive save nothing, and the refusal says where they stopped and
/// what the page showed. A name already taken is refused before the
/// browser is touched: the person replaces an area, never the assistant.
///
/// In a mapping run (spec 2026-10-09 section 2) the area is saved as the
/// run's own (`MadeBy::Mapping`), and every save is counted on the run:
/// past `MAPPING_CAP` saves it is refused before the browser is touched; a
/// name a person's area has is refused and counted unchanged; a name the
/// run's own area has is replayed and saved again only when its clicks or
/// where it arrives changed (`{saved: false, unchanged: true}` and no
/// write otherwise); clicks that do not arrive are counted unreached.
/// Outside one, every area saved is the person's.
///
/// The saves the guard blocked on the replay's trip are counted on the run
/// whatever the answer (`count_blocked_writes`).
#[allow(clippy::too_many_arguments)]
pub async fn discover_area_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    name: &str,
    module: &str,
    clicks: Vec<crate::browser::locator::Target>,
    timing: &crate::browser::timing::Timing,
) -> (u16, String) {
    let answer = save_discovered_area(slot, root, organization, project, name, module, clicks, timing).await;
    if let Some(browser) = slot.as_mut() {
        let p = browser.parts();
        count_blocked_writes(p.driver, p.discovery.as_mut(), &[]);
    }
    answer
}

/// Where a discovery stands once the area `kept` is saved or found as it
/// was. An ordinary discovery goes on to explore that area, so it becomes
/// current. A mapping run files the screen it arrived on under `kept` now,
/// then stands in no area: the pages it reads on its way to the next screen
/// belong to none of the screens it saved.
async fn stand_in_saved_area<D: crate::browser::cdp::Driver>(
    d: &mut D,
    discovery: &mut Option<crate::commands::autorun::DiscoveryState>,
    signed_in: Option<&str>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    kept: String,
) {
    let Some(state) = discovery.as_mut() else { return };
    let mapping = state.mapping.is_some();
    state.area = Some(kept);
    if !mapping {
        return;
    }
    let at = discovery_sighting(root, organization, project, discovery.as_ref(), None, signed_in);
    let _ = read_page(d, crate::browser::snapshot::DEFAULT_LIMIT, at.as_ref()).await;
    if let Some(state) = discovery.as_mut() {
        state.area = None;
    }
}

/// `discover_area_in`, before the blocked saves are counted.
#[allow(clippy::too_many_arguments)]
async fn save_discovered_area<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    name: &str,
    module: &str,
    clicks: Vec<crate::browser::locator::Target>,
    timing: &crate::browser::timing::Timing,
) -> (u16, String) {
    use crate::autorun::nav;
    let Some(browser) = slot.as_mut() else {
        return (409, NO_DISCOVERY.to_string());
    };
    let p = browser.parts();
    let Some(state) = p.discovery.as_ref() else {
        return (409, NO_DISCOVERY.to_string());
    };
    let (name, module) = (name.trim().to_string(), module.trim().to_string());
    if name.is_empty() || module.is_empty() || clicks.is_empty() {
        return (400, "an area needs a name, the module it belongs to and at least one click".to_string());
    }
    let saved_this_run = state.mapping.as_ref().map(|run| run.added.len() + run.updated.len());
    if saved_this_run.is_some_and(|n| n >= MAPPING_CAP) {
        return (409, MAPPING_CAP_REACHED.to_string());
    }
    let mapping = saved_this_run.is_some();
    let navfile = match nav::load_nav(root, organization, project) {
        Ok(n) => n,
        Err(why) => return (409, why),
    };
    // The run's own area of this name, which this save may update.
    let existing = match nav::find_area(&navfile, &name) {
        None => None,
        Some(found) if mapping && found.made_by == nav::MadeBy::Mapping => {
            if let Err(why) = nav::check_area_free(&navfile, &name, &module) {
                return (409, why);
            }
            Some(found.clone())
        }
        Some(found) if mapping => {
            let kept = found.name().to_string();
            // Located where the person's area is, so it can stand for an
            // unreached entry of the same screen under another name.
            let arrived = found.arrived.trim();
            let screen = crate::commands::autorun::MappingScreen {
                arrived: (!arrived.is_empty()).then(|| arrived.to_string()),
                menu: nav::menu_path(&found.clicks),
            };
            if let Some(run) = p.discovery.as_mut().and_then(|s| s.mapping.as_mut()) {
                run.record_unchanged(kept.clone());
                run.locate_last(screen);
            }
            return (409, person_area_kept(&kept));
        }
        Some(_) => {
            return (
                409,
                format!(
                    "An area named '{name}' already exists; pick another name or ask the person to replace it in Auto Run"
                ),
            )
        }
    };
    let Some(state) = p.discovery.as_ref() else {
        return (409, NO_DISCOVERY.to_string());
    };
    let recipe = match crate::autorun::recipe::load_effective_recipe(root, organization, project) {
        Ok(r) => r,
        Err(why) => return (409, why),
    };
    let account = state.account.clone().or_else(|| p.signed_in.clone());
    let d = p.driver;
    // Where the clicks must arrive: where the assistant clicked its way to,
    // read as a run's trip reads it.
    let href = crate::browser::page::eval_value(d, "location.href").await;
    let href = href.ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
    let arrived = if href.trim().is_empty() { String::new() } else { nav::path_of(&href) };
    if arrived.is_empty() {
        return (409, "the page would not say where it is - read the page and try again".to_string());
    }
    // Where the screen is, for a mapping run's summary, however this ends.
    let screen = crate::commands::autorun::MappingScreen { arrived: Some(arrived.clone()), menu: nav::menu_path(&clicks) };
    // The check starts from a fresh home page, never from where the page
    // stands: PeoplesHR's menu remembers whether a person left it open, so
    // the clicks would otherwise depend on it (spec 2026-10-09, mapped
    // areas). `load_home` runs `after_sign_in` once, on the fresh load.
    let home = nav::Home::of(&recipe);
    let mut went = nav::load_home(d, &home, timing).await;
    let signed_in = went.ok
        && crate::browser::expect::expect(
            d,
            &home.signed_in,
            crate::browser::expect::Check::Visible,
            timing.nav_ms,
            timing.poll_ms,
        )
        .await
        .ok;
    if !signed_in {
        // The saved session has gone: sign in again as the discovery's
        // account. A sign-in ends on a fresh, signed-in page with
        // `after_sign_in` run, so that is the start: loading home again
        // would run it a second time, and a toggle run twice closes the
        // menu it opened.
        let Some(key) = account else {
            return (409, "the discovery has no account to sign in again with - start it again".to_string());
        };
        match crate::autorun::signin::sign_in_leased(d, root, organization, project, &key, p.lease, p.signed_in, timing)
            .await
        {
            Err(why) => return (409, why),
            Ok(out) if !out.ok => return (409, out.detail),
            Ok(_) => {}
        }
        went = crate::browser::actions::ActionOutcome::passed("signed in again");
    }
    let reached = if went.ok {
        let start = match crate::browser::page::eval_value(d, "location.href").await {
            Ok(v) => nav::path_of(v.as_str().unwrap_or("")),
            Err(_) => String::new(),
        };
        let path = nav::ModulePath {
            // An update keeps the area's name as it was saved.
            area: existing.as_ref().map_or_else(|| name.clone(), |old| old.name().to_string()),
            module,
            clicks,
            arrived,
            recorded: crate::commands::autorun_record::now_iso(),
            start,
            made_by: if mapping { nav::MadeBy::Mapping } else { nav::MadeBy::Person },
        };
        let route = nav::Route::new(&recipe, path);
        nav::go_to_module(d, &route, nav::TripFrom::SignIn, timing).await.map(|at| (at, route.path))
    } else {
        Err(nav::PathFailure { at: nav::Where::Home, reason: went.detail, harness: went.harness })
    };
    let (at, path) = match reached {
        Ok(found) => found,
        Err(failure) => {
            let reason = failure.for_dialog();
            let (status, page) = read_page(d, crate::browser::snapshot::DEFAULT_LIMIT, None).await;
            let showed = if status == 200 {
                page.lines().take(AREA_PAGE_LINES).collect::<Vec<_>>().join("\n")
            } else {
                "nothing - the page could not be read".to_string()
            };
            if let Some(run) = p.discovery.as_mut().and_then(|s| s.mapping.as_mut()) {
                run.record_unreached(name, &reason);
                run.locate_last(screen);
            }
            return (409, format!("The clicks did not arrive: {reason}. The page showed: {showed}"));
        }
    };
    // The run's own area, found where it was: nothing to write.
    if let Some(old) = existing.as_ref().filter(|old| old.clicks == path.clicks && old.arrived == path.arrived) {
        let kept = old.name().to_string();
        if let Some(state) = p.discovery.as_mut() {
            if let Some(run) = state.mapping.as_mut() {
                run.record_unchanged(kept.clone());
                run.locate_last(screen);
            }
        }
        stand_in_saved_area(d, p.discovery, p.signed_in.as_deref(), root, organization, project, kept).await;
        crate::applog::info("Auto Run mapping run found an area unchanged");
        return (200, serde_json::json!({ "saved": false, "unchanged": true, "arrived": at }).to_string());
    }
    let saved = path.clicks.len();
    let new_menu = nav::menu_path(&path.clicks);
    if let Err(why) = nav::put_path(root, organization, project, path) {
        return (409, why);
    }
    if let Some(state) = p.discovery.as_mut() {
        if let Some(run) = state.mapping.as_mut() {
            match &existing {
                Some(old) => run.record_updated(old.name().to_string(), nav::menu_path(&old.clicks), new_menu),
                None => run.record_added(name.clone()),
            }
            run.locate_last(screen);
        }
    }
    let kept = match &existing {
        Some(old) => old.name().to_string(),
        None => name,
    };
    stand_in_saved_area(d, p.discovery, p.signed_in.as_deref(), root, organization, project, kept).await;
    crate::applog::info(format!("Auto Run discovery saved an area ({saved} clicks)"));
    (200, serde_json::json!({ "saved": true, "arrived": at }).to_string())
}

/// `/autorun-discover-start`: open the Auto Run browser for the assistant
/// and sign in as `account` (`discover_start_in`). Refused, before
/// anything opens, while anything else holds the browser, and for an
/// account or recipe that is not there.
async fn autorun_discover_start(ctx: &BridgeContext, body: &str) -> (u16, String) {
    const SHAPE: &str = "{ \"account\": <an account key>, \"area\": <an area name, optional>, \"browser\": \"edge\" | \"chrome\", optional, \"mapping\": true | false, optional, \"modules\": [<a module name>], for a mapping run }";
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return (400, format!("that is not readable JSON: {e}. Expected {SHAPE}.")),
    };
    let Some(account) = named(v.get("account").and_then(|a| a.as_str())) else {
        return (
            400,
            format!("this call needs an \"account\": the key of an account saved under Auto Run, Accounts. Expected {SHAPE}."),
        );
    };
    let area = named(v.get("area").and_then(|a| a.as_str()));
    let browser = match v.get("browser") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(b)) if matches!(b.trim().to_ascii_lowercase().as_str(), "edge" | "chrome") => {
            Some(b.trim().to_ascii_lowercase())
        }
        Some(_) => return (400, "browser is \"edge\" or \"chrome\"".to_string()),
    };
    let mapping = match v.get("mapping") {
        None | Some(serde_json::Value::Null) => false,
        Some(serde_json::Value::Bool(b)) => *b,
        Some(_) => return (400, format!("mapping is true or false. Expected {SHAPE}.")),
    };
    let modules: Vec<String> = match v.get("modules") {
        None | Some(serde_json::Value::Null) => Vec::new(),
        Some(serde_json::Value::Array(items)) => {
            match items.iter().map(|m| m.as_str().map(str::to_string)).collect::<Option<Vec<_>>>() {
                Some(names) => names,
                None => return (400, format!("modules is a list of module names. Expected {SHAPE}.")),
            }
        }
        Some(_) => return (400, format!("modules is a list of module names. Expected {SHAPE}.")),
    };
    let mapping = if mapping {
        let run = crate::commands::autorun::MappingRun::new(&modules);
        if run.modules.is_empty() {
            return (400, MAPPING_NEEDS_A_MODULE.to_string());
        }
        Some(run)
    } else {
        None
    };
    if let Err(why) = crate::commands::autorun::refuse_discovery_while_busy() {
        return (409, why);
    }
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    // No account, no recipe: said before a browser opens for nothing.
    if let Err(why) = crate::autorun::signin::prepare(&root, &ctx.org, &ctx.project, &account) {
        return (409, why);
    }
    let browser = browser.unwrap_or_else(|| crate::autorun::store::last_browser(&root));
    if let Err(why) = crate::commands::autorun::open_for_discovery(&browser, mapping).await {
        return (409, why);
    }
    let mut slot = crate::commands::autorun::supervised().lock().await;
    let answer = discover_start_in(
        &mut slot,
        &root,
        &ctx.org,
        &ctx.project,
        &account,
        area.as_deref(),
        &crate::browser::timing::Timing::supervised(),
    )
    .await;
    // A sign-in that failed closed the browser, and the discovery with it.
    crate::commands::autorun::publish_discovery(&slot);
    // A mapping run's guard pauses every request: the browser is answered
    // between the assistant's calls too.
    if slot.as_ref().is_some_and(|s| s.cdp.is_guarding_saves()) {
        crate::commands::autorun::answer_between_commands();
    }
    answer
}

/// `/autorun-discover-action`: one action in the discovery's browser
/// (`discover_action_in`). Refused as a tried action is (`sign_in`, a local
/// file), and a `navigate` or `open_tab` to a site outside the sign-in
/// recipe's, before the browser is touched. A `use_component` may carry a
/// `draft` component, tried in place of the saved one.
async fn autorun_discover_action(ctx: &BridgeContext, body: &str) -> (u16, String) {
    const SHAPE: &str = "{ \"action\": <one script action>, \"draft\": <a component, optional, for a use_component>, \"area\": <an area name, optional> }";
    let raw = match body_field(body, "action", SHAPE) {
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
    if let Err(refused) = refuse_as_a_tried_action(&action) {
        return refused;
    }
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
    let area = named(v.get("area").and_then(|a| a.as_str()));
    // A component not saved yet, tried by the `use_component` that names it.
    let draft: Option<crate::autorun::components::Component> = match v.get("draft") {
        None | Some(serde_json::Value::Null) => None,
        Some(raw) => match serde_json::from_value(raw.clone()) {
            Ok(c) => Some(c),
            Err(e) => return (400, format!("that draft is not a component: {e}. Expected {SHAPE}.")),
        },
    };
    if draft.is_some() && !matches!(action, crate::browser::actions::Action::UseComponent { .. }) {
        return (400, format!("a draft is tried by a use_component action that names it. Expected {SHAPE}."));
    }
    if let Some(busy) = unattended_run_is_using_the_browser() {
        return busy;
    }
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let recipe = match crate::autorun::recipe::load_effective_recipe(&root, &ctx.org, &ctx.project) {
        Ok(r) => r,
        Err(why) => return (409, why),
    };
    let policy = crate::autorun::runner::policy_for(Some(&recipe));
    if let Err(refused) = refuse_outside_the_recipe(&policy, &action) {
        return refused;
    }
    let mut slot = crate::commands::autorun::supervised().lock().await;
    let answer = discover_action_in(&mut slot, &root, &ctx.org, &ctx.project, &action, draft.as_ref(), area.as_deref()).await;
    let answer = let_go_if_silent(&mut slot, answer).await;
    crate::commands::autorun::publish_discovery(&slot);
    answer
}

/// `/autorun-discover-actions`: several actions in the discovery's browser,
/// in order, under one hold of it (`discover_actions_in`). Every action is
/// checked as `/autorun-discover-action` checks one, all before the
/// browser is touched: a batch with one refused action runs none. A
/// `draft` is tried by each `use_component` that names it; `area` moves the
/// discovery before the first action.
async fn autorun_discover_actions(ctx: &BridgeContext, body: &str) -> (u16, String) {
    const SHAPE: &str = concat!("{ \"actions\": [<script actions, at most ", max_batch!(), ">], \"stop_on_failure\": <true or false, optional, true by default>, \"draft\": <a component, optional, for a use_component>, \"area\": <an area name, optional> }");
    let raw = match body_field(body, "actions", SHAPE) {
        Ok(v) => v,
        Err(refused) => return refused,
    };
    let Some(raw) = raw.as_array() else {
        return (400, format!("\"actions\" is a list of actions. Expected {SHAPE}."));
    };
    if let Err(refused) = refuse_batch_size(raw.len()) {
        return refused;
    }
    let mut actions = Vec::with_capacity(raw.len());
    for (i, one) in raw.iter().enumerate() {
        match serde_json::from_value::<crate::browser::actions::Action>(one.clone()) {
            Ok(a) => actions.push(a),
            Err(e) => {
                return (
                    400,
                    format!("action {} is not an action: {e} - call get_autorun_guide for the vocabulary.", i + 1),
                )
            }
        }
    }
    for (i, action) in actions.iter().enumerate() {
        if let Err((status, why)) = refuse_as_a_tried_action(action) {
            return (status, format!("action {}: {why}", i + 1));
        }
    }
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
    let stop_on_failure = match v.get("stop_on_failure") {
        None | Some(serde_json::Value::Null) => true,
        Some(serde_json::Value::Bool(b)) => *b,
        Some(_) => return (400, format!("\"stop_on_failure\" is true or false. Expected {SHAPE}.")),
    };
    let area = named(v.get("area").and_then(|a| a.as_str()));
    let draft: Option<crate::autorun::components::Component> = match v.get("draft") {
        None | Some(serde_json::Value::Null) => None,
        Some(raw) => match serde_json::from_value(raw.clone()) {
            Ok(c) => Some(c),
            Err(e) => return (400, format!("that draft is not a component: {e}. Expected {SHAPE}.")),
        },
    };
    if let Some(c) = &draft {
        if !actions.iter().any(|a| draft_for(a, Some(c)).is_some()) {
            return (400, format!("a draft is tried by a use_component action that names it. Expected {SHAPE}."));
        }
    }
    if let Some(busy) = unattended_run_is_using_the_browser() {
        return busy;
    }
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let recipe = match crate::autorun::recipe::load_effective_recipe(&root, &ctx.org, &ctx.project) {
        Ok(r) => r,
        Err(why) => return (409, why),
    };
    let policy = crate::autorun::runner::policy_for(Some(&recipe));
    for (i, action) in actions.iter().enumerate() {
        if let Err((status, why)) = refuse_outside_the_recipe(&policy, action) {
            return (status, format!("action {}: {why}", i + 1));
        }
    }
    // Every component expanded as its action would expand it, so one that
    // is not saved, misses an input or leads outside the recipe refuses
    // the batch before any earlier action drives the browser.
    for (i, action) in actions.iter().enumerate() {
        if let crate::browser::actions::Action::UseComponent { component, inputs } = action {
            if let Err((status, why)) =
                component_to_try(&root, &ctx.org, &ctx.project, component, inputs, draft_for(action, draft.as_ref()))
            {
                return (status, format!("action {}: {why}", i + 1));
            }
        }
    }
    // A stop asked for before this batch is not this batch's.
    BATCH_STOP.store(false, std::sync::atomic::Ordering::SeqCst);
    // Held for the whole batch: nothing else drives the browser between
    // its actions. End discovery, Close browser or a release asks it to
    // give way (`BATCH_STOP`). A gone browser is let go inside, keeping
    // the lines of the actions that ran.
    let mut slot = crate::commands::autorun::supervised().lock().await;
    let answer = discover_actions_in(
        &mut slot,
        &root,
        &ctx.org,
        &ctx.project,
        &actions,
        draft.as_ref(),
        area.as_deref(),
        stop_on_failure,
        &BATCH_STOP,
    )
    .await;
    crate::commands::autorun::publish_discovery(&slot);
    answer
}

/// `/autorun-component-save`: a component, under every save rule
/// (`components::save_tried`), against the open discovery: its area and
/// the components it tried that worked. With no discovery going, nothing
/// was tried, and the save is refused. The test cases of the saved scripts
/// that use it are read first (a read), so each of this project's is
/// checked against the new version; when they cannot be read, a save that
/// has users is refused.
async fn autorun_component_save(
    ctx: &BridgeContext,
    client: Option<&crate::ado::AdoClient>,
    body: &str,
) -> (u16, String) {
    use crate::autorun::components::{users_of, Component, UserCases};
    const SHAPE: &str = "{ \"name\": <its name>, \"description\": <what it does>, \"inputs\": [{ \"name\", \"kind\": \"text\" or \"target\", \"description\" }], \"actions\": [<script actions>], \"why\": <why it changes, for a saved one> }";
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return (400, format!("that is not readable JSON: {e}. Expected {SHAPE}.")),
    };
    let draft: Component = match serde_json::from_value(v.clone()) {
        Ok(c) => c,
        Err(e) => return (400, format!("that is not a component: {e}. Expected {SHAPE}.")),
    };
    let why = match v.get("why") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(_) => return (400, "\"why\" is one sentence".to_string()),
    };
    let dry_run = match v.get("dry_run") {
        None | Some(serde_json::Value::Null) => false,
        Some(serde_json::Value::Bool(b)) => *b,
        Some(_) => return (400, "\"dry_run\" is true or false.".to_string()),
    };
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    // The scripts that use it, by case: this project's with their text,
    // and the rest. `save_tried` reads the users again under its lock.
    let users = users_of(&root, &draft.name).cases;
    let cases: Option<UserCases> = if users.is_empty() {
        Some(UserCases::default())
    } else {
        match client {
            None => None,
            Some(client) => match client.get_case_texts_with_projects(&ctx.org, &users).await {
                Ok(found) => {
                    let mut cases = UserCases::default();
                    for c in found {
                        if c.project.trim().eq_ignore_ascii_case(ctx.project.trim()) {
                            cases.here.insert(c.id, c.text);
                        }
                    }
                    cases.elsewhere = users.iter().copied().filter(|id| !cases.here.contains_key(id)).collect();
                    Some(cases)
                }
                Err(_) => {
                    // The error itself can carry the request's address; the
                    // line names only what could not be read.
                    crate::applog::warn(format!(
                        "Auto Run component {}: the test cases of the scripts that use it could not be read",
                        draft.name.trim()
                    ));
                    None
                }
            },
        }
    };
    let now = crate::autorun::sessions::now_ms();
    if unattended_run_is_using_the_browser().is_some() {
        // No discovery can be going while an unattended run has the
        // browser, so the save is refused for that, as it always was.
        let mut none: Option<crate::commands::autorun::Session> = None;
        if dry_run {
            return dry_run_component_in(&mut none, &root, &ctx.org, &ctx.project, draft, why.as_deref(), now, cases.as_ref());
        }
        return save_component_in(&mut none, &root, &ctx.org, &ctx.project, draft, why.as_deref(), now, cases.as_ref())
            .await;
    }
    let mut slot = crate::commands::autorun::supervised().lock().await;
    if dry_run {
        return dry_run_component_in(&mut slot, &root, &ctx.org, &ctx.project, draft, why.as_deref(), now, cases.as_ref());
    }
    save_component_in(&mut slot, &root, &ctx.org, &ctx.project, draft, why.as_deref(), now, cases.as_ref()).await
}

/// A component save's dry run (`"dry_run": true`) against the discovery
/// going in `slot`: every rule a save makes (`components::check_tried`),
/// answered as the save would answer it, or `would_save` with the version
/// it would become. Nothing is written, nothing is probed or recorded, and
/// it is not a try of the component.
#[allow(clippy::too_many_arguments)]
pub fn dry_run_component_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    draft: crate::autorun::components::Component,
    why: Option<&str>,
    now: u64,
    cases: Option<&crate::autorun::components::UserCases>,
) -> (u16, String) {
    use crate::autorun::components::{check_tried, TriedIn};
    let held = match slot.as_mut() {
        Some(b) => b.parts().discovery.as_ref().map(|d| (d.area.clone(), d.tried.clone())),
        None => None,
    };
    let session = held.as_ref().map(|(area, tried)| TriedIn { area: area.as_deref(), tried });
    match check_tried(root, organization, project, draft, why, session, now, cases) {
        Ok(would) => (
            200,
            serde_json::json!({
                "would_save": would.saved,
                "version": would.version,
                "changes": would.changes,
                "cap_reached": would.cap_reached,
            })
            .to_string(),
        ),
        Err(why) => component_answer(Err(why)),
    }
}

/// A component save (`components::save_tried`) against the discovery going
/// in `slot`: its area and the components it tried that worked. Refused
/// only because some of its own locators were never seen, while that
/// discovery is open, the refused locators are checked on the discovery's
/// current page first (`record_refused_in`), and the save is tried once
/// more; the answer then says what was recorded.
#[allow(clippy::too_many_arguments)]
pub async fn save_component_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    draft: crate::autorun::components::Component,
    why: Option<&str>,
    now: u64,
    cases: Option<&crate::autorun::components::UserCases>,
) -> (u16, String) {
    use crate::autorun::components::{save_tried, TriedIn};
    let held = |slot: &mut Option<B>| match slot.as_mut() {
        Some(b) => b.parts().discovery.as_ref().map(|d| (d.area.clone(), d.tried.clone())),
        None => None,
    };
    let save = |held: &Option<(Option<String>, Vec<String>)>, draft: crate::autorun::components::Component| {
        let session = held.as_ref().map(|(area, tried)| TriedIn { area: area.as_deref(), tried });
        save_tried(root, organization, project, draft, why, session, now, cases)
    };
    let first = held(slot);
    let saved = save(&first, draft.clone());
    let Err(refused) = &saved else {
        return component_answer(saved);
    };
    // Refused by the check of its own locators, for nothing but locators
    // never seen: what that check alone says, recomputed here.
    let Some((area, _)) = first.as_ref() else {
        return component_answer(saved);
    };
    let area = area.as_deref().map(str::trim).filter(|a| !a.is_empty());
    let targets = crate::autorun::seen_check::load_checked_map(root, organization, project).ok().and_then(|map| {
        let own = crate::autorun::seen_check::check_component_seen(&map, area, &draft.actions);
        if own.as_ref().err() != Some(refused) {
            return None;
        }
        crate::autorun::seen_check::unseen_component_targets(&map, area, &draft.actions)
    });
    let Some(targets) = targets.filter(|t| !t.is_empty()) else {
        return component_answer(saved);
    };
    let Some(recorded) = record_refused_in(slot, root, organization, project, &targets).await else {
        return component_answer(saved);
    };
    if recorded.is_empty() {
        return after_recording(&recorded, component_answer(saved));
    }
    let again = save(&held(slot), draft.clone());
    after_recording(&recorded, component_answer(again))
}

/// A component save's answer: the saved name, version and changes as
/// JSON, or the refusal with its status.
fn component_answer(saved: Result<crate::autorun::components::Saved, String>) -> (u16, String) {
    use crate::autorun::components::{SIGN_IN_TO_CHECK_USERS, TRY_IT_FIRST};
    match saved {
        Ok(saved) => {
            crate::applog::info(format!("Auto Run component {} saved as version {}", saved.saved, saved.version));
            (200, serde_json::to_string(&saved).unwrap_or_default())
        }
        Err(why) if why == TRY_IT_FIRST => (409, why),
        Err(why) if why == SIGN_IN_TO_CHECK_USERS => (503, why),
        Err(why) => (400, why),
    }
}

/// `/autorun-component-retire`: a component no saved script uses
/// (`components::remove_unused`). One in use stays, and the refusal names
/// the cases that use it.
fn autorun_component_retire(ctx: &BridgeContext, body: &str) -> (u16, String) {
    let name = match body_field(body, "name", "{ \"name\": <the component's name> }") {
        Ok(serde_json::Value::String(s)) if !s.trim().is_empty() => s,
        Ok(_) => return (400, "\"name\" is a saved component's name".to_string()),
        Err(refused) => return refused,
    };
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    match crate::autorun::components::remove_unused(&root, &ctx.org, &ctx.project, &name) {
        Ok(removed) => {
            crate::applog::info(format!("Auto Run component {removed} removed"));
            (200, serde_json::json!({ "removed": removed }).to_string())
        }
        Err(why) if why == crate::autorun::components::not_saved(&name) => (404, why),
        Err(why) => (409, why),
    }
}

/// A discovery's `navigate` or `open_tab` to a site outside `policy`'s,
/// refused. A relative address names no site: the runner makes it absolute
/// on the page and holds it to the same list.
fn refuse_outside_the_recipe(
    policy: &crate::browser::actions::Policy,
    action: &crate::browser::actions::Action,
) -> Result<(), (u16, String)> {
    for url in tried_addresses(action) {
        if let Some(origin) = crate::autorun::recipe::origin_of(url) {
            if !policy.allows(url) {
                return Err((
                    400,
                    format!(
                        "{origin} is not one of {} - a discovery goes only where the sign-in recipe does",
                        crate::browser::actions::ALLOWED_ORIGINS
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// `/autorun-discover-area`: save the clicks a discovery found as an area
/// (`discover_area_in`), once a replay of them arrives.
async fn autorun_discover_area(ctx: &BridgeContext, body: &str) -> (u16, String) {
    const SHAPE: &str = "{ \"name\": <the area's name>, \"module\": <the test-case Module it belongs to>, \"clicks\": [<a script's click selector>, ...] }";
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return (400, format!("that is not readable JSON: {e}. Expected {SHAPE}.")),
    };
    let Some(name) = named(v.get("name").and_then(|a| a.as_str())) else {
        return (400, format!("this call needs a \"name\" for the area. Expected {SHAPE}."));
    };
    let Some(module) = named(v.get("module").and_then(|a| a.as_str())) else {
        return (400, format!("this call needs the \"module\" the area belongs to. Expected {SHAPE}."));
    };
    let clicks: Vec<crate::browser::locator::Target> = match v.get("clicks").cloned().map(serde_json::from_value) {
        Some(Ok(clicks)) => clicks,
        Some(Err(e)) => return (400, format!("those are not clicks: {e}. Expected {SHAPE}.")),
        None => return (400, format!("this call needs the \"clicks\" from the home page. Expected {SHAPE}.")),
    };
    if clicks.is_empty() {
        return (400, format!("an area needs at least one click from the home page. Expected {SHAPE}."));
    }
    for (i, click) in clicks.iter().enumerate() {
        if let Err(e) = click.validate() {
            return (400, format!("click {}: {e}", i + 1));
        }
    }
    if let Some(busy) = unattended_run_is_using_the_browser() {
        return busy;
    }
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let mut slot = crate::commands::autorun::supervised().lock().await;
    discover_area_in(
        &mut slot,
        &root,
        &ctx.org,
        &ctx.project,
        &name,
        &module,
        clicks,
        &crate::browser::timing::Timing::supervised(),
    )
    .await
}

/// A future the replay host hands back: boxed, so the host can be a trait
/// object the app installs once.
pub type HostFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// What the app lends the bridge for an assistant's replay to a step: the
/// bridge has no `AppHandle` of its own (see `INTAKE_SINK`). A test hands
/// `autorun_replay_with` a fake one.
pub trait ReplayHost: Send + Sync {
    /// Tell the app's window: the Allow prompt for a must-not-save script,
    /// and its end.
    fn notify(&self, notice: crate::autorun::replay_ask::Notice<'_>);
    /// Whether a discovery holds the Auto Run browser now, so the Allow
    /// prompt can say the replay ends it first.
    fn discovering(&self) -> bool;
    /// Run the replay in the supervised browser, by the same path as the
    /// person's Replay to step button, as the assistant's: it never lifts a
    /// guard held for another case, and a discovery holding the browser is
    /// ended first rather than refusing (`end_discovery_for_replay`). `Err`
    /// is a browser that would not open.
    fn replay(
        &self,
        organization: String,
        project: String,
        req: crate::autorun::replay_to::ReplayRequest,
    ) -> HostFuture<'_, Result<AssistantReplay, String>>;
    /// The page the replay left the browser on, as `/autorun-page` answers
    /// for this organization and project.
    fn page(&self, organization: String, project: String) -> HostFuture<'_, (u16, String)>;
}

/// How an assistant's replay ended, and the discovery it ended before it
/// took the browser, if any.
#[derive(Debug, Clone, PartialEq)]
pub struct AssistantReplay {
    pub end: crate::autorun::replay_to::ReplayEnd,
    pub discovery: DiscoveryEnded,
}

static REPLAY_HOST: std::sync::OnceLock<Box<dyn ReplayHost>> = std::sync::OnceLock::new();

/// Called once by the app when the bridge starts. Later calls are ignored.
pub fn set_replay_host(host: Box<dyn ReplayHost>) {
    let _ = REPLAY_HOST.set(host);
}

/// Said when a replay is asked for and the app has lent the bridge no host.
const NO_REPLAY_HOST: &str = "the app could not start replays this session - restart the app";

/// Replay a case to a step in the supervised browser, for the assistant.
async fn autorun_replay(ctx: &BridgeContext, body: &str) -> (u16, String) {
    let Some(host) = REPLAY_HOST.get() else {
        return (503, NO_REPLAY_HOST.to_string());
    };
    let asks = crate::autorun::replay_ask::asks();
    autorun_replay_with(ctx, body, host.as_ref(), asks, crate::autorun::replay_ask::WAIT).await
}

/// `/autorun-replay`, body `{ case_id, step }`, with its host, its request
/// registry and how long the person has to answer.
///
/// Refused, with the sentence, before anything opens: a body without the
/// two numbers, an unattended run going, a case with no saved script, a
/// step outside it, a replay already running. A script marked must not
/// save then asks the person and waits (`replay_ask`): Deny, no answer or
/// another request waiting refuse it, and only Allow goes on. Database
/// Read Access is the `db_query` switch, as for the person's own replay.
///
/// A discovery holding the browser does not refuse it: the replay ends it
/// first (`end_discovery_for_replay`), the Allow prompt says it will, and
/// the answer's sentence starts with `ENDED_DISCOVERY_FIRST`.
///
/// The answer is `{ "sentence" }`, and once the browser stands before the
/// step, `"page"` beside it: the page as `get_autorun_page` shows it, so the
/// assistant can try the step at once.
pub async fn autorun_replay_with(
    ctx: &BridgeContext,
    body: &str,
    host: &dyn ReplayHost,
    asks: &crate::autorun::replay_ask::Asks,
    wait: std::time::Duration,
) -> (u16, String) {
    use crate::autorun::replay_to::{self, ReplayEnd, ReplayRequest};
    const SHAPE: &str = "{ \"case_id\": <number>, \"step\": <number> }";
    let number = |key: &str| -> Result<i32, (u16, String)> {
        body_field(body, key, SHAPE)?
            .as_i64()
            .and_then(|n| i32::try_from(n).ok())
            .ok_or((400, format!("{key} must be a number")))
    };
    let (case_id, step) = match (number("case_id"), number("step")) {
        (Ok(c), Ok(s)) => (c, s),
        (Err(refused), _) | (_, Err(refused)) => return refused,
    };
    if let Some(busy) = unattended_run_is_using_the_browser() {
        return busy;
    }
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let db_read_access = !ctx.disabled_tools.iter().any(|t| t == "db_query");
    let req = ReplayRequest { case_id, step, db_read_access };
    let checked = match replay_to::check(&root, &ctx.org, &ctx.project, &req) {
        Ok(c) => c,
        Err(why) => return (409, why),
    };
    if replay_to::is_running() {
        return (409, replay_to::ALREADY_RUNNING.to_string());
    }
    if checked.script.no_save {
        let notify = |n: crate::autorun::replay_ask::Notice<'_>| host.notify(n);
        let ends_discovery = host.discovering();
        if let Err(why) = asks.ask(case_id, &checked.script.title, step, ends_discovery, wait, &notify).await {
            crate::applog::info(format!("Auto Run replay of case {case_id} for the assistant: {why}"));
            return (409, why);
        }
    }
    let (end, discovery) = match host.replay(ctx.org.clone(), ctx.project.clone(), req).await {
        Ok(AssistantReplay { end, discovery }) => (end, discovery),
        Err(why) => return (503, why),
    };
    // Said first when the replay ended the assistant's own discovery.
    let sentence = after_ended_discovery(discovery.ended, &end.sentence());
    // An ended mapping run's summary goes beside the sentence, as
    // `end_autorun_discovery` hands it back.
    let with_summary = |mut answer: serde_json::Value| {
        if let Some(summary) = &discovery.summary {
            answer["summary"] = serde_json::json!(summary);
        }
        answer.to_string()
    };
    match end {
        ReplayEnd::Refused(_) | ReplayEnd::Blocked(_) => (409, sentence),
        ReplayEnd::Ready { .. } => {
            let answer = match host.page(ctx.org.clone(), ctx.project.clone()).await {
                (200, page) => serde_json::json!({ "sentence": sentence, "page": page }),
                (_, why) => serde_json::json!({ "sentence": sentence, "page_unavailable": why }),
            };
            (200, with_summary(answer))
        }
        ReplayEnd::StoppedAt { .. } | ReplayEnd::Stopped { .. } | ReplayEnd::BrowserGone { .. } => {
            (200, with_summary(serde_json::json!({ "sentence": sentence })))
        }
    }
}

/// Said to a try that does not name its case.
pub const TRY_NEEDS_CASE: &str = "name the case this try is for (case_id), so its no-save guard applies";

/// The `case_id` a try body must carry.
pub fn try_case_id(body: &str) -> Result<i32, (u16, String)> {
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
    match v.get("case_id") {
        None | Some(serde_json::Value::Null) => Err((400, TRY_NEEDS_CASE.to_string())),
        Some(id) => id
            .as_i64()
            .and_then(|n| i32::try_from(n).ok())
            .ok_or((400, "case_id must be a number".to_string())),
    }
}

/// One tried action in a browser already guarded for its case
/// (`commands::autorun::guard_for_case`), as the route answers it.
/// `case_id` is the case the try is for: a tried `return_to_area` goes to
/// the area that case's saved script names. A try that worked files every
/// locator it acted on in the discovery map, under that case's area.
#[allow(clippy::too_many_arguments)]
pub async fn try_in<D: crate::browser::cdp::Driver>(
    d: &mut D,
    account: &mut Option<String>,
    lease: &mut crate::autorun::lease::Held,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    case_id: i32,
    action: &crate::browser::actions::Action,
) -> (u16, String) {
    try_in_area(d, account, lease, root, organization, project, case_id, None, action).await
}

/// `try_in`, its sightings filed under `discovery_area` when one is given
/// (`recording_area`).
#[allow(clippy::too_many_arguments)]
async fn try_in_area<D: crate::browser::cdp::Driver>(
    d: &mut D,
    account: &mut Option<String>,
    lease: &mut crate::autorun::lease::Held,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    case_id: i32,
    discovery_area: Option<&str>,
    action: &crate::browser::actions::Action,
) -> (u16, String) {
    let tried = try_action(d, account, lease, root, organization, project, case_id, discovery_area, false, action).await;
    let outcome = match tried {
        Ok(tried) => tried.outcome,
        Err(refused) => return refused,
    };
    let mut text = format!("{}: {}", if outcome.ok { "ok" } else { "failed" }, outcome.detail);
    if let Some(shot) = &outcome.screenshot {
        text.push_str(&format!(" (picture: {})", crate::autorun::store::shot_path(root, shot).display()));
    }
    (200, text)
}

/// What one action an assistant ran did: its outcome, and the dialog the
/// page raised while it ran, if any.
struct Tried {
    outcome: crate::browser::actions::ActionOutcome,
    dialog: Option<crate::autorun::StepDialog>,
}

/// One action, carried out by the runner's own step loop as a step of one
/// numbered 0 (`try_in_area` says why), with what it acted on filed when it
/// worked. `saves_only_counted` is a mapping run's: a stopped save never
/// fails it (`runner::InRun::saves_only_counted`).
#[allow(clippy::too_many_arguments)]
async fn try_action<D: crate::browser::cdp::Driver>(
    d: &mut D,
    account: &mut Option<String>,
    lease: &mut crate::autorun::lease::Held,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    case_id: i32,
    discovery_area: Option<&str>,
    saves_only_counted: bool,
    action: &crate::browser::actions::Action,
) -> Result<Tried, (u16, String)> {
    use crate::autorun::runner::{area_route, area_routes, named_areas, AreaRoute, InRun, NEEDS_SCRIPT_AREA};
    // A bare `return_to_area` goes to the case's own area; one that names
    // an area goes to that one.
    let resolved = (matches!(action, crate::browser::actions::Action::ReturnToArea { .. }) && action.area_named().is_none())
        .then(|| area_route(root, organization, project, case_id));
    let area = match &resolved {
        Some(Ok(r)) => AreaRoute::To(r),
        Some(Err(why)) => AreaRoute::Unknown(why),
        None => AreaRoute::Unknown(NEEDS_SCRIPT_AREA),
    };
    let areas = area_routes(root, organization, project, &named_areas(std::iter::once(action)));
    // A step of one, numbered 0 - it belongs to no case, and nothing
    // records it. The runner's step loop still carries it out, so a tried
    // action behaves exactly as it will inside a script - the runner's own
    // kinds included: a tried `expect_response` takes its mark as the try
    // starts, and checks a request the page makes while it waits - and a
    // save the guard stops fails it with the run's own sentence.
    //
    // `{{username}}` and `{{password}}` are refused where a script is
    // SAVED, not here: a tried `fill` is not on its way into a file, and
    // it types the literal text it was given rather than standing in for
    // anything a recipe would have substituted.
    // The page the try starts on: a click that navigates was matched on
    // this page, not the one it leads to.
    let matched = matched_targets(action);
    let started_on = if matched.is_empty() { String::new() } else { page_address(d).await.0 };
    let step = crate::autorun::StepScript { step_number: 0, actions: vec![action.clone()], unchecked: None };
    let mut run = InRun { areas: Some(&areas), saves_only_counted, ..Default::default() };
    let outcomes = match crate::autorun::runner::run_step_in_run(
        d,
        root,
        organization,
        project,
        &step,
        &crate::browser::timing::Timing::supervised(),
        account,
        lease,
        None,
        area,
        &mut run,
    )
    .await
    {
        Ok(v) => v,
        Err(why) => return Err((500, why)),
    };
    let Some(outcome) = outcomes.into_iter().next() else {
        return Err((500, "the action produced no outcome".to_string()));
    };
    // Shown to a person reading Settings -> Logs, never returned to the
    // assistant - and never a `fill`'s VALUE, which `describe_try` never
    // even looks at.
    crate::applog::info(describe_try(action, outcome.ok));
    // Only a try that worked: a locator that failed was never seen working.
    if outcome.ok {
        let at = Sighting {
            root: root.to_path_buf(),
            org: organization.to_string(),
            project: project.to_string(),
            area: recording_area(root, discovery_area, Some(case_id)),
            account: None,
            discovering: None,
            // Only the application's own pages count (`may_record`).
            policy: recording_policy(root, organization, project),
        };
        record_matched_targets(&at, &started_on, &matched);
    }
    Ok(Tried { outcome, dialog: run.dialog })
}

/// A run's failed cases, as text an assistant can act on - and, when it
/// must not touch the script at all, the reason why.
fn autorun_failures(ctx: &BridgeContext, target: &str) -> (u16, String) {
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
    // The project's components, so an action one ran is shown as it
    // expands. A file that does not read leaves those actions named by
    // their component only.
    let components = crate::autorun::components::load_components(&root, &ctx.org, &ctx.project).unwrap_or_default();
    (200, crate::autorun::failures::describe_failures_in(Some(&root), &run, &scripts, &components))
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

/// Mark a case as a suspected application defect: the script is right, the
/// application did not do what the case expects. The newest run of the
/// case decides whether the step really failed there (and not for a
/// `STOP:` reason) - `defects::check_mark`. Answers with the stored mark.
/// Never touches the script's actions, its repairs or its last repair.
fn autorun_defect(body: &str) -> (u16, String) {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct MarkBody {
        case_id: i32,
        step_number: i32,
        note: String,
    }
    let shape = "{ \"case_id\": 501, \"step_number\": 3, \"note\": \"what the application did, against what the case expects\" }";
    let m: MarkBody = match serde_json::from_str(body) {
        Ok(m) => m,
        Err(e) => return (400, format!("that is not a mark: {e}. Expected {shape}.")),
    };
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let script = match crate::autorun::store::load_script(&root, m.case_id) {
        Ok(s) => s,
        Err(e) => return (500, e),
    };
    let run = crate::autorun::failures::latest_run(&root, Some(m.case_id));
    let mark = match crate::autorun::defects::check_mark(
        run.as_ref(),
        script.as_ref(),
        m.case_id,
        m.step_number,
        &m.note,
        crate::autorun::sessions::now_ms(),
    ) {
        Ok(mark) => mark,
        Err(why) => return (400, why),
    };
    if let Err(e) = crate::autorun::store::set_suspected_defect(&root, m.case_id, Some(mark.clone())) {
        return (500, format!("could not save the mark: {e}"));
    }
    crate::applog::info(format!(
        "AI marked case {} step {} as a suspected application defect",
        m.case_id, mark.step_number
    ));
    (200, serde_json::to_string(&mark).unwrap_or_default())
}

/// `POST /autorun-order`, body `{ pbi_id, case_ids }`: Auto Run's own
/// execution order for the PBI on this machine (`store::save_order`). Run
/// Tests' order is not touched. Every id must be one of the PBI's test
/// cases, read from Azure DevOps the way the Auto Run tab lists them; a
/// listed case with no saved script is saved in the order and named back.
async fn autorun_order(ctx: &BridgeContext, client: Option<&crate::ado::AdoClient>, body: &str) -> (u16, String) {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct OrderBody {
        pbi_id: i32,
        case_ids: Vec<i32>,
    }
    let shape = "{ \"pbi_id\": 100, \"case_ids\": [503, 501, 502] }";
    let o: OrderBody = match serde_json::from_str(body) {
        Ok(o) => o,
        Err(e) => return (400, format!("that is not an order: {e}. Expected {shape}.")),
    };
    if o.case_ids.is_empty() {
        return (400, "case_ids is empty - list the PBI's cases in the order Auto Run should run them".to_string());
    }
    for (i, id) in o.case_ids.iter().enumerate() {
        if o.case_ids[..i].contains(id) {
            return (400, format!("case {id} is listed twice"));
        }
    }
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let Some(client) = client else {
        return (503, "sign in to Test Case Manager first".to_string());
    };
    let on_pbi: Vec<i32> = match client.get_pbi_test_cases(&ctx.org, o.pbi_id).await {
        Ok(cases) => cases.into_iter().map(|c| c.id).collect(),
        Err(crate::ado::AdoError::NotFound) => {
            return (404, format!("Azure DevOps has no work item #{} - check the PBI id", o.pbi_id))
        }
        Err(e) => return (502, format!("Azure DevOps error: {}", e.user_text())),
    };
    let not_on_pbi: Vec<String> = o
        .case_ids
        .iter()
        .filter(|id| !on_pbi.contains(id))
        .map(|id| format!("case {id} is not in PBI {}", o.pbi_id))
        .collect();
    if !not_on_pbi.is_empty() {
        return (400, not_on_pbi.join("; "));
    }
    if let Err(e) = crate::autorun::store::save_order(&root, o.pbi_id, &o.case_ids) {
        return (500, format!("could not save the order: {e}"));
    }
    crate::applog::info(format!("AI set Auto Run's order for PBI {}: {} case(s)", o.pbi_id, o.case_ids.len()));
    let mut lines = vec![format!(
        "saved Auto Run's order for PBI {}: {}",
        o.pbi_id,
        o.case_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(", ")
    )];
    for id in &o.case_ids {
        if !matches!(crate::autorun::store::load_script(&root, *id), Ok(Some(_))) {
            lines.push(format!("case {id} has no saved script"));
        }
    }
    (200, lines.join("\n"))
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

/// Said for a proposal whose accounts do not have the expected shape -
/// in place of the parser's own words, which can quote a password.
pub const PROPOSAL_SHAPE_REFUSED: &str = "each account needs a key, a label and a username as text, and a password as text when one is given - and no other field";

/// `POST /accounts-propose`: the assistant's proposed logins for the active
/// environment, REPLACING whatever it proposed before, passwords included.
/// Each may carry the password the assistant read in the same database
/// lookup - accepted only for an environment marked as a test environment,
/// and otherwise the whole call is refused. A password is never logged or
/// repeated in an answer, and nothing reaches the accounts until a person
/// adds it in the app.
fn accounts_propose(body: &str) -> (u16, String) {
    use crate::environments::{active, save_proposals, StoredProposal, PROPOSED_PASSWORD_NOT_TEST};
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Proposal {
        key: String,
        #[serde(default)]
        label: String,
        username: String,
        #[serde(default)]
        role: Option<String>,
        #[serde(default)]
        password: Option<String>,
    }
    let shape = "{ \"accounts\": [{ \"key\": \"hr.supervisor\", \"label\": \"HR supervisor\", \"username\": \"sup1\", \"role\"?: \"Supervisor\", \"password\"?: \"(test environments only)\" }] }";
    let raw = match body_field(body, "accounts", shape) {
        Ok(v) => v,
        Err(refused) => return refused,
    };
    let raw = json_arg(Some(&raw)).unwrap_or(raw);
    // No parser text in the answer: serde's would quote a value it could
    // not read, and the value may be a password.
    let list: Vec<Proposal> = match serde_json::from_value(raw) {
        Ok(l) => l,
        Err(_) => return (400, format!("{PROPOSAL_SHAPE_REFUSED}. Expected {shape}.")),
    };
    if list.is_empty() {
        return (400, format!("\"accounts\" needs at least one account. Expected {shape}."));
    }
    let proposals: Vec<StoredProposal> = list
        .into_iter()
        .map(|p| {
            let key = p.key.trim().to_string();
            let label = match p.label.trim() {
                "" => key.clone(),
                l => l.to_string(),
            };
            let role = p.role.map(|r| r.trim().to_string()).filter(|r| !r.is_empty());
            // Kept exactly as read: a password's spaces are its own.
            StoredProposal { key, label, username: p.username.trim().to_string(), role, password: p.password }
        })
        .collect();
    let with_password = proposals.iter().filter(|p| p.password.is_some()).count();
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let env = match active(&root) {
        Ok(e) => e,
        Err(e) => return (500, e),
    };
    if with_password > 0 && !env.test_environment {
        return (400, PROPOSED_PASSWORD_NOT_TEST.to_string());
    }
    if let Err(why) = save_proposals(&root, &env.id, &proposals) {
        return (400, why);
    }
    crate::applog::info(format!(
        "Environments: the assistant proposed {} account(s) for {}, {with_password} with a password",
        proposals.len(),
        env.name
    ));
    (
        200,
        format!(
            "proposed {} account(s) for the environment {}, {with_password} with a password - a person picks which to add in the app, and a proposed password is used unless they type another. Another call replaces this proposal.",
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
    // Resolved now, not when the context was pushed: a login saved since
    // is the one this call signs in with. An id this build does not know -
    // one saved for a preset a later release removed - is nothing chosen
    // too: the person's next step is the same, pick a database.
    crate::db::query::ready(ctx.db_secrets.as_deref(), ctx.db_id.as_deref()).map_err(|why| (409, why))
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
const SAVE_BODY_KEYS: [&str; 3] = ["scripts", "edits", "dry_run"];

struct SaveRequest {
    scripts: Vec<crate::autorun::CaseScript>,
    edits: Vec<crate::autorun::edits::Edit>,
    /// Run every check and say what would happen, writing nothing.
    dry_run: bool,
}

fn bad_scripts(e: serde_json::Error) -> String {
    format!(
        "that is not a list of action scripts: {e}. Expected an array of {{ case_id, title, steps: [{{ step_number, actions }}] }} - call get_autorun_guide for the format."
    )
}

fn bad_edits(e: serde_json::Error) -> String {
    format!(
        "that is not a list of declared edits: {e}. Each is {{ case_id, steps: [number], why, area (optional: true when the area changed), quirk (optional) }}."
    )
}

/// Either shape: the bare array a new bundle has always been, or the
/// object that carries the declarations a repair needs alongside it.
///
/// A declaration may also travel INSIDE its script, as `"edits"` on the
/// script object (one entry or a list, its `case_id` taken from the
/// script when left out). `CaseScript` ignores keys it does not know, so
/// before this was read here a nested declaration was dropped without a
/// word and the repair was refused as undeclared - the shape behind the
/// owner's "save_autorun_script drops the edits list". `"edits": null` is
/// no declaration, the same as leaving it out.
fn parse_save_request(body: &str) -> Result<SaveRequest, String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(bad_scripts)?;
    let mut dry_run = false;
    let (mut scripts_value, edits_value) = match v {
        serde_json::Value::Array(_) => (v, None),
        serde_json::Value::Object(mut map) => {
            let unknown: Vec<String> = map
                .keys()
                .filter(|k| !SAVE_BODY_KEYS.contains(&k.as_str()))
                .map(|k| format!("\"{k}\""))
                .collect();
            if !unknown.is_empty() {
                return Err(format!(
                    "this body carries {} save_autorun_script does not read: {}. It reads \"scripts\", \"edits\" and \"dry_run\".",
                    if unknown.len() == 1 { "a key" } else { "keys" },
                    unknown.join(", ")
                ));
            }
            dry_run = match map.remove("dry_run") {
                None | Some(serde_json::Value::Null) => false,
                Some(serde_json::Value::Bool(b)) => b,
                Some(_) => return Err("\"dry_run\" is true or false.".to_string()),
            };
            let scripts = map.remove("scripts").ok_or_else(|| {
                "this body has no \"scripts\". Send { \"scripts\": [...], \"edits\": [...] }.".to_string()
            })?;
            (scripts, map.remove("edits").filter(|e| !e.is_null()))
        }
        _ => {
            return Err(
                "that is not a list of action scripts. Send the array, or { \"scripts\": [...], \"edits\": [...] }."
                    .to_string(),
            )
        }
    };
    let mut edit_values: Vec<serde_json::Value> = match edits_value {
        None => vec![],
        Some(serde_json::Value::Array(list)) => list,
        // One entry on its own is a one-entry list, the same as one
        // nested in a script.
        Some(one @ serde_json::Value::Object(_)) => vec![one],
        // Anything else: refused in serde's own words for what it is.
        Some(other) => {
            return Err(match serde_json::from_value::<Vec<crate::autorun::edits::Edit>>(other) {
                Err(e) => bad_edits(e),
                Ok(_) => "that is not a list of declared edits.".to_string(),
            })
        }
    };
    if let serde_json::Value::Array(items) = &mut scripts_value {
        for script in items.iter_mut().filter_map(|s| s.as_object_mut()) {
            let Some(nested) = script.remove("edits") else { continue };
            let case_id = script.get("case_id").cloned();
            let list = match nested {
                serde_json::Value::Null => vec![],
                serde_json::Value::Array(list) => list,
                one => vec![one],
            };
            for mut edit in list {
                if let (Some(fields), Some(id)) = (edit.as_object_mut(), case_id.as_ref()) {
                    fields.entry("case_id").or_insert_with(|| id.clone());
                }
                edit_values.push(edit);
            }
        }
    }
    let scripts = serde_json::from_value(scripts_value).map_err(bad_scripts)?;
    // The same declaration sent twice (nested and at the top) is one; two
    // DIFFERENT declarations for one case are refused, since the gate
    // could only ever read one of them.
    let mut edits: Vec<crate::autorun::edits::Edit> = Vec::with_capacity(edit_values.len());
    for value in edit_values {
        let edit: crate::autorun::edits::Edit = serde_json::from_value(value).map_err(bad_edits)?;
        if edits.contains(&edit) {
            continue;
        }
        if edits.iter().any(|e| e.case_id == edit.case_id) {
            return Err(format!(
                "case {} is declared twice in \"edits\", with different contents - send one entry per case",
                edit.case_id
            ));
        }
        edits.push(edit);
    }
    Ok(SaveRequest { scripts, edits, dry_run })
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
/// plus the fields outside the steps a save can carry, `title`,
/// `account`, `no_save`, `preconditions`, `setup` and `area` (a blank area is no
/// area; case does not tell two area names apart). `no_save` counts: a re-send that leaves it
/// out of a script marked Must not save is a repair turning it off, which
/// the gate refuses - never a quiet "unchanged". Positional rather than keyed by step number, so a bundle
/// that merely REORDERS the same steps counts as a change and goes
/// through the gate rather than around it. `repairs` is deliberately
/// not compared: it is never the sender's to set. Nor are the marks
/// (`changes`, `needs_unchanged`): they affect order, not safety, so
/// marking a saved script is no repair and uses none of its count. The
/// sent marks are what is saved.
fn unchanged_script(old: &crate::autorun::CaseScript, sent: &crate::autorun::CaseScript) -> bool {
    old.title == sent.title
        && old.account == sent.account
        && old.no_save == sent.no_save
        && old.preconditions == sent.preconditions
        && old.setup == sent.setup
        && old.area_name().map(crate::autorun::nav::module_key) == sent.area_name().map(crate::autorun::nav::module_key)
        && old.steps.len() == sent.steps.len()
        && old.steps.iter().zip(&sent.steps).all(|(a, b)| {
            a.step_number == b.step_number
                && crate::autorun::edits::step_signature(a)
                    == crate::autorun::edits::step_signature(b)
        })
}
/// The steps of `old`'s test case that the case itself changed or dropped
/// since the script was saved: the case read as of the script's `saved_at`
/// (or, for a script saved before that existed, its file's modified time)
/// against the case now. Empty when there is no signed-in client or either
/// read fails - the gate then judges the repair exactly as before.
async fn steps_the_case_changed(
    client: Option<&crate::ado::AdoClient>,
    organization: &str,
    root: &std::path::Path,
    old: &crate::autorun::CaseScript,
) -> std::collections::BTreeSet<i32> {
    let none = std::collections::BTreeSet::new;
    let Some(client) = client else { return none() };
    let Some(as_of) = old
        .saved_at
        .clone()
        .or_else(|| crate::autorun::store::script_modified(root, old.case_id))
    else {
        return none();
    };
    let before = match client.get_test_case_steps_as_of(organization, old.case_id, &as_of).await {
        Ok(steps) => steps,
        Err(e) => {
            crate::applog::info(format!(
                "Auto Run: case {} could not be read as of {as_of} to see what the case changed: {e:?}",
                old.case_id
            ));
            return none();
        }
    };
    let now = match client.get_test_cases_by_ids(organization, &[old.case_id], None, None).await {
        Ok(cases) => match cases.into_iter().find(|c| c.id == old.case_id) {
            Some(c) => c.steps,
            None => return none(),
        },
        Err(_) => return none(),
    };
    crate::autorun::edits::steps_changed_by_case(&before, &now)
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
    let SaveRequest { scripts, edits, dry_run } = match parse_save_request(body) {
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
    // Cases whose repair changes the step their suspected-defect mark is
    // on: the assistant has decided that step was the script's fault after
    // all, so the mark goes once the save has landed.
    let mut repaired_marks: Vec<(i32, i32)> = Vec::new();
    // Which steps of each script the seen check reads, by case: `None`
    // for a new script (all of them), its declared steps for a repair. An
    // unchanged resave is not listed: it was checked when it was saved.
    let mut seen_scope: Vec<(i32, Option<Vec<i32>>)> = Vec::new();
    // Whether scripts saved before they were stamped count here, read once.
    let legacy = crate::autorun::seen_check::legacy_scripts_count(&root, &ctx.org, &ctx.project);
    for sent in &scripts {
        let existing = match crate::autorun::store::load_script(&root, sent.case_id) {
            Ok(v) => v,
            Err(e) => return (500, e),
        };
        let declared = edits.iter().find(|e| e.case_id == sent.case_id);
        let mut script = sent.clone();
        // A new script is checked whole; an unchanged one and a repair keep
        // steps checked by an earlier save, so they vouch as it did.
        let vouched = existing
            .as_ref()
            .is_none_or(|old| crate::autorun::seen_check::vouch_carries_over(old, &ctx.org, &ctx.project, legacy));
        script.organization = Some(ctx.org.trim().to_string());
        script.project = Some(ctx.project.trim().to_string());
        script.checked = vouched;
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
                let marks_changed = old.changes != sent.changes || old.needs_unchanged != sent.needs_unchanged;
                let said = if marks_changed { "marks updated" } else { "unchanged" };
                lines.push(format!("case {} ({said})", script.case_id));
            }
            Some(old) => {
                let mut verdict = crate::autorun::edits::check_edits(&old, sent, declared);
                // Refused for losing checks: the test case itself may have
                // changed or dropped those steps since the script was saved,
                // and a repair that follows it is not weakening it. Only
                // then is the case's history read - an ordinary repair costs
                // no extra request.
                if verdict.as_ref().err().is_some_and(|w| w.contains(crate::autorun::edits::NEVER_WEAKENED)) {
                    let changed_by_case = steps_the_case_changed(client, &ctx.org, &root, &old).await;
                    if !changed_by_case.is_empty() {
                        verdict = crate::autorun::edits::check_edits_following_case(&old, sent, declared, &changed_by_case);
                    }
                }
                if let Err(why) = verdict {
                    // A body with no declarations at all, refused for
                    // something a declaration would cover (the sentences
                    // that point at "edits"): say first that the list
                    // itself is missing, which is the actual fix. Only
                    // then: when the bundle declares OTHER cases, the list
                    // is there and this case's entry is what is missing,
                    // which `check_edits`' own "case N: ... not declared"
                    // sentence already says accurately.
                    if edits.is_empty() && why.contains("\"edits\"") {
                        return (
                            400,
                            format!(
                                "case {} already has a script, so this save is a repair, and \"edits\" is missing. {why}",
                                sent.case_id
                            ),
                        );
                    }
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
                seen_scope.push((sent.case_id, crate::autorun::seen_check::steps_to_check(declared)));
                if let Some(e) = declared {
                    let why = e.why.trim();
                    if !dry_run {
                        crate::applog::info(format!(
                            "AI repaired case {} steps {:?}: {why}",
                            script.case_id, e.steps
                        ));
                    }
                    script.last_repair = Some(why.to_string());
                    if let Some(d) = old.suspected_defect.as_ref().filter(|d| e.steps.contains(&d.step_number)) {
                        repaired_marks.push((script.case_id, d.step_number));
                    }
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
                seen_scope.push((sent.case_id, None));
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

    // Gate 3: every locator a new or changed step names was seen on the
    // live app (`seen_check`), so no script is saved against a guess.
    // The components are read only when a checked script uses one, so a
    // damaged file never holds up a save that does not need it.
    let uses_components = seen_scope.iter().any(|(case_id, _)| {
        prepared
            .iter()
            .any(|s| s.case_id == *case_id && crate::autorun::seen_check::uses_components(s))
    });
    // The saved scripts, read once for every check of this save.
    let saved_scripts =
        if seen_scope.is_empty() { Vec::new() } else { crate::autorun::store::list_scripts(&root) };
    let check_and_save = || -> Result<(), SaveRefusal> {
        if !seen_scope.is_empty() {
            let (map, components, files) = seen_check_inputs(&root, ctx, uses_components, &saved_scripts)
                .map_err(|e| SaveRefusal::Other(400, e))?;
            // Every script is checked, and every locator never seen in any
            // of them is named at once; a refusal of any other kind is
            // answered alone, as it always was.
            let mut unseen: Vec<(i32, Vec<String>)> = Vec::new();
            for (case_id, only) in &seen_scope {
                let (Some(script), Some(case)) = (
                    prepared.iter().find(|s| s.case_id == *case_id),
                    cases.iter().find(|c| c.id == *case_id),
                ) else {
                    return Err(SaveRefusal::Other(
                        400,
                        format!("case {case_id} could not be checked against the live app, so it was not saved"),
                    ));
                };
                use crate::autorun::seen_check::SeenVerdict;
                match crate::autorun::seen_check::seen_verdict(
                    &map,
                    &components,
                    script,
                    &case_step_text(case),
                    only.as_deref(),
                    &files,
                ) {
                    SeenVerdict::Passed => {}
                    SeenVerdict::Other(why) => return Err(SaveRefusal::Unseen(why)),
                    SeenVerdict::Unseen(lines) => unseen.push((*case_id, lines)),
                }
            }
            if !unseen.is_empty() {
                return Err(SaveRefusal::Unseen(unseen_in_bundle(&unseen)));
            }
        }
        // Everything has passed. A dry run checks the bundle as the write
        // would and stops there; a save writes it.
        if dry_run {
            return crate::autorun::store::check_scripts(&prepared).map_err(|e| SaveRefusal::Other(400, e));
        }
        match crate::autorun::store::save_scripts_atomically(&root, &prepared) {
            Ok(()) => Ok(()),
            Err(crate::autorun::store::SaveScriptsError::Invalid(e)) => Err(SaveRefusal::Other(400, e)),
            Err(crate::autorun::store::SaveScriptsError::Io(e)) => {
                Err(SaveRefusal::Other(500, format!("could not save the bundle: {e}")))
            }
        }
    };
    // With a component in use, the components load, the check and the
    // write hold the components lock, so a component save (which re-checks
    // the scripts that use it) cannot slip between them. Nothing in here
    // awaits or saves a component. A dry run writes nothing, so it does
    // not hold the lock.
    let check_and_save_locked = || {
        if uses_components && !dry_run {
            crate::autorun::components::with_components_locked(check_and_save)
        } else {
            check_and_save()
        }
    };
    let mut checked = check_and_save_locked();
    // Refused only for locators never seen, while a discovery is open: the
    // refused ones are checked on the discovery's current page, the ones
    // there are recorded, and the save is checked once more.
    // Never on a dry run: it records nothing.
    let mut recorded: Option<Vec<String>> = None;
    if !dry_run && matches!(checked, Err(SaveRefusal::Unseen(_))) {
        let targets = seen_check_inputs(&root, ctx, uses_components, &saved_scripts).ok().and_then(|(map, components, files)| {
            let mut all: Vec<crate::browser::locator::Target> = Vec::new();
            for (case_id, only) in &seen_scope {
                let script = prepared.iter().find(|s| s.case_id == *case_id)?;
                let case = cases.iter().find(|c| c.id == *case_id)?;
                all.extend(crate::autorun::seen_check::unseen_targets(
                    &map,
                    &components,
                    script,
                    &case_step_text(case),
                    only.as_deref(),
                    &files,
                )?);
            }
            Some(all)
        });
        // Each checked script's own area: a sighting is recorded under the
        // discovery's area, so it can only count when that is theirs.
        let areas: Vec<Option<&str>> = seen_scope
            .iter()
            .map(|(case_id, _)| prepared.iter().find(|s| s.case_id == *case_id).and_then(|s| s.area_name()))
            .collect();
        if let Some(targets) = targets.filter(|t| !t.is_empty()) {
            if let Some(found) = record_refused_on_page(ctx, &root, &areas, &targets).await {
                if !found.is_empty() {
                    checked = check_and_save_locked();
                }
                recorded = Some(found);
            }
        }
    }
    let answer = |said: (u16, String)| match &recorded {
        Some(found) => after_recording(found, said),
        None => said,
    };
    if let Err(refused) = checked {
        return answer(refused.said());
    }
    if dry_run {
        return (200, format!("would save {} script(s): {}", prepared.len(), lines.join(", ")));
    }
    crate::applog::info(format!("AI saved {} auto-run script(s)", prepared.len()));

    let mut report = vec![format!("saved {} script(s): {}", prepared.len(), lines.join(", "))];
    for (case_id, step) in &repaired_marks {
        match crate::autorun::store::clear_suspected_defect_at(&root, *case_id, *step) {
            Ok(true) => {
                crate::applog::info(format!(
                    "AI repair of case {case_id} step {step} cleared its suspected defect"
                ));
                report.push(format!(
                    "case {case_id}: suspected defect at step {step} cleared - the step was repaired"
                ));
            }
            // The mark moved or went since the repair was read: not ours
            // to clear, and nothing to report.
            Ok(false) => {}
            Err(e) => report.push(format!(
                "case {case_id}: the suspected defect at step {step} could not be cleared: {e}"
            )),
        }
    }
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
    answer((200, report.join("\n")))
}

/// The refusal of a bundle whose scripts name locators never seen, from
/// each refused case's lines in bundle order: the lines alone when one
/// case is refused, each after "case <id>: " when more are; listed as
/// `seen_check::refusal_list` lists them.
fn unseen_in_bundle(unseen: &[(i32, Vec<String>)]) -> String {
    let lines: Vec<String> = match unseen {
        [(_, lines)] => lines.clone(),
        many => many
            .iter()
            .flat_map(|(case_id, lines)| lines.iter().map(move |l| format!("case {case_id}: {l}")))
            .collect(),
    };
    crate::autorun::seen_check::refusal_list(&lines)
}

/// Why a script save's last gate refused it: a locator never seen on the
/// live app (`Unseen`, the check's own sentence), or anything else.
enum SaveRefusal {
    Unseen(String),
    Other(u16, String),
}

impl SaveRefusal {
    fn said(self) -> (u16, String) {
        match self {
            SaveRefusal::Unseen(why) => (400, why),
            SaveRefusal::Other(status, why) => (status, why),
        }
    }
}

/// A test case's step actions and expected results, as the seen check
/// reads them.
fn case_step_text(case: &crate::ado::TestCaseFull) -> Vec<String> {
    case.steps.iter().flat_map(|s| [s.action.clone(), s.expected.clone()]).collect()
}

/// What a script save's seen check reads: the discovery map, the project's
/// components (only when a checked script uses one, so a damaged file
/// never holds up a save that does not need it) and its Test files, so the
/// size the app shows for one a script uploads passes as the script's own
/// data.
fn seen_check_inputs(
    root: &std::path::Path,
    ctx: &BridgeContext,
    uses_components: bool,
    saved_scripts: &[crate::autorun::CaseScript],
) -> Result<
    (crate::autorun::discovery_map::DiscoveryMap, crate::autorun::components::ComponentFile, Vec<crate::test_files::TestFile>),
    String,
> {
    let map = crate::autorun::seen_check::load_checked_map_with(root, &ctx.org, &ctx.project, saved_scripts)?;
    let components = if uses_components {
        crate::autorun::components::load_components(root, &ctx.org, &ctx.project)?
    } else {
        crate::autorun::components::ComponentFile::default()
    };
    let files = crate::test_files::list(&crate::test_files::folder(root, &ctx.org, &ctx.project)).unwrap_or_else(|e| {
        crate::applog::warn(format!("Auto Run save: the Test files could not be listed: {e}"));
        Vec::new()
    });
    Ok((map, components, files))
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
        // A `specs` entry the rule refuses is taken out on the way back, so
        // a tool's write never keeps one. The importer has already warned
        // about each by name.
        Some(old) => crate::import_parser::merge_cases_into_draft(old, cases)
            .and_then(|t| crate::import_parser::specs::without_refused_specs(&t)),
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

/// The writing guide's standard granularity section, used while the
/// person's own writing style is off.
pub const GRANULARITY: &str = "\
        ## Granularity - quality over quantity\n\
        Similar checks belong in ONE case, not several. Checking a\n\
        notification's title and checking its body is one case with two\n\
        steps (or one step with both in the expected result) - not two\n\
        cases. Split only when the checks need different setup or data, or\n\
        can fail independently in a way the tester must record separately.\n\
        A padded case count is not coverage; every extra case is another\n\
        row someone has to execute and maintain.\n\n\
";

/// The writing guide's standard edge-case section, used while the
/// person's own writing style is off.
pub const EDGE_CASES: &str = "\
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

/// The person's writing style (`crate::writing_style`) as the guide
/// carries it: their text under its own heading, then the line that keeps
/// the format and import rules above it.
fn custom_style_section(text: &str) -> String {
    format!(
        "## Team test design style (set on this machine)\n{}\n\n\
        If anything here conflicts with the Format section or the import rules, \
        the Format section and the import rules win.\n\n",
        text.trim()
    )
}

/// The workflow line the guide carries while the person's writing style is
/// on: where the style's own steps, if it has any, fit in.
const CUSTOM_STYLE_STEP: &str = "\
        1.5. If the team test design style above adds steps (for example a scenario list to approve \
        before drafting, or a summary or review at the end), follow them at the point it says.\n\
";

/// Live writing guide: format rules from the importer's own constants +
/// the org's Module values, fetched fresh (no snapshot staleness). The
/// person's writing style is read from disk on every call, so a save takes
/// effect the next time an assistant reads the guide.
async fn guide(ctx: &BridgeContext, client: &crate::ado::AdoClient) -> String {
    let custom = crate::writing_style::current_text().map(|t| custom_style_section(&t));
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
        Only `.md` files and Azure DevOps wiki links may be listed in\n\
        `specs`. Any other file - code such as `.cshtml` or `.cs`, text\n\
        files, PDFs - may be read and cited in `reviewer_notes`, but never\n\
        added to `specs`: it is refused if added, and the importer warns\n\
        about each one it drops.\n\n\
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
        `transform_cases` instead of rewriting the file yourself.\n",
        org = ctx.org,
        project = ctx.project,
        granularity = custom.as_deref().unwrap_or(GRANULARITY),
        edge_cases = if custom.is_some() { "" } else { EDGE_CASES },
        scenario_step = if custom.is_some() { CUSTOM_STYLE_STEP } else { "" },
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
