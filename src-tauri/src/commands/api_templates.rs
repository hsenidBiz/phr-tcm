//! IPC surface for the API templates tab - see design doc "API templates"
//! §8. Building, proving and running a template are separate commands
//! (later tasks); this module only reads what is already saved and lets
//! the person remove one.
//!
//! Offered wherever Auto Run itself is offered - `ai_tools::autorun_offered`
//! - since the whole feature rides on the same signed-in browser session.

use crate::api_templates::fixture_run::{self, FixtureReport};
use crate::api_templates::fixture_store::{self, SavedFixture};
use crate::api_templates::flow::Flow;
use crate::api_templates::flow_store;
use crate::api_templates::share::{self, TemplatesExportResult, TemplatesImportNote, TemplatesImportResult, TemplatesImportSkip};
use crate::api_templates::store::{self, SavedTemplate};
use crate::autorun::recipe::{load_effective_recipe_if_any, origin_of};

fn refuse_unless_offered() -> Result<(), String> {
    refuse_unless(crate::ai_tools::autorun_offered())
}

pub(crate) fn refuse_unless(offered: bool) -> Result<(), String> {
    if !offered {
        return Err("not available in this build".to_string());
    }
    Ok(())
}

/// Everything the tab needs to draw itself: the origin these templates run
/// against - the active environment's address, else the sign-in recipe's
/// (`None` when the project has no site address yet) - and every saved
/// template with its run history, and every saved flow.
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
    let origin = load_effective_recipe_if_any(root, organization, project)?.and_then(|r| origin_of(&r.start_url));
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

/// Every saved fixture of the project, with its run history, for the
/// Fixtures tab.
#[tauri::command]
#[specta::specta]
pub fn api_fixtures_list(app: tauri::AppHandle, organization: String, project: String) -> Result<Vec<SavedFixture>, String> {
    refuse_unless_offered()?;
    let root = crate::commands::autorun::root(&app)?;
    fixture_store::list(&root, &organization, &project)
}

/// Runs a saved fixture: Run (its first build) and Rebuild are the same
/// command. A headless browser, as a template run from the AI Bridge uses;
/// the run holds the one-at-a-time template slot throughout.
#[tauri::command]
#[specta::specta]
pub async fn api_fixture_run(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    id: String,
) -> Result<FixtureReport, String> {
    refuse_unless_offered()?;
    let root = crate::commands::autorun::root(&app)?;
    let mut browsers =
        crate::commands::autorun_replay::RealBrowsers::new(crate::browser::launch::Browser::Edge, false);
    let timing = crate::commands::autorun_replay::replay_timing(false);
    fixture_run::run_saved(&mut browsers, &root, &organization, &project, &id, &timing)
        .await
        .map_err(|e| e.to_string())
}

/// Removes a fixture and its run history - the person's, as removing a
/// template is. What it made stays in the record of test-made drafts.
#[tauri::command]
#[specta::specta]
pub fn api_fixture_remove(app: tauri::AppHandle, organization: String, project: String, id: String) -> Result<(), String> {
    let root = crate::commands::autorun::root(&app)?;
    remove_fixture_at(crate::ai_tools::autorun_offered(), &root, &organization, &project, &id)
}

/// `api_fixture_remove` for a given data root, with "is Auto Run offered
/// here" passed in, as `remove_flow_at` takes it.
pub fn remove_fixture_at(
    offered: bool,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    id: &str,
) -> Result<(), String> {
    refuse_unless(offered)?;
    fixture_store::remove(root, organization, project, id)?;
    crate::applog::info(format!("api fixture removed: {id}"));
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

#[tauri::command]
#[specta::specta]
pub fn api_templates_open_flow(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    id: String,
    palette: crate::webtheme::PagePalette,
) -> Result<(), String> {
    let root = crate::commands::autorun::root(&app)?;
    let path = write_flow_page_at(crate::ai_tools::autorun_offered(), &root, &organization, &project, &id, &palette)?;
    tauri_plugin_opener::open_path(&path, None::<&str>).map_err(|e| {
        crate::applog::warn(format!("the flow page could not be opened: {e}"));
        "the flow page could not be opened in your browser - see Settings, Logs".to_string()
    })
}

/// `api_templates_open_flow` up to the page on disk: the saved flow and
/// the templates on it, drawn on their own page (`flow_page`). "Is Auto Run
/// offered here" is passed in, as `remove_flow_at` takes it.
pub fn write_flow_page_at(
    offered: bool,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    id: &str,
    palette: &crate::webtheme::PagePalette,
) -> Result<std::path::PathBuf, String> {
    refuse_unless(offered)?;
    let flow = flow_store::load(root, organization, project, id)?
        .ok_or_else(|| format!("the flow {id} is no longer saved"))?;
    let templates = store::list(root, organization, project)?;
    crate::api_templates::flow_page::write(&flow, &templates, palette)
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

#[tauri::command]
#[specta::specta]
pub fn api_templates_export(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    path: String,
) -> Result<TemplatesExportResult, String> {
    let root = crate::commands::autorun::root(&app)?;
    export_at(crate::ai_tools::autorun_offered(), &root, &organization, &project, std::path::Path::new(&path))
}

#[tauri::command]
#[specta::specta]
pub fn api_templates_import(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    path: String,
) -> Result<TemplatesImportResult, String> {
    let root = crate::commands::autorun::root(&app)?;
    import_at(crate::ai_tools::autorun_offered(), &root, &organization, &project, std::path::Path::new(&path))
}

/// The name of the file a person picked, for a sentence or the log - never
/// the folder it is in.
fn file_name(path: &std::path::Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "that file".to_string())
}

/// How many `.json` files in `dir` a listing that found `listed` of them
/// left out because they did not read or parse (the listing logged each).
/// `runs` files are run history, never a template.
fn unreadable(dir: &std::path::Path, listed: usize) -> u32 {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    let files = entries
        .flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.ends_with(".json") && !name.ends_with(".runs.json")
        })
        .count();
    files.saturating_sub(listed) as u32
}

/// `api_templates_export` for a given data root: every saved template and
/// flow of the project, proof stripped (`share::build_doc`), written
/// atomically to `path`. Run history never goes in the file: `store::list`
/// loads each template's history beside it (a damaged history file is
/// logged there, as the tab's own listing logs it) and the export drops
/// it. A saved file that does not parse is left out and counted, as the
/// tab's listing leaves it out.
/// "Is Auto Run offered here" is passed in, as `remove_flow_at` takes it.
pub fn export_at(
    offered: bool,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    path: &std::path::Path,
) -> Result<TemplatesExportResult, String> {
    refuse_unless(offered)?;
    let templates = store::list(root, organization, project)?;
    let flows = flow_store::list(root, organization, project)?;
    let skipped = unreadable(&store::templates_dir(root, organization, project), templates.len())
        + unreadable(&flow_store::flows_dir(root, organization, project), flows.len());
    if templates.is_empty() && flows.is_empty() {
        return Err(share::NOTHING_TO_EXPORT.to_string());
    }
    let result = TemplatesExportResult { templates: templates.len() as u32, flows: flows.len() as u32, skipped };
    let doc = share::build_doc(
        templates.into_iter().map(|s| s.template).collect(),
        flows,
        &crate::run_order::now_rfc3339(),
    );
    let json = share::to_json(&doc)?;
    let name = file_name(path);
    crate::ai_tools::atomic_write(path, &json).map_err(|e| {
        crate::applog::warn(format!("api templates export to {name} could not be written: {e}"));
        format!("{name} could not be written - see Settings, Logs")
    })?;
    crate::applog::info(format!(
        "api templates exported to {name}: {} template(s), {} flow(s), {skipped} unreadable file(s) left out",
        result.templates, result.flows
    ));
    Ok(result)
}

/// `api_templates_import` for a given data root. The file is refused whole
/// only when it is not an export this app can read (`share::read_doc`);
/// past that, each entry stands alone - an invalid one is a skip line and
/// the rest still land. Flows are written before templates, so a
/// template's stage resolves against the flows just imported. A same-id
/// entry is written over the saved one - a template arrives unproven, a
/// flow without its sample - and a template's run history is left as it
/// is: it is a record of runs that happened here.
pub fn import_at(
    offered: bool,
    root: &std::path::Path,
    organization: &str,
    project: &str,
    path: &std::path::Path,
) -> Result<TemplatesImportResult, String> {
    refuse_unless(offered)?;
    let name = file_name(path);
    let unreadable_file = |e: std::io::Error| {
        crate::applog::warn(format!("api templates import from {name} could not be read: {e}"));
        format!("{name} could not be read - see Settings, Logs")
    };
    // The size is checked on what is read, not on what the file said it
    // was a moment before: at most one byte over the limit is ever read.
    let mut bytes = Vec::new();
    {
        use std::io::Read;
        let file = std::fs::File::open(path).map_err(&unreadable_file)?;
        file.take(share::MAX_FILE_BYTES + 1).read_to_end(&mut bytes).map_err(&unreadable_file)?;
    }
    if bytes.len() as u64 > share::MAX_FILE_BYTES {
        return Err(share::TOO_BIG.to_string());
    }
    let text = String::from_utf8(bytes).map_err(|_| share::NOT_JSON.to_string())?;
    let doc = share::read_doc(&text)?;
    let plan = share::plan_import(&doc);

    let mut result = TemplatesImportResult { skipped: plan.skipped, ..TemplatesImportResult::default() };
    let could_not_save = |what: &str, id: &str, e: String| {
        crate::applog::warn(format!("api templates import: {what} {id} could not be saved: {e}"));
        TemplatesImportSkip { id: id.to_string(), reason: format!("this {what} could not be saved - see Settings, Logs") }
    };
    // The names in this machine's Test files, for the note on a template
    // that uploads one this machine does not have. A folder that cannot be
    // read is logged and reads as empty: every file is then named.
    let have: Vec<String> = crate::test_files::list(&crate::test_files::folder(root, organization, project))
        .unwrap_or_default()
        .into_iter()
        .map(|f| f.name)
        .collect();
    let mut replaced_flows: Vec<&Flow> = Vec::new();
    for f in &plan.flows {
        // A saved copy that no longer reads still stands for that id.
        let existed = !matches!(flow_store::load(root, organization, project, &f.id), Ok(None));
        match flow_store::save(root, organization, project, f) {
            Ok(()) if existed => {
                result.replaced.push(share::flow_label(f));
                replaced_flows.push(f);
            }
            Ok(()) => result.added.push(share::flow_label(f)),
            Err(e) => result.skipped.push(could_not_save("flow", &f.id, e)),
        }
    }
    for t in &plan.templates {
        let existed = !matches!(store::load(root, organization, project, &t.id), Ok(None));
        match store::save(root, organization, project, t) {
            Ok(()) => {
                if existed {
                    result.replaced.push(t.title.clone());
                } else {
                    result.added.push(t.title.clone());
                }
                let loaded = match &t.stage {
                    None => Ok(None),
                    Some(r) => flow_store::load(root, organization, project, &r.flow).map_err(|e| {
                        crate::applog::warn(format!("api templates import: flow {} could not be read: {e}", r.flow));
                    }),
                };
                let found = match &loaded {
                    Ok(Some(f)) => share::FlowFound::Saved(f),
                    Ok(None) => share::FlowFound::Missing,
                    Err(()) => share::FlowFound::Unreadable,
                };
                if let Some(note) = share::stage_note(t, found) {
                    result.notes.push(TemplatesImportNote { id: t.id.clone(), title: t.title.clone(), note });
                }
                if let Some(note) = share::missing_files_note(t, &have) {
                    result.notes.push(TemplatesImportNote { id: t.id.clone(), title: t.title.clone(), note });
                }
            }
            Err(e) => result.skipped.push(could_not_save("template", &t.id, e)),
        }
    }
    // A replaced flow can leave templates saved here - not in this file -
    // performing a stage it no longer has, or no longer fits: each is named,
    // as the AI Bridge's own flow save names them.
    if !replaced_flows.is_empty() {
        let imported: std::collections::HashSet<&str> = plan.templates.iter().map(|t| t.id.as_str()).collect();
        let saved = store::list(root, organization, project).unwrap_or_else(|e| {
            crate::applog::warn(format!("api templates import: the saved templates could not be listed: {e}"));
            Vec::new()
        });
        for s in saved.iter().filter(|s| !imported.contains(s.template.id.as_str())) {
            let t = &s.template;
            let Some(f) = t.stage.as_ref().and_then(|r| replaced_flows.iter().find(|f| f.id == r.flow)) else {
                continue;
            };
            if let Some(why) = share::stage_note(t, share::FlowFound::Saved(f)) {
                result.notes.push(TemplatesImportNote {
                    id: t.id.clone(),
                    title: t.title.clone(),
                    note: share::replaced_flow_note(&f.id, &why),
                });
            }
        }
    }
    crate::applog::info(format!(
        "api templates imported from {name}: {} added, {} replaced, {} skipped, {} with a note",
        result.added.len(),
        result.replaced.len(),
        result.skipped.len(),
        result.notes.len()
    ));
    Ok(result)
}
