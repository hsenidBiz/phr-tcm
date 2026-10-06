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

fn no_flow(_: &str) -> Option<v2_lib::api_templates::flow::Flow> {
    None
}

fn problems(f: &Fixture) -> Vec<String> {
    validate(f, &lookup, &no_flow).unwrap_err()
}

#[test]
fn a_valid_fixture_has_no_problems() {
    validate(&fixture(), &lookup, &no_flow).unwrap();
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
    validate(&f, &lookup, &no_flow).unwrap();
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

// ---- placeholder rules folded in from the part 5 Task 1 review -----------

#[test]
fn a_step_index_must_be_all_digits() {
    let mut f = fixture();
    f.steps[1].params.insert("a".into(), "{{steps.+1.cycleId}}".into());
    let p = problems(&f);
    assert_eq!(p, vec!["step 2: {{steps.+1.cycleId}} does not come from an earlier step"], "{p:?}");
}

#[test]
fn a_step_past_the_last_one_is_refused() {
    let mut f = fixture();
    f.steps[1].params.insert("a".into(), "{{steps.10.cycleId}}".into());
    let p = problems(&f);
    assert_eq!(p, vec!["step 2: {{steps.10.cycleId}} does not come from an earlier step"], "{p:?}");
}

#[test]
fn whitespace_inside_the_braces_is_accepted() {
    let mut f = fixture();
    f.steps[0].params.insert("cycleName".into(), "{{ prefix }} cycle".into());
    f.steps[1].params.insert("cycleId".into(), "{{ steps.1.cycleId }}".into());
    f.outputs.insert("spaced".into(), " {{ steps.1.cycleId }} ".into());
    f.creates[0].id = "{{ steps.1.cycleId }}".into();
    validate(&f, &lookup, &no_flow).unwrap();
}

#[test]
fn a_malformed_step_reference_is_refused() {
    let mut f = fixture();
    f.steps[1].params.insert("a".into(), "{{steps.1}}".into());
    let p = problems(&f);
    assert_eq!(p, vec!["step 2: {{steps.1}} does not come from an earlier step"], "{p:?}");
}

#[test]
fn a_placeholder_inside_other_text_is_checked_and_accepted() {
    let mut f = fixture();
    f.steps[1].params.insert("cycleName".into(), "copy of {{steps.1.cycleName}} for {{prefix}}".into());
    validate(&f, &lookup, &no_flow).unwrap();
    f.steps[1].params.insert("cycleName".into(), "copy of {{steps.2.cycleName}}".into());
    assert_eq!(problems(&f), vec!["step 2: {{steps.2.cycleName}} does not come from an earlier step"]);
}

#[test]
fn step_one_may_not_read_step_one() {
    let mut f = fixture();
    f.steps[0].params.insert("other".into(), "{{steps.1.cycleId}}".into());
    let p = problems(&f);
    assert_eq!(p, vec!["step 1: {{steps.1.cycleId}} does not come from an earlier step"], "{p:?}");
}

#[test]
fn the_same_bad_placeholder_in_several_params_is_said_once() {
    let mut f = fixture();
    f.steps[1].params.insert("a".into(), "{{steps.3.cycleId}}".into());
    f.steps[1].params.insert("b".into(), "{{steps.3.cycleId}}".into());
    f.steps[1].params.insert("c".into(), "x {{steps.3.cycleId}} y".into());
    assert_eq!(problems(&f), vec!["step 2: {{steps.3.cycleId}} does not come from an earlier step"]);
}

#[test]
fn now_is_written_with_its_format_letters() {
    use v2_lib::api_templates::fixture_run::{format_now, Clock};
    let clock = Clock { year: 2026, month: 3, day: 7, hour: 9, minute: 5, second: 2 };
    assert_eq!(format_now(&clock, "yyyyMMdd"), "20260307");
    assert_eq!(format_now(&clock, "yyyy-MM-dd HH:mm:ss"), "2026-03-07 09:05:02");
    assert_eq!(format_now(&clock, "run dd/MM"), "run 07/03", "other letters stay as they are");
}

#[test]
fn removing_a_fixture_takes_its_history_and_needs_auto_run() {
    use v2_lib::commands::api_templates::remove_fixture_at;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    store::save(root, ORG, PROJECT, &template("make-cycle", "create", true)).unwrap();
    store::save(root, ORG, PROJECT, &template("make-suite", "create", true)).unwrap();
    fixture_store::save(root, ORG, PROJECT, &fixture()).unwrap();
    fixture_store::append_run(root, ORG, PROJECT, "cycle-fixture", run(1, true)).unwrap();

    assert_eq!(
        remove_fixture_at(false, root, ORG, PROJECT, "cycle-fixture").unwrap_err(),
        "not available in this build"
    );
    assert!(fixture_store::load(root, ORG, PROJECT, "cycle-fixture").unwrap().is_some());

    remove_fixture_at(true, root, ORG, PROJECT, "cycle-fixture").unwrap();
    assert_eq!(fixture_store::load(root, ORG, PROJECT, "cycle-fixture").unwrap(), None);
    assert_eq!(fixture_store::current_outputs(root, ORG, PROJECT, "cycle-fixture"), None, "its history went too");
    remove_fixture_at(true, root, ORG, PROJECT, "cycle-fixture").unwrap();
}

/// The guide's fixture sentences (design doc section 1, "The assistant's
/// tools").
#[test]
fn the_guide_explains_fixtures() {
    let text = v2_lib::api_templates::guide::text(&[], None);
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(one_line.contains("## Fixtures"), "no Fixtures section");
    for tool in ["save_api_fixture", "run_api_fixture", "list_api_fixtures"] {
        assert!(one_line.contains(tool), "no {tool}");
    }
    assert!(one_line.contains("Build a fixture from proven templates"), "{one_line}");
    assert!(one_line.contains("Name what it makes with `{{prefix}}`"), "{one_line}");
    assert!(one_line.contains("use Rebuild (run the fixture again), never a hand fix"), "{one_line}");
    assert!(!text.contains('\u{2014}') && !text.contains('\u{2013}'), "no em or en dashes");
}

// ---- a fixture performs a flow's earlier stages itself --------------------

/// A three-stage flow: setup creates the cycle, rules needs setup, publish
/// needs rules.
fn cycle_flow() -> v2_lib::api_templates::flow::Flow {
    serde_json::from_value(json!({
        "id": "cycle-flow",
        "title": "Performance cycle wizard",
        "module": "PMS",
        "subject": { "name": "cycleId", "type": "number" },
        "stages": [
            { "id": "setup", "title": "Cycle setup", "creates": true, "check": "SELECT 1 FROM t WHERE id = {{cycleId}}" },
            { "id": "rules", "title": "Evaluation rules", "requires": ["setup"], "check": "SELECT 1 FROM r WHERE id = {{cycleId}}" },
            { "id": "publish", "title": "Publish", "requires": ["rules"], "check": "SELECT 1 FROM p WHERE id = {{cycleId}}" }
        ]
    }))
    .unwrap()
}

fn flow_lookup(id: &str) -> Option<ApiTemplate> {
    let on = |stage: &str| {
        let mut t = template(id, "create", true);
        t.stage = Some(v2_lib::api_templates::flow::StageRef { flow: "cycle-flow".into(), id: stage.into() });
        Some(t)
    };
    match id {
        "flow-setup" => on("setup"),
        "flow-rules" => on("rules"),
        "flow-publish" => on("publish"),
        _ => lookup(id),
    }
}

fn flows(id: &str) -> Option<v2_lib::api_templates::flow::Flow> {
    (id == "cycle-flow").then(cycle_flow)
}

fn flow_fixture(steps: Vec<FixtureStep>) -> Fixture {
    Fixture { steps, outputs: BTreeMap::new(), creates: vec![], ..fixture() }
}

fn flow_problems(f: &Fixture) -> Vec<String> {
    validate(f, &flow_lookup, &flows).unwrap_err()
}

#[test]
fn a_later_stage_alone_is_refused() {
    let f = flow_fixture(vec![step("flow-publish", &[("cycleId", "274")])]);
    assert_eq!(
        flow_problems(&f),
        vec![
            "step 1: template flow-publish performs Publish of flow Performance cycle wizard, so the steps before it must perform Cycle setup, Evaluation rules",
            "step 1: template flow-publish performs Publish of flow Performance cycle wizard, so its cycleId must be one {{steps.<m>.<output>}} of an earlier step on that flow",
        ]
    );
}

#[test]
fn a_fixture_that_performs_every_stage_in_order_is_accepted() {
    let f = flow_fixture(vec![
        step("flow-setup", &[("cycleName", "{{prefix}} cycle")]),
        step("flow-rules", &[("cycleId", "{{steps.1.cycleId}}")]),
        step("flow-publish", &[("cycleId", "{{ steps.1.cycleId }}")]),
    ]);
    validate(&f, &flow_lookup, &flows).unwrap();
    // A template on no flow is unaffected.
    validate(&fixture(), &flow_lookup, &flows).unwrap();
}

/// Every stage acts on the one record the flow's creating step made: here
/// step 4 approves record A while steps 2 and 3 made and submitted B.
#[test]
fn stages_acting_on_two_records_are_refused() {
    let f = flow_fixture(vec![
        step("flow-setup", &[("cycleName", "{{prefix}} A")]),
        step("flow-setup", &[("cycleName", "{{prefix}} B")]),
        step("flow-rules", &[("cycleId", "{{steps.2.cycleId}}")]),
        step("flow-publish", &[("cycleId", "{{steps.1.cycleId}}")]),
    ]);
    let p = flow_problems(&f);
    let sentence = |n: usize, id: &str, title: &str| {
        format!("step {n}: template {id} performs {title} of flow Performance cycle wizard, so its cycleId must be one {{{{steps.<m>.<output>}}}} of an earlier step on that flow")
    };
    assert_eq!(p, vec![sentence(3, "flow-rules", "Evaluation rules"), sentence(4, "flow-publish", "Publish")]);
}

/// A subject that is another output of the creating step is not the record.
#[test]
fn a_subject_that_is_not_the_records_output_is_refused() {
    let f = flow_fixture(vec![
        step("flow-setup", &[("cycleName", "{{prefix}} A")]),
        step("flow-rules", &[("cycleId", "{{steps.1.cycleName}}")]),
    ]);
    assert_eq!(
        flow_problems(&f),
        vec!["step 2: template flow-rules performs Evaluation rules of flow Performance cycle wizard, so its cycleId must be one {{steps.<m>.<output>}} of an earlier step on that flow"]
    );
}

/// A one-record chain with a step on no flow between its stages.
#[test]
fn a_one_record_chain_is_accepted() {
    let f = flow_fixture(vec![
        step("flow-setup", &[("cycleName", "{{prefix}} A")]),
        step("make-cycle", &[("cycleName", "{{steps.1.cycleName}} copy")]),
        step("flow-rules", &[("cycleId", "{{steps.1.cycleId}}")]),
        step("flow-publish", &[("cycleId", "{{steps.1.cycleId}}")]),
    ]);
    validate(&f, &flow_lookup, &flows).unwrap();
}

/// A template whose flow, or whose stage, is gone is refused at save.
#[test]
fn a_step_on_a_removed_flow_or_stage_is_refused() {
    let f = flow_fixture(vec![
        step("flow-setup", &[("cycleName", "{{prefix}} A")]),
        step("flow-rules", &[("cycleId", "{{steps.1.cycleId}}")]),
    ]);
    assert_eq!(
        validate(&f, &flow_lookup, &no_flow).unwrap_err(),
        vec![
            "step 1: template flow-setup belongs to flow cycle-flow, which is no longer saved",
            "step 2: template flow-rules belongs to flow cycle-flow, which is no longer saved",
        ]
    );
    let without_rules = |id: &str| {
        flows(id).map(|mut fl| {
            fl.stages.retain(|s| s.id != "rules");
            fl.stages.iter_mut().for_each(|s| s.requires.retain(|r| r != "rules"));
            fl
        })
    };
    assert_eq!(
        validate(&f, &flow_lookup, &without_rules).unwrap_err(),
        vec!["step 2: template flow-rules belongs to flow cycle-flow, but stage rules is no longer in flow cycle-flow"]
    );
}

#[test]
fn a_missing_middle_stage_is_refused() {
    let f = flow_fixture(vec![
        step("flow-setup", &[("cycleName", "{{prefix}} cycle")]),
        step("flow-publish", &[("cycleId", "{{steps.1.cycleId}}")]),
    ]);
    assert_eq!(
        flow_problems(&f),
        vec!["step 2: template flow-publish performs Publish of flow Performance cycle wizard, so the steps before it must perform Evaluation rules"]
    );
}

#[test]
fn a_subject_from_a_step_not_on_the_flow_is_refused() {
    let f = flow_fixture(vec![
        step("flow-setup", &[("cycleName", "{{prefix}} cycle")]),
        step("make-cycle", &[("cycleName", "{{prefix}} other")]),
        step("flow-rules", &[("cycleId", "{{steps.2.cycleId}}")]),
    ]);
    assert_eq!(
        flow_problems(&f),
        vec!["step 3: template flow-rules performs Evaluation rules of flow Performance cycle wizard, so its cycleId must be one {{steps.<m>.<output>}} of an earlier step on that flow"]
    );
}

/// Running fixtures against the fake page the template runner's own tests
/// use (`api_templates_runner`): one browser, one sign-in, each template's
/// token page and steps in turn.
mod running {
    use crate::api_templates_runner::{answer, rig, Rig, ORG, PAGE, PROJECT, QUICK_PAUSES};
    use crate::common::quick;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    use v2_lib::api_templates::fixture::{Creates, Fixture, FixtureRun, FixtureStep};
    use v2_lib::api_templates::fixture_run::{prefix_warning, run_fixture_within, Clock, FixtureReport};
    use v2_lib::api_templates::runner::{preflight, Mode, RunRequest, RUN_LIMIT};
    use v2_lib::api_templates::{fixture_store, store, ApiTemplate, Proven};
    use v2_lib::autorun::test_made;

    const CLOCK: Clock = Clock { year: 2026, month: 10, day: 6, hour: 14, minute: 30, second: 0 };

    fn proven(mut t: ApiTemplate) -> ApiTemplate {
        t.proven = Some(Proven {
            at: "2026-10-01 09:00:00".into(),
            origin: "https://hr.example.internal".into(),
            account: "admin".into(),
            outputs: BTreeMap::new(),
            environment: None,
        });
        t
    }

    /// Makes a cycle in one request; hands back its id and name.
    fn make_cycle() -> ApiTemplate {
        proven(
            serde_json::from_value(json!({
                "id": "make-cycle", "title": "Make a cycle", "module": "PMS", "effect": "create",
                "description": "d", "sources": ["x:1"], "antiforgery": { "page": PAGE },
                "params": [ { "name": "cycleName", "type": "string", "required": true } ],
                "steps": [ { "name": "Cycle setup", "method": "POST", "path": "/hr/pmsv10/performancecycle",
                             "query": { "handler": "SaveProgress" }, "form": { "CycleName": "{{cycleName}}" },
                             "capture": { "cycleId": "$.cycleId", "cycleName": "$.cycleName" } } ],
                "outputs": ["cycleId", "cycleName"]
            }))
            .unwrap(),
        )
    }

    /// Adds a suite to a cycle given as a number.
    fn add_suite() -> ApiTemplate {
        proven(
            serde_json::from_value(json!({
                "id": "add-suite", "title": "Add a suite", "module": "PMS", "effect": "edit",
                "description": "d", "sources": ["x:1"], "antiforgery": { "page": PAGE },
                "params": [ { "name": "cycleId", "type": "number", "required": true } ],
                "steps": [ { "name": "Suite", "method": "POST", "path": "/hr/pmsv10/performancecycle",
                             "query": { "handler": "AddSuite" }, "form": { "CycleId": "{{cycleId}}" },
                             "capture": { "suiteId": "$.suiteId" } } ],
                "outputs": ["suiteId"]
            }))
            .unwrap(),
        )
    }

    fn step(template: &str, params: &[(&str, &str)]) -> FixtureStep {
        FixtureStep {
            template: template.into(),
            params: params.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        }
    }

    fn fixture() -> Fixture {
        Fixture {
            id: "draft-cycle".into(),
            name: "A draft cycle".into(),
            account: "admin".into(),
            steps: vec![
                step("make-cycle", &[("cycleName", "{{prefix}} cycle {{now:yyyyMMdd}}")]),
                step("add-suite", &[("cycleId", "{{steps.1.cycleId}}")]),
            ],
            outputs: [
                ("cycle_id".to_string(), "{{steps.1.cycleId}}".to_string()),
                ("suite_id".to_string(), "{{steps.2.suiteId}}".to_string()),
            ]
            .into(),
            creates: vec![Creates {
                kind: "cycle".into(),
                id: "{{steps.1.cycleId}}".into(),
                name: "{{steps.1.cycleName}}".into(),
            }],
        }
    }

    /// A rig whose root holds both templates and the fixture.
    fn rig_with(responses: Vec<Value>, f: &Fixture, templates: &[ApiTemplate]) -> Rig {
        rig_broken(responses, None, f, templates)
    }

    /// `rig_with` on a page that lacks `broken`, so signing in fails.
    fn rig_broken(responses: Vec<Value>, broken: Option<&'static str>, f: &Fixture, templates: &[ApiTemplate]) -> Rig {
        let r = rig(responses, broken);
        for t in templates {
            store::save(r.root.path(), ORG, PROJECT, t).unwrap();
        }
        fixture_store::save(r.root.path(), ORG, PROJECT, f).unwrap();
        r
    }

    async fn run(r: &mut Rig, f: &Fixture) -> FixtureReport {
        run_fixture_within(&mut r.browsers, r.root.path(), ORG, PROJECT, f, &quick(), RUN_LIMIT, &QUICK_PAUSES, CLOCK)
            .await
    }

    fn history(r: &Rig, id: &str) -> Vec<FixtureRun> {
        fixture_store::list(r.root.path(), ORG, PROJECT)
            .unwrap()
            .into_iter()
            .find(|s| s.fixture.id == id)
            .map(|s| s.runs)
            .unwrap_or_default()
    }

    fn the_cycle() -> Value {
        answer(200, json!({ "cycleId": 274, "cycleName": "AUTOTEST cycle 20261006" }))
    }

    #[tokio::test]
    async fn two_steps_pass_a_value_between_them_in_one_signed_in_browser() {
        let _act = crate::serial::activity_log();
        let f = fixture();
        let mut r = rig_with(vec![the_cycle(), answer(200, json!({ "suiteId": 9 }))], &f, &[make_cycle(), add_suite()]);
        let report = run(&mut r, &f).await;
        assert!(report.ok, "{report:?}");
        assert_eq!(report.failed, None);
        assert_eq!(report.message(), "every step passed (2 steps)");
        assert_eq!(report.steps.len(), 2);
        assert_eq!(
            report.outputs,
            BTreeMap::from([("cycle_id".to_string(), json!(274)), ("suite_id".to_string(), json!(9))])
        );

        let fetched = r.fetched();
        assert_eq!(fetched.len(), 2);
        // `{{prefix}}` is the active environment's prefix; `{{now:yyyyMMdd}}`
        // the run's start, in local time.
        assert_eq!(fetched[0][0]["body"]["fields"]["CycleName"], json!("AUTOTEST cycle 20261006"));
        // Step 1's captured id reached step 2.
        assert_eq!(fetched[1][0]["body"]["fields"]["CycleId"], json!("274"));

        // One browser, one sign-in, each template on its own token page.
        assert_eq!(r.browsers.opened, 1);
        assert_eq!(r.browsers.closed, 1);
        assert_eq!(r.sign_ins(), 1);
        assert_eq!(r.navigations_to_the_page(), 2);

        // The run is in the history and its outputs are current.
        let runs = history(&r, "draft-cycle");
        assert_eq!(runs.len(), 1);
        assert!(runs[0].ok);
        assert_eq!(runs[0].failed_step, None);
        assert_eq!(fixture_store::current_outputs(r.root.path(), ORG, PROJECT, "draft-cycle").unwrap(), report.outputs);

        // What it made is recorded as test-made, present, with no warning.
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let made = test_made::list(r.root.path());
        assert_eq!(made, report.made);
        assert_eq!(made.len(), 1);
        let m = &made[0];
        assert_eq!((m.kind.as_str(), m.id.as_str(), m.name.as_str()), ("cycle", "274", "AUTOTEST cycle 20261006"));
        assert_eq!(m.status, "present");
        assert_eq!(m.fixture, "draft-cycle");
        assert_eq!(m.case_id, None);
        assert!(m.run_id.starts_with("run-"), "{}", m.run_id);
        let env = v2_lib::environments::active(r.root.path()).unwrap();
        assert_eq!(m.environment, env.id);
    }

    /// Review Focus 1: a later step failing stops the run with that step's
    /// own sentence, still records what step 1 made, and keeps the outputs
    /// of the run before.
    #[tokio::test]
    async fn a_failed_step_stops_the_run_records_what_was_made_and_keeps_the_outputs() {
        let _act = crate::serial::activity_log();
        let f = fixture();
        let mut r = rig_with(vec![the_cycle(), answer(500, json!({ "error": "no" }))], &f, &[make_cycle(), add_suite()]);
        let before = FixtureRun {
            at: "2026-10-05 10:00:00".into(),
            ok: true,
            failed_step: None,
            detail: None,
            outputs: BTreeMap::from([("cycle_id".to_string(), json!(100)), ("suite_id".to_string(), json!(1))]),
        };
        fixture_store::append_run(r.root.path(), ORG, PROJECT, "draft-cycle", before.clone()).unwrap();

        let report = run(&mut r, &f).await;
        assert!(!report.ok);
        assert!(report.outputs.is_empty(), "a failed run has no outputs");
        assert_eq!(report.steps.len(), 2);
        let want = format!("step 2: {}", report.steps[1].message());
        assert_eq!(report.failed.as_deref(), Some(want.as_str()));
        assert!(want.starts_with("step 2: nothing had been captured yet; failed at Suite (AddSuite): "), "{want}");

        let made = test_made::list(r.root.path());
        assert_eq!(made.len(), 1, "{made:?}");
        assert_eq!((made[0].id.as_str(), made[0].status.as_str()), ("274", "present"));

        let runs = history(&r, "draft-cycle");
        assert_eq!(runs.len(), 2);
        assert!(!runs[0].ok);
        assert_eq!(runs[0].failed_step, Some(2));
        assert_eq!(runs[0].detail.as_deref(), Some(want.as_str()));
        assert_eq!(
            fixture_store::current_outputs(r.root.path(), ORG, PROJECT, "draft-cycle").unwrap(),
            before.outputs,
            "the outputs before the failed Rebuild are still current"
        );
        assert_eq!(r.browsers.closed, 1, "the browser is closed on the way out");
    }

    /// A step that fails after capturing a declared output still made it:
    /// its `created` map is read under that output's name.
    #[tokio::test]
    async fn what_a_failed_step_captured_is_still_recorded() {
        let _act = crate::serial::activity_log();
        let mut t = make_cycle();
        t.steps.push(
            serde_json::from_value(json!({
                "name": "Evaluation rules", "method": "POST", "path": "/hr/pmsv10/performancecycle",
                "query": { "handler": "SaveEvalRulesProgress" }, "form": { "CycleId": "{{cycleId}}" }
            }))
            .unwrap(),
        );
        let f = Fixture { steps: vec![fixture().steps.remove(0)], outputs: BTreeMap::new(), ..fixture() };
        let mut r = rig_with(vec![the_cycle(), answer(400, json!({ "success": false }))], &f, &[t]);
        let report = run(&mut r, &f).await;
        assert!(!report.ok);
        assert!(report.failed.as_deref().unwrap().starts_with("step 1: cycleId 274, cycleName AUTOTEST cycle 20261006 created; failed at Evaluation rules"), "{:?}", report.failed);
        let made = test_made::list(r.root.path());
        assert_eq!(made.len(), 1, "{made:?}");
        assert_eq!((made[0].id.as_str(), made[0].name.as_str()), ("274", "AUTOTEST cycle 20261006"));
    }

    /// Nothing captured: the entry's id does not resolve, so it is skipped.
    #[tokio::test]
    async fn an_entry_whose_id_was_never_captured_is_skipped() {
        let _act = crate::serial::activity_log();
        let f = fixture();
        let mut r = rig_with(vec![answer(500, json!({}))], &f, &[make_cycle(), add_suite()]);
        let report = run(&mut r, &f).await;
        assert!(!report.ok);
        assert!(report.failed.as_deref().unwrap().starts_with("step 1: "), "{:?}", report.failed);
        assert!(report.made.is_empty());
        assert!(test_made::list(r.root.path()).is_empty());
        assert_eq!(history(&r, "draft-cycle")[0].failed_step, Some(1));
    }

    #[tokio::test]
    async fn a_name_without_the_prefix_is_recorded_with_a_warning() {
        let _act = crate::serial::activity_log();
        let f = fixture();
        let mut r = rig_with(
            vec![answer(200, json!({ "cycleId": 275, "cycleName": "Hand made cycle" })), answer(200, json!({ "suiteId": 9 }))],
            &f,
            &[make_cycle(), add_suite()],
        );
        let report = run(&mut r, &f).await;
        assert!(report.ok, "{report:?}");
        assert_eq!(report.warnings, vec![prefix_warning("cycle", "Hand made cycle")]);
        assert_eq!(
            report.warnings[0],
            "cycle Hand made cycle does not start with the test prefix, so Clean up will not find it"
        );
        let made = test_made::list(r.root.path());
        assert_eq!(made.len(), 1);
        assert_eq!(made[0].name, "Hand made cycle", "recorded all the same");
    }

    /// A fixture step whose template became a delete template after the
    /// fixture was saved is refused at run time with the save rule's own
    /// sentence, before any browser opens - and the run is still recorded.
    #[tokio::test]
    async fn a_step_that_now_deletes_is_refused_before_anything_opens() {
        let _act = crate::serial::activity_log();
        let f = fixture();
        let mut r = rig_with(vec![], &f, &[make_cycle(), add_suite()]);
        let mut deleting = add_suite();
        deleting.effect = v2_lib::api_templates::Effect::Delete;
        deleting.deletes_kind = Some("suite".into());
        store::save(r.root.path(), ORG, PROJECT, &deleting).unwrap();

        let report = run(&mut r, &f).await;
        assert!(!report.ok);
        assert_eq!(report.failed.as_deref(), Some("step 2: template add-suite deletes, and a fixture never deletes"));
        assert_eq!(r.browsers.opened, 0, "nothing was opened");
        assert!(report.steps.is_empty());
        let runs = history(&r, "draft-cycle");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].detail, report.failed);
    }

    /// A flow removed after the fixture was saved: the run refuses the step
    /// with the save rule's sentence before any browser opens.
    #[tokio::test]
    async fn a_flow_removed_after_saving_is_refused_before_any_browser() {
        let _act = crate::serial::activity_log();
        let flow: v2_lib::api_templates::flow::Flow = serde_json::from_value(json!({
            "id": "cycle-flow", "title": "Performance cycle wizard", "module": "PMS",
            "subject": { "name": "cycleId", "type": "number" },
            "stages": [
                { "id": "setup", "title": "Cycle setup", "creates": true, "check": "SELECT 1 FROM t WHERE id = {{cycleId}}" },
                { "id": "rules", "title": "Suites", "requires": ["setup"], "check": "SELECT 1 FROM r WHERE id = {{cycleId}}" }
            ]
        }))
        .unwrap();
        let on = |mut t: ApiTemplate, stage: &str| {
            t.stage = Some(v2_lib::api_templates::flow::StageRef { flow: "cycle-flow".into(), id: stage.into() });
            t
        };
        let templates = [on(make_cycle(), "setup"), on(add_suite(), "rules")];
        let f = Fixture { outputs: BTreeMap::new(), ..fixture() };
        let r0 = rig(vec![], None);
        v2_lib::api_templates::flow_store::save(r0.root.path(), ORG, PROJECT, &flow).unwrap();
        for t in &templates {
            store::save(r0.root.path(), ORG, PROJECT, t).unwrap();
        }
        fixture_store::save(r0.root.path(), ORG, PROJECT, &f).unwrap();
        let mut r = r0;
        v2_lib::api_templates::flow_store::remove(r.root.path(), ORG, PROJECT, "cycle-flow").unwrap();

        let report = run(&mut r, &f).await;
        assert_eq!(
            report.failed.as_deref(),
            Some("step 1: template make-cycle belongs to flow cycle-flow, which is no longer saved; step 2: template add-suite belongs to flow cycle-flow, which is no longer saved")
        );
        assert_eq!(r.browsers.opened, 0, "nothing was opened");
        assert_eq!(history(&r, "draft-cycle").len(), 1);
    }

    /// Whether the fixture's account lease is free again: one can be taken
    /// at once.
    async fn lease_is_free(r: &Rig) -> bool {
        let env = v2_lib::environments::active(r.root.path()).unwrap().id;
        v2_lib::autorun::lease::acquire(&env, "admin", v2_lib::autorun::lease::Holder::Browser, std::time::Duration::ZERO)
            .await
            .is_ok()
    }

    /// A step that runs out of its own limit stops the run there: the
    /// browser is closed, the lease let go, the run kept in the history,
    /// and what step 1 made recorded.
    #[tokio::test]
    async fn a_step_that_times_out_stops_the_run_and_leaves_nothing_held() {
        let _act = crate::serial::activity_log();
        let _leases = crate::serial::account_leases();
        let f = fixture();
        let mut r = rig_with(vec![the_cycle()], &f, &[make_cycle(), add_suite()]);
        r.script.lock().unwrap().hang_after = Some(1);
        let report = run_fixture_within(
            &mut r.browsers,
            r.root.path(),
            ORG,
            PROJECT,
            &f,
            &quick(),
            std::time::Duration::from_secs(2),
            &QUICK_PAUSES,
            CLOCK,
        )
        .await;
        assert!(!report.ok);
        let failed = report.failed.clone().unwrap();
        assert!(failed.starts_with("step 2: "), "{failed}");
        assert!(failed.ends_with("the run took longer than 3 minutes"), "{failed}");
        assert_eq!((r.browsers.opened, r.browsers.closed), (1, 1));
        assert!(lease_is_free(&r).await, "the lease was let go");
        let runs = history(&r, "draft-cycle");
        assert_eq!((runs.len(), runs[0].failed_step), (1, Some(2)));
        let made = test_made::list(r.root.path());
        assert_eq!(made.len(), 1);
        assert_eq!(made[0].id, "274");
    }

    #[tokio::test]
    async fn a_failed_sign_in_stops_the_run_and_leaves_nothing_held() {
        let _act = crate::serial::activity_log();
        let _leases = crate::serial::account_leases();
        let f = fixture();
        let mut r = rig_broken(vec![], Some("#go"), &f, &[make_cycle(), add_suite()]);
        let report = run(&mut r, &f).await;
        assert!(!report.ok);
        let failed = report.failed.clone().unwrap();
        assert!(failed.starts_with("step 1: nothing had been captured yet; failed at Sign in: could not sign in"), "{failed}");
        assert_eq!((r.browsers.opened, r.browsers.closed), (1, 1));
        assert!(r.fetched().is_empty(), "nothing was sent");
        assert!(lease_is_free(&r).await, "the lease was let go");
        let runs = history(&r, "draft-cycle");
        assert_eq!((runs.len(), runs[0].failed_step), (1, Some(1)));
        assert!(test_made::list(r.root.path()).is_empty(), "nothing was made");
    }

    #[tokio::test]
    async fn a_refused_lease_stops_the_run_before_any_browser() {
        let _act = crate::serial::activity_log();
        let _leases = crate::serial::account_leases();
        let f = fixture();
        let mut r = rig_with(vec![], &f, &[make_cycle(), add_suite()]);
        let env = v2_lib::environments::active(r.root.path()).unwrap().id;
        let held = v2_lib::autorun::lease::acquire(
            &env,
            "admin",
            v2_lib::autorun::lease::Holder::Browser,
            std::time::Duration::ZERO,
        )
        .await
        .unwrap();
        let report = run(&mut r, &f).await;
        assert!(!report.ok);
        let failed = report.failed.clone().unwrap();
        assert!(failed.starts_with("step 1: nothing had been captured yet; failed at Sign in: "), "{failed}");
        assert!(failed.contains("the Auto Run browser"), "says who had it: {failed}");
        assert_eq!((r.browsers.opened, r.browsers.closed), (0, 0), "no browser was opened");
        let runs = history(&r, "draft-cycle");
        assert_eq!((runs.len(), runs[0].failed_step), (1, Some(1)));
        assert!(test_made::list(r.root.path()).is_empty(), "nothing was made");
        drop(held);
        assert!(lease_is_free(&r).await, "the run took no lease of its own");
    }

    /// Step 1 passed but gave no value for what step 2 reads: step 2 is
    /// refused, the placeholder as written, rather than sent as text.
    #[test]
    fn a_placeholder_with_no_value_is_refused_not_sent() {
        use v2_lib::api_templates::fixture_run::step_values;
        let vars = BTreeMap::from([("prefix".to_string(), json!("AUTOTEST"))]);
        let params = BTreeMap::from([("cycleId".to_string(), "{{ steps.1.cycleId }}".to_string())]);
        assert_eq!(
            step_values(2, &add_suite(), &params, &vars, &CLOCK),
            Err("step 2: {{ steps.1.cycleId }} has no value - the step that should give it did not".to_string())
        );
        // Inside other text too, and a now or prefix the run did not set.
        let params = BTreeMap::from([("cycleName".to_string(), "copy of {{steps.1.cycleName}}".to_string())]);
        assert_eq!(
            step_values(2, &make_cycle(), &params, &vars, &CLOCK).unwrap_err(),
            "step 2: {{steps.1.cycleName}} has no value - the step that should give it did not"
        );
        let params = BTreeMap::from([("cycleName".to_string(), "{{prefix}}".to_string())]);
        assert_eq!(
            step_values(1, &make_cycle(), &params, &BTreeMap::new(), &CLOCK).unwrap_err(),
            "step 1: {{prefix}} has no value - the step that should give it did not"
        );
        // With the value there, it is filled in, and typed for a number param.
        let vars = BTreeMap::from([("steps.1.cycleId".to_string(), json!(274))]);
        let params = BTreeMap::from([("cycleId".to_string(), "{{steps.1.cycleId}}".to_string())]);
        assert_eq!(step_values(2, &add_suite(), &params, &vars, &CLOCK).unwrap()["cycleId"], json!(274));
        // A placeholder that is not a fixture's is the template's own business.
        let params = BTreeMap::from([("cycleName".to_string(), "{{other}}".to_string())]);
        assert!(step_values(1, &make_cycle(), &params, &BTreeMap::new(), &CLOCK).is_ok());
    }

    /// `run_api_template` (a run) refuses a delete template; proving one is
    /// left as it is.
    #[test]
    fn a_run_of_a_delete_template_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        v2_lib::autorun::recipe::save_recipe(dir.path(), ORG, PROJECT, &crate::common::recipe()).unwrap();
        v2_lib::autorun::accounts::save_accounts(dir.path(), &[crate::common::account()]).unwrap();
        let mut t = add_suite();
        t.effect = v2_lib::api_templates::Effect::Delete;
        t.deletes_kind = Some("suite".into());
        let mut values = serde_json::Map::new();
        values.insert("cycleId".into(), json!(274));
        let req = |mode| RunRequest {
            org: ORG.into(),
            project: PROJECT.into(),
            account: "admin".into(),
            values: values.clone(),
            mode,
            template: t.clone(),
        };
        let problems = preflight(dir.path(), &req(Mode::Run), None).unwrap_err();
        assert_eq!(
            problems,
            vec!["template add-suite deletes, and only Clean up test-made drafts runs a delete template"]
        );
        let draft = ApiTemplate { proven: None, ..t.clone() };
        let prove = RunRequest { template: draft, ..req(Mode::Prove { replace: false, why: None }) };
        assert_eq!(preflight(dir.path(), &prove, None), Ok(()), "proving is Task 5's rule");
    }

    /// The bridge's run: the switch first, then the fixture's sentence,
    /// outputs, what it made (kind, id and name) and warnings.
    #[tokio::test]
    async fn the_bridge_runs_a_saved_fixture() {
        use v2_lib::ai_bridge::{api_fixture_run, BridgeContext, API_WRITES_OFF};
        let _root = crate::serial::autorun();
        let _slot = crate::serial::api_template_run();
        let _act = crate::serial::activity_log();
        let f = fixture();
        let Rig { browsers, root, .. } =
            rig_with(vec![the_cycle(), answer(200, json!({ "suiteId": 9 }))], &f, &[make_cycle(), add_suite()]);
        v2_lib::autorun::store::set_root(root.path().to_path_buf());
        let off = BridgeContext { org: ORG.into(), project: PROJECT.into(), ..BridgeContext::default() };
        let body = json!({ "id": "draft-cycle" }).to_string();
        let (status, out) = api_fixture_run(&off, &body, |_| -> crate::api_templates_runner::FakeBrowsers {
            panic!("opened with the switch off")
        }, &quick())
        .await;
        assert_eq!((status, out.as_str()), (400, API_WRITES_OFF));

        let on = BridgeContext { api_writes: true, ..off };
        let (status, out) = api_fixture_run(&on, &body, |_| browsers, &quick()).await;
        assert_eq!(status, 200, "{out}");
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            v,
            json!({
                "ok": true,
                "sentence": "every step passed (2 steps)",
                "outputs": { "cycle_id": 274, "suite_id": 9 },
                "made": [ { "kind": "cycle", "id": "274", "name": "AUTOTEST cycle 20261006" } ],
                "warnings": [],
            })
        );
        assert_eq!(test_made::list(root.path()).len(), 1);
    }

    /// Save validates first and answers with the refusal sentences as they
    /// are; list answers with the switch off.
    #[tokio::test]
    async fn the_bridge_saves_only_a_valid_fixture_and_lists_with_the_switch_off() {
        use v2_lib::ai_bridge::{api_fixture_save, route, BridgeContext, API_WRITES_OFF};
        let _root = crate::serial::autorun();
        let dir = tempfile::tempdir().unwrap();
        v2_lib::autorun::store::set_root(dir.path().to_path_buf());
        store::save(dir.path(), ORG, PROJECT, &make_cycle()).unwrap();
        let off = BridgeContext { org: ORG.into(), project: PROJECT.into(), ..BridgeContext::default() };
        let on = BridgeContext { api_writes: true, ..off.clone() };
        let body = json!({ "fixture": fixture() }).to_string();

        assert_eq!(api_fixture_save(&off, &body), (400, API_WRITES_OFF.to_string()));

        let (status, out) = api_fixture_save(&on, &body);
        assert_eq!(status, 400);
        assert_eq!(out, "step 2: template add-suite is not proven\noutput suite_id: {{steps.2.suiteId}} does not come from a step");
        assert_eq!(fixture_store::load(dir.path(), ORG, PROJECT, "draft-cycle").unwrap(), None);

        store::save(dir.path(), ORG, PROJECT, &add_suite()).unwrap();
        let (status, out) = route(&on, None, "POST", "/api-template-fixture-save", &body, "1.0.0").await;
        assert_eq!(status, 200, "{out}");
        assert_eq!(fixture_store::load(dir.path(), ORG, PROJECT, "draft-cycle").unwrap(), Some(fixture()));

        let (status, out) = route(&off, None, "GET", "/api-template-fixtures", "", "1.0.0").await;
        assert_eq!(status, 200, "{out}");
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["fixtures"][0]["id"], "draft-cycle");
        assert_eq!(v["fixtures"][0]["current_outputs"], Value::Null, "never built");
        assert_eq!(v["fixtures"][0]["last_run"], Value::Null);
    }
}

/// Review Focus 5: no bridge route and no MCP tool writes the record of
/// test-made drafts or an approval itself. Only the fixture runner adds to
/// the record and only Clean up changes a status; approvals are a person's.
/// A source-scan tripwire, in the style of the other tripwires here: it
/// lists every route and tool so a new one is in view, then checks that
/// neither file reaches a writer.
#[test]
fn no_bridge_route_or_mcp_tool_writes_the_test_made_record_or_an_approval() {
    let read = |p: &str| {
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(p)).unwrap().replace("\r\n", "\n")
    };
    let bridge = read("src/ai_bridge.rs");
    let mcp = read("src/mcp.rs");

    let route_body = &bridge[bridge.find("pub async fn route(").unwrap()..];
    let route_body = &route_body[..route_body.find("\n}\n").unwrap()];
    let routes: Vec<&str> = route_body
        .split("(\"")
        .skip(1)
        .filter_map(|s| {
            let (method, rest) = s.split_once("\", \"")?;
            matches!(method, "GET" | "POST").then(|| rest.split('"').next().unwrap_or(""))
        })
        .collect();
    for r in ["/api-template-fixtures", "/api-template-fixture-save", "/api-template-fixture-run", "/api-template-run"] {
        assert!(routes.contains(&r), "{r} is not in the router: {routes:?}");
    }

    let list = &mcp[mcp.find("fn tools_list").unwrap()..];
    let tools: Vec<&str> =
        list.split("\"name\": \"").skip(1).filter_map(|s| s.split('"').next()).collect();
    for t in ["save_api_fixture", "run_api_fixture", "list_api_fixtures"] {
        assert!(tools.contains(&t), "{t} is not listed: {tools:?}");
    }
    assert!(routes.len() > 20 && tools.len() > 20, "the scan found too little: {routes:?} {tools:?}");

    for (name, text) in [("src/ai_bridge.rs", &bridge), ("src/mcp.rs", &mcp)] {
        // Lexical: it catches the writers named, a `use` of the module's
        // items and the file names, not a writer reached some other way.
        for writer in [
            "test_made::record",
            "test_made::set_status",
            "test_made::{",
            "test-made.json",
            "approvals/",
            "\"approvals\"",
            "approvals::approve",
            "approvals::withdraw",
            "approvals::{",
            "auto_run_setup_view",
            "auto_run_approve_setup",
            "auto_run_withdraw_setup",
        ] {
            assert!(!text.contains(writer), "{name} reaches {writer}");
        }
    }
}
