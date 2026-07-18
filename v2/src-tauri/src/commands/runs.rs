//! Test execution: points, runs, results, history, and the HTML
//! execution report.

use crate::state::get_fresh_token;
use crate::{ado, ado_testplan, report};

/// The last run's comment + linked bugs for one test point, so Run Tests can
/// show why a case failed without opening the runner. Read only.
#[derive(serde::Serialize, specta::Type)]
pub struct ResultFailureDetail {
    pub comment: String,
    pub bug_ids: Vec<i32>,
}

#[derive(Clone, serde::Deserialize, specta::Type)]
pub struct RunAttachment {
    pub file_name: String,
    pub b64: String,
}

#[derive(serde::Deserialize, specta::Type)]
pub struct PointOutcome {
    pub point_id: i32,
    /// Passed / Failed / Blocked / NotApplicable.
    pub outcome: String,
    pub comment: Option<String>,
    pub duration_ms: Option<i32>,
    /// The case's real step ids (from TestCaseFull.step_ids), aligned with
    /// step_outcomes; both present only when steps were marked individually.
    pub step_ids: Option<Vec<String>>,
    pub step_outcomes: Option<Vec<Option<String>>>,
    /// Files (screenshots or anything else) to attach to this result.
    pub attachments: Option<Vec<RunAttachment>>,
    /// Bug work-item ids to associate with this result.
    pub bug_ids: Option<Vec<i32>>,
}

#[tauri::command]
#[specta::specta]
pub async fn list_test_points(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    plan_id: i32,
    suite_id: i32,
) -> Result<Vec<ado_testplan::TestPoint>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .get_test_points(&organization, &project, plan_id, suite_id, &[])
        .await
}

/// Full manual-run lifecycle ported from v1 run_screen submission: create a
/// run seeded from the points, map each point to its auto-created result,
/// PATCH outcomes, complete the run. Returns the run's web URL.
#[tauri::command]
#[specta::specta]
pub async fn submit_test_run(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    plan_id: i32,
    run_name: String,
    outcomes: Vec<PointOutcome>,
) -> Result<ado_testplan::RunCreated, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    let client = ado::AdoClient::new(token);
    let point_ids: Vec<i32> = outcomes.iter().map(|o| o.point_id).collect();
    let run = client
        .create_test_run(&organization, &project, plan_id, &run_name, &point_ids)
        .await?;
    let results = client
        .get_run_results(&organization, &project, run.run_id)
        .await?;
    let updates: Vec<ado_testplan::OutcomeUpdate> = outcomes
        .iter()
        .filter_map(|o| {
            let result = results.iter().find(|r| r.point_id == Some(o.point_id))?;
            Some(ado_testplan::OutcomeUpdate {
                id: result.result_id,
                outcome: o.outcome.clone(),
                comment: o.comment.clone(),
                duration_ms: o.duration_ms,
                bug_ids: o.bug_ids.clone(),
            })
        })
        .collect();
    client
        .update_run_results(&organization, &project, run.run_id, &updates)
        .await?;

    // Per-step outcomes + screenshots are additive and best-effort (v1
    // semantics): a failure here never loses the recorded outcomes.
    for o in &outcomes {
        let Some(result) = results.iter().find(|r| r.point_id == Some(o.point_id)) else {
            continue;
        };
        if let (Some(ids), Some(step_ocs)) = (&o.step_ids, &o.step_outcomes) {
            if let Some(details) =
                ado_testplan::build_iteration_details(ids, step_ocs, &o.outcome)
            {
                let _ = client
                    .update_result_steps(
                        &organization,
                        &project,
                        run.run_id,
                        result.result_id,
                        details,
                    )
                    .await;
            }
        }
        if let Some(files) = &o.attachments {
            for att in files {
                let _ = client
                    .add_result_attachment(
                        &organization,
                        &project,
                        run.run_id,
                        result.result_id,
                        &att.b64,
                        &att.file_name,
                        "",
                    )
                    .await;
            }
        }
    }

    client
        .complete_test_run(&organization, &project, run.run_id)
        .await?;
    Ok(run)
}

#[tauri::command]
#[specta::specta]
pub async fn get_result_detail(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    run_id: i32,
    result_id: i32,
) -> Result<ado_testplan::ResultDetail, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .get_result(&organization, &project, run_id, result_id)
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn result_failure_detail(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    run_id: i32,
    result_id: i32,
) -> Result<ResultFailureDetail, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    let (comment, bug_ids) = ado::AdoClient::new(token)
        .get_result_report_info(&organization, &project, run_id, result_id)
        .await?;
    Ok(ResultFailureDetail { comment, bug_ids })
}

#[tauri::command]
#[specta::specta]
pub async fn result_screenshots(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    run_id: i32,
    result_id: i32,
) -> Result<Vec<String>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .get_result_screenshots(&organization, &project, run_id, result_id)
        .await
}

/// Recent outcome history per test case for a plan (last 5, newest first).
#[tauri::command]
#[specta::specta]
pub async fn run_history(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    plan_id: i32,
) -> Result<Vec<ado_testplan::CaseHistory>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .run_history(&organization, &project, plan_id)
        .await
}

/// Execution report for one or more suites (a folder passes all its
/// descendants): gathers points + failure details (comments, linked bugs),
/// renders the failures-first HTML to a temp file and opens the browser.
/// GET-only against ADO; writes only the local temp file.
#[tauri::command]
#[specta::specta]
pub async fn view_execution_report(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    plan_id: i32,
    suite_ids: Vec<i32>,
    title: String,
) -> Result<(), String> {
    let token = get_fresh_token(&app).await.map_err(|e| e.to_string())?;
    let client = ado::AdoClient::new(token);

    // Points per suite (deduped across a folder's overlapping suites).
    let mut points: Vec<ado_testplan::TestPoint> = vec![];
    let mut seen = std::collections::HashSet::new();
    for sid in &suite_ids {
        let pts = client
            .get_test_points(&organization, &project, plan_id, *sid, &[])
            .await
            .map_err(|e| e.to_string())?;
        for p in pts {
            if seen.insert(p.point_id) {
                points.push(p);
            }
        }
    }

    // Failure details (comment + linked bugs) for failed points only.
    let mut failures = std::collections::HashMap::new();
    for p in &points {
        if !p.last_outcome.eq_ignore_ascii_case("failed") {
            continue;
        }
        if let (Some(run), Some(res)) = (p.last_run_id, p.last_result_id) {
            if let Ok((comment, bug_ids)) = client
                .get_result_report_info(&organization, &project, run, res)
                .await
            {
                failures.insert(p.point_id, report::FailureInfo { comment, bug_ids });
            }
        }
    }

    let generated_at = {
        use std::time::{SystemTime, UNIX_EPOCH};
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        report::format_epoch_utc(secs)
    };
    let html = report::build_report_html(
        &title,
        &organization,
        &project,
        &points,
        &failures,
        &generated_at,
    );
    let path = std::env::temp_dir().join(format!(
        "execution-report-{}-{}.html",
        std::process::id(),
        points.len()
    ));
    std::fs::write(&path, html).map_err(|e| e.to_string())?;
    tauri_plugin_opener::open_path(path.to_string_lossy().as_ref(), None::<&str>)
        .map_err(|e| e.to_string())
}
