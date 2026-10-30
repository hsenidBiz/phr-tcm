//! The "what changed" diff: exact intended changes show as JSON-path lines; identical
//! models diff clean.

use rpt_core::diff::diff_bytes;
use rpt_core::open_bytes;
use rpt_core::plan::apply_plan_json;

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{}", env!("CARGO_MANIFEST_DIR"), name)
}

#[test]
fn diff_names_exactly_the_intended_change() {
    let plan = r#"{"planVersion":1,"operations":[
        {"action":"setFont","target":"FtConf","fontName":"Tahoma"}]}"#;
    let (edited, _) = apply_plan_json(&fixture("PMSV10_GoalAlignCascade.rpt"), plan).expect("edit");
    let base = std::fs::read(fixture("PMSV10_GoalAlignCascade.rpt")).unwrap();
    let lines = diff_bytes(&base, &edited).expect("diff");
    assert_eq!(lines.len(), 2, "the mirrored object+run font: {lines:?}");
    assert!(lines.iter().all(|l| l.contains("font.name") && l.contains("Tahoma")));
    assert!(lines[0].starts_with("$.report_definition."), "`{}`", lines[0]);
    assert!(lines[0].contains("Tahoma"), "`{}`", lines[0]);

    // A byte-identical model diffs clean.
    assert!(diff_bytes(&base, &base).expect("self diff").is_empty());
    // Re-encoding does not change the model: clean diff through the writer.
    let rt = open_bytes(&base).unwrap().reencode().unwrap();
    assert!(diff_bytes(&base, &rt).expect("reencode diff").is_empty());
}
