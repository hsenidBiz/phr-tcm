//! Running a fixture: its templates in order, in ONE fresh browser signed
//! in ONCE as the fixture's account, under that account's lease - the
//! design doc's "Running a fixture" (section 1).
//!
//! Each step is an ordinary template run (`runner::run_in_session`) whose
//! values are the step's params with their placeholders filled in:
//! `{{steps.<n>.<output>}}` from an earlier step's outputs, `{{now:<format>}}`
//! from the run's start time in local time, and `{{prefix}}` from the active
//! environment's test name prefix. The run stops at the first failed step.
//!
//! Whatever the run made - even when a later step failed - goes into the
//! record of test-made drafts (`autorun::test_made`), so Clean up can find a
//! half-made draft too. Every run, failed or not, is added to the fixture's
//! history; only a successful one changes its current outputs.
//!
//! Nothing here formats a password, a cookie, a host or a query string:
//! the step reports are the runner's own sentences, and the app log gets
//! ids and names only.

use super::exec;
use super::fixture::{validate, Fixture, FixtureRun};
use super::fixture_store;
use super::runner::{
    account_lease, claim, open_session, preflight, run_in_session, stopped_before_sign_in, Mode, RunReport, RunRequest,
    API_TEMPLATE_BUSY, RETRY_PAUSES, RUN_LIMIT,
};
use super::{store as template_store, ApiTemplate, ParamType};
use crate::applog;
use crate::autorun::replay::Browsers;
use crate::autorun::test_made::{self, TestMade, PRESENT};
use crate::browser::cdp::Driver;
use crate::browser::timing::Timing;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

/// What a fixture run did.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct FixtureReport {
    pub ok: bool,
    /// The fixture's outputs - only when every step passed.
    #[specta(type = BTreeMap<String, specta_typescript::Unknown>)]
    pub outputs: BTreeMap<String, Value>,
    /// What the run made, as it was recorded as test-made.
    pub made: Vec<TestMade>,
    /// Each step's own report, in order, up to the one that stopped the run.
    pub steps: Vec<RunReport>,
    /// Why the run stopped: `step <n>: <that template's sentence>`, or the
    /// reason it could not start.
    pub failed: Option<String>,
    pub warnings: Vec<String>,
}

impl FixtureReport {
    /// The run in one sentence.
    pub fn message(&self) -> String {
        match &self.failed {
            Some(why) => why.clone(),
            None => format!("every step passed ({} steps)", self.steps.len()),
        }
    }
}

/// The warning for a made thing whose name does not start with the test
/// prefix: it is recorded all the same.
pub fn prefix_warning(kind: &str, name: &str) -> String {
    format!("{kind} {name} does not start with the test prefix, so Clean up will not find it")
}

/// Said when a fixture id is not saved for the project.
pub fn no_such_fixture(id: &str) -> String {
    format!("no fixture called \"{id}\" is saved for this project")
}

/// The run's start time, in local time, as `{{now:<format>}}` writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clock {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl Clock {
    pub fn now_local() -> Self {
        use chrono::{Datelike, Timelike};
        let t = chrono::Local::now();
        Clock { year: t.year(), month: t.month(), day: t.day(), hour: t.hour(), minute: t.minute(), second: t.second() }
    }
}

/// `format` with `yyyy MM dd HH mm ss` replaced by the clock's year, month,
/// day, hour (24-hour), minute and second; every other character as it is.
pub fn format_now(clock: &Clock, format: &str) -> String {
    let mut out = String::new();
    let mut rest = format;
    while let Some(c) = rest.chars().next() {
        let token = [
            ("yyyy", format!("{:04}", clock.year)),
            ("MM", format!("{:02}", clock.month)),
            ("dd", format!("{:02}", clock.day)),
            ("HH", format!("{:02}", clock.hour)),
            ("mm", format!("{:02}", clock.minute)),
            ("ss", format!("{:02}", clock.second)),
        ]
        .into_iter()
        .find(|(t, _)| rest.starts_with(t));
        match token {
            Some((t, text)) => {
                out.push_str(&text);
                rest = &rest[t.len()..];
            }
            None => {
                out.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    out
}

/// A JSON value as text: a string without its quotes.
fn plain(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// A value that is text where the template's param wants a number, a
/// boolean or a list: a fixture's params are text, so `"5"` for a number
/// param is read as the number 5. Anything that does not read as the
/// wanted type is left as it is, for the run's own check to name.
fn typed(t: &ApiTemplate, name: &str, v: Value) -> Value {
    let Some(kind) = t.params.iter().find(|p| p.name == name).map(|p| p.kind) else { return v };
    let Value::String(s) = &v else { return v };
    let fits: fn(&Value) -> bool = match kind {
        ParamType::Number => Value::is_number,
        ParamType::Boolean => Value::is_boolean,
        ParamType::List => Value::is_array,
        ParamType::String | ParamType::Date => return v,
    };
    match serde_json::from_str::<Value>(s.trim()) {
        Ok(parsed) if fits(&parsed) => parsed,
        _ => v,
    }
}

/// Step `n`'s run request: its params with every placeholder filled in from
/// `vars` (earlier steps' outputs and the prefix) and the clock.
fn step_request(
    org: &str,
    project: &str,
    account: &str,
    t: &ApiTemplate,
    params: &BTreeMap<String, String>,
    vars: &BTreeMap<String, Value>,
    clock: &Clock,
) -> RunRequest {
    let mut values = serde_json::Map::new();
    for (name, raw) in params {
        let mut here = vars.clone();
        for p in exec::placeholders(raw) {
            if let Some(format) = p.strip_prefix("now:") {
                here.insert(p.clone(), Value::String(format_now(clock, format)));
            }
        }
        let v = exec::substitute(&Value::String(raw.clone()), &here);
        values.insert(name.clone(), typed(t, name, v));
    }
    RunRequest {
        org: org.to_string(),
        project: project.to_string(),
        account: account.to_string(),
        values,
        mode: Mode::Run,
        template: t.clone(),
    }
}

/// The value a whole `{{steps.<n>.<output>}}` stands for, if it was captured.
fn resolve(placeholder: &str, vars: &BTreeMap<String, Value>) -> Option<Value> {
    let names = exec::placeholders(placeholder);
    match names.as_slice() {
        [one] => vars.get(one).cloned(),
        _ => None,
    }
}

/// What the steps did: each step's report, why the run stopped (with the
/// step, when a step stopped it), and every value a later step, an output
/// or a `creates` entry may read, under `steps.<n>.<output>`.
struct Ran {
    steps: Vec<RunReport>,
    failed: Option<(Option<usize>, String)>,
    vars: BTreeMap<String, Value>,
}

/// Every step of `f`, in one browser signed in once. `prefix` stands for
/// `{{prefix}}`.
#[allow(clippy::too_many_arguments)]
async fn run_steps<B: Browsers>(
    browsers: &mut B,
    root: &Path,
    org: &str,
    project: &str,
    f: &Fixture,
    prefix: &str,
    timing: &Timing,
    limit: Duration,
    retry_pauses: &[Duration],
    clock: &Clock,
) -> Ran {
    let mut ran = Ran { steps: vec![], failed: None, vars: BTreeMap::new() };
    ran.vars.insert("prefix".to_string(), Value::String(prefix.to_string()));

    // The save rules again: a template may have been replaced, re-proven
    // as a delete, or removed since the fixture was saved.
    let lookup = |id: &str| template_store::load(root, org, project, id).ok().flatten();
    if let Err(problems) = validate(f, &lookup) {
        ran.failed = Some((None, problems.join("; ")));
        return ran;
    }
    let mut templates = Vec::with_capacity(f.steps.len());
    for (i, step) in f.steps.iter().enumerate() {
        match lookup(&step.template) {
            Some(t) => templates.push(t),
            // `validate` has just found it; gone in between is "not proven".
            None => {
                ran.failed = Some((Some(i + 1), format!("step {}: template {} is not proven", i + 1, step.template)));
                return ran;
            }
        }
    }

    // Step 1 is checked before the lease and the browser: its values can
    // only use the prefix and the clock, so nothing is opened for a run
    // that cannot start.
    let first = step_request(org, project, &f.account, &templates[0], &f.steps[0].params, &ran.vars, clock);
    if let Err(problems) = preflight(root, &first, None) {
        ran.failed = Some((Some(1), format!("step 1: {}", problems.join("; "))));
        return ran;
    }

    // One lease for the whole run, held until the browser is closed.
    let _lease = match account_lease(root, &f.account, timing).await {
        Ok(l) => l,
        Err(why) => {
            let report = stopped_before_sign_in(&first, false, why);
            ran.failed = Some((Some(1), format!("step 1: {}", report.message())));
            ran.steps.push(report);
            return ran;
        }
    };
    let mut d = match browsers.open().await {
        Ok(d) => d,
        Err(why) => {
            let report = stopped_before_sign_in(&first, true, why);
            ran.failed = Some((Some(1), format!("step 1: {}", report.message())));
            ran.steps.push(report);
            return ran;
        }
    };

    // The 3-minute run limit (`limit`) applies to each template step on its
    // own - and to the sign-in before them - never to the whole fixture: a
    // fixture of several templates may well take longer than one run may.
    match open_session(&mut d, root, &first, timing, limit).await {
        Err(report) => {
            ran.failed = Some((Some(1), format!("step 1: {}", report.message())));
            ran.steps.push(report);
        }
        Ok(session) => {
            for (i, (step, t)) in f.steps.iter().zip(&templates).enumerate() {
                let n = i + 1;
                let req = if n == 1 {
                    first.clone()
                } else {
                    let req = step_request(org, project, &f.account, t, &step.params, &ran.vars, clock);
                    if let Err(problems) = preflight(root, &req, None) {
                        ran.failed = Some((Some(n), format!("step {n}: {}", problems.join("; "))));
                        break;
                    }
                    req
                };
                let report = run_in_session(&mut d, root, &req, timing, &session, limit, retry_pauses).await;
                if report.ok {
                    for (name, v) in &report.outputs {
                        ran.vars.insert(format!("steps.{n}.{name}"), v.clone());
                    }
                    ran.steps.push(report);
                    continue;
                }
                // What a failed step had captured under a declared output's
                // name is still what it made.
                for name in &t.outputs {
                    if let Some(v) = report.created.get(name) {
                        ran.vars.insert(format!("steps.{n}.{name}"), v.clone());
                    }
                }
                ran.failed = Some((Some(n), format!("step {n}: {}", report.message())));
                ran.steps.push(report);
                break;
            }
        }
    }
    d.set_deadline(None);
    browsers.close(d).await;
    ran
}

/// Runs `f` and records what it made and the run itself. See the module
/// comment. The caller holds the one-at-a-time template slot (`run_saved`
/// takes it).
pub async fn run_fixture<B: Browsers>(
    browsers: &mut B,
    root: &Path,
    org: &str,
    project: &str,
    f: &Fixture,
    timing: &Timing,
) -> FixtureReport {
    run_fixture_within(browsers, root, org, project, f, timing, RUN_LIMIT, &RETRY_PAUSES, Clock::now_local()).await
}

/// `run_fixture` with the per-step limit, the retry pauses and the clock
/// given - the way a test reaches a short pause or a known date.
#[allow(clippy::too_many_arguments)]
pub async fn run_fixture_within<B: Browsers>(
    browsers: &mut B,
    root: &Path,
    org: &str,
    project: &str,
    f: &Fixture,
    timing: &Timing,
    limit: Duration,
    retry_pauses: &[Duration],
    clock: Clock,
) -> FixtureReport {
    let at = applog::stamp();
    let created_at = applog::iso_stamp();
    let run_id = crate::autorun::store::new_run_id();

    let ran = match crate::environments::active(root) {
        Ok(env) => {
            let ran = run_steps(browsers, root, org, project, f, &env.test_prefix, timing, limit, retry_pauses, &clock).await;
            Some((env, ran))
        }
        Err(e) => {
            applog::warn(format!("fixture {}: the active environment could not be read: {e}", f.id));
            None
        }
    };
    let (env, ran) = match ran {
        Some(x) => x,
        None => {
            let failed = "the active environment could not be read - see Settings, Logs".to_string();
            let report = FixtureReport {
                ok: false,
                outputs: BTreeMap::new(),
                made: vec![],
                steps: vec![],
                failed: Some(failed),
                warnings: vec![],
            };
            append(root, org, project, f, &at, &report, None);
            return report;
        }
    };

    let ok = ran.failed.is_none();
    let outputs: BTreeMap<String, Value> = if ok {
        f.outputs.iter().filter_map(|(name, p)| resolve(p, &ran.vars).map(|v| (name.clone(), v))).collect()
    } else {
        BTreeMap::new()
    };

    let mut made = Vec::new();
    let mut warnings = Vec::new();
    for c in &f.creates {
        let Some(id) = resolve(&c.id, &ran.vars).map(|v| plain(&v)).filter(|id| !id.trim().is_empty()) else {
            applog::info(format!(
                "fixture {}: run {run_id}: no {} id was captured, so that {} is not recorded as test-made",
                f.id, c.kind, c.kind
            ));
            continue;
        };
        let name = match resolve(&c.name, &ran.vars) {
            Some(v) => plain(&v),
            None => {
                applog::info(format!(
                    "fixture {}: run {run_id}: {} {id} was made but no name was captured for it",
                    f.id, c.kind
                ));
                String::new()
            }
        };
        if !name.is_empty() && !name.starts_with(&env.test_prefix) {
            warnings.push(prefix_warning(&c.kind, &name));
        }
        made.push(TestMade {
            environment: env.id.clone(),
            kind: c.kind.clone(),
            id,
            name,
            created_at: created_at.clone(),
            fixture: f.id.clone(),
            run_id: run_id.clone(),
            case_id: None,
            status: PRESENT.to_string(),
        });
    }
    if let Err(e) = test_made::record(root, &made) {
        applog::warn(format!("fixture {}: run {run_id}: what it made could not be recorded as test-made: {e}", f.id));
        warnings.push("what this run made could not be recorded as test-made - see Settings, Logs".to_string());
    }

    let (failed_step, failed) = match ran.failed {
        Some((step, why)) => (step, Some(why)),
        None => (None, None),
    };
    let report = FixtureReport { ok, outputs, made, steps: ran.steps, failed, warnings };
    append(root, org, project, f, &at, &report, failed_step);
    match failed_step {
        _ if report.ok => applog::info(format!(
            "fixture {}: run {run_id} ok, {} steps, {} made",
            f.id,
            report.steps.len(),
            report.made.len()
        )),
        Some(n) => applog::info(format!("fixture {}: run {run_id} failed at step {n}, {} made", f.id, report.made.len())),
        None => applog::info(format!("fixture {}: run {run_id} did not start", f.id)),
    }
    report
}

/// Adds the run to the fixture's history. A history that cannot be written
/// is logged; the run itself stands.
fn append(root: &Path, org: &str, project: &str, f: &Fixture, at: &str, report: &FixtureReport, failed_step: Option<usize>) {
    let run = FixtureRun {
        at: at.to_string(),
        ok: report.ok,
        failed_step: failed_step.and_then(|n| u32::try_from(n).ok()),
        detail: report.failed.clone(),
        outputs: report.outputs.clone(),
    };
    if let Err(e) = fixture_store::append_run(root, org, project, &f.id, run) {
        applog::warn(format!("fixture {}: the run could not be added to its history: {e}", f.id));
    }
}

/// Why a saved fixture did not run at all.
#[derive(Debug, Clone, PartialEq)]
pub enum NotRun {
    /// Another template or fixture run holds the slot.
    Busy,
    /// No such fixture, or its file does not read.
    Refused(String),
}

impl std::fmt::Display for NotRun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotRun::Busy => write!(f, "{API_TEMPLATE_BUSY}"),
            NotRun::Refused(why) => write!(f, "{why}"),
        }
    }
}

/// Runs the saved fixture `id` - Run and Rebuild alike - holding the
/// one-at-a-time template slot for the whole run, so no other template run
/// and no environment switch can come between its steps.
pub async fn run_saved<B: Browsers>(
    browsers: &mut B,
    root: &Path,
    org: &str,
    project: &str,
    id: &str,
    timing: &Timing,
) -> Result<FixtureReport, NotRun> {
    let Some(_claim) = claim() else {
        return Err(NotRun::Busy);
    };
    let f = fixture_store::load(root, org, project, id)
        .map_err(NotRun::Refused)?
        .ok_or_else(|| NotRun::Refused(no_such_fixture(id)))?;
    Ok(run_fixture(browsers, root, org, project, &f, timing).await)
}
