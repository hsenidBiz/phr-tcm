//! Playwright export: string literals and selectors as TypeScript.

use serde_json::json;
use v2_lib::browser::locator::Target;
use v2_lib::pw_export::ts::{lit, locator};

fn t(v: serde_json::Value) -> Target {
    serde_json::from_value::<Target>(v).unwrap()
}

#[test]
fn lit_escapes_quotes_backslashes_and_control_characters() {
    assert_eq!(lit("it's \\ a `x` ${y}\nz"), r#"'it\'s \\ a `x` ${y}\nz'"#);
    let bs = '\\';
    assert_eq!(lit("a\r\tb\u{2028}\u{2029}"), format!("'a{bs}r{bs}tb{bs}u2028{bs}u2029'"));
}

#[test]
fn role_with_name_and_exact() {
    let got = locator("cur", &t(json!({"role":"button","name":"Save","exact":true})));
    assert_eq!(got, "cur.getByRole('button', { name: 'Save', exact: true }).filter({ visible: true })");
}

#[test]
fn role_without_exact_or_name() {
    assert_eq!(
        locator("cur", &t(json!({"role":"button","name":"Save"}))),
        "cur.getByRole('button', { name: 'Save' }).filter({ visible: true })"
    );
    assert_eq!(locator("cur", &t(json!({"role":"dialog"}))), "cur.getByRole('dialog').filter({ visible: true })");
}

#[test]
fn text_and_css() {
    assert_eq!(locator("cur", &t(json!({"text":"Hi"}))), "cur.getByText('Hi').filter({ visible: true })");
    assert_eq!(
        locator("cur", &t(json!({"text":"Hi","exact":true}))),
        "cur.getByText('Hi', { exact: true }).filter({ visible: true })"
    );
    assert_eq!(locator("cur", &t(json!({"css":"#a","visible":true}))), "cur.locator('#a').filter({ visible: true })");
}

#[test]
fn visible_false_and_nth() {
    assert_eq!(locator("cur", &t(json!({"css":"#a","visible":false,"nth":2}))), "cur.locator('#a').nth(2)");
    assert_eq!(
        locator("cur", &t(json!({"css":"#a","nth":0}))),
        "cur.locator('#a').filter({ visible: true }).nth(0)"
    );
}

#[test]
fn legacy_css_and_text() {
    assert_eq!(locator("cur", &t(json!("#x .y"))), "cur.locator('#x .y')");
    assert_eq!(locator("cur", &t(json!("text=Objectives"))), "cur.locator('text=Objectives').last()");
}

#[test]
fn iframe_chain_enters_the_frame() {
    let got = locator(
        "cur",
        &t(json!([{"css":"iframe[title='Employee Search']"},{"role":"button","name":"Search"}])),
    );
    assert_eq!(
        got,
        r#"cur.locator('iframe[title=\'Employee Search\']').filter({ visible: true }).contentFrame().getByRole('button', { name: 'Search' }).filter({ visible: true })"#
    );
}

#[test]
fn iframe_role_and_plain_chain() {
    let got = locator("cur", &t(json!([{"role":"IFrame"},{"css":"a"}])));
    assert_eq!(got, "cur.getByRole('IFrame').filter({ visible: true }).contentFrame().locator('a').filter({ visible: true })");
    let got = locator("cur", &t(json!([{"role":"dialog","name":"Add"},{"role":"button","name":"Go"}])));
    assert_eq!(
        got,
        "cur.getByRole('dialog', { name: 'Add' }).filter({ visible: true }).getByRole('button', { name: 'Go' }).filter({ visible: true })"
    );
}
