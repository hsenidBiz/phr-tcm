//! Fixtures: the save rules, the store and its run history, and the delete
//! template's kind. Validation is a pure function over a template lookup,
//! so most of these tests touch no files.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use v2_lib::api_templates::fixture::{validate, Creates, Fixture, FixtureRun, FixtureStep};
use v2_lib::api_templates::{fixture_store, parse_draft, store, ApiTemplate, Proven};

const ORG: &str = "Org";
const PROJECT: &str = "Proj";

/// A template whose single step captures `cycleId` and `cycleName`; both
/// are declared outputs. `effect` is `create` or `delete`.
fn template(id: &str, effect: &str, proven: bool) -> ApiTemplate {
    let mut v = json!({
        "id": id,
        "title": "A template",
        "module": "PMS",
        "effect": effect,
        "description": "d",
        "sources": ["x:1"],
        "antiforgery": { "page": "/hr/x" },
        "params": [{ "name": "cycleName", "type": "string", "required": true }],
        "steps": [{
            "name": "Save", "method": "POST", "path": "/hr/x",
            "form": { "CycleName": "{{cycleName}}" },
            "expect": { "status": 200 },
            "capture": { "cycleId": "$.cycleId", "cycleName": "$.cycleName", "hidden": "$.hidden" }
        }],
        "outputs": ["cycleId", "cycleName"]
    });
    if effect == "delete" {
        v["deletes_kind"] = json!("cycle");
    }
    let mut t: ApiTemplate = serde_json::from_value(v).unwrap();
    if proven {
        t.proven = Some(Proven {
            at: "2026-10-06 09:00:00".into(),
            origin: "https://hr.example.internal".into(),
            account: "admin".into(),
            outputs: BTreeMap::new(),
            environment: None,
        });
    }
    t
}

fn lookup(id: &str) -> Option<ApiTemplate> {
    match id {
        "make-cycle" | "make-suite" => Some(template(id, "create", true)),
        "unproven" => Some(template(id, "create", false)),
        "remove-cycle" => Some(template(id, "delete", true)),
        _ => None,
    }
}

fn step(template: &str, params: &[(&str, &str)]) -> FixtureStep {
    FixtureStep { template: template.into(), params: params.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect() }
}

fn fixture() -> Fixture {
    Fixture {
        id: "cycle-fixture".into(),
        name: "A cycle".into(),
        account: "admin".into(),
        steps: vec![
            step("make-cycle", &[("cycleName", "{{prefix}} cycle {{now:yyyyMMdd}}")]),
            step("make-suite", &[("cycleName", "{{steps.1.cycleName}}"), ("cycleId", "{{steps.1.cycleId}}")]),
        ],
        outputs: [("cycle_id".to_string(), "{{steps.1.cycleId}}".to_string())].into(),
        creates: vec![Creates {
            kind: "cycle".into(),
            id: "{{steps.1.cycleId}}".into(),
            name: "{{steps.1.cycleName}}".into(),
        }],
    }
}

fn problems(f: &Fixture) -> Vec<String> {
    validate(f, &lookup).unwrap_err()
}

#[test]
fn a_valid_fixture_has_no_problems() {
    validate(&fixture(), &lookup).unwrap();
}

#[test]
fn a_fixture_needs_a_step() {
    let f = Fixture { steps: vec![], outputs: BTreeMap::new(), creates: vec![], ..fixture() };
    assert_eq!(problems(&f), vec!["a fixture needs at least one step"]);
}

#[test]
fn an_unproven_or_unknown_template_is_refused() {
    let mut f = fixture();
    f.steps[1] = step("unproven", &[]);
    f.steps.push(step("nowhere", &[]));
    let p = problems(&f);
    assert!(p.contains(&"step 2: template unproven is not proven".to_string()), "{p:?}");
    assert!(p.contains(&"step 3: template nowhere is not proven".to_string()), "{p:?}");
}

#[test]
fn a_delete_template_is_refused() {
    let mut f = fixture();
    f.steps[1] = step("remove-cycle", &[]);
    let p = problems(&f);
    assert_eq!(p, vec!["step 2: template remove-cycle deletes, and a fixture never deletes"], "{p:?}");
}

#[test]
fn a_step_may_only_read_an_earlier_step() {
    let mut f = fixture();
    // Its own step, a later step, and step zero.
    f.steps[1].params.insert("a".into(), "{{steps.2.cycleId}}".into());
    f.steps[1].params.insert("b".into(), "{{steps.3.cycleId}}".into());
    f.steps[1].params.insert("c".into(), "{{steps.0.cycleId}}".into());
    let p = problems(&f);
    for m in ["2", "3", "0"] {
        let want = format!("step 2: {{{{steps.{m}.cycleId}}}} does not come from an earlier step");
        assert!(p.contains(&want), "{want} not in {p:?}");
    }
    assert_eq!(p.len(), 3, "{p:?}");
}

#[test]
fn a_step_may_not_read_a_value_the_template_does_not_declare() {
    let mut f = fixture();
    // `hidden` is captured by the template's step but is not a declared output.
    f.steps[1].params.insert("a".into(), "{{steps.1.hidden}}".into());
    f.steps[1].params.insert("b".into(), "{{steps.1.nothing}}".into());
    let p = problems(&f);
    assert!(p.contains(&"step 2: {{steps.1.hidden}} does not come from an earlier step".to_string()), "{p:?}");
    assert!(p.contains(&"step 2: {{steps.1.nothing}} does not come from an earlier step".to_string()), "{p:?}");
}

#[test]
fn an_output_must_come_from_a_step() {
    let mut f = fixture();
    f.outputs.insert("bad".into(), "{{steps.1.nothing}}".into());
    f.outputs.insert("late".into(), "{{steps.9.cycleId}}".into());
    f.outputs.insert("plain".into(), "just text".into());
    let p = problems(&f);
    assert!(p.contains(&"output bad: {{steps.1.nothing}} does not come from a step".to_string()), "{p:?}");
    assert!(p.contains(&"output late: {{steps.9.cycleId}} does not come from a step".to_string()), "{p:?}");
    assert!(p.contains(&"output plain: just text does not come from a step".to_string()), "{p:?}");
}

#[test]
fn a_creates_entry_must_come_from_a_step() {
    let mut f = fixture();
    f.creates[0].id = "{{steps.1.nothing}}".into();
    f.creates[0].name = "fixed name".into();
    let p = problems(&f);
    assert!(p.contains(&"creates: {{steps.1.nothing}} does not come from a step".to_string()), "{p:?}");
    assert!(p.contains(&"creates: fixed name does not come from a step".to_string()), "{p:?}");
}

#[test]
fn a_fixture_with_creates_must_use_the_prefix() {
    let mut f = fixture();
    f.steps[0].params.insert("cycleName".into(), "plain name".into());
    assert_eq!(problems(&f), vec!["a fixture with creates must use {{prefix}} in a step's params"]);
    // Without creates the prefix is not required.
    f.creates.clear();
    validate(&f, &lookup).unwrap();
}

#[test]
fn a_valid_fixture_saves_and_loads() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    store::save(root, ORG, PROJECT, &template("make-cycle", "create", true)).unwrap();
    store::save(root, ORG, PROJECT, &template("make-suite", "create", true)).unwrap();
    fixture_store::save(root, ORG, PROJECT, &fixture()).unwrap();
    assert!(fixture_store::fixtures_dir(root, ORG, PROJECT).join("cycle-fixture.json").is_file());
    assert_eq!(fixture_store::load(root, ORG, PROJECT, "cycle-fixture").unwrap(), Some(fixture()));
    assert_eq!(fixture_store::load(root, ORG, PROJECT, "other").unwrap(), None);
    let listed = fixture_store::list(root, ORG, PROJECT).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].fixture, fixture());
    assert!(listed[0].runs.is_empty());
    // Another project sees none.
    assert!(fixture_store::list(root, ORG, "Elsewhere").unwrap().is_empty());
}

#[test]
fn saving_checks_the_templates_on_disk_and_writes_nothing_when_refused() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    store::save(root, ORG, PROJECT, &template("make-cycle", "create", true)).unwrap();
    // make-suite was never saved.
    let err = fixture_store::save(root, ORG, PROJECT, &fixture()).unwrap_err();
    assert!(err.contains(&"step 2: template make-suite is not proven".to_string()), "{err:?}");
    assert_eq!(fixture_store::load(root, ORG, PROJECT, "cycle-fixture").unwrap(), None);

    let bad_id = Fixture { id: "Bad Id".into(), ..fixture() };
    assert_eq!(fixture_store::save(root, ORG, PROJECT, &bad_id).unwrap_err(), vec!["'Bad Id' is not a valid fixture id"]);
}

fn run(n: usize, ok: bool) -> FixtureRun {
    FixtureRun {
        at: format!("2026-10-06 10:{n:02}:00"),
        ok,
        failed_step: if ok { None } else { Some(2) },
        detail: if ok { None } else { Some("step 2 failed".into()) },
        outputs: BTreeMap::from([("cycle_id".to_string(), Value::from(n as i64))]),
    }
}

#[test]
fn the_run_history_keeps_twenty_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    store::save(root, ORG, PROJECT, &template("make-cycle", "create", true)).unwrap();
    store::save(root, ORG, PROJECT, &template("make-suite", "create", true)).unwrap();
    fixture_store::save(root, ORG, PROJECT, &fixture()).unwrap();
    for n in 0..25 {
        fixture_store::append_run(root, ORG, PROJECT, "cycle-fixture", run(n, true)).unwrap();
    }
    let runs = &fixture_store::list(root, ORG, PROJECT).unwrap()[0].runs;
    assert_eq!(runs.len(), 20);
    assert_eq!(runs[0].at, "2026-10-06 10:24:00", "newest first");
    assert_eq!(runs[19].at, "2026-10-06 10:05:00", "the oldest five were dropped");
}

#[test]
fn current_outputs_are_the_newest_successful_runs() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    assert_eq!(fixture_store::current_outputs(root, ORG, PROJECT, "cycle-fixture"), None, "never ran");
    fixture_store::append_run(root, ORG, PROJECT, "cycle-fixture", run(1, false)).unwrap();
    assert_eq!(fixture_store::current_outputs(root, ORG, PROJECT, "cycle-fixture"), None, "only failures");
    fixture_store::append_run(root, ORG, PROJECT, "cycle-fixture", run(2, true)).unwrap();
    fixture_store::append_run(root, ORG, PROJECT, "cycle-fixture", run(3, false)).unwrap();
    let out = fixture_store::current_outputs(root, ORG, PROJECT, "cycle-fixture").unwrap();
    assert_eq!(out["cycle_id"], json!(2), "the failed run after it is skipped");
}

#[test]
fn a_delete_template_must_say_which_kind_it_deletes() {
    let mut v = serde_json::to_value(template("remove-cycle", "delete", false)).unwrap();
    v.as_object_mut().unwrap().remove("deletes_kind");
    let err = parse_draft(&v).unwrap_err();
    assert!(err.contains(&"a delete template must say which kind it deletes".to_string()), "{err:?}");

    v["deletes_kind"] = json!("  ");
    assert!(parse_draft(&v).is_err(), "a blank kind is no kind");

    v["deletes_kind"] = json!("cycle");
    assert_eq!(parse_draft(&v).unwrap().deletes_kind.as_deref(), Some("cycle"));

    // Any other effect needs none, and writes none back.
    let create = serde_json::to_value(template("make-cycle", "create", false)).unwrap();
    assert!(create.get("deletes_kind").is_none());
    assert!(parse_draft(&create).is_ok());
}
