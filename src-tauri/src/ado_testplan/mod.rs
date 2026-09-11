//! Test plans, requirement suites, test points and test runs - ported from
//! the v1 devops_client.py testplan/test-execution sections. Requirement
//! suites auto-populate from the PBI's "Tested By" links, which is what makes
//! created tests show on the board's test count. Only GET/POST/PATCH here -
//! no DELETE anywhere - test plans, suites and runs are never removed by
//! this app. Only Test Case work items can be deleted, from `recycle.rs`.
//!
//! Layout: this file owns the shared types, pure helpers and the plan URL
//! base; `plans` owns plan/suite discovery and find-or-create; `runs` owns
//! points, runs, results and attachments; `history` owns the recent-runs
//! outcome sweep.

mod history;
mod plans;
mod runs;

use crate::ado::AdoClient;
use serde::Serialize;

/// How many per-plan suite requests run concurrently while scanning. These
/// are cheap GETs; the write budget (2/s) is unaffected.
const SUITE_SCAN_CONCURRENCY: usize = 8;

/// Hard stop for any continuation-token loop.
///
/// ADO's token is opaque: nothing in its contract promises the server will
/// eventually stop sending one, and a loop that trusts it has no floor. At
/// the page sizes these endpoints use this is far past any real project,
/// so hitting it means something is wrong - and stopping with the pages we
/// have beats spinning forever behind a UI that only says "Loading".
pub(crate) const MAX_PAGES: usize = 200;

/// Append a continuation token to a query string, encoded.
///
/// The token is server-supplied and opaque, so it may legally contain
/// characters that are structural in a query string - `&` would start a
/// bogus parameter, `+` would decode as a space, `%` would begin an escape
/// sequence. The `url` crate's query encode set covers none of those, so
/// until the 2026-08 audit (R-5) the token was concatenated raw. Microsoft's
/// own MCP server encodes it via URLSearchParams for the same reason.
pub(crate) fn push_continuation(url: &mut String, token: &str) {
    url.push_str("&continuationToken=");
    for b in token.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            url.push(b as char);
        } else {
            url.push_str(&format!("%{b:02X}"));
        }
    }
}

/// Decide whether a paging loop may continue.
///
/// Stops on a repeated token as well as on the page cap: a server that
/// hands back the token it was just given would otherwise loop forever
/// fetching the same page, which is indistinguishable from a hang.
pub(crate) fn may_continue(pages: usize, prev: Option<&String>, next: &str, what: &str) -> bool {
    if pages >= MAX_PAGES {
        crate::applog::warn(format!(
            "stopped paging {what} at the {MAX_PAGES}-page cap - results may be incomplete"
        ));
        return false;
    }
    if prev.is_some_and(|p| p == next) {
        crate::applog::warn(format!(
            "stopped paging {what}: the server repeated its continuation token"
        ));
        return false;
    }
    true
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct TestPlan {
    pub id: i32,
    pub name: String,
    pub area_path: String,
    pub root_suite_id: Option<i32>,
    /// The plan's iteration path and state, read for RANKING candidate
    /// plans when a requirement suite has to be created (see
    /// `ensure_requirement_suite_cb`). Kept off the IPC boundary: no
    /// screen shows them, and every frontend literal of a plan would
    /// otherwise have to grow two fields it never reads.
    #[serde(skip)]
    #[specta(skip)]
    pub iteration: String,
    #[serde(skip)]
    #[specta(skip)]
    pub state: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct SuiteRef {
    pub id: i32,
    pub name: String,
    pub suite_type: String,
    pub requirement_id: Option<i32>,
    /// Parent suite id so the browser can render the real folder tree
    /// (None = direct child of the plan's stripped root).
    pub parent_id: Option<i32>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct TestPoint {
    pub point_id: i32,
    pub test_case_id: Option<i32>,
    pub test_case_name: String,
    pub config_name: String,
    pub tester: String,
    pub last_outcome: String,
    pub last_run_id: Option<i32>,
    pub last_result_id: Option<i32>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct RunCreated {
    pub run_id: i32,
    pub web_url: String,
    /// Marked outcomes Azure DevOps had no result row for, so they were
    /// never recorded at all.
    ///
    /// Deliberately NOT folded into `extras_failed`. These were briefly
    /// reported through that list, whose one consumer wraps everything in
    /// "The outcomes were recorded, but this did not attach - add it in
    /// Azure DevOps." Both halves of that sentence are false for a lost
    /// outcome: it was not recorded, and it cannot be added there - it has
    /// to be marked again here. A channel whose framing contradicts the
    /// item is worse than no channel.
    pub outcomes_unrecorded: Vec<i32>,
    /// Per-step marks and attachments that did NOT make it onto the run.
    ///
    /// These are attached after the outcomes are already recorded, so a
    /// failure here must not fail the run - but it was not reported
    /// either, and a tester who marked five steps individually and
    /// attached a screenshot of the failure had no way to know none of it
    /// arrived. Each entry names what was lost, for which case.
    pub extras_failed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct RunResultRef {
    pub result_id: i32,
    pub test_case_id: Option<i32>,
    pub point_id: Option<i32>,
}

#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
pub struct OutcomeUpdate {
    pub id: i32,
    /// Passed / Failed / Blocked / NotApplicable.
    pub outcome: String,
    pub comment: Option<String>,
    pub duration_ms: Option<i32>,
    /// Bug work-item ids to associate with this result.
    pub bug_ids: Option<Vec<i32>>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct ResultDetail {
    pub outcome: String,
    pub comment: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct PlanWithSuites {
    pub plan: TestPlan,
    pub suites: Vec<SuiteRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct EnsuredSuite {
    pub plan_id: i32,
    pub plan_name: String,
    pub suite_id: i32,
    /// True when NO area-matched test plan existed and one was created on
    /// the fly - callers surface this to the user (plans appearing out of
    /// nowhere would otherwise be a mystery).
    pub created_plan: bool,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct RunOutcome {
    pub outcome: String,
    pub completed_date: String,
    pub run_id: i32,
    /// The result row inside that run, so an execution-history view can
    /// pull the comment and linked bugs for a PRIOR result, not just the
    /// latest one the point itself carries.
    pub result_id: i32,
    /// Who ran it (display name), matching the "Run by" column in Azure
    /// DevOps' own execution history. Empty when the result carries none.
    pub run_by: String,
}

/// One test case's recent outcomes (newest first, capped at 5).
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct CaseHistory {
    pub test_case_id: i32,
    pub outcomes: Vec<RunOutcome>,
}

/// Build the ADO iterationDetails payload from per-step outcomes, ported
/// from v1 _iteration_details: actionPath is the step id as 8-digit hex,
/// only individually-marked steps are included, None when nothing marked.
pub fn build_iteration_details(
    step_ids: &[String],
    step_outcomes: &[Option<String>],
    overall: &str,
) -> Option<serde_json::Value> {
    let mut action_results = vec![];
    for (idx, sid) in step_ids.iter().enumerate() {
        let Some(Some(oc)) = step_outcomes.get(idx) else { continue };
        if oc.is_empty() {
            continue;
        }
        let action_path = match sid.parse::<i64>() {
            Ok(n) => format!("{n:08X}"),
            Err(_) => sid.clone(),
        };
        action_results.push(serde_json::json!({
            "actionPath": action_path,
            "iterationId": 1,
            "stepIdentifier": sid,
            "outcome": oc,
        }));
    }
    if action_results.is_empty() {
        return None;
    }
    let overall = if overall.is_empty() { "Failed" } else { overall };
    Some(serde_json::json!([{
        "id": 1,
        "outcome": overall,
        "actionResults": action_results,
    }]))
}

/// True if the plan's area path equals or is an ancestor of the PBI's area
/// path. ADO area paths use backslashes; tolerate slashes too.
pub fn area_matches(plan_area: &str, pbi_area: &str) -> bool {
    if plan_area.trim().is_empty() || pbi_area.trim().is_empty() {
        return false;
    }
    let pa = plan_area.trim().to_lowercase().replace('/', "\\");
    let ba = pbi_area.trim().to_lowercase().replace('/', "\\");
    ba == pa || ba.starts_with(&format!("{pa}\\"))
}

/// PBI -> resolved requirement suite, held for the app's lifetime and
/// shared by everything that resolves one: the upload, Run Tests and the
/// AI bridge. Resolving scans EVERY test plan in the project, one request
/// per plan and throttle-paced (about a minute on a large org), and the
/// ids are stable once found. A suite deleted in Azure DevOps since is
/// caught by the 404 on its points; the caller forgets it and scans once.
/// The client's base_url is in the key so parallel tests on different
/// mock servers cannot poison each other.
fn suite_cache() -> &'static std::sync::Mutex<std::collections::HashMap<(String, String, String, i32), EnsuredSuite>> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<(String, String, String, i32), EnsuredSuite>>,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn suite_key(base_url: &str, org: &str, project: &str, pbi_id: i32) -> (String, String, String, i32) {
    (base_url.to_string(), org.to_string(), project.to_string(), pbi_id)
}

pub fn cached_suite(base_url: &str, org: &str, project: &str, pbi_id: i32) -> Option<EnsuredSuite> {
    suite_cache().lock().ok()?.get(&suite_key(base_url, org, project, pbi_id)).cloned()
}

pub fn remember_suite(base_url: &str, org: &str, project: &str, pbi_id: i32, suite: &EnsuredSuite) {
    if let Ok(mut c) = suite_cache().lock() {
        c.insert(suite_key(base_url, org, project, pbi_id), suite.clone());
    }
}

pub fn forget_suite(base_url: &str, org: &str, project: &str, pbi_id: i32) {
    if let Ok(mut c) = suite_cache().lock() {
        c.remove(&suite_key(base_url, org, project, pbi_id));
    }
}

pub fn default_plan_name(area_path: &str) -> String {
    let leaf = area_path
        .replace('/', "\\")
        .split('\\')
        .next_back()
        .unwrap_or("")
        .trim()
        .to_string();
    if leaf.is_empty() {
        "Test Plan".to_string()
    } else {
        format!("{leaf} - Test Plan")
    }
}

/// ADO serializes some ids ("testCase.id", "testPoint.id") as strings in run
/// results and as numbers elsewhere - accept either.
fn id_i32(v: &serde_json::Value) -> Option<i32> {
    v.as_str()
        .and_then(|s| s.parse::<i32>().ok())
        .or_else(|| v.as_i64().map(|i| i as i32))
}

/// Result comments are capped at 1000 chars, matching v1.
fn cap_comment(c: &str) -> String {
    c.chars().take(1000).collect()
}

impl AdoClient {
    fn tp_base(&self, org: &str, project: &str) -> String {
        format!("{}/{}/{}/_apis", self.base_url, org, project)
    }
}
