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
/// How many edit steps undo remembers (front is dropped beyond this).
const HISTORY_CAP: usize = 50;

#[derive(Default)]
pub struct EditorState {
    inner: std::sync::Mutex<Option<OpenReport>>,
    /// Where `save_report()` with no argument writes.
    path: std::sync::Mutex<Option<std::path::PathBuf>>,
    /// Byte snapshots BEFORE each accepted mutation, oldest first - the undo stack.
    undo: std::sync::Mutex<Vec<Vec<u8>>>,
    /// Bytes REDOable after undo(s), oldest first.
    redo: std::sync::Mutex<Vec<Vec<u8>>>,
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
        let before = if open.bytes.is_empty() {
            rpt_core::reencode(&open.rpt)?
        } else {
            open.bytes.clone()
        };
        let bytes = f(open, &mut ApplyOutcome::default()).map_err(|e| e.to_string())?;
        self.push_undo(before);
        let rpt = rpt_core::open_bytes(&bytes)?;
        let path = open.ir.file.clone();
        let ir = project(&rpt, &path);
        *open = OpenReport { rpt, ir: ir.clone(), bytes };
        Ok(ir)
    }
}

impl EditorState {
    fn push_undo(&self, bytes: Vec<u8>) {
        if let Ok(mut u) = self.undo.lock() {
            u.push(bytes);
            let over = u.len().saturating_sub(HISTORY_CAP);
            u.drain(..over);
        }
        if let Ok(mut r) = self.redo.lock() {
            r.clear();
        }
    }

    fn reset_history(&self) {
        if let Ok(mut u) = self.undo.lock() {
            u.clear();
        }
        if let Ok(mut r) = self.redo.lock() {
            r.clear();
        }
    }

    fn current_bytes(open: &OpenReport) -> CmdResult<Vec<u8>> {
        if open.bytes.is_empty() {
            rpt_core::reencode(&open.rpt)
        } else {
            Ok(open.bytes.clone())
        }
    }

    /// Swap the open report for `bytes` (projecting a fresh IR over the kept file label),
    /// returning the new IR. The caller manages the stacks.
    fn swap_to(&self, guard: &mut Option<OpenReport>, bytes: Vec<u8>) -> CmdResult<Ir> {
        let rpt = rpt_core::open_bytes(&bytes)?;
        let path = guard.as_ref().map(|o| o.ir.file.clone()).unwrap_or_default();
        let ir = project(&rpt, &path);
        *guard = Some(OpenReport { rpt, ir: ir.clone(), bytes });
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
    state.reset_history();
    Ok(ir)
}

/// Open a report held in memory (e.g. straight from [`new_report`]) instead of from a file -
/// the create-then-edit flow without temp files. There is no save path until one is given
/// to `save_report`.
pub fn open_report_bytes(state: &EditorState, bytes: Vec<u8>) -> CmdResult<Ir> {
    let rpt = rpt_core::open_bytes(&bytes)?;
    let ir = project(&rpt, "<memory>");
    let mut guard = state
        .inner
        .lock()
        .map_err(|_| "editor state poisoned by a panicked command".to_string())?;
    *guard = Some(OpenReport { rpt, ir: ir.clone(), bytes });
    if let Ok(mut p) = state.path.lock() {
        *p = None;
    }
    state.reset_history();
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

/// Step back one accepted edit: the report reverts to its bytes before that batch and the
/// refreshed IR returns. Redoable until the next new edit.
pub fn undo_report(state: &EditorState) -> CmdResult<Ir> {
    let prev = {
        let mut u = state
            .undo
            .lock()
            .map_err(|_| "editor state poisoned by a panicked command".to_string())?;
        u.pop().ok_or_else(|| "nothing to undo".to_string())?
    };
    let mut guard = state
        .inner
        .lock()
        .map_err(|_| "editor state poisoned by a panicked command".to_string())?;
    if let Some(open) = guard.as_ref() {
        let now = EditorState::current_bytes(open)?;
        if let Ok(mut r) = state.redo.lock() {
            r.push(now);
        }
    }
    state.swap_to(&mut guard, prev)
}

/// Re-apply the last undone edit; the refreshed IR returns.
pub fn redo_report(state: &EditorState) -> CmdResult<Ir> {
    let next = {
        let mut r = state
            .redo
            .lock()
            .map_err(|_| "editor state poisoned by a panicked command".to_string())?;
        r.pop().ok_or_else(|| "nothing to redo".to_string())?
    };
    let mut guard = state
        .inner
        .lock()
        .map_err(|_| "editor state poisoned by a panicked command".to_string())?;
    if let Some(open) = guard.as_ref() {
        let now = EditorState::current_bytes(open)?;
        if let Ok(mut u) = state.undo.lock() {
            u.push(now);
        }
    }
    state.swap_to(&mut guard, next)
}

/// The named embedded subreport's OWN IR — its objects, areas, and fields, scoped to the
/// subreport. Object ids in this IR are the ones subreport-scoped edits (the plan
/// vocabulary's `subreport` field) resolve against.
pub fn get_subreport_ir(state: &EditorState, name: String) -> CmdResult<Ir> {
    state.with_open(|open| rpt_core::subedit::subreport_ir(&open.rpt, &name))
}

/// Find & replace across every literal text in the report — the main report AND every
/// embedded subreport — as one atomic batch (undo treats it as a single step). Multi-run
/// (locked) text objects are skipped and counted, never mangled.
pub fn find_replace(
    state: &EditorState,
    find: String,
    replace: String,
    case_insensitive: bool,
) -> CmdResult<rpt_core::find::FindReplaceCounts> {
    let (ops, replaced, skipped_locked) = state.with_open(|open| {
        rpt_core::find::plan_find_replace(&open.rpt, &find, &replace, case_insensitive)
    })?;
    if !ops.is_empty() {
        state.replace(|open, outcome| apply_edits_on(&open.rpt, &ops, outcome))?;
    }
    Ok(rpt_core::find::FindReplaceCounts { replaced, skipped_locked })
}

/// The render-geometry bridge: the open report's rendered pages as selectable hit boxes
/// (one per object instance, true on-page rectangles in printable-relative twips) — the
/// overlay surface for click-to-select and drag-to-edit on the preview. Edits flow through
/// the ordinary op vocabulary.
pub fn preview_hits(state: &EditorState) -> CmdResult<rpt_core::hits::HitsOutput> {
    state.with_open(|open| rpt_core::hits::preview_hits(&open.rpt))
}

/// Resolve a printable-relative point on a rendered page (1-based) to the topmost object
/// hit box — the click-to-select lookup server-side (the front end may hit-test the
/// [`rpt_core::hits::HitsOutput`] geometry itself instead).
pub fn hit_test(
    state: &EditorState,
    page: usize,
    x: i32,
    y: i32,
) -> CmdResult<Option<rpt_core::hits::HitBox>> {
    state.with_open(|open| {
        let hits = rpt_core::hits::preview_hits(&open.rpt)?;
        Ok(rpt_core::hits::hit_test(&hits, page, x, y))
    })
}

/// "What changed": a JSON-path diff of two report byte-sets over their decoded models —
/// `$.path: old -> new` lines, empty when the models are identical. For a changes panel or
/// version compare; neither side needs to be the open report.
pub fn diff_reports(a: Vec<u8>, b: Vec<u8>) -> CmdResult<Vec<String>> {
    rpt_core::diff::diff_bytes(&a, &b)
}

/// The open report's declared parameters (name, kind, prompt text, pick lists) - the prompt
/// form for the preview paths. Main-report parameters only; subreport `Pm-` parameters are
/// bound by links, not prompted.
pub fn get_parameters(state: &EditorState) -> CmdResult<Vec<rpt_core::ParameterInfo>> {
    state.with_open(|open| Ok(rpt_core::report_parameters(open.rpt.report())))
}

/// "Export PDF": the preview render written with export options (document title, PDF/A
/// level-b archival conformance, optional structure-tree tagging).
pub fn export_pdf(
    state: &EditorState,
    opts: rpt_core::PdfExportOptions,
) -> CmdResult<rpt_core::PreviewOutput> {
    state.with_open(|open| rpt_core::preview::preview_report_with(&open.rpt, &opts))
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
    // Atomic: write a sibling temp file, then rename over the target - a crash mid-save
    // can never truncate the user's report (the rename replaces atomically).
    let tmp = target.with_file_name(format!(
        "{}.tmp",
        target.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
    ));
    std::fs::write(&tmp, bytes)
        .and_then(|()| std::fs::rename(&tmp, &target))
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("cannot write `{}`: {e}", target.display())
        })
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

/// A list-view thumbnail of the open report: page 1 only, as a one-page PDF — much smaller
/// than the full preview, for file-browser surfaces.
pub fn preview_thumbnail(state: &EditorState) -> CmdResult<rpt_core::PreviewOutput> {
    state.with_open(|open| rpt_core::preview::thumbnail_report(&open.rpt))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn blank_bytes() -> Vec<u8> {
        rpt_core::blank::create_report(&rpt_core::blank::NewReportOptions::default()).unwrap()
    }

    const ADD_TEXT: &str = r#"{"planVersion":1,"operations":[
        {"action":"addText","section":"ReportHeaderSection1","newName":"ScratchTitle",
         "leftTwips":500,"topTwips":200,"widthTwips":6000,"heightTwips":400,
         "text":"undo me"}]}"#;

    /// The create-then-edit flow without temp files: open from bytes, edit, undo back to
    /// the original IR, redo forward again — and undo's refusal at the stack's bottom.
    #[test]
    fn open_bytes_then_undo_redo() {
        let state = EditorState::default();
        let ir0 = open_report_bytes(&state, blank_bytes()).expect("open from bytes");
        assert_eq!(ir0.objects.len(), 0);

        let ir1 = apply_plan(&state, ADD_TEXT.to_string()).expect("edit");
        assert_eq!(ir1.objects.len(), 1);

        let back = undo_report(&state).expect("undo");
        assert_eq!(back.objects.len(), 0, "undone to the blank");
        let fwd = redo_report(&state).expect("redo");
        assert_eq!(fwd.objects.len(), 1, "redone to the edit");
        // A fresh mutation clears redo.
        undo_report(&state).expect("undo again");
        let _ = apply_plan(&state, ADD_TEXT.replace("undo me", "second")).expect("second edit");
        assert!(redo_report(&state).is_err(), "redo cleared by the new edit");
        // Stack bottom.
        for _ in 0..64 {
            let _ = undo_report(&state);
        }
        assert_eq!(undo_report(&state).unwrap_err(), "nothing to undo");
    }

    /// Opening anything resets the history.
    #[test]
    fn open_resets_history() {
        let state = EditorState::default();
        open_report_bytes(&state, blank_bytes()).unwrap();
        apply_plan(&state, ADD_TEXT.to_string()).unwrap();
        open_report_bytes(&state, blank_bytes()).unwrap();
        assert_eq!(undo_report(&state).unwrap_err(), "nothing to undo");
    }

    /// get_parameters over a stored-proc fixture: five `@`-named inputs, the cycle id Number.
    #[test]
    fn parameters_of_proc_fixture() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../rpt-core/tests/fixtures/PMSV10_GoalAlignCascade.rpt"
        );
        let state = EditorState::default();
        open_report(&state, path.to_string()).expect("open");
        let params = get_parameters(&state).expect("params");
        assert_eq!(params.len(), 5);
        assert_eq!(params[0].name, "@performance_cycle_id");
        assert_eq!(params[0].value_kind, "Number");
    }

    /// The atomic save leaves the target written and no `.tmp` sibling behind.
    #[test]
    fn atomic_save_leaves_no_temp() {
        let state = EditorState::default();
        open_report_bytes(&state, blank_bytes()).unwrap();
        let target = std::env::temp_dir().join("rpt_atomic_save_test.rpt");
        save_report(&state, Some(target.to_string_lossy().to_string())).expect("save");
        assert!(target.exists(), "the report file is written");
        let tmp = std::env::temp_dir().join("rpt_atomic_save_test.rpt.tmp");
        assert!(!tmp.exists(), "no temp sibling survives");
        // Round-trips as a report.
        let _ = rpt_core::open(&target.to_string_lossy()).expect("saved file opens");
        let _ = std::fs::remove_file(&target);
    }
}
