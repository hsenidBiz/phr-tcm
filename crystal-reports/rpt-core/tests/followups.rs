//! Follow-up batch tests: setFormula / setRecordSelection, the Group #N Order mint, Pm-
//! parameter creation, and addRunningTotal + `{#name}` placement. Each asserts the model
//! read-back, the validator's verdict, and a clean record round-trip.

use rpt_core::edit::apply_edits;
use rpt_core::plan::apply_plan_json;
use rpt_core::rpt_reader::StreamId;
use rpt_core::validate::validate;
use rpt_core::EditOp;
use rpt_core::{open, open_bytes, reencode};

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{}", env!("CARGO_MANIFEST_DIR"), name)
}

fn assert_clean(bytes: &[u8], what: &str) {
    let rpt = open_bytes(bytes).unwrap_or_else(|e| panic!("{what}: {e}"));
    let reopened = reencode(&rpt).unwrap_or_else(|e| panic!("{what} reencode: {e}"));
    let again = open_bytes(&reopened).unwrap_or_else(|e| panic!("{what} reopen: {e}"));
    assert_eq!(
        rpt.stream(&StreamId::Contents).unwrap().serialize_tree(),
        again.stream(&StreamId::Contents).unwrap().serialize_tree(),
        "{what}: record bytes drifted across the round-trip"
    );
    let v = validate(&rpt);
    assert!(
        v.errors().next().is_none(),
        "{what}: validator errors: {:?}",
        v.errors().map(|f| f.message.to_string()).collect::<Vec<_>>()
    );
}

/* ---------- setFormula / setRecordSelection ---------- */

/// setFormula replaces exactly the named formula's text and nothing else.
#[test]
fn set_formula_rewrites_text_only() {
    let ops = vec![EditOp::SetFormula {
        formula: "pmsEmp".to_string(),
        source: "\"E\" + {sheet.emp_display_number}".to_string(),
    }];
    let out = apply_edits(&fixture("PMSV10_IndDetailedEval.rpt"), &ops).expect("apply");
    let rpt = open_bytes(&out).unwrap();
    let f = rpt
        .report()
        .data_definition
        .field_definitions
        .iter()
        .find(|f| f.name == "pmsEmp")
        .expect("formula def");
    let text = match &f.kind {
        rpt_reader::model::FieldKindData::Formula(fx) => fx.text.0.clone(),
        _ => String::new(),
    };
    assert_eq!(text, "\"E\" + {sheet.emp_display_number}");
    assert_clean(&out, "setFormula");
}

/// setFormula refuses an unresolvable reference, naming it.
#[test]
fn set_formula_refuses_unknown_refs() {
    let ops = vec![EditOp::SetFormula {
        formula: "pmsEmp".to_string(),
        source: "{no_such_table.no_such_field} + 1".to_string(),
    }];
    let e = apply_edits(&fixture("PMSV10_IndDetailedEval.rpt"), &ops).unwrap_err();
    assert!(e.contains("no_such_field"), "`{e}` names the reference");
}

/// setRecordSelection rewrites the stored selection; the model reads the new formula back.
#[test]
fn set_record_selection_round_trips() {
    let ops = vec![EditOp::SetRecordSelection {
        source: "{sheet.goal_id} > 0".to_string(),
    }];
    let out = apply_edits(&fixture("PMSV10_GoalAlignCascade.rpt"), &ops).expect("apply");
    let rpt = open_bytes(&out).unwrap();
    let sel = rpt.report().data_definition.record_selection.clone();
    assert_eq!(sel.as_ref().map(|s| s.0.as_str()), Some("{sheet.goal_id} > 0"));
    assert_clean(&out, "setRecordSelection");
}

/* ---------- Group #N Order mint ---------- */

/// addGroup mints the new group's `Group #N Order` formula and re-numbers the clone's
/// identity fields — no more donor-number collisions.
#[test]
fn add_group_mints_order_formula() {
    let plan = r#"{"planVersion":1,"operations":[
        {"action":"addGroup","fieldRef":"{sheet.goal_name}","direction":"ascending"}]}"#;
    let (out, _) = apply_plan_json(&fixture("PMSV10_GoalAlignCascade.rpt"), plan).expect("apply");
    let rpt = open_bytes(&out).unwrap();
    let report = rpt.report();
    assert_eq!(report.data_definition.groups.len(), 2, "two groups");
    // The minted wrapper reads back: a formula named `Group #2 Order` whose reference and
    // body name the new condition field.
    let stream = rpt.stream(&StreamId::Contents).unwrap();
    let logical = stream.logical_bytes();
    let mut minted = false;
    for w in stream.record_tree().iter().filter(|n| n.rtype == 0x0077) {
        let Some(f) = w.children.iter().find(|c| c.rtype == 0x0076) else { continue };
        let Some(child) = f.children.iter().find(|c| c.rtype == 0x0071) else { continue };
        let Some(read) = rpt_reader::fields::read(child, logical, rpt_reader::raw::Dialect::Contents) else { continue };
        let name = read.fields.iter().find(|x| x.name == "name").map(|x| match &x.value {
            rpt_reader::fields::FieldValue::Text(t) => t.clone(),
            _ => String::new(),
        }).unwrap_or_default();
        if name == "Group #2 Order" {
            let Some(fr) = rpt_reader::fields::read(f, logical, rpt_reader::raw::Dialect::Contents) else { continue };
            let text = fr.fields.iter().find(|x| x.path == "text").map(|x| match &x.value {
                rpt_reader::fields::FieldValue::Text(t) => t.clone(),
                _ => String::new(),
            }).unwrap_or_default();
            assert!(text.contains("{sheet.goal_name}"), "body names the condition field: {}", text);
            let refs = fr.fields.iter().any(|x| matches!(&x.value,
                rpt_reader::fields::FieldValue::FieldRef { name, .. } if name.contains("goal_name")));
            assert!(refs, "the reference list names the condition field");
            minted = true;
        }
    }
    assert!(minted, "the minted `Group #2 Order` wrapper reads back");
    // The new 0xe5 carries the re-numbered identity.
    let stream2 = rpt.stream(&StreamId::Contents).unwrap();
    let mut groups = 0usize;
    for _n in stream2.record_tree().iter().filter(|n| n.rtype == 0x00e5) {
        groups += 1;
    }
    assert_eq!(groups, 2);
    assert_clean(&out, "addGroup mint");
}

/* ---------- Pm- parameter creation ---------- */

/// setSubreportLink on a subreport with NO `Pm-` parameter creates it (typed like the main
/// field) and writes the full two-sided binding.
#[test]
fn subreport_link_creates_pm_parameter() {
    let plan = r#"{"planVersion":1,"operations":[
        {"action":"setSubreportLink","target":"company logo",
         "mainReportField":"{sheet.performance_cycle_id}",
         "subreportField":"{sp_perf_company_logo;1.HIE_LOGO_IMAGE}",
         "linkedParameter":"Pm-sheet.performance_cycle_id"}]}"#;
    let (out, _) = apply_plan_json(&fixture("PMSV10_IndPerfOverviewSS.rpt"), plan).expect("apply");
    let rpt = open_bytes(&out).unwrap();
    let report = rpt.report();
    let sub = &report.subreports[0];
    // The parameter exists (the model synthesizes the GUID-less record) and is typed Number
    // like the main field.
    let pm = sub
        .report
        .data_definition
        .field_definitions
        .iter()
        .find(|f| f.name.eq_ignore_ascii_case("Pm-sheet.performance_cycle_id"))
        .expect("the Pm- parameter reads back");
    assert_eq!(pm.value_type, rpt_reader::model::FieldValueType::Number);
    // The sub-side Record Selection carries the binding against it.
    let sel = sub.report.data_definition.record_selection.clone();
    assert_eq!(
        sel.as_ref().map(|s| s.0.as_str()),
        Some("{sp_perf_company_logo;1.HIE_LOGO_IMAGE}={?Pm-sheet.performance_cycle_id}")
    );
    // And the main-side link registers on the placeholder object.
    assert_eq!(report.subreports[0].links.len(), 1);
    assert_clean(&out, "Pm- creation");
}

/* ---------- addRunningTotal ---------- */

/// addRunningTotal synthesizes a definition the model reads back, and addField places it
/// with the `{#name}` reference form the engine's own donor carries.
#[test]
fn add_running_total_and_place() {
    let plan = r#"{"planVersion":1,"operations":[
        {"action":"addRunningTotal","newName":"OrderCount","fieldRef":"{sheet.goal_id}","operation":"count"},
        {"action":"addField","section":"ReportHeaderSection1","newName":"RtField",
         "fieldRef":"{#OrderCount}","leftTwips":500,"topTwips":200,"widthTwips":2000,"heightTwips":300}]}"#;
    let (out, _) = apply_plan_json(&fixture("PMSV10_GoalAlignCascade.rpt"), plan).expect("apply");
    let rpt = open_bytes(&out).unwrap();
    let report = rpt.report();
    let rt = report
        .data_definition
        .field_definitions
        .iter()
        .find(|f| f.name == "OrderCount")
        .expect("the running total reads back");
    match &rt.kind {
        rpt_reader::model::FieldKindData::RunningTotal(r) => {
            assert_eq!(r.summarized_field, "sheet.goal_id");
            assert_eq!(format!("{:?}", r.operation), "Count");
        }
        other => panic!("wrong kind: {other:?}"),
    }
    let placed = report
        .report_definition
        .areas
        .iter()
        .flat_map(|a| a.sections.iter().flat_map(|s| s.objects.iter()))
        .find(|o| o.name == "RtField")
        .expect("the placed field");
    match &placed.kind {
        rpt_reader::model::ReportObjectKind::Field(f) => {
            assert_eq!(f.data_source, "{#OrderCount}", "the donor form");
        }
        other => panic!("wrong placed kind: {other:?}"),
    }
    assert_clean(&out, "addRunningTotal");
}

/// addRunningTotal refuses an unknown operation name.
#[test]
fn add_running_total_refuses_unknown_operation() {
    let ops = vec![EditOp::AddRunningTotal {
        name: "X".to_string(),
        summarize: "{sheet.goal_id}".to_string(),
        operation: "medianize".to_string(),
        evaluate_field: None,
        reset_field: None,
    }];
    let e = apply_edits(&fixture("PMSV10_GoalAlignCascade.rpt"), &ops).unwrap_err();
    assert!(e.contains("medianize"), "`{e}` names the operation");
}

/// The fixtures stay clean under the new validator check (summary-operation range).
#[test]
fn summary_operation_check_clean_on_fixtures() {
    for name in [
        "PMSV10_GoalAlignCascade.rpt",
        "World Sales Report.rpt",
        "PMSV10_IndDetailedEvalSS.rpt",
    ] {
        let v = validate(&open(&fixture(name)).unwrap());
        assert!(
            v.errors().next().is_none(),
            "{name}: {:?}",
            v.errors().map(|f| f.message.to_string()).collect::<Vec<_>>()
        );
    }
}
