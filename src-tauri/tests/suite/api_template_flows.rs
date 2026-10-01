//! API template flows - the model and the checks a flow must pass. See the
//! design doc "API template flows" §3 (the format) and §4 (a template joins
//! a flow).

use serde_json::{json, Value};
use v2_lib::api_templates::flow::{
    check_flow, check_stage_ref, creating_stage, parse_flow, required_before, substitute_check, Flow, Subject,
    SubjectType,
};
use v2_lib::api_templates::flow_store;
use v2_lib::api_templates::store;
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

#[test]
fn flows_live_beside_templates() {
    let root = std::path::Path::new("data-root");
    let slug = store::templates_dir(root, "Org", "Proj").file_name().unwrap().to_owned();
    let dir = flow_store::flows_dir(root, "Org", "Proj");
    assert_eq!(dir, root.join("flows").join(slug));
}

#[test]
fn save_then_load_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let mut f = flow();
    f.saved = Some(serde_json::from_value(json!({ "at": "2026-09-29T10:00:00Z", "sample": { "cycleId": 274 } })).unwrap());
    flow_store::save(dir.path(), "Org", "Proj", &f).unwrap();
    assert!(flow_store::flows_dir(dir.path(), "Org", "Proj").join("pms-performance-cycle.json").is_file());
    let back = flow_store::load(dir.path(), "Org", "Proj", &f.id).unwrap();
    assert_eq!(back, Some(f));
    assert_eq!(flow_store::load(dir.path(), "Org", "Proj", "nothing-here").unwrap(), None);
}

#[test]
fn list_skips_a_file_that_does_not_parse() {
    let dir = tempfile::tempdir().unwrap();
    assert!(flow_store::list(dir.path(), "Org", "Proj").unwrap().is_empty());
    let good = flow();
    flow_store::save(dir.path(), "Org", "Proj", &good).unwrap();
    std::fs::write(flow_store::flows_dir(dir.path(), "Org", "Proj").join("bad.json"), "{").unwrap();
    let listed = flow_store::list(dir.path(), "Org", "Proj").unwrap();
    assert_eq!(listed, vec![good]);
}

#[test]
fn list_is_sorted_by_title() {
    let dir = tempfile::tempdir().unwrap();
    for (id, title) in [("b-flow", "Zeta"), ("a-flow", "Alpha")] {
        let mut f = flow();
        f.id = id.into();
        f.title = title.into();
        flow_store::save(dir.path(), "Org", "Proj", &f).unwrap();
    }
    let titles: Vec<String> = flow_store::list(dir.path(), "Org", "Proj").unwrap().into_iter().map(|f| f.title).collect();
    assert_eq!(titles, vec!["Alpha", "Zeta"]);
}

#[test]
fn remove_deletes_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let f = flow();
    flow_store::save(dir.path(), "Org", "Proj", &f).unwrap();
    flow_store::remove(dir.path(), "Org", "Proj", &f.id).unwrap();
    assert_eq!(flow_store::load(dir.path(), "Org", "Proj", &f.id).unwrap(), None);
    flow_store::remove(dir.path(), "Org", "Proj", &f.id).unwrap();
}

#[test]
fn remove_refuses_an_id_with_a_path_in_it() {
    let dir = tempfile::tempdir().unwrap();
    for id in ["..\\x", "../x", ""] {
        assert!(flow_store::remove(dir.path(), "Org", "Proj", id).is_err(), "{id:?}");
        assert!(flow_store::load(dir.path(), "Org", "Proj", id).is_err(), "{id:?}");
    }
    let mut f = flow();
    f.id = "..\\x".into();
    assert!(flow_store::save(dir.path(), "Org", "Proj", &f).is_err());
}

// ------------------------------------------------------------------- the gate

mod gate_tests {
    use super::*;
    use crate::common::FakeStageDb;
    use std::path::Path;
    use std::time::Duration;
    use v2_lib::api_templates::gate::{
        gate, progress, stage_state, stage_state_within, templates_on, CheckFor, CHECK_TIMEOUT, SqlcmdStageDb, StageDb, StageProgress, StageState,
    };
    use v2_lib::api_templates::store::SavedTemplate;
    use v2_lib::db::{Connection, Output, Runner};

    /// The flow with `/*stage-id*/` on the end of every check, so FakeStageDb
    /// can tell them apart.
    fn marked(mut f: Flow) -> Flow {
        for s in &mut f.stages {
            s.check = format!("{} /*{}*/", s.check, s.id);
        }
        f
    }

    fn cycle_flow() -> Flow {
        marked(flow())
    }

    fn diamond() -> Flow {
        marked(flow_of(vec![creating("a"), stage("b", &["a"]), stage("c", &["a"]), stage("d", &["b", "c"])]))
    }

    fn saved(id: &str, title: &str, stage_id: &str) -> SavedTemplate {
        let mut t = template(
            json!({ "flow": "pms-performance-cycle", "id": stage_id }),
            json!([]),
            plain_steps(),
            json!([]),
        );
        t.id = id.into();
        t.title = title.into();
        SavedTemplate { template: t, runs: vec![] }
    }

    fn done(marker: &str) -> (String, Result<bool, String>) {
        (format!("/*{marker}*/"), Ok(true))
    }

    fn not_done(marker: &str) -> (String, Result<bool, String>) {
        (format!("/*{marker}*/"), Ok(false))
    }

    fn broken(marker: &str, why: &str) -> (String, Result<bool, String>) {
        (format!("/*{marker}*/"), Err(why.to_string()))
    }

    fn fake(answers: Vec<(String, Result<bool, String>)>) -> FakeStageDb {
        answers.into_iter().fold(FakeStageDb::new(), |db, (m, r)| db.answer(&m, r))
    }

    fn markers_asked(db: &FakeStageDb) -> Vec<String> {
        db.calls()
            .iter()
            .map(|sql| {
                let start = sql.rfind("/*").expect("marker") + 2;
                sql[start..sql.rfind("*/").unwrap()].to_string()
            })
            .collect()
    }

    fn block<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(f)
    }

    #[test]
    fn the_creating_stage_is_never_gated() {
        let _g = crate::serial::activity_log();
        let db = FakeStageDb::new();
        let r = block(gate(&db, &cycle_flow(), "setup", &json!(274), &[], "t"));
        assert_eq!(r, Ok(()));
        assert!(db.calls().is_empty());
    }

    /// A stage the flow does not have is refused, not waved through as one
    /// with nothing before it - and nothing is asked of the database.
    #[test]
    fn a_stage_not_in_the_flow_is_refused() {
        let _g = crate::serial::activity_log();
        let db = FakeStageDb::new();
        let r = block(gate(&db, &cycle_flow(), "reviews", &json!(274), &[], "t"));
        assert_eq!(r, Err("stage \"reviews\" is no longer in flow pms-performance-cycle".to_string()));
        assert!(db.calls().is_empty());
    }

    #[test]
    fn every_required_stage_done_lets_it_through() {
        let _g = crate::serial::activity_log();
        let db = fake(vec![done("setup"), done("rules")]);
        let r = block(gate(&db, &cycle_flow(), "participants", &json!(274), &[], "t"));
        assert_eq!(r, Ok(()));
        assert_eq!(markers_asked(&db), vec!["setup", "rules"]);
    }

    #[test]
    fn a_missing_stage_is_named_with_its_template() {
        let _g = crate::serial::activity_log();
        let db = fake(vec![done("setup"), not_done("rules"), not_done("participants")]);
        let templates = vec![saved("pms-set-eval-rules", "Set the evaluation rules", "rules")];
        let r = block(gate(&db, &cycle_flow(), "publish", &json!(274), &templates, "pms-publish"));
        assert_eq!(
            r,
            Err("Evaluation rules is not done for cycleId 274 - do it first with pms-set-eval-rules (Set the evaluation rules). Then: Participants."
                .to_string())
        );
        // Every required stage was asked, in flow order.
        assert_eq!(markers_asked(&db), vec!["setup", "rules", "participants"]);
    }

    #[test]
    fn a_missing_stage_with_no_template_says_so() {
        let _g = crate::serial::activity_log();
        let db = fake(vec![done("setup"), not_done("rules")]);
        let r = block(gate(&db, &cycle_flow(), "participants", &json!(274), &[], "t"));
        let said = r.unwrap_err();
        assert!(said.contains("no template performs Evaluation rules yet: prove one first"), "{said}");
        assert!(!said.contains("Then:"), "{said}");
    }

    #[test]
    fn several_templates_on_one_stage_are_all_offered() {
        let _g = crate::serial::activity_log();
        let db = fake(vec![done("setup"), not_done("rules")]);
        let templates =
            vec![saved("one", "First", "rules"), saved("two", "Second", "rules"), saved("other", "Other", "setup")];
        let said = block(gate(&db, &cycle_flow(), "participants", &json!(274), &templates, "t")).unwrap_err();
        assert!(said.contains("do it first with one (First) or two (Second)"), "{said}");
        assert!(!said.contains("other"), "{said}");
    }

    #[test]
    fn a_check_that_cannot_run_is_not_not_done() {
        let _g = crate::serial::activity_log();
        let db = fake(vec![done("setup"), broken("rules", "Login failed")]);
        let said = block(gate(&db, &cycle_flow(), "participants", &json!(274), &[], "t")).unwrap_err();
        assert!(said.contains("the check for Evaluation rules could not be run"), "{said}");
        assert!(said.contains("see the activity folder in Settings, Logs"), "{said}");
        assert!(!said.contains("Login failed"), "{said}");
        assert!(!said.contains("is not done"), "{said}");
    }

    #[test]
    fn a_check_that_cannot_run_wins_over_one_that_is_not_done() {
        let _g = crate::serial::activity_log();
        let db = fake(vec![not_done("setup"), broken("rules", "boom")]);
        let said = block(gate(&db, &cycle_flow(), "participants", &json!(274), &[], "t")).unwrap_err();
        assert!(said.contains("the check for Evaluation rules could not be run"), "{said}");
        assert!(!said.contains("is not done"), "{said}");
    }

    #[test]
    fn the_diamond_requires_both() {
        let _g = crate::serial::activity_log();
        let db = fake(vec![done("a"), done("b"), not_done("c")]);
        let said = block(gate(&db, &diamond(), "d", &json!(1), &[], "t")).unwrap_err();
        assert!(said.starts_with("c is not done for cycleId 1"), "{said}");
        assert!(!said.contains("b is not done"), "{said}");
    }

    #[test]
    fn a_value_of_the_wrong_type_is_refused_before_any_query() {
        let _g = crate::serial::activity_log();
        let db = FakeStageDb::new();
        for bad in [json!("274"), json!(2.5), json!(-1), json!(null)] {
            let said = block(gate(&db, &cycle_flow(), "participants", &bad, &[], "t")).unwrap_err();
            assert!(said.contains("cycleId is a number subject"), "{said}");
            let said = block(progress(&db, &cycle_flow(), &bad, &[])).unwrap_err();
            assert!(said.contains("cycleId is a number subject"), "{said}");
        }
        assert!(db.calls().is_empty());
    }

    fn states(p: &[StageProgress]) -> Vec<(&str, &str)> {
        p.iter().map(|s| (s.id.as_str(), s.state)).collect()
    }

    #[test]
    fn progress_marks_each_stage() {
        let _g = crate::serial::activity_log();
        let db = fake(vec![
            done("setup"),
            not_done("rules"),
            not_done("competencies"),
            not_done("participants"),
            not_done("publish"),
        ]);
        let templates = vec![saved("pms-set-eval-rules", "Set the evaluation rules", "rules")];
        let p = block(progress(&db, &cycle_flow(), &json!(274), &templates)).unwrap();
        assert_eq!(
            states(&p),
            vec![
                ("setup", "done"),
                ("rules", "next"),
                ("competencies", "blocked"),
                ("participants", "blocked"),
                ("publish", "blocked")
            ]
        );
        assert_eq!(db.calls().len(), 5, "every stage is checked once");
        assert_eq!(p[1].templates, vec!["pms-set-eval-rules".to_string()]);
        assert_eq!(p[1].title, "Evaluation rules");
        assert!(p[2].optional && !p[1].optional);

        let db = fake(vec![
            done("setup"),
            done("rules"),
            not_done("competencies"),
            not_done("participants"),
            not_done("publish"),
        ]);
        let p = block(progress(&db, &cycle_flow(), &json!(274), &[])).unwrap();
        assert_eq!(
            states(&p),
            vec![
                ("setup", "done"),
                ("rules", "done"),
                ("competencies", "skippable"),
                ("participants", "next"),
                ("publish", "blocked")
            ]
        );
    }

    #[test]
    fn progress_marks_a_check_that_could_not_run_and_blocks_what_needs_it() {
        let _g = crate::serial::activity_log();
        let db = fake(vec![
            done("setup"),
            broken("rules", "timeout"),
            not_done("competencies"),
            not_done("participants"),
            not_done("publish"),
        ]);
        let p = block(progress(&db, &cycle_flow(), &json!(274), &[])).unwrap();
        assert_eq!(
            states(&p),
            vec![
                ("setup", "done"),
                ("rules", "could_not_check"),
                ("competencies", "blocked"),
                ("participants", "blocked"),
                ("publish", "blocked")
            ]
        );
    }

    #[test]
    fn templates_on_a_stage_are_found_by_flow_and_stage() {
        let mut other_flow = saved("elsewhere", "Elsewhere", "rules");
        other_flow.template.stage.as_mut().unwrap().flow = "another-flow".into();
        let mut none = saved("plain", "Plain", "rules");
        none.template.stage = None;
        let all = vec![saved("a", "A", "rules"), other_flow, none, saved("b", "B", "setup")];
        let ids: Vec<&str> =
            templates_on(&all, "pms-performance-cycle", "rules").iter().map(|t| t.template.id.as_str()).collect();
        assert_eq!(ids, vec!["a"]);
    }

    // -------------------------------------------------- the sqlcmd-backed reader

    struct FakeRunner {
        stdout: String,
    }

    impl Runner for FakeRunner {
        async fn run(
            &self,
            _exe: &Path,
            _args: &[String],
            _env: &[(String, String)],
            _timeout: Duration,
        ) -> Result<Output, String> {
            Ok(Output { status: 0, stdout: self.stdout.clone(), stderr: String::new() })
        }
    }

    fn conn() -> Connection {
        v2_lib::db::parse_connection(v2_lib::db_defaults::DB_PRESETS[0].connection_string).unwrap()
    }

    fn reader(stdout: &str) -> SqlcmdStageDb<FakeRunner> {
        SqlcmdStageDb { runner: FakeRunner { stdout: stdout.into() }, exe: "sqlcmd.exe".into(), conn: conn() }
    }

    #[test]
    fn a_check_without_a_row_count_could_not_run() {
        // "1" and no "(1 row affected)" - SET NOCOUNT ON, or the output was cut.
        assert!(block(reader("1\n").read("SELECT 1")).is_err());
        assert_eq!(block(reader("1\n\n(1 row affected)\n").read("SELECT 1")), Ok(true));
        assert_eq!(block(reader("\n(0 rows affected)\n").read("SELECT 1 WHERE 1 = 0")), Ok(false));
    }

    #[test]
    fn the_reader_is_labelled_server_slash_database() {
        let r = reader("");
        let c = conn();
        assert_eq!(r.label(), format!("{}/{}", c.server, c.database));
    }

    #[test]
    fn a_write_is_refused_by_the_reader_not_run() {
        let r = reader("(1 row affected)");
        assert!(block(r.read("UPDATE t SET a = 1")).is_err());
    }

    // ----------------------------------------------------- the activity trail

    #[test]
    fn every_check_is_in_the_activity_log_and_no_sql_in_the_app_log() {
        // log_tail before activity_log, the order every other module takes
        // them in - the other way round deadlocks against those tests.
        let _l = crate::serial::log_tail();
        let _g = crate::serial::activity_log();
        let dir = tempfile::tempdir().unwrap();
        v2_lib::activity_log::init(dir.path().to_path_buf());

        let f = cycle_flow();
        let db = fake(vec![done("setup"), not_done("rules")]);
        let why = CheckFor { purpose: "gate", template: Some("pms-x") };
        let a = block(stage_state(&db, &f, &f.stages[0], &json!(274), &why));
        let b = block(stage_state(&db, &f, &f.stages[1], &json!(274), &why));
        let c = block(stage_state(
            &fake(vec![broken("rules", "Login failed for user sa")]),
            &f,
            &f.stages[1],
            &json!(274),
            &CheckFor { purpose: "progress", template: None },
        ));
        assert_eq!((a, b, c), (StageState::Done, StageState::NotDone, StageState::CouldNotRun));

        let recs = crate::common::activity_records(dir.path(), "db");
        assert_eq!(recs.len(), 3);
        assert_eq!(recs[0]["verdict"], "flow check");
        assert_eq!(recs[0]["flow"], "pms-performance-cycle");
        assert_eq!(recs[0]["stage"], "setup");
        assert_eq!(recs[0]["purpose"], "gate");
        assert_eq!(recs[0]["template"], "pms-x");
        assert_eq!(recs[0]["ok"], true);
        assert_eq!(recs[0]["done"], true);
        assert_eq!(recs[0]["connection"], "fake-server/fake-db");
        assert!(recs[0]["duration_ms"].is_u64());
        let sql = recs[0]["sql"].as_str().unwrap();
        assert!(sql.starts_with("SELECT 1 FROM PeoplesHR.perf_cycle WHERE cycle_id = 274"), "{sql}");
        assert_eq!(recs[1]["done"], false);
        assert_eq!(recs[1]["ok"], true);
        assert_eq!(recs[2]["ok"], false);
        assert_eq!(recs[2]["done"], false);
        assert_eq!(recs[2]["error"], "Login failed for user sa");
        assert_eq!(recs[2]["purpose"], "progress");
        assert!(recs[2]["template"].is_null());

        let tail: Vec<String> = v2_lib::applog::recent(500).into_iter().map(|l| l.message).collect();
        let mine: Vec<&String> = tail.iter().filter(|m| m.starts_with("db flow check on")).collect();
        assert!(mine.len() >= 3, "{tail:?}");
        let mine = &mine[mine.len() - 3..];
        assert!(mine[0].ends_with("pms-performance-cycle/setup done"), "{}", mine[0]);
        assert!(mine[1].ends_with("pms-performance-cycle/rules not done"), "{}", mine[1]);
        assert!(mine[2].ends_with("pms-performance-cycle/rules could not run"), "{}", mine[2]);
        assert!(mine.iter().all(|m| !m.contains("SELECT") && !m.contains("Login failed")), "{mine:?}");
    }

    struct Hangs;

    impl StageDb for Hangs {
        fn label(&self) -> String {
            "s/d".into()
        }
        async fn read(&self, _sql: &str) -> Result<bool, String> {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            Ok(true)
        }
    }

    #[test]
    fn a_check_that_hangs_gives_up_and_the_limit_is_fifteen_seconds() {
        let _g = crate::serial::activity_log();
        assert_eq!(CHECK_TIMEOUT, Duration::from_secs(15));
        let f = cycle_flow();
        let why = CheckFor { purpose: "save", template: None };
        let state = block(stage_state_within(&Hangs, &f, &f.stages[0], &json!(1), &why, Duration::from_millis(50)));
        assert_eq!(state, StageState::CouldNotRun);
    }
}

// ---------------------------------------------------------------------------
// The flow on its own page (the tab's "View flow"): large coloured stages
// joined by connectors that blend from one stage's colour into the next.

mod page_tests {
    use v2_lib::api_templates::flow::Flow;
    use v2_lib::api_templates::flow_page::{columns, page_html, tone_of, Tone};
    use v2_lib::api_templates::store::SavedTemplate;
    use v2_lib::api_templates::{ApiTemplate, Effect};
    use v2_lib::webtheme::PagePalette;

    fn flow() -> Flow {
        serde_json::from_value(crate::common::cycle_flow_json()).expect("the fixture flow")
    }

    fn on(id: &str, title: &str, effect: Effect, stage: &str) -> SavedTemplate {
        let t = ApiTemplate { effect, ..crate::common::saved_on_stage(id, title, stage) };
        SavedTemplate { template: t, runs: vec![] }
    }

    #[test]
    fn stages_sit_in_columns_by_their_longest_path() {
        let f = flow();
        let ids: Vec<Vec<&str>> = columns(&f).iter().map(|c| c.iter().map(|s| s.id.as_str()).collect()).collect();
        assert_eq!(ids, [vec!["setup"], vec!["rules"], vec!["competencies", "participants"], vec!["publish"]]);
    }

    #[test]
    fn a_stage_takes_the_colour_of_what_it_does() {
        let t = |effect: Effect| ApiTemplate { effect, ..crate::common::saved_on_stage("x", "X", "rules") };
        let (c, e, d) = (t(Effect::Create), t(Effect::Edit), t(Effect::Delete));
        assert_eq!(tone_of(&[]), Tone::Open);
        assert_eq!(tone_of(&[&c, &c]), Tone::Create);
        assert_eq!(tone_of(&[&e]), Tone::Edit);
        assert_eq!(tone_of(&[&d]), Tone::Delete);
        assert_eq!(tone_of(&[&c, &d]), Tone::Open, "templates that disagree take the accent");
        assert_eq!((Tone::Create.var(), Tone::Edit.var(), Tone::Delete.var(), Tone::Open.var()),
            ("--success", "--warning", "--danger", "--accent"));
    }

    #[test]
    fn the_page_colours_each_stage_and_blends_each_connector() {
        let templates = [
            on("pms-create-cycle", "Create a cycle", Effect::Create, "setup"),
            on("pms-save-rules", "Save the rules", Effect::Edit, "rules"),
        ];
        let html = page_html(&flow(), &templates, &PagePalette::default());

        assert!(html.contains("class='stage create' data-stage='setup'"), "{html}");
        assert!(html.contains("--tone:var(--success)"));
        assert!(html.contains("class='stage edit' data-stage='rules'"));
        assert!(html.contains("class='stage open optional' data-stage='competencies'"), "an optional stage with no template");
        assert!(html.contains("No template yet"));
        assert!(html.contains("Creates the record"));

        // One connector per `requires`, each a gradient from the stage it
        // leaves to the stage it reaches: setup (create) into rules (edit).
        assert_eq!(html.matches("class='wire'").count(), 4);
        assert!(html.contains("<stop offset='0' style='stop-color:var(--success)'/><stop offset='1' style='stop-color:var(--warning)'/>"), "{html}");
        assert!(html.contains("data-from='setup' data-to='rules'"));

        // The browser lays the columns out, so text wraps rather than
        // being cut short; the page's script draws the connectors.
        assert_eq!(html.matches("<div class='col'>").count(), 4);
        assert!(!html.contains("text-overflow:ellipsis"), "nothing is truncated");
        assert!(html.contains("overflow-wrap:anywhere"));
        assert!(html.contains("data-grad='g0'"));
        assert!(html.contains("window.FlowPage"), "the connector script is in the page");
        // Motion only for those who have not asked for less.
        assert!(html.contains("@media (prefers-reduced-motion:no-preference)"));
        // The same in words, for a screen reader.
        assert!(html.contains("<li>Evaluation rules. Requires: Cycle setup. Templates: Save the rules.</li>"), "{html}");
        // The scheme restore in <head>, the light/dark switch, and the
        // connector script.
        assert_eq!(html.matches("<script").count(), 3);
    }

    #[test]
    fn an_unproven_template_is_marked_on_the_page() {
        let mut imported = on("pms-save-rules", "Save the rules", Effect::Edit, "rules");
        imported.template.proven = None;
        let html = page_html(&flow(), &[imported], &PagePalette::default());
        assert!(html.contains("Save the rules</span><span class='fx' title='Imported - not proven on this site."), "{html}");
        assert!(html.contains("Templates: Save the rules (unproven).</li>"), "{html}");

        let proven = on("pms-save-rules", "Save the rules", Effect::Edit, "rules");
        assert!(!page_html(&flow(), &[proven], &PagePalette::default()).contains("unproven"));
    }

    #[test]
    fn every_title_the_assistant_wrote_is_escaped() {
        let mut f = flow();
        f.title = "Cycle </style><script>alert(1)</script>".into();
        f.stages[1].title = "<img src=x onerror=alert(1)>".into();
        let templates = [on("pms-x", "Save <b>rules</b>", Effect::Edit, "rules")];
        let html = page_html(&f, &templates, &PagePalette::default());
        assert!(!html.contains("<script>alert"), "{html}");
        assert!(!html.contains("<img src=x"), "{html}");
        assert!(!html.contains("<b>rules</b>"), "{html}");
        assert!(html.contains("&lt;img src=x onerror=alert(1)&gt;"));
    }

    #[test]
    fn a_hand_edited_loop_still_draws() {
        let mut f = flow();
        f.stages[0].requires = vec!["publish".into()];
        assert_eq!(columns(&f).iter().map(Vec::len).sum::<usize>(), 5);
        // Every connector is drawable, even one running backwards.
        let _ = page_html(&f, &[], &PagePalette::default());
    }

    #[test]
    fn the_flow_page_restores_the_scheme_before_the_first_paint() {
        let html = page_html(&flow(), &[], &PagePalette::default());
        crate::webtheme::assert_scheme_restored_before_paint("flow page", &html);
    }

    #[test]
    fn opening_writes_the_page_only_where_auto_run_is_offered() {
        use v2_lib::commands::api_templates::write_flow_page_at;
        let dir = tempfile::tempdir().unwrap();
        let (org, project) = ("acme", "Web");
        let palette = PagePalette::default();

        let refused = write_flow_page_at(false, dir.path(), org, project, "pms-performance-cycle", &palette);
        assert_eq!(refused.unwrap_err(), "not available in this build");

        let missing = write_flow_page_at(true, dir.path(), org, project, "pms-performance-cycle", &palette);
        assert_eq!(missing.unwrap_err(), "the flow pms-performance-cycle is no longer saved");

        v2_lib::api_templates::flow_store::save(dir.path(), org, project, &flow()).unwrap();
        let path = write_flow_page_at(true, dir.path(), org, project, "pms-performance-cycle", &palette).unwrap();
        let html = std::fs::read_to_string(&path).unwrap();
        assert!(html.contains("<title>Performance cycle wizard - flow</title>"), "{html}");
        let _ = std::fs::remove_file(path);
    }
}
