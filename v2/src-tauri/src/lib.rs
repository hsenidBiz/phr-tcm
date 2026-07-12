pub mod ado;
pub mod ado_testplan;
pub mod auth;
pub mod import_parser;
pub mod model;
pub mod steps_xml;
pub mod updater;
pub mod work_board;

use std::sync::Mutex;
use std::time::Instant;
use tauri::Manager;
use tauri_specta::{collect_commands, collect_events, Builder, Event};

/// Emitted once per queue item while submit_queue runs.
#[derive(Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct SubmitProgress {
    pub index: u32,
    pub total: u32,
    pub title: String,
    /// "created" | "updated" | "failed"
    pub action: String,
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
) -> Result<Vec<SubmitItemResult>, String> {
    // Best-effort board visibility (ported from v1 CreationWorker._ensure_suite):
    // make sure the PBI's requirement-based suite exists before creating, so
    // linked cases surface on the board's test count. Failures never block
    // creation.
    if let Ok(token) = get_fresh_token(&app).await {
        let client = ado::AdoClient::new(token);
        if let Ok((area, iteration)) = client
            .get_work_item_paths(&organization, &project, pbi_id)
            .await
        {
            let _ = client
                .ensure_requirement_suite(&organization, &project, pbi_id, &area, &iteration)
                .await;
        }
    }

    let total = queue.len() as u32;
    let mut results: Vec<SubmitItemResult> = vec![];
    for (i, tc) in queue.iter().enumerate() {
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
            .create_test_case(organization, project, tc, m_ref, "", "", p_ref)
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

#[tauri::command]
#[specta::specta]
async fn list_plans_with_suites(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<Vec<ado_testplan::PlanWithSuites>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .list_plans_with_suites(&organization, &project)
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
    client
        .ensure_requirement_suite(&organization, &project, pbi_id, &area, &iteration)
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

#[derive(serde::Deserialize, specta::Type)]
pub struct PointOutcome {
    pub point_id: i32,
    /// Passed / Failed / Blocked / NotApplicable.
    pub outcome: String,
    pub comment: Option<String>,
    pub duration_ms: Option<i32>,
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
            })
        })
        .collect();
    client
        .update_run_results(&organization, &project, run.run_id, &updates)
        .await?;
    client
        .complete_test_run(&organization, &project, run.run_id)
        .await?;
    Ok(run)
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
) -> Result<work_board::BoardData, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token).fetch_board(&organization, &project).await
}

/// Move a board item into a column: resolves the target state exactly like
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
    client
        .update_work_item_fields(
            &organization,
            &project,
            item_id,
            &[("System.State".to_string(), target.clone())],
        )
        .await?;
    Ok(target)
}

pub fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .events(collect_events![SubmitProgress])
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
        check_update,
        apply_update,
        list_test_case_fields,
        pbi_test_cases_full,
        update_test_case,
        export_queue_json,
        list_plans_with_suites
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
        .invoke_handler(builder.invoke_handler())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
