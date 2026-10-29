//! Find and replace across the open report and its subreports.
//!
//! Matching is the vendored library's own rule (`rpt_core::find`): a
//! substring, ASCII case-insensitive unless the caller asks for case. A
//! replace plans with the library, applies through the same path as an
//! edit (`apply_ops`: saved over the file before it answers), and with
//! `dry_run` lists every change and saves nothing. Text that holds fields
//! is found but never rewritten; fields are never replaced.

use rpt_core::edit::EditOp;
use rpt_core::ir::{Ir, ObjectIr};
use rpt_core::rpt_reader::Rpt;

use super::{apply_ops, state, NOTHING_OPEN};

/// Said when the text to find is empty.
pub const GIVE_TEXT: &str = "Give the text to find.";

/// One object that matched.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct FindHit {
    /// The subreport it is in; `None` for the main report.
    pub subreport: Option<String>,
    pub object_id: String,
    pub name: String,
    pub kind: String,
    pub section: String,
    /// "text" or "field".
    pub matched: String,
    /// The text's display, or the field's reference.
    pub text: String,
    /// Whether a replace can change it: a text object with no fields in it.
    pub replaceable: bool,
}

/// One change a replace makes (or, on a dry run, would make).
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct ReplaceChange {
    pub subreport: Option<String>,
    pub object_id: String,
    pub name: String,
    pub before: String,
    pub after: String,
}

/// What a replace did.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct ReplaceOutcome {
    /// Objects rewritten (or that would be, on a dry run).
    pub replaced: u32,
    /// Matching text objects left alone because their text holds fields.
    pub skipped_locked: u32,
    pub changes: Vec<ReplaceChange>,
    /// Whether anything was saved.
    pub applied: bool,
}

fn contains(haystack: &str, needle: &str, match_case: bool) -> bool {
    if match_case {
        haystack.contains(needle)
    } else {
        haystack.to_ascii_lowercase().contains(&needle.to_ascii_lowercase())
    }
}

fn hit_of(o: &ObjectIr, subreport: Option<&str>, text: &str, match_case: bool) -> Option<FindHit> {
    // A field object also carries its reference as read-only text, so it is judged as a field.
    let (matched, shown, replaceable) = match (&o.field, &o.text) {
        (Some(f), _) if contains(&f.data_source, text, match_case) => ("field", f.data_source.clone(), false),
        (Some(_), _) => return None,
        (None, Some(t)) if contains(&t.display, text, match_case) => ("text", t.display.clone(), t.editable),
        _ => return None,
    };
    Some(FindHit {
        subreport: subreport.map(str::to_string),
        object_id: o.id.clone(),
        name: o.name.clone(),
        kind: o.kind.clone(),
        section: o.section.clone(),
        matched: matched.to_string(),
        text: shown,
        replaceable,
    })
}

fn main_ir(rpt: &Rpt) -> Ir {
    rpt_core::build_ir(rpt, &rpt_core::locate::locate(rpt), "")
}

/// Every object of the open report and its subreports whose text, or whose
/// field reference, contains `text`. The main report comes first.
pub fn find(text: &str, match_case: bool) -> Result<Vec<FindHit>, String> {
    if text.is_empty() {
        return Err(GIVE_TEXT.to_string());
    }
    let slot = state();
    let open = slot.as_ref().ok_or(NOTHING_OPEN)?;
    let mut hits: Vec<FindHit> = main_ir(&open.rpt).objects.iter().filter_map(|o| hit_of(o, None, text, match_case)).collect();
    for sub in &open.rpt.report().subreports {
        // A subreport whose IR cannot be built is skipped, as the library's replace skips it.
        if let Ok(ir) = rpt_core::subedit::subreport_ir(&open.rpt, &sub.name) {
            hits.extend(ir.objects.iter().filter_map(|o| hit_of(o, Some(&sub.name), text, match_case)));
        }
    }
    Ok(hits)
}

fn change_of(ir: &Ir, subreport: Option<&str>, op: &EditOp) -> Option<ReplaceChange> {
    let EditOp::SetText { object, text } = op else { return None };
    let id = object.id.as_deref()?;
    let o = ir.objects.iter().find(|o| o.id == id)?;
    Some(ReplaceChange {
        subreport: subreport.map(str::to_string),
        object_id: o.id.clone(),
        name: o.name.clone(),
        before: o.text.as_ref().map(|t| t.display.clone()).unwrap_or_default(),
        after: text.clone(),
    })
}

/// Replace `find` with `replace` in every plain text object of the open
/// report and its subreports. With `dry_run` (or nothing to change) it only
/// lists; otherwise the changes are saved over the file like any edit.
pub fn replace(find: &str, replace: &str, match_case: bool, dry_run: bool) -> Result<ReplaceOutcome, String> {
    if find.is_empty() {
        return Err(GIVE_TEXT.to_string());
    }
    let mut slot = state();
    let open = slot.as_mut().ok_or(NOTHING_OPEN)?;
    let (ops, replaced, skipped) = rpt_core::find::plan_find_replace(&open.rpt, find, replace, !match_case)?;

    let main = main_ir(&open.rpt);
    let mut changes = Vec::new();
    for op in &ops {
        match op {
            EditOp::InSubreport { subreport, ops } => {
                if let Ok(ir) = rpt_core::subedit::subreport_ir(&open.rpt, subreport) {
                    changes.extend(ops.iter().filter_map(|op| change_of(&ir, Some(subreport), op)));
                }
            }
            op => changes.extend(change_of(&main, None, op)),
        }
    }
    let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    let mut outcome = ReplaceOutcome { replaced: count(replaced), skipped_locked: count(skipped), changes, applied: false };
    if dry_run || ops.is_empty() {
        return Ok(outcome);
    }
    apply_ops(open, &ops)?;
    outcome.applied = true;
    Ok(outcome)
}
