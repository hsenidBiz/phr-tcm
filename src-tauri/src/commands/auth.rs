//! Sign-in and session status.

use std::sync::atomic::Ordering;
use std::sync::Mutex;
use std::time::Duration;
use tauri::Manager;

use crate::auth;
use crate::saved_session::{self, Resume, SessionVault};

/// How long a launch waits on the kept sign-in before showing the sign-in
/// screen. The loading screen stays up meanwhile, so this caps how long a
/// launch with no network can sit on it.
const RESUME_WINDOW: Duration = Duration::from_secs(10);

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

/// `auth_status`, after first trying the sign-in kept by Stay signed in
/// when nobody is signed in yet. What the app asks on launch: a session
/// Microsoft still honours goes straight in, anything else answers
/// "signed out" and the sign-in screen's button opens the browser as it
/// always did. Cheap once signed in - it answers from memory.
#[tauri::command]
#[specta::specta]
pub async fn resume_session(app: tauri::AppHandle) -> AuthStatus {
    let state = app.state::<Mutex<auth::AuthState>>();
    let vault = app.state::<SessionVault>();
    let signed_in = state.lock().unwrap().tokens.is_some();
    // After Sign out in this run, never back in without the browser - even
    // if removing the kept copy failed and it is still there.
    let signed_out = vault.choose_account.load(Ordering::SeqCst);
    if signed_in || signed_out || !crate::app_settings::current().stay_signed_in {
        return status_from(&state.lock().unwrap());
    }
    let store = vault.store.clone();
    let attempt = saved_session::resume_with(store.as_ref(), |rt, account| async move {
        auth::refresh(&rt, account).await
    });
    let outcome = tokio::time::timeout(RESUME_WINDOW, attempt).await.unwrap_or_else(|_| {
        crate::applog::warn("The kept sign-in took too long to check; showing the sign-in screen");
        Resume::Unreachable
    });
    if let Resume::Resumed(tokens) = outcome {
        // A browser sign-in that finished first wins; this one is dropped.
        if state.lock().unwrap().tokens.is_none() {
            // As in `sign_in`: before the tokens are visible to anything
            // that reads the cache.
            crate::cache::claim_for(tokens.account.as_deref());
            state.lock().unwrap().tokens = Some(tokens);
            crate::applog::info("Signed in to Azure DevOps with the kept sign-in");
            // Keeps the rotated refresh token Microsoft just handed back.
            saved_session::keep_for_next_launch(&app);
        }
    }
    let status = status_from(&state.lock().unwrap());
    status
}

#[tauri::command]
#[specta::specta]
pub async fn sign_in(app: tauri::AppHandle) -> Result<AuthStatus, String> {
    let vault = app.state::<SessionVault>();
    // After Sign out, ask which account - cleared only once a sign-in
    // succeeds, so one abandoned in the browser still asks next time.
    let choose_account = vault.choose_account.load(Ordering::SeqCst);
    let tokens = auth::sign_in_interactive(
        |url| {
            let _ = tauri_plugin_opener::open_url(url, None::<&str>);
        },
        choose_account,
    )
    .await
    .inspect_err(|e| crate::applog::error(format!("Sign-in failed: {e}")))?;
    crate::applog::info("Signed in to Azure DevOps");
    vault.choose_account.store(false, Ordering::SeqCst);
    // Before the tokens are visible to anything that reads the cache: a
    // different account than last time must not be served the previous
    // one's tags or suite ids (the webview's cache does the same).
    crate::cache::claim_for(tokens.account.as_deref());
    let state = app.state::<Mutex<auth::AuthState>>();
    state.lock().unwrap().tokens = Some(tokens);
    saved_session::keep_for_next_launch(&app);
    let status = status_from(&state.lock().unwrap());
    Ok(status)
}

/// Sign out: the session goes from memory AND from Credential Manager, and
/// the next browser sign-in asks which account to use. An error means the
/// session is gone from this run but the kept copy could not be removed,
/// so the next launch would sign back in - the person needs to know that.
#[tauri::command]
#[specta::specta]
pub fn sign_out(app: tauri::AppHandle) -> Result<AuthStatus, String> {
    let state = app.state::<Mutex<auth::AuthState>>();
    state.lock().unwrap().tokens = None;
    let vault = app.state::<SessionVault>();
    vault.choose_account.store(true, Ordering::SeqCst);
    // Browsers API template runs kept signed in go with the person's
    // session, closed off this thread.
    tauri::async_runtime::spawn_blocking(crate::api_templates::held::close_all);
    crate::applog::info("Signed out");
    saved_session::forget(vault.store.as_ref()).map_err(|e| {
        crate::applog::warn(format!("Removing the kept sign-in failed: {e}"));
        "Signed out, but the kept sign-in could not be removed from Windows Credential Manager, so the next launch may sign you back in. Settings → Logs has the details.".to_string()
    })?;
    let status = status_from(&state.lock().unwrap());
    Ok(status)
}
