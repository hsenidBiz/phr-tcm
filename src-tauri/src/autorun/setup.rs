//! Scripts that use fixtures: the design doc's section 2.
//!
//! A step's values may hold two kinds of placeholder:
//!
//! - `{{fixture.<id>.<output>}}`: a shared draft, the fixture's current
//!   output (its newest successful run), read when the case starts;
//! - `{{setup.<output>}}`: the case's own draft, an output of the run of
//!   the script's `setup` fixture, made fresh on every run of the case.
//!
//! Every save path checks the names (`check_saved`, by way of
//! `nav::check_project_rules`). When a case starts - unattended,
//! supervised, or replayed to a step - `prepare_case` runs after its
//! preconditions and before its browser signs in: the setup's approval,
//! the shared fixtures' outputs, the setup's fixture run (in a browser of
//! its own, closed and its lease released before it returns), then the
//! values put into a COPY of the script. The saved script is never
//! changed. A case that cannot start is Blocked with one sentence, and
//! nothing of it signs in.
//!
//! No sentence here carries a host, a query string or a secret: only
//! fixture ids and names, output names and the runner's own sentences.

use super::approvals::{self, Approval};
use super::replay::Browsers;
use super::{CaseScript, Setup, StepScript};
use crate::api_templates::fixture::Fixture;
use crate::api_templates::fixture_run::{self, no_such_fixture, run_fixture_for, Clock};
use crate::api_templates::runner::{claim, API_TEMPLATE_BUSY, RETRY_PAUSES, RUN_LIMIT};
use crate::api_templates::{exec, fixture_store, store as template_store, ApiTemplate};
use crate::browser::timing::Timing;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// The Blocked sentence for a setup that is not approved, or whose
/// fingerprint no longer matches.
pub const NOT_APPROVED: &str = "setup not approved - approve it in the script editor";

/// The Blocked sentence for a shared fixture with no successful run.
pub fn not_built(name: &str) -> String {
    format!("fixture {name} has not been built - run it from API Templates, Fixtures")
}

/// The Blocked sentence for a setup whose fixture run failed: the
/// fixture's own failure sentence after it.
pub fn setup_failed(why: &str) -> String {
    format!("setup failed: {why}")
}

/// The Blocked sentence for a step that still holds a `{{setup.<output>}}`
/// once the setup has run: its fixture gave no such output.
pub fn setup_gave_none(output: &str) -> String {
    format!("setup gave no {output} - check its fixture's outputs in API Templates, Fixtures")
}

/// The Blocked sentence for a supervised step that still holds a
/// `{{setup.<output>}}`: what the setup gave at the case's start is not
/// kept (the start was Blocked, or someone else signed in since).
pub fn start_again(output: &str) -> String {
    format!("setup gave no {output} - start the case again")
}

/// Said when the setup changed between the person seeing it and pressing
/// Approve setup: nothing is approved.
pub const CHANGED_WHILE_LOOKING: &str =
    "the setup changed while you were looking at it - review it again before approving";

/// Save-time: a setup naming a fixture the project does not have.
pub fn no_fixture(id: &str) -> String {
    format!("setup: there is no fixture {id}")
}

/// Save-time: a `{{setup.<output>}}` the setup's fixture has no output for.
pub fn no_setup_output(name: &str, output: &str) -> String {
    format!("{{{{setup.{output}}}}}: fixture {name} has no output {output}")
}

/// Save-time: a `{{setup.<output>}}` in a script with no setup.
pub fn no_setup(output: &str) -> String {
    format!("{{{{setup.{output}}}}}: this script has no setup")
}

/// Save-time: a `{{fixture.<id>.<output>}}` the project does not have.
pub fn no_fixture_output(id: &str, output: &str) -> String {
    format!("{{{{fixture.{id}.{output}}}}}: there is no such fixture output")
}

/// One placeholder this module owns, by its parts.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Ref {
    Fixture { id: String, output: String },
    Setup { output: String },
}

/// The placeholder `name` (inside the braces, trimmed) as one of ours.
fn parse(name: &str) -> Option<Ref> {
    if let Some(rest) = name.strip_prefix("fixture.") {
        let (id, output) = rest.split_once('.').unwrap_or((rest, ""));
        return Some(Ref::Fixture { id: id.to_string(), output: output.to_string() });
    }
    name.strip_prefix("setup.").map(|output| Ref::Setup { output: output.to_string() })
}

/// Every string value in `v`, keys left out.
fn strings<'a>(v: &'a Value, out: &mut Vec<&'a str>) {
    match v {
        Value::String(s) => out.push(s),
        Value::Array(items) => items.iter().for_each(|i| strings(i, out)),
        Value::Object(map) => map.values().for_each(|i| strings(i, out)),
        _ => {}
    }
}

/// The steps as JSON: every value an action carries is a string in it.
fn steps_value(steps: &[StepScript]) -> Value {
    serde_json::to_value(steps).unwrap_or(Value::Null)
}

/// Every one of our placeholders in `steps`, in order, each once.
fn refs(steps: &[StepScript]) -> Vec<Ref> {
    let v = steps_value(steps);
    let mut texts = Vec::new();
    strings(&v, &mut texts);
    let mut seen = BTreeSet::new();
    texts
        .into_iter()
        .flat_map(exec::placeholders)
        .filter_map(|n| parse(&n))
        .filter(|r| seen.insert(r.clone()))
        .collect()
}

/// Whether a script uses a fixture at all: a setup, or a placeholder.
pub fn uses_fixtures(script: &CaseScript) -> bool {
    script.setup.is_some() || !refs(&script.steps).is_empty()
}

/// Every problem with one script's fixture names against the project's
/// fixtures, one sentence each, in order: the setup first, then each
/// placeholder. `Ok` is fit to save.
pub fn check_saved(script: &CaseScript, fixtures: &[Fixture]) -> Result<(), Vec<String>> {
    let mut problems = Vec::new();
    let find = |id: &str| fixtures.iter().find(|f| f.id == id);
    // `None`: no setup. `Some(None)`: a setup naming no fixture.
    let setup = script.setup.as_ref().map(|s| find(&s.fixture));
    if let (Some(s), Some(None)) = (&script.setup, &setup) {
        problems.push(no_fixture(&s.fixture));
    }
    for r in refs(&script.steps) {
        match r {
            Ref::Fixture { id, output } => {
                if !find(&id).is_some_and(|f| f.outputs.contains_key(&output)) {
                    problems.push(no_fixture_output(&id, &output));
                }
            }
            Ref::Setup { output } => match setup {
                None => problems.push(no_setup(&output)),
                // Said once, for the setup itself.
                Some(None) => {}
                Some(Some(f)) if !f.outputs.contains_key(&output) => problems.push(no_setup_output(&f.name, &output)),
                Some(Some(_)) => {}
            },
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// The save-time check every save path makes, for every script. The
/// project's fixtures are read only when a script uses one. A refusal
/// names the case, then every problem.
pub fn check_saved_all(root: &Path, org: &str, project: &str, scripts: &[CaseScript]) -> Result<(), String> {
    if !scripts.iter().any(uses_fixtures) {
        return Ok(());
    }
    let fixtures: Vec<Fixture> = fixture_store::list(root, org, project)
        .map_err(|e| {
            crate::applog::warn(format!("Auto Run setup: the fixtures could not be read: {e}"));
            "the fixtures could not be read to check the script - see Settings, Logs".to_string()
        })?
        .into_iter()
        .map(|s| s.fixture)
        .collect();
    let refused: Vec<String> = scripts
        .iter()
        .filter_map(|s| check_saved(s, &fixtures).err().map(|p| format!("case {}: {}", s.case_id, p.join("; "))))
        .collect();
    if refused.is_empty() {
        Ok(())
    } else {
        Err(refused.join("; "))
    }
}

/// A shared fixture as a case start sees it: its name, for the Blocked
/// sentence, and its current outputs - `None` when it has never run
/// successfully.
#[derive(Debug, Clone, PartialEq)]
pub struct FixtureValues {
    pub name: String,
    pub outputs: Option<BTreeMap<String, Value>>,
}

/// The shared fixtures `steps` name, by id, as they are now. A fixture that
/// is not saved (or does not read) is named by its id and has no outputs.
pub fn fixture_values(root: &Path, org: &str, project: &str, steps: &[StepScript]) -> BTreeMap<String, FixtureValues> {
    let mut out = BTreeMap::new();
    for r in refs(steps) {
        let Ref::Fixture { id, .. } = r else { continue };
        if out.contains_key(&id) {
            continue;
        }
        let name = fixture_store::load(root, org, project, &id).ok().flatten().map(|f| f.name);
        let outputs = name.as_ref().and_then(|_| fixture_store::current_outputs(root, org, project, &id));
        out.insert(id.clone(), FixtureValues { name: name.unwrap_or(id), outputs });
    }
    out
}

/// `steps` with every string value's placeholders that `vars` names filled
/// in, as text; everything else as it was.
fn substitute_steps(steps: &[StepScript], vars: &BTreeMap<String, Value>) -> Result<Vec<StepScript>, String> {
    fn walk(v: &Value, vars: &BTreeMap<String, Value>) -> Value {
        match v {
            Value::String(s) => Value::String(exec::substitute_str(s, vars)),
            Value::Array(items) => Value::Array(items.iter().map(|i| walk(i, vars)).collect()),
            Value::Object(map) => Value::Object(map.iter().map(|(k, i)| (k.clone(), walk(i, vars))).collect()),
            other => other.clone(),
        }
    }
    if vars.is_empty() {
        return Ok(steps.to_vec());
    }
    serde_json::from_value(walk(&steps_value(steps), vars)).map_err(|e| {
        crate::applog::warn(format!("Auto Run setup: the filled-in steps did not read back: {e}"));
        "the script's fixture values could not be filled in - see Settings, Logs".to_string()
    })
}

/// The values `fixtures` give, under `fixture.<id>.<output>`.
fn fixture_vars(fixtures: &BTreeMap<String, FixtureValues>) -> BTreeMap<String, Value> {
    let mut vars = BTreeMap::new();
    for (id, f) in fixtures {
        for (output, v) in f.outputs.iter().flatten() {
            vars.insert(format!("fixture.{id}.{output}"), v.clone());
        }
    }
    vars
}

/// The Blocked sentence for the first of our placeholders still in
/// `steps`, if any. `setup`: the sentence for a `{{setup.` left over;
/// `None` before the setup has run, when only a `{{fixture.` counts.
fn leftover(
    steps: &[StepScript],
    fixtures: &BTreeMap<String, FixtureValues>,
    setup: Option<fn(&str) -> String>,
) -> Option<String> {
    refs(steps).into_iter().find_map(|r| match r {
        Ref::Fixture { id, .. } => Some(not_built(fixtures.get(&id).map_or(id.as_str(), |f| f.name.as_str()))),
        Ref::Setup { output } => setup.map(|say| say(&output)),
    })
}

/// A copy of `script` with every `{{fixture.<id>.<output>}}` replaced by
/// that fixture's current output and every `{{setup.<output>}}` by the
/// setup run's, in every string value its steps carry. `Err` is the
/// Blocked sentence for a placeholder left with no value: `not_built` for
/// a fixture, `setup_gave_none` for the setup.
pub fn resolve(
    script: &CaseScript,
    fixtures: &BTreeMap<String, FixtureValues>,
    setup_outputs: &BTreeMap<String, Value>,
) -> Result<CaseScript, String> {
    let mut vars = fixture_vars(fixtures);
    for (output, v) in setup_outputs {
        vars.insert(format!("setup.{output}"), v.clone());
    }
    let steps = substitute_steps(&script.steps, &vars)?;
    if let Some(why) = leftover(&steps, fixtures, Some(setup_gave_none)) {
        return Err(why);
    }
    Ok(CaseScript { steps, ..script.clone() })
}

/// A setup's fixture as it is saved now, with its fingerprint.
pub struct Current {
    pub fixture: Fixture,
    /// One per fixture step, `None` for a template that is not saved.
    pub templates: Vec<Option<ApiTemplate>>,
    pub fingerprint: String,
}

/// The setup's fixture and its step templates as saved now, and their
/// fingerprint. `Err` is the sentence for a fixture that is not saved or
/// does not read.
pub fn current(root: &Path, org: &str, project: &str, setup: &Setup) -> Result<Current, String> {
    let fixture = fixture_store::load(root, org, project, &setup.fixture)?.ok_or_else(|| no_such_fixture(&setup.fixture))?;
    let templates: Vec<Option<ApiTemplate>> = fixture
        .steps
        .iter()
        .map(|s| template_store::load(root, org, project, &s.template).ok().flatten())
        .collect();
    let fingerprint = approvals::fingerprint(setup, &fixture, &templates);
    Ok(Current { fixture, templates, fingerprint })
}

/// What a case start hands on: its script with every fixture value filled
/// in, and what its setup gave (empty with no setup).
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    pub script: CaseScript,
    pub setup_outputs: BTreeMap<String, Value>,
}

/// `prepare_case_within` with the template runner's own limits and
/// nothing to do just before the setup runs.
pub async fn prepare_case<B: Browsers>(
    browsers: &mut B,
    root: &Path,
    org: &str,
    project: &str,
    script: &CaseScript,
    timing: &Timing,
    cancel: &AtomicBool,
) -> Result<Prepared, String> {
    prepare_case_hooked(browsers, root, org, project, script, timing, cancel, |_| std::future::ready(())).await
}

/// `prepare_case_within` with the template runner's own limits.
#[allow(clippy::too_many_arguments)]
pub async fn prepare_case_hooked<B: Browsers, H, Fut>(
    browsers: &mut B,
    root: &Path,
    org: &str,
    project: &str,
    script: &CaseScript,
    timing: &Timing,
    cancel: &AtomicBool,
    before_run: H,
) -> Result<Prepared, String>
where
    H: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    prepare_case_within(
        browsers,
        root,
        org,
        project,
        script,
        timing,
        cancel,
        before_run,
        RUN_LIMIT,
        &RETRY_PAUSES,
        Clock::now_local(),
    )
    .await
}

/// Everything a case needs from fixtures before it signs in, in order:
///
/// 1. its setup's approval: not approved, or approved for a fingerprint
///    that no longer matches, is `NOT_APPROVED`, and nothing opens;
/// 2. its shared fixtures' current outputs: one never built is
///    `not_built`, and nothing opens;
/// 3. its setup's fixture run, in a browser from `browsers` holding the
///    one-at-a-time template slot: what it makes is recorded as test-made
///    with this case's id, and its browser is closed and its lease
///    released before this returns. A failed run is `setup_failed`, and
///    one `cancel` stopped between its steps is
///    `setup failed: the run was stopped`;
/// 4. the values into a copy of the script (`resolve`).
///
/// `before_run` is called with the fixture's account once every check that
/// can still Block has passed (the approval, the shared fixtures, the
/// one-at-a-time slot and the run's own checks before it signs in), just
/// before the setup's browser opens: where a supervised browser makes way
/// (`make_way`). It is never called for a case that will not run its setup.
///
/// A script that uses no fixture comes back as it is, and nothing is read.
#[allow(clippy::too_many_arguments)]
pub async fn prepare_case_within<B: Browsers, H, Fut>(
    browsers: &mut B,
    root: &Path,
    org: &str,
    project: &str,
    script: &CaseScript,
    timing: &Timing,
    cancel: &AtomicBool,
    before_run: H,
    limit: Duration,
    retry_pauses: &[Duration],
    clock: Clock,
) -> Result<Prepared, String>
where
    H: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    if !uses_fixtures(script) {
        return Ok(Prepared { script: script.clone(), setup_outputs: BTreeMap::new() });
    }
    let id = script.case_id;

    let setup = match &script.setup {
        None => None,
        Some(s) => {
            let now = current(root, org, project, s).map_err(|why| setup_failed(&why))?;
            match approvals::state(root, id, &now.fingerprint) {
                Approval::Approved { .. } => Some(now),
                other => {
                    crate::applog::info(format!("case {id}: setup is {}, so the case is Blocked", other.word()));
                    return Err(NOT_APPROVED.to_string());
                }
            }
        }
    };

    let fixtures = fixture_values(root, org, project, &script.steps);
    let shared = substitute_steps(&script.steps, &fixture_vars(&fixtures))?;
    if let Some(why) = leftover(&shared, &fixtures, None) {
        return Err(why);
    }

    let mut setup_outputs = BTreeMap::new();
    if let Some(now) = setup {
        let Some(_claim) = claim() else {
            return Err(setup_failed(API_TEMPLATE_BUSY));
        };
        // The templates that were fingerprinted, never read again: what
        // runs is exactly what the person approved.
        let f = &now.fixture;
        fixture_run::ready_to_run(root, org, project, f, Some(&now.templates)).map_err(|why| setup_failed(&why))?;
        if cancel.load(Ordering::SeqCst) {
            return Err(setup_failed(fixture_run::RUN_STOPPED));
        }
        before_run(f.account.clone()).await;
        let report = run_fixture_for(
            browsers,
            root,
            org,
            project,
            f,
            timing,
            limit,
            retry_pauses,
            clock,
            Some(id),
            Some(&now.templates),
            Some(cancel),
        )
        .await;
        if !report.ok {
            crate::applog::info(format!("case {id}: setup fixture {} failed, {} made", f.id, report.made.len()));
            return Err(setup_failed(&report.message()));
        }
        crate::applog::info(format!("case {id}: setup fixture {} ran, {} made", f.id, report.made.len()));
        setup_outputs = report.outputs;
    }

    let resolved = resolve(script, &fixtures, &setup_outputs)?;
    check_filled_inputs(root, org, project, script.area_name(), &script.steps, &[], &script.steps, &resolved.steps)?;
    Ok(Prepared { script: resolved, setup_outputs })
}

/// The run-time half of the seen check for every locator that held a data
/// placeholder when the script was saved, a step's own or one a component
/// input gives (`seen_check::check_resolved_inputs`): `saved` as saved,
/// `filled` with
/// the run's values in, checked in the areas of `area` and `area_steps`
/// (the script's own, and those its steps return to). `before` are the
/// steps that ran before `filled`, filled in: what they typed, picked and
/// uploaded is the script's own data, as at save. The map, the components
/// and the project's Test files are read only when a saved step holds such
/// a locator. `Err` is the Blocked sentence, naming the step.
#[allow(clippy::too_many_arguments)]
fn check_filled_inputs(
    root: &Path,
    org: &str,
    project: &str,
    area: Option<&str>,
    area_steps: &[StepScript],
    before: &[StepScript],
    saved: &[StepScript],
    filled: &[StepScript],
) -> Result<(), String> {
    use super::seen_check;
    if !seen_check::has_data_placeholders(saved) {
        return Ok(());
    }
    let map = super::discovery_map::load_map(root, org, project)?;
    let components = super::components::load_components(root, org, project)?;
    let areas = seen_check::script_areas(&components, area, area_steps);
    let areas: Vec<&str> = areas.iter().map(String::as_str).collect();
    // One that cannot be read is logged by `list` and reads as none: then
    // no file name or size is the script's own.
    let files = crate::test_files::list(&crate::test_files::folder(root, org, project)).unwrap_or_default();
    seen_check::check_resolved_inputs_with(&map, &components, &areas, before, saved, filled, &files)
}

/// Keeps what case `case_id`'s setup gave at its supervised start (or its
/// replay to a step), replacing what an earlier start gave, for the steps
/// a person then runs one at a time. Memory only, in the cache's session
/// tier: signing in as someone else clears it, and the case is started
/// again.
pub fn remember(case_id: i32, outputs: BTreeMap<String, Value>) {
    crate::cache::session_put(&crate::cache::keys::setup_outputs(case_id), outputs);
}

/// Lets go of what case `case_id`'s setup gave at an earlier supervised
/// start: a start that ends Blocked or in an error leaves nothing stale.
pub fn forget(case_id: i32) {
    remember(case_id, BTreeMap::new());
}

/// What case `case_id`'s setup gave at its last supervised start, if it is
/// still kept.
fn remembered(case_id: i32) -> Option<BTreeMap<String, Value>> {
    crate::cache::session_fresh(&crate::cache::keys::setup_outputs(case_id), crate::cache::keys::SETUP_OUTPUTS_TTL)
}

/// One supervised step with its fixture values filled in: shared fixtures'
/// current outputs, and what the case's setup gave at its start
/// (`remember`). `Err` is the Blocked sentence for a value still missing.
/// A step with none of our placeholders comes back as it is.
pub fn resolve_step(root: &Path, org: &str, project: &str, case_id: i32, step: &StepScript) -> Result<StepScript, String> {
    let one = std::slice::from_ref(step);
    let filled = if refs(one).is_empty() {
        step.clone()
    } else {
        let (steps, fixtures) = fill_supervised(root, org, project, case_id, one)?;
        if let Some(why) = leftover(&steps, &fixtures, Some(start_again)) {
            return Err(why);
        }
        steps.into_iter().next().unwrap_or_else(|| step.clone())
    };
    if super::seen_check::has_data_placeholders(one) {
        // The areas are the saved script's: its own and those it returns to.
        let saved = super::store::load_script(root, case_id).ok().flatten();
        let area = saved.as_ref().and_then(|s| s.area_name());
        let area_steps = saved.as_ref().map_or(one, |s| s.steps.as_slice());
        // The steps before this one, filled in the same way, for what they
        // typed, picked and uploaded; as saved where a value is missing.
        let earlier: Vec<StepScript> =
            area_steps.iter().filter(|s| s.step_number < step.step_number).cloned().collect();
        let before = fill_supervised(root, org, project, case_id, &earlier).map_or(earlier, |(steps, _)| steps);
        check_filled_inputs(root, org, project, area, area_steps, &before, one, std::slice::from_ref(&filled))?;
    }
    Ok(filled)
}

/// `steps` with shared fixtures' current outputs and what case `case_id`'s
/// setup gave at its supervised start (`remember`) put in, and the
/// fixtures they name. A value still missing is left as written.
fn fill_supervised(
    root: &Path,
    org: &str,
    project: &str,
    case_id: i32,
    steps: &[StepScript],
) -> Result<(Vec<StepScript>, BTreeMap<String, FixtureValues>), String> {
    let fixtures = fixture_values(root, org, project, steps);
    let mut vars = fixture_vars(&fixtures);
    if let Some(given) = remembered(case_id) {
        for (output, v) in given {
            vars.insert(format!("setup.{output}"), v.clone());
        }
    }
    Ok((substitute_steps(steps, &vars)?, fixtures))
}

/// Said when a setup is asked to run where no browser is given for it.
pub const NO_SETUP_BROWSER: &str = "no browser is given for a setup here";

/// Browsers for a caller with none to give a setup: every open is refused
/// (`NO_SETUP_BROWSER`), so an approved setup fails there and its case is
/// Blocked rather than run without its draft.
pub struct NoBrowsers<D>(std::marker::PhantomData<D>);

impl<D> Default for NoBrowsers<D> {
    fn default() -> Self {
        NoBrowsers(std::marker::PhantomData)
    }
}

impl<D: crate::browser::cdp::Driver> Browsers for NoBrowsers<D> {
    type D = D;

    async fn open(&mut self) -> Result<D, String> {
        Err(NO_SETUP_BROWSER.to_string())
    }

    async fn close(&mut self, _d: D) {}
}

/// A case's setup as the script editor shows it, with where its approval
/// stands.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct SetupView {
    pub fixture_name: String,
    /// The Auto Run account the fixture runs as (a key, never a login).
    pub account: String,
    /// Each step as `<template name>: <params>`.
    pub steps: Vec<String>,
    /// What it makes, each as `<kind> <name>`.
    pub creates: Vec<String>,
    /// `approved`, `changed` (approved once, changed since) or `none`.
    pub approval: String,
    /// When it was approved, while `approval` is `approved`.
    pub approved_at: Option<String>,
    /// What Approve setup signs: it approves only while the setup still
    /// has this fingerprint (`approval_target`).
    pub fingerprint: String,
}

/// One fixture step as the editor shows it: its template's title (or its
/// id, for a template that is not saved) and its params as written.
fn step_line(step: &crate::api_templates::fixture::FixtureStep, t: Option<&ApiTemplate>) -> String {
    let name = t.map_or(step.template.as_str(), |t| t.title.as_str());
    let params: Vec<String> = step.params.iter().map(|(k, v)| format!("{k} = {v}")).collect();
    if params.is_empty() {
        format!("{name}: no params")
    } else {
        format!("{name}: {}", params.join(", "))
    }
}

/// The editor's view of `script`'s setup, or `None` when it has none.
/// `Err` is the sentence for a setup whose fixture is not saved.
pub fn view(root: &Path, org: &str, project: &str, script: &CaseScript) -> Result<Option<SetupView>, String> {
    let Some(s) = &script.setup else { return Ok(None) };
    let now = current(root, org, project, s)?;
    let approval = approvals::state(root, script.case_id, &now.fingerprint);
    let f = &now.fixture;
    Ok(Some(SetupView {
        fixture_name: f.name.clone(),
        account: f.account.clone(),
        steps: f.steps.iter().zip(&now.templates).map(|(step, t)| step_line(step, t.as_ref())).collect(),
        creates: f.creates.iter().map(|c| format!("{} {}", c.kind, c.name)).collect(),
        approval: approval.word().to_string(),
        approved_at: match approval {
            Approval::Approved { at } => Some(at),
            _ => None,
        },
        fingerprint: now.fingerprint.clone(),
    }))
}

/// The fingerprint Approve setup may record for `script`: the setup's as
/// saved now, and only when it is the one the person was shown
/// (`expected`, from `SetupView::fingerprint`). Otherwise
/// `CHANGED_WHILE_LOOKING`, and nothing may be approved.
pub fn approval_target(root: &Path, org: &str, project: &str, script: &CaseScript, expected: &str) -> Result<String, String> {
    let Some(s) = &script.setup else { return Err(format!("case {}'s script has no setup", script.case_id)) };
    let now = current(root, org, project, s)?;
    if now.fingerprint != expected {
        return Err(CHANGED_WHILE_LOOKING.to_string());
    }
    Ok(now.fingerprint)
}

/// Before a supervised setup signs in as `key`: when the supervised browser
/// is signed in as that account, its session is ended (the
/// `expire_session` action) and its lease let go, so the setup can sign in
/// as it. PeoplesHR keeps one session per user, so the setup's sign-in
/// would end it anyway. The case's own sign-in follows as usual. A browser
/// signed in as another account is left alone. Called only from
/// `prepare_case`'s `before_run`, once the setup will run.
pub async fn make_way<D: crate::browser::cdp::Driver>(
    d: &mut D,
    account: &mut Option<String>,
    lease: &mut super::lease::Held,
    key: &str,
    timing: &Timing,
) {
    if lease.account() != Some(key) && account.as_deref() != Some(key) {
        return;
    }
    let ended = crate::browser::actions::execute_with(d, &crate::browser::actions::Action::ExpireSession, timing).await;
    if !ended.ok {
        crate::applog::info(format!(
            "the Auto Run browser's session as {key} could not be ended before a setup; its lease is let go all the same"
        ));
    }
    *account = None;
    lease.let_go();
    crate::applog::info(format!("the Auto Run browser let go of {key} for a setup"));
}

/// The stop for a supervised start's setup: set by Close (`stop`), heard
/// between the setup's template steps. Each start clears it.
pub static CANCEL: AtomicBool = AtomicBool::new(false);

/// Ask a supervised start's setup that is running, if any, to stop.
pub fn stop() {
    CANCEL.store(true, Ordering::SeqCst);
}

/// The supervised start's part: case `case_id`'s fixtures prepared as an
/// unattended case's are, with what its setup gave kept for its steps
/// (`remember`); what an earlier start gave is let go first (`forget`).
/// `before_run` is `prepare_case`'s: where the supervised browser makes
/// way (`make_way`), only once the setup will run. Close stops it between
/// template steps (`stop`). `Some` is the Blocked sentence. A case with no
/// script has nothing to prepare.
#[allow(clippy::too_many_arguments)]
pub async fn check_supervised<B: Browsers, H, Fut>(
    browsers: &mut B,
    root: &Path,
    org: &str,
    project: &str,
    case_id: i32,
    timing: &Timing,
    before_run: H,
    limit: Duration,
    retry_pauses: &[Duration],
) -> Option<String>
where
    H: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    forget(case_id);
    CANCEL.store(false, Ordering::SeqCst);
    let script = match super::store::load_script(root, case_id) {
        Ok(Some(s)) => s,
        Ok(None) => return None,
        Err(why) => return Some(why),
    };
    let prepared = prepare_case_within(
        browsers,
        root,
        org,
        project,
        &script,
        timing,
        &CANCEL,
        before_run,
        limit,
        retry_pauses,
        Clock::now_local(),
    )
    .await;
    match prepared {
        Ok(prepared) => {
            remember(case_id, prepared.setup_outputs);
            None
        }
        Err(why) => Some(why),
    }
}
