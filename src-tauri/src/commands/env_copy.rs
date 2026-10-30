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
    // Read after the copy, not before the guards: a switch may have landed
    // while this waited.
    let active = crate::autorun::store::selected_root().as_deref()
        == Some(crate::autorun::roots::env_root(shared, target_id).as_path());
    Ok((done, active))
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

/// What copying one case would do, and the pieces both case commands share:
/// the source (the active environment's root) and the target's root.
/// `opening` is held for reading by the caller, so no switch lands between
/// the two reads: a switch writes it.
fn case_roots(shared: &Path, target_id: &str) -> Result<(PathBuf, PathBuf), String> {
    let envs = envs(shared)?;
    if !envs.environments.iter().any(|e| e.id == target_id) || !environments::valid_id(target_id) {
        return Err(env_copy::COPY_UNKNOWN_TARGET.to_string());
    }
    let source = crate::autorun::store::resolve_root()?;
    let target = crate::autorun::roots::env_root(shared, target_id);
    if source == target {
        return Err(env_copy::COPY_TO_ACTIVE.to_string());
    }
    Ok((source, target))
}

/// What copying a case into `target_id` would bring and keep; writes nothing.
pub async fn case_plan_with(
    shared: &Path,
    org: &str,
    project: &str,
    case_id: i32,
    target_id: &str,
) -> Result<env_copy::CaseCopyPlan, String> {
    let _no_switch = super::autorun_slots::opening().read().await;
    let (source, target) = case_roots(shared, target_id)?;
    let (org, project, id) = (org.to_string(), project.to_string(), target_id.to_string());
    tauri::async_runtime::spawn_blocking(move || env_copy::plan_case_copy(&source, &target, &id, &org, &project, case_id))
        .await
        .map_err(|_| env_copy::COPY_FAILED.to_string())?
}

/// Copies a case into `target_id`; `replace` agrees to replace a script the
/// target already has for it.
pub async fn case_copy_with(
    shared: &Path,
    org: &str,
    project: &str,
    case_id: i32,
    target_id: &str,
    replace: bool,
) -> Result<env_copy::CaseCopyPlan, String> {
    // Holding `opening` for reading keeps a switch (which writes it) from
    // changing which environment is the source while the copy runs. It is
    // taken before anything else, as a switch takes it first.
    let _no_switch = super::autorun_slots::opening().read().await;
    let (source, target) = case_roots(shared, target_id)?;
    let (org, project, id) = (org.to_string(), project.to_string(), target_id.to_string());
    tauri::async_runtime::spawn_blocking(move || {
        env_copy::copy_case(&source, &target, &id, &org, &project, case_id, replace)
    })
    .await
    .map_err(|_| env_copy::COPY_FAILED.to_string())?
}

/// What copying a case into another environment would bring (components,
/// area, sign-in recipe), what the target keeps its own version of, and what
/// the person has to add there. Writes nothing.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_copy_case_plan(
    _app: tauri::AppHandle,
    organization: String,
    project: String,
    case_id: i32,
    target_id: String,
) -> Result<env_copy::CaseCopyPlan, String> {
    case_plan_with(&shared()?, &organization, &project, case_id, &target_id).await
}

/// Copy a case's script, with what it needs, into another environment.
/// `replace` agrees to replace a script the target already has for the case.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_copy_case(
    _app: tauri::AppHandle,
    organization: String,
    project: String,
    case_id: i32,
    target_id: String,
    replace: bool,
) -> Result<env_copy::CaseCopyPlan, String> {
    case_copy_with(&shared()?, &organization, &project, case_id, &target_id, replace).await
}
