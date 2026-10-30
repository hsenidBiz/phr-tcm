//! The AI bridge: a 127.0.0.1-only service that lets the tcm-mcp stdio
//! binary (and therefore AI tools) read a live writing guide, fetch real
//! example cases, reorganise a draft into a run sheet, and search PBIs.
//! Reads and pure transforms ONLY - no writes, no raw ADO passthrough, and
//! the bearer token never leaves the app. Modeled on note_server.rs; the
//! TCP loop is thin, all logic lives in `route` so tests need no sockets.

use crate::autorun::agents::AgentKey;
use crate::autorun::slots::{SlotNo, SLOT_COUNT};
use rand::RngExt;
use serde::Serialize;

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
    route_timed(&crate::autorun::agents::Caller::Default, ctx, client, method, target, body, version).await
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
        _ => Ok(()),
    };
    if let Err(refused) = claimed {
        return refused;
    }
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
            Err(refused) => return refused,
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
                crate::commands::autorun::end_discovery(EndedBy::Assistant, slot).await
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
            with_other_holds(crate::commands::autorun::release_for_assistant(slot).await, holder)
        }
        // Reset: a clean page in the assistant's own browser - other tabs
        // closed, dialogs and its guard cleared, back home - keeping the
        // sign-in. Never the person's browser, never while a replay runs.
        ("POST", "/autorun-reset") => {
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
        // (never passwords) for a person to add, and reads the ones there -
        // passwords included only in an environment marked as a test one.
        ("POST", "/accounts-propose") => accounts_propose(body),
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

/// Said in `get_accounts`' answer, in place of the Test files, with no
/// project open: Test files belong to one.
pub const TEST_FILES_NEED_A_PROJECT: &str = "open a project in the app first - Test files belong to a project";

/// This project's Test files for `get_accounts`' answer, each by name and
/// human size and nothing else - never a path or a date. `Err` is the
/// sentence said in their place: no project open, or a folder that could
/// not be read.
fn test_files_answer(root: &std::path::Path, ctx: &BridgeContext) -> Result<Vec<serde_json::Value>, String> {
    if ctx.org.trim().is_empty() || ctx.project.trim().is_empty() {
        return Err(TEST_FILES_NEED_A_PROJECT.to_string());
    }
    // `list`'s own sentence already says what failed and where to look.
    let files = crate::test_files::list(&crate::test_files::folder(root, &ctx.org, &ctx.project))?;
    Ok(files
        .into_iter()
        .map(|f| serde_json::json!({ "name": f.name, "size": crate::test_files::human_size(u64::from(f.size)) }))
        .collect())
}

/// Said when `save_test_file` is given no content, or more than one.
pub const TEST_FILE_ONE_SOURCE: &str = "Give the file's content as exactly one of text, base64 or from_path.";

/// Said when `save_test_file` is given no name and no `from_path` to take it from.
pub const TEST_FILE_NEEDS_A_NAME: &str = "Give the file a name, or a from_path to take it from.";

/// Said when `base64` is not standard base64.
pub const TEST_FILE_BAD_BASE64: &str = "base64 is not standard base64 - encode the file's bytes and save it again.";

/// Said when `from_path` is not a plain local absolute path: a relative one, a network share
/// (`\\server\share`) or a device path (`\\?\`, `\\.\`).
pub const TEST_FILE_PATH_NOT_LOCAL: &str = "from_path must be a file on this computer's own drives.";

/// `POST /test-file-save`: a file the assistant made, saved into the project's Test files
/// (`test_files::save_bytes`). Exactly one of `text`, `base64` or `from_path` carries the
/// content. Every refusal is a 400 with a sentence that names the file and no path; the
/// raw error goes to the app log. It writes only inside the project's Test files folder.
fn test_file_save(ctx: &BridgeContext, body: &str) -> (u16, String) {
    use base64::Engine as _;
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return (400, "that is not readable JSON.".to_string()),
    };
    let text_of = |key: &str| match &v[key] {
        serde_json::Value::String(s) => Some(s.clone()),
        _ => None,
    };
    let (text, b64, from_path) = (text_of("text"), text_of("base64"), text_of("from_path"));
    if [text.is_some(), b64.is_some(), from_path.is_some()].iter().filter(|x| **x).count() != 1 {
        return (400, TEST_FILE_ONE_SOURCE.to_string());
    }
    let replace = v["replace"].as_bool() == Some(true);
    let malformed = v["malformed"].as_bool() == Some(true);
    // Judged on the text alone, before any file system call: a share is never opened.
    if from_path.as_deref().is_some_and(|p| !crate::test_files::is_plain_local_path(p)) {
        return (400, TEST_FILE_PATH_NOT_LOCAL.to_string());
    }
    if ctx.org.trim().is_empty() || ctx.project.trim().is_empty() {
        return (400, TEST_FILES_NEED_A_PROJECT.to_string());
    }
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let folder = crate::test_files::folder(&root, &ctx.org, &ctx.project);
    let given_name = text_of("name").map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    let name = match (&given_name, &from_path) {
        (Some(n), _) => n.clone(),
        (None, Some(p)) => crate::test_files::picked_name(std::path::Path::new(p)),
        (None, None) => return (400, TEST_FILE_NEEDS_A_NAME.to_string()),
    };
    let bytes = if let Some(t) = text {
        t.into_bytes()
    } else if let Some(b) = b64 {
        match base64::engine::general_purpose::STANDARD.decode(b.trim()) {
            Ok(bytes) => bytes,
            Err(_) => return (400, TEST_FILE_BAD_BASE64.to_string()),
        }
    } else {
        let path = std::path::PathBuf::from(from_path.unwrap_or_default());
        // The source is itself a kind of file a test uploads, and the saved name keeps its
        // extension: a file cannot be copied in under another kind's name.
        let source_name = crate::test_files::picked_name(&path);
        if !crate::test_files::is_uploadable(&source_name) {
            return (400, crate::test_files::not_uploadable(&source_name));
        }
        let ext = crate::test_files::extension_of(&source_name).unwrap_or_default();
        if crate::test_files::extension_of(&name).as_deref() != Some(ext.as_str()) {
            return (400, format!("The saved name must keep the file's own extension (.{ext})."));
        }
        match std::fs::metadata(&path) {
            Ok(m) if m.is_file() => {}
            Ok(_) => return (400, format!("\"{name}\" is not a file")),
            Err(e) => {
                crate::applog::warn(format!("test files: \"{name}\" could not be read for the assistant: {e}"));
                return (400, format!("\"{name}\" could not be read - check the path and save it again"));
            }
        }
        match crate::test_files::read_capped(&path, &name) {
            Ok(bytes) => bytes,
            Err(why) => return (400, why),
        }
    };
    let replaced = replace && crate::test_files::resolve(&folder, &name).is_ok_and(|p| p.exists());
    match crate::test_files::save_bytes(&folder, &name, &bytes, replace, malformed) {
        Ok(saved) => (
            200,
            serde_json::json!({
                "saved": saved.name,
                "size": crate::test_files::human_size(u64::from(saved.size)),
                "replaced": replaced,
                "use": "Refer to it by its name: an Auto Run upload action's file, or a template's files.",
            })
            .to_string(),
        ),
        Err(why) => (400, why),
    }
}

/// Said on every answer of a discovery that must not save
/// (`with_no_save_note`): the flag, once set, holds until the discovery
/// ends.
pub const NO_SAVE_ON: &str = "(must not save: on for this discovery)";

/// `answer` saying `NO_SAVE_ON` when `on`: a JSON object gets it as
/// `must_not_save`, any other answer after its text.
pub fn with_no_save_note(answer: (u16, String), on: bool) -> (u16, String) {
    if !on {
        return answer;
    }
    let (status, text) = answer;
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(serde_json::Value::Object(mut map)) => {
            map.insert("must_not_save".to_string(), serde_json::json!(NO_SAVE_ON));
            (status, serde_json::Value::Object(map).to_string())
        }
        _ => (status, format!("{text} {NO_SAVE_ON}")),
    }
}

/// Whether the discovery holding slot `n`'s Auto Run browser must not save. Only
/// looks: a browser someone else holds right now (a run, a replay) is
/// never waited for, and then there is no discovery to speak of.
fn discovery_no_save(n: SlotNo) -> bool {
    match crate::commands::autorun::session_slot(n).try_lock() {
        Ok(slot) => slot.as_ref().and_then(|s| s.discovery.as_ref()).is_some_and(|d| d.no_save),
        Err(_) => false,
    }
}

/// What a template or fixture run refused its account says when the
/// assistant's own discovery is what holds it: end the discovery first,
/// or, for a template, run as another account. A fixture always signs in
/// as its own account.
pub fn discovery_holds_hint(fixture: bool) -> String {
    let other = if fixture {
        "a fixture always signs in as its own account"
    } else {
        "or run the template as another account no browser holds"
    };
    format!(
        "Your discovery holds this account in the Auto Run browser: end it with end_autorun_discovery first (what it mapped is kept) and run again; {other}."
    )
}

/// `answer` with `discovery_holds_hint` added when a discovery is going
/// (`discovering`) and the run was refused its account because the Auto
/// Run browser holds it: as `discovery` on a JSON object, else on a line
/// after the text.
pub fn with_discovery_hint(answer: (u16, String), discovering: bool, fixture: bool) -> (u16, String) {
    let held = format!(" was in use by {}", crate::autorun::lease::BROWSER_WORDS);
    if !discovering || answer.0 < 400 || !answer.1.contains(&held) {
        return answer;
    }
    let (status, text) = answer;
    let hint = discovery_holds_hint(fixture);
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(serde_json::Value::Object(mut map)) => {
            map.insert("discovery".to_string(), serde_json::json!(hint));
            (status, serde_json::Value::Object(map).to_string())
        }
        _ => (status, format!("{text}\n{hint}")),
    }
}

/// Said in place of the page dump an answer leaves out (`without_page`).
pub const PAGE_LEFT_OUT: &str = "left out (page: false) - get_autorun_page reads it";

/// Whether a call's body wants the page in its answer: every body but one
/// that sent `"page": false`.
pub fn page_wanted(body: &str) -> bool {
    // `page` takes text too (`"full"`), which `coerce_args` then leaves as
    // text: the text `false` is the boolean.
    let asked = serde_json::from_str::<serde_json::Value>(body).ok().and_then(|v| match v.get("page")? {
        serde_json::Value::Bool(b) => Some(*b),
        serde_json::Value::String(s) if s.trim().eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    });
    asked != Some(false)
}

/// Whether a call's body asked for the whole page tree (`"page": "full"`)
/// rather than the compact one (`page_compact`).
pub fn page_full(body: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .is_some_and(|v| v.get("page").and_then(|p| p.as_str()).is_some_and(|p| p.trim().eq_ignore_ascii_case("full")))
}

/// Who reads the pages of a call: the caller as `kept::owner_of` names it.
pub fn page_owner(caller: &crate::autorun::agents::Caller) -> String {
    crate::autorun::kept::owner_of(caller)
}

/// The page an answer carries: the `page` of a JSON answer, or the text of a
/// plain one (a batch's lines and its page).
fn page_of(text: &str) -> Option<String> {
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(serde_json::Value::Object(map)) => map.get("page").and_then(|p| p.as_str()).map(str::to_string),
        Ok(_) => None,
        Err(_) => Some(text.to_string()),
    }
}

/// `read` (a call that reads the page) for slot `n`'s reader `owner`: the
/// landmarks that reader already has are one line each in the page it
/// reads (`page_compact`), unless `full`; and what the answer printed is what
/// the reader is remembered to have. A failed answer remembers nothing.
pub async fn compacted<F: std::future::Future<Output = (u16, String)>>(
    n: SlotNo,
    owner: &str,
    full: bool,
    read: F,
) -> (u16, String) {
    use crate::autorun::page_compact::{known_for, remember, scoped};
    let answer = scoped(known_for(n, owner, full), read).await;
    if answer.0 == 200 {
        if let Some(page) = page_of(&answer.1) {
            remember(n, owner, &page);
        }
    }
    answer
}

/// A reset's or a release's answer, ending with the other assistants' live
/// holds (`autorun_slots::live_holds_except`) so the coordinator sees who has
/// which browser, account and cases and for how long it has been quiet. With
/// none held it is the answer unchanged.
fn with_other_holds(answer: (u16, String), holder: Option<&AgentKey>) -> (u16, String) {
    let holds = crate::commands::autorun_slots::live_holds_except(crate::autorun::sessions::now_ms(), holder);
    let note = crate::commands::autorun_slots::holds_note(&holds);
    if note.is_empty() {
        return answer;
    }
    (answer.0, format!("{}\n\n{note}", answer.1))
}

/// `answer` with its `page` dump left out when the call's body sent
/// `"page": false` (`page_wanted`): start_autorun_discovery,
/// discover_autorun_actions and replay_autorun_to_step answer the whole
/// page, which a caller that only wants the outcome can do without. The
/// page is still read, so what it shows is still recorded as seen;
/// `page_left_out` says where to read it. Any other body, an answer that
/// is not a JSON object, or one with no page, is unchanged. The batch's
/// text answer is asked for it instead (`discover_actions_paged`).
pub fn without_page(answer: (u16, String), body: &str) -> (u16, String) {
    if page_wanted(body) {
        return answer;
    }
    let (status, text) = answer;
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(serde_json::Value::Object(mut map)) if map.contains_key("page") => {
            map.remove("page");
            map.insert("page_left_out".to_string(), serde_json::json!(PAGE_LEFT_OUT));
            (status, serde_json::Value::Object(map).to_string())
        }
        _ => (status, text),
    }
}

/// How many templates one answer of the list carries when no `limit` is
/// given, and the most it ever carries.
pub const TEMPLATE_PAGE: usize = 25;
pub const TEMPLATE_PAGE_MAX: usize = 100;
/// The most characters a list answer in full carries; over it the answer is
/// the index, which says how to narrow.
pub const TEMPLATE_ANSWER_CAP: usize = 40_000;
/// A run's `detail` is cut to this many characters in a list answer.
const RUN_DETAIL_CUT: usize = 300;

/// A run record as a list answer shows it: its `detail` cut to
/// `RUN_DETAIL_CUT` characters plus `...`.
fn last_run_cut(r: &crate::api_templates::store::RunRecord) -> serde_json::Value {
    let mut v = serde_json::json!(r);
    if let Some(d) = r.detail.as_deref().filter(|d| d.chars().count() > RUN_DETAIL_CUT) {
        let cut: String = d.chars().take(RUN_DETAIL_CUT).collect();
        v["detail"] = serde_json::json!(format!("{cut}..."));
    }
    v
}

/// The saved templates for this project, filtered and paged.
///
/// Arguments, all optional, from the query: `module` (a case-insensitive
/// substring), `search` (a case-insensitive substring of the id, the
/// title, or a param or output name), `flow` (that flow's templates, and
/// that flow alone), `offset` and `limit` (25 by default, 100 at most),
/// and `id` (that one template in full, with the flow it is on).
/// `with_flow=false` leaves the flows and the Test files out of an answer
/// in full: asked for one template by `id`, the flow's every stage and the
/// file list were most of the answer, and the same each time.
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

    let build = |full: bool| -> serde_json::Value {
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
                    "last_run": s.runs.iter().find(|r| r.mode == store::MODE_RUN).map(last_run_cut),
                })
            })
            .collect();
        let mut paging = serde_json::json!({ "total": total, "offset": offset, "returned": rows.len() });
        if offset + rows.len() < total {
            paging["next_offset"] = serde_json::json!(offset + rows.len());
        }
        let mut answer = serde_json::json!({ "templates": rows, "flows": flow_rows, "paging": paging });
        let with_flow = !matches!(arg("with_flow").as_deref(), Some("false") | Some("0"));
        if full && !with_flow {
            if let Some(map) = answer.as_object_mut() {
                map.remove("flows");
            }
            answer["left_out"] = serde_json::json!("the flows and the Test files (flow: false)");
        } else if full {
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
        answer
    };
    let mut answer = build(full);
    if full && !by_id {
        let n = answer.to_string().len();
        if n > TEMPLATE_ANSWER_CAP {
            answer = build(false);
            answer["note"] = serde_json::json!(format!(
                "The answer in full would be {n} characters, more than one answer carries ({TEMPLATE_ANSWER_CAP}): this is the index of {total} templates. Pass id for one template in full, narrow with module, search or flow, or page with a smaller limit."
            ));
        }
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
                    "this call needs an \"account\" - one of the account keys get_guide (topic api_templates) lists. Expected {shape}."
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
/// The default caller's: a refusal for an account the Auto Run browser holds
/// asks main's browser (`api_template_prove_for`).
pub async fn api_template_prove<B: crate::api_templates::held::Keeps, D: crate::api_templates::gate::StageDb>(
    ctx: &BridgeContext,
    body: &str,
    open: impl FnOnce(crate::browser::launch::Browser) -> B,
    open_db: impl FnOnce(&BridgeContext) -> Result<D, (u16, String)>,
    timing: &crate::browser::timing::Timing,
) -> (u16, String) {
    api_template_prove_for(Some(SlotNo::MAIN), ctx, body, open, open_db, timing).await
}

/// `api_template_prove` for a caller whose own Auto Run browser is slot
/// `asker` (none: it has no browser). A proof refused its account because the
/// Auto Run browser holds it says so only when THAT browser holds it.
pub async fn api_template_prove_for<B: crate::api_templates::held::Keeps, D: crate::api_templates::gate::StageDb>(
    asker: Option<SlotNo>,
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
        Err(e) => return (400, format!("that is not a template: {e} - call get_guide (topic api_templates) for the format")),
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
    run_api_template_request(ctx, &root, req, existing.as_ref(), call.browser, asker, open, open_db, timing).await
}

/// `POST /api-template-run`: run a saved template and return its outputs,
/// or the failing step and what had been created. As
/// `api_template_prove`, with the browser factory and the flow database
/// handed in. The default caller's: a refusal for an account the Auto Run
/// browser holds asks main's browser (`api_template_run_for`).
pub async fn api_template_run<B: crate::api_templates::held::Keeps, D: crate::api_templates::gate::StageDb>(
    ctx: &BridgeContext,
    body: &str,
    open: impl FnOnce(crate::browser::launch::Browser) -> B,
    open_db: impl FnOnce(&BridgeContext) -> Result<D, (u16, String)>,
    timing: &crate::browser::timing::Timing,
) -> (u16, String) {
    api_template_run_for(Some(SlotNo::MAIN), ctx, body, open, open_db, timing).await
}

/// `api_template_run` for a caller whose own Auto Run browser is slot
/// `asker` (none: it has no browser), as `api_template_prove_for`.
pub async fn api_template_run_for<B: crate::api_templates::held::Keeps, D: crate::api_templates::gate::StageDb>(
    asker: Option<SlotNo>,
    ctx: &BridgeContext,
    body: &str,
    open: impl FnOnce(crate::browser::launch::Browser) -> B,
    open_db: impl FnOnce(&BridgeContext) -> Result<D, (u16, String)>,
    timing: &crate::browser::timing::Timing,
) -> (u16, String) {
    if !ctx.api_writes {
        return (400, API_WRITES_OFF.to_string());
    }
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let (req, which) = match run_request_from(ctx, &root, body) {
        Ok(read) => read,
        Err(refused) => return refused,
    };
    run_api_template_request(ctx, &root, req, None, which, asker, open, open_db, timing).await
}

/// What `api_template_run` reads of its call: the saved template, the
/// account, the values and the browser asked for, as a run request. Every
/// refusal is a 400 naming what was wrong.
fn run_request_from(
    ctx: &BridgeContext,
    root: &std::path::Path,
    body: &str,
) -> Result<(crate::api_templates::runner::RunRequest, crate::browser::launch::Browser), (u16, String)> {
    use crate::api_templates::runner::{Mode, RunRequest};
    use crate::api_templates::store;
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return Err((400, format!("that is not readable JSON: {e}. Expected {RUN_SHAPE}."))),
    };
    let id = match v.get("id") {
        Some(serde_json::Value::String(s)) if !s.trim().is_empty() => s.trim().to_string(),
        _ => return Err((400, format!("this call needs an \"id\". Expected {RUN_SHAPE}."))),
    };
    let call = template_call(&v, RUN_SHAPE)?;
    let template = match store::load(root, &ctx.org, &ctx.project, &id) {
        Ok(Some(t)) => t,
        Ok(None) => {
            return Err((
                400,
                format!("no template called \"{id}\" is saved for this project - list_api_templates shows the ones that are"),
            ))
        }
        Err(e) => return Err((400, e)),
    };
    let req = RunRequest {
        org: ctx.org.clone(),
        project: ctx.project.clone(),
        account: call.account,
        values: call.values,
        mode: Mode::Run,
        template,
    };
    Ok((req, call.browser))
}

/// Said beside the report of a saved template that ran in the assistant's
/// own explore session instead of a browser of its own.
pub const RAN_IN_YOUR_SESSION: &str = "Ran in your explore session's browser, as its account.";
/// Said beside such a run when the template saves: what it created or
/// changed is not on the change list until the assistant records it.
pub const RUN_REMINDER: &str = "Record what this run created or changed with record_explore_change.";

/// `/api-template-run` for a saved template, tried before the usual run: a
/// template runs in the explore session's own browser, as the account that
/// session holds, instead of signing in to a browser of its own. `None` -
/// the usual run goes ahead - unless the slot holds an explore session
/// whose lease holds the call's account and the template is on no flow.
/// Then, in order: a template with a POST step in a session that must not
/// save is refused before anything is sent; the one-at-a-time slot, the
/// environment and the run's own checks; the run from the page the
/// template's token page opens; the run on the template's history and the
/// tab told; each POST step that was sent noted on the session's list of
/// saves. The answer is the run's report with `"ran_in"` beside it (and
/// `"record_change"` when it posts).
#[allow(clippy::too_many_arguments)]
pub async fn run_saved_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    ctx: &BridgeContext,
    root: &std::path::Path,
    req: &crate::api_templates::runner::RunRequest,
    timing: &crate::browser::timing::Timing,
    limit: std::time::Duration,
    retry_pauses: &[std::time::Duration],
) -> Option<(u16, String)> {
    use crate::api_templates::runner::{claim, preflight, run_in_page, TokenPage};
    use crate::api_templates::store::{self, RunRecord};
    use crate::api_templates::Method;
    use crate::autorun::explore as ex;
    if !ctx.api_writes || req.template.stage.is_some() {
        return None;
    }
    let browser = slot.as_mut()?;
    let parts = browser.parts();
    let state = parts.discovery.as_ref()?;
    let session = state.explore_session()?.to_string();
    if !parts.lease.holds(root, &req.account) {
        return None;
    }
    let id = req.template.id.as_str();
    let (org, project) = (req.org.as_str(), req.project.as_str());
    let vars: std::collections::BTreeMap<String, serde_json::Value> =
        crate::api_templates::with_defaults(&req.template, &req.values).into_iter().collect();
    let posts = req.template.steps.iter().any(|s| s.method == Method::Post);
    if posts && discovery_must_not_save(root, state) {
        let first = req.template.steps.iter().find(|s| s.method == Method::Post)?;
        let path = crate::api_templates::exec::substitute_str(&first.path, &vars);
        return Some((409, crate::browser::save_guard::blocked("POST", &path)));
    }
    let Some(_claim) = claim() else {
        return Some((409, API_TEMPLATE_BUSY.to_string()));
    };
    if let Err(moved) = assistant_may_still_write(ctx, root) {
        return Some((409, moved));
    }
    if let Err(problems) = preflight(root, req, None) {
        return Some((400, problems.join("\n")));
    }
    let how = TokenPage::Open(req.template.antiforgery.page.clone());
    let out = run_in_page(parts.driver, root, req, timing, how, limit, retry_pauses).await;
    let report = &out.report;

    let record = RunRecord {
        at: crate::applog::stamp(),
        mode: req.mode.label().to_string(),
        account: req.account.clone(),
        ok: report.ok,
        failed_step: report.failed.clone(),
        detail: (!report.ok).then(|| report.message()),
        outputs: if report.ok { report.outputs.clone() } else { report.created.clone() },
    };
    match store::append_run(root, org, project, id, record) {
        Ok(()) => templates_changed(id),
        Err(e) => crate::applog::warn(format!("api template {id}: the run could not be added to its history: {e}")),
    }

    // Each POST step that was sent, by its path with the values and what the
    // run captured filled in.
    let mut with_captures = vars;
    with_captures.extend(report.created.clone());
    let sent: Vec<(String, String)> = req
        .template
        .steps
        .iter()
        .filter(|s| s.method == Method::Post)
        .filter(|s| report.steps.iter().any(|r| r.name == s.name && r.status.is_some()))
        .map(|s| {
            let path = crate::api_templates::exec::substitute_str(&s.path, &with_captures);
            // The `handler` says which save the page's address is.
            let path = match s.query.iter().find(|(k, _)| k.eq_ignore_ascii_case("handler")) {
                Some((_, handler)) => format!(
                    "{path}?handler={}",
                    crate::api_templates::exec::substitute_str(handler.trim(), &with_captures)
                ),
                None => path,
            };
            ("POST".to_string(), path)
        })
        .collect();
    if let Err(why) = ex::note_writes(root, &session, &sent) {
        crate::applog::warn(format!("Auto Run explore session: a save could not be noted: {why}"));
    }

    let mut v = serde_json::to_value(report).unwrap_or_else(|_| serde_json::json!({ "ok": report.ok }));
    v["ran_in"] = serde_json::json!(RAN_IN_YOUR_SESSION);
    if posts {
        v["record_change"] = serde_json::json!(RUN_REMINDER);
    }
    Some((if report.ok { 200 } else { 502 }, v.to_string()))
}

/// `POST /api-template-run` with the caller's own browser (`n`, when it has
/// one): the explore session's browser when it can run the template
/// (`run_saved_in`), and `None` when the usual run should go ahead - at once
/// for a caller with no browser.
async fn run_saved_in_session(ctx: &BridgeContext, n: Option<SlotNo>, body: &str) -> Option<(u16, String)> {
    let n = n?;
    if !ctx.api_writes || unattended_run_is_using_the_browser().is_some() {
        return None;
    }
    let root = autorun_root().ok()?;
    let (req, _) = run_request_from(ctx, &root, body).ok()?;
    let mut slot = crate::commands::autorun::session_slot(n).lock().await;
    let answer = run_saved_in(
        &mut slot,
        ctx,
        &root,
        &req,
        &crate::browser::timing::Timing::supervised(),
        crate::api_templates::runner::RUN_LIMIT,
        &crate::api_templates::runner::RETRY_PAUSES,
    )
    .await?;
    Some(let_go_if_silent(&mut slot, answer).await)
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
    asker: Option<SlotNo>,
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
    // Judged on the context it arrived with; looked at again now that no
    // switch can land (`still_allowed`).
    if let Err(moved) = assistant_may_still_write(ctx, root) {
        return (409, moved);
    }
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
            via: None,
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
    let mut text = serde_json::to_string(&report).unwrap_or_default();
    if !report.ok && held_by_the_auto_run_browser(&text, &req.account) {
        // A template run is not the caller's own browser's, so the hint asks
        // the caller's: a caller with no browser has none to free.
        if let Some(n) = asker {
            let holds = crate::commands::autorun::assistant_browser_holds(n, &req.account).await;
            text = template_report_hint(text, &req.account, holds);
        }
    }
    (if report.ok { 200 } else { 502 }, text)
}

/// Said beside a template run refused its account because the Auto Run
/// browser holds it, when the assistant's replay opened that browser.
pub const FREE_IT_WITH_RELEASE: &str =
    "The Auto Run browser your replay left open holds this account: close it with reset_autorun_browser and `release: true`, then run the template again.";

/// Does a template run's report say its account `key` was refused because
/// the Auto Run browser holds it (the lease's own sentence,
/// `lease::in_use_by_browser`)?
fn held_by_the_auto_run_browser(report_json: &str, key: &str) -> bool {
    report_json.contains(&crate::autorun::lease::in_use_by_browser(key))
}

/// A failed template run's report, with `FREE_IT_WITH_RELEASE` beside it as
/// `"free_it"` when its account `key` was refused because the Auto Run
/// browser holds it and that browser is the assistant's own, holding this
/// very account (`assistant_holds`). Otherwise the report as it was.
pub fn template_report_hint(report_json: String, key: &str, assistant_holds: bool) -> String {
    if !assistant_holds || !held_by_the_auto_run_browser(&report_json, key) {
        return report_json;
    }
    match serde_json::from_str::<serde_json::Value>(&report_json) {
        Ok(mut v @ serde_json::Value::Object(_)) => {
            v["free_it"] = serde_json::json!(FREE_IT_WITH_RELEASE);
            v.to_string()
        }
        _ => report_json,
    }
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

/// Said beside a summary list of fixtures: how to read one in full.
pub const FIXTURES_IN_SHORT: &str =
    "each fixture in short - call list_api_templates with kind fixtures and id for one in full (its steps, current outputs and last run), or full: true for all";

/// `GET /api-template-fixtures`: the project's saved fixtures. By default a
/// summary of each - its id, name, output names, what it creates (kind and
/// name) and its last run's status - since the whole of every fixture
/// outgrows what an assistant can read. `?id=` answers that one fixture in
/// full (`fixture`), and `?full=true` every one in full, each with its
/// current outputs (from its newest successful run) and its last run. A
/// read: it answers with the API templates switch off.
fn api_fixture_list(ctx: &BridgeContext, target: &str) -> (u16, String) {
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
    let in_full = |s: &crate::api_templates::fixture_store::SavedFixture| {
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
    };
    if let Some(id) = q(target, "id").map(|i| i.trim().to_string()).filter(|i| !i.is_empty()) {
        return match saved.iter().find(|s| s.fixture.id == id) {
            Some(s) => (200, serde_json::json!({ "fixture": in_full(s) }).to_string()),
            None => {
                let ids: Vec<&str> = saved.iter().map(|s| s.fixture.id.as_str()).collect();
                let known = if ids.is_empty() { "none is saved".to_string() } else { format!("the saved ones are {}", ids.join(", ")) };
                (404, format!("no fixture \"{id}\" is saved in this project - {known}"))
            }
        };
    }
    if q(target, "full").is_some_and(|v| v.trim().eq_ignore_ascii_case("true")) {
        let fixtures: Vec<serde_json::Value> = saved.iter().map(in_full).collect();
        let answer = serde_json::json!({ "fixtures": fixtures }).to_string();
        if answer.len() <= TEMPLATE_ANSWER_CAP {
            return (200, answer);
        }
        let n = answer.len();
        return (200, fixture_index(&saved, Some(format!("The fixtures in full would be {n} characters: pass id for one in full."))));
    }
    (200, fixture_index(&saved, None))
}

fn fixture_index(saved: &[crate::api_templates::fixture_store::SavedFixture], over: Option<String>) -> String {
    let fixtures: Vec<serde_json::Value> = saved
        .iter()
        .map(|s| {
            let f = &s.fixture;
            let last = s.runs.first().map(|r| {
                let mut v = serde_json::json!({ "at": r.at, "ok": r.ok });
                if let Some(step) = r.failed_step {
                    v["failed_step"] = serde_json::json!(step);
                }
                v
            });
            serde_json::json!({
                "id": f.id,
                "name": f.name,
                "outputs": f.outputs.keys().collect::<Vec<_>>(),
                "creates": f.creates.iter().map(|c| serde_json::json!({ "kind": c.kind, "name": c.name })).collect::<Vec<_>>(),
                "last_run": last,
            })
        })
        .collect();
    let note = over.unwrap_or_else(|| FIXTURES_IN_SHORT.to_string());
    serde_json::json!({ "fixtures": fixtures, "note": note }).to_string()
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
    let fixture: Fixture = match serde_json::from_value(raw.clone()) {
        Ok(f) => f,
        Err(e) => {
            let said = crate::api_templates::fixture::shape_problem(&raw, &e);
            return (400, format!("that is not a fixture: {said} - call get_guide (topic api_templates) for the format"));
        }
    };
    let existed = matches!(fixture_store::load(&root, &ctx.org, &ctx.project, &fixture.id), Ok(Some(_)));
    match fixture_store::save(&root, &ctx.org, &ctx.project, &fixture) {
        Ok(()) => {
            crate::applog::info(format!("api fixture {}: {}", fixture.id, if existed { "replaced" } else { "saved" }));
            templates_changed(&fixture.id);
            let mut answer = serde_json::json!({ "saved": fixture.id, "replaced": existed });
            // The stages it says this record does not include, echoed so a
            // wrong name is seen at once.
            let templates = |id: &str| crate::api_templates::store::load(&root, &ctx.org, &ctx.project, id).ok().flatten();
            let flows = |id: &str| crate::api_templates::flow_store::load(&root, &ctx.org, &ctx.project, id).ok().flatten();
            let skipped = crate::api_templates::fixture::skipped_stages(&fixture, &templates, &flows);
            if !skipped.is_empty() {
                answer["skipped_stages"] = serde_json::json!(skipped);
            }
            (200, answer.to_string())
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
    let may_run = || assistant_may_still_write(ctx, &root);
    let report = match run_saved(&mut browsers, &root, &ctx.org, &ctx.project, &id, timing, may_run).await {
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

/// What a flow save is told when it would replace a flow without `replace: true` and a why,
/// whether that is found before the database checks or at the moment of writing.
fn flow_already_exists(id: &str) -> String {
    format!("a flow called \"{id}\" already exists - send replace: true and a why to change it")
}

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
    let may_replace = replace && why.as_deref().is_some_and(|w| !w.trim().is_empty());
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
        if replacing && !may_replace {
            problems.push(flow_already_exists(&f.id));
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
    // The early check ran before the database awaits above; a second call may have saved this
    // id since. Looked at again at the moment of writing, under the store's lock, so a call that
    // did not say replace with a why never replaces a flow it did not see.
    match flow_store::save_if(&root, org, project, &f, !may_replace) {
        Ok(()) => {}
        Err(flow_store::SaveIfError::Appeared) => {
            crate::applog::warn(format!("api template flow {}: another call saved it while this one was checked", f.id));
            return (409, flow_already_exists(&f.id));
        }
        Err(flow_store::SaveIfError::Failed(e)) => {
            crate::applog::warn(format!("api template flow {}: it could not be saved: {e}", f.id));
            return (500, "the flow could not be saved - see Settings, Logs".to_string());
        }
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
/// the same live sections after it. `n` is the asker's browser, when it has
/// one: the rules listed are those of the area its discovery is exploring.
fn autorun_guide_with_quirks(ctx: &BridgeContext, quick: bool, n: Option<SlotNo>) -> String {
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
        crate::autorun::discovery_map::rules_section(&map, running_discovery_area(n).as_deref()),
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

/// The area of the discovery going on now in slot `n`'s browser, for the
/// guide's rules; none for a caller with no browser (`n` is `None`). It never
/// waits: a call that holds the browser is busy, and the guide then lists
/// every area's rules, which is still true.
pub fn running_discovery_area(n: Option<SlotNo>) -> Option<String> {
    let n = n?;
    let slot = crate::commands::autorun::session_slot(n).try_lock().ok()?;
    slot.as_ref()?.discovery.as_ref()?.area.clone()
}

/// What `/autorun-network-wait` answers. The registry's view carries no
/// host or port, and nothing is added here.
fn autorun_network_wait_answer() -> String {
    match crate::autorun::network::waits().current() {
        None => serde_json::json!({ "waiting": false }).to_string(),
        Some(w) => serde_json::json!({
            "waiting": true,
            "run_id": w.run_id,
            "before_case_id": w.before_case_id,
            "waited_s": w.waited_s,
            "give_up_in_s": w.give_up_in_s,
        })
        .to_string(),
    }
}

/// The assistant's capture tools (`autorun::capture_tools`), one per route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureTool {
    List,
    Search,
    Request,
    Response,
    ResponseBody,
    GetFilter,
    SetFilter,
    Export,
    Import,
}

impl CaptureTool {
    /// Reads one set: the live capture, or an imported one named by `from`.
    fn reads_a_set(self) -> bool {
        matches!(self, Self::List | Self::Search | Self::Request | Self::Response | Self::ResponseBody)
    }
}

/// How long a capture call waits for the supervised browser's lock: a
/// step, a try or a discovery action holds it for its own job only.
pub const CAPTURE_LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// Said when the browser's lock does not come free for a capture call.
pub const CAPTURE_BUSY: &str =
    "The Auto Run browser is busy (a replay, a step or a discovery action is using it): ask again when it has finished.";

/// A capture tool's call. Nothing is redacted, matched or written while a
/// lock is held: the call takes what it needs out of its set (cloned) and
/// lets the lock go before it answers (`CapturePhase::finish`). An import
/// reads and parses its file before it changes the imported sets (one
/// import at a time, `commands::autorun::change_imported_captures`); a
/// read naming `from` is checked, and its set looked for, before anything
/// else, and never takes the browser's lock. The browser is slot `n`'s.
async fn autorun_capture(tool: CaptureTool, body: &str, n: SlotNo) -> (u16, String) {
    use crate::autorun::capture_tools as ct;
    let root = crate::autorun::store::configured_root();
    let args = match ct::args_of(body) {
        Ok(a) => a,
        Err(e) => return e,
    };
    if tool == CaptureTool::Import {
        let Some(root) = root.as_deref() else {
            return (503, crate::commands::autorun::NO_DATA_DIRECTORY.to_string());
        };
        return match ct::read_import(root, &args) {
            Ok((stem, entries)) => {
                crate::commands::autorun::change_imported_captures(|sets| ct::store_import(sets, stem, entries))
            }
            Err(e) => e,
        };
    }
    let from = if tool.reads_a_set() {
        match ct::from_of(&args) {
            Ok(f) => f,
            Err(e) => return e,
        }
    } else {
        None
    };
    if from.is_some() {
        let phase = {
            let mut imported = crate::commands::autorun::imported_captures();
            capture_tool_begin(&mut None::<crate::commands::autorun::Session>, &mut imported, root.as_deref(), tool, body)
        };
        return phase.finish();
    }
    if crate::autorun::replay_to::state(n).is_running() {
        return (409, CAPTURE_BUSY.to_string());
    }
    let phase = {
        let Ok(mut slot) = tokio::time::timeout(CAPTURE_LOCK_WAIT, crate::commands::autorun::session_slot(n).lock()).await
        else {
            return (409, CAPTURE_BUSY.to_string());
        };
        let mut imported = crate::commands::autorun::imported_captures();
        capture_tool_begin(&mut slot, &mut imported, root.as_deref(), tool, body)
    };
    phase.finish()
}

/// `answer` ending with what the assistant changed for testing in the
/// Auto Run browser (`overrides::with_note`): as `seen` under the session
/// lock the moment the call finished, or - for an answer given before the
/// browser was taken (a refusal) - as last published
/// (`commands::autorun::overrides_now_in`, slot `n`'s).
fn with_overrides_seen(
    answer: (u16, String),
    seen: Option<crate::autorun::overrides::Overrides>,
    n: SlotNo,
) -> (u16, String) {
    let changed = seen.unwrap_or_else(|| crate::commands::autorun::overrides_now_in(n));
    crate::autorun::overrides::with_note(answer, &changed)
}

/// The assistant's test header and cookie tools (`autorun::overrides`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverrideTool {
    /// `set_autorun_headers`
    SetHeaders,
    /// `autorun_cookies`' list: a read, in any supervised browser.
    Cookies,
    /// `autorun_cookies`' set
    SetCookie,
    /// `autorun_cookies`' clear: a real sign-out.
    ClearCookies,
}

/// Said when the browser's lock does not come free for a header or cookie
/// call.
pub const OVERRIDES_BUSY: &str =
    "The Auto Run browser is busy (a step or a discovery action is using it): ask again when it has finished.";

/// Said when a test header or cookie is asked for in the person's browser:
/// only one the assistant opened is changed (spec decision 4).
pub fn persons_browser_not_changed() -> String {
    format!(
        "Test headers and cookies are changed only in an Auto Run browser you opened (a discovery or your replay): {}",
        crate::commands::autorun::PERSONS_BROWSER_OPEN
    )
}

/// Said by a browser that keeps no record of test changes (a test's fake).
pub const KEEPS_NO_OVERRIDES: &str = "this Auto Run browser keeps no record of test changes, so none is made in it";

/// `/autorun-headers`, `/autorun-cookies` and `/autorun-cookie`: the call
/// over the supervised browser (`override_tool_in`). Refused at once while
/// a replay runs (one going holds the browser); the browser's lock is
/// waited for 5 s at most. What is changed is published for the replay's
/// answer (`publish_overrides_in`). The sign-out is
/// `commands::autorun::clear_cookies_for_assistant`. The browser is slot `n`'s.
async fn autorun_override(tool: OverrideTool, body: &str, n: SlotNo) -> (u16, String) {
    if crate::autorun::replay_to::state(n).is_running() {
        return (409, replay_running_refusal(n));
    }
    let Ok(mut slot) = tokio::time::timeout(CAPTURE_LOCK_WAIT, crate::commands::autorun::session_slot(n).lock()).await else {
        return (409, OVERRIDES_BUSY.to_string());
    };
    // Only the sign-out has news for the panes, and it is answered by
    // `commands::autorun::clear_cookies_for_assistant`, not here.
    let (answer, _) = override_tool_in(n, &mut slot, tool, body).await;
    let answer = let_go_if_silent(&mut slot, answer).await;
    crate::commands::autorun::publish_overrides_in(n, &slot);
    if tool != OverrideTool::Cookies {
        // Names only - never a value.
        let now = slot.as_ref().and_then(|s| s.overrides.names()).unwrap_or_else(|| "nothing".to_string());
        crate::applog::info(format!("Auto Run: the assistant's {tool:?} answered {}; changed for testing now: {now}", answer.0));
    }
    answer
}

/// A header or cookie call over the browser `slot` holds (the app's
/// session, or a test's fake), with what the panes must hear (a sign-out).
/// `n` is the slot the browser is in: a replay going there refuses the call.
pub async fn override_tool_in<B: DiscoveryBrowser>(
    n: SlotNo,
    slot: &mut Option<B>,
    tool: OverrideTool,
    body: &str,
) -> ((u16, String), Option<crate::events::AutorunSessionChanged>) {
    let args = match body.trim() {
        "" => serde_json::json!({}),
        text => match serde_json::from_str::<serde_json::Value>(text) {
            Ok(v @ serde_json::Value::Object(_)) => v,
            _ => return ((400, "the arguments are a JSON object".to_string()), None),
        },
    };
    match tool {
        OverrideTool::SetHeaders => (set_headers_in(n, slot, &args).await, None),
        OverrideTool::Cookies => (cookies_in(slot, &args).await, None),
        OverrideTool::SetCookie => (set_cookie_in(n, slot, &args).await, None),
        OverrideTool::ClearCookies => crate::commands::autorun::clear_cookies_in_for_assistant(n, slot).await,
    }
}

/// Refused unless `slot` holds a live browser the assistant opened, with no
/// replay going and no guard of the person's on it - the checks the
/// assistant's reset makes (spec decision 4). A browser that has gone is
/// let go first. A replay going in slot `n`, where the browser is, refuses it.
pub(crate) async fn refuse_acting<B: DiscoveryBrowser>(n: SlotNo, slot: &mut Option<B>) -> Result<(), (u16, String)> {
    if crate::autorun::replay_to::state(n).is_running() {
        return Err((409, replay_running_refusal(n)));
    }
    if forget_gone_browser(slot).await {
        return Err((404, crate::commands::autorun::NO_BROWSER_OPEN.to_string()));
    }
    match slot.as_mut() {
        None => Err((404, crate::commands::autorun::NO_BROWSER_OPEN.to_string())),
        Some(b) => {
            let persons = !b.opened_by_assistant()
                || *b.parts().guard_owner == Some(crate::commands::autorun::GuardOwner::Person);
            if persons {
                Err((409, persons_browser_not_changed()))
            } else {
                Ok(())
            }
        }
    }
}

/// Where the test changes stand, as the acting tools answer: `said`, what
/// is changed (`changed_for_testing`), or that nothing is.
pub(crate) fn overrides_answer(said: String, o: &crate::autorun::overrides::Overrides) -> (u16, String) {
    let mut answer = serde_json::json!({ "said": said });
    answer[crate::autorun::overrides::NOTE_KEY] = match o.changed_line() {
        Some(line) => serde_json::json!(line),
        None => serde_json::json!("Nothing is changed for testing now."),
    };
    (200, answer.to_string())
}

/// What a set of test headers or a cookie lasts for, said once it is set.
const LASTS_UNTIL: &str = "Every try, discovery and replay answer names it until you remove it, or reset_autorun_browser, autorun_cookies' clear or a release takes it away.";

/// `set_autorun_headers` over `slot`: the headers asked for merged into the
/// set in place (a `null` removes one), sent on every tab and on any tab
/// the page opens (`Driver::set_extra_headers`), never through `Fetch`.
/// A refused name (`overrides::REFUSED_HEADERS`) refuses the whole call
/// before the browser is touched.
pub async fn set_headers_in<B: DiscoveryBrowser>(n: SlotNo, slot: &mut Option<B>, args: &serde_json::Value) -> (u16, String) {
    use crate::autorun::overrides as ov;
    let changes = match ov::header_changes(args) {
        Ok(c) => c,
        Err(why) => return (400, why),
    };
    if let Err(refused) = refuse_acting(n, slot).await {
        return refused;
    }
    let Some(b) = slot.as_mut() else {
        return (404, crate::commands::autorun::NO_BROWSER_OPEN.to_string());
    };
    let p = b.parts();
    let Some(o) = p.overrides else {
        return (500, KEEPS_NO_OVERRIDES.to_string());
    };
    let next = match ov::merged(&o.headers, &changes) {
        Ok(n) => n,
        Err(why) => return (400, why),
    };
    if let Err(e) = crate::browser::cdp::Driver::set_extra_headers(p.driver, &next).await {
        // Some tabs may send the new set already: every name stays named
        // until a reset sends none.
        for (name, value) in next {
            o.headers.retain(|k, _| !k.eq_ignore_ascii_case(&name));
            o.headers.insert(name, value);
        }
        let (status, why) = ov::browser_failed(e);
        return (status, format!("{why}. Some tabs may send the new headers already; reset_autorun_browser removes every test header."));
    }
    o.headers = next;
    let said = if o.headers.is_empty() {
        "No test headers are sent now.".to_string()
    } else {
        format!(
            "Sent with every request of every tab, and of any tab the page opens: {}. {LASTS_UNTIL}",
            o.headers.keys().cloned().collect::<Vec<_>>().join(", ")
        )
    };
    overrides_answer(said, o)
}

/// `autorun_cookies`' list over `slot`: every cookie the browser holds for
/// the app's site - the one TCM's sign-in confirmed (`overrides::confirmed_origin`),
/// never whatever page shows, which may be the identity provider's - whatever
/// its path, as `overrides::shown_cookie` shows it (the sign-in's values
/// hidden). `url` may name that site's host or one of its parent domains,
/// nothing else (400). A read: any supervised browser, the person's too.
pub async fn cookies_in<B: DiscoveryBrowser>(slot: &mut Option<B>, args: &serde_json::Value) -> (u16, String) {
    use crate::autorun::overrides as ov;
    let asked = match args.get("url") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(u)) => match ov::url_host(u) {
            Some(host) => Some(host),
            None => return (400, "url is an http or https address, such as https://app.example/".to_string()),
        },
        Some(_) => return (400, "url is an http or https address, such as https://app.example/".to_string()),
    };
    if forget_gone_browser(slot).await {
        return (404, crate::commands::autorun::NO_BROWSER_OPEN.to_string());
    }
    let Some(b) = slot.as_mut() else {
        return (404, crate::commands::autorun::NO_BROWSER_OPEN.to_string());
    };
    let p = b.parts();
    let site = match ov::confirmed_origin(&*p.driver) {
        Ok(origin) => ov::url_host(&origin).unwrap_or_default(),
        Err(refused) => return refused,
    };
    let host = match asked {
        Some(host) if !crate::browser::session::cookie_belongs(&host, &site) => {
            return (400, format!("url is outside the app's site ({site}): name that site or a parent domain of it"));
        }
        Some(host) => host,
        None => site,
    };
    let cookies = match ov::cookies_for_host(p.driver, &host).await {
        Ok(c) => c,
        Err(e) => return ov::browser_failed(e),
    };
    let set = p.overrides.as_deref().cloned().unwrap_or_default();
    let shown: Vec<serde_json::Value> = cookies
        .iter()
        .take(ov::MAX_LISTED)
        .map(|c| {
            let name = c["name"].as_str().unwrap_or("");
            ov::shown_cookie(c, set.cookies.get(name).is_some_and(|s| s.is(name, c)))
        })
        .collect();
    let answer = serde_json::json!({
        "site": host,
        "total": cookies.len(),
        "returned": shown.len(),
        "cookies": shown,
        "note": "The sign-in's cookies (HttpOnly, the sign-in and anti-forgery cookies, names like a secret) show their value as (hidden), and are never set by autorun_cookies.",
    });
    (200, answer.to_string())
}

/// `autorun_cookies`' set over `slot`: one cookie on the app's own site - the
/// one TCM's sign-in confirmed (`overrides::confirmed_origin`), never
/// whatever page shows - on its host only, `path` `/` unless asked;
/// recorded exactly (`SetCookie`) so a reset deletes it only while it is
/// still that, and left out of the sign-in's print
/// (`Held::ignore_in_print`), so it never makes the sign-in look changed.
/// Refused for the sign-in's cookies (`overrides::protected_cookie`), an
/// HttpOnly cookie of that name (in any case) on the site included.
pub async fn set_cookie_in<B: DiscoveryBrowser>(n: SlotNo, slot: &mut Option<B>, args: &serde_json::Value) -> (u16, String) {
    use crate::autorun::overrides as ov;
    let ask = match ov::cookie_ask(args) {
        Ok(a) => a,
        Err(why) => return (400, why),
    };
    if ov::protected_cookie(&ask.name, false) {
        return (403, ov::protected_cookie_sentence(&ask.name));
    }
    if let Err(refused) = refuse_acting(n, slot).await {
        return refused;
    }
    let Some(b) = slot.as_mut() else {
        return (404, crate::commands::autorun::NO_BROWSER_OPEN.to_string());
    };
    let p = b.parts();
    let Some(o) = p.overrides else {
        return (500, KEEPS_NO_OVERRIDES.to_string());
    };
    let origin = match ov::confirmed_origin(&*p.driver) {
        Ok(origin) => origin,
        Err(refused) => return refused,
    };
    let host = ov::url_host(&origin).unwrap_or_default();
    let existing = match ov::cookies_for_host(p.driver, &host).await {
        Ok(c) => c,
        Err(e) => return ov::browser_failed(e),
    };
    let http_only_named = |c: &serde_json::Value| {
        c["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(&ask.name)) && c["httpOnly"].as_bool() == Some(true)
    };
    if existing.iter().any(http_only_named) {
        return (403, ov::protected_cookie_sentence(&ask.name));
    }
    match crate::browser::cdp::Driver::call(p.driver, "Network.setCookie", ov::set_cookie_params(&ask, &origin)).await {
        Ok(reply) if reply["success"].as_bool() == Some(false) => (400, ov::COOKIE_REFUSED.to_string()),
        Ok(_) => {
            let set = ov::SetCookie { domain: host.clone(), path: ask.path.clone(), value: ask.value.clone() };
            o.cookies.insert(ask.name.clone(), set);
            p.lease.ignore_in_print(&ask.name);
            let said = format!("Set cookie {} on {host} (path {}). {LASTS_UNTIL}", ask.name, ask.path);
            overrides_answer(said, o)
        }
        Err(crate::browser::cdp::CdpError::Protocol { message, .. }) => {
            (400, format!("{} The browser said: {message}", ov::COOKIE_REFUSED))
        }
        Err(e) => ov::browser_failed(e),
    }
}


/// A capture call part way: answered already, or holding what it took out
/// of its set, to be redacted and answered with no lock held.
pub enum CapturePhase {
    Answered((u16, String)),
    Read(crate::autorun::capture_tools::Read, crate::autorun::capture_tools::Taken, serde_json::Value),
    Export(std::path::PathBuf, crate::autorun::capture_tools::TakenExport),
}

impl CapturePhase {
    /// The answer: the redaction, the matching and the export's write
    /// happen here.
    pub fn finish(self) -> (u16, String) {
        use crate::autorun::capture_tools as ct;
        match self {
            CapturePhase::Answered(a) => a,
            CapturePhase::Read(read, taken, args) => ct::answer(&taken, read, &args),
            CapturePhase::Export(root, taken) => ct::write_export(&root, taken),
        }
    }
}

/// A capture tool's call over the browser `slot` holds (the app's session,
/// or a test's fake) and the imported sets: what `/autorun-requests` and
/// its siblings answer. A read naming `from` reads that imported set, with
/// no browser needed; every other read, the filter and the export need the
/// browser (404 "No browser is open." without one). An import needs only
/// the Auto Run folder (`root`). `capture_tool_begin`, then its `finish`.
pub fn capture_tool_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    imported: &mut crate::autorun::capture_tools::ImportedSets,
    root: Option<&std::path::Path>,
    tool: CaptureTool,
    body: &str,
) -> (u16, String) {
    capture_tool_begin(slot, imported, root, tool, body).finish()
}

/// The part of a capture call that needs its set: what it takes out of the
/// live capture or an imported set, cloned and unredacted, or an answer
/// already (a refusal, the filter, an import).
pub fn capture_tool_begin<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    imported: &mut crate::autorun::capture_tools::ImportedSets,
    root: Option<&std::path::Path>,
    tool: CaptureTool,
    body: &str,
) -> CapturePhase {
    use crate::autorun::capture_tools as ct;
    use crate::browser::cdp::Driver;
    use CapturePhase::Answered;
    let args = match ct::args_of(body) {
        Ok(a) => a,
        Err(e) => return Answered(e),
    };
    let no_root = || Answered((503, crate::commands::autorun::NO_DATA_DIRECTORY.to_string()));
    let read = match tool {
        CaptureTool::List => Some(ct::Read::List),
        CaptureTool::Search => Some(ct::Read::Search),
        CaptureTool::Request => Some(ct::Read::Request),
        CaptureTool::Response => Some(ct::Read::Response),
        CaptureTool::ResponseBody => Some(ct::Read::ResponseBody),
        _ => None,
    };
    let taken = |set: &ct::Set, read: ct::Read, args: serde_json::Value| match ct::take(set, read, &args) {
        Ok(t) => CapturePhase::Read(read, t, args),
        Err(e) => Answered(e),
    };
    if tool == CaptureTool::Import {
        return match root {
            Some(root) => Answered(ct::import(root, imported, &args)),
            None => no_root(),
        };
    }
    if let Some(read) = read {
        match ct::from_of(&args) {
            Err(e) => return Answered(e),
            Ok(Some(name)) => {
                return match ct::imported_set(imported, &name) {
                    Ok(set) => taken(&set, read, args),
                    Err(e) => Answered(e),
                };
            }
            Ok(None) => {}
        }
    }
    let Some(b) = slot.as_mut() else {
        return Answered((404, crate::commands::autorun::NO_BROWSER_OPEN.to_string()));
    };
    let driver = b.parts().driver;
    if tool == CaptureTool::SetFilter {
        return Answered(match Driver::capture_mut(driver) {
            Some(cap) => ct::set_filter(cap, imported, &args),
            None => (404, ct::NO_CAPTURE.to_string()),
        });
    }
    let Some(cap) = Driver::capture(&*driver) else {
        return Answered((404, ct::NO_CAPTURE.to_string()));
    };
    match (tool, read) {
        (_, Some(read)) => taken(&ct::Set::live(cap), read, args),
        (CaptureTool::GetFilter, _) => Answered((200, ct::filter_answer(cap, imported).to_string())),
        (CaptureTool::Export, _) => match root {
            Some(root) => match ct::take_export(cap, &args) {
                Ok(t) => CapturePhase::Export(root.to_path_buf(), t),
                Err(e) => Answered(e),
            },
            None => no_root(),
        },
        _ => Answered((404, ct::NO_CAPTURE.to_string())),
    }
}

/// The page in the browser the person opened, as text: Chrome's own
/// accessibility tree with a locator on every line.
/// `within` (a locator, as a script writes one: a string, an object or a
/// chain), its `nth` (`within_target_nth`) and `limit`
/// (`PAGE_LINES_DEFAULT` when left out, at most `PAGE_LINES_MAX`) are read
/// from the query.
async fn autorun_page(ctx: &BridgeContext, target: &str, n: SlotNo, owner: &str) -> (u16, String) {
    if let Some(busy) = unattended_run_is_using_the_browser() {
        return busy;
    }
    let raw = q(target, "within").filter(|w| !w.trim().is_empty());
    let within = match within_target_nth(raw.as_deref(), q(target, "nth").as_deref()) {
        Ok(t) => t,
        Err(why) => return (400, why),
    };
    let query = q(target, "query").map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let limit = page_limit(q(target, "limit").as_deref());
    let read = supervised_page_read_in(n, &ctx.org, &ctx.project, limit, within.as_ref(), query.as_deref());
    // The whole page: a landmark this reader already has is one line, before
    // the limit cuts the page (`page_compact`), unless `page=full`. A part of
    // the page, or the lines of a query, are answered as they are.
    if within.is_none() && query.is_none() {
        let full = q(target, "page").is_some_and(|p| p.trim().eq_ignore_ascii_case("full"));
        return Box::pin(compacted(n, owner, full, read)).await;
    }
    read.await
}

/// The line limit a page read was asked for: `PAGE_LINES_DEFAULT` when
/// none (or none that is a positive number) is given, and never more than
/// `PAGE_LINES_MAX`. Capped as well as defaulted: a snapshot is text an
/// assistant has to read, and a limit of a million turns "see the page"
/// into a reply nothing can use.
pub fn page_limit(asked: Option<&str>) -> usize {
    asked
        .and_then(|s| s.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(crate::browser::snapshot::PAGE_LINES_DEFAULT)
        .min(crate::browser::snapshot::PAGE_LINES_MAX)
}

/// A `within` locator from its text: JSON (an object, a chain, or a quoted
/// string), or a bare css selector. Checked as a script's locator is. A
/// css string, bare or quoted, becomes a structured css link, never a
/// legacy string target: a legacy target resolves to its first match,
/// hidden or not, so "within matched 3" could never be said of it, and a
/// hidden template could be listed in place of the dialog a person sees.
/// A structured css link counts every visible match. A `text=` string
/// becomes a text link the same way.
pub fn within_target(raw: &str) -> Result<crate::browser::locator::Target, String> {
    use crate::browser::locator::{LocatorStep, Target};
    let t = raw.trim();
    let target = if t.starts_with('{') || t.starts_with('[') || t.starts_with('"') {
        serde_json::from_str::<Target>(t).map_err(|e| format!("within is not a locator: {e}"))?
    } else {
        Target::Legacy(t.to_string())
    };
    // A `text=` string is the legacy text form: a text link, which counts
    // its visible matches too.
    let target = match target {
        Target::Legacy(s) => match s.strip_prefix("text=") {
            Some(words) => Target::One(LocatorStep { text: Some(words.trim().to_string()), ..LocatorStep::default() }),
            None => Target::One(LocatorStep { css: Some(s), ..LocatorStep::default() }),
        },
        structured => structured,
    };
    target.validate().map_err(|e| format!("within is not a locator: {e}"))?;
    Ok(target)
}

/// Said to an `nth` sent with no `within` for it to pick from.
pub const NTH_NEEDS_WITHIN: &str = "nth picks one of the elements within matches: send within with it";

/// A `within` locator from its text (`within_target`), with `nth`, when
/// given, picking one of its matches: counted from 0 as a locator's `nth`
/// is, and put on its last link, so a chain picks among its final matches.
/// `None` with no `within`. Refused: an `nth` with no `within`, one that is
/// not a count from 0, and one the `within` already gives.
pub fn within_target_nth(
    raw: Option<&str>,
    nth: Option<&str>,
) -> Result<Option<crate::browser::locator::Target>, String> {
    use crate::browser::locator::Target;
    let nth = match nth.map(str::trim).filter(|n| !n.is_empty()) {
        None => None,
        Some(n) => Some(
            n.parse::<i32>()
                .ok()
                .filter(|n| *n >= 0)
                .ok_or_else(|| format!("nth is a whole number counting from 0, not {n}"))?,
        ),
    };
    let Some(raw) = raw else {
        return match nth {
            Some(_) => Err(NTH_NEEDS_WITHIN.to_string()),
            None => Ok(None),
        };
    };
    let target = within_target(raw)?;
    let Some(n) = nth else {
        return Ok(Some(target));
    };
    let mut links = match target {
        Target::One(step) => vec![step],
        Target::Chain(steps) => steps,
        // `within_target` makes a css or text link of every string.
        Target::Legacy(_) => target.links(),
    };
    let last = links.last_mut().ok_or_else(|| "within is not a locator: a locator list is empty".to_string())?;
    if last.nth.is_some() {
        return Err("within gives an nth already: send nth once, in within or beside it".to_string());
    }
    last.nth = Some(n);
    Ok(Some(if links.len() == 1 { Target::One(links.remove(0)) } else { Target::Chain(links) }))
}

/// The supervised browser's page as text, `limit` lines at most: what
/// `/autorun-page` answers, and what a replay that reached its step hands
/// back beside its sentence. What it printed is filed in the project's
/// discovery map.
pub async fn supervised_page(organization: &str, project: &str, limit: usize) -> (u16, String) {
    supervised_page_in(SlotNo::MAIN, organization, project, limit).await
}

/// [`supervised_page`] in slot `slot`'s browser.
pub async fn supervised_page_in(slot: SlotNo, organization: &str, project: &str, limit: usize) -> (u16, String) {
    supervised_page_within_in(slot, organization, project, limit, None).await
}

/// [`supervised_page`], listing only what is inside the one element
/// `within` matches when it is given (`page_read_in_within`).
pub async fn supervised_page_within(
    organization: &str,
    project: &str,
    limit: usize,
    within: Option<&crate::browser::locator::Target>,
) -> (u16, String) {
    supervised_page_within_in(SlotNo::MAIN, organization, project, limit, within).await
}

/// [`supervised_page_within`] in slot `n`'s browser.
pub async fn supervised_page_within_in(
    n: SlotNo,
    organization: &str,
    project: &str,
    limit: usize,
    within: Option<&crate::browser::locator::Target>,
) -> (u16, String) {
    supervised_page_read_in(n, organization, project, limit, within, None).await
}

/// `get_autorun_page`'s read in slot `n`'s browser. A `within` that matches
/// several elements lists the first and says how many there were
/// (`first_of_many`). With a `query` (the lines of the whole page, or of the
/// `within` element, that contain it, case ignored, each with the lines above
/// it that it sits under) the page is read whole, nothing of it is recorded
/// as seen (the answer says so), and `limit` cuts the matching lines.
async fn supervised_page_read_in(
    n: SlotNo,
    organization: &str,
    project: &str,
    limit: usize,
    within: Option<&crate::browser::locator::Target>,
    query: Option<&str>,
) -> (u16, String) {
    if crate::autorun::replay_to::state(n).is_running() {
        return (409, replay_going_for_a_page_read(n));
    }
    // The lock is held for exactly one protocol job - whoever holds it
    // holds the browser, and the person may be using it.
    let mut slot = crate::commands::autorun::session_slot(n).lock().await;
    let Some(session) = slot.as_mut() else {
        return (409, NO_SUPERVISED_BROWSER.to_string());
    };
    let (within, many) = first_of_many(&mut session.cdp, within).await;
    let root = if project.trim().is_empty() { None } else { crate::autorun::store::configured_root() };
    let tabs_case = session.tabs_case;
    let answer = match query {
        Some(query) => {
            let (status, text, _) =
                read_and_file_within(&mut session.cdp, crate::browser::snapshot::PAGE_LINES_MAX, within.as_ref(), None).await;
            if status == 200 { (status, query_lines(&text, query, limit)) } else { (status, text) }
        }
        None => {
            page_read_in_within(session, root.as_deref(), organization, project, tabs_case, limit, within.as_ref()).await
        }
    };
    let answer = match many {
        Some(said) if answer.0 == 200 => (answer.0, format!("{said}\n{}", answer.1)),
        _ => answer,
    };
    // A browser that has gone is let go here, not kept to answer 503 again.
    let answer = let_go_if_silent(&mut slot, answer).await;
    crate::commands::autorun::publish_discovery_in(n, &slot);
    answer
}

/// A `within` that matches several elements, as the first of them (`nth` 0)
/// and the sentence that says how many matched. A `within` that already has
/// an `nth`, matches one or none, or cannot be counted is returned as it is.
pub async fn first_of_many<D: crate::browser::cdp::Driver>(
    d: &mut D,
    within: Option<&crate::browser::locator::Target>,
) -> (Option<crate::browser::locator::Target>, Option<String>) {
    use crate::browser::locator::Target;
    let Some(target) = within else { return (None, None) };
    let mut links = target.links();
    if links.is_empty() || links.last().is_some_and(|l| l.nth.is_some()) {
        return (Some(target.clone()), None);
    }
    let count = match crate::browser::locator::resolve_explained(d, target).await {
        Ok(found) => found.handles.len(),
        Err(_) => return (Some(target.clone()), None),
    };
    if count < 2 {
        return (Some(target.clone()), None);
    }
    if let Some(last) = links.last_mut() {
        last.nth = Some(0);
    }
    let first = if links.len() == 1 { Target::One(links.remove(0)) } else { Target::Chain(links) };
    (
        Some(first),
        Some(format!("within matched {count} elements: this lists the first (nth 0) - send nth to list another.")),
    )
}

/// The lines of a page read (`text`, one line each, the indent showing the
/// depth) that contain `query` (case ignored), each with the lines above it
/// that it sits under, at most `limit` of them in all. Said when nothing
/// matches, and that nothing was recorded as seen.
pub fn query_lines(text: &str, query: &str, limit: usize) -> String {
    let want = query.trim().to_lowercase();
    let lines: Vec<&str> = text.lines().collect();
    let depth = |l: &str| l.len() - l.trim_start().len();
    let mut keep = vec![false; lines.len()];
    let mut matched = 0usize;
    for (i, line) in lines.iter().enumerate() {
        if !line.to_lowercase().contains(&want) {
            continue;
        }
        matched += 1;
        keep[i] = true;
        let mut need = depth(line);
        for j in (0..i).rev() {
            if need == 0 {
                break;
            }
            if depth(lines[j]) < need {
                keep[j] = true;
                need = depth(lines[j]);
            }
        }
    }
    let said = "Recorded nothing as seen: a read with a query lists only the matching lines.";
    if matched == 0 {
        return format!("no line of the page contains \"{}\"\n{said}", query.trim());
    }
    let shown: Vec<&str> = lines.iter().zip(&keep).filter(|(_, k)| **k).map(|(l, _)| *l).collect();
    let total = shown.len();
    let mut out: Vec<String> = shown.iter().take(limit).map(|l| (*l).to_string()).collect();
    if total > limit {
        out.push(format!("{} more lines - call get_autorun_page with a larger limit or a narrower query", total - limit));
    }
    out.push(format!("{matched} lines contain \"{}\". {said}", query.trim()));
    out.join("\n")
}

/// Said at the end of a page read during a discovery that has no area yet:
/// nothing was recorded.
pub const READ_WITH_NO_AREA: &str = "Recorded nothing as seen: this discovery has no area yet. Save the area with save_autorun_area, or name it with \"area\" on discover_autorun_actions, then read the page again.";

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
    page_read_in_within(browser, root, organization, project, tabs_case, limit, None).await
}

/// [`page_read_in`], listing only the subtree of the one element `within`
/// matches when it is given, refused (400, naming the count) when it
/// matches none or several. Such a read is filed as one the limit cut
/// short (`read_and_file_within`): it never explores the page.
#[allow(clippy::too_many_arguments)]
pub async fn page_read_in_within<B: DiscoveryBrowser>(
    browser: &mut B,
    root: Option<&std::path::Path>,
    organization: &str,
    project: &str,
    tabs_case: Option<i32>,
    limit: usize,
    within: Option<&crate::browser::locator::Target>,
) -> (u16, String) {
    let p = browser.parts();
    // An explore session's read is the page text alone: nothing is filed
    // and no line about the map is added.
    if p.discovery.as_ref().is_some_and(|s| s.explore.is_some()) {
        let (status, text, _) = read_and_file_within(p.driver, limit, within, None).await;
        return (status, text);
    }
    let discovery_area = p.discovery.as_ref().map(|s| named(s.area.as_deref()));
    let at = root.and_then(|root| {
        discovery_sighting(root, organization, project, p.discovery.as_ref(), tabs_case, p.signed_in.as_deref())
    });
    let Some(area) = discovery_area else {
        let (status, text, _) = read_and_file_within(p.driver, limit, within, at.as_ref()).await;
        return (status, text);
    };
    let Some(area) = area else {
        let (status, text, _) = read_and_file_within(p.driver, limit, within, None).await;
        return if status == 200 { (status, format!("{text}\n\n{READ_WITH_NO_AREA}")) } else { (status, text) };
    };
    let (status, text, filed) = read_and_file_within(p.driver, limit, within, at.as_ref()).await;
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
    // An explore session files nothing in the discovery map.
    if discovery.is_some_and(|s| s.explore.is_some()) {
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
    read_and_file_within(d, limit, None, at).await
}

/// [`read_and_file`], listing only the subtree of the one element `within`
/// matches when it is given (`snapshot::within_node`): 400 with the count
/// when it matches none or several, and nothing filed. Each listed line is
/// counted against the whole page when every frame of it was read
/// (`discovery_map::record_read_counted`); a `within` read is never whole.
pub async fn read_and_file_within<D: crate::browser::cdp::Driver>(
    d: &mut D,
    limit: usize,
    within: Option<&crate::browser::locator::Target>,
    at: Option<&Sighting>,
) -> (u16, String, Filed) {
    let node = match within {
        None => None,
        Some(t) => match crate::browser::snapshot::within_node(d, t).await {
            Ok(Ok(node)) => Some(node),
            Ok(Err(why)) => return (400, why, Filed::Not),
            Err(e) => return (503, format!("the browser did not answer: {e}"), Filed::Not),
        },
    };
    let read = match crate::browser::snapshot::snapshot_read_within(d, limit, node).await {
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
    let recorded = crate::autorun::discovery_map::record_read_counted(
        &at.root,
        &at.org,
        &at.project,
        at.area.as_deref(),
        &crate::autorun::discovery_map::path_only(&href),
        &title,
        &read.lines,
        read.page_lines.as_deref(),
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

/// One locator to file, with how many elements it matched and how many
/// with hidden ones included (`discovery_map::NOT_ONE` when not known).
type Matched<'a> = (&'a crate::browser::locator::Target, usize, usize);

/// Files each locator in `targets` at `at` under the page at `href` (its
/// full address, cut to its path here), with how many elements it matched,
/// only when `may_record` allows that page. Says whether every locator was
/// filed.
fn record_matched_targets(at: &Sighting, href: &str, targets: &[Matched]) -> bool {
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
    for (target, matched, all) in targets {
        let area = at.area.as_deref();
        if let Err(why) = crate::autorun::discovery_map::record_matched_counts(
            &at.root, &at.org, &at.project, area, &path, target, *matched, *all, now,
        ) {
            unrecorded(&why);
            filed = false;
        }
    }
    filed
}

/// What a step's components found on the page: the locator of each
/// `wait_for` and `expect_text` a `use_component` ran that passed, with
/// how many elements it is filed as matching, as a probe would file it. An
/// `expect_text` reads one element and fails on several, so one (a plain
/// css link's hidden matches, and a legacy string's count, are not known:
/// `NOT_ONE`); a `wait_for` passes on one or several, so not known. What
/// it waited for may be gone a moment later (a toast), so this is filed
/// from the passed action itself, never a probe after it. `expanded` is
/// the step's actions as the runner ran them (`components::expand_step`),
/// paired by position with `outcomes`; only an outcome a component gave,
/// beside an action of a component, counts.
pub fn component_finds(
    expanded: &[(crate::browser::actions::Action, Option<String>)],
    outcomes: &[crate::browser::actions::ActionOutcome],
) -> Vec<(crate::browser::locator::Target, usize, usize)> {
    use crate::autorun::discovery_map::{plain_target, NOT_ONE};
    use crate::browser::actions::Action;
    let unknown = NOT_ONE as usize;
    expanded
        .iter()
        .zip(outcomes)
        .filter(|((_, from), o)| from.is_some() && o.component.is_some() && o.ok)
        .filter_map(|((action, _), _)| match action {
            Action::WaitFor { selector, .. } => Some((selector.clone(), unknown, unknown)),
            Action::ExpectText { selector, .. } => {
                let css = plain_target(selector) && selector.links().first().is_some_and(|l| l.css.is_some());
                Some(if selector.is_legacy() {
                    (selector.clone(), unknown, unknown)
                } else if css {
                    (selector.clone(), 1, unknown)
                } else {
                    (selector.clone(), 1, 1)
                })
            }
            _ => None,
        })
        .collect()
}

/// Files `component_finds` of a step that ran `actions` with `outcomes`, at
/// the page the browser is on now, under `area`: a try's, a discovery's or
/// a replay's. Nothing is filed for a step that uses no component, and a
/// map that could not be written is said in the log.
#[allow(clippy::too_many_arguments)]
pub async fn file_component_finds<D: crate::browser::cdp::Driver>(
    d: &mut D,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    area: Option<String>,
    actions: &[crate::browser::actions::Action],
    outcomes: &[crate::browser::actions::ActionOutcome],
) {
    let uses = actions.iter().any(|a| matches!(a, crate::browser::actions::Action::UseComponent { .. }));
    if !uses || project.trim().is_empty() {
        return;
    }
    let Ok((expanded, _)) = crate::autorun::components::expand_step(root, organization, project, actions) else { return };
    let finds = component_finds(&expanded, outcomes);
    if finds.is_empty() {
        return;
    }
    let at = Sighting {
        root: root.to_path_buf(),
        org: organization.to_string(),
        project: project.to_string(),
        area,
        account: None,
        discovering: None,
        policy: recording_policy(root, organization, project),
    };
    let (href, _) = page_address(d).await;
    let matched: Vec<Matched> = finds.iter().map(|(t, n, all)| (t, *n, *all)).collect();
    record_matched_targets(&at, &href, &matched);
}

/// The locators an action that WORKED must have matched at least once,
/// each with how many elements it matched. Kinds that pass while matching
/// nothing give none: `expect_hidden`, an `expect_count` of 0,
/// `expect_no_row`, a `expect_row_count` with no positive bound, and
/// `when_visible`, whose outcome does not say whether its selector was
/// there. A click, a fill and the checks that read one element fail on
/// several, so theirs matched one; an `expect_count` matched the number it
/// counted. A `wait_for` passes on one or more and its outcome does not
/// say how many, so it is left out here and probed once the try worked
/// (`try_action`), as is any legacy string target: a run takes its first
/// match, so it passes on several too, and so are the list of an
/// `expect_each` or `expect_order` and an `expect_each`'s child chain
/// (`list_targets`, `list_chain_of`), whose outcome does not say how many
/// items it read. Matched without a catch-all, so a
/// new kind will not compile until it is classified here.
fn matched_targets(action: &crate::browser::actions::Action) -> Vec<(&crate::browser::locator::Target, usize)> {
    use crate::browser::actions::Action;
    match action {
        Action::Click { selector }
        | Action::Fill { selector, .. }
        | Action::Upload { selector, .. }
        | Action::ExpectVisible { selector, .. }
        | Action::ExpectText { selector, .. }
        | Action::ExpectContainsText { selector, .. }
        | Action::ExpectAttribute { selector, .. }
        | Action::ExpectStyle { selector, .. }
        | Action::ExpectEnabled { selector, .. }
        | Action::ExpectDisabled { selector, .. }
        | Action::ExpectChecked { selector, .. }
        | Action::ExpectFocused { selector, .. } => vec![(selector, 1)],
        // Only an exact count says how many elements it saw; a bound
        // (`at_least`, `at_most`) does not.
        Action::ExpectCount { selector, equals, .. } => match equals {
            Some(n) if *n >= 1 => vec![(selector, *n as usize)],
            _ => vec![],
        },
        Action::Drag { from, to, .. } => vec![(from, 1), (to, 1)],
        Action::ExpectPosition { selector, relative_to, .. } => vec![(selector, 1), (relative_to, 1)],
        // The clicked element, and the region when it names one: a pass
        // read each as one element.
        Action::ExpectNoChange { selector, region, .. } => {
            std::iter::once((selector, 1)).chain(region.as_ref().map(|r| (r, 1))).collect()
        }
        Action::ExpectRow { table, .. } | Action::ExpectSorted { table, .. } => vec![(table, 1)],
        Action::ExpectRowCount { table, equals, at_least, .. } => {
            if equals.is_some_and(|n| n >= 1) || at_least.is_some_and(|n| n >= 1) {
                vec![(table, 1)]
            } else {
                vec![]
            }
        }
        Action::ExpectHidden { .. }
        | Action::WaitFor { .. }
        | Action::ExpectEach { .. }
        | Action::ExpectOrder { .. }
        | Action::UseComponent { .. }
        | Action::ExpectNoRow { .. }
        | Action::WhenVisible { .. }
        | Action::RepeatWhileVisible { .. }
        | Action::Navigate { .. }
        | Action::CheckText { .. }
        | Action::CheckUrl { .. }
        | Action::RememberUrl { .. }
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
        | Action::ExpectDialog { .. }
        // It may pass without ever clicking `next`, and says nothing of how
        // many elements an `until` locator matched on the page it was seen.
        | Action::PageUntil { .. } => vec![],
    }
}

/// The list an `expect_each` or `expect_order` reads: probed once the try
/// passed (`try_action`).
fn list_targets(action: &crate::browser::actions::Action) -> Vec<&crate::browser::locator::Target> {
    use crate::browser::actions::Action;
    match action {
        Action::ExpectEach { selector, .. } | Action::ExpectOrder { selector, .. } if !selector.is_legacy() => vec![selector],
        _ => vec![],
    }
}

/// An `expect_each`'s child as it is looked for inside the list: the list's
/// links and then its own, the chain the save needs seen whole.
fn list_chain_of(action: &crate::browser::actions::Action) -> Option<crate::browser::locator::Target> {
    match action {
        crate::browser::actions::Action::ExpectEach { selector, child: Some(child), .. } if !selector.is_legacy() => {
            Some(crate::browser::locator::Target::Chain(selector.links().into_iter().chain(child.links()).collect()))
        }
        _ => None,
    }
}

/// A probe's answer read back (`snapshot::probe`): how many elements
/// matched (`"matches: N ..."`), how many of them it listed (at most ten),
/// and how many of those listed are visible (a line `<tag> "<text>" visible
/// at ...`).
struct ProbeAnswer {
    matches: usize,
    listed: usize,
    visible: usize,
}

fn read_probe_answer(text: &str) -> ProbeAnswer {
    let matches = text
        .strip_prefix("matches: ")
        .map(|rest| rest.chars().take_while(char::is_ascii_digit).collect::<String>())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    // After the text's closing quote, which the text itself cannot follow;
    // the name the page read gives the element, after it, is cut off first.
    let shown = |line: &str| {
        let line = match line.rfind(crate::browser::snapshot::PAGE_READ_NAME) {
            Some(i) if line.ends_with('"') => &line[..i],
            _ => line,
        };
        line.rfind('"').map(|i| &line[i + 1..]).is_some_and(|rest| rest.starts_with(" visible "))
    };
    let listed = matches.min(10);
    let visible = text.lines().skip(1).take(listed).filter(|l| shown(l)).count();
    ProbeAnswer { matches, listed, visible }
}

/// How many visible elements a probe of a structured locator says
/// matched. `None` when none of them is visible, which files nothing. One
/// visible beside any number of hidden ones is one, as a run finds only
/// visible ones. When the probe matched more than it lists and one listed
/// is visible, the ones it left out may be visible too, so that is at
/// least two, never one.
pub fn visible_matches(text: &str) -> Option<usize> {
    let answer = read_probe_answer(text);
    if answer.visible == 0 {
        return None;
    }
    if answer.matches > answer.listed {
        return Some(answer.visible.max(2));
    }
    Some(answer.visible)
}

/// What a probe of `target` (its answer `text`) files the target as
/// matching: how many elements, and how many with hidden ones included.
/// `None` files nothing.
///
/// A legacy string target resolves to at most one element (`LEGACY_JS`
/// keeps the first css match or the last `text=` one), so its probe never
/// shows more than one: it is counted for real with `LEGACY_COUNT_JS`,
/// hidden ones included, as a run does not leave them out. It is filed
/// only when the element a run takes is visible.
///
/// A structured locator files its visible matches (`visible_matches`).
/// When it is one plain css link, whose key a legacy string shares, the
/// hidden ones are counted too by probing it with them included, so a
/// legacy use of that key is held to every match; any other files the
/// visible count for both.
async fn counted<D: crate::browser::cdp::Driver>(
    d: &mut D,
    target: &crate::browser::locator::Target,
    text: &str,
) -> Option<(usize, usize)> {
    use crate::autorun::discovery_map::{plain_target, NOT_ONE};
    if let crate::browser::locator::Target::Legacy(sel) = target {
        if read_probe_answer(text).visible == 0 {
            return None;
        }
        let all = crate::browser::locator::legacy_count(d, sel).await.ok().filter(|n| *n > 0)?;
        return Some((all, all));
    }
    let matched = visible_matches(text)?;
    let css = plain_target(target) && target.links().first().is_some_and(|l| l.css.is_some());
    if !css {
        return Some((matched, matched));
    }
    let all = match crate::browser::snapshot::probe_exact(d, &target.including_hidden()).await {
        Ok(t) => read_probe_answer(&t).matches.max(matched),
        Err(_) => NOT_ONE as usize,
    };
    Some((matched, all))
}

/// What a locator matches on the page, as the probe answers it; a locator
/// that matched at least one visible element is filed at `at`, with how
/// many visible ones it matched (`visible_matches`).
pub async fn probe_page<D: crate::browser::cdp::Driver>(
    d: &mut D,
    target: &crate::browser::locator::Target,
    at: Option<&Sighting>,
) -> (u16, String) {
    let text = match crate::browser::snapshot::probe(d, target).await {
        Ok(text) => text,
        Err(e) => return (503, format!("the browser did not answer: {e}")),
    };
    if let Some(at) = at {
        // Filed as a run takes the target (a plain string: its first match), not as shown.
        let as_run = match target {
            crate::browser::locator::Target::Legacy(_) => crate::browser::snapshot::probe_exact(d, target).await.unwrap_or_else(|_| text.clone()),
            _ => text.clone(),
        };
        match counted(d, target, &as_run).await {
            Some((matched, all)) => {
                let (href, _) = page_address(d).await;
                record_matched_targets(at, &href, &[(target, matched, all)]);
            }
            // Nothing seen: what no page read can print of it is taken off
            // this page (`discovery_map::record_matched_counts` with 0).
            None if read_probe_answer(&as_run).visible == 0 => {
                let (href, _) = page_address(d).await;
                record_matched_targets(at, &href, &[(target, 0, 0)]);
            }
            None => {}
        }
    }
    (200, text)
}

/// How a locator recorded on the page is named in a save's answer: as it
/// describes itself, with how many elements it matched when that was more
/// than one.
fn recorded_as(described: &str, matched: usize) -> String {
    if matched > 1 {
        format!("{described} (matched {matched} elements)")
    } else {
        described.to_string()
    }
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
/// that matches one or more visible elements is recorded under the
/// discovery's current area, with how many it matched. Whether that is
/// enough for the save is the seen check's to say: a check that may match
/// several takes it, any other use needs one (`seen_check::takes_many`).
/// Hands back the locators recorded, as each describes itself, with
/// "(matched N elements)" after one that matched several; `None` when no
/// discovery is going, and then nothing is probed.
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
        let Ok(text) = crate::browser::snapshot::probe_exact(p.driver, target).await else {
            unchecked += 1;
            continue;
        };
        let Some((matched, all)) = counted(p.driver, target, &text).await else {
            continue;
        };
        let Some(at) = at.as_ref() else {
            continue;
        };
        let (href, _) = page_address(p.driver).await;
        if record_matched_targets(at, &href, &[(target, matched, all)]) {
            recorded.push(recorded_as(&described, matched));
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
    n: SlotNo,
) -> Option<Vec<String>> {
    if unattended_run_is_using_the_browser().is_some() || crate::autorun::replay_to::state(n).is_running() {
        return None;
    }
    let mut slot = crate::commands::autorun::session_slot(n).lock().await;
    record_refused_for_scripts_in(&mut slot, root, &ctx.org, &ctx.project, script_areas, targets).await
}

/// A save's own answer, after the line that says what a refusal checked on
/// the page recorded.
pub fn after_recording(recorded: &[String], (status, text): (u16, String)) -> (u16, String) {
    let line = if recorded.is_empty() {
        "Recorded on the current page: nothing - no refused locator matched a visible element.".to_string()
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

/// What a probe body's `selector` may be.
const PROBE_SHAPE: &str = "{ \"selector\": <a script's target: a CSS string, one locator object, or a list of them (a chain, outermost first)> }";

/// The locator a probe names: a string, one locator object, or the chain
/// form scripts use, a list of links (outermost first). A string holding
/// such a list or object as JSON text - how a client whose schema allowed
/// only a string or an object sent a chain - is read as that list or
/// object, never as CSS: a selector that is valid JSON of either shape is
/// not valid CSS. `Err` is the refusal's sentence.
pub fn probe_target(selector: serde_json::Value) -> Result<crate::browser::locator::Target, String> {
    let selector = match selector {
        serde_json::Value::String(text) => match serde_json::from_str::<serde_json::Value>(text.trim()) {
            Ok(v @ (serde_json::Value::Array(_) | serde_json::Value::Object(_))) => v,
            _ => serde_json::Value::String(text),
        },
        other => other,
    };
    let target: crate::browser::locator::Target =
        serde_json::from_value(selector).map_err(|e| format!("that is not a locator: {e}"))?;
    target.validate()?;
    if target.links().iter().filter_map(|l| l.css.as_deref()).any(is_playwright_selector) {
        return Err(NOT_A_PLAYWRIGHT_SELECTOR.to_string());
    }
    Ok(target)
}

/// Said, with 400, to a probe whose CSS holds a Playwright pseudo-selector.
pub const NOT_A_PLAYWRIGHT_SELECTOR: &str =
    "that is a Playwright selector, which Auto Run does not take: use a CSS selector or a role and name (see get_guide topic autorun, Selectors).";

/// Is `css` written with a Playwright-only selector (`:text-is`,
/// `:has-text`, `:text(`, or the `>>` chain), which no browser's CSS takes?
fn is_playwright_selector(css: &str) -> bool {
    let lower = css.to_ascii_lowercase();
    [":text-is", ":has-text", ":text(", ">>"].iter().any(|p| lower.contains(p))
}

/// Said, with 409, to a page read, a probe, a try or the assistant's End
/// discovery while a replay holds the browser: it holds the session's lock
/// for its whole run, so waiting on it could outlast the call.
pub const REPLAY_GOING: &str =
    "a replay is going - call replay_autorun_to_step with the same case and step to wait for it";

/// Said, with 409, to a page read while a replay holds the browser. The
/// page is not read beside the replay: the replay holds the browser's one
/// connection for its whole run, and that connection is what answers the
/// page's dialogs and the requests a no-save guard pauses, so a second
/// reader would either wait out the replay or act behind it. The replay's
/// own call says where it is, and hands back the page once it stands
/// before its step. It is slot `n`'s replay that is going, and slot `n`'s
/// run that names the case and step.
pub fn replay_going_for_a_page_read(n: SlotNo) -> String {
    let call = call_the_replay_going(n);
    format!(
        "the page cannot be read while a replay drives the browser - {call}: it waits for the replay, says the step and the action it is on, and hands back the page once the browser stands before the step"
    )
}

/// How to wait on the replay going: the assistant's own names its case and
/// step; the person's, or one not known, says "the same case and step".
/// Read from the run kept in slot `n`, the one whose replay is going.
fn call_the_replay_going(n: SlotNo) -> String {
    let which = assistant_run(n).as_ref().filter(|r| r.answer.borrow().is_none()).map(|r| (r.case_id, r.step));
    match which {
        Some((case_id, step)) => format!("call replay_autorun_to_step with case {case_id} and step {step}"),
        None => "call replay_autorun_to_step with the same case and step".to_string(),
    }
}

/// Said, with 409, to the assistant's release while a replay holds the
/// browser: a release never stops a replay, so it says how to wait for it.
pub fn replay_going_for_a_release(n: SlotNo) -> String {
    let stop = if assistant_replay_going(n) { ", or replay_autorun_to_step with stop: true to stop it" } else { "" };
    format!(
        "a replay is going and holds the browser, and a release never stops one - {} to wait for it{stop}, then release",
        call_the_replay_going(n)
    )
}

/// Whether the replay going in slot `n` is the assistant's own: its call's
/// slot holds no answer yet. Only then does a refusal point at the replay
/// tool's `stop`, which never stops the person's.
pub fn assistant_replay_going(n: SlotNo) -> bool {
    assistant_run(n).as_ref().is_some_and(|r| r.answer.borrow().is_none())
}

/// The case and step, `(case_id, step)`, of the assistant's replay kept in
/// slot `n` - running, or ended with its answer not yet taken - if there is
/// one.
pub fn assistant_replay_of(n: SlotNo) -> Option<(i32, i32)> {
    assistant_run(n).as_ref().map(|r| (r.case_id, r.step))
}

/// `commands::autorun::REPLAY_RUNNING_AT_RESET`, as the assistant's reset,
/// cookie and header tools say it: naming the replay tool's `stop` when the
/// replay going is the assistant's own.
pub fn replay_running_refusal(n: SlotNo) -> String {
    let said = crate::commands::autorun::REPLAY_RUNNING_AT_RESET;
    if assistant_replay_going(n) {
        format!("{said} Or stop it first with replay_autorun_to_step and stop: true.")
    } else {
        said.to_string()
    }
}

/// `REPLAY_GOING` while a replay to a step holds slot `n`'s browser.
pub fn replay_is_holding_the_browser(n: SlotNo) -> Option<(u16, String)> {
    crate::autorun::replay_to::state(n).is_running().then(|| (409, REPLAY_GOING.to_string()))
}

/// What a locator matches on the open page right now.
async fn autorun_probe(ctx: &BridgeContext, body: &str, n: SlotNo) -> (u16, String) {
    let selector = match body_field(body, "selector", PROBE_SHAPE) {
        Ok(v) => v,
        Err(refused) => return refused,
    };
    let target = match probe_target(selector) {
        Ok(t) => t,
        Err(why) => return (400, why),
    };
    if let Some(busy) = unattended_run_is_using_the_browser() {
        return busy;
    }
    if let Some(going) = replay_is_holding_the_browser(n) {
        return going;
    }
    let mut slot = crate::commands::autorun::session_slot(n).lock().await;
    let Some(session) = slot.as_mut() else {
        return (409, NO_SUPERVISED_BROWSER.to_string());
    };
    let at = supervised_sighting(session, &ctx.org, &ctx.project);
    let answer = probe_page(&mut session.cdp, &target, at.as_ref()).await;
    let answer = let_go_if_silent(&mut slot, answer).await;
    crate::commands::autorun::publish_discovery_in(n, &slot);
    answer
}

/// What an inspect body carries.
const INSPECT_SHAPE: &str = "{ \"locator\": <a script's target: a CSS string, one locator object, or a list of them (a chain, outermost first)>, \"show\"?: [\"styles\", \"listeners\", \"animations\", \"structure\"], \"properties\"?: [<CSS property names>] }";

/// Said to an inspect whose locator still waits for a value.
pub const INSPECT_PLACEHOLDER: &str =
    "a locator with a placeholder ({{...}}) or a component input cannot be inspected as written - put in the value it stands for";

/// The locator and what to show, out of an inspect's body: everything that
/// can be refused without the browser.
fn inspect_ask(body: &str) -> Result<(crate::browser::locator::Target, crate::browser::inspect::Ask), (u16, String)> {
    let locator = body_field(body, "locator", INSPECT_SHAPE)?;
    let target = probe_target(locator).map_err(|why| (400, why))?;
    if !probeable(&target) {
        return Err((400, INSPECT_PLACEHOLDER.to_string()));
    }
    let args: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    let ask = crate::browser::inspect::read_ask(&args).map_err(|why| (400, why))?;
    Ok((target, ask))
}

/// `probe_autorun_locator` with `show` on the page `d` shows: the body read
/// (`inspect_ask`), then the one element its locator matches inspected
/// (`browser::inspect`). Read only.
pub async fn inspect_page<D: crate::browser::cdp::Driver>(d: &mut D, body: &str) -> (u16, String) {
    match inspect_ask(body) {
        Ok((target, ask)) => inspected(d, &target, &ask).await,
        Err(refused) => refused,
    }
}

async fn inspected<D: crate::browser::cdp::Driver>(
    d: &mut D,
    target: &crate::browser::locator::Target,
    ask: &crate::browser::inspect::Ask,
) -> (u16, String) {
    match crate::browser::inspect::inspect(d, target, ask).await {
        Ok(v) => (200, v.to_string()),
        Err(refused) => refused,
    }
}

/// `/autorun-inspect`: one element in the supervised browser, as a probe
/// reaches it - any supervised browser, the person's too, since nothing is
/// changed. Refused at once while an unattended run or a replay holds it.
async fn autorun_inspect(body: &str, n: SlotNo) -> (u16, String) {
    let (target, ask) = match inspect_ask(body) {
        Ok(asked) => asked,
        Err(refused) => return refused,
    };
    if let Some(busy) = unattended_run_is_using_the_browser() {
        return busy;
    }
    if let Some(going) = replay_is_holding_the_browser(n) {
        return going;
    }
    let mut slot = crate::commands::autorun::session_slot(n).lock().await;
    let Some(session) = slot.as_mut() else {
        return (409, NO_SUPERVISED_BROWSER.to_string());
    };
    let answer = inspected(&mut session.cdp, &target, &ask).await;
    let answer = let_go_if_silent(&mut slot, answer).await;
    // A browser let go above may have ended a discovery: the window hears.
    crate::commands::autorun::publish_discovery_in(n, &slot);
    // Never the locator: it is the assistant's writing and can hold an address.
    crate::applog::info(format!("AI inspected an element in the supervised browser: {}", answer.0));
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
        // The words it looks for (a path or a name), never a noted address.
        Action::CheckUrl { contains, equals, same_as, .. } => equals
            .as_deref()
            .map(crate::autorun::discovery_map::path_only)
            .or_else(|| same_as.clone())
            .unwrap_or_else(|| contains.clone()),
        Action::RememberUrl { name } => name.clone(),
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
        | Action::ExpectStyle { selector, .. }
        | Action::ExpectEnabled { selector, .. }
        | Action::ExpectDisabled { selector, .. }
        | Action::ExpectChecked { selector, .. }
        | Action::ExpectFocused { selector, .. }
        | Action::ExpectEach { selector, .. }
        | Action::ExpectOrder { selector, .. }
        | Action::ExpectPosition { selector, .. }
        | Action::ExpectNoChange { selector, .. }
        | Action::Upload { selector, .. }
        | Action::WhenVisible { selector, .. }
        | Action::RepeatWhileVisible { selector, .. } => selector.describe(),
        // The control that pages: what the walk is about.
        Action::PageUntil { next, .. } => next.describe(),
    };
    let target = if what.is_empty() { String::new() } else { format!(" {what}") };
    format!("{kind}{target}")
}

/// What no action an assistant runs may be: one read from its body by
/// `/autorun-try` or `/autorun-discover-action`.
pub(crate) fn refuse_as_a_tried_action(action: &crate::browser::actions::Action) -> Result<(), (u16, String)> {
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

/// One action of a try or a discovery, read from the assistant's JSON. A
/// `wait_for` with no `timeout_ms` gets the discovery's own wait, and a wait
/// above the ceiling is lowered (`action_help`); `notes` say which. What is
/// not an action is refused with the field it expected and an example. A
/// saved script is read elsewhere and never gets either.
fn tried_action_from(mut raw: serde_json::Value) -> Result<(crate::browser::actions::Action, Vec<String>), String> {
    let default_ms = u32::try_from(crate::browser::timing::Timing::supervised().expect_ms).unwrap_or(u32::MAX);
    let notes = crate::autorun::action_help::prepare_waits(&mut raw, default_ms);
    match serde_json::from_value::<crate::browser::actions::Action>(raw.clone()) {
        Ok(action) => Ok((action, notes)),
        Err(e) => Err(crate::autorun::action_help::refusal_for(&raw, &e.to_string())),
    }
}

/// `answer`, with what was done to the action's waits said in it: a `waits`
/// field of a JSON answer, else a line before the text.
fn with_wait_notes(answer: (u16, String), notes: &[String]) -> (u16, String) {
    if notes.is_empty() {
        return answer;
    }
    let (status, text) = answer;
    let said = notes.join("; ");
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(mut v @ serde_json::Value::Object(_)) => {
            v["waits"] = serde_json::json!(said);
            (status, v.to_string())
        }
        _ => (status, format!("Waits: {said}.\n{text}")),
    }
}

/// Run ONE action against the open page, so an assistant can find out
/// whether a repair works before it writes the repair down.
async fn autorun_try(
    ctx: &BridgeContext,
    body: &str,
    seen: &mut Option<crate::autorun::overrides::Overrides>,
    n: SlotNo,
) -> (u16, String) {
    let raw = match body_field(body, "action", "{ \"action\": <one script action> }") {
        Ok(v) => v,
        Err(refused) => return refused,
    };
    if let Some(r) = crate::autorun::forbidden_fixes::forbidden_in(&raw) {
        return (400, r);
    }
    let (action, wait_notes) = match tried_action_from(raw) {
        Ok(read) => read,
        Err(refused) => return (400, refused),
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
    if let Some(going) = replay_is_holding_the_browser(n) {
        return going;
    }
    // Everything that can fail without the browser is settled BEFORE the
    // lock: whoever holds it holds the browser the person is using, and
    // a session held open to look up a path is a session held for no
    // reason.
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let mut slot = crate::commands::autorun::session_slot(n).lock().await;
    let Some(session) = slot.as_mut() else {
        return (409, NO_SUPERVISED_BROWSER.to_string());
    };
    // A try uses the browser: no replay carries on from where it stood, and
    // no watched step names what happened before it.
    session.used_otherwise();
    crate::commands::autorun::note_session_use_in(n);
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
    // An explore session files nothing (`files_finds` is false), and notes the
    // saves the try sent, as its actions do.
    let explore_session = session.discovery.as_ref().and_then(|s| s.explore_session()).map(str::to_string);
    // An action that never answers ends inside `try_action`, as a failed
    // action; the call as a whole is bounded by `CALL_BUDGET_MS`.
    let answer = crate::autorun::action_help::CALL_STARTED
        .scope(
            std::time::Instant::now(),
            try_in_session(
                &mut session.cdp,
                &mut session.tabs_case,
                &mut session.account,
                &mut session.lease,
                &root,
                &ctx.org,
                &ctx.project,
                case_id,
                discovery_area.as_deref(),
                explore_session.as_deref(),
                &action,
            ),
        )
        .await;
    let answer = with_wait_notes(answer, &wait_notes);
    let answer = let_go_if_silent(&mut slot, answer).await;
    crate::commands::autorun::publish_discovery_in(n, &slot);
    *seen = Some(crate::commands::autorun::overrides_of(&slot));
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
    try_in_area(d, account, lease, root, organization, project, case_id, discovery_area, true, action).await
}

/// `try_for_case` for a try in a discovery's browser: in an explore session
/// (`explore_session`) nothing the try acted on is filed in the discovery
/// map, and the saves it sent are noted on the session's list of saves (by
/// method and path, with the `handler`) and named at the end of the answer, as
/// the session's actions do. Any other try is `try_for_case`.
#[allow(clippy::too_many_arguments)]
pub async fn try_in_session<D: crate::browser::cdp::Driver>(
    d: &mut D,
    tabs_case: &mut Option<i32>,
    account: &mut Option<String>,
    lease: &mut crate::autorun::lease::Held,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    case_id: i32,
    discovery_area: Option<&str>,
    explore_session: Option<&str>,
    action: &crate::browser::actions::Action,
) -> (u16, String) {
    crate::autorun::runner::tabs_for_case(d, tabs_case, case_id).await;
    let mark = explore_session.map(|_| crate::browser::cdp::Driver::net_mark(d));
    let mut answer =
        try_in_area(d, account, lease, root, organization, project, case_id, discovery_area, explore_session.is_none(), action)
            .await;
    if let (Some(mark), Some(session)) = (mark, explore_session) {
        let sent = crate::browser::cdp::Driver::net_since(d, mark);
        crate::browser::cdp::Driver::net_unmark(d, mark);
        let saves: Vec<(String, String)> = sent
            .iter()
            .filter(|s| crate::browser::save_guard::SAVE_METHODS.contains(&s.method.as_str()))
            .filter(|s| !crate::browser::save_guard::stopped_by_guard(s))
            .map(|s| (s.method.clone(), s.path_query.clone()))
            .collect();
        if let Err(why) = crate::autorun::explore::note_writes(root, session, &saves) {
            crate::applog::warn(format!("Auto Run explore session: its saves could not be noted: {why}"));
        }
        if !saves.is_empty() {
            let lines: Vec<String> =
                saves.iter().map(|(m, p)| crate::autorun::explore::request_line(m, p)).collect();
            answer.1.push_str(&format!("\nSaves it sent: {}.", lines.join(", ")));
        }
    }
    answer
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
    /// The case whose no-save guard the browser holds, and who the guard
    /// belongs to (`commands::autorun::GuardOwner`).
    pub guarded_case: &'a mut Option<i32>,
    pub guard_owner: &'a mut Option<crate::commands::autorun::GuardOwner>,
    /// What the assistant changed for testing in this browser
    /// (`autorun::overrides`). `None` for a browser that keeps no record
    /// (a test's fake): nothing may be changed in it.
    pub overrides: Option<&'a mut crate::autorun::overrides::Overrides>,
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
    /// Whether the assistant opened this browser (its replay, or a
    /// discovery): its release may close it, and its discovery may take it
    /// over. A test's fake is the person's unless it says otherwise.
    fn opened_by_assistant(&mut self) -> bool {
        false
    }
    /// A discovery took this browser over from the replay that left it
    /// (`make_way_for_discovery`), or the assistant's reset cleaned it
    /// (`commands::autorun::reset_in_for_assistant`): no replay carries on
    /// from where it stood, and no watched step names what happened before.
    /// Nothing to forget in a test's fake unless it counts it.
    fn taken_over(&mut self) {}
    /// Which browser this is (Edge or Chrome), when known: a discovery that
    /// names the other one does not take it over. A test's fake says none.
    fn browser(&mut self) -> Option<crate::browser::launch::Browser> {
        None
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
    finish_explore(slot);
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

/// What `make_way_for_discovery` did with the browser `slot` held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MadeWay {
    /// None was held (or one that had gone was let go): the discovery opens
    /// a new one.
    Free,
    /// The assistant's replay had left one open: the discovery took it over,
    /// signed in as it was.
    TookOver,
    /// The replay's browser kept a guard it would not lift, so it was closed
    /// through its normal close: the discovery opens a new one.
    Closed,
}

/// Where a guarded discovery reads its save words from (`nav::load_nav`):
/// the project it explores. `make_way_for_discovery` arms the discovery's
/// guard with them when it takes a browser over.
#[derive(Debug, Clone, Copy)]
pub struct GuardFrom<'a> {
    pub root: &'a std::path::Path,
    pub organization: &'a str,
    pub project: &'a str,
}

/// Before a discovery opens in `slot`: a held browser that has gone is let
/// go first (`forget_gone_browser`). A live one the assistant's replay left
/// open (opened by the assistant, no discovery in it, no guard of the
/// person's) is taken over, sign-in and all: its tabs other than `main` are
/// closed, the dialogs the replay expected or saw are forgotten, and the
/// replay's stand and last page change too (`DiscoveryBrowser::taken_over`).
/// Its guard is never lifted here. A discovery that will be guarded
/// (`guarded`: it must not save, or is a mapping run, with where its save
/// words are) has its guard on before this returns, still under the lock
/// the caller took: the replay's guard taken over as it is, or - with none
/// on - the discovery's own armed now (`guard_a_no_save_discovery`), its
/// owner the discovery's either way. So no call that takes the lock between
/// the takeover and the start (`discover_start_in`) finds the browser
/// unguarded with a no_save discovery on record. A guard that will not go
/// on refuses the discovery with its sentence, the browser left as the
/// replay left it. For any other discovery, the replay's guard stays until
/// the start has decided the sign-in. One that is another browser than the
/// one `wanted` (Edge or Chrome) is closed through its normal close
/// instead, and a new one opened. Any other live browser - the person's, or
/// a discovery's - refuses it with the busy sentence (`refuse_while_held`),
/// untouched. A replay going never reaches here
/// (`refuse_discovery_while_busy`).
pub async fn make_way_for_discovery<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    wanted: Option<crate::browser::launch::Browser>,
    guarded: Option<GuardFrom<'_>>,
) -> Result<MadeWay, String> {
    use crate::commands::autorun::GuardOwner;
    forget_gone_browser(slot).await;
    let left_open = slot.as_mut().is_some_and(|b| {
        b.opened_by_assistant() && {
            let p = b.parts();
            p.discovery.is_none() && *p.guard_owner != Some(GuardOwner::Person)
        }
    });
    if !left_open {
        return refuse_while_held(slot).await.map(|()| MadeWay::Free);
    }
    let other_browser = match (wanted, slot.as_mut().and_then(|b| b.browser())) {
        (Some(wanted), Some(open)) => wanted != open,
        _ => false,
    };
    if other_browser {
        close_browser_in(slot);
        crate::applog::info("Auto Run: a discovery asked for another browser than the one the assistant's replay left open, so it closed that one");
        return Ok(MadeWay::Closed);
    }
    if let Some(b) = slot.as_mut() {
        let p = b.parts();
        // First the guard, before the browser is touched: one that will not
        // go on leaves it as the replay left it.
        if let Some(from) = guarded {
            guard_a_no_save_discovery(p.driver, true, from.root, from.organization, from.project).await?;
            *p.guarded_case = None;
            crate::commands::autorun::claim_guard(p.guard_owner, GuardOwner::Discovery);
        }
        crate::browser::cdp::Driver::close_other_tabs(p.driver).await;
        // Nothing the replay expected of a dialog, or saw, is the discovery's.
        if let Some(book) = crate::browser::cdp::Driver::dialog_book(p.driver) {
            book.disarm();
            book.take_seen();
        }
        b.taken_over();
    }
    crate::applog::info("Auto Run: a discovery took over the browser the assistant's replay left open");
    Ok(MadeWay::TookOver)
}

/// The guard a call of the assistant's left on a browser a discovery took
/// over (`GuardOwner::Call`), lifted once the discovery's start has decided
/// its sign-in: after going home in a browser already signed in, right
/// before the sign-in otherwise. A discovery's own guard, or the person's,
/// is not this to lift. One the browser will not lift stays on, still the
/// call's: no discovery call lifts it as it ends (each lifts only the guard
/// it put on itself), so it stays until a later replay lifts it (or a reset
/// or release does). Saves stay stopped meanwhile, never let through.
async fn lift_a_calls_guard<D: crate::browser::cdp::Driver>(
    d: &mut D,
    guarded_case: &mut Option<i32>,
    owner: &mut Option<crate::commands::autorun::GuardOwner>,
) {
    if !d.is_guarding_saves() || !matches!(owner, None | Some(crate::commands::autorun::GuardOwner::Call { .. })) {
        return;
    }
    if !crate::commands::autorun::lift_guard_of(d, guarded_case, owner).await {
        crate::applog::warn("Auto Run discovery: the replay's guard would not lift yet; it stays on until a later replay lifts it");
    }
}

/// Go home in a browser a discovery took over already signed in
/// (`nav::load_home`, which keeps the cookies and waits for the signed-in
/// marker), so the replay's page is never filed under the discovery's area.
/// False when it did not arrive: the discovery then signs in.
pub(crate) async fn went_home<D: crate::browser::cdp::Driver>(
    d: &mut D,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    timing: &crate::browser::timing::Timing,
) -> bool {
    let recipe = match crate::autorun::recipe::load_effective_recipe(root, organization, project) {
        Ok(recipe) => recipe,
        Err(_) => return false,
    };
    let out = crate::autorun::nav::load_home(d, &crate::autorun::nav::Home::of(&recipe), timing).await;
    if !out.ok {
        crate::applog::info(format!("Auto Run discovery: going home in the kept browser failed ({}); it signs in", out.detail));
    }
    out.ok
}

/// Put before what a discovery start answers when it first closed the
/// browser the assistant's replay left open (`MadeWay::Closed`).
pub const CLOSED_LEFT_OPEN: &str = "Closed the browser your replay left open.";

/// Put before what a discovery start answers when it took over the browser
/// the assistant's replay left open (`MadeWay::TookOver`).
pub const TOOK_OVER_LEFT_OPEN: &str = "Took over the browser your replay left open (its other tabs closed).";

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

/// Said to a start that asks to explore and to map at once.
pub const EXPLORE_NOT_MAPPING: &str = "explore and mapping are two different runs - send one of them";

/// Said when an explore session's change list could not be made at its
/// start: the session did not start.
pub const EXPLORE_NOT_STARTED: &str = "the explore session's change list could not be started (see Settings -> Logs), so the session did not start and its browser is closed";

/// Whether an argument is true, as a boolean or as the text "true".
fn said_true(v: &serde_json::Value) -> bool {
    v.as_bool() == Some(true) || v.as_str().map(|s| s.trim().eq_ignore_ascii_case("true")) == Some(true)
}

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
/// path, page}`, the path with no host or query. A browser it took over
/// from the assistant's replay that is still signed in as that account -
/// the same checks a replay makes (`replay_to::signed_in_here`: the lease,
/// the sign-in's cookies, a session not refused, the signed-in marker) -
/// keeps its sign-in and goes home (`went_home`,
/// `discovery_signed_in_already`); any other is signed in as it. The discovery takes the
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
        let kept = crate::autorun::replay_to::signed_in_here(
            p.driver,
            root,
            organization,
            project,
            key,
            &*p.signed_in,
            &*p.lease,
            timing,
        )
        .await
            && went_home(p.driver, root, organization, project, timing).await
            // Going home may have re-issued a cookie of the same sign-in.
            && crate::autorun::replay_to::retake_sign_in_for(p.driver, root, organization, project, p.lease, "the home page")
                .await
                .is_ok();
        lift_a_calls_guard(p.driver, p.guarded_case, p.guard_owner).await;
        let signed = if kept {
            crate::applog::info(format!("Auto Run discovery as {key}: signed in already, the sign-in is kept"));
            Ok(discovery_signed_in_already(key))
        } else {
            match crate::autorun::signin::sign_in_leased(
                p.driver,
                root,
                organization,
                project,
                key,
                p.lease,
                p.signed_in,
                timing,
            )
            .await
            {
                Err(why) => Err(why),
                Ok(out) if !out.ok => Err(out.detail),
                Ok(out) => Ok(out.detail),
            }
        };
        let signed_at = std::time::Instant::now();
        match signed {
            Err(why) => why,
            Ok(detail) => {
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
                // An explore session keeps its change list in a file of its
                // own, made now that the sign-in is known: one that cannot be
                // made closes the browser, as a sign-in that failed does.
                let (explore, explore_failed) = match p.discovery.as_mut().and_then(|s| s.explore.take()) {
                    None => (None, None),
                    Some(mut run) => match crate::autorun::explore::begin(root, organization, project, key, started_at) {
                        Ok(id) => {
                            run.session = Some(id);
                            // After the sign-in: its own answer is never
                            // what verifies a create.
                            run.began_ms = crate::autorun::sessions::now_ms();
                            (Some(run), None)
                        }
                        Err(why) => {
                            crate::applog::warn(format!("Auto Run explore session could not start: {why}"));
                            (None, Some(EXPLORE_NOT_STARTED.to_string()))
                        }
                    },
                };
                let (case_id, no_save) = p.discovery.as_ref().map_or((None, false), |s| (s.case_id, s.no_save));
                let state = crate::commands::autorun::DiscoveryState {
                    area,
                    account: Some(key.to_string()),
                    started_at,
                    tried,
                    mapping,
                    case_id,
                    no_save,
                    call_no_save: false,
                    call_case_id: None,
                    explore,
                    carried: Vec::new(),
                };
                let must_not_save = discovery_must_not_save(root, &state);
                *p.discovery = Some(crate::commands::autorun::DiscoveryState { no_save: must_not_save, ..state });
                // A mapping run saves nothing, and neither does a discovery
                // for a case that must not save: the guard goes on once the
                // sign-in (which itself sends a form) has arrived, and before
                // the page is touched again. A guard that will not go on
                // closes the browser: neither ever goes unguarded.
                let guarded = match (explore_failed, guard_a_mapping_run(p.driver, is_mapping, root, organization, project).await) {
                    (Some(why), _) => Err(why),
                    (None, Ok(())) => guard_a_no_save_discovery(p.driver, must_not_save, root, organization, project).await,
                    (None, Err(why)) => Err(why),
                };
                if let Err(why) = guarded {
                    why
                } else {
                    // Both guards are the discovery's, on until it ends.
                    if is_mapping || must_not_save {
                        crate::commands::autorun::claim_guard(p.guard_owner, crate::commands::autorun::GuardOwner::Discovery);
                    }
                    // Landed: at the area when it names a saved one, else home.
                    let named_area = p.discovery.as_ref().and_then(|s| s.area.clone());
                    let landing = land_at_area(
                        p.driver,
                        root,
                        organization,
                        project,
                        key,
                        named_area.as_deref(),
                        signed_at,
                        timing,
                    )
                    .await;
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
                        "detail": detail,
                        "path": path,
                        "page": if status == 200 { page.as_str() } else { "" },
                    });
                    if status != 200 {
                        answer["page_unavailable"] = serde_json::json!(page);
                    }
                    if let Some(at_area) = landing.at_area {
                        answer["at_area"] = serde_json::json!(at_area);
                        if at_area {
                            if let Some(name) = named_area.as_deref() {
                                answer["detail"] = serde_json::json!(format!("{detail}, then to {name}"));
                            }
                        }
                    }
                    if let Some(walk) = &landing.walk {
                        answer["area_walk"] = serde_json::json!(walk);
                    }
                    if !landing.notes.is_empty() {
                        answer["area_notes"] = serde_json::json!(landing.notes);
                    }
                    if must_not_save && !is_mapping {
                        answer["no_save"] = serde_json::json!(true);
                        let blocked = take_no_save_blocks(p.driver, &[]);
                        if !blocked.is_empty() {
                            answer["blocked"] = serde_json::json!(blocked.len());
                            answer["blocked_writes"] = serde_json::json!(blocked);
                        }
                    }
                    let session = p.discovery.as_ref().and_then(|s| s.explore_session()).map(str::to_string);
                    if let Some(session) = &session {
                        answer["explore"] = serde_json::json!({ "session": session });
                    }
                    let what = if is_mapping {
                        "mapping run"
                    } else if session.is_some() {
                        "explore session"
                    } else {
                        "discovery"
                    };
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

/// Said in a discovery start's answer (`area_walk`) when `area` is no saved
/// area: the discovery starts at home and files what it sees under that name.
pub fn no_saved_area_to_start_at(area: &str) -> String {
    format!("no saved area is called {area}; the discovery starts at home and files what it sees under that name")
}

/// Where a discovery start landed and what it says about the area.
#[derive(Default)]
struct AreaLanding {
    /// `Some` when an area was named: whether the browser now stands at it.
    at_area: Option<bool>,
    /// Why it does not, when an area was named.
    walk: Option<String>,
    /// What the walk did on the way besides clicking the path.
    notes: Vec<String>,
}

/// After a discovery's sign-in: walk to the saved area `area` as a run
/// does (`nav::reach_module_noted`, from the sign-in), and close the
/// project's known pop-ups - waiting a moment for one to come up - so the
/// first action is never covered by a cookie banner. A walk that fails
/// leaves the browser where it stopped and the discovery going; an area that
/// is not saved leaves it at home. Never closes the browser.
async fn land_at_area<D: crate::browser::cdp::Driver>(
    d: &mut D,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    account: &str,
    area: Option<&str>,
    signed_at: std::time::Instant,
    timing: &crate::browser::timing::Timing,
) -> AreaLanding {
    /// How long, from when the sign-in finished, a banner is waited for.
    const BANNER_WAIT_MS: u64 = 3_000;
    let recipe = crate::autorun::recipe::load_effective_recipe_if_any(root, organization, project).ok().flatten();
    let mut landing = AreaLanding::default();
    let route = area.map(str::trim).filter(|a| !a.is_empty()).map(|a| {
        match crate::autorun::nav::load_nav(root, organization, project) {
            Err(why) => Err(why),
            Ok(nav) if crate::autorun::nav::find_area(&nav, a).is_some() => {
                crate::autorun::runner::discovery_area_route(root, organization, project, Some(a))
            }
            Ok(_) => Err(no_saved_area_to_start_at(a)),
        }
    });
    let mut walked = false;
    match route {
        None => {}
        Some(Err(why)) => {
            landing.at_area = Some(false);
            landing.walk = Some(why);
        }
        Some(Ok(route)) => {
            walked = true;
            // A banner up over the menu would take the first click.
            if let Some(recipe) = &recipe {
                crate::autorun::popups::close_shown_within(d, recipe, 0, timing).await;
            }
            let (out, notes) = crate::autorun::nav::reach_module_noted(
                d,
                &route,
                crate::autorun::nav::TripFrom::SignIn,
                timing,
                "Auto Run discovery",
                Some(account),
            )
            .await;
            landing.at_area = Some(out.ok);
            landing.notes = notes;
            if !out.ok {
                crate::applog::info(format!("Auto Run discovery: the walk to its area did not arrive ({})", out.detail));
                landing.walk = Some(out.detail);
            }
        }
    }
    // The walk's own time counts toward the wait: a banner not up by then is not coming.
    let remaining = BANNER_WAIT_MS.saturating_sub(signed_at.elapsed().as_millis() as u64);
    if let Some(recipe) = &recipe {
        crate::autorun::popups::close_shown_within(d, recipe, remaining, timing).await;
    }
    if walked {
        settle_landing(d, timing).await;
    }
    landing
}

/// After a walk to an area: let the page finish what it does on arriving,
/// as a run lets it before a click (`actions::settle`), then wait for its
/// address to hold still for `SETTLE_QUIET` (at most `SETTLE_CAP`), since a
/// screen can redirect itself a moment after it loads. The answer that
/// follows then describes where the browser is. Never fails: a page that
/// will not hold still is answered as it stands.
async fn settle_landing<D: crate::browser::cdp::Driver>(d: &mut D, timing: &crate::browser::timing::Timing) {
    use crate::browser::actions::{SETTLE_CAP, SETTLE_QUIET};
    crate::browser::actions::settle(d, timing).await;
    let began = std::time::Instant::now();
    let mut seen = String::new();
    let mut since = began;
    loop {
        match crate::browser::page::eval_value(d, "location.href").await {
            Ok(v) => {
                let at = v.as_str().map(crate::autorun::nav::path_of).unwrap_or_default();
                if at != seen {
                    seen = at;
                    since = std::time::Instant::now();
                }
            }
            // Between two documents the page refuses: that is a change.
            Err(e) if e.is_transient() => since = std::time::Instant::now(),
            Err(_) => return,
        }
        if since.elapsed() >= SETTLE_QUIET || began.elapsed() >= SETTLE_CAP {
            return;
        }
        d.idle(std::time::Duration::from_millis(timing.poll_ms.max(10))).await;
    }
}

/// A discovery start's `detail` when the browser it took over was signed in
/// as its account already: nothing was signed in again.
pub fn discovery_signed_in_already(key: &str) -> String {
    format!("signed in already as {key}; went to the home page")
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

/// Put a discovery's save guard on when it must not save
/// (`discovery_must_not_save`) and the guard is not on already, as a Must
/// not save case's is: the built-in save words and the project's own.
/// Asked before every action, so a guard something lifted between two
/// actions is back on before the next. `Err` is the guard's own sentence:
/// such a discovery never acts unguarded.
pub(crate) async fn guard_a_no_save_discovery<D: crate::browser::cdp::Driver>(
    d: &mut D,
    must_not_save: bool,
    root: &std::path::Path,
    organization: &str,
    project: &str,
) -> Result<(), String> {
    if !must_not_save || d.is_guarding_saves() {
        return Ok(());
    }
    guard_a_mapping_run(d, true, root, organization, project).await
}

/// The saves the guard stopped for a discovery that must not save since
/// this was last asked, each as `METHOD /path` (never a host, a query or a
/// value an action in `ran` typed). The stop's pending sentence goes with
/// them: a save stopped between two actions is reported here, and never
/// fails the next action.
fn take_no_save_blocks<D: crate::browser::cdp::Driver>(d: &mut D, ran: &[&crate::browser::actions::Action]) -> Vec<String> {
    let _ = crate::browser::cdp::Driver::take_save_blocked(d);
    crate::browser::cdp::Driver::take_saves_stopped(d)
        .into_iter()
        .map(|(method, path)| {
            let path = ran.iter().fold(crate::autorun::discovery_map::path_only(&path), |p, a| without_typed(&p, a));
            format!("{method} {path}")
        })
        .collect()
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
    /// Saves the guard stopped, each `METHOD /path`: not writes.
    blocked: Vec<String>,
    after: String,
}

/// What the page answered each action of one discovery call - one action,
/// a component's actions, or a batch - kept across them. One `Noted`
/// carries the call's last action that changed the page on to the next, as
/// a run's steps carry it: a message or a refusal that came after an
/// action's own reads is still put on it, and a later failure names it.
/// Each action runs as a step of its own, so the steps `noted` numbers are
/// mapped back to the call's actions.
///
/// It also keeps, for each of those steps, what the application's rules
/// are read from (`discovery_map::rules_from`): the action, the page its
/// read after it was on, and what the page answered it. The call keeps them once,
/// when it is over (`remember`).
#[derive(Default)]
struct Heard {
    noted: crate::autorun::runner::Noted,
    /// The call's action each step `noted` numbered belongs to.
    owners: Vec<usize>,
    /// Everything the page answered each of the call's actions so far.
    answers: Vec<Answered>,
    /// What the page answered each step `noted` numbered, by that number.
    spots: Vec<crate::autorun::discovery_map::ActionAnswer>,
    /// The discovery's area and case, as the call found them.
    area: Option<String>,
    case_id: Option<i32>,
    /// The discovery is an explore session: nothing it learns is filed.
    explore: bool,
}

/// What the page answered one action of a discovery call.
#[derive(Default, Clone)]
struct Answered {
    said: Vec<crate::browser::page_messages::PageMessage>,
    refused: Vec<crate::autorun::refusals::RefusedRequest>,
}

impl Heard {
    /// The call's action `at` is about to run one step.
    fn begin(&mut self, at: usize) {
        self.noted.step = self.owners.len();
        self.owners.push(at);
        self.spots.push(Default::default());
    }

    fn add(
        &mut self,
        at: usize,
        said: Vec<crate::browser::page_messages::PageMessage>,
        refused: Vec<crate::autorun::refusals::RefusedRequest>,
    ) {
        if self.answers.len() <= at {
            self.answers.resize_with(at + 1, Answered::default);
        }
        let a = &mut self.answers[at];
        a.said.extend(said);
        a.said.truncate(crate::browser::page_messages::MAX_MESSAGES);
        a.refused.extend(refused);
    }

    /// What the step action `at` just ran answered it, and what came late
    /// for the call's earlier actions. `one` is the action the step ran (a
    /// component's own).
    fn ran(
        &mut self,
        at: usize,
        outcome: &crate::browser::actions::ActionOutcome,
        one: &crate::browser::actions::Action,
    ) {
        self.add(at, outcome.page_said.clone(), outcome.refused.clone());
        if let Some(spot) = self.spots.get_mut(self.noted.step) {
            spot.page = outcome.page_path.clone().unwrap_or_default();
            spot.step = outcome.page_step.clone();
            spot.after = describe_action(one);
            spot.said.extend(outcome.page_said.iter().cloned());
            spot.refused.extend(outcome.refused.iter().cloned());
            spot.changed = outcome.page_path.is_some();
            spot.failed = !outcome.ok && !crate::autorun::runner::tcm_made(outcome);
        }
        self.take_late();
    }

    fn take_late(&mut self) {
        for late in self.noted.take_late() {
            if let Some(spot) = self.spots.get_mut(late.step) {
                spot.said.extend(late.page_said.iter().cloned());
                spot.refused.extend(late.refused.iter().cloned());
            }
            if let Some(&at) = self.owners.get(late.step) {
                self.add(at, late.page_said, late.refused);
            }
        }
    }

    /// The call is over: what it taught of the application's rules is kept
    /// under the discovery's area, in one write - a map that could not be
    /// written fails nothing.
    fn remember(&self, root: &std::path::Path, organization: &str, project: &str) {
        use crate::autorun::discovery_map::{remember_refusals_logged, rules_from, RuleSource};
        // An explore session keeps nothing in the discovery map.
        if self.explore {
            return;
        }
        let rules = rules_from(&self.spots, RuleSource::Discovery, self.case_id, crate::autorun::sessions::now_ms());
        remember_refusals_logged(root, organization, project, self.area.as_deref(), rules);
    }

    /// The call's last action has no next one to read the page for it: the
    /// messages are read once more, before the call's closing page read.
    async fn look_again<D: crate::browser::cdp::Driver>(&mut self, d: &mut D) {
        self.noted.look_again(d).await;
        self.take_late();
    }

    /// Before a batch's next action: the requests the application refused
    /// the last action that changed the page since its own reads.
    async fn look_for_refusals<D: crate::browser::cdp::Driver>(&mut self, d: &mut D) {
        self.noted.look_for_refusals(d).await;
        self.take_late();
    }

    fn of(&self, at: usize) -> Answered {
        self.answers.get(at).cloned().unwrap_or_default()
    }

    /// Put what the page answered action `at` on its JSON answer:
    /// `page_said` and `app_refused`, each only when there is one.
    fn put_on(&self, at: usize, v: &mut serde_json::Value) {
        let Some(o) = v.as_object_mut() else {
            return;
        };
        let a = self.of(at);
        o.remove("page_said");
        o.remove("app_refused");
        if !a.said.is_empty() {
            o.insert("page_said".into(), serde_json::json!(a.said));
        }
        if !a.refused.is_empty() {
            o.insert("app_refused".into(), serde_json::json!(a.refused));
        }
    }

    /// The call is over: the mark its last action kept is let go.
    fn forget<B: DiscoveryBrowser>(&mut self, slot: &mut Option<B>) {
        if let Some(b) = slot.as_mut() {
            self.noted.forget(&*b.parts().driver);
        }
    }
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
///
/// What the page newly showed after it is `page_said`, and the requests the
/// application refused it `app_refused`, each only when there is one: the
/// messages are read once more before the closing page read, as nothing
/// comes after this action to read them for it.
pub async fn discover_action_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    action: &crate::browser::actions::Action,
    draft: Option<&crate::autorun::components::Component>,
    area: Option<&str>,
) -> (u16, String) {
    let mut heard = Heard::default();
    let was_guarding = guarding_saves(slot);
    let stood = sign_in_stands_now(slot, root, organization, project).await;
    let answer = crate::autorun::action_help::CALL_STARTED
        .scope(
            std::time::Instant::now(),
            discover_action_heard(slot, root, organization, project, action, draft, area, &mut heard, 0, true),
        )
        .await;
    heard.remember(root, organization, project);
    heard.forget(slot);
    end_call_guard(slot, root, was_guarding).await;
    retake_print_after_call(slot, root, organization, project, stood, std::slice::from_ref(action)).await;
    answer
}

/// Whether the browser in `slot` is signed in as its session's account and
/// still holds that sign-in's cookies as they were printed
/// (`replay_to::sign_in_stands`): looked at before a discovery call.
async fn sign_in_stands_now<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
) -> bool {
    let Some(b) = slot.as_mut() else { return false };
    let p = b.parts();
    let Some(key) = p.signed_in.clone() else { return false };
    crate::autorun::replay_to::sign_in_stands(p.driver, root, organization, project, &key, &*p.signed_in, &*p.lease)
        .await
        .is_ok()
}

/// After a discovery call that began with the sign-in standing (`stood`) and
/// did not sign in itself: when the page still shows it is signed in, the
/// sign-in's cookies are printed again. The app's own pages re-issue the
/// session cookie as they load (`replay_to::retake_sign_in`); without this a
/// long discovery leaves a stale print, and a later reset or replay takes the
/// same sign-in for a changed one and signs in again.
async fn retake_print_after_call<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    stood: bool,
    actions: &[crate::browser::actions::Action],
) {
    if !stood {
        return;
    }
    let Some(b) = slot.as_mut() else { return };
    let p = b.parts();
    let Some(key) = p.signed_in.clone() else { return };
    let Ok(recipe) = crate::autorun::recipe::load_effective_recipe(root, organization, project) else { return };
    // The page cannot say who is signed in, so a call that could have swapped
    // the login is never taken as the same sign-in.
    if let Some(why) = crate::autorun::replay_to::touches_sign_in(actions, &recipe) {
        crate::applog::info(format!("Auto Run: a discovery call {why}, so the sign-in's cookies were not printed again"));
        return;
    }
    let timing = crate::browser::timing::Timing::supervised();
    let shows = crate::autorun::signin::already_signed_in_why(
        p.driver,
        &recipe,
        Some(&key),
        &key,
        &timing,
        crate::autorun::signin::SIGNED_IN_LOOK_MS,
    )
    .await;
    if shows.is_ok() {
        let _ = crate::autorun::replay_to::retake_sign_in(p.driver, &recipe.origins(), &mut *p.lease, "a discovery call").await;
    }
}

/// Whether the browser in `slot` guards saves now.
fn guarding_saves<B: DiscoveryBrowser>(slot: &mut Option<B>) -> bool {
    slot.as_mut().is_some_and(|b| crate::browser::cdp::Driver::is_guarding_saves(&*b.parts().driver))
}

/// A discovery call is over: its own `no_save` (`DiscoveryState::call_no_save`)
/// is let go, and the guard it put on is lifted (`commands::autorun::lift_guard_of`)
/// unless the discovery itself must not save (`discovery_must_not_save`) - a
/// mapping run, or one started with `no_save` or for a case marked so. A guard
/// already on before the call (`was_guarding`) was not the call's, and stays.
async fn end_call_guard<B: DiscoveryBrowser>(slot: &mut Option<B>, root: &std::path::Path, was_guarding: bool) {
    let Some(browser) = slot.as_mut() else {
        return;
    };
    let p = browser.parts();
    let Some(state) = p.discovery.as_mut() else {
        return;
    };
    let said = std::mem::take(&mut state.call_no_save);
    state.call_case_id = None;
    if !said || was_guarding || state.mapping.is_some() || discovery_must_not_save(root, state) {
        return;
    }
    if !matches!(p.guard_owner, None | Some(crate::commands::autorun::GuardOwner::Call { .. })) {
        return;
    }
    crate::commands::autorun::lift_guard_of(p.driver, p.guarded_case, p.guard_owner).await;
}

/// `discover_action_in` as action `at` of a discovery call that `heard`
/// keeps; `closing` when it is the call's last, which reads the page's
/// messages once more for it.
#[allow(clippy::too_many_arguments)]
async fn discover_action_heard<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    action: &crate::browser::actions::Action,
    draft: Option<&crate::autorun::components::Component>,
    area: Option<&str>,
    heard: &mut Heard,
    at: usize,
    closing: bool,
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
        stand_in_area(state, root, organization, project, &moved);
    }
    let area_name = state.area.clone();
    heard.area = area_name.clone();
    heard.case_id = state.case_id;
    let exploring = state.explore.is_some();
    heard.explore = exploring;
    let explore_session = state.explore_session().map(str::to_string);
    let setup_running = state.explore.as_ref().is_some_and(|e| e.setup_running);
    let mapping = state.mapping.is_some();
    let began_ms = crate::autorun::sessions::now_ms();
    // Once a discovery must not save, it stays so (`DiscoveryState::no_save`).
    // A call that said `no_save` itself is guarded too, for that call only
    // (`end_call_guard` lifts it): it never makes the discovery one.
    let discovery_guarded = discovery_must_not_save(root, state);
    state.no_save = discovery_guarded;
    let must_not_save = discovery_guarded || state.call_no_save;
    // A call's own guard is for the case that call named, not the
    // discovery's.
    let call_case_id = state.call_case_id;
    let d = p.driver;
    let runs: Vec<&crate::browser::actions::Action> = match &component {
        Some((_, actions)) => actions.iter().collect(),
        None => vec![action],
    };
    // Its guard is on before the action, or the action does not run; a save
    // it stopped between calls is reported with this action, never failing it.
    if let Err(why) = guard_a_no_save_discovery(d, must_not_save, root, organization, project).await {
        return (409, why);
    }
    if discovery_guarded {
        crate::commands::autorun::claim_guard(p.guard_owner, crate::commands::autorun::GuardOwner::Discovery);
    } else if must_not_save {
        crate::commands::autorun::claim_guard(p.guard_owner, crate::commands::autorun::GuardOwner::Call { case_id: call_case_id });
    }
    let reports_blocks = must_not_save && !mapping;
    let mut blocked_writes = if reports_blocks { take_no_save_blocks(d, &runs) } else { Vec::new() };
    let (mut steps, mut writes, mut dialogs) = (Vec::new(), Vec::new(), Vec::new());
    let mut last = None;
    let mut after = String::new();
    for (i, one) in runs.iter().copied().enumerate() {
        heard.begin(at);
        let ran = discover_one(
            d,
            p.signed_in,
            p.lease,
            root,
            organization,
            project,
            area_name.as_deref(),
            mapping,
            exploring,
            one,
            &mut heard.noted,
        )
        .await;
        let mut ran = match ran {
            Ok(ran) => ran,
            Err(refused) => return refused,
        };
        heard.ran(at, &ran.outcome, one);
        if !ran.outcome.ok {
            ran.outcome.detail = crate::browser::input::with_offscreen_hint(&ran.outcome.detail);
        }
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
        if reports_blocks {
            blocked_writes.extend(ran.blocked);
        }
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
    if closing {
        heard.look_again(d).await;
    }
    let sighting = discovery_sighting(root, organization, project, p.discovery.as_ref(), None, p.signed_in.as_deref());
    let (status, page) = read_page(d, crate::browser::snapshot::PAGE_LINES_DEFAULT, sighting.as_ref()).await;
    let mut blocked = count_blocked_writes(d, p.discovery.as_mut(), &runs);
    if reports_blocks {
        blocked_writes.extend(take_no_save_blocks(d, &runs));
        // A save seen both ways (in the request record and as a stop) is
        // said once, in the order it was first seen.
        let mut seen = std::collections::HashSet::new();
        blocked_writes.retain(|w| seen.insert(w.clone()));
        blocked = Some(blocked_writes.len());
    }
    let mut answer = serde_json::json!({
        "ok": outcome.ok,
        "detail": outcome.detail,
        "path": after,
        "dialogs": dialogs,
        "writes": writes,
        "page": if status == 200 { page.as_str() } else { "" },
    });
    heard.put_on(at, &mut answer);
    if let Some(n) = blocked {
        answer["blocked"] = serde_json::json!(n);
    }
    if reports_blocks {
        answer["no_save"] = serde_json::json!(true);
        answer["blocked_writes"] = serde_json::json!(blocked_writes);
        let posted: Vec<(String, String)> = writes
            .iter()
            .filter_map(|w| Some((w["method"].as_str()?.to_string(), w["path"].as_str()?.to_string())))
            .collect();
        if let Some(line) = crate::browser::save_guard::let_through_line(&posted) {
            answer["let_through"] = serde_json::json!(line);
        }
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
    // An explore session keeps an action that worked for the assistant that
    // ran it (`autorun::kept`), unless a setup is driving it, a save it sent
    // was stopped, or it typed a login. A keep never fails the action.
    if let (Some(session), true) = (explore_session.as_deref(), outcome.ok && !setup_running && !mapping && blocked.unwrap_or(0) == 0) {
        let steps: Option<Vec<crate::browser::actions::Action>> = component.as_ref().map(|(_, actions)| actions.clone());
        let credential = match crate::autorun::accounts::load_accounts(root) {
            Ok(accounts) => crate::autorun::kept::types_a_credential(action, steps.as_deref(), &accounts),
            Err(_) => true,
        };
        if !credential {
            crate::autorun::kept::keep(
                root,
                session,
                action,
                steps,
                area_name.as_deref(),
                began_ms,
                crate::autorun::sessions::now_ms(),
            );
        }
    }
    // An explore session keeps the saves the call reported, by method and
    // path, so its end can name the ones no change names. A change list
    // that will not take them is logged and never fails the action.
    if let Some(session) = explore_session.as_deref() {
        let sent: Vec<(String, String)> = answer["writes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|w| {
                let path = w["path"].as_str()?;
                let path = match w["handler"].as_str() {
                    Some(handler) => format!("{path}?handler={handler}"),
                    None => path.to_string(),
                };
                Some((w["method"].as_str()?.to_string(), path))
            })
            .collect();
        if let Err(why) = crate::autorun::explore::note_writes(root, session, &sent) {
            crate::applog::warn(format!("Auto Run explore session: its saves could not be noted: {why}"));
        }
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
/// page path when it changed, whether its own page read failed, where its
/// picture is, and what the page answered it (`page_said`, `app_refused`).
/// A failed action's detail already names its refusal, so only what the
/// page showed it and the detail does not say is added.
fn batch_line(n: usize, action: &crate::browser::actions::Action, v: &serde_json::Value, last_path: &mut Option<String>) -> String {
    let ok = v["ok"].as_bool().unwrap_or(false);
    let detail = v["detail"].as_str().map(str::trim).filter(|d| !d.is_empty());
    let mut said = batch_detail(&detail.map(str::to_string).unwrap_or_else(|| describe_action(action)));
    for dialog in v["dialogs"].as_array().into_iter().flatten().filter_map(|d| d.as_str()) {
        said.push_str(&format!("; dialog {}", one_short_line(dialog)));
    }
    for write in v["writes"].as_array().into_iter().flatten() {
        let (method, path) = (write["method"].as_str().unwrap_or(""), write["path"].as_str().unwrap_or(""));
        said.push_str(&format!("; wrote {method} {path}"));
    }
    for stopped in v["blocked_writes"].as_array().into_iter().flatten().filter_map(|w| w.as_str()) {
        said.push_str(&format!("; blocked {stopped}"));
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
    if ok {
        let refused: Vec<crate::autorun::refusals::RefusedRequest> =
            serde_json::from_value(v["app_refused"].clone()).unwrap_or_default();
        if let Some(first) = refused.first() {
            said.push_str(&format!("; {}", crate::autorun::refusals::app_refused_line(first)));
        }
    }
    let shown: Vec<crate::browser::page_messages::PageMessage> =
        serde_json::from_value(v["page_said"].clone()).unwrap_or_default();
    if !shown.is_empty() {
        let note = crate::autorun::runner::page_showed_note(&shown);
        if ok || !detail.is_some_and(|d| d.contains(&note)) {
            said.push_str(&format!("; {note}"));
        }
    }
    format!("{n}. {}: {said}", if ok { "ok" } else { "failed" })
}

/// An action's detail on its batch line: one line of at most 200
/// characters, unless it carries what the page or the application answered
/// - the reason the assistant needs whole - which is kept on one line, as
/// long as the runner made it.
fn batch_detail(detail: &str) -> String {
    use crate::autorun::{refusals::APP_REFUSED, runner::PAGE_SHOWED};
    if detail.contains(APP_REFUSED) || detail.contains(PAGE_SHOWED) {
        detail.split_whitespace().collect::<Vec<_>>().join(" ")
    } else {
        one_short_line(detail)
    }
}

/// Whether a failed action's detail already names what the page answered
/// an earlier action (`runner::AFTER_EARLIER`).
fn names_an_earlier_answer(detail: &str) -> bool {
    use crate::autorun::{refusals::APP_REFUSED, runner::{AFTER_EARLIER, PAGE_SHOWED}};
    detail.find(AFTER_EARLIER).is_some_and(|i| detail[i..].contains(PAGE_SHOWED) || detail[i..].contains(APP_REFUSED))
}

/// Said after a batch stopped at action `failed` (from 0): the last action
/// before it that the page answered with a message, as the likelier cause.
/// Nothing when the failed action has a refusal of its own, or its detail
/// already names an earlier action's answer.
fn points_back(actions: &[crate::browser::actions::Action], heard: &Heard, failed: usize, detail: &str) -> Option<String> {
    if !heard.of(failed).refused.is_empty() || names_an_earlier_answer(detail) {
        return None;
    }
    let (k, said) = (0..failed).rev().map(|k| (k, heard.of(k).said)).find(|(_, said)| !said.is_empty())?;
    let what = actions.get(k).map(describe_action).unwrap_or_default();
    Some(format!("After action {} ({what}) {}.", k + 1, crate::autorun::runner::page_showed_note(&said)))
}

/// The first action of a batch that worked and was refused once it had
/// ended (from 0): a refusal found within its own reads would have failed it.
fn refused_once_ended(rows: &[Row], heard: &Heard) -> Option<usize> {
    rows.iter().find_map(|row| match row {
        Row::Ran(k, v) if v["ok"].as_bool() == Some(true) && !heard.of(*k).refused.is_empty() => Some(*k),
        _ => None,
    })
}

/// One line of a batch's answer, before the batch is done: said as it is,
/// or the action (from 0) that ran with its single answer, put into words
/// once everything the page answered it is known.
enum Row {
    Said(String),
    Ran(usize, serde_json::Value),
}

/// A batch's rows as its answer's lines (`batch_line`), each action with
/// what the page answered it (`Heard::put_on`).
fn batch_rows(actions: &[crate::browser::actions::Action], rows: &[Row], heard: &Heard) -> Vec<String> {
    let mut last_path = None;
    rows.iter()
        .map(|row| match row {
            Row::Said(line) => line.clone(),
            Row::Ran(i, v) => {
                let mut v = v.clone();
                heard.put_on(*i, &mut v);
                match actions.get(*i) {
                    Some(action) => batch_line(i + 1, action, &v, &mut last_path),
                    None => String::new(),
                }
            }
        })
        .collect()
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
    format!("Stopped: {n} not run - End discovery, Close browser, a release or a reset asked the batch to stop.")
}

/// Each browser slot's batch stop control: set by End discovery, Close
/// browser, both releases and the assistant's reset before they wait for the
/// browser, so a batch holding it gives way before its next action instead
/// of keeping them waiting. Cleared when a batch hears it, and before each
/// batch starts. Coordination only.
static BATCH_STOPS: [std::sync::atomic::AtomicBool; SLOT_COUNT] =
    [const { std::sync::atomic::AtomicBool::new(false) }; SLOT_COUNT];

/// Slot `n`'s batch stop control (`BATCH_STOPS`).
pub fn batch_stop(n: SlotNo) -> &'static std::sync::atomic::AtomicBool {
    &BATCH_STOPS[n.index()]
}

/// How many batches of discovery actions are going in each slot (waiting for
/// the browser or driving it). Batches in one slot can overlap (two callers
/// sharing it, or parallel calls of one proxy), so this counts. Kept so the
/// Assistants card can offer to stop them without taking the browser's lock.
/// Coordination only.
static BATCHES_GOING: [std::sync::atomic::AtomicUsize; SLOT_COUNT] =
    [const { std::sync::atomic::AtomicUsize::new(0) }; SLOT_COUNT];

/// Whether a batch of discovery actions is going in slot `n` (`BATCHES_GOING`).
pub fn batch_going(n: SlotNo) -> bool {
    BATCHES_GOING[n.index()].load(std::sync::atomic::Ordering::SeqCst) > 0
}

/// Counts slot `n`'s batch as going for as long as it lives; dropped at the
/// batch's end, however it ends.
pub struct BatchGoing(SlotNo);

impl BatchGoing {
    pub fn begin(n: SlotNo) -> BatchGoing {
        BATCHES_GOING[n.index()].fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        BatchGoing(n)
    }
}

impl Drop for BatchGoing {
    fn drop(&mut self) {
        BATCHES_GOING[self.0.index()].fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Ask the batch going in slot `n`, if any, to stop before its next action.
pub fn stop_batch_in(n: SlotNo) {
    batch_stop(n).store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Ask the batch going in the main slot, if any, to stop before its next
/// action.
pub fn stop_batch() {
    stop_batch_in(SlotNo::MAIN);
}

/// Ask the batch going in every slot, if any, to stop before its next action.
pub fn stop_all_batches() {
    for n in SlotNo::all() {
        stop_batch_in(n);
    }
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
/// nothing comes between its actions; `stop` (`batch_stop` from the route)
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
///
/// The batch's actions share one `Heard`: what the page showed only after
/// an action's own reads is found by the next action's first read (or, for
/// the last, by one more read before the page's) and said on that action's
/// line. When the batch stopped at a failure whose own line cannot name it,
/// the stop line points back to the last action the page answered with a
/// message (`points_back`).
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
    discover_actions_paged(slot, root, organization, project, actions, draft, area, stop_on_failure, stop, true).await
}

/// [`discover_actions_in`], with the page after the last action left out
/// of the answer when `page` is false (`"page": false`): it is still read,
/// and what it shows recorded as seen, and a last line says where to read
/// it (`PAGE_LEFT_OUT`).
#[allow(clippy::too_many_arguments)]
pub async fn discover_actions_paged<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    actions: &[crate::browser::actions::Action],
    draft: Option<&crate::autorun::components::Component>,
    area: Option<&str>,
    stop_on_failure: bool,
    stop: &std::sync::atomic::AtomicBool,
    page: bool,
) -> (u16, String) {
    let mut heard = Heard::default();
    let was_guarding = guarding_saves(slot);
    let stood = sign_in_stands_now(slot, root, organization, project).await;
    let answer = crate::autorun::action_help::CALL_STARTED
        .scope(
            std::time::Instant::now(),
            discover_actions_heard(
                slot,
                root,
                organization,
                project,
                actions,
                draft,
                area,
                stop_on_failure,
                stop,
                page,
                &mut heard,
            ),
        )
        .await;
    heard.remember(root, organization, project);
    heard.forget(slot);
    end_call_guard(slot, root, was_guarding).await;
    retake_print_after_call(slot, root, organization, project, stood, actions).await;
    answer
}

/// `discover_actions_in`, with what the page answered its actions kept in
/// `heard`.
#[allow(clippy::too_many_arguments)]
async fn discover_actions_heard<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    actions: &[crate::browser::actions::Action],
    draft: Option<&crate::autorun::components::Component>,
    area: Option<&str>,
    stop_on_failure: bool,
    stop: &std::sync::atomic::AtomicBool,
    page_wanted: bool,
    heard: &mut Heard,
) -> (u16, String) {
    if let Err(refused) = refuse_batch_size(actions.len()) {
        return refused;
    }
    let Some(state) = slot.as_mut().and_then(|b| b.parts().discovery.as_mut()) else {
        return (409, NO_DISCOVERY.to_string());
    };
    if let Some(moved) = named(area) {
        stand_in_area(state, root, organization, project, &moved);
    }
    let blocked_by = if state.mapping.is_some() { "the mapping run" } else { "the no-save guard" };
    let mut rows = Vec::new();
    let mut blocked = 0usize;
    let mut ran = 0;
    // The action the batch stopped at, and its detail.
    let mut stopped_at = None;
    // An action that worked, refused once it had ended, and how many were
    // then not run.
    let mut refused_after = None;
    for (i, action) in actions.iter().enumerate() {
        if stop.swap(false, std::sync::atomic::Ordering::SeqCst) {
            let mut lines = batch_rows(actions, &rows, heard);
            lines.push(ended_line(actions.len() - i));
            return (409, lines.join("\n"));
        }
        // A refused request stops the batch even when it was answered after
        // the action that sent it had ended (past the answer wait, or sent
        // after its own reads): the next action does not run.
        if stop_on_failure && i > 0 {
            if let Some(b) = slot.as_mut() {
                heard.look_for_refusals(b.parts().driver).await;
            }
            if let Some(k) = refused_once_ended(&rows, heard) {
                refused_after = Some((k, actions.len() - i));
                break;
            }
        }
        // The call has a time of its own (`CALL_BUDGET_MS`); when it has run
        // out, nothing more is started and the page is not read.
        if crate::autorun::action_help::action_budget(1, crate::autorun::action_help::call_elapsed_ms()).is_none() {
            let secs = crate::autorun::action_help::CALL_BUDGET_MS / 1000;
            rows.push(Row::Said(format!("{}. failed: {}", i + 1, crate::autorun::action_help::no_answer(i + 1, secs))));
            let mut lines = batch_rows(actions, &rows, heard);
            if i + 1 < actions.len() {
                lines.push(not_run_line(actions.len() - i - 1));
            }
            return (504, lines.join("
"));
        }
        let (status, body) =
            discover_action_heard(slot, root, organization, project, action, draft_for(action, draft), None, heard, i, false)
                .await;
        ran = i + 1;
        if status == 200 && body.contains(crate::autorun::action_help::DID_NOT_ANSWER) {
            // An action that did not answer: the browser may not answer the
            // next either, so the batch stops here and does not read the page.
            let v: serde_json::Value = serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
            if v["detail"].as_str().is_some_and(|d| d.starts_with(crate::autorun::action_help::DID_NOT_ANSWER)) {
                rows.push(Row::Ran(i, v));
                let mut lines = batch_rows(actions, &rows, heard);
                if ran < actions.len() {
                    lines.push(not_run_line(actions.len() - ran));
                }
                return (504, lines.join("
"));
            }
        }
        if status != 200 {
            // The browser failed or the discovery is gone: nothing after
            // it can run.
            let stopped = said_browser_failed(&(status, body.clone())) || body == NO_DISCOVERY;
            rows.push(Row::Said(format!("{}. failed: {}", i + 1, one_short_line(&body))));
            if stopped {
                let mut lines = batch_rows(actions, &rows, heard);
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
        let failed = !v["ok"].as_bool().unwrap_or(false);
        let detail = v["detail"].as_str().unwrap_or("").to_string();
        rows.push(Row::Ran(i, v));
        if stop_on_failure && failed {
            stopped_at = Some((i, detail));
            break;
        }
    }
    // The last action has no next one to read the page's messages for it.
    if let Some(b) = slot.as_mut() {
        heard.look_again(b.parts().driver).await;
    }
    let mut lines = batch_rows(actions, &rows, heard);
    let back = stopped_at.and_then(|(i, detail)| points_back(actions, heard, i, &detail));
    match (refused_after, ran < actions.len(), back) {
        (Some((k, n)), _, _) => {
            let what = actions.get(k).map(describe_action).unwrap_or_default();
            lines.push(format!("Stopped: the app refused action {} ({what}) after it ended: {n} not run.", k + 1));
        }
        (None, true, Some(back)) => lines.push(format!("{} {back}", not_run_line(actions.len() - ran))),
        (None, true, None) => lines.push(not_run_line(actions.len() - ran)),
        (None, false, Some(back)) => lines.push(back),
        (None, false, None) => {}
    }
    if blocked > 0 {
        lines.push(format!("Saves blocked by {blocked_by}: {blocked}."));
    }
    let Some(browser) = slot.as_mut() else {
        return (409, NO_DISCOVERY.to_string());
    };
    let filing = if project.trim().is_empty() { None } else { Some(root) };
    let (status, page) =
        page_read_in(browser, filing, organization, project, None, crate::browser::snapshot::PAGE_LINES_DEFAULT).await;
    if status != 200 {
        lines.push(format!("The page could not be read: {}", one_short_line(&page)));
        return batch_ended_by_the_browser(slot, (status, page), lines).await;
    }
    if !page_wanted {
        return (200, format!("{}\n\nPage: {PAGE_LEFT_OUT}", lines.join("\n")));
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
    exploring: bool,
    action: &crate::browser::actions::Action,
    noted: &mut crate::autorun::runner::Noted,
) -> Result<Discovered, (u16, String)> {
    // An explore session files no write, outcome or locator in the map.
    let recording = !project.trim().is_empty() && !exploring;
    let bucket = area_name.unwrap_or("");
    let before = current_page(d).await.0;
    let notes_before = status_texts(d).await;
    let mark = crate::browser::cdp::Driver::net_mark(d);
    let tried = try_action(
        d,
        signed_in,
        lease,
        root,
        organization,
        project,
        0,
        area_name,
        mapping,
        !exploring,
        action,
        Some(noted),
    )
    .await;
    // The writes are read once and the mark let go, however the action
    // went: on the whole browser a mark kept pushes out the ones still in
    // use (`MAX_NET_MARKS`).
    let sent = crate::browser::cdp::Driver::net_since(d, mark);
    crate::browser::cdp::Driver::net_unmark(d, mark);
    let tried = tried?;
    let what = describe_action(action);
    let now = crate::autorun::sessions::now_ms();
    let mut writes = Vec::new();
    let mut blocked = Vec::new();
    for sent in sent {
        if !crate::browser::save_guard::SAVE_METHODS.contains(&sent.method.as_str()) {
            continue;
        }
        let path = without_typed(&crate::autorun::discovery_map::path_only(&sent.path_query), action);
        // A save the guard failed in the browser never reached the server:
        // it is blocked, not written, and the map does not log it.
        if crate::browser::save_guard::stopped_by_guard(&sent) {
            blocked.push(format!("{} {path}", sent.method));
            continue;
        }
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
        let mut write = serde_json::json!({ "method": sent.method, "path": path });
        // Which save a page's address is: the one part of the query worth
        // keeping (`explore::handler_of`).
        if let Some(handler) = crate::autorun::explore::handler_of(&sent.path_query) {
            write["handler"] = serde_json::json!(handler);
        }
        writes.push(write);
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
    Ok(Discovered { outcome: tried.outcome, dialogs, writes, blocked, after })
}

/// Said by `end_autorun_discovery` once the discovery is over and its
/// browser kept (`end_discovery_in`).
pub const DISCOVERY_ENDED_KEPT: &str =
    "the discovery is over; its browser stays open and signed in, for your next replay or discovery";

/// Said once a discovery is over and its browser closed: by the assistant's
/// release, and by an end whose guard would not lift.
pub(crate) const DISCOVERY_ENDED_CLOSED: &str = "the discovery is over and its browser is closed";

/// End the discovery in `slot` (`end_discovery_keeping`): its browser is
/// kept, signed in, for the next replay or discovery, and its guard
/// (`GuardOwner::Discovery`) is lifted. Any other guard - the person's, a
/// call's - is not the discovery's to lift. With nothing to end this is
/// still an answer, not an error - ending twice is fine.
///
/// A mapping run's summary is kept first (`finish_mapping`), and the
/// answer is `{detail, summary}`.
pub async fn end_discovery_in<B: DiscoveryBrowser>(slot: &mut Option<B>) -> (u16, String) {
    match end_discovery_keeping(slot, true).await {
        Some(ended) => {
            let detail = if ended.kept { DISCOVERY_ENDED_KEPT } else { DISCOVERY_ENDED_CLOSED };
            ended_answer(detail, ended.summary, ended.explore)
        }
        None => (200, "no discovery is going".to_string()),
    }
}

/// Who ends a discovery (`end_discovery_as`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndedBy {
    /// The person's End discovery on the Setup card: it closes the
    /// assistant's discovery browser so Auto Run can run again.
    Person,
    /// The assistant's `end_autorun_discovery`: the browser stays, signed in,
    /// for its next replay or discovery.
    Assistant,
}

/// End the discovery in `slot` as `by` ends it: the assistant's end keeps
/// the browser (`end_discovery_in`), the person's closes it
/// (`end_discovery_closing`). A mapping run's summary is kept either way.
pub async fn end_discovery_as<B: DiscoveryBrowser>(slot: &mut Option<B>, by: EndedBy) -> (u16, String) {
    match by {
        EndedBy::Assistant => end_discovery_in(slot).await,
        EndedBy::Person => end_discovery_closing(slot),
    }
}

/// End the discovery in `slot` and close its browser, as the person's End
/// discovery and the assistant's release
/// (`commands::autorun::release_in_for_assistant`) do: the person ends it
/// so Auto Run can run again, and release is the repair for a broken
/// browser, so neither keeps one. The answer is `end_discovery_in`'s, with
/// the browser closed.
pub fn end_discovery_closing<B: DiscoveryBrowser>(slot: &mut Option<B>) -> (u16, String) {
    match close_discovery(slot) {
        Some((summary, explore)) => ended_answer(DISCOVERY_ENDED_CLOSED, summary, explore),
        None => (200, "no discovery is going".to_string()),
    }
}

/// A discovery's end, said: `{detail, summary}` for a mapping run,
/// `{detail, explore, note}` for an explore session (`note` only when saves
/// its browser sent are not on its change list), else the words.
pub(crate) fn ended_answer(
    detail: &str,
    summary: Option<crate::autorun::mapping_summary::MappingSummary>,
    explore: Option<crate::autorun::explore::ExploreEnd>,
) -> (u16, String) {
    match (summary, explore) {
        (Some(summary), _) => (200, serde_json::json!({ "detail": detail, "summary": summary }).to_string()),
        (None, Some(explore)) => {
            let mut answer = serde_json::json!({ "detail": detail, "explore": explore });
            if !explore.saves_not_on_the_change_list.is_empty() {
                answer["note"] = serde_json::json!(crate::autorun::explore::UNRECORDED_SAVES);
            }
            (200, answer.to_string())
        }
        (None, None) => (200, detail.to_string()),
    }
}

/// How a discovery ended (`end_discovery_keeping`): the mapping run's
/// summary, if it was one, the explore session's end, if it was one, and
/// whether its browser was kept.
pub(crate) struct Ended {
    pub(crate) summary: Option<crate::autorun::mapping_summary::MappingSummary>,
    pub(crate) kept: bool,
    pub(crate) explore: Option<crate::autorun::explore::ExploreEnd>,
}

/// The way a discovery in `slot` ends and its browser stays: a mapping
/// run's summary is kept (`finish_mapping`), the discovery is taken off the
/// browser, and - when `lift` - the discovery's own guard
/// (`GuardOwner::Discovery`) is lifted. The browser keeps its sign-in, its
/// lease and its page. A guard that will not lift is never left behind with
/// no discovery to end it: a browser the assistant opened is then closed
/// through its normal close; one the person opened is never closed, and its
/// guard stays on until the person's next step decides it. `None` with no
/// discovery going.
pub(crate) async fn end_discovery_keeping<B: DiscoveryBrowser>(slot: &mut Option<B>, lift: bool) -> Option<Ended> {
    use crate::commands::autorun::GuardOwner;
    if !slot.as_mut().is_some_and(|b| b.parts().discovery.is_some()) {
        return None;
    }
    let summary = finish_mapping(slot);
    let explore = finish_explore(slot);
    let (lifted, assistants) = match slot.as_mut() {
        Some(b) => {
            let assistants = b.opened_by_assistant();
            let p = b.parts();
            *p.discovery = None;
            let lifted = if lift && *p.guard_owner == Some(GuardOwner::Discovery) {
                crate::commands::autorun::lift_guard_of(p.driver, p.guarded_case, p.guard_owner).await
            } else {
                true
            };
            (lifted, assistants)
        }
        None => (true, false),
    };
    if !lifted && assistants {
        if let Some(browser) = slot.take() {
            crate::browser::tree::blocking(|| browser.close());
        }
        crate::applog::info("Auto Run discovery ended; its guard would not lift, so its browser is closed");
        return Some(Ended { summary, kept: false, explore });
    }
    crate::applog::info("Auto Run discovery ended; its browser is kept, signed in");
    Some(Ended { summary, kept: true, explore })
}

/// A discovery in `slot` ends and its browser is closed: a mapping run's
/// summary is kept (`finish_mapping`), then the browser goes: the person's
/// End discovery and the assistant's release (`end_discovery_closing`).
/// `None` with no discovery going, else the mapping run's summary, if it
/// was one, and the explore session's end, if it was one.
fn close_discovery<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
) -> Option<(Option<crate::autorun::mapping_summary::MappingSummary>, Option<crate::autorun::explore::ExploreEnd>)> {
    if !slot.as_mut().is_some_and(|b| b.parts().discovery.is_some()) {
        return None;
    }
    let summary = finish_mapping(slot);
    let explore = finish_explore(slot);
    if let Some(browser) = slot.take() {
        browser.close();
    }
    crate::applog::info("Auto Run discovery ended; its browser is closed");
    Some((summary, explore))
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

/// An explore change list refusal as the route says it: a file that does
/// not read is a conflict, a change that could not be kept a server error,
/// an id with no session unknown, and anything else (an entry number the
/// list lacks, a verified create) a request that cannot be done.
fn explore_refusal(why: String) -> (u16, String) {
    use crate::autorun::explore as ex;
    if why == ex::CHANGE_NOT_KEPT {
        (500, why)
    } else if why == ex::CHANGES_UNREADABLE {
        (409, why)
    } else if ex::is_unknown_session(&why) {
        (404, why)
    } else {
        (400, why)
    }
}

/// `/autorun-explore-change`: one change the going explore session made,
/// onto its change list - `{ "change": "created" | "changed" | "deleted",
/// ... }` - or, for any saved session, going or ended, the entries the
/// assistant set back by hand: `{ "session": <id>, "undone": [<n>, ...] }`.
/// A create is checked against the saves the session's own browser made
/// (`explore::record_change`), never taken on the caller's word.
///
/// A session of another organisation or project than `org` and `project` is
/// refused, as the undo refuses it.
pub fn record_change_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    org: &str,
    project: &str,
    body: &str,
) -> (u16, String) {
    use crate::autorun::explore as ex;
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
    if let Some(undone) = v.get("undone") {
        const SHAPE: &str = "{ \"session\": <the session id>, \"undone\": [<entry numbers>] }";
        let entries: Option<Vec<u32>> = undone
            .as_array()
            .and_then(|a| a.iter().map(|n| n.as_u64().and_then(|n| u32::try_from(n).ok())).collect());
        let (Some(session), Some(entries)) = (v.get("session").and_then(|s| s.as_str()), entries) else {
            return (400, format!("that is not an undone list. Expected {SHAPE}."));
        };
        let session = session.trim();
        match ex::load(root, session) {
            Ok(s) if !ex::ran_in(&s, org, project) => return (400, ex::other_project(&s)),
            Ok(_) => {}
            Err(why) if ex::is_unknown_session(&why) => return (404, why),
            Err(why) => {
                crate::applog::warn(format!("Auto Run explore session: {why}"));
                return (409, ex::CHANGES_UNREADABLE.to_string());
            }
        }
        return match ex::mark_undone(root, session, &entries) {
            Ok(()) => (200, serde_json::json!({ "session": session, "undone": entries }).to_string()),
            Err(why) => explore_refusal(why),
        };
    }
    let Some(browser) = slot.as_mut() else {
        return (409, ex::NO_EXPLORE_SESSION.to_string());
    };
    let parts = browser.parts();
    let Some(state) = parts.discovery.as_ref() else {
        return (409, ex::NO_EXPLORE_SESSION.to_string());
    };
    let Some(session) = state.explore_session().map(str::to_string) else {
        return (409, ex::NO_EXPLORE_SESSION.to_string());
    };
    // After the sign-in: the time the change list began, not the discovery's.
    let since = state.explore.as_ref().map_or(0, |run| run.began_ms);
    let change = match ex::change_from_args(&v) {
        Ok(change) => change,
        Err(why) => return (400, why),
    };
    let request = v.get("request").and_then(|r| r.as_str()).map(str::to_string);
    let captured: Vec<&crate::browser::net_capture::Captured> = crate::browser::cdp::Driver::capture(&*parts.driver)
        .map(|c| c.list(0, usize::MAX).0)
        .unwrap_or_default();
    match ex::record_change(root, &session, change, request, &captured, since) {
        Ok(recorded) => (200, serde_json::to_string(&recorded).unwrap_or_default()),
        Err(why) => explore_refusal(why),
    }
}

/// `get_kept_actions`: the actions the owner's explore session kept, or why
/// there are none.
pub fn kept_actions_for(root: &std::path::Path, owner: &str) -> (u16, String) {
    match crate::autorun::kept::last_of(root, owner) {
        Ok(list) => (200, crate::autorun::kept::kept_answer(&list).to_string()),
        Err(why) => (409, why),
    }
}

/// `list_setups`: this project's saved setups, without actions, SQL or values.
pub fn setups_list_for(root: &std::path::Path, org: &str, project: &str) -> (u16, String) {
    match crate::autorun::setups::load_setups(root, org, project) {
        Ok(f) => (200, crate::autorun::setups::list_answer(&f).to_string()),
        Err(why) => (409, why),
    }
}

/// `save_setup`: a range of the owner's kept actions saved as a named setup
/// with a find rule. Everything is checked against files and memory on this
/// machine; the one thing that runs is the database find rule, once, as a
/// SELECT, when `rows` is given (Database Read Access is on). A body that is
/// not a save ask is 400 in serde's words.
pub async fn setup_save_for<S: crate::autorun::setup_find::RowSource>(
    root: &std::path::Path,
    org: &str,
    project: &str,
    owner: &str,
    body: &str,
    rows: Option<&S>,
    now: u64,
) -> (u16, String) {
    use crate::autorun::{explore as ex, kept, nav, seen_check, setup_find, setups};
    let asked: setups::SaveAsked = match serde_json::from_str(body) {
        Ok(a) => a,
        Err(e) => return (400, e.to_string()),
    };
    // A removal needs no explore session and no reason.
    if asked.remove {
        return match setups::remove(root, org, project, &asked.name) {
            Ok(name) => {
                crate::applog::info(format!("Auto Run setup {name}: removed"));
                (200, serde_json::json!({ "removed": name }).to_string())
            }
            Err(why) if why == setups::no_such_setup(&asked.name) => (404, why),
            Err(why) => (409, why),
        };
    }
    let list = match kept::last_of(root, owner) {
        Ok(l) => l,
        Err(why) => return (409, why),
    };
    let session = match ex::load(root, &list.session) {
        Ok(s) => s,
        Err(why) if ex::is_unknown_session(&why) => return (404, why),
        Err(why) => {
            crate::applog::warn(format!("Auto Run explore session: {why}"));
            return (409, ex::CHANGES_UNREADABLE.to_string());
        }
    };
    let prefix = crate::environments::active(root)
        .map(|e| e.test_prefix)
        .unwrap_or_else(|_| crate::environments::default_test_prefix());
    let recipe = match crate::autorun::recipe::load_effective_recipe(root, org, project) {
        Ok(r) => r,
        Err(why) => return (409, why),
    };
    let policy = crate::autorun::runner::policy_for(Some(&recipe));
    let allowed = |url: &str| policy.allows(url);
    // The map and the areas are read only when the ask needs them.
    let seen = |area: Option<&str>, actions: &[crate::browser::actions::Action]| {
        let map = seen_check::load_checked_map(root, org, project)?;
        seen_check::check_component_seen(&map, area, actions)
    };
    let area_known = |area: &str| nav::load_nav(root, org, project).is_ok_and(|n| nav::find_area(&n, area).is_some());
    let accounts = match crate::autorun::accounts::load_accounts(root) {
        Ok(a) => a,
        Err(_) => return (409, "The saved accounts could not be read, so nothing was saved.".to_string()),
    };
    let built = {
        let from = setups::SaveFrom {
            accounts: &accounts,
            kept: &list,
            session: &session,
            prefix: &prefix,
            allowed: &allowed,
            seen: &seen,
            area_known: &area_known,
            now,
        };
        match setups::build_setup(&asked, &from) {
            Ok(b) => b,
            Err(problems) => return (400, problems.join(" ")),
        }
    };
    let database_checked = match &built.setup.find.database {
        None => None,
        Some(db) => Some(match rows {
            None => "not run: Database Read Access is off",
            Some(rows) => match setup_find::find_in_database(rows, db, &prefix).await {
                setup_find::DbOutcome::Found(_) => "found",
                setup_find::DbOutcome::NotFound => "not found",
                setup_find::DbOutcome::CouldNotRun(why) => {
                    return (400, format!("The database find rule did not run: {why}"))
                }
            },
        }),
    };
    let setup = built.setup.clone();
    let (replaced, cut) = (built.replaced.clone(), built.cut);
    let saved = match setups::save(root, org, project, built, asked.why.as_deref()) {
        Ok(s) => s,
        Err(why) => return (409, why),
    };
    crate::applog::info(format!("Auto Run setup {}: saved, version {}", saved.saved, saved.version));
    let mut answer = serde_json::json!({
        "saved": saved.saved,
        "version": saved.version,
        "changes": saved.changes,
        "actions": setup.actions.len(),
        "area": setup.area,
        "inputs": setup.inputs.iter().map(|i| i.name.clone()).collect::<Vec<_>>(),
        "replaced": replaced,
        "creates": setup
            .creates
            .iter()
            .map(|c| serde_json::json!({ "kind": c.kind, "request": c.request }))
            .collect::<Vec<_>>(),
        "returns": setup.creates.get(setup.returns).map(|c| c.kind.clone()),
        "cut": cut,
    });
    if let (Some(checked), Some(map)) = (database_checked, answer.as_object_mut()) {
        map.insert("database_checked".to_string(), serde_json::Value::String(checked.to_string()));
    }
    (200, answer.to_string())
}

/// A discovery started in `slot` for `caller`: its previous kept actions are
/// dropped, and an explore session with a change list gets a list of its own.
async fn kept_started(slot: SlotNo, caller: &crate::autorun::agents::Caller) {
    let Ok(root) = autorun_root() else {
        return;
    };
    let mut held = crate::commands::autorun::session_slot(slot).lock().await;
    kept_started_in(&root, &mut held, caller);
}

/// `kept_started` over the browser in `held`: the discovery it holds decides
/// whether `caller` gets a kept list (an explore session with a change list)
/// or none.
pub fn kept_started_in<B: DiscoveryBrowser>(
    root: &std::path::Path,
    held: &mut Option<B>,
    caller: &crate::autorun::agents::Caller,
) {
    let (session, area) = held
        .as_mut()
        .and_then(|b| b.parts().discovery.as_ref().map(|s| (s.explore_session().map(str::to_string), s.area.clone())))
        .unwrap_or((None, None));
    crate::autorun::kept::started(root, &crate::autorun::kept::owner_of(caller), session.as_deref(), area.as_deref());
}

/// `/autorun-explore-changes`: `{}` lists this project's explore sessions,
/// newest first, each counted; `{ "session": <id> }` gives one session's
/// whole change list, the saves its browser sent that no change accounts for,
/// and its `verified_creates` (kind, id, name and the request that proved
/// each: what an undo would delete). A session of another project is refused.
pub fn explore_changes(root: &std::path::Path, org: &str, project: &str, body: &str) -> (u16, String) {
    use crate::autorun::explore as ex;
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
    if let Some(id) = v.get("session").and_then(|s| s.as_str()) {
        return match ex::load(root, id.trim()) {
            Ok(s) if !ex::ran_in(&s, org, project) => (400, ex::other_project(&s)),
            Ok(s) => {
                let mut out = serde_json::to_value(&s).unwrap_or_default();
                out["saves_not_on_the_change_list"] = serde_json::json!(ex::unaccounted(&s));
                // What an undo would delete, as the person is told it first.
                out["verified_creates"] = serde_json::json!(ex::verified_creates(&s));
                (200, out.to_string())
            }
            Err(why) if ex::is_unknown_session(&why) => (404, why),
            Err(why) => {
                crate::applog::warn(format!("Auto Run explore session: {why}"));
                (409, ex::CHANGES_UNREADABLE.to_string())
            }
        };
    }
    let sessions: Vec<serde_json::Value> = ex::list(root, org, project)
        .iter()
        .map(|s| {
            serde_json::json!({
                "session": s.session,
                "started_at": s.started_at,
                "ended_at": s.ended_at,
                "account": s.account,
                "environment": s.environment_name,
                "changes": s.changes.len(),
                "undone": s.changes.iter().filter(|c| c.undone).count(),
            })
        })
        .collect();
    (200, serde_json::json!({ "sessions": sessions }).to_string())
}

/// `/api-template-send`: one GET or POST from the page the discovery's
/// browser is on, in the account it signed in as - no sign-in of its own, no
/// lease of its own. A POST also needs an explore session (so what it
/// changes goes on a change list) and the person's API templates switch.
/// Every refusal comes before anything is sent: the body, then the switch,
/// then the session, the no-save guard, the account the browser still holds,
/// the one-at-a-time slot and the run's own checks. A POST that was sent
/// without being a read is noted on the session's list of saves
/// (`explore::note_writes`); a create that worked goes on the change list
/// from the response's own id (`explore::record_send_create`); an edit or a
/// delete says to record what it changed.
pub async fn send_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    ctx: &BridgeContext,
    root: &std::path::Path,
    body: &str,
    timing: &crate::browser::timing::Timing,
    limit: std::time::Duration,
    retry_pauses: &[std::time::Duration],
) -> (u16, String) {
    use crate::api_templates::one_off as oo;
    use crate::api_templates::runner::{claim, preflight, run_in_page, Mode, RunRequest, TokenPage};
    use crate::api_templates::{Effect, Method};
    use crate::autorun::explore as ex;
    let one = match oo::parse(body) {
        Ok(one) => one,
        Err(refused) => return refused,
    };
    let posts = one.step.method == Method::Post;
    if posts && !ctx.api_writes {
        return (400, API_WRITES_OFF.to_string());
    }
    let path = oo::filled_path(&one);
    let Some(browser) = slot.as_mut() else {
        return (409, oo::NO_SESSION_FOR_SEND.to_string());
    };
    let parts = browser.parts();
    let Some(state) = parts.discovery.as_ref() else {
        return (409, oo::NO_SESSION_FOR_SEND.to_string());
    };
    let session = state.explore_session().map(str::to_string);
    let floor = state.explore.as_ref().map_or(0, |run| run.began_ms);
    if posts && session.is_none() {
        return (409, oo::POST_NEEDS_EXPLORE.to_string());
    }
    if posts && discovery_must_not_save(root, state) {
        return (409, crate::browser::save_guard::blocked("POST", &path));
    }
    let account = match state.account.clone() {
        Some(account) if parts.lease.holds(root, &account) => account,
        _ => return (409, oo::SESSION_NOT_SIGNED_IN.to_string()),
    };
    let Some(_claim) = claim() else {
        return (409, API_TEMPLATE_BUSY.to_string());
    };
    if posts {
        if let Err(moved) = assistant_may_still_write(ctx, root) {
            return (409, moved);
        }
    }
    let page = one.antiforgery_page.clone();
    let req = RunRequest {
        org: ctx.org.clone(),
        project: ctx.project.clone(),
        account,
        values: one.values.clone(),
        mode: Mode::Explore,
        template: oo::as_template(&one, oo::ONE_OFF_ID, page.as_deref().unwrap_or("/")),
    };
    if let Err(problems) = preflight(root, &req, None) {
        return (400, problems.join("\n"));
    }
    let how = match page {
        Some(p) => TokenPage::Open(p),
        None => TokenPage::Here,
    };
    // What the capture saw before this call was sent is what its answer is
    // judged against.
    let sent_at = crate::autorun::sessions::now_ms();
    let out = run_in_page(parts.driver, root, &req, timing, how, limit, retry_pauses).await;

    let report = &out.report;
    let request = oo::request_of(&one);
    let sent = report.steps.iter().any(|s| s.status.is_some());
    // Every POST that was sent is a save, whatever the call said it does: a
    // "read" that wrote must not leave the session's saves looking clean.
    if posts && sent {
        if let Some(session) = &session {
            if let Err(why) = ex::note_writes(root, session, &[("POST".to_string(), oo::request_path(&one))]) {
                crate::applog::warn(format!("Auto Run explore session: a save could not be noted: {why}"));
            }
        }
    }
    let mut v = serde_json::json!({
        "ok": report.ok,
        "status": out.answer.as_ref().map(|a| a.status),
        "captured": report.created,
        "request": request,
    });
    if let Some(a) = &out.answer {
        v["response"] = serde_json::json!({
            "content_type": a.content_type,
            "body": crate::autorun::capture_redact::redact_body(a.content_type.as_deref(), &a.body),
            "cut": a.cut,
        });
    }
    if !report.ok {
        v["failed"] = serde_json::json!(report.failed);
        v["detail"] = serde_json::json!(report.message());
    }
    match (one.effect, &one.created, &session) {
        (Effect::Create, Some(made), Some(session)) if report.ok => {
            // The id is verified, and so can be deleted by an undo, only when
            // it is new to this session (`explore::send_create_is_new`): the
            // call did not send it, nothing the page showed or sent before
            // carried it, and no change on the list names it. With no capture
            // to prove that it is kept, not verified.
            let captured: Option<Vec<&crate::browser::net_capture::Captured>> =
                crate::browser::cdp::Driver::capture(&*parts.driver).map(|c| c.list(0, usize::MAX).0);
            let answered_2xx = out.answer.as_ref().is_some_and(|a| (200..300).contains(&a.status));
            let is_new = |id: &str, earlier: &[ex::ChangeEntry]| {
                answered_2xx
                    && ex::send_create_is_new(captured.as_deref(), floor, sent_at, id, earlier, oo::sends_id(&one, id))
            };
            let kept = report
                .created
                .get(&made.id)
                .ok_or_else(|| ex::CHANGE_NOT_KEPT.to_string())
                .and_then(|id| ex::record_send_create(root, session, &made.kind, id, &made.name, &request, &is_new));
            match kept {
                Ok((n, verified)) => {
                    v["change"] = serde_json::json!(n);
                    v["verified"] = serde_json::json!(verified);
                    if !verified {
                        v["note"] = serde_json::json!(ex::NOT_VERIFIED);
                    }
                }
                Err(why) => {
                    crate::applog::warn(format!("Auto Run explore session: a create could not be kept: {why}"));
                    v["change_not_kept"] = serde_json::json!(why);
                }
            }
        }
        (Effect::Edit, _, _) if sent => v["record_change"] = serde_json::json!(oo::edit_reminder(&request)),
        (Effect::Delete, _, _) if sent => v["record_change"] = serde_json::json!(oo::delete_reminder(&request)),
        _ => {}
    }
    if report.ok {
        match &session {
            Some(session) => {
                let status = out.answer.as_ref().map(|a| a.status);
                let token = (out.token_page.as_deref(), out.token_found);
                v["template"] = save_or_match_one_off(root, ctx, &req.account, session, &one, status, token, report);
            }
            // A plain discovery has no session to name as the proof.
            None => v["template"] = serde_json::json!({ "not_saved": oo::PLAIN_DISCOVERY_NOT_SAVED }),
        }
    }
    (if report.ok { 200 } else { 502 }, v.to_string())
}

/// Said, as `ended_session`, when an undo ended the going discovery first.
pub const ENDED_FOR_UNDO: &str =
    "Ended your explore session and closed its browser first: Clean up signs in as the delete template's account.";

/// What `/api-template-explore-undo` expects.
const UNDO_SHAPE: &str = "{ \"session\": \"<the session id>\", \"ids\": [\"<id>\", ...], \"person_said_yes\": true, \"kind\": \"<kind, only when an id is two kinds>\" }";

/// Said to an undo the person has not said yes to in the chat.
pub const UNDO_NEEDS_YES: &str = "Tell the person exactly what will be deleted (each record's kind, id and name, from get_explore_changes) and wait for their yes in the chat first.";

/// Said when the account of a delete template is held by a discovery or
/// mapping run that is not the assistant's own explore session.
pub const UNDO_ACCOUNT_IN_USE: &str = "A discovery or mapping run that is not your explore session is using that account's browser: end it with end_autorun_discovery, then ask again.";

/// `/api-template-explore-undo`: delete, through the proven delete templates,
/// the creates `ids` of one explore session - and only those: an id must be
/// one the test-made list holds as this session's verified create, made in
/// the active environment and in the project open now. The person's ask is the gate, so it also
/// needs the API templates switch. Every refusal comes before anything is
/// deleted: the switch, the body, the session, the one-at-a-time slot, then
/// the plan, which refuses the whole undo for one id that is not the
/// session's.
///
/// Clean up signs in as the delete template's proven account, and an account
/// is held by one browser at a time. A going discovery whose browser holds
/// the account of a template this undo needs is therefore ended and its
/// browser closed first (`ended_session`); one on another account is left
/// alone. Each delete that worked marks its create undone on the change
/// list. The answer is `{ results, total, stopped, ended_session? }`.
pub async fn explore_undo_in<S: DiscoveryBrowser, B: crate::autorun::replay::Browsers>(
    slot: &mut Option<S>,
    browsers: &mut B,
    ctx: &BridgeContext,
    root: &std::path::Path,
    body: &str,
    timing: &crate::browser::timing::Timing,
) -> (u16, String) {
    use crate::autorun::explore as ex;
    use serde_json::Value;
    if !ctx.api_writes {
        return (400, API_WRITES_OFF.to_string());
    }
    let shape = || (400, format!("that is not an undo. Expected {UNDO_SHAPE}."));
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return shape();
    };
    let Some(session) = v.get("session").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) else {
        return shape();
    };
    let ids: Option<Vec<String>> = v.get("ids").and_then(Value::as_array).and_then(|a| {
        a.iter()
            .map(|i| match i {
                Value::String(s) => Some(s.trim().to_string()),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .collect()
    });
    let Some(ids) = ids else {
        return shape();
    };
    let kind = match v.get("kind") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.trim().to_string()),
        Some(_) => return shape(),
    };
    // Like a delete call, an undo deletes only after the person said yes in
    // the chat to a message naming what will go.
    if v.get("person_said_yes").and_then(Value::as_bool) != Some(true) {
        return (400, UNDO_NEEDS_YES.to_string());
    }
    let session_file = match ex::load(root, session) {
        Ok(file) => file,
        Err(why) => {
            return if ex::is_unknown_session(&why) {
                (404, why)
            } else {
                crate::applog::warn(format!("Auto Run explore session: {why}"));
                (409, ex::CHANGES_UNREADABLE.to_string())
            };
        }
    };
    // The slot first, as every run does: holding it is what refuses an
    // environment switch while the plan and the deletes are going.
    let Some(_claim) = crate::api_templates::runner::claim() else {
        return (409, API_TEMPLATE_BUSY.to_string());
    };
    if let Err(moved) = assistant_may_still_write(ctx, root) {
        return (409, moved);
    }
    let plan = match crate::autorun::cleanup::undo_plan(root, &ctx.org, &ctx.project, session, &ids, kind.as_deref()) {
        Ok(plan) => plan,
        Err(why) => return (400, why),
    };

    let accounts: Vec<String> = plan
        .iter()
        .filter_map(|(_, t)| t.as_ref().and_then(|t| t.proven.as_ref()).map(|p| p.account.clone()))
        .collect();
    // (holds an account of the plan, and is the assistant's own explore session)
    let held = slot.as_mut().and_then(|b| {
        let p = b.parts();
        let explore = p.discovery.as_ref()?.explore.is_some();
        accounts.iter().any(|a| p.lease.holds(root, a)).then_some(explore)
    });
    // Only the assistant's own explore session is ended for an undo. Any
    // other discovery or mapping run that holds the account is the person's
    // to end: refused before anything is deleted.
    if held == Some(false) {
        return (409, UNDO_ACCOUNT_IN_USE.to_string());
    }
    let mut ended = None;
    let mut explore_end = None;
    if held == Some(true) {
        explore_end = close_discovery(slot).and_then(|(_, explore)| explore);
        ended = Some(ENDED_FOR_UNDO);
    }

    let cancel = std::sync::atomic::AtomicBool::new(false);
    let report = match crate::autorun::cleanup::undo_explore_session(
        browsers,
        root,
        &ctx.org,
        &ctx.project,
        session,
        &ids,
        kind.as_deref(),
        timing,
        &cancel,
        |_| {},
    )
    .await
    {
        Ok(report) => report,
        Err(why) => {
            // The session was closed above whatever came of the deletes.
            let mut said = ended.map_or(why.clone(), |said| format!("{why}\n{said}"));
            if let Some(end) = explore_end.as_ref().filter(|e| !e.saves_not_on_the_change_list.is_empty()) {
                said.push_str(&format!(
                    "\nSaves not on the change list: {}. {}",
                    end.saves_not_on_the_change_list.join(", "),
                    ex::UNRECORDED_SAVES
                ));
            }
            return (400, said);
        }
    };

    let mut not_marked = Vec::new();
    for r in report.results.iter().filter(|r| r.ok) {
        match ex::mark_create_undone(root, session, &r.kind, &r.id) {
            Ok(true) => {}
            Ok(false) => not_marked.push(r.id.clone()),
            Err(why) => {
                crate::applog::warn(format!("Auto Run explore session: an undone create could not be marked: {why}"));
                not_marked.push(r.id.clone());
            }
        }
    }
    let mut answer = serde_json::json!({
        "results": report.results,
        "total": report.total,
        "stopped": report.stopped,
    });
    // What was deleted, as the person was told it: each verified create the
    // undo was asked for, with the request that proved it.
    answer["creates"] = serde_json::json!(ex::verified_creates(&session_file)
        .into_iter()
        .filter(|c| c["id"].as_str().is_some_and(|id| ids.iter().any(|i| i == id)))
        .collect::<Vec<_>>());
    if let Some(said) = ended {
        answer["ended_session"] = serde_json::json!(said);
    }
    if let Some(end) = explore_end {
        if !end.saves_not_on_the_change_list.is_empty() {
            answer["note"] = serde_json::json!(ex::UNRECORDED_SAVES);
        }
        answer["explore"] = serde_json::json!(end);
    }
    if !not_marked.is_empty() {
        answer["not_marked_undone"] = serde_json::json!(not_marked);
    }
    (200, answer.to_string())
}

/// `POST /api-template-explore-undo` with slot `n`'s own session and real
/// browsers.
async fn api_template_explore_undo(ctx: &BridgeContext, body: &str, n: SlotNo) -> (u16, String) {
    if let Some(busy) = unattended_run_is_using_the_browser() {
        return busy;
    }
    let root = match autorun_root() {
        Ok(root) => root,
        Err(refused) => return refused,
    };
    let mut slot = crate::commands::autorun::session_slot(n).lock().await;
    // Looked at again now the lock is held: a run may have started while
    // this call waited for it.
    if let Some(busy) = unattended_run_is_using_the_browser() {
        return busy;
    }
    let mut browsers = real_template_browsers(crate::browser::launch::Browser::Edge);
    explore_undo_in(&mut slot, &mut browsers, ctx, &root, body, &crate::commands::autorun_replay::replay_timing(false)).await
}

/// What `send_in` answers under `template` for a call that worked: it is
/// not saved (a delete, a secret written out, a page with no token), or it
/// is the same call as a template already saved (`reused`), or it is saved
/// now as a one-step template proven by the session that made it. A save
/// that fails is logged and answered without a path.
#[allow(clippy::too_many_arguments)]
fn save_or_match_one_off(
    root: &std::path::Path,
    ctx: &BridgeContext,
    account: &str,
    session: &str,
    one: &crate::api_templates::one_off::OneOff,
    status: Option<u16>,
    token: (Option<&str>, bool),
    report: &crate::api_templates::runner::RunReport,
) -> serde_json::Value {
    use crate::api_templates::one_off as oo;
    use crate::api_templates::store::{self, RunRecord};
    use crate::api_templates::{Effect, Expect, Proven, EXPLORE_PROOF};
    let not_saved = |why: &str| serde_json::json!({ "not_saved": why });
    if one.effect == Effect::Delete {
        return not_saved(oo::DELETES_NOT_SAVED);
    }
    if oo::sends_a_secret(&one.step) {
        return not_saved(oo::SECRET_NOT_SAVED);
    }
    let token_page = match token {
        (Some(page), true) => page,
        _ => return not_saved(oo::NO_TOKEN_NOT_SAVED),
    };
    if oo::page_sends_a_secret(token_page) {
        return not_saved(oo::SECRET_NOT_SAVED);
    }
    let could_not = || not_saved("the template could not be saved - see Settings, Logs");
    let (org, project) = (ctx.org.as_str(), ctx.project.as_str());
    let saved = match store::list(root, org, project) {
        Ok(saved) => saved,
        Err(e) => {
            crate::applog::warn(format!("Auto Run explore session: the saved templates could not be read: {e}"));
            return could_not();
        }
    };
    let mut one = one.clone();
    // A POST that said it only reads is saved as a read only when the change
    // list shows it changed nothing: one the session has recorded a change
    // for (or whose list cannot be read) is saved as an edit.
    if one.step.method == crate::api_templates::Method::Post && one.effect == Effect::Read {
        let changed = crate::autorun::explore::load(root, session)
            .map_or(true, |s| crate::autorun::explore::names_request(&s, &oo::request_of(&one)));
        if changed {
            one.effect = Effect::Edit;
        }
    }
    // A path segment that is a record id becomes a param, so one template
    // serves every record, whether it is being matched or saved.
    let (step, params) = oo::with_id_params(&one.step, &one.params);
    one.step = step;
    one.params = params;
    // A call that did not say what it expects keeps what it got, so a later
    // run checks exactly that.
    if one.step.expect == Expect::default() {
        if let Some(status) = status {
            one.step.expect.status = status;
        }
    }
    if let Some(same) = oo::matching(&saved, &one.step) {
        return serde_json::json!({ "id": same.id, "reused": true });
    }
    let taken = |id: &str| !matches!(store::load(root, org, project, id), Ok(None));
    let id = match one.id.as_deref().filter(|id| crate::api_templates::valid_id(id)) {
        Some(id) => oo::unique_id(id, &taken),
        None => oo::derived_id(&one.step, &taken),
    };
    let mut t = oo::as_template(&one, &id, token_page);
    t.title = one.name.clone().unwrap_or_else(|| oo::derived_title(&one.step));
    t.module = one.module.clone().unwrap_or_else(|| oo::EXPLORE_MODULE.to_string());
    t.description = one.description.clone().unwrap_or_else(|| {
        format!("Saved from an explore session on {}.", crate::applog::stamp().split(' ').next().unwrap_or(""))
    });
    t.sources = vec![format!("explore session {session}")];
    let problems = crate::api_templates::check(&t);
    if !problems.is_empty() {
        crate::applog::warn(format!("Auto Run explore session: the call is not a template: {}", problems.join("; ")));
        return could_not();
    }
    t.proven = Some(Proven {
        at: crate::applog::stamp(),
        origin: recipe_origin(root, org, project).unwrap_or_default(),
        account: account.to_string(),
        outputs: report.outputs.clone(),
        environment: crate::environments::active(root).ok().map(|e| e.name),
        via: Some(EXPLORE_PROOF.to_string()),
    });
    if let Err(e) = store::save(root, org, project, &t) {
        crate::applog::warn(format!("Auto Run explore session: the template could not be saved: {e}"));
        return could_not();
    }
    let record = RunRecord {
        at: crate::applog::stamp(),
        mode: crate::api_templates::runner::Mode::Explore.label().to_string(),
        account: account.to_string(),
        ok: true,
        failed_step: None,
        detail: None,
        outputs: report.outputs.clone(),
    };
    if let Err(e) = store::append_run(root, org, project, &id, record) {
        crate::applog::warn(format!("api template {id}: the run could not be added to its history: {e}"));
    }
    templates_changed(&id);
    serde_json::json!({ "id": id, "saved": true })
}

/// Close out the explore session in `slot`, if one is going, however it
/// ends (the same ways as `finish_mapping`): its change list file gets its
/// end time, and the answer counts its changes and names the saves its
/// browser sent that no change names (`explore::end`). The run is taken off
/// the discovery, so closing it twice ends it once. `None` with no explore
/// session going, with one whose change list was never made, or whose file
/// does not read (the log says so, without a path).
pub fn finish_explore<B: DiscoveryBrowser>(slot: &mut Option<B>) -> Option<crate::autorun::explore::ExploreEnd> {
    let browser = slot.as_mut()?;
    let run = browser.parts().discovery.as_mut()?.explore.take()?;
    let session = run.session?;
    let Some(root) = crate::autorun::store::configured_root() else {
        crate::applog::warn("Auto Run explore session: there is no data folder, so its change list is not closed");
        return None;
    };
    match crate::autorun::explore::end(&root, &session) {
        Ok(end) => {
            crate::applog::info(format!(
                "Auto Run explore session ended: {} changes listed, {} saves not on the list",
                end.changes,
                end.saves_not_on_the_change_list.len()
            ));
            Some(end)
        }
        Err(why) => {
            crate::applog::warn(format!("Auto Run explore session: its change list could not be closed: {why}"));
            None
        }
    }
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
/// discovery holding it is ended as End discovery ends it
/// (`end_discovery_keeping`), so what it mapped is kept, a mapping run's
/// summary is saved, and the browser stays, signed in, for the replay. A
/// no_save discovery's guard stays on - never off between the discovery and
/// the replay that is its last call (`DiscoveryEnded::no_save`); any other
/// discovery's guard (a mapping run's) is lifted. Says whether one was
/// ended, with that summary. Called under the session lock the replay then
/// keeps while it runs, so nothing takes the browser between the end and
/// the replay. The person's own Replay to step still goes through
/// `refuse_while_discovering`.
pub async fn end_discovery_for_replay<B: DiscoveryBrowser>(slot: &mut Option<B>, root: &std::path::Path) -> DiscoveryEnded {
    // Decided as before each of its actions (`discovery_must_not_save`), so a
    // case marked after its last action counts too.
    let no_save = slot
        .as_mut()
        .and_then(|b| b.parts().discovery.as_ref().map(|s| discovery_must_not_save(root, s)))
        .unwrap_or(false);
    match end_discovery_keeping(slot, !no_save).await {
        Some(ended) => {
            crate::applog::info("Auto Run: the assistant's replay to a step ended its discovery first");
            DiscoveryEnded { ended: true, summary: ended.summary, no_save, explore: ended.explore }
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
    /// How an explore session it ended ended: its changes, and the saves its
    /// browser sent that no change accounts for.
    pub explore: Option<crate::autorun::explore::ExploreEnd>,
    /// The discovery it ended must not save (`discovery_must_not_save`): its
    /// guard stays on for that replay, the call that ended it, and for no
    /// call after (`commands::autorun::guard_before_replay`,
    /// `settle_replay_guard`).
    pub no_save: bool,
}

/// Put before the answer to an assistant's replay that ended its discovery
/// first (`end_discovery_for_replay`).
pub const ENDED_DISCOVERY_FIRST: &str =
    "Ended your discovery first (what it mapped is kept); start a new one to explore again.";

/// Said after `ENDED_DISCOVERY_FIRST` when the discovery the replay ended
/// was a no_save one: its guard stays on for this replay
/// (`DiscoveryEnded::no_save`).
pub const DISCOVERY_GUARD_KEPT: &str = "Its save guard stayed on for this replay, because the discovery was started without saving; your next replay of a case that may save is not guarded.";

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
    finish_explore(slot);
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
/// browser is touched: the person replaces an area, never the assistant -
/// except to repair a path a run cannot take (`repair_area_in`).
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
    let answer = save_discovered_area(slot, root, organization, project, name, module, clicks, None, timing).await;
    if let Some(browser) = slot.as_mut() {
        let p = browser.parts();
        count_blocked_writes(p.driver, p.discovery.as_mut(), &[]);
    }
    answer
}

/// An assistant's repair of an existing area's menu path
/// (`save_autorun_area` with `repair: { reason }`, owner decision
/// 2026-10-20). The name must be an area the project has - whoever made
/// it, a person included - and the repair keeps it under its own module
/// (`module` may be blank; another one is refused), and new clicks equal
/// to the saved ones are refused (`area_repair::SAME_PATH`), judged after the
/// new clicks are pinned (`nav::go_to_module_pinning`). It is allowed
/// only while the area is failing to be reached, decided live every time:
/// the area's saved path is replayed from a fresh home as a run takes it
/// (`nav::reach_module`, two tries with a reload between) before anything
/// is written, and the repair is refused while it still arrives
/// (`area_repair::NOTHING_TO_REPAIR`) or when the browser stops answering
/// during that check. The new clicks are then replayed from a fresh home
/// exactly as a new area's are, and must arrive where the discovery browser
/// stands. The change is written to the areas file as it is after those
/// trips (`area_repair::save_repair`), so a Remove or a put-back made
/// meanwhile is never overwritten. Only the clicks, where they arrive and
/// the page they begin on change; the area keeps its name, module and
/// maker, and records the path it replaced with the scrubbed reason
/// (`repaired_by_assistant`) so the person can put it back.
#[allow(clippy::too_many_arguments)]
pub async fn repair_area_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    name: &str,
    module: &str,
    clicks: Vec<crate::browser::locator::Target>,
    reason: &str,
    timing: &crate::browser::timing::Timing,
) -> (u16, String) {
    let answer =
        save_discovered_area(slot, root, organization, project, name, module, clicks, Some(reason), timing).await;
    if let Some(browser) = slot.as_mut() {
        let p = browser.parts();
        count_blocked_writes(p.driver, p.discovery.as_mut(), &[]);
    }
    answer
}

/// A fresh home page for an area's replay, never where the page stands:
/// PeoplesHR's menu remembers whether a person left it open, so the clicks
/// would otherwise depend on it (spec 2026-10-09, mapped areas).
/// `load_home` runs `after_sign_in` once, on the fresh load. A session that
/// has gone is signed in again as `account`; a sign-in ends on a fresh,
/// signed-in page with `after_sign_in` run, so that is the start: loading
/// home again would run it a second time, and a toggle run twice closes the
/// menu it opened. The outcome says whether home was reached; `Err` is the
/// answer to give instead.
#[allow(clippy::too_many_arguments)]
async fn fresh_home_for_area<D: crate::browser::cdp::Driver>(
    d: &mut D,
    home: &crate::autorun::nav::Home,
    timing: &crate::browser::timing::Timing,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    account: Option<&str>,
    lease: &mut crate::autorun::lease::Held,
    signed_in: &mut Option<String>,
) -> Result<crate::browser::actions::ActionOutcome, (u16, String)> {
    let went = crate::autorun::nav::load_home(d, home, timing).await;
    let signed = went.ok
        && crate::browser::expect::expect(
            d,
            &home.signed_in,
            crate::browser::expect::Check::Visible,
            timing.nav_ms,
            timing.poll_ms,
        )
        .await
        .ok;
    if signed {
        return Ok(went);
    }
    let Some(key) = account else {
        return Err((409, "the discovery has no account to sign in again with - start it again".to_string()));
    };
    match crate::autorun::signin::sign_in_leased(d, root, organization, project, key, lease, signed_in, timing).await {
        Err(why) => Err((409, why)),
        Ok(out) if !out.ok => Err((409, out.detail)),
        Ok(_) => Ok(crate::browser::actions::ActionOutcome::passed("signed in again")),
    }
}

/// The discovery now stands in the area `moved`. What it has seen since it
/// started, in the area it leaves, is kept in its own state for the new one
/// (`discovery_map::carry_seen`), so a script saved while this discovery goes
/// on can use locators met before the move. Never written to the map. A mapping run reads and saves nothing, so it
/// carries nothing.
fn stand_in_area(
    state: &mut crate::commands::autorun::DiscoveryState,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    moved: &str,
) {
    let to = crate::autorun::discovery_map::canonical_area(root, organization, project, moved);
    if state.mapping.is_none() {
        if let Some(from) = state.area.as_deref() {
            match crate::autorun::discovery_map::load_map(root, organization, project) {
                Ok(map) => {
                    if let Some(more) = crate::autorun::discovery_map::carry_seen(&map, from, &to, state.started_at) {
                        crate::autorun::discovery_map::keep_carried(&mut state.carried, more);
                    }
                }
                Err(why) => unrecorded(&why),
            }
        }
    }
    state.area = Some(to);
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
    stand_in_area(state, root, organization, project, &kept);
    if !mapping {
        return;
    }
    let at = discovery_sighting(root, organization, project, discovery.as_ref(), None, signed_in);
    let _ = read_page(d, crate::browser::snapshot::DEFAULT_LIMIT, at.as_ref()).await;
    if let Some(state) = discovery.as_mut() {
        state.area = None;
    }
}

/// `save_autorun_area` with `variant: true`: the clicks the discovery found
/// are added to the existing area `name` as another menu path
/// (`ModulePath::variants`), for the other menu shape. They are replayed from
/// a fresh home exactly as a new area's are and must arrive where the
/// discovery browser stands; the area's primary path, name, module and
/// scripts are untouched. A path the area already has is not added twice
/// (`{saved: false, unchanged: true}`). Not in a mapping run, which adds and
/// updates its own areas only.
pub async fn variant_area_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    name: &str,
    module: &str,
    clicks: Vec<crate::browser::locator::Target>,
    timing: &crate::browser::timing::Timing,
) -> (u16, String) {
    let answer = save_area_variant(slot, root, organization, project, name, module, clicks, timing).await;
    if let Some(browser) = slot.as_mut() {
        let p = browser.parts();
        count_blocked_writes(p.driver, p.discovery.as_mut(), &[]);
    }
    answer
}

async fn save_area_variant<B: DiscoveryBrowser>(
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
    let name = name.trim();
    if name.is_empty() || clicks.is_empty() {
        return (400, "a menu path needs the name of an existing area and at least one click".to_string());
    }
    if state.mapping.is_some() {
        return (409, "a mapping run does not add menu paths; end it and add the menu path in an ordinary discovery".to_string());
    }
    let navfile = match nav::load_nav(root, organization, project) {
        Ok(n) => n,
        Err(why) => return (409, why),
    };
    let Some(found) = nav::find_area(&navfile, name).cloned() else {
        return (409, format!("there is no area named '{name}' to add a menu path to - save it first, without variant"));
    };
    if !module.trim().is_empty() && nav::module_key(module) != nav::module_key(&found.module) {
        return (
            409,
            format!("area '{name}' belongs to module '{}', not '{}' - a menu path never moves an area", found.module, module.trim()),
        );
    }
    let recipe = match crate::autorun::recipe::load_effective_recipe(root, organization, project) {
        Ok(r) => r,
        Err(why) => return (409, why),
    };
    let account = state.account.clone().or_else(|| p.signed_in.clone());
    let d = p.driver;
    let href = crate::browser::page::eval_value(d, "location.href").await;
    let href = href.ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
    if href.trim().is_empty() {
        return (409, "the page would not say where it is - read the page and try again".to_string());
    }
    let arrived = nav::path_of(&href);
    let home = nav::Home::of(&recipe);
    let went = match fresh_home_for_area(
        d,
        &home,
        timing,
        root,
        organization,
        project,
        account.as_deref(),
        p.lease,
        p.signed_in,
    )
    .await
    {
        Ok(went) => went,
        Err(refused) => return refused,
    };
    let tried = if went.ok {
        let path = nav::ModulePath {
            clicks,
            arrived,
            start: String::new(),
            variants: Vec::new(),
            repaired_by_assistant: None,
            ..found.clone()
        };
        nav::go_to_module_pinning(d, &nav::Route::new(&recipe, path), timing).await
    } else {
        Err(nav::PathFailure { at: nav::Where::Home, reason: went.detail, harness: went.harness })
    };
    let (at, pinned_clicks, pinned) = match tried {
        Ok(found) => found,
        Err(failure) => {
            let (status, page) = read_page(d, crate::browser::snapshot::DEFAULT_LIMIT, None).await;
            let showed = if status == 200 {
                page.lines().take(AREA_PAGE_LINES).collect::<Vec<_>>().join("\n")
            } else {
                "nothing - the page could not be read".to_string()
            };
            return (409, format!("The clicks did not arrive: {}. The page showed: {showed}", failure.for_dialog()));
        }
    };
    let kept = found.name().to_string();
    let same = |clicks: &[crate::browser::locator::Target], arrived: &str| clicks == pinned_clicks.as_slice() && arrived == at;
    let held = if same(&found.clicks, &found.arrived) {
        Some(1)
    } else {
        found.variants.iter().position(|v| same(&v.clicks, &v.arrived)).map(|i| i + 2)
    };
    if let Some(n) = held {
        stand_in_saved_area(d, p.discovery, p.signed_in.as_deref(), root, organization, project, kept.clone()).await;
        let message = format!("already menu path {n} of \"{kept}\"");
        return (200, serde_json::json!({ "saved": false, "unchanged": true, "arrived": at, "message": message }).to_string());
    }
    let mut updated = found.clone();
    updated.variants.push(nav::PathVariant { clicks: pinned_clicks, arrived: at.clone() });
    let n = updated.variants.len() + 1;
    if let Err(why) = nav::put_path_if(root, organization, project, updated, nav::AreaExpect::Was(&found)) {
        return (409, why);
    }
    stand_in_saved_area(d, p.discovery, p.signed_in.as_deref(), root, organization, project, kept.clone()).await;
    crate::applog::info(format!("Auto Run discovery added menu path {n} to an area"));
    let message = format!("saved as menu path {n} of \"{kept}\"");
    let mut answer = serde_json::json!({ "saved": true, "arrived": at, "message": message });
    if !pinned.is_empty() {
        answer["pinned"] = serde_json::json!(pinned);
    }
    (200, answer.to_string())
}

/// `discover_area_in` and `repair_area_in`, before the blocked saves are
/// counted. `repair` is the repair's reason, as the assistant sent it.
#[allow(clippy::too_many_arguments)]
async fn save_discovered_area<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    name: &str,
    module: &str,
    clicks: Vec<crate::browser::locator::Target>,
    repair: Option<&str>,
    timing: &crate::browser::timing::Timing,
) -> (u16, String) {
    use crate::autorun::{area_repair, nav};
    let Some(browser) = slot.as_mut() else {
        return (409, NO_DISCOVERY.to_string());
    };
    let p = browser.parts();
    let Some(state) = p.discovery.as_ref() else {
        return (409, NO_DISCOVERY.to_string());
    };
    let (name, module) = (name.trim().to_string(), module.trim().to_string());
    let reason = match repair.map(area_repair::check_reason) {
        None => None,
        Some(Ok(reason)) => Some(reason),
        Some(Err(why)) => return (400, why),
    };
    // A repair may leave the module out: it keeps the area's own.
    if name.is_empty() || (module.is_empty() && reason.is_none()) || clicks.is_empty() {
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
    // The area a repair replaces the path of: one the project has, under
    // the module it already has.
    let repairing = match &reason {
        None => None,
        Some(_) => match nav::find_area(&navfile, &name) {
            None => return (409, area_repair::no_area_to_repair(&name)),
            Some(found) if !module.is_empty() && nav::module_key(&module) != nav::module_key(&found.module) => {
                return (409, area_repair::other_module(found))
            }
            Some(found) => Some(found.clone()),
        },
    };
    // The clicks as sent: an area is compared with them, not with the clicks
    // as they are saved once pinned (`nav::go_to_module_pinning`), and a
    // repair is the same path only if pinning would not change them either.
    let sent = clicks.clone();
    let module = repairing.as_ref().map_or(module, |old| old.module.clone());
    // The run's own area of this name, which this save may update.
    let existing = match nav::find_area(&navfile, &name) {
        _ if repairing.is_some() => None,
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
    // read as a run's trip reads it - and read before a repair's try of the
    // saved path moves the page.
    let href = crate::browser::page::eval_value(d, "location.href").await;
    let href = href.ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
    let arrived = if href.trim().is_empty() { String::new() } else { nav::path_of(&href) };
    if arrived.is_empty() {
        return (409, "the page would not say where it is - read the page and try again".to_string());
    }
    // Where the screen is, for a mapping run's summary, however this ends.
    let screen = crate::commands::autorun::MappingScreen { arrived: Some(arrived.clone()), menu: nav::menu_path(&clicks) };
    let home = nav::Home::of(&recipe);
    // A repair first takes the area's saved path from a fresh home, as a
    // run would, and goes no further when it still arrives.
    if let Some(old) = repairing.as_ref() {
        let went = match fresh_home_for_area(
            d,
            &home,
            timing,
            root,
            organization,
            project,
            account.as_deref(),
            p.lease,
            p.signed_in,
        )
        .await
        {
            Ok(went) => went,
            Err(refused) => return refused,
        };
        if !went.ok {
            return (409, format!("the area's saved path could not be tried: {}", went.detail));
        }
        let route = nav::Route::new(&recipe, old.clone());
        let tried = nav::reach_module(d, &route, nav::TripFrom::SignIn, timing, "Auto Run area repair", account.as_deref()).await;
        if tried.ok {
            return (409, area_repair::NOTHING_TO_REPAIR.to_string());
        }
        if tried.harness {
            return (
                409,
                format!(
                    "the browser stopped answering while the area's saved path was tried, so nothing was saved: {}",
                    tried.detail
                ),
            );
        }
    }
    // The check starts from a fresh home page, never from where the page
    // stands (`fresh_home_for_area`).
    let went = match fresh_home_for_area(
        d,
        &home,
        timing,
        root,
        organization,
        project,
        account.as_deref(),
        p.lease,
        p.signed_in,
    )
    .await
    {
        Ok(went) => went,
        Err(refused) => return refused,
    };
    let reached = if went.ok {
        let start = match crate::browser::page::eval_value(d, "location.href").await {
            Ok(v) => nav::path_of(v.as_str().unwrap_or("")),
            Err(_) => String::new(),
        };
        // The menu preferences in force, which the area's clicks are for: a
        // project whose recipe names them records them with the area. A
        // dialog that cannot be read leaves them out; the area is saved
        // without.
        let preferences = match recipe.menu_preferences.as_ref() {
            Some(prefs) => match crate::autorun::menu_setup::read_preferences(d, prefs, timing).await {
                Ok(read) => Some(read),
                Err(why) => {
                    crate::applog::warn(format!("area {name}: the menu preferences could not be read: {why}"));
                    None
                }
            },
            None => None,
        };
        // The mode the account is in (a project whose recipe names modes):
        // the area is for that one.
        let mode = crate::autorun::menu_setup::read_mode(d, &recipe.modes, timing).await;
        let mut path = match (&repairing, &reason) {
            (Some(old), Some(reason)) => area_repair::repaired(
                old,
                clicks,
                arrived,
                start,
                reason.clone(),
                crate::commands::autorun_record::now_iso(),
            ),
            _ => nav::ModulePath {
                // An update keeps the area's name as it was saved.
                area: existing.as_ref().map_or_else(|| name.clone(), |old| old.name().to_string()),
                module,
                clicks,
                arrived,
                recorded: crate::commands::autorun_record::now_iso(),
                start,
                made_by: if mapping { nav::MadeBy::Mapping } else { nav::MadeBy::Person },
                // A mapping run writes its own area again only when the path
                // changed, and an assistant's repair note describes the path
                // it replaced: it goes with that path.
                repaired_by_assistant: None,
                // What an area already saved keeps when a read fails below.
                preferences: existing.as_ref().map(|old| old.preferences.clone()).unwrap_or_default(),
                mode: existing.as_ref().and_then(|old| old.mode.clone()),
                // The menu paths for other menu shapes stay with the area.
                variants: existing.as_ref().map(|old| old.variants.clone()).unwrap_or_default(),
            },
        };
        if let Some(read) = preferences {
            path.preferences = read;
        }
        if mode.is_some() {
            path.mode = mode;
        }
        // The clicks are saved as the trip made them: each finds exactly
        // one element on the page (`nav::go_to_module_pinning`).
        let mut route = nav::Route::new(&recipe, path);
        match nav::go_to_module_pinning(d, &route, timing).await {
            Ok((at, clicks, notes)) => {
                route.path.clicks = clicks;
                Ok((at, route.path, notes))
            }
            Err(failure) => Err(failure),
        }
    } else {
        Err(nav::PathFailure { at: nav::Where::Home, reason: went.detail, harness: went.harness })
    };
    let (at, path, pinned) = match reached {
        Ok(found) => found,
        Err(failure) => {
            // The saved path again fails the way it did: nothing new to try.
            let refused_pin = failure.reason.contains("counting hidden ones");
            if !refused_pin && repairing.as_ref().is_some_and(|old| old.clicks == sent) {
                return (409, area_repair::SAME_PATH.to_string());
            }
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
    if repairing.as_ref().is_some_and(|old| old.clicks == path.clicks) {
        return (409, area_repair::SAME_PATH.to_string());
    }
    // The run's own area, found where it was: nothing to write. Its saved
    // clicks stand when they are the clicks found now, even when pinning
    // would only make a name exact; they do not when one now matches more
    // than one element and had to be scoped.
    let scoped = pinned.iter().any(|note| note.contains(nav::SCOPED_NOTE));
    let found_as_saved = |old: &nav::ModulePath| old.clicks == path.clicks || (old.clicks == sent && !scoped);
    if let Some(old) = existing.as_ref().filter(|old| found_as_saved(old) && old.arrived == path.arrived) {
        let kept = old.name().to_string();
        if let Some(state) = p.discovery.as_mut() {
            if let Some(run) = state.mapping.as_mut() {
                run.record_unchanged(kept.clone());
                run.locate_last(screen);
            }
        }
        // The same clicks, but the mode or the menu preferences read now are
        // not what the area holds (an area saved before they were kept, or
        // one recorded in another state): only those are written, the saved
        // clicks stay as they are.
        let (mode_differs, prefs_differ) = (old.mode != path.mode, old.preferences != path.preferences);
        if mode_differs || prefs_differ {
            let mut updated = old.clone();
            updated.preferences = path.preferences.clone();
            updated.mode = path.mode.clone();
            if let Err(why) = nav::put_path_if(root, organization, project, updated, nav::AreaExpect::Was(old)) {
                return (409, why);
            }
            stand_in_saved_area(d, p.discovery, p.signed_in.as_deref(), root, organization, project, kept.clone()).await;
            let what = match (mode_differs, prefs_differ) {
                (true, true) => "mode/menu preferences",
                (true, false) => "mode",
                _ => "menu preferences",
            };
            crate::applog::info(format!("Auto Run area {kept}: recorded its {what}"));
            let message = format!("unchanged path; recorded its {what}");
            return (200, serde_json::json!({ "saved": true, "arrived": at, "message": message }).to_string());
        }
        stand_in_saved_area(d, p.discovery, p.signed_in.as_deref(), root, organization, project, kept).await;
        crate::applog::info("Auto Run mapping run found an area unchanged");
        return (200, serde_json::json!({ "saved": false, "unchanged": true, "arrived": at }).to_string());
    }
    let saved = path.clicks.len();
    let preferences = path.preferences.clone();
    let mode = path.mode.clone();
    let new_menu = nav::menu_path(&path.clicks);
    let written = match (&repairing, path.repaired_by_assistant.as_ref()) {
        // A repair goes to the file as it is now (`area_repair::save_repair`).
        (Some(old), Some(note)) => area_repair::save_repair_recording(
            root,
            organization,
            project,
            old,
            path.clicks.clone(),
            path.arrived.clone(),
            path.start.clone(),
            note.reason.clone(),
            note.at.clone(),
            Some((path.preferences.clone(), path.mode.clone())),
        ),
        // The area as it was read before the trips decides the write: an
        // area saved or changed since is not written over.
        _ => match existing.as_ref() {
            Some(old) => nav::put_path_if(root, organization, project, path, nav::AreaExpect::Was(old)),
            None => nav::put_path_if(root, organization, project, path, nav::AreaExpect::New),
        },
    };
    if let Err(why) = written {
        return (409, why);
    }
    // The area whose path this save replaced, if any.
    let replaced = existing.as_ref().or(repairing.as_ref());
    if let Some(state) = p.discovery.as_mut() {
        if let Some(run) = state.mapping.as_mut() {
            match replaced {
                Some(old) => run.record_updated(old.name().to_string(), nav::menu_path(&old.clicks), new_menu),
                None => run.record_added(name.clone()),
            }
            run.locate_last(screen);
        }
    }
    let kept = match replaced {
        Some(old) => old.name().to_string(),
        None => name,
    };
    stand_in_saved_area(d, p.discovery, p.signed_in.as_deref(), root, organization, project, kept.clone()).await;
    if let Some(reason) = reason {
        crate::applog::info(format!("AI repaired area {kept}: {reason}"));
        let message = area_repair::repaired_answer(&kept, &reason);
        let mut answer = serde_json::json!({ "saved": true, "repaired": true, "arrived": at, "message": message });
        if !pinned.is_empty() {
            answer["pinned"] = serde_json::json!(pinned);
        }
        return (200, answer.to_string());
    }
    crate::applog::info(format!("Auto Run discovery saved an area ({saved} clicks)"));
    let mut answer = serde_json::json!({ "saved": true, "arrived": at });
    if !pinned.is_empty() {
        answer["pinned"] = serde_json::json!(pinned);
    }
    if !preferences.is_empty() {
        answer["preferences"] = serde_json::json!(preferences);
    }
    if let Some(mode) = mode {
        answer["mode"] = serde_json::json!(mode);
    }
    (200, answer.to_string())
}

/// `/autorun-discover-start`: open the Auto Run browser for the assistant
/// and sign in as `account` (`discover_start_in`). Refused, before
/// anything opens, while anything else holds the browser, and for an
/// account or recipe that is not there. `holder` is the assistant whose call
/// it is: the browser opened in slot `n` stays its own through any browser it
/// replaces.
async fn autorun_discover_start(
    ctx: &BridgeContext,
    body: &str,
    seen: &mut Option<crate::autorun::overrides::Overrides>,
    n: SlotNo,
    holder: Option<&AgentKey>,
) -> (u16, String) {
    const SHAPE: &str = "{ \"account\": <an account key>, \"area\": <an area name, optional>, \"browser\": \"edge\" | \"chrome\", optional, \"mapping\": true | false, optional, \"modules\": [<a module name>], for a mapping run, \"case_id\": <the case explored for, optional>, \"no_save\": true | false, optional, \"explore\": true | false, optional }";
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
    let explore = match v.get("explore") {
        None | Some(serde_json::Value::Null) => false,
        Some(serde_json::Value::Bool(b)) => *b,
        Some(serde_json::Value::String(s)) if matches!(s.trim().to_ascii_lowercase().as_str(), "true" | "false") => {
            s.trim().eq_ignore_ascii_case("true")
        }
        Some(_) => return (400, format!("explore is true or false. Expected {SHAPE}.")),
    };
    if explore && mapping {
        return (400, EXPLORE_NOT_MAPPING.to_string());
    }
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
    // One mapping run at a time, whoever runs it and in whichever browser:
    // claimed before anything opens, refused at once naming whose browser it
    // is going in, and given back when this start returns (failed or not).
    let _mapping_start = match mapping.is_some().then(|| crate::commands::autorun::claim_mapping_start(n)) {
        Some(Err(going)) => return (409, mapping_refusal(n, going).unwrap_or_else(|| mapping_going("an Auto Run browser"))),
        Some(Ok(start)) => Some(start),
        None => None,
    };
    if let Err(why) = crate::commands::autorun::refuse_discovery_while_busy(n) {
        return (409, why);
    }
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    // A discovery for a case that must not save is guarded from its
    // sign-in on: settled before a browser opens.
    let asked = match discovery_for(&root, body) {
        Ok(asked) => crate::commands::autorun::DiscoveryFor { explore, ..asked },
        Err(refused) => return refused,
    };
    // No account, no recipe: said before a browser opens for nothing.
    if let Err(why) = crate::autorun::signin::prepare(&root, &ctx.org, &ctx.project, &account) {
        return (409, why);
    }
    // None named: a browser the replay left is kept, whichever it is, and a
    // new one is the one last chosen (`open_for_discovery`). A guarded
    // discovery's guard is on in a browser it took over before the lock goes.
    let from = GuardFrom { root: &root, organization: &ctx.org, project: &ctx.project };
    let made_way = match crate::commands::autorun::open_for_discovery(browser.as_deref(), mapping, asked, from, n, holder).await {
        Ok(made_way) => made_way,
        Err(why) => return (409, why),
    };
    let mut slot = crate::commands::autorun::session_slot(n).lock().await;
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
    crate::commands::autorun::publish_discovery_in(n, &slot);
    // A mapping run's guard pauses every request: the browser is answered
    // between the assistant's calls too.
    if slot.as_ref().is_some_and(|s| s.cdp.is_guarding_saves()) {
        crate::commands::autorun::answer_between_commands_in(n);
    }
    *seen = Some(crate::commands::autorun::overrides_of(&slot));
    said_made_way(made_way, answer)
}

/// The case a discovery call names (`case_id`, optional) and whether it
/// said `no_save` (optional). `no_save` is also true when the case's saved
/// script on this machine is marked `no_save`, so naming such a case is
/// enough. A script that cannot be read refuses the call: a case that may
/// be marked `no_save` is never explored unguarded.
pub fn discovery_for(root: &std::path::Path, body: &str) -> Result<crate::commands::autorun::DiscoveryFor, (u16, String)> {
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
    let case_id = match v.get("case_id") {
        None | Some(serde_json::Value::Null) => None,
        Some(id) => Some(
            id.as_i64()
                .and_then(|n| i32::try_from(n).ok())
                .filter(|n| *n > 0)
                .ok_or((400, "case_id must be a test case id: a positive number".to_string()))?,
        ),
    };
    let said = match v.get("no_save") {
        None | Some(serde_json::Value::Null) => false,
        Some(serde_json::Value::Bool(b)) => *b,
        Some(_) => return Err((400, "no_save is true or false".to_string())),
    };
    let marked = match case_id {
        None => false,
        Some(id) => crate::autorun::store::load_script(root, id).map_err(|why| (409, why))?.is_some_and(|s| s.no_save),
    };
    Ok(crate::commands::autorun::DiscoveryFor { case_id, no_save: said || marked, explore: false })
}

/// Put what a discovery call named (`discovery_for`) on the discovery in
/// `slot`, for that call: when it said `no_save` (or named a case marked
/// so), that call is guarded (`DiscoveryState::call_no_save`), for the case
/// the call named (`DiscoveryState::call_case_id`) - the discovery does not
/// become one that must not save. The first case named is the one it
/// explores for, and a case marked `no_save` keeps its discovery guarded
/// from then on (`discovery_must_not_save`). Nothing to do with no
/// discovery going.
pub fn mark_discovery_for<B: DiscoveryBrowser>(slot: &mut Option<B>, asked: crate::commands::autorun::DiscoveryFor) {
    let Some(state) = slot.as_mut().and_then(|b| b.parts().discovery.as_mut()) else {
        return;
    };
    state.call_no_save = asked.no_save;
    state.call_case_id = asked.case_id;
    if state.case_id.is_none() {
        state.case_id = asked.case_id;
    }
}

/// Whether a discovery must not save: it was told so, or the case it
/// explores for is marked `no_save` now (read again each time, so a mark
/// added on the Setup tab counts from the next action). A script that
/// cannot be read counts as marked.
pub fn discovery_must_not_save(root: &std::path::Path, state: &crate::commands::autorun::DiscoveryState) -> bool {
    state.no_save
        || state
            .case_id
            .is_some_and(|id| crate::autorun::store::load_script(root, id).map_or(true, |s| s.is_some_and(|s| s.no_save)))
}

/// A discovery start's answer, saying first what it made of the browser the
/// assistant's replay left open (`MadeWay`): took it over (a `"took_over"`
/// field) or closed it (a `"closed"` field) beside the JSON of one that
/// started, the sentence before the text of one that did not.
pub fn said_made_way(made_way: MadeWay, (status, text): (u16, String)) -> (u16, String) {
    let (field, sentence) = match made_way {
        MadeWay::Free => return (status, text),
        MadeWay::TookOver => ("took_over", TOOK_OVER_LEFT_OPEN),
        MadeWay::Closed => ("closed", CLOSED_LEFT_OPEN),
    };
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(mut v @ serde_json::Value::Object(_)) => {
            v[field] = serde_json::json!(sentence);
            (status, v.to_string())
        }
        _ => (status, format!("{sentence} {text}")),
    }
}

/// `/autorun-discover-action`: one action in the discovery's browser
/// (`discover_action_in`). Refused as a tried action is (`sign_in`, a local
/// file), and a `navigate` or `open_tab` to a site outside the sign-in
/// recipe's, before the browser is touched. A `use_component` may carry a
/// `draft` component, tried in place of the saved one.
async fn autorun_discover_action(
    ctx: &BridgeContext,
    body: &str,
    seen: &mut Option<crate::autorun::overrides::Overrides>,
    n: SlotNo,
) -> (u16, String) {
    const SHAPE: &str = "{ \"action\": <one script action>, \"draft\": <a component, optional, for a use_component>, \"area\": <an area name, optional>, \"case_id\": <the case explored for, optional>, \"no_save\": true | false, optional }";
    let raw = match body_field(body, "action", SHAPE) {
        Ok(v) => v,
        Err(refused) => return refused,
    };
    if let Some(r) = crate::autorun::forbidden_fixes::forbidden_in(&raw) {
        return (400, r);
    }
    let (action, wait_notes) = match tried_action_from(raw) {
        Ok(read) => read,
        Err(refused) => return (400, refused),
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
    let asked = match discovery_for(&root, body) {
        Ok(asked) => asked,
        Err(refused) => return refused,
    };
    let mut slot = crate::commands::autorun::session_slot(n).lock().await;
    mark_discovery_for(&mut slot, asked);
    let answer = discover_action_in(&mut slot, &root, &ctx.org, &ctx.project, &action, draft.as_ref(), area.as_deref()).await;
    let answer = with_wait_notes(answer, &wait_notes);
    let answer = let_go_if_silent(&mut slot, answer).await;
    crate::commands::autorun::publish_discovery_in(n, &slot);
    *seen = Some(crate::commands::autorun::overrides_of(&slot));
    answer
}

/// `/autorun-discover-actions`: several actions in the discovery's browser,
/// in order, under one hold of it (`discover_actions_in`). Every action is
/// checked as `/autorun-discover-action` checks one, all before the
/// browser is touched: a batch with one refused action runs none. A
/// `draft` is tried by each `use_component` that names it; `area` moves the
/// discovery before the first action.
async fn autorun_discover_actions(
    ctx: &BridgeContext,
    body: &str,
    seen: &mut Option<crate::autorun::overrides::Overrides>,
    n: SlotNo,
) -> (u16, String) {
    const SHAPE: &str = concat!("{ \"actions\": [<script actions, at most ", max_batch!(), ">], \"stop_on_failure\": <true or false, optional, true by default>, \"draft\": <a component, optional, for a use_component>, \"area\": <an area name, optional>, \"case_id\": <the case explored for, optional>, \"no_save\": true | false, optional }");
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
    let mut wait_notes = Vec::new();
    for (i, one) in raw.iter().enumerate() {
        if let Some(r) = crate::autorun::forbidden_fixes::forbidden_in(one) {
            return (400, format!("action {}: {r}", i + 1));
        }
        match tried_action_from(one.clone()) {
            Ok((a, notes)) => {
                wait_notes.extend(notes.into_iter().map(|n| format!("action {}: {n}", i + 1)));
                actions.push(a);
            }
            Err(refused) => return (400, format!("action {}: {refused}", i + 1)),
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
    let asked = match discovery_for(&root, body) {
        Ok(asked) => asked,
        Err(refused) => return refused,
    };
    // A stop asked for before this batch is not this batch's.
    batch_stop(n).store(false, std::sync::atomic::Ordering::SeqCst);
    let _going = BatchGoing::begin(n);
    // Held for the whole batch: nothing else drives the browser between
    // its actions. End discovery, Close browser or a release asks it to
    // give way (`BATCH_STOPS`). A gone browser is let go inside, keeping
    // the lines of the actions that ran.
    let mut slot = crate::commands::autorun::session_slot(n).lock().await;
    mark_discovery_for(&mut slot, asked);
    let answer = discover_actions_paged(
        &mut slot,
        &root,
        &ctx.org,
        &ctx.project,
        &actions,
        draft.as_ref(),
        area.as_deref(),
        stop_on_failure,
        batch_stop(n),
        page_wanted(body),
    )
    .await;
    let answer = with_wait_notes(answer, &wait_notes);
    crate::commands::autorun::publish_discovery_in(n, &slot);
    *seen = Some(crate::commands::autorun::overrides_of(&slot));
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
    n: SlotNo,
) -> (u16, String) {
    use crate::autorun::components::{users_of, Component, UserCases};
    const SHAPE: &str = "{ \"name\": <its name>, \"description\": <what it does>, \"inputs\": [{ \"name\", \"kind\": \"text\" or \"target\", \"description\" }], \"actions\": [<script actions>], \"why\": <why it changes, for a saved one> }";
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return (400, format!("that is not readable JSON: {e}. Expected {SHAPE}.")),
    };
    if let Some(r) = v.get("actions").and_then(crate::autorun::forbidden_fixes::forbidden_in) {
        return (400, r);
    }
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
    let mut slot = crate::commands::autorun::session_slot(n).lock().await;
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
        return with_delete_notes(component_answer(saved), &draft);
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

/// A component save's answer with `notes` added when its actions delete by
/// position (a warning, never a refusal).
fn with_delete_notes(answer: (u16, String), draft: &crate::autorun::components::Component) -> (u16, String) {
    let notes = crate::autorun::script_notes::positional_delete_notes_for_actions(&draft.actions);
    if answer.0 != 200 || notes.is_empty() {
        return answer;
    }
    match serde_json::from_str::<serde_json::Value>(&answer.1) {
        Ok(mut v) if v.is_object() => {
            v["notes"] = serde_json::json!(notes);
            (answer.0, v.to_string())
        }
        _ => answer,
    }
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

/// The case id a script route names: a positive whole number.
fn script_case_id(found: Option<serde_json::Value>, shape: &str) -> Result<i32, (u16, String)> {
    let id = match found {
        Some(serde_json::Value::Number(n)) => n.as_i64(),
        Some(serde_json::Value::String(s)) => s.trim().parse::<i64>().ok(),
        _ => None,
    };
    match id.and_then(|n| i32::try_from(n).ok()).filter(|n| *n > 0) {
        Some(id) => Ok(id),
        None => Err((400, format!("\"case_id\" is a test case's id, a positive whole number. Expected {shape}."))),
    }
}

/// `GET /autorun-script?case_id=N`: case N's saved script, whole, as the
/// JSON `save_autorun_script` takes for one entry. 404 when the case has
/// none on this machine.
fn autorun_script_read(target: &str) -> (u16, String) {
    match autorun_root() {
        Ok(root) => autorun_script_read_at(&root, target),
        Err(refused) => refused,
    }
}

/// `autorun_script_read` against the store at `root`.
pub fn autorun_script_read_at(root: &std::path::Path, target: &str) -> (u16, String) {
    const SHAPE: &str = "?case_id=<the test case id>";
    let case_id = match script_case_id(q(target, "case_id").map(serde_json::Value::String), SHAPE) {
        Ok(id) => id,
        Err(refused) => return refused,
    };
    match crate::autorun::store::load_script(root, case_id) {
        Ok(Some(script)) => (200, serde_json::to_string_pretty(&script).unwrap_or_default()),
        Ok(None) => (404, crate::autorun::store::no_script(case_id)),
        Err(e) => {
            crate::applog::warn(format!("Auto Run: the script of case {case_id} could not be read: {e}"));
            (500, format!("the script of case {case_id} could not be read - see Settings, Logs"))
        }
    }
}

/// `POST /autorun-script-delete`, body `{ case_id }`: the script is moved
/// aside, not removed (`store::delete_script`), and scripts kept aside more
/// than 30 days are purged first. Refused while the case is being replayed.
/// A local file move: nothing reaches Azure DevOps.
fn autorun_script_delete(body: &str) -> (u16, String) {
    match autorun_root() {
        Ok(root) => autorun_script_delete_at(&root, body, crate::autorun::sessions::now_ms()),
        Err(refused) => refused,
    }
}

/// `autorun_script_delete` against the store at `root`, at `now` (epoch ms).
pub fn autorun_script_delete_at(root: &std::path::Path, body: &str, now: u64) -> (u16, String) {
    const SHAPE: &str = "{ \"case_id\": <the test case id> }";
    let case_id = match body_field(body, "case_id", SHAPE).and_then(|v| script_case_id(Some(v), SHAPE)) {
        Ok(id) => id,
        Err(refused) => return refused,
    };
    crate::autorun::store::purge_deleted_scripts(root, now);
    match crate::autorun::store::delete_script(root, case_id, now) {
        Ok(_) => {
            crate::applog::info(format!("Auto Run: the script of case {case_id} was deleted by the assistant (kept aside for 30 days)"));
            (
                200,
                serde_json::json!({
                    "deleted": case_id,
                    "sentence": format!("The script of case {case_id} is deleted. It is kept aside on this machine for {} days, then purged.", crate::autorun::store::DELETED_KEPT_DAYS),
                })
                .to_string(),
            )
        }
        Err(why) if why == crate::autorun::store::no_script(case_id) => (404, why),
        Err(why) if why == crate::autorun::store::replaying_refusal(case_id) => (409, why),
        Err(why) => {
            crate::applog::warn(format!("Auto Run: the script of case {case_id} could not be deleted: {why}"));
            (500, format!("the script of case {case_id} could not be deleted - see Settings, Logs"))
        }
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
pub(crate) fn refuse_outside_the_recipe(
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
async fn autorun_discover_area(ctx: &BridgeContext, body: &str, n: SlotNo) -> (u16, String) {
    const SHAPE: &str = "{ \"name\": <the area's name>, \"module\": <the test-case Module it belongs to>, \"clicks\": [<a script's click selector>, ...], \"repair\": <optional: { \"reason\": <one line> } to replace an existing area's path>, \"variant\": <optional: true to add these clicks to the existing area as another menu path> }";
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return (400, format!("that is not readable JSON: {e}. Expected {SHAPE}.")),
    };
    let Some(name) = named(v.get("name").and_then(|a| a.as_str())) else {
        return (400, format!("this call needs a \"name\" for the area. Expected {SHAPE}."));
    };
    let repair = match v.get("repair") {
        None | Some(serde_json::Value::Null) => None,
        Some(r) => match r.get("reason").and_then(|a| a.as_str()) {
            Some(reason) => Some(reason.to_string()),
            None => return (400, format!("\"repair\" is {{ \"reason\": <one line> }}. Expected {SHAPE}.")),
        },
    };
    let variant = match v.get("variant") {
        None | Some(serde_json::Value::Null) => false,
        Some(serde_json::Value::Bool(b)) => *b,
        Some(_) => return (400, format!("\"variant\" is true or false. Expected {SHAPE}.")),
    };
    if variant && repair.is_some() {
        return (400, format!("a menu path is added with \"variant\" or an area is repaired with \"repair\", not both. Expected {SHAPE}."));
    }
    // A repair keeps the area's own module, and so does a menu path added to
    // an existing area: either may leave it out.
    let module = match named(v.get("module").and_then(|a| a.as_str())) {
        Some(module) => module,
        None if repair.is_some() || variant => String::new(),
        None => return (400, format!("this call needs the \"module\" the area belongs to. Expected {SHAPE}.")),
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
    let mut slot = crate::commands::autorun::session_slot(n).lock().await;
    if variant {
        return variant_area_in(
            &mut slot,
            &root,
            &ctx.org,
            &ctx.project,
            &name,
            &module,
            clicks,
            &crate::browser::timing::Timing::supervised(),
        )
        .await;
    }
    if let Some(reason) = repair {
        return repair_area_in(
            &mut slot,
            &root,
            &ctx.org,
            &ctx.project,
            &name,
            &module,
            clicks,
            &reason,
            &crate::browser::timing::Timing::supervised(),
        )
        .await;
    }
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
    /// Run the replay in slot `slot`'s supervised browser, by the same path
    /// as the person's Replay to step button, as the assistant's: it never lifts a
    /// guard held for another case, and a discovery holding the browser is
    /// ended first rather than refusing (`end_discovery_for_replay`). `Err`
    /// is a browser that would not open.
    ///
    /// `holder` is the assistant whose call it is, if it is one: the browser
    /// the replay uses stays bound to it through a browser it replaces. The
    /// default caller passes none and is never bound.
    fn replay(
        &self,
        slot: SlotNo,
        holder: Option<AgentKey>,
        organization: String,
        project: String,
        req: crate::autorun::replay_to::ReplayRequest,
    ) -> HostFuture<'_, Result<AssistantReplay, String>>;
    /// The page the replay left slot `slot`'s browser on, as
    /// `/autorun-page` answers for this organization and project.
    fn page(&self, slot: SlotNo, organization: String, project: String) -> HostFuture<'_, (u16, String)>;
}

/// How an assistant's replay ended, and the discovery it ended before it
/// took the browser, if any.
#[derive(Debug, Clone, PartialEq)]
pub struct AssistantReplay {
    pub end: crate::autorun::replay_to::ReplayEnd,
    pub discovery: DiscoveryEnded,
    /// The supervised browser the replay leaves open, if it does: its
    /// answer says what that blocks (`replay_to::left_open_line`).
    pub left_open: Option<crate::autorun::replay_to::LeftOpen>,
    /// Where the replay started (`replay_to::carry_from`): its answer says
    /// it first.
    pub from: crate::autorun::replay_to::ReplayFrom,
    /// What it made of a browser already signed in as the case's account:
    /// its answer says so next (`replay_to::Reuse::said`).
    pub reuse: crate::autorun::replay_to::Reuse,
    /// What the assistant had changed for testing in the browser when the
    /// replay ended (`autorun::overrides`): its answer ends naming it - a
    /// replay that passed says it passed with these changes in place.
    pub overrides: crate::autorun::overrides::Overrides,
}

static REPLAY_HOST: std::sync::OnceLock<Box<dyn ReplayHost>> = std::sync::OnceLock::new();

/// Called once by the app when the bridge starts. Later calls are ignored.
pub fn set_replay_host(host: Box<dyn ReplayHost>) {
    let _ = REPLAY_HOST.set(host);
}

/// Said when a replay is asked for and the app has lent the bridge no host.
const NO_REPLAY_HOST: &str = "the app could not start replays this session - restart the app";

/// Replay a case to a step in the supervised browser, for the assistant.
async fn autorun_replay(ctx: &BridgeContext, body: &str, n: SlotNo, holder: Option<&AgentKey>) -> (u16, String) {
    let Some(host) = REPLAY_HOST.get() else {
        return (503, NO_REPLAY_HOST.to_string());
    };
    autorun_replay_as(holder, ctx, n, body, host.as_ref(), REPLAY_ANSWER_WAIT).await
}

/// How long `replay_autorun_to_step` waits for its replay to reach its stop
/// point before it answers that the replay is still running. Kept under
/// the wait of an assistant's own tool call.
pub const REPLAY_ANSWER_WAIT: std::time::Duration = std::time::Duration::from_secs(45);

/// An assistant's replay, running apart from the call that started it,
/// and its answer once it ends. Each slot keeps its own (`ASSISTANT_RUNS`). The answer is kept here until a call for
/// the same case and step takes it, so a replay that ends while no call
/// waits never loses it; a replay for another case replaces a finished one.
struct AssistantRun {
    id: u64,
    case_id: i32,
    step: i32,
    /// The script's `saved_at` when the replay started.
    saved_at: Option<String>,
    answer: tokio::sync::watch::Receiver<Option<ReplayDone>>,
    /// Whether the call that started it wanted the page (`page_wanted`): an
    /// answer the replay tool's `stop` collects honours it too.
    page: bool,
}

/// A finished assistant's replay: its answer, when it ended, and how many
/// times its slot's supervised browser had been used then
/// (`commands::autorun::session_uses_in`).
#[derive(Clone)]
struct ReplayDone {
    answer: (u16, String),
    ended: std::time::Instant,
    uses: u64,
}

/// The script's `saved_at` now, for case `case_id`.
fn script_saved_at(root: &std::path::Path, case_id: i32) -> Option<String> {
    crate::autorun::store::load_script(root, case_id).ok().flatten().and_then(|s| s.saved_at)
}

/// Whether `run` may still be handed to a call: running, or ended no more
/// than twice the call's `wait` ago (`REPLAY_ANSWER_WAIT` makes that 90 s)
/// with the script unchanged since it started and the browser not used
/// since it ended. An answer that fails any of these is stale: a new
/// replay starts instead.
fn still_fresh(run: &AssistantRun, n: SlotNo, root: &std::path::Path, wait: std::time::Duration) -> bool {
    let Some(done) = run.answer.borrow().clone() else {
        return true;
    };
    done.ended.elapsed() <= wait * 2
        && script_saved_at(root, run.case_id) == run.saved_at
        && crate::commands::autorun::session_uses_in(n) == done.uses
}

/// Each slot's assistant replay (`AssistantRun`), if any: one per browser.
/// Coordination only, not a cache. Never held across an await.
static ASSISTANT_RUNS: [std::sync::Mutex<Option<AssistantRun>>; SLOT_COUNT] =
    [const { std::sync::Mutex::new(None) }; SLOT_COUNT];

/// Numbers each assistant's replay, so a call takes only its own answer.
static NEXT_ASSISTANT_RUN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn assistant_run(n: SlotNo) -> std::sync::MutexGuard<'static, Option<AssistantRun>> {
    ASSISTANT_RUNS[n.index()].lock().unwrap_or_else(|e| e.into_inner())
}

/// Said when the assistant asks for a replay while its replay of another
/// case or step is still running.
pub fn replay_running_elsewhere(case_id: i32, step: i32) -> String {
    format!(
        "a replay of case {case_id} to step {step} is already running - call replay_autorun_to_step with case {case_id} and step {step} to wait for it, then ask for this one"
    )
}

/// Said, with 202, when the replay has not reached its stop point within
/// the call's wait: where it is, and how to keep waiting.
pub fn replay_still_running(case_id: i32, step: i32, at: i32, of: i32) -> String {
    replay_still_running_on(case_id, step, at, of, None, None)
}

/// Where a replay is, as its 202s say it: step `at` of `of` - the case's
/// clean-up (step `cleanup`) by that name - or, before step 1, signing in
/// and going to the area.
fn replay_now(at: i32, of: i32, cleanup: Option<i32>) -> String {
    match at {
        at if at > 0 && cleanup == Some(at) => "now at the clean-up".to_string(),
        at if at > 0 => format!("now at step {at} of {of}"),
        _ => "now signing in and going to the case's area".to_string(),
    }
}

/// `replay_still_running`, saying too which action of the step it is on
/// (`runner::ActionNow`), for how long, and whether one of the step's
/// actions failed. A replay never tries an action again: a failed one is
/// pictured, the rest of its step is not run, and the replay ends at that
/// step. The case's clean-up (step `cleanup`) is said by that name.
pub fn replay_still_running_on(
    case_id: i32,
    step: i32,
    at: i32,
    of: i32,
    action: Option<&crate::autorun::runner::ActionNow>,
    cleanup: Option<i32>,
) -> String {
    let mut now = replay_now(at, of, cleanup);
    if let Some(a) = action.filter(|a| at > 0 && a.index > 0) {
        let what = if a.target.is_empty() { a.kind.clone() } else { format!("{} {}", a.kind, a.target) };
        let secs = a.since.map_or(0, |s| s.elapsed().as_secs());
        now.push_str(&format!(", on action {} of {} ({what}) for {secs} s", a.index, a.of));
        // A failed action stops its step: no later one of it begins, so
        // the one begun last is the one that failed.
        match a.failed {
            0 => now.push_str(", none of this step's actions has failed yet"),
            _ => now.push_str(&format!(", action {} failed - the replay ends at this step", a.index)),
        }
    }
    format!(
        "still running, {now} - call replay_autorun_to_step again with the same case ({case_id}) and step ({step}) to keep waiting"
    )
}

/// The 202 a replay call answers while its replay still runs: the sentence
/// (`replay_still_running_on`), the step and how many, and once a step's
/// action has begun, `action` (its place, kind, target and seconds on it),
/// `failed_in_step`, and `retries: false` (a replay never retries).
pub fn replay_still_running_answer(
    case_id: i32,
    step: i32,
    at: i32,
    of: i32,
    action: &crate::autorun::runner::ActionNow,
    cleanup: Option<i32>,
) -> serde_json::Value {
    let sentence = replay_still_running_on(case_id, step, at, of, Some(action), cleanup);
    let mut v = serde_json::json!({ "status": "running", "sentence": sentence, "step": at, "of": of });
    if at > 0 && action.index > 0 {
        v["action"] = serde_json::json!({
            "index": action.index,
            "of": action.of,
            "kind": action.kind,
            "target": action.target,
            "seconds": action.since.map_or(0, |s| s.elapsed().as_secs()),
        });
        v["failed_in_step"] = serde_json::json!(action.failed);
        v["retries"] = serde_json::json!(false);
    }
    v
}

/// Said when a replay ended without handing back its answer: its task
/// stopped before it could.
const REPLAY_LOST: &str = "the replay ended without an answer - see Settings, Logs, then replay again";

/// Wait up to `wait` for replay `id`'s answer. Once it is there it is
/// taken: the slot is freed for the next replay. Not there yet, the
/// answer is 202 with where the replay is now.
async fn wait_for_replay(
    n: SlotNo,
    mut answer: tokio::sync::watch::Receiver<Option<ReplayDone>>,
    id: u64,
    case_id: i32,
    step: i32,
    wait: std::time::Duration,
) -> (u16, String) {
    let got = tokio::time::timeout(wait, answer.wait_for(|a| a.is_some()))
        .await
        .map(|r| r.map(|a| a.as_ref().map(|done| done.answer.clone())));
    let taken = |answer: (u16, String)| {
        let mut slot = assistant_run(n);
        if slot.as_ref().is_some_and(|r| r.id == id) {
            *slot = None;
        }
        answer
    };
    match got {
        Ok(Ok(Some(done))) => taken(done),
        Ok(_) => taken((500, REPLAY_LOST.to_string())),
        Err(_) => {
            let replay = crate::autorun::replay_to::state(n);
            let (at, of) = replay.progress();
            let action = replay.action_now();
            let cleanup = replay.cleanup_at();
            (202, replay_still_running_answer(case_id, step, at, of, &action, cleanup).to_string())
        }
    }
}

/// `/autorun-replay`, body `{ case_id, step }`, in slot `slot`'s browser,
/// with its host and how long the call waits for the replay
/// (`REPLAY_ANSWER_WAIT`).
///
/// Refused, with the sentence, before anything opens: a body without the
/// two numbers, an unattended run going, a case with no saved script, a
/// step outside it, the person's replay going, or the assistant's replay of
/// another case or step still running (`replay_running_elsewhere`). Nothing
/// asks the person first: a script marked must not save replays at once,
/// its saves stopped throughout by its no-save guard (`guard_for_case`).
/// Database Access is the module, as for the person's own replay.
///
/// The replay runs apart from the call (`AssistantRun`). One that has not
/// reached its stop point within `wait` answers 202 with where it is
/// (`replay_still_running`); the same case and step asked for again waits
/// on that replay, up to `wait` more, and takes its answer - one that
/// ended in between hands it over at once.
///
/// A discovery holding the browser does not refuse it: the replay ends it
/// first (`end_discovery_for_replay`), and the answer's sentence starts
/// with `ENDED_DISCOVERY_FIRST`.
///
/// The answer is `{ "sentence" }`, and once the browser stands before the
/// step, or where the replay stopped at a failure (not while signing in:
/// the sign-in page shows the account's login), `"page"` beside it: the
/// page as `get_autorun_page` shows it, so the assistant can try the step
/// at once.
pub async fn autorun_replay_with(
    ctx: &BridgeContext,
    slot: SlotNo,
    body: &str,
    host: &'static dyn ReplayHost,
    wait: std::time::Duration,
) -> (u16, String) {
    autorun_replay_as(None, ctx, slot, body, host, wait).await
}

/// `autorun_replay_with` for the assistant `holder`, whose browser the
/// replay uses or opens in slot `slot`; none for the default caller, which
/// holds no browser and is never bound to one by its replay.
pub async fn autorun_replay_as(
    holder: Option<&AgentKey>,
    ctx: &BridgeContext,
    slot: SlotNo,
    body: &str,
    host: &'static dyn ReplayHost,
    wait: std::time::Duration,
) -> (u16, String) {
    let answer = replay_answer_with(holder, ctx, slot, body, host, wait).await;
    with_changes_on_a_refusal(answer, slot)
}

/// A replay's refusal (any 4xx or 5xx) that does not name what the
/// assistant changed for testing yet - one given before the replay took the
/// browser - names it, as last published for slot `slot`
/// (`commands::autorun::overrides_now_in`): a replay ended by the host names
/// it already, read under the lock.
pub fn with_changes_on_a_refusal(answer: (u16, String), slot: SlotNo) -> (u16, String) {
    use crate::autorun::overrides::{CHANGED_FOR_TESTING, PASSED_WITH_CHANGES};
    if answer.0 < 400 || answer.1.contains(CHANGED_FOR_TESTING) || answer.1.contains(PASSED_WITH_CHANGES) {
        return answer;
    }
    crate::autorun::overrides::with_note(answer, &crate::commands::autorun::overrides_now_in(slot))
}

/// `autorun_replay_with`, before a refusal names the test changes.
async fn replay_answer_with(
    holder: Option<&AgentKey>,
    ctx: &BridgeContext,
    n: SlotNo,
    body: &str,
    host: &'static dyn ReplayHost,
    wait: std::time::Duration,
) -> (u16, String) {
    use crate::autorun::replay_to::{self, ReplayRequest};
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
    // The same replay still running, or ended with a fresh answer not yet
    // taken (`still_fresh`): this call waits on it, and nothing new
    // starts. A stale answer is dropped, and a new replay starts below.
    let same = {
        let mut slot = assistant_run(n);
        match slot.as_ref().filter(|r| r.case_id == case_id && r.step == step) {
            Some(r) if still_fresh(r, n, &root, wait) => Some((r.answer.clone(), r.id)),
            Some(_) => {
                *slot = None;
                None
            }
            None => None,
        }
    };
    if let Some((answer, id)) = same {
        return wait_for_replay(n, answer, id, case_id, step, wait).await;
    }
    let db_read_access =
        crate::autorun::preconditions::read_access(!ctx.disabled_tools.iter().any(|t| t == "db_query"));
    let req = ReplayRequest { case_id, step, db_read_access };
    if let Err(why) = replay_to::check(&root, &ctx.org, &ctx.project, &req) {
        return (409, why);
    }
    // Looked at and claimed under one lock, so two calls cannot both start.
    let (tx, answer) = tokio::sync::watch::channel(None);
    let id = NEXT_ASSISTANT_RUN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let attach = {
        let mut slot = assistant_run(n);
        let fresh = slot.as_ref().is_some_and(|r| still_fresh(r, n, &root, wait));
        let running = slot.as_ref().map(|r| (r.case_id, r.step, r.id, r.answer.clone()));
        match running {
            Some((c, s, other, theirs)) if c == case_id && s == step && fresh => Some((theirs, other)),
            Some((c, s, _, theirs)) if theirs.borrow().is_none() => return (409, replay_running_elsewhere(c, s)),
            _ if replay_to::state(n).is_running() => return (409, replay_to::ALREADY_RUNNING.to_string()),
            _ => {
                let saved_at = script_saved_at(&root, case_id);
                *slot = Some(AssistantRun { id, case_id, step, saved_at, answer: answer.clone(), page: page_wanted(body) });
                replay_to::state(n).forget_stop_request();
                None
            }
        }
    };
    if let Some((theirs, other)) = attach {
        return wait_for_replay(n, theirs, other, case_id, step, wait).await;
    }
    let (org, project) = (ctx.org.clone(), ctx.project.clone());
    let holder = holder.cloned();
    let reader = match &holder {
        Some(key) => format!("{}:{}", key.id, key.name.as_deref().unwrap_or("")),
        None => String::new(),
    };
    let full = page_full(body);
    tokio::spawn(async move {
        let answer = run_assistant_replay(host, n, holder, org, project, req, reader, full).await;
        // Kept in the slot for the next call when none is waiting, while
        // it stays fresh (`still_fresh`).
        let done = ReplayDone {
            answer,
            ended: std::time::Instant::now(),
            uses: crate::commands::autorun::session_uses_in(n),
        };
        let _ = tx.send(Some(done));
    });
    wait_for_replay(n, answer, id, case_id, step, wait).await
}

/// Said by the replay tool's `stop` when no replay is going.
pub const NO_REPLAY_RUNNING: &str = "no replay is running - there is nothing to stop";

/// Said by the replay tool's `stop` while the replay going is the person's own.
pub const PERSONS_REPLAY_GOING: &str =
    "the replay going is the person's own - only they can stop it, with Stop in the Auto Run pane";

/// Said by the replay tool's `stop` when the assistant's last replay has
/// already ended, its answer not yet collected.
pub fn replay_already_ended(case_id: i32, step: i32) -> String {
    format!(
        "{NO_REPLAY_RUNNING}: the replay of case {case_id} to step {step} has ended - call replay_autorun_to_step with case {case_id} and step {step} for its answer"
    )
}

/// The 202 the replay tool's `stop` answers when the replay has not ended
/// within the call's wait: it was asked to stop, and hears it once the
/// action it is on finishes (an action's own wait for the page runs out
/// first).
pub fn replay_stopping_answer(n: SlotNo, case_id: i32, step: i32) -> serde_json::Value {
    let replay = crate::autorun::replay_to::state(n);
    let (at, of) = replay.progress();
    let action = replay.action_now();
    let mut now = replay_now(at, of, replay.cleanup_at());
    if at > 0 && action.index > 0 {
        now.push_str(&format!(", on action {} of {}", action.index, action.of));
    }
    let sentence = format!(
        "asked to stop: the replay of case {case_id} to step {step} ends once the action it is on finishes ({now}) - send replay_autorun_to_step stop: true again to wait for it"
    );
    serde_json::json!({ "status": "stopping", "sentence": sentence, "step": at, "of": of })
}

/// The 202 the replay tool's `stop` answers when the assistant's replay was
/// running no step to stop - it had not yet started, or had ended and its
/// answer was still being made ready - and no answer came within the wait.
/// Nothing was asked to stop.
pub fn replay_not_asked_answer(case_id: i32, step: i32) -> serde_json::Value {
    let sentence = format!(
        "not asked to stop: the replay of case {case_id} to step {step} was not running a step (it was starting, or had ended and its answer was being made ready) - send replay_autorun_to_step stop: true again to stop it, or replay_autorun_to_step with case {case_id} and step {step} for its answer"
    );
    serde_json::json!({ "status": "not_stopped", "sentence": sentence })
}

/// `/autorun-replay-stop`, the assistant's `replay_autorun_to_step` with `stop`, waiting
/// up to `wait` for the replay in slot `slot` to end. Its replay still going
/// is asked to stop through the replay's own stop
/// (`ReplayState::stop_on_request`, the one the person's Stop sets), which every replay hears before each
/// action, before each step and inside its waits; the answer is then the
/// replay's own, taken as `replay_autorun_to_step` takes it: stopped on
/// request, after which action of which step, with no page - or, had it
/// already stopped at a failing step, that failure with its page, or had
/// it already reached its step, that finished answer with its page. Not
/// ended within `wait`: 202 (`replay_stopping_answer`). Only the
/// assistant's replay running is asked to stop: between its end and its
/// answer the call only waits (202 `replay_not_asked_answer` past the
/// wait). No replay going: 404; the person's own: 409, never stopped.
pub async fn autorun_replay_stop_with(slot: SlotNo, wait: std::time::Duration) -> (u16, String) {
    let replay = crate::autorun::replay_to::state(slot);
    let mine = assistant_run(slot).as_ref().map(|r| (r.answer.clone(), r.id, r.case_id, r.step, r.page));
    match mine {
        Some((answer, id, case_id, step, page)) if answer.borrow().is_none() => {
            // Only the assistant's own replay going is asked to stop. Its
            // slot is freed before its answer is sent (the page is read in
            // between), and a replay the person starts in that gap clears
            // the stop and must never hear this one: with nothing of the
            // assistant's running, the call only waits for its answer.
            let asked = replay.assistants_replay_running();
            if asked {
                replay.stop_on_request();
                crate::applog::info(format!("Auto Run replay of case {case_id} to step {step}: the assistant asked it to stop"));
            }
            match wait_for_replay(slot, answer, id, case_id, step, wait).await {
                (202, _) if asked => (202, replay_stopping_answer(slot, case_id, step).to_string()),
                (202, _) => (202, replay_not_asked_answer(case_id, step).to_string()),
                // As the replay call that started it asked: `page: false`
                // leaves the page out here too.
                done if !page => without_page(done, r#"{"page":false}"#),
                done => done,
            }
        }
        _ if replay.is_running() => (409, PERSONS_REPLAY_GOING.to_string()),
        Some((_, _, case_id, step, _)) => (404, replay_already_ended(case_id, step)),
        None => (404, NO_REPLAY_RUNNING.to_string()),
    }
}

/// `s` with its first letter a capital, as a sentence after another one.
fn capitalised(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The assistant's replay through `host`, and its answer.
async fn run_assistant_replay(
    host: &'static dyn ReplayHost,
    n: SlotNo,
    holder: Option<AgentKey>,
    org: String,
    project: String,
    req: crate::autorun::replay_to::ReplayRequest,
    reader: String,
    full: bool,
) -> (u16, String) {
    use crate::autorun::replay_to::{ReplayEnd, ReplayFrom};
    let (case_id, step) = (req.case_id, req.step);
    let replay = crate::autorun::replay_to::state(n);
    let (end, discovery, left_open, from, reuse, overrides) = match host.replay(n, holder, org.clone(), project.clone(), req).await {
        Ok(AssistantReplay { end, discovery, left_open, from, reuse, overrides }) => {
            (end, discovery, left_open, from, reuse, overrides)
        }
        Err(why) => return (503, why),
    };
    // Where it started, said first: carried on, from the beginning, or
    // already there (then nothing ran, and that is all there is to say).
    // A browser signed in already says so next, where a sign-in would be.
    let reused = reuse.said(reuse.account.as_deref()).map(|line| format!("{line} ")).unwrap_or_default();
    // Stopped by the replay tool's `stop`: said as stopped on request, after the
    // last action to begin of the step it was on.
    let on_request = matches!(end, ReplayEnd::Stopped { .. }) && replay.stop_was_requested();
    // The case's clean-up is said as such, never as a step number.
    let cleanup = replay.cleanup_at();
    let ended = if on_request {
        let (at, _) = replay.progress();
        let action = replay.action_now();
        crate::autorun::replay_to::stopped_on_request(case_id, at, action.index, action.stopped_inside, cleanup)
    } else {
        end.sentence_in(cleanup)
    };
    // From the beginning for a reason another case's stand gave, it says so.
    let started = match (&reuse.not_carried, from) {
        (Some(why), ReplayFrom::Beginning) => format!("Started from the beginning: {why}"),
        _ => from.said(case_id, step),
    };
    let said = match (&end, from) {
        (ReplayEnd::Refused(_) | ReplayEnd::Blocked(_), _) => end.sentence(),
        (_, ReplayFrom::AlreadyThere) => from.said(case_id, step),
        _ => format!("{started} {reused}{}", capitalised(&ended)),
    };
    // Said before that when the replay ended the assistant's own discovery,
    // with the guard a no_save one leaves on.
    let said = match discovery.no_save {
        true => format!("{DISCOVERY_GUARD_KEPT} {said}"),
        false => said,
    };
    let mut sentence = after_ended_discovery(discovery.ended, &said);
    // A browser the replay leaves open, on a line of its own after it.
    // Not after the person's stop: Stop, Close or Release stopped it, and
    // the browser may be closing right behind the answer. A stop the
    // assistant asked for leaves it open, holding its account, so it is said.
    let refused_or_stopped = matches!(end, ReplayEnd::Refused(_) | ReplayEnd::Blocked(_))
        || (matches!(end, ReplayEnd::Stopped { .. }) && !on_request);
    if let (Some(left), false) = (&left_open, refused_or_stopped) {
        sentence.push('\n');
        sentence.push_str(&crate::autorun::replay_to::left_open_line(left));
    }
    // What the assistant changed for testing, last, so the result is never
    // taken for a normal run's: a replay that reached its step passed with
    // these changes in place.
    let changed = match end {
        ReplayEnd::Ready { .. } => overrides.passed_line(),
        _ => overrides.changed_line(),
    };
    if let Some(line) = changed {
        sentence.push('\n');
        sentence.push_str(&line);
    }
    // An ended explore session's unrecorded saves are said in the sentence,
    // so no arm of the answer drops them.
    if let Some(end) = discovery.explore.as_ref().filter(|e| !e.saves_not_on_the_change_list.is_empty()) {
        sentence.push_str(&format!(
            "\nThe explore session this replay ended sent saves that are not on its change list: {}. {}",
            end.saves_not_on_the_change_list.join(", "),
            crate::autorun::explore::UNRECORDED_SAVES
        ));
    }
    // An ended mapping run's summary goes beside the sentence, as
    // `end_autorun_discovery` hands it back; an ended explore session's end
    // beside it too.
    let with_summary = |mut answer: serde_json::Value| {
        if let Some(summary) = &discovery.summary {
            answer["summary"] = serde_json::json!(summary);
        }
        if let Some(end) = &discovery.explore {
            answer["explore"] = serde_json::json!(end);
        }
        answer.to_string()
    };
    match end {
        ReplayEnd::Refused(_) | ReplayEnd::Blocked(_) => (409, sentence),
        // A sign-in that stopped stands on the sign-in page, whose username
        // field shows the account's login: accounts are named by key, never
        // by login, so its page is not answered.
        ReplayEnd::StoppedAt { phase: crate::autorun::replay_to::ReplayPhase::SignIn, .. } => {
            (200, with_summary(serde_json::json!({ "sentence": sentence })))
        }
        // Where the browser stands: before the step, or at the failure it
        // stopped on (a browser that went is never `StoppedAt`).
        ReplayEnd::Ready { .. } | ReplayEnd::StoppedAt { .. } => {
            let answer = match compacted(n, &reader, full, host.page(n, org, project)).await {
                (200, page) => serde_json::json!({ "sentence": sentence, "page": page }),
                (_, why) => serde_json::json!({ "sentence": sentence, "page_unavailable": why }),
            };
            (200, with_summary(answer))
        }
        // Stop, Close or Release ended it, or the browser went: there is
        // no page to read, and the browser may be closing behind the answer.
        ReplayEnd::Stopped { .. } | ReplayEnd::BrowserGone { .. } => {
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

/// A component's try as one outcome, from each of its actions' (`steps`):
/// ok when every one passed; otherwise the first failure's detail, named by
/// its place and action, and its picture.
fn component_outcome(steps: &[(String, crate::browser::actions::ActionOutcome)]) -> crate::browser::actions::ActionOutcome {
    use crate::browser::actions::ActionOutcome;
    match steps.iter().enumerate().find(|(_, (_, o))| !o.ok) {
        None => {
            let n = steps.len();
            ActionOutcome::passed(if n == 1 { "its 1 action passed".to_string() } else { format!("all {n} of its actions passed") })
        }
        Some((i, (what, o))) => {
            // An action the failure stopped was never carried out.
            let failed = steps.iter().filter(|(_, o)| !o.ok && !o.detail.starts_with("not run:")).count();
            let more = if failed > 1 { format!(" ({failed} of {} failed)", steps.len()) } else { String::new() };
            let mut out = ActionOutcome::failed(format!("action {} ({what}) failed: {}{more}", i + 1, o.detail));
            out.screenshot = o.screenshot.clone();
            out.harness = o.harness;
            out
        }
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
    try_in_area(d, account, lease, root, organization, project, case_id, None, true, action).await
}

/// `try_in`, its sightings filed under `discovery_area` when one is given
/// (`recording_area`), and what it acted on filed in the discovery map
/// unless `files_finds` is false (an explore session).
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
    files_finds: bool,
    action: &crate::browser::actions::Action,
) -> (u16, String) {
    let guarded = d.is_guarding_saves();
    let mark = if guarded { Some(crate::browser::cdp::Driver::net_mark(d)) } else { None };
    let tried = try_action(d, account, lease, root, organization, project, case_id, discovery_area, false, files_finds, action, None).await;
    // The writes a guarded try sent that the guard let through, by method and path.
    let let_through: Vec<(String, String)> = match mark {
        Some(mark) => {
            let sent = crate::browser::cdp::Driver::net_since(d, mark);
            crate::browser::cdp::Driver::net_unmark(d, mark);
            sent.iter()
                .filter(|s| crate::browser::save_guard::SAVE_METHODS.contains(&s.method.as_str()))
                .filter(|s| !crate::browser::save_guard::stopped_by_guard(s))
                .map(|s| (s.method.clone(), crate::browser::save_guard::path_of(&s.path_query)))
                .collect()
        }
        None => Vec::new(),
    };
    let (outcome, steps, answered) = match tried {
        Ok(tried) => (tried.outcome, tried.steps, tried.answered),
        Err(refused) => return refused,
    };
    if files_finds {
        remember_tried(root, organization, project, discovery_area, case_id, &answered);
    }
    let mut text = format!("{}: {}", if outcome.ok { "ok" } else { "failed" }, outcome.detail);
    if let Some(shot) = &outcome.screenshot {
        text.push_str(&format!(" (picture: {})", crate::autorun::store::shot_path(root, shot).display()));
    }
    // A component's try answers each of its actions, as a discovery's does.
    for (i, (what, o)) in steps.iter().enumerate() {
        let detail = o.detail.trim();
        // One the failure before it stopped says so, and why, not "failed".
        if let Some(why) = detail.strip_prefix("not run: ").filter(|_| !o.ok) {
            text.push_str(&format!("\n{}. not run: {what} ({why})", i + 1));
            continue;
        }
        let said = if detail.is_empty() { what.clone() } else { format!("{what}: {}", one_short_line(detail)) };
        text.push_str(&format!("\n{}. {}: {said}", i + 1, if o.ok { "ok" } else { "failed" }));
    }
    if let Some(line) = crate::browser::save_guard::let_through_line(&let_through) {
        text.push_str(&format!("\nlet_through: {line}"));
    }
    (200, text)
}

/// What one action an assistant ran did: its outcome, and the dialog the
/// page raised while it ran, if any.
struct Tried {
    /// A `use_component`'s actions, each described with its outcome; empty
    /// for any other action.
    steps: Vec<(String, crate::browser::actions::ActionOutcome)>,
    outcome: crate::browser::actions::ActionOutcome,
    dialog: Option<crate::autorun::StepDialog>,
    /// Each action the try carried out - a component's own, or the one -
    /// described, with its own outcome: what the page answered each.
    answered: Vec<(String, crate::browser::actions::ActionOutcome)>,
}

/// What a try teaches of the application's rules, kept as a discovery's
/// (`discovery_map::rules_from`) under the area its sightings go to
/// (`recording_area`): every request the application refused its actions
/// up to the first that failed, and what the page showed after the nearest
/// one before that failure that changed the page. With no area known
/// nothing is kept, and a map that could not be written fails nothing.
fn remember_tried(
    root: &std::path::Path,
    organization: &str,
    project: &str,
    discovery_area: Option<&str>,
    case_id: i32,
    answered: &[(String, crate::browser::actions::ActionOutcome)],
) {
    use crate::autorun::discovery_map::{remember_refusals_logged, rules_from, ActionAnswer, RuleSource};
    // What the first failure stopped was never carried out.
    let ran = answered.iter().position(|(_, o)| !o.ok).map_or(answered.len(), |i| i + 1);
    let answers: Vec<ActionAnswer> = answered[..ran]
        .iter()
        .map(|(what, o)| ActionAnswer {
            page: o.page_path.clone().unwrap_or_default(),
            step: o.page_step.clone(),
            after: what.clone(),
            said: o.page_said.clone(),
            refused: o.refused.clone(),
            changed: o.page_path.is_some(),
            failed: !o.ok && !crate::autorun::runner::tcm_made(o),
        })
        .collect();
    let case = (case_id != 0).then_some(case_id);
    let rules = rules_from(&answers, RuleSource::Discovery, case, crate::autorun::sessions::now_ms());
    if rules.is_empty() || project.trim().is_empty() {
        return;
    }
    let area = recording_area(root, discovery_area, case);
    remember_refusals_logged(root, organization, project, area.as_deref(), rules);
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
    files_finds: bool,
    action: &crate::browser::actions::Action,
    noted: Option<&mut crate::autorun::runner::Noted>,
) -> Result<Tried, (u16, String)> {
    use crate::autorun::runner::{area_route, area_routes, named_areas, AreaRoute, InRun, NEEDS_SCRIPT_AREA};
    // A check that times a request from the action before it, or compares the page with the one its step began
    // on, has nothing to measure from when tried alone: it says so, and where to try it.
    if crate::autorun::api_checks::needs_its_step(action) {
        let outcome = crate::browser::actions::ActionOutcome::failed(format!(
            "{}{}",
            crate::browser::actions::CANNOT_RUN,
            crate::autorun::api_checks::TRY_IN_ITS_STEP
        ));
        let answered = vec![(describe_action(action), outcome.clone())];
        return Ok(Tried { outcome, dialog: None, steps: Vec::new(), answered });
    }
    // A bare `return_to_area` goes to the case's own area - in a discovery
    // (case 0, which has no script), the discovery's own area; one that
    // names an area goes to that one.
    // A `when_visible` may hold one in its `then`, to recover from a bounce.
    let bare = |a: &crate::browser::actions::Action| {
        matches!(a, crate::browser::actions::Action::ReturnToArea { .. }) && a.area_named().is_none()
    };
    let resolved = action.each().into_iter().any(bare)
        .then(|| {
            if case_id == 0 {
                crate::autorun::runner::discovery_area_route(root, organization, project, discovery_area)
            } else {
                area_route(root, organization, project, case_id)
            }
        });
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
    // A `wait_for`, and any legacy string target, passes on one match or
    // several and does not say which, so once the try worked each is
    // probed, as `probe_autorun_locator` does, and filed with the count the
    // probe gives (`visible_matches`). The probe never clicks or types.
    // A legacy string target is probed BEFORE the try, on the page it acts
    // on: a probe is read-only, and after a click that navigates it would
    // count the wrong page. A structured target that is one plain css link
    // matched one visible element, but its hidden matches are not known
    // (`NOT_ONE`), so it never vouches for a legacy use of its key.
    let (known, legacy): (Vec<_>, Vec<_>) = matched_targets(action).into_iter().partition(|(t, _)| !t.is_legacy());
    let mut matched: Vec<Matched> = known
        .into_iter()
        .map(|(t, n)| {
            let css = crate::autorun::discovery_map::plain_target(t) && t.links().first().is_some_and(|l| l.css.is_some());
            (t, n, if css { crate::autorun::discovery_map::NOT_ONE as usize } else { n })
        })
        .collect();
    for (target, _) in legacy {
        let text = crate::browser::snapshot::probe_exact(d, target).await.ok();
        match text {
            Some(text) => match counted(d, target, &text).await {
                Some((n, all)) => matched.push((target, n, all)),
                None => crate::applog::info("Auto Run try: a tried locator was not filed, as the probe before it saw nothing visible"),
            },
            None => crate::applog::info("Auto Run try: a tried locator was not filed, as the probe before it got no answer"),
        }
    }
    // A `wait_for` passes on one match or several and does not say which,
    // and what it waits for may not be there yet when the try starts: it is
    // probed once the try worked, and filed as found, its count not known,
    // when what it waited for is already gone. Waiting changes no page.
    let waited = match action {
        crate::browser::actions::Action::WaitFor { selector, .. } => Some(selector),
        _ => None,
    };
    // (A list check changes no page either, so its list is filed at the page
    // it is probed on, after the try.)
    let started_on = if matched.is_empty() && waited.is_none() && list_targets(action).is_empty() {
        String::new()
    } else {
        page_address(d).await.0
    };
    let step = crate::autorun::StepScript { step_number: 0, actions: vec![action.clone()], unchecked: None, cleanup: false, because: Vec::new() };
    // A try fails on a request the application refused: the assistant learns
    // the reason now, not from a wait that times out later.
    // A discovery's `noted` carries its last action that changed the page
    // to the next action, as a run's steps carry it.
    let mut run = InRun { areas: Some(&areas), saves_only_counted, refusals_fail: true, noted, ..Default::default() };
    // An action that never answers is a failed action, so everything that
    // follows (what its writes and finds filed) still runs. It is given its
    // own wait and a margin, and no more than the call has left.
    let timing = crate::browser::timing::Timing::supervised();
    let own = crate::autorun::action_help::wait_budget_ms(action, timing.nav_ms);
    let budget = crate::autorun::action_help::action_budget(own, crate::autorun::action_help::call_elapsed_ms());
    // With no time left the action is not started at all.
    let mut timed_out = false;
    let outcomes = match crate::autorun::action_help::time_to_run(budget) {
        Err(spent) => {
            timed_out = true;
            vec![crate::browser::actions::ActionOutcome::failed(spent)]
        }
        Ok(ms) => {
            let ran = crate::autorun::action_help::within_budget(
                ms,
                crate::autorun::runner::run_step_in_run(d, root, organization, project, &step, &timing, account, lease, None, area, &mut run),
            )
            .await;
            match ran {
                Ok(Ok(v)) => v,
                Ok(Err(why)) => return Err((500, why)),
                Err(seconds) => {
                    timed_out = true;
                    vec![crate::browser::actions::ActionOutcome::failed(crate::autorun::action_help::did_not_answer(seconds))]
                }
            }
        }
    };
    // What a component's waits and reads found is filed though a later
    // action of it failed: each passed (`component_finds`).
    if files_finds && matches!(action, crate::browser::actions::Action::UseComponent { .. }) {
        let recorded_in = recording_area(root, discovery_area, Some(case_id));
        file_component_finds(d, root, organization, project, recorded_in, std::slice::from_ref(action), &outcomes).await;
    }
    // A `use_component` runs as its component's actions, one outcome each:
    // every one is answered, and the try as a whole is ok only when all are.
    let steps: Vec<(String, crate::browser::actions::ActionOutcome)> = match action {
        // A timeout is one outcome for the whole component, labelled as the
        // component, not as its first action.
        crate::browser::actions::Action::UseComponent { .. } if timed_out => {
            vec![(describe_action(action), outcomes[0].clone())]
        }
        crate::browser::actions::Action::UseComponent { .. } => {
            let expanded = crate::autorun::components::expand_step(root, organization, project, std::slice::from_ref(action))
                .map(|(x, _)| x.into_iter().map(|(a, _)| describe_action(&a)).collect::<Vec<_>>())
                .unwrap_or_default();
            outcomes
                .iter()
                .enumerate()
                .map(|(i, o)| (expanded.get(i).cloned().unwrap_or_else(|| format!("action {}", i + 1)), o.clone()))
                .collect()
        }
        _ => Vec::new(),
    };
    let answered = match (&steps[..], outcomes.first()) {
        ([], Some(o)) => vec![(describe_action(action), o.clone())],
        _ => steps.clone(),
    };
    let outcome = if steps.is_empty() {
        let Some(outcome) = outcomes.into_iter().next() else {
            return Err((500, "the action produced no outcome".to_string()));
        };
        outcome
    } else {
        component_outcome(&steps)
    };
    // Shown to a person reading Settings -> Logs, never returned to the
    // assistant - and never a `fill`'s VALUE, which `describe_try` never
    // even looks at.
    crate::applog::info(describe_try(action, outcome.ok));
    // Only a try that worked: a locator that failed was never seen working.
    // (An explore session files no locator: `files_finds` is false.)
    if outcome.ok && files_finds {
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
        // A list check's outcome does not say how many items it read, and a
        // passed one has proved its list was there: the list is probed once
        // the try worked, and an `expect_each`'s child as the chain inside
        // the list (the whole the save asks to have seen), each filed as
        // found. Nothing visible now files nothing: the check alone does not
        // say what was there.
        let list_chain = list_chain_of(action);
        let mut matched = matched;
        for target in list_targets(action).into_iter().chain(list_chain.as_ref()) {
            if let Ok(text) = crate::browser::snapshot::probe_exact(d, target).await {
                if let Some((n, all)) = counted(d, target, &text).await {
                    matched.push((target, n, all));
                }
            }
        }
        if let Some(target) = waited {
            let found = match crate::browser::snapshot::probe_exact(d, target).await {
                Ok(text) => counted(d, target, &text).await,
                Err(_) => None,
            };
            // Nothing visible now, or the browser did not answer: the wait
            // found it, so it was there (a toast is gone a few seconds
            // later), filed with its count not known.
            let unknown = crate::autorun::discovery_map::NOT_ONE as usize;
            matched.push(match found {
                Some((n, all)) => (target, n, all),
                None => (target, unknown, unknown),
            });
        }
        record_matched_targets(&at, &started_on, &matched);
    }
    Ok(Tried { outcome, dialog: run.dialog, steps, answered })
}

/// Before a replay or a failures answer: a case whose divergence waits for a
/// rewrite that has since been saved in Azure DevOps runs again (read only;
/// nothing when signed out).
async fn release_rewritten_cases(ctx: &BridgeContext, client: Option<&crate::ado::AdoClient>) {
    let (Some(client), Ok(root)) = (client, autorun_root()) else {
        return;
    };
    crate::commands::autorun_divergence::refresh_rewritten(client, &root, &ctx.org, &ctx.project, None).await;
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
        // A case's newest word may be a replay that stopped on a failure,
        // when it is newer than the case's newest run or no run has it.
        // With no case named, a replay that stopped on a failure counts as
        // the newest word too, when it is newer than the newest run.
        None => match case_id.map_or_else(
            || crate::autorun::replay_record::latest(&root),
            |id| crate::autorun::replay_record::latest_for_case(&root, id),
        ) {
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
    let mut said = crate::autorun::failures::describe_failures_in(Some(&root), &run, &scripts, &components);
    // The divergences reported for the run's failed or blocked cases: what
    // a person has yet to rule on, what waits for a repair, what was ruled.
    let stopped: Vec<i32> =
        run.cases.iter().filter(|c| crate::autorun::failures::is_failed(c)).map(|c| c.case_id).collect();
    if let Ok(list) = crate::autorun::divergences::load(&root, &ctx.org, &ctx.project) {
        if let Some(section) =
            crate::autorun::divergences::failures_section(&list, &stopped, crate::autorun::sessions::now_ms())
        {
            said.push_str("\n\n");
            said.push_str(&section);
        }
    }
    // The bounds of a repair end the answer whenever a case failed or was
    // Blocked, so an assistant that reads only this still sees them.
    let bounds = if said.starts_with("no failed case") {
        None
    } else {
        Some(format!("\n\n{}", crate::autorun::failures::REPAIR_BOUNDS))
    };
    if case_id.is_some() || q(target, "run_id").is_some() {
        if let Some(b) = &bounds {
            said.push_str(b);
        }
        return (200, said);
    }
    // Asked of no case and no run: say which run that was, and name the
    // other recent runs here with failed or blocked cases, so failures a
    // run or two back are not missed.
    let all_scripts = crate::autorun::store::list_scripts(&root);
    let runs = crate::autorun::store::list_runs(&root);
    let now = crate::autorun::sessions::now_ms();
    let (head, others) = crate::autorun::failures::newest_run_context(&run, &all_scripts, &runs, now);
    let mut out = format!("{head}\n\n{said}");
    if let Some(others) = others {
        out.push_str("\n\n");
        out.push_str(&others);
    }
    if let Some(b) = &bounds {
        out.push_str(b);
    }
    (200, out)
}

/// Record something learned about the application, attributed, so the
/// next script does not rediscover the same surprise.
///
/// `from` says which assistant's work it came out of: "autorun" (the
/// default) or "api", for one building API templates. Both read the same
/// list - one per project.
fn autorun_quirk(ctx: &BridgeContext, body: &str) -> (u16, String) {
    use crate::autorun::quirks::{record_quirk_classed, QuirkClass, Recorded, FROM_API, FROM_AUTORUN};
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
    let class = match v.get("class") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(s)) => match QuirkClass::parse(s) {
            Some(c) => Some(c),
            None => return (400, QUIRK_CLASS_REFUSED.to_string()),
        },
        Some(_) => return (400, QUIRK_CLASS_REFUSED.to_string()),
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
    match record_quirk_classed(
        &root,
        &ctx.org,
        &ctx.project,
        &text,
        "assistant",
        from,
        sources,
        class,
        crate::autorun::sessions::now_ms(),
    ) {
        Ok((recorded, kept)) => {
            let mut answer = match &recorded {
                Recorded::Added(id) => format!("recorded as {id}"),
                Recorded::AlreadyKnown(id) => format!("already known, as {id}"),
                Recorded::Reactivated(id, reason) => reactivated_reply(id, reason.as_deref()),
            };
            if let (Some(asked), Some(kept)) = (class, kept) {
                if !matches!(recorded, Recorded::Added(_)) && asked != kept {
                    answer.push_str(&format!(" (kept its class: {})", kept.label()));
                }
            }
            if kept == Some(QuirkClass::ProductBug) {
                answer.push_str(PRODUCT_BUG_ANSWER);
            }
            (200, answer)
        }
        Err(why) => (400, why),
    }
}

/// Said when a quirk is sent a `class` that is not one of the three.
const QUIRK_CLASS_REFUSED: &str = "\"class\" is \"app quirk\", \"environment flake\" or \"product bug\"";

/// Added to the answer for a product bug.
const PRODUCT_BUG_ANSWER: &str = " A product bug is shown on every result of the cases it names and at the top of the run report: tell the person, never script around it.";

/// Remember, or take out, one rule the application enforced in an area -
/// the words its page showed after an action it refused - as the
/// assistant judged it (`record_validation_rule`). Reads the open
/// discovery's area and the page the browser is on, the way `try` does,
/// without waiting for a browser an unattended run holds.
async fn record_validation_rule(ctx: &BridgeContext, body: &str, n: SlotNo) -> (u16, String) {
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    if unattended_run_is_using_the_browser().is_some() || crate::autorun::replay_to::state(n).is_running() {
        return rule_in_area(&root, &ctx.org, &ctx.project, body, None, "");
    }
    let mut slot = crate::commands::autorun::session_slot(n).lock().await;
    record_validation_rule_in(&mut slot, &root, &ctx.org, &ctx.project, body).await
}

/// `record_validation_rule` in the browser `slot` holds: its discovery's
/// area is where a rule goes when the body names none, and the page it is
/// on is the rule's page when the body names none. No browser, or one open
/// for no discovery, leaves the body to name them.
pub async fn record_validation_rule_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    body: &str,
) -> (u16, String) {
    let Some(browser) = slot.as_mut() else {
        return rule_in_area(root, organization, project, body, None, "");
    };
    let p = browser.parts();
    let area = p.discovery.as_ref().and_then(|d| d.area.clone());
    // Only an add that names no page takes the browser's.
    let page = if takes_the_browsers_page(body) { current_page(p.driver).await.0 } else { String::new() };
    rule_in_area(root, organization, project, body, area.as_deref(), &page)
}

/// Whether a `record_validation_rule` body is an add that names no page,
/// the one kind filed under the page the browser is on. A body that is not
/// a rule is left to `rule_in_area` to refuse.
fn takes_the_browsers_page(body: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };
    let removal = v["remove"].as_bool().unwrap_or(false);
    let named = v["page"].as_str().is_some_and(|p| !p.trim().is_empty());
    !removal && !named
}

/// The rule a body tells, kept in (or taken out of) the map. `discovery_area`
/// and `browser_page` (a path, empty when not known) fill in what the body
/// leaves out; an area from neither is asked for rather than guessed, and
/// one the body names that is neither recorded nor in the map is refused,
/// so a typo makes no stray area. An add without a page is on the page the
/// browser is on; a removal without one is from every page of the area,
/// whatever the browser shows: the words are the rule.
fn rule_in_area(
    root: &std::path::Path,
    organization: &str,
    project: &str,
    body: &str,
    discovery_area: Option<&str>,
    browser_page: &str,
) -> (u16, String) {
    use crate::autorun::discovery_map::{canonical_area, forget_refusal_saved, page_path, remember_refusals, RefusalRule, RuleSource};
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct RuleBody {
        message: String,
        after: Option<String>,
        area: Option<String>,
        page: Option<String>,
        step: Option<String>,
        #[serde(default)]
        remove: bool,
    }
    let shape = "{ \"message\": \"the words the page showed\", \"after\": \"the action that caused it\" }";
    let rule: RuleBody = match serde_json::from_str(body) {
        Ok(r) => r,
        Err(e) => return (400, format!("that is not a rule: {e}. Expected {shape}.")),
    };
    // One line, as the page's messages are read.
    let words = |text: &str| -> String {
        let one = text.split_whitespace().collect::<Vec<_>>().join(" ");
        one.chars().take(crate::browser::page_messages::MAX_MESSAGE_CHARS).collect()
    };
    let named = |text: &Option<String>| text.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string);
    let message = words(&rule.message);
    if message.is_empty() {
        return (400, "\"message\" is the application's own words, as the page showed them".to_string());
    }
    if organization.trim().is_empty() || project.trim().is_empty() {
        return (400, RULE_NEEDS_A_PROJECT.to_string());
    }
    let named_area = named(&rule.area);
    let Some(area) = named_area.clone().or_else(|| discovery_area.map(str::trim).filter(|a| !a.is_empty()).map(str::to_string)) else {
        return (400, "Name the area: no discovery is running.".to_string());
    };
    let area = canonical_area(root, organization, project, &area);
    if named_area.is_some() {
        // The discovery's own area is one the assistant started; a name
        // typed into the body must already be known.
        let key = crate::autorun::discovery_map::area_key(&area);
        let recorded = crate::autorun::nav::load_nav(root, organization, project)
            .ok()
            .is_some_and(|nav| crate::autorun::nav::find_area(&nav, &area).is_some());
        let mapped = crate::autorun::discovery_map::load_map(root, organization, project)
            .ok()
            .is_some_and(|map| map.areas.iter().any(|a| crate::autorun::discovery_map::area_key(&a.area) == key));
        if !recorded && !mapped {
            return (400, format!("No area named \"{area}\" is recorded."));
        }
    }
    // Only an add takes the page the browser is on.
    let page = named(&rule.page)
        .or_else(|| Some(browser_page.trim().to_string()).filter(|p| !p.is_empty() && !rule.remove))
        .map(|p| page_path(&p));
    if rule.remove {
        return match forget_refusal_saved(root, organization, project, &area, page.as_deref(), &message) {
            Ok(true) => (200, format!("Removed from {area}.")),
            Ok(false) => (200, format!("No such rule in {area}.")),
            Err(_) => rule_not_kept(),
        };
    }
    let after = words(rule.after.as_deref().unwrap_or(""));
    if after.is_empty() {
        return (400, "\"after\" is the action that caused it, in a few words (needed unless you remove the rule)".to_string());
    }
    let Some(page) = page else {
        return (400, "Name the page: the Auto Run browser is not on one.".to_string());
    };
    let now = crate::autorun::sessions::now_ms();
    let step = named(&rule.step);
    let rule = RefusalRule { page, step, after, message, source: RuleSource::Assistant, case_id: None, first_seen: now, last_seen: now, count: 1 };
    match remember_refusals(root, organization, project, &area, vec![rule]) {
        Ok(()) => (200, format!("Remembered for {area}.")),
        Err(_) => rule_not_kept(),
    }
}

/// Said by `record_validation_rule` with no project open: a rule belongs to one.
const RULE_NEEDS_A_PROJECT: &str = "open a project in the app first - a validation rule belongs to a project";

/// Said when the map could not be written: how, never in the rule's words
/// or a file's path.
fn rule_not_kept() -> (u16, String) {
    crate::applog::warn("Discovery map: a rule the assistant gave could not be kept".to_string());
    (500, "the discovery map could not be written, so the rule was not kept".to_string())
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
    // The newest word on the case: a replay that stopped on a failure
    // counts when it is newer than the case's newest run, or none has it.
    let run = crate::autorun::replay_record::latest_for_case(&root, m.case_id);
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

/// `POST /autorun-divergence`: where a test case and the live application
/// disagree, reported instead of fitting a check to the page. The case is
/// read from Azure DevOps and must hold the words the assistant quotes
/// (`case_says`), at a step it has; the divergence is stored for a person
/// to rule on (`divergences::report_in`). Never touches a script.
async fn autorun_divergence(ctx: &BridgeContext, client: Option<&crate::ado::AdoClient>, body: &str) -> (u16, String) {
    use crate::autorun::divergences::{self, Reported};
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct DivergenceBody {
        case_id: i32,
        step: i32,
        case_says: String,
        app_shows: String,
        kind: divergences::DivergenceKind,
        #[serde(default)]
        proposal: Option<String>,
    }
    let shape = "{ \"case_id\": 147556, \"step\": 2, \"case_says\": \"exactly 4 levels\", \"app_shows\": \"6 levels\", \"kind\": \"data\" }";
    let b: DivergenceBody = match serde_json::from_str(body) {
        Ok(b) => b,
        Err(e) => {
            return (
                400,
                format!("that is not a divergence: {e}. Expected {shape} - kind is data, app or wording."),
            )
        }
    };
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let Some(client) = client else {
        return (
            503,
            "sign in to Test Case Manager first - a divergence is checked against its test case".to_string(),
        );
    };
    let case = match client
        .get_test_cases_by_ids(&ctx.org, &[b.case_id], ctx.module_ref.as_deref(), ctx.preconditions_ref.as_deref())
        .await
    {
        Ok(cases) => cases.into_iter().find(|c| c.id == b.case_id),
        Err(crate::ado::AdoError::NotFound) => None,
        Err(e) => {
            // The raw error goes to the log; the reader gets the sentence
            // that names no address (CLAUDE.md, user-facing errors).
            crate::applog::warn(format!("Auto Run: a divergence's test case could not be read: {e:?}"));
            return (502, format!("Azure DevOps error: {}", e.user_text()));
        }
    };
    let Some(case) = case else {
        return (400, format!("case {} is not a test case in this organization", b.case_id));
    };
    if b.step < 1 || b.step as usize > case.steps.len() {
        return (400, format!("case {} has {} steps - \"step\" is one of them", b.case_id, case.steps.len()));
    }
    if !crate::autorun::case_text::CaseText::from_case(&case).holds_quote(&b.case_says) {
        return (
            400,
            format!(
                "\"case_says\" is the case's own words, quoted from step {}, another expected result or the preconditions - case {} does not hold \"{}\"",
                b.step, b.case_id, b.case_says
            ),
        );
    }
    let (case_says, app_shows) =
        (crate::autorun::defects::scrub_note(&b.case_says), crate::autorun::defects::scrub_note(&b.app_shows));
    let proposal = b.proposal.as_deref().map(crate::autorun::defects::scrub_note);
    let now = crate::autorun::sessions::now_ms();
    let mut refused: Option<String> = None;
    let reported = divergences::update(&root, &ctx.org, &ctx.project, |list| {
        let r = divergences::report_with_proposal_in(list, b.case_id, b.step, &case_says, &app_shows, b.kind, proposal.as_deref(), now)
            .inspect_err(|why| refused = Some(why.clone()))?;
        let id = match &r {
            Reported::New(id) | Reported::Updated(id) => id,
        };
        let stored = list.iter().find(|d| &d.id == id).cloned().expect("the divergence just reported");
        Ok((stored, matches!(r, Reported::Updated(_))))
    });
    match reported {
        Ok((stored, updated)) => {
            crate::applog::info(format!(
                "AI reported a {} divergence on case {} step {}",
                stored.kind.word(),
                b.case_id,
                b.step
            ));
            (200, divergences::tool_answer(b.case_id, &stored, updated))
        }
        // A rule's refusal is the assistant's to read; a file that could
        // not be read or written is the machine's.
        Err(e) if refused.is_some() => (400, e),
        Err(e) => (500, e),
    }
}

/// `POST /autorun-divergence-ruling`: the assistant applies the ruling the person gave in the chat, on one
/// divergence or on several (all or none), quoting their words (`divergences::rule_by_assistant_in`, by
/// `commands::autorun_divergence::rule_by_assistant_at`). A rewrite ruling takes the case's print at once, so a
/// later rewrite is seen as a difference. The person's words are stored and shown, never written to the log.
async fn autorun_divergence_ruling(ctx: &BridgeContext, client: Option<&crate::ado::AdoClient>, body: &str) -> (u16, String) {
    use crate::autorun::divergences::{self, AssistantRuling};
    #[derive(serde::Deserialize)]
    struct Pick {
        case_id: i32,
        divergence_id: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct RulingBody {
        case_id: Option<i32>,
        divergence_id: Option<String>,
        divergences: Option<Vec<Pick>>,
        ruling: AssistantRuling,
        said: String,
        note: Option<String>,
    }
    let shape = "{ \"case_id\": 141701, \"divergence_id\": \"d3\", \"ruling\": \"case_right\", \"said\": \"a different cycle name is fine\" } - ruling is case_right, data_fixed or rewrite_case";
    let b: RulingBody = match serde_json::from_str(body) {
        Ok(b) => b,
        Err(e) => return (400, format!("that is not a ruling: {e}. Expected {shape}.")),
    };
    let single = b.case_id.is_some() || b.divergence_id.is_some();
    let picks: Vec<(i32, String)> = match (single, b.divergences) {
        (true, Some(_)) => {
            return (
                400,
                "name one divergence (case_id and divergence_id) or several (divergences), not both".to_string(),
            )
        }
        (true, None) => match (b.case_id, b.divergence_id) {
            (Some(c), Some(d)) => vec![(c, d)],
            _ => return (400, divergences::NO_PICK.to_string()),
        },
        (false, Some(list)) => list.into_iter().map(|p| (p.case_id, p.divergence_id)).collect(),
        (false, None) => return (400, divergences::NO_PICK.to_string()),
    };
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let ruling = b.ruling.ruling();
    let now = crate::autorun::sessions::now_ms();
    let answer = match crate::commands::autorun_divergence::rule_by_assistant_at(
        &root,
        &ctx.org,
        &ctx.project,
        &picks,
        ruling,
        &b.said,
        b.note.as_deref(),
        now,
    ) {
        Ok(answer) => answer,
        Err(crate::commands::autorun_divergence::RulingFailure::Refused(why)) => return (400, why),
        Err(crate::commands::autorun_divergence::RulingFailure::Machine(e)) => return (500, e),
    };
    if ruling == crate::autorun::divergences::Ruling::Rewrite {
        if let Some(client) = client {
            let mut cases: Vec<i32> = picks.iter().map(|(c, _)| *c).collect();
            cases.sort_unstable();
            cases.dedup();
            crate::commands::autorun_divergence::refresh_rewritten(client, &root, &ctx.org, &ctx.project, Some(&cases)).await;
        }
    }
    (200, answer)
}

/// `POST /autorun-defect-clear`: the assistant clears the suspected-defect mark it set, once the person said so
/// in the chat. Their words are kept with the cleared mark (`store::clear_suspected_defect_with_words`) so the
/// case card can show them; there is no Undo. Never written to the log: only the case and the step.
fn autorun_defect_clear(body: &str) -> (u16, String) {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ClearBody {
        case_id: i32,
        said: String,
        note: Option<String>,
    }
    let shape = "{ \"case_id\": 501, \"said\": \"the person's own words from this chat\" }";
    let b: ClearBody = match serde_json::from_str(body) {
        Ok(b) => b,
        Err(e) => return (400, format!("that is not a clearing: {e}. Expected {shape}.")),
    };
    let said = match crate::autorun::divergences::check_said(&b.said) {
        Ok(s) => s,
        Err(why) => return (400, why),
    };
    let note = match crate::autorun::divergences::check_note(b.note.as_deref()) {
        Ok(n) => n,
        Err(why) => return (400, why),
    };
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let script = match crate::autorun::store::load_script(&root, b.case_id) {
        Ok(s) => s,
        Err(e) => return (500, e),
    };
    if script.as_ref().and_then(|s| s.suspected_defect.as_ref()).is_none() {
        return (400, format!("case {} has no suspected-defect mark", b.case_id));
    }
    let now = crate::autorun::sessions::now_ms();
    match crate::autorun::store::clear_suspected_defect_with_words(&root, b.case_id, &said, note.as_deref(), now) {
        Ok(Some(step)) => {
            crate::applog::info(format!("AI cleared the suspected defect on case {} step {step} on the person's words", b.case_id));
            (
                200,
                format!(
                    "Cleared the suspected-defect mark on case {} step {step}, on the person's words. A failure there is no longer labelled as an application defect.",
                    b.case_id
                ),
            )
        }
        Ok(None) => (400, format!("case {} has no suspected-defect mark", b.case_id)),
        Err(e) => (500, e),
    }
}

/// What `set_autorun_order` says after the order it saved: with
/// `mode: "replace"` (the default) the call replaced the PBI's whole saved
/// order, and the PBI's other cases run after the listed ones.
pub const ORDER_REPLACED: &str = "This replaced the PBI's whole Auto Run order: its other cases run after these, in list order. To add cases ahead of the saved order and keep the rest of it, send mode: \"append\".";

/// `POST /autorun-order`, body `{ pbi_id, case_ids, mode? }`: Auto Run's own
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
        #[serde(default)]
        mode: Option<String>,
    }
    let shape = "{ \"pbi_id\": 100, \"case_ids\": [503, 501, 502], \"mode\": \"replace\" | \"append\" (optional) }";
    let o: OrderBody = match serde_json::from_str(body) {
        Ok(o) => o,
        Err(e) => return (400, format!("that is not an order: {e}. Expected {shape}.")),
    };
    let append = match o.mode.as_deref().map(str::trim) {
        None | Some("replace") => false,
        Some("append") => true,
        Some(other) => return (400, format!("mode \"{other}\" is not one: send \"replace\" (the default) or \"append\". Expected {shape}.")),
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
    // The Azure DevOps read is done: only the file's read and write hold the
    // order lock (never across an await), so another call's append cannot
    // be read past and then overwritten here.
    let (kept, saved): (Vec<i32>, Vec<i32>) = match crate::autorun::store::with_order_locked(|| {
        // Appending keeps the saved order's other cases after the given ones.
        let kept: Vec<i32> = match (append, crate::autorun::store::load_order(&root, o.pbi_id)) {
            (true, Some(existing)) => crate::autorun::store::append_order(&existing, &o.case_ids)
                .into_iter()
                .filter(|id| !o.case_ids.contains(id))
                .collect(),
            _ => Vec::new(),
        };
        let saved: Vec<i32> = o.case_ids.iter().chain(&kept).copied().collect();
        crate::autorun::store::save_order(&root, o.pbi_id, &saved).map(|()| (kept, saved))
    }) {
        Ok(both) => both,
        Err(e) => return (500, format!("could not save the order: {e}")),
    };
    let how = if append { "appended" } else { "replaced" };
    crate::applog::info(format!("AI set Auto Run's order for PBI {} ({how}): {} case(s)", o.pbi_id, saved.len()));
    let ids = |list: &[i32]| list.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(", ");
    let mut lines = vec![format!("saved Auto Run's order for PBI {}: {}", o.pbi_id, ids(&o.case_ids))];
    if append {
        lines.push(match kept.is_empty() {
            true => "Appended: the saved order had no other cases.".to_string(),
            false => format!("Appended: the saved order's other cases run after these, as they were: {}", ids(&kept)),
        });
    } else {
        lines.push(ORDER_REPLACED.to_string());
    }
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
    use crate::environments::{
        active, proposed_password_missing, save_proposals, StoredProposal, PROPOSED_PASSWORD_NOT_TEST,
    };
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
        #[serde(default)]
        no_password: Option<String>,
    }
    let shape = "{ \"accounts\": [{ \"key\": \"hr.supervisor\", \"label\": \"HR supervisor\", \"username\": \"sup1\", \"role\"?: \"Supervisor\", \"password\"?: \"(test environments only)\", \"no_password\"?: \"why there is none\" }] }";
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
    // A password and the reason for having none say opposite things.
    if let Some(p) = list.iter().find(|p| p.password.is_some() && p.no_password.is_some()) {
        return (
            400,
            format!("the account \"{}\" has both a password and no_password - give one or the other", p.key.trim()),
        );
    }
    let mut proposals: Vec<StoredProposal> = list
        .into_iter()
        .map(|p| {
            let key = p.key.trim().to_string();
            let label = match p.label.trim() {
                "" => key.clone(),
                l => l.to_string(),
            };
            let role = p.role.map(|r| r.trim().to_string()).filter(|r| !r.is_empty());
            // Kept exactly as read: a password's spaces are its own.
            let no_password = p.no_password.map(|r| r.trim().to_string()).filter(|r| !r.is_empty());
            StoredProposal {
                key,
                label,
                username: p.username.trim().to_string(),
                role,
                password: p.password,
                no_password,
            }
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
    if env.test_environment {
        // Every account says what its password is, or why it has none.
        let missing: Vec<&str> = proposals
            .iter()
            .filter(|p| p.password.is_none() && p.no_password.is_none())
            .map(|p| p.key.as_str())
            .collect();
        if !missing.is_empty() {
            return (400, proposed_password_missing(&missing));
        }
    } else {
        if with_password > 0 {
            return (400, PROPOSED_PASSWORD_NOT_TEST.to_string());
        }
        // Only a test environment keeps passwords, so only there is a
        // reason for a missing one worth keeping.
        for p in &mut proposals {
            p.no_password = None;
        }
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
pub const GET_ACCOUNTS_OFF: &str = "reading the accounts is switched off in Test Case Manager";

/// `GET /accounts`: the active environment's accounts - key, label and
/// username, and the password ONLY when the environment is marked as a
/// test environment. The one place a password leaves the app; it is never
/// logged, and the log line says only how many were read. Beside them, the
/// project's Test files (`test_files_answer`), or `test_files_note` saying
/// why there are none to show. An account that is held right now carries
/// `in_use_by`, who holds it as the caller reads it (`in_use_by_words`); a
/// free one has no such key.
///
/// Refused while the person has switched `get_accounts` off on the AI
/// Bridge tab. The MCP proxy already hides a switched-off tool; this route
/// checks again itself, as the database routes do (`stage_db`), because it
/// is the one that can hand out passwords.
fn accounts_read(ctx: &BridgeContext, caller_slot: Option<SlotNo>) -> (u16, String) {
    if ctx.disabled_tools.iter().any(|t| t == "get_accounts") {
        return (409, GET_ACCOUNTS_OFF.to_string());
    }
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let files = test_files_answer(&root, ctx);
    // The Test files do not hang on the environment: when its accounts
    // cannot be read they are still answered, with why the accounts are
    // not. Only when neither can be read is the call a failure.
    let accounts_failed = |e: String| -> (u16, String) {
        match &files {
            Ok(rows) => (200, serde_json::json!({ "accounts_note": e, "test_files": rows }).to_string()),
            Err(_) => (500, e),
        }
    };
    let env = match crate::environments::active(&root) {
        Ok(e) => e,
        Err(e) => return accounts_failed(e),
    };
    // Read by the id just read, so the accounts and the test-environment
    // mark are the same environment's even if a switch lands in between.
    let accounts = match crate::autorun::accounts::load_accounts_for(&root, &env.id) {
        Ok(a) => a,
        Err(e) => return accounts_failed(e),
    };
    let shown = env.test_environment;
    // Who holds each account right now, as this caller reads it: a free
    // account has no `in_use_by`. Main's session is looked at only when it
    // holds one, and never waited for.
    let held = crate::autorun::lease::holders_in(&env.id);
    let main_is_persons = held.iter().any(|(_, h)| matches!(h, crate::autorun::lease::Holder::Browser { slot: 0 }))
        && main_is_opened_by_the_person();
    let list: Vec<serde_json::Value> = accounts
        .iter()
        .map(|a| {
            let mut one = serde_json::json!({ "key": a.key, "label": a.label, "username": a.username });
            if shown {
                one["password"] = serde_json::Value::String(a.password.clone());
            }
            if let Some((_, holder)) = held.iter().find(|(key, _)| *key == a.key) {
                one["in_use_by"] = serde_json::Value::String(in_use_by_words(holder, caller_slot, main_is_persons));
            }
            one
        })
        .collect();
    let mut out = serde_json::json!({
        "environment": env.name,
        "test_environment": shown,
        "accounts": list,
    });
    match files {
        Ok(files) => out["test_files"] = serde_json::Value::Array(files),
        Err(why) => out["test_files_note"] = serde_json::Value::String(why),
    }
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

/// Whether main's session is open and the person opened it (`OpenedBy`),
/// read without waiting: a session busy with a step or a replay reads as not.
fn main_is_opened_by_the_person() -> bool {
    use crate::commands::autorun::{session_slot, try_lock_briefly, OpenedBy};
    try_lock_briefly(session_slot(SlotNo::MAIN))
        .is_some_and(|session| session.as_ref().is_some_and(|s| s.opened_by == OpenedBy::Person))
}

/// The live assistant a slot's browser is in use by, by label.
fn label_of_slot(slot: SlotNo) -> Option<String> {
    let key = crate::commands::autorun_slots::bound(slot)?;
    crate::autorun::agents::info(&key).filter(|i| i.gone_since_ms.is_none()).map(|i| i.label)
}

/// `get_accounts`' `in_use_by` for one held account, as `caller_slot` (the
/// asker's own browser, if it has one) reads it:
///
/// - the asker's own browser: "your browser";
/// - an assistant's: "{label}'s browser";
/// - main's, bound to nobody: "the person's browser" when the person opened
///   it (`main_is_persons`), else "the main Auto Run browser";
/// - an extra browser nobody is bound to now (its assistant is gone and the
///   browser not yet released): "another Auto Run browser";
/// - an unattended run, an API template run, Auto Run setup: as the
///   refusals word them.
///
/// Labels only: never a login, an id or a host.
pub fn in_use_by_words(holder: &crate::autorun::lease::Holder, caller_slot: Option<SlotNo>, main_is_persons: bool) -> String {
    use crate::autorun::lease::Holder;
    match holder {
        Holder::Browser { slot } => {
            let Some(slot) = SlotNo::from_number(*slot) else {
                return "another Auto Run browser".to_string();
            };
            if caller_slot == Some(slot) {
                "your browser".to_string()
            } else if let Some(label) = label_of_slot(slot) {
                format!("{label}'s browser")
            } else if slot.is_main() {
                (if main_is_persons { "the person's browser" } else { "the main Auto Run browser" }).to_string()
            } else {
                "another Auto Run browser".to_string()
            }
        }
        Holder::Case { .. } => "an unattended run".to_string(),
        Holder::Template => "an API template run".to_string(),
        Holder::Setup => "Auto Run setup".to_string(),
    }
}

/// An answer holding a lease refusal for an account whose holder is another
/// slot's browser, with that browser's live assistant named right after the
/// refusal's sentence: ". {label} is signed in as it: work on cases that
/// sign in as another account, or wait until {label} is done." The refusal
/// itself stays word for word (`lease::is_in_use` still matches it). An
/// answer with no refusal, an account held by the asker's own browser, by
/// anything else, or by a browser no live assistant is bound to comes back
/// unchanged. `env` is the active environment's id, the key the lease table
/// uses. Labels only: never a login or an id.
pub fn with_holder_named(answer: (u16, String), caller_slot: impl Into<Option<SlotNo>>, env: &str) -> (u16, String) {
    use crate::autorun::lease::{holder_of, locate_in_use, Holder};
    let (status, text) = answer;
    let Some((key, end)) = locate_in_use(&text).map(|(key, _, end)| (key.to_string(), end)) else {
        return (status, text);
    };
    let caller_slot: Option<SlotNo> = caller_slot.into();
    let named = (|| {
        let Holder::Browser { slot } = holder_of(env, &key)? else { return None };
        let slot = SlotNo::from_number(slot)?;
        if caller_slot == Some(slot) {
            return None;
        }
        label_of_slot(slot)
    })();
    let Some(label) = named else { return (status, text) };
    // The sentence's own full stop, if the answer has one, is the one this ends with.
    let rest = text[end..].strip_prefix('.').unwrap_or(&text[end..]);
    let said = format!(
        ". {label} is signed in as it: work on cases that sign in as another account, or wait until {label} is done."
    );
    (status, format!("{}{said}{rest}", &text[..end]))
}

/// `with_holder_named` for the answer to one request, in the active
/// environment, for the routes that sign an account in: those that open or
/// act on the caller's browser, and the API template runs. Any other route's
/// answer is returned as it is - a failure listing may quote an old refusal.
fn name_the_holder(
    answer: (u16, String),
    caller: &crate::autorun::agents::Caller,
    method: &str,
    path: &str,
) -> (u16, String) {
    use crate::commands::autorun_slots::{need_of, slot_if_any, Need};
    let signs_in = matches!(need_of(method, path), Some(Need::Open | Need::Act))
        || (method == "POST" && matches!(path, "/api-template-run" | "/api-template-prove"));
    if !signs_in || crate::autorun::lease::locate_in_use(&answer.1).is_none() {
        return answer;
    }
    let env = autorun_root().ok().and_then(|root| crate::environments::active_id(&root).ok());
    match env {
        Some(env) => with_holder_named(answer, slot_if_any(caller), &env),
        None => answer,
    }
}

// ------------------------------------------------- case claims, one mapping run

/// The caller works on `case_id` from now, or is told who does. An assistant
/// claims it (`claims::claim`: refused, 409, while another live assistant
/// holds it). The default caller (an older proxy, a test, the person's own
/// tools) never claims, and is refused only by a LIVE assistant's claim, so
/// with no assistant connected nothing is ever refused. The answer names the
/// holder by label only.
pub fn claim_for(caller: &crate::autorun::agents::Caller, case_id: i32) -> Result<(), (u16, String)> {
    use crate::autorun::{agents, claims};
    let now = crate::autorun::sessions::now_ms();
    match caller {
        agents::Caller::Agent(key) => claims::claim(key, case_id, now).map_err(|why| (409, why)),
        agents::Caller::Default => {
            let live = claims::holder(case_id, now).and_then(|h| agents::info(&h)).filter(|i| i.gone_since_ms.is_none());
            match live {
                Some(held_by) => Err((409, claims::case_claimed(case_id, &held_by.label))),
                None => Ok(()),
            }
        }
    }
}

/// The `case_id` a request body names, when it names one.
fn named_case_id(body: &str) -> Option<i32> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    v.get("case_id")?.as_i64().and_then(|n| i32::try_from(n).ok())
}

/// `claim_for` the case a try, a discovery start or a replay body names; a
/// body that names none claims nothing (the call answers its own 400).
fn claim_named_case(caller: &crate::autorun::agents::Caller, body: &str) -> Result<(), (u16, String)> {
    match named_case_id(body) {
        Some(case_id) => claim_for(caller, case_id),
        None => Ok(()),
    }
}

/// `claim_for` every case a save bundle names (a list of scripts, or an
/// object with `scripts`), in the bundle's order, before any of the save's
/// gates: the first refusal is the answer.
fn claim_bundle_cases(caller: &crate::autorun::agents::Caller, body: &str) -> Result<(), (u16, String)> {
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    let scripts = if v.is_array() { Some(&v) } else { v.get("scripts") };
    let mut seen: Vec<i32> = Vec::new();
    for script in scripts.and_then(|s| s.as_array()).into_iter().flatten() {
        let Some(case_id) = script.get("case_id").and_then(|n| n.as_i64()).and_then(|n| i32::try_from(n).ok()) else {
            continue;
        };
        if !seen.contains(&case_id) {
            seen.push(case_id);
            claim_for(caller, case_id)?;
        }
    }
    Ok(())
}

/// The refusal for a second mapping run: whose browser it is going in.
pub fn mapping_going(whose: &str) -> String {
    format!("a mapping run is going in {whose} - one mapping run at a time; wait for it to end")
}

/// The refusal for a mapping start in `caller_slot` while a mapping run goes
/// in another slot (`mapping`, per slot: `commands::autorun::mapping_in`), or
/// None. It names whose browser that is: "{label}'s browser" when the slot is
/// bound to a live assistant, else the main browser (the person's) or an
/// Auto Run browser. Never an id, a pid or a login.
pub fn mapping_refusal(caller_slot: SlotNo, mapping: [bool; SLOT_COUNT]) -> Option<String> {
    let going = SlotNo::all().into_iter().find(|m| *m != caller_slot && mapping[m.index()])?;
    let whose = match label_of_slot(going) {
        Some(label) => format!("{label}'s browser"),
        None if going.is_main() => "the main Auto Run browser".to_string(),
        None => "an Auto Run browser".to_string(),
    };
    Some(mapping_going(&whose))
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

/// Said when a flow check would run while the Database Access module is
/// off: every check is an assistant-written read of that database, so none
/// runs until the module is on again.
pub const FLOW_NEEDS_READING: &str =
    "flow checks read the company database: turn the Database Access module on in Settings";

/// The chosen database as the place flow checks are asked, with
/// `db_ready`'s refusals as they are - what saving a flow and a flow's
/// progress answer with. Refused first while the Database Access module is
/// off, whatever the disabled list says, before any connection is looked
/// at.
fn stage_db(
    ctx: &BridgeContext,
) -> Result<crate::api_templates::gate::SqlcmdStageDb<crate::db::RealRunner>, (u16, String)> {
    stage_db_for(ctx, &crate::modules::current())
}

/// `stage_db` with the modules that are on passed in, so both answers are
/// testable. The module is the switch for reading the database; a
/// `db_query` entry among the disabled tools refuses too, as a second door.
pub fn stage_db_for(
    ctx: &BridgeContext,
    on: &std::collections::BTreeSet<crate::modules::Module>,
) -> Result<crate::api_templates::gate::SqlcmdStageDb<crate::db::RealRunner>, (u16, String)> {
    if !on.contains(&crate::modules::Module::DbAccess) || ctx.disabled_tools.iter().any(|t| t == "db_query") {
        return Err((409, FLOW_NEEDS_READING.to_string()));
    }
    let (conn, exe) = db_ready(ctx)?;
    Ok(crate::api_templates::gate::SqlcmdStageDb { runner: crate::db::RealRunner, exe, conn })
}

/// The database a prove or a run of a template on a flow gates on. With
/// nothing chosen the answer says why a template needs one at all
/// (`FLOW_NEEDS_DB`), with `db_ready`'s own status; the Database Access
/// module being off is `stage_db`'s `FLOW_NEEDS_READING`; every other
/// refusal - no login saved, a store that cannot be read, no sqlcmd - is
/// `db_ready`'s, unchanged.
pub fn real_stage_db(
    ctx: &BridgeContext,
) -> Result<crate::api_templates::gate::SqlcmdStageDb<crate::db::RealRunner>, (u16, String)> {
    real_stage_db_for(ctx, &crate::modules::current())
}

/// `real_stage_db` with the modules that are on passed in.
pub fn real_stage_db_for(
    ctx: &BridgeContext,
    on: &std::collections::BTreeSet<crate::modules::Module>,
) -> Result<crate::api_templates::gate::SqlcmdStageDb<crate::db::RealRunner>, (u16, String)> {
    stage_db_for(ctx, on).map_err(|(status, why)| {
        if why == crate::db::query::NO_CONNECTION {
            (status, FLOW_NEEDS_DB.to_string())
        } else {
            (status, why)
        }
    })
}

// ---- Crystal Reports -----------------------------------------------------
//
// Every call below runs the report library on a blocking thread: it may
// render a PDF while holding the open report's lock, and a panic inside the
// bundle must come back as an answer, never take the bridge down.

/// Said when the report library panicked inside a call.
const CRYSTAL_STOPPED: &str = "The report library stopped unexpectedly while doing that. Nothing was saved unless the report says otherwise.";

/// Run `work` on a blocking thread: `Err((400, sentence))` when it refuses,
/// `Err((500, ..))` when it panicked, and what it answers otherwise.
async fn crystal_blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, (u16, String)> {
    match tokio::task::spawn_blocking(work).await {
        Ok(Ok(answer)) => Ok(answer),
        Ok(Err(why)) => Err((400, why)),
        Err(_) => Err((500, CRYSTAL_STOPPED.to_string())),
    }
}

/// A 200 with `value` as JSON text, or the refusal.
fn crystal_answer(done: Result<serde_json::Value, (u16, String)>) -> (u16, String) {
    match done {
        Ok(value) => (200, serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())),
        Err(refused) => refused,
    }
}

/// A crystal route's body as an object. An empty body is no arguments, for
/// the routes whose arguments are all optional.
fn crystal_args(body: &str) -> Result<serde_json::Value, (u16, String)> {
    if body.trim().is_empty() {
        return Ok(serde_json::json!({}));
    }
    match serde_json::from_str::<serde_json::Value>(body) {
        Ok(v) if v.is_object() => Ok(v),
        Ok(_) => Err((400, "the arguments must be a JSON object.".to_string())),
        Err(e) => Err((400, format!("that is not readable JSON: {e}."))),
    }
}

/// A text argument, trimmed; `None` when it is missing, null or blank.
fn crystal_text(args: &serde_json::Value, key: &str) -> Option<String> {
    args.get(key).and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

/// A text argument the call cannot do without.
fn crystal_required(args: &serde_json::Value, key: &str) -> Result<String, (u16, String)> {
    crystal_text(args, key).ok_or_else(|| (400, format!("this call needs a \"{key}\".")))
}

/// `ops` and `plan` arrive as JSON, but an assistant that wrapped them in a
/// string (as it does for several other tools) is read the same way.
fn crystal_json_arg(args: &serde_json::Value, key: &str) -> Option<serde_json::Value> {
    match args.get(key) {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(text)) => {
            Some(serde_json::from_str(text).unwrap_or_else(|_| serde_json::Value::String(text.clone())))
        }
        Some(found) => Some(found.clone()),
    }
}

async fn crystal_open(body: &str) -> (u16, String) {
    let path = match crystal_args(body).and_then(|args| crystal_required(&args, "path")) {
        Ok(path) => path,
        Err(refused) => return refused,
    };
    let done = crystal_blocking(move || crate::crystal::open(&path)).await;
    // A refused open leaves the old report open; only a new one changes the screen.
    if done.is_ok() {
        crystal_changed().await;
    }
    crystal_answer(done)
}

async fn crystal_read(body: &str) -> (u16, String) {
    let args = match crystal_args(body) {
        Ok(args) => args,
        Err(refused) => return refused,
    };
    let filter =
        crate::crystal::ReadFilter { section: crystal_text(&args, "section"), object: crystal_text(&args, "object") };
    crystal_answer(crystal_blocking(move || crate::crystal::read(&filter)).await)
}

/// Apply one batch. A batch the bundle refuses saves nothing, and the
/// answer is the refusal followed by the report as it now stands, so the
/// assistant has fresh ids to try again with.
async fn crystal_change(body: &str) -> (u16, String) {
    let args = match crystal_args(body) {
        Ok(args) => args,
        Err(refused) => return refused,
    };
    let (ops, plan) = (crystal_json_arg(&args, "ops"), crystal_json_arg(&args, "plan"));
    let done = crystal_blocking(move || {
        crate::crystal::edit(ops, plan).map_err(|why| {
            let now = crate::crystal::read(&crate::crystal::ReadFilter::default())
                .ok()
                .and_then(|ir| serde_json::to_string_pretty(&ir).ok());
            match now {
                Some(ir) => format!("{why}\n\n{ir}"),
                None => why,
            }
        })
    })
    .await;
    // A refused batch tells the screen too: the report is re-read from disk.
    crystal_changed().await;
    crystal_answer(done)
}

async fn crystal_compare(body: &str) -> (u16, String) {
    let args = match crystal_args(body) {
        Ok(args) => args,
        Err(refused) => return refused,
    };
    let (a, b) = match (crystal_required(&args, "a"), crystal_required(&args, "b")) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(refused), _) | (_, Err(refused)) => return refused,
    };
    let done = crystal_blocking(move || crate::crystal::compare(&a, &b)).await;
    crystal_answer(done.map(|lines| serde_json::json!(lines)))
}

async fn crystal_check(body: &str) -> (u16, String) {
    let args = match crystal_args(body) {
        Ok(args) => args,
        Err(refused) => return refused,
    };
    let path = crystal_text(&args, "path");
    crystal_answer(crystal_blocking(move || crate::crystal::check(path.as_deref())).await)
}

async fn crystal_new(body: &str) -> (u16, String) {
    let args = match crystal_args(body) {
        Ok(args) => args,
        Err(refused) => return refused,
    };
    let path = match crystal_required(&args, "path") {
        Ok(path) => path,
        Err(refused) => return refused,
    };
    let opts = rpt_core::blank::NewReportOptions {
        title: crystal_text(&args, "title"),
        author: crystal_text(&args, "author"),
        comments: crystal_text(&args, "comments"),
        ..Default::default()
    };
    let done = crystal_blocking(move || crate::crystal::new_report(&path, opts)).await;
    if done.is_ok() {
        crystal_changed().await;
    }
    crystal_answer(done)
}

async fn crystal_preview(body: &str) -> (u16, String) {
    let params = match crystal_args(body).and_then(|args| crystal_params(&args)) {
        Ok(params) => params,
        Err(refused) => return refused,
    };
    let done = crystal_blocking(move || crate::crystal::preview_saved_with(params)).await;
    // A render that failed is kept as the preview too, so the screen shows why.
    crystal_changed().await;
    crystal_answer(done.and_then(|info| serde_json::to_value(info).map_err(|e| (500, e.to_string()))))
}

/// The open report's parameters, in the order it declares them.
async fn crystal_parameters() -> (u16, String) {
    let done = crystal_blocking(crate::crystal::parameters).await;
    crystal_answer(done.and_then(|list| serde_json::to_value(list).map_err(|e| (500, e.to_string()))))
}

/// Find text across the open report and its subreports. The text keeps its
/// spaces: unlike the other routes' arguments it is not trimmed.
async fn crystal_find(body: &str) -> (u16, String) {
    let args = match crystal_args(body) {
        Ok(args) => args,
        Err(refused) => return refused,
    };
    let Some(text) = args.get("text").and_then(|v| v.as_str()).map(str::to_string) else {
        return (400, "this call needs a \"text\".".to_string());
    };
    let match_case = args.get("match_case").and_then(|v| v.as_bool()).unwrap_or(false);
    let done = crystal_blocking(move || crate::crystal::find(&text, match_case)).await;
    crystal_answer(done.and_then(|hits| serde_json::to_value(hits).map_err(|e| (500, e.to_string()))))
}

/// Replace text across the open report and its subreports; `dry_run` lists
/// the changes and saves nothing. `find` and `replace` keep their spaces and
/// `replace` may be empty (it removes the text).
async fn crystal_replace(body: &str) -> (u16, String) {
    let args = match crystal_args(body) {
        Ok(args) => args,
        Err(refused) => return refused,
    };
    let Some(find) = args.get("find").and_then(|v| v.as_str()).map(str::to_string) else {
        return (400, "this call needs a \"find\".".to_string());
    };
    let Some(replace) = args.get("replace").and_then(|v| v.as_str()).map(str::to_string) else {
        return (400, "this call needs a \"replace\" (it may be empty, to remove the text).".to_string());
    };
    let match_case = args.get("match_case").and_then(|v| v.as_bool()).unwrap_or(false);
    let dry_run = args.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(false);
    let done = crystal_blocking(move || crate::crystal::replace(&find, &replace, match_case, dry_run)).await;
    // A refused replace re-read the file, and an applied one changed it.
    crystal_changed().await;
    crystal_answer(done.and_then(|out| serde_json::to_value(out).map_err(|e| (500, e.to_string()))))
}

/// The two doors of a live preview that need no connection to check:
/// Database Access on (the route's own guard is Crystal Reports'), and the
/// person's switch for database writes - running a report's stored
/// procedure is a procedure call, which TCM treats as a write. `on` is
/// explicit so both are testable.
pub fn live_preview_gate(
    on: &std::collections::BTreeSet<crate::modules::Module>,
    writes_on: bool,
) -> Result<(), (u16, String)> {
    use crate::modules::Module;
    if !on.contains(&Module::DbAccess) {
        return Err((404, Module::DbAccess.off_sentence()));
    }
    if !writes_on {
        return Err((400, crate::db::query::WRITES_OFF.to_string()));
    }
    Ok(())
}

/// The report's parameters out of `params` (name to value). A number or a
/// true/false is taken as its text, which the bundle reads against the
/// parameter's declared kind.
fn crystal_params(args: &serde_json::Value) -> Result<Vec<(String, String)>, (u16, String)> {
    use serde_json::Value;
    match args.get("params") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Object(map)) => map
            .iter()
            .map(|(name, value)| match value {
                Value::String(text) => Ok((name.clone(), text.clone())),
                Value::Number(n) => Ok((name.clone(), n.to_string())),
                Value::Bool(b) => Ok((name.clone(), b.to_string())),
                _ => Err((400, format!("params.{name} must be text, a number or true/false."))),
            })
            .collect(),
        Some(_) => Err((
            400,
            "\"params\" must be an object of parameter name to value, e.g. { \"performance_cycle_id\": \"42\" }."
                .to_string(),
        )),
    }
}

/// The chosen database by the name the AI Bridge tab shows it under -
/// what a live preview says it came from, never the host.
fn chosen_database_name(ctx: &BridgeContext) -> String {
    ctx.db_secrets
        .as_deref()
        .zip(ctx.db_id.as_deref())
        .and_then(|(store, id)| crate::db::credentials::databases(store).into_iter().find(|d| d.id == id.trim()))
        .map(|d| d.label)
        .unwrap_or_else(|| "the chosen database".to_string())
}

/// Draw the open report with fresh rows from its stored procedure. Every
/// door is checked before anything connects: Database Access and writes
/// (`live_preview_gate`), a chosen connection, and the Dev login on it;
/// then `crystal::preview_live` checks the procedure and the parameters.
/// The bundle blocks on a runtime of its own, so the call runs on a
/// blocking thread (`crystal_blocking`), never on this one.
///
/// `on` is the modules that are on and `fetch` the bundle's
/// fetch-and-render (`preview_live_report`); both are parameters so a test
/// can show that a refused call never reaches the fetch.
pub async fn crystal_preview_live(
    ctx: &BridgeContext,
    body: &str,
    on: &std::collections::BTreeSet<crate::modules::Module>,
    fetch: impl FnOnce(&rpt_core::rpt_reader::Rpt, &rpt_core::LiveDb) -> Result<rpt_core::PreviewOutput, String>
        + Send
        + 'static,
) -> (u16, String) {
    let params = match crystal_args(body).and_then(|args| crystal_params(&args)) {
        Ok(params) => params,
        Err(refused) => return refused,
    };
    if let Err(refused) = live_preview_gate(on, ctx.db_writes) {
        return refused;
    }
    let conn = match crate::db::query::connection(ctx.db_secrets.as_deref(), ctx.db_id.as_deref()) {
        Ok(conn) => conn,
        Err(why) => return (409, why),
    };
    if crate::db::access_for_user(&conn.user) == crate::db::Access::ReadOnly {
        return (400, crate::db::READ_ONLY_SENTENCE.to_string());
    }
    let name = chosen_database_name(ctx);
    let done = crystal_blocking(move || crate::crystal::preview_live_with(&conn, &name, params, fetch)).await;
    // Past the doors a failed draw is kept as the preview (as with the saved one); a refusal is
    // only a re-read.
    crystal_changed().await;
    crystal_answer(done.and_then(|info| serde_json::to_value(info).map_err(|e| (500, e.to_string()))))
}

/// How to work with reports. The guide is a constant, so nothing here can
/// block or panic.
fn crystal_guide() -> (u16, String) {
    (200, crate::crystal::guide().to_string())
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
        "that is not a list of action scripts: {e}. Expected an array of {{ case_id, title, steps: [{{ step_number, actions }}] }} - call get_guide (topic autorun) for the format."
    )
}

fn bad_edits(e: serde_json::Error) -> String {
    format!(
        "that is not a list of declared edits: {e}. Each is {{ case_id, steps: [number], why, area (optional: true when the area changed), quirk (optional), precondition (optional: {{ op: \"add\" | \"remove\" | \"change\", flow, stage, value (only when two share the flow and stage), to (for add and change), reason }}) }}."
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
fn parse_save_request(
    body: &str,
    load: &dyn Fn(i32) -> Result<Option<crate::autorun::CaseScript>, String>,
) -> Result<SaveRequest, String> {
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
    // A `dry_run` inside a script is refused rather than dropped: dropped,
    // the save would be real, and a misplaced dry run must never save.
    if let serde_json::Value::Array(items) = &scripts_value {
        if items.iter().any(|s| s.get("dry_run").is_some()) {
            return Err(DRY_RUN_IN_A_SCRIPT.to_string());
        }
    }
    // A repair that sends only the steps it changes: merged into the saved
    // script here, so every gate below reads the whole script.
    if let serde_json::Value::Array(items) = &mut scripts_value {
        apply_patch_steps(items, load)?;
    }
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
    if let serde_json::Value::Array(items) = &scripts_value {
        for script in items {
            if let Some(r) = script.get("steps").and_then(crate::autorun::forbidden_fixes::forbidden_in) {
                return Err(match script.get("case_id") {
                    Some(id) => format!("case {id}: {r}"),
                    None => r,
                });
            }
        }
    }
    let mut scripts: Vec<crate::autorun::CaseScript> = serde_json::from_value(scripts_value).map_err(bad_scripts)?;
    // The same declaration sent twice (nested and at the top) is one; two
    // DIFFERENT declarations for one case are refused, since the gate
    // could only ever read one of them.
    let mut edits: Vec<crate::autorun::edits::Edit> = Vec::with_capacity(edit_values.len());
    for mut value in edit_values {
        // A precondition edit is one object: a list of them, or anything
        // else, is refused in words rather than serde's.
        if let Some(fields) = value.as_object_mut() {
            let case = fields.get("case_id").map(|c| format!("case {c}: ")).unwrap_or_default();
            match fields.get("precondition") {
                None | Some(serde_json::Value::Null | serde_json::Value::Object(_)) => {}
                Some(serde_json::Value::Array(list)) if list.len() > 1 => {
                    return Err(format!("{case}{}", crate::autorun::edits::ONE_PRECONDITION_EDIT))
                }
                Some(_) => return Err(format!("{case}{}", crate::autorun::edits::PRECONDITION_ONE_OBJECT)),
            }
        }
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
    // A precondition edit for a script sent with its preconditions as
    // saved - whole, or through `patch_steps` - is applied here, so every
    // gate below reads the script as it will be saved. A script sent with
    // the change already made is left as it is, and the gate checks it is
    // exactly the declared change.
    for edit in &edits {
        let Some(precondition) = edit.precondition.as_ref() else { continue };
        let Some(script) = scripts.iter_mut().find(|s| s.case_id == edit.case_id) else { continue };
        let Some(saved) = load(edit.case_id)? else { continue };
        if crate::autorun::edits::same_preconditions(&script.preconditions, &saved.preconditions) {
            let (applied, _) = crate::autorun::edits::apply_precondition_edit(&saved.preconditions, precondition)
                .map_err(|why| format!("case {}: {why}", edit.case_id))?;
            script.preconditions = applied;
        }
    }
    Ok(SaveRequest { scripts, edits, dry_run })
}

/// A script entry that carries `patch_steps` in place of `steps`: a repair
/// that sends only the steps it changes, each `{ step, actions }` (any
/// other field of a step, such as `unchecked`, may come with it). Each
/// replaces those fields of that step of the case's SAVED script, every
/// other step is kept as saved, and any other key of the entry (`title`,
/// `area`, ...) replaces the saved one. The result is the whole script, as
/// if it had been sent whole, so the repair's `edits` must still declare
/// every patched step. Refused: an entry with both `steps` and
/// `patch_steps`, one with no `case_id`, a case with no saved script, a
/// step the saved script does not have (send the whole script to add one),
/// and a step patched twice.
pub fn apply_patch_steps(
    items: &mut [serde_json::Value],
    load: &dyn Fn(i32) -> Result<Option<crate::autorun::CaseScript>, String>,
) -> Result<(), String> {
    for item in items.iter_mut() {
        let Some(map) = item.as_object_mut() else { continue };
        let Some(patches) = map.remove("patch_steps").filter(|p| !p.is_null()) else { continue };
        let case_id = map
            .get("case_id")
            .and_then(serde_json::Value::as_i64)
            .and_then(|n| i32::try_from(n).ok())
            .ok_or_else(|| "a script with \"patch_steps\" needs its \"case_id\"".to_string())?;
        if map.contains_key("steps") {
            return Err(format!("case {case_id}: send \"steps\" (the whole script) or \"patch_steps\" (only the changed steps), not both"));
        }
        let stored = load(case_id)?.ok_or_else(|| {
            format!("case {case_id} has no saved script to patch - send its whole script with \"steps\"")
        })?;
        let mut whole = serde_json::to_value(&stored).map_err(|e| e.to_string())?;
        let patches = match patches {
            serde_json::Value::Array(list) => list,
            one @ serde_json::Value::Object(_) => vec![one],
            _ => return Err(format!("case {case_id}: \"patch_steps\" is a list of {{ step, actions }}")),
        };
        let Some(steps) = whole.get_mut("steps").and_then(serde_json::Value::as_array_mut) else {
            return Err(format!("case {case_id}: its saved script has no steps to patch"));
        };
        let mut patched: Vec<i64> = Vec::new();
        for patch in patches {
            let Some(fields) = patch.as_object() else {
                return Err(format!("case {case_id}: each of \"patch_steps\" is {{ step, actions }}"));
            };
            let Some(n) = fields.get("step").or_else(|| fields.get("step_number")).and_then(serde_json::Value::as_i64) else {
                return Err(format!("case {case_id}: each of \"patch_steps\" names its \"step\" by number"));
            };
            if !fields.contains_key("actions") {
                return Err(format!("case {case_id}: patch_steps step {n} has no \"actions\""));
            }
            if patched.contains(&n) {
                return Err(format!("case {case_id}: patch_steps names step {n} twice"));
            }
            patched.push(n);
            let Some(step) = steps.iter_mut().find(|s| s.get("step_number").and_then(serde_json::Value::as_i64) == Some(n)) else {
                return Err(format!(
                    "case {case_id}: patch_steps step {n} is not in its saved script - send the whole script with \"steps\" to add a step"
                ));
            };
            for (k, v) in fields.iter().filter(|(k, _)| !matches!(k.as_str(), "step" | "step_number")) {
                step[k.as_str()] = v.clone();
            }
        }
        // The entry's own keys win over the saved ones.
        for (k, v) in map.iter() {
            whole[k.as_str()] = v.clone();
        }
        *item = whole;
    }
    Ok(())
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
/// (`changes`, `needs_unchanged`, `needs_state`): they affect order, not safety, so
/// marking a saved script is no repair and does not add to its count. The
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

/// What a dry run that passed every check answers. It opens with
/// `DRY_RUN_NOTHING_SAVED` and never says "saved N script(s)", so it can
/// never be read as the save's own answer.
pub fn dry_run_answer(count: usize, lines: &[String]) -> String {
    format!("{DRY_RUN_NOTHING_SAVED} It would save {count} script(s): {}", lines.join(", "))
}

/// Said to a save whose `dry_run` is inside a script rather than beside
/// `"scripts"`: nothing is checked or saved.
pub const DRY_RUN_IN_A_SCRIPT: &str = "\"dry_run\" goes beside \"scripts\", not inside a script - nothing was saved. Send { \"scripts\": [...], \"dry_run\": true }.";

/// How a script save's dry run answer begins.
pub const DRY_RUN_NOTHING_SAVED: &str = "Dry run: nothing was saved.";

/// Save one or many Auto Run action scripts, as an assistant writes them.
///
/// A BUNDLE by design: the body is an array, so a whole PBI's worth of
/// cases lands in one call - and the same shape is what the Auto Run
/// screen's Import button reads from a file. One case is a bundle of one.
///
/// ALL OR NOTHING, for real. Every gate below runs for every script
/// before a single byte is written: the body parses, each change to a
/// script that already exists is declared, and each script meets its test case's expected results. Only
/// then does `store::save_scripts_checked` run - and it re-checks, under the
/// scripts lock, that every script the gates read is still the one they
/// read (409 if not), then validates and
/// serialises every entry before writing any of them, so a bad case id
/// or a filesystem error on entry 16 of 30 can never leave the other 29
/// half-applied. An unknown action `kind` fails here rather than
/// mid-run, with the browser already open in front of them.
///
/// `repairs` NEVER comes from the body. It is the count of changes made
/// without a person looking, shown to the person in the script editor, and
/// only a person's own save sets it back to zero. A script that comes back
/// word for word unchanged does not add to it: re-saving a whole PBI after
/// fixing one case is exactly that, for every other case in the bundle.
async fn save_autorun_scripts(
    ctx: &BridgeContext,
    client: Option<&crate::ado::AdoClient>,
    body: &str,
    asker: Option<SlotNo>,
) -> (u16, String) {
    // What the caller's discovery carried from the areas it left, while it goes on.
    let carried = carried_of(asker).await;
    save_autorun_scripts_carrying(ctx, client, body, asker, carried).await
}

/// [`save_autorun_scripts`] with the areas the caller's discovery carried
/// (`DiscoveryState::carried`) already read: the save's seen check lays them
/// over the saved map (`discovery_map::add_carried`).
pub async fn save_autorun_scripts_carrying(
    ctx: &BridgeContext,
    client: Option<&crate::ado::AdoClient>,
    body: &str,
    asker: Option<SlotNo>,
    carried: Vec<crate::autorun::discovery_map::AreaMap>,
) -> (u16, String) {
    // Parsing reads the saved script of a repair (to merge its patch onto
    // it); that read, not the later one at Gate 1, is the version the
    // bundle is built from, so it is the one the write checks against.
    let reads = crate::autorun::store::ScriptReads::new();
    let load = |id: i32| match autorun_root() {
        Ok(root) => reads.load(&root, id),
        Err((_, why)) => Err(why),
    };
    let SaveRequest { scripts, edits, dry_run } = match parse_save_request(body, &load) {
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
    // Marks a repair left alone because the step's checks did not change.
    let mut kept_marks: Vec<String> = Vec::new();
    // Which steps of each script the seen check reads, by case: `None`
    // for a new script (all of them), its declared steps for a repair. An
    // unchanged resave is not listed: it was checked when it was saved.
    let mut seen_scope: Vec<(i32, Option<Vec<i32>>)> = Vec::new();
    // The saved script of each repair, for the check floor and the values a
    // repair may not move away from the case (`case_values::check_repair_values`).
    let mut repair_olds: Vec<(i32, crate::autorun::CaseScript)> = Vec::new();
    // One line per check this save's repairs give up, said after the save's
    // first line (outside the notes cap).
    let mut given_up_lines: Vec<String> = Vec::new();
    // Whether scripts saved before they were stamped count here, read once.
    let legacy = crate::autorun::seen_check::legacy_scripts_count(&root, &ctx.org, &ctx.project);
    // Every case's refusal from this gate, said together: a bundle with two
    // bad repairs is told about both in one answer.
    let mut refused: Vec<String> = Vec::new();
    // What each repair's precondition edit did, one line per case, said
    // after the save's first line. The flows name the stages, read only
    // when a precondition edit is declared; without them the ids are said.
    let mut precondition_lines: Vec<String> = Vec::new();
    // Cases kept unchecked because a divergence of theirs waits for a ruling, said after those.
    let mut unchecked_lines: Vec<String> = Vec::new();
    let flows = if edits.iter().any(|e| e.precondition.is_some()) {
        crate::api_templates::flow_store::list(&root, &ctx.org, &ctx.project).unwrap_or_default()
    } else {
        Vec::new()
    };
    // The version of each case's script this bundle was built from (`None`:
    // there was none): the first read of it, else the one below. The write
    // re-checks these under the scripts lock, so a save that overlaps this
    // one cannot be overwritten unseen.
    let mut read: Vec<(i32, Option<String>)> = Vec::with_capacity(scripts.len());
    for sent in &scripts {
        let existing = match crate::autorun::store::load_script(&root, sent.case_id) {
            Ok(v) => v,
            Err(e) => return (500, e),
        };
        read.push((sent.case_id, reads.first_version(sent.case_id, existing.as_ref())));
        let declared = edits.iter().find(|e| e.case_id == sent.case_id);
        // An unchanged re-send never lowers the floor it was stored with.
        let mut stored_floor: u32 = 0;
        let mut script = sent.clone();
        // A new script is checked whole; an unchanged one and a repair keep
        // steps checked by an earlier save, so they vouch as it did.
        let vouched = existing
            .as_ref()
            .is_none_or(|old| crate::autorun::seen_check::vouch_carries_over(old, &ctx.org, &ctx.project, legacy));
        script.organization = Some(ctx.org.trim().to_string());
        script.project = Some(ctx.project.trim().to_string());
        let may_check = crate::autorun::proven::may_be_checked(&root, &ctx.org, &ctx.project, sent.case_id);
        script.checked = vouched && may_check;
        if vouched && !may_check {
            unchecked_lines.push(format!(
                "case {}: not marked checked: a divergence of the case waits for a person's ruling, and the script counts as checked once a run passes after the ruling",
                sent.case_id
            ));
        }
        match existing {
            // A re-send that changes nothing is not a repair. The count
            // is of changes made without a person looking, so an
            // identical bundle - which is what saving a whole PBI again
            // after fixing one case IS, for every other case in it -
            // must not raise it. A declaration
            // for such a case is still refused, by `check_edits`' own
            // rule 2: it names a step nothing happened to.
            Some(old) if unchanged_script(&old, sent) => {
                if declared.is_some() {
                    if let Err(why) = crate::autorun::edits::check_edits(&old, sent, declared) {
                        refused.push(why);
                        continue;
                    }
                }
                script.repairs = old.repairs;
                script.last_repair = old.last_repair.clone();
                script.precondition_repairs = old.precondition_repairs.clone();
                script.given_up = old.given_up.clone();
                stored_floor = old.checks;
                let marks_changed = old.changes != sent.changes
                    || old.needs_unchanged != sent.needs_unchanged
                    || old.needs_state != sent.needs_state;
                let said = if marks_changed { "marks updated" } else { "unchanged" };
                lines.push(format!("case {} ({said})", script.case_id));
            }
            Some(old) => {
                if let Err(why) = crate::autorun::edits::check_edits(&old, sent, declared) {
                    // A body with no declarations at all, refused for
                    // something a declaration would cover (the sentences
                    // that point at "edits"): say first that the list
                    // itself is missing, which is the actual fix. Only
                    // then: when the bundle declares OTHER cases, the list
                    // is there and this case's entry is what is missing,
                    // which `check_edits`' own "case N: ... not declared"
                    // sentence already says accurately.
                    if edits.is_empty() && why.contains("\"edits\"") {
                        refused.push(format!(
                            "case {} already has a script, so this save is a repair, and \"edits\" is missing. {why}",
                            sent.case_id
                        ));
                        continue;
                    }
                    refused.push(why);
                    continue;
                }
                script.repairs = old.repairs.saturating_add(1);
                // What was given up before stays; this repair's own entries
                // are added from its edit, never from what was sent.
                script.given_up = old.given_up.clone();
                if let Some(e) = declared {
                    let at = crate::applog::iso_stamp();
                    for g in &e.gives_up {
                        let entry = crate::autorun::GivenUp {
                            step: g.step,
                            kind: g.kind.trim().to_lowercase(),
                            why: g.why.trim().to_string(),
                            at: at.clone(),
                        };
                        given_up_lines.push(format!(
                            "case {}: check given up: step {} {} - {}. The person sees it on the case; name it in your report.",
                            script.case_id, entry.step, entry.kind, entry.why
                        ));
                        script.given_up.push(entry);
                    }
                }
                repair_olds.push((sent.case_id, old.clone()));
                // `check_edits` above only returns Ok when a real change
                // was fully declared, so `declared` is always Some here -
                // there is no path where a script actually differs from
                // disk and this branch is reached with nothing declared.
                seen_scope.push((sent.case_id, crate::autorun::seen_check::steps_to_check(declared)));
                script.precondition_repairs = old.precondition_repairs.clone();
                if let Some(e) = declared {
                    let why = e.why.trim();
                    // `check_edits` above applied the same edit to the same
                    // saved preconditions, so it applies here too.
                    let precondition_line = e
                        .precondition
                        .as_ref()
                        .and_then(|p| crate::autorun::edits::apply_precondition_edit(&old.preconditions, p).ok())
                        .map(|(_, record)| {
                            let line = crate::autorun::preconditions::repair_line(&flows, &record);
                            script.precondition_repairs.push(record);
                            line
                        });
                    if !dry_run {
                        if !why.is_empty() {
                            crate::applog::info(format!(
                                "AI repaired case {} steps {:?}: {why}",
                                script.case_id, e.steps
                            ));
                        }
                        if let Some(line) = &precondition_line {
                            crate::applog::info(format!("AI repaired case {}: {line}", script.case_id));
                        }
                    }
                    script.last_repair = Some(match &precondition_line {
                        Some(line) if why.is_empty() => line.clone(),
                        Some(line) => format!("{why}; {line}"),
                        None => why.to_string(),
                    });
                    if let Some(line) = precondition_line {
                        precondition_lines.push(format!("case {}: {line}", script.case_id));
                    }
                    if let Some(d) = old.suspected_defect.as_ref().filter(|d| e.steps.contains(&d.step_number)) {
                        // Only a repair of the step's checks takes the mark:
                        // a moved locator is no decision that the step was
                        // wrong. A step missing on either side is changed.
                        let at = |s: &crate::autorun::CaseScript| s.steps.iter().find(|x| x.step_number == d.step_number).cloned();
                        let changed = match (at(&old), at(&script)) {
                            (Some(before), Some(after)) => crate::autorun::defects::checks_changed(&before, &after),
                            _ => true,
                        };
                        if changed {
                            repaired_marks.push((script.case_id, d.step_number));
                        } else {
                            kept_marks.push(crate::autorun::defects::kept_line(script.case_id, d.step_number));
                        }
                    }
                    // Read now, while the script on disk is still the
                    // one that ran: its targets classify the failure.
                    if e.quirk.as_deref().is_some_and(|t| !t.trim().is_empty()) {
                        repair_sources.push(repair_source(&root, &old, e));
                    }
                }
                lines.push(format!("case {} (repaired)", script.case_id));
            }
            None => {
                if declared.is_some() {
                    refused.push(format!(
                        "case {} has no script yet - \"edits\" is for changing one that exists",
                        sent.case_id
                    ));
                    continue;
                }
                script.repairs = 0;
                script.last_repair = None;
                script.precondition_repairs = Vec::new();
                script.given_up = Vec::new();
                seen_scope.push((sent.case_id, None));
                lines.push(format!("case {} (new)", script.case_id));
            }
        }
        script.checks = (crate::autorun::case_values::check_count(&script) as u32).max(stored_floor);
        prepared.push(script);
    }
    if !refused.is_empty() {
        return (400, refused.join("\n"));
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
    // Every case's shortfalls, said together.
    let mut short: Vec<String> = Vec::new();
    for script in &prepared {
        let Some(case) = cases.iter().find(|c| c.id == script.case_id) else {
            short.push(format!("case {} is not a test case in this organization", script.case_id));
            continue;
        };
        let shortfalls =
            crate::autorun::floor::check_floor(script, &crate::autorun::floor::expected_of(&case.steps));
        if !shortfalls.is_empty() {
            short.push(format!("case {}: {}", script.case_id, shortfalls.join("; ")));
        }
    }
    if !short.is_empty() {
        return (400, short.join("\n"));
    }

    // The environment's test name prefix, for the note on a record a script
    // relies on without declaring it, and for the values a check may carry
    // that the case never held.
    let test_prefix = crate::environments::active(&root)
        .map(|e| e.test_prefix)
        .unwrap_or_else(|_| crate::environments::default_test_prefix());
    // The project's quirks, for the class a repair's quirk will actually have
    // and whether it fits (a file that cannot be read is judged as empty; the
    // record itself says what it could not do).
    let quirks_now = crate::autorun::quirks::load_quirks(&root, &ctx.org, &ctx.project).unwrap_or_default();

    // Gate 2b: a new or changed step's check values come from the case. A
    // value the case does not hold needs a "because"; a count the case
    // states differently is refused. Unchanged steps are not compared.
    let mut value_refusals: Vec<String> = Vec::new();
    let mut value_sources: Vec<(i32, Vec<crate::autorun::case_values::SourcedValue>)> = Vec::new();
    let mut value_notes: Vec<String> = Vec::new();
    for (case_id, only) in &seen_scope {
        let (Some(script), Some(case)) =
            (prepared.iter().find(|s| s.case_id == *case_id), cases.iter().find(|c| c.id == *case_id))
        else {
            continue;
        };
        let text = crate::autorun::case_text::CaseText::from_case(case);
        // A repair also keeps its checks and the case's values.
        if let (Some((_, old)), Some(edit)) =
            (repair_olds.iter().find(|(id, _)| id == case_id), edits.iter().find(|e| e.case_id == *case_id))
        {
            value_refusals.extend(
                crate::autorun::case_values::check_repair_values(old, script, &text, &edit.steps, &edit.gives_up, &test_prefix)
                    .into_iter()
                    .map(|r| format!("case {case_id}: {r}")),
            );
            // And it never lets the page decide whether a failure counts.
            value_refusals.extend(
                crate::autorun::forbidden_fixes::check_repair_absorbs_with(old, script, Some(edit), &quirks_now)
                    .into_iter()
                    .map(|r| format!("case {case_id}: {r}")),
            );
        }
        // A repair holds only the values new or changed in its steps.
        let old_script = repair_olds.iter().find(|(id, _)| id == case_id).map(|(_, old)| old);
        let found = crate::autorun::case_values::check_case_values_against(
            script,
            &text,
            only.as_deref(),
            &test_prefix,
            old_script,
        );
        value_refusals.extend(found.refusals.into_iter().map(|r| format!("case {case_id}: {r}")));
        value_sources.push((*case_id, found.sourced));
        value_notes.extend(found.notes.into_iter().map(|n| format!("case {case_id}: note: {n}")));
    }
    let value_listed = crate::autorun::case_values::value_source_lines(&value_sources);
    if !value_refusals.is_empty() {
        return (400, value_refusals.join("\n"));
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
    // The project's own save words, for what counts as saving a record.
    let save_words = crate::autorun::nav::load_nav(&root, &ctx.org, &ctx.project).map(|n| n.save_words).unwrap_or_default();
    let check_and_save = || -> Result<Vec<String>, SaveRefusal> {
        // What the save noticed without refusing: said after its answer.
        let mut notes: Vec<String> = Vec::new();
        if !seen_scope.is_empty() {
            let (map, components, files) = seen_check_inputs(&root, ctx, uses_components, &saved_scripts, &carried)
                .map_err(|e| SaveRefusal::Other(400, e))?;
            // The placement of a timed or same_document check, again with each component expanded.
            if uses_components {
                for (case_id, only) in &seen_scope {
                    let Some(script) = prepared.iter().find(|s| s.case_id == *case_id) else { continue };
                    crate::autorun::store::check_expanded_response_timings(script, only.as_deref(), &components)
                        .map_err(|e| SaveRefusal::Other(400, e))?;
                }
            }
            // Every script is checked, and every locator never seen in any
            // of them is named at once; a refusal of any other kind is
            // answered alone, as it always was.
            let mut unseen: Vec<(i32, Vec<String>)> = Vec::new();
            // Locators only ever seen matching nothing, used to check that
            // something is absent: refused after the unseen ones, which a
            // discovery in progress may still record.
            let mut unproven: Vec<(i32, Vec<String>)> = Vec::new();
            // Absence checks let through on the case's own words: listed
            // first among the notes, so the cap never drops one before the
            // others.
            let mut from_case: Vec<String> = Vec::new();
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
                    SeenVerdict::Passed => {
                        let review = crate::autorun::seen_check::absence_review(
                            &map,
                            script,
                            &case_step_text(case),
                            only.as_deref(),
                            saved_scripts.iter().find(|s| s.case_id == *case_id),
                        );
                        if !review.refusals.is_empty() {
                            unproven.push((*case_id, review.refusals));
                        }
                        from_case.extend(review.from_case.into_iter().map(|n| format!("case {case_id}: note: {n}")));
                    }
                    SeenVerdict::Other(why) => return Err(SaveRefusal::Unseen(why)),
                    SeenVerdict::Unseen(lines) => unseen.push((*case_id, lines)),
                }
            }
            if !unseen.is_empty() {
                return Err(SaveRefusal::Unseen(unseen_in_bundle(&unseen)));
            }
            if !unproven.is_empty() {
                return Err(SaveRefusal::Other(400, unseen_in_bundle(&unproven)));
            }
            // Everything is seen. Notes, for the new and changed steps only.
            notes.extend(from_case);
            for (case_id, only) in &seen_scope {
                let Some(script) = prepared.iter().find(|s| s.case_id == *case_id) else { continue };
                let sentences = crate::autorun::seen_check::loose_name_notes(&map, &components, script, only.as_deref())
                    .into_iter()
                    .chain(crate::autorun::preconditions::undeclared_record_note(script, only.as_deref(), &test_prefix, &save_words));
                notes.extend(sentences.map(|n| format!("case {case_id}: note: {n}")));
            }
            // The value check's notes count toward the same cap.
            notes.extend(value_notes.iter().cloned());
            // The delete notes are style warnings: after the case-value
            // notes, so the cap drops them first.
            for (case_id, only) in &seen_scope {
                let Some(script) = prepared.iter().find(|s| s.case_id == *case_id) else { continue };
                let sentences = crate::autorun::script_notes::delete_after_check_notes(script, Some(&components), only.as_deref())
                    .into_iter()
                    .chain(crate::autorun::script_notes::positional_delete_notes(script, Some(&components), only.as_deref()));
                notes.extend(sentences.map(|n| format!("case {case_id}: note: {n}")));
            }
            if notes.len() > MAX_SAVE_NOTES {
                let more = notes.len() - MAX_SAVE_NOTES;
                notes.truncate(MAX_SAVE_NOTES);
                notes.push(format!("and {more} more notes"));
            }
        }
        // Everything has passed. A dry run checks the bundle as the write
        // would and stops there; a save writes it.
        if dry_run {
            return crate::autorun::store::check_scripts(&prepared)
                .map(|()| notes)
                .map_err(|e| SaveRefusal::Other(400, e));
        }
        match crate::autorun::store::save_scripts_checked(&root, &prepared, &read) {
            Ok(()) => Ok(notes),
            Err(crate::autorun::store::SaveScriptsError::Invalid(e)) => Err(SaveRefusal::Other(400, e)),
            Err(changed @ crate::autorun::store::SaveScriptsError::Changed { .. }) => {
                Err(SaveRefusal::Other(409, changed.to_string()))
            }
            Err(unread @ crate::autorun::store::SaveScriptsError::Unreadable { .. }) => {
                Err(SaveRefusal::Other(500, unread.to_string()))
            }
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
        let targets = seen_check_inputs(&root, ctx, uses_components, &saved_scripts, &carried).ok().and_then(|(map, components, files)| {
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
        // On the page of the caller's own browser, if it has one.
        if let (Some(n), Some(targets)) = (asker, targets.filter(|t| !t.is_empty())) {
            if let Some(found) = record_refused_on_page(ctx, &root, &areas, &targets, n).await {
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
    let notes = match checked {
        Ok(notes) => notes,
        Err(refused) => return answer(refused.said()),
    };
    if dry_run {
        let blocked = blocked_by_divergence_lines(&root, ctx, &prepared);
        let said = std::iter::once(dry_run_answer(prepared.len(), &lines))
            .chain(precondition_lines)
            .chain(unchecked_lines)
            .chain(given_up_lines)
            .chain(value_listed)
            .chain(notes)
            .chain(blocked);
        return (200, said.collect::<Vec<_>>().join("\n"));
    }
    crate::applog::info(format!("AI saved {} auto-run script(s)", prepared.len()));

    let mut report = vec![format!("saved {} script(s): {}", prepared.len(), lines.join(", "))];
    report.extend(precondition_lines);
    report.extend(unchecked_lines);
    report.extend(given_up_lines);
    report.extend(value_listed);
    report.extend(notes);
    report.extend(kept_marks);
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
    // A repair declared for the steps of a case the person is rewriting
    // closes the divergence at those steps: the script now follows the
    // rewritten case. A failure to close is said, never a refusal: the
    // scripts are already down.
    for edit in edits.iter().filter(|e| !e.steps.is_empty()) {
        let now = crate::autorun::sessions::now_ms();
        // A step the rewrite renumbered is found by its action text.
        let text = cases.iter().find(|c| c.id == edit.case_id).map(crate::autorun::case_text::CaseText::from_case);
        let closed = crate::autorun::divergences::update(&root, &ctx.org, &ctx.project, |list| {
            Ok(crate::autorun::divergences::close_repaired_with(list, edit.case_id, &edit.steps, now, &|step| {
                text.as_ref().and_then(|t| t.step_action_folded(step))
            }))
        });
        match closed {
            Ok(steps) => report.extend(steps.into_iter().map(|s| {
                format!(
                    "case {}: the divergence at step {s} is closed - the script now follows the rewritten case",
                    edit.case_id
                )
            })),
            Err(e) => {
                // Said only when something was waiting on this repair.
                let waiting = crate::autorun::divergences::load(&root, &ctx.org, &ctx.project)
                    .map(|l| {
                        l.iter().any(|d| {
                            d.case_id == edit.case_id
                                && d.status == crate::autorun::divergences::DivergenceStatus::AwaitingRepair
                                && edit.steps.contains(&d.step)
                        })
                    })
                    .unwrap_or(false);
                if waiting {
                    let at = edit.steps.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(", ");
                    report.push(format!(
                        "case {}: the divergence at step {at} could not be closed: {e}",
                        edit.case_id
                    ));
                }
            }
        }
    }
    report.extend(blocked_by_divergence_lines(&root, ctx, &prepared));
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
        match crate::autorun::quirks::record_quirk_classed(
            &root,
            &ctx.org,
            &ctx.project,
            text,
            "assistant",
            crate::autorun::quirks::FROM_AUTORUN,
            sources,
            edit.quirk_class,
            crate::autorun::sessions::now_ms(),
        )
        .map(|(r, _)| r)
        {
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

/// One line for each saved case that a divergence keeps from running: its
/// runs are Blocked until a person rules (or the repair lands). Read only;
/// a divergences file that cannot be read says nothing.
fn blocked_by_divergence_lines(
    root: &std::path::Path,
    ctx: &BridgeContext,
    saved: &[crate::autorun::CaseScript],
) -> Vec<String> {
    let Ok(list) = crate::autorun::divergences::load(root, &ctx.org, &ctx.project) else {
        return Vec::new();
    };
    saved
        .iter()
        .filter_map(|s| {
            let d = crate::autorun::divergences::blocking(&list, s.case_id)?;
            Some(format!(
                "case {}: {} - runs are Blocked until then",
                s.case_id,
                crate::autorun::divergences::run_sentence(d)
            ))
        })
        .collect()
}

/// The most notes one save answers with before saying how many more.
const MAX_SAVE_NOTES: usize = 20;

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
/// reads them: plain words, the Markdown taken out.
fn case_step_text(case: &crate::ado::TestCaseFull) -> Vec<String> {
    crate::steps_xml::plain_texts(&case.steps)
}

/// What a script save's seen check reads: the discovery map, the project's
/// components (only when a checked script uses one, so a damaged file
/// never holds up a save that does not need it) and its Test files, so the
/// size the app shows for one a script uploads passes as the script's own
/// data.
/// The areas the discovery in the asker's browser carried over from areas it
/// left (`DiscoveryState::carried`); none with no discovery or no browser, or
/// when the browser is busy.
async fn carried_of(asker: Option<SlotNo>) -> Vec<crate::autorun::discovery_map::AreaMap> {
    let Some(n) = asker else { return Vec::new() };
    let Ok(slot) = tokio::time::timeout(std::time::Duration::from_millis(500), crate::commands::autorun::session_slot(n).lock()).await
    else {
        return Vec::new();
    };
    slot.as_ref().and_then(|s| s.discovery.as_ref()).map(|d| d.carried.clone()).unwrap_or_default()
}

fn seen_check_inputs(
    root: &std::path::Path,
    ctx: &BridgeContext,
    uses_components: bool,
    saved_scripts: &[crate::autorun::CaseScript],
    carried: &[crate::autorun::discovery_map::AreaMap],
) -> Result<
    (crate::autorun::discovery_map::DiscoveryMap, crate::autorun::components::ComponentFile, Vec<crate::test_files::TestFile>),
    String,
> {
    let mut map = crate::autorun::seen_check::load_checked_map_with(root, &ctx.org, &ctx.project, saved_scripts)?;
    crate::autorun::discovery_map::add_carried(&mut map, carried);
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
    caller: &crate::autorun::agents::Caller,
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
                         start writing. If the job is for a PBI and you are the assistant the \
                         person talks to, call `get_current_pbi` first: when the developer \
                         named no PBI and TCM has one selected, say \
                         \"PBI <id>, <title>, is selected in TCM - is that the one?\" and wait \
                         for their yes; when they named a different one, ask which to use; \
                         when TCM has none, find it with `search_pbis` or ask for the id. A \
                         subagent never asks: it uses the PBI id it was given. When they have \
                         answered, call this tool again with \
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
    note_writing_job_pbi(caller, &ctx.org, answers.examples_pbi);

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

                // How the text is formatted. validate has no copy of an
                // updated case's current text to compare with, so a case
                // with an id gets only the warnings that plain text read
                // back from Azure DevOps can never trigger (see
                // `case_format::formatting_warnings`).
                warnings.extend(crate::case_format::formatting_warnings(
                    i + 1,
                    tc,
                    tc.update_id.is_none(),
                ));

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
                                 this organization - pick one from get_guide (topic writing).",
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
    // know it was cut short, and hand a half-draft to the next step.
    if let Err(e) = crate::ai_tools::atomic_write(std::path::Path::new(&req.output_path), &text) {
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
    caller: &crate::autorun::agents::Caller,
    ctx: &BridgeContext,
    client: Option<&crate::ado::AdoClient>,
    target: &str,
) -> (u16, String) {
    let project = tags_project(caller, ctx, client).await;
    let key = crate::cache::keys::tags(&ctx.org, &project);
    let (values, source) = match crate::cache::get::<Vec<String>>(&key) {
        Some(v) => (v, "cache"),
        None => match client {
            Some(c) => match c.get_tags(&ctx.org, &project).await {
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
    let mut out = serde_json::json!({
        "tags": values,
        "count": values.len(),
        "total": total,
        "source": source,
        "note": note,
    });
    // Tags are per project, so while combined the answer says whose they
    // are. Off, the answer is exactly what it always was.
    if ctx.all_projects {
        out["project"] = serde_json::json!(project);
    }
    (200, out.to_string())
}

/// Who a writing job belongs to: one assistant (its proxy id and, for a named
/// subagent, its name), or `None` for the default caller (an older proxy, a
/// test, the person).
type JobOwner = Option<crate::autorun::agents::AgentKey>;

fn job_owner(caller: &crate::autorun::agents::Caller) -> JobOwner {
    match caller {
        crate::autorun::agents::Caller::Default => None,
        crate::autorun::agents::Caller::Agent(key) => Some(key.clone()),
    }
}

/// The PBI each assistant's latest writing job named as its reference (the
/// intake's `examples_pbi`), per organization, kept so `get_tags` can read
/// the tags of the project that job's cases belong to. Process coordination,
/// not cached data: it holds only what the intake named, nothing fetched.
/// In memory only. A later job by the same assistant in the same organization
/// replaces its entry (a job naming no PBI removes it), an assistant's
/// goodbye drops its entries, a different account signing in drops all of
/// them, and it is gone when the app closes.
static WRITING_JOB_PBIS: StdMutex<std::collections::BTreeMap<(JobOwner, String), i32>> =
    StdMutex::new(std::collections::BTreeMap::new());

fn note_writing_job_pbi(caller: &crate::autorun::agents::Caller, org: &str, pbi: Option<i32>) {
    let mut jobs = WRITING_JOB_PBIS.lock().unwrap_or_else(|e| e.into_inner());
    let key = (job_owner(caller), org.to_string());
    match pbi {
        Some(p) => {
            jobs.insert(key, p);
        }
        None => {
            jobs.remove(&key);
        }
    }
}

/// An assistant said goodbye, or its session was found gone past its grace:
/// its jobs (and its named subagents') are over.
pub fn forget_writing_jobs_of(agent_id: &str) {
    WRITING_JOB_PBIS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|(owner, _), _| owner.as_ref().is_none_or(|k| k.id != agent_id));
}

/// Every job's PBI is forgotten: called when a different account signs in,
/// as the caches are, so one person's job is never read as the next one's.
pub fn forget_writing_jobs() {
    WRITING_JOB_PBIS.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// Whose tags `get_tags` reads: the home project (where cases are written),
/// except that while combined the caller's own writing job naming a PBI
/// points at that PBI's project. A PBI whose project cannot be read leaves
/// it at home.
async fn tags_project(
    caller: &crate::autorun::agents::Caller,
    ctx: &BridgeContext,
    client: Option<&crate::ado::AdoClient>,
) -> String {
    if !ctx.all_projects {
        return ctx.project.clone();
    }
    let pbi = WRITING_JOB_PBIS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&(job_owner(caller), ctx.org.clone()))
        .copied();
    match (pbi, client) {
        (Some(pbi), Some(c)) => {
            work_item_project_cached(ctx, c, pbi).await.unwrap_or_else(|_| ctx.project.clone())
        }
        _ => ctx.project.clone(),
    }
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

/// The guide's custom style section: the person's saved style while the
/// Writing Style module is on (`on`), nothing while it is off - the guide is
/// then exactly the standard one, whatever is saved. `on` is passed in so
/// both answers are testable.
pub fn custom_style_when(on: bool) -> Option<String> {
    if !on {
        return None;
    }
    crate::writing_style::current_text().map(|t| custom_style_section(&t))
}

/// Live writing guide: format rules from the importer's own constants +
/// the org's Module values, fetched fresh (no snapshot staleness). The
/// person's writing style is read from disk on every call, so a save takes
/// effect the next time an assistant reads the guide.
async fn guide(ctx: &BridgeContext, client: &crate::ado::AdoClient) -> String {
    let custom = custom_style_when(crate::modules::on(crate::modules::Module::WritingStyle));
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
        `automation_status` (exactly {statuses}),\n\
        `priority` (optional, a number 1-4: a new case without one is created with 2,\n\
        and an update without one leaves the work item's priority as it is), `module` (ONLY from the list\n\
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
        - AVOID markdown and asterisks in titles (reviewer_notes is rendered as\n\
        markdown and needs none). Steps, expected results and preconditions\n\
        follow the Formatting section.\n\
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
        ## Formatting\n\
        Steps and preconditions are Markdown. Use formatting only where it helps\n\
        reading; never excessively. In a step, bold the one value or control it\n\
        turns on, such as the **Save** button. Use a list in the preconditions\n\
        for several conditions. Put any SQL or code in a fenced block: a lead-in\n\
        sentence such as \"Run the following query.\", then the query in a\n\
        ```sql fence. In a step its lines stay plain lines in Azure DevOps; in\n\
        the preconditions it shows as code. A step keeps bold, italic and line\n\
        breaks, a few bold words at most; never headings, tables, links,\n\
        images or emphasis on a whole sentence. Text Markdown would change\n\
        (`\\\\server`, a line starting with `>`, a `---` line, `__init__`) goes in\n\
        backticks.\n\n\
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
        {transform_ops}\n\
        ## Workflow\n\
        0. The assistant the person talks to settles the PBI, if the job is for\n\
        one: call `get_current_pbi`. The developer named one that is TCM's: use\n\
        it, no search. A different one: ask which. None named and TCM has one:\n\
        say \"PBI <id>, <title>, is selected in TCM - is that the one?\" and wait\n\
        for their yes. TCM has none: `search_pbis`, or ask for the id; a job for\n\
        a specification alone needs no PBI. Pass the settled PBI id to every\n\
        slice subagent: a subagent never asks about the PBI, it uses the one it\n\
        was given.\n\
        Then call `begin_test_case_writing` and put its questions to the\n\
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
        Its options: `dry_run: true` answers only the report (run it first and\n\
        read `expected_rewritten`, every expected result it would shorten,\n\
        before and after). `entry` is the first step of every preamble\n\
        (default \"Launch the application.\"); a non-launch entry, such as\n\
        opening a module, goes after the sign-in step. `reorder: false` keeps a\n\
        set meant to be read against the specification in document order -\n\
        navigation and expected results are still cleaned up. `trim_expected:\n\
        false` leaves every expected result exactly as written and still does\n\
        the navigation, ordering and dedupe work: use it when the value of the\n\
        set is in what each expected result ASSERTS (arithmetic, formulas,\n\
        quoted messages), or when a dry run reports that reordering saved 0\n\
        environment switches. For a large draft pass `path`, and `in_place:\n\
        true` to write the result back to that file (atomic) and get only the\n\
        report; an in-place write keeps the file's other fields (`specs`,\n\
        `comments`), and without it the response carries the file's `specs`\n\
        for you to write back.\n\
        4. Call `validate_cases` and fix every warning. For a large draft,\n\
        pass a local file via its `path` argument instead of inlining the\n\
        JSON.\n\
        5. For later edits - retagging, retitling, setting a module - call\n\
        `transform_cases` instead of rewriting the file yourself.\n",
        org = ctx.org,
        project = ctx.project,
        transform_ops = crate::transform::ops_reference(),
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
                let mut rec = serde_json::json!({
                    "id": c.id,
                    "title": c.title,
                    "tags": c.tags,
                    "automation_status": c.automation_status,
                    "module": c.module_value,
                    "preconditions": c.preconditions,
                    "steps": c.steps.iter().map(crate::import_parser::step_json).collect::<Vec<_>>(),
                });
                // Present only when the work item has one, the importer's
                // shape rule for optional fields.
                if let Some(p) = c.priority {
                    rec["priority"] = serde_json::json!(p);
                }
                rec
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

/// The plans and suites the Test Suites tab shows for `project`, cached in
/// memory per org and project for `cache::keys::SUITE_TREE_TTL`;
/// `refresh=true` reads them again. Session tier: the tree is large, and
/// TestPlan's `#[serde(skip)]` fields would not survive a trip through the
/// disk.
async fn suite_tree(
    ctx: &BridgeContext,
    client: &crate::ado::AdoClient,
    project: &str,
    refresh: bool,
) -> Result<SuiteTree, crate::ado::AdoError> {
    let key = crate::cache::keys::suite_tree(&client.base_url, &ctx.org, project);
    if !refresh {
        if let Some(tree) = crate::cache::session_fresh::<SuiteTree>(&key, crate::cache::keys::SUITE_TREE_TTL) {
            return Ok(tree);
        }
    }
    let tree = client.list_plans_with_suites(&ctx.org, project).await?;
    crate::cache::session_put(&key, tree.clone());
    Ok(tree)
}

/// The project that holds test plan `plan`, for a combined `suite-cases`
/// call that was not told: the project whose cached suite tree lists the
/// plan (the suite search keeps them), else one organization-level work item
/// read, because a test plan is a work item and a work item never changes
/// project. A project list that cannot be read only costs the shortcut.
async fn plan_project(
    ctx: &BridgeContext,
    client: &crate::ado::AdoClient,
    plan: i32,
) -> Result<String, crate::ado::AdoError> {
    if let Ok(names) = client.org_project_names(&ctx.org).await {
        for name in names {
            let key = crate::cache::keys::suite_tree(&client.base_url, &ctx.org, &name);
            if let Some(tree) = crate::cache::session_fresh::<SuiteTree>(&key, crate::cache::keys::SUITE_TREE_TTL) {
                if tree.iter().any(|p| p.plan.id == plan) {
                    return Ok(name);
                }
            }
        }
    }
    work_item_project_cached(ctx, client, plan).await
}

/// Every suite in the project, one row each with its plan, filtered by
/// `q` against the plan name, the suite name, or a requirement id. The
/// plan id and suite id in a row are what `get_suite_test_cases` takes.
/// Combined, the rows are every project's (capped, one project at a time),
/// each naming its project, and the answer says how many projects it read.
async fn suites(
    ctx: &BridgeContext,
    client: &crate::ado::AdoClient,
    target: &str,
) -> (u16, String) {
    suites_within(ctx, client, target, SUITES_WALK_BUDGET).await
}

/// How long the combined suites walk goes on starting projects. A cold walk
/// of a big organization can outlast the MCP proxy's own limit on a call
/// (mcp.rs, 300 s), which would report the app unreachable; past this the
/// walk answers with what it has read, and the trees it did read stay cached
/// so the next call carries on from there.
const SUITES_WALK_BUDGET: std::time::Duration = std::time::Duration::from_secs(120);

/// `suites` with the time the combined walk may spend starting projects. A
/// project already started is read to the end, so a call can run up to one
/// project's read past the budget.
pub async fn suites_within(
    ctx: &BridgeContext,
    client: &crate::ado::AdoClient,
    target: &str,
    budget: std::time::Duration,
) -> (u16, String) {
    let started = std::time::Instant::now();
    const CAP: usize = 200;
    let query = q(target, "q").map(|s| s.trim().to_lowercase()).unwrap_or_default();
    let refresh = matches!(q(target, "refresh").as_deref(), Some("true") | Some("1"));
    // (the project a tree belongs to, named in a row only while combined; the tree)
    let mut trees: Vec<(Option<String>, SuiteTree)> = vec![];
    let mut coverage: Option<crate::ado::fanout::ProjectCoverage> = None;
    if ctx.all_projects {
        let names = match client.org_project_names(&ctx.org).await {
            Ok(n) => n,
            Err(e) => return (502, format!("Azure DevOps error: {e:?}")),
        };
        let (names, total_projects) = crate::ado::fanout::capped(names);
        // One project at a time: each tree is itself a scan of its plans,
        // several requests at once.
        let mut results = Vec::with_capacity(names.len());
        for name in &names {
            // Out of time: answer with what is read (the coverage note below
            // says how much that is). Always at least one project is read.
            if !results.is_empty() && started.elapsed() >= budget {
                break;
            }
            let result = suite_tree(ctx, client, name, refresh).await;
            let stop = matches!(&result, Err(e) if !matches!(e, crate::ado::AdoError::Forbidden | crate::ado::AdoError::NotFound));
            results.push((name.clone(), result));
            if stop {
                break;
            }
        }
        match crate::ado::fanout::gather(results, total_projects) {
            Ok((read, cov)) => {
                trees.extend(read.into_iter().map(|(name, tree)| (Some(name), tree)));
                coverage = Some(cov);
            }
            Err(e) => return (502, format!("Azure DevOps error: {e:?}")),
        }
    } else {
        match suite_tree(ctx, client, &ctx.project, refresh).await {
            Ok(t) => trees.push((None, t)),
            Err(e) => return (502, format!("Azure DevOps error: {e:?}")),
        }
    }
    let mut rows: Vec<serde_json::Value> = vec![];
    for (project, tree) in &trees {
        for p in tree {
            let plan_hit = query.is_empty() || p.plan.name.to_lowercase().contains(&query);
            for s in &p.suites {
                let hit = plan_hit
                    || s.name.to_lowercase().contains(&query)
                    || s.requirement_id.map(|r| r.to_string() == query).unwrap_or(false);
                if !hit {
                    continue;
                }
                let mut row = serde_json::json!({
                    "plan_id": p.plan.id,
                    "plan_name": p.plan.name,
                    "suite_id": s.id,
                    "suite_name": s.name,
                    "suite_type": s.suite_type,
                    "requirement_id": s.requirement_id,
                    "parent_suite_id": s.parent_id,
                });
                if let Some(project) = project {
                    row["project"] = serde_json::json!(project);
                }
                rows.push(row);
            }
        }
    }
    let total = rows.len();
    rows.truncate(CAP);
    let mut out = serde_json::json!({ "suites": rows, "total": total });
    let mut notes: Vec<String> = vec![];
    if total > CAP {
        notes.push(format!(
            "{CAP} of {total} suites returned - pass q=<plan or suite name> to narrow the list."
        ));
    }
    if let Some(cov) = coverage {
        // Said whenever a project is missing, whether the cap cut it or the
        // sign-in cannot read it: a short list must never look complete.
        if cov.shown < cov.total {
            out["projects_shown"] = serde_json::json!(cov.shown);
            out["projects_total"] = serde_json::json!(cov.total);
            notes.push(format!(
                "Suites from {} of {} projects are listed; the rest are past the {} project limit, not readable with this sign-in, or not read in time (call again to read more of them).",
                cov.shown,
                cov.total,
                crate::ado::fanout::MAX_FANOUT_PROJECTS
            ));
        }
    }
    if !notes.is_empty() {
        out["note"] = serde_json::json!(notes.join(" "));
    }
    (200, out.to_string())
}

/// The cases in one suite, in the import JSON record shape, by way of the
/// suite's test points - the same read the Run Tests tab makes. With
/// `children=true` the endpoint's own `isRecursive` flag brings in every
/// child suite's cases, so a folder reads in one call. Combined, the points
/// are read in the plan's own project: the `project` the caller names, else
/// the one found by `plan_project`.
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
    // Switched off, the project the bridge was given, with no lookup (and a
    // `project` in the request is ignored).
    let project = if ctx.all_projects {
        match q(target, "project").map(|p| p.trim().to_string()).filter(|p| !p.is_empty()) {
            Some(p) => p,
            None => match plan_project(ctx, client, plan).await {
                Ok(p) => p,
                Err(crate::ado::AdoError::NotFound) => {
                    return (
                        404,
                        format!("Plan {plan} was not found in this organization (find one with search_test_suites)."),
                    )
                }
                Err(e) => return (502, format!("Azure DevOps error: {e:?}")),
            },
        }
    } else {
        ctx.project.clone()
    };
    let points = match client
        .get_test_points_in(&ctx.org, &project, plan, suite, &[], children)
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
            if ctx.all_projects {
                out["project"] = serde_json::json!(project);
            }
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

/// The project a work item (a PBI, or a test plan) is in, from the session
/// cache or one organization-level work item read. A work item never changes
/// project, so the answer is kept for `WORK_ITEM_PROJECT_TTL`.
async fn work_item_project_cached(
    ctx: &BridgeContext,
    client: &crate::ado::AdoClient,
    pbi: i32,
) -> Result<String, crate::ado::AdoError> {
    let key = crate::cache::keys::work_item_project(&client.base_url, &ctx.org, pbi);
    if let Some(p) = crate::cache::session_fresh::<String>(&key, crate::cache::keys::WORK_ITEM_PROJECT_TTL) {
        return Ok(p);
    }
    let project = client.work_item_project(&ctx.org, pbi).await?;
    crate::cache::session_put(&key, project.clone());
    Ok(project)
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
    // Combined, the PBI's own project holds its suite and results; the home
    // project is only where this app keeps its files. Switched off, the
    // project the bridge was given, with no lookup.
    let project = if ctx.all_projects {
        match work_item_project_cached(ctx, client, pbi).await {
            Ok(p) => p,
            Err(crate::ado::AdoError::NotFound) => {
                return (
                    404,
                    format!("Work item {pbi} was not found in this organization (find one with search_pbis)."),
                )
            }
            Err(e) => return (502, format!("Azure DevOps error: {e:?}")),
        }
    } else {
        ctx.project.clone()
    };
    let project = project.as_str();
    let mut retried = false;
    let (suite, points) = loop {
        let cached = crate::ado_testplan::cached_suite(&client.base_url, &ctx.org, project, pbi);
        let (suite, from_cache) = match cached {
            Some(s) => (s, true),
            None => {
                // No area path here: the scan only uses it to order plans,
                // and it checks every plan regardless, so "" costs at most
                // a slower hit.
                match client
                    .find_pbi_requirement_suite(&ctx.org, project, pbi, "")
                    .await
                {
                    Ok(Some(s)) => {
                        crate::ado_testplan::remember_suite(&client.base_url, &ctx.org, project, pbi, &s);
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
            .get_test_points(&ctx.org, project, suite.plan_id, suite.suite_id, &[])
            .await
        {
            Ok(p) => break (suite, p),
            // A cached suite that 404s was deleted in Azure DevOps since
            // it was resolved: forget it and scan once from scratch.
            Err(crate::ado::AdoError::NotFound) if from_cache && !retried => {
                crate::ado_testplan::forget_suite(&client.base_url, &ctx.org, project, pbi);
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
                    .get_result_report_info(&ctx.org, project, run, res)
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
    if ctx.all_projects {
        // Every project's PBIs; each hit names its own project.
        return match client.search_pbis_all(&ctx.org, &query, 20).await {
            Ok(hits) => (
                200,
                serde_json::json!({
                    "pbis": hits.iter().map(|h| serde_json::json!({
                        "id": h.id, "title": h.title, "work_item_type": h.work_item_type,
                        "project": h.project
                    })).collect::<Vec<_>>()
                })
                .to_string(),
            ),
            Err(e) => (502, format!("Azure DevOps error: {e:?}")),
        };
    }
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
    if ctx.all_projects {
        // The organization's wikis; each hit names its own project, which
        // `get_wiki_page` takes back as `project`.
        return match client.search_wiki_all(&ctx.org, &query, top).await {
            Ok(hits) => (
                200,
                serde_json::json!({
                    "results": hits.iter().map(|h| serde_json::json!({
                        "file_name": h.file_name,
                        "path": h.path,
                        "wiki_name": h.wiki_name,
                        "wiki_id": h.wiki_id,
                        "project": h.project,
                        "highlights": h.highlights,
                    })).collect::<Vec<_>>()
                })
                .to_string(),
            ),
            Err(e) => (502, format!("Azure DevOps error: {e:?}")),
        };
    }
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
    // While combined the page may be in any project: the hit's (`?project=`),
    // else the one a pasted URL names, else home. Off, the chosen project.
    let project = if ctx.all_projects {
        q(target, "project")
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .or_else(|| {
                crate::ado::endpoints::wiki_url_org_project(&path)
                    .filter(|(org, _)| org.eq_ignore_ascii_case(&ctx.org))
                    .map(|(_, project)| project)
            })
            .unwrap_or_else(|| ctx.project.clone())
    } else {
        ctx.project.clone()
    };
    match client.get_wiki_page(&ctx.org, &project, &wiki_id, &path).await {
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

/// After an answer sent before the request was all read (a body over the cap): half-closes the socket so the
/// caller sees the answer end, then reads and discards what is still coming, at most 1 MiB and 2 s. Closing
/// with unread bytes makes Windows reset the connection, and the caller then reads the reset, not the answer.
async fn drain_unread_request(sock: &mut tokio::net::TcpStream) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let _ = sock.shutdown().await;
    let drain = async {
        let mut sink = [0u8; 8192];
        let mut drained = 0usize;
        while drained < 1024 * 1024 {
            match sock.read(&mut sink).await {
                Ok(0) | Err(_) => break,
                Ok(n) => drained += n,
            }
        }
    };
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), drain).await;
}

/// Bind 127.0.0.1:0, optionally write the handshake file, serve forever on
/// the tokio runtime. Returns (port, token). Requests: tiny HTTP/1.1, one
/// request per connection, 64 KiB body cap (and 1 KiB more for the headers). `handshake` is Some in the real
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
                // 64 KiB for the request plus a kilobyte for its head: the agent
                // headers a proxy now sends must not take room a body had before.
                let mut buf = vec![0u8; 65536 + 1024];
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
                        Parsed::Complete { method, target, token, body, proxy_version, tool, agent } => {
                            break (method, target, token, body, proxy_version, tool, agent)
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
                        drain_unread_request(&mut sock).await;
                        return;
                    }
                };
                let (method, target, tok, body, proxy_version, tool, agent) = parsed;
                let (status, payload) = if tok.as_deref() != Some(state.token.as_str()) {
                    (401, String::new())
                } else {
                    note_proxy_version(proxy_version.as_deref(), &state.version);
                    let ctx = state.ctx.lock().unwrap().clone();
                    let client = match &make_client {
                        Some(f) => f().await,
                        None => None,
                    };
                    // Who is calling is known only after the token check: a
                    // request without the token registers nobody.
                    let caller = agent.caller();
                    let label = crate::autorun::agents::note_call(&caller, &agent, crate::autorun::sessions::now_ms());
                    let (status, payload) =
                        route_timed(&caller, &ctx, client.as_ref(), &method, &target, &body, &state.version).await;
                    if let Some(line) = tool_call_line_for(tool.as_deref(), label.as_deref(), &target, status, &payload) {
                        crate::applog::debug(line);
                    }
                    (status, payload)
                };
                let reason = match status {
                    200 => "OK",
                    202 => "Accepted",
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
        /// The `x-tcm-tool` header: the MCP tool this request was made for
        /// (`mcp::TOOL_HEADER`). None outside a tool call.
        tool: Option<String>,
        /// The `x-tcm-agent*` headers (`mcp::AGENT_HEADER` and its three
        /// companions), each None when absent or unreadable. A bad value is
        /// ignored, never a reason to refuse the request.
        agent: crate::autorun::agents::AgentHeaders,
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
    let mut tool = None;
    let mut agent = crate::autorun::agents::AgentHeaders::default();
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
        if k.eq_ignore_ascii_case(crate::mcp::TOOL_HEADER) {
            tool = Some(v.to_string());
        }
        if k.eq_ignore_ascii_case(crate::mcp::AGENT_HEADER) {
            agent.id = Some(v.to_string());
        }
        if k.eq_ignore_ascii_case(crate::mcp::AGENT_PID_HEADER) {
            // Not a number, or 0 (no process has it): left out, the request
            // is still served.
            agent.pid = v.parse().ok().filter(|p| *p != 0);
        }
        if k.eq_ignore_ascii_case(crate::mcp::AGENT_CLIENT_HEADER) {
            agent.client = Some(v.to_string());
        }
        if k.eq_ignore_ascii_case(crate::mcp::AGENT_NAME_HEADER) {
            agent.name = Some(v.to_string());
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
        tool,
        agent,
    }
}

/// The app-log line for one MCP tool call, or None for a request that is
/// not one to log. The proxy names the tool (`mcp::TOOL_HEADER`) on every
/// request it makes for a call: first the `/tools` read that decides
/// whether the tool is offered, then - unless that refused it - the tool's
/// own route. So a call that reaches the app is logged once: "refused" on
/// the `/tools` read when this very answer switches the tool off (decided
/// as the proxy decides it, `mcp::tool_policy_from`), otherwise "ok" or the
/// HTTP status of the route's answer. Only the tool's name and that outcome
/// - never an argument, the answer's text, an id or a path - and only for a
/// name this server offers.
///
/// What is NOT logged is what the proxy refuses before it asks the app
/// anything, because the app never sees those calls and there is no
/// channel for them: an argument-shape refusal (a mix of arguments, a
/// `get_guide` part sent with another topic, a quirk or cookie shape), a
/// `get_guide` topic the proxy refuses, and a call to a retired tool name.
pub fn tool_call_line(tool: Option<&str>, target: &str, status: u16, payload: &str) -> Option<String> {
    tool_call_line_for(tool, None, target, status, payload)
}

/// `tool_call_line` for a caller with a label: "AI tool {tool} ({label}):
/// {outcome}". With no label it is exactly `tool_call_line`'s line. The label
/// is the registry's ("Assistant 2"), never an id or a pid.
pub fn tool_call_line_for(
    tool: Option<&str>,
    label: Option<&str>,
    target: &str,
    status: u16,
    payload: &str,
) -> Option<String> {
    static NAMES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    let tool = tool?;
    if !NAMES.get_or_init(crate::mcp::all_tool_names).iter().any(|t| t == tool) {
        return None;
    }
    let path = target.split_once('?').map_or(target, |(p, _)| p);
    let outcome = if path == "/tools" {
        let (off, _) = crate::mcp::tool_policy_from(Ok((status, payload.to_string())));
        if !off.iter().any(|o| o == tool) {
            return None;
        }
        "refused".to_string()
    } else if status < 400 {
        "ok".to_string()
    } else {
        format!("tool error, HTTP {status}")
    };
    Some(match label {
        Some(label) => format!("AI tool {tool} ({label}): {outcome}"),
        None => format!("AI tool {tool}: {outcome}"),
    })
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
