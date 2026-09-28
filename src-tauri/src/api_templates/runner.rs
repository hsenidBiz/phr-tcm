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
use super::{check, check_values, is_safe_relative_path, ApiTemplate, Method, Step};
use crate::activity_log::{self, Kind};
use crate::applog;
use crate::autorun::accounts::Account;
use crate::autorun::nav::path_of;
use crate::autorun::recipe::{origin_of, SignInRecipe};
use crate::autorun::replay::Browsers;
use crate::autorun::sessions::forget_session;
use crate::autorun::signin::{prepare, sign_in, SignInOutcome};
use crate::browser::actions::{execute_in, Action, Policy};
use crate::browser::cdp::{CdpError, Driver};
use crate::browser::page::{call_value, document, eval_value, Handle};
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
/// How long a whole run may take once its browser is open.
pub const RUN_LIMIT: Duration = Duration::from_secs(180);

const RUN_TOO_LONG: &str = "the run took longer than 3 minutes";
const STEP_TOO_LONG: &str = "the step took longer than 30 seconds";
/// Names for the parts of a run that are not a template step, as a
/// `StepReport` and `RunReport::failed` show them.
const SIGN_IN: &str = "Sign in";
const TOKEN_PAGE: &str = "Anti-forgery token";
const BROWSER: &str = "Browser";

/// Reads the anti-forgery token Razor Pages writes into every form page.
/// Called on the document; returns the token or `null`.
pub const TOKEN_FN: &str = r#"function () { const i = this.querySelector('input[name="__RequestVerificationToken"]'); return i ? i.value : null; }"#;

/// Sends one built request (`exec::BuiltRequest`, serialized) from the page
/// itself, so the browser attaches the session cookies; `token` goes only
/// into the `RequestVerificationToken` header and is never returned. The
/// request is aborted after 30 s, and at most 64 KB of the body is read
/// back. A request that did not complete comes back as `{ error }` rather
/// than a throw, so the runner can tell a timeout from anything else.
pub const FETCH_FN: &str = r#"async function (req, token) {
  const headers = { "RequestVerificationToken": token, "Accept": "application/json" };
  let body;
  if (req.body && req.body.kind === "json") {
    headers["Content-Type"] = "application/json";
    body = JSON.stringify(req.body.value);
  } else if (req.body && req.body.kind === "form") {
    body = new FormData();
    for (const [k, v] of Object.entries(req.body.fields)) body.append(k, v);
  }
  const ctrl = new AbortController();
  const timer = setTimeout(() => ctrl.abort(), 30000);
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
/// existing id needs `replace` and a `why`), or running a saved template.
#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    Prove { replace: bool, why: Option<String> },
    Run,
}

impl Mode {
    /// `"prove"` / `"run"` - how the activity log and the app log name it.
    pub fn label(&self) -> &'static str {
        match self {
            Mode::Prove { .. } => "prove",
            Mode::Run => "run",
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepReport {
    pub name: String,
    /// The step's `handler` query value, if it has one.
    pub handler: Option<String>,
    pub status: Option<u16>,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunReport {
    pub ok: bool,
    /// The template's id.
    pub template: String,
    /// The template's declared outputs - only on success.
    pub outputs: BTreeMap<String, Value>,
    /// Everything captured before the run stopped: what a failed run
    /// already did in the application, which nothing undoes.
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
/// anti-forgery page, the recipe and the account - and, proving over an
/// existing id, `replace: true` with a non-blank `why`.
pub fn preflight(root: &Path, req: &RunRequest, existing: Option<&ApiTemplate>) -> Result<(), Vec<String>> {
    let t = &req.template;
    // A saved template carries the app's `proven` block, which `check`
    // refuses in a DRAFT; running a saved one is exactly when it is there.
    let mut problems = match req.mode {
        Mode::Run => check(&ApiTemplate { proven: None, ..t.clone() }),
        Mode::Prove { .. } => check(t),
    };
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
        }
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

/// The request body as the activity log keeps it (then excerpted).
fn body_text(b: &Body) -> String {
    match b {
        Body::None => String::new(),
        Body::Json { value } => value.to_string(),
        Body::Form { fields } => serde_json::to_string(fields).unwrap_or_default(),
    }
}

/// What one run needs from its request, resolved once.
struct Ctx<'a> {
    root: &'a Path,
    req: &'a RunRequest,
    timing: &'a Timing,
    recipe: SignInRecipe,
    account: Account,
    origin: String,
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

/// Runs `req.template` in a fresh browser from `browsers` and says what
/// happened. The browser is closed on every path out, a timeout included.
/// Saves nothing: see the module comment.
pub async fn run_template<B: Browsers>(browsers: &mut B, root: &Path, req: &RunRequest, timing: &Timing) -> RunReport {
    run_template_within(browsers, root, req, timing, RUN_LIMIT).await
}

/// `run_template` with the run limit given rather than `RUN_LIMIT` - the
/// only way a test reaches the timeout path without waiting three minutes.
/// The sentence a timeout reports still says three minutes.
pub async fn run_template_within<B: Browsers>(
    browsers: &mut B,
    root: &Path,
    req: &RunRequest,
    timing: &Timing,
    limit: Duration,
) -> RunReport {
    let mut progress = Progress::new();
    match browsers.open().await {
        Err(why) => progress.fail(None, format!("the browser did not open: {why}")),
        Ok(mut d) => {
            // `close` sits outside the timed future on purpose: dropping
            // that future on a timeout must not skip it.
            let timed = tokio::time::timeout(limit, drive(&mut d, root, req, timing, &mut progress)).await;
            if timed.is_err() && progress.failed.is_none() {
                progress.fail(None, RUN_TOO_LONG);
            }
            d.set_deadline(None);
            browsers.close(d).await;
        }
    }

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

/// Everything after the browser is open: sign in, fetch the token, run the
/// steps. Stops at the first failure, recording it in `progress`.
async fn drive<D: Driver>(d: &mut D, root: &Path, req: &RunRequest, timing: &Timing, progress: &mut Progress) {
    progress.at(SIGN_IN, None);
    let (recipe, account) = match prepare(root, &req.org, &req.project, &req.account) {
        Ok(x) => x,
        Err(e) => return progress.fail(None, e),
    };
    let Some(origin) = recipe.origins().into_iter().next() else {
        return progress.fail(
            None,
            "the sign-in recipe's start address is not a usable http or https address - fix it in Auto Run, Sign-in recipe",
        );
    };
    let ctx = Ctx { root, req, timing, recipe, account, origin };

    let signed = sign_in(d, root, &ctx.recipe, &ctx.account, timing).await;
    if !signed.ok {
        return progress.fail(None, ctx.could_not_sign_in(&signed));
    }

    progress.at(TOKEN_PAGE, None);
    let Some((doc, token)) = token(d, &ctx, progress).await else { return };

    let mut vars: BTreeMap<String, Value> = req.values.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    for step in &req.template.steps {
        let handler = step.query.get("handler").map(|h| exec::substitute_str(h, &vars));
        progress.at(&step.name, handler.clone());
        progress.sent += 1;
        d.set_deadline(Some(Instant::now() + STEP_LIMIT));
        let result = run_step(d, &ctx, &doc, &token, step, handler.as_deref(), &mut vars, progress).await;
        d.set_deadline(None);
        match result {
            Ok((status, detail)) => progress.steps.push(StepReport {
                name: step.name.clone(),
                handler,
                status: Some(status),
                ok: true,
                detail,
            }),
            Err((status, detail)) => return progress.fail(status, detail),
        }
    }
    progress.finished = true;
}

/// Opens the template's anti-forgery page and reads its token. A page that
/// turns out to be somewhere else (the saved session had gone stale and
/// the application sent us to its login) gets exactly one more sign-in.
async fn token<D: Driver>(d: &mut D, ctx: &Ctx<'_>, progress: &mut Progress) -> Option<(Handle, String)> {
    let page = &ctx.req.template.antiforgery.page;
    if !is_safe_relative_path(page) {
        progress.fail(None, format!("the antiforgery page {page} is not a relative path on this origin"));
        return None;
    }
    let url = format!("{}{page}", ctx.origin);
    let policy = Policy::only(ctx.recipe.origins());
    let mut signed_in_again = false;
    loop {
        let went = execute_in(d, &Action::Navigate { url: url.clone() }, ctx.timing, &policy).await;
        if !went.ok {
            applog::warn(format!("api template {}: the token page did not open: {}", ctx.id(), went.detail));
            progress.fail(None, format!("the token page {page} did not open - see Settings, Logs"));
            return None;
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
        if same_path(&href, page) && origin_of(&href).as_deref() == Some(ctx.origin.as_str()) {
            break;
        }
        if signed_in_again {
            progress.fail(None, "the token page sent us to another page - check the template's antiforgery page");
            return None;
        }
        // The saved session was no longer good: the application sent the
        // page to its login. Throw it away and sign in properly, once.
        applog::info(format!("api template {}: the saved session had ended - signing in again", ctx.id()));
        forget_session(ctx.root, &ctx.account.key);
        progress.at(SIGN_IN, None);
        let again = sign_in(d, ctx.root, &ctx.recipe, &ctx.account, ctx.timing).await;
        if !again.ok {
            progress.fail(None, ctx.could_not_sign_in(&again));
            return None;
        }
        progress.at(TOKEN_PAGE, None);
        signed_in_again = true;
    }

    let read = match document(d).await {
        Ok(doc) => call_value(d, &doc, TOKEN_FN, &[]).await.map(|v| (doc, v)),
        Err(e) => Err(e),
    };
    match read {
        Ok((doc, Value::String(t))) if !t.is_empty() => Some((doc, t)),
        Ok(_) => {
            progress.fail(None, format!("no anti-forgery token on {page}"));
            None
        }
        Err(e) => {
            applog::warn(format!("api template {}: reading the token: {e}", ctx.id()));
            progress.fail(None, "the browser did not answer on the token page - see Settings, Logs");
            None
        }
    }
}

/// A browser call that failed, as the person reads it; the raw error goes
/// to the app log.
fn browser_failed(ctx: &Ctx<'_>, step: &Step, e: &CdpError) -> String {
    applog::warn(format!("api template {}: step {}: {e}", ctx.id(), step.name));
    match e {
        CdpError::Timeout { .. } => STEP_TOO_LONG.to_string(),
        _ => "the browser did not answer while sending this step - see Settings, Logs".to_string(),
    }
}

type StepResult = Result<(u16, String), (Option<u16>, String)>;

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
) -> StepResult {
    let built = build_request(step, vars).map_err(|e| (None, e))?;
    let wire = serde_json::to_value(&built).map_err(|e| (None, format!("the request could not be built: {e}")))?;
    let started = Instant::now();
    let answer = call_value(d, doc, FETCH_FN, &[wire, Value::String(token.to_string())]).await;
    let duration_ms = started.elapsed().as_millis() as u64;

    let answer = match answer {
        Ok(a) => a,
        Err(e) => {
            record(ctx, step, &built, handler, None, duration_ms, "", token);
            return Err((None, browser_failed(ctx, step, &e)));
        }
    };
    let status = answer["status"].as_u64().and_then(|s| u16::try_from(s).ok());
    let text = answer["text"].as_str().unwrap_or("");
    record(ctx, step, &built, handler, status, duration_ms, text, token);

    if let Some(err) = answer.get("error") {
        if err.as_str() == Some("timeout") {
            return Err((None, STEP_TOO_LONG.to_string()));
        }
        applog::warn(format!(
            "api template {}: step {}: the request did not complete: {}",
            ctx.id(),
            step.name,
            excerpt(&scrub_tokens(&plain(err), Some(token)))
        ));
        return Err((None, "the request did not complete - see Settings, Logs".to_string()));
    }
    let Some(status) = status else {
        // Only the answer's shape: whatever it carries may be the body.
        let keys = match answer.as_object() {
            Some(map) => format!("an object with keys: {}", map.keys().cloned().collect::<Vec<_>>().join(", ")),
            None => "something that is not an object".to_string(),
        };
        applog::warn(format!("api template {}: step {}: the page answered {keys}", ctx.id(), step.name));
        return Err((None, "the page gave no answer for this request - see Settings, Logs".to_string()));
    };

    let final_url = answer["finalUrl"].as_str().unwrap_or("");
    if answer["redirected"].as_bool() == Some(true) && !same_path(final_url, &built.url) {
        applog::warn(format!(
            "api template {}: step {} was redirected to {}",
            ctx.id(),
            step.name,
            path_of(final_url)
        ));
        return Err((Some(status), "was sent to another page - the session may have ended".to_string()));
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
            return Err((Some(status), format!("capture {name} found nothing at {path}")));
        };
        // Later steps get what was captured; the report - and everything
        // built from it - only ever gets it without a token in it.
        progress.created.insert(name.clone(), scrub_value(&value, Some(token)));
        vars.insert(name.clone(), value);
        got.push(name.as_str());
    }
    let detail = if got.is_empty() {
        format!("status {status}")
    } else {
        format!("status {status}, captured {}", got.join(", "))
    };
    Ok((status, detail))
}

/// A request or response body as a record or a sentence shows it: every
/// anti-forgery token taken out (`scrub_tokens`) BEFORE the 500-character
/// excerpt, so the cap can never leave half a token showing.
fn shown(body: &str, token: &str) -> String {
    excerpt(&scrub_tokens(body, Some(token)))
}

/// One activity record for a request the page made. Never a token: the
/// one the runner read is not in `built`, and both bodies go through
/// `shown`, which takes out that one and any other a page carries.
#[allow(clippy::too_many_arguments)]
fn record(
    ctx: &Ctx<'_>,
    step: &Step,
    built: &exec::BuiltRequest,
    handler: Option<&str>,
    status: Option<u16>,
    duration_ms: u64,
    response: &str,
    token: &str,
) {
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
            "status": status,
            "duration_ms": duration_ms,
            "request": shown(&body_text(&built.body), token),
            "response": shown(response, token),
        }),
    );
}
