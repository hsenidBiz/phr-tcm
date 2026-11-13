//! Test points, manual runs, results and result attachments.

use super::{
    attachment_kind, cap_comment, id_i32, is_no_verdict, OutcomeUpdate, ResultAttachment, ResultDetail, RunCreated, RunResultRef, TestPoint,
};
use crate::ado::{AdoClient, AdoError};

const MAX_THUMBS: usize = 12;
const MAX_THUMB_SOURCE_BYTES: u32 = 2 * 1024 * 1024;

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
        self.get_test_points_in(org, project, plan_id, suite_id, test_case_ids, false).await
    }

    /// Same, with `recursive` mapping to the endpoint's documented
    /// `isRecursive` flag: the points of every child suite come too, so a
    /// folder suite reads in one call.
    pub async fn get_test_points_in(
        &self,
        org: &str,
        project: &str,
        plan_id: i32,
        suite_id: i32,
        test_case_ids: &[i32],
        recursive: bool,
    ) -> Result<Vec<TestPoint>, AdoError> {
        let mut tc_filter = if test_case_ids.is_empty() {
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
        if recursive {
            tc_filter.push_str("&isRecursive=true");
        }
        let mut points = vec![];
        let mut continuation: Option<String> = None;
        let mut pages = 0usize;
        loop {
            let mut url = format!(
                "{}/testplan/Plans/{}/Suites/{}/TestPoint?api-version=7.1{}",
                self.tp_base(org, project),
                plan_id,
                suite_id,
                tc_filter
            );
            if let Some(c) = &continuation {
                super::push_continuation(&mut url, c);
            }
            let (data, cont) = self.get_json_with_continuation(url).await?;
            for p in data["value"].as_array().cloned().unwrap_or_default() {
                points.push(TestPoint {
                    point_id: p["id"].as_i64().unwrap_or_default() as i32,
                    test_case_id: p["testCaseReference"]["id"].as_i64().map(|i| i as i32),
                    test_case_name: p["testCaseReference"]["name"].as_str().unwrap_or_default().to_string(),
                    config_name: p["configuration"]["name"].as_str().unwrap_or_default().to_string(),
                    tester: p["tester"]["displayName"].as_str().unwrap_or_default().to_string(),
                    // A point with no verdict ("unspecified" or "None") reads
                    // as a real outcome in the UI, so strip it here.
                    last_outcome: {
                        let o = p["results"]["outcome"].as_str().unwrap_or_default();
                        if is_no_verdict(o) { String::new() } else { o.to_string() }
                    },
                    last_run_id: p["results"]["lastTestRunId"].as_i64().map(|i| i as i32),
                    last_result_id: p["results"]["lastResultId"].as_i64().map(|i| i as i32),
                    tags: String::new(),
                });
            }
            pages += 1;
            continuation = match cont {
                Some(next)
                    if super::may_continue(pages, continuation.as_ref(), &next, "test points") =>
                {
                    Some(next)
                }
                _ => break,
            };
        }
        Ok(points)
    }

    /// PATCH the given test points back to Active - ADO's own "reset test"
    /// mechanism (resetToActive). The point's outcome reads as never-run
    /// afterwards; nothing is deleted, and past run results stay intact.
    pub async fn reset_points_to_active(
        &self,
        org: &str,
        project: &str,
        plan_id: i32,
        suite_id: i32,
        point_ids: &[i32],
    ) -> Result<(), AdoError> {
        if point_ids.is_empty() {
            return Ok(());
        }
        let ids_csv = point_ids
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let url = format!(
            "{}/test/Plans/{}/Suites/{}/points/{}?api-version=7.1",
            self.tp_base(org, project),
            plan_id,
            suite_id,
            ids_csv
        );
        self.patch_plain_json(url, &serde_json::json!({"resetToActive": true}))
            .await?;
        Ok(())
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
            // Both filled in by the caller, which is what records and
            // attaches them.
            outcomes_unrecorded: vec![],
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
        let url = format!(
            "{}/test/Runs/{}/results?api-version=7.1",
            self.tp_base(org, project),
            run_id
        );
        let (rows, _) = self.get_skip_paged(url, "run results").await?;
        Ok(rows
            .iter()
            .map(|r| RunResultRef {
                result_id: r["id"].as_i64().unwrap_or_default() as i32,
                test_case_id: id_i32(&r["testCase"]["id"]),
                point_id: id_i32(&r["testPoint"]["id"]),
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

    /// Every attachment on a result: (id, file name, size). A failure to
    /// list is an error; nothing is downloaded here.
    async fn list_result_attachments(
        &self,
        org: &str,
        project: &str,
        run_id: i32,
        result_id: i32,
    ) -> Result<Vec<(i32, String, u32)>, AdoError> {
        let url = format!(
            "{}/test/Runs/{}/Results/{}/attachments?api-version=7.1",
            self.tp_base(org, project),
            run_id,
            result_id
        );
        let data = self.get_json(url).await?;
        let mut out = vec![];
        for a in data["value"].as_array().cloned().unwrap_or_default() {
            let Some(id) = a["id"].as_i64().and_then(|i| i32::try_from(i).ok()) else { continue };
            let name = a["fileName"].as_str().unwrap_or_default().to_string();
            let size = a["size"]
                .as_u64()
                .map(|n| u32::try_from(n).unwrap_or(u32::MAX))
                .unwrap_or(0);
            out.push((id, name, size));
        }
        Ok(out)
    }

    fn result_attachment_url(
        &self,
        org: &str,
        project: &str,
        run_id: i32,
        result_id: i32,
        id: i32,
    ) -> String {
        format!(
            "{}/test/Runs/{}/Results/{}/attachments/{}?api-version=7.1",
            self.tp_base(org, project),
            run_id,
            result_id,
            id
        )
    }

    /// All of a result's attachments with a thumbnail for each image. A
    /// thumbnail that cannot be read leaves the entry without one; a 401 or
    /// 429 stops further thumbnails (it is about the session) but the list
    /// is still returned. Read only.
    pub async fn get_result_attachments(
        &self,
        org: &str,
        project: &str,
        run_id: i32,
        result_id: i32,
    ) -> Result<Vec<ResultAttachment>, AdoError> {
        use base64::Engine;
        let listed = self.list_result_attachments(org, project, run_id, result_id).await?;
        let mut out = Vec::with_capacity(listed.len());
        let mut thumbs_ok = true;
        let mut thumbs_tried = 0usize;
        for (id, name, size) in listed {
            let kind = attachment_kind(&name);
            let mut thumb_b64 = None;
            // One IPC reply carries every thumbnail, so bound it: only the
            // first MAX_THUMBS images, and none whose file is large.
            if kind == "image" && thumbs_ok && thumbs_tried < MAX_THUMBS && size <= MAX_THUMB_SOURCE_BYTES {
                thumbs_tried += 1;
                let dl = self.result_attachment_url(org, project, run_id, result_id, id);
                match self.get_bytes(dl).await {
                    Ok(bytes) => {
                        thumb_b64 = Some(base64::engine::general_purpose::STANDARD.encode(&bytes))
                    }
                    Err(e @ (AdoError::Unauthorized | AdoError::RateLimited { .. })) => {
                        crate::applog::warn(format!(
                            "run {run_id} result {result_id}: thumbnails stopped: {e}"
                        ));
                        thumbs_ok = false;
                    }
                    Err(e) => crate::applog::warn(format!(
                        "run {run_id} result {result_id}: thumbnail {id} could not be read: {e}"
                    )),
                }
            }
            out.push(ResultAttachment {
                id,
                file_name: name,
                size_bytes: size,
                kind: kind.to_string(),
                thumb_b64,
            });
        }
        Ok(out)
    }

    /// Stream one attachment straight to `dest`; returns the bytes written.
    pub async fn download_result_attachment(
        &self,
        org: &str,
        project: &str,
        run_id: i32,
        result_id: i32,
        attachment_id: i32,
        size_bytes: u32,
        dest: &std::path::Path,
    ) -> Result<u64, AdoError> {
        let url = self.result_attachment_url(org, project, run_id, result_id, attachment_id);
        // An unknown size (0) gets the longest deadline, not the shortest.
        let deadline = if size_bytes == 0 {
            crate::ado::UPLOAD_TIMEOUT_CAP
        } else {
            crate::ado::upload_timeout(size_bytes as usize)
        };
        self.get_to_file(url, dest, deadline)
            .await
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
        let listed = self.list_result_attachments(org, project, run_id, result_id).await?;
        let mut shots = vec![];
        for (id, name, _) in listed {
            let name = name.to_lowercase();
            if !(name.ends_with(".png")
                || name.ends_with(".jpg")
                || name.ends_with(".jpeg")
                || name.ends_with(".gif"))
            {
                continue;
            }
            let dl = self.result_attachment_url(org, project, run_id, result_id, id);
            // Through the transport, so this is paced, logged, and a 401
            // becomes a re-sign-in rather than an empty screenshot strip.
            // A single picture failing must still not lose the others, so
            // the error is reported and the loop carries on.
            match self.get_bytes(dl).await {
                Ok(bytes) => shots.push(base64::engine::general_purpose::STANDARD.encode(&bytes)),
                Err(e @ (AdoError::Unauthorized | AdoError::RateLimited { .. })) => {
                    // About the SESSION, not this picture - swallowing them
                    // showed "no screenshots" to a tester whose token had
                    // simply expired. But returning Err discarded the ones
                    // already downloaded, and the strip then showed nothing
                    // at all rather than "three of five".
                    //
                    // So: keep what arrived, and only surface the error when
                    // there is nothing to show. A partial strip beats an
                    // empty one, and the next open retries the rest.
                    crate::applog::warn(format!(
                        "run {run_id} result {result_id}: stopped after {} screenshot(s): {e}",
                        shots.len()
                    ));
                    if shots.is_empty() {
                        return Err(e);
                    }
                    return Ok(shots);
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
        // A screen recording is uncapped and base64-inflated: give it a
        // deadline scaled to its size instead of the ordinary 60 s.
        let deadline = crate::ado::upload_timeout(b64.len());
        let resp = self
            .send(reqwest::Method::POST, &url, |r| {
                r.timeout(deadline)
                    .header("Accept", "application/json")
                    .json(&body)
            })
            .await?;
        Self::handle_json(resp).await?;
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
