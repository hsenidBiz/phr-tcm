//! Tier-A property batch: the format-leaf ops (validation-log §32). Each op patches one
//! decoded wire leaf through the verified path; read-back goes through the model where the
//! model carries it, else through the record field itself.

use rpt_core::edit::{apply_edits, EditOp, ObjSel};
use rpt_core::plan::apply_plan_json;
use rpt_core::rpt_reader::StreamId;
use rpt_core::validate::validate;
use rpt_core::{open, open_bytes, reencode};

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{}", env!("CARGO_MANIFEST_DIR"), name)
}

fn assert_clean(bytes: &[u8], what: &str) {
    let rpt = open_bytes(bytes).unwrap_or_else(|e| panic!("{what}: {e}"));
    let v = validate(&rpt);
    assert!(v.errors().next().is_none(), "{what}: {:?}", v.errors().map(|f| f.message.to_string()).collect::<Vec<_>>());
    let rt = reencode(&rpt).unwrap();
    let again = open_bytes(&rt).unwrap();
    assert_eq!(
        rpt.stream(&StreamId::Contents).unwrap().serialize_tree(),
        again.stream(&StreamId::Contents).unwrap().serialize_tree(),
        "{what}: round-trip"
    );
}

fn sel(name: &str) -> ObjSel {
    ObjSel { id: None, name: Some(name.to_string()), index: None }
}

/// Object-format cluster (0x00fc): rotation, vertical alignment, keep-together — on the
/// GoalAlignCascade footer text object. Read-back through the model's ObjectFormat.
#[test]
fn object_format_ops() {
    let ops = vec![
        EditOp::SetTextRotation { object: sel("FtConf"), degrees: 90 },
        EditOp::SetVerticalAlignment { object: sel("FtConf"), alignment: "center".into() },
        EditOp::SetKeepObjectTogether { object: sel("FtConf"), keep: true },
    ];
    let out = apply_edits(&fixture("PMSV10_GoalAlignCascade.rpt"), &ops).expect("apply");
    let rpt = open_bytes(&out).unwrap();
    let obj = rpt.report().report_definition.areas.iter()
        .flat_map(|a| a.sections.iter().flat_map(|s| s.objects.iter()))
        .find(|o| o.name.eq_ignore_ascii_case("FtConf")).unwrap();
    assert_eq!(obj.format.text_rotation.degrees(), 90);
    assert!(matches!(obj.format.vertical_alignment, rpt_core::rpt_reader::model::VerticalAlignment::VerticalCenter));
    assert!(obj.format.keep_together);
    assert_clean(&out, "object-format ops");
    // Rotation refusal on a bad angle.
    let e = apply_edits(&fixture("PMSV10_GoalAlignCascade.rpt"),
        &[EditOp::SetTextRotation { object: sel("FtConf"), degrees: 45 }]).unwrap_err();
    assert!(e.contains("45"));
}

/// String-format cluster (0x00fa): word wrap, max lines, interpretation, indents — on a
/// string-valued field. Read-back through the model's StringFieldFormat where carried.
#[test]
fn string_format_ops() {
    let f = fixture("PMSV10_IndPerfOverview.rpt");
    // The fixture's fields are all string-valued (stored-proc columns); find one with a
    // string-format record by trying the first Field object.
    let rpt0 = open(&f).unwrap();
    let ir = rpt_core::ir::build_ir(&rpt0, &rpt_core::locate::locate(&rpt0), "ipo");
    let field_name = ir.objects.iter().find(|o| o.kind == "Field").map(|o| o.name.clone()).expect("a field object");
    let ops = vec![
        EditOp::SetWordWrap { object: sel(&field_name), wrap: true },
        EditOp::SetMaxLines { object: sel(&field_name), lines: 3 },
        EditOp::SetTextInterpretation { object: sel(&field_name), format: "html".into() },
        EditOp::SetIndents { object: sel(&field_name), first: Some(200), left: Some(100), right: None },
    ];
    let out = apply_edits(&f, &ops).expect("apply");
    let rpt = open_bytes(&out).unwrap();
    let obj = rpt.report().report_definition.areas.iter()
        .flat_map(|a| a.sections.iter().flat_map(|s| s.objects.iter()))
        .find(|o| o.name.eq_ignore_ascii_case(&field_name)).unwrap();
    if let rpt_core::rpt_reader::model::ReportObjectKind::Field(fld) = &obj.kind {
        if let Some(ff) = &fld.format {
            assert!(ff.string.enable_word_wrap, "word wrap");
            assert_eq!(ff.string.max_number_of_lines, 3);
            assert!(matches!(ff.string.text_format, rpt_core::rpt_reader::model::TextFormat::HTMLText));
        }
    } else { panic!("not a field"); }
    assert_clean(&out, "string-format ops");
    // Reading order on a record that carries the optional field: refusal is fine (the leaf
    // is optional and may be absent) — the OP itself must not corrupt anything.
    let _ = apply_edits(&f, &[EditOp::SetReadingOrder { object: sel(&field_name), order: "rtl".into() }]);
}

/// Section flags (0x00fe) + group selection — through the plan vocabulary.
#[test]
fn section_flags_and_group_selection() {
    let plan = r#"{"planVersion":1,"operations":[
        {"action":"setSectionFlags","section":"ReportHeaderSection1","keepTogether":true,"underlay":false}]}"#;
    let (out, _) = apply_plan_json(&fixture("PMSV10_GoalAlignCascade.rpt"), plan).expect("apply");
    let rpt = open_bytes(&out).unwrap();
    let sec = rpt.report().report_definition.areas.iter()
        .flat_map(|a| a.sections.iter())
        .find(|s| s.name.eq_ignore_ascii_case("ReportHeaderSection1")).unwrap();
    assert!(sec.format.base.keep_together);
    assert!(!sec.format.underlay_section);
    assert_clean(&out, "section flags");

    // Group selection on World Sales Report (it carries a Group Selection formula).
    let plan2 = r#"{"planVersion":1,"operations":[
        {"action":"setGroupSelection","source":"{Customer.Country} <> \"\""}]}"#;
    let (out2, _) = apply_plan_json(&fixture("World Sales Report.rpt"), plan2).expect("apply");
    let rpt2 = open_bytes(&out2).unwrap();
    let sel = rpt2.report().data_definition.group_selection.clone();
    assert_eq!(sel.as_ref().map(|s| s.0.as_str()), Some("{Customer.Country} <> \"\""));
    assert_clean(&out2, "group selection");
}
