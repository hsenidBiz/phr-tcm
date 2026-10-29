//! The Tauri half — one `#[tauri::command]` wrapper per portable function in `commands`.
//!
//! Only compiled with the `tauri` feature. Register these with
//! `.manage(EditorState::default())` and `generate_handler![…]`; see the folder README.

use tauri::State;

use crate::commands::{self, EditorState};

#[tauri::command]
pub fn new_report(opts: rpt_core::blank::NewReportOptions) -> Result<Vec<u8>, String> {
    commands::new_report(opts)
}

#[tauri::command]
pub fn open_report(state: State<EditorState>, path: String) -> Result<rpt_core::Ir, String> {
    commands::open_report(&state, path)
}

#[tauri::command]
pub fn get_ir(state: State<EditorState>) -> Result<rpt_core::Ir, String> {
    commands::get_ir(&state)
}

#[tauri::command]
pub fn apply_edits(state: State<EditorState>, ops: Vec<rpt_core::EditOp>) -> Result<rpt_core::Ir, String> {
    commands::apply_edits(&state, ops)
}

#[tauri::command]
pub fn apply_plan(state: State<EditorState>, plan_json: String) -> Result<rpt_core::Ir, String> {
    commands::apply_plan(&state, plan_json)
}

#[tauri::command]
pub fn save_report(state: State<EditorState>, path: Option<String>) -> Result<(), String> {
    commands::save_report(&state, path)
}

#[tauri::command]
pub fn save_as_bytes(state: State<EditorState>) -> Result<Vec<u8>, String> {
    commands::save_as_bytes(&state)
}

#[tauri::command]
pub fn preview_report(state: State<EditorState>) -> Result<rpt_core::PreviewOutput, String> {
    commands::preview_report(&state)
}

#[tauri::command]
pub fn preview_thumbnail(state: State<EditorState>) -> Result<rpt_core::PreviewOutput, String> {
    commands::preview_thumbnail(&state)
}

#[tauri::command]
pub fn open_report_bytes(state: State<EditorState>, bytes: Vec<u8>) -> Result<rpt_core::Ir, String> {
    commands::open_report_bytes(&state, bytes)
}

#[tauri::command]
pub fn undo_report(state: State<EditorState>) -> Result<rpt_core::Ir, String> {
    commands::undo_report(&state)
}

#[tauri::command]
pub fn redo_report(state: State<EditorState>) -> Result<rpt_core::Ir, String> {
    commands::redo_report(&state)
}

#[tauri::command]
pub fn find_replace(
    state: State<EditorState>,
    find: String,
    replace: String,
    case_insensitive: bool,
) -> Result<rpt_core::find::FindReplaceCounts, String> {
    commands::find_replace(&state, find, replace, case_insensitive)
}

#[tauri::command]
pub fn preview_hits(state: State<EditorState>) -> Result<rpt_core::hits::HitsOutput, String> {
    commands::preview_hits(&state)
}

#[tauri::command]
pub fn hit_test(
    state: State<EditorState>,
    page: usize,
    x: i32,
    y: i32,
) -> Result<Option<rpt_core::hits::HitBox>, String> {
    commands::hit_test(&state, page, x, y)
}

#[tauri::command]
pub fn diff_reports(a: Vec<u8>, b: Vec<u8>) -> Result<Vec<String>, String> {
    commands::diff_reports(a, b)
}

#[tauri::command]
pub fn get_subreport_ir(state: State<EditorState>, name: String) -> Result<rpt_core::Ir, String> {
    commands::get_subreport_ir(&state, name)
}

#[tauri::command]
pub fn get_parameters(state: State<EditorState>) -> Result<Vec<rpt_core::ParameterInfo>, String> {
    commands::get_parameters(&state)
}

#[tauri::command]
pub fn export_pdf(
    state: State<EditorState>,
    opts: rpt_core::PdfExportOptions,
) -> Result<rpt_core::PreviewOutput, String> {
    commands::export_pdf(&state, opts)
}

#[tauri::command]
pub fn preview_report_live(
    state: State<EditorState>,
    db: rpt_core::preview::LiveDb,
) -> Result<rpt_core::PreviewOutput, String> {
    commands::preview_report_live(&state, db)
}
