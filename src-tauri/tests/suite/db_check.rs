//! The `expect_database` action's pure half: what a check may ask, how its
//! placeholders are bound, how the answer is read and judged.

use serde_json::json;
use v2_lib::autorun::db_check::{
    bind, judge, noted_names, parse, unbound, validate, Expect, Rows, Verdict,
};
use v2_lib::browser::actions::Action;
use v2_lib::db::guard::{classify, Verdict as Guard};

fn act(v: serde_json::Value) -> Action {
    let mut v = v;
    v["kind"] = json!("expect_database");
    serde_json::from_value(v).unwrap()
}

fn rows(columns: &[&str], data: &[&[&str]], capped: bool) -> Rows {
    Rows {
        columns: columns.iter().map(|s| s.to_string()).collect(),
        rows: data.iter().map(|r| r.iter().map(|s| s.to_string()).collect()).collect(),
        capped,
    }
}

fn column(name: &str, equals: &str) -> Expect {
    Expect::Column { name: name.into(), equals: Some(equals.into()), matches: None }
}

// ---- validation ----

#[test]
fn validate_refuses_a_write() {
    assert!(validate("UPDATE t SET a=1", Ok(Expect::Exists(true)), None).is_err());
}

#[test]
fn validate_accepts_a_select() {
    assert!(validate("SELECT 1 FROM t WHERE id = {{noted.req}}", Ok(Expect::Exists(true)), None).is_ok());
}

#[test]
fn validate_refuses_two_expectations() {
    let a = act(json!({"sql": "SELECT 1", "exists": true, "count": 1}));
    assert!(a.validate().is_err());
}

#[test]
fn validate_refuses_no_expectation() {
    let a = act(json!({"sql": "SELECT 1"}));
    assert!(a.validate().is_err());
}

#[test]
fn validate_accepts_each_group() {
    assert!(act(json!({"sql": "SELECT 1", "exists": false})).validate().is_ok());
    assert!(act(json!({"sql": "SELECT 1", "at_least": 2})).validate().is_ok());
    assert!(act(json!({"sql": "SELECT s FROM t ORDER BY id", "column": "s", "equals": "A"})).validate().is_ok());
    assert!(act(json!({"sql": "SELECT 1", "count": 1, "at_most": 2})).validate().is_err());
}

#[test]
fn validate_caps_timeout() {
    assert!(validate("SELECT 1", Ok(Expect::Exists(true)), Some(60_001)).is_err());
    assert!(validate("SELECT 1", Ok(Expect::Exists(true)), Some(60_000)).is_ok());
}

#[test]
fn validate_refuses_quoted_placeholder() {
    let sql = "SELECT 1 FROM t WHERE id = '{{noted.req}}'";
    assert!(validate(sql, Ok(Expect::Exists(true)), None).is_err());
}

#[test]
fn validate_column_needs_order_by() {
    let e = || Ok(column("s", "A"));
    assert!(validate("SELECT s FROM t", e(), None).is_err());
    assert!(validate("SELECT s FROM t WHERE n = 'order by x'", e(), None).is_err());
    assert!(validate("SELECT s FROM t ORDER BY created DESC", e(), None).is_ok());
}

#[test]
fn validate_reads_quotes_and_order_by_without_comments() {
    let e = || Ok(column("s", "A"));
    // An apostrophe in a comment opens no literal.
    let sql = "-- don't
SELECT s FROM t WHERE n = {{noted.req}} ORDER BY id";
    assert!(validate(sql, e(), None).is_ok());
    let block = "/* it's */ SELECT s FROM t WHERE n = {{noted.req}} ORDER BY id";
    assert!(validate(block, e(), None).is_ok());
    // A quote in a literal still counts, and `--` inside one is no comment.
    assert!(validate("SELECT s FROM t WHERE a = '--' AND n = '{{noted.req}}' ORDER BY id", e(), None).is_err());
    // A commented ORDER BY orders nothing.
    assert!(validate("SELECT s FROM t -- order by x", e(), None).is_err());
    assert!(validate("SELECT s FROM t /* ORDER BY x */", e(), None).is_err());
}

#[test]
fn validate_refuses_bad_regex() {
    let e = Expect::Column { name: "s".into(), equals: None, matches: Some("(".into()) };
    assert!(validate("SELECT s FROM t ORDER BY id", Ok(e), None).is_err());
}

#[test]
fn validate_refuses_empty_sql() {
    assert!(validate("  ", Ok(Expect::Exists(true)), None).is_err());
}

// ---- binding ----

#[test]
fn bind_quotes_values() {
    let out = bind("SELECT * FROM r WHERE no = {{noted.req}}", |n| (n == "noted.req").then(|| "LR-0457".into()));
    assert_eq!(out.unwrap(), "SELECT * FROM r WHERE no = N'LR-0457'");
}

#[test]
fn bind_escapes_injection() {
    let out = bind("SELECT * FROM r WHERE no = {{noted.req}}", |_| Some("x'; DROP TABLE r --".into())).unwrap();
    assert_eq!(out, "SELECT * FROM r WHERE no = N'x''; DROP TABLE r --'");
    assert!(matches!(classify(&out), Guard::Read));
}

#[test]
fn bind_leaves_unanswered_and_unbound_lists_them() {
    let out = bind("{{setup.a}} {{noted.b}}", |n| (n == "setup.a").then(|| "1".into())).unwrap();
    assert_eq!(out, "N'1' {{noted.b}}");
    assert_eq!(unbound(&out), vec!["{{noted.b}}".to_string()]);
}

#[test]
fn a_stray_open_brace_pair_does_not_swallow_a_placeholder() {
    let sql = "SELECT '{{' + {{noted.a}}";
    assert_eq!(noted_names(sql), vec!["a"]);
    let out = bind(sql, |n| (n == "noted.a").then(|| "1".into())).unwrap();
    assert_eq!(out, "SELECT '{{' + N'1'");
}

#[test]
fn noted_names_in_order() {
    assert_eq!(noted_names("... {{noted.a}} ... {{setup.x}} {{noted.b}}"), vec!["a", "b"]);
}

// ---- parsing ----

#[test]
fn parse_reads_header_and_rows() {
    let r = parse("status\tid\n------\t--\nApproved\t7\n\n(1 rows affected)\n").unwrap();
    assert_eq!(r.columns, vec!["status", "id"]);
    assert_eq!(r.rows, vec![vec!["Approved".to_string(), "7".to_string()]]);
    assert!(!r.capped);
}

#[test]
fn parse_reads_capped_prefix() {
    let r = parse("rows: 200 (capped)\nid\n--\n1\n2\n").unwrap();
    assert!(r.capped);
    assert_eq!(r.rows.len(), 2);
}

#[test]
fn parse_refuses_text_without_a_count() {
    assert!(parse("garbage").is_err());
}

// ---- judging ----

#[test]
fn judge_column_mismatch_says_expected_and_found() {
    let r = rows(&["status"], &[&["Pending"]], false);
    assert_eq!(
        judge(&column("Status", "Approved"), &r),
        Verdict::Fail("expected status 'Approved', found 'Pending'".into())
    );
}

#[test]
fn judge_column_match_passes() {
    let r = rows(&["status"], &[&["Approved"]], false);
    assert!(matches!(judge(&column("STATUS", "Approved"), &r), Verdict::Pass(_)));
}

#[test]
fn judge_null_is_found_as_null() {
    let r = rows(&["status"], &[&["NULL"]], false);
    assert_eq!(
        judge(&column("Status", "Approved"), &r),
        Verdict::Fail("expected status 'Approved', found NULL".into())
    );
}

#[test]
fn judge_missing_column_names_the_others() {
    let r = rows(&["a", "b"], &[&["1", "2"]], false);
    match judge(&column("zzz", "x"), &r) {
        Verdict::Fail(d) => {
            assert!(d.contains("no column named"), "{d}");
            assert!(d.contains("a, b"), "{d}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn judge_no_row_counts() {
    let r = rows(&["id"], &[&["1"]], false);
    assert_eq!(judge(&Expect::Exists(false), &r), Verdict::Fail("expected no row, found 1".into()));
    assert!(matches!(judge(&Expect::Exists(true), &r), Verdict::Pass(_)));
}

#[test]
fn judge_count() {
    let r = rows(&["id"], &[], false);
    let want = Expect::Count { equals: Some(2), at_least: None, at_most: None };
    assert_eq!(judge(&want, &r), Verdict::Fail("expected 2 rows, found 0".into()));
}

#[test]
fn judge_count_on_capped_fails_with_sentence() {
    let r = rows(&["id"], &[&["1"], &["2"]], true);
    let want = Expect::Count { equals: Some(2), at_least: None, at_most: None };
    match judge(&want, &r) {
        Verdict::Fail(d) => assert!(d.contains("COUNT(*)"), "{d}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn judge_count_at_most_on_capped_says_more_than_found() {
    let r = rows(&["id"], &[&["1"], &["2"], &["3"]], true);
    let want = Expect::Count { equals: None, at_least: None, at_most: Some(2) };
    assert_eq!(judge(&want, &r), Verdict::Fail("expected at most 2 rows, found more than 3".into()));
    // Too few kept rows cannot show the limit holds: the capped sentence.
    let few = rows(&["id"], &[&["1"]], true);
    match judge(&want, &few) {
        Verdict::Fail(d) => assert!(d.contains("COUNT(*)"), "{d}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn judge_column_names_match_in_any_case() {
    let r = rows(&["STATUS"], &[&["Approved"]], false);
    assert!(matches!(judge(&column("status", "Approved"), &r), Verdict::Pass(_)));
}

#[test]
fn judge_secret_column_hidden() {
    let r = rows(&["password_hash"], &[&["xyz"]], false);
    match judge(&column("password_hash", "abc"), &r) {
        Verdict::Fail(d) => {
            assert!(d.contains("the hidden value"), "{d}");
            assert!(!d.contains("abc") && !d.contains("xyz"), "{d}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn judge_truncated_cell_compares_as_printed() {
    let r = rows(&["note"], &[&["a long text cut off..."]], false);
    assert!(matches!(judge(&column("note", "a long text cut off..."), &r), Verdict::Pass(_)));
}

#[test]
fn judge_the_full_value_does_not_match_a_truncated_cell() {
    let r = rows(&["note"], &[&["a long text cut off..."]], false);
    assert_eq!(
        judge(&column("note", "a long text cut off and more"), &r),
        Verdict::Fail("expected note 'a long text cut off and more', found 'a long text cut off...'".into())
    );
}

#[test]
fn judge_matches_uses_the_pattern() {
    let r = rows(&["no"], &[&["LR-0457"]], false);
    let e = Expect::Column { name: "no".into(), equals: None, matches: Some(r"^LR-\d+$".into()) };
    assert!(matches!(judge(&e, &r), Verdict::Pass(_)));
}
