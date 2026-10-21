//! A person's side of a reported divergence: the list the case card shows,
//! the ruling, and the question a step-by-step run asks before it signs in.

use crate::autorun::case_text::CaseText;
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

/// The pure half of [`auto_run_undo_divergence_ruling`].
pub fn undo_at(root: &Path, org: &str, project: &str, id: &str, now_ms: u64) -> Result<Vec<Divergence>, String> {
    let case = divergences::update(root, org, project, |list| divergences::undo_in(list, id))?;
    crate::applog::info(format!("Auto Run: the ruling on divergence {id} (case {case}) was undone in the app"));
    load_at(root, org, project, now_ms)
}

/// The cases the project has a divergence waiting on a rewrite for, among
/// `only` (all of them when `None`). A file that cannot be read has none.
pub fn awaiting_cases(root: &Path, org: &str, project: &str, only: Option<&[i32]>) -> Vec<i32> {
    let mut ids: Vec<i32> = divergences::load(root, org, project)
        .unwrap_or_default()
        .iter()
        .filter(|d| d.status == DivergenceStatus::AwaitingRepair)
        .map(|d| d.case_id)
        .filter(|id| only.is_none_or(|o| o.contains(id)))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// Reads the cases whose divergences wait for a rewrite and compares each
/// case's steps text with the print taken when the person ruled: a case
/// rewritten in Azure DevOps closes its divergences, and a ruling with no
/// print yet gets one (`divergences::note_case_text`). Read only; a case or
/// a file that cannot be read leaves its divergences as they are, said in
/// the log. Answers the (case, step) pairs closed.
pub async fn refresh_rewritten(
    client: &crate::ado::AdoClient,
    root: &Path,
    org: &str,
    project: &str,
    only: Option<&[i32]>,
) -> Vec<(i32, i32)> {
    let ids = awaiting_cases(root, org, project, only);
    if ids.is_empty() {
        return vec![];
    }
    let cases = match client.get_test_cases_by_ids(org, &ids, None, None).await {
        Ok(cases) => cases,
        Err(e) => {
            crate::applog::warn(format!("Auto Run: the cases waiting for a rewrite could not be read: {}", e.user_text()));
            return vec![];
        }
    };
    let now = crate::autorun::sessions::now_ms();
    let closed = divergences::update(root, org, project, |list| {
        let mut closed = vec![];
        for case in &cases {
            let text = CaseText::from_case(case);
            closed.extend(divergences::note_case_text(list, case.id, &text, now).into_iter().map(|s| (case.id, s)));
        }
        Ok(closed)
    });
    match closed {
        Ok(closed) => {
            for (case, step) in &closed {
                crate::applog::info(format!(
                    "Auto Run: case {case} was rewritten in Azure DevOps; its divergence at step {step} is closed"
                ));
            }
            closed
        }
        Err(e) => {
            crate::applog::warn(format!("Auto Run: the divergences could not be updated after reading the cases: {e}"));
            vec![]
        }
    }
}

/// [`refresh_rewritten`] with the signed-in person's client; nothing when
/// signed out.
pub async fn refresh_rewritten_in_app(
    app: &tauri::AppHandle,
    root: &Path,
    org: &str,
    project: &str,
    only: Option<&[i32]>,
) {
    if awaiting_cases(root, org, project, only).is_empty() {
        return;
    }
    let Ok(token) = crate::state::get_fresh_token(app).await else { return };
    refresh_rewritten(&crate::ado::AdoClient::new(token), root, org, project, only).await;
}

#[tauri::command]
#[specta::specta]
pub async fn auto_run_load_divergences(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<Vec<Divergence>, String> {
    let root = super::autorun::root(&app)?;
    refresh_rewritten_in_app(&app, &root, &organization, &project, None).await;
    load_at(&root, &organization, &project, crate::autorun::sessions::now_ms())
}

#[tauri::command]
#[specta::specta]
pub async fn auto_run_rule_divergence(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    id: String,
    ruling: Ruling,
) -> Result<Vec<Divergence>, String> {
    let root = super::autorun::root(&app)?;
    let ruled = rule_at(&root, &organization, &project, &id, ruling, crate::autorun::sessions::now_ms())?;
    if ruling != Ruling::Rewrite {
        return Ok(ruled);
    }
    // The case's steps text as it reads at the ruling, so a later rewrite is
    // seen as a difference.
    refresh_rewritten_in_app(&app, &root, &organization, &project, None).await;
    load_at(&root, &organization, &project, crate::autorun::sessions::now_ms())
}

/// A person takes back a "Rewrite the case" ruling: the divergence needs a
/// ruling again.
#[tauri::command]
#[specta::specta]
pub fn auto_run_undo_divergence_ruling(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    id: String,
) -> Result<Vec<Divergence>, String> {
    undo_at(&super::autorun::root(&app)?, &organization, &project, &id, crate::autorun::sessions::now_ms())
}

#[tauri::command]
#[specta::specta]
pub async fn auto_run_needs_ruling(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    case_id: i32,
) -> Result<Option<String>, String> {
    let root = super::autorun::root(&app)?;
    refresh_rewritten_in_app(&app, &root, &organization, &project, Some(&[case_id])).await;
    Ok(divergences::needs_ruling(&root, &organization, &project, case_id))
}
