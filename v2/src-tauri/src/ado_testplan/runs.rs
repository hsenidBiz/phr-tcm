//! Test points, manual runs, results and result attachments.

use super::{cap_comment, id_i32, OutcomeUpdate, ResultDetail, RunCreated, RunResultRef, TestPoint};
use crate::ado::{AdoClient, AdoError};

impl AdoClient {
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
            // Filled in by the caller, which is what actually attaches them.
            extras_failed: vec![],
        })
    }

    /// The results auto-created for a run. Read only.
    pub async fn get_run_results(
        &self,
        org: &str,
        project: &str,
        run_id: i32,
    ) -> Result<Vec<RunResultRef>, AdoError> {
        // PAGED. This is the Test Management API, which pages with
        // $top/$skip and does NOT send the continuation-token header the
        // paginated-testplan helper reads - so a run of more than one page
        // silently returned only the first, and every outcome beyond it was
        // dropped as "no matching result".
        const PAGE: usize = 200;
        let mut out: Vec<RunResultRef> = vec![];
        loop {
            let url = format!(
                "{}/test/Runs/{}/results?$top={PAGE}&$skip={}&api-version=7.1",
                self.tp_base(org, project),
                run_id,
                out.len()
            );
            let data = self.get_json(url).await?;
            let page = data["value"].as_array().cloned().unwrap_or_default();
            let got = page.len();
            out.extend(page.iter().map(|r| RunResultRef {
                result_id: r["id"].as_i64().unwrap_or_default() as i32,
                test_case_id: id_i32(&r["testCase"]["id"]),
                point_id: id_i32(&r["testPoint"]["id"]),
            }));
            if got < PAGE {
                return Ok(out);
            }
        }
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
                        item["comment"] = serde_json::json!(cap_comment(c));
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
            .filter_map(|b| id_i32(&b["id"]))
            .collect();
        Ok((comment, bugs))
    }

    /// A single result's last outcome + comment (runner preload). Read only.
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
            // Through the transport, so this is paced, logged, and a 401
            // becomes a re-sign-in rather than an empty screenshot strip.
            // A single picture failing must still not lose the others, so
            // the error is reported and the loop carries on.
            match self.get_bytes(dl).await {
                Ok(bytes) => shots.push(base64::engine::general_purpose::STANDARD.encode(&bytes)),
                Err(e @ (AdoError::Unauthorized | AdoError::RateLimited { .. })) => {
                    // These are about the SESSION, not this picture -
                    // swallowing them showed "no screenshots" to a tester
                    // whose token had simply expired. Propagated as-is so
                    // the caller keeps the Retry-After it was given.
                    return Err(e);
                }
                Err(e) => crate::applog::warn(format!(
                    "run {run_id} result {result_id}: screenshot {id} could not be read: {e}"
                )),
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
            body["comment"] = serde_json::json!(cap_comment(comment));
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
