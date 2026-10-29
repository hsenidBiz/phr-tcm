//! The Crystal Reports screen's commands: what the open report looks like,
//! its last PDF, and the button that hands the file to Crystal Reports. The
//! assistant changes the report through the AI Bridge (`crystal`); the screen
//! only reads it, and tells the person when the bridge changed it
//! (`events::CrystalReportChanged`). Refused while the module is off.
//!
//! Each command waits on the open report's lock, which a slow render or a
//! live fetch can hold for a long time, so all three are `async` commands:
//! they run off the thread that draws the window.

use std::path::Path;

use base64::Engine;

use crate::crystal::{self, FindHit, PreviewInfo, ReplaceOutcome, ReportParameter};
use crate::modules::{self, Module};

/// Said when the open report's file has been moved or deleted since.
pub const FILE_GONE: &str = "The report file is no longer there.";

/// Said when Windows could not hand the .rpt to a program. The raw reason
/// goes to the log, not to the screen.
pub const NO_PROGRAM: &str =
    "Windows has no program set to open .rpt files. Install Crystal Reports, or open the file from it.";

const NOTHING_OPEN: &str = "No report is open.";

/// The report TCM has open, as the screen shows it.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct CurrentReport {
    /// The last render: its path, pages, notes and where its data came from.
    pub preview: PreviewInfo,
    /// Whether the file is gone from disk, so the screen says so instead of
    /// showing a preview of something that no longer exists.
    pub missing: bool,
}

fn offered(on: bool) -> Result<(), String> {
    modules::require(Module::CrystalReports, on)
}

/// The open report and its last render, if a report is open; `None` if not.
pub fn current_with(on: bool) -> Result<Option<CurrentReport>, String> {
    offered(on)?;
    let preview = match crystal::last_pdf() {
        Some((info, _)) => info,
        // Open, but never drawn: the screen says the preview could not be.
        None => match crystal::current_path() {
            Some(path) => PreviewInfo { path, page_count: 0, diagnostics: Vec::new(), live: None },
            None => return Ok(None),
        },
    };
    let missing = !Path::new(&preview.path).is_file();
    Ok(Some(CurrentReport { preview, missing }))
}

/// The last PDF as base64 with no data-URL prefix: empty when the render
/// failed, `None` when no report is open.
pub fn preview_pdf_with(on: bool) -> Result<Option<String>, String> {
    offered(on)?;
    Ok(crystal::last_pdf().map(|(_, pdf)| base64::engine::general_purpose::STANDARD.encode(pdf)))
}

/// Hand the open report's file to `opener` (the program Windows has for
/// .rpt). The file must still be there; any failure to open it is one
/// sentence, with the raw reason in the log.
pub fn open_in_designer_with(on: bool, opener: impl FnOnce(&str) -> Result<(), String>) -> Result<(), String> {
    offered(on)?;
    let path = crystal::current_path().ok_or(NOTHING_OPEN)?;
    if !Path::new(&path).is_file() {
        return Err(FILE_GONE.to_string());
    }
    opener(&path).map_err(|e| {
        crate::applog::warn(format!("Crystal Reports: the report could not be opened in its program: {e}"));
        NO_PROGRAM.to_string()
    })
}

/// Open the report the person picked, through the same `crystal::open` the
/// assistant's open_report uses, so both see one current report. A refusal
/// is one sentence naming the file, never its path; the raw sentence goes
/// to the log.
pub fn open_file_with(on: bool, path: &str) -> Result<(), String> {
    offered(on)?;
    crystal::open(path).map(|_| ()).map_err(|e| {
        crate::applog::info(format!("Crystal Reports: a picked report could not be opened: {e}"));
        e.replace(path, &file_name(path))
    })
}

/// One value the person typed for a parameter of the open report.
#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
pub struct ParameterValue {
    pub name: String,
    pub value: String,
}

/// The open report's parameters, for the panel.
pub fn parameters_with(on: bool) -> Result<Vec<ReportParameter>, String> {
    offered(on)?;
    crystal::parameters()
}

/// Draw the open report again with the values typed. A blank value is no
/// value. They are used for this drawing only and are never saved. The
/// refusal sentences name the file and the parameter, never a path.
pub fn draw_with(on: bool, values: Vec<ParameterValue>) -> Result<(), String> {
    offered(on)?;
    let params = values
        .into_iter()
        .filter(|v| !v.value.trim().is_empty())
        .map(|v| (v.name, v.value.trim().to_string()))
        .collect();
    crystal::preview_saved_with(params).map(|_| ())
}

/// Every object of the open report and its subreports that holds `text`.
pub fn find_with(on: bool, text: &str, match_case: bool) -> Result<Vec<FindHit>, String> {
    offered(on)?;
    crystal::find(text, match_case)
}

/// Replace text in the open report. With `dry_run` it only lists what would
/// change; otherwise the changes are saved over the file like any edit. A
/// refusal names the file, never its path.
pub fn replace_with(
    on: bool,
    find: &str,
    replace: &str,
    match_case: bool,
    dry_run: bool,
) -> Result<ReplaceOutcome, String> {
    offered(on)?;
    crystal::replace(find, replace, match_case, dry_run).map_err(|e| match crystal::current_path() {
        Some(path) => e.replace(&path, &file_name(&path)),
        None => e,
    })
}

fn file_name(path: &str) -> String {
    Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string())
}

/// Open a report the person picked; it becomes the open report.
#[tauri::command(async)]
#[specta::specta]
pub fn crystal_open_file(path: String) -> Result<(), String> {
    open_file_with(modules::on(Module::CrystalReports), &path)
}

/// Find text and field references in the open report and its subreports.
#[tauri::command(async)]
#[specta::specta]
pub fn crystal_find(text: String, match_case: bool) -> Result<Vec<FindHit>, String> {
    find_with(modules::on(Module::CrystalReports), &text, match_case)
}

/// Replace text in the open report; `dry_run` lists the changes and saves nothing.
#[tauri::command(async)]
#[specta::specta]
pub fn crystal_replace(
    find: String,
    replace: String,
    match_case: bool,
    dry_run: bool,
) -> Result<ReplaceOutcome, String> {
    replace_with(modules::on(Module::CrystalReports), &find, &replace, match_case, dry_run)
}

/// The open report's parameters.
#[tauri::command(async)]
#[specta::specta]
pub fn crystal_parameters() -> Result<Vec<ReportParameter>, String> {
    parameters_with(modules::on(Module::CrystalReports))
}

/// Draw the open report with the parameter values typed on the screen.
#[tauri::command(async)]
#[specta::specta]
pub fn crystal_draw(values: Vec<ParameterValue>) -> Result<(), String> {
    draw_with(modules::on(Module::CrystalReports), values)
}

/// The report TCM has open and how it last drew; `None` when none is open.
#[tauri::command(async)]
#[specta::specta]
pub fn crystal_current() -> Result<Option<CurrentReport>, String> {
    current_with(modules::on(Module::CrystalReports))
}

/// The open report's last PDF, base64 encoded with no data-URL prefix.
#[tauri::command(async)]
#[specta::specta]
pub fn crystal_preview_pdf() -> Result<Option<String>, String> {
    preview_pdf_with(modules::on(Module::CrystalReports))
}

/// Open the report's file in the program Windows has for .rpt files.
#[tauri::command(async)]
#[specta::specta]
pub fn crystal_open_in_designer() -> Result<(), String> {
    open_in_designer_with(modules::on(Module::CrystalReports), |path| {
        tauri_plugin_opener::open_path(path, None::<&str>).map_err(|e| e.to_string())
    })
}
