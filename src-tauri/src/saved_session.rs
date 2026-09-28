//! Stay signed in: the refresh token kept between launches, so a launch
//! whose session Microsoft still honours goes straight into the app, and
//! only one it no longer honours (expired, revoked, a password change, a
//! policy that wants the person at the keyboard) opens the browser.
//!
//! Where it lives: Windows Credential Manager, the same per-user store the
//! database logins use (`db::credentials`), encrypted by Windows to the
//! signed-in account. Never a file of the app's own, never the log, never
//! the webview - the webview only learns whether someone is signed in. The
//! access token is never kept: it lasts about an hour, and the refresh
//! token gets a new one.
//!
//! One Credential Manager entry holds at most 2,560 bytes and an Entra
//! refresh token is not far short of that, so the token is split into
//! numbered parts under a head entry that says how many there are.

use std::future::Future;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use tauri::Manager;

use crate::auth::{self, AuthState, TokenSet};
use crate::db::SecretStore;

const HEAD: &str = "tcm-v2/auth/session";
/// Characters per part - well inside the entry limit.
pub const PART: usize = 1200;
/// A token needing more parts than this is not a refresh token.
pub const MAX_PARTS: usize = 16;

fn part(i: usize) -> String {
    format!("{HEAD}/{i}")
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Head {
    parts: usize,
    account: Option<String>,
}

/// A kept session. Deliberately no Debug: nothing may print the token.
pub struct Saved {
    pub refresh_token: String,
    pub account: Option<String>,
}

/// The managed store, plus whether the next browser sign-in should ask
/// which account to use - set by Sign out, cleared by the next sign-in.
pub struct SessionVault {
    pub store: Arc<dyn SecretStore>,
    pub choose_account: AtomicBool,
}

impl SessionVault {
    pub fn new(store: Arc<dyn SecretStore>) -> Self {
        Self { store, choose_account: AtomicBool::new(false) }
    }
}

fn read_head(store: &dyn SecretStore) -> Result<Option<Head>, String> {
    // A head that no longer parses is nothing saved, not an error.
    Ok(store.get(HEAD)?.and_then(|s| serde_json::from_str(&s).ok()))
}

/// Keep `refresh_token` for the next launch, replacing whatever was kept.
/// The parts go first and the head last, so an interrupted save leaves a
/// token Microsoft refuses - and a refused token is forgotten - never one
/// stitched from two sessions that it accepts for the wrong person.
pub fn save(store: &dyn SecretStore, refresh_token: &str, account: Option<&str>) -> Result<(), String> {
    let chars: Vec<char> = refresh_token.chars().collect();
    let parts: Vec<String> = chars.chunks(PART).map(|c| c.iter().collect()).collect();
    if parts.is_empty() {
        return Err("There is no sign-in to keep.".into());
    }
    if parts.len() > MAX_PARTS {
        return Err("The sign-in is too large to keep.".into());
    }
    let before = read_head(store).ok().flatten().map_or(0, |h| h.parts.min(MAX_PARTS));
    for (i, p) in parts.iter().enumerate() {
        store.put(&part(i), p)?;
    }
    let head = Head { parts: parts.len(), account: account.map(str::to_string) };
    store.put(HEAD, &serde_json::to_string(&head).map_err(|e| e.to_string())?)?;
    // A shorter token than last time: the tail of the old one goes.
    for i in parts.len()..before {
        store.remove(&part(i))?;
    }
    Ok(())
}

/// The kept session, or None when there is none - or only part of one,
/// which is no use to anybody.
pub fn load(store: &dyn SecretStore) -> Result<Option<Saved>, String> {
    let Some(head) = read_head(store)? else { return Ok(None) };
    if head.parts == 0 || head.parts > MAX_PARTS {
        return Ok(None);
    }
    let mut refresh_token = String::new();
    for i in 0..head.parts {
        match store.get(&part(i))? {
            Some(p) => refresh_token.push_str(&p),
            None => return Ok(None),
        }
    }
    Ok(Some(Saved { refresh_token, account: head.account }))
}

/// Remove the kept session. The head goes first, so a forget that stops
/// partway never leaves something `load` would accept; then every part
/// there could be, whatever the head said.
pub fn forget(store: &dyn SecretStore) -> Result<(), String> {
    store.remove(HEAD)?;
    for i in 0..MAX_PARTS {
        store.remove(&part(i))?;
    }
    Ok(())
}

/// Keep the session now in `state` for the next launch - or, with Stay
/// signed in off or nobody signed in, make sure none is kept. Never fails
/// the caller: a session that could not be kept means the browser next
/// launch, which is how every launch worked before this existed.
pub fn keep_current(store: &dyn SecretStore, state: &Mutex<AuthState>, enabled: bool) {
    let current = {
        let s = state.lock().unwrap();
        s.tokens
            .as_ref()
            .and_then(|t| t.refresh_token.clone().map(|rt| (rt, t.account.clone())))
    };
    let result = match (enabled, current) {
        (true, Some((rt, account))) => save(store, &rt, account.as_deref()),
        _ => forget(store),
    };
    if let Err(e) = result {
        crate::applog::warn(format!("Keeping the sign-in for the next launch failed: {e}"));
    }
}

/// `keep_current` with the app's own store, state and setting. A process
/// without them (the `--mcp` proxy never runs setup) keeps nothing.
pub fn keep_for_next_launch(app: &tauri::AppHandle) {
    let (Some(vault), Some(state)) = (app.try_state::<SessionVault>(), app.try_state::<Mutex<AuthState>>())
    else {
        return;
    };
    keep_current(vault.store.as_ref(), &state, crate::app_settings::current().stay_signed_in);
}

/// What trying the kept session came to.
pub enum Resume {
    /// Nothing kept (or nothing readable).
    Nothing,
    /// Microsoft took it: the new tokens, to be stored by the caller.
    Resumed(TokenSet),
    /// Microsoft refused it, so it has been forgotten - the browser it is.
    Refused,
    /// Microsoft could not be reached, or had trouble. Kept, for next time.
    Unreachable,
}

/// Try the kept session: exchange its refresh token for a fresh set. The
/// caller stores the result - and keeps its rotated refresh token - only if
/// nobody signed in while this was out. `refresh` is the seam the tests
/// drive; the app passes `auth::refresh`.
pub async fn resume_with<F, Fut>(store: &dyn SecretStore, refresh: F) -> Resume
where
    F: FnOnce(String, Option<String>) -> Fut,
    Fut: Future<Output = Result<TokenSet, String>>,
{
    let saved = match load(store) {
        Ok(Some(saved)) => saved,
        Ok(None) => return Resume::Nothing,
        Err(e) => {
            crate::applog::warn(format!("Reading the kept sign-in failed: {e}"));
            return Resume::Nothing;
        }
    };
    match refresh(saved.refresh_token, saved.account).await {
        Ok(tokens) => Resume::Resumed(tokens),
        Err(e) if auth::is_refusal(&e) => {
            crate::applog::info("The kept sign-in is no longer accepted; it has been removed");
            if let Err(e) = forget(store) {
                crate::applog::warn(format!("Removing the kept sign-in failed: {e}"));
            }
            Resume::Refused
        }
        // The token endpoint has already logged what went wrong.
        Err(_) => Resume::Unreachable,
    }
}

