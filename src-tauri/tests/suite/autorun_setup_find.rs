//! Find rules for saved setups (`autorun::setup_find`): a rule only ever
//! matches records named with the test prefix, and its SQL is a SELECT with
//! `{prefix}` as the only placeholder. The database is faked.

use serde_json::json;
use v2_lib::autorun::setup_find::{
    browser_with_prefix, check_find, choose, find_in_database, first_test_record, sql_with_prefix,
    starts_with_prefix, DbFind, DbOutcome, FindChoice, FindRule, FoundRecord, RowSource,
    ONLY_TEST_RECORDS,
};
use v2_lib::browser::actions::Action;

const GOOD_SQL: &str = "SELECT Id, Name FROM dbo.LeaveType WHERE Name LIKE {prefix} + N'%'";

fn db(sql: &str, id: &str, name: &str) -> DbFind {
    DbFind { sql: sql.to_string(), id_column: id.to_string(), name_column: name.to_string() }
}

fn db_rule(sql: &str, id: &str, name: &str) -> FindRule {
    FindRule { database: Some(db(sql, id, name)), browser: None }
}

fn actions(list: serde_json::Value) -> Vec<Action> {
    serde_json::from_value(list).unwrap()
}

fn said(rule: &FindRule) -> String {
    check_find(rule).join(" | ")
}

#[test]
fn a_database_rule_must_select_with_the_prefix_and_name_its_columns() {
    assert!(check_find(&db_rule(GOOD_SQL, "Id", "Name")).is_empty());

    let no_prefix = db_rule("SELECT Id, Name FROM dbo.LeaveType WHERE Name = N'x'", "Id", "Name");
    assert!(said(&no_prefix).contains(ONLY_TEST_RECORDS));

    assert!(said(&db_rule(GOOD_SQL, "Id", "")).contains(ONLY_TEST_RECORDS));

    let update = db_rule("UPDATE dbo.LeaveType SET x = 1 WHERE Name LIKE {prefix}", "Id", "Name");
    assert!(said(&update).contains("one SELECT statement"));

    assert!(said(&db_rule(GOOD_SQL, "", "Name")).contains("id_column"));

    let other = db_rule(
        "SELECT Id, Name FROM dbo.T WHERE Name LIKE {prefix} AND X = {{x}}",
        "Id",
        "Name",
    );
    assert!(said(&other).contains("no placeholder but {prefix}"));

    assert!(said(&FindRule::default()).contains("needs a find rule"));
}

#[test]
fn the_prefix_inside_quotes_is_refused() {
    let quoted = db_rule("SELECT Id, Name FROM T WHERE Name LIKE '{prefix}%'", "Id", "Name");
    assert!(said(&quoted).contains("outside quotes"));
    assert!(sql_with_prefix("SELECT 1 WHERE a LIKE '{prefix}%'", "AUTOTEST").is_err());
    // A doubled quote keeps the literal open.
    let doubled = db_rule("SELECT Id, Name FROM T WHERE Name = 'it''s {prefix}'", "Id", "Name");
    assert!(said(&doubled).contains("outside quotes"));
}

#[test]
fn the_prefix_goes_in_as_a_quoted_value() {
    assert_eq!(
        sql_with_prefix("SELECT Id, Name FROM T WHERE Name LIKE {prefix} + N'%'", "AUTOTEST"),
        Ok("SELECT Id, Name FROM T WHERE Name LIKE N'AUTOTEST' + N'%'".to_string())
    );
    // A quote in the prefix is doubled, never left to end the literal.
    let out = sql_with_prefix("SELECT Id, Name FROM T WHERE Name LIKE {prefix}", "A'B").unwrap();
    assert!(out.ends_with("N'A''B'"));
    // Not a read: refused.
    assert!(sql_with_prefix("DELETE FROM T WHERE Name LIKE {prefix}", "AUTOTEST").is_err());
}

#[test]
fn a_row_named_outside_the_prefix_is_skipped() {
    let text = "Id\tName\n--\t----\n7\tReal Person\n9\tautotest leave\n\n(2 rows affected)\n";
    assert_eq!(
        first_test_record(text, &db(GOOD_SQL, "Id", "Name"), "AUTOTEST"),
        Ok(Some(FoundRecord { id: Some("9".to_string()), name: "autotest leave".to_string() }))
    );
}

#[test]
fn no_prefixed_row_is_not_found() {
    let text = "Id\tName\n--\t----\n7\tReal Person\n8\tNULL\n\n(2 rows affected)\n";
    assert_eq!(first_test_record(text, &db(GOOD_SQL, "Id", "Name"), "AUTOTEST"), Ok(None));
}

#[test]
fn an_answer_with_no_row_count_or_column_could_not_run() {
    let cut = "Id\tName\n--\t----\n9\tautotest leave\n";
    let err = first_test_record(cut, &db(GOOD_SQL, "Id", "Name"), "AUTOTEST").unwrap_err();
    assert!(err.contains("no row count"));

    let text = "Id\tName\n--\t----\n9\tautotest leave\n\n(1 rows affected)\n";
    let err = first_test_record(text, &db(GOOD_SQL, "Id", "Title"), "AUTOTEST").unwrap_err();
    assert!(err.contains("no column Title"));
}

struct Fake(Result<String, String>);

impl RowSource for Fake {
    async fn select(&self, _sql: &str) -> Result<String, String> {
        self.0.clone()
    }
}

#[tokio::test]
async fn a_database_error_could_not_run() {
    let sentence = "the database is not reachable right now (VPN or network?) - try again";
    let rows = Fake(Err(sentence.to_string()));
    assert_eq!(
        find_in_database(&rows, &db(GOOD_SQL, "Id", "Name"), "AUTOTEST").await,
        DbOutcome::CouldNotRun(sentence.to_string())
    );

    let rows = Fake(Ok("Id\tName\n--\t----\n9\tautotest leave\n\n(1 rows affected)\n".to_string()));
    assert_eq!(
        find_in_database(&rows, &db(GOOD_SQL, "Id", "Name"), "AUTOTEST").await,
        DbOutcome::Found(FoundRecord {
            id: Some("9".to_string()),
            name: "autotest leave".to_string()
        })
    );

    let rows = Fake(Ok("Id\tName\n--\t----\n\n(0 rows affected)\n".to_string()));
    assert_eq!(
        find_in_database(&rows, &db(GOOD_SQL, "Id", "Name"), "AUTOTEST").await,
        DbOutcome::NotFound
    );
}

#[test]
fn the_database_rule_is_preferred_when_it_can_run() {
    let browser = actions(json!([
        { "kind": "expect_visible", "selector": { "role": "cell", "name": "{prefix}" } }
    ]));
    let both = FindRule {
        database: Some(db(GOOD_SQL, "Id", "Name")),
        browser: Some(browser.clone()),
    };
    assert_eq!(choose(&both, true), FindChoice::Database);
    assert_eq!(choose(&both, false), FindChoice::Browser);
    assert_eq!(choose(&db_rule(GOOD_SQL, "Id", "Name"), false), FindChoice::Refused);
    let only_browser = FindRule { database: None, browser: Some(browser) };
    assert_eq!(choose(&only_browser, true), FindChoice::Browser);
}

fn browser_rule(list: serde_json::Value) -> FindRule {
    FindRule { database: None, browser: Some(actions(list)) }
}

#[test]
fn a_browser_rule_ends_in_a_check_that_uses_the_prefix() {
    let click_last = browser_rule(json!([
        { "kind": "click", "selector": "#search" }
    ]));
    assert!(said(&click_last).contains("ends in a check"));

    let no_prefix = browser_rule(json!([
        { "kind": "expect_visible", "selector": { "role": "row", "name": "Leave A" } }
    ]));
    assert!(said(&no_prefix).contains(ONLY_TEST_RECORDS));

    let eleven: Vec<_> = (0..10)
        .map(|_| json!({ "kind": "click", "selector": "#a" }))
        .chain(std::iter::once(
            json!({ "kind": "expect_visible", "selector": { "role": "cell", "name": "{prefix}" } }),
        ))
        .collect();
    assert!(!check_find(&browser_rule(json!(eleven))).is_empty());

    let sign_in = browser_rule(json!([
        { "kind": "sign_in", "account": "kim" },
        { "kind": "expect_visible", "selector": { "role": "cell", "name": "{prefix}" } }
    ]));
    assert!(!check_find(&sign_in).is_empty());

    let good = browser_rule(json!([
        { "kind": "click", "selector": "#search" },
        { "kind": "expect_visible", "selector": { "role": "cell", "name": "{prefix}" } }
    ]));
    assert!(check_find(&good).is_empty());

    let built = browser_with_prefix(good.browser.as_ref().unwrap(), "AUTOTEST").unwrap();
    let last = serde_json::to_value(built.last().unwrap()).unwrap();
    assert_eq!(last["selector"]["name"], "AUTOTEST");
}

#[test]
fn starts_with_prefix_ignores_case_and_spaces() {
    assert!(starts_with_prefix(" autotestX", "AUTOTEST"));
    assert!(!starts_with_prefix("XAUTOTEST", "AUTOTEST"));
}
