//! Excel / JSON queue exports and the blank import template.

use super::EXCEL_HEADERS;
use crate::model::TestCase;
use crate::steps_xml::Step;

const EXCEL_COL_WIDTHS: [f64; 9] = [12.0, 30.0, 12.0, 45.0, 45.0, 20.0, 18.0, 20.0, 40.0];

const ID_COLUMN_HELP: &str = "Work item ID. Leave blank to create a NEW test case. \
When you re-import a file exported from the Edit Test Cases tab, keep this \
value to UPDATE that existing test case instead of creating a duplicate.";

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
                // In-app note: round-trips through this file, never sent to ADO.
                "comment": tc.comment,
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
