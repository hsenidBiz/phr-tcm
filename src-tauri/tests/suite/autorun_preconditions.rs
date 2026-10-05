//! Auto Run preconditions: the records a case relies on, checked through the
//! flow machinery before the case signs in, and validated on every save.
//! See the design "Auto Run run safety" §2.

use crate::common::{cycle_flow_json, FakeStageDb};
use serde_json::{json, Value};
use std::path::Path;
use v2_lib::api_templates::flow::Flow;
use v2_lib::api_templates::flow_store;
use v2_lib::autorun::preconditions::{
    check_all, check_case, check_script, environment_db, not_met, problems, NoDb, NEED_DB,
};
use v2_lib::autorun::{store, CaseScript, Precondition};

fn block<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(f)
}

fn cycle_flow() -> Flow {
    serde_json::from_value(cycle_flow_json()).unwrap()
}

/// A flow whose subject is the cycle's NAME, a string, so a value with
/// quotes in it can be sent through the substitution.
fn named_cycle_flow() -> Flow {
    serde_json::from_value(json!({
        "id": "pms-named-cycle",
        "title": "Named cycle",
        "module": "PMS / Performance Cycle",
        "subject": { "name": "cycleName", "type": "string" },
        "stages": [
            { "id": "setup", "title": "Cycle setup", "creates": true,
              "check": "SELECT 1 FROM PeoplesHR.perf_cycle WHERE cycle_name = {{cycleName}} /*setup*/" }
        ]
    }))
    .unwrap()
}

fn pre(flow: &str, stage: &str, value: Value, why: Option<&str>) -> Precondition {
    Precondition { flow: flow.into(), stage: stage.into(), value, why: why.map(str::to_string) }
}

fn publish(why: Option<&str>) -> Precondition {
    pre("pms-performance-cycle", "publish", json!(274), why)
}

fn script_with(case_id: i32, preconditions: Value) -> Value {
    json!({ "case_id": case_id, "title": "t", "preconditions": preconditions, "steps": [
        { "step_number": 1, "actions": [{ "kind": "check_text", "value": "ok" }] }
    ] })
}

fn save_flows(root: &Path) {
    flow_store::save(root, "acme", "Web", &cycle_flow()).unwrap();
    flow_store::save(root, "acme", "Web", &named_cycle_flow()).unwrap();
}

// ------------------------------------------------------------ at run time

#[test]
fn every_stage_done_lets_the_case_go_on() {
    let _g = crate::serial::activity_log();
    let db = FakeStageDb::new().answer("/*publish*/", Ok(true)).answer("/*rules*/", Ok(true));
    let both = [publish(None), pre("pms-performance-cycle", "rules", json!(274), None)];
    assert_eq!(block(check_all(&db, &[cycle_flow()], &both)), Ok(()));
    assert_eq!(db.calls().len(), 2, "each precondition is asked once");
}

#[test]
fn a_stage_not_done_blocks_the_case_with_the_designs_sentence() {
    let _g = crate::serial::activity_log();
    let db = FakeStageDb::new().answer("/*publish*/", Ok(false));
    assert_eq!(
        block(check_all(&db, &[cycle_flow()], &[publish(None)])),
        Err("precondition not met: Publish for 274 (Performance cycle wizard)".to_string())
    );
    assert_eq!(
        block(check_all(&db, &[cycle_flow()], &[publish(Some("the case opens a published cycle"))])),
        Err(
            "precondition not met: Publish for 274 (Performance cycle wizard) - the case opens a published cycle"
                .to_string()
        )
    );
    // A blank reason is no reason.
    assert_eq!(not_met("Publish", &json!("Q4"), "Wizard", Some("  ")), "precondition not met: Publish for Q4 (Wizard)");
}

#[test]
fn the_first_precondition_not_met_is_the_one_named_and_nothing_after_it_is_asked() {
    let _g = crate::serial::activity_log();
    let db = FakeStageDb::new().answer("/*rules*/", Ok(false)).answer("/*publish*/", Ok(true));
    let both = [pre("pms-performance-cycle", "rules", json!(274), None), publish(None)];
    assert_eq!(
        block(check_all(&db, &[cycle_flow()], &both)),
        Err("precondition not met: Evaluation rules for 274 (Performance cycle wizard)".to_string())
    );
    assert_eq!(db.calls().len(), 1);
}

#[test]
fn a_check_that_could_not_run_blocks_in_the_gates_words_with_no_sql_or_error_text() {
    let _g = crate::serial::activity_log();
    let db = FakeStageDb::new().answer("/*publish*/", Err("Login failed for user sa on tcp:db01".into()));
    let out = block(check_all(&db, &[cycle_flow()], &[publish(None)])).unwrap_err();
    assert_eq!(
        out,
        "precondition could not be checked: the check for Publish could not be run - see the activity folder in Settings, Logs"
    );
    for leak in ["SELECT", "Login failed", "db01", "tcp:"] {
        assert!(!out.contains(leak), "{leak} in {out}");
    }
}

#[test]
fn each_check_is_recorded_in_the_activity_log_as_a_precondition() {
    let _l = crate::serial::log_tail();
    let _g = crate::serial::activity_log();
    let dir = tempfile::tempdir().unwrap();
    v2_lib::activity_log::init(dir.path().to_path_buf());
    let db = FakeStageDb::new().answer("/*publish*/", Ok(true));
    block(check_all(&db, &[cycle_flow()], &[publish(None)])).unwrap();
    let recs = crate::common::activity_records(dir.path(), "db");
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0]["purpose"], "precondition");
    assert_eq!(recs[0]["stage"], "publish");
    assert!(recs[0]["template"].is_null());
}

/// Review Focus 4: the value goes through the flow's own typed
/// substitution, quoted and escaped, never pasted into the check.
#[test]
fn a_value_with_quotes_goes_through_the_substitution() {
    let _g = crate::serial::activity_log();
    let db = FakeStageDb::new().answer("/*setup*/", Ok(true));
    let sly = "Q4 O'Brien'; DELETE FROM PeoplesHR.perf_cycle --";
    let p = pre("pms-named-cycle", "setup", json!(sly), None);
    assert_eq!(block(check_all(&db, &[named_cycle_flow()], &[p])), Ok(()));
    let calls = db.calls();
    assert_eq!(calls.len(), 1);
    assert!(
        calls[0].contains("cycle_name = N'Q4 O''Brien''; DELETE FROM PeoplesHR.perf_cycle --'"),
        "the value was not quoted and escaped: {}",
        calls[0]
    );
}

#[test]
fn a_flow_or_stage_gone_since_the_save_blocks_the_case_without_asking() {
    let _g = crate::serial::activity_log();
    let db = FakeStageDb::new();
    assert_eq!(
        block(check_all(&db, &[], &[publish(None)])),
        Err("precondition could not be checked: precondition 1: no flow pms-performance-cycle".to_string())
    );
    assert!(db.calls().is_empty());
}

#[test]
fn no_database_blocks_a_case_with_preconditions_and_never_one_without() {
    let dir = tempfile::tempdir().unwrap();
    let no_db: Result<NoDb, String> = Err(NEED_DB.to_string());
    assert_eq!(NEED_DB, "preconditions need a database chosen on the AI Bridge tab");
    assert_eq!(block(check_case(&no_db, dir.path(), "acme", "Web", &[publish(None)])), Err(NEED_DB.to_string()));
    assert_eq!(block(check_case(&no_db, dir.path(), "acme", "Web", &[])), Ok(()));
}

/// The active environment names a database this build does not know, or
/// there is no store to look it up in: no database is chosen.
#[test]
fn an_environment_with_no_usable_database_is_no_database() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("environments.json"),
        json!({ "active": "env-0000abcd", "environments": [
            { "id": "env-0000abcd", "name": "Default", "start_url": "", "allowed_origins": [],
              "db_id": "gone", "test_environment": false }
        ] })
        .to_string(),
    )
    .unwrap();
    let store = v2_lib::db::MemoryStore::default();
    assert_eq!(environment_db(dir.path(), Some(&store)).err().as_deref(), Some(NEED_DB));
    assert_eq!(environment_db(dir.path(), None).err().as_deref(), Some(NEED_DB));
}

#[test]
fn the_supervised_check_reads_the_script_and_asks_for_a_database_only_when_it_needs_one() {
    let _g = crate::serial::activity_log();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    save_flows(root);
    let never = || -> Result<FakeStageDb, String> { panic!("the database was looked up for a case with no preconditions") };
    assert_eq!(block(check_script(root, "acme", "Web", 1, never)), Ok(None), "no script");

    let plain: CaseScript = serde_json::from_value(script_with(2, json!([]))).unwrap();
    store::save_script(root, &plain).unwrap();
    assert_eq!(block(check_script(root, "acme", "Web", 2, never)), Ok(None), "no preconditions");

    let needs: CaseScript = serde_json::from_value(script_with(3, json!([
        { "flow": "pms-performance-cycle", "stage": "publish", "value": 274 }
    ])))
    .unwrap();
    store::save_script(root, &needs).unwrap();
    let not_done = || Ok(FakeStageDb::new().answer("/*publish*/", Ok(false)));
    assert_eq!(
        block(check_script(root, "acme", "Web", 3, not_done)),
        Ok(Some("precondition not met: Publish for 274 (Performance cycle wizard)".to_string()))
    );
    let done = || Ok(FakeStageDb::new().answer("/*publish*/", Ok(true)));
    assert_eq!(block(check_script(root, "acme", "Web", 3, done)), Ok(None));
    let none = || -> Result<FakeStageDb, String> { Err(NEED_DB.to_string()) };
    assert_eq!(block(check_script(root, "acme", "Web", 3, none)), Ok(Some(NEED_DB.to_string())));
}

// ------------------------------------------------------------- on a save

#[test]
fn a_save_names_every_problem_one_sentence_each() {
    let flows = [cycle_flow()];
    let sent = [
        pre("pms-cycle", "publish", json!(274), None),
        pre("pms-performance-cycle", "published", json!(274), None),
        pre("pms-performance-cycle", "publish", Value::Null, None),
        pre("pms-performance-cycle", "publish", json!("274"), None),
        publish(None),
    ];
    assert_eq!(
        problems(&flows, &sent),
        vec![
            "precondition 1: no flow pms-cycle",
            "precondition 2: flow Performance cycle wizard has no stage published",
            "precondition 3: give the value the flow's checks take",
            "precondition 4: give the value the flow's checks take",
        ]
    );
    // A string subject wants a string that is not blank.
    let named = [named_cycle_flow()];
    assert_eq!(problems(&named, &[pre("pms-named-cycle", "setup", json!("  "), None)]).len(), 1);
    assert!(problems(&named, &[pre("pms-named-cycle", "setup", json!("Q4"), None)]).is_empty());
}

/// Every door a script comes in by - the editor, a file import, the
/// assistant's save and its repair - refuses a precondition the project
/// cannot check, and writes nothing.
#[tokio::test]
async fn every_save_path_refuses_a_precondition_it_cannot_check() {
    use v2_lib::ai_bridge::{route, BridgeContext};
    use v2_lib::autorun::store::{load_script, set_root};
    use v2_lib::commands::autorun::{import_scripts_from_path, save_script_from_editor};

    let _root = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("data");
    save_flows(&root);
    let bad = json!([
        { "flow": "pms-cycle", "stage": "publish", "value": 274 },
        { "flow": "pms-performance-cycle", "stage": "published" }
    ]);
    let expected = |id: i32| {
        format!(
            "case {id}: precondition 1: no flow pms-cycle; precondition 2: flow Performance cycle wizard has no stage published; precondition 2: give the value the flow's checks take"
        )
    };

    // The editor.
    let script: CaseScript = serde_json::from_value(script_with(7, bad.clone())).unwrap();
    assert_eq!(save_script_from_editor(&root, "acme", "Web", script).unwrap_err(), expected(7));
    assert!(load_script(&root, 7).unwrap().is_none());

    // A file import: all or nothing.
    let good = json!([{ "flow": "pms-performance-cycle", "stage": "publish", "value": 274 }]);
    let file = dir.path().join("bundle.json");
    std::fs::write(&file, json!([script_with(8, good.clone()), script_with(9, bad.clone())]).to_string()).unwrap();
    assert_eq!(import_scripts_from_path(&root, "acme", "Web", file.to_str().unwrap()).unwrap_err(), expected(9));
    assert!(load_script(&root, 8).unwrap().is_none());

    // The assistant's save, refused before it needs Azure DevOps.
    set_root(root.clone());
    let ctx = BridgeContext { org: "acme".into(), project: "Web".into(), ..BridgeContext::default() };
    let (status, out) = route(&ctx, None, "POST", "/autorun-script", &json!([script_with(10, bad.clone())]).to_string(), "1.0.0").await;
    assert_eq!((status, out), (400, expected(10)));
    assert!(load_script(&root, 10).unwrap().is_none());

    // A repair of a saved script cannot bring one in either.
    let saved: CaseScript = serde_json::from_value(script_with(11, good.clone())).unwrap();
    save_script_from_editor(&root, "acme", "Web", saved.clone()).unwrap();
    let body = json!({
        "scripts": [script_with(11, bad.clone())],
        "edits": [{ "case_id": 11, "steps": [], "why": "the cycle changed" }]
    });
    let (status, out) = route(&ctx, None, "POST", "/autorun-script", &body.to_string(), "1.0.0").await;
    assert_eq!((status, out), (400, expected(11)));
    assert_eq!(load_script(&root, 11).unwrap().unwrap(), saved, "the script on disk is unchanged");

    // A good one saves, and is kept on the script.
    assert_eq!(
        load_script(&root, 11).unwrap().unwrap().preconditions,
        vec![pre("pms-performance-cycle", "publish", json!(274), None)]
    );
}

// ------------------------------------------------------- old script files

/// Review Focus 5: a script file from before preconditions loads with none,
/// and a script with none saves exactly as it did.
#[test]
fn an_old_script_loads_unchanged_and_saves_byte_identical() {
    let old = json!({ "case_id": 1, "title": "t", "steps": [
        { "step_number": 1, "actions": [{ "kind": "check_text", "value": "ok" }] }
    ] });
    let sc: CaseScript = serde_json::from_value(old.clone()).unwrap();
    assert!(sc.preconditions.is_empty());
    assert_eq!(serde_json::to_value(&sc).unwrap(), old);
    assert!(!serde_json::to_string(&sc).unwrap().contains("preconditions"));

    let dir = tempfile::tempdir().unwrap();
    store::save_script(dir.path(), &sc).unwrap();
    assert_eq!(store::load_script(dir.path(), 1).unwrap().unwrap(), sc);

    // With some, they are written and read back.
    let with: CaseScript = serde_json::from_value(script_with(2, json!([
        { "flow": "pms-performance-cycle", "stage": "publish", "value": 274, "why": "it opens a published cycle" }
    ])))
    .unwrap();
    let text = serde_json::to_string(&with).unwrap();
    assert!(text.contains("\"preconditions\":[{\"flow\":\"pms-performance-cycle\",\"stage\":\"publish\",\"value\":274,\"why\":\"it opens a published cycle\"}]"), "{text}");
}

// ------------------------------------------------------------- the guide

#[test]
fn the_guide_teaches_preconditions() {
    let g = v2_lib::autorun::guide::autorun_guide();
    let section = g.split_once("## Preconditions").expect("the guide has no preconditions section").1;
    let section = section.split("\n## ").next().unwrap();
    for term in [
        "\"preconditions\"",
        "rely on a record built beforehand",
        "Add a precondition whenever",
        "list_api_templates",
        "precondition not met: Publish for 274 (Performance cycle wizard) - the case opens a published cycle",
        "precondition could not be checked: the check for Publish could not be run - see the activity folder in Settings, Logs",
        NEED_DB,
        "precondition 1: no flow",
        "has no stage",
        "give the value the flow's checks take",
        "A repair sends",
    ] {
        assert!(section.contains(term), "the preconditions section never says {term:?}");
    }
    assert!(!section.contains('\u{2014}'), "no em dashes in text an assistant reads");
    // The example precondition is one a save would take.
    let at = section.find("{ \"flow\"").unwrap();
    let end = at + section[at..].find('}').unwrap() + 1;
    let p: Precondition = serde_json::from_str(&section[at..end]).unwrap();
    assert!(problems(&[cycle_flow()], &[p]).is_empty());
}
