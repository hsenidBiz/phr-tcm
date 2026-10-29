//! Subreport internals editing — ops inside an embedded subreport, through the
//! lift-into-a-mini-report seam (subedit.rs; validation-log §29). The fixture's StageWise
//! subreport (7 embedded subreports in this file) carries 30 objects, boxes, and 7 formulas
//! — a full editing surface.

use rpt_core::edit::{apply_edits, EditOp, ObjSel};
use rpt_core::plan::apply_plan_json;
use rpt_core::subedit::subreport_ir;
use rpt_core::{open, open_bytes, reencode};

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{}", env!("CARGO_MANIFEST_DIR"), name)
}

/// The subreport's own IR: objects locate with zero warnings, ids are scoped to it.
#[test]
fn subreport_ir_locates_objects() {
    let rpt = open(&fixture("PMSV10_IndDetailedEvalSS.rpt")).expect("open");
    let ir = subreport_ir(&rpt, "StageWise").expect("sub IR");
    assert_eq!(ir.objects.len(), 30, "the StageWise object count");
    assert!(ir.warnings.is_empty(), "{:?}", ir.warnings);
    // Main-scope IR object ids and sub-scope ids are different spaces; the sub IR is a
    // complete projection of its own.
    assert!(!ir.areas.is_empty());
}

/// setText inside the subreport through the native op vocabulary.
#[test]
fn set_text_inside_subreport() {
    let ir_target = {
        let rpt = open(&fixture("PMSV10_IndDetailedEvalSS.rpt")).unwrap();
        let ir = subreport_ir(&rpt, "StageWise").unwrap();
        ir.objects
            .iter()
            .find(|o| o.text.as_ref().is_some_and(|t| t.editable))
            .expect("an editable text object")
            .id
            .clone()
    };
    let out = apply_edits(
        &fixture("PMSV10_IndDetailedEvalSS.rpt"),
        &[EditOp::InSubreport {
            subreport: "StageWise".into(),
            ops: vec![EditOp::SetText { object: ObjSel { id: Some(ir_target.clone()), name: None, index: None }, text: "SUB EDIT".into() }],
        }],
    )
    .expect("apply");
    let rpt = open_bytes(&out).unwrap();
    let ir = subreport_ir(&rpt, "StageWise").unwrap();
    let obj = ir.objects.iter().find(|o| o.id == ir_target).unwrap();
    assert_eq!(obj.text.as_ref().unwrap().display, "SUB EDIT");
    // Parent gates: validator + round-trip.
    let v = rpt_core::validate::validate(&rpt);
    assert!(v.errors().next().is_none(), "{:?}", v.errors().collect::<Vec<_>>());
    let rt = reencode(&rpt).unwrap();
    let again = open_bytes(&rt).unwrap();
    assert_eq!(
        rpt.stream(&rpt_core::rpt_reader::StreamId::Contents).unwrap().serialize_tree(),
        again.stream(&rpt_core::rpt_reader::StreamId::Contents).unwrap().serialize_tree()
    );
}

/// The plan vocabulary routes any action into a subreport with a `subreport` field — the
/// same JSON the main scope takes, one extra key. addText inside the subreport uses the
/// sub's own donors (it carries text/box objects).
#[test]
fn plan_add_text_inside_subreport() {
    let plan = r#"{"planVersion":1,"operations":[
        {"action":"addText","subreport":"StageWise","section":"ReportHeaderSection1",
         "newName":"SubAdded","leftTwips":200,"topTwips":100,"widthTwips":3000,"heightTwips":300,
         "text":"added inside"}]}"#;
    let (out, _) = apply_plan_json(&fixture("PMSV10_IndDetailedEvalSS.rpt"), plan).expect("apply");
    let rpt = open_bytes(&out).unwrap();
    let ir = subreport_ir(&rpt, "StageWise").unwrap();
    assert!(ir.objects.iter().any(|o| o.name == "SubAdded"), "the object was added inside the subreport");
    let v = rpt_core::validate::validate(&rpt);
    assert!(v.errors().next().is_none());
}

/// setFormula works inside the subreport (StageWise carries its own formulas).
#[test]
fn set_formula_inside_subreport() {
    let rpt = open(&fixture("PMSV10_IndDetailedEvalSS.rpt")).unwrap();
    let ir = subreport_ir(&rpt, "StageWise").unwrap();
    let sub_report = rpt.report().subreports.iter().find(|s| s.name == "StageWise").unwrap();
    let formula_name = sub_report
        .report
        .data_definition
        .field_definitions
        .iter()
        .find(|f| matches!(f.kind, rpt_core::rpt_reader::model::FieldKindData::Formula(_)))
        .map(|f| f.name.clone())
        .expect("a formula inside the subreport");
    let _ = ir;
    let out = apply_edits(
        &fixture("PMSV10_IndDetailedEvalSS.rpt"),
        &[EditOp::InSubreport {
            subreport: "StageWise".into(),
            ops: vec![EditOp::SetFormula { formula: formula_name.clone(), source: "\"sub-fx\"".into() }],
        }],
    )
    .expect("apply");
    let rpt = open_bytes(&out).unwrap();
    let sub = rpt.report().subreports.iter().find(|s| s.name == "StageWise").unwrap();
    let f = sub
        .report
        .data_definition
        .field_definitions
        .iter()
        .find(|f| f.name == formula_name)
        .unwrap();
    match &f.kind {
        rpt_core::rpt_reader::model::FieldKindData::Formula(fx) => {
            assert_eq!(fx.text.0, "\"sub-fx\"");
        }
        _ => panic!("not a formula"),
    }
    let v = rpt_core::validate::validate(&rpt);
    assert!(v.errors().next().is_none());
}

/// Nesting is refused with the clear message; an unknown subreport name is refused.
#[test]
fn subreport_refusals() {
    let nested = vec![EditOp::InSubreport {
        subreport: "StageWise".into(),
        ops: vec![EditOp::InSubreport { subreport: "Inner".into(), ops: vec![] }],
    }];
    let e = apply_edits(&fixture("PMSV10_IndDetailedEvalSS.rpt"), &nested).unwrap_err();
    assert!(e.contains("nested"), "`{e}`");
    let unknown = vec![EditOp::InSubreport { subreport: "Nope".into(), ops: vec![] }];
    let e = apply_edits(&fixture("PMSV10_IndDetailedEvalSS.rpt"), &unknown).unwrap_err();
    assert!(e.contains("Nope"), "`{e}`");
}

/// Section visibility formulas — every subreport of this fixture carries one on its detail
/// section (the probe found all six). Setting it rewrites the indexed formula body; the
/// read-back goes through the PARENT's subreport model (reserved name Section_Visibility).
#[test]
fn set_section_visibility_inside_subreport() {
    let plan = r#"{"planVersion":1,"operations":[
        {"action":"setSectionVisibility","subreport":"StageWise","section":"DetailSection1",
         "source":"{sp_perf_detailed_eval_stages;1.stage_name} <> \"\""}]}"#;
    let (out, _) = apply_plan_json(&fixture("PMSV10_IndDetailedEvalSS.rpt"), plan).expect("apply");
    let rpt = open_bytes(&out).unwrap();
    let sub = rpt.report().subreports.iter().find(|s| s.name == "StageWise").unwrap();
    let conds: Vec<&(String, String)> = sub
        .report
        .report_definition
        .areas
        .iter()
        .flat_map(|a| a.sections.iter())
        .flat_map(|s| s.condition_formulas.iter())
        .collect();
    let vis = conds
        .iter()
        .find(|(n, _)| n == "Section_Visibility")
        .expect("the visibility condition survives");
    assert_eq!(vis.1, "{sp_perf_detailed_eval_stages;1.stage_name} <> \"\"");
    let v = rpt_core::validate::validate(&rpt);
    assert!(v.errors().next().is_none());
    let rt = reencode(&rpt).unwrap();
    let again = open_bytes(&rt).unwrap();
    assert_eq!(
        rpt.stream(&rpt_core::rpt_reader::StreamId::Contents).unwrap().serialize_tree(),
        again.stream(&rpt_core::rpt_reader::StreamId::Contents).unwrap().serialize_tree()
    );

    // The other subreports' formulas are untouched.
    let cmp = rpt.report().subreports.iter().find(|s| s.name == "RatingCmp").unwrap();
    let cmp_vis = cmp
        .report
        .report_definition
        .areas
        .iter()
        .flat_map(|a| a.sections.iter())
        .flat_map(|s| s.condition_formulas.iter())
        .find(|(n, _)| n == "Section_Visibility")
        .unwrap();
    assert!(cmp_vis.1.contains("pmsEmp"), "RatingCmp's own formula untouched");
}

/// A section with no visibility formula refuses with the clear message.
#[test]
fn section_visibility_refusals() {
    let e = apply_plan_json(
        &fixture("PMSV10_GoalAlignCascade.rpt"),
        r#"{"planVersion":1,"operations":[
            {"action":"setSectionVisibility","section":"ReportHeaderSection1","source":"True"}]}"#,
    )
    .unwrap_err();
    assert!(e.contains("no Section_Visibility formula"), "`{e}`");
}
