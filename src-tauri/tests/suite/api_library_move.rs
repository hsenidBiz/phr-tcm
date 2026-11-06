//! The one-time fold of each environment's API templates, flows and
//! fixtures into the shared library: who wins, what stays confirmed where,
//! what is kept aside, and that a cut-short fold is finished or rebuilt.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use v2_lib::api_templates::fixture::{Fixture, FixtureStep};
use v2_lib::api_templates::flow::Flow;
use v2_lib::api_templates::here::{fixture_here, load_state, template_here, EnvApiState, Here};
use v2_lib::api_templates::library::{template_hash, ApiKind, REMOVED_API_DIR};
use v2_lib::api_templates::library_move::{fold_in, fold_in_with, move_log_line, MoveReport, STAGING_PREFIX};
use v2_lib::api_templates::{fixture_store, flow_store, store, ApiTemplate, Proven};
use v2_lib::autorun::recipe::project_slug;
use v2_lib::autorun::{env_copy, roots};
use v2_lib::environments::{self, Environment};

use super::serial;

const ORG: &str = "Org";
const PROJECT: &str = "Proj";
const A: &str = "env-0000000a";
const B: &str = "env-0000000b";
const NOW: u64 = 1_800_000_000_000;

fn env_named(id: &str, name: &str) -> Environment {
    Environment {
        id: id.into(),
        name: name.into(),
        start_url: String::new(),
        allowed_origins: vec![],
        db_id: "dev-read".into(),
        test_environment: false,
        test_prefix: "AUTOTEST".into(),
        db_auto_approve: false,
        password_expiry_query: String::new(),
        active_session_query: String::new(),
        reset_expired_passwords: false,
    }
}

fn env_file() -> environments::EnvFile {
    environments::EnvFile {
        active: A.into(),
        environments: vec![env_named(A, "QA"), env_named(B, "Staging")],
        auto_approve_adopted: false,
        playwright_export: None,
    }
}

/// A shared root listing QA (A, active) and Staging (B), with both folders.
fn shared_with_two_envs() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    std::fs::write(t.path().join("environments.json"), serde_json::to_string(&env_file()).unwrap()).unwrap();
    for id in [A, B] {
        std::fs::create_dir_all(roots::env_root(t.path(), id)).unwrap();
    }
    t
}

fn slug() -> String {
    project_slug(ORG, PROJECT)
}

fn template(id: &str, title: &str) -> ApiTemplate {
    serde_json::from_value(json!({
        "id": id, "title": title, "module": "PMS", "effect": "create", "description": "d",
        "sources": ["x:1"], "antiforgery": { "page": "/hr/x" },
        "params": [{ "name": "cycleName", "type": "string", "required": true }],
        "steps": [{ "name": "Save", "method": "POST", "path": "/hr/save",
                    "form": { "CycleName": "{{cycleName}}" }, "expect": { "status": 200 },
                    "capture": { "cycleId": "$.cycleId" } }],
        "outputs": ["cycleId"]
    }))
    .unwrap()
}

fn proven(t: &ApiTemplate, at: &str, environment: Option<&str>) -> ApiTemplate {
    let mut t = t.clone();
    t.proven = Some(Proven {
        at: at.into(),
        origin: "https://hr.example.internal".into(),
        account: "admin".into(),
        outputs: BTreeMap::new(),
        environment: environment.map(str::to_string),
        via: None,
    });
    t
}

fn fixture(id: &str, templates: &[&str]) -> Fixture {
    Fixture {
        id: id.into(),
        name: "A fixture".into(),
        account: "admin".into(),
        steps: templates.iter().map(|t| FixtureStep { template: (*t).into(), params: BTreeMap::new() }).collect(),
        outputs: BTreeMap::new(),
        creates: vec![],
        skip_stages: vec![],
    }
}

fn flow(id: &str) -> Flow {
    serde_json::from_value(json!({
        "id": id, "title": "Cycle", "module": "PMS",
        "subject": { "name": "cycleId", "type": "number" },
        "stages": [{ "id": "setup", "title": "Setup", "creates": true, "check": "SELECT 1 WHERE id = {{cycleId}}" }]
    }))
    .unwrap()
}

fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// Where a holder (`envs/<id>` or `removed-envs/<key>`) keeps `kind`'s files.
fn kind_dir(shared: &Path, holder: &str, kind: ApiKind) -> PathBuf {
    shared.join(holder).join(kind.folder()).join(slug())
}

fn env_holder(id: &str) -> String {
    format!("envs/{id}")
}

fn put_template(shared: &Path, holder: &str, t: &ApiTemplate) {
    put(&kind_dir(shared, holder, ApiKind::Templates).join(format!("{}.json", t.id)), &serde_json::to_string_pretty(t).unwrap());
}

fn put_fixture(shared: &Path, holder: &str, f: &Fixture) {
    put(&kind_dir(shared, holder, ApiKind::Fixtures).join(format!("{}.json", f.id)), &serde_json::to_string_pretty(f).unwrap());
}

fn put_runs(shared: &Path, holder: &str, kind: ApiKind, id: &str, runs: Value) {
    put(&kind_dir(shared, holder, kind).join(format!("{id}.runs.json")), &runs.to_string());
}

fn run_line(at: &str, mode: &str, ok: bool) -> Value {
    json!({ "at": at, "mode": mode, "account": "admin", "ok": ok, "outputs": {} })
}

fn fixture_run(at: &str, ok: bool) -> Value {
    json!({ "at": at, "ok": ok, "outputs": {} })
}

fn env_root(id: &str, shared: &Path) -> PathBuf {
    roots::env_root(shared, id)
}

fn library_template(shared: &Path, id: &str) -> Option<ApiTemplate> {
    store::load(&env_root(A, shared), ORG, PROJECT, id).unwrap()
}

fn here_of(shared: &Path, env: &str, t: &ApiTemplate) -> Here {
    template_here(&load_state(&env_root(env, shared), ORG, PROJECT), t)
}

fn is_confirmed(h: &Here) -> bool {
    matches!(h, Here::Confirmed { .. })
}

fn aside_dir(shared: &Path, ms: u64) -> PathBuf {
    shared.join(REMOVED_API_DIR).join(ms.to_string())
}

fn staging_entries(shared: &Path) -> Vec<String> {
    std::fs::read_dir(shared)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(STAGING_PREFIX))
        .collect()
}

/// Every definition (non-history) file left under `dir`.
fn definition_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    let Ok(entries) = std::fs::read_dir(dir) else { return out };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(definition_files(&p));
        } else if p.extension().is_some_and(|x| x == "json") && !p.to_string_lossy().ends_with(".runs.json") {
            out.push(p);
        }
    }
    out
}

#[test]
fn an_id_held_by_one_environment_moves_as_is_and_is_confirmed_where_it_was_proven() {
    let t = shared_with_two_envs();
    let shared = t.path();
    let tpl = proven(&template("t", "Add cycle"), "2026-10-05 09:00:00", Some("QA"));
    put_template(shared, &env_holder(A), &tpl);
    put_runs(shared, &env_holder(A), ApiKind::Templates, "t", json!([run_line("2026-10-05 09:00:00", "prove", true)]));

    let report = fold_in(shared, NOW).unwrap().unwrap();

    assert_eq!(report, MoveReport { moved: 1, conflicts: 0, kept_aside: 0 });
    assert_eq!(library_template(shared, "t").unwrap(), tpl);
    assert!(is_confirmed(&here_of(shared, A, &tpl)));
    assert_eq!(here_of(shared, B, &tpl), Here::Unconfirmed);
    assert!(!kind_dir(shared, &env_holder(A), ApiKind::Templates).join("t.json").exists());
    assert!(kind_dir(shared, &env_holder(A), ApiKind::Templates).join("t.runs.json").is_file(), "the run history stays");
    assert!(staging_entries(shared).is_empty());
}

#[test]
fn identical_copies_merge_and_keep_each_environments_confirmation() {
    let t = shared_with_two_envs();
    let shared = t.path();
    let base = template("t", "Add cycle");
    let in_qa = proven(&base, "2026-10-05 09:00:00", Some("QA"));
    let in_staging = proven(&base, "2026-10-06 09:00:00", Some("Staging"));
    put_template(shared, &env_holder(A), &in_qa);
    put_template(shared, &env_holder(B), &in_staging);

    let report = fold_in(shared, NOW).unwrap().unwrap();

    assert_eq!(report.conflicts, 0);
    assert_eq!(report.kept_aside, 0);
    assert!(is_confirmed(&here_of(shared, A, &base)));
    assert!(is_confirmed(&here_of(shared, B, &base)));
    assert_eq!(
        library_template(shared, "t").unwrap().proven.unwrap().at,
        "2026-10-06 09:00:00",
        "of identical copies the one with the newest proof is written"
    );
    assert!(!aside_dir(shared, NOW).exists(), "identical copies lose nothing, so nothing is kept");
}

#[test]
fn the_newest_proven_version_wins_and_only_matching_environments_stay_confirmed() {
    let t = shared_with_two_envs();
    let shared = t.path();
    let v1 = proven(&template("t", "Old title"), "2026-10-01 09:00:00", Some("QA"));
    let v2 = proven(&template("t", "New title"), "2026-11-01 09:00:00", Some("Staging"));
    put_template(shared, &env_holder(A), &v1);
    put_template(shared, &env_holder(B), &v2);

    let report = fold_in(shared, NOW).unwrap().unwrap();

    assert_eq!(report, MoveReport { moved: 1, conflicts: 1, kept_aside: 1 });
    assert_eq!(library_template(shared, "t").unwrap(), v2);
    assert!(is_confirmed(&here_of(shared, B, &v2)));
    assert_eq!(here_of(shared, A, &v2), Here::Unconfirmed);
    let kept = aside_dir(shared, NOW).join(A).join("templates").join(slug()).join("t.json");
    let kept: ApiTemplate = serde_json::from_str(&std::fs::read_to_string(kept).unwrap()).unwrap();
    assert_eq!(kept, v1, "the losing version is kept whole");
}

#[test]
fn an_environment_whose_proof_came_from_elsewhere_needs_a_working_run_to_stay_confirmed() {
    let t = shared_with_two_envs();
    let shared = t.path();
    // Both hold the same version, proved on QA; Staging has since run it fine, QA has no history of its own.
    let on_qa = proven(&template("t", "Add cycle"), "2026-10-05 09:00:00", Some("QA"));
    put_template(shared, &env_holder(A), &on_qa);
    put_template(shared, &env_holder(B), &on_qa);
    put_runs(shared, &env_holder(B), ApiKind::Templates, "t", json!([run_line("2026-10-08 09:00:00", "run", true)]));

    fold_in(shared, NOW).unwrap().unwrap();

    assert!(is_confirmed(&here_of(shared, A, &on_qa)), "proved on QA");
    assert!(is_confirmed(&here_of(shared, B, &on_qa)), "an ok run since the proof");

    let t = shared_with_two_envs();
    let shared = t.path();
    put_template(shared, &env_holder(B), &on_qa);
    put_runs(shared, &env_holder(B), ApiKind::Templates, "t", json!([run_line("2026-10-01 09:00:00", "run", true), run_line("2026-10-09 09:00:00", "run", false)]));
    fold_in(shared, NOW).unwrap().unwrap();
    assert_eq!(here_of(shared, B, &on_qa), Here::Unconfirmed, "proved elsewhere, and nothing ok since");
}

#[test]
fn a_fixture_is_confirmed_only_where_it_built_with_the_winning_templates() {
    let t = shared_with_two_envs();
    let shared = t.path();
    let v1 = proven(&template("t", "Old title"), "2026-10-01 09:00:00", Some("QA"));
    let v2 = proven(&template("t", "New title"), "2026-11-01 09:00:00", Some("Staging"));
    let fx = fixture("f", &["t"]);
    for (env, tpl, ran) in [(A, &v1, "2026-11-02 09:00:00"), (B, &v2, "2026-11-03 09:00:00")] {
        put_template(shared, &env_holder(env), tpl);
        put_fixture(shared, &env_holder(env), &fx);
        put_runs(shared, &env_holder(env), ApiKind::Fixtures, "f", json!([fixture_run(ran, true)]));
    }

    fold_in(shared, NOW).unwrap().unwrap();

    let saved = fixture_store::load(&env_root(A, shared), ORG, PROJECT, "f").unwrap().unwrap();
    assert_eq!(saved, fx);
    let lookup = |id: &str| library_template(shared, id);
    let state = |env| load_state(&env_root(env, shared), ORG, PROJECT);
    assert!(is_confirmed(&fixture_here(&state(B), &fx, &lookup)), "Staging built it with the winning template");
    assert_eq!(fixture_here(&state(A), &fx, &lookup), Here::Unconfirmed, "QA built it with the version that lost");
}

#[test]
fn a_fixture_whose_newest_run_failed_is_not_confirmed() {
    let t = shared_with_two_envs();
    let shared = t.path();
    let tpl = proven(&template("t", "Add cycle"), "2026-10-01 09:00:00", Some("QA"));
    let fx = fixture("f", &["t"]);
    put_template(shared, &env_holder(A), &tpl);
    put_fixture(shared, &env_holder(A), &fx);
    put_runs(shared, &env_holder(A), ApiKind::Fixtures, "f", json!([fixture_run("2026-11-02 09:00:00", false), fixture_run("2026-11-01 09:00:00", true)]));

    fold_in(shared, NOW).unwrap().unwrap();

    let lookup = |id: &str| library_template(shared, id);
    let state = load_state(&env_root(A, shared), ORG, PROJECT);
    assert_eq!(fixture_here(&state, &fx, &lookup), Here::Unconfirmed);
    assert!(is_confirmed(&template_here(&state, &tpl)), "the template itself still is");
}

#[test]
fn without_history_the_file_time_decides() {
    let t = shared_with_two_envs();
    let shared = t.path();
    put_template(shared, &env_holder(A), &template("t", "Older file"));
    put_template(shared, &env_holder(B), &template("t", "Newer file"));
    let set_age = |env: &str, secs_ago: u64| {
        let f = std::fs::OpenOptions::new()
            .write(true)
            .open(kind_dir(shared, &env_holder(env), ApiKind::Templates).join("t.json"))
            .unwrap();
        f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(secs_ago)).unwrap();
    };
    set_age(A, 7200);
    set_age(B, 60);

    let report = fold_in(shared, NOW).unwrap().unwrap();

    assert_eq!(library_template(shared, "t").unwrap().title, "Newer file");
    assert_eq!(report.conflicts, 1);
}

#[test]
fn a_flow_with_a_saved_sample_beats_an_older_one_and_keeps_no_state() {
    let t = shared_with_two_envs();
    let shared = t.path();
    let mut old = flow("cycle");
    old.title = "Old flow".into();
    let mut new = flow("cycle");
    new.title = "New flow".into();
    new.saved = Some(v2_lib::api_templates::flow::FlowSaved { at: "2026-11-01 09:00:00".into(), sample: json!({}) });
    old.saved = Some(v2_lib::api_templates::flow::FlowSaved { at: "2026-10-01 09:00:00".into(), sample: json!({}) });
    for (env, f) in [(A, &new), (B, &old)] {
        put(&kind_dir(shared, &env_holder(env), ApiKind::Flows).join("cycle.json"), &serde_json::to_string_pretty(f).unwrap());
    }

    let report = fold_in(shared, NOW).unwrap().unwrap();

    assert_eq!(flow_store::load(&env_root(B, shared), ORG, PROJECT, "cycle").unwrap().unwrap().title, "New flow");
    assert_eq!(report, MoveReport { moved: 1, conflicts: 1, kept_aside: 1 });
    for env in [A, B] {
        assert!(!env_root(env, shared).join("flows").exists(), "the emptied flows folder is removed");
        let s: EnvApiState = load_state(&env_root(env, shared), ORG, PROJECT);
        assert!(s.templates.is_empty() && s.fixtures.is_empty());
    }
}

#[test]
fn a_library_copy_that_is_newer_stays_and_the_older_environment_copy_is_kept_aside() {
    let t = shared_with_two_envs();
    let shared = t.path();
    let in_library = proven(&template("t", "Library title"), "2026-12-01 09:00:00", Some("QA"));
    let in_env = proven(&template("t", "Env title"), "2026-10-01 09:00:00", Some("QA"));
    put(
        &shared.join("api-library").join("templates").join(slug()).join("t.json"),
        &serde_json::to_string_pretty(&in_library).unwrap(),
    );
    put_template(shared, &env_holder(A), &in_env);

    let report = fold_in(shared, NOW).unwrap().unwrap();

    assert_eq!(library_template(shared, "t").unwrap(), in_library);
    assert_eq!(report, MoveReport { moved: 0, conflicts: 1, kept_aside: 1 });
    assert!(aside_dir(shared, NOW).join(A).join("templates").join(slug()).join("t.json").is_file());
    assert_eq!(here_of(shared, A, &in_library), Here::Unconfirmed);
}

#[test]
fn a_removed_folder_only_fills_ids_no_environment_holds_and_is_emptied() {
    let t = shared_with_two_envs();
    let shared = t.path();
    let removed = "removed-envs/env-0000000c-1799000000000";
    put_template(shared, &env_holder(A), &template("t", "Live title"));
    put_template(shared, removed, &template("t", "Removed title"));
    put_template(shared, removed, &proven(&template("u", "Only removed"), "2026-10-01 09:00:00", Some("Gone")));
    put_runs(shared, removed, ApiKind::Templates, "u", json!([run_line("2026-10-01 09:00:00", "prove", true)]));

    let report = fold_in(shared, NOW).unwrap().unwrap();

    assert_eq!(library_template(shared, "t").unwrap().title, "Live title");
    assert_eq!(library_template(shared, "u").unwrap().title, "Only removed");
    assert_eq!(report, MoveReport { moved: 2, conflicts: 1, kept_aside: 1 });
    assert!(definition_files(&shared.join(removed)).is_empty(), "no definition is left in the removed folder");
    assert!(shared.join(removed).join("templates").join(slug()).join("u.runs.json").is_file());
    assert!(!shared.join(removed).join("api-state").exists(), "a removed folder is never given state");
    assert!(aside_dir(shared, NOW).join("env-0000000c-1799000000000").join("templates").join(slug()).join("t.json").is_file());
    for env in [A, B] {
        assert_eq!(here_of(shared, env, &template("u", "Only removed")), Here::Unconfirmed, "a removed folder confirms nothing");
    }
}

#[test]
fn the_move_is_idempotent() {
    let t = shared_with_two_envs();
    let shared = t.path();
    put_template(shared, &env_holder(A), &proven(&template("t", "Add cycle"), "2026-10-05 09:00:00", Some("QA")));
    put_template(shared, &env_holder(B), &template("u", "Other template"));

    assert!(fold_in(shared, NOW).unwrap().is_some());
    let before = std::fs::read_to_string(shared.join("api-library/templates").join(slug()).join("t.json")).unwrap();
    let state_before = std::fs::read_to_string(env_root(A, shared).join("api-state").join(format!("{}.json", slug()))).unwrap();

    assert_eq!(fold_in(shared, NOW + 1).unwrap(), None);

    let after = std::fs::read_to_string(shared.join("api-library/templates").join(slug()).join("t.json")).unwrap();
    assert_eq!(before, after);
    assert_eq!(state_before, std::fs::read_to_string(env_root(A, shared).join("api-state").join(format!("{}.json", slug()))).unwrap());
    assert!(!aside_dir(shared, NOW + 1).exists());
}

#[test]
fn an_unreadable_definition_is_kept_aside_and_the_move_finishes() {
    let t = shared_with_two_envs();
    let shared = t.path();
    put_template(shared, &env_holder(A), &template("good", "Good"));
    put(&kind_dir(shared, &env_holder(A), ApiKind::Templates).join("bad.json"), "{");

    let report = fold_in(shared, NOW).unwrap().unwrap();

    assert_eq!(report, MoveReport { moved: 1, conflicts: 0, kept_aside: 1 });
    assert!(library_template(shared, "good").is_some());
    assert!(!shared.join("api-library/templates").join(slug()).join("bad.json").exists());
    let kept = aside_dir(shared, NOW).join(A).join("templates").join(slug()).join("bad.json");
    assert_eq!(std::fs::read_to_string(kept).unwrap(), "{");
    assert!(definition_files(&env_root(A, shared)).is_empty());
}

#[test]
fn a_move_cut_short_after_its_mark_is_finished_by_the_next_start() {
    let t = shared_with_two_envs();
    let shared = t.path();
    let tpl = proven(&template("t", "Add cycle"), "2026-10-05 09:00:00", Some("QA"));
    put_template(shared, &env_holder(A), &tpl);
    // An old library, set aside by the cut-short start before the new one went in.
    let staging = shared.join(format!("{STAGING_PREFIX}5"));
    put(&staging.join("library-old").join("templates").join(slug()).join("older.json"), "{}");
    put(&staging.join("library/templates").join(slug()).join("t.json"), &serde_json::to_string_pretty(&tpl).unwrap());
    let mut state = EnvApiState::default();
    state.templates.insert(
        "t".into(),
        v2_lib::api_templates::here::ItemState {
            hash: template_hash(&tpl),
            uses: BTreeMap::new(),
            confirmed_at: Some("2026-10-05 09:00:00".into()),
            failure: None,
            failed_here: false,
        },
    );
    put(&staging.join("state").join(A).join(format!("{}.json", slug())), &serde_json::to_string(&state).unwrap());
    put(&staging.join("sources.json"), &json!([format!("envs/{A}/templates/{}/t.json", slug())]).to_string());
    put(&shared.join(format!("{STAGING_PREFIX}5.complete")), "1 0 0");
    assert!(!shared.join("api-library").exists());

    let report = fold_in(shared, NOW).unwrap().unwrap();

    assert_eq!(report.moved, 1);
    assert_eq!(library_template(shared, "t").unwrap(), tpl);
    assert!(is_confirmed(&here_of(shared, A, &tpl)));
    assert!(!kind_dir(shared, &env_holder(A), ApiKind::Templates).join("t.json").exists(), "the source is gone");
    assert!(staging_entries(shared).is_empty(), "neither the folder nor its mark is left");
}

#[test]
fn an_unmarked_staging_folder_is_discarded_and_rebuilt() {
    let t = shared_with_two_envs();
    let shared = t.path();
    let tpl = template("t", "Add cycle");
    put_template(shared, &env_holder(A), &tpl);
    let half = shared.join(format!("{STAGING_PREFIX}9"));
    put(&half.join("library/templates").join(slug()).join("stale.json"), "{}");
    put(&half.join("sources.json"), &json!([format!("envs/{A}/templates/{}/t.json", slug())]).to_string());

    let report = fold_in(shared, NOW).unwrap().unwrap();

    assert_eq!(report.moved, 1, "the fold was rebuilt from the environments, not taken from the half-built folder");
    assert_eq!(library_template(shared, "t").unwrap(), tpl);
    assert!(!shared.join("api-library/templates").join(slug()).join("stale.json").exists());
    assert!(staging_entries(shared).is_empty());
}

#[test]
fn kept_losers_are_purged_after_30_days_and_never_offered_as_a_copy_source() {
    let t = shared_with_two_envs();
    let shared = t.path();
    let day = 86_400_000u64;
    let old = NOW - 31 * day;
    let recent = NOW - 2 * day;
    for ms in [old, recent] {
        put(&aside_dir(shared, ms).join(A).join("templates").join(slug()).join("t.json"), "{}");
    }

    let choices = env_copy::copy_choices(shared, &env_file(), A, NOW).unwrap();
    assert_eq!(choices.sources.iter().map(|s| s.key.as_str()).collect::<Vec<_>>(), vec![B], "only the other environment");

    assert_eq!(roots::purge_removed(shared, NOW), 1);
    assert!(!aside_dir(shared, old).exists());
    assert!(aside_dir(shared, recent).exists());
    assert_eq!(roots::purge_removed(shared, NOW), 0);
}

#[test]
fn start_up_folds_only_where_the_env_migration_may_run() {
    let _s = serial::autorun();
    let t = shared_with_two_envs();
    let shared = t.path();
    put_template(shared, &env_holder(A), &template("t", "Add cycle"));

    let left_alone = roots::start_up(shared, false);
    assert!(left_alone.per_env);
    assert!(kind_dir(shared, &env_holder(A), ApiKind::Templates).join("t.json").is_file());
    assert!(!shared.join("api-library").exists());

    roots::start_up(shared, true);
    assert!(!kind_dir(shared, &env_holder(A), ApiKind::Templates).join("t.json").exists());
    assert!(library_template(shared, "t").is_some());
}

#[test]
fn the_shared_items_name_the_library_and_the_kept_versions() {
    assert_eq!(roots::SHARED_ITEMS.len(), 12);
    assert!(roots::SHARED_ITEMS.contains(&"api-library") && roots::SHARED_ITEMS.contains(&"removed-api"));
}

#[test]
fn the_move_log_line_names_no_path() {
    let line = move_log_line(&MoveReport { moved: 12, conflicts: 2, kept_aside: 1 });
    assert!(roots::path_free(&line), "{line}");
    assert_eq!(
        line,
        "Auto Run: API templates, flows and fixtures are now shared by every environment (12 moved, 1 kept aside for 30 days)"
    );
}

// ---- histories that were copied, not earned ----------------------------------

#[test]
fn a_history_copied_byte_for_byte_confirms_nothing_but_each_own_history_does() {
    let proved = proven(&template("t", "Add cycle"), "2026-10-05 09:00:00", None);
    let copied = json!([run_line("2026-10-08 09:00:00", "run", true)]);

    let t = shared_with_two_envs();
    let shared = t.path();
    for env in [A, B] {
        put_template(shared, &env_holder(env), &proved);
        put_runs(shared, &env_holder(env), ApiKind::Templates, "t", copied.clone());
    }
    fold_in(shared, NOW).unwrap().unwrap();
    assert_eq!(here_of(shared, A, &proved), Here::Unconfirmed);
    assert_eq!(here_of(shared, B, &proved), Here::Unconfirmed);

    let t = shared_with_two_envs();
    let shared = t.path();
    for (env, at) in [(A, "2026-10-08 09:00:00"), (B, "2026-10-09 09:00:00")] {
        put_template(shared, &env_holder(env), &proved);
        put_runs(shared, &env_holder(env), ApiKind::Templates, "t", json!([run_line(at, "run", true)]));
    }
    fold_in(shared, NOW).unwrap().unwrap();
    assert!(is_confirmed(&here_of(shared, A, &proved)));
    assert!(is_confirmed(&here_of(shared, B, &proved)));
}

#[test]
fn a_copy_whose_source_ran_again_afterwards_confirms_nowhere_from_the_shared_lines() {
    let proved = proven(&template("t", "Add cycle"), "2026-10-05 09:00:00", None);
    let t = shared_with_two_envs();
    let shared = t.path();
    let copied = run_line("2026-10-08 09:00:00", "run", true);
    put_template(shared, &env_holder(A), &proved);
    put_template(shared, &env_holder(B), &proved);
    // B holds what Copy Auto Run copied from A; A then ran once more.
    put_runs(shared, &env_holder(B), ApiKind::Templates, "t", json!([copied.clone()]));
    put_runs(shared, &env_holder(A), ApiKind::Templates, "t", json!([copied, run_line("2026-10-09 09:00:00", "run", false)]));

    fold_in(shared, NOW).unwrap().unwrap();

    assert_eq!(here_of(shared, B, &proved), Here::Unconfirmed, "a copied line is not a run here");
    assert_eq!(here_of(shared, A, &proved), Here::Unconfirmed, "nor there: it was shared, and its own newer run failed");
}

#[test]
fn a_copy_taken_from_an_environment_that_was_since_removed_confirms_nothing() {
    let proved = proven(&template("t", "Add cycle"), "2026-10-05 09:00:00", None);
    let t = shared_with_two_envs();
    let shared = t.path();
    let removed = "removed-envs/env-0000000c-1799000000000";
    let copied = json!([run_line("2026-10-08 09:00:00", "run", true)]);
    put_template(shared, &env_holder(B), &proved);
    put_runs(shared, &env_holder(B), ApiKind::Templates, "t", copied.clone());
    put_template(shared, removed, &proved);
    put_runs(shared, removed, ApiKind::Templates, "t", copied);

    fold_in(shared, NOW).unwrap().unwrap();

    assert_eq!(here_of(shared, B, &proved), Here::Unconfirmed);
}

#[test]
fn a_copied_fixture_history_confirms_on_neither_environment() {
    let t = shared_with_two_envs();
    let shared = t.path();
    let tpl = proven(&template("t", "Add cycle"), "2026-10-01 09:00:00", Some("QA"));
    let fx = fixture("f", &["t"]);
    for env in [A, B] {
        put_template(shared, &env_holder(env), &tpl);
        put_fixture(shared, &env_holder(env), &fx);
        put_runs(shared, &env_holder(env), ApiKind::Fixtures, "f", json!([fixture_run("2026-11-02 09:00:00", true)]));
    }

    fold_in(shared, NOW).unwrap().unwrap();

    let lookup = |id: &str| library_template(shared, id);
    let state = |env| load_state(&env_root(env, shared), ORG, PROJECT);
    assert_eq!(fixture_here(&state(A), &fx, &lookup), Here::Unconfirmed);
    assert_eq!(fixture_here(&state(B), &fx, &lookup), Here::Unconfirmed);
}

#[test]
fn lines_unique_to_an_environment_still_confirm_it_beside_shared_ones() {
    let proved = proven(&template("t", "Add cycle"), "2026-10-05 09:00:00", None);
    let t = shared_with_two_envs();
    let shared = t.path();
    let shared_line = run_line("2026-10-08 09:00:00", "run", true);
    for (env, own) in [(A, "2026-10-09 09:00:00"), (B, "2026-10-10 09:00:00")] {
        put_template(shared, &env_holder(env), &proved);
        put_runs(shared, &env_holder(env), ApiKind::Templates, "t", json!([shared_line.clone(), run_line(own, "run", true)]));
    }

    fold_in(shared, NOW).unwrap().unwrap();

    assert!(is_confirmed(&here_of(shared, A, &proved)), "its own later run");
    assert!(is_confirmed(&here_of(shared, B, &proved)), "its own later run");
}

// ---- a rename that fails ------------------------------------------------------

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// A fold whose folder moves fail where `fails(from, to)` says so.
fn fold_failing(shared: &Path, fails: impl Fn(&Path, &Path) -> bool) -> Result<Option<MoveReport>, String> {
    let held = roots::switch_lock().write().unwrap_or_else(|e| e.into_inner());
    fold_in_with(&held, shared, NOW, &mut |a, b| {
        if fails(a, b) {
            Err(std::io::Error::other("held by another program"))
        } else {
            std::fs::rename(a, b)
        }
    })
}

fn library_file(shared: &Path, id: &str) -> PathBuf {
    shared.join("api-library/templates").join(slug()).join(format!("{id}.json"))
}

/// A library holding `old`, and environment A holding the proven template `t`.
fn library_and_one_env() -> (tempfile::TempDir, ApiTemplate) {
    let t = shared_with_two_envs();
    let shared = t.path();
    put(&library_file(shared, "old"), &serde_json::to_string_pretty(&template("old", "Old")).unwrap());
    let tpl = proven(&template("t", "Add cycle"), "2026-10-05 09:00:00", Some("QA"));
    put_template(shared, &env_holder(A), &tpl);
    (t, tpl)
}

#[test]
fn when_the_old_library_cannot_be_set_aside_what_is_saved_meanwhile_is_kept_and_the_next_start_finishes() {
    let (t, tpl) = library_and_one_env();
    let shared = t.path();

    let failed = fold_failing(shared, |_, to| name_of(to) == "library-old");

    assert!(failed.is_err());
    assert!(library_file(shared, "old").is_file(), "the live library is untouched");
    assert!(kind_dir(shared, &env_holder(A), ApiKind::Templates).join("t.json").is_file(), "nothing was deleted");
    // The app carries on and saves into the live library.
    put(&library_file(shared, "late"), "{}");

    let report = fold_in(shared, NOW + 1).unwrap().unwrap();

    assert_eq!(report.moved, 1);
    assert_eq!(library_template(shared, "t").unwrap(), tpl);
    assert!(library_file(shared, "old").is_file());
    assert!(aside_dir(shared, NOW).join("library-old/templates").join(slug()).join("late.json").is_file(), "the meanwhile save is kept aside");
    assert!(!kind_dir(shared, &env_holder(A), ApiKind::Templates).join("t.json").exists());
    assert!(staging_entries(shared).is_empty());
}

#[test]
fn when_the_new_library_cannot_be_put_in_place_the_old_one_is_put_back_and_the_next_start_finishes() {
    let (t, tpl) = library_and_one_env();
    let shared = t.path();

    let failed = fold_failing(shared, |from, _| name_of(from) == "library");

    assert!(failed.is_err());
    assert!(library_file(shared, "old").is_file(), "the app still has its library");
    assert!(!library_file(shared, "t").exists());

    let report = fold_in(shared, NOW + 1).unwrap().unwrap();

    assert_eq!(report.moved, 1);
    assert_eq!(library_template(shared, "t").unwrap(), tpl);
    assert!(library_file(shared, "old").is_file(), "the old content is in the new library");
    assert!(aside_dir(shared, NOW).join("library-old/templates").join(slug()).join("old.json").is_file());
    assert!(staging_entries(shared).is_empty());
}

#[test]
fn when_the_old_library_cannot_even_be_put_back_nothing_wedges_and_nothing_is_lost() {
    let (t, tpl) = library_and_one_env();
    let shared = t.path();

    let failed = fold_failing(shared, |from, to| name_of(from) == "library" || (name_of(from) == "library-old" && name_of(to) == "api-library"));

    assert!(failed.is_err());
    assert!(!shared.join("api-library").exists(), "the session has no library");
    // The first save of the session makes a new one.
    put(&library_file(shared, "late"), "{}");

    let report = fold_in(shared, NOW + 1).unwrap().unwrap();

    assert_eq!(report.moved, 1);
    assert_eq!(library_template(shared, "t").unwrap(), tpl);
    assert!(library_file(shared, "old").is_file());
    assert!(aside_dir(shared, NOW).join("library-live/templates").join(slug()).join("late.json").is_file(), "the session's own library is kept aside");
    assert!(aside_dir(shared, NOW).join("library-old/templates").join(slug()).join("old.json").is_file());
    assert!(staging_entries(shared).is_empty());
    assert_eq!(fold_in(shared, NOW + 2).unwrap(), None);
}
