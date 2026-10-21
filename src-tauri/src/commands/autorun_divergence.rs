//! A person's side of a reported divergence: the list the case card shows,
//! the ruling, and the question a step-by-step run asks before it signs in.

use crate::autorun::divergences::{self, Divergence, DivergenceStatus, Ruling};
use std::path::Path;

/// A ruled divergence stays visible this long.
const SHOW_RULED_MS: u64 = 7 * 24 * 60 * 60 * 1000;

/// Every open or awaiting-repair divergence, and the ruled ones of the last
/// seven days.
pub fn load_at(root: &Path, org: &str, project: &str, now_ms: u64) -> Result<Vec<Divergence>, String> {
    let mut list = divergences::load(root, org, project)?;
    list.retain(|d| match d.status {
        DivergenceStatus::Ruled => d
            .ruled_at
            .as_deref()
            .and_then(|s| s.parse::<u64>().ok())
            .is_some_and(|at| now_ms.saturating_sub(at) <= SHOW_RULED_MS),
        _ => true,
    });
    Ok(list)
}

/// The pure half of [`auto_run_rule_divergence`].
pub fn rule_at(
    root: &Path,
    org: &str,
    project: &str,
    id: &str,
    ruling: Ruling,
    now_ms: u64,
) -> Result<Vec<Divergence>, String> {
    let case = divergences::update(root, org, project, |list| divergences::rule_in(list, id, ruling, now_ms))?;
    crate::applog::info(format!("Auto Run: divergence {id} on case {case} ruled {ruling:?} in the app"));
    load_at(root, org, project, now_ms)
}

#[tauri::command]
#[specta::specta]
pub fn auto_run_load_divergences(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<Vec<Divergence>, String> {
    load_at(&super::autorun::root(&app)?, &organization, &project, crate::autorun::sessions::now_ms())
}

#[tauri::command]
#[specta::specta]
pub fn auto_run_rule_divergence(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    id: String,
    ruling: Ruling,
) -> Result<Vec<Divergence>, String> {
    rule_at(&super::autorun::root(&app)?, &organization, &project, &id, ruling, crate::autorun::sessions::now_ms())
}

#[tauri::command]
#[specta::specta]
pub fn auto_run_needs_ruling(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    case_id: i32,
) -> Result<Option<String>, String> {
    Ok(divergences::needs_ruling(&super::autorun::root(&app)?, &organization, &project, case_id))
}
