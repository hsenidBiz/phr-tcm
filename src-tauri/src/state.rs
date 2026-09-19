//! Managed Tauri state and the token-access helper every command shares.
//! Commands depend on this module for credentials; the raw token itself
//! never crosses the IPC boundary (see tests/bindings.rs).

use std::sync::Mutex;
use std::time::Instant;
use tauri::Manager;

use crate::{ado, auth};

/// Single-flight guard for the submit loop. A second call while one is
/// running would put a second loop over the same queue, and every case
/// that pair creates twice is a duplicate that only someone with delete
/// permission can remove, if anyone can. Not making them is still far
/// better than tidying them up. (This once also carried a cooperative
/// cancel flag; an upload now runs to the end, so only the guard remains.)
#[derive(Default)]
pub struct SubmitCancel(pub(crate) std::sync::atomic::AtomicBool);

impl SubmitCancel {
    /// Claim the loop, or None if one is already running. `compare_exchange`
    /// rather than load-then-store: two clicks land on the same millisecond.
    pub(crate) fn claim(&self) -> Option<SubmitGuard<'_>> {
        use std::sync::atomic::Ordering::SeqCst;
        self.0
            .compare_exchange(false, true, SeqCst, SeqCst)
            .ok()
            .map(|_| SubmitGuard(self))
    }
}

/// Releases the claim however the loop ends - returned, errored, or
/// unwound. A submit that could never be started again would be worse than
/// the duplicates this guards against.
pub(crate) struct SubmitGuard<'a>(&'a SubmitCancel);

impl Drop for SubmitGuard<'_> {
    fn drop(&mut self) {
        self.0 .0.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

/// One refresh at a time, process-wide. Near expiry every concurrent
/// command used to send its own refresh; now the first refreshes and the
/// rest, re-reading after the gate, find a token that no longer needs it.
fn refresh_gate() -> &'static tokio::sync::Mutex<()> {
    static GATE: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    GATE.get_or_init(|| tokio::sync::Mutex::new(()))
}

type Snapshot = (String, bool, Option<String>, Option<String>);

/// (access token, needs refresh, refresh token, account). Sync, so no lock
/// guard lives across an await.
fn snapshot(state: &Mutex<auth::AuthState>) -> Result<Snapshot, ado::AdoError> {
    let s = state.lock().unwrap();
    match &s.tokens {
        None => Err(ado::AdoError::Unauthorized),
        Some(t) => Ok((
            t.access_token.clone(),
            auth::needs_refresh(t.expires_at, Instant::now()),
            t.refresh_token.clone(),
            t.account.clone(),
        )),
    }
}

/// Store a refresh result if its session is still current, and return the
/// token the caller should use either way: the stored one.
fn keep(state: &Mutex<auth::AuthState>, sent: &str, fresh: auth::TokenSet) -> Result<String, ado::AdoError> {
    let mut s = state.lock().unwrap();
    auth::store_refreshed(&mut s, sent, fresh);
    s.tokens
        .as_ref()
        .map(|t| t.access_token.clone())
        .ok_or(ado::AdoError::Unauthorized)
}

/// `get_fresh_token` with the state and the refresh call passed in - the
/// seam tests/auth.rs drives without a Tauri app.
pub async fn fresh_token_with<F, Fut>(
    state: &Mutex<auth::AuthState>,
    refresh: F,
) -> Result<String, ado::AdoError>
where
    F: Fn(String, Option<String>) -> Fut,
    Fut: std::future::Future<Output = Result<auth::TokenSet, String>>,
{
    let (token, needed, _, _) = snapshot(state)?;
    if !needed {
        return Ok(token);
    }
    let _gate = refresh_gate().lock().await;
    // Re-read: whoever held the gate before us may have refreshed already.
    let (token, needed, refresh_token, account) = snapshot(state)?;
    if !needed {
        return Ok(token);
    }
    let Some(rt) = refresh_token else {
        // No refresh token: keep using the current one until it hard-fails.
        return Ok(token);
    };
    match refresh(rt.clone(), account).await {
        Ok(new_tokens) => keep(state, &rt, new_tokens),
        // Refresh failed (revoked, offline, CAE): fall back to the token
        // stored NOW, not the one read before the refresh - another account
        // may have signed in (or everyone out) while it was out. A hard 401
        // from the API will surface as Unauthorized.
        Err(_) => snapshot(state).map(|(current, _, _, _)| current),
    }
}

/// Returns a valid access token, silently refreshing when it is within
/// 5 minutes of expiry. The token itself never leaves the Rust side.
pub(crate) async fn get_fresh_token(app: &tauri::AppHandle) -> Result<String, ado::AdoError> {
    let state = app.state::<Mutex<auth::AuthState>>();
    fresh_token_with(&state, |rt, account| async move { auth::refresh(&rt, account).await }).await
}
