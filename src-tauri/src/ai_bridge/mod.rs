//! The AI bridge: a 127.0.0.1-only service that lets the tcm-mcp stdio
//! binary (and therefore AI tools) use the app: read a live writing guide,
//! fetch real example cases, reorganise a draft into a run sheet, search
//! PBIs, and work Auto Run, API templates, the database tools and the
//! report tools. Modeled on note_server.rs; the TCP loop is thin
//! (`server`), all logic lives in `route` so tests need no sockets.
//!
//! What it reads: Azure DevOps (cases, suites, tags, PBIs, wiki pages, run
//! results) - read only, no writes to Azure DevOps and no raw passthrough
//! to it - plus the app's own saved state (accounts without passwords,
//! scripts, quirks, templates, Test files).
//!
//! What it writes, and what guards each write:
//! - Case files: a draft the app follows, or one under the working
//!   repository's `.test-cases` folder, and only by canonical path
//!   (`bridge_may_write`, `workspace::is_inside`).
//! - The app's own Auto Run and Test file stores (scripts, components,
//!   areas, quirks, validation rules, defects, run order, setups, kept
//!   actions, proposed accounts (with their passwords, on test
//!   environments), Test files, API templates, fixtures and flows): each
//!   route validates what it is handed, and a route that belongs to a module
//!   is answered only while that module is on (`module_guard`).
//! - The application under test, two ways. Through its UI: discovery tries
//!   and batches and replays drive the Auto Run browser and can submit
//!   forms, held back only where a case or discovery says must-not-save.
//!   Through HTTP: proving or running an API template, or an explore
//!   session's own sends, only with the API templates switch on
//!   (`API_WRITES_OFF`). The Auto Run browser's captured requests, test
//!   headers and cookies (the network tools) are answered only with their
//!   switch on (`NETWORK_TOOLS_OFF`).
//! - The database: `db_query` writes only with the create, update and
//!   delete switch on AND a connection whose own user may write.
//! - The open report file, through the report tools.
//!
//! Every tool the person switched off is refused, any other method or path
//! that looks like a write gets `WRITE_REFUSAL`, and the bearer token never
//! leaves the app.

use crate::autorun::agents::AgentKey;
use crate::autorun::slots::{SlotNo, SLOT_COUNT};
use rand::RngExt;
use serde::Serialize;
use std::sync::{Arc, Mutex as StdMutex};

mod account_routes;
mod autorun_common;
mod autorun_network;
mod autorun_page;
mod autorun_review;
mod autorun_save;
mod autorun_scripts;
mod autorun_try;
mod case_reads;
mod db_tools;
mod discovery;
mod discovery_areas;
mod discovery_browser;
mod discovery_end;
mod explore;
mod guide_routes;
mod replay;
mod report_tools;
mod run_results;
mod server;
mod template_routes;
mod test_file_routes;
mod writing_routes;

pub use self::account_routes::*;
pub use self::autorun_common::*;
pub use self::autorun_network::*;
pub use self::autorun_page::*;
pub use self::autorun_review::*;
pub use self::autorun_save::*;
pub use self::autorun_scripts::*;
pub use self::autorun_try::*;
pub use self::case_reads::*;
pub use self::db_tools::*;
pub use self::discovery::*;
pub use self::discovery_areas::*;
pub use self::discovery_browser::*;
pub use self::discovery_end::*;
pub use self::explore::*;
pub use self::guide_routes::*;
pub use self::replay::*;
pub use self::report_tools::*;
pub use self::run_results::*;
pub use self::server::*;
pub use self::template_routes::*;
pub use self::test_file_routes::*;
use self::writing_routes::*;

static INTAKE_SINK: std::sync::OnceLock<IntakeSink> = std::sync::OnceLock::new();

/// How many tag names the writing guide inlines before it stops and
/// points at the dedicated tool. Long enough to be genuinely useful,
/// short enough not to drown the guide.
const GUIDE_TAG_LIMIT: usize = 60;

/// The PBI selected in Test Case Manager's top bar, as the assistants see it
/// (`get_current_pbi`). Pushed with the bridge context; only ever read here.
#[derive(Debug, Clone, Default, PartialEq, Serialize, serde::Deserialize, specta::Type)]
pub struct CurrentPbi {
    pub id: i32,
    pub title: String,
    pub project: String,
}

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
    /// Whether the person has switched the network tools on (the captured
    /// requests, HAR files, test headers and cookies of the Auto Run
    /// browser): until they do, those tools are not listed and their routes
    /// refuse (`NETWORK_TOOLS_OFF`). Each environment keeps its own; off by
    /// default.
    pub network_tools: bool,
    /// The webview's "Treat all projects as one" switch. Read only by the
    /// Azure DevOps routes (from Phase 6); `project` stays the home project
    /// either way.
    pub all_projects: bool,
    /// The active environment's "Run database changes without asking".
    /// Never the webview's to say: Rust sets it from the environment, on a
    /// push and on a switch (`commands::ai_bridge::push_context_with`,
    /// `commands::environments::move_bridge_to`).
    pub db_no_ask: bool,
    /// Whose context this is and whether its writes are held off - Rust's
    /// alone, kept across every push (`commands::ai_bridge::push_context_with`).
    pub gate: SwitchGate,
    /// The PBI selected in the top bar, or None when none is. The webview's
    /// to say; the assistants only read it (`get_current_pbi`).
    pub current_pbi: Option<CurrentPbi>,
}

/// The part of the context only Rust sets: which environment the context's
/// database, tool list and write switches belong to, and whether writes are
/// held off while the allow files are brought in line with it.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct SwitchGate {
    /// The environment the context is for: set by a switch
    /// (`move_bridge_to`) and by a push for the active environment. A
    /// context with a write on always names one.
    pub env_id: Option<String>,
    /// How many switches or repairs are still making each AI tool's own
    /// "always allow" match the environment. While any is, no write is on.
    pub holds: u32,
    /// The write switches a push for this environment asked for while a
    /// hold was on, put in place when the last hold ends.
    pub wanted: Option<(bool, bool)>,
}

/// Said when an API template call was allowed for one environment, or
/// with the switch on, and by the time it held the run slot that was no
/// longer so. Nothing ran.
pub const API_WRITES_MOVED: &str =
    "the environment or the API templates switch changed while this call waited - nothing was run; call it again";

/// Whether a template call allowed on `snapshot` - the context it arrived
/// with - may still run now that it holds the run slot (which a switch also
/// takes, so nothing can move after this). `active` is the active
/// environment now; `live` is the bridge's context now, when there is one.
pub fn still_allowed(snapshot: &BridgeContext, active: Option<&str>, live: Option<&BridgeContext>) -> Result<(), String> {
    let moved = || Err(API_WRITES_MOVED.to_string());
    if let Some(env) = snapshot.gate.env_id.as_deref() {
        if active != Some(env) {
            return moved();
        }
    }
    if let Some(now) = live {
        if !now.api_writes || now.gate.env_id != snapshot.gate.env_id {
            return moved();
        }
    }
    Ok(())
}

/// The bridge the app runs, for `still_allowed`'s look at the context now.
/// Set by the app when it starts its bridge (never by a test's listener),
/// held weakly so it never keeps one alive.
static LIVE: std::sync::Mutex<Option<std::sync::Weak<BridgeState>>> = std::sync::Mutex::new(None);

pub fn set_live_bridge(shared: &SharedBridge) {
    *LIVE.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::downgrade(shared));
}

fn live_context() -> Option<BridgeContext> {
    let live = LIVE.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.upgrade()?;
    let ctx = live.ctx.lock().unwrap_or_else(|e| e.into_inner()).clone();
    Some(ctx)
}

/// `still_allowed` against the active environment and the live context.
fn assistant_may_still_write(snapshot: &BridgeContext, root: &std::path::Path) -> Result<(), String> {
    let active = crate::environments::read_active(root).ok().flatten().map(|e| e.id);
    still_allowed(snapshot, active.as_deref(), live_context().as_ref())
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
            .field("network_tools", &self.network_tools)
            .field("all_projects", &self.all_projects)
            .field("current_pbi", &self.current_pbi.as_ref().map(|p| p.id))
            .field("db_no_ask", &self.db_no_ask)
            .field("gate", &self.gate)
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

/// Where the Crystal Reports routes say the report on the screen may have
/// changed, so it reads the open report again. Handed the open report's
/// path (`None` when none is). A process-wide sink for the same reason as
/// `INTAKE_SINK`; `None` in a test that never installs one.
type CrystalSink = Box<dyn Fn(Option<String>) + Send + Sync>;
static CRYSTAL_SINK: std::sync::OnceLock<CrystalSink> = std::sync::OnceLock::new();

/// Called once by the app when the bridge starts. Later calls are ignored.
pub fn set_crystal_sink(f: CrystalSink) {
    let _ = CRYSTAL_SINK.set(f);
}

/// Tell the screen. The open report's path is read on a blocking thread:
/// the report's lock can be held for a long time by a render or a live
/// fetch, and waiting on it must not stall an async worker.
async fn crystal_changed() {
    if let Some(f) = CRYSTAL_SINK.get() {
        // A thread that panicked has no path to give; the screen reads the report itself.
        let path = tokio::task::spawn_blocking(crate::crystal::current_path).await.unwrap_or(None);
        f(path);
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

/// `GET /autorun-role?role=<key>`: the role's text, or a 400 naming the roles there are.
pub fn autorun_role_answer(target: &str) -> (u16, String) {
    let asked = q(target, "role").unwrap_or_default();
    match crate::autorun::roles::Role::from_key(&asked) {
        Some(role) => (200, role.text()),
        None => (400, crate::autorun::roles::unknown_role(&asked)),
    }
}

/// `GET /autorun-pw-skills[?skill=<name>]`: the Playwright repo's skills, listed or one given, read from the clone set on the Playwright tab.
pub fn autorun_pw_skills_answer(target: &str) -> (u16, String) {
    crate::autorun::pw_rules::skills_answer(&crate::app_settings::current().playwright_clone, q(target, "skill").as_deref())
}

/// `GET /autorun-pw-repo[?file=<name>][&page=<n>]`: one of the Playwright repo's own files (its CLAUDE.md when none is named), in pages when long.
pub fn autorun_pw_repo_answer(target: &str) -> (u16, String) {
    crate::autorun::pw_rules::repo_answer_page(
        &crate::app_settings::current().playwright_clone,
        q(target, "file").as_deref(),
        q(target, "page").as_deref(),
    )
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
    // Boxed: the router's future is large, and a caller that holds it inline
    // (a test on a 2 MB thread) would carry all of it on its own stack.
    Box::pin(route_timed(&crate::autorun::agents::Caller::Default, ctx, client, method, target, body, version)).await
}

/// [`route_as`], leaving the log one line for the call: its route, status
/// and how long it took. What `route` and the listener call.
pub async fn route_timed(
    caller: &crate::autorun::agents::Caller,
    ctx: &BridgeContext,
    client: Option<&crate::ado::AdoClient>,
    method: &str,
    target: &str,
    body: &str,
    version: &str,
) -> (u16, String) {
    let started = std::time::Instant::now();
    let answer = route_as(caller, ctx, client, method, target, body, version).await;
    // One line per call: the route and how long it took, so a slow call can
    // be told from a slow site. The query and the body are never logged.
    crate::applog::debug(call_time_line(method, target, answer.0, started.elapsed().as_millis()));
    crate::assistant_activity::record_call(
        caller,
        &ctx.org,
        ctx.gate.env_id.as_deref().unwrap_or(""),
        method,
        target,
        body,
        answer.0,
    );
    answer
}

/// The log line a bridge call leaves: method, path (the query left out),
/// status and milliseconds. Never an argument.
pub fn call_time_line(method: &str, target: &str, status: u16, ms: u128) -> String {
    let path = target.split_once('?').map(|(p, _)| p).unwrap_or(target);
    format!("Bridge call: {method} {path} answered {status} in {ms} ms")
}

/// What `get_current_pbi` answers: the PBI as JSON, or a plain sentence when
/// the person has none selected in TCM.
fn current_pbi_answer(ctx: &BridgeContext) -> String {
    match &ctx.current_pbi {
        Some(p) => serde_json::json!({ "id": p.id, "title": p.title, "project": p.project }).to_string(),
        None => "No PBI is selected in TCM.".to_string(),
    }
}

/// `route` for one caller: what the listener's `route_timed` calls, with the caller read from
/// the request's `x-tcm-agent*` headers. `route` is this for the default
/// caller (an older proxy, a test, the person).
pub async fn route_as(
    caller: &crate::autorun::agents::Caller,
    ctx: &BridgeContext,
    client: Option<&crate::ado::AdoClient>,
    method: &str,
    target: &str,
    body: &str,
    version: &str,
) -> (u16, String) {
    let path = target.split_once('?').map(|(p, _)| p).unwrap_or(target);
    // Every route that belongs to a module is answered only while that
    // module is on, and the check runs ONCE here, by path, before the
    // router's match (`module_guard_for`).
    if let Some(refused) = module_guard(path, body, &crate::modules::current()) {
        return refused;
    }
    if let Some(refused) = network_guard(path, ctx.network_tools) {
        return refused;
    }
    // A case another assistant works on is refused first, before a browser is
    // picked, so a refused call binds a free main to nobody and reserves no
    // extra browser. Every case a try, a discovery start, a replay or a save
    // bundle names is claimed here, before any gate reads anything.
    let claimed = match (method, path) {
        ("POST", "/autorun-try" | "/autorun-discover-start" | "/autorun-replay") => claim_named_case(caller, body),
        ("POST", "/autorun-script") => claim_bundle_cases(caller, body),
        _ => Ok(None),
    };
    let newly_claimed = match claimed {
        Ok(case) => case,
        Err(refused) => return refused,
    };
    // The browser this call uses, found ONCE: every handler below locks,
    // publishes to and answers for that slot. A route that uses no
    // supervised browser (or one the caller must not be refused for) is
    // main's, as everything was before there were slots. A capture read that
    // names an imported set needs none either, so an assistant with no
    // browser of its own can still read it.
    let need = crate::commands::autorun_slots::need_of(method, path)
        .filter(|_| !crate::commands::autorun_slots::reads_an_imported_set(method, path, body));
    let slot = match need {
        Some(need) => match crate::commands::autorun_slots::slot_for(caller, need) {
            Ok(slot) => slot,
            Err(refused) => {
                release_failed_start(newly_claimed, caller, path, refused.0);
                return refused;
            }
        },
        None => SlotNo::MAIN,
    };
    // Who opens a browser in `slot` if this call does: the real assistant,
    // never whoever the bindings name at that moment. The default caller
    // holds none.
    let holder = caller.key();
    let answer = match (method, path) {
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
        // A proxy saying goodbye as it exits. It belongs to no module, so
        // the guard above lets it through even with Auto Run off. Only an
        // agent's goodbye ends anything; the default caller is answered and
        // nothing more.
        ("POST", "/agent-bye") => {
            if let crate::autorun::agents::Caller::Agent(key) = caller {
                crate::autorun::agents::note_bye(&key.id, crate::autorun::sessions::now_ms());
                forget_writing_jobs_of(&key.id);
            }
            (200, "{}".to_string())
        }
        // The PBI selected in TCM: read only, and in no module, so it
        // answers whichever modules are on.
        ("GET", "/current-pbi") => (200, current_pbi_answer(ctx)),
        ("POST", "/optimize") => optimize_json(body, target, ctx),
        ("POST", "/transform") => transform_json(body, ctx),
        ("POST", "/validate") => (200, validate_json(body, target, ctx, client).await),
        ("POST", "/check-coverage") => check_coverage_route(body).await,
        ("POST", "/merge-cases") => merge_cases_route(body, ctx),
        ("POST", "/begin") => begin_writing(caller, body, target, ctx, client).await,
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
        ("GET", "/tags") => tags(caller, ctx, client, target).await,
        // The Auto Run routes. Only ONE of them reaches Azure DevOps, and
        // only to read: the save route looks the test cases up so the
        // floor has something to check the script against. The rest
        // document a format, drive the browser the person opened, or read
        // files this machine already wrote - so none of them needs a
        // signed-in client. All were let through by the guard above.
        ("GET", "/autorun-guide") => {
            // The guide lists the rules of the area the CALLER's discovery is
            // exploring, if it has one, never another assistant's: a caller
            // with no browser has no area, and the guide lists every area's.
            let asker = crate::commands::autorun_slots::slot_if_any(caller);
            match q(target, "section").filter(|s| !s.trim().is_empty()) {
                // One section of the live guide, after the Quick rules.
                Some(title) => {
                    match crate::autorun::guide::guide_section(&autorun_guide_with_quirks(ctx, false, asker), &title) {
                        Ok(text) => (200, text),
                        Err(titles) => (404, titles),
                    }
                }
                // Without arguments the MCP tool asks for the overview: the
                // Quick rules, the live sections and the section titles.
                None if matches!(q(target, "overview").as_deref(), Some("true") | Some("1")) => {
                    let full = autorun_guide_with_quirks(ctx, false, asker);
                    (200, crate::autorun::guide::guide_overview(&autorun_guide_with_quirks(ctx, true, asker), &full))
                }
                None => {
                    let quick = matches!(q(target, "quick").as_deref(), Some("true") | Some("1"));
                    (200, autorun_guide_with_quirks(ctx, quick, asker))
                }
            }
        }
        // A role's rules, read from the Playwright repo that is set: the proxy cannot, so it asks here.
        ("GET", "/autorun-role") => autorun_role_answer(target),
        // The Playwright repo's own skills and files, read from the clone that is set.
        ("GET", "/autorun-pw-skills") => autorun_pw_skills_answer(target),
        ("GET", "/autorun-pw-repo") => autorun_pw_repo_answer(target),
        // A save that repairs (carries `edits`) ends with the repair rules, a refusal too.
        // Every case in the bundle was claimed at the top (`claim_bundle_cases`):
        // a case another assistant works on is refused before any gate reads
        // anything.
        ("POST", "/autorun-script") => {
            let answer = save_autorun_scripts(ctx, client, body, crate::commands::autorun_slots::slot_if_any(caller)).await;
            let parsed: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
            if crate::autorun::roles::cases_repaired_by(&parsed).is_empty() {
                answer
            } else {
                crate::autorun::roles::with_rules(answer, crate::autorun::roles::RULES_REPAIR_SAVE)
            }
        }
        // One saved script, and a delete that keeps it aside: files on this
        // machine only, never Azure DevOps.
        ("GET", "/autorun-script") => autorun_script_read(target),
        ("POST", "/autorun-script-delete") => autorun_script_delete(body),
        ("GET", "/autorun-page") => autorun_page(ctx, target, slot, &page_owner(caller)).await,
        // Read only: whether an unattended run is paused for the network.
        // Nothing here stops or answers the wait.
        ("GET", "/autorun-network-wait") => (200, autorun_network_wait_answer()),
        // The page's captured requests (`capture_tools`): read, searched,
        // filtered, exported to TCM's own captures folder and imported from
        // it. Read only - nothing here sends a request to the application,
        // and every value shown is redacted.
        ("POST", "/autorun-requests") => autorun_capture(CaptureTool::List, body, slot).await,
        ("POST", "/autorun-requests-search") => autorun_capture(CaptureTool::Search, body, slot).await,
        ("POST", "/autorun-request") => autorun_capture(CaptureTool::Request, body, slot).await,
        ("POST", "/autorun-response") => autorun_capture(CaptureTool::Response, body, slot).await,
        ("POST", "/autorun-response-body") => autorun_capture(CaptureTool::ResponseBody, body, slot).await,
        ("GET", "/autorun-capture") => autorun_capture(CaptureTool::GetFilter, body, slot).await,
        ("POST", "/autorun-capture") => autorun_capture(CaptureTool::SetFilter, body, slot).await,
        ("POST", "/autorun-requests-export") => autorun_capture(CaptureTool::Export, body, slot).await,
        ("POST", "/autorun-requests-import") => autorun_capture(CaptureTool::Import, body, slot).await,
        // The assistant's test headers and cookies (`autorun::overrides`):
        // set only in a browser it opened, never while a replay runs, and
        // named in every try, discovery and replay answer while in place.
        // Raw CDP only - nothing here sends a request to the application.
        ("POST", "/autorun-headers") => autorun_override(OverrideTool::SetHeaders, body, slot).await,
        ("POST", "/autorun-cookies") => autorun_override(OverrideTool::Cookies, body, slot).await,
        ("POST", "/autorun-cookie") => autorun_override(OverrideTool::SetCookie, body, slot).await,
        // A real sign-out: the panes hear it, as they hear the reset's.
        ("POST", "/autorun-cookies-clear") => crate::commands::autorun::clear_cookies_for_assistant(slot).await,
        ("POST", "/autorun-probe") => autorun_probe(ctx, body, slot).await,
        // One element's styles, listeners, animations and structure. Read
        // only: nothing is clicked, typed, focused or scrolled.
        ("POST", "/autorun-inspect") => autorun_inspect(body, slot).await,
        // A try for a case another assistant works on was refused at the top.
        ("POST", "/autorun-try") => {
            let mut seen = None;
            let answer = autorun_try(ctx, body, &mut seen, slot).await;
            with_overrides_seen(answer, seen, slot)
        }
        // Discovery: the assistant opens the Auto Run browser itself, signs
        // in as a saved account (the app types the password) and explores
        // the live application, one action at a time. A case another
        // assistant works on was refused at the top, before a browser was
        // picked or anything opened.
        ("POST", "/autorun-discover-start") => {
            let mut seen = None;
            // A new discovery may be a new reader of this slot: it is told the whole page.
            crate::autorun::page_compact::forget(slot);
            let answer = Box::pin(compacted(slot, &page_owner(caller), page_full(body), Box::pin(async { without_page(autorun_discover_start(ctx, body, &mut seen, slot, holder).await, body) }))).await;
            // A start refused or failed before it opened a browser does not
            // keep the extra browser it reserved.
            crate::commands::autorun_slots::free_extra_if_unused(slot, holder, answer.0);
            let answer = with_overrides_seen(with_no_save_note(answer, discovery_no_save(slot)), seen, slot);
            // A discovery that started drops this assistant's previous kept
            // actions; an explore session begins its own list.
            if answer.0 == 200 {
                kept_started(slot, caller).await;
            }
            // A mapping run records areas and writes no check: no rules.
            let mapping = serde_json::from_str::<serde_json::Value>(body)
                .map(|v| v["mapping"].as_bool() == Some(true) || v["mapping"].as_str().map(|s| s.trim().eq_ignore_ascii_case("true")) == Some(true))
                .unwrap_or(false);
            // An explore session has its own rules, whatever the status.
            let explore = serde_json::from_str::<serde_json::Value>(body).map(|v| said_true(&v["explore"])).unwrap_or(false);
            if explore {
                crate::autorun::roles::with_rules(answer, crate::autorun::roles::RULES_EXPLORE)
            } else if mapping {
                answer
            } else {
                crate::autorun::roles::with_rules(answer, crate::autorun::roles::RULES_DISCOVERY)
            }
        }
        ("POST", "/autorun-discover-action") => {
            let mut seen = None;
            let answer = Box::pin(compacted(slot, &page_owner(caller), page_full(body), Box::pin(async { without_page(autorun_discover_action(ctx, body, &mut seen, slot).await, body) }))).await;
            with_overrides_seen(with_no_save_note(answer, discovery_no_save(slot)), seen, slot)
        }
        ("POST", "/autorun-discover-actions") => {
            let mut seen = None;
            let answer = Box::pin(compacted(slot, &page_owner(caller), page_full(body), Box::pin(async { without_page(autorun_discover_actions(ctx, body, &mut seen, slot).await, body) }))).await;
            with_overrides_seen(with_no_save_note(answer, discovery_no_save(slot)), seen, slot)
        }
        // A replay going has already ended any discovery, and holds the
        // browser: said at once rather than waited for.
        ("POST", "/autorun-discover-end") => match replay_is_holding_the_browser(slot) {
            Some(going) => with_overrides_seen(going, None, slot),
            None => {
                crate::autorun::page_compact::forget(slot);
                let keeping = serde_json::from_str::<serde_json::Value>(body)
                    .ok()
                    .and_then(|v| v.get("keep_browser").and_then(serde_json::Value::as_bool))
                    .unwrap_or(false);
                let by = if keeping { EndedBy::AssistantKeeping } else { EndedBy::Assistant };
                let answer = crate::commands::autorun::end_discovery(by, slot).await;
                // Done with its discovery: the cases it held are free.
                end_claims_of(holder, answer.0);
                answer
            }
        },
        // An explore session's change list, recorded and read back. Files
        // on this machine only: nothing here sends a request to the
        // application.
        ("POST", "/autorun-explore-change") => {
            match (unattended_run_is_using_the_browser(), autorun_root()) {
                (Some(busy), _) => busy,
                (None, Ok(root)) => {
                    let mut held = crate::commands::autorun::session_slot(slot).lock().await;
                    record_change_in(&mut held, &root, &ctx.org, &ctx.project, body)
                }
                (None, Err(refused)) => refused,
            }
        }
        // The actions this assistant's explore session kept, in memory only.
        ("POST", "/autorun-kept-actions") => match autorun_root() {
            Ok(root) => kept_actions_for(&root, &crate::autorun::kept::owner_of(caller)),
            Err(refused) => refused,
        },
        // The project's saved setups, read from the setups file.
        ("POST", "/autorun-setups") => match autorun_root() {
            Ok(root) => setups_list_for(&root, &ctx.org, &ctx.project),
            Err(refused) => refused,
        },
        // Save a range of the kept actions as a named setup. Files and
        // memory on this machine; the database rule is one SELECT.
        ("POST", "/autorun-setup-save") => match autorun_root() {
            Ok(root) => {
                let owner = crate::autorun::kept::owner_of(caller);
                let now = crate::autorun::sessions::now_ms();
                let reading = crate::modules::current().contains(&crate::modules::Module::DbAccess);
                match reading.then(|| db_ready(ctx)).and_then(Result::ok) {
                    Some((conn, exe)) => {
                        let rows = crate::autorun::setup_find::SqlcmdRows { runner: crate::db::RealRunner, exe, conn };
                        setup_save_for(&root, &ctx.org, &ctx.project, &owner, body, Some(&rows), now).await
                    }
                    None => {
                        setup_save_for::<crate::autorun::setup_find::SqlcmdRows<crate::db::RealRunner>>(
                            &root, &ctx.org, &ctx.project, &owner, body, None, now,
                        )
                        .await
                    }
                }
            }
            Err(refused) => refused,
        },
        // Run a saved setup in the going explore session: it finds a test
        // record or builds one in the caller's own browser, and the build
        // goes on the change list. The database rule is one SELECT.
        ("POST", "/autorun-setup-run") => {
            let answer = match (unattended_run_is_using_the_browser(), autorun_root()) {
                (Some(busy), _) => busy,
                (None, Err(refused)) => refused,
                (None, Ok(root)) => {
                    let now = crate::autorun::sessions::now_ms();
                    let timing = crate::browser::timing::Timing::supervised();
                    let reading = crate::modules::current().contains(&crate::modules::Module::DbAccess);
                    let mut held = crate::commands::autorun::session_slot(slot).lock().await;
                    match reading.then(|| db_ready(ctx)).and_then(Result::ok) {
                        Some((conn, exe)) => {
                            let rows = crate::autorun::setup_find::SqlcmdRows { runner: crate::db::RealRunner, exe, conn };
                            crate::autorun::setup_run::run_setup_in(
                                &mut held, &root, &ctx.org, &ctx.project, body, Some(&rows), &timing, now,
                            )
                            .await
                        }
                        None => {
                            crate::autorun::setup_run::run_setup_in::<_, crate::autorun::setup_find::SqlcmdRows<crate::db::RealRunner>>(
                                &mut held, &root, &ctx.org, &ctx.project, body, None, &timing, now,
                            )
                            .await
                        }
                    }
                }
            };
            crate::autorun::roles::with_rules(answer, crate::autorun::roles::RULES_EXPLORE)
        }
        ("POST", "/autorun-explore-changes") => match autorun_root() {
            Ok(root) => explore_changes(&root, &ctx.org, &ctx.project, body),
            Err(refused) => refused,
        },
        // Release: every Auto Run browser record the app holds is cleared
        // and the app's own browsers closed, for a browser held after it
        // is gone. Never a process outside the app's own jobs.
        ("POST", "/autorun-release") => {
            crate::autorun::page_compact::forget(slot);
            let answer = crate::commands::autorun::release_for_assistant(slot).await;
            // Released, the browser let go (a person's browser left open
            // included): the cases it held are free, as the person's release
            // frees them.
            let unbound = answer.0 < 300
                || (answer.0 == 409 && answer.1 == crate::commands::autorun::PERSONS_BROWSER_OPEN);
            end_claims_of(holder, if unbound { 200 } else { answer.0 });
            with_other_holds(answer, holder)
        }
        // Reset: a clean page in the assistant's own browser - other tabs
        // closed, dialogs and its guard cleared, back home - keeping the
        // sign-in. Never the person's browser, never while a replay runs.
        ("POST", "/autorun-reset") => {
            // A reset from any caller ends a sign-in check going.
            crate::autorun::sign_in_check::cancel();
            crate::autorun::page_compact::forget(slot);
            with_other_holds(crate::commands::autorun::reset_for_assistant(slot, &ctx.org, &ctx.project).await, holder)
        }
        ("POST", "/autorun-discover-area") => autorun_discover_area(ctx, body, slot).await,
        ("POST", "/autorun-replay") => {
            // The claim came at the top, before the rewritten cases are
            // released and before a browser is picked or anything opens.
            release_rewritten_cases(ctx, client).await;
            crate::autorun::page_compact::forget(slot);
            let answer = without_page(autorun_replay(ctx, body, slot, holder).await, body);
            // A replay refused or failed before it opened a browser does not
            // keep the extra browser it reserved.
            crate::commands::autorun_slots::free_extra_if_unused(slot, holder, answer.0);
            crate::autorun::roles::with_rules(answer, crate::autorun::roles::RULES_REPLAY)
        }
        // The assistant's replay going, stopped through the same stop the
        // person's Stop sets, and its answer once it has ended.
        ("POST", "/autorun-replay-stop") => {
            crate::autorun::roles::with_rules(autorun_replay_stop_with(slot, REPLAY_ANSWER_WAIT).await, crate::autorun::roles::RULES_REPLAY)
        }
        ("GET", "/autorun-failures") => {
            release_rewritten_cases(ctx, client).await;
            crate::autorun::roles::with_rules(autorun_failures(ctx, target), crate::autorun::roles::RULES_FAILURES)
        }
        ("POST", "/autorun-quirk") => autorun_quirk(ctx, body),
        ("POST", "/autorun-quirk-retire") => autorun_quirk_retire(ctx, body),
        ("POST", "/autorun-validation-rule") => record_validation_rule(ctx, body, slot).await,
        ("POST", "/autorun-defect") => autorun_defect(body),
        // A place the live app or its data differs from the case: stored for a
        // person to rule on, quoting the case, checked against it.
        ("POST", "/autorun-divergence") => autorun_divergence(ctx, client, body).await,
        // The assistant applying a ruling the person gave in the chat, quoting
        // their words, and clearing its own suspected-defect mark the same way.
        ("POST", "/autorun-divergence-ruling") => autorun_divergence_ruling(ctx, client, body).await,
        ("POST", "/autorun-defect-clear") => autorun_defect_clear(body),
        // Components: saved only once the open discovery has tried them
        // live, and retired only while no saved script uses them.
        ("POST", "/autorun-component-save") => autorun_component_save(ctx, client, body, slot).await,
        ("POST", "/autorun-component-retire") => autorun_component_retire(ctx, body),
        // Auto Run's own order for a PBI. Reads the PBI's cases from Azure
        // DevOps (a read) to refuse an id the PBI is not tested by.
        ("POST", "/autorun-order") => autorun_order(ctx, client, body).await,
        // The active environment's accounts: the assistant proposes logins
        // for a person to add, and reads the ones there - key, label,
        // username and whether a password is set, never the password.
        ("POST", "/accounts-propose") => accounts_propose(ctx, body).await,
        // Check sign-in: each account signed in once, in a browser of its own,
        // and walked as far as an area. Uses no slot: the person's Auto Run
        // browser and the assistant's own are left alone.
        ("POST", "/autorun-check-sign-in") => autorun_check_sign_in(ctx, body).await,
        // Look at a sign-in page, and propose a recipe that signed in on a
        // trial: kept for the person to approve, never saved from here.
        ("POST", "/autorun-inspect-sign-in") => autorun_sign_in_proposal(ctx, body, false).await,
        ("POST", "/autorun-propose-sign-in") => autorun_sign_in_proposal(ctx, body, true).await,
        // With the project's Test files beside them, by name and size: what
        // a case uploads.
        ("GET", "/accounts") => accounts_read(ctx, crate::commands::autorun_slots::slot_if_any(caller)),
        // A file the assistant made, saved into those Test files.
        ("POST", "/test-file-save") => test_file_save(ctx, body),
        // The API template routes: gated with the Auto Run ones by the
        // guard above. Proving and running write to the application, so
        // both also need the person's own switch (`ctx.api_writes`); the
        // guide and the list are reads and answer either way.
        ("GET", "/api-template-guide") => (200, api_template_guide(ctx)),
        ("GET", "/api-templates") => api_template_list(ctx, target),
        ("POST", "/api-template-prove") => {
            // A proof refused its account because the Auto Run browser holds
            // it asks the caller's own browser, as a run does; a caller with
            // no browser has none to ask.
            api_template_prove_for(
                crate::commands::autorun_slots::slot_if_any(caller),
                ctx,
                body,
                real_template_browsers,
                real_stage_db,
                &crate::commands::autorun_replay::replay_timing(false),
            )
            .await
        }
        ("POST", "/api-template-run") => {
            // The assistant's own explore session runs it when that session
            // holds the template's account; any other call goes the usual way.
            // A caller with no browser of its own is refused nothing here.
            let slot = crate::commands::autorun_slots::slot_if_any(caller);
            if let Some(answer) = run_saved_in_session(ctx, slot, body).await {
                return answer;
            }
            let answer = api_template_run_for(
                slot,
                ctx,
                body,
                real_template_browsers,
                real_stage_db,
                &crate::commands::autorun_replay::replay_timing(false),
            )
            .await;
            with_discovery_hint(answer, crate::commands::autorun::discovery_active_in(slot.unwrap_or(SlotNo::MAIN)), false)
        }
        // One GET or POST from the assistant's own explore session, sent
        // from the page that session's browser is on.
        ("POST", "/api-template-send") => match (unattended_run_is_using_the_browser(), autorun_root()) {
            (Some(busy), _) => busy,
            (None, Ok(root)) => {
                let mut held = crate::commands::autorun::session_slot(slot).lock().await;
                let answer = send_in(
                    &mut held,
                    ctx,
                    &root,
                    body,
                    &crate::browser::timing::Timing::supervised(),
                    crate::api_templates::runner::RUN_LIMIT,
                    &crate::api_templates::runner::RETRY_PAUSES,
                )
                .await;
                let_go_if_silent(&mut held, answer).await
            }
            (None, Err(refused)) => refused,
        },
        // Takes back the creates one explore session verified, and only
        // those; needs the person's API templates switch.
        ("POST", "/api-template-explore-undo") => api_template_explore_undo(ctx, body, slot).await,
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
        ("GET", "/api-template-fixtures") => api_fixture_list(ctx, target),
        ("POST", "/api-template-fixture-save") => api_fixture_save(ctx, body),
        ("POST", "/api-template-fixture-run") => {
            let answer =
                api_fixture_run(ctx, body, real_template_browsers, &crate::commands::autorun_replay::replay_timing(false))
                    .await;
            let asker = crate::commands::autorun_slots::slot_if_any(caller).unwrap_or(SlotNo::MAIN);
            with_discovery_hint(answer, crate::commands::autorun::discovery_active_in(asker), true)
        }
        // The database routes: Database Access's (the guard above). Beyond
        // that, what they may do is decided by the connection the person
        // chose and the write switch beside it - never by the build kind.
        ("POST", "/db-lookup") => db_lookup(ctx, body).await,
        ("POST", "/db-query") => db_query(ctx, body).await,
        // The Crystal Reports routes: the module's (the guard above). They
        // act on the one report TCM has open and on report files on this
        // machine, so none of them needs a signed-in client; an accepted
        // edit is saved over the file before the answer comes back.
        ("POST", "/crystal-open") => crystal_open(body).await,
        ("POST", "/crystal-read") => crystal_read(body).await,
        ("POST", "/crystal-change") => crystal_change(body).await,
        ("POST", "/crystal-compare") => crystal_compare(body).await,
        ("POST", "/crystal-check") => crystal_check(body).await,
        ("POST", "/crystal-new") => crystal_new(body).await,
        ("POST", "/crystal-preview") => crystal_preview(body).await,
        ("POST", "/crystal-parameters") => crystal_parameters().await,
        ("POST", "/crystal-find") => crystal_find(body).await,
        ("POST", "/crystal-replace") => crystal_replace(body).await,
        // Runs the report's stored procedure on the chosen database, so it
        // is gated like a database write besides (`crystal_preview_live`).
        ("POST", "/crystal-preview-live") => {
            crystal_preview_live(ctx, body, &crate::modules::current(), rpt_core::preview::preview_live_report).await
        }
        ("GET", "/crystal-guide") => crystal_guide(),
        // The proxy asks for this before listing tools and before every
        // call, so a toggle in the app takes effect on the assistant's next
        // tools/list. `modules` names the modules that are on: the proxy is
        // a separate process and cannot read the app's choice.
        //
        // `autorun` is TRANSITIONAL, for one release: a proxy from before
        // modules, still running in an editor, reads it to decide whether
        // the Auto Run tools are offered. Remove it in the release after.
        ("GET", "/tools") => {
            let mut answer = serde_json::json!({
                "disabled": ctx.disabled_tools,
                "modules": crate::modules::current().iter().map(|m| m.key()).collect::<Vec<_>>(),
                // The proxy words its refusals by this: it cannot read the
                // switch itself.
                "unlocked": crate::extras::unlocked(),
                "autorun": crate::modules::on(crate::modules::Module::AutoRun),
                // Only while writes are on too: without them there is no
                // change to run, asked about or not.
                "db_no_ask": ctx.db_no_ask && ctx.db_writes,
                // The proxy offers the network tools only on an explicit yes.
                "network_tools": ctx.network_tools,
            });
            // The proxy words the Azure DevOps tools' descriptions by this:
            // they read across every project only while it is on. Sent only
            // then, so with the switch off the answer is what it always was
            // (a proxy that does not know the key reads a missing one as off).
            if ctx.all_projects {
                answer["all_projects"] = serde_json::json!(true);
            }
            (200, answer.to_string())
        }
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
    };
    // A lease refusal in the answer of a route that signs in says whose
    // browser holds the account, once, here, whichever arm answered.
    release_failed_start(newly_claimed, caller, path, answer.0);
    name_the_holder(answer, caller, method, path)
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

/// The module a bridge route belongs to, by the shape of its PATH: every
/// `/autorun-` route and `/accounts` / every `/accounts-` one (the Auto Run
/// accounts) is Auto Run's; every `/api-template` one is API Templates';
/// `/db-lookup` and `/db-query` are Database Access's; every `/crystal-`
/// one is Crystal Reports'. Anything else is Core. By path rather than by
/// arm, so a route added later is covered by its name rather than by
/// somebody remembering to repeat a check.
pub fn module_of_route(path: &str) -> Option<crate::modules::Module> {
    use crate::modules::Module;
    if path.starts_with("/autorun-") || path == "/accounts" || path.starts_with("/accounts-") || path == "/test-file-save"
    {
        Some(Module::AutoRun)
    } else if path.starts_with("/api-template") {
        Some(Module::ApiTemplates)
    } else if path == "/db-lookup" || path == "/db-query" {
        Some(Module::DbAccess)
    } else if path.starts_with("/crystal-") {
        Some(Module::CrystalReports)
    } else {
        None
    }
}

/// The guard `route` runs ONCE, before its match: a route whose module is
/// off is refused 404 with that module's sentence, unconditionally - not
/// falling through to whatever `ctx.disabled_tools` says. `on` is explicit
/// so every combination stays testable.
pub fn module_guard_for(path: &str, on: &std::collections::BTreeSet<crate::modules::Module>) -> Option<(u16, String)> {
    match module_of_route(path) {
        Some(m) if !on.contains(&m) => Some((404, m.off_sentence())),
        _ => None,
    }
}

/// `module_guard_for`, plus the one route shared by two modules: the quirk
/// routes are Auto Run's, but a quirk sent `from: "api"` (`record_quirk`
/// from an assistant building API templates) is API Templates' too, so it
/// is refused with that module's sentence while API Templates is off.
pub fn module_guard(path: &str, body: &str, on: &std::collections::BTreeSet<crate::modules::Module>) -> Option<(u16, String)> {
    use crate::modules::Module;
    if let Some(refused) = module_guard_for(path, on) {
        return Some(refused);
    }
    if path.starts_with("/autorun-quirk") && !on.contains(&Module::ApiTemplates) {
        let from_api = serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .is_some_and(|v| v.get("from").and_then(|f| f.as_str()) == Some(crate::autorun::quirks::FROM_API));
        if from_api {
            return Some((404, Module::ApiTemplates.off_sentence()));
        }
    }
    None
}

/// The network tools' routes: the Auto Run browser's captured requests,
/// HAR files, test headers and cookies. Answered only while the person's
/// Network tools switch is on (`network_guard`).
pub const NETWORK_ROUTES: [&str; 12] = [
    "/autorun-requests",
    "/autorun-requests-search",
    "/autorun-request",
    "/autorun-response",
    "/autorun-response-body",
    "/autorun-capture",
    "/autorun-requests-export",
    "/autorun-requests-import",
    "/autorun-headers",
    "/autorun-cookies",
    "/autorun-cookie",
    "/autorun-cookies-clear",
];

/// Said when a network tool's route is called while the person's Network
/// tools switch is off.
pub const NETWORK_TOOLS_OFF: &str =
    "the network tools are switched off - the person turns them on under Network tools on the AI Bridge tab (each environment keeps its own)";

/// The guard `route` runs after the module's: a network route is refused
/// while the switch is off, before anything else about the call is looked
/// at. `on` is explicit so both sides stay testable.
pub fn network_guard(path: &str, on: bool) -> Option<(u16, String)> {
    (!on && NETWORK_ROUTES.contains(&path)).then(|| (403, NETWORK_TOOLS_OFF.to_string()))
}

/// Said when a prove or a run arrives while the person's API templates
/// switch is off - before anything else about the call is looked at.
pub const API_WRITES_OFF: &str = "API templates are switched off, and that switch also covers POSTs from send_api_request (even ones that only read) - turn it on under API templates on the AI Bridge tab";

// Said when a prove, a run or a fixture run arrives while another one
// holds the process-wide slot (`runner::claim`).
use crate::api_templates::runner::API_TEMPLATE_BUSY;

