//! Managed Tauri state and the token-access helper every command shares.
//! Commands depend on this module for credentials; the raw token itself
//! never crosses the IPC boundary (see tests/bindings.rs).

use std::sync::Mutex;
use std::time::Instant;
use tauri::Manager;

use crate::{ado, auth};

/// Cooperative cancel for the submit loop: checked between items, so the
/// in-flight item always completes (never a half-created case).
#[derive(Default)]
pub struct SubmitCancel(pub(crate) std::sync::atomic::AtomicBool);

/// Returns a valid access token, silently refreshing when it is within
/// 5 minutes of expiry. The token itself never leaves the Rust side.
pub(crate) async fn get_fresh_token(app: &tauri::AppHandle) -> Result<String, ado::AdoError> {
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
