//! Sign-in and session status.

use std::sync::Mutex;
use tauri::Manager;

use crate::auth;

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

#[tauri::command]
#[specta::specta]
pub fn auth_status(state: tauri::State<'_, Mutex<auth::AuthState>>) -> AuthStatus {
    status_from(&state.lock().unwrap())
}

#[tauri::command]
#[specta::specta]
pub async fn sign_in(app: tauri::AppHandle) -> Result<AuthStatus, String> {
    let tokens = auth::sign_in_interactive(|url| {
        let _ = tauri_plugin_opener::open_url(url, None::<&str>);
    })
    .await?;
    let state = app.state::<Mutex<auth::AuthState>>();
    let mut s = state.lock().unwrap();
    s.tokens = Some(tokens);
    Ok(status_from(&s))
}
