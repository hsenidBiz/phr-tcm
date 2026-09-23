//! The ranked schema lookup: a ranking in one scan of the catalogue, the
//! details for the tables it picked, and the text an assistant reads back.

use v2_lib::db::{
    classify, describe_sql, detail_sql, lookup_sql, parse_ranked, render_describe, render_lookup,
    Picked, Verdict,
};

#[test]
fn the_ranking_is_one_scan_its_own_guard_calls_a_read() {
    let sql = lookup_sql("leave request", "PeoplesHR", 10);

    assert!(sql.contains("FROM INFORMATION_SCHEMA.COLUMNS c"), "{sql}");
    assert!(sql.contains("TABLE_SCHEMA = N'PeoplesHR'"), "{sql}");
    assert!(sql.contains("N'leave'"), "{sql}");
    assert!(sql.contains("N'request'"), "{sql}");
    assert!(sql.contains("TOP (10)"), "{sql}");
    assert!(sql.contains("GROUP BY c.TABLE_SCHEMA, c.TABLE_NAME"), "{sql}");
    // What made the old single statement time out on a 13,000-table
    // database: a second catalogue view, and per-row EXISTS tests against a
    // ranking SQL Server recomputed for every row (2026-09-23).
    assert!(!sql.contains("INFORMATION_SCHEMA.TABLES"), "{sql}");
    assert!(!sql.contains("EXISTS"), "{sql}");
    assert!(!sql.contains("CROSS JOIN"), "{sql}");
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
    // The per-term scores are ADDED inside one expression, so a table
    // called "LeaveRequest" scores 120 where one called "Leave" scores 60.
    // MAX only ever works across a table's rows, never across the terms.
    assert!(sql.contains("ELSE 0 END + CASE WHEN LOWER(c.TABLE_NAME)"), "{sql}");
    assert!(sql.contains("ELSE 0 END + CASE WHEN LOWER(c.COLUMN_NAME)"), "{sql}");
    assert_eq!(classify(&sql), Verdict::Read);
}

fn picked(pairs: &[(&str, &str, i64)]) -> Vec<Picked> {
    pairs
        .iter()
        .map(|(sch, tab, score)| Picked { sch: sch.to_string(), tab: tab.to_string(), score: *score })
        .collect()
}

#[test]
fn the_details_are_read_only_for_the_tables_that_were_picked() {
    let sql = detail_sql("leave", &picked(&[("dbo", "LeaveRequest", 160), ("dbo", "LeaveType", 60)]));
    // The picked tables arrive as constants, so nothing is ranked again.
    assert!(sql.contains("(VALUES (N'dbo', N'LeaveRequest', 160), (N'dbo', N'LeaveType', 60))"), "{sql}");
    assert_eq!(sql.matches("JOIN picked p").count(), 2, "the columns and the keys: {sql}");
    assert!(!sql.contains("EXISTS"), "{sql}");
    assert!(sql.contains("sys.foreign_keys"), "{sql}");
    assert!(sql.contains("sys.foreign_key_columns"), "{sql}");
    assert!(sql.contains("STRING_AGG"), "{sql}");
    assert!(sql.contains("LIKE N'%leave%'"), "only the columns that match the words: {sql}");
    assert_eq!(classify(&sql), Verdict::Read);
    assert_eq!(sql.matches(';').count(), 0, "a semicolon would read as a second statement");

    // A name read back from the database is escaped like anything else.
    let odd = detail_sql("x", &picked(&[("o'neill", "it's", 1)]));
    assert!(odd.contains("(N'o''neill', N'it''s', 1)"), "{odd}");
    assert_eq!(classify(&odd), Verdict::Read);
}

#[test]
fn the_ranking_answer_parses_into_the_picked_tables_in_order() {
    let tsv = "sch\ttab\tscore\n\
---\t---\t-----\n\
PeoplesHR\tperf_performance_cycle\t160\r\n\
PeoplesHR\tperf_cycle_calibration\t100\n\
\n\
(2 rows affected)\n";
    assert_eq!(
        parse_ranked(tsv),
        picked(&[("PeoplesHR", "perf_performance_cycle", 160), ("PeoplesHR", "perf_cycle_calibration", 100)])
    );
    assert!(parse_ranked("").is_empty());
    assert!(parse_ranked("sch\ttab\tscore\n---\t---\t---\n\n(0 rows affected)\n").is_empty());
    // Anything that is not three cells ending in a number is not a table.
    assert!(parse_ranked("Msg 208, Level 16\nInvalid object name\n").is_empty());
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

// --------------------------------------------------------- describe a table
//
// The ranked lookup lists only the columns that MATCHED the words, which is
// the right answer to "what is leave request about" and the wrong one to
// "what is in dbo.LeaveRequest". A bare name asks the second question, and
// gets every column of that table instead.

#[test]
fn a_bare_table_name_asks_for_the_whole_column_list() {
    let sql = describe_sql("dbo.LeaveRequest").expect("schema.table is a name");
    assert!(sql.contains("INFORMATION_SCHEMA.COLUMNS"), "{sql}");
    assert!(sql.contains("N'leaverequest'"), "lower-cased for the comparison: {sql}");
    assert!(sql.contains("N'dbo'"), "{sql}");
    assert!(sql.contains("ORDINAL_POSITION"), "the table's own column order: {sql}");
    // The same gate as every other statement, and one statement only.
    assert_eq!(classify(&sql), Verdict::Read);
    assert_eq!(sql.matches(';').count(), 0, "a semicolon would read as a second statement");

    // Without a schema the name alone is matched, across schemas.
    let bare = describe_sql("LeaveRequest").expect("a bare table is a name too");
    assert!(bare.contains("N'leaverequest'"), "{bare}");
    assert!(!bare.contains("TABLE_SCHEMA) = N'"), "no schema was given: {bare}");
    assert_eq!(classify(&bare), Verdict::Read);
}

#[test]
fn a_topic_is_not_a_name_and_gets_the_ranked_lookup_instead() {
    for topic in [
        "leave request",
        "",
        "   ",
        "dbo.leave.request",
        "leave-request",
        "select * from t",
        "dbo.",
        "o'brien",
    ] {
        assert!(describe_sql(topic).is_none(), "{topic:?} is a topic, not a table name");
    }
}

/// What sqlcmd writes for the describe statement: header, rule, a row per
/// column, then its footer.
const COLUMNS: &str = "sch\ttab\tcol\ttyp\tlen\tnul\n\
----\t---\t---\t---\t---\t---\n\
dbo\tLeaveRequest\tLeaveRequestId\tint\tNULL\tNO\n\
dbo\tLeaveRequest\tReason\tnvarchar\t200\tYES\n\
dbo\tLeaveRequest\tNotes\tnvarchar\t-1\tYES\n\
\n\
(3 rows affected)\n";

#[test]
fn the_column_list_renders_with_its_types_and_nullability() {
    let out = render_describe(COLUMNS);

    assert!(out.starts_with("dbo.LeaveRequest"), "{out}");
    assert!(out.contains("LeaveRequestId int not null"), "{out}");
    assert!(out.contains("Reason nvarchar(200) null"), "{out}");
    // -1 is how SQL Server reports nvarchar(max).
    assert!(out.contains("Notes nvarchar(max) null"), "{out}");
    // sqlcmd's own furniture is not part of the answer.
    assert!(!out.contains("rows affected"), "{out}");
    assert!(!out.contains("----"), "{out}");
}

#[test]
fn a_table_that_does_not_exist_renders_as_nothing_so_the_lookup_can_answer() {
    assert_eq!(render_describe(""), "");
    assert_eq!(
        render_describe("sch\ttab\tcol\ttyp\tlen\tnul\n---\t---\t---\t---\t---\t---\n\n(0 rows affected)\n"),
        ""
    );
}

/// A bare name with no schema in front of it (`describe_sql("LeaveRequest")`)
/// matches every schema with a table of that name, so sqlcmd answers with
/// both `dbo.LeaveRequest`'s columns and `hr.LeaveRequest`'s columns, in
/// that order (the query's own `ORDER BY c.TABLE_SCHEMA, c.TABLE_NAME`).
/// Rendering them under one heading would silently hand back a table that
/// does not exist - `dbo.LeaveRequest` with `hr`'s columns folded in.
const TWO_SCHEMAS: &str = "sch\ttab\tcol\ttyp\tlen\tnul\n\
----\t---\t---\t---\t---\t---\n\
dbo\tLeaveRequest\tLeaveRequestId\tint\tNULL\tNO\n\
dbo\tLeaveRequest\tReason\tnvarchar\t200\tYES\n\
hr\tLeaveRequest\tId\tint\tNULL\tNO\n\
hr\tLeaveRequest\tApprover\tnvarchar\t100\tYES\n\
hr\tLeaveRequest\tCreatedAt\tdatetime\tNULL\tYES\n\
\n\
(5 rows affected)\n";

#[test]
fn a_name_in_two_schemas_renders_as_two_blocks_not_one_merged_table() {
    let out = render_describe(TWO_SCHEMAS);

    assert!(out.contains("dbo.LeaveRequest (2 columns)"), "{out}");
    assert!(out.contains("hr.LeaveRequest (3 columns)"), "{out}");

    // Each block carries only its own schema's columns - dbo's heading
    // must not have picked up hr's, and vice versa.
    let dbo_block = out.split("hr.LeaveRequest").next().unwrap();
    assert!(dbo_block.contains("LeaveRequestId int not null"), "{out}");
    assert!(dbo_block.contains("Reason nvarchar(200) null"), "{out}");
    assert!(!dbo_block.contains("Approver"), "hr's columns leaked into dbo's block: {out}");
    assert!(!dbo_block.contains("CreatedAt"), "hr's columns leaked into dbo's block: {out}");

    let hr_block = out.split("hr.LeaveRequest").nth(1).unwrap();
    assert!(hr_block.contains("Approver nvarchar(100) null"), "{out}");
    assert!(hr_block.contains("CreatedAt datetime null"), "{out}");
    assert!(!hr_block.contains("Reason"), "dbo's columns leaked into hr's block: {out}");
}
