//! The JSON export: the round-trip document an assistant edits and the app
//! re-imports. JSON is the only interchange format - the spreadsheet
//! readers and writers this module was ported from were never reachable
//! from the v2 UI and have been removed.

use super::{
    alias_in_use, AREA_KEYS, COMMENT_KEYS, ID_KEYS, MODULE_KEYS, PRECONDITIONS_KEYS,
    REVIEWER_NOTES_KEYS, SPEC_ORDER_KEYS, STEP_ACTION_KEYS, STEP_EXPECTED_KEYS, TESTER_ORDER_KEYS,
    TITLE_KEYS,
};
use crate::model::{DraftEdit, SourceIndex, TestCase};
use crate::steps_xml::Step;
use serde_json::{Map, Value};

const AI_FORMAT_NAME: &str = "azure-devops-test-cases";
const AI_FORMAT_VERSION: u32 = 1;
const AI_INSTRUCTIONS: &str = "Each entry in test_cases is one Azure DevOps Test Case. Edit this file \
freely but keep it valid JSON with this exact structure. Rules: keep \
'id' unchanged so re-importing UPDATES that existing work item; set 'id' \
to null to CREATE a new test case. 'title' is required (max 255 chars). \
'steps' is an ordered list; every step needs a non-empty 'action', \
'expected' may be an empty string - except a Shared Steps reference, written \
{\"shared\": N}, which has neither: keep it exactly as exported and never \
invent one. 'automation_status' must be exactly \
'Not Automated' or 'Planned'. 'tags' is a single semicolon-separated \
string - commas are not allowed in tags. 'module' and 'preconditions' \
are free text and may be empty strings. Two optional fields never reach \
Azure DevOps and exist only in this file: 'comment', a short in-app note, \
and 'reviewer_notes': one or two plain sentences saying what THIS case \
checks, in words anyone would understand, then where the requirement \
lives (the spec section or the code symbol). Add an 'Out of scope:' line \
only if this case deliberately leaves something out. Leave out where the \
cases came from as a body of work and the scope of the whole set - both \
were agreed once and repeating them per case is noise on every case. 'reviewer_notes' is rendered as MARKDOWN \
when the cases are opened in a browser, so a link to the spec works. A third file-only field, 'findings', \
is a list of problems found while writing THIS case: each entry has 'kind' (test_case, spec or code), an \
optional 'subject' (the spec section or code symbol), a one-line 'title' and an optional markdown 'detail'. \
Put a contradiction between the spec and the code here, never in 'comment' (the developer's field) and \
never in 'reviewer_notes'. An optional 'area' says where the case sits on the page or in the feature, as a \
path with '/' between the levels ('Manage Events / Create / Validation'). This is the app's own grouping path, not the work item's Area Path; it draws \
the app's Test map and never reaches Azure DevOps. A top-level 'specs' list (beside 'test_cases') names the specification documents these cases were written from: \
file paths (absolute, or relative to this file) or Azure DevOps wiki page URLs, as strings. The app shows them beside \
the cases in the browser; fill it from the documents named at intake.";

/// One step as the case file writes it. A Shared Steps reference is only
/// `{"shared": N}`: its steps live in that work item and are edited there.
pub fn step_json(s: &crate::steps_xml::Step) -> serde_json::Value {
    match s.shared {
        Some(id) => serde_json::json!({ "shared": id }),
        None => serde_json::json!({ "action": s.action, "expected": s.expected }),
    }
}

/// One case as the exporter writes it.
fn case_record(tc: &TestCase) -> Value {
    let mut rec = serde_json::json!({
        "title": tc.title,
        "tags": tc.tags,
        "automation_status": tc.automation_status,
        "module": tc.module_value,
        "preconditions": tc.preconditions,
        "steps": tc.steps.iter().map(step_json).collect::<Vec<_>>(),
    });
    // An id only appears when there IS one. `id: null` and no `id` both
    // mean "create", so emitting the null said nothing - but a caller who
    // passed 14 cases with no id got 14 back carrying a key they never
    // wrote, which makes confirming that a bulk transform did only what it
    // claimed mean reading past a shape change on every case. Same rule as
    // the two notes below; it was only ever applied to them.
    if let Some(id) = tc.update_id {
        rec["id"] = serde_json::json!(id);
    }
    // In-app note: round-trips through this file, never sent to ADO - and
    // only present when non-empty, so a tool that round-trips a caller's
    // draft does not inject a field the caller never wrote (a transform
    // must be idempotent in shape).
    if !tc.comment.is_empty() {
        rec["comment"] = serde_json::json!(tc.comment);
    }
    if !tc.reviewer_notes.is_empty() {
        rec["reviewer_notes"] = serde_json::json!(tc.reviewer_notes);
    }
    // The area path, present only when set - same shape rule.
    if !tc.area.is_empty() {
        rec["area"] = serde_json::json!(tc.area);
    }
    // The two sort orders, present only when known - same shape rule as
    // the notes above.
    if let Some(n) = tc.spec_order {
        rec["spec_order"] = serde_json::json!(n);
    }
    if let Some(n) = tc.tester_order {
        rec["tester_order"] = serde_json::json!(n);
    }
    if !tc.findings.is_empty() {
        rec["findings"] = serde_json::json!(tc.findings);
    }
    rec
}

pub fn queue_to_json_string(queue: &[TestCase]) -> Result<String, String> {
    let records: Vec<Value> = queue.iter().map(case_record).collect();
    let doc = serde_json::json!({
        "format": AI_FORMAT_NAME,
        "version": AI_FORMAT_VERSION,
        "instructions": AI_INSTRUCTIONS,
        "test_cases": records,
    });
    let mut text = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
    text.push('\n');
    Ok(text)
}

/// Export the queue in the v1 AI round-trip JSON format
/// (export_formats.export_records_to_json) - re-importable via parse_file.
pub fn export_queue_to_json(queue: &[TestCase], path: &str) -> Result<(), String> {
    std::fs::write(path, queue_to_json_string(queue)?).map_err(|e| e.to_string())
}

/// Write `cases` back into the draft `old_text` by PATCHING it, not by
/// regenerating it. A tool owns the cases, never the file.
///
/// - A case whose `source` points at an entry the importer read has that
///   entry patched in place: only the fields that differ from what the
///   importer read are written, each under the alias key the entry already
///   uses. Unknown keys, untouched fields and their spellings stay.
/// - A case with no source (inserted, or not found in this file) is written
///   fresh, at its position in `cases`.
/// - An entry the importer read that no case points at was removed by the
///   operation, and it goes.
/// - An entry the importer SKIPPED (no title, no step with an action, not
///   an object) is kept verbatim, right after the entry that preceded it in
///   the file, or at the front if none did.
///
/// Every other top-level key is left exactly as written. A bare array comes
/// back inside the standard wrapper (the repair, not a loss); text that does
/// not parse at all is replaced by the standard wrapper of `cases`.
pub fn merge_cases_into_draft(old_text: &str, cases: &[TestCase]) -> Result<String, String> {
    let old_text = super::strip_bom(old_text);
    let Ok(mut doc) = serde_json::from_str::<Value>(old_text) else {
        return queue_to_json_string(cases);
    };
    let old_array: Vec<Value> = match &doc {
        Value::Array(a) => a.clone(),
        Value::Object(o) => o.get("test_cases").and_then(Value::as_array).cloned().unwrap_or_default(),
        _ => return queue_to_json_string(cases),
    };
    // What the importer made of each entry it READ, keyed by position.
    let read: std::collections::HashMap<usize, TestCase> = super::parse_json_text(old_text)
        .map(|p| p.cases.into_iter().filter_map(|c| c.source.0.map(|i| (i, c))).collect())
        .unwrap_or_default();

    let mut used = vec![false; old_array.len()];
    let mut placed: Vec<(Option<usize>, Value)> = Vec::with_capacity(cases.len());
    for c in cases {
        let from = c.source.0.filter(|i| read.contains_key(i) && !used[*i]);
        match from.and_then(|i| old_array[i].as_object().map(|o| (i, o))) {
            Some((i, obj)) => {
                used[i] = true;
                let mut obj = obj.clone();
                patch_case(&mut obj, &read[&i], c);
                placed.push((Some(i), Value::Object(obj)));
            }
            None => placed.push((None, case_record(c))),
        }
    }
    for (i, raw) in old_array.iter().enumerate() {
        if read.contains_key(&i) {
            continue; // read: either placed above or removed by the operation
        }
        let at = (0..i)
            .rev()
            .find_map(|j| placed.iter().position(|(s, _)| *s == Some(j)))
            .map_or(0, |p| p + 1);
        placed.insert(at, (Some(i), raw.clone()));
    }

    let array = Value::Array(placed.into_iter().map(|(_, v)| v).collect());
    if !doc.is_object() {
        doc = serde_json::from_str(&queue_to_json_string(&[])?).map_err(|e| e.to_string())?;
    }
    doc["test_cases"] = array;
    super::comments::render(&doc)
}

/// Bring one case object in line with `new`, touching only the fields that
/// differ from what the importer read out of it (`old`).
fn patch_case(obj: &mut Map<String, Value>, old: &TestCase, new: &TestCase) {
    if new.title != old.title {
        set_text(obj, &TITLE_KEYS, &new.title);
    }
    if new.update_id != old.update_id {
        match new.update_id {
            Some(id) => set_value(obj, &ID_KEYS, serde_json::json!(id)),
            None => remove_all(obj, &ID_KEYS),
        }
    }
    if new.tags != old.tags {
        // A list stays a list; the importer joins it with "; ".
        let value = if obj.get("tags").is_some_and(Value::is_array) {
            Value::Array(
                new.tags
                    .split(';')
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                    .map(|t| Value::String(t.to_string()))
                    .collect(),
            )
        } else {
            Value::String(new.tags.clone())
        };
        obj.insert("tags".into(), value);
    }
    if new.automation_status != old.automation_status {
        obj.insert("automation_status".into(), Value::String(new.automation_status.clone()));
    }
    if new.module_value != old.module_value {
        set_text(obj, &MODULE_KEYS, &new.module_value);
    }
    if new.preconditions != old.preconditions {
        set_text(obj, &PRECONDITIONS_KEYS, &new.preconditions);
    }
    if new.comment != old.comment {
        set_optional_text(obj, &COMMENT_KEYS, &new.comment);
    }
    if new.reviewer_notes != old.reviewer_notes {
        set_optional_text(obj, &REVIEWER_NOTES_KEYS, &new.reviewer_notes);
    }
    if new.area != old.area {
        set_optional_text(obj, &AREA_KEYS, &new.area);
    }
    if new.spec_order != old.spec_order {
        set_optional_number(obj, &SPEC_ORDER_KEYS, new.spec_order);
    }
    if new.tester_order != old.tester_order {
        set_optional_number(obj, &TESTER_ORDER_KEYS, new.tester_order);
    }
    if new.findings != old.findings {
        if new.findings.is_empty() {
            obj.remove("findings");
        } else {
            obj.insert("findings".into(), serde_json::json!(new.findings));
        }
    }
    if new.steps != old.steps {
        patch_steps(obj, &old.steps, &new.steps);
    }
}

/// Write under the alias the object already uses, else the canonical key.
/// `insert` on an existing key keeps its position (preserve_order).
fn set_value(obj: &mut Map<String, Value>, keys: &[&'static str], value: Value) {
    let key = alias_in_use(obj, keys).unwrap_or(keys[0]);
    obj.insert(key.to_string(), value);
}

fn set_text(obj: &mut Map<String, Value>, keys: &[&'static str], text: &str) {
    set_value(obj, keys, Value::String(text.to_string()));
}

fn remove_all(obj: &mut Map<String, Value>, keys: &[&'static str]) {
    for k in keys {
        obj.remove(*k);
    }
}

/// Optional fields are absent when empty, under every spelling.
fn set_optional_text(obj: &mut Map<String, Value>, keys: &[&'static str], text: &str) {
    if text.is_empty() {
        remove_all(obj, keys);
    } else {
        set_text(obj, keys, text);
    }
}

fn set_optional_number(obj: &mut Map<String, Value>, keys: &[&'static str], n: Option<u32>) {
    match n {
        Some(n) => set_value(obj, keys, serde_json::json!(n)),
        None => remove_all(obj, keys),
    }
}

/// When every raw entry became exactly one step and the count is unchanged,
/// each entry is edited in place (keeping its spelling and extra keys).
/// Otherwise the list is rewritten in the exporter's shape - through
/// `step_json`, so a Shared Steps reference stays `{"shared": N}`.
fn patch_steps(obj: &mut Map<String, Value>, old: &[Step], new: &[Step]) {
    let raw = obj.get("steps").and_then(Value::as_array).cloned().unwrap_or_default();
    let steps: Vec<Value> = if raw.len() == old.len() && old.len() == new.len() {
        raw.iter().zip(old).zip(new).map(|((r, o), n)| patch_step(r, o, n)).collect()
    } else {
        new.iter().map(step_json).collect()
    };
    obj.insert("steps".into(), Value::Array(steps));
}

fn patch_step(raw: &Value, old: &Step, new: &Step) -> Value {
    if old == new {
        return raw.clone();
    }
    match raw.as_object() {
        Some(m) if old.shared.is_none() && new.shared.is_none() => {
            let mut m = m.clone();
            if new.action != old.action {
                set_text(&mut m, &STEP_ACTION_KEYS, &new.action);
            }
            if new.expected != old.expected {
                set_text(&mut m, &STEP_EXPECTED_KEYS, &new.expected);
            }
            Value::Object(m)
        }
        _ => step_json(new),
    }
}

/// The write-back behind `save_draft_cases`. `edits` are the owned queue
/// rows IN QUEUE ORDER: each one's pre-edit row (`before`) and what it is
/// now (`after`, `None` = removed). Each `before` finds its entry in the
/// file by work item id, or by title among id-less entries, the first
/// unclaimed one in file order - claimed in queue order, so the Nth
/// same-titled row is the Nth same-titled entry (the app's occurrence rule,
/// `keysFor`). Found entries are patched or removed. File entries no row
/// mentions are kept (cases an assistant added since the last sync). A row
/// whose entry is gone from the file is appended.
pub fn apply_draft_edits(old_text: &str, edits: &[DraftEdit]) -> Result<String, String> {
    let old_text = super::strip_bom(old_text);
    let parsed = super::parse_json_text(old_text).map(|p| p.cases).unwrap_or_default();
    let mut claimed = vec![false; parsed.len()];
    // None = untouched; Some(None) = removed; Some(Some(case)) = edited.
    let mut fate: Vec<Option<Option<&TestCase>>> = vec![None; parsed.len()];
    let mut unmatched: Vec<TestCase> = vec![];
    for edit in edits {
        match (claim(&parsed, &mut claimed, &edit.before), &edit.after) {
            (Some(k), after) => fate[k] = Some(after.as_ref()),
            (None, Some(after)) => {
                unmatched.push(TestCase { source: SourceIndex(None), ..after.clone() })
            }
            (None, None) => {} // removed, and already gone from the file
        }
    }
    let mut out: Vec<TestCase> = vec![];
    for (k, p) in parsed.iter().enumerate() {
        match fate[k] {
            None => out.push(p.clone()),
            Some(None) => {}
            Some(Some(after)) => out.push(TestCase { source: p.source, ..after.clone() }),
        }
    }
    out.extend(unmatched);
    merge_cases_into_draft(old_text, &out)
}

fn claim(parsed: &[TestCase], claimed: &mut [bool], want: &TestCase) -> Option<usize> {
    let hit = (0..parsed.len()).find(|&k| {
        !claimed[k]
            && match want.update_id {
                Some(id) => parsed[k].update_id == Some(id),
                None => {
                    parsed[k].update_id.is_none()
                        && parsed[k].title.trim().eq_ignore_ascii_case(want.title.trim())
                }
            }
    })?;
    claimed[hit] = true;
    Some(hit)
}
