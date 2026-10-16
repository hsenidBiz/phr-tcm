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
pub fn preview_report_live(
    state: State<EditorState>,
    db: rpt_core::preview::LiveDb,
) -> Result<rpt_core::PreviewOutput, String> {
    commands::preview_report_live(&state, db)
}
