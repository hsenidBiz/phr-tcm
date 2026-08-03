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
    /// Passed / Failed / Paused / Blocked / NotApplicable - the verdicts
    /// Azure DevOps's own runner offers, and all real TestOutcome values.
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

/// A live run's identity plus every point's result row, so the runner can
/// PATCH one case at a time as the tester advances.
#[derive(serde::Serialize, specta::Type)]
pub struct RunStarted {
    pub run_id: i32,
    pub web_url: String,
    /// point_id -> result_id, flattened to pairs for the bindings.
    pub results: Vec<PointResult>,
    /// Points Azure DevOps created no result row for - marks against these
    /// can NEVER be recorded in this run, and the runner says so up front
    /// instead of discovering it at the end.
    pub unmatched: Vec<i32>,
}

#[derive(serde::Serialize, specta::Type)]
pub struct PointResult {
    pub point_id: i32,
    pub result_id: i32,
}

/// Open a run over the session's points WITHOUT completing it - the
/// incremental half of what submit_test_run did in one shot. The runner
/// calls this lazily on the first recorded outcome, then `record_result`
/// per case as the tester clicks Next, then `finish_test_run`.
///
/// A run left open (window closed mid-session) stays In Progress in Azure
/// DevOps - which is what ADO's own runner does with a paused session,
/// and every already-recorded outcome is already saved.
#[tauri::command]
#[specta::specta]
pub async fn start_test_run(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    plan_id: i32,
    run_name: String,
    point_ids: Vec<i32>,
) -> Result<RunStarted, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    let client = ado::AdoClient::new(token);
    let run = client
        .create_test_run(&organization, &project, plan_id, &run_name, &point_ids)
        .await?;
    let rows = client.get_run_results(&organization, &project, run.run_id).await?;
    let results: Vec<PointResult> = rows
        .iter()
        .filter_map(|r| r.point_id.map(|p| PointResult { point_id: p, result_id: r.result_id }))
        .collect();
    let unmatched = point_ids
        .iter()
        .copied()
        .filter(|p| !results.iter().any(|r| r.point_id == *p))
        .collect();
    Ok(RunStarted { run_id: run.run_id, web_url: run.web_url, results, unmatched })
}

/// Record ONE case's outcome into a live run - the write behind the Next
/// button. Idempotent by nature: going back and changing a verdict PATCHes
/// the same result row again. Per-step marks and attachments are additive
/// and best-effort exactly as in the batch flow; a failure there never
/// loses the recorded outcome, and the returned list names what did not
/// attach.
#[tauri::command]
#[specta::specta]
pub async fn record_result(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    run_id: i32,
    result_id: i32,
    outcome: PointOutcome,
) -> Result<Vec<String>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    let client = ado::AdoClient::new(token);
    client
        .update_run_results(
            &organization,
            &project,
            run_id,
            &[ado_testplan::OutcomeUpdate {
                id: result_id,
                outcome: outcome.outcome.clone(),
                comment: outcome.comment.clone(),
                duration_ms: outcome.duration_ms,
                bug_ids: outcome.bug_ids.clone(),
            }],
        )
        .await?;

    let mut extras_failed: Vec<String> = vec![];
    if let (Some(ids), Some(step_ocs)) = (&outcome.step_ids, &outcome.step_outcomes) {
        if let Some(details) = ado_testplan::build_iteration_details(ids, step_ocs, &outcome.outcome)
        {
            if let Err(e) = client
                .update_result_steps(&organization, &project, run_id, result_id, details)
                .await
            {
                crate::applog::warn(format!(
                    "run {run_id}: per-step marks for point {} were not saved: {e}",
                    outcome.point_id
                ));
                extras_failed.push(format!("step-by-step marks for test point {}", outcome.point_id));
            }
        }
    }
    if let Some(files) = &outcome.attachments {
        for att in files {
            if let Err(e) = client
                .add_result_attachment(
                    &organization,
                    &project,
                    run_id,
                    result_id,
                    &att.b64,
                    &att.file_name,
                    "",
                )
                .await
            {
                crate::applog::warn(format!(
                    "run {run_id}: attachment {} for point {} was not saved: {e}",
                    att.file_name, outcome.point_id
                ));
                extras_failed.push(format!("{} (test point {})", att.file_name, outcome.point_id));
            }
        }
    }
    Ok(extras_failed)
}

/// Close a live run. Refused for a run nothing was recorded into - the
/// runner tracks that and never calls this before the first record.
#[tauri::command]
#[specta::specta]
pub async fn finish_test_run(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    run_id: i32,
) -> Result<(), ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .complete_test_run(&organization, &project, run_id)
        .await
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
/// `palette` is the app's live theme, so the page opens looking like the
/// app the user just came from rather than a hardcoded light page - and
/// carries the other scheme too, for the switch in the page's corner.
#[tauri::command]
#[specta::specta]
pub async fn view_execution_report(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    plan_id: i32,
    suite_ids: Vec<i32>,
    title: String,
    palette: crate::webtheme::PagePalette,
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
        &palette,
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
