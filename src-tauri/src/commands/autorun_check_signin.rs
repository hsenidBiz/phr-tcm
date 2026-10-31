//! Check sign-in, for the Accounts dialog (`auto_run_check_sign_in`) and the
//! assistant's `check_autorun_sign_in` (`ai_bridge`). The checking itself is
//! `autorun::sign_in_check`; this is who may ask, and when.
//!
//! A check opens a browser of its own for each account (never the person's
//! Auto Run browser), so it holds the same claim an unattended run does
//! while it goes: no run, replay or second check starts beside it, and
//! `auto_run_open_browser` waits. It is refused while an unattended run, a
//! replay or a discovery is using the Auto Run browser.

use crate::autorun::active_login::session_is_ours;
use crate::autorun::replay::Browsers;
use crate::autorun::setup_find::RowSource;
use crate::autorun::sign_in_check::{check_accounts, precheck, summary, Mark, Precheck, SignInCheck, BROWSER_IN_USE};
use crate::autorun::slots::SlotNo;
use std::collections::BTreeMap;
use std::path::Path;

/// Why a check cannot begin now, or `None`: an unattended run, a replay to a
/// step or a discovery has the Auto Run browser.
pub fn refusal() -> Option<String> {
    let busy = crate::commands::autorun_replay::replay_is_running()
        || SlotNo::all().into_iter().any(|n| {
            crate::autorun::replay_to::state(n).is_running() || crate::commands::autorun::discovery_active_in(n)
        });
    busy.then(|| BROWSER_IN_USE.to_string())
}

/// Checks `keys` one at a time (`sign_in_check::check_accounts`), the
/// environment's two queries (`queries`: password expiry, active session)
/// asked first through `rows` when there is one. `Err` is the sentence for a
/// check that could not begin at all.
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
    if let Some(busy) = refusal() {
        return Err(busy);
    }
    // Held until the last browser is closed.
    let Some(_claim) = crate::commands::autorun_replay::OneAtATime::claim() else {
        return Err(BROWSER_IN_USE.to_string());
    };
    let mut pre: BTreeMap<String, Precheck> = BTreeMap::new();
    if let (Some(rows), false) = (rows, queries.0.trim().is_empty() && queries.1.trim().is_empty()) {
        let env_id = crate::environments::active_id(root)?;
        let minutes = crate::autorun::recipe::load_effective_recipe_if_any(root, org, project)
            .ok()
            .flatten()
            .map_or(480, |r| r.session_minutes);
        let accounts = crate::autorun::accounts::load_accounts(root)?;
        let today = crate::autorun::password_expiry::today();
        for key in keys {
            let Some(account) = accounts.iter().find(|a| &a.key == key) else { continue };
            let ours = session_is_ours(root, &env_id, key, minutes, crate::autorun::sessions::now_ms());
            pre.insert(key.clone(), precheck(rows, queries.0, queries.1, key, &account.username, ours, &today).await);
        }
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
