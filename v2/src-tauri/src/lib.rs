pub mod ado;
pub mod auth;

use std::sync::Mutex;
use tauri::Manager;
use tauri_specta::{collect_commands, Builder};

#[derive(serde::Serialize, specta::Type)]
pub struct AuthStatus {
    pub signed_in: bool,
    pub account: Option<String>,
}

#[tauri::command]
#[specta::specta]
fn ping(msg: String) -> String {
    format!("pong: {msg}")
}

#[tauri::command]
#[specta::specta]
fn auth_status(state: tauri::State<'_, Mutex<auth::AuthState>>) -> AuthStatus {
    let s = state.lock().unwrap();
    AuthStatus {
        signed_in: s.access_token.is_some(),
        account: s.account.clone(),
    }
}

#[tauri::command]
#[specta::specta]
async fn sign_in(app: tauri::AppHandle) -> Result<AuthStatus, String> {
    let (token, upn) = auth::sign_in_interactive(|url| {
        let _ = tauri_plugin_opener::open_url(url, None::<&str>);
    })
    .await?;
    let state = app.state::<Mutex<auth::AuthState>>();
    let mut s = state.lock().unwrap();
    s.access_token = Some(token);
    s.account = upn.clone();
    Ok(AuthStatus {
        signed_in: true,
        account: upn,
    })
}

#[tauri::command]
#[specta::specta]
async fn list_projects(
    app: tauri::AppHandle,
    organization: String,
) -> Result<Vec<ado::Project>, ado::AdoError> {
    let token = {
        let state = app.state::<Mutex<auth::AuthState>>();
        let s = state.lock().unwrap();
        s.access_token.clone().ok_or(ado::AdoError::Unauthorized)?
    };
    ado::AdoClient::new(token).get_projects(&organization).await
}

pub fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(collect_commands![
        ping,
        auth_status,
        sign_in,
        list_projects
    ])
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = specta_builder();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Mutex::new(auth::AuthState::default()))
        .invoke_handler(builder.invoke_handler())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
