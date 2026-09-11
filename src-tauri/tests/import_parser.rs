//! Golden vectors ported 1:1 from v1 tests/test_import_parser.py.

use std::collections::HashMap;
use v2_lib::import_parser::{export_queue_to_html, 
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
            reviewer_notes: "## Source\n\nSpec **3.2**, AC-4. Out of scope: SSO.".into(),
            spec_order: None,
            tester_order: None,
            findings: vec![],
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
    // Reviewer notes ride the same road: written out, read back, markdown
    // untouched on the way through - the file is the transport, not the
    // renderer.
    assert_eq!(
        cases[0].reviewer_notes,
        "## Source\n\nSpec **3.2**, AC-4. Out of scope: SSO."
    );
    assert_eq!(cases[1].reviewer_notes, "");

    // And neither app-only field is written when it is empty, so a draft
    // an assistant round-trips does not grow keys nobody asked for.
    let second = &doc["test_cases"][1];
    assert!(second.get("comment").is_none(), "{second}");
    assert!(second.get("reviewer_notes").is_none(), "{second}");
    // Nor `id`. It used to be written as `null` for every case that did not
    // have one, which says exactly what saying nothing says - while making
    // a caller who passed 14 id-less cases read past an added key on all 14
    // to confirm their bulk edit did only what it claimed.
    assert!(second.get("id").is_none(), "{second}");
    // A case that HAS an id still carries it - that is the whole update
    // contract.
    assert_eq!(doc["test_cases"][0]["id"], serde_json::json!(77));
}

/// A problem an assistant found travels with its case: read from the
/// file, written back on export, kind validated, bare strings allowed.
#[test]
fn findings_round_trip_with_the_case_and_bad_kinds_warn() {
    let json = serde_json::json!({
        "test_cases": [{
            "title": "Cut-off closes the order",
            "automation_status": "Not Automated",
            "steps": [{ "action": "Open the page.", "expected": "Closed." }],
            "findings": [
                { "kind": "spec", "subject": "Orders.md 7.7", "title": "AC-3 contradicts the table", "detail": "Table says **closed**." },
                "Step 3 expects a toast the spec never mentions",
                { "kind": "vibes", "title": "Not a kind" }
            ]
        }]
    })
    .to_string();
    let path = tmp_path("findings.json");
    std::fs::write(&path, &json).unwrap();
    let (cases, warnings) = parse_file(&path).unwrap();
    std::fs::remove_file(&path).ok();
    assert_eq!(cases.len(), 1);
    let f = &cases[0].findings;
    assert_eq!(f.len(), 2, "the bad kind is dropped: {f:?}");
    assert_eq!(f[0].kind, "spec");
    assert_eq!(f[0].subject, "Orders.md 7.7");
    assert_eq!(f[0].detail, "Table says **closed**.");
    assert_eq!(f[1].kind, "test_case", "a bare string is a finding about the case itself");
    assert_eq!(f[1].title, "Step 3 expects a toast the spec never mentions");
    assert!(warnings.iter().any(|w| w.contains("findings") && w.contains("vibes")), "{warnings:?}");

    let out = v2_lib::import_parser::queue_to_json_string(&cases).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let back = &v["test_cases"][0]["findings"];
    assert_eq!(back.as_array().unwrap().len(), 2);
    assert_eq!(back[0]["subject"], "Orders.md 7.7");
    assert!(back[1].get("subject").is_none(), "empty subject is not written");

    // A case with no findings writes no key (the AI_INSTRUCTIONS text
    // itself mentions the word, so check the record, not the whole doc).
    let plain = TestCase { title: "P".into(), steps: cases[0].steps.clone(), automation_status: "Planned".into(), ..Default::default() };
    let out = v2_lib::import_parser::queue_to_json_string(&[plain]).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(v["test_cases"][0].get("findings").is_none(), "{out}");
}

/// The two sort orders ride the JSON like the notes do: written when
/// present, absent when not, read back exactly - and a junk value warns
/// instead of silently vanishing, because a file that LOOKS ordered and is
/// not would be sorted into nonsense with no explanation.
#[test]
fn sort_orders_round_trip_and_junk_values_warn() {
    let queue = vec![
        TestCase {
            title: "Ordered".into(),
            steps: vec![Step { action: "Do".into(), expected: String::new() }],
            automation_status: "Not Automated".into(),
            spec_order: Some(2),
            tester_order: Some(1),
            ..Default::default()
        },
        TestCase {
            title: "Unordered".into(),
            steps: vec![Step { action: "Go".into(), expected: String::new() }],
            automation_status: "Not Automated".into(),
            ..Default::default()
        },
    ];
    let path = tmp_path("orders.json");
    export_queue_to_json(&queue, &path).unwrap();

    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(doc["test_cases"][0]["spec_order"], serde_json::json!(2));
    assert_eq!(doc["test_cases"][0]["tester_order"], serde_json::json!(1));
    // Absent, not null: same shape rule as the notes and `id`.
    assert!(doc["test_cases"][1].get("spec_order").is_none());
    assert!(doc["test_cases"][1].get("tester_order").is_none());

    let (cases, warnings) = parse_file(&path).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(cases[0].spec_order, Some(2));
    assert_eq!(cases[0].tester_order, Some(1));
    assert_eq!(cases[1].spec_order, None);

    // Junk: a string, a zero, a negative - each is refused with a warning,
    // never read as an order.
    let junk = serde_json::json!({ "test_cases": [
        { "title": "A", "automation_status": "Not Automated",
          "steps": [{ "action": "x" }], "spec_order": "first", "tester_order": 0 },
    ]})
    .to_string();
    let path2 = tmp_path("orders-junk.json");
    std::fs::write(&path2, junk).unwrap();
    let (cases, warnings) = parse_file(&path2).unwrap();
    assert_eq!(cases[0].spec_order, None);
    assert_eq!(cases[0].tester_order, None);
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(warnings[0].contains("spec_order"), "{warnings:?}");
}

/// The browser view says what importing each case will DO - the same
/// UPDATE/NEW distinction the queue rows carry. Draft pages only: cases
/// already living in Azure DevOps are neither.
#[test]
fn the_draft_page_badges_each_case_new_or_update() {
    let queue = vec![
        TestCase {
            title: "Creates fresh".into(),
            steps: vec![Step { action: "x".into(), expected: String::new() }],
            automation_status: "Not Automated".into(),
            ..Default::default()
        },
        TestCase {
            title: "Writes over 77".into(),
            steps: vec![Step { action: "x".into(), expected: String::new() }],
            automation_status: "Not Automated".into(),
            update_id: Some(77),
            ..Default::default()
        },
    ];
    let path = tmp_path("op-badges.html");
    v2_lib::import_parser::export_queue_to_html(&queue, &path, "", None, &Default::default())
        .unwrap();
    let html = std::fs::read_to_string(&path).unwrap();
    assert!(html.contains("op-new"), "a case without an id creates: {html}");
    assert!(html.contains("op-update"), "a case with an id updates");
    // The UPDATE chip sits beside the id it will write over.
    assert!(html.contains("<span class='chip op-update'>UPDATE</span><span class='wid'>#77</span>"));

    // The page of EXISTING cases carries no operation chips.
    let note_ctx = v2_lib::import_parser::NoteCtx {
        port: 1,
        token: "t".into(),
        org: "acme".into(),
        notes: Default::default(),
    };
    let path2 = tmp_path("op-badges-ado.html");
    v2_lib::import_parser::export_queue_to_html(
        &queue,
        &path2,
        "",
        Some(v2_lib::import_parser::CommentCtx::Ado(&note_ctx)),
        &Default::default(),
    )
    .unwrap();
    let ado = std::fs::read_to_string(&path2).unwrap();
    // Assert on the chip MARKUP, not the bare class names - the shared
    // stylesheet defines .op-new/.op-update on every page, including this
    // one where no chip is ever rendered.
    assert!(
        !ado.contains("<span class='chip op-new'>") && !ado.contains("<span class='chip op-update'>"),
        "existing cases are neither"
    );
}

/// A bulk edit owns the CASES, not the file: writing them back must leave
/// every other top-level key - the general comments, keys this app has
/// never heard of - exactly as the file had them.
#[test]
fn merging_cases_back_preserves_the_rest_of_the_file() {
    let original = serde_json::json!({
        "format": "azure-devops-test-cases",
        "comments": { "general": "Reviewed by QA on Friday" },
        "somebody_elses_key": [1, 2, 3],
        "test_cases": [
            { "title": "Old title", "automation_status": "Not Automated",
              "steps": [{ "action": "x" }] },
        ],
    })
    .to_string();
    let edited = vec![TestCase {
        title: "Renamed by bulk edit".into(),
        steps: vec![Step { action: "x".into(), expected: String::new() }],
        automation_status: "Not Automated".into(),
        tags: "smoke".into(),
        ..Default::default()
    }];

    let out = v2_lib::import_parser::merge_cases_into_draft(&original, &edited).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(doc["test_cases"][0]["title"], "Renamed by bulk edit");
    assert_eq!(doc["test_cases"][0]["tags"], "smoke");
    assert_eq!(doc["comments"]["general"], "Reviewed by QA on Friday", "{out}");
    assert_eq!(doc["somebody_elses_key"], serde_json::json!([1, 2, 3]));

    // And the result re-imports: the write-back must never produce a file
    // the importer itself would refuse.
    let path = tmp_path("merged.json");
    std::fs::write(&path, &out).unwrap();
    let (cases, warnings) = parse_file(&path).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(cases[0].title, "Renamed by bulk edit");
}

/// A bare-array draft has nothing to preserve; it comes back in the
/// standard wrapper shape, which is the repair rather than a loss.
#[test]
fn merging_into_a_bare_array_produces_the_wrapper_shape() {
    let edited = vec![TestCase {
        title: "A".into(),
        steps: vec![Step { action: "x".into(), expected: String::new() }],
        automation_status: "Not Automated".into(),
        ..Default::default()
    }];
    let out =
        v2_lib::import_parser::merge_cases_into_draft("[{\"title\":\"A\"}]", &edited).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(doc["format"], "azure-devops-test-cases");
    assert_eq!(doc["test_cases"][0]["title"], "A");
}

/// A reviewer who wants the notes out of the way is usually halfway down
/// a long page when they decide that, so the control lives in the sticky
/// search bar - one that has scrolled away is no control.
#[test]
fn the_report_can_hide_its_reviewer_notes() {
    let with_notes = vec![TestCase {
        title: "Has notes".into(),
        steps: vec![Step { action: "Do".into(), expected: "Done".into() }],
        automation_status: "Not Automated".into(),
        reviewer_notes: "Spec: Step10.md 7.7".into(),
        spec_order: None,
        tester_order: None,
        ..Default::default()
    }];
    let path = tmp_path("notes-toggle.html");
    export_queue_to_html(&with_notes, &path, "", None, &Default::default()).unwrap();
    let html = std::fs::read_to_string(&path).unwrap();

    // The button is in the sticky bar, not beside a case.
    let bar = html.split("class='searchbar'").nth(1).expect("search bar");
    let bar = bar.split("</div>").next().unwrap();
    assert!(bar.contains("id='tc-notes'"), "toggle belongs in the sticky bar: {bar}");
    // Pressed state is exposed, so it is a real toggle to a screen reader.
    assert!(html.contains("aria-pressed="), "{html}");
    // And one rule does the hiding, rather than walking the DOM - as a
    // grid-row collapse, so the notes close with a motion instead of
    // blinking out of existence.
    assert!(html.contains("body.notes-off .rev-wrap"), "{html}");
    assert!(html.contains("grid-template-rows: 0fr"), "the hide must animate: {html}");

    // Each note also carries its own x, which collapses just that case's
    // notes - and the global Show resurrects them all, so a note cannot be
    // lost to a forgotten click.
    assert!(html.contains("class='rev-close'"), "{html}");
    assert!(html.contains("rev-closed"), "{html}");
    assert!(
        html.contains("aria-label='Hide these reviewer notes'"),
        "the x needs a name for screen readers: {html}"
    );

    // No notes anywhere: no button. A control that hides nothing is just
    // another thing to read.
    let without = vec![TestCase {
        title: "No notes".into(),
        steps: vec![Step { action: "Do".into(), expected: "Done".into() }],
        automation_status: "Not Automated".into(),
        ..Default::default()
    }];
    let path2 = tmp_path("notes-toggle-none.html");
    export_queue_to_html(&without, &path2, "", None, &Default::default()).unwrap();
    let plain = std::fs::read_to_string(&path2).unwrap();
    assert!(!plain.contains("id='tc-notes'"), "nothing to hide, so no button");
}
/// The reviewer-facing half: notes reach the browser page as RENDERED
/// markdown, in their own panel, and the page still carries the ordinary
/// comment boxes alongside them.
#[test]
fn reviewer_notes_render_as_markdown_in_the_review_page() {
    let queue = vec![TestCase {
        title: "Login".into(),
        steps: vec![Step { action: "Open".into(), expected: "Shown".into() }],
        automation_status: "Planned".into(),
        reviewer_notes: "## Where this came from\n\n\
                         Covers [AC-4](https://spec.invalid/auth#ac4).\n\n\
                         - `POST /session` only\n- SSO is **out of scope**\n\n\
                         <img src=x onerror=alert(1)>"
            .into(),
        spec_order: None,
        tester_order: None,
        ..Default::default()
    }];
    let path = tmp_path("reviewer-notes.html");
    v2_lib::import_parser::export_queue_to_html(&queue, &path, "", None, &Default::default())
        .unwrap();
    let html = std::fs::read_to_string(&path).unwrap();

    // The label opens the summary; the per-note close button lives inside
    // it too, so an exact "<summary>...</summary>" match would be asserting
    // a layout this page deliberately does not have.
    assert!(
        html.contains("<summary>Reviewer notes"),
        "the panel is labelled"
    );
    assert!(
        html.contains("class='rev-close'"),
        "each note carries its own close control"
    );
    assert!(html.contains("<details class='rev' open>"), "and open by default");
    assert!(html.contains("<h5>Where this came from</h5>"), "markdown headings render: {html}");
    assert!(html.contains(r#"<a href="https://spec.invalid/auth#ac4""#), "links render");
    assert!(html.contains("<strong>out of scope</strong>"));
    assert!(html.contains("<code>POST /session</code>"));
    // The note is remote-authored text in a page opened from a temp file.
    assert!(!html.contains("onerror"), "HTML in a note must not survive: {html}");

    // A case with no notes gets no panel at all - an empty labelled box on
    // every card would be worse than nothing.
    let bare = vec![TestCase {
        title: "No notes".into(),
        steps: vec![Step { action: "Open".into(), expected: "".into() }],
        automation_status: "Planned".into(),
        ..Default::default()
    }];
    let path2 = tmp_path("reviewer-notes-none.html");
    v2_lib::import_parser::export_queue_to_html(&bare, &path2, "", None, &Default::default())
        .unwrap();
    // Checked against the MARKUP, not the words: "Reviewer notes" also
    // appears in the stylesheet's own comment, which every page carries.
    assert!(!std::fs::read_to_string(&path2).unwrap().contains("<details class='rev'"));
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
        reviewer_notes: String::new(),
        spec_order: None,
        tester_order: None,
        findings: vec![],
    }];
    let path = tmp_path("report.html");
    v2_lib::import_parser::export_queue_to_html(&queue, &path, "PBI #7", None, &Default::default())
        .unwrap();
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

/// The page opens in a browser, so it wears the app's theme - and carries
/// the other scheme plus a switch, because the tab outlives the theme the
/// app was in when it opened.
#[test]
fn the_test_case_page_is_themed_and_can_be_flipped() {
    let queue = vec![v2_lib::model::TestCase {
        title: "Login".into(),
        steps: vec![],
        tags: String::new(),
        automation_status: "Planned".into(),
        module_value: String::new(),
        preconditions: String::new(),
        update_id: None,
        comment: String::new(),
        reviewer_notes: String::new(),
        spec_order: None,
        tester_order: None,
        findings: vec![],
    }];
    // Spelled out rather than `..Default::default()`: that default is the
    // LIGHT palette, so a partial dark fixture inherits #1f2530 text onto
    // a black page - a page this test would then have called themed.
    let oled = v2_lib::webtheme::ReportPalette {
        bg: "#000000".into(),
        surface: "#0b0b0d".into(),
        surface_2: "#141418".into(),
        text: "#e5e7eb".into(),
        muted: "#9ca3af".into(),
        faint: "#6b7280".into(),
        border: "#27272a".into(),
        accent: "#22c55e".into(),
        success: "#22c55e".into(),
        danger: "#ef4444".into(),
        warning: "#f59e0b".into(),
        dark: true,
    };
    let path = tmp_path("report-themed.html");
    v2_lib::import_parser::export_queue_to_html(
        &queue,
        &path,
        "",
        None,
        &v2_lib::webtheme::PagePalette {
            light: Default::default(),
            dark: oled,
            dark_first: true,
        },
    )
    .unwrap();
    let html = std::fs::read_to_string(&path).unwrap();

    assert!(html.contains(r#"data-scheme="dark""#), "opens in the app's scheme");
    assert!(html.contains("--bg: #000000"), "the app's palette reaches the page");
    // Text and background have to come from the SAME scheme, or the page
    // opens as light-grey text on black.
    let dark_vars = html.split(r#":root[data-scheme="dark"]"#).nth(1).unwrap();
    assert!(dark_vars.contains("--text: #e5e7eb"), "dark text with the dark background");
    assert!(html.contains(r#":root[data-scheme="dark"]"#), "the other scheme ships too");
    assert!(html.contains(r#"id="scheme-switch""#));
    // The stylesheet has to CONSUME the variables. It was written in fixed
    // hex, and a single leftover literal is an element that stays light on
    // a black page - so the check is that none survive in the rules.
    assert!(html.contains("background: var(--bg)"));
    let css = html.split("</style>").next().unwrap();
    let rules = css.split(r#":root[data-scheme="dark"]"#).nth(1).unwrap();
    for leftover in ["#f3f5f8", "#dde3ec", "#2a7ab8", "#44506a", "#c9d3e2"] {
        assert!(
            !rules.contains(leftover),
            "hardcoded {leftover} left in the stylesheet - it will not follow the theme"
        );
    }
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
    let ctx = v2_lib::import_parser::NoteCtx {
        port: 4711,
        token: "secret".into(),
        org: "acme".into(),
        notes,
    };

    let path = tmp_path("report-notes.html");
    v2_lib::import_parser::export_queue_to_html(
        &queue,
        &path,
        "",
        Some(v2_lib::import_parser::CommentCtx::Ado(&ctx)),
        &Default::default(),
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

/// An id that is not a work item id must NEVER become one. `as i32`
/// saturates, so "99999999999" used to arrive as 2147483647 and "12.7" as
/// 12 - both silently retargeting the update at a real work item nobody
/// asked for. Refusing means the case is created new, which is the
/// recoverable half of being wrong.
#[test]
fn an_id_out_of_range_or_fractional_is_refused_not_rounded() {
    let parse = |id: &str| {
        let path = tmp_path(&format!("id-{}.json", id.replace(['.', '-'], "_")));
        std::fs::write(
            &path,
            format!(
                r#"[{{"id": "{id}", "title": "Retarget me", "steps": [{{"action":"a","expected":"b"}}]}}]"#
            ),
        )
        .unwrap();
        let out = parse_file(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        out
    };

    for bad in ["99999999999", "2147483648", "12.7", "-5", "0", "1e40", "abc"] {
        let (cases, warnings) = parse(bad);
        assert_eq!(cases.len(), 1, "the case itself must survive: {bad}");
        assert_eq!(
            cases[0].update_id, None,
            "id '{bad}' was accepted as a work item to update"
        );
        assert!(
            warnings.iter().any(|w| w.contains(bad)),
            "id '{bad}' was dropped without a warning; got {warnings:?}"
        );
    }

    // The tolerated shape - an integral id a JSON writer rendered as a
    // float - still works, and so does a plain one.
    for (good, want) in [("123.0", 123), ("123", 123), ("2147483647", i32::MAX)] {
        let (cases, _) = parse(good);
        assert_eq!(cases[0].update_id, Some(want), "id '{good}' was refused");
    }
}

/// An AI-written draft happily puts a line break inside a step. Azure
/// DevOps stores steps in a single-line HTML field and the app's own step
/// editor is a one-line input, so that layout was never going to survive -
/// it just used to disappear without anyone saying so, and the text came
/// back from Azure DevOps looking edited. Fold it here, and say so.
#[test]
fn a_step_that_spans_lines_is_folded_and_the_author_is_told() {
    let path = tmp_path("wrapped-steps.json");
    let json = serde_json::json!([{
        "title": "Multi-line draft",
        "steps": [
            { "action": "Open Settings\nthen the Payments tab", "expected": "The tab opens" },
            { "action": "Save", "expected": "A toast appears:\r\n  'Saved'" },
            { "action": "Close", "expected": "It closes" }
        ]
    }]);
    std::fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();
    let (cases, warnings) = parse_file(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    assert_eq!(cases[0].steps[0].action, "Open Settings then the Payments tab");
    assert_eq!(cases[0].steps[1].expected, "A toast appears: 'Saved'");
    // The untouched step keeps its exact text - folding is not a reformat.
    assert_eq!(cases[0].steps[2].expected, "It closes");

    // One warning for the case, not one per offending step.
    let folded: Vec<_> = warnings.iter().filter(|w| w.contains("line breaks")).collect();
    assert_eq!(folded.len(), 1, "got {warnings:?}");
    assert!(folded[0].contains("Multi-line draft"));
}
