//! API template flows - the model and the checks a flow must pass. See the
//! design doc "API template flows" §3 (the format) and §4 (a template joins
//! a flow).

use serde_json::{json, Value};
use v2_lib::api_templates::flow::{
    check_flow, check_stage_ref, creating_stage, parse_flow, required_before, substitute_check, Flow, Subject,
    SubjectType,
};
use v2_lib::api_templates::ApiTemplate;

/// The design doc's §3 example, with Competencies optional.
fn flow_json() -> Value {
    json!({
        "id": "pms-performance-cycle",
        "title": "Performance cycle wizard",
        "module": "PMS / Performance Cycle",
        "subject": { "name": "cycleId", "type": "number" },
        "sources": ["Pages/PerformanceCycle/Index.cshtml.cs:40"],
        "stages": [
            { "id": "setup", "title": "Cycle setup", "creates": true,
              "check": "SELECT 1 FROM PeoplesHR.perf_cycle WHERE cycle_id = {{cycleId}}" },
            { "id": "rules", "title": "Evaluation rules", "requires": ["setup"],
              "check": "SELECT 1 FROM PeoplesHR.perf_cycle_step_progress WHERE cycle_id = {{cycleId}} AND step_key = 'EvalRules' AND is_complete = 1" },
            { "id": "competencies", "title": "Competencies", "requires": ["rules"], "optional": true,
              "check": "SELECT 1 FROM PeoplesHR.perf_cycle_competency WHERE cycle_id = {{cycleId}}" },
            { "id": "participants", "title": "Participants", "requires": ["rules"],
              "check": "SELECT 1 FROM PeoplesHR.perf_cycle_participant WHERE cycle_id = {{cycleId}}" },
            { "id": "publish", "title": "Publish", "requires": ["participants"],
              "check": "SELECT 1 FROM PeoplesHR.perf_cycle WHERE cycle_id = {{cycleId}} AND status = 'Published'" }
        ]
    })
}

fn flow() -> Flow {
    serde_json::from_value(flow_json()).expect("fixture should deserialize")
}

/// A stage as JSON, for building flows in the shapes each rule needs.
fn stage(id: &str, requires: &[&str]) -> Value {
    json!({ "id": id, "title": id, "requires": requires,
            "check": "SELECT 1 FROM t WHERE id = {{cycleId}}" })
}

fn creating(id: &str) -> Value {
    json!({ "id": id, "title": id, "creates": true, "check": "SELECT 1 FROM t WHERE id = {{cycleId}}" })
}

/// A flow built from the given stages, deserialized without `check_flow`,
/// so a deliberately-invalid shape reaches it.
fn flow_of(stages: Vec<Value>) -> Flow {
    let mut v = flow_json();
    v["stages"] = Value::Array(stages);
    serde_json::from_value(v).expect("fixture should deserialize")
}

fn mentions(problems: &[String], needles: &[&str]) -> bool {
    problems.iter().any(|p| needles.iter().all(|n| p.contains(n)))
}

fn number_subject() -> Subject {
    Subject { name: "cycleId".into(), kind: SubjectType::Number }
}

fn string_subject() -> Subject {
    Subject { name: "cycleId".into(), kind: SubjectType::String }
}

#[test]
fn a_valid_flow_has_no_problems() {
    assert_eq!(check_flow(&flow()), Vec::<String>::new());
    assert!(parse_flow(&flow_json()).is_ok());
}

#[test]
fn an_unknown_field_is_refused_at_every_level() {
    let mut v = flow_json();
    v["colour"] = json!(1);
    let problems = parse_flow(&v).unwrap_err();
    assert!(mentions(&problems, &["colour"]), "{problems:?}");

    let mut v = flow_json();
    v["stages"][0]["colour"] = json!(1);
    assert!(mentions(&parse_flow(&v).unwrap_err(), &["colour"]));

    let mut v = flow_json();
    v["subject"]["colour"] = json!(1);
    assert!(mentions(&parse_flow(&v).unwrap_err(), &["colour"]));
}

#[test]
fn a_bad_flow_id_is_refused() {
    let mut f = flow();
    f.id = "Pms X".into();
    let problems = check_flow(&f);
    assert!(mentions(&problems, &["Pms X"]), "{problems:?}");
}

#[test]
fn a_bad_stage_id_is_refused() {
    let f = flow_of(vec![creating("Set Up"), stage("rules", &["Set Up"])]);
    assert!(mentions(&check_flow(&f), &["Set Up"]));
}

#[test]
fn a_duplicate_stage_id_is_refused() {
    let f = flow_of(vec![creating("setup"), stage("rules", &["setup"]), stage("rules", &["setup"])]);
    let problems = check_flow(&f);
    assert!(mentions(&problems, &["rules", "more than once"]), "{problems:?}");
}

#[test]
fn a_subject_name_that_is_not_a_placeholder_name_is_refused() {
    let mut f = flow();
    f.subject.name = "cycle id".into();
    let problems = check_flow(&f);
    assert!(mentions(&problems, &["cycle id"]), "{problems:?}");
}

#[test]
fn a_subject_type_other_than_number_or_string_is_refused() {
    let mut v = flow_json();
    v["subject"]["type"] = json!("date");
    let problems = parse_flow(&v).unwrap_err();
    assert!(mentions(&problems, &["date"]), "{problems:?}");
}

#[test]
fn a_flow_with_no_creating_stage_is_refused() {
    let mut v = flow_json();
    v["stages"][0]["creates"] = json!(false);
    v["stages"][0]["requires"] = json!(["publish"]);
    let f: Flow = serde_json::from_value(v).unwrap();
    let problems = check_flow(&f);
    assert!(mentions(&problems, &["exactly one", "creates"]), "{problems:?}");
}

#[test]
fn two_creating_stages_are_refused_naming_both() {
    let f = flow_of(vec![creating("setup"), creating("also")]);
    let problems = check_flow(&f);
    assert!(mentions(&problems, &["setup", "also", "creates"]), "{problems:?}");
}

#[test]
fn a_creating_stage_cannot_require_anything() {
    let f = flow_of(vec![
        json!({ "id": "setup", "title": "s", "creates": true, "requires": ["rules"],
                "check": "SELECT 1 WHERE id = {{cycleId}}" }),
        stage("rules", &["setup"]),
    ]);
    let problems = check_flow(&f);
    assert!(mentions(&problems, &["setup", "creates", "require"]), "{problems:?}");
}

#[test]
fn a_stage_that_does_not_create_must_require_something() {
    let f = flow_of(vec![creating("setup"), stage("rules", &[])]);
    let problems = check_flow(&f);
    assert!(mentions(&problems, &["rules", "at least one"]), "{problems:?}");
}

#[test]
fn requiring_a_stage_that_is_not_in_the_flow_is_refused() {
    let f = flow_of(vec![creating("setup"), stage("rules", &["nope"])]);
    let problems = check_flow(&f);
    assert!(mentions(&problems, &["rules", "nope"]), "{problems:?}");
}

#[test]
fn a_stage_requiring_itself_is_refused() {
    let f = flow_of(vec![creating("setup"), stage("rules", &["setup", "rules"])]);
    let problems = check_flow(&f);
    assert!(mentions(&problems, &["rules", "itself"]), "{problems:?}");
}

#[test]
fn a_cycle_is_reported_once_naming_both_stages() {
    let f = flow_of(vec![creating("setup"), stage("a", &["b"]), stage("b", &["a"])]);
    let problems = check_flow(&f);
    let loops: Vec<&String> = problems.iter().filter(|p| p.contains("loop")).collect();
    assert_eq!(loops.len(), 1, "{problems:?}");
    assert!(loops[0].contains("'a'") && loops[0].contains("'b'"), "{loops:?}");
}

#[test]
fn requiring_an_optional_stage_is_refused() {
    let mut v = flow_json();
    // participants now requires competencies, which is optional.
    v["stages"][3]["requires"] = json!(["competencies"]);
    let f: Flow = serde_json::from_value(v).unwrap();
    let problems = check_flow(&f);
    assert!(mentions(&problems, &["participants", "competencies", "optional"]), "{problems:?}");
}

#[test]
fn more_than_thirty_stages_are_refused() {
    let mut stages = vec![creating("s0")];
    for i in 1..=30 {
        stages.push(stage(&format!("s{i}"), &["s0"]));
    }
    assert_eq!(stages.len(), 31);
    let problems = check_flow(&flow_of(stages));
    assert!(mentions(&problems, &["30", "31"]), "{problems:?}");
}

#[test]
fn a_check_without_the_subject_placeholder_is_refused() {
    let mut f = flow();
    f.stages[1].check = "SELECT 1 FROM t".into();
    let problems = check_flow(&f);
    assert!(mentions(&problems, &["rules", "{{cycleId}}"]), "{problems:?}");
}

#[test]
fn a_check_with_another_placeholder_is_refused() {
    let mut f = flow();
    f.stages[1].check = "SELECT 1 FROM t WHERE a = {{cycleId}} AND b = {{other}}".into();
    let problems = check_flow(&f);
    assert!(mentions(&problems, &["rules", "{{other}}"]), "{problems:?}");
}

#[test]
fn a_check_that_writes_is_refused() {
    let mut f = flow();
    f.stages[1].check = "UPDATE t SET a = 1 WHERE id = {{cycleId}}".into();
    let problems = check_flow(&f);
    assert!(mentions(&problems, &["rules"]), "{problems:?}");
}

#[test]
fn a_draft_carrying_saved_is_refused() {
    let mut v = flow_json();
    v["saved"] = json!({ "at": "2026-09-29T09:00:00Z", "sample": 273 });
    let problems = parse_flow(&v).unwrap_err();
    assert!(mentions(&problems, &["saved"]), "{problems:?}");
}

#[test]
fn every_problem_is_reported_together() {
    let mut f = flow();
    f.id = "Bad Id".into();
    f.stages[1].check = "SELECT 1".into();
    f.stages[4].requires = vec!["nope".into()];
    let problems = check_flow(&f);
    assert!(problems.len() >= 3, "{problems:?}");
}

#[test]
fn substitution_is_typed() {
    assert_eq!(
        substitute_check("SELECT 1 FROM t WHERE id = {{cycleId}}", &number_subject(), &json!(274)).unwrap(),
        "SELECT 1 FROM t WHERE id = 274"
    );
    assert_eq!(
        substitute_check("SELECT 1 FROM t WHERE id = {{cycleId}}", &string_subject(), &json!("O'Brien")).unwrap(),
        "SELECT 1 FROM t WHERE id = N'O''Brien'"
    );
    assert_eq!(
        substitute_check("SELECT 1 WHERE a = {{cycleId}} OR b = {{ cycleId }}", &number_subject(), &json!(7)).unwrap(),
        "SELECT 1 WHERE a = 7 OR b = 7"
    );
    for bad in [json!("274"), json!(2.5), json!(-1), json!(true), json!(null)] {
        let err = substitute_check("SELECT 1 WHERE id = {{cycleId}}", &number_subject(), &bad).unwrap_err();
        assert!(err.contains("cycleId") && err.contains("number"), "{bad}: {err}");
    }
    let err = substitute_check("SELECT 1 WHERE id = {{cycleId}}", &string_subject(), &json!(5)).unwrap_err();
    assert!(err.contains("cycleId") && err.contains("string"), "{err}");
}

#[test]
fn substitution_is_classified_again() {
    let err = substitute_check("SELECT 1 WHERE x = {{cycleId}}; DELETE FROM t", &number_subject(), &json!(1));
    assert!(err.is_err());
    // A string value cannot smuggle a statement past the doubled quote.
    let out = substitute_check("SELECT 1 WHERE x = {{cycleId}}", &string_subject(), &json!("x'; DELETE FROM t; --"));
    assert_eq!(out.unwrap(), "SELECT 1 WHERE x = N'x''; DELETE FROM t; --'");
}

#[test]
fn required_before_is_the_transitive_closure() {
    let f = flow();
    let ids = |stage: &str| required_before(&f, stage).iter().map(|s| s.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids("publish"), ["setup", "rules", "participants"]);
    assert!(!ids("publish").contains(&"competencies".to_string()));
    assert!(ids("setup").is_empty());
    assert!(ids("nope").is_empty());

    let diamond = flow_of(vec![creating("a"), stage("b", &["a"]), stage("c", &["a"]), stage("d", &["b", "c"])]);
    let ids: Vec<&str> = required_before(&diamond, "d").iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["a", "b", "c"]);
}

#[test]
fn the_creating_stage_is_found() {
    assert_eq!(creating_stage(&flow()).map(|s| s.id.as_str()), Some("setup"));
}

fn template(stage: Value, params: Value, steps: Value, outputs: Value) -> ApiTemplate {
    serde_json::from_value(json!({
        "id": "pms-x", "title": "X", "module": "PMS", "effect": "create",
        "description": "d", "sources": [], "antiforgery": { "page": "/hr/x" },
        "params": params, "steps": steps, "outputs": outputs,
        "stage": stage
    }))
    .expect("template fixture should deserialize")
}

fn capturing_steps() -> Value {
    json!([{ "name": "Save", "method": "POST", "path": "/hr/x", "capture": { "cycleId": "$.cycleId" } }])
}

fn plain_steps() -> Value {
    json!([{ "name": "Save", "method": "POST", "path": "/hr/x" }])
}

#[test]
fn a_template_names_its_stage() {
    let f = flow();
    let setup = json!({ "flow": "pms-performance-cycle", "id": "setup" });
    let participants = json!({ "flow": "pms-performance-cycle", "id": "participants" });

    // The flow is gone.
    let t = template(setup.clone(), json!([]), capturing_steps(), json!(["cycleId"]));
    assert_eq!(
        check_stage_ref(&t, None),
        vec!["this template's flow pms-performance-cycle is no longer saved".to_string()]
    );

    // The stage is gone.
    let t = template(json!({ "flow": "pms-performance-cycle", "id": "gone" }), json!([]), plain_steps(), json!([]));
    assert_eq!(
        check_stage_ref(&t, Some(&f)),
        vec!["stage \"gone\" is no longer in flow pms-performance-cycle".to_string()]
    );

    // Creating stage: no capture, then capture without the output, then right.
    let t = template(setup.clone(), json!([]), plain_steps(), json!([]));
    assert!(mentions(&check_stage_ref(&t, Some(&f)), &["cycleId", "capture"]));
    let t = template(setup.clone(), json!([]), capturing_steps(), json!([]));
    let problems = check_stage_ref(&t, Some(&f));
    assert!(mentions(&problems, &["cycleId", "outputs"]), "{problems:?}");
    let t = template(setup, json!([]), capturing_steps(), json!(["cycleId"]));
    assert_eq!(check_stage_ref(&t, Some(&f)), Vec::<String>::new());

    // Any other stage: the subject as a parameter of the subject's type.
    let t = template(participants.clone(), json!([]), plain_steps(), json!([]));
    assert!(mentions(&check_stage_ref(&t, Some(&f)), &["cycleId", "parameter"]));
    let t = template(
        participants.clone(),
        json!([{ "name": "cycleId", "type": "string", "required": true }]),
        plain_steps(),
        json!([]),
    );
    let problems = check_stage_ref(&t, Some(&f));
    assert!(mentions(&problems, &["cycleId", "string", "number"]), "{problems:?}");
    let t = template(
        participants,
        json!([{ "name": "cycleId", "type": "number", "required": true }]),
        plain_steps(),
        json!([]),
    );
    assert_eq!(check_stage_ref(&t, Some(&f)), Vec::<String>::new());
}

#[test]
fn a_template_saved_before_flows_still_reads() {
    let v = json!({
        "id": "pms-x", "title": "X", "module": "PMS", "effect": "create",
        "description": "d", "sources": [], "antiforgery": { "page": "/hr/x" },
        "params": [], "steps": [], "outputs": []
    });
    let t: ApiTemplate = serde_json::from_value(v).unwrap();
    assert!(t.stage.is_none());
    assert_eq!(check_stage_ref(&t, None), Vec::<String>::new());
    let back = serde_json::to_value(&t).unwrap();
    assert!(back.get("stage").is_none(), "{back}");
}
