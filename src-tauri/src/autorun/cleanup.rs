//! Clean up test-made drafts - the design doc's section 4.
//!
//! A person picks an environment, a name prefix and an age; `preview` lists
//! the entries of the record of test-made drafts (`test_made`) that match,
//! each with whether a proven delete template can delete it. The person
//! ticks the ones to go and confirms; `run_cleanup` checks the same preview
//! again, here, and deletes each ticked entry that is still in it, one at a
//! time, through its kind's delete template, in one browser signed in once
//! as that template's proven account, under that account's lease.
//!
//! Cleanup only ever deletes a record entry: it never searches the
//! application under test. It starts only from the app, by a person: no
//! bridge route and no MCP tool reaches `run_cleanup` (a tripwire test
//! scans for it).
//!
//! Proving a delete template is the other way an entry can go: the prove
//! only accepts an `id` that is a `present` entry of the template's kind in
//! the active environment (`proof_subject`), and a proof that deleted it
//! marks it `deleted` (`proof_deleted`).
//!
//! Nothing here formats a password, a cookie, a host or a query string: an
//! outcome is the runner's own sentence, and the app log gets ids, kinds
//! and names only.

use crate::api_templates::runner::{
    account_lease, open_session, preflight, run_in_session, Mode, RunRequest, RETRY_PAUSES, RUN_LIMIT,
};
use crate::api_templates::{store as template_store, ApiTemplate, Effect, ParamType};
use crate::applog;
use crate::autorun::replay::Browsers;
use crate::autorun::test_made::{self, TestMade, DELETED, PRESENT};
use crate::browser::cdp::Driver;
use crate::browser::timing::Timing;
use serde::Serialize;
use serde_json::Value;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Said when "older than" is under a day.
pub const OLDER_THAN_MIN: &str = "older than must be at least 1 day";

/// Said when the prefix is blank: every name would match it.
pub const PREFIX_NEEDED: &str = "enter the test name prefix to look for";

/// Said when the environment asked for is not saved.
pub const NO_SUCH_ENVIRONMENT: &str = "that environment is no longer saved";

/// Said when a run names a draft the preview does not show any more.
pub const NOT_IN_PREVIEW: &str =
    "some of the chosen drafts are no longer in the preview, so nothing was deleted. Preview again";

/// Said when none of the chosen drafts has a delete template to go by.
pub const NOTHING_TO_DELETE: &str = "none of the chosen drafts has a proven delete template, so nothing was deleted";

/// Said when a delete template is proven on an id that is not a `present`
/// entry of its kind in the active environment.
pub const PROVE_REFUSAL: &str = "a delete template is only proven on a draft the tests made";

/// Said when the active environment could not be read.
const ENVIRONMENT_UNREADABLE: &str = "the environments could not be read - see Settings, Logs";

/// What a line that cannot be ticked says.
pub fn no_delete_template(kind: &str) -> String {
    format!("no proven delete template for {kind}")
}

/// Said when the environment to clean up is not the active one: a delete
/// goes to the active environment's site, so it is the only one cleaned.
pub fn not_active(name: &str) -> String {
    format!("Clean up only deletes in the active environment. Switch to {name} first")
}

/// One line of the preview.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct CleanupLine {
    pub entry: TestMade,
    /// A proven delete template for the entry's kind exists.
    pub deletable: bool,
    /// Why it cannot be ticked, when it cannot.
    pub note: Option<String>,
}

/// What the preview was asked for. `run_cleanup` asks it again.
#[derive(Debug, Clone, PartialEq)]
pub struct CleanupQuery {
    /// The environment's id.
    pub environment: String,
    pub prefix: String,
    pub older_than_days: i32,
}

/// One delete's progress, as the dialog hears it.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct CleanupProgress {
    pub done: u32,
    pub total: u32,
    pub id: String,
    /// `deleted`, or the sentence the delete failed with.
    pub outcome: String,
}

/// One entry's result.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct CleanupResult {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub ok: bool,
    /// `deleted`, or the sentence the delete failed with.
    pub outcome: String,
}

/// What a cleanup did, in the order it did it.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct CleanupReport {
    pub results: Vec<CleanupResult>,
    /// How many were chosen to go.
    pub total: u32,
    /// A Stop ended it before the last one.
    pub stopped: bool,
}

/// The newest proven delete template for `kind` in the environment called
/// `env_name`. A proof that names its environment must name this one; an
/// older proof that names none counts anywhere. A delete template with no
/// kind never matches.
pub fn delete_template_for<'a>(templates: &'a [ApiTemplate], kind: &str, env_name: &str) -> Option<&'a ApiTemplate> {
    templates
        .iter()
        .filter(|t| t.effect == Effect::Delete)
        .filter(|t| t.deletes_kind.as_deref().map(str::trim).is_some_and(|k| !k.is_empty() && k == kind.trim()))
        .filter_map(|t| t.proven.as_ref().map(|p| (t, p)))
        .filter(|(_, p)| p.environment.as_deref().is_none_or(|e| e.trim().eq_ignore_ascii_case(env_name.trim())))
        // `at` is "YYYY-MM-DD HH:MM:SS", so text order is time order.
        .max_by(|(_, a), (_, b)| a.at.cmp(&b.at))
        .map(|(t, _)| t)
}

/// Whether `entry` is one the preview shows: still there (or its delete
/// failed), in `environment`, named with `prefix` (ignoring case), and at
/// least `days` old at `now`. A `created_at` that does not read is never
/// old enough.
fn shown(entry: &TestMade, environment: &str, prefix: &str, days: i32, now: chrono::DateTime<chrono::Utc>) -> bool {
    let status_ok = entry.status == PRESENT || entry.status.starts_with("delete failed");
    let prefix_ok = entry.name.to_lowercase().starts_with(&prefix.to_lowercase());
    let age_ok = chrono::DateTime::parse_from_rfc3339(&entry.created_at)
        .map(|t| now.signed_duration_since(t) >= chrono::Duration::days(i64::from(days)))
        .unwrap_or(false);
    status_ok && entry.environment == environment && prefix_ok && age_ok
}

/// The preview, each line with the template that would delete it.
fn lines_with_templates(
    root: &Path,
    org: &str,
    project: &str,
    query: &CleanupQuery,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<Vec<(CleanupLine, Option<ApiTemplate>)>, String> {
    if query.older_than_days < 1 {
        return Err(OLDER_THAN_MIN.to_string());
    }
    let prefix = query.prefix.trim();
    if prefix.is_empty() {
        return Err(PREFIX_NEEDED.to_string());
    }
    let envs = crate::environments::load_or_init(root, None).map_err(|e| {
        applog::warn(format!("clean up: the environments could not be read: {e}"));
        ENVIRONMENT_UNREADABLE.to_string()
    })?;
    let env = envs.environments.iter().find(|e| e.id == query.environment).ok_or(NO_SUCH_ENVIRONMENT.to_string())?;
    let templates: Vec<ApiTemplate> = template_store::list(root, org, project)
        .unwrap_or_else(|e| {
            applog::warn(format!("clean up: the api templates could not be listed: {e}"));
            vec![]
        })
        .into_iter()
        .map(|s| s.template)
        .collect();
    Ok(test_made::list(root)
        .into_iter()
        .filter(|e| shown(e, &env.id, prefix, query.older_than_days, now))
        .map(|entry| {
            let t = delete_template_for(&templates, &entry.kind, &env.name).cloned();
            let note = t.is_none().then(|| no_delete_template(&entry.kind));
            (CleanupLine { entry, deletable: t.is_some(), note }, t)
        })
        .collect())
}

/// The record entries a cleanup of `environment` would offer, oldest first.
/// See `shown` for the filter and `delete_template_for` for `deletable`.
pub fn preview(
    root: &Path,
    org: &str,
    project: &str,
    query: &CleanupQuery,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<Vec<CleanupLine>, String> {
    Ok(lines_with_templates(root, org, project, query, now)?.into_iter().map(|(l, _)| l).collect())
}

/// The value a delete template's `id` param gets: the entry's id, as a
/// number when the param is one and the id reads as one.
fn id_value(t: &ApiTemplate, id: &str) -> Value {
    let number = t.params.iter().any(|p| p.name == "id" && p.kind == ParamType::Number);
    match serde_json::from_str::<Value>(id.trim()) {
        Ok(v) if number && v.is_number() => v,
        _ => Value::String(id.to_string()),
    }
}

/// One delete's request.
fn request(org: &str, project: &str, t: &ApiTemplate, entry: &TestMade) -> RunRequest {
    let mut values = serde_json::Map::new();
    values.insert("id".to_string(), id_value(t, &entry.id));
    RunRequest {
        org: org.to_string(),
        project: project.to_string(),
        account: t.proven.as_ref().map(|p| p.account.clone()).unwrap_or_default(),
        values,
        mode: Mode::Cleanup,
        template: t.clone(),
    }
}

/// Writes one result to the record and says it.
struct Tally<'a, F: FnMut(CleanupProgress)> {
    root: &'a Path,
    total: u32,
    report: CleanupReport,
    on_progress: F,
}

impl<F: FnMut(CleanupProgress)> Tally<'_, F> {
    fn done(&mut self, entry: &TestMade, outcome: Result<(), String>) {
        let (ok, said) = match outcome {
            Ok(()) => (true, DELETED.to_string()),
            Err(why) => (false, why),
        };
        let status = if ok { DELETED.to_string() } else { test_made::delete_failed(&said) };
        match test_made::set_status(self.root, &entry.environment, &entry.kind, &entry.id, &status) {
            Ok(true) => {}
            Ok(false) => applog::warn(format!("clean up: {} {} was no longer in the record", entry.kind, entry.id)),
            Err(e) => applog::warn(format!("clean up: the result for {} {} could not be recorded: {e}", entry.kind, entry.id)),
        }
        applog::info(format!(
            "clean up: {} {} {}",
            entry.kind,
            entry.id,
            if ok { "deleted" } else { "not deleted" }
        ));
        self.report.results.push(CleanupResult {
            id: entry.id.clone(),
            kind: entry.kind.clone(),
            name: entry.name.clone(),
            ok,
            outcome: said.clone(),
        });
        (self.on_progress)(CleanupProgress {
            done: self.report.results.len() as u32,
            total: self.total,
            id: entry.id.clone(),
            outcome: said,
        });
    }
}

/// Deletes the entries of `ids` that `query`'s preview shows as deletable,
/// at `now`, one at a time. See the module comment. The caller holds the
/// one-at-a-time template slot, so no template run, fixture run or
/// environment switch comes between the deletes.
///
/// Refused whole, before anything is deleted, when `query`'s environment
/// is not the active one, or when an id is not in the preview. Each delete
/// sets its entry `deleted` or `delete failed: <the runner's sentence>`
/// and calls `on_progress`. `cancel` is heard between deletes: the rest
/// stay as they were.
#[allow(clippy::too_many_arguments)]
pub async fn run_cleanup<B: Browsers>(
    browsers: &mut B,
    root: &Path,
    org: &str,
    project: &str,
    query: &CleanupQuery,
    ids: &[String],
    timing: &Timing,
    cancel: &AtomicBool,
    on_progress: impl FnMut(CleanupProgress),
) -> Result<CleanupReport, String> {
    let now = chrono::Utc::now();
    run_cleanup_within(browsers, root, org, project, query, ids, timing, RUN_LIMIT, &RETRY_PAUSES, now, cancel, on_progress)
        .await
}

/// `run_cleanup` with the per-delete limit, the retry pauses and the time
/// given - the way a test reaches a known age.
#[allow(clippy::too_many_arguments)]
pub async fn run_cleanup_within<B: Browsers>(
    browsers: &mut B,
    root: &Path,
    org: &str,
    project: &str,
    query: &CleanupQuery,
    ids: &[String],
    timing: &Timing,
    limit: Duration,
    retry_pauses: &[Duration],
    now: chrono::DateTime<chrono::Utc>,
    cancel: &AtomicBool,
    on_progress: impl FnMut(CleanupProgress),
) -> Result<CleanupReport, String> {
    let active = crate::environments::active(root).map_err(|e| {
        applog::warn(format!("clean up: the active environment could not be read: {e}"));
        ENVIRONMENT_UNREADABLE.to_string()
    })?;
    let lines = lines_with_templates(root, org, project, query, now)?;
    if active.id != query.environment {
        let name = crate::environments::load_or_init(root, None)
            .ok()
            .and_then(|f| f.environments.into_iter().find(|e| e.id == query.environment).map(|e| e.name))
            .unwrap_or_default();
        return Err(not_active(&name));
    }
    if ids.iter().any(|id| !lines.iter().any(|(l, _)| &l.entry.id == id)) {
        return Err(NOT_IN_PREVIEW.to_string());
    }
    let chosen: Vec<(TestMade, ApiTemplate)> = lines
        .into_iter()
        .filter(|(l, _)| ids.contains(&l.entry.id))
        .filter_map(|(l, t)| t.map(|t| (l.entry, t)))
        .collect();
    if chosen.is_empty() {
        return Err(NOTHING_TO_DELETE.to_string());
    }
    let stopped = || cancel.load(Ordering::SeqCst);
    let mut tally = Tally {
        root,
        total: chosen.len() as u32,
        report: CleanupReport { results: vec![], total: chosen.len() as u32, stopped: false },
        on_progress,
    };
    applog::info(format!("clean up: {} drafts to delete in {}", chosen.len(), active.name));

    // Each delete template signs in as its own proven account: the entries
    // go in groups, one per account, in the order each account first comes.
    // A cleanup of one kind - the usual one - is one group, one browser and
    // one sign-in.
    let mut groups: Vec<(String, Vec<(TestMade, ApiTemplate)>)> = Vec::new();
    for (entry, t) in chosen {
        let account = t.proven.as_ref().map(|p| p.account.clone()).unwrap_or_default();
        match groups.iter_mut().find(|(a, _)| *a == account) {
            Some((_, items)) => items.push((entry, t)),
            None => groups.push((account, vec![(entry, t)])),
        }
    }

    'groups: for (account, items) in groups {
        if stopped() {
            tally.report.stopped = true;
            break;
        }
        // One lease for the group, held until its browser is closed.
        let _lease = match account_lease(root, &account, timing).await {
            Ok(l) => l,
            Err(why) => {
                for (entry, _) in &items {
                    tally.done(entry, Err(why.clone()));
                }
                continue;
            }
        };
        let mut d = match browsers.open().await {
            Ok(d) => d,
            Err(why) => {
                for (entry, _) in &items {
                    tally.done(entry, Err(format!("the browser did not open: {why}")));
                }
                continue;
            }
        };
        let (first, t) = &items[0];
        match open_session(&mut d, root, &request(org, project, t, first), timing, limit).await {
            Err(report) => {
                let why = report.message();
                for (entry, _) in &items {
                    tally.done(entry, Err(why.clone()));
                }
            }
            Ok(session) => {
                for (entry, t) in &items {
                    // Heard between deletes: one already sent runs to its end.
                    if stopped() {
                        tally.report.stopped = true;
                        d.set_deadline(None);
                        browsers.close(d).await;
                        break 'groups;
                    }
                    let req = request(org, project, t, entry);
                    let outcome = match preflight(root, &req, None) {
                        Err(problems) => Err(problems.join("; ")),
                        Ok(()) => {
                            let report = run_in_session(&mut d, root, &req, timing, &session, limit, retry_pauses).await;
                            if report.ok {
                                Ok(())
                            } else {
                                Err(report.message())
                            }
                        }
                    };
                    tally.done(entry, outcome);
                }
            }
        }
        d.set_deadline(None);
        browsers.close(d).await;
    }
    Ok(tally.report)
}

/// The entry a prove of delete template `t` with `values` would delete,
/// or `PROVE_REFUSAL`: its `id` must be a `present` entry of the template's
/// kind in the active environment.
pub fn proof_subject(root: &Path, t: &ApiTemplate, values: &serde_json::Map<String, Value>) -> Result<TestMade, String> {
    let env = crate::environments::active(root).map_err(|e| {
        applog::warn(format!("api template {}: the active environment could not be read: {e}", t.id));
        PROVE_REFUSAL.to_string()
    })?;
    let id = match values.get("id") {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Number(n)) => n.to_string(),
        _ => return Err(PROVE_REFUSAL.to_string()),
    };
    let kind = t.deletes_kind.as_deref().map(str::trim).unwrap_or("");
    test_made::list(root)
        .into_iter()
        .find(|e| e.environment == env.id && e.kind == kind && e.id == id && e.status == PRESENT)
        .ok_or_else(|| PROVE_REFUSAL.to_string())
}

/// Marks `entry` deleted: a proof of a delete template deleted it.
pub fn proof_deleted(root: &Path, entry: &TestMade) {
    match test_made::set_status(root, &entry.environment, &entry.kind, &entry.id, DELETED) {
        Ok(_) => applog::info(format!("api template proof deleted {} {}", entry.kind, entry.id)),
        Err(e) => applog::warn(format!(
            "the proof that deleted {} {} could not be recorded: {e}",
            entry.kind, entry.id
        )),
    }
}
