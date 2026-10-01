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

pub fn list_view(root: &Path, store: &dyn SecretStore, current_db: Option<&str>) -> Result<EnvListView, String> {
    Ok(view(store, environments::load_or_init(root, current_db)?))
}

pub fn save_with(root: &Path, store: &dyn SecretStore, env: EnvInput) -> Result<EnvListView, String> {
    let env = Environment {
        id: env.id,
        name: env.name,
        start_url: env.start_url,
        allowed_origins: env.allowed_origins,
        db_id: env.db_id,
        test_environment: env.test_environment,
    };
    let file = environments::save_env(root, env, &known_db_ids(store))?;
    crate::applog::info(format!("Environments: saved ({} environment(s))", file.environments.len()));
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
pub fn env_save(app: tauri::AppHandle, secrets: State<'_, DbSecrets>, env: EnvInput) -> Result<EnvListView, String> {
    save_with(&super::autorun::root(&app)?, &*secrets.0, env)
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
