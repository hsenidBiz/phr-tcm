//! The shared API library's paths and content hashes, and the per-environment
//! confirmation state (`here`): what is derived, what a failure does, and
//! that the state file survives concurrent writers.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use v2_lib::api_templates::fixture::{Fixture, FixtureStep};
use v2_lib::api_templates::flow::{Flow, StageRef};
use v2_lib::api_templates::here::{
    after_template_saved, confirm_fixture, confirm_template, fixture_here, flow_here, forget, load_state,
    note_fixture_run, note_template_run, record_fixture_failure, record_template_failure, template_here, Here, HereFailure, TrySource,
};
use v2_lib::api_templates::library::{
    flow_hash, history_dir, library_dir, state_path, template_hash, ApiKind, REMOVED_API_DIR,
};
use v2_lib::api_templates::store::SavedTemplate;
use v2_lib::api_templates::{ApiTemplate, Proven};
use v2_lib::autorun::recipe::project_slug;

const ORG: &str = "Org";
const PROJECT: &str = "Proj";

fn template(id: &str, path: &str) -> ApiTemplate {
    serde_json::from_value(json!({
        "id": id, "title": "A template", "module": "PMS", "effect": "create", "description": "d",
        "sources": ["x:1"], "antiforgery": { "page": "/hr/x" },
        "params": [{ "name": "cycleName", "type": "string", "required": true }],
        "steps": [{ "name": "Save", "method": "POST", "path": path,
                    "form": { "CycleName": "{{cycleName}}" }, "expect": { "status": 200 },
                    "capture": { "cycleId": "$.cycleId" } }],
        "outputs": ["cycleId"]
    }))
    .unwrap()
}

fn proof() -> Proven {
    Proven {
        at: "2026-10-06 09:00:00".into(),
        origin: "https://hr.example.internal".into(),
        account: "admin".into(),
        outputs: BTreeMap::new(),
        environment: None,
        via: None,
    }
}

fn fixture(id: &str, templates: &[&str]) -> Fixture {
    Fixture {
        id: id.into(),
        name: "A fixture".into(),
        account: "admin".into(),
        steps: templates
            .iter()
            .map(|t| FixtureStep { template: (*t).into(), params: BTreeMap::new() })
            .collect(),
        outputs: BTreeMap::new(),
        creates: vec![],
        skip_stages: vec![],
    }
}

fn flow() -> Flow {
    let v: Value = json!({
        "id": "cycle", "title": "Cycle", "module": "PMS",
        "subject": { "name": "cycleId", "type": "number" },
        "stages": [
            { "id": "setup", "title": "Setup", "creates": true, "check": "SELECT 1 WHERE id = {{cycleId}}" },
            { "id": "publish", "title": "Publish", "requires": ["setup"], "check": "SELECT 1 WHERE id = {{cycleId}}" }
        ]
    });
    serde_json::from_value(v).unwrap()
}

fn on_stage(mut t: ApiTemplate, stage: &str) -> ApiTemplate {
    t.stage = Some(StageRef { flow: "cycle".into(), id: stage.into() });
    t
}

fn fail(step: &str, detail: &str) -> HereFailure {
    HereFailure { at: "2026-11-06 10:00:00".into(), step: Some(step.into()), detail: Some(detail.into()) }
}

fn lookup(templates: &[ApiTemplate]) -> impl Fn(&str) -> Option<ApiTemplate> + '_ {
    move |id| templates.iter().find(|t| t.id == id).cloned()
}

fn here_of(root: &std::path::Path, t: &ApiTemplate) -> Here {
    template_here(&load_state(root, ORG, PROJECT), t)
}

#[test]
fn library_paths_sit_under_the_shared_root_and_histories_under_the_environment() {
    let t = PathBuf::from("shared");
    let root = t.join("envs").join("env-0000000a");
    let slug = project_slug(ORG, PROJECT);
    assert_eq!(library_dir(&root, ApiKind::Templates, ORG, PROJECT), t.join("api-library").join("templates").join(&slug));
    assert_eq!(library_dir(&t, ApiKind::Flows, ORG, PROJECT), t.join("api-library").join("flows").join(&slug));
    assert_eq!(history_dir(&root, ApiKind::Fixtures, ORG, PROJECT), root.join("fixtures").join(&slug));
    assert_eq!(state_path(&root, ORG, PROJECT), root.join("api-state").join(format!("{slug}.json")));
    assert_eq!(REMOVED_API_DIR, "removed-api");
}

#[test]
fn a_template_hash_ignores_its_proof() {
    let a = template("a", "/hr/x");
    let mut with_proof = a.clone();
    with_proof.proven = Some(proof());
    assert_eq!(template_hash(&with_proof), template_hash(&a));
    assert_ne!(template_hash(&a), template_hash(&template("a", "/hr/y")));
}

#[test]
fn a_flow_hash_ignores_its_sample() {
    let plain = flow();
    let mut v = serde_json::to_value(&plain).unwrap();
    v["saved"] = json!({ "at": "2026-09-29T09:00:00Z", "sample": 273 });
    let sampled: Flow = serde_json::from_value(v).unwrap();
    assert!(sampled.saved.is_some());
    assert_eq!(flow_hash(&sampled), flow_hash(&plain));
}

#[test]
fn nothing_recorded_reads_unconfirmed() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(here_of(dir.path(), &template("a", "/hr/x")), Here::Unconfirmed);
}

#[test]
fn a_confirmed_template_reads_confirmed_until_its_content_changes() {
    let dir = tempfile::tempdir().unwrap();
    let t = template("a", "/hr/x");
    confirm_template(dir.path(), ORG, PROJECT, &t).unwrap();
    assert_eq!(here_of(dir.path(), &t), Here::Confirmed { last_failure: None });
    assert_eq!(here_of(dir.path(), &template("a", "/hr/changed")), Here::Unconfirmed);
}

#[test]
fn an_unconfirmed_item_that_fails_is_failed_here_with_its_step_and_detail() {
    let dir = tempfile::tempdir().unwrap();
    let t = template("a", "/hr/x");
    record_template_failure(dir.path(), ORG, PROJECT, &t, fail("2", "HTTP 500"), TrySource::Plain).unwrap();
    assert!(matches!(
        here_of(dir.path(), &t),
        Here::Failed { failure } if failure.step.as_deref() == Some("2") && failure.detail.as_deref() == Some("HTTP 500")
    ));
}

#[test]
fn a_confirmed_item_stays_confirmed_after_one_failure() {
    let dir = tempfile::tempdir().unwrap();
    let t = template("a", "/hr/x");
    confirm_template(dir.path(), ORG, PROJECT, &t).unwrap();
    record_template_failure(dir.path(), ORG, PROJECT, &t, fail("1", "HTTP 500"), TrySource::Assistant).unwrap();
    assert_eq!(here_of(dir.path(), &t), Here::Confirmed { last_failure: Some(fail("1", "HTTP 500")) });
}

#[test]
fn a_plain_run_never_flips_a_confirmed_item() {
    let dir = tempfile::tempdir().unwrap();
    let t = template("a", "/hr/x");
    confirm_template(dir.path(), ORG, PROJECT, &t).unwrap();
    for _ in 0..3 {
        record_template_failure(dir.path(), ORG, PROJECT, &t, fail("1", "HTTP 500"), TrySource::Plain).unwrap();
    }
    assert!(matches!(here_of(dir.path(), &t), Here::Confirmed { last_failure: Some(_) }));
}

#[test]
fn the_assistants_retry_after_a_failure_flips_it_to_failed_here() {
    let dir = tempfile::tempdir().unwrap();
    let t = template("a", "/hr/x");
    confirm_template(dir.path(), ORG, PROJECT, &t).unwrap();
    record_template_failure(dir.path(), ORG, PROJECT, &t, fail("1", "first"), TrySource::Plain).unwrap();
    record_template_failure(dir.path(), ORG, PROJECT, &t, fail("1", "second"), TrySource::Assistant).unwrap();
    assert_eq!(here_of(dir.path(), &t), Here::Failed { failure: fail("1", "second") });
}

#[test]
fn a_success_clears_the_strike() {
    let dir = tempfile::tempdir().unwrap();
    let t = template("a", "/hr/x");
    confirm_template(dir.path(), ORG, PROJECT, &t).unwrap();
    record_template_failure(dir.path(), ORG, PROJECT, &t, fail("1", "first"), TrySource::Assistant).unwrap();
    confirm_template(dir.path(), ORG, PROJECT, &t).unwrap();
    assert_eq!(here_of(dir.path(), &t), Here::Confirmed { last_failure: None });
    record_template_failure(dir.path(), ORG, PROJECT, &t, fail("1", "again"), TrySource::Assistant).unwrap();
    assert_eq!(here_of(dir.path(), &t), Here::Confirmed { last_failure: Some(fail("1", "again")) });
}

#[test]
fn a_fixture_reads_unconfirmed_when_a_template_it_uses_changed() {
    let dir = tempfile::tempdir().unwrap();
    let (t1, t2) = (template("t1", "/hr/a"), template("t2", "/hr/b"));
    let f = fixture("f", &["t1", "t2"]);
    confirm_fixture(dir.path(), ORG, PROJECT, &f, &[t1.clone(), t2.clone()]).unwrap();
    let s = load_state(dir.path(), ORG, PROJECT);
    assert_eq!(fixture_here(&s, &f, &lookup(&[t1.clone(), t2.clone()])), Here::Confirmed { last_failure: None });
    let t2_changed = template("t2", "/hr/changed");
    assert_eq!(fixture_here(&s, &f, &lookup(&[t1.clone(), t2_changed])), Here::Unconfirmed);
    assert_eq!(fixture_here(&s, &f, &lookup(&[t1])), Here::Unconfirmed, "a template that is gone");
}

#[test]
fn a_successful_build_also_confirms_its_templates() {
    let dir = tempfile::tempdir().unwrap();
    let (t1, t2) = (template("t1", "/hr/a"), template("t2", "/hr/b"));
    confirm_fixture(dir.path(), ORG, PROJECT, &fixture("f", &["t1", "t2"]), &[t1.clone(), t2.clone()]).unwrap();
    assert_eq!(here_of(dir.path(), &t1), Here::Confirmed { last_failure: None });
    assert_eq!(here_of(dir.path(), &t2), Here::Confirmed { last_failure: None });
}

#[test]
fn a_failed_build_marks_only_the_fixture() {
    let dir = tempfile::tempdir().unwrap();
    let (t1, t2) = (template("t1", "/hr/a"), template("t2", "/hr/b"));
    let f = fixture("f", &["t1", "t2"]);
    let both = [t1.clone(), t2.clone()];
    record_fixture_failure(dir.path(), ORG, PROJECT, &f, &both, fail("2", "HTTP 500"), TrySource::Plain).unwrap();
    let s = load_state(dir.path(), ORG, PROJECT);
    assert!(matches!(fixture_here(&s, &f, &lookup(&both)), Here::Failed { .. }));
    assert_eq!(template_here(&s, &t1), Here::Unconfirmed);
    assert_eq!(template_here(&s, &t2), Here::Unconfirmed);
}

#[test]
fn a_confirmed_fixture_follows_the_failure_rule_too() {
    let dir = tempfile::tempdir().unwrap();
    let t1 = template("t1", "/hr/a");
    let f = fixture("f", &["t1"]);
    let ran = [t1];
    confirm_fixture(dir.path(), ORG, PROJECT, &f, &ran).unwrap();
    record_fixture_failure(dir.path(), ORG, PROJECT, &f, &ran, fail("1", "first"), TrySource::Plain).unwrap();
    let s = load_state(dir.path(), ORG, PROJECT);
    assert!(matches!(fixture_here(&s, &f, &lookup(&ran)), Here::Confirmed { last_failure: Some(_) }));
    record_fixture_failure(dir.path(), ORG, PROJECT, &f, &ran, fail("1", "second"), TrySource::Assistant).unwrap();
    let s = load_state(dir.path(), ORG, PROJECT);
    assert_eq!(fixture_here(&s, &f, &lookup(&ran)), Here::Failed { failure: fail("1", "second") });
}

#[test]
fn a_flow_is_confirmed_only_when_every_template_on_it_is() {
    let dir = tempfile::tempdir().unwrap();
    let fl = flow();
    let setup = on_stage(template("setup-t", "/hr/a"), "setup");
    let publish = on_stage(template("publish-t", "/hr/b"), "publish");
    let elsewhere = template("other", "/hr/c");
    let saved = |ts: &[&ApiTemplate]| -> Vec<SavedTemplate> {
        ts.iter()
            .map(|t| SavedTemplate { template: (*t).clone(), runs: vec![], here: here_of(dir.path(), t) })
            .collect()
    };

    assert_eq!(flow_here(&fl, &saved(&[&elsewhere])), Here::Unconfirmed, "no template on the flow");
    confirm_template(dir.path(), ORG, PROJECT, &setup).unwrap();
    assert_eq!(flow_here(&fl, &saved(&[&setup, &publish])), Here::Unconfirmed, "one of two confirmed");
    confirm_template(dir.path(), ORG, PROJECT, &publish).unwrap();
    assert_eq!(flow_here(&fl, &saved(&[&setup, &publish, &elsewhere])), Here::Confirmed { last_failure: None });
    let fresh = on_stage(template("fresh", "/hr/z"), "setup");
    record_template_failure(dir.path(), ORG, PROJECT, &fresh, fail("1", "x"), TrySource::Plain).unwrap();
    assert!(matches!(here_of(dir.path(), &fresh), Here::Failed { .. }));
    assert_eq!(flow_here(&fl, &saved(&[&setup, &publish, &fresh])), Here::Unconfirmed, "never Failed");
}

#[test]
fn an_unreadable_state_file_reads_as_nothing_recorded() {
    let dir = tempfile::tempdir().unwrap();
    let path = state_path(dir.path(), ORG, PROJECT);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{").unwrap();
    let t = template("a", "/hr/x");
    assert!(load_state(dir.path(), ORG, PROJECT).templates.is_empty());
    assert_eq!(here_of(dir.path(), &t), Here::Unconfirmed);
}

#[test]
fn concurrent_records_for_one_project_both_land() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let root = root.clone();
            std::thread::spawn(move || {
                if i < 4 {
                    confirm_template(&root, ORG, PROJECT, &template(&format!("ta{i}"), "/hr/x")).unwrap();
                } else {
                    let f = fixture(&format!("fx{i}"), &["ta0"]);
                    record_fixture_failure(&root, ORG, PROJECT, &f, &[], fail("1", "x"), TrySource::Plain).unwrap();
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let s = load_state(&root, ORG, PROJECT);
    for i in 0..4 {
        assert!(s.templates.contains_key(&format!("ta{i}")), "ta{i}");
    }
    for i in 4..8 {
        assert!(s.fixtures.contains_key(&format!("fx{i}")), "fx{i}");
    }
}

#[test]
fn forget_drops_only_that_entry() {
    let dir = tempfile::tempdir().unwrap();
    let (t1, t2) = (template("t1", "/hr/a"), template("t2", "/hr/b"));
    confirm_template(dir.path(), ORG, PROJECT, &t1).unwrap();
    confirm_template(dir.path(), ORG, PROJECT, &t2).unwrap();
    forget(dir.path(), ORG, PROJECT, ApiKind::Templates, "t1").unwrap();
    assert_eq!(here_of(dir.path(), &t1), Here::Unconfirmed);
    assert_eq!(here_of(dir.path(), &t2), Here::Confirmed { last_failure: None });
}

#[test]
fn the_state_file_names_no_definition() {
    let dir = tempfile::tempdir().unwrap();
    let t = template("a", "/hr/secret-path");
    confirm_fixture(dir.path(), ORG, PROJECT, &fixture("f", &["a"]), &[t]).unwrap();
    let text = std::fs::read_to_string(state_path(dir.path(), ORG, PROJECT)).unwrap();
    for key in ["\"steps\"", "\"path\"", "secret-path", "\"form\""] {
        assert!(!text.contains(key), "{key} in {text}");
    }
    assert!(text.contains("\"hash\""));
}

#[test]
fn a_save_with_a_proof_confirms_and_moves_the_fixtures_that_used_the_old_version() {
    let dir = tempfile::tempdir().unwrap();
    let old = template("t1", "/hr/a");
    let f = fixture("f", &["t1"]);
    confirm_fixture(dir.path(), ORG, PROJECT, &f, &[old.clone()]).unwrap();

    let mut saved = template("t1", "/hr/changed");
    saved.proven = Some(proof());
    after_template_saved(dir.path(), ORG, PROJECT, Some(&old), &saved).unwrap();

    let s = load_state(dir.path(), ORG, PROJECT);
    assert_eq!(template_here(&s, &saved), Here::Confirmed { last_failure: None });
    assert_eq!(fixture_here(&s, &f, &lookup(&[saved.clone()])), Here::Confirmed { last_failure: None });
}

#[test]
fn a_save_without_a_proof_records_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let draft = template("t1", "/hr/a");
    after_template_saved(dir.path(), ORG, PROJECT, None, &draft).unwrap();
    assert!(!state_path(dir.path(), ORG, PROJECT).exists());
}

// ---- The stores read and write the shared library ----------------------

use v2_lib::api_templates::{fixture_store, flow_store, store};
use v2_lib::autorun::roots;

/// Three environment roots under one shared folder, each writable for the
/// test's length (the way a switch would make it the active one).
struct Envs {
    _shared: tempfile::TempDir,
    qa: PathBuf,
    staging: PathBuf,
    prod: PathBuf,
    _allowed: Vec<roots::AllowedTarget>,
}

fn envs() -> Envs {
    let shared = tempfile::tempdir().unwrap();
    let make = |id: &str| roots::env_root(shared.path(), id);
    let (qa, staging, prod) = (make("env-0000000a"), make("env-0000000b"), make("env-0000000c"));
    let allowed = [&qa, &staging, &prod]
        .iter()
        .map(|r| {
            std::fs::create_dir_all(r).unwrap();
            roots::allow_writes_to(r)
        })
        .collect();
    Envs { _shared: shared, qa, staging, prod, _allowed: allowed }
}

fn proven(mut t: ApiTemplate) -> ApiTemplate {
    t.proven = Some(proof());
    t
}

fn put_fixture(root: &std::path::Path, f: &Fixture) {
    let dir = fixture_store::fixtures_dir(root, ORG, PROJECT);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{}.json", f.id)), serde_json::to_string(f).unwrap()).unwrap();
}

fn template_here_in(root: &std::path::Path, id: &str) -> Here {
    store::list(root, ORG, PROJECT).unwrap().into_iter().find(|s| s.template.id == id).unwrap().here
}

fn fixture_here_in(root: &std::path::Path, id: &str) -> Here {
    fixture_store::list(root, ORG, PROJECT).unwrap().into_iter().find(|s| s.fixture.id == id).unwrap().here
}

#[test]
fn a_template_saved_in_one_environment_is_listed_in_every_environment() {
    let _lock = crate::serial::autorun();
    let e = envs();
    store::save_proven(&e.qa, ORG, PROJECT, &proven(template("t1", "/hr/a"))).unwrap();
    for root in [&e.qa, &e.staging, &e.prod] {
        let listed = store::list(root, ORG, PROJECT).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].template.id, "t1");
    }
}

#[test]
fn a_proven_save_confirms_only_the_saving_environment() {
    let _lock = crate::serial::autorun();
    let e = envs();
    store::save_proven(&e.qa, ORG, PROJECT, &proven(template("t1", "/hr/a"))).unwrap();
    assert_eq!(template_here_in(&e.qa, "t1"), Here::Confirmed { last_failure: None });
    assert_eq!(template_here_in(&e.staging, "t1"), Here::Unconfirmed);
}

#[test]
fn a_save_that_only_keeps_a_proof_from_elsewhere_leaves_the_template_unconfirmed_here() {
    let _lock = crate::serial::autorun();
    let e = envs();
    store::save_proven(&e.qa, ORG, PROJECT, &proven(template("t1", "/hr/a"))).unwrap();
    // Staging saves the very same proven definition without proving it there.
    let carried = store::load(&e.qa, ORG, PROJECT, "t1").unwrap().unwrap();
    assert!(carried.proven.is_some());
    store::save(&e.staging, ORG, PROJECT, &carried).unwrap();
    assert_eq!(template_here_in(&e.staging, "t1"), Here::Unconfirmed);
    assert_eq!(template_here_in(&e.qa, "t1"), Here::Confirmed { last_failure: None }, "same content, still proven in qa");
}

#[test]
fn an_import_style_save_without_proof_confirms_nothing() {
    let _lock = crate::serial::autorun();
    let e = envs();
    store::save(&e.qa, ORG, PROJECT, &template("t1", "/hr/a")).unwrap();
    assert_eq!(template_here_in(&e.qa, "t1"), Here::Unconfirmed);
    assert!(!state_path(&e.qa, ORG, PROJECT).exists());
}

#[test]
fn run_histories_stay_per_environment() {
    let _lock = crate::serial::autorun();
    let e = envs();
    store::save(&e.qa, ORG, PROJECT, &template("t1", "/hr/a")).unwrap();
    let run: store::RunRecord = serde_json::from_value(json!({
        "at": "2026-11-06 10:00:00", "account": "admin", "ok": true, "outputs": {}
    }))
    .unwrap();
    store::append_run(&e.qa, ORG, PROJECT, "t1", run).unwrap();
    assert_eq!(store::list(&e.qa, ORG, PROJECT).unwrap()[0].runs.len(), 1);
    assert!(store::list(&e.staging, ORG, PROJECT).unwrap()[0].runs.is_empty());
    assert!(e.qa.join("templates").join(project_slug(ORG, PROJECT)).join("t1.runs.json").is_file());
}

#[test]
fn a_template_file_written_before_the_library_still_loads() {
    let _lock = crate::serial::autorun();
    let e = envs();
    let dir = store::templates_dir(&e.qa, ORG, PROJECT);
    std::fs::create_dir_all(&dir).unwrap();
    // Today's file: the template with its proof, as the old store wrote it.
    let old = serde_json::to_string_pretty(&proven(template("t1", "/hr/a"))).unwrap();
    std::fs::write(dir.join("t1.json"), old).unwrap();
    let loaded = store::load(&e.qa, ORG, PROJECT, "t1").unwrap().unwrap();
    assert!(loaded.proven.is_some(), "the proof is kept");
    assert_eq!(template_here_in(&e.qa, "t1"), Here::Unconfirmed, "but nothing is confirmed on this environment");
}

#[test]
fn a_template_replaced_elsewhere_unconfirms_fixtures_here_but_not_on_the_proving_environment() {
    let _lock = crate::serial::autorun();
    let e = envs();
    let v1 = proven(template("t1", "/hr/v1"));
    let f = fixture("f", &["t1"]);
    for root in [&e.qa, &e.staging] {
        store::save_proven(root, ORG, PROJECT, &v1).unwrap();
        confirm_fixture(root, ORG, PROJECT, &f, &[v1.clone()]).unwrap();
    }
    put_fixture(&e.qa, &f);
    assert_eq!(fixture_here_in(&e.qa, "f"), Here::Confirmed { last_failure: None });
    assert_eq!(fixture_here_in(&e.staging, "f"), Here::Confirmed { last_failure: None });

    store::save_proven(&e.staging, ORG, PROJECT, &proven(template("t1", "/hr/v2"))).unwrap();
    assert_eq!(fixture_here_in(&e.qa, "f"), Here::Unconfirmed);
    assert_eq!(fixture_here_in(&e.staging, "f"), Here::Confirmed { last_failure: None });
    assert_eq!(template_here_in(&e.qa, "t1"), Here::Unconfirmed);
    assert_eq!(template_here_in(&e.staging, "t1"), Here::Confirmed { last_failure: None });
}

#[test]
fn reproving_after_a_replace_elsewhere_does_not_revive_a_stale_fixture_confirmation() {
    let _lock = crate::serial::autorun();
    let e = envs();
    let v1 = proven(template("t1", "/hr/v1"));
    let f = fixture("f", &["t1"]);
    for root in [&e.qa, &e.staging] {
        store::save_proven(root, ORG, PROJECT, &v1).unwrap();
        confirm_fixture(root, ORG, PROJECT, &f, &[v1.clone()]).unwrap();
    }
    put_fixture(&e.qa, &f);
    store::save_proven(&e.staging, ORG, PROJECT, &proven(template("t1", "/hr/v2"))).unwrap();

    // qa proves a third version: its fixture's uses held v1, and the copy it
    // replaced was v2, so the confirmation does not follow.
    store::save_proven(&e.qa, ORG, PROJECT, &proven(template("t1", "/hr/v3"))).unwrap();
    assert_eq!(template_here_in(&e.qa, "t1"), Here::Confirmed { last_failure: None });
    assert_eq!(fixture_here_in(&e.qa, "f"), Here::Unconfirmed);
}

#[test]
fn removing_a_template_removes_it_for_every_environment_and_forgets_it_here() {
    let _lock = crate::serial::autorun();
    let e = envs();
    store::save_proven(&e.qa, ORG, PROJECT, &proven(template("t1", "/hr/a"))).unwrap();
    assert!(load_state(&e.qa, ORG, PROJECT).templates.contains_key("t1"));
    store::remove(&e.qa, ORG, PROJECT, "t1").unwrap();
    assert!(store::list(&e.staging, ORG, PROJECT).unwrap().is_empty());
    assert!(store::list(&e.qa, ORG, PROJECT).unwrap().is_empty());
    assert!(!load_state(&e.qa, ORG, PROJECT).templates.contains_key("t1"));
}

#[test]
fn removing_a_fixture_removes_it_for_every_environment_and_forgets_it_here() {
    let _lock = crate::serial::autorun();
    let e = envs();
    let t = template("t1", "/hr/a");
    let f = fixture("f", &["t1"]);
    put_fixture(&e.qa, &f);
    confirm_fixture(&e.qa, ORG, PROJECT, &f, &[t]).unwrap();
    fixture_store::remove(&e.qa, ORG, PROJECT, "f").unwrap();
    assert!(fixture_store::list(&e.staging, ORG, PROJECT).unwrap().is_empty());
    assert!(!load_state(&e.qa, ORG, PROJECT).fixtures.contains_key("f"));
}

#[test]
fn the_overview_reports_each_flows_state_here() {
    let _lock = crate::serial::autorun();
    let e = envs();
    let fl = flow();
    flow_store::save(&e.qa, ORG, PROJECT, &fl).unwrap();
    let only = on_stage(template("setup-t", "/hr/a"), "setup");
    store::save(&e.qa, ORG, PROJECT, &only).unwrap();

    let overview = v2_lib::commands::api_templates::overview_at(&e.qa, ORG, PROJECT).unwrap();
    assert_eq!(overview.flow_here[&fl.id], Here::Unconfirmed);
    confirm_template(&e.qa, ORG, PROJECT, &only).unwrap();
    let overview = v2_lib::commands::api_templates::overview_at(&e.qa, ORG, PROJECT).unwrap();
    assert_eq!(overview.flow_here[&fl.id], Here::Confirmed { last_failure: None });
    let elsewhere = v2_lib::commands::api_templates::overview_at(&e.prod, ORG, PROJECT).unwrap();
    assert_eq!(elsewhere.flow_here[&fl.id], Here::Unconfirmed, "confirmation is per environment");
}

#[test]
fn a_run_of_a_draft_that_differs_from_the_saved_copy_records_nothing() {
    let _lock = crate::serial::autorun();
    let e = envs();
    let saved = proven(template("t1", "/hr/a"));
    store::save(&e.qa, ORG, PROJECT, &saved).unwrap();
    let changed = template("t1", "/hr/changed");
    note_template_run(&e.qa, ORG, PROJECT, &changed, false, Some(fail("1", "HTTP 500")), TrySource::Assistant);
    note_template_run(&e.qa, ORG, PROJECT, &changed, true, None, TrySource::Assistant);
    assert_eq!(here_of(&e.qa, &saved), Here::Unconfirmed);
    assert!(!state_path(&e.qa, ORG, PROJECT).exists());
}

#[test]
fn a_successful_run_of_the_saved_version_confirms_it() {
    let _lock = crate::serial::autorun();
    let e = envs();
    let saved = proven(template("t1", "/hr/a"));
    store::save(&e.qa, ORG, PROJECT, &saved).unwrap();
    assert_eq!(template_here_in(&e.qa, "t1"), Here::Unconfirmed);
    note_template_run(&e.qa, ORG, PROJECT, &saved, true, None, TrySource::Assistant);
    assert_eq!(template_here_in(&e.qa, "t1"), Here::Confirmed { last_failure: None });
    assert_eq!(template_here_in(&e.staging, "t1"), Here::Unconfirmed, "confirmation is per environment");
}

#[test]
fn a_fixture_note_with_no_ran_templates_reads_them_from_the_store() {
    let _lock = crate::serial::autorun();
    let e = envs();
    let (t1, t2) = (proven(template("t1", "/hr/a")), proven(template("t2", "/hr/b")));
    store::save(&e.qa, ORG, PROJECT, &t1).unwrap();
    store::save(&e.qa, ORG, PROJECT, &t2).unwrap();
    let f = fixture("f", &["t1", "t2"]);
    put_fixture(&e.qa, &f);
    note_fixture_run(&e.qa, ORG, PROJECT, &f, &[], true, None, TrySource::Plain);
    assert_eq!(fixture_here_in(&e.qa, "f"), Here::Confirmed { last_failure: None });
    assert_eq!(template_here_in(&e.qa, "t2"), Here::Confirmed { last_failure: None });
    // A failure with the full set reads back Failed, not Unconfirmed.
    let g = fixture("g", &["t1", "t2"]);
    put_fixture(&e.qa, &g);
    note_fixture_run(&e.qa, ORG, PROJECT, &g, &[], false, Some(fail("step 2", "HTTP 500")), TrySource::Plain);
    assert!(matches!(fixture_here_in(&e.qa, "g"), Here::Failed { failure } if failure.step.as_deref() == Some("step 2")));
}
