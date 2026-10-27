//! `run_setup`: a saved setup finds an existing test record or builds a new
//! one in the explore session that asked, and whatever it builds goes on that
//! session's change list.
//!
//! The find runs first and decides everything: a record found is handed back
//! and nothing else runs; a find that could not run builds nothing; only a
//! find that ran and came back empty lets the build start. The build drives
//! the caller's own browser through `ai_bridge::discover_action_in`, one
//! action at a time, so a setup never signs in, never types a login and never
//! talks to Azure DevOps. A record is confirmed only by the save it made: its
//! id must sit in that save's answer where it sat when the setup was saved.

use super::components::{self, Component, ComponentInput, InputKind};
use super::explore::{self as ex, Change};
use super::nav;
use super::sessions;
use super::setup_find::{
    browser_with_prefix, choose, find_in_database, starts_with_prefix, DbOutcome, FindChoice, FoundRecord, RowSource,
    DATABASE_OFF, NEEDS_A_FIND_RULE,
};
use super::setups::{fresh_value, find_setup, load_setups, no_such_setup, stamp_used, SavedSetup, SetupCreate};
use crate::ai_bridge::{
    discover_action_in, discovery_must_not_save, refuse_as_a_tried_action, refuse_outside_the_recipe, went_home,
    DiscoveryBrowser,
};
use crate::browser::actions::Action;
use crate::browser::cdp::Driver;
use crate::browser::locator::{self, Target};
use crate::browser::net_capture::{Body, Captured};
use crate::browser::page;
use crate::browser::timing::Timing;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

pub const FOUND: &str = "Found an existing record";
pub const BUILT: &str = "Built a new record";
pub const BUILD_NOT_SAVED: &str =
    "No existing record was found, and this explore session must not save, so nothing was built.";

/// The longest name a browser find rule reads from the page.
const MATCHED_TEXT_MAX: usize = 200;

/// `this` is the element a find rule's last check matched: its text.
pub const MATCHED_TEXT_JS: &str =
    "function() { return this.innerText === undefined ? (this.textContent || '') : this.innerText; }";

pub fn find_could_not_run(why: &str) -> String {
    format!("The find rule could not run, so nothing was built: {why}")
}

pub fn action_failed(n: usize, what: &str, detail: &str) -> String {
    format!(
        "Action {n} ({what}) failed: {detail}. The setup may need saving again; what it created so far is on the change list."
    )
}

pub fn not_confirmed(request: &str) -> String {
    format!(
        "The record was built, but it could not be confirmed: the save {request} did not answer with an id where it did when the setup was saved. Record it with record_explore_change."
    )
}

pub fn not_prefixed() -> String {
    format!(
        "The record was built, but it is not named with the test prefix. {} The setup needs saving again; check the change list.",
        super::setups::ONLY_TEST_BUILDS
    )
}

/// What `run_setup` is asked: the setup's name and the values for its inputs.
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct RunAsked {
    name: String,
    #[serde(default)]
    values: Map<String, Value>,
}

/// Every input's value: the one given (text or a number), else a fresh one
/// for a `unique` input, else a problem. Every problem is collected, so the
/// assistant hears them all at once. A `unique` or `names_the_record` value
/// must start with the test prefix.
pub fn values_for(
    setup: &SavedSetup,
    given: &Map<String, Value>,
    prefix: &str,
    now_ms: u64,
) -> Result<BTreeMap<String, String>, Vec<String>> {
    let mut bad: Vec<String> = Vec::new();
    for key in given.keys() {
        if !setup.inputs.iter().any(|i| i.name == *key) {
            bad.push(format!("{} has no input {key}.", setup.name));
        }
    }
    let mut out = BTreeMap::new();
    for input in &setup.inputs {
        let value = match given.get(&input.name).filter(|v| !v.is_null()) {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            Some(_) => {
                bad.push(format!("{} must be text or a number.", input.name));
                continue;
            }
            None if input.unique => fresh_value(prefix, &setup.name, now_ms),
            None => String::new(),
        };
        if value.trim().is_empty() {
            bad.push(format!("{} needs a value.", input.name));
            continue;
        }
        if (input.unique || input.names_the_record) && !starts_with_prefix(&value, prefix) {
            bad.push(format!("{} must start with the test prefix {prefix}.", input.name));
            continue;
        }
        out.insert(input.name.clone(), value);
    }
    if bad.is_empty() {
        Ok(out)
    } else {
        Err(bad)
    }
}

/// Per create, in order: the id its save answered with. The save is the first
/// capture, started at or after `floor_ms` and not taken by an earlier create,
/// that is the create's request, answered 2xx with JSON that holds an id at
/// the create's `id_at`.
pub fn ids_of_built(captured: &[&Captured], floor_ms: u64, creates: &[SetupCreate]) -> Vec<Option<String>> {
    let mut since: Vec<&Captured> = captured.iter().copied().filter(|c| c.started_ms >= floor_ms).collect();
    since.sort_by_key(|c| (c.started_ms, c.id));
    let mut used = vec![false; since.len()];
    creates
        .iter()
        .map(|create| {
            for (i, c) in since.iter().enumerate() {
                if used[i] || !ex::same_save(&ex::request_line(&c.method, &c.url_path_query), &create.request) {
                    continue;
                }
                if !c.status.is_some_and(|s| (200..300).contains(&s)) {
                    continue;
                }
                let Some(Body::Text(text)) = &c.resp_body else { continue };
                let Ok(answer) = serde_json::from_str::<Value>(text) else { continue };
                if let Some(id) = ex::id_at(&answer, &create.id_at).filter(|id| !id.is_empty()) {
                    used[i] = true;
                    return Some(id);
                }
            }
            None
        })
        .collect()
}

/// A record the build made, as the change list holds it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BuiltCreate {
    pub entry: u32,
    pub kind: String,
    pub id: String,
    pub name: String,
    pub verified: bool,
}

/// `template` with each `{{input}}` put in.
fn filled(template: &str, values: &BTreeMap<String, String>) -> String {
    values.iter().fold(template.to_string(), |text, (k, v)| text.replace(&format!("{{{{{k}}}}}"), v))
}

/// Puts what the build created on the change list. Each id found goes through
/// `explore::record_change`, so verification, the proof and the test-made
/// list are the change list's own. Hands back what was recorded and the
/// request of every create that could not be confirmed: no save answered
/// with an id where the setup says, or the change list did not verify it.
pub fn record_built(
    root: &Path,
    session: &str,
    setup: &SavedSetup,
    values: &BTreeMap<String, String>,
    captured: &[&Captured],
    floor_ms: u64,
) -> (Vec<BuiltCreate>, Vec<String>) {
    let ids = ids_of_built(captured, floor_ms, &setup.creates);
    let (mut built, mut unconfirmed) = (Vec::new(), Vec::new());
    for (create, id) in setup.creates.iter().zip(ids) {
        let Some(id) = id else {
            unconfirmed.push(create.request.clone());
            continue;
        };
        let name = filled(&create.name, values);
        let change = Change::Created { kind: create.kind.clone(), id: id.clone(), name: name.clone(), verified: false };
        match ex::record_change(root, session, change, Some(create.request.clone()), captured, floor_ms) {
            Ok(recorded) => {
                let verified = recorded.verified == Some(true);
                if !verified {
                    unconfirmed.push(create.request.clone());
                }
                built.push(BuiltCreate { entry: recorded.n, kind: create.kind.clone(), id, name, verified });
            }
            Err(why) => {
                crate::applog::warn(format!("Auto Run setup {}: a built record could not be put on the change list: {why}", setup.name));
                unconfirmed.push(create.request.clone());
            }
        }
    }
    (built, unconfirmed)
}

/// The first line of the text of the first element `target` matches, trimmed
/// and cut to 200 characters; `None` when nothing matches or it has no text.
pub async fn matched_text<D: Driver>(d: &mut D, target: &Target) -> Option<String> {
    let handles = locator::resolve(d, target).await.ok()?;
    let first = handles.first()?;
    let text = page::call_value(d, first, MATCHED_TEXT_JS, &[]).await.ok()?;
    let line = text.as_str()?.trim().lines().next()?.trim();
    (!line.is_empty()).then(|| line.chars().take(MATCHED_TEXT_MAX).collect())
}

/// What a find rule's last check points at.
fn check_target(a: &Action) -> Option<&Target> {
    match a {
        Action::ExpectVisible { selector, .. }
        | Action::ExpectText { selector, .. }
        | Action::ExpectContainsText { selector, .. } => Some(selector),
        _ => None,
    }
}

/// Why an action's answer from `discover_action_in` was not a pass; `None`
/// for a pass.
fn failure_of(answer: &(u16, String)) -> Option<String> {
    let said = |text: &str| text.trim().trim_end_matches('.').to_string();
    if answer.0 != 200 {
        return Some(said(&answer.1));
    }
    match serde_json::from_str::<Value>(&answer.1) {
        Ok(v) if v["ok"].as_bool() == Some(true) => None,
        Ok(v) => Some(said(v["detail"].as_str().unwrap_or("the action did not pass"))),
        Err(_) => Some("the browser's answer could not be read".to_string()),
    }
}

fn going<B: DiscoveryBrowser>(slot: &mut Option<B>) -> Option<String> {
    let parts = slot.as_mut()?.parts();
    parts.discovery.as_ref()?.explore_session().map(str::to_string)
}

fn must_not_save<B: DiscoveryBrowser>(slot: &mut Option<B>, root: &Path) -> bool {
    slot.as_mut().is_some_and(|b| b.parts().discovery.as_ref().is_some_and(|s| discovery_must_not_save(root, s)))
}

fn set_running<B: DiscoveryBrowser>(slot: &mut Option<B>, on: bool) {
    if let Some(run) = slot.as_mut().and_then(|b| b.parts().discovery.as_mut()).and_then(|s| s.explore.as_mut()) {
        run.setup_running = on;
    }
}

/// `record_built` over what the browser captured so far.
fn record_over<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &Path,
    session: &str,
    setup: &SavedSetup,
    values: &BTreeMap<String, String>,
    floor_ms: u64,
) -> (Vec<BuiltCreate>, Vec<String>, Vec<Option<String>>) {
    let Some(browser) = slot.as_mut() else {
        return (Vec::new(), setup.creates.iter().map(|c| c.request.clone()).collect(), vec![None; setup.creates.len()]);
    };
    let parts = browser.parts();
    let captured: Vec<&Captured> = Driver::capture(&*parts.driver).map(|c| c.list(0, usize::MAX).0).unwrap_or_default();
    let ids = ids_of_built(&captured, floor_ms, &setup.creates);
    let (built, unconfirmed) = record_built(root, session, setup, values, &captured, floor_ms);
    (built, unconfirmed, ids)
}

/// The trip to the setup's area, or home when it has none.
async fn trip<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &Path,
    org: &str,
    project: &str,
    setup: &SavedSetup,
    timing: &Timing,
) -> Result<(), String> {
    match setup.area.as_deref() {
        Some(area) => {
            let there = Action::ReturnToArea { area: Some(area.to_string()) };
            let answer = discover_action_in(slot, root, org, project, &there, None, None).await;
            failure_of(&answer).map_or(Ok(()), Err)
        }
        None => {
            let Some(browser) = slot.as_mut() else {
                return Err("no Auto Run browser is open".to_string());
            };
            let parts = browser.parts();
            if went_home(&mut *parts.driver, root, org, project, timing).await {
                Ok(())
            } else {
                Err("the browser could not go to the home page".to_string())
            }
        }
    }
}

/// A browser find rule: the trip, then its actions. `Err` when it could not
/// run; `Ok(None)` when it ran and its last check found no test record.
async fn find_in_browser<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    root: &Path,
    org: &str,
    project: &str,
    setup: &SavedSetup,
    prefix: &str,
    timing: &Timing,
) -> Result<Option<FoundRecord>, String> {
    let rule = setup.find.browser.as_deref().unwrap_or_default();
    let actions = browser_with_prefix(rule, prefix)?;
    let Some(last) = actions.len().checked_sub(1) else {
        return Err("the browser find rule has no actions".to_string());
    };
    trip(slot, root, org, project, setup, timing).await?;
    for (i, action) in actions.iter().enumerate() {
        let answer = discover_action_in(slot, root, org, project, action, None, None).await;
        let Some(why) = failure_of(&answer) else { continue };
        if i < last {
            return Err(why);
        }
        // Only a check that ran and reported a failure means "not found". Any
        // other answer (an error, a refusal, one that cannot be read) means
        // the check did not run, and nothing is built on that.
        let plain = answer.0 == 200
            && serde_json::from_str::<Value>(&answer.1).is_ok_and(|v| v["ok"].as_bool() == Some(false));
        return if plain { Ok(None) } else { Err(why) };
    }
    let target = actions.get(last).and_then(check_target).ok_or("the browser find rule does not end in a check")?;
    let Some(browser) = slot.as_mut() else {
        return Err("no Auto Run browser is open".to_string());
    };
    let parts = browser.parts();
    let Some(text) = matched_text(&mut *parts.driver, target).await else {
        return Err("the check passed, but the record's name could not be read from the page".to_string());
    };
    Ok(starts_with_prefix(&text, prefix).then_some(FoundRecord { id: None, name: text }))
}

/// Every action loaded from the setups file goes back through the refusals an
/// action an assistant tries meets (no sign-in, a valid action, http or https
/// only, only the sign-in recipe's sites, none of the fixes that are never
/// made), so a file edited by hand cannot make a run sign in or go anywhere.
fn recheck_actions(root: &Path, org: &str, project: &str, setup: &SavedSetup, prefix: &str) -> Result<(), String> {
    let recipe = crate::autorun::recipe::load_effective_recipe(root, org, project)?;
    let policy = crate::autorun::runner::policy_for(Some(&recipe));
    let mut all: Vec<&Action> = setup.actions.iter().collect();
    let find = match setup.find.browser.as_deref() {
        Some(rule) => browser_with_prefix(rule, prefix)?,
        None => Vec::new(),
    };
    all.extend(find.iter());
    for action in all {
        if action.each().iter().any(|a| matches!(a, Action::SignIn { .. })) {
            return Err("an action signs in".to_string());
        }
        refuse_as_a_tried_action(action).map_err(|(_, why)| why)?;
        // Its own refusal names the site; this one never does.
        refuse_outside_the_recipe(&policy, action)
            .map_err(|_| "an action goes to an address outside the sign-in recipe's sites".to_string())?;
        if let Some(why) = crate::autorun::forbidden_fixes::forbidden_in(&serde_json::to_value(action).unwrap_or(Value::Null)) {
            return Err(why);
        }
    }
    Ok(())
}

/// Clears the setup-running mark when the run ends, however it ends: a panic
/// in the run must not leave the session unable to keep actions.
struct Running<'a, B: DiscoveryBrowser> {
    slot: &'a mut Option<B>,
}

impl<'a, B: DiscoveryBrowser> Running<'a, B> {
    fn on(slot: &'a mut Option<B>) -> Self {
        set_running(slot, true);
        Running { slot }
    }
}

impl<B: DiscoveryBrowser> Drop for Running<'_, B> {
    fn drop(&mut self) {
        set_running(self.slot, false);
    }
}

fn stamp(root: &Path, org: &str, project: &str, setup: &SavedSetup, now: u64) {
    if let Err(why) = stamp_used(root, org, project, &setup.name, now) {
        crate::applog::warn(format!("Auto Run setup {}: its use could not be noted: {why}", setup.name));
    }
}

/// `run_setup` for the explore session in `slot`: `{ "name": <setup>,
/// "values": { <input>: <value> } }`. `rows` is the database when Database
/// Read Access is on, else `None`. See the module's notes for the order.
#[allow(clippy::too_many_arguments)]
pub async fn run_setup_in<B: DiscoveryBrowser, S: RowSource>(
    slot: &mut Option<B>,
    root: &Path,
    org: &str,
    project: &str,
    body: &str,
    rows: Option<&S>,
    timing: &Timing,
    now: u64,
) -> (u16, String) {
    let asked: RunAsked = match serde_json::from_str(body) {
        Ok(a) => a,
        Err(e) => return (400, e.to_string()),
    };
    let Some(session) = going(slot) else {
        return (409, ex::NO_EXPLORE_SESSION.to_string());
    };
    let file = match load_setups(root, org, project) {
        Ok(f) => f,
        Err(why) => return (409, why),
    };
    let Some(setup) = find_setup(&file, &asked.name).cloned() else {
        return (404, no_such_setup(&asked.name));
    };
    // Before the find or any action: a setup whose area is gone cannot make
    // its trip.
    if let Some(area) = setup.area.as_deref() {
        let known = nav::load_nav(root, org, project).is_ok_and(|n| nav::find_area(&n, area).is_some());
        if !known {
            return (409, nav::unrecorded_area(area));
        }
    }
    let prefix = crate::environments::active(root)
        .map(|e| e.test_prefix)
        .unwrap_or_else(|_| crate::environments::default_test_prefix());
    let values = match values_for(&setup, &asked.values, &prefix, now) {
        Ok(v) => v,
        Err(problems) => return (400, problems.join(" ")),
    };
    if let Err(why) = recheck_actions(root, org, project, &setup, &prefix) {
        return (409, format!("The setup needs saving again: {}.", why.trim().trim_end_matches('.')));
    }
    let running = Running::on(slot);
    run_found_or_built(&mut *running.slot, root, org, project, &setup, &session, &values, &prefix, rows, timing, now).await
}

#[allow(clippy::too_many_arguments)]
async fn run_found_or_built<B: DiscoveryBrowser, S: RowSource>(
    slot: &mut Option<B>,
    root: &Path,
    org: &str,
    project: &str,
    setup: &SavedSetup,
    session: &str,
    values: &BTreeMap<String, String>,
    prefix: &str,
    rows: Option<&S>,
    timing: &Timing,
    now: u64,
) -> (u16, String) {
    // The find decides everything.
    let found = match (choose(&setup.find, rows.is_some()), &setup.find.database, rows) {
        (FindChoice::Database, Some(db), Some(rows)) => match find_in_database(rows, db, prefix).await {
            DbOutcome::Found(f) => Some(f),
            DbOutcome::NotFound => None,
            DbOutcome::CouldNotRun(why) => return (409, find_could_not_run(&why)),
        },
        (FindChoice::Browser, _, _) => match find_in_browser(slot, root, org, project, setup, prefix, timing).await {
            Ok(f) => f,
            Err(why) => return (409, find_could_not_run(&why)),
        },
        _ => {
            let said = if setup.find.database.is_some() { DATABASE_OFF } else { NEEDS_A_FIND_RULE };
            return (409, said.to_string());
        }
    };
    if let Some(record) = found {
        stamp(root, org, project, setup, now);
        crate::applog::info(format!("Auto Run setup {}: found an existing record", setup.name));
        return (200, json!({ "said": FOUND, "found": true, "record": record, "setup": setup.name }).to_string());
    }

    // Nothing found: build, unless this session must not save.
    if must_not_save(slot, root) {
        return (409, BUILD_NOT_SAVED.to_string());
    }
    if let Err(why) = trip(slot, root, org, project, setup, timing).await {
        return (409, action_failed(0, "the trip to the area", &why));
    }
    let floor = sessions::now_ms();
    let component = Component {
        name: setup.name.clone(),
        description: setup.description.clone(),
        inputs: setup
            .inputs
            .iter()
            .map(|i| ComponentInput { name: i.name.clone(), kind: InputKind::Text, description: i.description.clone() })
            .collect(),
        actions: setup.actions.clone(),
        tried_at: 0,
        tried_area: String::new(),
        version: 0,
        changes: 0,
    };
    let given: Map<String, Value> = values.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect();
    let actions = match components::expand(&component, &given) {
        Ok(a) => a,
        Err(why) => return (409, action_failed(0, "putting the values in", &why)),
    };
    for (i, action) in actions.iter().enumerate() {
        let n = i + 1;
        let answer = discover_action_in(slot, root, org, project, action, None, None).await;
        if let Some(why) = failure_of(&answer) {
            let (recorded, _, _) = record_over(slot, root, session, setup, values, floor);
            crate::applog::info(format!("Auto Run setup {}: stopped at action {n}", setup.name));
            return (
                200,
                json!({
                    "said": action_failed(n, &action.kind(), &why),
                    "built": false,
                    "failed_at": n,
                    "recorded": recorded,
                })
                .to_string(),
            );
        }
    }
    let (recorded, unconfirmed, ids) = record_over(slot, root, session, setup, values, floor);
    stamp(root, org, project, setup, now);
    crate::applog::info(format!("Auto Run setup {}: built a new record", setup.name));
    let named = setup.inputs.iter().find(|i| i.names_the_record).and_then(|i| values.get(&i.name).cloned());
    let returned = setup.creates.get(setup.returns);
    let record = json!({
        "id": ids.get(setup.returns).cloned().flatten(),
        "name": named.or_else(|| returned.map(|c| filled(&c.name, values))),
    });
    // A record outside the test prefix is never a clean build, whatever the
    // setups file said it would make.
    let off_prefix = setup.creates.iter().any(|c| {
        let name = filled(&c.name, values);
        !name.trim().is_empty() && !starts_with_prefix(&name, prefix)
    });
    let said = match unconfirmed.first() {
        _ if off_prefix => not_prefixed(),
        None => BUILT.to_string(),
        Some(request) => not_confirmed(request),
    };
    (
        200,
        json!({
            "said": said,
            "built": true,
            "confirmed": unconfirmed.is_empty() && !off_prefix,
            "record": record,
            "values": values,
            "recorded": recorded,
            "setup": setup.name,
        })
        .to_string(),
    )
}
