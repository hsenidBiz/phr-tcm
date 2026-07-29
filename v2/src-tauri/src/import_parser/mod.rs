//! Spreadsheet import/export, ported from v1 app/utils/import_parser.py
//! against tests/test_import_parser.py as golden vectors.
//!
//! Canonical 9-column format: TestCaseID, TestCaseName, StepNumber,
//! StepAction, StepExpected, Tags, AutomationStatus, ModuleValue,
//! Preconditions. One row per step; per-case fields sit only on each case's
//! first step row. A populated TestCaseID flags the case as an UPDATE of that
//! exact work item; a blank ID creates a new one.
//!
//! Layout: this file owns parsing (Excel/CSV/JSON -> TestCase); `export`
//! owns the Excel/JSON writers and the template; `html` owns the standalone
//! HTML report and its autosaving note boxes.

pub mod comments;
mod export;
mod html;

pub use export::{export_queue_to_json, queue_to_json_string};
pub use html::{export_queue_to_html, CommentCtx, DraftFile, DraftNoteCtx, NoteCtx};

use crate::model::{TestCase, MAX_TITLE_LEN, VALID_STATUSES};
use crate::steps_xml::Step;
use std::collections::HashMap;
use std::path::Path;

pub const EXCEL_HEADERS: [&str; 9] = [
    "TestCaseID",
    "TestCaseName",
    "StepNumber",
    "StepAction",
    "StepExpected",
    "Tags",
    "AutomationStatus",
    "ModuleValue",
    "Preconditions",
];

const REQUIRED_COLUMNS: [&str; 3] = ["TestCaseName", "StepNumber", "StepAction"];

pub type Row = (u32, HashMap<String, String>);

/// Parse a file into test cases. Returns (cases, warnings) or a user-facing
/// error string (bad file type / missing columns / unreadable file).
pub fn parse_file(path: &str) -> Result<(Vec<TestCase>, Vec<String>), String> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "json" => parse_json(path),
        other => Err(format!("Unsupported file type: .{other}. Use .json")),
    }
}

/// A work item id from a file, or None with the reason it was rejected.
///
/// `as i32` is a SATURATING cast, and that was the whole bug: "99999999999"
/// became i32::MAX and "12.7" became 12, so an id that was never a work
/// item quietly turned into an UPDATE of a real one somebody else owns.
/// Only an integral value inside the range Azure DevOps actually issues
/// counts; everything else is refused and the case is created instead,
/// which is the recoverable half of being wrong.
fn work_item_id(raw: &str) -> Option<i32> {
    // A JSON writer may render an integral id as "123.0" - tolerate that
    // exact shape and nothing looser. (NaN and infinity fail `fract() == 0`,
    // so they need no separate check.)
    let f = raw.parse::<f64>().ok()?;
    if f.fract() != 0.0 || f < 1.0 || f > i32::MAX as f64 {
        return None;
    }
    Some(f as i32)
}

/// Flatten a step's internal line breaks, reporting whether any were there.
///
/// Azure DevOps stores steps in an HTML field, and the app's own step
/// editor is a single-line input - so a line break in an imported step
/// never survives to anywhere the user will see it. It used to be dropped
/// silently somewhere between here and the browser; folding it here means
/// what the file said and what the app shows are the same thing, and the
/// author gets told their layout did not carry.
fn flatten_step_text(s: &str) -> (String, bool) {
    if !s.contains(['\n', '\r']) {
        return (s.to_string(), false);
    }
    (s.split_whitespace().collect::<Vec<_>>().join(" "), true)
}

fn step_sort_key(item: &Row) -> (i64, u32) {
    let (row_num, row) = item;
    let num = row
        .get("StepNumber")
        .map(|s| s.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("0")
        .parse::<i64>()
        .unwrap_or(0);
    (num, *row_num)
}

/// First non-empty value of `col` across a block's rows (per-case fields sit
/// on whichever row carries them, usually the first step row).
fn block_value(block_rows: &[Row], col: &str) -> String {
    for (_n, row) in block_rows {
        if let Some(v) = row.get(col) {
            if !v.trim().is_empty() {
                return v.trim().to_string();
            }
        }
    }
    String::new()
}

struct Block {
    raw_id: String,
    name: String,
    row: u32,
    rows: Vec<Row>,
}

/// Parse (sheet_row_number, row_map) pairs into test cases. Warnings
/// reference the original sheet row so users can jump straight to it.
pub fn parse_rows(rows: &[Row], headers: &[String]) -> Result<(Vec<TestCase>, Vec<String>), String> {
    let mut warnings: Vec<String> = vec![];

    let missing: Vec<&str> = REQUIRED_COLUMNS
        .iter()
        .filter(|c| !headers.iter().any(|h| h == *c))
        .cloned()
        .collect();
    if !missing.is_empty() {
        let mut sorted_missing = missing.clone();
        sorted_missing.sort();
        let mut all = EXCEL_HEADERS.to_vec();
        all.sort();
        return Err(format!(
            "Missing required columns: {}.\nExpected columns: {}",
            sorted_missing.join(", "),
            all.join(", ")
        ));
    }

    // Group rows into test-case blocks: a row starts a new block when it
    // carries a TestCaseID (keyed by that ID) or a TestCaseName different
    // from the current block; rows blank in both are continuation steps.
    let mut blocks: Vec<Block> = vec![];
    let mut current: Option<usize> = None;
    for (row_num, row) in rows {
        let raw_id = row.get("TestCaseID").map(|s| s.trim()).unwrap_or("").to_string();
        let name = row.get("TestCaseName").map(|s| s.trim()).unwrap_or("").to_string();

        if !raw_id.is_empty() {
            let differs = current
                .map(|i| blocks[i].raw_id != raw_id)
                .unwrap_or(true);
            if differs {
                blocks.push(Block { raw_id: raw_id.clone(), name: name.clone(), row: *row_num, rows: vec![] });
                current = Some(blocks.len() - 1);
            }
        } else if !name.is_empty() {
            let differs = current.map(|i| blocks[i].name != name).unwrap_or(true);
            if differs {
                blocks.push(Block { raw_id: String::new(), name: name.clone(), row: *row_num, rows: vec![] });
                current = Some(blocks.len() - 1);
            }
        } else if current.is_none() {
            warnings.push(format!(
                "Row {row_num}: skipped - no TestCaseName/TestCaseID and no open test case."
            ));
            continue;
        }
        blocks[current.unwrap()].rows.push((*row_num, row.clone()));
    }

    let mut test_cases: Vec<TestCase> = vec![];
    for block in &blocks {
        let mut group = block.rows.clone();
        group.sort_by_key(step_sort_key);
        let first_row = block.row;
        let name = if !block.name.is_empty() {
            block.name.clone()
        } else {
            block_value(&group, "TestCaseName")
        };
        if name.is_empty() {
            warnings.push(format!(
                "Row {first_row}: group of rows skipped - no TestCaseName found."
            ));
            continue;
        }

        if name.chars().count() > MAX_TITLE_LEN {
            let head: String = name.chars().take(60).collect();
            warnings.push(format!(
                "Row {first_row}: test case '{head}...' has a title longer than {MAX_TITLE_LEN} characters - Azure DevOps will reject it."
            ));
        }

        let mut update_id: Option<i32> = None;
        let raw_id = if !block.raw_id.is_empty() {
            block.raw_id.clone()
        } else {
            block_value(&group, "TestCaseID")
        };
        if !raw_id.is_empty() {
            match work_item_id(&raw_id) {
                Some(id) => update_id = Some(id),
                None => warnings.push(format!(
                    "Row {first_row}: test case '{name}' - TestCaseID '{raw_id}' is not a valid work item ID; it will be created as a new test case instead of updating."
                )),
            }
        }

        let tags = block_value(&group, "Tags");
        if tags.contains(',') {
            warnings.push(format!(
                "Row {first_row}: test case '{name}' - Tags contain a comma; separate tags with semicolons (Azure DevOps does not allow commas in tag names)."
            ));
        }
        let mut automation_status = block_value(&group, "AutomationStatus");
        let module_value = block_value(&group, "ModuleValue");
        let preconditions = block_value(&group, "Preconditions");

        if automation_status.is_empty() {
            automation_status = "Not Automated".into();
        } else if !VALID_STATUSES.contains(&automation_status.as_str()) {
            let mut valid = VALID_STATUSES.to_vec();
            valid.sort();
            warnings.push(format!(
                "Row {first_row}: test case '{name}' - AutomationStatus '{automation_status}' is invalid. Defaulting to 'Not Automated'. Valid values: {}",
                valid.join(", ")
            ));
            automation_status = "Not Automated".into();
        }

        let mut steps: Vec<Step> = vec![];
        let mut wrapped_steps = false;
        for (row_num, row) in &group {
            let action = row.get("StepAction").map(|s| s.trim()).unwrap_or("");
            let expected = row.get("StepExpected").map(|s| s.trim()).unwrap_or("");
            if action.is_empty() {
                if !expected.is_empty() {
                    warnings.push(format!(
                        "Row {row_num}: StepExpected is filled but StepAction is empty - step skipped."
                    ));
                }
                continue;
            }
            let (action, action_wrapped) = flatten_step_text(action);
            let (expected, expected_wrapped) = flatten_step_text(expected);
            if action_wrapped || expected_wrapped {
                wrapped_steps = true;
            }
            steps.push(Step { action, expected });
        }

        if steps.is_empty() {
            warnings.push(format!(
                "Row {first_row}: test case '{name}' has no rows with a StepAction - skipped."
            ));
            continue;
        }
        if wrapped_steps {
            warnings.push(format!(
                "Row {first_row}: test case '{name}' - a step spanned several lines; Azure DevOps stores steps on one line, so the line breaks were removed."
            ));
        }

        test_cases.push(TestCase {
            title: name,
            steps,
            tags,
            automation_status,
            module_value,
            preconditions,
            update_id,
            // The spreadsheet format has no comment column - JSON only.
            comment: String::new(),
        });
    }

    Ok((test_cases, warnings))
}

fn json_value<'a>(d: &'a serde_json::Value, keys: &[&str]) -> Option<&'a serde_json::Value> {
    for k in keys {
        match d.get(k) {
            Some(serde_json::Value::Null) | None => continue,
            Some(serde_json::Value::String(s)) if s.is_empty() => continue,
            Some(v) => return Some(v),
        }
    }
    None
}

fn value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

/// Parse the AI round-trip JSON format (wrapper {"test_cases": [...]} or a
/// bare list). A kept `id` flags the case as an UPDATE - same contract as
/// the TestCaseID column. Ported from v1 _parse_json.
fn parse_json(path: &str) -> Result<(Vec<TestCase>, Vec<String>), String> {
    let content = std::fs::read_to_string(path).map_err(|e| format!("Could not read JSON: {e}"))?;
    let content = content.strip_prefix('\u{feff}').unwrap_or(&content);
    let data: serde_json::Value =
        serde_json::from_str(content).map_err(|e| format!("Invalid JSON: {e}"))?;

    let raw_cases = match &data {
        serde_json::Value::Object(o) => o
            .get("test_cases")
            .ok_or("JSON must contain a \"test_cases\" list (or be a bare list of test-case objects).")?
            .clone(),
        serde_json::Value::Array(_) => data.clone(),
        _ => {
            return Err("JSON must be an object with a \"test_cases\" list, or a list of test-case objects.".into())
        }
    };
    let raw_cases = raw_cases
        .as_array()
        .ok_or("\"test_cases\" must be a list.")?
        .clone();

    let mut warnings = vec![];
    let mut test_cases = vec![];
    for (i, raw) in raw_cases.iter().enumerate() {
        let label = format!("Test case {}", i + 1);
        let Some(obj) = raw.as_object() else {
            warnings.push(format!("{label}: skipped - expected an object."));
            continue;
        };
        let raw_v = serde_json::Value::Object(obj.clone());

        let title = json_value(&raw_v, &["title", "name", "test_case_name"])
            .map(value_to_string)
            .unwrap_or_default()
            .trim()
            .to_string();
        if title.is_empty() {
            warnings.push(format!("{label}: skipped - no title."));
            continue;
        }
        if title.chars().count() > MAX_TITLE_LEN {
            let head: String = title.chars().take(60).collect();
            warnings.push(format!(
                "{label}: '{head}...' has a title longer than {MAX_TITLE_LEN} characters - Azure DevOps will reject it."
            ));
        }

        let mut update_id = None;
        if let Some(raw_id) = json_value(&raw_v, &["id", "test_case_id", "work_item_id"]) {
            let s = value_to_string(raw_id);
            match work_item_id(&s) {
                Some(id) => update_id = Some(id),
                None => warnings.push(format!(
                    "{label} ('{title}'): id '{s}' is not a valid work item ID; it will be created as a new test case instead of updating."
                )),
            }
        }

        let tags = match obj.get("tags") {
            Some(serde_json::Value::Array(list)) => list
                .iter()
                .map(value_to_string)
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>()
                .join("; "),
            Some(v) => value_to_string(v).trim().to_string(),
            None => String::new(),
        };
        if tags.contains(',') {
            warnings.push(format!(
                "{label} ('{title}'): Tags contain a comma; separate tags with semicolons (Azure DevOps does not allow commas in tag names)."
            ));
        }

        let mut automation_status = obj
            .get("automation_status")
            .map(value_to_string)
            .unwrap_or_default()
            .trim()
            .to_string();
        if automation_status.is_empty() {
            automation_status = "Not Automated".into();
        } else if !VALID_STATUSES.contains(&automation_status.as_str()) {
            let mut valid = VALID_STATUSES.to_vec();
            valid.sort();
            warnings.push(format!(
                "{label} ('{title}'): AutomationStatus '{automation_status}' is invalid. Defaulting to 'Not Automated'. Valid values: {}",
                valid.join(", ")
            ));
            automation_status = "Not Automated".into();
        }

        let module_value = json_value(&raw_v, &["module", "module_value"])
            .map(value_to_string)
            .unwrap_or_default()
            .trim()
            .to_string();
        let preconditions = json_value(&raw_v, &["preconditions", "prerequisites"])
            .map(value_to_string)
            .unwrap_or_default()
            .trim()
            .to_string();
        // In-app note (round-trips through the JSON export; never sent to ADO).
        let comment = json_value(&raw_v, &["comment", "notes"])
            .map(value_to_string)
            .unwrap_or_default()
            .trim()
            .to_string();

        let raw_steps = match obj.get("steps") {
            None | Some(serde_json::Value::Null) => vec![],
            Some(serde_json::Value::Array(list)) => list.clone(),
            Some(_) => {
                warnings.push(format!("{label} ('{title}'): 'steps' must be a list - skipped."));
                continue;
            }
        };
        let mut steps = vec![];
        let mut wrapped_steps = false;
        for (j, rs) in raw_steps.iter().enumerate() {
            let (action, expected) = match rs {
                serde_json::Value::String(s) => (s.trim().to_string(), String::new()),
                serde_json::Value::Object(_) => {
                    let action = json_value(rs, &["action", "step"])
                        .map(value_to_string)
                        .unwrap_or_default()
                        .trim()
                        .to_string();
                    let expected = json_value(rs, &["expected", "expected_result", "result"])
                        .map(value_to_string)
                        .unwrap_or_default()
                        .trim()
                        .to_string();
                    (action, expected)
                }
                _ => {
                    warnings.push(format!(
                        "{label} ('{title}') step {}: expected an object or string - step skipped.",
                        j + 1
                    ));
                    continue;
                }
            };
            if action.is_empty() {
                if !expected.is_empty() {
                    warnings.push(format!(
                        "{label} ('{title}') step {}: has an expected result but no action - step skipped.",
                        j + 1
                    ));
                }
                continue;
            }
            let (action, action_wrapped) = flatten_step_text(&action);
            let (expected, expected_wrapped) = flatten_step_text(&expected);
            if action_wrapped || expected_wrapped {
                wrapped_steps = true;
            }
            steps.push(Step { action, expected });
        }

        if steps.is_empty() {
            warnings.push(format!(
                "{label} ('{title}'): has no steps with an action - skipped."
            ));
            continue;
        }
        if wrapped_steps {
            warnings.push(format!(
                "{label} ('{title}'): a step spanned several lines; Azure DevOps stores steps on one line, so the line breaks were removed."
            ));
        }

        test_cases.push(TestCase {
            title,
            steps,
            tags,
            automation_status,
            module_value,
            preconditions,
            update_id,
            comment,
        });
    }

    Ok((test_cases, warnings))
}