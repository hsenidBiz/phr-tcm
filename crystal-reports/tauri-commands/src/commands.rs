//! Tauri command handlers for the Crystal Reports editor backend — the portable half.
//!
//! Every function here is a plain function over `&EditorState`: this file compiles with the
//! default (empty) feature set, outside any Tauri app, which is what makes the handoff bundle
//! verifiable. The `tauri` feature adds `crate::glue` — one `#[tauri::command]` wrapper per
//! function, forwarding from `tauri::State` — and that is what a Tauri app registers.
//!
//! Data-flow contract (unchanged): `apply_edits`/`apply_plan` return the **refreshed IR**;
//! the UI replaces its state with it wholesale. A refused op batch rejects with a formatted
//! `op #N` message; ops before the refused one in a batch stand.

use rpt_core::edit::{apply_edits_on, ApplyOutcome, EditOp};
use rpt_core::ir::{build_ir, Ir};
use rpt_core::plan::{self, LayoutPlan};
use rpt_core::rpt_reader::Rpt;

/// The open report: the writer handle and its current IR snapshot.
///
/// Invariants: `rpt` is always the report as it stands *after* every accepted edit batch, and
/// `ir` is the projection of exactly those bytes — `get_ir` never re-derives anything.
#[derive(Default)]
pub struct EditorState {
    inner: std::sync::Mutex<Option<OpenReport>>,
    /// Where `save_report()` with no argument writes.
    path: std::sync::Mutex<Option<std::path::PathBuf>>,
}

struct OpenReport {
    rpt: Rpt,
    ir: Ir,
    bytes: Vec<u8>,
}

/// Error shape returned to the front end (Tauri renders `Err(String)` as a rejection).
type CmdResult<T> = Result<T, String>;

fn project(rpt: &Rpt, path: &str) -> Ir {
    let located = rpt_core::locate::locate(rpt);
    build_ir(rpt, &located, path)
}

impl EditorState {
    fn with_open<T>(&self, f: impl FnOnce(&OpenReport) -> CmdResult<T>) -> CmdResult<T> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| "editor state poisoned by a panicked command".to_string())?;
        match guard.as_ref() {
            Some(open) => f(open),
            None => Err("no report is open".to_string()),
        }
    }

    fn replace<F>(&self, f: F) -> CmdResult<Ir>
    where
        F: FnOnce(&OpenReport, &mut ApplyOutcome) -> Result<Vec<u8>, rpt_core::edit::OpError>,
    {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| "editor state poisoned by a panicked command".to_string())?;
        let open = guard.as_mut().ok_or_else(|| "no report is open".to_string())?;
        let bytes = f(open, &mut ApplyOutcome::default()).map_err(|e| e.to_string())?;
        let rpt = rpt_core::open_bytes(&bytes)?;
        let path = open.ir.file.clone();
        let ir = project(&rpt, &path);
        *open = OpenReport { rpt, ir: ir.clone(), bytes };
        Ok(ir)
    }
}

/// Create a blank `.rpt` from scratch (no donor file) with the given options; returns its
/// bytes. The created file is openable and immediately editable.
pub fn new_report(opts: rpt_core::blank::NewReportOptions) -> CmdResult<Vec<u8>> {
    rpt_core::blank::create_report(&opts)
}

/// Open a `.rpt` and return its IR. Fails (message) if the file is not a readable report.
pub fn open_report(state: &EditorState, path: String) -> CmdResult<Ir> {
    let rpt = rpt_core::open(&path)?;
    let ir = project(&rpt, &path);
    let mut slot = state
        .inner
        .lock()
        .map_err(|_| "editor state poisoned by a panicked command".to_string())?;
    *slot = Some(OpenReport { rpt, ir: ir.clone(), bytes: Vec::new() });
    *state
        .path
        .lock()
        .map_err(|_| "editor state poisoned by a panicked command".to_string())? = Some(std::path::PathBuf::from(&path));
    Ok(ir)
}

/// The current IR snapshot.
pub fn get_ir(state: &EditorState) -> CmdResult<Ir> {
    state.with_open(|open| Ok(open.ir.clone()))
}

/// Apply a batch of native edit ops; on success returns the refreshed IR (use its object ids
/// thereafter). A refused op aborts the batch with the refusal message.
pub fn apply_edits(state: &EditorState, ops: Vec<EditOp>) -> CmdResult<Ir> {
    state.replace(|open, outcome| apply_edits_on(&open.rpt, &ops, outcome))
}

/// Apply a VibeyReports LayoutPlan JSON (camelCase action names) to the open report.
pub fn apply_plan(state: &EditorState, plan_json: String) -> CmdResult<Ir> {
    let parsed: LayoutPlan =
        serde_json::from_str(&plan_json).map_err(|e| format!("bad plan JSON: {e}"))?;
    let ops = plan::lower(&parsed)?;
    state.replace(|open, outcome| apply_edits_on(&open.rpt, &ops, outcome))
}

/// Save the current report: to `path` when given, else over the file it was opened from.
pub fn save_report(state: &EditorState, path: Option<String>) -> CmdResult<()> {
    let bytes = save_bytes_of(state)?;
    let target = match path {
        Some(p) => std::path::PathBuf::from(p),
        None => state
            .path
            .lock()
            .map_err(|_| "editor state poisoned by a panicked command".to_string())?
            .clone()
            .ok_or_else(|| "no save path: open the report from a file or pass a path".to_string())?,
    };
    std::fs::write(&target, bytes).map_err(|e| format!("cannot write `{}`: {e}", target.display()))
}

/// The current `.rpt` bytes — for `dialog.save` + `fs.writeFile` flows on the front end.
pub fn save_as_bytes(state: &EditorState) -> CmdResult<Vec<u8>> {
    save_bytes_of(state)
}

/// Render the currently open report — including every accepted-but-unsaved edit — to a PDF
/// preview. Saved data drives the detail rows when the report carries it; a report saved
/// without data renders its static bands (headers/footers/unbound text) over an empty row set.
/// Pipeline fidelity warnings (an object kind the renderer does not draw, a formula that
/// failed evaluation) arrive in `diagnostics` rather than failing the render.
pub fn preview_report(state: &EditorState) -> CmdResult<rpt_core::PreviewOutput> {
    state.with_open(|open| rpt_core::preview::preview_report(&open.rpt))
}

/// [`preview_report`] with LIVE SQL Server data: the application passes its own ADO/OLE DB
/// connection string (parsed once, never logged, never echoed in an error) and any report
/// parameter values; the fetch runs the report's stored procedures fresh, so the preview
/// reflects the database as it stands — not the data saved into the file.
pub fn preview_report_live(
    state: &EditorState,
    db: rpt_core::preview::LiveDb,
) -> CmdResult<rpt_core::PreviewOutput> {
    state.with_open(|open| rpt_core::preview::preview_live_report(&open.rpt, &db))
}

fn save_bytes_of(state: &EditorState) -> CmdResult<Vec<u8>> {
    state.with_open(|open| {
        if open.bytes.is_empty() {
            rpt_core::reencode(&open.rpt)
        } else {
            Ok(open.bytes.clone())
        }
    })
}
