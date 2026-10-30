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

/// Which way a case moves, between the environment on screen (`active_id`)
/// and `other_id`.
#[derive(Clone, Copy)]
pub enum CaseWay {
    /// From the active environment into `other_id`.
    Push,
    /// From `other_id` into the active environment.
    Pull,
}

/// The source and target roots of a case copy, pinned: read once, after the
/// caller holds `opening` for reading (a switch writes it), and used for the
/// whole copy. Refused when the environment on screen is no longer active,
/// when `other_id` is not a listed environment, and when the per-environment
/// folders are not in use (a copy would start them early).
fn case_roots(shared: &Path, active_id: &str, other_id: &str, way: CaseWay) -> Result<(PathBuf, PathBuf), String> {
    use env_copy::{CASE_COPY_NEEDS_LAYOUT, CASE_COPY_SWITCHED, COPY_FROM_ACTIVE, COPY_TO_ACTIVE};
    let envs = envs(shared)?;
    let listed = |id: &str| environments::valid_id(id) && envs.environments.iter().any(|e| e.id == id);
    let active = crate::autorun::store::resolve_root()?;
    if crate::autorun::roots::env_id_of(&active).is_none() {
        return Err(CASE_COPY_NEEDS_LAYOUT.to_string());
    }
    if !listed(other_id) {
        return Err(env_copy::COPY_UNKNOWN_TARGET.to_string());
    }
    if active != crate::autorun::roots::env_root(shared, active_id) || !listed(active_id) {
        return Err(CASE_COPY_SWITCHED.to_string());
    }
    if other_id == active_id {
        return Err(match way {
            CaseWay::Push => COPY_TO_ACTIVE,
            CaseWay::Pull => COPY_FROM_ACTIVE,
        }
        .to_string());
    }
    let other = crate::autorun::roots::env_root(shared, other_id);
    Ok(match way {
        CaseWay::Push => (active, other),
        CaseWay::Pull => (other, active),
    })
}

/// What copying a case between the active environment and `other_id` would
/// bring and keep; writes nothing.
pub async fn case_plan_with(
    shared: &Path,
    org: &str,
    project: &str,
    case_id: i32,
    active_id: &str,
    other_id: &str,
    way: CaseWay,
) -> Result<env_copy::CaseCopyPlan, String> {
    let _no_switch = super::autorun_slots::opening().read().await;
    let (source, target) = case_roots(shared, active_id, other_id, way)?;
    let target_id = crate::autorun::roots::env_id_of(&target).unwrap_or_default();
    let (org, project) = (org.to_string(), project.to_string());
    tauri::async_runtime::spawn_blocking(move || env_copy::plan_case_copy(&source, &target, &target_id, &org, &project, case_id))
        .await
        .map_err(|_| env_copy::COPY_FAILED.to_string())?
}

/// Copies a case between the active environment and `other_id`; `replace`
/// agrees to replace a script the target already has for it.
pub async fn case_copy_with(
    shared: &Path,
    org: &str,
    project: &str,
    case_id: i32,
    active_id: &str,
    other_id: &str,
    way: CaseWay,
    replace: bool,
) -> Result<env_copy::CaseCopyPlan, String> {
    // Holding `opening` for reading keeps a switch (which writes it) from
    // changing which environment is active while the copy runs; it is taken
    // before the roots are read, which are then used throughout.
    let _no_switch = super::autorun_slots::opening().read().await;
    let (source, target) = case_roots(shared, active_id, other_id, way)?;
    let target_id = crate::autorun::roots::env_id_of(&target).unwrap_or_default();
    let (org, project) = (org.to_string(), project.to_string());
    tauri::async_runtime::spawn_blocking(move || {
        env_copy::copy_case(&source, &target, &target_id, &org, &project, case_id, replace)
    })
    .await
    .map_err(|_| env_copy::COPY_FAILED.to_string())?
}

/// Push: what copying a case from the environment on screen (`active_id`)
/// into `target_id` would bring (components, areas, sign-in recipe), what
/// the target keeps its own version of, and what the person has to add
/// there. Writes nothing.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_copy_case_plan(
    _app: tauri::AppHandle,
    organization: String,
    project: String,
    case_id: i32,
    active_id: String,
    target_id: String,
) -> Result<env_copy::CaseCopyPlan, String> {
    case_plan_with(&shared()?, &organization, &project, case_id, &active_id, &target_id, CaseWay::Push).await
}

/// Push: copy a case's script, with what it needs, into another environment.
/// `replace` agrees to replace a script the target already has for the case.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_copy_case(
    _app: tauri::AppHandle,
    organization: String,
    project: String,
    case_id: i32,
    active_id: String,
    target_id: String,
    replace: bool,
) -> Result<env_copy::CaseCopyPlan, String> {
    case_copy_with(&shared()?, &organization, &project, case_id, &active_id, &target_id, CaseWay::Push, replace).await
}

/// Pull ("Copy from environment..."): the same dry run, for a case coming
/// from `source_id` into the environment on screen (`active_id`).
#[tauri::command]
#[specta::specta]
pub async fn auto_run_copy_case_from_plan(
    _app: tauri::AppHandle,
    organization: String,
    project: String,
    case_id: i32,
    active_id: String,
    source_id: String,
) -> Result<env_copy::CaseCopyPlan, String> {
    case_plan_with(&shared()?, &organization, &project, case_id, &active_id, &source_id, CaseWay::Pull).await
}

/// Pull: copy a case from another environment into the one on screen.
/// `replace` agrees to replace the script it already has for the case.
#[tauri::command]
#[specta::specta]
pub async fn auto_run_copy_case_from(
    _app: tauri::AppHandle,
    organization: String,
    project: String,
    case_id: i32,
    active_id: String,
    source_id: String,
    replace: bool,
) -> Result<env_copy::CaseCopyPlan, String> {
    case_copy_with(&shared()?, &organization, &project, case_id, &active_id, &source_id, CaseWay::Pull, replace).await
}
