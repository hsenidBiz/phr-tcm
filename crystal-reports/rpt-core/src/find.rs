//! Find & replace across every literal text in a report — the main report AND every
//! embedded subreport — in one batch. Each replacement is an ordinary `setText`, so the
//! whole operation is one atomic edit batch (any refusal rolls the report back to the
//! input bytes) and undo/redo treats it as a single step.
//!
//! Multi-run text objects (embedded field references) are skipped, not mangled: rewriting
//! their literal would drop the runs. They are counted and reported so a caller can say
//! "3 skipped (locked)" instead of silently missing hits.

use rpt_reader::Rpt;

use crate::edit::{apply_edits_on, EditOp, ObjSel};
use crate::ir::{build_ir, Ir};
use crate::locate;

/// The planner: returns the edit batch, the hit count, and the locked-skipped count.
pub fn find_replace_opts(
    rpt: &Rpt,
    find: &str,
    replace: &str,
    case_insensitive: bool,
) -> Result<(Vec<EditOp>, usize, usize), String> {
    if find.is_empty() {
        return Err("find is empty".to_string());
    }
    let mut ops: Vec<EditOp> = Vec::new();
    let mut replaced = 0usize;
    let mut skipped_locked = 0usize;

    let consider = |ir: &Ir, ops: &mut Vec<EditOp>| -> (usize, usize) {
        let (mut n, mut locked) = (0, 0);
        for o in &ir.objects {
            let Some(t) = &o.text else { continue };
            let hit = if case_insensitive {
                t.display.to_ascii_lowercase().contains(&find.to_ascii_lowercase())
            } else {
                t.display.contains(find)
            };
            if !hit {
                continue;
            }
            if !t.editable {
                locked += 1;
                continue;
            }
            let next = if case_insensitive {
                replace_ci(&t.display, find, replace)
            } else {
                t.display.replace(find, replace)
            };
            ops.push(EditOp::SetText {
                object: ObjSel { id: Some(o.id.clone()), name: None, index: None },
                text: next,
            });
            n += 1;
        }
        (n, locked)
    };

    let main_ir = build_ir(rpt, &locate::locate(rpt), "");
    let (n, l) = consider(&main_ir, &mut ops);
    replaced += n;
    skipped_locked += l;

    // Every subreport, through the lift: one InSubreport batch per sub that has hits.
    for sub in &rpt.report().subreports {
        if let Ok(ir) = crate::subedit::subreport_ir(rpt, &sub.name) {
            let mut sub_ops = Vec::new();
            let (n, l) = consider(&ir, &mut sub_ops);
            replaced += n;
            skipped_locked += l;
            if !sub_ops.is_empty() {
                ops.push(EditOp::InSubreport { subreport: sub.name.clone(), ops: sub_ops });
            }
        }
    }

    Ok((ops, replaced, skipped_locked))
}

/// [`find_replace_opts`]'s planning half: the edit batch (main + per-subreport), the hit
/// count, and the locked-skipped count — for callers that apply through their own state
/// layer (the Tauri command does, so the result participates in undo as one step).
pub fn plan_find_replace(
    rpt: &Rpt,
    find: &str,
    replace: &str,
    case_insensitive: bool,
) -> Result<(Vec<EditOp>, usize, usize), String> {
    find_replace_opts(rpt, find, replace, case_insensitive)
}

/// The applying half over a plan.
///
/// # Errors
///
/// A writer/verification failure (the input is then unchanged).
pub fn find_replace(rpt: &Rpt, find: &str, replace: &str) -> Result<FindReplaceOutcome, String> {
    let (ops, replaced, skipped_locked) = find_replace_opts(rpt, find, replace, false)?;
    let bytes = if ops.is_empty() {
        rpt.reencode().map_err(|e| format!("reencode: {e}"))?
    } else {
        let mut outcome = crate::edit::ApplyOutcome::default();
        apply_edits_on(rpt, &ops, &mut outcome).map_err(|e| e.to_string())?
    };
    Ok(FindReplaceOutcome { bytes, replaced, skipped_locked })
}

/// Case-insensitive replace: ASCII-lowercasing is byte-for-byte 1:1, so matches located
/// in the lowercased text map straight onto the original's byte offsets.
fn replace_ci(text: &str, find: &str, replace: &str) -> String {
    let lower_text = text.to_ascii_lowercase();
    let lower_find = find.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut rest_start = 0usize;
    while let Some(rel) = lower_text[rest_start..].find(&lower_find) {
        let at = rest_start + rel;
        out.push_str(&text[rest_start..at]);
        out.push_str(replace);
        rest_start = at + find.len();
    }
    out.push_str(&text[rest_start..]);
    out
}

/// The counts a find & replace reports (the serializable half; the bytes live in the
/// caller's state layer).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FindReplaceCounts {
    /// How many objects were rewritten.
    pub replaced: usize,
    /// Matching objects skipped (multi-run text — locked).
    pub skipped_locked: usize,
}

/// The outcome of a find & replace.
pub struct FindReplaceOutcome {
    /// The new file bytes (the input unchanged when nothing hit).
    pub bytes: Vec<u8>,
    /// How many objects were rewritten.
    pub replaced: usize,
    /// How many matching objects were skipped because their text is multi-run (locked).
    pub skipped_locked: usize,
}
