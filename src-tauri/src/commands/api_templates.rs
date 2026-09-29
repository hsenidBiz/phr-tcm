//! IPC surface for the API templates tab - see design doc "API templates"
//! §8. Building, proving and running a template are separate commands
//! (later tasks); this module only reads what is already saved and lets
//! the person remove one.
//!
//! Offered wherever Auto Run itself is offered - `ai_tools::autorun_offered`
//! - since the whole feature rides on the same signed-in browser session.

use crate::api_templates::flow::Flow;
use crate::api_templates::flow_store;
use crate::api_templates::store::{self, SavedTemplate};
use crate::autorun::recipe::{load_recipe, origin_of};

fn refuse_unless_offered() -> Result<(), String> {
    refuse_unless(crate::ai_tools::autorun_offered())
}

fn refuse_unless(offered: bool) -> Result<(), String> {
    if !offered {
        return Err("not available in this build".to_string());
    }
    Ok(())
}

/// Everything the tab needs to draw itself: the recipe's origin (so the
/// tab can show which host these templates run against - `None` when the
/// project has no sign-in recipe yet), and every saved template with its
/// run history, and every saved flow.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct TemplatesOverview {
    pub origin: Option<String>,
    pub templates: Vec<SavedTemplate>,
    pub flows: Vec<Flow>,
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
    overview_at(&root, &organization, &project)
}

/// `api_templates_overview` for a given data root. The flows are a
/// convenience on top of the templates: if the flows directory cannot be
/// listed at all, the tab still gets its templates and an empty list of
/// flows, and the reason goes to the log - flows must never break the tab.
pub fn overview_at(root: &std::path::Path, organization: &str, project: &str) -> Result<TemplatesOverview, String> {
    let origin = load_recipe(root, organization, project)?.and_then(|r| origin_of(&r.start_url));
    let templates = store::list(root, organization, project)?;
    let flows = flow_store::list(root, organization, project).unwrap_or_else(|e| {
        crate::applog::warn(format!("api template flows could not be listed: {e}"));
        Vec::new()
    });
    Ok(TemplatesOverview { origin, templates, flows })
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

#[tauri::command]
#[specta::specta]
pub fn api_templates_remove_flow(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    id: String,
) -> Result<(), String> {
    let root = crate::commands::autorun::root(&app)?;
    remove_flow_at(crate::ai_tools::autorun_offered(), &root, &organization, &project, &id)
}

/// `api_templates_remove_flow` for a given data root, with "is Auto Run
/// offered here" passed in so a locked build's refusal is testable.
pub fn remove_flow_at(
    offered: bool,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    id: &str,
) -> Result<(), String> {
    refuse_unless(offered)?;
    flow_store::remove(root, organization, project, id)?;
    crate::applog::info(format!("api template flow removed: {id}"));
    Ok(())
}
