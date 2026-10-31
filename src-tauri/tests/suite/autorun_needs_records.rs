//! `needs_records`: the kept records a script relies on, declared through
//! saved setups and checked, by each setup's database find rule, before the
//! case signs in. A missing record Blocks the case; an undeclared one is a
//! note at save, never a refusal.

use serde_json::{json, Value};
use v2_lib::api_templates::gate::StageDb;
use v2_lib::autorun::needs_records::{check_case_needs, check_declared, check_needed, NeedsOutcome, NOT_CHECKED};
use v2_lib::autorun::preconditions::{undeclared_record_note, PreconditionDb, NEED_DB};
use v2_lib::autorun::setup_find::RowSource;
use v2_lib::autorun::setups::SetupFile;
use v2_lib::autorun::{nav, CaseScript, NeededRecord};

fn setup_file(setups: Value) -> SetupFile {
    serde_json::from_value(json!({ "setups": setups })).unwrap()
}

fn setup(name: &str, find: Value) -> Value {
    json!({
        "name": name, "description": "d", "inputs": [], "actions": [], "find": find,
        "creates": [], "returns": 0, "version": 1, "changes": 0, "saved_at": 0
    })
}

fn db_find() -> Value {
    json!({ "database": { "sql": "SELECT Id, Name FROM dbo.Scale WHERE Name LIKE {prefix} + N'%'", "id_column": "Id", "name_column": "Name" } })
}

fn needs(setup: &str, why: &str) -> NeededRecord {
    NeededRecord { setup: setup.into(), why: why.into() }
}

fn rows(text: &str) -> String {
    format!("Id\tName\n--\t----\n{text}\n\n(1 rows affected)\n")
}

const EMPTY: &str = "Id\tName\n--\t----\n\n(0 rows affected)\n";

struct Rows(Result<String, String>);

impl RowSource for Rows {
    async fn select(&self, _sql: &str) -> Result<String, String> {
        self.0.clone()
    }
}

/// A run database whose reads answer with rows and whose stage checks
/// are never used.
struct RunDb(Result<String, String>);

impl StageDb for RunDb {
    fn label(&self) -> String {
        "fake".into()
    }
    async fn read(&self, _sql: &str) -> Result<bool, String> {
        Err("not used".into())
    }
    async fn select(&self, _sql: &str) -> Result<String, String> {
        self.0.clone()
    }
}

fn script(extra: Value) -> CaseScript {
    let mut v = json!({ "case_id": 7, "title": "t", "steps": [
        { "step_number": 1, "actions": [{ "kind": "click", "selector": { "text": "AUTOTEST Scale" } }] }
    ] });
    for (k, val) in extra.as_object().unwrap() {
        v[k] = val.clone();
    }
    serde_json::from_value(v).unwrap()
}

#[test]
fn save_refuses_unknown_setup() {
    let file = setup_file(json!([]));
    let said = check_declared(&[needs("Rating scale", "x")], &file);
    assert_eq!(
        said,
        vec!["needs_records names \"Rating scale\", which is not a saved setup - save it with save_setup first".to_string()]
    );
}

#[test]
fn save_refuses_setup_without_database_rule() {
    let browser_only = json!({ "browser": [{ "kind": "expect_visible", "selector": { "text": "{prefix}" } }] });
    let file = setup_file(json!([setup("Rating scale", browser_only)]));
    let said = check_declared(&[needs("Rating scale", "x")], &file);
    assert_eq!(
        said,
        vec!["needs_records names \"Rating scale\", which has no database find rule, so a run cannot look for its record before signing in".to_string()]
    );
    let good = setup_file(json!([setup("Rating scale", db_find())]));
    assert!(check_declared(&[needs("rating scale", "x")], &good).is_empty());
}

#[tokio::test]
async fn missing_record_blocks_with_sentence() {
    let file = setup_file(json!([setup("Rating scale", db_find())]));
    let out = check_needed(&Rows(Ok(EMPTY.into())), &[needs("Rating scale", "the AUTOTEST rating scale")], &file, "AUTOTEST", true).await;
    assert_eq!(
        out,
        NeedsOutcome::Missing(
            "the case needs the AUTOTEST rating scale (setup \"Rating scale\"), which is not there - run_setup Rating scale builds it".into()
        )
    );
}

#[tokio::test]
async fn found_record_lets_case_run() {
    let file = setup_file(json!([setup("Rating scale", db_find())]));
    let out = check_needed(&Rows(Ok(rows("4\tAUTOTEST Scale"))), &[needs("Rating scale", "x")], &file, "AUTOTEST", true).await;
    assert_eq!(out, NeedsOutcome::Met);
    // A row that is not a test record is not the record.
    let out = check_needed(&Rows(Ok(rows("4\tOther Scale"))), &[needs("Rating scale", "x")], &file, "AUTOTEST", true).await;
    assert!(matches!(out, NeedsOutcome::Missing(_)));
}

#[tokio::test]
async fn database_off_is_said_not_blocked() {
    let file = setup_file(json!([setup("Rating scale", db_find())]));
    let out = check_needed(&Rows(Ok(EMPTY.into())), &[needs("Rating scale", "x")], &file, "AUTOTEST", false).await;
    assert_eq!(out, NeedsOutcome::NotChecked("needs_records not checked: database read access is off".into()));
    assert_eq!(NOT_CHECKED, "needs_records not checked: database read access is off");
}

#[tokio::test]
async fn a_find_rule_that_cannot_run_blocks_without_sql_in_the_sentence() {
    let file = setup_file(json!([setup("Rating scale", db_find())]));
    let out = check_needed(&Rows(Err("the database is not reachable right now".into())), &[needs("Rating scale", "x")], &file, "AUTOTEST", true).await;
    match out {
        NeedsOutcome::Missing(s) => {
            assert!(s.contains("could not be checked") && s.contains("Rating scale"), "{s}");
            assert!(!s.to_lowercase().contains("select"), "{s}");
        }
        other => panic!("{other:?}"),
    }
}

fn save_setups(root: &std::path::Path, file: &SetupFile) {
    std::fs::create_dir_all(root.join("projects")).unwrap();
    std::fs::write(
        v2_lib::autorun::setups::setups_path(root, "acme", "Web"),
        serde_json::to_string(file).unwrap(),
    )
    .unwrap();
}

#[tokio::test]
async fn the_run_check_follows_the_database_choice() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    save_setups(root, &setup_file(json!([setup("Rating scale", db_find())])));
    let needed = [needs("Rating scale", "the AUTOTEST rating scale")];

    let missing = PreconditionDb::Ready(RunDb(Ok(EMPTY.into())));
    let c = check_case_needs(&missing, root, "acme", "Web", &needed).await;
    assert!(c.blocked.as_deref().unwrap().starts_with("the case needs the AUTOTEST rating scale"));

    let there = PreconditionDb::Ready(RunDb(Ok(rows("4\tAUTOTEST Scale"))));
    let c = check_case_needs(&there, root, "acme", "Web", &needed).await;
    assert!(c.blocked.is_none() && c.notice.is_none());

    let off: PreconditionDb<RunDb> = PreconditionDb::ReadingOff;
    let c = check_case_needs(&off, root, "acme", "Web", &needed).await;
    assert_eq!(c.blocked, None);
    assert_eq!(c.notice.as_deref(), Some(NOT_CHECKED));

    let none: PreconditionDb<RunDb> = PreconditionDb::Missing(NEED_DB.into());
    let c = check_case_needs(&none, root, "acme", "Web", &needed).await;
    assert_eq!(c.blocked.as_deref(), Some(NEED_DB));

    // Nothing declared: nothing asked, whatever the database is.
    let c = check_case_needs(&none, root, "acme", "Web", &[]).await;
    assert!(c.blocked.is_none() && c.notice.is_none());
}

#[test]
fn undeclared_note_quiet_when_needs_records_set() {
    let bare = script(json!({}));
    let note = undeclared_record_note(&bare, None, "AUTOTEST", &[]).expect("a note");
    assert!(note.contains("needs_records"), "{note}");
    let declared = script(json!({ "needs_records": [{ "setup": "Rating scale", "why": "the scale" }] }));
    assert_eq!(undeclared_record_note(&declared, None, "AUTOTEST", &[]), None);
}

#[test]
fn old_script_without_field_reads_and_writes_the_same() {
    let old = r#"{"case_id":7,"title":"t","steps":[{"step_number":1,"actions":[]}]}"#;
    let s: CaseScript = serde_json::from_str(old).unwrap();
    assert!(s.needs_records.is_empty());
    let v: Value = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert!(v.get("needs_records").is_none());
    let with = script(json!({ "needs_records": [{ "setup": "A", "why": "b" }] }));
    let text = serde_json::to_string(&with).unwrap();
    assert!(text.contains("\"needs_records\":[{\"setup\":\"A\",\"why\":\"b\"}]"), "{text}");
}

#[test]
fn every_save_path_validates_the_declaration() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let s = script(json!({ "needs_records": [{ "setup": "Rating scale", "why": "the scale" }] }));
    let refused = nav::check_project_rules(root, "acme", "Web", &[s.clone()]).unwrap_err();
    assert!(refused.contains("case 7") && refused.contains("not a saved setup"), "{refused}");

    save_setups(root, &setup_file(json!([setup("Rating scale", db_find())])));
    nav::check_project_rules(root, "acme", "Web", &[s]).unwrap();
    // A script declaring nothing reads no setups file at all.
    nav::check_project_rules(root, "acme", "Other", &[script(json!({}))]).unwrap();
}

#[tokio::test]
async fn the_supervised_check_reads_a_script_that_only_declares_needed_records() {
    let _g = crate::serial::activity_log();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    save_setups(root, &setup_file(json!([setup("Rating scale", db_find())])));
    let s = script(json!({ "needs_records": [{ "setup": "Rating scale", "why": "the AUTOTEST rating scale" }] }));
    v2_lib::autorun::store::save_script(root, &s).unwrap();
    let missing = || PreconditionDb::Ready(RunDb(Ok(EMPTY.into())));
    let c = v2_lib::autorun::preconditions::check_script(root, "acme", "Web", 7, missing).await.unwrap();
    assert!(c.blocked.unwrap().contains("which is not there - run_setup Rating scale builds it"));
    let there = || PreconditionDb::Ready(RunDb(Ok(rows("4\tAUTOTEST Scale"))));
    let c = v2_lib::autorun::preconditions::check_script(root, "acme", "Web", 7, there).await.unwrap();
    assert!(c.blocked.is_none());
}
