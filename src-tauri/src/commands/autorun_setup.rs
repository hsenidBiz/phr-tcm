//! A script's setup in the script editor: what it does.

use crate::autorun::{setup, store};

/// The editor's view of case `case_id`'s setup: its fixture, account,
/// steps and what it makes. `None` when the saved script has no setup.
#[tauri::command(async)]
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
