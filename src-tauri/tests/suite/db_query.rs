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
use v2_lib::db::{parse_connection, permissions, Access, BatchStatement, Connection, Output, Runner, READ_ONLY_SENTENCE};
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

/// The dev login preset, with a can-write verdict stored for it: writes are
/// decided by the server's answer now, not by the `_devlogin` name.
fn dev_login() -> Connection {
    let p = DB_PRESETS.iter().find(|p| p.id == "dev-login").expect("the dev login preset");
    let c = parse_connection(p.connection_string).unwrap();
    permissions::store(&c, true);
    c
}

fn exe() -> PathBuf {
    PathBuf::from("sqlcmd.exe")
}

const PROC: &str = "CREATE OR ALTER PROCEDURE dbo.rpt_Leave @From DATE AS SELECT id FROM dbo.Leave WHERE d >= @From";

// Was keyed on the `_devlogin` name; the verdict is the server's answer now.
#[test]
fn a_writer_makes_objects_only_with_both_modules_on() {
    let set = |ms: &[Module]| ms.iter().copied().collect::<BTreeSet<Module>>();
    let both = set(&[Module::CrystalReports, Module::DbAccess]);
    let writer = dev_login();
    assert_eq!(access_with(&writer, &both), Access::DevObjects);
    assert_eq!(access_with(&writer, &set(&[Module::DbAccess])), Access::DevWrites);
    assert_eq!(access_with(&writer, &set(&[Module::CrystalReports])), Access::DevWrites);
    assert_eq!(access_with(&writer, &set(&[])), Access::DevWrites);
    // The modules never make a read-only login more than it is - including a
    // devlogin-named one the server has not said can write.
    assert_eq!(access_with(&read_only(), &both), Access::ReadOnly);
    let unchecked = parse_connection("Server=q-nv;Database=d;User Id=x_devlogin;Password=p;").unwrap();
    assert_eq!(access_with(&unchecked, &both), Access::ReadOnly);
}

/// Rows follow the server's verdict alone; object changes also need a
/// `_devlogin` user (trimmed, any case).
#[test]
fn objects_need_the_dev_login_as_well_as_the_verdict() {
    let both: BTreeSet<Module> = [Module::CrystalReports, Module::DbAccess].into_iter().collect();
    // A non-dev login the server lets write: rows yes, objects no.
    let plain = parse_connection("Server=q-plain;Database=d;User Id=app_writer;Password=p;").unwrap();
    permissions::store(&plain, true);
    assert_eq!(access_with(&plain, &both), Access::RowWrites);
    assert!(v2_lib::db::guard::allowed_with("UPDATE dbo.t SET a = 1 WHERE id = 2", Access::RowWrites, true).is_ok());
    // Not the module sentence: no switch would help, the login is the thing.
    let said = v2_lib::db::guard::allowed_with(PROC, access_with(&plain, &both), true).unwrap_err();
    assert_eq!(said, v2_lib::db::guard::OBJECTS_NEED_DEV_LOGIN);
    assert_ne!(said, OBJECTS_NEED_MODULE);
    // The dev login with the modules off still hears the module sentence.
    let dev_off = v2_lib::db::guard::allowed_with(PROC, Access::DevWrites, true).unwrap_err();
    assert_eq!(dev_off, OBJECTS_NEED_MODULE);
    // A _devlogin with a verdict and the modules: objects, whatever the case or padding.
    let dev = parse_connection("Server=q-dev;Database=d;User Id= APP_DevLogin ;Password=p;").unwrap();
    permissions::store(&dev, true);
    assert_eq!(access_with(&dev, &both), Access::DevObjects);
    // A _devlogin with no verdict is read only.
    let unchecked = parse_connection("Server=q-dev-none;Database=d;User Id=x_devlogin;Password=p;").unwrap();
    assert_eq!(access_with(&unchecked, &both), Access::ReadOnly);
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
    assert_eq!(v2_lib::db::allowed_with(PROC, Access::DevWrites, true).unwrap_err(), OBJECTS_NEED_MODULE);
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

// ---- the server's own answer decides who may write -------------------------

use v2_lib::db::permissions::{parse_verdict, CAN_WRITE_REASON, CHECK_FAILED_REASON, READ_ONLY_REASON};

/// A runner that answers every call with the same text, or fails.
struct Answering {
    stdout: String,
    fail: Option<String>,
    calls: Mutex<Vec<Vec<String>>>,
}

impl Answering {
    fn says(stdout: &str) -> Answering {
        Answering { stdout: stdout.into(), fail: None, calls: Mutex::default() }
    }
    fn failing(why: &str) -> Answering {
        Answering { stdout: String::new(), fail: Some(why.into()), calls: Mutex::default() }
    }
}

impl Runner for Answering {
    async fn run(
        &self,
        _exe: &Path,
        args: &[String],
        _env: &[(String, String)],
        _timeout: Duration,
    ) -> Result<Output, String> {
        self.calls.lock().unwrap().push(args.to_vec());
        match &self.fail {
            Some(why) => Err(why.clone()),
            None => Ok(Output { status: 0, stdout: self.stdout.clone(), stderr: String::new() }),
        }
    }
}

/// sqlcmd's answer to the check: a header, its rule, one row, the footer.
fn answer(i: &str, u: &str, d: &str, w: &str, o: &str) -> String {
    answer6(i, u, d, w, o, "0")
}

/// The same, with the deny role's cell given.
fn answer6(i: &str, u: &str, d: &str, w: &str, o: &str, n: &str) -> String {
    format!("i\tu\td\tw\to\tn\n-\t-\t-\t-\t-\t-\n{i}\t{u}\t{d}\t{w}\t{o}\t{n}\n\n(1 rows affected)\n")
}

#[test]
fn the_verdict_parser_reads_permissions_and_roles() {
    // All three permissions.
    assert_eq!(parse_verdict(&answer("1", "1", "1", "0", "0")), Some(true));
    // db_datawriter alone, and db_owner alone.
    assert_eq!(parse_verdict(&answer("0", "0", "0", "1", "0")), Some(true));
    assert_eq!(parse_verdict(&answer("0", "0", "0", "0", "1")), Some(true));
    // Partial permissions are read only.
    assert_eq!(parse_verdict(&answer("1", "1", "0", "0", "0")), Some(false));
    assert_eq!(parse_verdict(&answer("1", "0", "1", "0", "0")), Some(false));
    // NULLs (an unknown role, a missing permission) are read only.
    assert_eq!(parse_verdict(&answer("NULL", "NULL", "NULL", "NULL", "NULL")), Some(false));
    assert_eq!(parse_verdict(&answer("1", "1", "NULL", "NULL", "0")), Some(false));
    // Not an answer at all: no row to read.
    assert_eq!(parse_verdict(""), None);
    assert_eq!(parse_verdict("Msg 229, Level 14, State 5\nThe SELECT permission was denied"), None);
    assert_eq!(parse_verdict(&answer("yes", "1", "1", "0", "0")), None);
    // A five-cell answer (the old question) is no answer: fail closed.
    assert_eq!(parse_verdict("i\tu\td\tw\to\n-\t-\t-\t-\t-\n1\t1\t1\t1\t1\n"), None);
}

#[test]
fn a_member_of_the_deny_role_is_read_only_whatever_else_it_holds() {
    assert_eq!(parse_verdict(&answer6("1", "1", "1", "1", "1", "1")), Some(false));
    assert_eq!(parse_verdict(&answer6("0", "0", "0", "1", "0", "1")), Some(false));
    assert_eq!(parse_verdict(&answer6("1", "1", "1", "0", "0", "0")), Some(true));
    // An unknown deny role (NULL) is not a denial.
    assert_eq!(parse_verdict(&answer6("0", "0", "0", "1", "0", "NULL")), Some(true));
    assert!(permissions::CHECK_SQL.contains("IS_ROLEMEMBER('db_denydatawriter')"));
}

#[test]
fn logins_whose_fields_join_to_the_same_text_have_different_tags() {
    let conn = |user: &str, server: &str, database: &str| Connection {
        server: server.into(),
        database: database.into(),
        user: user.into(),
        password: String::new(),
        trust_cert: true,
    };
    assert_ne!(permissions::tag(&conn("a|s", "x", "d")), permissions::tag(&conn("a", "s|x", "d")));
    // Case and padding still fold.
    assert_eq!(permissions::tag(&conn("A", "S", "D")), permissions::tag(&conn(" a ", "s", " d")));
}

#[tokio::test]
async fn a_check_stores_the_servers_verdict_for_this_login_only() {
    let c = parse_connection("Server=chk-a;Database=d;User Id=anyone;Password=secret-pw;").unwrap();
    let fake = Answering::says(&answer("1", "1", "1", "0", "0"));
    let got = permissions::check(&fake, &exe(), &c).await.unwrap();
    assert!(got.can_write && got.reason == CAN_WRITE_REASON && got.checked_at > 0.0);
    assert_eq!(v2_lib::db::access_for_connection(&c), Access::DevWrites);
    // It sent the one read-only query, as the database's own login.
    let calls = fake.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].iter().any(|a| a == permissions::CHECK_SQL));
    assert!(!permissions::CHECK_SQL.contains("secret"));

    // The server now says read only: the verdict follows it.
    let fake = Answering::says(&answer("1", "0", "0", "0", "0"));
    let got = permissions::check(&fake, &exe(), &c).await.unwrap();
    assert!(!got.can_write && got.reason == READ_ONLY_REASON);
    assert_eq!(v2_lib::db::access_for_connection(&c), Access::ReadOnly);
}

#[tokio::test]
async fn a_failed_check_leaves_no_verdict_and_says_nothing_of_the_server() {
    let c = parse_connection("Server=chk-b;Database=d;User Id=anyone;Password=secret-pw;").unwrap();
    permissions::store(&c, true);
    let fake = Answering::failing("error sending request for url (https://x?secret-pw)");
    let why = permissions::check(&fake, &exe(), &c).await.unwrap_err();
    assert_eq!(why, CHECK_FAILED_REASON);
    // Fail closed: the earlier can-write verdict is gone, not kept.
    assert!(permissions::stored(&c).is_none());
    assert_eq!(v2_lib::db::access_for_connection(&c), Access::ReadOnly);
    // A reply with no answer row is the same.
    permissions::store(&c, true);
    let why = permissions::check(&Answering::says("garbage"), &exe(), &c).await.unwrap_err();
    assert_eq!(why, CHECK_FAILED_REASON);
    assert!(permissions::stored(&c).is_none());
}

#[tokio::test]
async fn a_write_needs_both_the_switch_and_a_can_write_verdict() {
    let _on = crate::serial::crystal_on();
    let _m = crate::serial::modules();
    let _act = crate::serial::activity_log();
    let sql = "UPDATE dbo.t SET a = 1 WHERE id = 1";
    let fake = FakeRunner::default();

    // A devlogin-named login with no verdict: refused even with the switch on.
    let unchecked = parse_connection("Server=enf-a;Database=d;User Id=x_devlogin;Password=p;").unwrap();
    let (status, said) = run_query(&fake, &exe(), &unchecked, true, sql).await.unwrap_err();
    assert_eq!((status, said.as_str()), (400, READ_ONLY_SENTENCE));
    // A read-only verdict: refused.
    let reader = parse_connection("Server=enf-b;Database=d;User Id=u;Password=p;").unwrap();
    permissions::store(&reader, false);
    let (status, said) = run_query(&fake, &exe(), &reader, true, sql).await.unwrap_err();
    assert_eq!((status, said.as_str()), (400, READ_ONLY_SENTENCE));
    // A can-write verdict with the switch off: refused.
    let writer = parse_connection("Server=enf-c;Database=d;User Id=u;Password=p;").unwrap();
    permissions::store(&writer, true);
    let (status, said) = run_query(&fake, &exe(), &writer, false, sql).await.unwrap_err();
    assert_eq!((status, said.as_str()), (400, WRITES_OFF));
    assert!(fake.calls().is_empty(), "a write ran through a shut door");
    // Both open: it runs.
    run_query(&fake, &exe(), &writer, true, sql).await.expect("both doors open");
    assert_eq!(fake.calls().len(), 1);
    // The guard under it (run_sql) refuses the same writes with no help from
    // run_query, so there is no way round it.
    assert!(v2_lib::db::run_sql(&fake, &exe(), &unchecked, sql).await.is_err());
    assert_eq!(fake.calls().len(), 1);
}

#[test]
fn the_switch_does_not_stand_in_for_the_verdict_in_a_batch() {
    // `run_batch` validates with the connection's own access, so a login
    // with no verdict cannot write through a batch either.
    let unchecked = parse_connection("Server=enf-d;Database=d;User Id=x_devlogin;Password=p;").unwrap();
    let batch = [BatchStatement { sql: "UPDATE dbo.t SET a = 1".into(), expect_rows: Some(1) }];
    let fake = FakeRunner::default();
    let r = futures_lite_block(v2_lib::db::run_batch(&fake, &exe(), &unchecked, &batch, true));
    assert!(r.is_err());
    assert!(fake.calls().is_empty());
}

fn futures_lite_block<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(f)
}
