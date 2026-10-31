//! Check sign-in, for the Accounts dialog (`auto_run_check_sign_in`) and the
//! assistant's `check_autorun_sign_in` (`ai_bridge`). The checking itself is
//! `autorun::sign_in_check`; this is who may ask, and when.
//!
//! A check opens a browser of its own for each account (never the person's
//! Auto Run browser), so it holds the same claim an unattended run does
//! while it goes: a run, a second check, a discovery or a replay to a step
//! that starts is refused, and so is `auto_run_open_browser`, each with the
//! sentence "a sign-in check is running (<account>) - wait for it or cancel
//! it". It is refused itself while an unattended run, a replay or a
//! discovery is using the Auto Run browser. Cancel ends it between
//! accounts and closes the browser of the one it is on.

use crate::autorun::active_login::session_is_ours;
use crate::autorun::replay::Browsers;
use crate::autorun::setup_find::RowSource;
use crate::autorun::sign_in_check::{check_accounts, precheck, summary, Mark, Precheck, SignInCheck, BROWSER_IN_USE};
use crate::autorun::slots::SlotNo;
use std::collections::BTreeMap;
use std::path::Path;

/// Why a check cannot begin now, or `None`: another check, an unattended
/// run, a replay to a step or a discovery has the Auto Run browser.
pub fn refusal() -> Option<String> {
    if let Some(going) = crate::autorun::sign_in_check::running_sentence() {
        return Some(going);
    }
    let busy = crate::commands::autorun_replay::replay_is_running() || others_busy();
    busy.then(|| BROWSER_IN_USE.to_string())
}

/// A replay to a step or a discovery has a browser.
fn others_busy() -> bool {
    SlotNo::all().into_iter().any(|n| {
        crate::autorun::replay_to::state(n).is_running() || crate::commands::autorun::discovery_active_in(n)
    })
}

/// Checks `keys` one at a time (`sign_in_check::check_accounts`), the
/// environment's two queries (`queries`: password expiry, active session)
/// asked first for each through `rows` (`None`: reading the database is off
/// or it is not set up - an account the active session query cannot be
/// asked about is then not signed in). `Err` is the sentence for a check
/// that could not begin at all.
///
/// The claim comes first and the other users of the browser are looked at
/// under it, so nothing starts between the look and the claim. Cancelled by
/// `sign_in_check::cancel`.
#[allow(clippy::too_many_arguments)]
pub async fn check_with<B: Browsers, S: RowSource>(
    browsers: &mut B,
    rows: Option<&S>,
    queries: (&str, &str),
    root: &Path,
    org: &str,
    project: &str,
    keys: &[String],
    area: Option<&str>,
) -> Result<Vec<SignInCheck>, String> {
    // Held until the last browser is closed.
    let Some(_claim) = crate::commands::autorun_replay::OneAtATime::claim() else {
        return Err(refusal().unwrap_or_else(|| BROWSER_IN_USE.to_string()));
    };
    if others_busy() {
        return Err(BROWSER_IN_USE.to_string());
    }
    let Some(first) = keys.first() else { return Ok(vec![]) };
    let _going = crate::autorun::sign_in_check::begin(first);
    let env_id = crate::environments::active_id(root)?;
    let minutes = crate::autorun::recipe::load_effective_recipe_if_any(root, org, project)
        .ok()
        .flatten()
        .map_or(480, |r| r.session_minutes);
    let accounts = crate::autorun::accounts::load_accounts(root)?;
    let today = crate::autorun::password_expiry::today();
    let mut pre: BTreeMap<String, Precheck> = BTreeMap::new();
    for key in keys {
        let Some(account) = accounts.iter().find(|a| &a.key == key) else { continue };
        let ours = session_is_ours(root, &env_id, key, minutes, crate::autorun::sessions::now_ms());
        pre.insert(key.clone(), precheck(rows, queries.0, queries.1, key, &account.username, ours, &today).await);
    }
    Ok(check_accounts(browsers, root, org, project, keys, area, &pre, &crate::browser::timing::Timing::supervised()).await)
}

/// One account's check and its words, as the dialog shows them.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct CheckedAccount {
    pub check: SignInCheck,
    /// `sign_in_check::summary`.
    pub summary: String,
}

impl CheckedAccount {
    pub fn of(check: SignInCheck) -> Self {
        CheckedAccount { summary: summary(&check), check }
    }
}

/// Check the sign-in of one saved account, and walk it to `area` (an area
/// name, optional). The Accounts dialog calls this for each account it ticks,
/// one after another. The account's password is typed once, in a browser of
/// its own that is closed after, and is never in the answer.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_check_sign_in(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    account_key: String,
    area: Option<String>,
    db_read_access: bool,
) -> Result<CheckedAccount, String> {
    use tauri::Manager;
    let root = super::autorun::root(&app)?;
    let env = crate::environments::active(&root)?;
    // The environment's database, when reading it is on and it is set up: a
    // check is never held back by the database being away.
    let db = crate::autorun::preconditions::read_access(db_read_access)
        .then(|| {
            let secrets = std::sync::Arc::clone(&app.state::<crate::db::DbSecrets>().0);
            crate::autorun::preconditions::environment_db(&root, Some(secrets.as_ref())).ok()
        })
        .flatten();
    let rows = db.as_ref().map(crate::autorun::active_login::StageRows);
    let mut browsers = super::autorun_replay::RealBrowsers::new(
        crate::browser::launch::Browser::from_name(&crate::autorun::store::last_browser(&root)),
        false,
    );
    let queries = (env.password_expiry_query.as_str(), env.active_session_query.as_str());
    let mut done = check_with(
        &mut browsers,
        rows.as_ref(),
        queries,
        &root,
        &organization,
        &project,
        std::slice::from_ref(&account_key),
        area.as_deref(),
    )
    .await?;
    done.pop().map(CheckedAccount::of).ok_or_else(|| "the check gave no answer".to_string())
}

/// The last check of each account of the active environment, by key: when,
/// whether it reached its area, and the words.
#[tauri::command]
#[specta::specta]
pub fn auto_run_sign_in_checks(app: tauri::AppHandle) -> Result<BTreeMap<String, Mark>, String> {
    let root = super::autorun::root(&app)?;
    let env_id = crate::environments::active_id(&root)?;
    Ok(crate::autorun::sign_in_check::load_marks(&root, &env_id))
}

/// End the sign-in check going, if any (the dialog's Cancel).
#[tauri::command]
#[specta::specta]
pub fn auto_run_cancel_check_sign_in() {
    crate::autorun::sign_in_check::cancel();
}

/// The saved accounts (`account_key` names one; none means all) whose
/// password is expired, forced to change or due within a week, one line
/// each, for the run window's warning. Empty when the environment has no
/// expiry query, reading the database is off or it does not answer.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_expiring_accounts(
    app: tauri::AppHandle,
    account_key: Option<String>,
    db_read_access: bool,
) -> Result<Vec<String>, String> {
    use tauri::Manager;
    let root = super::autorun::root(&app)?;
    let env = crate::environments::active(&root)?;
    if env.password_expiry_query.trim().is_empty() || !crate::autorun::preconditions::read_access(db_read_access) {
        return Ok(vec![]);
    }
    let secrets = std::sync::Arc::clone(&app.state::<crate::db::DbSecrets>().0);
    let Ok(db) = crate::autorun::preconditions::environment_db(&root, Some(secrets.as_ref())) else {
        return Ok(vec![]);
    };
    let mut accounts = crate::autorun::accounts::load_accounts(&root)?;
    if let Some(key) = account_key.filter(|k| !k.trim().is_empty()) {
        accounts.retain(|a| a.key == key);
    }
    Ok(crate::autorun::sign_in_check::expiring(
        &crate::autorun::active_login::StageRows(&db),
        &env.password_expiry_query,
        &accounts,
        &crate::autorun::password_expiry::today(),
    )
    .await)
}
