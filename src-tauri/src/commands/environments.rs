//! The environments, for the AI Bridge tab's Environment card and its
//! dialog. Each command is a thin wrapper over a `*_with` function that
//! takes the root and the secret store, so the tests drive the same body
//! against `MemoryStore`.
//!
//! The default password goes IN through `env_set_default_password` and never
//! comes back out: a view carries `has_default_password` and nothing more.

use crate::db::credentials::{self, DbSecrets, SecretStore};
use crate::environments::{self, EnvFile, Environment};
use std::path::Path;
use tauri::State;

/// One environment as the webview sees it. There is deliberately no
/// password field, not even an empty one.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct EnvView {
    pub id: String,
    pub name: String,
    pub start_url: String,
    pub allowed_origins: Vec<String>,
    pub db_id: String,
    pub test_environment: bool,
    pub has_default_password: bool,
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct EnvListView {
    pub active: String,
    pub environments: Vec<EnvView>,
}

/// An environment to save. An empty `id` adds a new one.
#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
pub struct EnvInput {
    pub id: String,
    pub name: String,
    pub start_url: String,
    pub allowed_origins: Vec<String>,
    pub db_id: String,
    pub test_environment: bool,
}

/// Said when a switch would land in the middle of a run, recording, check
/// or Try - which would mix two environments' accounts and sessions.
pub const SWITCH_BUSY: &str =
    "the environment cannot be switched while something is being recorded or run in Auto Run - finish or cancel it first";
/// Said while an API template run holds its slot.
pub const SWITCH_TEMPLATE_RUNNING: &str =
    "the environment cannot be switched while an API template is running - wait for it to finish first";
/// Said while the supervised Auto Run browser is open: it is signed in to
/// the environment it was opened in.
pub const SWITCH_SUPERVISED_OPEN: &str =
    "the environment cannot be switched while the supervised browser is open in Auto Run - close it first";

fn has_password(store: &dyn SecretStore, id: &str) -> bool {
    match store.get(&environments::password_target(id)) {
        Ok(v) => v.is_some(),
        Err(e) => {
            crate::applog::warn(format!("Environments: could not read whether a default password is set: {e}"));
            false
        }
    }
}

fn view(store: &dyn SecretStore, file: EnvFile) -> EnvListView {
    EnvListView {
        active: file.active,
        environments: file
            .environments
            .into_iter()
            .map(|e| EnvView {
                has_default_password: has_password(store, &e.id),
                id: e.id,
                name: e.name,
                start_url: e.start_url,
                allowed_origins: e.allowed_origins,
                db_id: e.db_id,
                test_environment: e.test_environment,
            })
            .collect(),
    }
}

fn known_db_ids(store: &dyn SecretStore) -> Vec<String> {
    credentials::databases(store).into_iter().map(|d| d.id).collect()
}

fn must_exist(root: &Path, id: &str) -> Result<(), String> {
    if environments::load_or_init(root, None)?.environments.iter().any(|e| e.id == id) {
        Ok(())
    } else {
        Err("that environment is not there any more".to_string())
    }
}

/// `current_db` is the database the Company database card has chosen, and
/// it only seeds a NEW Default: a choice that names no database this build
/// knows is dropped (Default then takes the first shipped one), so an id
/// that names nothing is never written into an environment.
pub fn list_view(root: &Path, store: &dyn SecretStore, current_db: Option<&str>) -> Result<EnvListView, String> {
    let known = known_db_ids(store);
    let current_db = current_db.map(str::trim).filter(|d| known.iter().any(|k| k == d));
    Ok(view(store, environments::load_or_init(root, current_db)?))
}

/// What every switch refusal starts with; an address change says
/// `ADDRESS_CHANGE_REFUSED` in its place.
const SWITCH_REFUSED: &str = "the environment cannot be switched";
/// Said, with the switch's reason, when the ACTIVE environment's address
/// or allowed sites would change under a run.
pub const ADDRESS_CHANGE_REFUSED: &str = "the active environment's website address cannot be changed";

fn sites(list: &[String]) -> Vec<String> {
    list.iter().map(|o| o.trim().to_string()).filter(|o| !o.is_empty()).collect()
}

/// Add or edit an environment. Changing where an environment signs in -
/// its address or allowed sites - is handled like a switch when it is the
/// ACTIVE one: refused while something records, runs, or holds the
/// supervised browser, because the rest of a run would sign in somewhere
/// else. A changed address also drops that environment's saved sessions:
/// they were made at the old address, and cookies are not port-scoped.
pub async fn save_with(root: &Path, store: &dyn SecretStore, env: EnvInput) -> Result<EnvListView, String> {
    let env = Environment {
        id: env.id,
        name: env.name,
        start_url: env.start_url,
        allowed_origins: env.allowed_origins,
        db_id: env.db_id,
        test_environment: env.test_environment,
    };
    let before = if env.id.is_empty() {
        None
    } else {
        environments::load_or_init(root, None)?.environments.into_iter().find(|e| e.id == env.id)
    };
    let address_moved = before.as_ref().is_some_and(|b| b.start_url.trim() != env.start_url.trim());
    let sites_moved = before.as_ref().is_some_and(|b| sites(&b.allowed_origins) != sites(&env.allowed_origins));
    if !address_moved && !sites_moved {
        let file = environments::save_env(root, env, &known_db_ids(store))?;
        crate::applog::info(format!("Environments: saved ({} environment(s))", file.environments.len()));
        return Ok(view(store, file));
    }
    // Held across the check and the write, exactly as a switch holds them
    // (`set_active_with`) - which also means no switch can make this the
    // active environment halfway through.
    let refused = |e: String| e.replacen(SWITCH_REFUSED, ADDRESS_CHANGE_REFUSED, 1);
    let slot = crate::commands::autorun::supervised().lock().await;
    let run_slot = if environments::active_id(root)? == env.id {
        refuse_switch(slot.is_some()).map_err(refused)?;
        Some(crate::api_templates::runner::claim().ok_or_else(|| refused(SWITCH_TEMPLATE_RUNNING.to_string()))?)
    } else {
        None
    };
    let id = env.id.clone();
    let file = environments::save_env(root, env, &known_db_ids(store))?;
    if address_moved {
        // `id` matched an entry the file check passed, so it is a
        // well-formed id and this path stays inside the root.
        match std::fs::remove_dir_all(crate::autorun::accounts::sessions_dir_for(root, &id)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => crate::applog::warn(format!(
                "Environments: the saved sessions of a moved environment stayed behind: {e}"
            )),
            _ => {}
        }
    }
    drop(run_slot);
    drop(slot);
    crate::applog::info(format!(
        "Environments: saved ({} environment(s)), one with a new {}",
        file.environments.len(),
        if address_moved { "website address" } else { "list of allowed sites" }
    ));
    Ok(view(store, file))
}

/// Removes the environment's files and its default password. A password
/// that will not go is logged, not fatal: the environment is gone already,
/// and the id is never handed out again.
pub fn remove_with(root: &Path, store: &dyn SecretStore, id: &str) -> Result<EnvListView, String> {
    let file = environments::remove_env(root, id)?;
    if let Err(e) = store.remove(&environments::password_target(id)) {
        crate::applog::warn(format!("Environments: a removed environment's default password stayed behind: {e}"));
    }
    crate::applog::info("Environments: one removed");
    Ok(view(store, file))
}

/// Whether a switch may happen now. `supervised_open` is passed in because
/// the supervised slot is behind an async lock the caller holds.
pub fn refuse_switch(supervised_open: bool) -> Result<(), String> {
    if crate::commands::autorun_record::recording_is_going() || crate::commands::autorun_replay::replay_is_running() {
        return Err(SWITCH_BUSY.to_string());
    }
    if crate::api_templates::runner::is_running() {
        return Err(SWITCH_TEMPLATE_RUNNING.to_string());
    }
    if supervised_open {
        return Err(SWITCH_SUPERVISED_OPEN.to_string());
    }
    Ok(())
}

pub async fn set_active_with(root: &Path, store: &dyn SecretStore, id: &str) -> Result<EnvListView, String> {
    // Held across the switch, so a supervised browser cannot open halfway
    // through it - the same way Open browser holds it to look for a recording.
    let slot = crate::commands::autorun::supervised().lock().await;
    refuse_switch(slot.is_some())?;
    // The template slot is TAKEN, not just looked at, for the write: a run
    // that started between the look in `refuse_switch` and the write would
    // otherwise sign in to one environment and finish in another.
    let run_slot = crate::api_templates::runner::claim().ok_or_else(|| SWITCH_TEMPLATE_RUNNING.to_string())?;
    let file = environments::set_active(root, id)?;
    drop(run_slot);
    drop(slot);
    if let Some(e) = file.environments.iter().find(|e| e.id == file.active) {
        crate::applog::info(format!("Environments: switched to {}", e.name));
    }
    Ok(view(store, file))
}

pub fn set_default_password_with(root: &Path, store: &dyn SecretStore, id: &str, password: &str) -> Result<(), String> {
    if password.is_empty() {
        return Err("type the default password first".to_string());
    }
    must_exist(root, id)?;
    store.put(&environments::password_target(id), password)
}

pub fn clear_default_password_with(root: &Path, store: &dyn SecretStore, id: &str) -> Result<(), String> {
    must_exist(root, id)?;
    store.remove(&environments::password_target(id))
}

/// One proposed account a person picked to add. `password` is what they
/// typed; empty means "use the environment's default password".
#[derive(Clone, serde::Deserialize, specta::Type)]
pub struct AccountInput {
    pub key: String,
    pub label: String,
    pub username: String,
    pub password: String,
}

/// Hand-written so a password cannot reach a log line through `{:?}`.
impl std::fmt::Debug for AccountInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountInput")
            .field("key", &self.key)
            .field("label", &self.label)
            .field("username", &self.username)
            .field("password", &"(hidden)")
            .finish()
    }
}

/// Said for a pick with no password typed while the environment has no
/// default password.
pub const NO_DEFAULT_PASSWORD: &str = "no default password set - type one";

/// The active environment's proposed accounts.
pub fn proposals_with(root: &Path) -> Result<Vec<environments::ProposedAccount>, String> {
    environments::load_proposals(root, &environments::active_id(root)?)
}

/// Drop the active environment's whole proposal.
pub fn dismiss_proposals_with(root: &Path) -> Result<(), String> {
    environments::save_proposals(root, &environments::active_id(root)?, &[])
}

/// Add the picked proposals to the active environment's accounts.
///
/// Every pick is checked first - a usable key used once, a username, and a
/// password (typed, or the environment's default) - and one that fails
/// refuses the whole call with nothing written. A pick whose key is
/// already an account is written only when `replace` names it; otherwise
/// it is left as it is and its key returned, for the person to confirm.
/// What was added leaves the proposal.
pub fn add_proposals_with(
    root: &Path,
    store: &dyn SecretStore,
    picks: Vec<AccountInput>,
    replace: Vec<String>,
) -> Result<Vec<String>, String> {
    use crate::autorun::accounts::{load_accounts_for, save_accounts_for, valid_key, Account};
    let env_id = environments::active_id(root)?;
    let mut accounts = load_accounts_for(root, &env_id)?;
    let mut default: Option<Option<String>> = None;
    let mut seen = std::collections::HashSet::new();
    let mut ready: Vec<Account> = vec![];
    let mut confirm: Vec<String> = vec![];
    for p in picks {
        let key = p.key.trim().to_string();
        if !valid_key(&key) {
            return Err(format!(
                "\"{key}\" is not a usable account key - use lowercase letters, digits, dot, underscore or hyphen"
            ));
        }
        if !seen.insert(key.clone()) {
            return Err(format!("the account key \"{key}\" appears more than once"));
        }
        let username = p.username.trim().to_string();
        if username.is_empty() {
            return Err(format!("the account \"{key}\" has no username"));
        }
        if accounts.iter().any(|a| a.key == key) && !replace.iter().any(|r| r.trim() == key) {
            confirm.push(key);
            continue;
        }
        let password = if p.password.is_empty() {
            if default.is_none() {
                let found = store
                    .get(&environments::password_target(&env_id))
                    .map_err(|e| format!("the default password could not be read: {e}"))?;
                default = Some(found.filter(|d| !d.is_empty()));
            }
            match default.as_ref().and_then(|d| d.clone()) {
                Some(d) => d,
                None => return Err(format!("\"{key}\": {NO_DEFAULT_PASSWORD}")),
            }
        } else {
            p.password
        };
        let label = match p.label.trim() {
            "" => key.clone(),
            l => l.to_string(),
        };
        ready.push(Account { key, label, username, password });
    }
    if ready.is_empty() {
        return Ok(confirm);
    }
    let added: Vec<String> = ready.iter().map(|a| a.key.clone()).collect();
    for a in ready {
        match accounts.iter_mut().find(|e| e.key == a.key) {
            Some(slot) => *slot = a,
            None => accounts.push(a),
        }
    }
    // Written back by the id it was read by: a switch in between must not
    // put this environment's list into the next one.
    save_accounts_for(root, &env_id, &accounts)?;
    crate::applog::info(format!("Environments: added {} proposed account(s)", added.len()));
    // The accounts are saved: a proposal that will not update is logged,
    // not a failure of the add.
    let left = environments::load_proposals(root, &env_id)
        .map(|list| list.into_iter().filter(|p| !added.contains(&p.key)).collect::<Vec<_>>())
        .and_then(|list| environments::save_proposals(root, &env_id, &list));
    if let Err(e) = left {
        crate::applog::warn(format!("Environments: the added accounts stayed among the proposed ones: {e}"));
    }
    Ok(confirm)
}

/// The environments and which is active. `current_db` is the database the
/// Company database card has chosen now: on first use it becomes Default's.
#[tauri::command]
#[specta::specta]
pub fn env_list(
    app: tauri::AppHandle,
    secrets: State<'_, DbSecrets>,
    current_db: Option<String>,
) -> Result<EnvListView, String> {
    list_view(&super::autorun::root(&app)?, &*secrets.0, current_db.as_deref())
}

#[tauri::command]
#[specta::specta]
pub async fn env_save(
    app: tauri::AppHandle,
    secrets: State<'_, DbSecrets>,
    env: EnvInput,
) -> Result<EnvListView, String> {
    save_with(&super::autorun::root(&app)?, &*secrets.0, env).await
}

#[tauri::command]
#[specta::specta]
pub fn env_remove(app: tauri::AppHandle, secrets: State<'_, DbSecrets>, id: String) -> Result<EnvListView, String> {
    remove_with(&super::autorun::root(&app)?, &*secrets.0, &id)
}

#[tauri::command]
#[specta::specta]
pub async fn env_set_active(
    app: tauri::AppHandle,
    secrets: State<'_, DbSecrets>,
    id: String,
) -> Result<EnvListView, String> {
    set_active_with(&super::autorun::root(&app)?, &*secrets.0, &id).await
}

#[tauri::command]
#[specta::specta]
pub fn env_set_default_password(
    app: tauri::AppHandle,
    secrets: State<'_, DbSecrets>,
    id: String,
    password: String,
) -> Result<(), String> {
    set_default_password_with(&super::autorun::root(&app)?, &*secrets.0, &id, &password)
}

#[tauri::command]
#[specta::specta]
pub fn env_clear_default_password(
    app: tauri::AppHandle,
    secrets: State<'_, DbSecrets>,
    id: String,
) -> Result<(), String> {
    clear_default_password_with(&super::autorun::root(&app)?, &*secrets.0, &id)
}

/// The assistant's proposed accounts for the active environment.
#[tauri::command]
#[specta::specta]
pub fn env_proposals(app: tauri::AppHandle) -> Result<Vec<environments::ProposedAccount>, String> {
    proposals_with(&super::autorun::root(&app)?)
}

/// Dismiss the active environment's whole proposal.
#[tauri::command]
#[specta::specta]
pub fn env_dismiss_proposals(app: tauri::AppHandle) -> Result<(), String> {
    dismiss_proposals_with(&super::autorun::root(&app)?)
}

/// Add the picked proposals; returns the keys that are already accounts and
/// need the person's confirmation (resend them in `replace`).
#[tauri::command]
#[specta::specta]
pub fn env_add_proposals(
    app: tauri::AppHandle,
    secrets: State<'_, DbSecrets>,
    picks: Vec<AccountInput>,
    replace: Vec<String>,
) -> Result<Vec<String>, String> {
    add_proposals_with(&super::autorun::root(&app)?, &*secrets.0, picks, replace)
}
