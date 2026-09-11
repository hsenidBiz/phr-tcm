//! The browser report carries the open findings for the project in a
//! section of its own - never inside a case's notes or comment.

use v2_lib::findings::Finding;
use v2_lib::import_parser::export_queue_to_html;
use v2_lib::model::TestCase;
use v2_lib::steps_xml::Step;

fn case(title: &str) -> TestCase {
    TestCase {
        title: title.into(),
        steps: vec![Step { action: "Open".into(), expected: "Shown".into() }],
        ..Default::default()
    }
}

fn finding(kind: &str, title: &str, status: &str) -> Finding {
    Finding {
        id: format!("id-{title}"),
        org: "acme".into(),
        project: "Web".into(),
        kind: kind.into(),
        subject: "155170".into(),
        title: title.into(),
        detail: "The table says **closed**.".into(),
        created_at: "2026-09-11T10:00:00Z".into(),
        status: status.into(),
    }
}

fn render(findings: &[Finding]) -> String {
    let path = std::env::temp_dir().join(format!("tcm-findings-report-{}-{}.html", std::process::id(), findings.len()));
    export_queue_to_html(&[case("A")], path.to_str().unwrap(), "Sub", None, &Default::default(), findings).unwrap();
    let html = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    html
}

#[test]
fn open_findings_get_their_own_section_with_kind_and_markdown() {
    let html = render(&[finding("spec", "AC-3 contradicts the table", "open"), finding("code", "Resolved one", "resolved")]);
    let section = html.split("<section class='findings'").nth(1).expect("a findings section");
    assert!(section.contains("AI Findings"));
    assert!(section.contains("AC-3 contradicts the table"));
    assert!(section.contains("<strong>closed</strong>"), "detail is markdown: {section}");
    assert!(section.contains("Spec"));
    assert!(!section.contains("Resolved one"), "only open findings: {section}");
    // The section is its own block, not inside the case card.
    let case_at = html.find("<div class='case'>").unwrap();
    let findings_at = html.find("<section class='findings'").unwrap();
    assert!(findings_at < case_at, "findings come before the cases");
}

#[test]
fn no_open_findings_means_no_section() {
    let html = render(&[finding("code", "Done", "resolved")]);
    assert!(!html.contains("<section class='findings'"));
    let html = render(&[]);
    assert!(!html.contains("AI Findings"));
}
