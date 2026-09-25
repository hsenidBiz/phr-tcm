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
