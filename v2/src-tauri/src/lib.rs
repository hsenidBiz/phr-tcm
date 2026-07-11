pub mod ado;
pub mod auth;
pub mod import_parser;
pub mod model;
pub mod steps_xml;

use std::sync::Mutex;
use std::time::Instant;
use tauri::Manager;
use tauri_specta::{collect_commands, Builder};

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
) -> Result<Vec<SubmitItemResult>, String> {
    let mut results = vec![];
    for (i, tc) in queue.iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        if let Err(msg) = tc.is_valid() {
            results.push(SubmitItemResult {
                index: i as u32,
                title: tc.title.clone(),
                action: "failed".into(),
                id: None,
                error: Some(msg),
            });
            continue;
        }
        let token = match get_fresh_token(&app).await {
            Ok(t) => t,
            Err(e) => {
                results.push(SubmitItemResult {
                    index: i as u32,
                    title: tc.title.clone(),
                    action: "failed".into(),
                    id: None,
                    error: Some(e.to_string()),
                });
                continue;
            }
        };
        let client = ado::AdoClient::new(token);
        let outcome = match tc.update_id {
            Some(existing_id) => client
                .update_test_case_from_model(&organization, &project, existing_id, tc, None, None)
                .await
                .map(|_| (existing_id, "updated")),
            None => match client
                .create_test_case(&organization, &project, tc, None, "", "", None)
                .await
            {
                Ok(new_id) => client
                    .link_to_pbi(&organization, &project, new_id, pbi_id)
                    .await
                    .map(|_| (new_id, "created")),
                Err(e) => Err(e),
            },
        };
        match outcome {
            Ok((id, action)) => results.push(SubmitItemResult {
                index: i as u32,
                title: tc.title.clone(),
                action: action.into(),
                id: Some(id),
                error: None,
            }),
            Err(e) => results.push(SubmitItemResult {
                index: i as u32,
                title: tc.title.clone(),
                action: "failed".into(),
                id: None,
                error: Some(e.to_string()),
            }),
        }
    }
    Ok(results)
}

pub fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(collect_commands![
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
        submit_queue
    ])
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = specta_builder();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(Mutex::new(auth::AuthState::default()))
        .invoke_handler(builder.invoke_handler())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
