//! A script's setup in the script editor: what it does, and the person's
//! approval of it (`autorun::approvals`). These three commands are the
//! only callers of `approvals::approve` and `approvals::withdraw`: they
//! are the webview's, never a bridge route or an MCP tool, so an assistant
//! can never approve its own setup.

use crate::autorun::{approvals, setup, store};

/// The script's setup, or the sentence for a case with none.
fn script_with_setup(root: &std::path::Path, case_id: i32) -> Result<crate::autorun::CaseScript, String> {
    let script = store::load_script(root, case_id)?.ok_or_else(|| format!("case {case_id} has no saved script"))?;
    if script.setup.is_none() {
        return Err(format!("case {case_id}'s script has no setup"));
    }
    Ok(script)
}

/// The editor's view of case `case_id`'s setup: its fixture, account,
/// steps and what it makes, and where its approval stands. `None` when the
/// saved script has no setup.
#[tauri::command]
#[specta::specta]
pub fn auto_run_setup_view(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    case_id: i32,
) -> Result<Option<setup::SetupView>, String> {
    let root = super::autorun::root(&app)?;
    let Some(script) = store::load_script(&root, case_id)? else { return Ok(None) };
    setup::view(&root, &organization, &project, &script)
}

/// The person's Approve setup: approves case `case_id`'s setup exactly as
/// the person was shown it - `expected_fingerprint` is the
/// `SetupView::fingerprint` they saw. A setup that changed since is
/// refused (`setup::CHANGED_WHILE_LOOKING`) and nothing is approved. Any
/// later change to the setup, its fixture or a template it runs clears it.
#[tauri::command]
#[specta::specta]
pub fn auto_run_approve_setup(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    case_id: i32,
    expected_fingerprint: String,
) -> Result<setup::SetupView, String> {
    let root = super::autorun::root(&app)?;
    let script = script_with_setup(&root, case_id)?;
    let fp = setup::approval_target(&root, &organization, &project, &script, &expected_fingerprint)?;
    approvals::approve(&root, case_id, &fp)?;
    crate::applog::info(format!("Auto Run: the person approved case {case_id}'s setup"));
    setup::view(&root, &organization, &project, &script)?.ok_or_else(|| format!("case {case_id}'s script has no setup"))
}

/// The person's Withdraw approval: case `case_id`'s setup is no longer
/// approved, and the case is Blocked until it is approved again.
#[tauri::command]
#[specta::specta]
pub fn auto_run_withdraw_setup(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    case_id: i32,
) -> Result<setup::SetupView, String> {
    let root = super::autorun::root(&app)?;
    let script = script_with_setup(&root, case_id)?;
    approvals::withdraw(&root, case_id)?;
    crate::applog::info(format!("Auto Run: the person withdrew the approval of case {case_id}'s setup"));
    setup::view(&root, &organization, &project, &script)?.ok_or_else(|| format!("case {case_id}'s script has no setup"))
}
