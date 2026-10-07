//! Playwright export: one Auto Run script as a raw spec.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use v2_lib::autorun::recipe::RecipeStep;
use v2_lib::autorun::CaseScript;
use v2_lib::browser::locator::Target;
use v2_lib::pw_export::translate::{check, raw_spec, RawSpecInput};

fn origins() -> Vec<String> {
    vec!["https://app.example".into(), "https://sso.example".into()]
}

fn lf(s: &str) -> String {
    s.replace("\r\n", "\n")
}

fn target(v: Value) -> Target {
    serde_json::from_value(v).unwrap()
}

fn after_sign_in() -> Vec<RecipeStep> {
    serde_json::from_value(json!([
        {
            "kind": "when_visible",
            "selector": { "css": ".bootbox.modal.show .modal-footer button" },
            "within_ms": 4000,
            "then": [{ "kind": "click", "selector": { "css": ".bootbox.modal.show .modal-footer button" } }]
        },
        {
            "kind": "when_visible",
            "selector": { "css": "#sidebar-toggle-menu:not(.active)" },
            "within_ms": 5000,
            "then": [{ "kind": "click", "selector": { "css": "#sidebar-toggle-menu" } }]
        }
    ]))
    .unwrap()
}

fn script(steps: Value) -> CaseScript {
    serde_json::from_value(json!({ "case_id": 1, "title": "A case", "steps": steps })).unwrap()
}

struct Fx {
    script: CaseScript,
    signin: Vec<RecipeStep>,
    area: Vec<Target>,
    by_name: BTreeMap<String, Vec<Target>>,
    texts: BTreeMap<i32, String>,
}

impl Fx {
    fn new(script: CaseScript) -> Fx {
        Fx {
            script,
            signin: vec![],
            area: vec![target(json!({ "css": "a.area" }))],
            by_name: BTreeMap::new(),
            texts: BTreeMap::new(),
        }
    }
    fn run(&self) -> Result<String, String> {
        raw_spec(&RawSpecInput {
            script: &self.script,
            md_path: "suites/sl/admin/m/f/test-cases/f.md".into(),
            feature_title: "F".into(),
            after_sign_in: &self.signin,
            area_clicks: &self.area,
            area_clicks_by_name: &self.by_name,
            step_texts: &self.texts,
            origins: &origins(),
        })
        .map_err(|e| e.0)
    }
}

fn one_step(actions: Value) -> Fx {
    Fx::new(script(json!([{ "step_number": 1, "actions": actions }])))
}

#[test]
fn a_sibling_script_becomes_the_golden_raw_spec() {
    let script: CaseScript = serde_json::from_str(include_str!("../fixtures/pw_export/script-135560.json")).unwrap();
    let texts: BTreeMap<String, String> =
        serde_json::from_value(json!({
            "1": "Open \"Performance Management\", go to \"Setup & Configuration\" and select \"Definition Wizard\".",
            "2": "If the landing page shows, make sure the \"Goals / KPIs\" and \"Competencies\" cards are both selected and click \"Continue to Configuration\".",
            "3": "Make sure at least one rating method is listed and click \"Save & Continue\".",
            "4": "On the \"Performance-based\" card, click \"Activate\", then click \"Activate\" in the \"Activate Template\" dialog.",
            "5": "Click \"Add Level\". Copy the two lines Senior and Expert from a text editor and paste them into the \"Level name\" field of the new row.",
            "6": "Click the \"Confirm edit\" check icon, then click \"Use Custom Levels\"."
        }))
        .unwrap();
    let texts: BTreeMap<i32, String> = texts.into_iter().map(|(k, v)| (k.parse().unwrap(), v)).collect();
    let signin = after_sign_in();
    let area = vec![target(json!({ "css": "a[href*='DefinitionWizard']" }))];
    let by_name = BTreeMap::new();
    let got = raw_spec(&RawSpecInput {
        script: &script,
        md_path: "suites/sl/admin/performance/definition-wizard/test-cases/definition-wizard.md".into(),
        feature_title: "Definition Wizard".into(),
        after_sign_in: &signin,
        area_clicks: &area,
        area_clicks_by_name: &by_name,
        step_texts: &texts,
        origins: &origins(),
    })
    .unwrap();
    assert_eq!(got, lf(include_str!("../fixtures/pw_export/raw-135560.spec.ts")));
}

#[test]
fn a_response_is_armed_before_the_click_and_awaited_after() {
    let got = one_step(json!([
        { "kind": "click", "selector": "#save" },
        { "kind": "expect_response", "method": "post", "url_contains": "API/Save", "status": 201,
          "json": { "ok": true, "n": 2, "who": "it's", "tags": ["a", "b"] } }
    ]))
    .run()
    .unwrap();
    let arm = got.find("const resp1_0 = cur.waitForResponse(r => r.url().toLowerCase().includes('api/save') && r.request().method() === 'POST');").expect("armed");
    let click = got.find("await cur.locator('#save').first().click();").unwrap();
    let wait = got.find("const r1_0 = await resp1_0;").expect("awaited");
    assert!(arm < click && click < wait, "{got}");
    assert!(got.contains("expect(r1_0.status()).toBe(201);"));
    assert!(got.contains(
        "expect(await r1_0.json()).toMatchObject({ 'ok': true, 'n': 2, 'who': 'it\\'s', 'tags': ['a', 'b'] });"
    ), "{got}");
}

#[test]
fn a_list_of_objects_in_a_json_check_is_refused() {
    let why = one_step(json!([
        { "kind": "expect_response", "url_contains": "x", "json": { "rows": [{ "a": 1 }] } }
    ]))
    .run()
    .unwrap_err();
    assert!(why.contains("list of objects"), "{why}");
    let why = one_step(json!([
        { "kind": "api_request", "path": "/x", "expect": { "json": [{ "a": 1 }] } }
    ]))
    .run()
    .unwrap_err();
    assert!(why.contains("list of objects"), "{why}");
}

#[test]
fn an_api_request_sends_its_query_and_checks_the_answer() {
    let got = one_step(json!([
        { "kind": "api_request", "path": "/api/x", "query": { "q": "1" }, "expect": { "status": 200, "json": { "a": "b" } } }
    ]))
    .run()
    .unwrap();
    assert!(got.contains("const a1_0 = await cur.request.get('/api/x', { params: { 'q': '1' }, maxRedirects: 0 });"), "{got}");
    assert!(got.contains("expect(a1_0.status()).toBe(200);"));
    assert!(got.contains("expect(await a1_0.json()).toMatchObject({ 'a': 'b' });"));
}

#[test]
fn when_visible_waits_then_acts_inside_an_if() {
    let got = one_step(json!([
        { "kind": "when_visible", "selector": "#banner", "then": [{ "kind": "click", "selector": "#banner .x" }] },
        { "kind": "when_visible", "selector": "text=Hello", "within_ms": 500,
          "then": [{ "kind": "click", "selector": "text=Hello" }] }
    ]))
    .run()
    .unwrap();
    assert!(
        got.contains(
            "    if (await cur.locator('#banner').first().waitFor({ timeout: 2000 }).then(() => true, () => false)) {\n      await cur.locator('#banner .x').first().click();\n    }\n"
        ),
        "{got}"
    );
    // A Legacy text= selector already ends in .last(); no .first() after it.
    assert!(got.contains("await cur.locator('text=Hello').last().waitFor({ timeout: 500 })"), "{got}");
    assert!(!got.contains(".last().first()"));
}

#[test]
fn first_goes_on_single_element_locators_but_not_on_nth_or_counts() {
    let got = one_step(json!([
        { "kind": "click", "selector": { "role": "button", "name": "Save", "nth": 1 } },
        { "kind": "expect_count", "selector": "li", "equals": 3, "timeout_ms": 900 },
        { "kind": "expect_attribute", "selector": [{ "css": "#a" }, { "role": "textbox" }], "name": "title", "equals": "x" },
        { "kind": "expect_hidden", "selector": "#gone", "timeout_ms": 100 },
        { "kind": "expect_focused", "selector": "#f" }
    ]))
    .run()
    .unwrap();
    assert!(got.contains("getByRole('button', { name: 'Save' }).filter({ visible: true }).nth(1).click();"), "{got}");
    assert!(got.contains("await expect(cur.locator('li')).toHaveCount(3, { timeout: 900 });"), "{got}");
    assert!(got.contains(".getByRole('textbox').filter({ visible: true }).first()).toHaveAttribute('title', 'x');"), "{got}");
    assert!(got.contains("await expect(cur.locator('#gone').first()).toBeHidden({ timeout: 100 });"), "{got}");
    assert!(got.contains("await expect(cur.locator('#f').first().and(cur.locator(':focus-within'))).toHaveCount(1);"), "{got}");
}

#[test]
fn tab_switching_moves_cur() {
    let got = one_step(json!([
        { "kind": "click", "selector": "#open" },
        { "kind": "expect_tab", "name": "help-tab", "url_contains": "a.b?c" },
        { "kind": "switch_tab", "name": "help-tab" },
        { "kind": "click", "selector": "#inside" },
        { "kind": "close_tab", "name": "help-tab" },
        { "kind": "open_tab", "name": "t2", "url": "/path?x=1" },
        { "kind": "switch_tab", "name": "main" },
        { "kind": "expect_tab_closed", "name": "t2", "within_ms": 3000 }
    ]))
    .run()
    .unwrap();
    assert!(got.contains("    let tab_help_tab: typeof page;\n"), "{got}");
    assert!(got.contains("const mark1 = opened.length;"), "{got}");
    assert!(got.contains("tab_help_tab = await nextTab(mark1, 10000);"), "{got}");
    assert!(got.contains("claimed.add(tab_t2);"), "{got}");
    assert!(got.contains("await expect(tab_help_tab).toHaveURL(new RegExp('a\\\\.b\\\\?c'));"), "{got}");
    assert!(got.contains("cur = tab_help_tab;\n    await cur.locator('#inside').first().click();"), "{got}");
    assert!(got.contains("await tab_help_tab.close();\n    if (cur === tab_help_tab) { cur = page; }"), "{got}");
    assert!(got.contains("tab_t2 = await cur.context().newPage();\n    await tab_t2.goto('/path?x=1');\n    claimed.add(tab_t2);\n    cur = tab_t2;"), "{got}");
    assert!(got.contains("cur = page;\n    await tab_t2.waitForEvent('close', { timeout: 3000 });"), "{got}");
}

#[test]
fn return_to_area_replays_the_named_area() {
    let mut fx = one_step(json!([
        { "kind": "return_to_area" },
        { "kind": "return_to_area", "area": "Other" }
    ]));
    fx.signin = after_sign_in();
    fx.by_name.insert("Other".into(), vec![target(json!({ "css": "a.other" }))]);
    let got = fx.run().unwrap();
    assert_eq!(got.matches("await cur.goto('/');").count(), 2, "{got}");
    let own = got.find("    // 1.").unwrap();
    let tail = &got[own..];
    assert!(tail.find("a.area").unwrap() < tail.find("a.other").unwrap(), "{got}");
    assert!(tail.contains("#sidebar-toggle-menu"), "the recipe's after_sign_in runs again");

    let mut fx = one_step(json!([{ "kind": "return_to_area", "area": "Nowhere" }]));
    fx.signin = vec![];
    let why = fx.run().unwrap_err();
    assert!(why.contains("Nowhere"), "{why}");
}

#[test]
fn press_key_times_three_presses_three_times() {
    let got = one_step(json!([
        { "kind": "press_key", "key": "Shift+Tab", "times": 3 },
        { "kind": "press_key", "key": "Ctrl+ArrowUp" },
        { "kind": "press_key", "key": "Space" }
    ]))
    .run()
    .unwrap();
    assert_eq!(got.matches("await cur.keyboard.press('Shift+Tab');").count(), 3, "{got}");
    assert!(got.contains("await cur.keyboard.press('Control+ArrowUp');"));
    assert!(got.contains("await cur.keyboard.press('Space');"));
}

#[test]
fn text_url_download_and_session_actions() {
    let got = one_step(json!([
        { "kind": "check_text", "value": "it's `x` ${y}" },
        { "kind": "check_url", "contains": "/a.b?c=(1)" },
        { "kind": "reload" },
        { "kind": "expire_session" },
        { "kind": "click", "selector": "#dl" },
        { "kind": "expect_download", "name": "Report_*.xlsx" }
    ]))
    .run()
    .unwrap();
    assert!(got.contains("await expect(cur.locator('body')).toContainText('it\\'s `x` ${y}', { ignoreCase: true, useInnerText: true });"), "{got}");
    assert!(got.contains("await expect(cur).toHaveURL(new RegExp('\\\\/a\\\\.b\\\\?c=\\\\(1\\\\)'));"), "{got}");
    assert!(got.contains("await cur.reload();"));
    assert!(got.contains("await cur.context().clearCookies();"));
    let arm = got.find("const dl1_0 = cur.waitForEvent('download', { timeout: 30000 });").unwrap();
    assert!(arm < got.find("#dl").unwrap());
    assert!(got.contains("expect(d1_0.suggestedFilename()).toMatch(new RegExp('^Report_.*\\\\.xlsx$', 'i'));"), "{got}");
}

#[test]
fn drag_onto_translates_and_before_after_do_not() {
    let got = one_step(json!([{ "kind": "drag", "from": "#a", "to": "#b", "position": "onto" }])).run().unwrap();
    assert!(got.contains("await cur.locator('#a').first().dragTo(cur.locator('#b').first());"), "{got}");
    for pos in ["before", "after"] {
        let why = one_step(json!([{ "kind": "drag", "from": "#a", "to": "#b", "position": pos }])).run().unwrap_err();
        assert!(why.contains("drag"), "{why}");
    }
}

#[test]
fn the_header_comes_first_and_a_missing_text_leaves_the_number_alone() {
    let mut fx = Fx::new(script(json!([
        { "step_number": 7, "actions": [], "unchecked": "line one\nline two" },
        { "step_number": 8, "actions": [] }
    ])));
    fx.texts.insert(7, "first\nsecond".into());
    let got = fx.run().unwrap();
    assert!(got.starts_with(
        "// spec: suites/sl/admin/m/f/test-cases/f.md\n// seed: suites/_generated/seed.spec.ts\n\nimport { test, expect } from '@playwright/test';\n\ntest.describe('F', () => {\n  test('A case', async ({ page }) => {\n    let cur = page;\n    await page.goto('/');\n"
    ), "{got}");
    assert!(got.contains("    // 7. first second\n    // Not checked: line one line two\n"), "{got}");
    assert!(got.contains("\n    // 8.\n"), "{got}");
    assert!(got.ends_with("  });\n});\n"));
    assert!(!got.contains("fixme") && !got.contains("skip"));
}

#[test]
fn each_unexportable_kind_says_why() {
    let table: Vec<(&str, Value)> = vec![
        ("upload", json!({ "kind": "upload", "selector": "#f", "file": "a.xlsx" })),
        ("sign_in", json!({ "kind": "sign_in", "account": "x" })),
        ("expect_dialog", json!({ "kind": "expect_dialog", "answer": "accept" })),
        ("expect_download", json!({ "kind": "expect_download", "name": "a.xlsx", "sheet": "S" })),
        ("expect_download", json!({ "kind": "expect_download", "name": "a.xlsx", "contains_text": ["x"] })),
        ("expect_row", json!({ "kind": "expect_row", "table": "#t", "cells": { "A": "b" } })),
        ("expect_no_row", json!({ "kind": "expect_no_row", "table": "#t", "cells": { "A": "b" } })),
        ("expect_sorted", json!({ "kind": "expect_sorted", "table": "#t", "column": "A", "order": "ascending" })),
        ("expect_row_count", json!({ "kind": "expect_row_count", "table": "#t", "equals": 1 })),
    ];
    for (kind, action) in table {
        let s = script(json!([{ "step_number": 1, "actions": [action.clone()] }]));
        let why = check(&s, &origins()).unwrap_err().0;
        assert!(why.contains(kind), "{kind}: {why}");
        let why = one_step(json!([action])).run().unwrap_err();
        assert!(why.contains(kind), "{kind}: {why}");
    }
    // Inside a when_visible too.
    let s = script(json!([{ "step_number": 1, "actions": [
        { "kind": "when_visible", "selector": "#a", "then": [{ "kind": "upload", "selector": "#f", "file": "a" }] }
    ]}]));
    assert!(check(&s, &origins()).unwrap_err().0.contains("upload"));
}

#[test]
fn a_tab_opened_by_the_previous_step_is_claimed_by_the_next() {
    let fx = Fx::new(script(json!([
        { "step_number": 4, "actions": [{ "kind": "click", "selector": "#open" }] },
        { "step_number": 5, "actions": [{ "kind": "expect_tab", "name": "a" }] }
    ])));
    let got = fx.run().unwrap();
    assert!(got.contains("page.context().on('page', p => { opened.push(p); });"), "{got}");
    assert!(got.contains("const mark4 = opened.length;"), "{got}");
    assert!(got.contains("const mark5 = opened.length;"), "{got}");
    assert!(got.contains("tab_a = await nextTab(mark4, 10000);"), "counts from the previous step: {got}");
    assert!(got.contains("claimed.add(p);"));
    assert!(!got.contains("waitForEvent('page'"));
}

#[test]
fn two_expect_tabs_in_one_step_claim_distinct_pages_in_the_helper() {
    let got = one_step(json!([
        { "kind": "expect_tab", "name": "a" },
        { "kind": "expect_tab", "name": "b", "within_ms": 2000 }
    ]))
    .run()
    .unwrap();
    assert!(got.contains("tab_a = await nextTab(mark1, 10000);\n    tab_b = await nextTab(mark1, 2000);"), "{got}");
    // The helper marks what it hands out as claimed and skips claimed pages.
    assert!(got.contains("filter(p => !claimed.has(p))"), "{got}");
}

#[test]
fn when_visible_on_a_hidden_selector_looks_for_a_visible_match_with_the_floor() {
    let got = one_step(json!([
        { "kind": "when_visible", "selector": { "css": "#x", "visible": false }, "within_ms": 100,
          "then": [{ "kind": "click", "selector": "#x" }] },
        { "kind": "wait_for", "selector": { "css": "#x", "visible": false }, "timeout_ms": 900 }
    ]))
    .run()
    .unwrap();
    assert!(got.contains("if (await cur.locator('#x').filter({ visible: true }).first().waitFor({ timeout: 500 })"), "{got}");
    assert!(got.contains("cur.locator('#x').first().waitFor({ state: 'attached', timeout: 900 });"), "{got}");
}

#[test]
fn an_api_request_does_not_follow_redirects() {
    let got = one_step(json!([
        { "kind": "api_request", "path": "/a" },
        { "kind": "api_request", "path": "/b", "query": { "q": "1" } }
    ]))
    .run()
    .unwrap();
    assert!(got.contains("cur.request.get('/a', { maxRedirects: 0 })"), "{got}");
    assert!(got.contains("{ params: { 'q': '1' }, maxRedirects: 0 }"), "{got}");
}

#[test]
fn keys_are_read_the_way_the_app_reads_them() {
    let got = one_step(json!([
        { "kind": "press_key", "key": "ctrl+Enter" },
        { "kind": "press_key", "key": "Shift + Tab" },
        { "kind": "press_key", "key": "shift+ctrl+End" }
    ]))
    .run()
    .unwrap();
    assert!(got.contains("press('Control+Enter')"), "{got}");
    assert!(got.contains("press('Shift+Tab')"), "{got}");
    assert!(got.contains("press('Control+Shift+End')"), "{got}");
    let why = one_step(json!([{ "kind": "press_key", "key": "Hyper+Q" }])).run().unwrap_err();
    assert!(why.contains("press_key"), "{why}");
}

#[test]
fn a_blank_area_name_replays_the_own_area_and_response_text_is_trimmed() {
    let got = one_step(json!([
        { "kind": "return_to_area", "area": "  " },
        { "kind": "expect_response", "method": " post ", "url_contains": " Save " }
    ]))
    .run()
    .unwrap();
    assert!(got.contains("await cur.locator('a.area')"), "{got}");
    assert!(got.contains("includes('save') && r.request().method() === 'POST'"), "{got}");
}

#[test]
fn text_and_focus_checks_follow_the_app() {
    let got = one_step(json!([
        { "kind": "check_text", "value": "x" },
        { "kind": "expect_focused", "selector": "#f", "timeout_ms": 50 }
    ]))
    .run()
    .unwrap();
    assert!(got.contains("{ ignoreCase: true, useInnerText: true }"), "{got}");
    assert!(
        got.contains("await expect(cur.locator('#f').first().and(cur.locator(':focus-within'))).toHaveCount(1, { timeout: 50 });"),
        "{got}"
    );
}

#[test]
fn same_site_addresses_export_as_paths() {
    let got = one_step(json!([
        { "kind": "navigate", "url": " /hr/x?y=1 " },
        { "kind": "navigate", "url": "https://app.example/hr/a?b=2#c" },
        { "kind": "navigate", "url": "HTTPS://App.Example" },
        { "kind": "navigate", "url": "https://app.example?q=1" },
        { "kind": "navigate", "url": "https://sso.example/login?r=1" },
        { "kind": "open_tab", "name": "t", "url": "https://app.example/p?x=1" }
    ]))
    .run()
    .unwrap();
    assert!(got.contains("await cur.goto('/hr/x?y=1');"), "{got}");
    assert!(got.contains("await cur.goto('/hr/a?b=2#c');"), "{got}");
    assert!(got.contains("await cur.goto('/');"), "{got}");
    assert!(got.contains("await cur.goto('/?q=1');"), "{got}");
    assert!(got.contains("await cur.goto('https://sso.example/login?r=1');"), "another allowed origin stays whole: {got}");
    assert!(got.contains("await tab_t.goto('/p?x=1');"), "{got}");
}

#[test]
fn other_addresses_are_refused_by_check_and_by_the_spec() {
    for url in ["https://other.example/x", "file:///c:/x.html", "//host/x", "x/y", "ftp://app.example/x"] {
        let why = one_step(json!([{ "kind": "navigate", "url": url }])).run().unwrap_err();
        assert!(why.contains("navigate"), "{url}: {why}");
        let why = one_step(json!([{ "kind": "open_tab", "name": "t", "url": url }])).run().unwrap_err();
        assert!(why.contains("open_tab"), "{url}: {why}");
        let s = script(json!([{ "step_number": 1, "actions": [{ "kind": "navigate", "url": url }] }]));
        assert!(check(&s, &origins()).is_err(), "{url}");
    }
}
