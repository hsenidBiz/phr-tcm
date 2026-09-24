//! The gate every SQL statement passes before sqlcmd ever sees it. A
//! statement is classified first and run second - there is no other door.

use v2_lib::db::{
    access_for, access_for_user, allowed, classify, parse_connection, Access, Verdict,
    MAX_SQL_CHARS, READ_ONLY_SENTENCE,
};
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
fn both_doors_read_the_user_id_from_the_same_keys() {
    // A hand-typed connection string may spell the key any of these ways.
    // The guard reads it to decide, `parse_connection` reads it to sign in,
    // and if the two disagreed a `User=x_devlogin` could be refused a write
    // by one door while the other handed the dev login to sqlcmd.
    for key in ["User Id", "UserId", "user id", "UID", "uid", "User", "USER"] {
        let s = format!("Server=s;Database=d;{key}=x_devlogin;Password=p;");
        assert_eq!(access_for(&s), Access::DevWrites, "{key}");
        let c = parse_connection(&s).unwrap();
        assert_eq!(c.user, "x_devlogin", "{key}");
        assert_eq!(access_for_user(&c.user), Access::DevWrites, "{key}");
    }
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
        // sp_who is a real system procedure but not one of the look-up
        // ones on the allowlist, so it is still refused - just no longer
        // because EXEC itself is a refused word (see the EXEC tests below,
        // where EXEC of a NAMED USER procedure is now a Write instead of a
        // refusal - that rule replaced the old blanket one).
        ("EXEC sp_who", "EXEC"),
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
    assert!(refusal("SELECT xp_cmdshell FROM t").contains("xp_"));
    // A column that merely ends in those letters is not a call.
    assert_eq!(classify("SELECT resp_code FROM t"), Verdict::Read);
}

#[test]
fn a_remote_rowset_cannot_smuggle_a_payload_past_the_stripper() {
    // The stripper blanks what is inside a literal, so the second argument
    // here is never classified at all. That is exactly why the function
    // itself has to be refused rather than its contents inspected.
    let hidden = "SELECT * FROM OPENROWSET('SQLNCLI', 'server=x;trusted_connection=yes', 'EXEC xp_cmdshell ''dir''')";
    assert!(refusal(hidden).contains("OPENROWSET"), "{}", refusal(hidden));
    assert!(refusal("SELECT * FROM OPENQUERY(remote, 'SELECT 1')").contains("OPENQUERY"));
    assert!(refusal("SELECT * FROM OPENDATASOURCE('SQLNCLI', 'x').d.s.t").contains("OPENDATASOURCE"));
}

#[test]
fn select_into_creates_a_table_and_is_refused() {
    for sql in [
        "SELECT * INTO newtable FROM t",
        "select a, b into #tmp from t",
        "WITH c AS (SELECT 1 AS n) SELECT n INTO keep FROM c",
    ] {
        let why = refusal(sql);
        assert!(why.contains("SELECT INTO"), "{sql:?} was refused as {why:?}");
    }
    // INSERT INTO is a write, not a table being brought into existence, and
    // the write check runs first so it still reads as one.
    assert_eq!(classify("INSERT INTO t VALUES (1)"), Verdict::Write);
    assert_eq!(classify("WITH c AS (SELECT 1 AS n) INSERT INTO t SELECT n FROM c"), Verdict::Write);
    // A column that merely starts with those letters is not the keyword.
    assert_eq!(classify("SELECT into_date FROM t"), Verdict::Read);
}

#[test]
fn a_leading_semicolon_is_not_a_second_statement() {
    // `;WITH` is how a CTE is written after anything else, and people paste
    // it with the semicolon still attached.
    assert_eq!(classify(";WITH c AS (SELECT 1 AS n) SELECT n FROM c"), Verdict::Read);
    assert_eq!(classify("; SELECT 1"), Verdict::Read);
    assert!(refusal(";").contains("empty"));
    assert!(refusal(" ; ; ").contains("empty"));
    // One in the middle is still two statements.
    assert!(refusal(";SELECT 1; SELECT 2").contains("a second statement"));
}

#[test]
fn a_byte_order_mark_does_not_hide_the_verb() {
    assert_eq!(classify("\u{feff}SELECT 1"), Verdict::Read);
    assert_eq!(classify("\u{feff}DELETE FROM t"), Verdict::Write);
    assert!(refusal("\u{feff}DROP TABLE t").contains("DROP"));
}

#[test]
fn a_go_a_second_statement_and_an_empty_statement_are_refused() {
    assert!(refusal("SELECT 1; DELETE FROM t").contains("a second statement"));
    assert!(refusal("SELECT 1;SELECT 2").contains("a second statement"));
    assert!(refusal("").contains("empty"));
    assert!(refusal("   \n\t ").contains("empty"));
    assert!(refusal("-- nothing but a comment").contains("empty"));
    assert!(refusal("/* nothing but a comment */").contains("empty"));
}

#[test]
fn go_is_a_separator_whatever_follows_it_on_the_line() {
    // sqlcmd acts on GO itself, and it takes a repeat count and tolerates a
    // trailing comment - so matching the whole line missed both.
    for sql in [
        "SELECT 1\nGO\nSELECT 2",
        "SELECT 1\nGO 5\nSELECT 2",
        "SELECT 1\nGO -- run it five times\nSELECT 2",
        "select 1\n  go  \nselect 2",
        "select 1\n  go\t2  \nselect 2",
    ] {
        assert!(refusal(sql).contains("GO"), "{sql:?}");
    }
    // A name that merely starts with those letters is not the separator.
    assert_eq!(classify("SELECT a\nFROM t\nWHERE go_live = 1"), Verdict::Read);
    assert_eq!(classify("SELECT a,\ngoal\nFROM t"), Verdict::Read);
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

/// T-SQL needs no semicolon between statements, so "one statement" cannot
/// be judged by semicolons alone. Every chain below was confirmed as valid
/// T-SQL by the server itself (SET PARSEONLY ON, sgdev01db02, 2026-09-23)
/// and got past the guard in 1.25.17 - on the dev login it would have run.
#[test]
fn a_second_statement_without_a_semicolon_is_refused() {
    for sql in [
        "SELECT 1 KILL 1",
        "SELECT 1 USE master",
        "SELECT 1 DBCC DROPCLEANBUFFERS",
        "SELECT 1 WAITFOR DELAY '00:00:25'",
        "SELECT 1 SHUTDOWN",
        "SELECT 1 DECLARE @x INT",
        "SELECT 1 AS a SET NOCOUNT ON",
        "SELECT 1 UPDATE STATISTICS dbo.X",
        "SELECT 1 AS a DISABLE TRIGGER ALL ON dbo.X",
        "SELECT 1 AS a ENABLE TRIGGER ALL ON dbo.X",
        "SELECT 1 BEGIN TRAN",
        "SELECT 1 CHECKPOINT",
        "UPDATE dbo.X SET a = 1 KILL 1",
    ] {
        let why = refusal(sql);
        assert!(!why.is_empty(), "{sql}");
    }
}

/// The words that close that gap must not refuse what a person ordinarily
/// writes: names that merely contain them, literals, brackets, a query
/// hint, and the SET that belongs to an UPDATE.
#[test]
fn closing_the_chain_gap_leaves_ordinary_statements_alone() {
    assert_eq!(classify("SELECT BEGIN_DATE, STATUS_ENABLED, KILL_SWITCH FROM dbo.T"), Verdict::Read);
    assert_eq!(classify("SELECT 'KILL 1; USE master' AS s"), Verdict::Read);
    assert_eq!(classify("SELECT [Use], [Kill], [Set] FROM dbo.T"), Verdict::Read);
    assert_eq!(
        classify("SELECT a FROM dbo.T OPTION (USE HINT ('FORCE_LEGACY_CARDINALITY_ESTIMATION'))"),
        Verdict::Read
    );
    assert_eq!(classify("UPDATE dbo.T SET a = 1 WHERE id = 2"), Verdict::Write);
}

/// A stored procedure can change data, so calling a NAMED one gets exactly
/// the rule INSERT/UPDATE/DELETE already have: the dev login only, and only
/// with the writes switch on. This is the rule that replaced the old
/// blanket "EXEC is always refused" - `EXECUTE dbo.something`, refused in
/// 1.25.25 and earlier, is a Write now like any other named call.
#[test]
fn exec_of_a_named_procedure_is_a_write_gated_like_any_other_write() {
    assert_eq!(classify("EXEC dbo.GetLeave @EmpId = 5"), Verdict::Write);
    assert_eq!(
        allowed("EXEC dbo.GetLeave @EmpId = 5", Access::ReadOnly).unwrap_err(),
        READ_ONLY_SENTENCE
    );
    assert_eq!(allowed("EXEC dbo.GetLeave @EmpId = 5", Access::DevWrites), Ok(Verdict::Write));

    assert_eq!(classify("EXECUTE [dbo].[GetLeave] 5, N'x', NULL"), Verdict::Write);
    assert_eq!(classify("EXEC @rc = dbo.p @a = @b OUTPUT;"), Verdict::Write);

    // The old test asserted this was refused because EXEC itself was a
    // refused word. It is a named two-part procedure call now, so it goes
    // through the same door INSERT/UPDATE/DELETE do.
    assert_eq!(classify("EXECUTE dbo.something"), Verdict::Write);
}

/// The fixed set of look-up procedures reads schema information, never
/// data, so the owner approved it on every connection - it is a `Read`,
/// same as a SELECT.
#[test]
fn exec_of_a_lookup_system_procedure_is_a_read_everywhere() {
    for sql in [
        "EXEC sp_help 'dbo.Employee'",
        "exec sys.sp_columns N'Employee'",
        "EXEC sp_helptext 'dbo.p'",
    ] {
        assert_eq!(classify(sql), Verdict::Read, "{sql}");
        assert_eq!(allowed(sql, Access::ReadOnly), Ok(Verdict::Read), "{sql}");
    }
}

/// Everything outside the one accepted shape - dynamic SQL, a system
/// procedure that is not on the look-up list, an over-qualified name, EXEC
/// welded on mid-statement, or anything after the argument list besides one
/// trailing semicolon - is refused, whatever connection is asked.
#[test]
fn exec_outside_the_accepted_shape_is_refused() {
    for sql in [
        "EXEC('SELECT 1')",
        "EXEC (@sql)",
        "EXEC @p",
        "EXEC sp_executesql N'SELECT 1'",
        "EXEC xp_cmdshell 'dir'",
        "EXEC sp_configure",
        "EXEC otherdb.dbo.p",
        "EXEC srv.db.dbo.p",
        "SELECT 1 EXEC dbo.p",
        "EXEC dbo.p 1 DROP TABLE t",
        "EXEC dbo.p 1 SELECT 1",
        "EXEC dbo.p; SELECT 1",
        "EXEC dbo.p 1 GO",
    ] {
        let why = refusal(sql);
        assert!(!why.is_empty(), "{sql}");
        // A refusal is a refusal on every connection - the dev login does
        // not get to run any of these either.
        assert!(allowed(sql, Access::DevWrites).is_err(), "{sql}");
    }
}

/// Comments and string literals are stripped/blanked before anything is
/// classified, same as everywhere else in the guard - a keyword sitting
/// inside either one is data, not shape.
#[test]
fn exec_treats_comments_and_literals_in_its_arguments_as_data() {
    assert_eq!(classify("EXEC dbo.p 'DROP TABLE t'"), Verdict::Write);
    assert_eq!(classify("EXEC dbo.p /* xp_cmdshell */ 1"), Verdict::Write);
}
