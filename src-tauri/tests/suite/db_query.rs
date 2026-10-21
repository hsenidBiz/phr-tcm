//! Procedures, views and functions through the assistant's database tool:
//! which connection may make one, the writes switch in front of it, and
//! that a batch never carries one. The real process is never started -
//! `Runner` is faked, as in `db_sqlcmd`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use v2_lib::db::batch::{validate, OBJECT_ON_ITS_OWN};
use v2_lib::db::guard::OBJECTS_NEED_MODULE;
use v2_lib::db::query::{access_with, run_batch_query, run_query, WRITES_OFF};
use v2_lib::db::{parse_connection, Access, BatchStatement, Connection, Output, Runner, READ_ONLY_SENTENCE};
use v2_lib::db_defaults::DB_PRESETS;
use v2_lib::modules::Module;

#[derive(Default)]
struct FakeRunner {
    calls: Mutex<Vec<Vec<String>>>,
}

impl FakeRunner {
    fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }
}

impl Runner for FakeRunner {
    async fn run(
        &self,
        _exe: &Path,
        args: &[String],
        _env: &[(String, String)],
        _timeout: Duration,
    ) -> Result<Output, String> {
        self.calls.lock().unwrap().push(args.to_vec());
        Ok(Output { status: 0, stdout: String::new(), stderr: String::new() })
    }
}

fn read_only() -> Connection {
    parse_connection(DB_PRESETS[0].connection_string).unwrap()
}

fn dev_login() -> Connection {
    let p = DB_PRESETS.iter().find(|p| p.id == "dev-login").expect("the dev login preset");
    parse_connection(p.connection_string).unwrap()
}

fn exe() -> PathBuf {
    PathBuf::from("sqlcmd.exe")
}

const PROC: &str = "CREATE OR ALTER PROCEDURE dbo.rpt_Leave @From DATE AS SELECT id FROM dbo.Leave WHERE d >= @From";

#[test]
fn the_dev_login_makes_objects_only_with_both_modules_on() {
    let set = |ms: &[Module]| ms.iter().copied().collect::<BTreeSet<Module>>();
    let both = set(&[Module::CrystalReports, Module::DbAccess]);
    assert_eq!(access_with("sgdev01db01_devlogin", &both), Access::DevObjects);
    assert_eq!(access_with("sgdev01db01_devlogin", &set(&[Module::DbAccess])), Access::DevWrites);
    assert_eq!(access_with("sgdev01db01_devlogin", &set(&[Module::CrystalReports])), Access::DevWrites);
    assert_eq!(access_with("sgdev01db01_devlogin", &set(&[])), Access::DevWrites);
    // The modules never make a read-only login more than it is.
    assert_eq!(access_with("sgdev01db02_readonly", &both), Access::ReadOnly);
    assert_eq!(access_with("", &both), Access::ReadOnly);
}

#[tokio::test]
async fn an_object_change_needs_the_writes_switch_and_the_dev_login() {
    let _on = crate::serial::crystal_on();
    let _m = crate::serial::modules();
    let _act = crate::serial::activity_log();
    let fake = FakeRunner::default();
    let (status, said) = run_query(&fake, &exe(), &dev_login(), false, PROC).await.unwrap_err();
    assert_eq!((status, said.as_str()), (400, WRITES_OFF));
    let (status, said) = run_query(&fake, &exe(), &read_only(), true, PROC).await.unwrap_err();
    assert_eq!((status, said.as_str()), (400, READ_ONLY_SENTENCE));
    assert!(fake.calls().is_empty(), "an object change ran through a shut door");

    // Both open (and, in this development-build test binary, Crystal
    // Reports and Database Access on): it runs, from a file.
    run_query(&fake, &exe(), &dev_login(), true, PROC).await.expect("it runs");
    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].iter().any(|a| a == "-i"), "{:?}", calls[0]);
    assert!(!calls[0].iter().any(|a| a == "-Q"), "{:?}", calls[0]);
}

#[test]
fn the_module_sentence_is_what_the_dev_login_hears_without_crystal_reports() {
    // `validate` is the batch's own gate; on the dev login without the
    // module an object statement on its own is refused by the guard with
    // the module sentence - but in a batch it is refused for being in one.
    let batch = [BatchStatement { sql: PROC.to_string(), expect_rows: None }];
    for access in [Access::ReadOnly, Access::DevWrites, Access::DevObjects] {
        let why = validate(&batch, access).unwrap_err();
        assert!(why.contains(OBJECT_ON_ITS_OWN), "{access:?}: {why}");
    }
    assert!(OBJECT_ON_ITS_OWN.contains("send a procedure, view or function on its own"));
    assert_eq!(v2_lib::db::allowed(PROC, Access::DevWrites).unwrap_err(), OBJECTS_NEED_MODULE);
}

#[tokio::test]
async fn a_batch_never_carries_an_object() {
    let _m = crate::serial::modules();
    let _act = crate::serial::activity_log();
    let fake = FakeRunner::default();
    let statements = [
        BatchStatement { sql: "UPDATE dbo.Leave SET d = d WHERE id = 1".to_string(), expect_rows: Some(1) },
        BatchStatement { sql: PROC.to_string(), expect_rows: None },
    ];
    let (status, said) = run_batch_query(&fake, &exe(), &dev_login(), true, &statements, true).await.unwrap_err();
    assert_eq!(status, 400);
    assert!(said.starts_with("statement 2: "), "{said}");
    assert!(said.contains(OBJECT_ON_ITS_OWN), "{said}");
    assert!(fake.calls().is_empty(), "a batch with an object in it reached sqlcmd");
}
