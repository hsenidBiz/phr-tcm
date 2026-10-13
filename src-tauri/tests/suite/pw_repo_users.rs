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
use std::time::{Duration, Instant};
use v2_lib::pw_repo::process::{self, Launch, RunEvent, Sink};
use v2_lib::pw_repo::users::{
    self, change_args, clean_line, list_args, parse_list, AccountChange, AccountField, HIDDEN, LIST_UNREADABLE, NO_REGISTRY,
    NO_SUCH_ACCOUNT, OPTION_LIKE, REGISTRY_NOT_JSON, CONTROL_CHARACTER,
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

    // A control character anywhere is refused: a line break would end
    // cmd.exe's command line there and drop `--apply` without a word.
    for bad in ["a\nb", "a\r", "\0", "a\tb", "x\u{1b}[31m", "\u{7f}"] {
        assert_eq!(change_args(&add(bad), true).unwrap_err(), CONTROL_CHARACTER, "{bad:?}");
        assert_eq!(change_args(&set(AccountField::Username, bad), true).unwrap_err(), CONTROL_CHARACTER, "{bad:?}");
        let rename = AccountChange::Rename { from: "FakeSL.eim.general".into(), to: format!("FakeSL.eim.general{bad}") };
        assert_eq!(change_args(&rename, true).unwrap_err(), CONTROL_CHARACTER, "{bad:?}");
        assert_eq!(change_args(&AccountChange::Remove { key: bad.into() }, true).unwrap_err(), CONTROL_CHARACTER);
    }
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
        // A listing that takes about a second.
        let typed = dir.path().join("list.json");
        fs::write(dir.path().join("slow-list.cmd"), format!("@echo off\r\nping -n 2 127.0.0.1 >nul\r\ntype \"{}\"\r\n", typed.display())).unwrap();
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

    /// A listing that takes about a second and, as the app's listings do,
    /// waits for another short command to free the slot.
    fn slow_waiting(&self, args: &[String], wait: Duration) -> Result<Launch, String> {
        assert_eq!(args, list_args());
        self.asked.lock().unwrap().push(args.to_vec());
        let a = vec![OsString::from("/D"), OsString::from("/C"), self.dir.path().join("slow-list.cmd").into_os_string()];
        Ok(Launch::test_program(cmd(), a, self.dir.path().to_path_buf(), "npm run users").waiting(wait))
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

// ---- npm's own log -----------------------------------------------------------------

#[test]
fn logs_max_zero_never_goes_without_the_app_folder() {
    assert_eq!(process::npm_args(None).unwrap_err(), process::NO_NPM_LOGS_DIR);
    assert_eq!(process::npm_args(Some(std::path::Path::new(""))).unwrap_err(), process::NO_NPM_LOGS_DIR);
    let dir = std::env::temp_dir().join("tcm-tests-pw-npm-logs");
    let args: Vec<String> = process::npm_args(Some(&dir)).unwrap().iter().map(|a| a.to_string_lossy().to_string()).collect();
    assert_eq!(args, ["run".to_string(), "--silent".into(), format!("--logs-dir={}", dir.display()), "--logs-max=0".into()]);
    // Wherever --logs-max=0 is, --logs-dir= with the app's folder is too.
    let max = args.iter().position(|a| a == "--logs-max=0").unwrap();
    let logs = args.iter().position(|a| a.starts_with("--logs-dir=")).unwrap();
    assert!(logs < max && !args[logs].trim_start_matches("--logs-dir=").is_empty());
}

fn wait_done(rx: &Receiver<RunEvent>, run: u32) -> Option<i32> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).expect("the run ended") {
            RunEvent::Done { run: r, code } if r == run => return code,
            _ => {}
        }
    }
}

#[cfg(windows)]
#[test]
fn the_npm_log_folder_is_emptied_after_a_run() {
    let _held_browsers = crate::serial::held_browsers();
    let _slot = crate::serial::pw_repo();
    let logs = tempfile::tempdir().unwrap();
    fs::write(logs.path().join("2026-10-13T05_00_00_000Z-debug-0.log"), format!("verbose argv \"--password\" \"{PASS_A}\"")).unwrap();
    fs::create_dir_all(logs.path().join("nested")).unwrap();
    fs::write(logs.path().join("nested/x.log"), "x").unwrap();
    let launch = Launch::test_program(cmd(), ["/D", "/C", "echo", "ran"].map(OsString::from).to_vec(), logs.path().to_path_buf(), "npm run users")
        .with_npm_logs(logs.path().to_path_buf());
    let (sink, rx) = collect();
    let run = process::start(launch, sink).unwrap();
    assert_eq!(wait_done(&rx, run), Some(0));
    assert!(logs.path().is_dir(), "the folder itself stays");
    assert_eq!(fs::read_dir(logs.path()).unwrap().count(), 0, "nothing is left in it");
}

// ---- Waiting for the slot ----------------------------------------------------------

#[cfg(windows)]
#[test]
fn two_listings_started_together_both_succeed() {
    let _held_browsers = crate::serial::held_browsers();
    let _slot = crate::serial::pw_repo();
    let d = clone();
    let fake = Fake::new("exit /b 0");
    let (show, _rx) = collect();
    let (a, b) = std::thread::scope(|s| {
        let one = s.spawn(|| users::list(d.path(), &show, &|a: &[String]| fake.slow_waiting(a, process::SLOT_WAIT)));
        let two = s.spawn(|| users::list(d.path(), &show, &|a: &[String]| fake.slow_waiting(a, process::SLOT_WAIT)));
        (one.join().unwrap(), two.join().unwrap())
    });
    assert_eq!(a.unwrap().len(), 2);
    assert_eq!(b.unwrap().len(), 2);
    assert_eq!(fake.asked().len(), 2);
    assert_eq!(process::running(), None);
}

#[cfg(windows)]
#[test]
fn a_start_that_does_not_wait_or_meets_a_real_run_is_refused_at_once() {
    let _held_browsers = crate::serial::held_browsers();
    let _slot = crate::serial::pw_repo();
    let d = clone();
    let fake = Fake::new("exit /b 0");
    let (show, _rx) = collect();

    // Behind a short listing, a start that did not ask to wait is refused.
    std::thread::scope(|s| {
        let first = s.spawn(|| users::list(d.path(), &show, &|a: &[String]| fake.slow_waiting(a, process::SLOT_WAIT)));
        let at = Instant::now();
        while process::running().is_none() && at.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        let asked = Instant::now();
        let e = users::list(d.path(), &show, &|a: &[String]| fake.slow_waiting(a, Duration::ZERO)).unwrap_err();
        assert_eq!(e, process::busy_sentence("npm run users"));
        assert!(asked.elapsed() < Duration::from_millis(500));
        assert!(first.join().unwrap().is_ok());
    });

    // Behind a real run (not a short command), even a waiting one is refused at once.
    let ping = ["/D", "/C", "ping", "-n", "30", "127.0.0.1"].map(OsString::from).to_vec();
    let (sink, rx) = collect();
    let run = process::start(Launch::test_program(cmd(), ping, std::env::temp_dir(), "npx playwright test"), sink).unwrap();
    let asked = Instant::now();
    let e = users::list(d.path(), &show, &|a: &[String]| fake.slow_waiting(a, process::SLOT_WAIT)).unwrap_err();
    assert_eq!(e, process::busy_sentence("npx playwright test"));
    assert!(asked.elapsed() < Duration::from_secs(1));
    process::stop(run).unwrap();
    assert_eq!(wait_done(&rx, run), None);
}

// ---- Which passwords are hidden ------------------------------------------------------

#[cfg(windows)]
#[test]
fn short_registry_passwords_show_but_the_changes_own_never_does() {
    let _held_browsers = crate::serial::held_browsers();
    let _slot = crate::serial::pw_repo();
    let d = clone();
    fs::write(
        d.path().join("src/users/users.json"),
        format!(r#"{{ "FakeSL.a.general": {{ "username": "u", "password": "xyz" }}, "FakeSL.b.general": {{ "username": "v", "password": "{PASS_A}" }} }}"#),
    )
    .unwrap();
    assert_eq!(users::MIN_HIDDEN, 4);
    let fake = Fake::new(&format!("echo pq xyz {PASS_A}\r\nexit /b 0"));
    let (show, _rx) = collect();
    let out = users::change(d.path(), &add("pq"), false, &show, &|a: &[String]| fake.launch(a)).unwrap();
    assert_eq!(out.output, [format!("{HIDDEN} xyz {HIDDEN}")]);
}
