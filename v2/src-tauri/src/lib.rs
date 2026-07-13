pub mod ado;
pub mod ado_testplan;
pub mod audio;
pub mod auth;
pub mod capture;
pub mod import_parser;
pub mod model;
pub mod steps_xml;
pub mod report;
pub mod updater;
pub mod work_board;

use std::sync::Mutex;
use std::time::Instant;
use tauri::Manager;
use tauri_specta::{collect_commands, collect_events, Builder, Event};

/// Cooperative cancel for the submit loop: checked between items, so the
/// in-flight item always completes (never a half-created case).
#[derive(Default)]
pub struct SubmitCancel(std::sync::atomic::AtomicBool);

/// Emitted once per queue item while submit_queue runs.
#[derive(Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct SubmitProgress {
    pub index: u32,
    pub total: u32,
    pub title: String,
    /// "created" | "updated" | "failed"
    pub action: String,
}

/// Emitted while test plans are being scanned for suites, so Run Tests and
/// the Suites browser can show "Scanning plans X of Y" instead of a bare
/// skeleton.
#[derive(Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct SuiteScanProgress {
    pub done: u32,
    pub total: u32,
}

#[derive(serde::Serialize, specta::Type)]
pub struct AuthStatus {
    pub signed_in: bool,
    pub account: Option<String>,
}

fn status_from(state: &auth::AuthState) -> AuthStatus {
    AuthStatus {
        signed_in: state.tokens.is_some(),
        account: state.tokens.as_ref().and_then(|t| t.account.clone()),
    }
}

/// Returns a valid access token, silently refreshing when it is within
/// 5 minutes of expiry. The token itself never leaves the Rust side.
async fn get_fresh_token(app: &tauri::AppHandle) -> Result<String, ado::AdoError> {
    let (token, refresh_needed, refresh_token, account) = {
        let state = app.state::<Mutex<auth::AuthState>>();
        let s = state.lock().unwrap();
        match &s.tokens {
            None => return Err(ado::AdoError::Unauthorized),
            Some(t) => (
                t.access_token.clone(),
                auth::needs_refresh(t.expires_at, Instant::now()),
                t.refresh_token.clone(),
                t.account.clone(),
            ),
        }
    };
    if !refresh_needed {
        return Ok(token);
    }
    let Some(rt) = refresh_token else {
        // No refresh token: keep using the current one until it hard-fails.
        return Ok(token);
    };
    match auth::refresh(&rt, account).await {
        Ok(new_tokens) => {
            let fresh = new_tokens.access_token.clone();
            let state = app.state::<Mutex<auth::AuthState>>();
            state.lock().unwrap().tokens = Some(new_tokens);
            Ok(fresh)
        }
        // Refresh failed (revoked, offline, CAE): fall back to the existing
        // token; a hard 401 from the API will surface as Unauthorized.
        Err(_) => Ok(token),
    }
}

#[tauri::command]
#[specta::specta]
fn ping(msg: String) -> String {
    format!("pong: {msg}")
}

#[tauri::command]
#[specta::specta]
fn auth_status(state: tauri::State<'_, Mutex<auth::AuthState>>) -> AuthStatus {
    status_from(&state.lock().unwrap())
}

#[tauri::command]
#[specta::specta]
async fn sign_in(app: tauri::AppHandle) -> Result<AuthStatus, String> {
    let tokens = auth::sign_in_interactive(|url| {
        let _ = tauri_plugin_opener::open_url(url, None::<&str>);
    })
    .await?;
    let state = app.state::<Mutex<auth::AuthState>>();
    let mut s = state.lock().unwrap();
    s.tokens = Some(tokens);
    Ok(status_from(&s))
}

#[tauri::command]
#[specta::specta]
async fn list_projects(
    app: tauri::AppHandle,
    organization: String,
) -> Result<Vec<ado::Project>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token).get_projects(&organization).await
}

#[tauri::command]
#[specta::specta]
async fn list_orgs(app: tauri::AppHandle) -> Result<Vec<ado::Org>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token).list_orgs().await
}

#[tauri::command]
#[specta::specta]
async fn search_pbis(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    query: String,
) -> Result<Vec<ado::PbiHit>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .search_pbis(&organization, &project, &query, 20)
        .await
}

#[tauri::command]
#[specta::specta]
async fn pbi_test_cases(
    app: tauri::AppHandle,
    organization: String,
    pbi_id: i32,
) -> Result<Vec<ado::TestCaseSummary>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .get_pbi_test_cases(&organization, pbi_id)
        .await
}

#[tauri::command]
#[specta::specta]
fn parse_import_file(path: String) -> Result<ImportResult, String> {
    let (cases, warnings) = import_parser::parse_file(&path)?;
    Ok(ImportResult { cases, warnings })
}

#[derive(serde::Serialize, specta::Type)]
pub struct ImportResult {
    pub cases: Vec<model::TestCase>,
    pub warnings: Vec<String>,
}

#[tauri::command]
#[specta::specta]
fn export_queue(path: String, queue: Vec<model::TestCase>) -> Result<(), String> {
    import_parser::export_queue_to_excel(&queue, &path)
}

#[tauri::command]
#[specta::specta]
fn write_template(path: String) -> Result<(), String> {
    import_parser::generate_template(&path)
}

#[derive(serde::Serialize, specta::Type)]
pub struct SubmitItemResult {
    pub index: u32,
    pub title: String,
    /// "created" | "updated" | "failed"
    pub action: String,
    pub id: Option<i32>,
    pub error: Option<String>,
}

/// Serial creation loop ported from v1 CreationWorker: one item at a time,
/// 500 ms spacing (rate-limit respect), new cases linked to the PBI, updates
/// patched in place. A failed item never aborts the rest.
#[tauri::command]
#[specta::specta]
async fn submit_queue(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    pbi_id: i32,
    queue: Vec<model::TestCase>,
    module_ref: Option<String>,
    preconditions_ref: Option<String>,
    area_path: Option<String>,
    iteration_path: Option<String>,
) -> Result<Vec<SubmitItemResult>, String> {
    // Best-effort board visibility (ported from v1 CreationWorker._ensure_suite):
    // make sure the PBI's requirement-based suite exists before creating, so
    // linked cases surface on the board's test count. Failures never block
    // creation. The PBI's own paths double as the default area/iteration for
    // created cases (unset picker = "Same as PBI").
    let mut pbi_area = String::new();
    let mut pbi_iteration = String::new();
    if let Ok(token) = get_fresh_token(&app).await {
        let client = ado::AdoClient::new(token);
        if let Ok((area, iteration)) = client
            .get_work_item_paths(&organization, &project, pbi_id)
            .await
        {
            let _ = client
                .ensure_requirement_suite(&organization, &project, pbi_id, &area, &iteration)
                .await;
            pbi_area = area;
            pbi_iteration = iteration;
        }
    }
    let effective_area = area_path.filter(|s| !s.is_empty()).unwrap_or(pbi_area);
    let effective_iteration = iteration_path
        .filter(|s| !s.is_empty())
        .unwrap_or(pbi_iteration);

    let cancel = app.state::<SubmitCancel>();
    cancel.0.store(false, std::sync::atomic::Ordering::SeqCst);

    let total = queue.len() as u32;
    let mut results: Vec<SubmitItemResult> = vec![];
    for (i, tc) in queue.iter().enumerate() {
        if cancel.0.load(std::sync::atomic::Ordering::SeqCst) {
            break; // unprocessed items stay in the client's queue
        }
        if i > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        let item = process_queue_item(
            &app,
            &organization,
            &project,
            pbi_id,
            i as u32,
            tc,
            module_ref.as_deref(),
            preconditions_ref.as_deref(),
            &effective_area,
            &effective_iteration,
        )
        .await;
        let _ = SubmitProgress {
            index: item.index,
            total,
            title: item.title.clone(),
            action: item.action.clone(),
        }
        .emit(&app);
        results.push(item);
    }
    Ok(results)
}

#[allow(clippy::too_many_arguments)]
async fn process_queue_item(
    app: &tauri::AppHandle,
    organization: &str,
    project: &str,
    pbi_id: i32,
    index: u32,
    tc: &model::TestCase,
    m_ref: Option<&str>,
    p_ref: Option<&str>,
    area_path: &str,
    iteration_path: &str,
) -> SubmitItemResult {
    let failed = |error: String| SubmitItemResult {
        index,
        title: tc.title.clone(),
        action: "failed".into(),
        id: None,
        error: Some(error),
    };
    if let Err(msg) = tc.is_valid() {
        return failed(msg);
    }
    let token = match get_fresh_token(app).await {
        Ok(t) => t,
        Err(e) => return failed(e.to_string()),
    };
    let client = ado::AdoClient::new(token);
    let outcome = match tc.update_id {
        Some(existing_id) => client
            .update_test_case_from_model(organization, project, existing_id, tc, m_ref, p_ref)
            .await
            .map(|_| (existing_id, "updated")),
        None => match client
            .create_test_case(organization, project, tc, m_ref, area_path, iteration_path, p_ref)
            .await
        {
            Ok(new_id) => client
                .link_to_pbi(organization, project, new_id, pbi_id)
                .await
                .map(|_| (new_id, "created")),
            Err(e) => Err(e),
        },
    };
    match outcome {
        Ok((id, action)) => SubmitItemResult {
            index,
            title: tc.title.clone(),
            action: action.into(),
            id: Some(id),
            error: None,
        },
        Err(e) => failed(e.to_string()),
    }
}

#[tauri::command]
#[specta::specta]
async fn list_test_case_fields(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<Vec<ado::FieldRef>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .get_test_case_fields(&organization, &project)
        .await
}

#[tauri::command]
#[specta::specta]
async fn pbi_test_cases_full(
    app: tauri::AppHandle,
    organization: String,
    pbi_id: i32,
    module_ref: Option<String>,
    preconditions_ref: Option<String>,
) -> Result<Vec<ado::TestCaseFull>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .get_pbi_test_cases_full(
            &organization,
            pbi_id,
            module_ref.as_deref(),
            preconditions_ref.as_deref(),
        )
        .await
}

/// Save one existing case from the editor (no suite-ensure, no pacing).
/// The case must carry update_id; blank-skip semantics apply as always.
#[tauri::command]
#[specta::specta]
async fn update_test_case(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    tc: model::TestCase,
    module_ref: Option<String>,
    preconditions_ref: Option<String>,
) -> Result<(), String> {
    let id = tc.update_id.ok_or("update_test_case requires update_id")?;
    tc.is_valid()?;
    let token = get_fresh_token(&app).await.map_err(|e| e.to_string())?;
    ado::AdoClient::new(token)
        .update_test_case_from_model(
            &organization,
            &project,
            id,
            &tc,
            module_ref.as_deref(),
            preconditions_ref.as_deref(),
        )
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn export_queue_json(path: String, queue: Vec<model::TestCase>) -> Result<(), String> {
    import_parser::export_queue_to_json(&queue, &path)
}

/// Render the queue's HTML report to a temp file and open it in the
/// default browser - v1's "View" behaviour, no save dialog.
#[tauri::command]
#[specta::specta]
fn view_queue_html(queue: Vec<model::TestCase>, subtitle: String) -> Result<(), String> {
    let path = std::env::temp_dir().join(format!(
        "test-cases-{}-{}.html",
        std::process::id(),
        queue.len()
    ));
    let path_str = path.to_string_lossy().to_string();
    import_parser::export_queue_to_html(&queue, &path_str, &subtitle)?;
    tauri_plugin_opener::open_path(&path_str, None::<&str>).map_err(|e| e.to_string())
}

/// Test cases for arbitrary ids (suite browser handoffs).
#[tauri::command]
#[specta::specta]
async fn test_cases_by_ids(
    app: tauri::AppHandle,
    organization: String,
    ids: Vec<i32>,
    module_ref: Option<String>,
    preconditions_ref: Option<String>,
) -> Result<Vec<ado::TestCaseFull>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .get_test_cases_by_ids(
            &organization,
            &ids,
            module_ref.as_deref(),
            preconditions_ref.as_deref(),
        )
        .await
}

/// Values for ANY Test Case field: the definition's picklist when one
/// exists, otherwise the distinct values in use on the project's Test
/// Cases (many orgs keep Modules as plain values, not allowedValues).
#[tauri::command]
#[specta::specta]
async fn test_case_field_values(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    field_ref: String,
) -> Result<Vec<String>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    let client = ado::AdoClient::new(token);
    let picklist = client
        .get_field_allowed_values(&organization, &project, "Test Case", &field_ref)
        .await?;
    if !picklist.is_empty() {
        return Ok(picklist);
    }
    client
        .field_values_in_use(&organization, &project, &field_ref)
        .await
}

/// Read any file for attaching to a result (name + base64 bytes).
#[tauri::command]
#[specta::specta]
fn read_file_b64(path: String) -> Result<RunAttachmentOut, String> {
    use base64::Engine;
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    if bytes.len() > 25 * 1024 * 1024 {
        return Err("File is larger than 25 MB.".into());
    }
    let file_name = std::path::Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "attachment".into());
    Ok(RunAttachmentOut {
        file_name,
        b64: base64::engine::general_purpose::STANDARD.encode(&bytes),
    })
}

#[derive(serde::Serialize, specta::Type)]
pub struct RunAttachmentOut {
    pub file_name: String,
    pub b64: String,
}

/// Launch the Windows snipping overlay (result lands on the clipboard; the
/// runner polls and attaches it).
#[tauri::command]
#[specta::specta]
fn open_snip() -> Result<(), String> {
    tauri_plugin_opener::open_url("ms-screenclip:", None::<&str>).map_err(|e| e.to_string())
}

/// Stop the running submit loop after the in-flight item finishes.
#[tauri::command]
#[specta::specta]
fn cancel_submit(state: tauri::State<'_, SubmitCancel>) {
    state.0.store(true, std::sync::atomic::Ordering::SeqCst);
}

#[tauri::command]
#[specta::specta]
fn export_queue_html(path: String, queue: Vec<model::TestCase>, subtitle: String) -> Result<(), String> {
    import_parser::export_queue_to_html(&queue, &path, &subtitle)
}

#[tauri::command]
#[specta::specta]
async fn list_project_tags(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<Vec<String>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token).get_tags(&organization, &project).await
}

#[tauri::command]
#[specta::specta]
async fn result_screenshots(
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

/// The project's Area or Iteration paths for the create pickers.
#[tauri::command]
#[specta::specta]
async fn classification_paths(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    structure: String,
) -> Result<Vec<String>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .get_classification_paths(&organization, &project, &structure)
        .await
}

#[tauri::command]
#[specta::specta]
async fn list_plans_with_suites(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<Vec<ado_testplan::PlanWithSuites>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    let emitter = app.clone();
    ado::AdoClient::new(token)
        .list_plans_with_suites_cb(&organization, &project, move |done, total| {
            let _ = SuiteScanProgress { done, total }.emit(&emitter);
        })
        .await
}

/// Find-or-create the PBI's requirement suite and return it with its plan.
#[tauri::command]
#[specta::specta]
async fn ensure_pbi_suite(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    pbi_id: i32,
) -> Result<ado_testplan::EnsuredSuite, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    let client = ado::AdoClient::new(token);
    let (area, iteration) = client
        .get_work_item_paths(&organization, &project, pbi_id)
        .await?;
    let emitter = app.clone();
    client
        .ensure_requirement_suite_cb(&organization, &project, pbi_id, &area, &iteration, move |done, total| {
            let _ = SuiteScanProgress { done, total }.emit(&emitter);
        })
        .await
}

/// Read-only suite lookup for background prefetch: finds the PBI's
/// requirement suite if one exists anywhere, but NEVER creates a plan or
/// suite (creation stays on the Run Tests screen where the user asked).
#[tauri::command]
#[specta::specta]
async fn find_pbi_suite(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    pbi_id: i32,
) -> Result<Option<ado_testplan::EnsuredSuite>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    let client = ado::AdoClient::new(token);
    let (area, _iteration) = client
        .get_work_item_paths(&organization, &project, pbi_id)
        .await?;
    client
        .find_pbi_requirement_suite(&organization, &project, pbi_id, &area)
        .await
}

/// Execution report for one or more suites (a folder passes all its
/// descendants): gathers points + failure details (comments, linked bugs),
/// renders the failures-first HTML to a temp file and opens the browser.
/// GET-only against ADO; writes only the local temp file.
#[tauri::command]
#[specta::specta]
async fn view_execution_report(
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

/// Recent outcome history per test case for a plan (last 5, newest first).
#[tauri::command]
#[specta::specta]
async fn run_history(
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

#[tauri::command]
#[specta::specta]
async fn list_test_points(
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

/// Full manual-run lifecycle ported from v1 run_screen submission: create a
/// run seeded from the points, map each point to its auto-created result,
/// PATCH outcomes, complete the run. Returns the run's web URL.
#[tauri::command]
#[specta::specta]
async fn submit_test_run(
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
async fn get_result_detail(
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
async fn capture_screens() -> Result<Vec<capture::ScreenShot>, String> {
    tauri::async_runtime::spawn_blocking(capture::capture_all_monitors)
        .await
        .map_err(|e| e.to_string())?
}

/// File a Bug (or Issue on Basic-process projects) for a failed case:
/// Related links to the test case + PBI, screenshots attached. POST only.
#[tauri::command]
#[specta::specta]
async fn file_bug(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    title: String,
    repro_text: String,
    test_case_id: i32,
    pbi_id: i32,
    screenshots_b64: Vec<String>,
) -> Result<work_bug::FiledBug, String> {
    use base64::Engine;
    let token = get_fresh_token(&app).await.map_err(|e| e.to_string())?;
    let client = ado::AdoClient::new(token);
    let info = client
        .detect_bug_type(&organization, &project)
        .await
        .map_err(|e| e.to_string())?;
    let repro_html = format!("<div>{}</div>", repro_text.replace('\n', "<br>"));
    let fields = vec![
        ("System.Title".to_string(), title),
        (info.repro_field.clone(), repro_html),
    ];
    let (id, url) = client
        .create_work_item(&organization, &project, &info.wi_type, &fields, &[test_case_id, pbi_id])
        .await
        .map_err(|e| e.to_string())?;
    for (i, b64) in screenshots_b64.iter().enumerate() {
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) else {
            continue;
        };
        if let Ok(att_url) = client
            .upload_wi_attachment(&organization, &project, &format!("bug-{}-{}.png", id, i + 1), bytes)
            .await
        {
            let _ = client
                .add_wi_attachment_relation(&organization, &project, id, &att_url)
                .await;
        }
    }
    Ok(work_bug::FiledBug { id, url })
}

pub mod work_bug {
    #[derive(serde::Serialize, specta::Type)]
    pub struct FiledBug {
        pub id: i32,
        pub url: String,
    }
}

/// Non-blocking update check; Some(version) when a newer build is published.
#[tauri::command]
#[specta::specta]
async fn check_update(app: tauri::AppHandle) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<updater::UpdateState>();
        updater::check(&state)
    })
    .await
    .ok()
    .flatten()
}

/// Download the pending update and restart into it.
#[tauri::command]
#[specta::specta]
async fn apply_update(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<updater::UpdateState>();
        updater::download_and_apply(&state)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
#[specta::specta]
async fn fetch_board(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    team: Option<String>,
) -> Result<work_board::BoardData, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .fetch_board(&organization, &project, team.as_deref())
        .await
}

#[tauri::command]
#[specta::specta]
async fn list_teams(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<Vec<work_board::TeamRef>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token).list_teams(&organization, &project).await
}

#[tauri::command]
#[specta::specta]
async fn list_team_members(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<Vec<work_board::Member>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .list_team_members(&organization, &project)
        .await
}

#[tauri::command]
#[specta::specta]
async fn work_item_detail(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    id: i32,
) -> Result<work_board::WorkItemDetail, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .get_work_item_detail(&organization, &project, id)
        .await
}

/// PATCH a work item's fields (create-or-replace 'add' ops, only the
/// changed refs). ADO 4xx (invalid transition / required field) surfaces
/// verbatim for the drawer to show.
#[tauri::command]
#[specta::specta]
async fn update_work_item(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    id: i32,
    patches: Vec<work_board::FieldPatch>,
) -> Result<(), ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    let fields: Vec<(String, String)> = patches
        .into_iter()
        .map(|p| (p.reference_name, p.value))
        .collect();
    ado::AdoClient::new(token)
        .update_work_item_fields(&organization, &project, id, &fields)
        .await
}

#[tauri::command]
#[specta::specta]
async fn activity_values(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    wi_type: String,
) -> Result<Vec<String>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .get_field_allowed_values(
            &organization,
            &project,
            &wi_type,
            "Microsoft.VSTS.Common.Activity",
        )
        .await
}

#[tauri::command]
#[specta::specta]
async fn work_item_comments(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    id: i32,
) -> Result<Vec<work_board::WorkComment>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .get_work_item_comments(&organization, &project, id)
        .await
}

#[tauri::command]
#[specta::specta]
async fn add_comment(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    id: i32,
    text: String,
) -> Result<(), ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .add_work_item_comment(&organization, &project, id, &text)
        .await
}

/// Best-effort avatar fetch (None -> initials disc in the UI).
#[tauri::command]
#[specta::specta]
async fn avatar_b64(app: tauri::AppHandle, url: String) -> Option<String> {
    let token = get_fresh_token(&app).await.ok()?;
    ado::AdoClient::new(token).get_avatar_b64(&url).await
}

/// Quick create a Task/Bug from the board, optionally assigned to me.
#[tauri::command]
#[specta::specta]
async fn quick_create_item(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    wi_type: String,
    title: String,
    assign_to_me: bool,
) -> Result<i32, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    let mut fields = vec![("System.Title".to_string(), title)];
    if assign_to_me {
        let account = {
            let state = app.state::<Mutex<auth::AuthState>>();
            let s = state.lock().unwrap();
            s.tokens.as_ref().and_then(|t| t.account.clone())
        };
        if let Some(upn) = account {
            fields.push(("System.AssignedTo".to_string(), upn));
        }
    }
    let (id, _url) = ado::AdoClient::new(token)
        .create_work_item(&organization, &project, &wi_type, &fields, &[])
        .await?;
    Ok(id)
}

/// Move a board item into a column: resolves the target state exactly like
/// Start streaming AudioSpectrum events from system-audio loopback (the
/// flask equalizer rings). No-op if already running; failures are silent by
/// design - the feature is decorative.
#[tauri::command]
#[specta::specta]
fn audio_capture_start(app: tauri::AppHandle) -> Result<(), String> {
    audio::start(app)
}

/// Stop the AudioSpectrum stream (last UI subscriber unmounted).
#[tauri::command]
#[specta::specta]
fn audio_capture_stop() {
    audio::stop();
}

/// v1 (_state_for_column) and PATCHes System.State. Returns the state set.
#[tauri::command]
#[specta::specta]
async fn move_board_item(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    item_id: i32,
    work_item_type: String,
    column: String,
) -> Result<String, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    let client = ado::AdoClient::new(token);
    let states = client
        .get_work_item_states(&organization, &project, &work_item_type)
        .await?;
    let mut by_type = std::collections::HashMap::new();
    by_type.insert(work_item_type.clone(), states);
    let target = work_board::state_for_column(&work_item_type, &column, &by_type).ok_or(
        ado::AdoError::Http {
            status: 0,
            body: format!("no state maps to column '{column}' for {work_item_type}"),
        },
    )?;
    // Verified move: ADO rules (e.g. required dates) can reject or rewrite
    // the transition even on a 2xx, so trust only the persisted state.
    let actual = client
        .set_work_item_state(&organization, &project, item_id, &target)
        .await?;
    if actual != target {
        return Err(ado::AdoError::Http {
            status: 409,
            body: format!(
                "Azure DevOps kept #{item_id} in '{actual}' — moving to '{target}' is blocked by \
                 work item rules (for example required dates). Open the item, fill the required \
                 fields, then try again."
            ),
        });
    }
    Ok(target)
}

pub fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .events(collect_events![SubmitProgress, SuiteScanProgress, audio::AudioSpectrum])
        .commands(collect_commands![
        ping,
        auth_status,
        sign_in,
        list_projects,
        list_orgs,
        search_pbis,
        pbi_test_cases,
        parse_import_file,
        export_queue,
        write_template,
        submit_queue,
        ensure_pbi_suite,
        list_test_points,
        submit_test_run,
        fetch_board,
        move_board_item,
        audio_capture_start,
        audio_capture_stop,
        check_update,
        apply_update,
        list_test_case_fields,
        pbi_test_cases_full,
        update_test_case,
        export_queue_json,
        list_plans_with_suites,
        get_result_detail,
        capture_screens,
        file_bug,
        list_teams,
        list_team_members,
        work_item_detail,
        update_work_item,
        activity_values,
        work_item_comments,
        add_comment,
        avatar_b64,
        quick_create_item,
        classification_paths,
        cancel_submit,
        export_queue_html,
        list_project_tags,
        result_screenshots,
        view_queue_html,
        test_cases_by_ids,
        test_case_field_values,
        find_pbi_suite,
        run_history,
        view_execution_report,
        read_file_b64,
        open_snip
    ])
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = specta_builder();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(Mutex::new(auth::AuthState::default()))
        .manage(updater::UpdateState::default())
        .manage(SubmitCancel::default())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            // Registers the typed-event registry in Tauri state; without
            // this every specta Event::emit panics with "EventRegistry not
            // found in Tauri state".
            builder.mount_events(app);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
