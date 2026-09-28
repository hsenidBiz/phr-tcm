//! Several statements as one all-or-nothing transaction: what may go into
//! one, the wrapper the app writes round them, and what comes back. The
//! real process is never started - `Runner` is faked, as in `db_sqlcmd`.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use v2_lib::db::batch::{compose, name_the_statement, read_report, row_lines, validate, MARK};
use v2_lib::db::query::{run_batch_query, WRITES_OFF};
use v2_lib::db::{
    lexically_closed, parse_connection, run_batch, Access, BatchStatement, Connection, Ended,
    Output, Runner, Verdict, MAX_BATCH_STATEMENTS, READ_ONLY_SENTENCE,
};
use v2_lib::db_defaults::DB_PRESETS;

#[derive(Default)]
struct FakeRunner {
    status: i32,
    stdout: String,
    calls: Mutex<Vec<Vec<String>>>,
}

impl FakeRunner {
    fn answering(status: i32, stdout: &str) -> FakeRunner {
        FakeRunner { status, stdout: stdout.to_string(), ..Default::default() }
    }
    fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }
    /// The batch sqlcmd was handed in `-Q`.
    fn sent(&self) -> String {
        let calls = self.calls();
        let args = calls.last().expect("sqlcmd was called");
        let at = args.iter().position(|a| a == "-Q").expect("-Q");
        args[at + 1].clone()
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
        Ok(Output { status: self.status, stdout: self.stdout.clone(), stderr: String::new() })
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

fn st(sql: &str, expect_rows: Option<i64>) -> BatchStatement {
    BatchStatement { sql: sql.to_string(), expect_rows }
}

fn four_updates() -> Vec<BatchStatement> {
    vec![
        st("UPDATE dbo.Participant SET Active = 0 WHERE CycleId = 7", Some(2)),
        st("UPDATE dbo.Goal SET Status = 'Closed' WHERE CycleId = 7;", Some(24)),
        st("UPDATE dbo.Rating SET Locked = 1 WHERE CycleId = 7 -- ratings", Some(48)),
        st("UPDATE dbo.Stage SET Done = 1 WHERE CycleId = 7", Some(8)),
    ]
}

// ------------------------------------------------------------ closed or open

#[test]
fn a_statement_must_end_outside_every_string_name_and_comment() {
    for closed in [
        "SELECT 1",
        "SELECT 'it''s' AS a",
        "SELECT [a]]b] FROM t",
        "SELECT \"a\"\"b\" FROM t",
        "SELECT 1 /* a /* nested */ comment */",
        "SELECT 1 -- a trailing comment",
        "SELECT '--not a comment' AS a",
        "SELECT '/*' AS a",
    ] {
        assert!(lexically_closed(closed), "{closed}");
    }
    for open in [
        "SELECT 'unclosed",
        "SELECT [unclosed",
        "SELECT \"unclosed",
        "SELECT 1 /* unclosed",
        "SELECT 1 /* a /* nested */ still open",
        "SELECT 'it''s",
    ] {
        assert!(!lexically_closed(open), "{open}");
    }
}

// ------------------------------------------------------------ what may go in

#[test]
fn every_statement_is_held_to_the_guard_and_named_by_its_position() {
    let why = validate(&[st("SELECT 1", None), st("DROP TABLE t", None)], Access::DevWrites).unwrap_err();
    assert!(why.starts_with("statement 2:"), "{why}");
    assert!(why.contains("DROP"), "{why}");

    // The transaction is the app's: a statement cannot bring its own.
    for own in ["COMMIT", "BEGIN TRANSACTION", "ROLLBACK", "DECLARE @x INT", "SELECT 1; THROW 50000, 'x', 1"] {
        let why = validate(&[st("UPDATE t SET a = 1", None), st(own, None)], Access::DevWrites).unwrap_err();
        assert!(why.starts_with("statement 2:"), "{own}: {why}");
    }
}

#[test]
fn an_open_statement_cannot_swallow_the_next_one() {
    // Alone, the second statement's string is data. After an open quote
    // it would become code - so the open statement is refused first.
    let smuggled = [
        st("SELECT 'a", None),
        st("SELECT 1 WHERE 'x' = '; DELETE FROM t; --'", None),
    ];
    let why = validate(&smuggled, Access::DevWrites).unwrap_err();
    assert!(why.contains("statement 1 ends inside"), "{why}");

    let why = validate(&[st("UPDATE t SET a = 1 /*", Some(1)), st("*/ SELECT 1", None)], Access::DevWrites)
        .unwrap_err();
    assert!(why.contains("statement 1 ends inside"), "{why}");
}

#[test]
fn the_wrappers_own_variables_are_off_limits() {
    let why = validate(&[st("SELECT @tcm_rows AS n", None)], Access::DevWrites).unwrap_err();
    assert!(why.contains("@tcm_"), "{why}");
}

#[test]
fn a_batch_has_a_size_and_a_shape() {
    assert!(validate(&[], Access::DevWrites).unwrap_err().contains("no statements"));
    let many: Vec<_> = (0..=MAX_BATCH_STATEMENTS).map(|_| st("SELECT 1", None)).collect();
    assert!(validate(&many, Access::DevWrites).unwrap_err().contains("at most"));
    let why = validate(&[st("UPDATE t SET a = 1", Some(-1))], Access::DevWrites).unwrap_err();
    assert!(why.contains("negative"), "{why}");
}

#[test]
fn one_write_makes_the_batch_a_write_and_the_connection_decides() {
    assert_eq!(validate(&[st("SELECT 1", None), st("SELECT 2", None)], Access::ReadOnly), Ok(Verdict::Read));
    assert_eq!(
        validate(&[st("SELECT 1", None), st("UPDATE t SET a = 1", None)], Access::DevWrites),
        Ok(Verdict::Write)
    );
    let why = validate(&[st("SELECT 1", None), st("UPDATE t SET a = 1", None)], Access::ReadOnly).unwrap_err();
    assert!(why.contains(READ_ONLY_SENTENCE), "{why}");
}

// ------------------------------------------------------------ the wrapper

#[test]
fn the_wrapper_is_one_transaction_that_aborts_on_any_error_and_checks_each_count() {
    let (sql, starts) = compose(&four_updates(), false);
    assert!(sql.starts_with("SET NOCOUNT ON;\nSET XACT_ABORT ON;\n"), "{sql}");
    assert!(sql.contains("BEGIN TRANSACTION;"), "{sql}");
    assert!(sql.trim_end().ends_with(&format!("PRINT N'{MARK}|end|saved';")), "{sql}");
    assert!(sql.contains("\nCOMMIT TRANSACTION;\n"), "{sql}");
    assert!(!sql.contains("ROLLBACK"), "{sql}");
    // Each statement, its own terminator gone, then at once its count.
    assert!(sql.contains("WHERE CycleId = 7\n;\nSET @tcm_rows = @@ROWCOUNT;"), "{sql}");
    assert!(!sql.contains("7;\n;"), "a statement's own ; is replaced, not doubled: {sql}");
    // A trailing comment ends at the line break, before the terminator.
    assert!(sql.contains("-- ratings\n;\nSET @tcm_rows"), "{sql}");
    assert!(sql.contains("IF @tcm_rows <> 24 BEGIN"), "{sql}");
    assert!(sql.contains("THROW 50000, @tcm_msg, 1;"), "{sql}");
    // Where each statement starts, for naming errors.
    let lines: Vec<&str> = sql.lines().collect();
    assert_eq!(starts.len(), 4);
    assert!(lines[starts[0] - 1].starts_with("UPDATE dbo.Participant"), "{:?}", lines[starts[0] - 1]);
    assert!(lines[starts[3] - 1].starts_with("UPDATE dbo.Stage"), "{:?}", lines[starts[3] - 1]);
}

#[test]
fn a_dry_run_rolls_back_and_a_statement_without_expect_rows_is_not_checked() {
    let (sql, _) = compose(&[st("UPDATE t SET a = 1", None)], true);
    assert!(sql.contains("\nROLLBACK TRANSACTION;\n"), "{sql}");
    assert!(!sql.contains("COMMIT"), "{sql}");
    assert!(sql.contains(&format!("{MARK}|end|rolled-back")), "{sql}");
    assert!(!sql.contains("THROW"), "{sql}");
}

// ------------------------------------------------------------ what comes back

#[test]
fn the_report_is_read_off_the_wrappers_own_lines() {
    let out = format!("{MARK}|1|2\nId\tName\n--\t----\n7\tAlpha\n{MARK}|2|24\n{MARK}|end|saved\n");
    let r = read_report(&out, 3);
    assert_eq!(r.rows, vec![Some(2), Some(24), None]);
    assert_eq!(r.ended, Some(Ended::Saved));
    assert_eq!(r.rest, "Id\tName\n--\t----\n7\tAlpha");
}

#[test]
fn a_server_line_number_is_said_as_the_statement() {
    let said = "Msg 547, Level 16, State 0, Server dev, Line 12\nThe UPDATE conflicted with a constraint.";
    let named = name_the_statement(said, &[5, 10, 15]);
    assert!(named.starts_with("Msg 547, Level 16, State 0, Server dev, in statement 2\n"), "{named}");
}

#[test]
fn each_statements_line_says_what_it_did() {
    let lines = row_lines(&four_updates()[..2], &[Some(2), Some(30)]);
    assert!(lines.starts_with("1. 2 rows (expected 2) - UPDATE dbo.Participant"), "{lines}");
    assert!(lines.contains("2. 30 rows - expected 24 - UPDATE dbo.Goal"), "{lines}");
}

// ------------------------------------------------------------ running one

#[tokio::test]
async fn a_refused_statement_stops_the_whole_batch_before_sqlcmd() {
    let fake = FakeRunner::answering(0, "");
    let why = run_batch(&fake, &exe(), &dev_login(), &[st("UPDATE t SET a = 1", None), st("COMMIT", None)], false)
        .await
        .unwrap_err();
    assert!(why.contains("statement 2"), "{why}");
    assert!(fake.calls().is_empty(), "a refused batch reached the runner");

    // And the connection is asked, not the caller.
    let why = run_batch(&fake, &exe(), &read_only(), &[st("UPDATE t SET a = 1", None)], false)
        .await
        .unwrap_err();
    assert!(why.contains("read only"), "{why}");
    assert!(fake.calls().is_empty());
}

#[tokio::test]
async fn a_batch_that_saves_says_so_with_every_count() {
    // `run_batch_query` also touches `activity_log`'s process-wide
    // directory now - held so a concurrent test's own tempdir assertions
    // never see a stray write from this one (see serial::activity_log).
    let _act = crate::serial::activity_log();
    let out = format!("{MARK}|1|2\n{MARK}|2|24\n{MARK}|3|48\n{MARK}|4|8\n{MARK}|end|saved\n");
    let fake = FakeRunner::answering(0, &out);
    let said = run_batch_query(&fake, &exe(), &dev_login(), true, &four_updates(), false).await.unwrap();
    assert!(said.starts_with("Saved: all 4 statements ran in one transaction."), "{said}");
    assert!(said.contains("3. 48 rows (expected 48)"), "{said}");
    assert!(!said.contains(MARK), "{said}");
    assert!(fake.sent().contains("COMMIT TRANSACTION;"));
}

#[tokio::test]
async fn a_dry_run_says_nothing_was_saved() {
    let _act = crate::serial::activity_log();
    let out = format!("{MARK}|1|2\n{MARK}|2|24\n{MARK}|3|48\n{MARK}|4|8\n{MARK}|end|rolled-back\n");
    let fake = FakeRunner::answering(0, &out);
    let said = run_batch_query(&fake, &exe(), &dev_login(), true, &four_updates(), true).await.unwrap();
    assert!(said.starts_with("Dry run: all 4 statements ran and were rolled back - nothing was saved."), "{said}");
    assert!(fake.sent().contains("ROLLBACK TRANSACTION;"));
}

#[tokio::test]
async fn a_count_that_does_not_match_rolls_everything_back_and_says_what_ran() {
    let _act = crate::serial::activity_log();
    // What sqlcmd prints when the row check throws after statement 2.
    let out = format!(
        "{MARK}|1|2\n{MARK}|2|30\nMsg 50000, Level 16, State 1, Server dev, Line 14\nstatement 2 affected 30 rows, expected 24\n"
    );
    let fake = FakeRunner::answering(1, &out);
    let (status, said) =
        run_batch_query(&fake, &exe(), &dev_login(), true, &four_updates(), false).await.unwrap_err();
    assert_eq!(status, 502);
    assert!(said.starts_with("nothing was saved - the batch stopped and was rolled back:"), "{said}");
    assert!(said.contains("statement 2 affected 30 rows, expected 24"), "{said}");
    assert!(said.contains("Before it stopped:\n1. 2 rows (expected 2)"), "{said}");
    assert!(said.contains("2. 30 rows - expected 24"), "{said}");
    assert!(!said.contains("|end|"), "{said}");
}

#[tokio::test]
async fn the_two_write_doors_apply_to_a_batch_as_a_whole() {
    let _act = crate::serial::activity_log();
    let fake = FakeRunner::answering(0, "");
    let (status, said) = run_batch_query(&fake, &exe(), &dev_login(), false, &four_updates(), true)
        .await
        .unwrap_err();
    assert_eq!((status, said.as_str()), (400, WRITES_OFF));
    let (status, said) = run_batch_query(&fake, &exe(), &read_only(), true, &four_updates(), true)
        .await
        .unwrap_err();
    assert_eq!((status, said.as_str()), (400, READ_ONLY_SENTENCE));
    assert!(fake.calls().is_empty(), "a write batch ran through a shut door");

    // A batch of reads needs neither door.
    let out = format!("{MARK}|1|1\n{MARK}|end|saved\n");
    let reads = FakeRunner::answering(0, &out);
    run_batch_query(&reads, &exe(), &read_only(), false, &[st("SELECT 1 AS n", Some(1))], false)
        .await
        .expect("a read batch runs on the read-only connection");
}
