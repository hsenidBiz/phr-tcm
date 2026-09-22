//! The gate every SQL statement passes before sqlcmd ever sees it. A
//! statement is classified first and run second - there is no other door.

use v2_lib::db::{access_for, access_for_user, allowed, classify, Access, Verdict, MAX_SQL_CHARS};
use v2_lib::db_defaults::DB_PRESETS;

/// Panics unless the statement was refused, and hands back the sentence.
fn refusal(sql: &str) -> String {
    match classify(sql) {
        Verdict::Refused(why) => why,
        other => panic!("{sql:?} was not refused: {other:?}"),
    }
}

#[test]
fn only_the_dev_login_preset_may_write() {
    // The shipped set, so the loop below cannot pass by being empty.
    assert_eq!(DB_PRESETS.len(), 3);
    let writers: Vec<&str> = DB_PRESETS
        .iter()
        .filter(|p| access_for(p.connection_string) == Access::DevWrites)
        .map(|p| p.label)
        .collect();
    assert_eq!(writers, vec!["Dev — dev login"]);
    for p in DB_PRESETS {
        let expected =
            if p.label == "Dev — dev login" { Access::DevWrites } else { Access::ReadOnly };
        assert_eq!(access_for(p.connection_string), expected, "{}", p.label);
    }
}

#[test]
fn the_user_id_decides_and_the_password_is_never_looked_at() {
    // Case folds, and the key may be spelled any way a connection string does.
    assert_eq!(access_for("Server=s;Database=d;User Id=SGDEV01DB01_DevLogin;Password=x;"), Access::DevWrites);
    assert_eq!(access_for("server=s;database=d;userid=a_DEVLOGIN;password=x;"), Access::DevWrites);
    assert_eq!(access_for("Server=s;Database=d;UID=a_devlogin;Password=x;"), Access::DevWrites);
    // A password that happens to read like the dev login grants nothing.
    assert_eq!(access_for("Server=s;Database=d;User Id=a_readonly;Password=b_devlogin;"), Access::ReadOnly);
    // No user id at all is the safe answer.
    assert_eq!(access_for("Server=s;Database=d;Password=x;"), Access::ReadOnly);
    assert_eq!(access_for_user("sgdev01db01_devlogin"), Access::DevWrites);
    assert_eq!(access_for_user("sgdev01db02_readonly"), Access::ReadOnly);
}

#[test]
fn a_select_in_its_many_shapes_is_a_read() {
    for sql in [
        "SELECT 1",
        "  with x as (select 1) select * from x",
        "(SELECT 1)",
        "-- comment\nSELECT 1",
        "/* DROP */ SELECT 1",
        // A keyword inside a string literal is data, not a verb.
        "SELECT 'DROP TABLE' AS s",
        "SELECT * FROM t; ",
    ] {
        assert_eq!(classify(sql), Verdict::Read, "{sql:?}");
    }
}

#[test]
fn the_four_write_verbs_are_writes() {
    for sql in [
        "INSERT INTO t VALUES (1)",
        "UPDATE t SET a = 1",
        "DELETE FROM t",
        "MERGE t AS x USING s AS y ON x.id = y.id WHEN MATCHED THEN UPDATE SET x.a = y.a",
        // A CTE in front of a write does not disguise it.
        "WITH c AS (SELECT 1 AS n) INSERT INTO t SELECT n FROM c",
    ] {
        assert_eq!(classify(sql), Verdict::Write, "{sql:?}");
    }
}

#[test]
fn ddl_procedures_and_permission_changes_are_refused_by_name() {
    for (sql, named) in [
        ("DROP TABLE t", "DROP"),
        ("TRUNCATE TABLE t", "TRUNCATE"),
        ("ALTER TABLE t ADD c INT", "ALTER"),
        ("CREATE TABLE t (c INT)", "CREATE"),
        ("EXEC sp_who", "EXEC"),
        ("EXECUTE dbo.something", "EXECUTE"),
        ("GRANT SELECT ON t TO x", "GRANT"),
        ("REVOKE SELECT ON t FROM x", "REVOKE"),
        ("DENY SELECT ON t TO x", "DENY"),
        ("BACKUP DATABASE d TO DISK = 'x'", "BACKUP"),
        ("RESTORE DATABASE d FROM DISK = 'x'", "RESTORE"),
        // Buried mid-statement, it still counts.
        ("SELECT * FROM t WHERE 1 = 1 DROP TABLE t", "DROP"),
    ] {
        let why = refusal(sql);
        assert!(why.contains(named), "{sql:?} was refused as {why:?}, expected {named}");
    }
}

#[test]
fn a_stored_procedure_call_is_refused_wherever_it_hides() {
    assert!(refusal("SELECT * FROM sys.sp_helptext").contains("sp_"));
    assert!(refusal("SELECT * FROM OPENQUERY(x, 'y') WHERE xp_cmdshell = 1").contains("xp_"));
    // A column that merely ends in those letters is not a call.
    assert_eq!(classify("SELECT resp_code FROM t"), Verdict::Read);
}

#[test]
fn a_go_a_second_statement_and_an_empty_statement_are_refused() {
    assert!(refusal("SELECT 1; DELETE FROM t").contains("a second statement"));
    assert!(refusal("SELECT 1;SELECT 2").contains("a second statement"));
    assert!(refusal("SELECT 1\nGO\nSELECT 2").contains("GO"));
    assert!(refusal("select 1\n  go  \nselect 2").contains("GO"));
    assert!(refusal("").contains("empty"));
    assert!(refusal("   \n\t ").contains("empty"));
    assert!(refusal("-- nothing but a comment").contains("empty"));
    assert!(refusal("/* nothing but a comment */").contains("empty"));
}

#[test]
fn a_statement_over_the_character_cap_is_refused() {
    let over = format!("SELECT {}", "1".repeat(MAX_SQL_CHARS + 1 - "SELECT ".len()));
    assert_eq!(over.chars().count(), MAX_SQL_CHARS + 1);
    assert!(refusal(&over).contains("20000"));

    let at_the_cap = format!("SELECT {}", "1".repeat(MAX_SQL_CHARS - "SELECT ".len()));
    assert_eq!(at_the_cap.chars().count(), MAX_SQL_CHARS);
    assert_eq!(classify(&at_the_cap), Verdict::Read);
}

#[test]
fn allowed_passes_reads_everywhere_and_writes_only_on_the_dev_login() {
    assert_eq!(allowed("SELECT 1", Access::ReadOnly), Ok(Verdict::Read));
    assert_eq!(allowed("SELECT 1", Access::DevWrites), Ok(Verdict::Read));
    assert_eq!(allowed("DELETE FROM t WHERE id = 1", Access::DevWrites), Ok(Verdict::Write));

    let why = allowed("DELETE FROM t", Access::ReadOnly).unwrap_err();
    assert!(why.contains("this connection is read only"), "{why}");
    assert!(why.contains("AI Bridge"), "{why}");
    assert!(!why.contains('—'), "no em dashes in user-facing text: {why}");

    // A refusal is a refusal on every connection.
    assert!(allowed("DROP TABLE t", Access::DevWrites).unwrap_err().contains("DROP"));
    assert!(allowed("DROP TABLE t", Access::ReadOnly).unwrap_err().contains("DROP"));
}
