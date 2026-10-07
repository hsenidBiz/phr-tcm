//! Playwright export: what can be exported, where each area goes, and the
//! write into the clone. The clone folder comes from the app settings; the
//! area and account choices are kept beside the Auto Run files.

use crate::pw_export::export::{self, ExportResult, Preview};
use crate::pw_export::mapping::{self, ExportMap};
use crate::pw_export::test_case::CaseDoc;
use std::collections::BTreeMap;

fn clone_path() -> String {
    crate::app_settings::current().playwright_clone
}

/// Which of `case_ids` can be exported, and why not for the rest.
#[tauri::command]
#[specta::specta]
pub fn pw_export_preview(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    case_ids: Vec<i32>,
    modules: Vec<(i32, String)>,
) -> Result<Preview, String> {
    let root = super::autorun::root(&app)?;
    let modules: BTreeMap<i32, String> = modules.into_iter().collect();
    export::preview_with(&root, &organization, &project, &case_ids, &clone_path(), &modules)
}

/// Keeps the area placements and account choices. One bad placement refuses
/// the whole map and nothing is saved.
#[tauri::command]
#[specta::specta]
pub fn pw_export_save_map(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    map: ExportMap,
) -> Result<(), String> {
    for (area, placement) in &map.areas {
        placement.validate().map_err(|e| format!("Area \"{area}\": {e}"))?;
    }
    let root = super::autorun::root(&app)?;
    mapping::save(&root, &organization, &project, &map)
}

/// Writes the cases into the clone. Reads each case's title, steps, state
/// and paths from Azure DevOps first (read-only). `modules` is each case's
/// Module as the screen has it - the same the preview was given, so the
/// write picks the same areas the preview showed.
#[tauri::command]
#[specta::specta]
pub async fn pw_export_write(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    case_ids: Vec<i32>,
    modules: Vec<(i32, String)>,
    module_ref: Option<String>,
    preconditions_ref: Option<String>,
) -> Result<ExportResult, String> {
    if case_ids.is_empty() {
        return Ok(ExportResult { files: vec![], cases: vec![], user_keys: vec![], missing_navigation: vec![] });
    }
    let root = super::autorun::root(&app)?;
    let modules: BTreeMap<i32, String> = modules.into_iter().collect();
    // Refuse an unexportable selection before any request is made.
    export::ensure_exportable(&root, &organization, &project, &case_ids, &clone_path(), &modules)?;
    let token = crate::state::get_fresh_token(&app).await.map_err(|e| e.user_text())?;
    let client = crate::ado::AdoClient::new(token);
    let cases = client
        .get_test_cases_by_ids(&organization, &case_ids, module_ref.as_deref(), preconditions_ref.as_deref())
        .await
        .map_err(|e| e.user_text())?;
    let metas = client.get_case_meta(&organization, &case_ids).await.map_err(|e| e.user_text())?;
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
                project: project.clone(),
                module: c.module_value,
                tags: c.tags,
                preconditions: c.preconditions,
                steps: c.steps.into_iter().map(|s| (s.action, s.expected)).collect(),
                // Filled in by the export from the area's placement and the clone.
                side: String::new(),
                navigation_captured: false,
                feature_title: String::new(),
            },
        );
    }
    export::write_with(&root, &organization, &project, &case_ids, &clone_path(), &modules, &docs)
}
