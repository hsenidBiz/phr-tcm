//! The Playwright tab's test accounts (`pw_repo::users`): the arguments for
//! each change, parsing `list --json` without its passwords, the dry run
//! without `apply`, a refusal passed through, and a password kept out of
//! the log, the events and every answer but Show.
//!
//! The repo's CLI never runs here. The tests that run a command get a fake
//! starter: cmd.exe typing a saved listing for `list`, and a script of the
//! test's own for a change. They hold `serial::held_browsers` then
//! `serial::pw_repo`: there is one run slot for the whole process. Every
//! value is made up.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use v2_lib::pw_repo::process::{self, Launch, RunEvent, Sink};
use v2_lib::pw_repo::users::{
    self, change_args, clean_line, list_args, parse_list, AccountChange, AccountField, HIDDEN, LIST_UNREADABLE, NO_REGISTRY,
    NO_SUCH_ACCOUNT, OPTION_LIKE, REGISTRY_NOT_JSON,
};

const PASS_A: &str = "Made-Up-Pass-4417";
const PASS_B: &str = "Other-Fake-Pass-9023";
const NEW_PASS: &str = "Brand-New-Fake-6650";

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn add(password: &str) -> AccountChange {
    AccountChange::Add {
        key: "FakeSL.payroll.general".into(),
        username: "fake.payroll@example.test".into(),
        password: password.into(),
        employee_no: "".into(),
        display_name: "Fake Payroll".into(),
        title: "".into(),
        supervisor: "".into(),
    }
}

// ---- Arguments --------------------------------------------------------------------

#[test]
fn the_arguments_for_each_change() {
    assert_eq!(list_args(), strings(&["run", "users", "--", "list", "--json"]));

    assert_eq!(
        change_args(&add("p1"), false).unwrap(),
        strings(&[
            "run", "users", "--", "add", "FakeSL.payroll.general", "--username", "fake.payroll@example.test", "--password", "p1",
            "--display-name", "Fake Payroll",
        ])
    );
    let full = AccountChange::Add {
        key: " FakeSL.eim.general ".into(),
        username: "u".into(),
        password: "p".into(),
        employee_no: "000074".into(),
        display_name: "D".into(),
        title: "HR Manager".into(),
        supervisor: "FakeSL.eim.supervisor".into(),
    };
    assert_eq!(
        change_args(&full, true).unwrap(),
        strings(&[
            "run", "users", "--", "add", "FakeSL.eim.general", "--username", "u", "--password", "p", "--employee-no", "000074",
            "--display-name", "D", "--emp-title", "HR Manager", "--supervisor", "FakeSL.eim.supervisor", "--apply",
        ])
    );

    let set = |field, value: &str| AccountChange::Set { key: "FakeSL.eim.general".into(), field, value: value.into() };
    assert_eq!(
        change_args(&set(AccountField::Password, "x y"), false).unwrap(),
        strings(&["run", "users", "--", "set", "FakeSL.eim.general", "password", "x y"])
    );
    assert_eq!(
        change_args(&set(AccountField::Title, "Lead"), true).unwrap(),
        strings(&["run", "users", "--", "set", "FakeSL.eim.general", "empTitle", "Lead", "--apply"])
    );
    // Empty clears an optional field, the CLI's `-`.
    assert_eq!(
        change_args(&set(AccountField::Supervisor, " "), false).unwrap(),
        strings(&["run", "users", "--", "set", "FakeSL.eim.general", "supervisor", "-"])
    );
    for (field, name) in [
        (AccountField::Username, "username"),
        (AccountField::EmployeeNo, "employeeNo"),
        (AccountField::DisplayName, "displayName"),
    ] {
        assert_eq!(change_args(&set(field, "v"), false).unwrap()[5], name);
    }

    assert_eq!(
        change_args(&AccountChange::Remove { key: "FakeSL.eim.general".into() }, true).unwrap(),
        strings(&["run", "users", "--", "rm", "FakeSL.eim.general", "--apply"])
    );
    assert_eq!(
        change_args(&AccountChange::Rename { from: "FakeSL.eim.general".into(), to: "FakeSL.eim.general.a".into() }, false).unwrap(),
        strings(&["run", "users", "--", "rename", "FakeSL.eim.general", "FakeSL.eim.general.a", "--cascade"])
    );

    // A value the CLI would read as an option is refused, never sent.
    assert_eq!(change_args(&add("--apply"), false).unwrap_err(), OPTION_LIKE);
    assert_eq!(change_args(&set(AccountField::DisplayName, "--x"), false).unwrap_err(), OPTION_LIKE);
}

// ---- Parsing --------------------------------------------------------------------------

fn listing() -> String {
    format!(
        r#"[
  {{ "key": "FakeSL.performance.general", "username": "fake.general@example.test", "password": "{PASS_A}",
     "employeeNo": "", "displayName": "Fake General", "supervisor": "FakeSL.performance.supervisor", "specs": 3, "otherRefs": 1 }},
  {{ "key": "FakeSL.performance.supervisor", "username": "fake.supervisor@example.test", "password": "{PASS_B}",
     "employeeNo": "E0001", "displayName": "", "supervisor": "", "specs": 0, "otherRefs": 2 }}
]"#
    )
}

fn registry() -> String {
    format!(
        r#"{{
  "FakeSL.performance.general": {{ "username": "fake.general@example.test", "password": "{PASS_A}", "displayName": "Fake General", "empTitle": "Analyst", "supervisor": "FakeSL.performance.supervisor" }},
  "FakeSL.performance.supervisor": {{ "username": "fake.supervisor@example.test", "password": "{PASS_B}", "employeeNo": "E0001" }}
}}
"#
    )
}

#[test]
fn the_list_json_parses_without_its_passwords() {
    let titles = HashMap::from([("FakeSL.performance.general".to_string(), "Analyst".to_string())]);
    let accounts = parse_list(&format!("a stray line first\n{}", listing()), &titles).unwrap();
    assert_eq!(accounts.len(), 2);
    let a = &accounts[0];
    assert_eq!(a.key, "FakeSL.performance.general");
    assert_eq!(a.username, "fake.general@example.test");
    assert_eq!(a.display_name, "Fake General");
    assert_eq!(a.title, "Analyst");
    assert_eq!(a.supervisor, "FakeSL.performance.supervisor");
    assert_eq!((a.specs, a.other_refs), (3, 1));
    assert_eq!(accounts[1].employee_no, "E0001");
    assert_eq!(accounts[1].title, "");

    // Neither the value nor what the webview is sent holds a password.
    let printed = format!("{accounts:?}");
    let sent = serde_json::to_string(&accounts).unwrap();
    for text in [&printed, &sent] {
        assert!(!text.contains(PASS_A) && !text.contains(PASS_B), "{text}");
        assert!(!text.to_lowercase().contains("password"), "{text}");
    }

    // An unreadable listing never quotes itself.
    let broken = format!("[{{ \"key\": 1, \"password\": \"{PASS_A}\" }}]");
    assert_eq!(parse_list(&broken, &titles).unwrap_err(), LIST_UNREADABLE);
    assert_eq!(parse_list("nothing here", &titles).unwrap_err(), LIST_UNREADABLE);
}

#[test]
fn output_lines_hide_passwords_and_json_errors() {
    let secrets = vec!["abc".to_string(), "abcdef".to_string()];
    assert_eq!(clean_line("set abcdef and abc", &secrets), format!("set {HIDDEN} and {HIDDEN}"));
    assert_eq!(clean_line("nothing secret", &secrets), "nothing secret");
    // Node quotes the text it could not parse.
    let quoted = format!(r#"Unexpected token '}}', "{{"a": "{PASS_A}" "... is not valid JSON"#);
    assert_eq!(clean_line(&quoted, &[]), REGISTRY_NOT_JSON);
    assert_eq!(clean_line("Unexpected end of JSON input in JSON at position 12", &[]), REGISTRY_NOT_JSON);
}

#[test]
fn show_reads_one_password_from_the_registry() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(users::password(d.path(), "FakeSL.performance.general").unwrap_err(), NO_REGISTRY);
    fs::create_dir_all(d.path().join("src/users")).unwrap();
    fs::write(d.path().join("src/users/users.json"), registry()).unwrap();
    let before = fs::read(d.path().join("src/users/users.json")).unwrap();
    assert_eq!(users::password(d.path(), "FakeSL.performance.general").unwrap(), PASS_A);
    assert_eq!(users::password(d.path(), " FakeSL.performance.supervisor ").unwrap(), PASS_B);
    assert_eq!(users::password(d.path(), "FakeSL.nope.general").unwrap_err(), NO_SUCH_ACCOUNT);
    assert_eq!(fs::read(d.path().join("src/users/users.json")).unwrap(), before, "read only");

    fs::write(d.path().join("src/users/users.json"), format!("{{ \"k\": {{ \"password\": \"{PASS_A}\" ")).unwrap();
    let e = users::password(d.path(), "k").unwrap_err();
    assert_eq!(e, REGISTRY_NOT_JSON);
}

#[test]
fn the_repo_start_command_refuses_npm() {
    let (sink, _rx) = collect();
    let e = process::start_repo(true, "C:\\nowhere", "npm", &strings(&["run", "users", "--", "list", "--json"]), sink).unwrap_err();
    assert_eq!(e, process::ACCOUNTS_ONLY);
}

// ---- Running, through a fake starter ----------------------------------------------

fn collect() -> (Sink, Receiver<RunEvent>) {
    let (tx, rx) = channel();
    let tx = Mutex::new(tx);
    let sink: Sink = Arc::new(move |e| {
        let _ = tx.lock().unwrap().send(e);
    });
    (sink, rx)
}

fn cmd() -> PathBuf {
    std::env::var_os("ComSpec").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows\System32\cmd.exe"))
}

/// A clone with the made-up registry.
fn clone() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    fs::create_dir_all(d.path().join("src/users")).unwrap();
    fs::write(d.path().join("src/users/users.json"), registry()).unwrap();
    d
}

/// A starter that records every argv, types the listing for `list`, and
/// runs `change.cmd` (the test's own script) for anything else.
struct Fake {
    dir: tempfile::TempDir,
    asked: Mutex<Vec<Vec<String>>>,
}

impl Fake {
    fn new(change_script: &str) -> Fake {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("list.json"), listing()).unwrap();
        fs::write(dir.path().join("change.cmd"), format!("@echo off\r\n{change_script}\r\n")).unwrap();
        Fake { dir, asked: Mutex::new(vec![]) }
    }

    fn launch(&self, args: &[String]) -> Result<Launch, String> {
        assert_eq!(&args[..3], ["run", "users", "--"], "only npm run users is run");
        self.asked.lock().unwrap().push(args.to_vec());
        let file = |name: &str| self.dir.path().join(name).into_os_string();
        let a = if args == list_args() {
            vec![OsString::from("/D"), OsString::from("/C"), OsString::from("type"), file("list.json")]
        } else {
            vec![OsString::from("/D"), OsString::from("/C"), file("change.cmd")]
        };
        Ok(Launch::test_program(cmd(), a, self.dir.path().to_path_buf(), "npm run users"))
    }

    fn asked(&self) -> Vec<Vec<String>> {
        self.asked.lock().unwrap().clone()
    }
}

fn drain(rx: &Receiver<RunEvent>) -> Vec<RunEvent> {
    // `Done` is sent before a command returns, so everything is here.
    rx.try_iter().collect()
}

fn holds_a_password(text: &str) -> bool {
    [PASS_A, PASS_B, NEW_PASS].iter().any(|p| text.contains(p))
}

fn event_text(e: &RunEvent) -> String {
    match e {
        RunEvent::Output { line, .. } => line.clone(),
        RunEvent::Done { .. } => String::new(),
    }
}

#[cfg(windows)]
#[test]
fn a_password_is_never_logged_or_sent_unasked() {
    let _held_browsers = crate::serial::held_browsers();
    let _slot = crate::serial::pw_repo();
    let _log = crate::serial::log_tail();
    let d = clone();
    // A CLI that, unlike the real one, echoes the password it was given and
    // every password in the registry, on both streams.
    let fake = Fake::new(&format!(
        "echo add FakeSL.payroll.general --password {NEW_PASS}\r\necho also {PASS_A} and {PASS_B}\r\n1>&2 echo stderr {NEW_PASS}\r\nexit /b 0"
    ));
    let (show, rx) = collect();
    let launch = |a: &[String]| fake.launch(a);

    // The list: no password in the accounts, and no line of its output
    // (the JSON with every password) reaches the sink.
    let accounts = users::list(d.path(), &show, &launch).unwrap();
    assert_eq!(accounts.len(), 2);
    assert!(!holds_a_password(&serde_json::to_string(&accounts).unwrap()));
    assert_eq!(accounts[0].title, "Analyst", "the title comes from the registry");
    let events = drain(&rx);
    assert!(events.iter().all(|e| matches!(e, RunEvent::Done { code: Some(0), .. })), "{events:?}");
    assert_eq!(events.len(), 1);

    // A change that carries a password: hidden in what comes back.
    let preview = users::change(d.path(), &add(NEW_PASS), false, &show, &launch).unwrap();
    assert!(!preview.applied);
    assert!(preview.output.iter().all(|l| !holds_a_password(l)), "{:?}", preview.output);
    assert!(preview.output.contains(&format!("add FakeSL.payroll.general --password {HIDDEN}")), "{:?}", preview.output);
    assert!(preview.output.contains(&format!("also {HIDDEN} and {HIDDEN}")), "{:?}", preview.output);
    assert!(preview.output.contains(&format!("stderr {HIDDEN}")), "{:?}", preview.output);

    // A password `set`, applied: the output and the listing after it.
    let set = AccountChange::Set { key: "FakeSL.performance.general".into(), field: AccountField::Password, value: format!(" {NEW_PASS} ") };
    let applied = users::change(d.path(), &set, true, &show, &launch).unwrap();
    assert!(applied.applied);
    assert!(!holds_a_password(&serde_json::to_string(&applied).unwrap()), "{applied:?}");

    // Nothing sent holds a password: every event of every command.
    let events = drain(&rx);
    assert!(events.iter().all(|e| matches!(e, RunEvent::Done { .. })), "only ends are sent: {events:?}");
    assert!(events.iter().all(|e| !holds_a_password(&event_text(e))));

    // Nothing logged does either, though every command was logged.
    let log: Vec<String> = v2_lib::applog::recent(500).into_iter().map(|l| l.message).collect();
    assert!(log.iter().any(|m| m.contains("started: npm run users")), "{log:?}");
    assert!(log.iter().all(|m| !holds_a_password(m)), "{log:?}");

    // Show, asked, is the one way a password comes back.
    assert_eq!(users::password(d.path(), "FakeSL.performance.general").unwrap(), PASS_A);
    assert_eq!(process::running(), None);
}

#[cfg(windows)]
#[test]
fn a_change_without_apply_is_a_dry_run() {
    let _held_browsers = crate::serial::held_browsers();
    let _slot = crate::serial::pw_repo();
    let d = clone();
    let fake = Fake::new("echo rename FakeSL.performance.general -^> FakeSL.performance.general.a\r\necho   Nothing written. Re-run with --apply to write it.\r\nexit /b 0");
    let (show, _rx) = collect();
    let launch = |a: &[String]| fake.launch(a);
    let rename = AccountChange::Rename { from: "FakeSL.performance.general".into(), to: "FakeSL.performance.general.a".into() };

    let preview = users::change(d.path(), &rename, false, &show, &launch).unwrap();
    assert!(!preview.applied);
    assert_eq!(preview.accounts, None);
    assert_eq!(
        preview.output,
        ["rename FakeSL.performance.general -> FakeSL.performance.general.a", "  Nothing written. Re-run with --apply to write it."]
    );
    // One command, without --apply, and nothing listed after it.
    let asked = fake.asked();
    assert_eq!(asked.len(), 1);
    assert!(!asked[0].iter().any(|a| a == "--apply"));
    assert!(asked[0].iter().any(|a| a == "--cascade"));

    // Applied: the change with --apply, then the list again.
    let applied = users::change(d.path(), &rename, true, &show, &launch).unwrap();
    assert!(applied.applied);
    assert_eq!(applied.accounts.as_ref().map(Vec::len), Some(2));
    assert_eq!(applied.list_error, None);
    let asked = fake.asked();
    assert_eq!(asked.len(), 3);
    assert_eq!(asked[1].last().map(String::as_str), Some("--apply"));
    assert_eq!(asked[2], list_args());
}

#[cfg(windows)]
#[test]
fn a_refusal_is_passed_through() {
    let _held_browsers = crate::serial::held_browsers();
    let _slot = crate::serial::pw_repo();
    let d = clone();
    let fake = Fake::new(&format!(
        "1>&2 echo \"FakeSL.performance.general\" is still the supervisor of FakeSL.x.general. Repoint or remove those first.\r\n1>&2 echo {PASS_A}\r\nexit /b 1"
    ));
    let (show, _rx) = collect();
    let launch = |a: &[String]| fake.launch(a);

    let e = users::change(d.path(), &AccountChange::Remove { key: "FakeSL.performance.general".into() }, true, &show, &launch).unwrap_err();
    assert_eq!(
        e,
        format!("\"FakeSL.performance.general\" is still the supervisor of FakeSL.x.general. Repoint or remove those first.\n{HIDDEN}")
    );
    // Refused: nothing listed after it.
    assert_eq!(fake.asked().len(), 1);

    // A failed listing says what its stderr said, and nothing of stdout.
    let fake = Fake::new("exit /b 0");
    fs::remove_file(fake.dir.path().join("list.json")).unwrap();
    let e = users::list(d.path(), &show, &|a: &[String]| fake.launch(a)).unwrap_err();
    assert!(!e.is_empty() && !holds_a_password(&e), "{e}");

    // A listing that exits 0 but is not the JSON is unreadable, unquoted.
    let fake = Fake::new("exit /b 0");
    fs::write(fake.dir.path().join("list.json"), format!("not json {PASS_A}")).unwrap();
    let e = users::list(d.path(), &show, &|a: &[String]| fake.launch(a)).unwrap_err();
    assert_eq!(e, LIST_UNREADABLE);
}

#[test]
fn a_registry_that_is_missing_or_broken_hides_nothing_it_should_not() {
    let d = tempfile::tempdir().unwrap();
    assert!(users::registry_passwords(d.path()).is_empty());
    assert!(users::registry_titles(d.path()).is_empty());
    let d = clone();
    let mut found = users::registry_passwords(d.path());
    found.sort();
    assert_eq!(found, [PASS_A, PASS_B]);
}
