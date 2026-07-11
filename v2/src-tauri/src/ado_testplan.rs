//! Test plans, requirement suites, test points and test runs - ported from
//! the v1 devops_client.py testplan/test-execution sections. Requirement
//! suites auto-populate from the PBI's "Tested By" links, which is what makes
//! created tests show on the board's test count. Only GET/POST/PATCH here -
//! no DELETE anywhere, same as the rest of the client.

use crate::ado::{AdoClient, AdoError};
use serde::Serialize;

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
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct EnsuredSuite {
    pub plan_id: i32,
    pub plan_name: String,
    pub suite_id: i32,
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
                });
            }
            continuation = cont;
            if continuation.is_none() {
                break;
            }
        }
        Ok(suites)
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
        let plans = self.get_test_plans(org, project).await?;
        // Area-matched plans first: the common case finds the suite quickly.
        let mut ordered: Vec<&TestPlan> = plans.iter().collect();
        ordered.sort_by_key(|p| if area_matches(&p.area_path, area_path) { 0 } else { 1 });
        for plan in &ordered {
            // A plan whose suites can't be listed (permissions) is skipped
            // rather than aborting the search; auth/rate-limit errors still
            // propagate so callers can re-auth / back off.
            match self.find_requirement_suite(org, project, plan.id, pbi_id).await {
                Ok(Some(suite)) => {
                    return Ok(EnsuredSuite {
                        plan_id: plan.id,
                        plan_name: plan.name.clone(),
                        suite_id: suite.id,
                    })
                }
                Ok(None) => {}
                Err(AdoError::Forbidden) | Err(AdoError::NotFound) => continue,
                Err(e) => return Err(e),
            }
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
                    last_outcome: p["results"]["outcome"].as_str().unwrap_or_default().to_string(),
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
