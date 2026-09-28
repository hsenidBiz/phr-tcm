//! IPC surface for the API templates tab - see design doc "API templates"
//! §8. Building, proving and running a template are separate commands
//! (later tasks); this module only reads what is already saved and lets
//! the person remove one.
//!
//! Offered wherever Auto Run itself is offered - `ai_tools::autorun_offered`
//! - since the whole feature rides on the same signed-in browser session.

use crate::api_templates::store::{self, SavedTemplate};
use crate::autorun::recipe::{load_recipe, origin_of};

fn refuse_unless_offered() -> Result<(), String> {
    if !crate::ai_tools::autorun_offered() {
        return Err("not available in this build".to_string());
    }
    Ok(())
}

/// Everything the tab needs to draw itself: the recipe's origin (so the
/// tab can show which host these templates run against - `None` when the
/// project has no sign-in recipe yet), and every saved template with its
/// run history.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct TemplatesOverview {
    pub origin: Option<String>,
    pub templates: Vec<SavedTemplate>,
}

#[tauri::command]
#[specta::specta]
pub fn api_templates_overview(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<TemplatesOverview, String> {
    refuse_unless_offered()?;
    let root = crate::commands::autorun::root(&app)?;
    let origin = load_recipe(&root, &organization, &project)?.and_then(|r| origin_of(&r.start_url));
    let templates = store::list(&root, &organization, &project)?;
    Ok(TemplatesOverview { origin, templates })
}

#[tauri::command]
#[specta::specta]
pub fn api_templates_remove(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    id: String,
) -> Result<(), String> {
    refuse_unless_offered()?;
    let root = crate::commands::autorun::root(&app)?;
    store::remove(&root, &organization, &project, &id)?;
    crate::applog::info(format!("api template removed: {id}"));
    Ok(())
}
