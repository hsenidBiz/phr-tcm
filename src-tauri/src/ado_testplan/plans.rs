//! Plan and suite discovery, and find-or-create of the PBI's
//! requirement-based suite.

use futures::stream::{self, StreamExt};

use super::{
    area_matches, default_plan_name, EnsuredSuite, PlanWithSuites, SuiteRef, TestPlan,
    SUITE_SCAN_CONCURRENCY,
};
use crate::ado::{AdoClient, AdoError};

impl AdoClient {
    /// All test plans in the project (paginated). Read only.
    pub async fn get_test_plans(&self, org: &str, project: &str) -> Result<Vec<TestPlan>, AdoError> {
        let mut plans = vec![];
        let mut continuation: Option<String> = None;
        let mut pages = 0usize;
        loop {
            let mut url = format!("{}/testplan/plans?api-version=7.1", self.tp_base(org, project));
            if let Some(c) = &continuation {
                super::push_continuation(&mut url, c);
            }
            let (data, cont) = self.get_json_with_continuation(url).await?;
            for p in data["value"].as_array().cloned().unwrap_or_default() {
                plans.push(TestPlan {
                    id: p["id"].as_i64().unwrap_or_default() as i32,
                    name: p["name"].as_str().unwrap_or_default().to_string(),
                    area_path: p["areaPath"].as_str().unwrap_or_default().to_string(),
                    root_suite_id: p["rootSuite"]["id"].as_i64().map(|i| i as i32),
                    iteration: p["iteration"].as_str().unwrap_or_default().to_string(),
                    state: p["state"].as_str().unwrap_or_default().to_string(),
                });
            }
            pages += 1;
            continuation = match cont {
                Some(next) if super::may_continue(pages, continuation.as_ref(), &next, "test plans") => {
                    Some(next)
                }
                _ => break,
            };
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
                    iteration: data["iteration"].as_str().unwrap_or_default().to_string(),
                    state: data["state"].as_str().unwrap_or_default().to_string(),
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
        let mut pages = 0usize;
        loop {
            let mut url = format!(
                "{}/testplan/Plans/{}/suites?api-version=7.1",
                self.tp_base(org, project),
                plan_id
            );
            if let Some(c) = &continuation {
                super::push_continuation(&mut url, c);
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
            pages += 1;
            continuation = match cont {
                Some(next)
                    if super::may_continue(pages, continuation.as_ref(), &next, "plan suites") =>
                {
                    Some(next)
                }
                _ => return Ok(None),
            };
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
        let mut pages = 0usize;
        loop {
            let mut url = format!(
                "{}/testplan/Plans/{}/suites?api-version=7.1",
                self.tp_base(org, project),
                plan_id
            );
            if let Some(c) = &continuation {
                super::push_continuation(&mut url, c);
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
            pages += 1;
            continuation = match cont {
                Some(next)
                    if super::may_continue(pages, continuation.as_ref(), &next, "plan suites") =>
                {
                    Some(next)
                }
                _ => break,
            };
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
                    iteration: data["iteration"].as_str().unwrap_or_default().to_string(),
                    state: data["state"].as_str().unwrap_or_default().to_string(),
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
                            created_plan: false,
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

        // No suite anywhere. Every area-matched plan is a candidate, ranked
        // so the first is the one a person would pick: the most specific
        // area, then the plan whose iteration is the PBI's, then active
        // plans, then the newest. The old rule took the first plan the API
        // listed among equally specific ones - the OLDEST - which in one
        // project was a years-old plan the user could not create suites in:
        // the 403 was a warning in the log, 197 cases were created and
        // linked, and no suite appeared anywhere.
        let mut candidates: Vec<&TestPlan> = plans
            .iter()
            .filter(|p| area_matches(&p.area_path, area_path))
            .collect();
        candidates.sort_by(|a, b| {
            b.area_path
                .len()
                .cmp(&a.area_path.len())
                .then_with(|| {
                    iteration_matches(&b.iteration, iteration)
                        .cmp(&iteration_matches(&a.iteration, iteration))
                })
                .then_with(|| is_active(&b.state).cmp(&is_active(&a.state)))
                .then_with(|| b.id.cmp(&a.id))
        });
        if candidates.is_empty() {
            let plan = self
                .create_test_plan(org, project, &default_plan_name(area_path), area_path, iteration)
                .await?;
            return self.suite_under(org, project, &plan, pbi_id, true).await;
        }
        // Try each candidate in turn. A 403 on one plan says nothing about
        // the next - plans have owners and their own permissions - so it
        // moves on; anything else propagates unchanged, so the caller keeps
        // telling 401 and 429 apart from the rest.
        let mut forbidden: Vec<String> = vec![];
        for p in candidates {
            let plan = if p.root_suite_id.is_some() {
                p.clone()
            } else {
                self.get_test_plan(org, project, p.id).await?
            };
            match self.suite_under(org, project, &plan, pbi_id, false).await {
                Ok(ensured) => return Ok(ensured),
                Err(AdoError::Forbidden) => {
                    crate::applog::warn(format!(
                        "no permission to create a suite in test plan '{}' (id {}) - trying the next plan for this area",
                        plan.name, plan.id
                    ));
                    forbidden.push(format!("'{}' (id {})", plan.name, plan.id));
                }
                Err(e) => return Err(e),
            }
        }
        // The one case a 403 is the whole answer: name the plans, so the
        // person knows whose door to knock on. Http rather than Forbidden
        // so the sentence reaches the screen - `describeAdoError` prints a
        // Forbidden as a bare "no permission" with nothing to act on.
        Err(AdoError::Http {
            status: 403,
            body: format!(
                "You don't have permission to create a test suite in {} - the test plan{} for this area. \
                 The test cases are linked to #{pbi_id}, but they will not appear in Run Tests until a \
                 requirement suite exists: ask the plan owner for access, or create the suite in Azure DevOps.",
                forbidden.join(", "),
                if forbidden.len() == 1 { "" } else { "s" }
            ),
        })
    }

    /// The requirement suite for `pbi_id` under `plan`'s root suite.
    /// `created_plan` is whether this call created the plan too - a
    /// failure past that point leaves an empty test plan behind that this
    /// tool cannot delete, so it is named in the log and the error rather
    /// than left to appear out of nowhere.
    async fn suite_under(
        &self,
        org: &str,
        project: &str,
        plan: &TestPlan,
        pbi_id: i32,
        created_plan: bool,
    ) -> Result<EnsuredSuite, AdoError> {
        let orphan = |what: &str| {
            if created_plan {
                crate::applog::error(format!(
                    "left test plan '{}' (id {}) in {project} with no requirement suite - {what}. \
                     This tool never deletes; remove it in Azure DevOps if it is not wanted.",
                    plan.name, plan.id
                ));
            }
        };
        let root = match plan.root_suite_id {
            Some(r) => r,
            None => {
                orphan("it has no root suite");
                return Err(AdoError::Http {
                    status: 0,
                    body: format!(
                        "test plan '{}' (id {}) has no root suite, so the requirement suite for \
                         #{pbi_id} could not be created.",
                        plan.name, plan.id
                    ),
                });
            }
        };
        let suite_id = match self.create_requirement_suite(org, project, plan.id, root, pbi_id).await {
            Ok(id) => id,
            Err(e) => {
                orphan(&format!("creating the suite failed: {e}"));
                return Err(e);
            }
        };
        Ok(EnsuredSuite {
            plan_id: plan.id,
            plan_name: plan.name.clone(),
            suite_id,
            created_plan,
        })
    }
}

/// Does the plan's iteration cover the PBI's? Either may be the other's
/// ancestor: a plan on "Proj\\2026" covers a PBI in "Proj\\2026\\S3",
/// and a plan pinned to the sprint covers the PBI on it. Case-insensitive,
/// like every path Azure DevOps hands back.
fn iteration_matches(plan_iteration: &str, pbi_iteration: &str) -> bool {
    let a = plan_iteration.trim().to_lowercase();
    let b = pbi_iteration.trim().to_lowercase();
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || b.starts_with(&format!("{a}\\")) || a.starts_with(&format!("{b}\\"))
}

/// Unknown counts as active - the list API has always sent the state, but
/// a plan without one should not lose a tie to one that has it.
fn is_active(state: &str) -> bool {
    state.is_empty() || state.eq_ignore_ascii_case("active")
}

