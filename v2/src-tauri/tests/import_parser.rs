//! Golden vectors ported 1:1 from v1 tests/test_import_parser.py.

use std::collections::HashMap;
use v2_lib::import_parser::{
    export_queue_to_json, parse_file, parse_rows,
    EXCEL_HEADERS,
};
use v2_lib::model::TestCase;
use v2_lib::steps_xml::Step;

fn headers() -> Vec<String> {
    EXCEL_HEADERS.iter().map(|s| s.to_string()).collect()
}

fn row(kv: &[(&str, &str)]) -> HashMap<String, String> {
    let mut m: HashMap<String, String> = EXCEL_HEADERS
        .iter()
        .map(|h| (h.to_string(), String::new()))
        .collect();
    for (k, v) in kv {
        m.insert(k.to_string(), v.to_string());
    }
    m
}

fn tmp_path(name: &str) -> String {
    let dir = std::env::temp_dir().join("tcm-v2-import-tests");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(format!("{}-{}", std::process::id(), name))
        .to_string_lossy()
        .to_string()
}

#[test]
fn missing_required_columns_errors() {
    let err = parse_rows(&[], &["TestCaseName".to_string(), "StepAction".to_string()])
        .unwrap_err();
    assert!(err.contains("Missing required columns"));
    assert!(err.contains("StepNumber"));
}

#[test]
fn basic_case_with_continuation_rows() {
    let rows = vec![
        (2, row(&[("TestCaseName", "Login"), ("StepNumber", "1"), ("StepAction", "Open"), ("Tags", "smoke")])),
        (3, row(&[("StepNumber", "2"), ("StepAction", "Type"), ("StepExpected", "Accepted")])),
        (4, row(&[("StepNumber", "3"), ("StepAction", "Submit")])),
    ];
    let (cases, warnings) = parse_rows(&rows, &headers()).unwrap();
    assert!(warnings.is_empty());
    assert_eq!(cases.len(), 1);
    let tc = &cases[0];
    assert_eq!(tc.title, "Login");
    assert_eq!(tc.tags, "smoke");
    assert_eq!(
        tc.steps.iter().map(|s| s.action.as_str()).collect::<Vec<_>>(),
        vec!["Open", "Type", "Submit"]
    );
    assert_eq!(tc.update_id, None);
}

#[test]
fn testcaseid_marks_update_and_tolerates_float_formatting() {
    let rows = vec![(2, row(&[("TestCaseID", "123.0"), ("TestCaseName", "Existing"), ("StepNumber", "1"), ("StepAction", "Do")]))];
    let (cases, _) = parse_rows(&rows, &headers()).unwrap();
    assert_eq!(cases[0].update_id, Some(123));
}

#[test]
fn invalid_testcaseid_warns_and_creates_new() {
    let rows = vec![(5, row(&[("TestCaseID", "abc"), ("TestCaseName", "Bad"), ("StepNumber", "1"), ("StepAction", "Do")]))];
    let (cases, warnings) = parse_rows(&rows, &headers()).unwrap();
    assert_eq!(cases[0].update_id, None);
    assert!(warnings.iter().any(|w| w.contains("Row 5") && w.contains("abc")));
}

#[test]
fn invalid_automation_status_warns_with_row() {
    let rows = vec![(7, row(&[("TestCaseName", "X"), ("StepNumber", "1"), ("StepAction", "Do"), ("AutomationStatus", "Nope")]))];
    let (cases, warnings) = parse_rows(&rows, &headers()).unwrap();
    assert_eq!(cases[0].automation_status, "Not Automated");
    assert!(warnings.iter().any(|w| w.contains("Row 7") && w.contains("Nope")));
}

#[test]
fn steps_sorted_by_step_number() {
    let rows = vec![
        (2, row(&[("TestCaseName", "Order"), ("StepNumber", "2"), ("StepAction", "Second")])),
        (3, row(&[("StepNumber", "1"), ("StepAction", "First")])),
    ];
    let (cases, _) = parse_rows(&rows, &headers()).unwrap();
    assert_eq!(
        cases[0].steps.iter().map(|s| s.action.as_str()).collect::<Vec<_>>(),
        vec!["First", "Second"]
    );
}

#[test]
fn comma_tags_warns() {
    let rows = vec![(2, row(&[("TestCaseName", "T"), ("StepNumber", "1"), ("StepAction", "Do"), ("Tags", "a, b")]))];
    let (_cases, warnings) = parse_rows(&rows, &headers()).unwrap();
    assert!(warnings.iter().any(|w| w.contains("comma")));
}

#[test]
fn long_title_warns_but_case_kept() {
    let long = "X".repeat(300);
    let rows = vec![(3, row(&[("TestCaseName", long.as_str()), ("StepNumber", "1"), ("StepAction", "Do")]))];
    let (cases, warnings) = parse_rows(&rows, &headers()).unwrap();
    assert_eq!(cases.len(), 1);
    assert!(warnings.iter().any(|w| w.contains("Row 3") && w.contains("255")));
}

#[test]
fn expected_without_action_warns_and_skips_step() {
    let rows = vec![
        (2, row(&[("TestCaseName", "T"), ("StepNumber", "1"), ("StepAction", "Do")])),
        (3, row(&[("StepNumber", "2"), ("StepExpected", "Orphan expected")])),
    ];
    let (cases, warnings) = parse_rows(&rows, &headers()).unwrap();
    assert_eq!(cases[0].steps.len(), 1);
    assert!(warnings
        .iter()
        .any(|w| w.contains("Row 3") && w.contains("StepAction is empty")));
}

#[test]
fn case_without_steps_skipped_with_row() {
    let rows = vec![(4, row(&[("TestCaseName", "Empty")]))];
    let (cases, warnings) = parse_rows(&rows, &headers()).unwrap();
    assert!(cases.is_empty());
    assert!(warnings.iter().any(|w| w.contains("Row 4") && w.contains("Empty")));
}




#[test]
fn json_export_round_trips_through_the_importer() {
    let queue = vec![
        TestCase {
            title: "JSON case".into(),
            steps: vec![Step { action: "Do".into(), expected: "Done".into() }],
            tags: "smoke".into(),
            automation_status: "Planned".into(),
            module_value: "Auth".into(),
            preconditions: "Logged out".into(),
            update_id: Some(77),
            comment: "Flaky on Fridays - re-check with QA".into(),
        },
        TestCase {
            title: "New one".into(),
            steps: vec![Step { action: "Go".into(), expected: "".into() }],
            automation_status: "Not Automated".into(),
            ..Default::default()
        },
    ];
    let path = tmp_path("roundtrip.json");
    export_queue_to_json(&queue, &path).unwrap();

    // v1 wrapper shape present
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(doc["format"], "azure-devops-test-cases");
    assert_eq!(doc["version"], 1);
    assert!(doc["instructions"].as_str().unwrap().contains("UPDATES"));

    let (cases, warnings) = parse_file(&path).unwrap();
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    assert_eq!(cases.len(), 2);
    assert_eq!(cases[0].update_id, Some(77));
    assert_eq!(cases[0].module_value, "Auth");
    assert_eq!(cases[0].preconditions, "Logged out");
    // The in-app comment survives the JSON round trip (and only that -
    // it is never mapped to an ADO field).
    assert_eq!(cases[0].comment, "Flaky on Fridays - re-check with QA");
    assert_eq!(cases[1].comment, "");
    assert_eq!(cases[1].update_id, None);
}

#[test]
fn html_export_carries_cases_and_search() {
    let queue = vec![TestCase {
        title: "Login <works>".into(),
        steps: vec![Step { action: "Open & go".into(), expected: "Shown".into() }],
        tags: "smoke; ui".into(),
        automation_status: "Planned".into(),
        module_value: "Auth".into(),
        preconditions: "".into(),
        update_id: Some(42),
        comment: String::new(),
    }];
    let path = tmp_path("report.html");
    v2_lib::import_parser::export_queue_to_html(&queue, &path, "PBI #7", None).unwrap();
    let html = std::fs::read_to_string(&path).unwrap();
    assert!(html.contains("Login &lt;works&gt;")); // escaped
    assert!(html.contains("Open &amp; go"));
    assert!(html.contains("#42"));
    assert!(html.contains("chip status")); // Planned chip
    assert!(html.contains("tc-search")); // client-side filter
    assert!(html.contains("None")); // empty prerequisites block still shown
    assert!(html.contains("PBI #7")); // subtitle
    assert!(!html.contains("class='note-box'")); // no note ctx -> no comment boxes
}

#[test]
fn html_export_with_note_ctx_adds_autosaving_comment_boxes() {
    let queue = vec![
        TestCase {
            title: "Existing case".into(),
            steps: vec![],
            automation_status: "Planned".into(),
            update_id: Some(42),
            ..Default::default()
        },
        TestCase {
            title: "New case (no id yet)".into(),
            steps: vec![],
            automation_status: "Planned".into(),
            update_id: None,
            ..Default::default()
        },
    ];
    let mut notes = std::collections::HashMap::new();
    notes.insert("42".to_string(), "Needs the <new> dialog".to_string());
    let ctx = v2_lib::import_parser::NoteCtx { port: 4711, org: "acme".into(), notes };

    let path = tmp_path("report-notes.html");
    v2_lib::import_parser::export_queue_to_html(
        &queue,
        &path,
        "",
        Some(v2_lib::import_parser::CommentCtx::Ado(&ctx)),
    )
    .unwrap();
    let html = std::fs::read_to_string(&path).unwrap();

    // The identified case gets a prefilled (escaped) comment box...
    assert!(html.contains("data-ado='42'"));
    assert!(html.contains("Needs the &lt;new&gt; dialog"));
    // ...wired to the loopback listener with the org baked in.
    assert!(html.contains("var NOTE_PORT=4711"));
    assert!(html.contains("var NOTE_ORG=\"acme\""));
    assert!(html.contains("/note"));
    // Cases without a work item id get no box (nowhere to attach the note).
    assert_eq!(html.matches("note-box' id=").count(), 1);
}

#[test]
fn is_valid_rules_ported() {
    let ok = TestCase {
        title: "T".into(),
        steps: vec![Step { action: "Do".into(), expected: "".into() }],
        automation_status: "Planned".into(),
        ..Default::default()
    };
    assert!(ok.is_valid().is_ok());

    let mut bad = ok.clone();
    bad.title = "  ".into();
    assert!(bad.is_valid().unwrap_err().contains("Title is required"));

    let mut bad = ok.clone();
    bad.title = "X".repeat(300);
    assert!(bad.is_valid().unwrap_err().contains("255"));

    let mut bad = ok.clone();
    bad.steps.clear();
    assert!(bad.is_valid().unwrap_err().contains("At least one step"));

    let mut bad = ok.clone();
    bad.steps[0].action = " ".into();
    assert!(bad.is_valid().unwrap_err().contains("Step 1 action"));

    let mut bad = ok.clone();
    bad.automation_status = "Automated".into();
    assert!(bad.is_valid().unwrap_err().contains("Invalid automation status"));

    let mut bad = ok.clone();
    bad.tags = "a, b".into();
    assert!(bad.is_valid().unwrap_err().contains("semicolons"));
}

/// JSON is the only interchange format. The spreadsheet readers and writers
/// were ported from v1 but nothing in the v2 UI ever called them - every
/// file dialog filters to .json - so they were removed along with calamine,
/// rust_xlsxwriter and csv. This pins the contract so they do not creep
/// back in unnoticed.
#[test]
fn only_json_is_accepted_and_the_error_says_so() {
    let path = tmp_path("cases.xlsx");
    std::fs::write(&path, b"not really a workbook").unwrap();
    let err = parse_file(&path).unwrap_err();
    // It echoes the extension it rejected, which is useful; what it must
    // not do is still OFFER the formats that no longer exist.
    assert!(err.contains("Use .json"), "got: {err}");
    assert!(!err.contains("Use .xlsx"), "the error still offers xlsx: {err}");
    assert!(!err.contains(".csv"), "the error still offers csv: {err}");
    let _ = std::fs::remove_file(&path);

    let csv = tmp_path("cases.csv");
    std::fs::write(&csv, b"TestCaseName,StepNumber,StepAction\na,1,b").unwrap();
    assert!(parse_file(&csv).is_err(), "csv is no longer an import format");
    let _ = std::fs::remove_file(&csv);
}
