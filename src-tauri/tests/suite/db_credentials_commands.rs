//! What the database commands do beneath the Tauri layer: a Test connection
//! run through a fake sqlcmd, and the one-time import of a connection
//! string saved before databases had ids. Everything runs against
//! `MemoryStore` - a test must never write to the real Credential Manager.

use std::path::Path;
use v2_lib::db::credentials::*;
use v2_lib::db::{Output, Runner};
use v2_lib::db_defaults::DB_PRESETS;

struct Fake(Result<Output, String>, std::sync::Mutex<Vec<Vec<String>>>);
impl Runner for Fake {
    async fn run(&self, _: &Path, args: &[String], _: &[(String, String)], _: std::time::Duration) -> Result<Output, String> {
        self.1.lock().unwrap().push(args.to_vec());
        match &self.0 {
            Ok(o) => Ok(Output { status: o.status, stdout: o.stdout.clone(), stderr: o.stderr.clone() }),
            Err(e) => Err(e.clone()),
        }
    }
}
fn ok() -> Fake {
    Fake(Ok(Output { status: 0, stdout: "1".into(), stderr: String::new() }), Default::default())
}

#[tokio::test]
async fn a_test_that_connects_names_database_server_and_user() {
    let r = ok();
    let msg = test_connection_with(&r, Path::new("sqlcmd"), "Server=h,1433;Database=Hr;User Id=me;Password=pw").await.unwrap();
    assert_eq!(msg, "Connected to Hr on h,1433 as me.");
    assert!(r.1.lock().unwrap()[0].iter().any(|a| a.contains("SELECT 1")));
}

#[tokio::test]
async fn a_failed_test_reports_the_server_reason_without_the_password() {
    let r = Fake(Ok(Output { status: 1, stdout: "Login failed for user 'me'. pw-Zq9".into(), stderr: String::new() }), Default::default());
    let err = test_connection_with(&r, Path::new("sqlcmd"), "Server=h;Database=Hr;User Id=me;Password=pw-Zq9").await.unwrap_err();
    assert!(err.contains("Login failed for user 'me'."));
    assert!(!err.contains("pw-Zq9"));
}

#[tokio::test]
async fn a_process_that_cannot_start_is_reported_without_the_password() {
    let r = Fake(Err("sqlcmd could not be started: -P pw-Zq9".into()), Default::default());
    let err = test_connection_with(&r, Path::new("sqlcmd"), "Server=h;Database=Hr;User Id=me;Password=pw-Zq9").await.unwrap_err();
    assert!(err.contains("could not be started"), "{err}");
    assert!(!err.contains("pw-Zq9"));
}

#[tokio::test]
async fn a_string_missing_a_key_is_refused_before_anything_runs() {
    let r = ok();
    let err = test_connection_with(&r, Path::new("sqlcmd"), "Server=h;Database=Hr;User Id=me").await.unwrap_err();
    assert!(err.contains("Password="), "{err}");
    assert!(r.1.lock().unwrap().is_empty(), "sqlcmd ran for a string that does not parse");
}

#[test]
fn legacy_import_selects_a_shipped_database_without_storing_it() {
    let s = MemoryStore::default();
    let p = DB_PRESETS[2];
    let spaced = p.connection_string.replace(';', " ; ");
    assert_eq!(import_legacy(&s, &spaced).unwrap(), p.id);
    assert!(databases(&s).iter().all(|d| !d.customised));
}

#[test]
fn legacy_import_of_anything_else_becomes_your_own_database() {
    let s = MemoryStore::default();
    assert_eq!(import_legacy(&s, "Server=x;Database=y;User Id=u;Password=p").unwrap(), OWN_ID);
    assert_eq!(resolve(&s, OWN_ID).unwrap().as_deref(), Some("Server=x;Database=y;User Id=u;Password=p"));
}

#[test]
fn legacy_import_of_something_that_is_not_a_connection_stores_nothing() {
    let s = MemoryStore::default();
    assert!(import_legacy(&s, "not a connection string").is_err());
    assert_eq!(resolve(&s, OWN_ID).unwrap(), None);
}

#[test]
fn only_own_and_the_shipped_ids_are_known() {
    let s = MemoryStore::default();
    assert!(is_known(&s, OWN_ID));
    assert!(DB_PRESETS.iter().all(|p| is_known(&s, p.id)));
    assert!(!is_known(&s, "a-preset-since-removed"));
    assert!(!is_known(&s, ""));
}

/// Removing one of the person's own databases asks the environments first:
/// one that still uses it would be left pointing at nothing.
#[test]
fn a_database_an_environment_uses_cannot_be_removed() {
    use v2_lib::commands::ai_tools::remove_custom_with;
    use v2_lib::environments::{load_or_init, save_env};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("autorun");
    let s = WithList { store: MemoryStore::default(), list: dir.path().join("databases.json") };
    let form = DbCredentialsForm { server: "h".into(), port: None, database: "d".into(), user: "u".into(), password: Some("p".into()), trust_cert: false };
    let staging = add_custom(&s, "Staging", &form).unwrap();
    let known: Vec<String> = databases(&s).into_iter().map(|d| d.id).collect();

    let mut default = load_or_init(&root, Some(&staging.id)).unwrap().environments.remove(0);
    let qa = v2_lib::environments::Environment {
        id: String::new(),
        name: "QA".into(),
        start_url: String::new(),
        allowed_origins: vec![],
        db_id: staging.id.clone(),
        test_environment: false,
    };
    let mut qa = save_env(&root, qa, &known).unwrap().environments.into_iter().find(|e| e.name == "QA").unwrap();

    assert_eq!(
        remove_custom_with(&root, &s, &staging.id).unwrap_err(),
        "\"Staging\" is used by the environments \"Default\", \"QA\" - pick another database for them first"
    );
    qa.db_id = DB_PRESETS[0].id.into();
    save_env(&root, qa, &known).unwrap();
    assert_eq!(
        remove_custom_with(&root, &s, &staging.id).unwrap_err(),
        "\"Staging\" is used by the environment \"Default\" - pick another database for it first"
    );
    // Refused means untouched: the login and the name are still there.
    assert!(is_known(&s, &staging.id));
    assert!(s.get(&format!("tcm-v2/db/{}", staging.id)).unwrap().is_some());

    default.db_id = DB_PRESETS[0].id.into();
    save_env(&root, default, &known).unwrap();
    remove_custom_with(&root, &s, &staging.id).unwrap();
    assert!(!is_known(&s, &staging.id));
    assert!(s.get(&format!("tcm-v2/db/{}", staging.id)).unwrap().is_none());
}

/// With no environments yet, nothing uses anything - and asking makes none.
#[test]
fn a_removal_before_any_environment_exists_makes_none() {
    use v2_lib::commands::ai_tools::remove_custom_with;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("autorun");
    let s = WithList { store: MemoryStore::default(), list: dir.path().join("databases.json") };
    remove_custom_with(&root, &s, OWN_ID).unwrap();
    assert!(!is_known(&s, OWN_ID));
    assert!(!root.join("environments.json").exists());
    assert_eq!(
        remove_custom_with(&root, &s, DB_PRESETS[0].id).unwrap_err(),
        "A shipped database can't be removed."
    );
}

/// A connection string an old version kept still lands in `own` after
/// `own` was removed: it comes back on the list to hold it.
#[test]
fn legacy_import_brings_own_back_when_it_was_removed() {
    let dir = tempfile::tempdir().unwrap();
    let s = WithList { store: MemoryStore::default(), list: dir.path().join("databases.json") };
    remove_custom(&s, OWN_ID).unwrap();
    assert!(!is_known(&s, OWN_ID));
    assert_eq!(import_legacy(&s, "Server=x;Database=y;User Id=u;Password=p").unwrap(), OWN_ID);
    assert!(is_known(&s, OWN_ID));
    let own = databases(&s).into_iter().find(|d| d.id == OWN_ID).unwrap();
    assert_eq!((own.label.as_str(), own.server.as_str()), ("Your own database", "x"));
}
