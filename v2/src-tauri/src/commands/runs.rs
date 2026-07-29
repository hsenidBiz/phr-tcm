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
    // Every marked outcome must find its result. A filter_map here dropped
    // any that did not, with no error and no log line, and the run was then
    // reported as fully recorded - so a tester's result simply never
    // existed. Refuse instead: the run has been created either way, and
    // saying which cases are missing is the only way to act on it.
    let mut updates: Vec<ado_testplan::OutcomeUpdate> = Vec::with_capacity(outcomes.len());
    let mut unmatched: Vec<i32> = vec![];
    for o in &outcomes {
        match results.iter().find(|r| r.point_id == Some(o.point_id)) {
            Some(result) => updates.push(ado_testplan::OutcomeUpdate {
                id: result.result_id,
                outcome: o.outcome.clone(),
                comment: o.comment.clone(),
                duration_ms: o.duration_ms,
                bug_ids: o.bug_ids.clone(),
            }),
            None => unmatched.push(o.point_id),
        }
    }
    // Save what CAN be saved, FIRST.
    //
    // This used to return here the moment anything was unmatched, which was
    // worse than the silent filter_map it replaced: two unmatched points out
    // of eight meant none of the eight were written, complete_test_run never
    // ran so the run sat In Progress, and the message told the tester not to
    // mark again - so the six good results were lost as well, and the advice
    // kept them lost. Record the six, then say which two are missing.
    if !updates.is_empty() {
        client
            .update_run_results(&organization, &project, run.run_id, &updates)
            .await?;
    }

    if !unmatched.is_empty() {
        crate::applog::error(format!(
            "run {} has no result rows for test point(s) {unmatched:?} - {} of {} outcomes could not be recorded",
            run.run_id,
            unmatched.len(),
            outcomes.len(),
        ));
    }
    // Nothing at all was recorded: completing the run would leave an empty
    // Completed run in Azure DevOps and tell the tester their marks landed.
    // Erroring here costs nothing, because nothing was written - and the
    // runner keeps the marks so they can be sent again.
    if updates.is_empty() {
        return Err(ado::AdoError::Http {
            status: 0,
            body: format!(
                "Azure DevOps created no result row for any of the {} marked case(s), so \
                 nothing was recorded. Run #{} exists but is empty - mark them again rather \
                 than looking for results in it.",
                outcomes.len(),
                run.run_id,
            ),
        });
    }

    // Attachment and per-step failures, reported SEPARATELY from outcomes
    // that were never recorded. They were briefly the same list, and its
    // consumer frames every entry as "the outcomes were recorded, but this
    // did not attach - add it in Azure DevOps". Both halves of that are
    // false for a lost outcome.
    let mut extras_failed: Vec<String> = vec![];

    // Per-step outcomes + screenshots are additive and best-effort (v1
    // semantics): a failure here never loses the recorded outcomes. It was
    // also never REPORTED, so a tester who marked five steps individually
    // and attached a screenshot of the failure had no way to know that none
    // of it arrived. These genuinely ARE "recorded, but this did not
    // attach" - which is why an unrecorded outcome must not share the list.
    for o in &outcomes {
        let Some(result) = results.iter().find(|r| r.point_id == Some(o.point_id)) else {
            continue;
        };
        if let (Some(ids), Some(step_ocs)) = (&o.step_ids, &o.step_outcomes) {
            if let Some(details) =
                ado_testplan::build_iteration_details(ids, step_ocs, &o.outcome)
            {
                if let Err(e) = client
                    .update_result_steps(
                        &organization,
                        &project,
                        run.run_id,
                        result.result_id,
                        details,
                    )
                    .await
                {
                    crate::applog::warn(format!(
                        "run {}: per-step marks for point {} were not saved: {e}",
                        run.run_id, o.point_id
                    ));
                    extras_failed.push(format!("step-by-step marks for test point {}", o.point_id));
                }
            }
        }
        if let Some(files) = &o.attachments {
            for att in files {
                if let Err(e) = client
                    .add_result_attachment(
                        &organization,
                        &project,
                        run.run_id,
                        result.result_id,
                        &att.b64,
                        &att.file_name,
                        "",
                    )
                    .await
                {
                    crate::applog::warn(format!(
                        "run {}: attachment {} for point {} was not saved: {e}",
                        run.run_id, att.file_name, o.point_id
                    ));
                    extras_failed.push(format!("{} (test point {})", att.file_name, o.point_id));
                }
            }
        }
    }

    client
        .complete_test_run(&organization, &project, run.run_id)
        .await?;
    Ok(ado_testplan::RunCreated { outcomes_unrecorded: unmatched, extras_failed, ..run })
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
/// app the user just came from rather than a hardcoded light page.
#[tauri::command]
#[specta::specta]
pub async fn view_execution_report(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    plan_id: i32,
    suite_ids: Vec<i32>,
    title: String,
    palette: report::ReportPalette,
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
