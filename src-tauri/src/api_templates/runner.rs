//! Running an API template from inside a signed-in page - design doc "API
//! templates" §5 (the runner) and §9 (errors).
//!
//! The requests have to carry the application's real session cookies and
//! its anti-forgery token, and the only thing that has those is a browser
//! that signed in the way a person does. So the runner borrows Auto Run's
//! machinery: a fresh browser (`Browsers`), the project's sign-in recipe
//! and the chosen account (`signin::sign_in`), then a navigation to the
//! template's anti-forgery page to read the token, and finally each step as
//! a `fetch` made BY the page (`FETCH_FN`), so the browser attaches the
//! cookies itself and nothing here ever sees one.
//!
//! A single run keeps its browser signed in afterwards (`held`), and the
//! next run as the same account within two minutes reuses it, skipping the
//! launch and the sign-in.
//!
//! The token is the one secret that does pass through here: it is read
//! from the page and handed straight back to it as `FETCH_FN`'s second
//! argument. It is never formatted into a string - not a report, not a
//! step detail, not an activity record, not a log line.
//!
//! This module decides nothing about saving: the caller (the bridge) saves
//! a proven template and appends run history from the `RunReport`.

use super::exec::{
    self, build_request, capture, check_expect, excerpt, parse_capture_path, scrub_tokens, scrub_value, Body,
};
use super::cookies::{case_blind_cookies, in_cookie_case, jar_cookies, lost_by_adapting};
use super::flow::{check_stage_ref, Flow};
use super::flow_store;
use super::held::{self, HeldBrowser, HeldEntry, Keeps, Taken};
use super::{check, check_values, is_safe_relative_path, ApiTemplate, Effect, Method, Step};
use crate::activity_log::{self, Kind};
use crate::applog;
use crate::autorun::accounts::Account;
use crate::autorun::lease;
use crate::autorun::nav::path_of;
use crate::autorun::recipe::{origin_of, SignInRecipe};
use crate::autorun::sessions::forget_session;
use crate::autorun::signin::{prepare, sign_in, SignInOutcome};
use crate::browser::actions::{execute_in, Action, Policy};
use crate::browser::cdp::{CdpError, Driver};
use crate::browser::page::{call_value, call_value_within, document, eval_value, Handle};
use crate::browser::timing::Timing;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// How long one step may take - `FETCH_FN` aborts its own request at the
/// same point, and every DevTools call of the step is bounded by it too.
pub const STEP_LIMIT: Duration = Duration::from_secs(30);
/// The same for a step that uploads test files: 25 MB on a slow link takes
/// far longer than 30 s, and a step Rust gave up on while the page's
/// request was still going could still save - and a re-run save twice.
pub const UPLOAD_STEP_LIMIT: Duration = Duration::from_secs(120);
/// How much longer than the page's own abort the DevTools call waits, so
/// a request that runs out of time is reported by the page that sent it
/// (and is over), never by Rust while the page is still sending it.
pub const FETCH_GRACE: Duration = Duration::from_secs(5);

/// How long `step` may take: `UPLOAD_STEP_LIMIT` when it sends files,
/// `STEP_LIMIT` otherwise.
pub fn step_limit(step: &Step) -> Duration {
    if step.files.is_empty() {
        STEP_LIMIT
    } else {
        UPLOAD_STEP_LIMIT
    }
}

/// The sentence for a step that ran out of `limit`.
fn step_too_long(limit: Duration) -> String {
    format!("the step took longer than {} seconds", limit.as_secs())
}
/// How long a whole run may take once its browser is open.
pub const RUN_LIMIT: Duration = Duration::from_secs(180);

const RUN_TOO_LONG: &str = "the run took longer than 3 minutes";
/// Names for the parts of a run that are not a template step, as a
/// `StepReport` and `RunReport::failed` show them.
const SIGN_IN: &str = "Sign in";
const TOKEN_PAGE: &str = "Anti-forgery token";
const BROWSER: &str = "Browser";
/// How a run that reused a held browser's sign-in says it signed in.
pub const REUSED: &str = "Signed in earlier, reused";

/// Reads the anti-forgery token Razor Pages writes into every form page.
/// Called on the document; returns the token or `null`.
pub const TOKEN_FN: &str = r#"function () { const i = this.querySelector('input[name="__RequestVerificationToken"]'); return i ? i.value : null; }"#;

/// Sends one built request (`exec::BuiltRequest`, serialized) from the page
/// itself, so the browser attaches the session cookies; `token` goes only
/// into the `RequestVerificationToken` header and is never returned. The
/// request is aborted after `limitMs` (the step's `step_limit`; 30 s when
/// not given), and at most 64 KB of the body is read
/// back. A request that did not complete comes back as `{ error }` rather
/// than a throw, so the runner can tell a timeout from anything else. A
/// form's files (`exec::FormFile`) are appended after its text fields, each
/// a `Blob` of its own bytes, type and name.
pub const FETCH_FN: &str = r#"async function (req, token, limitMs) {
  const headers = { "RequestVerificationToken": token, "Accept": "application/json" };
  let body;
  if (req.body && req.body.kind === "json") {
    headers["Content-Type"] = "application/json";
    body = JSON.stringify(req.body.value);
  } else if (req.body && req.body.kind === "form") {
    body = new FormData();
    for (const [k, v] of Object.entries(req.body.fields)) body.append(k, v);
    for (const f of (req.body.files || [])) {
      body.append(f.field, new Blob([Uint8Array.from(atob(f.base64), c => c.charCodeAt(0))], { type: f.contentType }), f.name);
    }
  }
  const ctrl = new AbortController();
  const timer = setTimeout(() => ctrl.abort(), limitMs || 30000);
  try {
    const r = await fetch(req.url, { method: req.method, credentials: "same-origin", headers, body, signal: ctrl.signal });
    const text = (await r.text()).slice(0, 65536);
    return { status: r.status, contentType: r.headers.get("content-type"), finalUrl: r.url, redirected: r.redirected, text };
  } catch (e) {
    return { error: e && e.name === "AbortError" ? "timeout" : String(e) };
  } finally {
    clearTimeout(timer);
  }
}"#;

/// Proving a draft (saved by the caller only on success; replacing an
/// existing id needs `replace` and a `why`), running a saved template, or
/// Clean up test-made drafts running a saved delete template
/// (`autorun::cleanup`), the one way a delete template ever runs.
#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    Prove { replace: bool, why: Option<String> },
    Run,
    Cleanup,
}

impl Mode {
    /// `"prove"` / `"run"` / `"cleanup"` - how the activity log and the app
    /// log name it.
    pub fn label(&self) -> &'static str {
        match self {
            Mode::Prove { .. } => "prove",
            Mode::Run => "run",
            Mode::Cleanup => "cleanup",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunRequest {
    pub org: String,
    pub project: String,
    /// An Auto Run account key.
    pub account: String,
    pub values: serde_json::Map<String, Value>,
    pub mode: Mode,
    pub template: ApiTemplate,
}

/// One step's result. Also used for the parts of a run that come before
/// the steps - `"Sign in"`, `"Anti-forgery token"`, `"Browser"` - when one
/// of those is what stopped it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct StepReport {
    pub name: String,
    /// The step's `handler` query value, if it has one.
    pub handler: Option<String>,
    pub status: Option<u16>,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct RunReport {
    pub ok: bool,
    /// The template's id.
    pub template: String,
    /// The template's declared outputs - only on success.
    // See `RunRecord::outputs` on why the value side is `unknown`.
    #[specta(type = BTreeMap<String, specta_typescript::Unknown>)]
    pub outputs: BTreeMap<String, Value>,
    /// Everything captured before the run stopped: what a failed run
    /// already did in the application, which nothing undoes.
    #[specta(type = BTreeMap<String, specta_typescript::Unknown>)]
    pub created: BTreeMap<String, Value>,
    pub steps: Vec<StepReport>,
    /// The name of whatever stopped the run (a step's name, or one of the
    /// `StepReport` names for the parts before the steps).
    pub failed: Option<String>,
}

/// A JSON value as a sentence shows it: a string without its quotes.
fn plain(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

impl RunReport {
    /// The run in one sentence, for the assistant and the run history:
    /// `cycleId 274 created; failed at Evaluation rules (SaveEvalRulesProgress):
    /// expected status 200, got 400 - the response began: ...`. Built only
    /// from step details, which are the runner's own sentences: raw browser
    /// errors go to the app log instead, and a sign-in detail has its
    /// addresses' hosts taken out (`Ctx::could_not_sign_in`). The one text
    /// here the runner did not write is the response excerpt, which is the
    /// application's own body with every anti-forgery token taken out
    /// (`shown`).
    pub fn message(&self) -> String {
        let captured = if self.created.is_empty() {
            "nothing had been captured yet".to_string()
        } else {
            let list: Vec<String> = self.created.iter().map(|(k, v)| format!("{k} {}", plain(v))).collect();
            format!("{} created", list.join(", "))
        };
        if self.ok {
            return format!("every step passed ({} steps); {captured}", self.steps.len());
        }
        let stopped = self.steps.iter().rev().find(|s| !s.ok);
        let name = self.failed.clone().unwrap_or_else(|| "an unknown point".to_string());
        let handler = stopped.and_then(|s| s.handler.clone()).map(|h| format!(" ({h})")).unwrap_or_default();
        let detail = stopped.map(|s| s.detail.clone()).unwrap_or_default();
        format!("{captured}; failed at {name}{handler}: {detail}")
    }
}

/// Everything preventing this run, all together, before anything starts:
/// the template's own checks, the values against its params, a safe
/// anti-forgery page, the recipe and the account, a template's flow stage
/// (`stage_problems`) - and, proving over an existing id, `replace: true`
/// with a non-blank `why`. Running a delete template is refused here: only
/// Clean up test-made drafts runs one, in `Mode::Cleanup`, which in turn
/// runs nothing but a delete template.
pub fn preflight(root: &Path, req: &RunRequest, existing: Option<&ApiTemplate>) -> Result<(), Vec<String>> {
    let t = &req.template;
    // A saved template carries the app's `proven` block, which `check`
    // refuses in a DRAFT; running a saved one is exactly when it is there.
    let mut problems = match req.mode {
        Mode::Run | Mode::Cleanup => check(&ApiTemplate { proven: None, ..t.clone() }),
        Mode::Prove { .. } => check(t),
    };
    if matches!(req.mode, Mode::Run) && t.effect == Effect::Delete {
        problems.push(deletes_refusal(&t.id));
    }
    // `check` above already refuses a delete template of any other shape
    // than one `id` (`DELETE_SHAPE`), in every mode: a draft at prove, and
    // a saved file - one written before the rule - at cleanup.
    if matches!(req.mode, Mode::Cleanup) && t.effect != Effect::Delete {
        problems.push(format!("template {} does not delete, so Clean up does not run it", t.id));
    }
    problems.extend(check_values(t, &req.values));
    if !is_safe_relative_path(&t.antiforgery.page) {
        problems.push(format!(
            "antiforgery page '{}' is not a safe relative path on this origin",
            t.antiforgery.page
        ));
    }
    if let Err(e) = prepare(root, &req.org, &req.project, &req.account) {
        problems.push(e);
    }
    problems.extend(file_problems(root, req));
    let (stage, missing_subject) = stage_problems(root, req);
    // A flow template left without its record id gets the flow's own
    // sentence, which says why the value is needed - not also the general
    // one about an optional param with no default.
    if let Some(subject) = missing_subject {
        let general = format!("param '{subject}' is optional but has no default");
        problems.retain(|p| !p.starts_with(&general));
    }
    problems.extend(stage);
    if let (Some(_), Mode::Prove { replace, why }) = (existing, &req.mode) {
        let has_why = why.as_deref().is_some_and(|w| !w.trim().is_empty());
        if !(*replace && has_why) {
            problems.push(format!(
                "a template called \"{}\" already exists - send replace: true and a why to change it",
                t.id
            ));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Every test file the template's steps upload that this project's Test
/// files does not have, or that is over the cap - one problem per file, the
/// first step that uploads it named.
fn file_problems(root: &Path, req: &RunRequest) -> Vec<String> {
    let folder = crate::test_files::folder(root, &req.org, &req.project);
    let mut seen = std::collections::HashSet::new();
    let mut problems = Vec::new();
    for step in &req.template.steps {
        for name in step.files.values() {
            // A name that is not a test file name is `check`'s problem,
            // already reported; this one is only about the file itself.
            if !seen.insert(name.as_str()) || !crate::test_files::valid_test_file_name(name) {
                continue;
            }
            let who = format!("the step \"{}\"", step.name);
            if let Err(e) = crate::test_files::check_for_run(&folder, name, &who) {
                problems.push(e);
            }
        }
    }
    problems
}

/// The saved flow a template's `stage` names, or `None` when it is not
/// saved - or saved but no longer readable, which is logged and otherwise
/// treated the same: the template is refused with "no longer saved", never
/// with a 500 (design doc "API template flows", Review Focus 3).
pub fn stage_flow(root: &Path, org: &str, project: &str, t: &ApiTemplate) -> Option<Flow> {
    let r = t.stage.as_ref()?;
    match flow_store::load(root, org, project, &r.flow) {
        Ok(found) => found,
        Err(e) => {
            applog::warn(format!("api template {}: its flow {} could not be read: {e}", t.id, r.flow));
            None
        }
    }
}

/// A template on a flow: its stage must still be in a saved flow, it must
/// have the shape that stage needs, and - unless it creates the record -
/// the call must carry the record's id. The subject param is not forced
/// `required`, so a missing value is caught here, by name, before any
/// database is asked - and its name comes back beside the problems.
fn stage_problems(root: &Path, req: &RunRequest) -> (Vec<String>, Option<String>) {
    let t = &req.template;
    let Some(r) = &t.stage else { return (Vec::new(), None) };
    let f = stage_flow(root, &req.org, &req.project, t);
    let problems = check_stage_ref(t, f.as_ref());
    if !problems.is_empty() {
        return (problems, None);
    }
    let Some(f) = f else { return (problems, None) };
    let creates = f.stages.iter().any(|s| s.id == r.id && s.creates);
    let name = &f.subject.name;
    // A required param that is missing already has check_values' sentence.
    let required = t.params.iter().any(|p| &p.name == name && p.required);
    if !creates && !required && !req.values.contains_key(name) {
        return (
            vec![format!("this template belongs to flow {}, so it needs \"{name}\" in values", f.id)],
            Some(name.clone()),
        );
    }
    (Vec::new(), None)
}

/// The sentence a run of a delete template is refused with, outside Clean
/// up test-made drafts.
pub fn deletes_refusal(id: &str) -> String {
    format!("template {id} deletes, and only Clean up test-made drafts runs a delete template")
}

/// Said when a prove, a run or a fixture run arrives while another one
/// holds the process-wide slot (`claim`).
pub const API_TEMPLATE_BUSY: &str = "another API template is running - wait for it to finish";

/// Whether a template run is going, process-wide. Only `claim` sets it;
/// only dropping the claim clears it - so a run that panics or returns
/// early still frees the slot as its stack unwinds.
static RUNNING: AtomicBool = AtomicBool::new(false);

/// The one-at-a-time slot, held for the length of a run.
pub struct RunClaim(());

impl Drop for RunClaim {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::SeqCst);
    }
}

/// The slot, or `None` while another run holds it.
pub fn claim() -> Option<RunClaim> {
    RUNNING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).ok().map(|_| RunClaim(()))
}

/// Whether a template run holds the slot right now. Only looks: switching
/// the environment asks this first for its refusal, then takes the slot
/// itself (`claim`) for the write, so a run never changes environment
/// midway.
pub fn is_running() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

/// What the run has done so far. Lives OUTSIDE the timed future, so a run
/// cut off by `RUN_LIMIT` still reports every step it finished, what it
/// captured, and where it was.
struct Progress {
    steps: Vec<StepReport>,
    created: BTreeMap<String, Value>,
    failed: Option<String>,
    /// What is happening now, and its handler: what a timeout names.
    phase: String,
    phase_handler: Option<String>,
    /// Template steps attempted - the summary line's count.
    sent: usize,
    finished: bool,
    /// How each sign-in of this run went, for the token page's activity
    /// record - see `signed_in`.
    sign_ins: Vec<Value>,
    /// A reused browser's session had ended by the run's first request
    /// (`Ctx::reused`): the run stopped there, to start again signed in
    /// afresh.
    dropped: bool,
    /// The run stopped because its session had ended: a request or the
    /// token page was sent to another page. Its browser is never kept.
    session_ended: bool,
}

impl Progress {
    fn new() -> Self {
        Progress {
            steps: vec![],
            created: BTreeMap::new(),
            failed: None,
            phase: BROWSER.to_string(),
            phase_handler: None,
            sent: 0,
            finished: false,
            sign_ins: vec![],
            dropped: false,
            session_ended: false,
        }
    }

    /// Notes a sign-in an earlier run made in this browser, in the shape
    /// `signed_in` notes one.
    fn reused(&mut self, id: &str, account: &str) {
        applog::info(format!("api template {id}: signed in as \"{account}\" earlier, in a kept browser, reused"));
        self.sign_ins.push(json!({ "via": REUSED, "appeared": [] }));
    }

    /// Notes how a sign-in went - from a saved session or through the
    /// recipe, and which optional recipe steps' elements showed up - and
    /// says so in the app log. PeoplesHR lets an account be signed in in
    /// one place at a time: a recipe sign-in that met "Continue here" has
    /// just logged that account out wherever else it was, and a session
    /// someone else takes over ends this run's with an empty 400.
    fn signed_in(&mut self, id: &str, account: &str, out: &SignInOutcome) {
        let (via, how) = if out.used_saved_session {
            ("saved session", "from a saved session")
        } else {
            ("sign-in recipe", "through the sign-in recipe")
        };
        let appeared =
            if out.appeared.is_empty() { String::new() } else { format!(", and {} appeared", out.appeared.join(", ")) };
        applog::info(format!("api template {id}: signed in as \"{account}\" {how}{appeared}"));
        self.sign_ins.push(json!({ "via": via, "appeared": out.appeared }));
    }

    fn at(&mut self, phase: &str, handler: Option<String>) {
        self.phase = phase.to_string();
        self.phase_handler = handler;
    }

    /// Stops the run at the current phase.
    fn fail(&mut self, status: Option<u16>, detail: impl Into<String>) {
        self.steps.push(StepReport {
            name: self.phase.clone(),
            handler: self.phase_handler.clone(),
            status,
            ok: false,
            detail: detail.into(),
        });
        self.failed = Some(self.phase.clone());
    }
}

/// Same page, as far as a stale session goes: the paths match ignoring
/// case, query, fragment and a trailing `/`.
fn same_path(a: &str, b: &str) -> bool {
    fn norm(u: &str) -> String {
        let p = path_of(u).to_ascii_lowercase();
        let trimmed = p.trim_end_matches('/');
        if trimmed.is_empty() {
            "/".to_string()
        } else {
            trimmed.to_string()
        }
    }
    norm(a) == norm(b)
}

fn method_name(m: Method) -> &'static str {
    match m {
        Method::Get => "GET",
        Method::Post => "POST",
    }
}

/// The request body as the activity log keeps it (then excerpted). A file
/// is named with its size (`FormFile::describe`), never its bytes.
pub fn body_text(b: &Body) -> String {
    match b {
        Body::None => String::new(),
        Body::Json { value } => value.to_string(),
        Body::Form { fields, files } => {
            let mut shown = fields.clone();
            for f in files {
                shown.insert(f.field.clone(), f.describe());
            }
            serde_json::to_string(&shown).unwrap_or_default()
        }
    }
}

/// The bytes of every test file `step` uploads, by name - read here, at
/// the step, so a run holds at most one step's files at a time.
fn step_files(ctx: &Ctx<'_>, step: &Step) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut out = BTreeMap::new();
    if step.files.is_empty() {
        return Ok(out);
    }
    let folder = crate::test_files::folder(ctx.root, &ctx.req.org, &ctx.req.project);
    let who = format!("the step \"{}\"", step.name);
    for name in step.files.values() {
        if !out.contains_key(name) {
            out.insert(name.clone(), crate::test_files::read_for_run(&folder, name, &who)?);
        }
    }
    Ok(out)
}

/// What a step that sent files says about them: `sent "a.pdf" (1.2 KB)`.
fn sent_files(body: &Body) -> Option<String> {
    let Body::Form { files, .. } = body else { return None };
    if files.is_empty() {
        return None;
    }
    let each: Vec<String> =
        files.iter().map(|f| format!("\"{}\" ({})", f.name, crate::test_files::human_size(f.size))).collect();
    Some(format!("sent {}", each.join(", ")))
}

/// What one run needs from its request, resolved once.
struct Ctx<'a> {
    root: &'a Path,
    req: &'a RunRequest,
    timing: &'a Timing,
    recipe: SignInRecipe,
    account: Account,
    origin: String,
    /// The browser was signed in by an earlier run and kept (`held`).
    reused: bool,
    /// The page the browser is on when the run starts, if known.
    on_page: Option<String>,
}

impl Ctx<'_> {
    fn id(&self) -> &str {
        &self.req.template.id
    }

    /// A failed sign-in, as the person reads it. `SignInOutcome.detail` is
    /// Auto Run's wording, which can carry a raw browser error or a full
    /// address: it goes to the app log as it is (it is already
    /// password-redacted), and the sentence gets either a fixed one - when
    /// the browser itself failed - or the detail with every address's
    /// scheme and host taken out.
    fn could_not_sign_in(&self, out: &SignInOutcome) -> String {
        applog::warn(format!("api template {}: signing in as \"{}\": {}", self.id(), self.req.account, out.detail));
        if out.harness {
            return format!(
                "could not sign in as \"{}\": the browser stopped answering while signing in - see Settings, Logs",
                self.req.account
            );
        }
        let mut detail = out.detail.clone();
        for origin in self.recipe.origins() {
            detail = detail.replace(&origin, "");
        }
        format!(
            "could not sign in as \"{}\": {} - sign that account in once from Auto Run, then try again",
            self.req.account,
            without_hosts(&detail)
        )
    }
}

/// `text` with the scheme and host of every http(s) address taken out, so
/// `https://Host:8443/hr/ did not load` reads `/hr/ did not load` - the
/// backstop behind stripping the recipe's own origins, for an address
/// written in another case or on some other host.
fn without_hosts(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let lower = rest.to_ascii_lowercase();
        let at = ["https://", "http://"].iter().filter_map(|s| lower.find(s).map(|i| (i, s.len()))).min();
        let Some((i, scheme_len)) = at else {
            out.push_str(rest);
            return out;
        };
        out.push_str(&rest[..i]);
        let after = &rest[i + scheme_len..];
        let host_len = after
            .find(|c: char| c == '/' || c == '?' || c == '#' || c.is_whitespace() || c == '"' || c == '\'')
            .unwrap_or(after.len());
        rest = &after[host_len..];
    }
}

/// Runs `req.template` signed in as its account and says what happened.
/// The browser a run as the same account kept within the last `HELD_IDLE`
/// is reused, sign-in and all (`held`); otherwise a fresh one opens from
/// `browsers` and signs in. Either way it is kept afterwards for the next
/// run, unless the run timed out, its sign-in failed or the browser
/// stopped answering: then it is closed. Saves nothing: see the module
/// comment.
pub async fn run_template<B: Keeps>(browsers: &mut B, root: &Path, req: &RunRequest, timing: &Timing) -> RunReport {
    run_template_within(browsers, root, req, timing, RUN_LIMIT, &RETRY_PAUSES).await
}

/// `run_template` with the run limit and retry pauses given rather than
/// `RUN_LIMIT` and `RETRY_PAUSES` - the only way a test reaches the timeout
/// path, or every retry, without waiting for them. The sentence a timeout
/// reports still says three minutes.
///
/// A kept browser whose process has ended is closed and never reused. A
/// reused browser whose session has ended by the run's first request (an
/// empty 400, a redirect, or the token page sent elsewhere) is closed, and
/// the run starts again once in a fresh browser that signs in, within the
/// same `limit`. A fresh browser that meets the same fails as it always
/// has.
pub async fn run_template_within<B: Keeps>(
    browsers: &mut B,
    root: &Path,
    req: &RunRequest,
    timing: &Timing,
    limit: Duration,
    retry_pauses: &[Duration],
) -> RunReport {
    // Held for the whole run, the browser's keep or close included, and
    // let go on every path out: an end, an error, a timeout, a panic, or
    // this future dropped.
    let (env, _lease) = match template_lease(root, &req.account, timing).await {
        Ok(x) => x,
        Err(why) => return stopped_before_sign_in(req, false, why),
    };
    let key = req.account.as_str();
    // One deadline for the run, from when its first browser is ready: a
    // restart after a dropped session shares it, so a run never takes
    // longer than the three minutes its timeout sentence says.
    let mut deadline = None;
    let fingerprint = prepare(root, &req.org, &req.project, key).ok().map(|(r, a)| held::fingerprint(&r, &a));
    if let Some(fingerprint) = fingerprint {
        match held::take::<B::Kept>(&env, key, fingerprint) {
            Taken::Reuse(entry) => match browsers.adopt(entry.driver) {
                Err(dead) => {
                    applog::info(format!("held browser: the browser kept for {key} had ended - opening a new one"));
                    dead.close();
                }
                Ok(mut d) => {
                    let until = Instant::now() + limit;
                    deadline = Some(until);
                    let reuse = Reuse { page: entry.page };
                    let left = until.saturating_duration_since(Instant::now());
                    let (progress, timed_out) =
                        in_session(&mut d, root, req, timing, &entry.session, Some(reuse), left, retry_pauses).await;
                    if !progress.dropped {
                        return keep_or_close(browsers, d, &env, req, entry.session, progress, timed_out).await;
                    }
                    applog::info(format!(
                        "held browser: the session kept for {key} had ended - signing in afresh in a new browser"
                    ));
                    browsers.close(d).await;
                }
            },
            Taken::Close(entry) => entry.driver.close(),
            Taken::Nothing => {}
        }
    }
    let mut d = match browsers.open().await {
        Ok(d) => d,
        Err(why) => return stopped_before_sign_in(req, true, why),
    };
    let deadline = deadline.unwrap_or_else(|| Instant::now() + limit);
    // A failed sign-in keeps nothing.
    let left = deadline.saturating_duration_since(Instant::now());
    let session = match open_session(&mut d, root, req, timing, left).await {
        Ok(session) => session,
        Err(report) => {
            browsers.close(d).await;
            return report;
        }
    };
    let left = deadline.saturating_duration_since(Instant::now());
    let (progress, timed_out) = in_session(&mut d, root, req, timing, &session, None, left, retry_pauses).await;
    keep_or_close(browsers, d, &env, req, session, progress, timed_out).await
}

/// What a run in a kept browser knows from the run that kept it.
struct Reuse {
    /// The page the browser was left on.
    page: Option<String>,
}

/// How long a kept browser may take to say where it is, after a run.
const WHERE_LIMIT: Duration = Duration::from_secs(5);

/// The report of a run that has ended in `d`, signed in as `session`, and
/// what becomes of the browser: kept for the next run as the same account,
/// unless the run timed out, signed in again and failed, found its session
/// ended (a redirect away from a request or the token page), or the
/// browser no longer answers - then it is closed.
async fn keep_or_close<B: Keeps>(
    browsers: &mut B,
    mut d: B::D,
    env: &str,
    req: &RunRequest,
    session: Session,
    progress: Progress,
    timed_out: bool,
) -> RunReport {
    let failed_sign_in = progress.failed.as_deref() == Some(SIGN_IN);
    let session_ended = progress.session_ended;
    let report = finish(req, progress);
    if timed_out || failed_sign_in || session_ended {
        browsers.close(d).await;
        return report;
    }
    let Some(page) = where_now(&mut d).await else {
        browsers.close(d).await;
        return report;
    };
    match browsers.keep(d) {
        Err(d) => browsers.close(d).await,
        Ok(kept) => {
            let entry = HeldEntry {
                driver: kept,
                fingerprint: session.fingerprint(),
                // Read under this run's Template lease, which never moves
                // it: anything else taking the account from here on does.
                generation: lease::generation(env, &req.account),
                session,
                page: Some(page),
            };
            held::keep(env, &req.account, entry);
        }
    }
    report
}

/// The path of the page `d` is on, or `None` when it does not say within
/// `WHERE_LIMIT` - a browser that no longer answers, never kept.
async fn where_now<D: Driver>(d: &mut D) -> Option<String> {
    d.set_deadline(Some(Instant::now() + WHERE_LIMIT));
    let href = eval_value(d, "location.href").await;
    d.set_deadline(None);
    match href {
        Ok(Value::String(h)) if !h.is_empty() => Some(path_of(&h)),
        Ok(_) => None,
        Err(e) => {
            applog::warn(format!("held browser: the browser did not say where it was after the run: {e}"));
            None
        }
    }
}

/// The report of a run that has stopped or finished, and its app-log line.
fn finish(req: &RunRequest, progress: Progress) -> RunReport {
    let t = &req.template;
    let ok = progress.finished && progress.failed.is_none();
    let outputs = if ok {
        t.outputs.iter().filter_map(|n| progress.created.get(n).map(|v| (n.clone(), v.clone()))).collect()
    } else {
        BTreeMap::new()
    };
    let verdict = match (&progress.failed, ok) {
        (_, true) => "ok".to_string(),
        (Some(at), false) => format!("failed at {at}"),
        (None, false) => "failed".to_string(),
    };
    applog::info(format!("api template {}: {} {verdict}, {} steps", t.id, req.mode.label(), progress.sent));
    RunReport { ok, template: t.id.clone(), outputs, created: progress.created, steps: progress.steps, failed: progress.failed }
}

/// The account's lease, in the active environment, before any browser
/// opens (`autorun::lease`): an unattended case or the Auto Run browser
/// signed in as it is waited for, up to `timing`'s lease wait, and then the
/// run is refused with the sentence that says who had it. A fixture takes
/// it once for all its steps.
///
/// A browser a single run kept signed in as the account (`held`) gives way
/// first, closed before the caller opens its own: a second sign-in would
/// end its session anyway, and one browser process per account is the
/// most there should be.
pub(crate) async fn account_lease(root: &Path, account: &str, timing: &Timing) -> Result<lease::Lease, String> {
    let (env, l) = template_lease(root, account, timing).await?;
    let key = account.to_string();
    // Off the async threads: ending a browser process waits for it to go.
    let _ = tauri::async_runtime::spawn_blocking(move || held::give_way(&env, &key)).await;
    Ok(l)
}

/// `account_lease`, with the id of the active environment it was taken in.
async fn template_lease(root: &Path, account: &str, timing: &Timing) -> Result<(String, lease::Lease), String> {
    let env = crate::environments::active_id(root)?;
    let l = lease::acquire(&env, account, lease::Holder::Template, timing.lease_wait()).await?;
    Ok((env, l))
}

/// The report of `req`'s template stopped before its browser signed in:
/// at `Sign in` (the lease was refused) or, with `browser`, at `Browser`
/// (the browser did not open). What a fixture's first step says when it
/// never got that far.
pub(crate) fn stopped_before_sign_in(req: &RunRequest, browser: bool, why: String) -> RunReport {
    let mut progress = Progress::new();
    if browser {
        progress.fail(None, format!("the browser did not open: {why}"));
    } else {
        progress.at(SIGN_IN, None);
        progress.fail(None, why);
    }
    finish(req, progress)
}

/// What one sign-in leaves for every template run after it in the same
/// browser: the recipe and account it signed in with, the origin, and how
/// the sign-in went (each template's token-page record says so). Kept with
/// its browser between runs by `held`.
pub struct Session {
    recipe: SignInRecipe,
    account: Account,
    origin: String,
    sign_ins: Vec<Value>,
}

impl Session {
    /// What a sign-in as `account` with `recipe` at `origin` left, with the
    /// records of how it went.
    pub fn new(recipe: SignInRecipe, account: Account, origin: String, sign_ins: Vec<Value>) -> Self {
        Session { recipe, account, origin, sign_ins }
    }

    fn ctx<'a>(&self, root: &'a Path, req: &'a RunRequest, timing: &'a Timing) -> Ctx<'a> {
        Ctx {
            root,
            req,
            timing,
            recipe: self.recipe.clone(),
            account: self.account.clone(),
            origin: self.origin.clone(),
            reused: false,
            on_page: None,
        }
    }

    /// How a browser signed in this way is fingerprinted (`held::fingerprint`).
    fn fingerprint(&self) -> u64 {
        held::fingerprint(&self.recipe, &self.account)
    }
}

/// Signs in once, in a browser already open, as `req`'s account: for a
/// single run, and for a fixture whose templates then each run in that
/// same browser (`run_in_session`). On failure, the report of `req`'s
/// template stopped at `Sign in`. Bounded by `limit`, like a run.
pub(crate) async fn open_session<D: Driver>(
    d: &mut D,
    root: &Path,
    req: &RunRequest,
    timing: &Timing,
    limit: Duration,
) -> Result<Session, RunReport> {
    let mut progress = Progress::new();
    let timed = tokio::time::timeout(limit, sign_in_once(d, root, req, timing, &mut progress)).await;
    d.set_deadline(None);
    match timed {
        Ok(Some(session)) => Ok(session),
        Ok(None) => Err(finish(req, progress)),
        Err(_) => {
            if progress.failed.is_none() {
                progress.fail(None, RUN_TOO_LONG);
            }
            Err(finish(req, progress))
        }
    }
}

/// Runs `req.template` in a browser `open_session` signed in: its
/// anti-forgery page, its token, then its steps. `limit` bounds this one
/// template, not whatever runs around it.
pub(crate) async fn run_in_session<D: Driver>(
    d: &mut D,
    root: &Path,
    req: &RunRequest,
    timing: &Timing,
    session: &Session,
    limit: Duration,
    retry_pauses: &[Duration],
) -> RunReport {
    let (progress, _) = in_session(d, root, req, timing, session, None, limit, retry_pauses).await;
    finish(req, progress)
}

/// `run_in_session`'s run, before its report: what it did, and whether it
/// ran out of `limit`. With `reuse`, the session is one an earlier run
/// signed in and kept, and the run stops at a session that has ended by
/// its first request (`Progress::dropped`).
#[allow(clippy::too_many_arguments)]
async fn in_session<D: Driver>(
    d: &mut D,
    root: &Path,
    req: &RunRequest,
    timing: &Timing,
    session: &Session,
    reuse: Option<Reuse>,
    limit: Duration,
    retry_pauses: &[Duration],
) -> (Progress, bool) {
    let mut progress = Progress::new();
    let mut ctx = session.ctx(root, req, timing);
    match reuse {
        Some(r) => {
            progress.reused(ctx.id(), &ctx.account.key);
            ctx.reused = true;
            ctx.on_page = r.page;
        }
        None => progress.sign_ins = session.sign_ins.clone(),
    }
    let timed = tokio::time::timeout(limit, run_signed_in(d, &ctx, retry_pauses, &mut progress)).await;
    let timed_out = timed.is_err();
    if timed_out && progress.failed.is_none() {
        progress.fail(None, RUN_TOO_LONG);
    }
    d.set_deadline(None);
    (progress, timed_out)
}

/// Signs in as `req`'s account, through its recipe or a saved session.
/// `None` when it could not, with the reason recorded in `progress`.
async fn sign_in_once<D: Driver>(
    d: &mut D,
    root: &Path,
    req: &RunRequest,
    timing: &Timing,
    progress: &mut Progress,
) -> Option<Session> {
    progress.at(SIGN_IN, None);
    let (recipe, account) = match prepare(root, &req.org, &req.project, &req.account) {
        Ok(x) => x,
        Err(e) => {
            progress.fail(None, e);
            return None;
        }
    };
    let Some(origin) = recipe.origins().into_iter().next() else {
        progress.fail(
            None,
            "the sign-in recipe's start address is not a usable http or https address - fix it in Auto Run, Sign-in recipe",
        );
        return None;
    };
    let ctx = Ctx { root, req, timing, recipe, account, origin, reused: false, on_page: None };

    let signed = sign_in(d, root, &ctx.recipe, &ctx.account, timing).await;
    if !signed.ok {
        progress.fail(None, ctx.could_not_sign_in(&signed));
        return None;
    }
    progress.signed_in(ctx.id(), &ctx.account.key, &signed);
    Some(Session { recipe: ctx.recipe, account: ctx.account, origin: ctx.origin, sign_ins: progress.sign_ins.clone() })
}

/// A signed-in browser's part of a run: the token page, then each step,
/// stopping at the first failure.
async fn run_signed_in<D: Driver>(d: &mut D, ctx: &Ctx<'_>, retry_pauses: &[Duration], progress: &mut Progress) {
    let req = ctx.req;
    progress.at(TOKEN_PAGE, None);
    let Some((mut doc, mut token)) = token(d, ctx, progress, true).await else { return };

    // What a run left out of an optional param, its default stands in for.
    let mut vars: BTreeMap<String, Value> = crate::api_templates::with_defaults(&req.template, &req.values).into_iter().collect();
    for (i, step) in req.template.steps.iter().enumerate() {
        let handler = step.query.get("handler").map(|h| exec::substitute_str(h, &vars));
        progress.at(&step.name, handler.clone());
        progress.sent += 1;
        d.set_deadline(Some(Instant::now() + step_limit(step)));
        let mut result = run_step(d, ctx, &doc, &token, step, handler.as_deref(), &mut vars, progress, 1).await;
        d.set_deadline(None);
        // A kept browser whose very first request is refused unread, or
        // sent to another page: its session most likely ended while it was
        // kept (signed in somewhere else since). Nothing was saved; the run
        // starts again, signed in afresh. A 400 with a body is the
        // template's own answer, and a template that expects a 400 is never
        // refused unread.
        if ctx.reused && i == 0 && matches!(&result, Err(f) if f.unread || f.session_ended) {
            applog::info(format!(
                "api template {}: the first request in a kept browser was refused unread or sent to another page",
                ctx.id()
            ));
            progress.dropped = true;
            return;
        }
        // Refused before any handler read it: nothing was saved, so it is
        // sent again - after each of `retry_pauses`, with a fresh token
        // (reading one signs in again if the session had ended).
        let mut attempt: u8 = 1;
        for pause in retry_pauses {
            if !matches!(&result, Err(f) if f.unread) {
                break;
            }
            attempt += 1;
            applog::info(format!(
                "api template {}: step {} was refused unread (an empty 400) - try {attempt} of {} in {} s, with a fresh token",
                ctx.id(),
                step.name,
                retry_pauses.len() + 1,
                pause.as_secs_f32()
            ));
            tokio::time::sleep(*pause).await;
            progress.at(TOKEN_PAGE, None);
            // `self::` because the loop's own `token` (the string) shadows the function.
            let Some((fresh_doc, fresh_token)) = self::token(d, ctx, progress, false).await else { return };
            (doc, token) = (fresh_doc, fresh_token);
            progress.at(&step.name, handler.clone());
            d.set_deadline(Some(Instant::now() + step_limit(step)));
            result = run_step(d, ctx, &doc, &token, step, handler.as_deref(), &mut vars, progress, attempt).await;
            d.set_deadline(None);
        }
        if attempt > 1 {
            result = result.map_err(|mut f| {
                if f.unread {
                    f.detail = format!("{} - it was refused the same way on all {attempt} tries", f.detail);
                }
                f
            });
        }
        match result {
            Ok((status, detail)) => progress.steps.push(StepReport {
                name: step.name.clone(),
                handler,
                status: Some(status),
                ok: true,
                detail,
            }),
            Err(StepFailure { status, detail, session_ended, .. }) => {
                progress.session_ended = session_ended;
                return progress.fail(status, detail);
            }
        }
    }
    progress.finished = true;
}

/// Opens the template's anti-forgery page and reads its token. A session
/// that has ended gets exactly one more sign-in, whichever way it shows:
/// the application sends the page to its login, or - where it does not
/// redirect (hosted PMSV10 renders `/hr/pmsv10/updatehub` for anyone) - the
/// page opens with no token on it.
///
/// `first` is the run's first look at the token page. Then a browser
/// already on the page (`Ctx::on_page`, same path) is not sent to it again
/// - the token is still read afresh - and a kept browser (`Ctx::reused`)
/// sent to another page, the application's sign-in, is not signed in here:
/// the run stops (`Progress::dropped`) to start again in a fresh browser.
async fn token<D: Driver>(d: &mut D, ctx: &Ctx<'_>, progress: &mut Progress, first: bool) -> Option<(Handle, String)> {
    let page = &ctx.req.template.antiforgery.page;
    if !is_safe_relative_path(page) {
        progress.fail(None, format!("the antiforgery page {page} is not a relative path on this origin"));
        return None;
    }
    let url = format!("{}{page}", ctx.origin);
    let policy = Policy::only(ctx.recipe.origins());
    let mut signed_in_again = false;
    let mut already_there = first && ctx.on_page.as_deref().is_some_and(|p| same_path(p, page));
    loop {
        if std::mem::take(&mut already_there) {
            applog::info(format!("api template {}: already on the token page, not loaded again", ctx.id()));
        } else {
            let went = execute_in(d, &Action::Navigate { url: url.clone() }, ctx.timing, &policy).await;
            if !went.ok {
                applog::warn(format!("api template {}: the token page did not open: {}", ctx.id(), went.detail));
                progress.fail(None, format!("the token page {page} did not open - see Settings, Logs"));
                return None;
            }
        }
        let href = match eval_value(d, "location.href").await {
            Ok(v) => v.as_str().unwrap_or("").to_string(),
            Err(e) => {
                applog::warn(format!("api template {}: reading the token page's address: {e}", ctx.id()));
                progress.fail(None, "the browser did not answer on the token page - see Settings, Logs");
                return None;
            }
        };
        // The token page on the recipe's own origin - the same path on any
        // other origin (an identity provider's, say) is another page.
        let why = if same_path(&href, page) && origin_of(&href).as_deref() == Some(ctx.origin.as_str()) {
            let read = match document(d).await {
                Ok(doc) => call_value(d, &doc, TOKEN_FN, &[]).await.map(|v| (doc, v)),
                Err(e) => Err(e),
            };
            let found = match &read {
                Ok((_, Value::String(t))) if !t.is_empty() => Some(t.as_str()),
                _ => None,
            };
            let cookies = cookies_for(d, ctx, &url).await;
            record_token_page(ctx, &href, found, cookies, &progress.sign_ins);
            match read {
                Ok((doc, Value::String(t))) if !t.is_empty() => return Some((doc, t)),
                Ok(_) if signed_in_again => {
                    progress.fail(None, format!("no anti-forgery token on {page}"));
                    return None;
                }
                Ok(_) => "the token page had no token",
                Err(e) => {
                    applog::warn(format!("api template {}: reading the token: {e}", ctx.id()));
                    progress.fail(None, "the browser did not answer on the token page - see Settings, Logs");
                    return None;
                }
            }
        } else {
            if signed_in_again {
                let cookies = cookies_for(d, ctx, &url).await;
                record_token_page(ctx, &href, None, cookies, &progress.sign_ins);
                progress.session_ended = true;
                progress.fail(None, "the token page sent us to another page - check the template's antiforgery page");
                return None;
            }
            if first && ctx.reused {
                let cookies = cookies_for(d, ctx, &url).await;
                record_token_page(ctx, &href, None, cookies, &progress.sign_ins);
                applog::info(format!(
                    "api template {}: the token page in a kept browser was sent to {}",
                    ctx.id(),
                    path_of(&href)
                ));
                progress.dropped = true;
                return None;
            }
            "the token page was sent elsewhere"
        };
        // The saved session was no longer good. Throw it away and sign in
        // properly, once.
        applog::info(format!("api template {}: the session had ended ({why}) - signing in again", ctx.id()));
        forget_session(ctx.root, &ctx.account.key);
        progress.at(SIGN_IN, None);
        let again = sign_in(d, ctx.root, &ctx.recipe, &ctx.account, ctx.timing).await;
        if !again.ok {
            progress.fail(None, ctx.could_not_sign_in(&again));
            return None;
        }
        progress.signed_in(ctx.id(), &ctx.account.key, &again);
        progress.at(TOKEN_PAGE, None);
        signed_in_again = true;
    }
}

/// A browser call that failed, as the person reads it; the raw error goes
/// to the app log.
fn browser_failed(ctx: &Ctx<'_>, step: &Step, e: &CdpError) -> String {
    applog::warn(format!("api template {}: step {}: {e}", ctx.id(), step.name));
    match e {
        CdpError::Timeout { .. } => step_too_long(step_limit(step)),
        _ => "the browser did not answer while sending this step - see Settings, Logs".to_string(),
    }
}

/// Why a step failed. `unread` marks the one failure worth trying again: a
/// 400 with no body, which the application sends when it refuses a request
/// before any handler reads it (hosted PeoplesHR does this at busy moments
/// and when the account's session was taken over) - so nothing was saved,
/// and sending it again with a fresh token cannot save anything twice.
/// `session_ended` marks a request sent to another page instead: the
/// session most likely ended, and the browser is not kept.
struct StepFailure {
    status: Option<u16>,
    detail: String,
    unread: bool,
    session_ended: bool,
}

impl From<(Option<u16>, String)> for StepFailure {
    fn from((status, detail): (Option<u16>, String)) -> Self {
        StepFailure { status, detail, unread: false, session_ended: false }
    }
}

type StepResult = Result<(u16, String), StepFailure>;

/// The pauses before each new try of a step the application refused
/// unread - one per retry, so a step is sent at most `len() + 1` times.
/// Hosted PeoplesHR refuses about two in five writes this way at busy
/// moments, each try independently of the last (a fresh token or a fresh
/// sign-in makes no difference), so what helps is more tries, spaced out.
pub const RETRY_PAUSES: [Duration; 3] = [Duration::from_secs(1), Duration::from_secs(3), Duration::from_secs(5)];

/// The sentence for a step refused unread, as the person reads it.
const REFUSED_UNREAD: &str = "the application refused this request without reading it (an empty 400) - \
     usually a busy moment on the server, or this account being signed in somewhere else";

/// One step: build it, send it from the page, check it, capture from it.
/// Every request the page actually made is written to the activity log,
/// failed ones included.
#[allow(clippy::too_many_arguments)]
async fn run_step<D: Driver>(
    d: &mut D,
    ctx: &Ctx<'_>,
    doc: &Handle,
    token: &str,
    step: &Step,
    handler: Option<&str>,
    vars: &mut BTreeMap<String, Value>,
    progress: &mut Progress,
    attempt: u8,
) -> StepResult {
    let files = step_files(ctx, step).map_err(|e| (None, e))?;
    let mut built = build_request(step, vars, &files).map_err(|e| (None, e))?;
    drop(files);
    let adapted = adapt_path_case(d, ctx, step, &mut built).await;
    let wire = serde_json::to_value(&built).map_err(|e| (None, format!("the request could not be built: {e}")))?;
    // Which cookies the browser holds for this address - the one thing a
    // rejected save (a 400 with no body) cannot say for itself.
    let cookies_sent = cookies_for(d, ctx, &format!("{}{}", ctx.origin, built.url))
        .await
        .map(|all| all.iter().map(|c| c["name"].clone()).collect::<Vec<_>>());
    let started = Instant::now();
    // The page aborts its own request at the step's limit; the DevTools
    // call waits a little longer, so it is the page that says so.
    let limit = step_limit(step);
    let limit_ms = Value::from(limit.as_millis() as u64);
    let answer =
        call_value_within(d, doc, FETCH_FN, &[wire, Value::String(token.to_string()), limit_ms], limit + FETCH_GRACE)
            .await;
    let duration_ms = started.elapsed().as_millis() as u64;

    let sent = Sent {
        status: None,
        duration_ms,
        response: "",
        cookies: cookies_sent.as_deref(),
        path_case_adapted: adapted.as_ref(),
        attempt,
    };
    let answer = match answer {
        Ok(a) => a,
        Err(e) => {
            record(ctx, step, &built, handler, &sent, token);
            return Err((None, browser_failed(ctx, step, &e)).into());
        }
    };
    let status = answer["status"].as_u64().and_then(|s| u16::try_from(s).ok());
    let text = answer["text"].as_str().unwrap_or("");
    record(ctx, step, &built, handler, &Sent { status, response: text, ..sent }, token);

    if let Some(err) = answer.get("error") {
        if err.as_str() == Some("timeout") {
            return Err((None, step_too_long(limit)).into());
        }
        applog::warn(format!(
            "api template {}: step {}: the request did not complete: {}",
            ctx.id(),
            step.name,
            excerpt(&scrub_tokens(&plain(err), Some(token)))
        ));
        return Err((None, "the request did not complete - see Settings, Logs".to_string()).into());
    }
    let Some(status) = status else {
        // Only the answer's shape: whatever it carries may be the body.
        let keys = match answer.as_object() {
            Some(map) => format!("an object with keys: {}", map.keys().cloned().collect::<Vec<_>>().join(", ")),
            None => "something that is not an object".to_string(),
        };
        applog::warn(format!("api template {}: step {}: the page answered {keys}", ctx.id(), step.name));
        return Err((None, "the page gave no answer for this request - see Settings, Logs".to_string()).into());
    };

    let final_url = answer["finalUrl"].as_str().unwrap_or("");
    if answer["redirected"].as_bool() == Some(true) && !same_path(final_url, &built.url) {
        applog::warn(format!(
            "api template {}: step {} was redirected to {}",
            ctx.id(),
            step.name,
            path_of(final_url)
        ));
        return Err(StepFailure {
            status: Some(status),
            detail: "was sent to another page - the session may have ended".to_string(),
            unread: false,
            session_ended: true,
        });
    }

    if status == 400 && text.trim().is_empty() && step.expect.status != 400 {
        return Err(StepFailure { status: Some(400), detail: REFUSED_UNREAD.to_string(), unread: true, session_ended: false });
    }

    let parsed = check_expect(&step.expect, status, text).map_err(|e| {
        // `e` can quote the answer's own values, so it is scrubbed too.
        let e = scrub_tokens(&e, Some(token));
        let shown = shown(text, token);
        let detail = if shown.is_empty() { e } else { format!("{e} - the response began: {shown}") };
        (Some(status), detail)
    })?;

    let mut got = vec![];
    for (name, path) in &step.capture {
        let segs = parse_capture_path(path).map_err(|e| (Some(status), e))?;
        let Some(value) = parsed.as_ref().and_then(|body| capture(body, &segs)) else {
            return Err((Some(status), format!("capture {name} found nothing at {path}")).into());
        };
        // Later steps get what was captured; the report - and everything
        // built from it - only ever gets it without a token in it.
        progress.created.insert(name.clone(), scrub_value(&value, Some(token)));
        vars.insert(name.clone(), value);
        got.push(name.as_str());
    }
    let mut detail = format!("status {status}");
    if let Some(sent) = sent_files(&built.body) {
        detail.push_str(&format!(", {sent}"));
    }
    if !got.is_empty() {
        detail.push_str(&format!(", captured {}", got.join(", ")));
    }
    Ok((status, detail))
}

/// A request or response body as a record or a sentence shows it: every
/// anti-forgery token taken out (`scrub_tokens`) BEFORE the 500-character
/// excerpt, so the cap can never leave half a token showing.
fn shown(body: &str, token: &str) -> String {
    excerpt(&scrub_tokens(body, Some(token)))
}

/// What came of one request, for its activity record.
#[derive(Clone, Copy)]
struct Sent<'a> {
    status: Option<u16>,
    duration_ms: u64,
    response: &'a str,
    /// The names of the cookies the browser held for the request's
    /// address; `None` when it would not say.
    cookies: Option<&'a [Value]>,
    /// `{ from, to, cookie }` when the step's path was sent in a cookie's
    /// letter case - see `adapt_path_case`.
    path_case_adapted: Option<&'a Value>,
    /// 1, then 2, 3... for each retry of a step the application refused
    /// unread (see `RETRY_PAUSES`).
    attempt: u8,
}

/// One activity record for a request the page made. Never a token: the
/// one the runner read is not in `built`, and both bodies go through
/// `shown`, which takes out that one and any other a page carries. Never
/// a cookie value either: only the names the browser would send.
fn record(ctx: &Ctx<'_>, step: &Step, built: &exec::BuiltRequest, handler: Option<&str>, sent: &Sent<'_>, token: &str) {
    activity_log::record(
        Kind::Api,
        json!({
            "template": ctx.id(),
            "mode": ctx.req.mode.label(),
            "account": ctx.req.account,
            "origin": ctx.origin,
            "step": step.name,
            "method": method_name(built.method),
            "url": built.url,
            "handler": handler,
            "status": sent.status,
            "duration_ms": sent.duration_ms,
            "request": shown(&body_text(&built.body), token),
            "response": shown(sent.response, token),
            "cookies_sent": sent.cookies,
            "path_case_adapted": sent.path_case_adapted,
            "attempt": sent.attempt,
        }),
    );
}

/// The activity record for the token page: where it was asked for, where
/// the browser actually ended up, whether a token was there (and how long
/// it was - never the token) and what cookies the page holds. It is what
/// tells a save the server rejected apart from one that never had a
/// token to send.
fn record_token_page(
    ctx: &Ctx<'_>,
    final_url: &str,
    token: Option<&str>,
    cookies: Option<Vec<Value>>,
    sign_ins: &[Value],
) {
    activity_log::record(
        Kind::Api,
        json!({
            "event": "token_page",
            "template": ctx.id(),
            "mode": ctx.req.mode.label(),
            "account": ctx.req.account,
            "origin": ctx.origin,
            "requested": ctx.req.template.antiforgery.page,
            "final_url": scrub_tokens(final_url, token),
            "token_found": token.is_some(),
            "token_length": token.map(str::len),
            "cookies": cookies,
            "sign_ins": sign_ins,
        }),
    );
}

/// Sends a step's path in the letter case of an application cookie that
/// covers it only when case is ignored (see `cookies`). Cookie paths are
/// case-sensitive and the application's routing is not, so as written the
/// request would go without that cookie - for an anti-forgery cookie, an
/// empty 400 - while in the cookie's case it reaches the same handler with
/// it. One template then runs wherever the application keeps its cookies,
/// `/hr/PMSV10` on one server and `/hr/pmsv10` on another. The most
/// specific such cookie decides. Returns `{ from, to, cookie }` for the
/// activity record when the path changed; a browser that will not list its
/// cookies is logged and the path is sent as written.
async fn adapt_path_case<D: Driver>(
    d: &mut D,
    ctx: &Ctx<'_>,
    step: &Step,
    built: &mut exec::BuiltRequest,
) -> Option<Value> {
    let jar = match d.call("Network.getAllCookies", json!({})).await {
        Ok(answer) => jar_cookies(&answer["cookies"]),
        Err(e) => {
            applog::warn(format!("api template {}: listing the cookies to check a path's case: {e}", ctx.id()));
            return None;
        }
    };
    // `scheme://host[:port]`. An IPv6 origin (`https://[::1]:8443`) does not
    // parse here: its paths go as written, which is the safe way to fail.
    let host = ctx.origin.split("://").nth(1)?.split(':').next()?;
    let at = built.url.find(['?', '#']).unwrap_or(built.url.len());
    let (from, rest) = (built.url[..at].to_string(), built.url[at..].to_string());
    let cookie = case_blind_cookies(&jar, host, &from).into_iter().max_by_key(|c| c.path.len())?.clone();
    let to = in_cookie_case(&from, &cookie.path);
    if let Some(kept) = lost_by_adapting(&jar, host, &from, &to) {
        applog::warn(format!(
            "api template {}: step {}: sent {from} as written - {to} would carry the cookie {} but lose {} on {}",
            ctx.id(),
            step.name,
            cookie.name,
            kept.name,
            kept.path
        ));
        return None;
    }
    applog::info(format!(
        "api template {}: step {}: sent {to} rather than {from} - the application keeps its cookie {} on {}, \
         and cookie paths are case-sensitive",
        ctx.id(),
        step.name,
        cookie.name,
        cookie.path
    ));
    built.url = format!("{to}{rest}");
    Some(json!({ "from": from, "to": to, "cookie": cookie.name }))
}

/// The cookies the browser holds for `url`, as a record may show them -
/// see `cookie_summary`. A browser that will not say is logged and read as
/// `None`: this is a diagnostic, and never a reason to stop a run.
async fn cookies_for<D: Driver>(d: &mut D, ctx: &Ctx<'_>, url: &str) -> Option<Vec<Value>> {
    match d.call("Network.getCookies", json!({ "urls": [url] })).await {
        Ok(answer) => Some(cookie_summary(&answer["cookies"])),
        Err(e) => {
            applog::warn(format!("api template {}: reading the cookies for its activity record: {e}", ctx.id()));
            None
        }
    }
}

/// Each cookie's name, where it applies and its flags - never its value,
/// which is copied from nowhere because it is never read.
fn cookie_summary(cookies: &Value) -> Vec<Value> {
    let Some(all) = cookies.as_array() else { return vec![] };
    all.iter()
        .map(|c| {
            json!({
                "name": c["name"],
                "domain": c["domain"],
                "path": c["path"],
                "http_only": c["httpOnly"],
                "secure": c["secure"],
                "same_site": c["sameSite"],
                "session": c["session"],
            })
        })
        .collect()
}
