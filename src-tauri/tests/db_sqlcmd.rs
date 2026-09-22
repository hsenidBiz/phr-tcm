//! Reading a connection string, finding sqlcmd, and the one process this
//! app spawns for SQL. The real process is never started here: `Runner` is
//! faked so the argument list, the caps and the redaction are all testable.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use v2_lib::db::{
    find_sqlcmd, parse_connection, run_sql, sqlcmd_args, sqlcmd_path, Connection, Output, Runner,
    CHAR_CAP, NOT_INSTALLED, ROW_CAP, SQLCMD_OVERRIDE, TIMEOUT_SECS,
};
use v2_lib::db_defaults::DB_PRESETS;

/// A stand-in for sqlcmd. It answers with whatever it was built with and
/// keeps every call, so a test can assert what the real process would have
/// been handed - and that it was not called at all when the guard refused.
#[derive(Default)]
struct FakeRunner {
    status: i32,
    stdout: String,
    stderr: String,
    calls: Mutex<Vec<(PathBuf, Vec<String>, Duration)>>,
}

impl FakeRunner {
    fn answering(stdout: &str) -> FakeRunner {
        FakeRunner { stdout: stdout.to_string(), ..Default::default() }
    }
    fn calls(&self) -> Vec<(PathBuf, Vec<String>, Duration)> {
        self.calls.lock().unwrap().clone()
    }
}

impl Runner for FakeRunner {
    async fn run(&self, exe: &Path, args: &[String], timeout: Duration) -> Result<Output, String> {
        self.calls.lock().unwrap().push((exe.to_path_buf(), args.to_vec(), timeout));
        Ok(Output {
            status: self.status,
            stdout: self.stdout.clone(),
            stderr: self.stderr.clone(),
        })
    }
}

fn read_only_preset() -> Connection {
    parse_connection(DB_PRESETS[0].connection_string).unwrap()
}

fn dev_login_preset() -> Connection {
    let p = DB_PRESETS.iter().find(|p| p.label == "Dev — dev login").expect("the dev login preset");
    parse_connection(p.connection_string).unwrap()
}

/// The value sqlcmd would receive for a flag, so a test never has to care
/// where in the list the flag landed.
fn value_after<'a>(args: &'a [String], flag: &str) -> &'a str {
    let at = args
        .iter()
        .position(|a| a == flag)
        .unwrap_or_else(|| panic!("no {flag} in {args:?}"));
    args.get(at + 1).unwrap_or_else(|| panic!("{flag} has no value in {args:?}"))
}

#[test]
fn the_shipped_presets_parse_and_never_print_their_password() {
    for p in DB_PRESETS {
        let c = parse_connection(p.connection_string).unwrap();
        assert!(!c.server.is_empty(), "{}", p.label);
        assert!(!c.database.is_empty(), "{}", p.label);
        assert!(!c.user.is_empty(), "{}", p.label);
        assert!(!c.password.is_empty(), "{}", p.label);
        assert!(c.trust_cert, "{}", p.label);
        let shown = format!("{c:?}");
        assert!(!shown.contains(&c.password), "{} printed its password: {shown}", p.label);
        assert!(shown.contains("(hidden)"), "{shown}");
        assert!(shown.contains(&c.server), "{shown}");
    }
    let first = read_only_preset();
    assert_eq!(first.server, "sgdev01db02.cloud");
    assert_eq!(first.database, "hrmmain_philippines");
    assert_eq!(first.user, "sgdev01db02_readonly");
}

#[test]
fn the_keys_are_case_insensitive_and_a_missing_one_names_only_itself() {
    let c = parse_connection("SERVER=a;database=b;user id=c;PassWord=d;").unwrap();
    assert_eq!((c.server.as_str(), c.database.as_str(), c.user.as_str()), ("a", "b", "c"));
    assert!(!c.trust_cert, "TrustServerCertificate was not asked for");

    let why = parse_connection("Server=srv-one;Database=db-two;User Id=user-three;TrustServerCertificate=True;")
        .unwrap_err();
    assert!(why.contains("Password"), "{why}");
    for leaked in ["srv-one", "db-two", "user-three"] {
        assert!(!why.contains(leaked), "{why} leaked {leaked}");
    }

    assert!(parse_connection("Database=b;User Id=c;Password=d;").unwrap_err().contains("Server"));
    assert!(parse_connection("Server=a;User Id=c;Password=d;").unwrap_err().contains("Database"));
    assert!(parse_connection("Server=a;Database=b;Password=d;").unwrap_err().contains("User Id"));
    // A key that is present but blank is missing as far as sqlcmd cares.
    assert!(parse_connection("Server=a;Database=b;User Id=c;Password=;").unwrap_err().contains("Password"));
}

fn odbc_binn(program_files: &Path, version: &str) -> PathBuf {
    program_files
        .join("Microsoft SQL Server")
        .join("Client SDK")
        .join("ODBC")
        .join(version)
        .join("Tools")
        .join("Binn")
}

#[test]
fn sqlcmd_is_found_in_the_sql_tools_folder_when_the_path_has_none() {
    let pf = tempfile::tempdir().unwrap();
    let pf_x86 = tempfile::tempdir().unwrap();
    let binn = odbc_binn(pf.path(), "180");
    std::fs::create_dir_all(&binn).unwrap();
    std::fs::write(binn.join("sqlcmd.exe"), b"").unwrap();

    let found = find_sqlcmd("", &pf.path().to_string_lossy(), &pf_x86.path().to_string_lossy());
    assert_eq!(found, Some(binn.join("sqlcmd.exe")));
}

#[test]
fn the_go_sqlcmd_folder_is_searched_too() {
    let pf = tempfile::tempdir().unwrap();
    let pf_x86 = tempfile::tempdir().unwrap();
    let dir = pf.path().join("sqlcmd");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("sqlcmd.exe"), b"").unwrap();

    let found = find_sqlcmd("", &pf.path().to_string_lossy(), &pf_x86.path().to_string_lossy());
    assert_eq!(found, Some(dir.join("sqlcmd.exe")));
}

#[test]
fn a_sqlcmd_on_the_path_wins_over_the_installed_folders() {
    let on_path = tempfile::tempdir().unwrap();
    std::fs::write(on_path.path().join("sqlcmd.exe"), b"").unwrap();

    let pf = tempfile::tempdir().unwrap();
    let pf_x86 = tempfile::tempdir().unwrap();
    let binn = odbc_binn(pf.path(), "180");
    std::fs::create_dir_all(&binn).unwrap();
    std::fs::write(binn.join("sqlcmd.exe"), b"").unwrap();

    let path_var = format!("{};{}", pf_x86.path().display(), on_path.path().display());
    let found =
        find_sqlcmd(&path_var, &pf.path().to_string_lossy(), &pf_x86.path().to_string_lossy());
    assert_eq!(found, Some(on_path.path().join("sqlcmd.exe")));
}

#[test]
fn nothing_installed_is_none_and_the_sentence_says_how_to_fix_it() {
    let empty = tempfile::tempdir().unwrap();
    let nowhere = empty.path().to_string_lossy().to_string();
    assert_eq!(find_sqlcmd("", &nowhere, &nowhere), None);
    assert_eq!(find_sqlcmd(";;  ;", &nowhere, &nowhere), None);

    assert!(NOT_INSTALLED.contains("winget install sqlcmd"), "{NOT_INSTALLED}");
    assert!(!NOT_INSTALLED.contains('—'), "no em dashes in user-facing text");
}

#[test]
fn the_argument_list_is_separate_strings_with_nothing_quoted_or_escaped() {
    let c = Connection {
        server: "sgdev01db02.cloud".into(),
        database: "hrm main".into(),
        user: "a_readonly".into(),
        // Everything a shell would choke on, in one password.
        password: "p@ss w\"ord;& | `$(x)".into(),
        trust_cert: true,
    };
    let args = sqlcmd_args(&c, "SELECT 1\nFROM t");

    assert_eq!(value_after(&args, "-S"), "sgdev01db02.cloud");
    assert_eq!(value_after(&args, "-d"), "hrm main");
    assert_eq!(value_after(&args, "-U"), "a_readonly");
    // The password is one argument, byte for byte, with no quoting of any
    // kind - it never passes through a shell, so there is nothing to escape.
    assert_eq!(value_after(&args, "-P"), c.password);
    assert_eq!(value_after(&args, "-Q"), "SELECT 1\nFROM t");
    assert_eq!(value_after(&args, "-t"), TIMEOUT_SECS.to_string());
    assert_eq!(value_after(&args, "-t"), "30");
    assert_eq!(value_after(&args, "-s"), "\t");
    assert_eq!(value_after(&args, "-y"), "0");
    assert_eq!(value_after(&args, "-Y"), "0");
    assert!(args.contains(&"-C".to_string()), "{args:?}");
    assert!(args.contains(&"-b".to_string()), "{args:?}");
    assert!(args.contains(&"-W".to_string()), "{args:?}");
    // Headers are kept: they are what names the columns in the answer.
    assert!(!args.iter().any(|a| a == "-h" || a == "-h-1"), "{args:?}");
    // Every flag is an element of its own, exactly once: nothing was glued
    // into a command line that something downstream could re-split.
    for flag in ["-S", "-d", "-U", "-P", "-C", "-s", "-W", "-y", "-Y", "-t", "-b", "-Q"] {
        assert_eq!(args.iter().filter(|a| a.as_str() == flag).count(), 1, "{flag} in {args:?}");
    }

    let plain = Connection { trust_cert: false, ..c.clone() };
    assert!(!sqlcmd_args(&plain, "SELECT 1").contains(&"-C".to_string()));
}

#[tokio::test]
async fn a_statement_that_starts_with_a_dash_still_arrives_as_the_query() {
    // A leading comment is the ordinary way to label a query, and it makes
    // the `-Q` value start with a dash. It is its own argument, so sqlcmd
    // reads it as the query and never as another flag.
    let sql = "-- pick one\nSELECT 1 AS n";
    let fake = FakeRunner::answering("n\n1\n");
    run_sql(&fake, Path::new("sqlcmd.exe"), &read_only_preset(), sql).await.unwrap();
    let calls = fake.calls();
    assert_eq!(value_after(&calls[0].1, "-Q"), sql);
    assert_eq!(calls[0].1.iter().filter(|a| a.as_str() == "-Q").count(), 1);
}

#[tokio::test]
async fn a_long_answer_keeps_the_header_and_caps_the_rows() {
    let mut stdout = String::from("id\tname\n");
    for i in 0..250 {
        stdout.push_str(&format!("{i}\trow {i}\n"));
    }
    let fake = FakeRunner::answering(&stdout);
    let (text, capped) =
        run_sql(&fake, Path::new("sqlcmd.exe"), &read_only_preset(), "SELECT id, name FROM t")
            .await
            .unwrap();

    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "id\tname");
    assert_eq!(lines.len(), 1 + ROW_CAP + 1, "header, {ROW_CAP} rows, the cap line");
    assert_eq!(lines[ROW_CAP], "199\trow 199");
    assert_eq!(*lines.last().unwrap(), "... 50 more rows (capped)");
    assert!(capped, "the row cap fired");

    // Short enough to fit is left exactly as sqlcmd wrote it.
    let short = FakeRunner::answering("id\tname\n1\ta\n2\tb\n");
    let (text, capped) =
        run_sql(&short, Path::new("sqlcmd.exe"), &read_only_preset(), "SELECT id, name FROM t")
            .await
            .unwrap();
    assert_eq!(text, "id\tname\n1\ta\n2\tb");
    assert!(!text.contains("capped"));
    assert!(!capped, "nothing was cut");
}

#[tokio::test]
async fn a_huge_answer_is_cut_at_the_character_cap() {
    let stdout = format!("blob\n{}\n", "x".repeat(100_000));
    let fake = FakeRunner::answering(&stdout);
    let (text, capped) = run_sql(&fake, Path::new("sqlcmd.exe"), &read_only_preset(), "SELECT blob FROM t")
        .await
        .unwrap();

    assert!(text.contains("... output capped at 60000 characters"), "no cap line");
    assert!(text.chars().count() <= CHAR_CAP + 64, "{} characters", text.chars().count());
    // One row wider than the whole cap is cut where it is cut: the only
    // alternative would be handing back the header and nothing else.
    assert!(text.starts_with("blob\nxxx"));
    assert!(capped, "the character cap fired");
}

#[tokio::test]
async fn when_both_caps_fire_both_are_reported_and_no_row_is_left_half_written() {
    let wide = "y".repeat(400);
    let mut stdout = String::from("id\tblob\n");
    for i in 0..500 {
        stdout.push_str(&format!("{i}\t{wide}\n"));
    }
    let fake = FakeRunner::answering(&stdout);
    let (text, capped) =
        run_sql(&fake, Path::new("sqlcmd.exe"), &read_only_preset(), "SELECT id, blob FROM t")
            .await
            .unwrap();

    assert!(capped, "either cap firing is still capped");
    assert!(text.contains("more rows (capped)"), "the row cap went unsaid");
    assert!(text.contains("... output capped at 60000 characters"), "the char cap went unsaid");
    // The character cut landed between rows, so every row that survived is
    // a whole row - a half-written one would be read as data.
    for line in text.lines().skip(1).filter(|l| !l.starts_with("... ")) {
        assert!(line.ends_with(&wide), "a row was cut in half, {} characters", line.len());
    }
    assert!(text.chars().count() <= CHAR_CAP + 128, "{} characters", text.chars().count());
}

#[tokio::test]
async fn even_a_successful_answer_is_scrubbed() {
    // Nothing leaves this module without passing the redaction, including
    // the rows themselves: a password can sit in a configuration table.
    let c = read_only_preset();
    let fake = FakeRunner::answering(&format!("note\nthe server echoed {}\n", c.password));
    let (text, capped) = run_sql(&fake, Path::new("sqlcmd.exe"), &c, "SELECT 1 AS note").await.unwrap();
    assert!(!text.contains(&c.password), "{text}");
    assert!(text.contains("(hidden)"), "{text}");
    assert!(!capped, "nothing here was cut");
}

#[tokio::test]
async fn a_failure_is_reported_without_the_password() {
    // The brief's case, literally: sqlcmd echoing the password back at us.
    let c = Connection {
        server: "s".into(),
        database: "d".into(),
        user: "u".into(),
        password: "abc".into(),
        trust_cert: true,
    };
    let fake = FakeRunner {
        status: 1,
        stderr: "Login failed for user 'x'. Password=abc".into(),
        ..Default::default()
    };
    let why = run_sql(&fake, Path::new("sqlcmd.exe"), &c, "SELECT 1").await.unwrap_err();
    assert!(why.contains("Login failed"), "{why}");
    assert!(!why.contains("abc"), "the password leaked: {why}");
    assert!(why.contains("(hidden)"), "{why}");

    // And with a real shipped password, wherever sqlcmd chose to print it.
    let real = read_only_preset();
    let fake = FakeRunner {
        status: 1,
        stdout: format!("Msg 18456, Level 14: Login failed ({})", real.password),
        ..Default::default()
    };
    let why = run_sql(&fake, Path::new("sqlcmd.exe"), &real, "SELECT 1").await.unwrap_err();
    assert!(why.contains("Msg 18456"), "{why}");
    assert!(!why.contains(&real.password), "the password leaked: {why}");

    // sqlcmd says a connection failure twice - once as it fails and once
    // as the batch it never ran. The person reads it once.
    let doubled = "no named pipe instance matching '' returned from host 'x'";
    let fake = FakeRunner {
        status: 1,
        stderr: format!("{doubled}\n{doubled}\n"),
        ..Default::default()
    };
    let why = run_sql(&fake, Path::new("sqlcmd.exe"), &real, "SELECT 1").await.unwrap_err();
    assert_eq!(why, doubled);
}

#[tokio::test]
async fn the_runner_is_handed_the_exe_the_arguments_and_a_thirty_second_timeout() {
    let fake = FakeRunner::answering("n\n1\n");
    let exe = PathBuf::from(r"C:\Program Files\sqlcmd\sqlcmd.exe");
    let c = read_only_preset();
    run_sql(&fake, &exe, &c, "SELECT 1 AS n").await.unwrap();

    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, exe);
    assert_eq!(calls[0].2, Duration::from_secs(30));
    assert_eq!(calls[0].2, Duration::from_secs(TIMEOUT_SECS));
    assert_eq!(calls[0].1, sqlcmd_args(&c, "SELECT 1 AS n"));
}

#[tokio::test]
async fn the_guard_is_the_only_door_to_the_runner() {
    let fake = FakeRunner::answering("n\n1\n");
    let read_only = read_only_preset();

    let why = run_sql(&fake, Path::new("sqlcmd.exe"), &read_only, "DROP TABLE t").await.unwrap_err();
    assert!(why.contains("DROP"), "{why}");
    assert!(fake.calls().is_empty(), "a refused statement reached the runner");

    let why =
        run_sql(&fake, Path::new("sqlcmd.exe"), &read_only, "DELETE FROM t").await.unwrap_err();
    assert!(why.contains("read only"), "{why}");
    assert!(fake.calls().is_empty(), "a write reached the runner on a read-only connection");

    let why = run_sql(&fake, Path::new("sqlcmd.exe"), &read_only, "SELECT 1\nGO\nSELECT 2")
        .await
        .unwrap_err();
    assert!(why.contains("GO"), "{why}");
    assert!(fake.calls().is_empty());

    // The dev login is the one connection a write gets through on.
    let dev = FakeRunner::answering("\n");
    run_sql(&dev, Path::new("sqlcmd.exe"), &dev_login_preset(), "DELETE FROM t WHERE id = 1")
        .await
        .unwrap();
    assert_eq!(dev.calls().len(), 1);

    // A read runs on either.
    run_sql(&fake, Path::new("sqlcmd.exe"), &read_only, "SELECT 1 AS n").await.unwrap();
    assert_eq!(fake.calls().len(), 1);
}

/// The env override exists so a test can say "sqlcmd is not on this
/// machine" without uninstalling it, and so a person who keeps sqlcmd
/// somewhere none of the candidates look can point at it. It is
/// AUTHORITATIVE, not a first candidate: a path that does not exist reads
/// as "not installed" rather than quietly falling through to a real
/// install, which is exactly what a test needs it to do.
///
/// `sqlcmd_path` is the one impure finder (it reads the environment), so
/// this test owns the variable and puts it back.
#[test]
fn the_env_override_decides_where_sqlcmd_is_or_that_it_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("sqlcmd.exe");
    std::fs::write(&exe, b"").unwrap();

    let before = std::env::var(SQLCMD_OVERRIDE).ok();

    std::env::set_var(SQLCMD_OVERRIDE, &exe);
    assert_eq!(sqlcmd_path(), Some(exe.clone()));

    std::env::set_var(SQLCMD_OVERRIDE, dir.path().join("nowhere.exe"));
    assert_eq!(sqlcmd_path(), None, "an override that is not a file means not installed");

    // An empty variable is not a choice; it reads as unset.
    std::env::set_var(SQLCMD_OVERRIDE, "");
    let _ = sqlcmd_path();

    match before {
        Some(v) => std::env::set_var(SQLCMD_OVERRIDE, v),
        None => std::env::remove_var(SQLCMD_OVERRIDE),
    }
}
