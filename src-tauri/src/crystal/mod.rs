//! The Crystal Reports module's open report: one `.rpt` at a time, read,
//! edited, checked and rendered through the vendored bundle
//! (`crystal-reports/`, `rpt_core`).
//!
//! TCM holds one open report - its path, the bundle's decoded report and
//! the IR (`crpt-ir/1`) built from it - and opening another replaces it.
//! An edit the bundle accepts is saved over the opened file before the
//! call answers, so the file, the IR and the preview always agree; git is
//! the undo (owner, 2026-10-16). A batch the bundle refuses saves nothing.
//!
//! Every answer that fails is a plain sentence naming the file, for the
//! assistant to read out or act on. A render from fresh database rows is
//! `live`'s, and is kept here exactly like a render from the saved data.

mod compare;
mod guide;
mod live;

pub use live::{live_target, live_target_of, preview_live, preview_live_with};

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rpt_core::edit::{apply_edits_on, ApplyOutcome, EditOp};
use rpt_core::plan::LayoutPlan;
use rpt_core::rpt_reader::Rpt;
use serde_json::Value;

/// The open report, if any.
static OPEN: Mutex<Option<Open>> = Mutex::new(None);

struct Open {
    /// The path it was opened from, as given; edits save over it.
    path: String,
    rpt: Rpt,
    /// The IR of `rpt`, as JSON.
    ir: Value,
    /// The last render and its PDF. An empty PDF means the render failed;
    /// its diagnostics say why.
    preview: Option<(PreviewInfo, Vec<u8>)>,
}

/// What a render of the open report produced, without the PDF itself.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct PreviewInfo {
    /// The report's path.
    pub path: String,
    /// Pages in the PDF; 0 when the render failed. (A `u32`, not the
    /// bundle's `usize`: specta refuses 64-bit numbers across IPC.)
    pub page_count: u32,
    /// The renderer's notes - an object it does not draw, a formula that
    /// failed - or why the render failed.
    pub diagnostics: Vec<String>,
    /// The connection a live-data render used, by name; `None` for a
    /// render from the data saved in the report.
    pub live: Option<String>,
}

/// Which part of the open report's IR `read` answers with. Both empty
/// means all of it.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct ReadFilter {
    /// Only this section (by name): its entry and its objects.
    pub section: Option<String>,
    /// Only this object, by IR id or designer name.
    pub object: Option<String>,
}

const NOTHING_OPEN: &str = "No report is open. Open one with open_report first.";

fn state() -> MutexGuard<'static, Option<Open>> {
    // A panic mid-call leaves the slot as it was before that call wrote it,
    // which is still a report that matches its file.
    OPEN.lock().unwrap_or_else(|e| e.into_inner())
}

/// The file name in `path`, for sentences; the whole path when it has none.
fn file_name(path: &str) -> String {
    Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string())
}

fn is_rpt(path: &str) -> bool {
    Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("rpt"))
}

/// Decode the report at `path`, refusing anything that is not a readable `.rpt`.
fn load(path: &str) -> Result<Rpt, String> {
    let name = file_name(path);
    if !is_rpt(path) {
        return Err(format!("{name} is not a .rpt file. The Crystal Reports tools work on .rpt files only."));
    }
    if !Path::new(path).is_file() {
        return Err(format!("There is no file at {path}. Check the path; it must be the full path to {name}."));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("Could not read {name}: {e}."))?;
    rpt_core::open_bytes(&bytes).map_err(|e| format!("{name} could not be opened as a Crystal Report: {e}."))
}

/// The IR of `rpt`, as JSON, labelled with `path`.
fn ir_of(rpt: &Rpt, path: &str) -> Result<Value, String> {
    let ir = rpt_core::build_ir(rpt, &rpt_core::locate::locate(rpt), path);
    serde_json::to_value(&ir).map_err(|e| format!("The IR of {} could not be built: {e}.", file_name(path)))
}

/// Render `open` from its saved data and keep the result. A failure - or
/// a panic inside the renderer - is kept too, as the stored preview's
/// diagnostic, so the screen shows why instead of an older render.
fn render(open: &mut Open) -> Result<PreviewInfo, String> {
    let rendered =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| rpt_core::preview::preview_report(&open.rpt)))
            .unwrap_or_else(|_| Err("the renderer stopped unexpectedly".to_string()));
    match rendered {
        Ok(out) => {
            let info = PreviewInfo {
                path: open.path.clone(),
                page_count: u32::try_from(out.page_count).unwrap_or(u32::MAX),
                diagnostics: out.diagnostics,
                live: None,
            };
            open.preview = Some((info.clone(), out.pdf));
            Ok(info)
        }
        Err(e) => {
            let why = format!("The preview of {} could not be drawn: {e}.", file_name(&open.path));
            let info =
                PreviewInfo { path: open.path.clone(), page_count: 0, diagnostics: vec![why.clone()], live: None };
            open.preview = Some((info, Vec::new()));
            Err(why)
        }
    }
}

/// Make `rpt` the open report and render it; answers with its IR.
fn install(path: &str, rpt: Rpt) -> Result<Value, String> {
    let ir = ir_of(&rpt, path)?;
    let mut open = Open { path: path.to_string(), rpt, ir: ir.clone(), preview: None };
    // The render's failure is in the stored preview; it never fails the open.
    let _ = render(&mut open);
    *state() = Some(open);
    Ok(ir)
}

/// Open the report at `path` (replacing any open one); answers with its IR.
pub fn open(path: &str) -> Result<Value, String> {
    let rpt = load(path)?;
    install(path, rpt)
}

/// The open report's IR, whole or narrowed by `filter`.
pub fn read(filter: &ReadFilter) -> Result<Value, String> {
    let slot = state();
    let open = slot.as_ref().ok_or(NOTHING_OPEN)?;
    narrow(&open.ir, filter, &file_name(&open.path))
}

fn same_name(value: &Value, name: &str) -> bool {
    value.as_str().is_some_and(|v| v.eq_ignore_ascii_case(name))
}

/// `ir` cut down to one section and/or one object: `format`, `file`, the
/// areas and sections that hold what was picked (each section's
/// `object_ids` cut to match), and the picked objects.
fn narrow(ir: &Value, filter: &ReadFilter, name: &str) -> Result<Value, String> {
    if filter.section.is_none() && filter.object.is_none() {
        return Ok(ir.clone());
    }
    let empty = Vec::new();
    let areas = ir["areas"].as_array().unwrap_or(&empty);
    let mut picked: Vec<&Value> = ir["objects"].as_array().unwrap_or(&empty).iter().collect();

    if let Some(section) = &filter.section {
        let sections: Vec<&Value> = areas.iter().flat_map(|a| a["sections"].as_array().unwrap_or(&empty)).collect();
        let found: Vec<&&Value> = sections.iter().filter(|s| same_name(&s["name"], section)).collect();
        if found.is_empty() {
            let names: Vec<&str> = sections.iter().filter_map(|s| s["name"].as_str()).collect();
            return Err(format!("{name} has no section named \"{section}\". Its sections are: {}.", names.join(", ")));
        }
        let ids: Vec<&Value> = found.iter().flat_map(|s| s["object_ids"].as_array().unwrap_or(&empty).iter()).collect();
        picked.retain(|o| ids.contains(&&o["id"]));
    }

    if let Some(object) = &filter.object {
        let by_id: Vec<&Value> = picked.iter().copied().filter(|o| o["id"] == object.as_str()).collect();
        let matches = if by_id.is_empty() {
            picked.iter().copied().filter(|o| same_name(&o["name"], object)).collect()
        } else {
            by_id
        };
        let place = filter.section.as_ref().map(|s| format!(" in section \"{s}\"")).unwrap_or_default();
        match matches.len() {
            0 => return Err(format!("{name} has no object with the id or name \"{object}\"{place}.")),
            1 => {}
            _ => {
                let ids: Vec<&str> = matches.iter().filter_map(|o| o["id"].as_str()).collect();
                return Err(format!(
                    "More than one object in {name} is named \"{object}\" ({}). Read one by its id instead.",
                    ids.join(", ")
                ));
            }
        }
        picked = matches;
    }

    let ids: Vec<&Value> = picked.iter().map(|o| &o["id"]).collect();
    let keep_section = |s: &Value| match &filter.section {
        Some(section) if filter.object.is_none() => same_name(&s["name"], section),
        _ => s["object_ids"].as_array().is_some_and(|o| o.iter().any(|id| ids.contains(&id))),
    };
    let mut kept_areas = Vec::new();
    for area in areas {
        let sections: Vec<Value> = area["sections"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .filter(|s| keep_section(s))
            .map(|s| {
                let mut s = s.clone();
                if let Some(list) = s["object_ids"].as_array_mut() {
                    list.retain(|id| ids.contains(&id));
                }
                s
            })
            .collect();
        if !sections.is_empty() {
            let mut area = area.clone();
            area["sections"] = Value::Array(sections);
            kept_areas.push(area);
        }
    }
    Ok(serde_json::json!({
        "format": ir["format"],
        "file": ir["file"],
        "areas": kept_areas,
        "objects": picked,
    }))
}

/// Apply one batch to the open report - native `ops` or a layout `plan`,
/// exactly one of the two - and save it over the file. Answers with the
/// new IR. A batch the bundle refuses saves nothing: the report is read
/// again from disk and the answer is the refusal.
pub fn edit(ops: Option<Value>, plan: Option<Value>) -> Result<Value, String> {
    let ops: Vec<EditOp> = match (ops, plan) {
        (Some(ops), None) => {
            serde_json::from_value(ops).map_err(|e| format!("The ops could not be read: {e}. Nothing was saved."))?
        }
        (None, Some(plan)) => {
            let plan: LayoutPlan = serde_json::from_value(plan)
                .map_err(|e| format!("The plan could not be read: {e}. Nothing was saved."))?;
            rpt_core::plan::lower(&plan).map_err(|e| format!("The plan was refused: {e}. Nothing was saved."))?
        }
        (Some(_), Some(_)) => return Err("Give either ops or a plan, not both.".to_string()),
        (None, None) => {
            return Err("Give the changes as ops (native edit ops) or as a plan (a layout plan).".to_string())
        }
    };
    if ops.is_empty() {
        return Err("The batch is empty: there is nothing to change.".to_string());
    }

    let mut slot = state();
    let open = slot.as_mut().ok_or(NOTHING_OPEN)?;
    let bytes = match apply_edits_on(&open.rpt, &ops, &mut ApplyOutcome::default()) {
        Ok(bytes) => bytes,
        Err(refusal) => {
            // The batch's bytes are dropped, so the open report never held any of
            // it; reading the file again also picks up a change made outside TCM.
            if let Ok(rpt) = load(&open.path) {
                if let Ok(ir) = ir_of(&rpt, &open.path) {
                    open.rpt = rpt;
                    open.ir = ir;
                    let _ = render(open);
                }
            }
            return Err(format!("{refusal}\n\nThe report was read again from disk: nothing in this batch was saved."));
        }
    };
    // Decode before saving, so bytes that would not open again never reach the file.
    let name = file_name(&open.path);
    let rpt = rpt_core::open_bytes(&bytes)
        .map_err(|e| format!("The edited {name} could not be read back ({e}), so it was not saved."))?;
    let ir = ir_of(&rpt, &open.path)?;
    save_over(&open.path, &bytes)?;
    open.rpt = rpt;
    open.ir = ir.clone();
    let _ = render(open);
    Ok(ir)
}

/// Write `bytes` over `path` whole or not at all: into a file beside it,
/// then renamed over it, so a failed write never leaves half a report.
fn save_over(path: &str, bytes: &[u8]) -> Result<(), String> {
    let target = Path::new(path);
    let name = file_name(path);
    let dir = target.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let temp = dir.join(format!(".{name}.{}.saving", std::process::id()));
    let written = std::fs::write(&temp, bytes).and_then(|()| std::fs::rename(&temp, target));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&temp);
        return Err(format!(
            "Could not save {name}: {e}. If it is read-only or open in another program (Crystal Reports keeps \
             a report it has open locked), clear Read-only or close it and try again. Nothing was saved, and \
             the open report is as it was before this batch."
        ));
    }
    Ok(())
}

/// Create a blank report at `path` and open it. Refuses a path that
/// already has a file: this never writes over anything.
pub fn new_report(path: &str, opts: rpt_core::blank::NewReportOptions) -> Result<Value, String> {
    let name = file_name(path);
    if !is_rpt(path) {
        return Err(format!("{name} is not a .rpt file name. A new report's path must end in .rpt."));
    }
    let exists =
        format!("{name} already exists. new_report never writes over a file: choose another path, or open it.");
    if Path::new(path).exists() {
        return Err(exists);
    }
    let bytes = rpt_core::blank::create_report(&opts).map_err(|e| format!("Could not create {name}: {e}."))?;
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            exists.clone()
        } else {
            format!("Could not create {name}: {e}.")
        }
    })?;
    std::io::Write::write_all(&mut file, &bytes).map_err(|e| format!("Could not write {name}: {e}."))?;
    drop(file);
    open(path)
}

/// The bundle's offline load checks on the report at `path`, or on the
/// open report when there is none: `{ "errors": [{check, message}],
/// "warnings": [...] }`.
pub fn check(path: Option<&str>) -> Result<Value, String> {
    let report = match path {
        Some(path) => rpt_core::validate::validate(&load(path)?),
        None => rpt_core::validate::validate(&state().as_ref().ok_or(NOTHING_OPEN)?.rpt),
    };
    let list = |findings: Vec<&rpt_core::validate::Finding>| -> Value {
        findings.iter().map(|f| serde_json::json!({ "check": f.check, "message": f.message })).collect()
    };
    Ok(serde_json::json!({
        "errors": list(report.errors().collect()),
        "warnings": list(report.warnings().collect()),
    }))
}

/// Every difference between the reports at `a` and `b`, one
/// `path: a -> b` line each; empty when they match.
pub fn compare(a: &str, b: &str) -> Result<Vec<String>, String> {
    let model = |path: &str| -> Result<Value, String> {
        serde_json::to_value(load(path)?.report())
            .map_err(|e| format!("{} could not be read for comparing: {e}.", file_name(path)))
    };
    Ok(compare::diff(&model(a)?, &model(b)?))
}

/// Render the open report from the data saved in it, and keep the PDF for
/// the screen.
pub fn preview_saved() -> Result<PreviewInfo, String> {
    let mut slot = state();
    render(slot.as_mut().ok_or(NOTHING_OPEN)?)
}

/// The open report's last render and its PDF (empty when that render
/// failed), if a report is open.
pub fn last_pdf() -> Option<(PreviewInfo, Vec<u8>)> {
    state().as_ref().and_then(|open| open.preview.clone())
}

/// The open report's path.
pub fn current_path() -> Option<String> {
    state().as_ref().map(|open| open.path.clone())
}

/// Forget the open report. The file is not touched.
pub fn close() {
    *state() = None;
}

/// How to work with reports through the tools, for `get_report_guide`.
pub fn guide() -> &'static str {
    guide::GUIDE
}
