//! The two commands behind "Copy Auto Run data from..." on an environment.
//! The file work is `autorun::env_copy`; this module holds the same guards a
//! switch holds, so a copy never lands in the middle of a run, a recording
//! or an open browser.

use crate::autorun::env_copy::{self, CopyChoices, CopyDone};
use crate::environments::{self, EnvFile};
use std::path::{Path, PathBuf};

fn shared() -> Result<PathBuf, String> {
    crate::autorun::store::shared_root().ok_or_else(|| super::autorun::NO_DATA_DIRECTORY.to_string())
}

fn envs(shared: &Path) -> Result<EnvFile, String> {
    environments::load_or_init(shared, None)
}

/// Where `target_id` can copy from, and whether it has data a copy would replace.
pub fn choices_with(shared: &Path, target_id: &str) -> Result<CopyChoices, String> {
    env_copy::copy_choices(shared, &envs(shared)?, target_id, crate::autorun::sessions::now_ms())
}

/// The copy, under the switch's own guards. The second value is whether the
/// target is the active environment, whose screens then read it again.
pub async fn copy_with(
    shared: &Path,
    target_id: &str,
    source_key: &str,
    replace: bool,
) -> Result<(CopyDone, bool), String> {
    // What can be known without the guards is checked first: a refusal that
    // needs no lock should not wait for one.
    let envs = envs(shared)?;
    let now = crate::autorun::sessions::now_ms();
    let target = envs
        .environments
        .iter()
        .find(|e| e.id == target_id)
        .ok_or_else(|| env_copy::COPY_UNKNOWN_TARGET.to_string())?;
    if source_key == target_id {
        return Err(env_copy::COPY_ONTO_ITSELF.to_string());
    }
    let source = env_copy::source_root(shared, &envs, source_key, now)?;
    // The same order a switch takes: no browser opens, every session is
    // held, nothing records, runs or has a browser open, and the template
    // slot is taken. Then the file work, which holds the switch lock.
    let _opening = super::autorun_slots::opening().write().await;
    let slots = super::autorun::lock_every_slot().await;
    if slots.busy {
        return Err(super::environments::SWITCH_BUSY.to_string());
    }
    super::environments::refuse_switch(slots.any_open())?;
    let _run_slot = crate::api_templates::runner::claim()
        .ok_or_else(|| super::environments::SWITCH_TEMPLATE_RUNNING.to_string())?;

    let (shared_owned, id, name) = (shared.to_path_buf(), target.id.clone(), target.name.clone());
    let done = tauri::async_runtime::spawn_blocking(move || {
        env_copy::copy_environment(&shared_owned, &source, &id, &name, replace, now)
    })
    .await
    .map_err(|_| env_copy::COPY_FAILED.to_string())??;
    Ok((done, envs.active == target_id))
}

/// Where the environment can copy from.
#[tauri::command]
#[specta::specta]
pub fn env_copy_choices(_app: tauri::AppHandle, target_id: String) -> Result<CopyChoices, String> {
    choices_with(&shared()?, &target_id)
}

/// Copy a source's Auto Run data into the environment. A target that already
/// holds data needs `replace`; what it held is kept for 30 days.
#[tauri::command]
#[specta::specta]
pub async fn env_copy_autorun(
    app: tauri::AppHandle,
    target_id: String,
    source_key: String,
    replace: bool,
) -> Result<CopyDone, String> {
    let (done, active) = copy_with(&shared()?, &target_id, &source_key, replace).await?;
    if active {
        use tauri_specta::Event as _;
        let _ = crate::events::AutorunEnvironmentChanged { env_id: target_id }.emit(&app);
    }
    Ok(done)
}
