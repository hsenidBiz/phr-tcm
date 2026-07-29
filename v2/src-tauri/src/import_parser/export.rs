//! The JSON export: the round-trip document an assistant edits and the app
//! re-imports. JSON is the only interchange format - the spreadsheet
//! readers and writers this module was ported from were never reachable
//! from the v2 UI and have been removed.

use crate::model::TestCase;

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

pub fn queue_to_json_string(queue: &[TestCase]) -> Result<String, String> {
    let records: Vec<serde_json::Value> = queue
        .iter()
        .map(|tc| {
            let mut rec = serde_json::json!({
                "id": tc.update_id,
                "title": tc.title,
                "tags": tc.tags,
                "automation_status": tc.automation_status,
                "module": tc.module_value,
                "preconditions": tc.preconditions,
                "steps": tc.steps.iter().map(|s| serde_json::json!({
                    "action": s.action, "expected": s.expected
                })).collect::<Vec<_>>(),
            });
            // In-app note: round-trips through this file, never sent to
            // ADO - and only present when non-empty, so a tool that
            // round-trips a caller's draft does not inject a field the
            // caller never wrote (a transform must be idempotent in shape).
            if !tc.comment.is_empty() {
                rec["comment"] = serde_json::json!(tc.comment);
            }
            rec
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
    Ok(text)
}

/// Export the queue in the v1 AI round-trip JSON format
/// (export_formats.export_records_to_json) - re-importable via parse_file.
pub fn export_queue_to_json(queue: &[TestCase], path: &str) -> Result<(), String> {
    std::fs::write(path, queue_to_json_string(queue)?).map_err(|e| e.to_string())
}
