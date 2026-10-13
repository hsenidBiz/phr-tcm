//! Playwright export of the table checks: `expect_row`, `expect_no_row`,
//! `expect_sorted` and `expect_row_count` poll the spec's `tables` helper,
//! which reads the table with the app's own reader and judges the read with
//! a TypeScript port of `browser::table`. The port is run through node on
//! the same inputs as the Rust functions, and must agree with them.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::process::Command;
use v2_lib::autorun::recipe::RecipeStep;
use v2_lib::autorun::CaseScript;
use v2_lib::browser::table::{
    check_sorted, find_row, no_row_check, parse_number, row_check, SortAs, SortOrder, TableCheck, TableRead,
    READ_TABLE_JS, SETTLE_MS, SETTLE_POLL_MS,
};
use v2_lib::browser::expect::NOT_ON_PAGE;
use v2_lib::browser::locator::Target;
use v2_lib::pw_export::table_ts::{read_table_ts, table_logic_ts, tables_ts};
use v2_lib::pw_export::translate::{check, raw_spec, RawSpecInput};

fn origins() -> Vec<String> {
    vec!["https://app.example".into()]
}

fn script(steps: Value) -> CaseScript {
    serde_json::from_value(json!({ "case_id": 1, "title": "A case", "steps": steps })).unwrap()
}

fn spec_with(script: &CaseScript, signin: &[RecipeStep]) -> Result<String, String> {
    let area: Vec<Target> = vec![serde_json::from_value(json!({ "css": "a.area" })).unwrap()];
    raw_spec(&RawSpecInput {
        script,
        md_path: "suites/sl/admin/m/f/test-cases/f.md".into(),
        feature_title: "F".into(),
        after_sign_in: signin,
        area_clicks: &area,
        area_clicks_by_name: &BTreeMap::new(),
        step_texts: &BTreeMap::new(),
        origins: &origins(),
    })
    .map_err(|e| e.0)
}

fn spec(actions: Value) -> Result<String, String> {
    spec_with(&script(json!([{ "step_number": 1, "actions": actions }])), &[])
}

#[test]
fn a_script_with_every_table_check_now_exports() {
    let s = script(json!([{ "step_number": 1, "actions": [
        { "kind": "expect_row", "table": "#t", "cells": { "A": "b" } },
        { "kind": "expect_no_row", "table": "#t", "cells": { "A": "b" } },
        { "kind": "expect_sorted", "table": "#t", "column": "A", "order": "ascending" },
        { "kind": "expect_row_count", "table": "#t", "equals": 1 }
    ]}]));
    assert_eq!(check(&s, &origins()), Ok(()));
    let got = spec_with(&s, &[]).unwrap();
    assert!(!got.contains("has no table-row check") && !got.contains("cannot be exported"), "{got}");
    let checks = got
        .lines()
        .map(str::trim_start)
        .filter(|l| l.starts_with("await expect.poll(() => tables.") || l.starts_with("expect(await tables."))
        .count();
    assert_eq!(checks, 4, "{got}");
}

#[test]
fn row_checks_poll_until_the_helper_says_null() {
    let got = spec(json!([
        { "kind": "expect_row", "table": { "role": "grid", "name": "People" }, "cells": { "Status": "Active", "Name": "Ann's" } },
        { "kind": "expect_row", "table": "#t", "cells": { "Joined": "" }, "exact": true, "timeout_ms": 900 },
        { "kind": "expect_no_row", "table": "#t", "cells": { "Name": "Ben" }, "timeout_ms": 1500 },
        { "kind": "expect_no_row", "table": "#t", "cells": { "Name": "Ben" }, "exact": true }
    ]))
    .unwrap();
    // The cells keep the script's order; every string goes through lit.
    assert!(
        got.contains("    await expect.poll(() => tables.row(cur.getByRole('grid', { name: 'People' }).filter({ visible: true }).first(), [['Status', 'Active'], ['Name', 'Ann\\'s']], false), { timeout: 10000 }).toBeNull();\n"),
        "{got}"
    );
    assert!(got.contains("tables.row(cur.locator('#t').first(), [['Joined', '']], true), { timeout: 900 }).toBeNull();"), "{got}");
    // An absence is not a poll: the helper runs the app's settling loop once, with the step's timeout.
    assert!(got.contains("    expect(await tables.noRow(cur.locator('#t').first(), [['Name', 'Ben']], false, 1500)).toBeNull();\n"), "{got}");
    assert!(got.contains("    expect(await tables.noRow(cur.locator('#t').first(), [['Name', 'Ben']], true, 10000)).toBeNull();\n"), "{got}");
}

#[test]
fn sorted_says_each_order_and_each_way_of_reading() {
    let got = spec(json!([
        { "kind": "expect_sorted", "table": "#t", "column": "Name", "order": "ascending" },
        { "kind": "expect_sorted", "table": "#t", "column": "Name", "order": "descending", "as": "text" },
        { "kind": "expect_sorted", "table": "#t", "column": "Amount", "order": "ascending", "as": "number" },
        { "kind": "expect_sorted", "table": "#t", "column": "Joined", "order": "descending", "as": "date", "timeout_ms": 700 },
        { "kind": "expect_sorted", "table": "#t", "column": "Joined", "order": "ascending", "as": { "date": "dd/MM/yyyy" } }
    ]))
    .unwrap();
    let l = |rest: &str| format!("await expect.poll(() => tables.sorted(cur.locator('#t').first(), {rest}).toBeNull();");
    assert!(got.contains(&l("'Name', 'ascending', 'text'), { timeout: 10000 }")), "no as is text: {got}");
    assert!(got.contains(&l("'Name', 'descending', 'text'), { timeout: 10000 }")), "{got}");
    assert!(got.contains(&l("'Amount', 'ascending', 'number'), { timeout: 10000 }")), "{got}");
    assert!(got.contains(&l("'Joined', 'descending', 'date'), { timeout: 700 }")), "{got}");
    assert!(got.contains(&l("'Joined', 'ascending', { date: 'dd/MM/yyyy' }), { timeout: 10000 }")), "{got}");
}

#[test]
fn row_counts_poll_the_count_and_settle_where_the_app_does() {
    let got = spec(json!([
        { "kind": "expect_row_count", "table": "#t", "equals": 3 },
        { "kind": "expect_row_count", "table": "#t", "equals": 0, "timeout_ms": 2000 },
        { "kind": "expect_row_count", "table": "#t", "at_least": 1 },
        { "kind": "expect_row_count", "table": "#t", "at_most": 5, "timeout_ms": 400 }
    ]))
    .unwrap();
    assert!(got.contains("    await expect.poll(() => tables.rows(cur.locator('#t').first()), { timeout: 10000 }).toBe(3);\n"), "{got}");
    // A count that passes on too few rows waits for the table to hold still.
    assert!(got.contains("    expect(await tables.stillRows(cur.locator('#t').first(), 0, 2000)).toBe(0);\n"), "{got}");
    assert!(got.contains("await expect.poll(() => tables.rows(cur.locator('#t').first()), { timeout: 10000 }).toBeGreaterThanOrEqual(1);"), "{got}");
    assert!(got.contains("    expect(await tables.stillRows(cur.locator('#t').first(), 5, 400)).toBeLessThanOrEqual(5);\n"), "{got}");
    assert!(!got.contains("toHaveCount"), "the app's rows are not one locator: {got}");
}

#[test]
fn a_row_count_the_app_refuses_is_refused_with_its_sentence() {
    for counts in [json!({ "at_least": 1, "at_most": 3 }), json!({ "equals": 1, "at_most": 3 }), json!({})] {
        let mut a = json!({ "kind": "expect_row_count", "table": "#t" });
        a.as_object_mut().unwrap().extend(counts.as_object().unwrap().clone());
        let s = script(json!([{ "step_number": 1, "actions": [a.clone()] }]));
        assert_eq!(check(&s, &origins()).unwrap_err().0, "expect_row_count takes one of equals, at_least or at_most", "{a}");
        assert!(spec(json!([a])).is_err());
    }
}

#[test]
fn a_table_inside_a_frame_is_read_in_its_frame() {
    let got = spec(json!([
        { "kind": "expect_row", "table": [{ "css": "iframe#body" }, { "css": "table.grid" }], "cells": { "A": "b" } },
        { "kind": "expect_row_count", "table": [{ "css": "iframe#body" }, { "role": "grid", "nth": 1 }], "at_least": 2 }
    ]))
    .unwrap();
    assert!(
        got.contains("tables.row(cur.locator('iframe#body').filter({ visible: true }).contentFrame().locator('table.grid').filter({ visible: true }).first(), [['A', 'b']], false)"),
        "{got}"
    );
    assert!(
        got.contains("tables.rows(cur.locator('iframe#body').filter({ visible: true }).contentFrame().getByRole('grid').filter({ visible: true }).nth(1)), { timeout: 10000 })"),
        "nth picks one already: {got}"
    );
}

#[test]
fn the_helper_is_written_once_and_only_when_a_table_check_is_used() {
    let got = spec(json!([
        { "kind": "expect_row", "table": "#t", "cells": { "A": "b" } },
        { "kind": "when_visible", "selector": "#more", "then": [{ "kind": "expect_row_count", "table": "#t", "equals": 2 }] }
    ]))
    .unwrap();
    assert_eq!(got.matches("const tables = ").count(), 1, "{got}");
    assert_eq!(got.matches("const readTable = ").count(), 1, "{got}");
    assert!(got.find("const tables = ").unwrap() < got.find("// 1.").unwrap(), "{got}");
    // Inside the when_visible too.
    assert!(got.contains("      await expect.poll(() => tables.rows(cur.locator('#t').first())"), "{got}");

    let plain = spec(json!([{ "kind": "click", "selector": "#a" }])).unwrap();
    assert!(!plain.contains("tables") && !plain.contains("readTable"), "{plain}");

    // A table check in the recipe's after_sign_in needs it as well.
    let signin: Vec<RecipeStep> =
        serde_json::from_value(json!([{ "kind": "expect_row_count", "table": "#home", "at_least": 1 }])).unwrap();
    let got = spec_with(&script(json!([{ "step_number": 1, "actions": [] }])), &signin).unwrap();
    assert_eq!(got.matches("const tables = ").count(), 1, "{got}");
}

#[test]
fn the_exported_table_reader_is_the_apps_own() {
    let ts = read_table_ts().join("\n");
    // Undo the TypeScript additions and the constant comes back word for word.
    let back = ts
        .replacen("const readTable = (e: Element) => (function (this: HTMLTableElement) {", "function() {", 1)
        .replacen("let headerRow: HTMLTableRowElement | null = null;", "let headerRow = null;", 1)
        .replacen("}).call(e as HTMLTableElement);", "}", 1);
    assert_eq!(back, READ_TABLE_JS.replace("\r\n", "\n"));
    // ...and it is what the helper holds, indented.
    let helper = tables_ts(4).join("\n");
    let indented: Vec<String> = read_table_ts().iter().map(|l| if l.is_empty() { String::new() } else { format!("      {l}") }).collect();
    assert!(helper.contains(&indented.join("\n")), "{helper}");
    // The helper waits as long as the app does before passing an absence.
    assert!(helper.contains(&format!(">= {SETTLE_MS})")) && helper.contains(&format!("Math.min({SETTLE_POLL_MS}, ")), "{helper}");
    assert!(helper.contains(&format!("'{NOT_ON_PAGE}'")), "{helper}");
    assert!(helper.contains("chars(s).slice(0, 200)"), "{helper}");
    assert!(!helper.contains("SETTLE_MS") && !helper.contains("MESSAGE_MAX") && !helper.contains("NOT_ON_PAGE"), "{helper}");
    // Escapes, never the characters: a U+2028 in a regex literal ends the line.
    assert!(helper.is_ascii(), "{helper}");
}

#[test]
fn the_settling_rule_is_the_apps() {
    use v2_lib::browser::table::RowCount;
    assert!(TableCheck::Count(RowCount::Equals(0)).needs_settling());
    assert!(TableCheck::Count(RowCount::AtMost(4)).needs_settling());
    assert!(!TableCheck::Count(RowCount::Equals(2)).needs_settling());
    assert!(!TableCheck::Count(RowCount::AtLeast(0)).needs_settling());
}

/// Compares `got` with a golden file under `tests/fixtures/pw_export/`, or
/// rewrites the file when PW_EXPORT_BLESS is set. `src/lib/pwExportGolden.test.ts`
/// type-checks every golden.
fn golden(name: &str, got: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pw_export").join(name);
    if std::env::var_os("PW_EXPORT_BLESS").is_some() {
        std::fs::write(&path, got).unwrap();
    }
    let want = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
    assert_eq!(got, want, "{name} is stale - rerun with PW_EXPORT_BLESS=1 and review the diff");
}

#[test]
fn every_table_check_becomes_the_tables_golden() {
    let got = spec(json!([
        { "kind": "expect_row", "table": { "role": "grid", "name": "People" }, "cells": { "Status": "Active", "Name": "Ann" } },
        { "kind": "expect_no_row", "table": "#t", "cells": { "Name": "Ben" }, "exact": true, "timeout_ms": 5000 },
        { "kind": "expect_sorted", "table": "#t", "column": "Joined", "order": "descending", "as": "date" },
        { "kind": "expect_sorted", "table": "#t", "column": "Joined", "order": "ascending", "as": { "date": "dd/MM/yyyy" } },
        { "kind": "expect_row_count", "table": [{ "css": "iframe#body" }, { "css": "table" }], "at_least": 1 },
        { "kind": "expect_row_count", "table": "#t", "equals": 0 },
        { "kind": "expect_row_count", "table": "#t", "at_most": 9 }
    ]))
    .unwrap();
    golden("tables.spec.ts.golden", &got);
}

// ---- The TypeScript port agrees with the Rust ----------------------------------------

fn got<T: serde::Serialize>(r: Result<T, String>) -> Value {
    match r {
        Ok(v) => json!({ "ok": true, "v": v }),
        Err(why) => json!({ "ok": false, "why": why }),
    }
}

fn read_json(r: &TableRead) -> Value {
    json!({ "headers": r.headers, "rows": r.rows })
}

/// Runs the TypeScript port (`table_logic_ts`, as the spec carries it) in
/// node on every case, and returns what it answered, one value per case.
fn node_answers(cases: &[Value]) -> Vec<Value> {
    let dir = tempfile::tempdir().unwrap();
    let harness = format!(
        "{}\n{}",
        table_logic_ts(),
        r#"const fs = require('node:fs');
const cases = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
const out = cases.map((c: any) => {
  switch (c.op) {
    case 'fromValue': return fromValue(c.value);
    case 'column': return column(c.read, c.name);
    case 'findRow': return findRow(c.read, c.cells, c.exact);
    case 'row': return rowCheck(c.read, c.cells, c.exact);
    case 'noRow': return noRowCheck(c.read, c.cells, c.exact);
    case 'sorted': return checkSorted(c.column, c.values, c.order, c.as);
    case 'sortedCheck': return sortedCheck(c.read, c.column, c.order, c.as);
    case 'number': return parseNumber(c.value);
  }
  throw new Error('unknown op ' + c.op);
});
fs.writeFileSync(process.argv[3], JSON.stringify(out));
"#
    );
    let script = dir.path().join("port.ts");
    let input = dir.path().join("cases.json");
    let output = dir.path().join("answers.json");
    std::fs::write(&script, harness).unwrap();
    std::fs::write(&input, serde_json::to_string(cases).unwrap()).unwrap();
    // Node runs TypeScript by stripping its types (22.18 and later).
    let run = Command::new("node")
        .arg("--no-warnings")
        .arg(&script)
        .arg(&input)
        .arg(&output)
        .output()
        .expect("node is needed for this test (it runs the frontend suite too)");
    assert!(run.status.success(), "node failed: {}", String::from_utf8_lossy(&run.stderr));
    serde_json::from_str(&std::fs::read_to_string(&output).unwrap()).unwrap()
}

fn table(headers: &[&str], rows: &[&[&str]]) -> TableRead {
    TableRead::from_value(&json!({ "headers": headers, "rows": rows })).unwrap()
}

fn cells(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs.iter().map(|(c, t)| (c.to_string(), t.to_string())).collect()
}

#[test]
fn the_typescript_port_agrees_with_the_rust_on_the_same_inputs() {
    let mut cases: Vec<Value> = Vec::new();
    let mut want: Vec<Value> = Vec::new();

    // Reading: padding, and what is not a table.
    for v in [
        json!({ "headers": ["A", "B", "C"], "rows": [["1"], ["1", "2", "3", "4"]] }),
        json!({ "headers": ["A"], "rows": [[1, null, "x"]] }),
        json!({ "headers": null, "rows": [] }),
        json!({ "headers": ["A"] }),
        json!(null),
        json!([]),
    ] {
        cases.push(json!({ "op": "fromValue", "value": v }));
        want.push(TableRead::from_value(&v).map(|r| read_json(&r)).unwrap_or(Value::Null));
    }

    let long = "z".repeat(300);
    let people = table(
        &["Name", "Status", "Joined", "Note\u{a0}Text"],
        &[&["Ann Lee", "Active", "2026-01-05", "x"], &["Ben Ray", "Inactive", "2025-11-30", "y"], &["Cy  Wu", "Active", "", "\u{feff}z"]],
    );
    let ragged = table(&["A", "B"], &[&["1"], &["2", "3"]]);
    let unicode = table(&["\u{130}d", "Stra\u{df}e"], &[&["\u{130}stanbul", "\u{3a3}\u{39f}\u{3a3}"], &["x\u{85}y", "a\u{2003}b"]]);
    let reads = [people.clone(), ragged, unicode, table(&[], &[&["1", "2"]]), table(&[&long, "B"], &[])];

    for read in &reads {
        for name in [" status ", "Grade", "note text", "NOTE  TEXT", "\u{130}D", "i\u{307}d", "stra\u{df}e", "A", "C", ""] {
            cases.push(json!({ "op": "column", "read": read_json(read), "name": name }));
            want.push(got(read.column(name)));
        }
        let wanted: Vec<Vec<(String, String)>> = vec![
            cells(&[("status", "ACT")]),
            cells(&[("Status", "act"), ("Name", "ben")]),
            cells(&[("Status", "Act")]),
            cells(&[("Status", "active"), ("Name", "ann lee")]),
            cells(&[("Name", "Cy  Wu")]),
            cells(&[("Joined", "")]),
            cells(&[("Grade", "A")]),
            cells(&[("Note Text", "\u{feff}z")]),
            cells(&[("Note Text", "z")]),
            cells(&[("B", "")]),
            cells(&[("A", "2"), ("B", "3")]),
            cells(&[("\u{130}d", "\u{130}stanbul")]),
            cells(&[("Stra\u{df}e", "\u{3c3}\u{3bf}\u{3c2}")]),
            cells(&[("Stra\u{df}e", "a b")]),
            cells(&[("\u{130}D", "x y")]),
        ];
        for c in &wanted {
            for exact in [false, true] {
                let base = json!({ "read": read_json(read), "cells": c, "exact": exact });
                let with = |op: &str| {
                    let mut v = base.clone();
                    v["op"] = json!(op);
                    v
                };
                cases.push(with("findRow"));
                want.push(got(find_row(read, c, exact)));
                cases.push(with("row"));
                want.push(got(row_check(read, c, exact)));
                cases.push(with("noRow"));
                want.push(got(no_row_check(read, c, exact)));
            }
        }
    }

    // Numbers, as the number reading of a column reads them.
    for v in [
        "1,234", "-5", "12,345,678.50", "1,23", "12a", "", "--1", "1.", "1,2345", "40%", "12.5 %", "LKR 1,250.50", "lkr1,250",
        "-$5", "$-5", "- $ 5", "Rs. 900", "rs900", "\u{a3}3", "\u{20ac} 1,000.25", "(5)", "$$5", "5%%", "Rsx5", "USD 5", "-$-5",
        "$", "%", "LKR", "rs.x", "Rs.5", "\u{a0}7\u{a0}", "0012", ",123", "1,", "1.2.3", "\u{ff11}", "9007199254740993", "0.1",
        "rs\u{e9}5", "RS.-4", "\u{feff}5", " 5\u{85}",
    ] {
        cases.push(json!({ "op": "number", "value": v }));
        want.push(parse_number(v).map(|n| json!(n)).unwrap_or(Value::Null));
    }

    // Order: every way of reading, both orders, over the app's own cases.
    let fmt = |f: &str| SortAs::Format { date: f.to_string() };
    let ways: Vec<SortAs> = vec![
        serde_json::from_value(json!("text")).unwrap(),
        serde_json::from_value(json!("number")).unwrap(),
        serde_json::from_value(json!("date")).unwrap(),
        fmt("dd/MM/yyyy"),
        fmt("MM/dd/yyyy"),
        fmt("d MMM yyyy"),
        fmt("yyyy.MM.dd"),
        fmt("yy-M"),
        fmt("dd\u{2013}MM\u{2013}yyyy"),
    ];
    let columns: Vec<Vec<&str>> = vec![
        vec!["apple", "", "Banana", "cherry"],
        vec!["b", "a"],
        vec!["-5", "9", "10", "1,234"],
        vec!["1", "two"],
        vec!["10", "", "9"],
        vec!["-$5", "40%", "Rs. 99", "LKR 1,250.50"],
        vec![&long, "a"],
        vec!["2025-11-30", "2026-01-05"],
        vec!["5 Mar 2026", "12 mar 2026", "1 Apr 2026"],
        vec!["01/02/2026", "25/12/2026"],
        vec!["12/25/2026", "01/02/2027"],
        vec!["2026-01-01", "5 Feb 2026", "20/03/2026"],
        vec!["2026-01-01", "soon"],
        vec!["2026-02-30"],
        vec!["2024-02-29", "1900-02-29", "2000-02-29"],
        vec!["01/02/2026", "03/01/2026"],
        vec!["01/01/2026", "02/02/2026"],
        vec!["9 Jan 2026", "10 Jan 2026"],
        vec!["2026.01.09", "2026.1.10"],
        vec!["13/13/2026", "01/02/2026"],
        vec!["12/25/2026", "25/12/2026"],
        vec!["09\u{2013}01\u{2013}2026", "10\u{2013}01\u{2013}2026"],
        vec!["\u{ff5e}", "\u{1f600}"],
        vec!["\u{e9}", "f", "E"],
        vec!["  b ", "\u{a0}a"],
        vec![],
        vec!["", " "],
    ];
    for values in &columns {
        let values: Vec<String> = values.iter().map(|v| v.to_string()).collect();
        for way in &ways {
            for order in [SortOrder::Ascending, SortOrder::Descending] {
                cases.push(json!({ "op": "sorted", "column": "Col", "values": values, "order": order, "as": way }));
                want.push(got(check_sorted("Col", &values, order, way)));
            }
        }
    }
    // The judge: the column found by header first, its name trimmed.
    let text: SortAs = serde_json::from_value(json!("text")).unwrap();
    for (read, name) in [(&people, " name "), (&people, "Grade"), (&reads[1], "B")] {
        for order in [SortOrder::Ascending, SortOrder::Descending] {
            cases.push(json!({ "op": "sortedCheck", "read": read_json(read), "column": name, "order": order, "as": text }));
            let check = TableCheck::Sorted { column: name, order, sort_as: &text };
            want.push(got(check.judge(read)));
        }
    }

    let answers = node_answers(&cases);
    assert_eq!(answers.len(), cases.len());
    let mut wrong = Vec::new();
    for ((case, w), a) in cases.iter().zip(&want).zip(&answers) {
        let same = match (w.as_f64(), a.as_f64()) {
            (Some(x), Some(y)) => x == y,
            _ => w == a,
        };
        if !same {
            wrong.push(format!("{case}\n  rust: {w}\n  ts:   {a}"));
        }
    }
    assert!(wrong.is_empty(), "{} of {} cases disagree:\n{}", wrong.len(), cases.len(), wrong.join("\n"));
}

// ---- The page-facing loop, run against a scripted locator -----------------------------

/// Runs the whole helper as the spec declares it (`tables_ts`) in node, with
/// `page` absent (only its type is named) and a fake locator whose `count`
/// and `evaluate` answer scripted reads, and returns what `body` printed.
fn run_helper(body: &str) -> Value {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("helper.ts");
    let output = dir.path().join("out.json");
    let source = format!(
        "{}\n{}\n{}",
        tables_ts(0).join("\n"),
        r#"const fs = require('node:fs');
// A locator whose table reads `read(i, ms)` on its i-th look, `ms` after it was made.
const fake = (read: (i: number, ms: number) => unknown, present = true) => {
  const t0 = Date.now();
  const seen = { evals: 0, timeouts: [] as number[] };
  const loc: any = {
    count: async () => (present ? 1 : 0),
    evaluate: async (_f: unknown, _a: unknown, o: { timeout?: number }) => {
      seen.timeouts.push(o && o.timeout !== undefined ? o.timeout : -1);
      return read(seen.evals++, Date.now() - t0);
    },
    toString: () => "locator('#t')",
  };
  return { loc, seen };
};
const people = (n: number) => ({ headers: ['Name'], rows: Array.from({ length: n }, (_, i) => ['P' + i]) });
const timed = async (f: () => Promise<unknown>) => {
  const t0 = Date.now();
  const answer = await f();
  return { answer, ms: Date.now() - t0 };
};
const main = async () => {
  const out: Record<string, unknown> = {};
"#,
        body
    );
    let source = format!(
        "{source}\n  fs.writeFileSync(process.argv[2], JSON.stringify(out));\n}};\nmain().catch((e) => {{ console.error(e); process.exit(1); }});\n"
    );
    std::fs::write(&script, source).unwrap();
    let run = Command::new("node").arg("--no-warnings").arg(&script).arg(&output).output().expect("node is needed for this test");
    assert!(run.status.success(), "node failed: {}", String::from_utf8_lossy(&run.stderr));
    serde_json::from_str(&std::fs::read_to_string(&output).unwrap()).unwrap()
}

#[test]
fn settled_checks_run_the_apps_loop_and_pass_or_fail_as_it_does() {
    let got = run_helper(
        r#"
  // A still table, a 400 ms step: shorter than the settling time, so the
  // check passes on its last read when time runs out.
  { const f = fake(() => people(3)); out.stillShort = await timed(() => tables.stillRows(f.loc, 5, 400)); out.stillShortTimeouts = f.seen.timeouts; }
  // A still table, a long step: passes once it has held still, not at the deadline.
  { const f = fake(() => people(3)); out.stillLong = await timed(() => tables.stillRows(f.loc, 5, 5000)); }
  // A table that changes on every look but always holds: passes at the deadline.
  { const f = fake((i) => people(1 + (i % 2))); out.changing = await timed(() => tables.stillRows(f.loc, 5, 1200)); }
  { const f = fake((i) => people(1 + (i % 3))); out.changingNoRow = await timed(() => tables.noRow(f.loc, [['Name', 'Ben']], false, 1200)); }
  // A table that fails: the app's sentence, after the step's time.
  { const f = fake(() => ({ headers: ['Name'], rows: [['Ann'], ['Ben']] })); out.failing = await timed(() => tables.noRow(f.loc, [['name', 'BEN']], false, 400)); }
  // A count that fails answers its count, for Playwright's numeric matcher.
  { const f = fake(() => people(3)); out.countFails = await timed(() => tables.stillRows(f.loc, 1, 400)); }
  { const f = fake(() => people(2)); out.zeroFails = await timed(() => tables.stillRows(f.loc, 0, 400)); }
  // A table that appears late: the earlier looks do not count.
  { const f = fake((_, ms) => (ms < 300 ? null : people(0))); out.late = await timed(() => tables.stillRows(f.loc, 0, 3000)); }
  // No table: said at once on each look, without evaluate waiting for one.
  { const f = fake(() => people(1), false); out.missing = await timed(() => tables.noRow(f.loc, [['Name', 'Ben']], false, 400)); out.missingEvals = f.seen.evals; }
  { const f = fake(() => people(1), false); out.missingCount = await timed(() => tables.stillRows(f.loc, 0, 400)); }
  { const f = fake(() => people(1), false); out.missingPoll = await timed(() => tables.rows(f.loc)); out.missingRow = await tables.row(f.loc, [['Name', 'P0']], false); }
  // Not a table.
  { const f = fake(() => null); out.notTable = await tables.noRow(f.loc, [['Name', 'Ben']], false, 300); }
  // The polled checks look once and answer.
  { const f = fake(() => people(2)); out.row = await tables.row(f.loc, [['Name', 'p1']], false); out.rowNo = await tables.row(f.loc, [['Name', 'x']], false); out.rows = await tables.rows(f.loc); }
"#,
    );
    let ms = |k: &str| got[k]["ms"].as_u64().unwrap();
    let answer = |k: &str| got[k]["answer"].clone();

    assert_eq!(answer("stillShort"), json!(3), "{got}");
    assert!(ms("stillShort") < 700, "a 400 ms step ends near 400 ms: {got}");
    for t in got["stillShortTimeouts"].as_array().unwrap() {
        let t = t.as_i64().unwrap();
        assert!((1..=400).contains(&t), "evaluate waits no longer than the step has left: {got}");
    }
    assert_eq!(answer("stillLong"), json!(3), "{got}");
    assert!(ms("stillLong") >= SETTLE_MS && ms("stillLong") < 2500, "passes once still for {SETTLE_MS} ms: {got}");

    assert!(matches!(answer("changing").as_u64(), Some(1 | 2)), "{got}");
    assert!(ms("changing") >= 1150, "a changing table is judged at the deadline: {got}");
    assert_eq!(answer("changingNoRow"), Value::Null, "{got}");

    let two = table(&["Name"], &[&["Ann"], &["Ben"]]);
    let want = no_row_check(&two, &cells(&[("name", "BEN")]), false).unwrap_err();
    assert_eq!(want, "a row has name \"BEN\" (row 2)");
    assert_eq!(answer("failing"), json!(want), "{got}");
    assert!(ms("failing") >= 390, "{got}");
    assert_eq!(answer("countFails"), json!(3), "{got}");
    assert_eq!(answer("zeroFails"), json!(2), "{got}");
    assert_eq!(answer("late"), json!(0), "{got}");
    assert!(ms("late") >= 300 + SETTLE_MS - 50, "{got}");

    let missing = format!("waited 400ms: locator('#t') {NOT_ON_PAGE}");
    assert_eq!(answer("missing"), json!(missing), "{got}");
    assert_eq!(got["missingEvals"], json!(0), "{got}");
    assert!(ms("missing") < 700, "{got}");
    assert_eq!(answer("missingCount"), json!(missing), "{got}");
    assert_eq!(answer("missingPoll"), json!(format!("locator('#t') {NOT_ON_PAGE}")), "{got}");
    assert!(ms("missingPoll") < 200, "{got}");
    assert_eq!(got["missingRow"], json!(format!("locator('#t') {NOT_ON_PAGE}")), "{got}");
    assert_eq!(got["notTable"], json!("locator('#t') is not a table or grid"), "{got}");

    assert_eq!(got["row"], Value::Null, "{got}");
    assert_eq!(got["rowNo"], json!("no row has Name \"x\" - the table has 2 rows"), "{got}");
    assert_eq!(got["rows"], json!(2), "{got}");
}
