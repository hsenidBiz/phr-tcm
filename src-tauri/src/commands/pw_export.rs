//! Playwright export: what can be exported, where each area goes, and the
//! write into the clone. The clone folder comes from the app settings; the
//! area and account choices are kept beside the Auto Run files.

use crate::pw_export::export::{self, ExportResult, Preview};
use crate::pw_export::mapping::{self, ExportMap};
use crate::pw_export::test_case::CaseDoc;
use std::collections::BTreeMap;

/// Export to Playwright is the Playwright module's: refused while it is off.
fn ensure_offered() -> Result<(), String> {
    crate::pw_repo::ensure_offered(crate::modules::on(crate::modules::Module::Playwright))
}

/// The root of the active environment, refused unless it is the Playwright
/// export environment: what Export, Sync and Finish in the repo all start
/// from. One sentence names the export environment.
pub(crate) fn export_root(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let root = super::autorun::root(app)?;
    crate::environments::check_export_env(&root)?;
    Ok(root)
}

/// The export environment's own folder, whichever environment is active:
/// where the export records live. `None` when the environments cannot be
/// read (the rename then moves the repo's folder and no placements).
pub(crate) fn export_env_root(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    let active = super::autorun::root(app).ok()?;
    let export = crate::environments::export_env(&active).ok()?;
    Some(crate::autorun::roots::env_root_beside(&active, &export.id))
}

/// The Playwright export environment, as the Playwright tab shows it.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct PwExportEnv {
    pub id: String,
    pub name: String,
    /// The export environment is the active one: Export and Sync are open.
    pub is_active: bool,
}

fn export_env_view(active: &std::path::Path) -> Result<PwExportEnv, String> {
    let env = crate::environments::export_env(active)?;
    let is_active = crate::environments::active_id(active)? == env.id;
    Ok(PwExportEnv { id: env.id, name: env.name, is_active })
}

/// Which environment Export and Sync run in.
#[tauri::command(async)]
#[specta::specta]
pub fn pw_export_env(app: tauri::AppHandle) -> Result<PwExportEnv, String> {
    let _t = crate::pw_repo::timed("pw_export_env");
    ensure_offered()?;
    export_env_view(&super::autorun::root(&app)?)
}

/// Names the environment Export and Sync run in, moving the export records
/// (area placements and account choices) from the old one's folder to the
/// new one's. The only place the export environment changes.
#[tauri::command(async)]
#[specta::specta]
pub fn pw_set_export_env(app: tauri::AppHandle, id: String) -> Result<PwExportEnv, String> {
    let _t = crate::pw_repo::timed("pw_set_export_env");
    ensure_offered()?;
    let root = super::autorun::root(&app)?;
    crate::environments::set_export_env(&root, &id)?;
    export_env_view(&root)
}

fn clone_path() -> String {
    crate::app_settings::current().playwright_clone
}

/// Which of `case_ids` can be exported, and why not for the rest, and which
/// the repo already has (and whether this app exported them).
#[tauri::command(async)]
#[specta::specta]
pub fn pw_export_preview(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    case_ids: Vec<i32>,
    modules: Vec<(i32, String)>,
) -> Result<Preview, String> {
    let _t = crate::pw_repo::timed("pw_export_preview");
    ensure_offered()?;
    let root = export_root(&app)?;
    let dir = sync_dir(&app)?;
    let modules: BTreeMap<i32, String> = modules.into_iter().collect();
    export::preview_recorded(&root, &dir, &organization, &project, &case_ids, &clone_path(), &modules)
}

/// Keeps the area placements and account choices. One bad placement refuses
/// the whole map and nothing is saved.
#[tauri::command(async)]
#[specta::specta]
pub fn pw_export_save_map(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    map: ExportMap,
) -> Result<(), String> {
    let _t = crate::pw_repo::timed("pw_export_save_map");
    ensure_offered()?;
    for (area, placement) in &map.areas {
        placement.validate().map_err(|e| format!("Area \"{area}\": {e}"))?;
    }
    for (id, placement) in &map.cases {
        placement.validate().map_err(|e| format!("Case {id}: {e}"))?;
    }
    let root = export_root(&app)?;
    mapping::save(&root, &organization, &project, &map)
}

/// The folder of the Sync records (`pw_repo::sync`): beside Auto Run's
/// files in the app's data folder.
pub(crate) fn sync_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    use tauri::Manager;
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join(crate::pw_repo::sync::SYNC_DIR))
}

/// Each case's title, steps, state and paths, read from Azure DevOps
/// (read-only), as the test-case section is written from them.
pub(crate) async fn read_docs(
    app: &tauri::AppHandle,
    organization: &str,
    project: &str,
    case_ids: &[i32],
    module_ref: Option<&str>,
    preconditions_ref: Option<&str>,
) -> Result<BTreeMap<i32, CaseDoc>, String> {
    let token = crate::state::get_fresh_token(app).await.map_err(|e| e.user_text())?;
    let client = crate::ado::AdoClient::new(token);
    let cases = client
        .get_test_cases_by_ids(organization, case_ids, module_ref, preconditions_ref)
        .await
        .map_err(|e| e.user_text())?;
    let metas = client.get_case_meta(organization, case_ids).await.map_err(|e| e.user_text())?;
    let meta: BTreeMap<i32, _> = metas.into_iter().map(|m| (m.id, m)).collect();

    let mut docs: BTreeMap<i32, CaseDoc> = BTreeMap::new();
    for c in cases {
        let m = meta.get(&c.id);
        docs.insert(
            c.id,
            CaseDoc {
                id: c.id,
                title: c.title,
                state: m.map(|m| m.state.clone()).unwrap_or_default(),
                area_path: m.map(|m| m.area_path.clone()).unwrap_or_default(),
                iteration_path: m.map(|m| m.iteration_path.clone()).unwrap_or_default(),
                project: project.to_string(),
                module: c.module_value,
                tags: c.tags,
                preconditions: c.preconditions,
                steps: c.steps.into_iter().map(|s| (s.action, s.expected)).collect(),
                // Filled in by the export from the area's placement and the clone.
                folder: String::new(),
                navigation_captured: false,
                feature_title: String::new(),
                // Filled in by the export from the script.
                handover: Default::default(),
            },
        );
    }
    Ok(docs)
}

/// An export of nothing: what an empty selection answers.
pub(crate) fn nothing_written() -> ExportResult {
    export::nothing_written()
}

/// Writes the cases into the clone, and records them for Sync. Reads each
/// case's title, steps, state and paths from Azure DevOps first
/// (read-only). `modules` is each case's Module as the screen has it - the
/// same the preview was given, so the write picks the same areas the
/// preview showed. `replace` is the cases the repo already has that the
/// person ticked to replace on purpose: any other case the repo has refuses
/// the whole write. `pbi_id` is the PBI the cases are shown under: its saved
/// Auto Run order goes into each test-case file's order notes.
#[tauri::command]
#[specta::specta]
pub async fn pw_export_write(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    case_ids: Vec<i32>,
    modules: Vec<(i32, String)>,
    replace: Vec<i32>,
    module_ref: Option<String>,
    preconditions_ref: Option<String>,
    pbi_id: Option<i32>,
) -> Result<ExportResult, String> {
    let _t = crate::pw_repo::timed("pw_export_write");
    ensure_offered()?;
    let root = export_root(&app)?;
    if case_ids.is_empty() {
        return Ok(nothing_written());
    }
    let modules: BTreeMap<i32, String> = modules.into_iter().collect();
    // Refuse an unexportable selection before any request is made.
    export::ensure_exportable(&root, &organization, &project, &case_ids, &clone_path(), &modules, &replace)?;
    let docs = read_docs(&app, &organization, &project, &case_ids, module_ref.as_deref(), preconditions_ref.as_deref()).await?;
    let dir = sync_dir(&app)?;
    let clone = clone_path();
    let result = export::write_recorded(&root, &dir, &organization, &project, &case_ids, &clone, &modules, &docs, &replace, pbi_id)?;
    remember_written(&clone, &result);
    Ok(result)
}

/// Remembers, for this run, the cases `result` wrote to the clone at
/// `clone`, each with the user key its raw spec runs as: Finish in the
/// repo builds a prompt for them even when their Sync record could not be
/// saved.
pub(crate) fn remember_written(clone: &str, result: &ExportResult) {
    if result.cases.is_empty() {
        return;
    }
    let mut ids = written(clone);
    for (id, _) in &result.cases {
        let key = result.user_keys.iter().find(|(i, _)| i == id).map(|(_, k)| k.clone()).unwrap_or_default();
        ids.insert(*id, key);
    }
    crate::cache::session_put(&crate::cache::keys::pw_written(&crate::pw_repo::sync::clone_key(clone)), ids);
}

/// The cases exports wrote to the clone at `clone` this run, each with its user key.
pub(crate) fn written(clone: &str) -> BTreeMap<i32, String> {
    crate::cache::session_fresh(
        &crate::cache::keys::pw_written(&crate::pw_repo::sync::clone_key(clone)),
        crate::cache::keys::PW_WRITTEN_TTL,
    )
    .unwrap_or_default()
}
