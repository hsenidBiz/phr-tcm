//! The ranked schema lookup: one SELECT over the database's own catalogue,
//! and the text an assistant reads back from it.

use v2_lib::db::{classify, lookup_sql, render_lookup, Verdict};

#[test]
fn the_lookup_is_one_statement_its_own_guard_calls_a_read() {
    let sql = lookup_sql("leave request", "PeoplesHR", 10);

    assert!(sql.contains("INFORMATION_SCHEMA.TABLES"), "{sql}");
    assert!(sql.contains("INFORMATION_SCHEMA.COLUMNS"), "{sql}");
    assert!(sql.contains("sys.foreign_keys"), "{sql}");
    assert!(sql.contains("sys.foreign_key_columns"), "{sql}");
    assert!(sql.contains("TABLE_SCHEMA = N'PeoplesHR'"), "{sql}");
    assert!(sql.contains("N'leave'"), "{sql}");
    assert!(sql.contains("N'request'"), "{sql}");
    assert!(sql.contains("TOP (10)"), "{sql}");
    // The foreign keys are folded in rather than fetched by a second trip.
    assert!(sql.contains("STRING_AGG"), "{sql}");
    // The scores the ranking is built from.
    for score in ["100", "60", "40", "20"] {
        assert!(sql.contains(score), "{score} missing from {sql}");
    }
    // The gate this statement has to pass is the same one every statement
    // passes, so the lookup can never be refused by the app's own guard.
    assert_eq!(classify(&sql), Verdict::Read);
    assert_eq!(sql.matches(';').count(), 0, "a semicolon would read as a second statement");
}

#[test]
fn the_words_are_lower_cased_split_and_escaped_as_literals() {
    let sql = lookup_sql("Leave REQUEST", "PeoplesHR", 10);
    assert!(sql.contains("N'leave'"), "{sql}");
    assert!(sql.contains("N'request'"), "{sql}");
    assert!(!sql.contains("N'Leave'"), "{sql}");

    let split = lookup_sql("leave-request_2, please", "", 5);
    for term in ["N'leave'", "N'request'", "N'please'"] {
        assert!(split.contains(term), "{term} missing from {split}");
    }
    // A single character is in half the names in the database; it narrows
    // nothing and only drags unrelated tables up the ranking.
    assert!(!split.contains("N'2'"), "{split}");

    // An apostrophe belongs to the word and is doubled, not split on: that
    // is the one non-alphanumeric a name really contains.
    let quoted = lookup_sql("o'brien", "", 5);
    assert!(quoted.contains("N'o''brien'"), "{quoted}");
    assert_eq!(classify(&quoted), Verdict::Read);

    // A schema filter with a quote is escaped the same way.
    let odd = lookup_sql("leave", "o'neill", 5);
    assert!(odd.contains("TABLE_SCHEMA = N'o''neill'"), "{odd}");
    assert_eq!(classify(&odd), Verdict::Read);
}

#[test]
fn an_empty_schema_filter_searches_every_schema() {
    let sql = lookup_sql("leave", "", 5);
    assert!(!sql.contains("TABLE_SCHEMA = N'"), "{sql}");
    assert!(sql.contains("TOP (5)"), "{sql}");
    assert_eq!(classify(&sql), Verdict::Read);
}

#[test]
fn a_query_with_no_words_is_still_a_read_that_can_match_nothing() {
    // The last one is all single characters, which are dropped - so a query
    // can end up with no terms even though it had words in it.
    for nothing in ["", "   ", "--- ,, ///", "a b c"] {
        let sql = lookup_sql(nothing, "PeoplesHR", 10);
        assert_eq!(classify(&sql), Verdict::Read, "{sql}");
    }
}

#[test]
fn a_table_matching_two_words_outranks_one_matching_only_one() {
    let sql = lookup_sql("leave request", "PeoplesHR", 10);
    // The per-term scores are added up. Under MAX, a table called
    // "LeaveRequest" scored the same 60 as one called "Leave".
    assert!(sql.contains("SUM(CASE WHEN LOWER(t.TABLE_NAME)"), "{sql}");
    assert!(sql.contains("SUM(CASE WHEN LOWER(c.COLUMN_NAME)"), "{sql}");
    assert!(!sql.contains("MAX(CASE"), "{sql}");
    assert_eq!(classify(&sql), Verdict::Read);
}

#[test]
fn the_columns_and_the_keys_are_gathered_only_for_the_tables_that_were_picked() {
    // Without this the two aggregates run over every table in the database
    // and are then thrown away by the join.
    let sql = lookup_sql("leave", "PeoplesHR", 10);
    assert_eq!(sql.matches("EXISTS (SELECT 1 FROM picked").count(), 2, "{sql}");
    assert_eq!(classify(&sql), Verdict::Read);
}

/// The shape sqlcmd writes with `-s "\t" -W`: a header, its dashes, then a
/// row per table, and the "(N rows affected)" line it signs off with.
const TWO_TABLES: &str = "sch\ttab\trows_est\tcols\tfks\n\
----\t---\t--------\t----\t---\n\
dbo\tLeaveRequest\t1240\tLeaveRequestId int | LeaveTypeId int\tEmployeeId -> dbo.Employee(EmployeeId) | LeaveTypeId -> dbo.LeaveType(LeaveTypeId)\n\
dbo\tLeaveType\t12\tLeaveTypeId int | LeaveTypeName nvarchar\t\n\
\n\
(2 rows affected)\n";

#[test]
fn the_answer_renders_as_one_block_per_table() {
    let out = render_lookup(TWO_TABLES);

    assert!(out.contains("dbo.LeaveRequest (1240 rows est.)"), "{out}");
    assert!(out.contains("LeaveRequestId int"), "{out}");
    assert!(out.contains("LeaveTypeName nvarchar"), "{out}");
    assert!(out.contains("EmployeeId -> dbo.Employee(EmployeeId)"), "{out}");
    assert!(out.contains("LeaveTypeId -> dbo.LeaveType(LeaveTypeId)"), "{out}");
    assert!(out.contains("dbo.LeaveType (12 rows est.)"), "{out}");

    // sqlcmd's own furniture is not part of the answer.
    assert!(!out.contains("rows affected"), "{out}");
    assert!(!out.contains("----"), "{out}");
    assert!(!out.contains("rows_est"), "{out}");
    // The table with no foreign keys does not get an empty heading.
    let second = out.split("dbo.LeaveType (12").nth(1).unwrap();
    assert!(!second.contains("foreign key"), "{out}");
    assert!(out.contains("columns: LeaveRequestId int, LeaveTypeId int"), "{out}");
}

#[test]
fn nothing_found_says_so_in_a_sentence() {
    assert_eq!(render_lookup(""), "no table or column matches those words");
    assert_eq!(
        render_lookup("sch\ttab\trows_est\tcols\tfks\n---\t---\t---\t---\t---\n\n(0 rows affected)\n"),
        "no table or column matches those words"
    );
    assert_eq!(render_lookup("   \n\n"), "no table or column matches those words");
}
