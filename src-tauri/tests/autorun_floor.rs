//! The expected-result floor: every way an Auto Run script falls short of
//! its test case's expected results. Pure functions over `CaseScript` and
//! `steps_xml::Step` - no browser, no filesystem.

use v2_lib::autorun::floor::{check_floor, expected_of, steps_on_shared_rows, Expected};
use v2_lib::autorun::CaseScript;
use v2_lib::steps_xml::Step;

fn case(expected: &[&str]) -> Vec<Expected> {
    expected
        .iter()
        .enumerate()
        .map(|(i, e)| Expected { step_number: i as i32 + 1, expected: e.to_string(), shared: false })
        .collect()
}

fn script(steps: serde_json::Value) -> CaseScript {
    serde_json::from_value(serde_json::json!({ "case_id": 1, "title": "t", "steps": steps })).unwrap()
}

#[test]
fn a_step_with_an_expected_result_must_check_something_or_say_why() {
    let sc = script(serde_json::json!([
        { "step_number": 1, "actions": [{ "kind": "navigate", "url": "https://a.example/" }] },
        { "step_number": 2, "actions": [{ "kind": "click", "selector": "#save" }] },
        { "step_number": 3, "actions": [{ "kind": "click", "selector": "#x" }], "unchecked": "the toast vanishes too fast to read" }
    ]));
    let out = check_floor(&sc, &case(&["", "A toast says Saved", "The row is gone"]));
    assert_eq!(out.len(), 1, "{out:?}");
    assert!(out[0].starts_with("step 2 expects \"A toast says Saved\""), "{}", out[0]);
    assert!(out[0].contains("unchecked"));
}

#[test]
fn the_first_60_chars_of_the_expected_result_are_quoted() {
    let long_expected = "A".repeat(80);
    let sc = script(serde_json::json!([
        { "step_number": 1, "actions": [{ "kind": "click", "selector": "#x" }] }
    ]));
    let out = check_floor(&sc, &case(&[&long_expected]));
    assert_eq!(out.len(), 1, "{out:?}");
    let first_60: String = long_expected.chars().take(60).collect();
    assert_eq!(
        out[0],
        format!(
            "step 1 expects \"{first_60}\" but the script checks nothing there - add an expect_ action, or say why in \"unchecked\""
        )
    );
}

#[test]
fn a_step_with_a_check_satisfies_its_expected_result() {
    let sc = script(serde_json::json!([
        { "step_number": 1, "actions": [{ "kind": "click", "selector": "#save" }, { "kind": "check_text", "value": "Saved" }] }
    ]));
    let out = check_floor(&sc, &case(&["A toast says Saved"]));
    assert!(out.is_empty(), "{out:?}");
}

#[test]
fn a_case_step_with_no_matching_script_step_is_reported() {
    let sc = script(serde_json::json!([
        { "step_number": 1, "actions": [{ "kind": "click", "selector": "#save" }, { "kind": "check_text", "value": "Saved" }] }
    ]));
    let out = check_floor(&sc, &case(&["A toast says Saved", "The row is gone"]));
    assert_eq!(out.len(), 1, "{out:?}");
    assert_eq!(out[0], "step 2 expects \"The row is gone\" but the script has no step 2");
}

#[test]
fn a_script_step_marked_unchecked_but_holding_a_check_is_a_contradiction() {
    let sc = script(serde_json::json!([
        { "step_number": 1, "actions": [{ "kind": "check_text", "value": "Saved" }], "unchecked": "not sure why this is here" }
    ]));
    let out = check_floor(&sc, &case(&["A toast says Saved"]));
    assert_eq!(out.len(), 1, "{out:?}");
    assert_eq!(out[0], "step 1 says it is unchecked but has a check - drop one or the other");
}

#[test]
fn a_script_step_marked_unchecked_when_the_case_expects_nothing_is_reported() {
    let sc = script(serde_json::json!([
        { "step_number": 1, "actions": [{ "kind": "click", "selector": "#x" }], "unchecked": "nothing to check here" }
    ]));
    let out = check_floor(&sc, &case(&[""]));
    assert_eq!(out.len(), 1, "{out:?}");
    assert_eq!(out[0], "step 1 says it is unchecked but the case expects nothing there");
}

#[test]
fn a_step_with_no_case_step_but_a_check_and_unchecked_is_a_contradiction() {
    // Rule 3 applies wherever an unchecked step sits, even step 0 (sign-in)
    // or a step number the case has nothing to say about at all.
    let sc = script(serde_json::json!([
        { "step_number": 0, "actions": [{ "kind": "check_text", "value": "Signed in" }], "unchecked": "not sure why" }
    ]));
    let out = check_floor(&sc, &case(&[]));
    assert_eq!(out.len(), 1, "{out:?}");
    assert_eq!(out[0], "step 0 says it is unchecked but has a check - drop one or the other");
}

#[test]
fn a_step_beyond_the_case_marked_unchecked_expects_nothing_there() {
    // Rule 4 applies to a script step past the case's count too: no case
    // step at that number IS "the case expects nothing there".
    let sc = script(serde_json::json!([
        { "step_number": 1, "actions": [{ "kind": "check_text", "value": "Saved" }] },
        { "step_number": 2, "actions": [{ "kind": "check_text", "value": "Gone" }] },
        { "step_number": 5, "actions": [{ "kind": "click", "selector": "#teardown" }], "unchecked": "teardown, not part of the case" }
    ]));
    let out = check_floor(&sc, &case(&["A toast says Saved", "The row is gone"]));
    assert_eq!(out.len(), 1, "{out:?}");
    assert_eq!(out[0], "step 5 says it is unchecked but the case expects nothing there");
}

#[test]
fn check_floor_ignores_a_whitespace_only_expected_result() {
    let sc = script(serde_json::json!([
        { "step_number": 1, "actions": [{ "kind": "click", "selector": "#x" }] }
    ]));
    let out = check_floor(&sc, &case(&["   \t  "]));
    assert!(out.is_empty(), "{out:?}");
}

#[test]
fn check_floor_truncates_a_multi_byte_expected_result_without_panicking() {
    let long_expected: String = std::iter::repeat('\u{00e9}').take(80).collect();
    let sc = script(serde_json::json!([
        { "step_number": 1, "actions": [{ "kind": "click", "selector": "#x" }] }
    ]));
    let out = check_floor(&sc, &case(&[&long_expected]));
    assert_eq!(out.len(), 1, "{out:?}");
    let quoted = out[0].split('"').nth(1).expect("a quoted sentence");
    assert_eq!(quoted.chars().count(), 60);
}

#[test]
fn the_sign_in_step_and_steps_beyond_the_case_are_ignored() {
    let sc = script(serde_json::json!([
        { "step_number": 0, "actions": [{ "kind": "click", "selector": "#signin" }] },
        { "step_number": 1, "actions": [{ "kind": "click", "selector": "#save" }, { "kind": "check_text", "value": "Saved" }] },
        { "step_number": 2, "actions": [{ "kind": "click", "selector": "#extra" }] }
    ]));
    let out = check_floor(&sc, &case(&["A toast says Saved"]));
    assert!(out.is_empty(), "{out:?}");
}

#[test]
fn a_case_with_no_steps_reports_nothing() {
    let sc = script(serde_json::json!([
        { "step_number": 1, "actions": [{ "kind": "click", "selector": "#x" }] }
    ]));
    let out = check_floor(&sc, &case(&[]));
    assert!(out.is_empty(), "{out:?}");
}

#[test]
fn output_is_sorted_by_step_number_with_no_duplicates() {
    let sc = script(serde_json::json!([
        { "step_number": 3, "actions": [{ "kind": "click", "selector": "#x" }] },
        { "step_number": 1, "actions": [{ "kind": "click", "selector": "#y" }] }
    ]));
    let out = check_floor(&sc, &case(&["First", "Second", "Third"]));
    assert_eq!(
        out,
        vec![
            "step 1 expects \"First\" but the script checks nothing there - add an expect_ action, or say why in \"unchecked\"",
            "step 2 expects \"Second\" but the script has no step 2",
            "step 3 expects \"Third\" but the script checks nothing there - add an expect_ action, or say why in \"unchecked\"",
        ]
    );
}

#[test]
fn expected_of_trims_and_keeps_position_including_empty_ones() {
    let steps = vec![
        Step { action: "Open the page".to_string(), expected: "  A toast says Saved  ".to_string(), shared: None },
        Step { action: "Click save".to_string(), expected: "".to_string(), shared: None },
        Step { action: "Reload".to_string(), expected: "\tThe row is gone\n".to_string(), shared: None },
    ];
    let out = expected_of(&steps);
    assert_eq!(out.len(), 3);
    assert_eq!(out[0].step_number, 1);
    assert_eq!(out[0].expected, "A toast says Saved");
    assert_eq!(out[1].step_number, 2);
    assert_eq!(out[1].expected, "");
    assert_eq!(out[2].step_number, 3);
    assert_eq!(out[2].expected, "The row is gone");
}

/// Scripts saved before 1.25.23 were numbered as if the case had no Shared
/// Steps rows. On a case with one, such a script puts a step on the Shared
/// Steps row itself. That is refused, with the way to fix it.
#[test]
fn a_script_step_on_a_shared_steps_row_is_refused() {
    let steps = vec![
        Step { action: "Open the page".into(), expected: "The page shows".into(), shared: None },
        Step { shared: Some(812), ..Default::default() },
        Step { action: "Save".into(), expected: "Saved".into(), shared: None },
    ];
    let expected = expected_of(&steps);
    assert!(expected[1].shared && !expected[0].shared && !expected[2].shared);

    // Numbered without the Shared Steps row: "Save" sits on step 2.
    let old = script(serde_json::json!([
        { "step_number": 1, "actions": [{ "kind": "check_text", "value": "page" }] },
        { "step_number": 2, "actions": [{ "kind": "check_text", "value": "Saved" }] }
    ]));
    let out = check_floor(&old, &expected);
    let on_shared = out.iter().find(|s| s.starts_with("step 2 is a Shared Steps entry")).expect("rule 5");
    assert!(on_shared.contains("from 2 on"), "{on_shared}");

    // Numbered with it: holds.
    let fixed = script(serde_json::json!([
        { "step_number": 1, "actions": [{ "kind": "check_text", "value": "page" }] },
        { "step_number": 3, "actions": [{ "kind": "check_text", "value": "Saved" }] }
    ]));
    assert!(check_floor(&fixed, &expected).is_empty(), "{:?}", check_floor(&fixed, &expected));
}

#[test]
fn steps_on_shared_rows_are_sorted_and_named_once() {
    assert_eq!(steps_on_shared_rows(&[4, 2, 2, 1, 0, -1], &[2, 4]), vec![2, 4]);
    assert!(steps_on_shared_rows(&[1, 3], &[2]).is_empty());
    assert!(steps_on_shared_rows(&[1, 2], &[]).is_empty());
}
