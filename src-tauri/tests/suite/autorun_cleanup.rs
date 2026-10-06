//! Clean up test-made drafts (design doc section 4): the preview's filters,
//! what can be deleted, the run with the fake browsers, the prove rule for
//! a delete template, the refusals that keep a delete template out of
//! everything but Clean up, and the tripwire that keeps Clean up out of the
//! bridge and the MCP tools.

use crate::api_templates_runner::{answer, rig, FakeBrowsers, Rig, ORG, PAGE, PROJECT, QUICK_PAUSES};
use crate::common::quick;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use v2_lib::api_templates::runner::RUN_LIMIT;
use v2_lib::api_templates::{store, ApiTemplate, Proven};
use v2_lib::autorun::cleanup::{
    delete_template_for, no_delete_template, not_active, preview, run_cleanup_within, CleanupProgress, CleanupQuery,
    NOTHING_TO_DELETE, NOT_IN_PREVIEW, NO_SUCH_ENVIRONMENT, OLDER_THAN_MIN, PREFIX_NEEDED, PROVE_REFUSAL,
};
use v2_lib::autorun::test_made::{self, TestMade};

/// The time every test looks from.
fn now() -> chrono::DateTime<chrono::Utc> {
    "2026-10-06T12:00:00Z".parse().unwrap()
}

/// `now` less `minutes`, as the record writes a time.
fn ago(minutes: i64) -> String {
    (now() - chrono::Duration::minutes(minutes)).format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

const DAY: i64 = 24 * 60;

fn entry(env: &str, kind: &str, id: &str, name: &str, minutes_old: i64, status: &str) -> TestMade {
    TestMade {
        environment: env.into(),
        kind: kind.into(),
        id: id.into(),
        name: name.into(),
        created_at: ago(minutes_old),
        fixture: "draft-cycle".into(),
        run_id: "run-1".into(),
        case_id: None,
        status: status.into(),
    }
}

/// Writes the record as the fixture runner would have left it. The record's
/// writers are the crate's own, so a test puts the file there itself.
fn seed(root: &Path, entries: &[TestMade]) {
    std::fs::write(root.join("test-made.json"), serde_json::to_string_pretty(entries).unwrap()).unwrap();
}

fn status_of(root: &Path, env: &str, kind: &str, id: &str) -> String {
    test_made::list(root)
        .into_iter()
        .find(|e| e.environment == env && e.kind == kind && e.id == id)
        .map(|e| e.status)
        .expect("in the record")
}

fn active(root: &Path) -> v2_lib::environments::Environment {
    v2_lib::environments::active(root).unwrap()
}

fn proof(at: &str, env: Option<&str>) -> Proven {
    Proven {
        at: at.into(),
        origin: "https://hr.example.internal".into(),
        account: "admin".into(),
        outputs: BTreeMap::new(),
        environment: env.map(str::to_string),
    }
}

/// A delete template for `kind`: one request that deletes the record named
/// by `{{id}}`. Unproven; `proven` adds the proof.
fn delete_draft(id: &str, kind: Option<&str>) -> ApiTemplate {
    let mut v = json!({
        "id": id, "title": "Delete a draft", "module": "PMS", "effect": "delete",
        "description": "d", "sources": ["x:1"], "antiforgery": { "page": PAGE },
        "params": [ { "name": "id", "type": "string", "required": true } ],
        "steps": [ { "name": "Delete", "method": "POST", "path": "/hr/pmsv10/performancecycle",
                     "query": { "handler": "DeleteCycle" }, "form": { "Id": "{{id}}" } } ],
        "outputs": []
    });
    if let Some(k) = kind {
        v["deletes_kind"] = json!(k);
    }
    serde_json::from_value(v).unwrap()
}

fn proven(mut t: ApiTemplate, at: &str, env: Option<&str>) -> ApiTemplate {
    t.proven = Some(proof(at, env));
    t
}

fn query(root: &Path, prefix: &str, days: i32) -> CleanupQuery {
    CleanupQuery { environment: active(root).id, prefix: prefix.into(), older_than_days: days }
}

fn ids(lines: &[v2_lib::autorun::cleanup::CleanupLine]) -> Vec<&str> {
    lines.iter().map(|l| l.entry.id.as_str()).collect()
}

/// Review Focus 4: the status, environment, prefix and age filters, each at
/// its boundary.
#[test]
fn the_preview_filters_by_status_environment_prefix_and_age() {
    let dir = tempfile::tempdir().unwrap();
    let env = active(dir.path()).id;
    seed(
        dir.path(),
        &[
            entry(&env, "cycle", "1", "AUTOTEST exactly seven days", 7 * DAY, "present"),
            entry(&env, "cycle", "2", "autotest in lower case", 8 * DAY, "present"),
            entry(&env, "cycle", "3", "AUTOTEST a minute too young", 7 * DAY - 1, "present"),
            entry(&env, "cycle", "4", "AUTOTEST already deleted", 30 * DAY, "deleted"),
            entry(&env, "cycle", "5", "AUTOTEST failed before", 30 * DAY, "delete failed: expected status 200, got 500"),
            entry("env-0000ffff", "cycle", "6", "AUTOTEST elsewhere", 30 * DAY, "present"),
            entry(&env, "cycle", "7", "Manual cycle", 30 * DAY, "present"),
            entry(&env, "cycle", "8", "AUTOTES too short a name", 30 * DAY, "present"),
        ],
    );
    let lines = preview(dir.path(), ORG, PROJECT, &query(dir.path(), "AUTOTEST", 7), now()).unwrap();
    assert_eq!(ids(&lines), vec!["1", "2", "5"], "{lines:?}");

    // The prefix is trimmed and its case does not matter.
    let lines = preview(dir.path(), ORG, PROJECT, &query(dir.path(), "  autoTest ", 7), now()).unwrap();
    assert_eq!(ids(&lines), vec!["1", "2", "5"]);

    // A day older is a day fewer: 8 days leaves out the one exactly 7 old.
    let lines = preview(dir.path(), ORG, PROJECT, &query(dir.path(), "AUTOTEST", 8), now()).unwrap();
    assert_eq!(ids(&lines), vec!["2", "5"]);

    // One day takes in the one a minute short of seven.
    let lines = preview(dir.path(), ORG, PROJECT, &query(dir.path(), "AUTOTEST", 1), now()).unwrap();
    assert_eq!(ids(&lines), vec!["1", "2", "3", "5"]);
}

#[test]
fn the_preview_refuses_what_it_cannot_filter_by() {
    let dir = tempfile::tempdir().unwrap();
    for days in [0, -1] {
        assert_eq!(preview(dir.path(), ORG, PROJECT, &query(dir.path(), "AUTOTEST", days), now()), Err(OLDER_THAN_MIN.to_string()));
    }
    assert_eq!(OLDER_THAN_MIN, "older than must be at least 1 day");
    assert_eq!(preview(dir.path(), ORG, PROJECT, &query(dir.path(), "   ", 7), now()), Err(PREFIX_NEEDED.to_string()));
    let elsewhere = CleanupQuery { environment: "env-0000ffff".into(), prefix: "AUTOTEST".into(), older_than_days: 7 };
    assert_eq!(preview(dir.path(), ORG, PROJECT, &elsewhere, now()), Err(NO_SUCH_ENVIRONMENT.to_string()));
}

/// A kind with no proven delete template - none at all, only an unproven
/// one, one proven in another environment, or an old delete template that
/// names no kind - is shown, not deletable, with its note.
#[test]
fn a_kind_with_no_proven_delete_template_is_not_deletable() {
    let dir = tempfile::tempdir().unwrap();
    let env = active(dir.path());
    for t in [
        proven(delete_draft("remove-cycle", Some("cycle")), "2026-10-01 09:00:00", Some(&env.name)),
        delete_draft("remove-suite", Some("suite")),
        proven(delete_draft("remove-goal", Some("goal")), "2026-10-01 09:00:00", Some("Staging")),
        proven(delete_draft("remove-anything", None), "2026-10-01 09:00:00", None),
    ] {
        store::save(dir.path(), ORG, PROJECT, &t).unwrap();
    }
    seed(
        dir.path(),
        &[
            entry(&env.id, "cycle", "1", "AUTOTEST cycle", 9 * DAY, "present"),
            entry(&env.id, "suite", "2", "AUTOTEST suite", 9 * DAY, "present"),
            entry(&env.id, "goal", "3", "AUTOTEST goal", 9 * DAY, "present"),
            entry(&env.id, "plan", "4", "AUTOTEST plan", 9 * DAY, "present"),
        ],
    );
    let lines = preview(dir.path(), ORG, PROJECT, &query(dir.path(), "AUTOTEST", 7), now()).unwrap();
    let got: Vec<(bool, Option<&str>)> = lines.iter().map(|l| (l.deletable, l.note.as_deref())).collect();
    assert_eq!(
        got,
        vec![
            (true, None),
            (false, Some("no proven delete template for suite")),
            (false, Some("no proven delete template for goal")),
            (false, Some("no proven delete template for plan")),
        ]
    );
    assert_eq!(no_delete_template("suite"), "no proven delete template for suite");
}

/// Of several proven delete templates for a kind, the most recently proven
/// one; a proof that names no environment counts in any.
#[test]
fn the_newest_proven_delete_template_is_the_one_used() {
    let older = proven(delete_draft("older", Some("cycle")), "2026-10-01 09:00:00", Some("Default"));
    let newer = proven(delete_draft("newer", Some("cycle")), "2026-10-03 09:00:00", None);
    let other = proven(delete_draft("other", Some("cycle")), "2026-10-05 09:00:00", Some("Staging"));
    let all = [older.clone(), newer, other];
    assert_eq!(delete_template_for(&all, "cycle", "Default").map(|t| t.id.as_str()), Some("newer"));
    assert_eq!(delete_template_for(&all, "cycle", "Staging").map(|t| t.id.as_str()), Some("other"));
    assert_eq!(delete_template_for(&all[..1], "cycle", "default").map(|t| t.id.as_str()), Some("older"), "names ignore case");
    assert_eq!(delete_template_for(&all, "suite", "Default"), None);
}

/// A rig whose root holds a proven delete template for cycles and the
/// record `entries` (each in the active environment).
fn cleanup_rig(responses: Vec<Value>, entries: &[(&str, &str, i64)]) -> (Rig, String) {
    let r = rig(responses, None);
    let env = active(r.root.path());
    let t = proven(delete_draft("remove-cycle", Some("cycle")), "2026-10-01 09:00:00", Some(&env.name));
    store::save(r.root.path(), ORG, PROJECT, &t).unwrap();
    let made: Vec<TestMade> =
        entries.iter().map(|(id, name, days)| entry(&env.id, "cycle", id, name, days * DAY, "present")).collect();
    seed(r.root.path(), &made);
    (r, env.id)
}

async fn clean(
    r: &mut Rig,
    q: &CleanupQuery,
    chosen: &[&str],
    cancel: &AtomicBool,
    heard: &mut Vec<CleanupProgress>,
    stop_after: Option<u32>,
) -> Result<v2_lib::autorun::cleanup::CleanupReport, String> {
    let chosen: Vec<String> = chosen.iter().map(|s| s.to_string()).collect();
    run_cleanup_within(
        &mut r.browsers,
        r.root.path(),
        ORG,
        PROJECT,
        q,
        &chosen,
        &quick(),
        RUN_LIMIT,
        &QUICK_PAUSES,
        now(),
        cancel,
        |p| {
            if stop_after == Some(p.done) {
                cancel.store(true, Ordering::SeqCst);
            }
            heard.push(p);
        },
    )
    .await
}

fn deleted_ids(r: &Rig) -> Vec<Value> {
    r.fetched().iter().map(|f| f[0]["body"]["fields"]["Id"].clone()).collect()
}

/// The ticked drafts go one at a time, in the record's order, in one
/// browser signed in once; each is marked deleted and said as it goes. An
/// unticked one, and an entry of the same id in another environment, stay
/// as they were (`test_made::set_status` matches environment, kind and id).
#[tokio::test]
async fn the_run_deletes_in_order_and_records_each() {
    let _act = crate::serial::activity_log();
    let _leases = crate::serial::account_leases();
    let (mut r, env) = cleanup_rig(
        vec![answer(200, json!({ "success": true })), answer(200, json!({ "success": true }))],
        &[("11", "AUTOTEST first", 9), ("12", "AUTOTEST second", 8), ("13", "AUTOTEST left alone", 8)],
    );
    let mut made = test_made::list(r.root.path());
    made.push(entry("env-0000ffff", "cycle", "11", "AUTOTEST first", 9 * DAY, "present"));
    seed(r.root.path(), &made);

    let q = query(r.root.path(), "AUTOTEST", 7);
    let cancel = AtomicBool::new(false);
    let mut heard = vec![];
    let report = clean(&mut r, &q, &["12", "11"], &cancel, &mut heard, None).await.unwrap();

    assert_eq!(deleted_ids(&r), vec![json!("11"), json!("12")], "in the record's order");
    assert_eq!(
        heard,
        vec![
            CleanupProgress { done: 1, total: 2, id: "11".into(), outcome: "deleted".into() },
            CleanupProgress { done: 2, total: 2, id: "12".into(), outcome: "deleted".into() },
        ]
    );
    assert!(!report.stopped);
    assert_eq!(report.total, 2);
    assert!(report.results.iter().all(|x| x.ok && x.outcome == "deleted"), "{report:?}");
    assert_eq!(status_of(r.root.path(), &env, "cycle", "11"), "deleted");
    assert_eq!(status_of(r.root.path(), &env, "cycle", "12"), "deleted");
    assert_eq!(status_of(r.root.path(), &env, "cycle", "13"), "present", "not ticked");
    assert_eq!(status_of(r.root.path(), "env-0000ffff", "cycle", "11"), "present", "another environment's");
    assert_eq!((r.browsers.opened, r.browsers.closed, r.sign_ins()), (1, 1, 1));

    // Deleted ones leave the preview.
    let lines = preview(r.root.path(), ORG, PROJECT, &q, now()).unwrap();
    assert_eq!(ids(&lines), vec!["13"]);
}

/// A delete that fails is recorded with the runner's own sentence, said as
/// it comes, and the cleanup goes on to the next; the failed one is in the
/// next preview again.
#[tokio::test]
async fn a_failed_delete_is_recorded_with_its_reason() {
    let _act = crate::serial::activity_log();
    let _leases = crate::serial::account_leases();
    let (mut r, env) = cleanup_rig(
        vec![answer(500, json!({ "error": "no" })), answer(200, json!({ "success": true }))],
        &[("21", "AUTOTEST refused", 9), ("22", "AUTOTEST fine", 9)],
    );
    let q = query(r.root.path(), "AUTOTEST", 7);
    let cancel = AtomicBool::new(false);
    let mut heard = vec![];
    let report = clean(&mut r, &q, &["21", "22"], &cancel, &mut heard, None).await.unwrap();

    let why = &heard[0].outcome;
    assert!(why.starts_with("nothing had been captured yet; failed at Delete (DeleteCycle)"), "{why}");
    assert!(why.contains("500"), "{why}");
    assert!(!why.contains("hr.example.internal"), "no host: {why}");
    assert_eq!(status_of(r.root.path(), &env, "cycle", "21"), format!("delete failed: {why}"));
    assert_eq!(status_of(r.root.path(), &env, "cycle", "22"), "deleted");
    assert_eq!(heard[1], CleanupProgress { done: 2, total: 2, id: "22".into(), outcome: "deleted".into() });
    assert_eq!(report.results.iter().map(|x| x.ok).collect::<Vec<_>>(), vec![false, true]);

    let lines = preview(r.root.path(), ORG, PROJECT, &q, now()).unwrap();
    assert_eq!(ids(&lines), vec!["21"], "a failed delete is offered again");
}

/// Stop is heard between deletes: the one sent finishes, the rest stay
/// present.
#[tokio::test]
async fn stop_between_deletes_leaves_the_rest_present() {
    let _act = crate::serial::activity_log();
    let _leases = crate::serial::account_leases();
    let (mut r, env) = cleanup_rig(
        vec![answer(200, json!({ "success": true }))],
        &[("31", "AUTOTEST one", 9), ("32", "AUTOTEST two", 9), ("33", "AUTOTEST three", 9)],
    );
    let q = query(r.root.path(), "AUTOTEST", 7);
    let cancel = AtomicBool::new(false);
    let mut heard = vec![];
    let report = clean(&mut r, &q, &["31", "32", "33"], &cancel, &mut heard, Some(1)).await.unwrap();

    assert!(report.stopped);
    assert_eq!(report.results.len(), 1);
    assert_eq!(heard.len(), 1);
    assert_eq!(deleted_ids(&r), vec![json!("31")]);
    assert_eq!(status_of(r.root.path(), &env, "cycle", "31"), "deleted");
    assert_eq!(status_of(r.root.path(), &env, "cycle", "32"), "present");
    assert_eq!(status_of(r.root.path(), &env, "cycle", "33"), "present");
    assert_eq!(r.browsers.closed, 1, "the browser is closed on a stop too");

    // A stop before the first delete deletes nothing and opens nothing.
    let (mut r, _) = cleanup_rig(vec![], &[("41", "AUTOTEST one", 9)]);
    let cancel = AtomicBool::new(true);
    let q = q_for(&r);
    let report = clean(&mut r, &q, &["41"], &cancel, &mut vec![], None).await.unwrap();
    assert!(report.stopped);
    assert!(report.results.is_empty());
    assert_eq!(r.browsers.opened, 0);
}

fn q_for(r: &Rig) -> CleanupQuery {
    query(r.root.path(), "AUTOTEST", 7)
}

/// Only an id the preview shows now is deleted; one outside it - unknown,
/// already deleted, too young, of another environment - refuses the whole
/// run before anything opens. A choice with nothing deletable in it is
/// refused too, and so is an environment that is not the active one.
#[tokio::test]
async fn ids_outside_the_preview_are_refused_and_nothing_runs() {
    let _act = crate::serial::activity_log();
    let _leases = crate::serial::account_leases();
    let (mut r, env) = cleanup_rig(vec![], &[("51", "AUTOTEST old", 9), ("52", "AUTOTEST young", 1)]);
    let mut made = test_made::list(r.root.path());
    made.push(entry(&env, "cycle", "53", "AUTOTEST gone", 9 * DAY, "deleted"));
    made.push(entry(&env, "suite", "54", "AUTOTEST suite", 9 * DAY, "present"));
    seed(r.root.path(), &made);
    let q = q_for(&r);
    let cancel = AtomicBool::new(false);

    for chosen in [vec!["51", "999"], vec!["52"], vec!["53"]] {
        let got = clean(&mut r, &q, &chosen, &cancel, &mut vec![], None).await;
        assert_eq!(got, Err(NOT_IN_PREVIEW.to_string()), "{chosen:?}");
    }
    // In the preview, but no delete template for suites.
    let got = clean(&mut r, &q, &["54"], &cancel, &mut vec![], None).await;
    assert_eq!(got, Err(NOTHING_TO_DELETE.to_string()));

    // Another environment, saved but not active.
    let mut staging = active(r.root.path());
    staging.id = String::new();
    staging.name = "Staging".into();
    let db = staging.db_id.clone();
    let file = v2_lib::environments::save_env(r.root.path(), staging, &[db]).unwrap();
    let staging = file.environments.iter().find(|e| e.name == "Staging").unwrap().id.clone();
    let mut made = test_made::list(r.root.path());
    made.push(entry(&staging, "cycle", "55", "AUTOTEST staging", 9 * DAY, "present"));
    seed(r.root.path(), &made);
    let there = CleanupQuery { environment: staging, ..q.clone() };
    let got = clean(&mut r, &there, &["55"], &cancel, &mut vec![], None).await;
    assert_eq!(got, Err(not_active("Staging")));

    assert_eq!(r.browsers.opened, 0, "nothing opened for a refused run");
    assert!(r.fetched().is_empty());
    for id in ["51", "52"] {
        assert_eq!(status_of(r.root.path(), &env, "cycle", id), "present");
    }
}

/// The bridge's prove of a delete template, end to end.
mod proving {
    use super::*;
    use v2_lib::ai_bridge::{api_template_prove, api_template_run, BridgeContext};
    use v2_lib::browser::launch::Browser;

    fn ctx() -> BridgeContext {
        BridgeContext { org: ORG.into(), project: PROJECT.into(), api_writes: true, ..BridgeContext::default() }
    }

    fn no_db(_: &BridgeContext) -> Result<crate::common::FakeStageDb, (u16, String)> {
        panic!("a database was asked for by a template that is on no flow")
    }

    fn never_opened(_: Browser) -> FakeBrowsers {
        panic!("a browser was opened for a call that should have been refused first")
    }

    fn body(id: Value) -> String {
        json!({ "template": delete_draft("remove-cycle", Some("cycle")), "account": "admin", "values": { "id": id } })
            .to_string()
    }

    /// Refused unless `{{id}}` is a present entry of the template's kind in
    /// the active environment: an unknown id, one already deleted, one of
    /// another kind, one of another environment. Nothing opens.
    #[tokio::test]
    async fn a_delete_template_proved_off_the_record_is_refused() {
        let _root = crate::serial::autorun();
        let _slot = crate::serial::api_template_run();
        let _act = crate::serial::activity_log();
        let dir = tempfile::tempdir().unwrap();
        v2_lib::autorun::store::set_root(dir.path().to_path_buf());
        v2_lib::autorun::recipe::save_recipe(dir.path(), ORG, PROJECT, &crate::common::recipe()).unwrap();
        v2_lib::autorun::accounts::save_accounts(dir.path(), &[crate::common::account()]).unwrap();
        let env = active(dir.path()).id;
        seed(
            dir.path(),
            &[
                entry(&env, "cycle", "61", "AUTOTEST gone", DAY, "deleted"),
                entry(&env, "suite", "62", "AUTOTEST suite", DAY, "present"),
                entry("env-0000ffff", "cycle", "63", "AUTOTEST elsewhere", DAY, "present"),
            ],
        );
        for id in [json!("999"), json!("61"), json!("62"), json!("63")] {
            let (status, out) = api_template_prove(&ctx(), &body(id.clone()), never_opened, no_db, &quick()).await;
            assert_eq!((status, out.as_str()), (400, PROVE_REFUSAL), "{id}");
        }
        assert_eq!(PROVE_REFUSAL, "a delete template is only proven on a draft the tests made");
        assert_eq!(store::load(dir.path(), ORG, PROJECT, "remove-cycle").unwrap(), None, "nothing saved");
    }

    /// On a present entry the proof runs, is saved, and marks the entry it
    /// deleted - and only that one.
    #[tokio::test]
    async fn a_proof_on_the_record_marks_the_entry_deleted() {
        let _root = crate::serial::autorun();
        let _slot = crate::serial::api_template_run();
        let _act = crate::serial::activity_log();
        let _leases = crate::serial::account_leases();
        let mut r = rig(vec![answer(200, json!({ "success": true }))], None);
        let browsers =
            std::mem::replace(&mut r.browsers, FakeBrowsers { next: None, opened: 0, closed: 0, last: None });
        let root = r.root.path().to_path_buf();
        v2_lib::autorun::store::set_root(root.clone());
        let env = active(&root).id;
        seed(
            &root,
            &[entry(&env, "cycle", "71", "AUTOTEST cycle", DAY, "present"), entry(&env, "cycle", "72", "AUTOTEST other", DAY, "present")],
        );
        let (status, out) = api_template_prove(&ctx(), &body(json!("71")), |_| browsers, no_db, &quick()).await;
        assert_eq!(status, 200, "{out}");
        assert!(store::load(&root, ORG, PROJECT, "remove-cycle").unwrap().unwrap().proven.is_some());
        assert_eq!(status_of(&root, &env, "cycle", "71"), "deleted");
        assert_eq!(status_of(&root, &env, "cycle", "72"), "present");
        assert_eq!(deleted_ids(&r), vec![json!("71")]);
    }

    /// Review Focus 3, pinned beside Clean up: `run_api_template` refuses a
    /// saved delete template before anything opens.
    #[tokio::test]
    async fn run_api_template_refuses_a_delete_template() {
        let _root = crate::serial::autorun();
        let _slot = crate::serial::api_template_run();
        let _act = crate::serial::activity_log();
        let dir = tempfile::tempdir().unwrap();
        v2_lib::autorun::store::set_root(dir.path().to_path_buf());
        v2_lib::autorun::recipe::save_recipe(dir.path(), ORG, PROJECT, &crate::common::recipe()).unwrap();
        v2_lib::autorun::accounts::save_accounts(dir.path(), &[crate::common::account()]).unwrap();
        let t = proven(delete_draft("remove-cycle", Some("cycle")), "2026-10-01 09:00:00", None);
        store::save(dir.path(), ORG, PROJECT, &t).unwrap();
        let body = json!({ "id": "remove-cycle", "account": "admin", "values": { "id": "1" } }).to_string();
        let (status, out) = api_template_run(&ctx(), &body, never_opened, no_db, &quick()).await;
        assert_eq!(status, 400);
        assert_eq!(out, "template remove-cycle deletes, and only Clean up test-made drafts runs a delete template");
    }
}

/// Review Focus 3, pinned beside Clean up: a fixture refuses a delete
/// template step, at save and again when it runs, before any browser opens.
#[tokio::test]
async fn a_fixture_step_on_a_delete_template_is_refused() {
    use v2_lib::api_templates::fixture::{Fixture, FixtureStep};
    use v2_lib::api_templates::fixture_run::{run_fixture_within, Clock};
    let _act = crate::serial::activity_log();
    let mut r = rig(vec![], None);
    let t = proven(delete_draft("remove-cycle", Some("cycle")), "2026-10-01 09:00:00", None);
    store::save(r.root.path(), ORG, PROJECT, &t).unwrap();
    let f = Fixture {
        id: "deletes".into(),
        name: "Deletes".into(),
        account: "admin".into(),
        steps: vec![FixtureStep { template: "remove-cycle".into(), params: [("id".to_string(), "1".to_string())].into() }],
        outputs: BTreeMap::new(),
        creates: vec![],
    };
    let clock = Clock { year: 2026, month: 10, day: 6, hour: 12, minute: 0, second: 0 };
    let report =
        run_fixture_within(&mut r.browsers, r.root.path(), ORG, PROJECT, &f, &quick(), RUN_LIMIT, &QUICK_PAUSES, clock).await;
    assert!(!report.ok);
    assert_eq!(report.failed.as_deref(), Some("step 1: template remove-cycle deletes, and a fixture never deletes"));
    assert_eq!(r.browsers.opened, 0);
    assert!(r.fetched().is_empty());
}

/// Review Focus 5, for Clean up: it starts only from the app, by a person.
/// No bridge route and no MCP tool reaches `run_cleanup` or names a cleanup
/// command, and the commands are registered for the webview.
#[test]
fn no_bridge_route_or_mcp_tool_reaches_clean_up() {
    let read = |p: &str| {
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(p)).unwrap().replace("\r\n", "\n")
    };
    let bridge = read("src/ai_bridge.rs");
    let mcp = read("src/mcp.rs");
    for (name, text) in [("src/ai_bridge.rs", &bridge), ("src/mcp.rs", &mcp)] {
        for forbidden in [
            "cleanup::run_cleanup",
            "run_cleanup",
            "cleanup::{",
            "auto_run_cleanup_preview",
            "auto_run_cleanup_run",
            "auto_run_cleanup_stop",
            "autorun-cleanup",
        ] {
            assert!(!text.contains(forbidden), "{name} reaches {forbidden}");
        }
    }
    let lib = read("src/lib.rs");
    for command in ["auto_run_cleanup_preview", "auto_run_cleanup_run", "auto_run_cleanup_stop"] {
        assert!(lib.contains(&format!("api_templates::{command},")), "{command} is not registered");
    }
}
