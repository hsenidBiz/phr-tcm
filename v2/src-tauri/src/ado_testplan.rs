//! Test plans, requirement suites, test points and test runs - ported from
//! the v1 devops_client.py testplan/test-execution sections. Requirement
//! suites auto-populate from the PBI's "Tested By" links, which is what makes
//! created tests show on the board's test count. Only GET/POST/PATCH here -
//! no DELETE anywhere, same as the rest of the client.

use crate::ado::{AdoClient, AdoError};
use futures::stream::{self, StreamExt};
use serde::Serialize;

/// How many per-plan suite requests run concurrently while scanning. These
/// are cheap GETs; the write budget (2/s) is unaffected.
const SUITE_SCAN_CONCURRENCY: usize = 8;

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct TestPlan {
    pub id: i32,
    pub name: String,
    pub area_path: String,
    pub root_suite_id: Option<i32>,
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

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct PlanWithSuites {
    pub plan: TestPlan,
    pub suites: Vec<SuiteRef>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct EnsuredSuite {
    pub plan_id: i32,
    pub plan_name: String,
    pub suite_id: i32,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct RunOutcome {
    pub outcome: String,
    pub completed_date: String,
    pub run_id: i32,
}

/// One test case's recent outcomes (newest first, capped at 5).
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct CaseHistory {
    pub test_case_id: i32,
    pub outcomes: Vec<RunOutcome>,
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

impl AdoClient {
    fn tp_base(&self, org: &str, project: &str) -> String {
        format!("{}/{}/{}/_apis", self.base_url, org, project)
    }

    /// All test plans in the project (paginated). Read only.
    pub async fn get_test_plans(&self, org: &str, project: &str) -> Result<Vec<TestPlan>, AdoError> {
        let mut plans = vec![];
        let mut continuation: Option<String> = None;
        loop {
            let mut url = format!("{}/testplan/plans?api-version=7.1", self.tp_base(org, project));
            if let Some(c) = &continuation {
                url.push_str(&format!("&continuationToken={c}"));
            }
            let (data, cont) = self.get_json_with_continuation(url).await?;
            for p in data["value"].as_array().cloned().unwrap_or_default() {
                plans.push(TestPlan {
                    id: p["id"].as_i64().unwrap_or_default() as i32,
                    name: p["name"].as_str().unwrap_or_default().to_string(),
                    area_path: p["areaPath"].as_str().unwrap_or_default().to_string(),
                    root_suite_id: p["rootSuite"]["id"].as_i64().map(|i| i as i32),
                });
            }
            continuation = cont;
            if continuation.is_none() {
                break;
            }
        }
        Ok(plans)
    }

    /// A single plan including its root suite id. Read only.
    pub async fn get_test_plan(&self, org: &str, project: &str, plan_id: i32) -> Result<TestPlan, AdoError> {
        let url = format!(
            "{}/testplan/plans/{}?api-version=7.1",
            self.tp_base(org, project),
            plan_id
        );
        let data = self.get_json(url).await?;
        Ok(TestPlan {
            id: data["id"].as_i64().unwrap_or_default() as i32,
            name: data["name"].as_str().unwrap_or_default().to_string(),
            area_path: data["areaPath"].as_str().unwrap_or_default().to_string(),
            root_suite_id: data["rootSuite"]["id"].as_i64().map(|i| i as i32),
        })
    }

    /// The requirement-based suite bound to `pbi_id` within `plan_id`, or
    /// None. Stops paging as soon as it matches. Read only.
    pub async fn find_requirement_suite(
        &self,
        org: &str,
        project: &str,
        plan_id: i32,
        pbi_id: i32,
    ) -> Result<Option<SuiteRef>, AdoError> {
        let mut continuation: Option<String> = None;
        loop {
            let mut url = format!(
                "{}/testplan/Plans/{}/suites?api-version=7.1",
                self.tp_base(org, project),
                plan_id
            );
            if let Some(c) = &continuation {
                url.push_str(&format!("&continuationToken={c}"));
            }
            let (data, cont) = self.get_json_with_continuation(url).await?;
            for s in data["value"].as_array().cloned().unwrap_or_default() {
                if s["requirementId"].as_i64() == Some(pbi_id as i64)
                    && s["suiteType"].as_str() == Some("requirementTestSuite")
                {
                    return Ok(Some(SuiteRef {
                        id: s["id"].as_i64().unwrap_or_default() as i32,
                        name: s["name"].as_str().unwrap_or_default().to_string(),
                        suite_type: "requirementTestSuite".to_string(),
                        requirement_id: Some(pbi_id),
                        parent_id: s["parentSuite"]["id"].as_i64().map(|i| i as i32),
                    }));
                }
            }
            continuation = cont;
            if continuation.is_none() {
                return Ok(None);
            }
        }
    }

    /// Every suite in a plan, flat (parent links let callers rebuild the
    /// tree). Read only.
    pub async fn get_all_suites(
        &self,
        org: &str,
        project: &str,
        plan_id: i32,
    ) -> Result<Vec<SuiteRef>, AdoError> {
        let mut suites = vec![];
        let mut continuation: Option<String> = None;
        loop {
            let mut url = format!(
                "{}/testplan/Plans/{}/suites?api-version=7.1",
                self.tp_base(org, project),
                plan_id
            );
            if let Some(c) = &continuation {
                url.push_str(&format!("&continuationToken={c}"));
            }
            let (data, cont) = self.get_json_with_continuation(url).await?;
            for s in data["value"].as_array().cloned().unwrap_or_default() {
                suites.push(SuiteRef {
                    id: s["id"].as_i64().unwrap_or_default() as i32,
                    name: s["name"].as_str().unwrap_or_default().to_string(),
                    suite_type: s["suiteType"].as_str().unwrap_or_default().to_string(),
                    requirement_id: s["requirementId"].as_i64().map(|i| i as i32),
                    parent_id: s["parentSuite"]["id"].as_i64().map(|i| i as i32),
                });
            }
            continuation = cont;
            if continuation.is_none() {
                break;
            }
        }
        Ok(suites)
    }

    /// Every plan with its suites, for the Test Suites browser. Plans whose
    /// suites can't be read (permissions) are skipped, and - the v1 rule the
    /// suite browser shipped with - plans containing no suites beyond their
    /// root are hidden entirely. Read only.
    pub async fn list_plans_with_suites(
        &self,
        org: &str,
        project: &str,
    ) -> Result<Vec<PlanWithSuites>, AdoError> {
        self.list_plans_with_suites_cb(org, project, |_, _| {}).await
    }

    /// Same, reporting (done, total) after each plan scanned so the UI can
    /// show "Scanning plans X of Y".
    pub async fn list_plans_with_suites_cb(
        &self,
        org: &str,
        project: &str,
        mut progress: impl FnMut(u32, u32),
    ) -> Result<Vec<PlanWithSuites>, AdoError> {
        let plans = self.get_test_plans(org, project).await?;
        let total = plans.len() as u32;
        let mut done = 0u32;

        // One suites request per plan, run SUITE_SCAN_CONCURRENCY at a time
        // (serial scanning made big projects take minutes). Results are put
        // back in plan order so the browser output is stable.
        let mut fetched: Vec<Option<Result<Vec<SuiteRef>, AdoError>>> =
            plans.iter().map(|_| None).collect();
        {
            // Futures are created eagerly (concrete lifetimes keep the tauri
            // command macro happy); the async block only owns (index, future).
            let futs: Vec<_> = plans
                .iter()
                .enumerate()
                .map(|(i, plan)| {
                    let fut = self.get_all_suites(org, project, plan.id);
                    async move { (i, fut.await) }
                })
                .collect();
            let mut in_flight = stream::iter(futs).buffer_unordered(SUITE_SCAN_CONCURRENCY);
            while let Some((i, res)) = in_flight.next().await {
                done += 1;
                progress(done, total);
                fetched[i] = Some(res);
            }
        }

        let mut out = vec![];
        for (plan, res) in plans.into_iter().zip(fetched) {
            let suites = match res.expect("every plan index was filled") {
                Ok(s) => s,
                Err(AdoError::Forbidden) | Err(AdoError::NotFound) => continue,
                Err(e) => return Err(e),
            };
            // The root suite is structural, not user content: a plan whose
            // only suite is its root has no suites worth browsing.
            let non_root: Vec<SuiteRef> = suites
                .into_iter()
                .filter(|s| Some(s.id) != plan.root_suite_id && s.suite_type != "")
                .collect();
            let non_root: Vec<SuiteRef> = if plan.root_suite_id.is_some() {
                non_root
            } else {
                // Root id unknown from the list endpoint: drop the first
                // suite only when it is the conventional "<plan name>" root.
                non_root
                    .into_iter()
                    .filter(|s| !(s.suite_type == "staticTestSuite" && s.name == plan.name))
                    .collect()
            };
            if non_root.is_empty() {
                continue;
            }
            // Children of the stripped root become top-level in the tree.
            let root_id = plan.root_suite_id;
            let non_root: Vec<SuiteRef> = non_root
                .into_iter()
                .map(|mut s| {
                    if s.parent_id == root_id {
                        s.parent_id = None;
                    }
                    s
                })
                .collect();
            out.push(PlanWithSuites { plan, suites: non_root });
        }
        Ok(out)
    }

    /// POST a new test plan (creating one also creates its root suite).
    pub async fn create_test_plan(
        &self,
        org: &str,
        project: &str,
        name: &str,
        area_path: &str,
        iteration: &str,
    ) -> Result<TestPlan, AdoError> {
        let mut body = serde_json::json!({ "name": name });
        if !area_path.is_empty() {
            body["areaPath"] = serde_json::json!(area_path);
        }
        if !iteration.is_empty() {
            body["iteration"] = serde_json::json!(iteration);
        }
        let url = format!("{}/testplan/plans?api-version=7.1", self.tp_base(org, project));
        let data = self.post_json(url, &body).await?;
        Ok(TestPlan {
            id: data["id"].as_i64().unwrap_or_default() as i32,
            name: data["name"].as_str().unwrap_or(name).to_string(),
            area_path: data["areaPath"].as_str().unwrap_or(area_path).to_string(),
            root_suite_id: data["rootSuite"]["id"].as_i64().map(|i| i as i32),
        })
    }

    /// POST a requirement-based suite bound to the PBI under the plan's root
    /// suite. ADO auto-populates it from the PBI's Tested By links.
    pub async fn create_requirement_suite(
        &self,
        org: &str,
        project: &str,
        plan_id: i32,
        root_suite_id: i32,
        pbi_id: i32,
    ) -> Result<i32, AdoError> {
        let body = serde_json::json!({
            "suiteType": "requirementTestSuite",
            "requirementId": pbi_id,
            "parentSuite": {"id": root_suite_id},
        });
        let url = format!(
            "{}/testplan/Plans/{}/suites?api-version=7.1",
            self.tp_base(org, project),
            plan_id
        );
        let data = self.post_json(url, &body).await?;
        Ok(data["id"].as_i64().unwrap_or_default() as i32)
    }

    /// Scan `plans` for the PBI's requirement suite - area-matched plans
    /// first (the common case finds it quickly), probed in concurrent
    /// batches; join_all keeps plan order within a batch so the FIRST plan
    /// holding the suite wins and the scan stops early. Read only.
    async fn scan_plans_for_suite(
        &self,
        org: &str,
        project: &str,
        pbi_id: i32,
        area_path: &str,
        plans: &[TestPlan],
        progress: &mut impl FnMut(u32, u32),
    ) -> Result<Option<EnsuredSuite>, AdoError> {
        let mut ordered: Vec<&TestPlan> = plans.iter().collect();
        ordered.sort_by_key(|p| if area_matches(&p.area_path, area_path) { 0 } else { 1 });
        let total = ordered.len() as u32;
        let mut done = 0u32;
        for batch in ordered.chunks(SUITE_SCAN_CONCURRENCY) {
            let results = futures::future::join_all(
                batch
                    .iter()
                    .map(|plan| self.find_requirement_suite(org, project, plan.id, pbi_id)),
            )
            .await;
            done += batch.len() as u32;
            progress(done, total);
            for (plan, res) in batch.iter().zip(results) {
                // A plan whose suites can't be listed (permissions) is
                // skipped rather than aborting the search; auth/rate-limit
                // errors still propagate so callers can re-auth / back off.
                match res {
                    Ok(Some(suite)) => {
                        return Ok(Some(EnsuredSuite {
                            plan_id: plan.id,
                            plan_name: plan.name.clone(),
                            suite_id: suite.id,
                        }))
                    }
                    Ok(None) => {}
                    Err(AdoError::Forbidden) | Err(AdoError::NotFound) => continue,
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(None)
    }

    /// Read-only lookup of the PBI's requirement suite across every plan -
    /// the background-prefetch variant of ensure: it NEVER creates a plan
    /// or suite, so it is safe to run without the user asking to run tests.
    pub async fn find_pbi_requirement_suite(
        &self,
        org: &str,
        project: &str,
        pbi_id: i32,
        area_path: &str,
    ) -> Result<Option<EnsuredSuite>, AdoError> {
        let plans = self.get_test_plans(org, project).await?;
        self.scan_plans_for_suite(org, project, pbi_id, area_path, &plans, &mut |_, _| {})
            .await
    }

    /// Find-or-create the area-matched plan and the PBI's requirement suite,
    /// ported from v1 ensure_requirement_suite. Reuses any existing suite so
    /// nothing is duplicated. May POST; never DELETEs.
    pub async fn ensure_requirement_suite(
        &self,
        org: &str,
        project: &str,
        pbi_id: i32,
        area_path: &str,
        iteration: &str,
    ) -> Result<EnsuredSuite, AdoError> {
        self.ensure_requirement_suite_cb(org, project, pbi_id, area_path, iteration, |_, _| {})
            .await
    }

    /// Same, reporting (done, total) per plan scanned.
    #[allow(clippy::too_many_arguments)]
    pub async fn ensure_requirement_suite_cb(
        &self,
        org: &str,
        project: &str,
        pbi_id: i32,
        area_path: &str,
        iteration: &str,
        mut progress: impl FnMut(u32, u32),
    ) -> Result<EnsuredSuite, AdoError> {
        let plans = self.get_test_plans(org, project).await?;
        if let Some(found) = self
            .scan_plans_for_suite(org, project, pbi_id, area_path, &plans, &mut progress)
            .await?
        {
            return Ok(found);
        }

        // No suite anywhere: find the most specific area-matched plan, or
        // create one, then create the requirement suite under its root.
        let mut best: Option<&TestPlan> = None;
        for p in &plans {
            if area_matches(&p.area_path, area_path) {
                let better = match best {
                    None => true,
                    Some(b) => p.area_path.len() > b.area_path.len(),
                };
                if better {
                    best = Some(p);
                }
            }
        }
        let plan = match best {
            Some(p) => {
                if p.root_suite_id.is_some() {
                    p.clone()
                } else {
                    self.get_test_plan(org, project, p.id).await?
                }
            }
            None => {
                self.create_test_plan(org, project, &default_plan_name(area_path), area_path, iteration)
                    .await?
            }
        };
        let root = plan.root_suite_id.ok_or(AdoError::Http {
            status: 0,
            body: "plan has no root suite".into(),
        })?;
        let suite_id = self
            .create_requirement_suite(org, project, plan.id, root, pbi_id)
            .await?;
        Ok(EnsuredSuite {
            plan_id: plan.id,
            plan_name: plan.name,
            suite_id,
        })
    }

    /// Per-test-case outcome history for a plan, via the recent-runs sweep:
    /// GET the plan's runs, take the RUN_HISTORY_RUNS most recent, fetch each
    /// run's results concurrently, and aggregate outcomes per test case -
    /// newest first, capped at RUN_HISTORY_PER_CASE. Unspecified/in-progress
    /// results are skipped; runs that 403/404 are skipped like the suite
    /// scan. Read only.
    pub async fn run_history(
        &self,
        org: &str,
        project: &str,
        plan_id: i32,
    ) -> Result<Vec<CaseHistory>, AdoError> {
        const RUN_HISTORY_RUNS: usize = 15;
        const RUN_HISTORY_PER_CASE: usize = 5;

        let url = format!(
            "{}/test/runs?planId={}&api-version=7.1",
            self.tp_base(org, project),
            plan_id
        );
        let data = self.get_json(url).await?;
        let mut runs: Vec<(i32, String)> = data["value"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|r| {
                Some((
                    r["id"].as_i64()? as i32,
                    r["completedDate"]
                        .as_str()
                        .or_else(|| r["startedDate"].as_str())
                        .unwrap_or_default()
                        .to_string(),
                ))
            })
            .collect();
        // Newest first (ISO dates sort lexicographically; undated runs last).
        runs.sort_by(|a, b| b.1.cmp(&a.1));
        runs.truncate(RUN_HISTORY_RUNS);

        // Fetch each run's results with bounded concurrency (eager futures -
        // closure-captured refs break the tauri command macro).
        let mut per_run: Vec<Option<Result<serde_json::Value, AdoError>>> =
            runs.iter().map(|_| None).collect();
        {
            let futs: Vec<_> = runs
                .iter()
                .enumerate()
                .map(|(i, (run_id, _))| {
                    let url = format!(
                        "{}/test/Runs/{}/results?api-version=7.1",
                        self.tp_base(org, project),
                        run_id
                    );
                    let fut = self.get_json(url);
                    async move { (i, fut.await) }
                })
                .collect();
            let mut in_flight = stream::iter(futs).buffer_unordered(SUITE_SCAN_CONCURRENCY);
            while let Some((i, res)) = in_flight.next().await {
                per_run[i] = Some(res);
            }
        }

        // Aggregate in run order (newest run first) so each case's list is
        // naturally newest-first.
        let mut by_case: std::collections::HashMap<i32, Vec<RunOutcome>> =
            std::collections::HashMap::new();
        for ((run_id, _), res) in runs.iter().zip(per_run) {
            let data = match res.expect("every run index was filled") {
                Ok(d) => d,
                Err(AdoError::Forbidden) | Err(AdoError::NotFound) => continue,
                Err(e) => return Err(e),
            };
            for r in data["value"].as_array().cloned().unwrap_or_default() {
                let outcome = r["outcome"].as_str().unwrap_or_default();
                if outcome.is_empty() || outcome.eq_ignore_ascii_case("unspecified") {
                    continue;
                }
                // testCase.id arrives as a string in run results.
                let case_id = match r["testCase"]["id"]
                    .as_str()
                    .and_then(|s| s.parse::<i32>().ok())
                    .or_else(|| r["testCase"]["id"].as_i64().map(|i| i as i32))
                {
                    Some(id) => id,
                    None => continue,
                };
                let list = by_case.entry(case_id).or_default();
                if list.len() < RUN_HISTORY_PER_CASE {
                    list.push(RunOutcome {
                        outcome: outcome.to_string(),
                        completed_date: r["completedDate"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string(),
                        run_id: *run_id,
                    });
                }
            }
        }
        Ok(by_case
            .into_iter()
            .map(|(test_case_id, outcomes)| CaseHistory { test_case_id, outcomes })
            .collect())
    }

    /// All test points in a plan+suite (paginated). Read only.
    pub async fn get_test_points(
        &self,
        org: &str,
        project: &str,
        plan_id: i32,
        suite_id: i32,
        test_case_ids: &[i32],
    ) -> Result<Vec<TestPoint>, AdoError> {
        let tc_filter = if test_case_ids.is_empty() {
            String::new()
        } else {
            format!(
                "&testCaseId={}",
                test_case_ids
                    .iter()
                    .map(|i| i.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        };
        let mut points = vec![];
        let mut continuation: Option<String> = None;
        loop {
            let mut url = format!(
                "{}/testplan/Plans/{}/Suites/{}/TestPoint?api-version=7.1{}",
                self.tp_base(org, project),
                plan_id,
                suite_id,
                tc_filter
            );
            if let Some(c) = &continuation {
                url.push_str(&format!("&continuationToken={c}"));
            }
            let (data, cont) = self.get_json_with_continuation(url).await?;
            for p in data["value"].as_array().cloned().unwrap_or_default() {
                points.push(TestPoint {
                    point_id: p["id"].as_i64().unwrap_or_default() as i32,
                    test_case_id: p["testCaseReference"]["id"].as_i64().map(|i| i as i32),
                    test_case_name: p["testCaseReference"]["name"].as_str().unwrap_or_default().to_string(),
                    config_name: p["configuration"]["name"].as_str().unwrap_or_default().to_string(),
                    tester: p["tester"]["displayName"].as_str().unwrap_or_default().to_string(),
                    // ADO reports never-run points as "unspecified" - that
                    // reads as a real outcome in the UI, so strip it here.
                    last_outcome: {
                        let o = p["results"]["outcome"].as_str().unwrap_or_default();
                        if o.eq_ignore_ascii_case("unspecified") { String::new() } else { o.to_string() }
                    },
                    last_run_id: p["results"]["lastTestRunId"].as_i64().map(|i| i as i32),
                    last_result_id: p["results"]["lastResultId"].as_i64().map(|i| i as i32),
                });
            }
            continuation = cont;
            if continuation.is_none() {
                break;
            }
        }
        Ok(points)
    }

    /// POST a manual test run seeded from the given point ids; ADO creates
    /// one result per point and the run starts InProgress.
    pub async fn create_test_run(
        &self,
        org: &str,
        project: &str,
        plan_id: i32,
        name: &str,
        point_ids: &[i32],
    ) -> Result<RunCreated, AdoError> {
        let body = serde_json::json!({
            "name": name,
            "plan": {"id": plan_id.to_string()},
            "pointIds": point_ids,
            "automated": false,
        });
        let url = format!("{}/test/runs?api-version=7.1", self.tp_base(org, project));
        let data = self.post_json(url, &body).await?;
        Ok(RunCreated {
            run_id: data["id"].as_i64().unwrap_or_default() as i32,
            web_url: data["webAccessUrl"].as_str().unwrap_or_default().to_string(),
        })
    }

    /// The results auto-created for a run. Read only.
    pub async fn get_run_results(
        &self,
        org: &str,
        project: &str,
        run_id: i32,
    ) -> Result<Vec<RunResultRef>, AdoError> {
        let url = format!(
            "{}/test/Runs/{}/results?api-version=7.1",
            self.tp_base(org, project),
            run_id
        );
        let data = self.get_json(url).await?;
        Ok(data["value"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|r| RunResultRef {
                result_id: r["id"].as_i64().unwrap_or_default() as i32,
                test_case_id: r["testCase"]["id"]
                    .as_str()
                    .and_then(|s| s.parse().ok())
                    .or_else(|| r["testCase"]["id"].as_i64().map(|i| i as i32)),
                point_id: r["testPoint"]["id"]
                    .as_str()
                    .and_then(|s| s.parse().ok())
                    .or_else(|| r["testPoint"]["id"].as_i64().map(|i| i as i32)),
            })
            .collect())
    }

    /// PATCH outcomes onto a run's results (plain JSON, marks each result
    /// Completed, comment capped at 1000 chars like v1).
    pub async fn update_run_results(
        &self,
        org: &str,
        project: &str,
        run_id: i32,
        results: &[OutcomeUpdate],
    ) -> Result<(), AdoError> {
        let body: Vec<serde_json::Value> = results
            .iter()
            .map(|r| {
                let mut item = serde_json::json!({
                    "id": r.id,
                    "outcome": r.outcome,
                    "state": "Completed",
                });
                if let Some(c) = &r.comment {
                    if !c.is_empty() {
                        let capped: String = c.chars().take(1000).collect();
                        item["comment"] = serde_json::json!(capped);
                    }
                }
                if let Some(d) = r.duration_ms {
                    if d > 0 {
                        item["durationInMs"] = serde_json::json!(d);
                    }
                }
                if let Some(bugs) = &r.bug_ids {
                    if !bugs.is_empty() {
                        item["associatedBugs"] = serde_json::json!(
                            bugs.iter().map(|b| serde_json::json!({"id": b})).collect::<Vec<_>>()
                        );
                    }
                }
                item
            })
            .collect();
        let url = format!(
            "{}/test/Runs/{}/results?api-version=7.1",
            self.tp_base(org, project),
            run_id
        );
        self.patch_plain_json(url, &serde_json::Value::Array(body)).await?;
        Ok(())
    }

    /// A single result's last outcome + comment (runner preload). Read only.
    /// Comment + associated bug ids for one result - the execution report's
    /// failure details ($expand-style WorkItems inclusion). Read only.
    pub async fn get_result_report_info(
        &self,
        org: &str,
        project: &str,
        run_id: i32,
        result_id: i32,
    ) -> Result<(String, Vec<i32>), AdoError> {
        let url = format!(
            "{}/test/Runs/{}/Results/{}?detailsToInclude=WorkItems&api-version=7.1",
            self.tp_base(org, project),
            run_id,
            result_id
        );
        let data = self.get_json(url).await?;
        let comment = data["comment"].as_str().unwrap_or_default().to_string();
        let bugs = data["associatedBugs"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|b| {
                b["id"]
                    .as_str()
                    .and_then(|s| s.parse::<i32>().ok())
                    .or_else(|| b["id"].as_i64().map(|i| i as i32))
            })
            .collect();
        Ok((comment, bugs))
    }

    pub async fn get_result(
        &self,
        org: &str,
        project: &str,
        run_id: i32,
        result_id: i32,
    ) -> Result<ResultDetail, AdoError> {
        let url = format!(
            "{}/test/Runs/{}/Results/{}?api-version=7.1",
            self.tp_base(org, project),
            run_id,
            result_id
        );
        let data = self.get_json(url).await?;
        Ok(ResultDetail {
            outcome: data["outcome"].as_str().unwrap_or_default().to_string(),
            comment: data["comment"].as_str().unwrap_or_default().to_string(),
        })
    }

    /// A result's previously-uploaded *image* attachments as b64 (v1
    /// get_result_screenshots). Non-image attachments skipped; download
    /// failures skipped (best-effort viewing). Read only.
    pub async fn get_result_screenshots(
        &self,
        org: &str,
        project: &str,
        run_id: i32,
        result_id: i32,
    ) -> Result<Vec<String>, AdoError> {
        use base64::Engine;
        let url = format!(
            "{}/test/Runs/{}/Results/{}/attachments?api-version=7.1",
            self.tp_base(org, project),
            run_id,
            result_id
        );
        let data = self.get_json(url).await?;
        let mut shots = vec![];
        for a in data["value"].as_array().cloned().unwrap_or_default() {
            let name = a["fileName"].as_str().unwrap_or_default().to_lowercase();
            if !(name.ends_with(".png")
                || name.ends_with(".jpg")
                || name.ends_with(".jpeg")
                || name.ends_with(".gif"))
            {
                continue;
            }
            let Some(id) = a["id"].as_i64() else { continue };
            let dl = format!(
                "{}/test/Runs/{}/Results/{}/attachments/{}?api-version=7.1",
                self.tp_base(org, project),
                run_id,
                result_id,
                id
            );
            let Ok(resp) = self
                .http
                .get(&dl)
                .bearer_auth(&self.token)
                .header("Accept", "application/octet-stream")
                .send()
                .await
            else {
                continue;
            };
            if !resp.status().is_success() {
                continue;
            }
            if let Ok(bytes) = resp.bytes().await {
                shots.push(base64::engine::general_purpose::STANDARD.encode(&bytes));
            }
        }
        Ok(shots)
    }

    /// Attach per-step (iteration) results so ADO's step-by-step view shows
    /// which steps passed/failed. Additive and best-effort. Plain-JSON PATCH.
    pub async fn update_result_steps(
        &self,
        org: &str,
        project: &str,
        run_id: i32,
        result_id: i32,
        iteration_details: serde_json::Value,
    ) -> Result<(), AdoError> {
        let body = serde_json::json!([{"id": result_id, "iterationDetails": iteration_details}]);
        let url = format!(
            "{}/test/Runs/{}/results?api-version=7.1",
            self.tp_base(org, project),
            run_id
        );
        self.patch_plain_json(url, &body).await?;
        Ok(())
    }

    /// POST a base64-encoded attachment (e.g. a screenshot) to a result.
    pub async fn add_result_attachment(
        &self,
        org: &str,
        project: &str,
        run_id: i32,
        result_id: i32,
        b64: &str,
        file_name: &str,
        comment: &str,
    ) -> Result<(), AdoError> {
        let mut body = serde_json::json!({
            "stream": b64,
            "fileName": file_name,
            "attachmentType": "GeneralAttachment",
        });
        if !comment.is_empty() {
            let capped: String = comment.chars().take(1000).collect();
            body["comment"] = serde_json::json!(capped);
        }
        let url = format!(
            "{}/test/Runs/{}/Results/{}/attachments?api-version=7.1",
            self.tp_base(org, project),
            run_id,
            result_id
        );
        self.post_json(url, &body).await?;
        Ok(())
    }

    /// PATCH the run to Completed (plain JSON).
    pub async fn complete_test_run(&self, org: &str, project: &str, run_id: i32) -> Result<(), AdoError> {
        let url = format!(
            "{}/test/runs/{}?api-version=7.1",
            self.tp_base(org, project),
            run_id
        );
        self.patch_plain_json(url, &serde_json::json!({"state": "Completed"}))
            .await?;
        Ok(())
    }
}
