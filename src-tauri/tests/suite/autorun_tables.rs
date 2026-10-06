//! Table and grid checks: the pure judging in `browser::table` (columns,
//! rows, order, counts), and the four actions against a fake page whose
//! reader answers in turn - a grid still loading, then filled.
//! `browser_live` reads a real HTML table and a real ARIA grid.

use crate::common;

use common::{FakePage, ScriptedDriver};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use v2_lib::autorun::patterns::{action_target, classify, ErrorClass};
use v2_lib::autorun::report::action_words;
use v2_lib::browser::actions::{execute_with, Action};
use v2_lib::browser::table::{
    check_sorted, count_check, find_row, parse_number, row_count, RowCount, SortAs, SortKind, SortOrder, TableRead,
    ONE_COUNT, READ_TABLE_JS,
};
use v2_lib::browser::timing::Timing;

fn quick() -> Timing {
    Timing { action_ms: 300, expect_ms: 300, nav_ms: 300, poll_ms: 10, highlight_ms: 0, lease_wait_ms: 300 }
}

fn action(v: Value) -> Action {
    serde_json::from_value(v).expect("an action")
}

fn table(headers: &[&str], rows: &[&[&str]]) -> TableRead {
    TableRead::from_value(&json!({ "headers": headers, "rows": rows })).unwrap()
}

fn cells(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs.iter().map(|(c, t)| (c.to_string(), t.to_string())).collect()
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| v.to_string()).collect()
}

const TEXT: SortAs = SortAs::Kind(SortKind::Text);
const NUMBER: SortAs = SortAs::Kind(SortKind::Number);
const DATE: SortAs = SortAs::Kind(SortKind::Date);

fn people() -> TableRead {
    table(
        &["Name", "Status", "Joined"],
        &[&["Ann Lee", "Active", "2026-01-05"], &["Ben Ray", "Inactive", "2025-11-30"], &["Cy  Wu", "Active", ""]],
    )
}

// ---- Reading ---------------------------------------------------------------------

#[test]
fn a_short_row_is_padded_and_a_non_table_reads_as_none() {
    let t = TableRead::from_value(&json!({ "headers": ["A", "B", "C"], "rows": [["1"], ["1", "2", "3", "4"]] })).unwrap();
    assert_eq!(t.rows[0], ["1", "", ""]);
    assert_eq!(t.rows[1].len(), 4, "a longer row keeps its cells");
    assert!(TableRead::from_value(&Value::Null).is_none());
}

#[test]
fn the_reader_knows_both_kinds_and_how_each_finds_its_parts() {
    let js = READ_TABLE_JS;
    for part in [
        "this.tagName === 'TABLE'",
        "this.tHead",
        "c.tagName === 'TH'",
        "tfoot",
        "'grid'",
        "'treegrid'",
        "'table'",
        "[role=columnheader]",
        "[role=row]",
        "[role=gridcell],[role=cell],[role=rowheader]",
        "aria-hidden=true",
        "innerText",
        "replace(/\\s+/g, ' ').trim()",
        "return null",
    ] {
        assert!(js.contains(part), "the reader never mentions {part}");
    }
}

#[test]
fn columns_are_found_by_header_ignoring_case_and_an_unknown_one_lists_them() {
    let t = people();
    assert_eq!(t.column(" status "), Ok(1));
    assert_eq!(
        t.column("Grade"),
        Err("the table has no column \"Grade\" - its columns are \"Name\", \"Status\", \"Joined\"".to_string())
    );
    let headless = table(&[], &[&["1", "2"]]);
    assert_eq!(
        headless.column("A"),
        Err("the table has no column \"A\" - its columns are none - it has no header row".to_string())
    );
}

// ---- Rows --------------------------------------------------------------------------

#[test]
fn a_row_matches_by_contains_ignoring_case_or_exactly() {
    let t = people();
    assert_eq!(find_row(&t, &cells(&[("status", "ACT")]), false), Ok(Some(0)));
    assert_eq!(find_row(&t, &cells(&[("Status", "act"), ("Name", "ben")]), false), Ok(Some(1)), "Inactive contains act");
    assert_eq!(find_row(&t, &cells(&[("Status", "Act")]), true), Ok(None));
    assert_eq!(find_row(&t, &cells(&[("Status", "active"), ("Name", "ann lee")]), true), Ok(Some(0)));
    // The cell's spaces were collapsed by the reader; the script's are too.
    assert_eq!(find_row(&t, &cells(&[("Name", "Cy  Wu")]), true), Ok(Some(2)));
    // An exact empty text finds the empty cell.
    assert_eq!(find_row(&t, &cells(&[("Joined", "")]), true), Ok(Some(2)));
    assert!(find_row(&t, &cells(&[("Grade", "A")]), false).unwrap_err().starts_with("the table has no column"));
}

// ---- Order -------------------------------------------------------------------------

#[test]
fn text_sorts_ignoring_case_and_passes_over_blank_cells() {
    let v = strings(&["apple", "", "Banana", "cherry"]);
    assert!(check_sorted("Fruit", &v, SortOrder::Ascending, &TEXT).is_ok());
    assert_eq!(
        check_sorted("Fruit", &v, SortOrder::Descending, &TEXT),
        Err("Fruit is not in descending order - row 1 \"apple\" comes before row 3 \"Banana\"".to_string())
    );
    let v = strings(&["b", "a"]);
    assert_eq!(
        check_sorted("Name", &v, SortOrder::Ascending, &TEXT),
        Err("Name is not in ascending order - row 1 \"b\" comes before row 2 \"a\"".to_string())
    );
}

#[test]
fn numbers_read_thousands_separators_negatives_and_decimals() {
    assert_eq!(parse_number("1,234"), Some(1234.0));
    assert_eq!(parse_number("-5"), Some(-5.0));
    assert_eq!(parse_number("12,345,678.50"), Some(12_345_678.5));
    for bad in ["1,23", "12a", "", "--1", "1.", "1,2345"] {
        assert_eq!(parse_number(bad), None, "{bad}");
    }
    // As text "10" would sort before "9"; as numbers it does not.
    assert!(check_sorted("Amount", &strings(&["-5", "9", "10", "1,234"]), SortOrder::Ascending, &NUMBER).is_ok());
    assert!(check_sorted("Amount", &strings(&["-5", "9", "10", "1,234"]), SortOrder::Ascending, &TEXT).is_err());
    assert_eq!(
        check_sorted("Amount", &strings(&["1", "two"]), SortOrder::Ascending, &NUMBER),
        Err("\"two\" in Amount is not a number".to_string())
    );
    assert_eq!(
        check_sorted("Amount", &strings(&["10", "", "9"]), SortOrder::Descending, &NUMBER),
        Ok("Amount is in descending order (2 values)".to_string())
    );
}

#[test]
fn numbers_read_amounts_and_percentages() {
    assert_eq!(parse_number("40%"), Some(40.0));
    assert_eq!(parse_number("12.5 %"), Some(12.5));
    assert_eq!(parse_number("LKR 1,250.50"), Some(1250.5));
    assert_eq!(parse_number("lkr1,250"), Some(1250.0));
    assert_eq!(parse_number("-$5"), Some(-5.0));
    assert_eq!(parse_number("$-5"), Some(-5.0));
    assert_eq!(parse_number("- $ 5"), Some(-5.0));
    assert_eq!(parse_number("Rs. 900"), Some(900.0));
    assert_eq!(parse_number("rs900"), Some(900.0));
    assert_eq!(parse_number("\u{a3}3"), Some(3.0));
    assert_eq!(parse_number("\u{20ac} 1,000.25"), Some(1000.25));
    for bad in ["(5)", "$$5", "5%%", "Rsx5", "USD 5", "-$-5", "$", "%", "LKR"] {
        assert_eq!(parse_number(bad), None, "{bad}");
    }
    // A column may mix them.
    let mixed = strings(&["-$5", "40%", "Rs. 99", "LKR 1,250.50"]);
    assert!(check_sorted("Amount", &mixed, SortOrder::Ascending, &NUMBER).is_ok());
}

/// Page text in a sentence - a cell, a header - is cut to 200 characters,
/// as a dialog's message is.
#[test]
fn page_text_in_a_sentence_is_cut_to_200_characters() {
    let long = "z".repeat(300);
    let cut = "z".repeat(200);
    assert_eq!(
        check_sorted("Name", &strings(&[&long, "a"]), SortOrder::Ascending, &TEXT),
        Err(format!("Name is not in ascending order - row 1 \"{cut}\" comes before row 2 \"a\""))
    );
    assert_eq!(
        check_sorted("Amount", &strings(&[&long]), SortOrder::Ascending, &NUMBER),
        Err(format!("\"{cut}\" in Amount is not a number"))
    );
    let t = table(&[&long, "B"], &[]);
    assert_eq!(t.column("C"), Err(format!("the table has no column \"C\" - its columns are \"{cut}\", \"B\"")));
}

#[test]
fn dates_read_each_format_and_mixed_ones() {
    let ok = |values: &[&str], order| check_sorted("Joined", &strings(values), order, &DATE);
    assert!(ok(&["2025-11-30", "2026-01-05"], SortOrder::Ascending).is_ok());
    assert!(ok(&["5 Mar 2026", "12 mar 2026", "1 Apr 2026"], SortOrder::Ascending).is_ok());
    // 25/12 can only be day-first: the column is read so.
    assert!(ok(&["01/02/2026", "25/12/2026"], SortOrder::Ascending).is_ok());
    // 12/25 can only be month-first.
    assert!(ok(&["12/25/2026", "01/02/2027"], SortOrder::Ascending).is_ok());
    assert!(ok(&["2026-01-01", "5 Feb 2026", "20/03/2026"], SortOrder::Ascending).is_ok());
    assert_eq!(
        ok(&["2026-01-01", "soon"], SortOrder::Ascending),
        Err("\"soon\" in Joined is not a date".to_string())
    );
    assert_eq!(
        ok(&["2026-02-30"], SortOrder::Ascending),
        Err("\"2026-02-30\" in Joined is not a date".to_string())
    );
    assert_eq!(
        ok(&["2026-03-01", "2026-01-01"], SortOrder::Ascending),
        Err("Joined is not in ascending order - row 1 \"2026-03-01\" comes before row 2 \"2026-01-01\"".to_string())
    );
}

#[test]
fn ambiguous_dates_are_refused_until_a_format_is_given() {
    // Day-first: 1 Feb, 3 Jan (descending). Month-first: 2 Jan, 4 Mar... the
    // two readings disagree on the order.
    let v = strings(&["01/02/2026", "03/01/2026"]);
    assert_eq!(
        check_sorted("Joined", &v, SortOrder::Ascending, &DATE),
        Err("the dates in Joined could be read two ways - give as: \"date\" a format".to_string())
    );
    let dmy = SortAs::Format { date: "dd/MM/yyyy".into() };
    let mdy = SortAs::Format { date: "MM/dd/yyyy".into() };
    assert!(check_sorted("Joined", &v, SortOrder::Descending, &dmy).is_ok(), "1 Feb comes after 3 Jan");
    assert!(check_sorted("Joined", &v, SortOrder::Ascending, &mdy).is_ok(), "2 Jan comes before 1 Mar");
    // Both readings, one order: not ambiguous.
    let same = strings(&["01/01/2026", "02/02/2026"]);
    assert!(check_sorted("Joined", &same, SortOrder::Ascending, &DATE).is_ok());
    // A value the given format does not read.
    assert_eq!(
        check_sorted("Joined", &strings(&["2026-01-01"]), SortOrder::Ascending, &dmy),
        Err("\"2026-01-01\" in Joined is not a date".to_string())
    );
    let named = SortAs::Format { date: "d MMM yyyy".into() };
    assert!(check_sorted("Joined", &strings(&["9 Jan 2026", "10 Jan 2026"]), SortOrder::Ascending, &named).is_ok());
}

// ---- Counts ------------------------------------------------------------------------

#[test]
fn every_row_count_form_and_its_refusal() {
    assert_eq!(count_check(5, RowCount::Equals(5)), Ok("the table has 5 rows".to_string()));
    assert_eq!(count_check(3, RowCount::Equals(5)), Err("the table has 3 rows, not 5".to_string()));
    assert_eq!(count_check(3, RowCount::AtLeast(5)), Err("the table has 3 rows, not at least 5".to_string()));
    assert!(count_check(5, RowCount::AtLeast(5)).is_ok());
    assert_eq!(count_check(3, RowCount::AtMost(2)), Err("the table has 3 rows, not at most 2".to_string()));
    assert!(count_check(0, RowCount::AtMost(2)).is_ok());
    assert_eq!(row_count(Some(1), None, None), Ok(RowCount::Equals(1)));
    assert_eq!(row_count(None, None, None), Err(ONE_COUNT.to_string()));
    assert_eq!(row_count(Some(1), Some(1), None), Err(ONE_COUNT.to_string()));
    assert_eq!(ONE_COUNT, "expect_row_count takes one of equals, at_least or at_most");
    let refused = action(json!({ "kind": "expect_row_count", "table": "#t", "equals": 1, "at_most": 3 })).validate();
    assert_eq!(refused, Err(ONE_COUNT.to_string()));
    let refused = action(json!({ "kind": "expect_row_count", "table": "#t" })).validate();
    assert_eq!(refused, Err(ONE_COUNT.to_string()));
}

// ---- What a script may say ---------------------------------------------------------

#[test]
fn each_refusal_and_round_trip() {
    let refused = |v: Value| action(v).validate().unwrap_err();
    assert_eq!(refused(json!({ "kind": "expect_row", "table": "#t", "cells": {} })), "expect_row needs at least one cell");
    assert_eq!(refused(json!({ "kind": "expect_no_row", "table": "#t", "cells": {} })), "expect_no_row needs at least one cell");
    assert!(refused(json!({ "kind": "expect_row", "table": "#t", "cells": { "A": " " } })).contains("holds for any cell"));
    assert!(action(json!({ "kind": "expect_row", "table": "#t", "cells": { "A": "" }, "exact": true })).validate().is_ok());
    assert_eq!(refused(json!({ "kind": "expect_sorted", "table": "#t", "column": " ", "order": "ascending" })), "expect_sorted needs a column");
    assert!(refused(json!({ "kind": "expect_sorted", "table": "#t", "column": "A", "order": "ascending", "as": { "date": "yy-M" } }))
        .contains("is not a date format"));
    assert!(serde_json::from_value::<Action>(json!({ "kind": "expect_row", "table": "#t", "cells": { "A": 3 } })).is_err());
    assert!(serde_json::from_value::<Action>(json!({ "kind": "expect_sorted", "table": "#t", "column": "A", "order": "up" })).is_err());
    for text in [
        r##"{"kind":"expect_row","table":{"role":"grid","name":"People"},"cells":{"Status":"Active","Name":"Ann"}}"##,
        r##"{"kind":"expect_no_row","table":"#t","cells":{"Name":"Ann"},"exact":true,"timeout_ms":5000}"##,
        r##"{"kind":"expect_sorted","table":"#t","column":"Joined","order":"descending","as":"date"}"##,
        r##"{"kind":"expect_sorted","table":"#t","column":"Joined","order":"ascending","as":{"date":"dd/MM/yyyy"}}"##,
        r##"{"kind":"expect_row_count","table":[{"css":"iframe"},{"css":"table"}],"at_least":1}"##,
    ] {
        let read: Action = serde_json::from_str(text).unwrap();
        assert!(read.validate().is_ok(), "{text}");
        assert!(read.is_check());
        assert_eq!(serde_json::to_string(&read).unwrap(), text, "the cells keep the script's order");
    }
}

#[test]
fn reports_patterns_and_tries_have_words() {
    let row = action(json!({ "kind": "expect_row", "table": "#t", "cells": { "Status": "Active", "Name": "Ann" } }));
    assert_eq!(action_words(&row), "expect a row in #t with Status \"Active\", Name \"Ann\"");
    assert_eq!(action_target(&row).as_deref(), Some("#t"));
    assert_eq!(v2_lib::ai_bridge::describe_try(&row, true), "AI tried expect_row #t in the supervised browser: ok");
    let sorted = action(json!({ "kind": "expect_sorted", "table": "#t", "column": "Joined", "order": "ascending" }));
    assert_eq!(action_words(&sorted), "expect #t sorted by Joined, ascending");
    let count = action(json!({ "kind": "expect_row_count", "table": "#t", "at_most": 3 }));
    assert_eq!(action_words(&count), "expect #t to have at most 3 rows");
    assert_eq!(classify("no row has Name \"Ann\" - the table has 3 rows", None), ErrorClass::TextMismatch);
    assert_eq!(classify("the table has 3 rows, not 5", None), ErrorClass::CountMismatch);
    assert_eq!(classify("the table has no column \"A\" - its columns are \"B\"", None), ErrorClass::NotFound);
    assert_eq!(classify("Name is not in ascending order - row 1 \"b\" comes before row 2 \"a\"", None), ErrorClass::TextMismatch);
}

// ---- Against a page ----------------------------------------------------------------

/// A page whose one table answers the reader with `reads` in turn, the last
/// repeating; `reads` counts the looks.
fn page_with(reads: Vec<Value>) -> (ScriptedDriver, Arc<Mutex<usize>>) {
    let fake = FakePage::default();
    let n = Arc::new(Mutex::new(0usize));
    let seen = n.clone();
    let d = ScriptedDriver::new(move |method, params| {
        if method == "Runtime.callFunctionOn" && params["functionDeclaration"] == READ_TABLE_JS {
            let mut i = seen.lock().unwrap();
            let v = reads[(*i).min(reads.len() - 1)].clone();
            *i += 1;
            return Ok(json!({ "result": { "value": v } }));
        }
        fake.answer(method, params)
    });
    (d, n)
}

fn loaded() -> Value {
    json!({ "headers": ["Name", "Status"], "rows": [["Ann", "Active"], ["Ben", "Inactive"]] })
}

#[tokio::test]
async fn a_grid_still_loading_is_read_again_until_it_holds() {
    let empty = json!({ "headers": ["Name", "Status"], "rows": [] });
    let (mut d, looks) = page_with(vec![empty.clone(), empty, loaded()]);
    let out = execute_with(
        &mut d,
        &action(json!({ "kind": "expect_row", "table": { "css": "#people" }, "cells": { "Name": "ben" } })),
        &quick(),
    )
    .await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(out.detail, "row 2 has Name \"ben\"");
    assert_eq!(*looks.lock().unwrap(), 3, "each look read the table again");
    assert!(d.deadline_was_cleared());

    let (mut d, _) = page_with(vec![json!({ "headers": ["Name"], "rows": [] }), loaded()]);
    let out = execute_with(&mut d, &action(json!({ "kind": "expect_row_count", "table": "#people", "equals": 2 })), &quick()).await;
    assert!(out.ok, "{}", out.detail);
}

/// Only the last look's failure is said, in the check's own words.
#[tokio::test]
async fn a_check_that_never_holds_says_the_last_look() {
    let (mut d, looks) = page_with(vec![json!({ "headers": ["Name"], "rows": [] }), loaded()]);
    let out = execute_with(
        &mut d,
        &action(json!({ "kind": "expect_row", "table": "#people", "cells": { "Name": "Cy" }, "timeout_ms": 60 })),
        &quick(),
    )
    .await;
    assert!(!out.ok);
    assert_eq!(out.detail, "no row has Name \"Cy\" - the table has 2 rows");
    assert!(*looks.lock().unwrap() >= 2);

    let (mut d, _) = page_with(vec![loaded()]);
    let out = execute_with(
        &mut d,
        &action(json!({ "kind": "expect_no_row", "table": "#people", "cells": { "Status": "inactive" }, "exact": true, "timeout_ms": 30 })),
        &quick(),
    )
    .await;
    assert_eq!(out.detail, "a row has Status \"inactive\" (row 2)");

    let (mut d, _) = page_with(vec![loaded()]);
    let out = execute_with(
        &mut d,
        &action(json!({ "kind": "expect_sorted", "table": "#people", "column": "Name", "order": "descending", "timeout_ms": 30 })),
        &quick(),
    )
    .await;
    assert_eq!(out.detail, "Name is not in descending order - row 1 \"Ann\" comes before row 2 \"Ben\"");

    let (mut d, _) = page_with(vec![loaded()]);
    let out = execute_with(
        &mut d,
        &action(json!({ "kind": "expect_sorted", "table": "#people", "column": "Joined", "order": "descending", "timeout_ms": 30 })),
        &quick(),
    )
    .await;
    assert_eq!(out.detail, "the table has no column \"Joined\" - its columns are \"Name\", \"Status\"");
}

/// A page whose table reads as `read(ms since the page was built)`.
fn timed_page(read: fn(u128) -> Value) -> ScriptedDriver {
    let fake = FakePage::default();
    let born = std::time::Instant::now();
    ScriptedDriver::new(move |method, params| {
        if method == "Runtime.callFunctionOn" && params["functionDeclaration"] == READ_TABLE_JS {
            return Ok(json!({ "result": { "value": read(born.elapsed().as_millis()) } }));
        }
        fake.answer(method, params)
    })
}

/// Headers shown, no rows for 500 ms, then Ann's row: the grid was still
/// loading, so a check that her row is absent must not pass on the empty
/// reads.
fn ann_after_500(ms: u128) -> Value {
    if ms < 500 {
        json!({ "headers": ["Name", "Status"], "rows": [] })
    } else {
        json!({ "headers": ["Name", "Status"], "rows": [["Ann", "Active"]] })
    }
}

#[tokio::test]
async fn a_loading_grid_does_not_pass_a_check_that_a_row_is_absent() {
    let mut d = timed_page(ann_after_500);
    let out = execute_with(
        &mut d,
        &action(json!({ "kind": "expect_no_row", "table": "#people", "cells": { "Name": "Ann" }, "timeout_ms": 2000 })),
        &quick(),
    )
    .await;
    assert!(!out.ok, "passed on a grid still loading: {}", out.detail);
    assert_eq!(out.detail, "a row has Name \"Ann\" (row 1)");

    let mut d = timed_page(ann_after_500);
    let out = execute_with(&mut d, &action(json!({ "kind": "expect_row_count", "table": "#people", "equals": 0, "timeout_ms": 2000 })), &quick()).await;
    assert_eq!(out.detail, "the table has 1 rows, not 0");
    let mut d = timed_page(ann_after_500);
    let out = execute_with(&mut d, &action(json!({ "kind": "expect_row_count", "table": "#people", "at_most": 0, "timeout_ms": 2000 })), &quick()).await;
    assert!(!out.ok, "{}", out.detail);

    // A positive check still passes as soon as it holds.
    let mut d = timed_page(ann_after_500);
    let began = std::time::Instant::now();
    let out = execute_with(&mut d, &action(json!({ "kind": "expect_row_count", "table": "#people", "at_least": 0, "timeout_ms": 2000 })), &quick()).await;
    assert!(out.ok && began.elapsed().as_millis() < 400, "{} after {:?}", out.detail, began.elapsed());
}

#[tokio::test]
async fn a_table_that_holds_still_passes_a_check_that_a_row_is_absent() {
    fn empty(_: u128) -> Value {
        json!({ "headers": ["Name", "Status"], "rows": [] })
    }
    for check in [
        json!({ "kind": "expect_no_row", "table": "#people", "cells": { "Name": "Ann" }, "timeout_ms": 2000 }),
        json!({ "kind": "expect_row_count", "table": "#people", "equals": 0, "timeout_ms": 2000 }),
        json!({ "kind": "expect_row_count", "table": "#people", "at_most": 3, "timeout_ms": 2000 }),
    ] {
        let mut d = timed_page(empty);
        let began = std::time::Instant::now();
        let out = execute_with(&mut d, &action(check.clone()), &quick()).await;
        let took = began.elapsed().as_millis();
        assert!(out.ok, "{check}: {}", out.detail);
        assert!((750..2000).contains(&took), "{check}: passed after {took} ms - it must hold still for 750 ms first");
    }
}

/// Still changing when time runs out: the last read is judged.
#[tokio::test]
async fn a_table_still_changing_at_the_timeout_is_judged_by_its_last_read() {
    fn changing(ms: u128) -> Value {
        json!({ "headers": ["Name", format!("Status {}", ms / 100)], "rows": [] })
    }
    let mut d = timed_page(changing);
    let out = execute_with(
        &mut d,
        &action(json!({ "kind": "expect_no_row", "table": "#people", "cells": { "Name": "Ann" }, "timeout_ms": 600 })),
        &quick(),
    )
    .await;
    assert!(out.ok, "{}", out.detail);
    assert_eq!(out.detail, "no row has Name \"Ann\" - the table has 0 rows");
}

#[tokio::test]
async fn something_that_is_not_a_table_says_so() {
    let (mut d, _) = page_with(vec![Value::Null]);
    let out = execute_with(&mut d, &action(json!({ "kind": "expect_row_count", "table": { "css": "#menu" }, "at_least": 1, "timeout_ms": 30 })), &quick()).await;
    assert_eq!(out.detail, "#menu is not a table or grid");
    // No table at all: said as any locator says it.
    let mut d = FakePage { found: 0, ..FakePage::default() }.driver();
    let out = execute_with(&mut d, &action(json!({ "kind": "expect_row_count", "table": { "css": "#nope" }, "at_least": 1, "timeout_ms": 30 })), &quick()).await;
    assert_eq!(out.detail, "waited 30ms: #nope is not on the page");
}

/// A non-breaking space, a figure space, a narrow non-breaking space and a
/// tab: what a page or a pasted name may hold where a space is meant.
const ODD_SPACES: [&str; 4] = ["\u{a0}", "\u{2007}", "\u{202f}", "\t"];

/// A column name and a cell's text with any Unicode space match the same
/// with a space, whichever side holds it.
#[test]
fn table_headers_and_cells_treat_a_unicode_space_as_a_space() {
    for sp in ODD_SPACES {
        let odd = |s: &str| s.replace(' ', sp);
        // The page's side holds the odd space.
        let read = table(&[&odd("Employee Name"), "Status"], &[&[&odd("Ann  Lee"), "Active"]]);
        assert_eq!(read.column("employee name"), Ok(0), "{sp:?}");
        assert_eq!(find_row(&read, &cells(&[("Employee Name", "Ann Lee")]), true), Ok(Some(0)), "{sp:?}");
        // The script's side holds it.
        let read = table(&["Employee Name", "Status"], &[&["Ann Lee", "Active"]]);
        assert_eq!(read.column(&odd(" Employee Name ")), Ok(0), "{sp:?}");
        assert_eq!(find_row(&read, &cells(&[(&odd("Employee Name"), &odd("ann lee"))]), true), Ok(Some(0)), "{sp:?}");
        assert_eq!(find_row(&read, &cells(&[("Employee Name", &odd("n L"))]), false), Ok(Some(0)), "{sp:?}");
    }
}
