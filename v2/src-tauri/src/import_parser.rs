//! Spreadsheet import/export, ported from v1 app/utils/import_parser.py
//! against tests/test_import_parser.py as golden vectors.
//!
//! Canonical 9-column format: TestCaseID, TestCaseName, StepNumber,
//! StepAction, StepExpected, Tags, AutomationStatus, ModuleValue,
//! Preconditions. One row per step; per-case fields sit only on each case's
//! first step row. A populated TestCaseID flags the case as an UPDATE of that
//! exact work item; a blank ID creates a new one.

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

const EXCEL_COL_WIDTHS: [f64; 9] = [12.0, 30.0, 12.0, 45.0, 45.0, 20.0, 18.0, 20.0, 40.0];

const ID_COLUMN_HELP: &str = "Work item ID. Leave blank to create a NEW test case. \
When you re-import a file exported from the Edit Test Cases tab, keep this \
value to UPDATE that existing test case instead of creating a duplicate.";

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
        "xlsx" => parse_excel(path),
        "csv" => parse_csv(path),
        "json" => parse_json(path),
        other => Err(format!(
            "Unsupported file type: .{other}. Use .xlsx, .csv or .json"
        )),
    }
}

fn parse_excel(path: &str) -> Result<(Vec<TestCase>, Vec<String>), String> {
    use calamine::{Data, Reader};
    let mut wb: calamine::Xlsx<_> =
        calamine::open_workbook(path).map_err(|e| format!("Could not open Excel file: {e}"))?;
    let sheet_name = wb
        .sheet_names()
        .first()
        .cloned()
        .ok_or("The Excel file is empty.")?;
    let range = wb
        .worksheet_range(&sheet_name)
        .map_err(|e| format!("Could not read the worksheet: {e}"))?;

    let mut rows_iter = range.rows();
    let Some(header_row) = rows_iter.next() else {
        return Err("The Excel file is empty.".into());
    };
    let cell_str = |c: &Data| -> String {
        match c {
            Data::Empty => String::new(),
            // Excel numeric cells render whole floats without ".0" (openpyxl
            // hands ints back to v1, so "2" not "2.0" is the faithful form).
            Data::Float(f) if f.fract() == 0.0 && f.is_finite() => format!("{}", *f as i64),
            other => other.to_string().trim().to_string(),
        }
    };
    let headers: Vec<String> = header_row.iter().map(|c| cell_str(c).trim().to_string()).collect();

    let mut data_rows: Vec<Row> = vec![];
    for (i, row) in rows_iter.enumerate() {
        let row_num = (i + 2) as u32; // header = sheet row 1
        let mut map = HashMap::new();
        let mut any = false;
        for (h, cell) in headers.iter().zip(row.iter()) {
            let v = cell_str(cell).trim().to_string();
            if !v.is_empty() {
                any = true;
            }
            map.insert(h.clone(), v);
        }
        if any {
            data_rows.push((row_num, map));
        }
    }
    parse_rows(&data_rows, &headers)
}

fn parse_csv(path: &str) -> Result<(Vec<TestCase>, Vec<String>), String> {
    let content = std::fs::read_to_string(path).map_err(|e| format!("Could not read CSV: {e}"))?;
    let content = content.strip_prefix('\u{feff}').unwrap_or(&content);
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(content.as_bytes());
    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| format!("Could not read CSV headers: {e}"))?
        .iter()
        .map(|h| h.trim().to_string())
        .collect();
    let mut data_rows: Vec<Row> = vec![];
    let mut records = reader.records();
    loop {
        let Some(record) = records.next() else { break };
        let record = record.map_err(|e| format!("Could not read CSV row: {e}"))?;
        // Physical line of the record's END, so blank lines and quoted
        // multi-line fields never drift the row numbers in warnings
        // (mirrors v1's reader.line_num). record.position() points at the
        // start of the scan (before skipped blank lines), so take the max
        // of it and reader-position-minus-one to survive both blank-line
        // runs and a missing trailing newline.
        let start = record.position().map(|p| p.line() as u32).unwrap_or(0);
        let after = records.reader().position().line() as u32;
        let line = start.max(after.saturating_sub(1)).max(1);
        let mut map = HashMap::new();
        for (h, v) in headers.iter().zip(record.iter()) {
            map.insert(h.clone(), v.trim().to_string());
        }
        data_rows.push((line, map));
    }
    parse_rows(&data_rows, &headers)
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
            // Excel numeric cells may render as "123.0"; tolerate that.
            match raw_id.parse::<f64>() {
                Ok(f) if f.is_finite() => update_id = Some(f as i32),
                _ => warnings.push(format!(
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
            steps.push(Step {
                action: action.to_string(),
                expected: expected.to_string(),
            });
        }

        if steps.is_empty() {
            warnings.push(format!(
                "Row {first_row}: test case '{name}' has no rows with a StepAction - skipped."
            ));
            continue;
        }

        test_cases.push(TestCase {
            title: name,
            steps,
            tags,
            automation_status,
            module_value,
            preconditions,
            update_id,
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
            match s.parse::<f64>() {
                Ok(f) if f.is_finite() => update_id = Some(f as i32),
                _ => warnings.push(format!(
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

        let raw_steps = match obj.get("steps") {
            None | Some(serde_json::Value::Null) => vec![],
            Some(serde_json::Value::Array(list)) => list.clone(),
            Some(_) => {
                warnings.push(format!("{label} ('{title}'): 'steps' must be a list - skipped."));
                continue;
            }
        };
        let mut steps = vec![];
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
            steps.push(Step { action, expected });
        }

        if steps.is_empty() {
            warnings.push(format!(
                "{label} ('{title}'): has no steps with an action - skipped."
            ));
            continue;
        }

        test_cases.push(TestCase {
            title,
            steps,
            tags,
            automation_status,
            module_value,
            preconditions,
            update_id,
        });
    }

    Ok((test_cases, warnings))
}

fn write_excel_headers(ws: &mut rust_xlsxwriter::Worksheet) -> Result<(), String> {
    use rust_xlsxwriter::{Color, Format, FormatAlign, Note};
    let header_fmt = Format::new()
        .set_background_color(Color::RGB(0x366092))
        .set_font_color(Color::White)
        .set_bold()
        .set_align(FormatAlign::Center);
    for (col, h) in EXCEL_HEADERS.iter().enumerate() {
        ws.write_string_with_format(0, col as u16, *h, &header_fmt)
            .map_err(|e| e.to_string())?;
        ws.set_column_width(col as u16, EXCEL_COL_WIDTHS[col])
            .map_err(|e| e.to_string())?;
    }
    // Explain the round-trip behaviour of the TestCaseID column.
    let note = Note::new(ID_COLUMN_HELP).set_author("Test Case Manager");
    ws.insert_note(0, 0, &note).map_err(|e| e.to_string())?;
    ws.set_freeze_panes(1, 0).map_err(|e| e.to_string())?;
    Ok(())
}

fn append_case_rows(
    ws: &mut rust_xlsxwriter::Worksheet,
    next_row: &mut u32,
    wid: &str,
    title: &str,
    steps: &[Step],
    tags: &str,
    automation_status: &str,
    module_value: &str,
    preconditions: &str,
) -> Result<(), String> {
    for (i, step) in steps.iter().enumerate() {
        let first = i == 0;
        let r = *next_row;
        ws.write_string(r, 0, if first { wid } else { "" })
            .map_err(|e| e.to_string())?;
        ws.write_string(r, 1, if first { title } else { "" })
            .map_err(|e| e.to_string())?;
        ws.write_number(r, 2, (i + 1) as f64).map_err(|e| e.to_string())?;
        ws.write_string(r, 3, &step.action).map_err(|e| e.to_string())?;
        ws.write_string(r, 4, &step.expected).map_err(|e| e.to_string())?;
        ws.write_string(r, 5, if first { tags } else { "" })
            .map_err(|e| e.to_string())?;
        ws.write_string(r, 6, if first { automation_status } else { "" })
            .map_err(|e| e.to_string())?;
        ws.write_string(r, 7, if first { module_value } else { "" })
            .map_err(|e| e.to_string())?;
        ws.write_string(r, 8, if first { preconditions } else { "" })
            .map_err(|e| e.to_string())?;
        *next_row += 1;
    }
    Ok(())
}

/// Export the in-memory queue to Excel (same 9-column format as the template).
pub fn export_queue_to_excel(queue: &[TestCase], path: &str) -> Result<(), String> {
    let mut wb = rust_xlsxwriter::Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name("Test Cases").map_err(|e| e.to_string())?;
    write_excel_headers(ws)?;

    let mut next_row = 1u32;
    for tc in queue {
        // Preserve update_id so a queued update still round-trips as an update.
        let wid = tc.update_id.map(|i| i.to_string()).unwrap_or_default();
        append_case_rows(
            ws,
            &mut next_row,
            &wid,
            &tc.title,
            &tc.steps,
            &tc.tags,
            &tc.automation_status,
            &tc.module_value,
            &tc.preconditions,
        )?;
    }
    wb.save(path).map_err(|e| e.to_string())?;
    Ok(())
}

const AI_FORMAT_NAME: &str = "azure-devops-test-cases";
const AI_FORMAT_VERSION: u32 = 1;
const AI_INSTRUCTIONS: &str = "Each entry in test_cases is one Azure DevOps Test Case. Edit this file \
freely but keep it valid JSON with this exact structure. Rules: keep \
'id' unchanged so re-importing UPDATES that existing work item; set 'id' \
to null to CREATE a new test case. 'title' is required (max 255 chars). \
'steps' is an ordered list; every step needs a non-empty 'action', \
'expected' may be an empty string. 'automation_status' must be exactly \
'Not Automated' or 'Planned'. 'tags' is a single semicolon-separated \
string - commas are not allowed in tags. 'module' and 'preconditions' \
are free text and may be empty strings.";

/// Export the queue in the v1 AI round-trip JSON format
/// (export_formats.export_records_to_json) - re-importable via parse_file.
pub fn export_queue_to_json(queue: &[TestCase], path: &str) -> Result<(), String> {
    let records: Vec<serde_json::Value> = queue
        .iter()
        .map(|tc| {
            serde_json::json!({
                "id": tc.update_id,
                "title": tc.title,
                "tags": tc.tags,
                "automation_status": tc.automation_status,
                "module": tc.module_value,
                "preconditions": tc.preconditions,
                "steps": tc.steps.iter().map(|s| serde_json::json!({
                    "action": s.action, "expected": s.expected
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    let doc = serde_json::json!({
        "format": AI_FORMAT_NAME,
        "version": AI_FORMAT_VERSION,
        "instructions": AI_INSTRUCTIONS,
        "test_cases": records,
    });
    let mut text = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
    text.push('\n');
    std::fs::write(path, text).map_err(|e| e.to_string())
}

fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const HTML_CSS: &str = r#"
:root { color-scheme: light; }
* { box-sizing: border-box; }
body { font-family: 'Segoe UI', system-ui, sans-serif; margin: 0; padding: 32px 16px;
       background: #f3f5f8; color: #1f2530; }
.page { max-width: 900px; margin: 0 auto; }
h1 { font-size: 22px; margin: 0 0 4px; }
.subtitle { color: #5c6675; font-size: 13px; margin: 0 0 24px; }
.case { background: #fff; border: 1px solid #dde3ec; border-radius: 10px;
        padding: 18px 22px; margin-bottom: 18px; box-shadow: 0 1px 3px rgba(20,30,50,.05);
        page-break-inside: avoid; }
.case h2 { font-size: 16px; margin: 0 0 8px; }
.case .wid { color: #2a7ab8; font-weight: 600; margin-right: 6px; }
.meta { display: flex; flex-wrap: wrap; gap: 6px; margin: 0 0 10px; }
.chip { font-size: 11.5px; border-radius: 999px; padding: 2px 10px;
        background: #eef2f8; color: #44506a; border: 1px solid #dbe2ee; }
.chip.status { background: #e8f3ea; color: #2f6b3c; border-color: #cfe5d4; }
.chip.module { background: #f0eafa; color: #5b3e9e; border-color: #e0d5f2; }
.pre { font-size: 13px; background: #f7f9fc; border-left: 3px solid #b9c6da;
       padding: 8px 12px; margin: 0 0 12px; white-space: pre-wrap; }
.pre b { color: #44506a; }
.pre .none { color: #8a94a6; font-style: italic; }
table { width: 100%; border-collapse: collapse; font-size: 13px; }
th { text-align: left; background: #f0f3f8; color: #44506a; font-size: 12px;
     padding: 6px 10px; border: 1px solid #e1e7f0; }
td { padding: 7px 10px; border: 1px solid #e7ecf3; vertical-align: top;
     white-space: pre-wrap; }
td.num { width: 34px; text-align: center; color: #7c8698; }
.searchbar { position: sticky; top: 0; z-index: 5; background: #f3f5f8;
             display: flex; align-items: center; gap: 12px; padding: 10px 0 14px; }
#tc-search { flex: 1; font: inherit; font-size: 14px; padding: 9px 14px;
             border: 1px solid #c9d3e2; border-radius: 8px; background: #fff;
             color: inherit; outline: none; }
#tc-search:focus { border-color: #2a7ab8; box-shadow: 0 0 0 3px rgba(42,122,184,.15); }
#tc-count { color: #5c6675; font-size: 12.5px; white-space: nowrap; }
.no-match { color: #5c6675; font-size: 14px; text-align: center;
            padding: 28px 0; border: 1px dashed #c9d3e2; border-radius: 10px; }
.hidden { display: none !important; }
@media print { body { background: #fff; padding: 0; }
               .case { box-shadow: none; border-color: #ccc; }
               .searchbar { display: none; } }
"#;

const HTML_JS: &str = r#"
(function () {
  var input = document.getElementById('tc-search');
  var count = document.getElementById('tc-count');
  var noMatch = document.getElementById('tc-no-match');
  var cards = Array.prototype.slice.call(document.querySelectorAll('.case'));
  var texts = cards.map(function (c) { return c.textContent.toLowerCase(); });
  var total = cards.length;

  function apply() {
    var words = input.value.toLowerCase().split(/\s+/).filter(Boolean);
    var shown = 0;
    texts.forEach(function (t, i) {
      var hit = words.every(function (w) { return t.indexOf(w) !== -1; });
      cards[i].classList.toggle('hidden', !hit);
      if (hit) shown++;
    });
    count.textContent = words.length
      ? shown + ' of ' + total + ' shown'
      : total + ' test case' + (total !== 1 ? 's' : '');
    noMatch.classList.toggle('hidden', shown !== 0);
  }

  input.addEventListener('input', apply);
  input.addEventListener('keydown', function (e) {
    if (e.key === 'Escape') { input.value = ''; apply(); }
  });
  apply();
})();
"#;

/// Standalone, print-friendly HTML report, ported from v1
/// export_records_to_html (same cards, chips, sticky search filter).
pub fn export_queue_to_html(queue: &[TestCase], path: &str, subtitle: &str) -> Result<(), String> {
    let mut parts: Vec<String> = vec![
        "<!DOCTYPE html>".into(),
        "<html lang=\"en\"><head><meta charset=\"utf-8\">".into(),
        "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">".into(),
        format!("<title>Test Cases ({})</title>", queue.len()),
        format!("<style>{HTML_CSS}</style></head><body><div class='page'>"),
        "<h1>Test Cases</h1>".into(),
        format!(
            "<p class='subtitle'>{}</p>",
            if subtitle.is_empty() {
                format!("{} test case(s)", queue.len())
            } else {
                esc(subtitle)
            }
        ),
        "<div class='searchbar'>".into(),
        "<input id='tc-search' type='search' placeholder='Search title, ID, tags, steps, prerequisites...' aria-label='Search test cases'>".into(),
        "<span id='tc-count'></span></div>".into(),
        "<p id='tc-no-match' class='no-match hidden'>No test cases match your search.</p>".into(),
    ];
    for tc in queue {
        parts.push("<div class='case'>".into());
        let wid = tc
            .update_id
            .map(|id| format!("<span class='wid'>#{id}</span>"))
            .unwrap_or_default();
        parts.push(format!("<h2>{wid}{}</h2>", esc(&tc.title)));

        let mut chips = vec![];
        if !tc.automation_status.is_empty() {
            chips.push(format!("<span class='chip status'>{}</span>", esc(&tc.automation_status)));
        }
        if !tc.module_value.is_empty() {
            chips.push(format!("<span class='chip module'>{}</span>", esc(&tc.module_value)));
        }
        for tag in tc.tags.split(';') {
            let tag = tag.trim();
            if !tag.is_empty() {
                chips.push(format!("<span class='chip'>{}</span>", esc(tag)));
            }
        }
        if !chips.is_empty() {
            parts.push(format!("<div class='meta'>{}</div>", chips.join("")));
        }

        // Every case shows a Prerequisites block, even when empty (v1 rule).
        let prereq = tc.preconditions.trim();
        let prereq_html = if prereq.is_empty() {
            "<span class='none'>None</span>".to_string()
        } else {
            esc(prereq)
        };
        parts.push(format!("<p class='pre'><b>Prerequisites:</b> {prereq_html}</p>"));

        if !tc.steps.is_empty() {
            parts.push("<table><tr><th>#</th><th>Action</th><th>Expected result</th></tr>".into());
            for (i, step) in tc.steps.iter().enumerate() {
                parts.push(format!(
                    "<tr><td class='num'>{}</td><td>{}</td><td>{}</td></tr>",
                    i + 1,
                    esc(&step.action),
                    esc(&step.expected)
                ));
            }
            parts.push("</table>".into());
        }
        parts.push("</div>".into());
    }
    parts.push(format!("</div><script>{HTML_JS}</script></body></html>"));
    std::fs::write(path, parts.join("\n")).map_err(|e| e.to_string())
}

/// Write a blank Excel template with the example rows from v1.
pub fn generate_template(save_path: &str) -> Result<(), String> {
    let mut wb = rust_xlsxwriter::Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name("Test Cases").map_err(|e| e.to_string())?;
    write_excel_headers(ws)?;

    let mut next_row = 1u32;
    let login_steps = [
        Step { action: "Navigate to the login page".into(), expected: "Login page is displayed".into() },
        Step { action: "Enter valid username and password".into(), expected: "Fields accept the input".into() },
        Step { action: "Click the Login button".into(), expected: "User is redirected to the dashboard".into() },
    ];
    append_case_rows(ws, &mut next_row, "", "Login as admin", &login_steps, "smoke", "Not Automated", "Authentication", "User is logged out")?;
    let invalid_steps = [
        Step { action: "Navigate to the login page".into(), expected: "Login page is displayed".into() },
        Step { action: "Enter an invalid password".into(), expected: "Error message is displayed".into() },
    ];
    append_case_rows(ws, &mut next_row, "", "Invalid login attempt", &invalid_steps, "regression", "Not Automated", "Authentication", "User is logged out")?;
    wb.save(save_path).map_err(|e| e.to_string())?;
    Ok(())
}
