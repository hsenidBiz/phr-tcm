//! The same failure across cases: grouped by action, target and error
//! class, or by the element that covered them - and reported to an
//! assistant only when at least two different cases hit it.

use v2_lib::autorun::failures::describe_failures;
use v2_lib::autorun::patterns::{classify, find_patterns, patterns_section, ErrorClass, PATTERN_ADVICE};
use v2_lib::autorun::{CaseRecord, CaseScript, LocalRun, StepRecord, StepScript};
use v2_lib::browser::actions::{Action, ActionOutcome};
use v2_lib::browser::locator::{LocatorStep, Target};

fn role(role: &str, name: &str) -> Target {
    Target::One(LocatorStep { role: Some(role.into()), name: Some(name.into()), ..Default::default() })
}

fn click(name: &str) -> Action {
    Action::Click { selector: role("button", name) }
}

fn fill(name: &str, value: &str) -> Action {
    Action::Fill { selector: role("textbox", name), value: value.into() }
}

fn failed_case(case_id: i32, step_number: i32, detail: &str) -> CaseRecord {
    CaseRecord {
        case_id,
        title: format!("case {case_id}"),
        verdict: String::new(),
        note: String::new(),
        steps: vec![StepRecord { step_number, outcomes: vec![ActionOutcome::failed(detail)], screenshot: None }],
        proposed: "Failed".into(),
        reason: format!("step {step_number}: {detail}"),
        duration_ms: None,
        account: None,
    }
}

fn script(case_id: i32, step_number: i32, action: Action) -> CaseScript {
    CaseScript {
        case_id,
        title: format!("case {case_id}"),
        account: None,
        steps: vec![StepScript { step_number, actions: vec![action], unchecked: None }],
        repairs: 0,
        last_repair: None,
    }
}

fn run(cases: Vec<CaseRecord>) -> LocalRun {
    LocalRun { id: "run-1".into(), pbi_id: 1, started_at: "1".into(), cases, mode: "unattended".into(), published: None }
}

/// Every sentence here is the driver's own wording, the way a run file
/// holds it - with and without the target the script names.
#[test]
fn the_classifier_reads_the_real_failure_sentences() {
    let save = "button \"Save\"";
    let cases: Vec<(&str, ErrorClass)> = vec![
        ("waited 5000ms: button \"Save\" not found", ErrorClass::NotFound),
        ("waited 5000ms: button \"Save\" is not on the page", ErrorClass::NotFound),
        ("waited 5000ms and never saw button \"Save\"", ErrorClass::NotFound),
        ("waited 5000ms: button \"Save\" matched 3 elements - narrow it, or add nth", ErrorClass::MatchedMany),
        ("waited 5000ms: button \"Save\" is covered by div.modal-backdrop", ErrorClass::CoveredBy("div.modal-backdrop".into())),
        (
            "moved or was covered just before the click: is covered by div#spinner",
            ErrorClass::CoveredBy("div#spinner".into()),
        ),
        ("waited 5000ms: button \"Save\" is not visible", ErrorClass::NotVisible),
        ("waited 5000ms: button \"Save\" is there but cannot be seen", ErrorClass::NotVisible),
        ("waited 5000ms: button \"Save\" is outside the visible part of the page", ErrorClass::Offscreen),
        ("waited 5000ms: button \"Save\" is disabled", ErrorClass::Disabled),
        ("waited 5000ms: button \"Save\" cannot be typed into", ErrorClass::NotEditable),
        ("waited 5000ms: button \"Save\" is still moving", ErrorClass::StillMoving),
        ("waited 5000ms: button \"Save\" was still being checked when the time ran out", ErrorClass::TimedOut),
        ("waited 5000ms: button \"Save\" is still visible", ErrorClass::StillVisible),
        ("waited 5000ms: button \"Save\" expected text \"Saved\" but saw \"Error\"", ErrorClass::TextMismatch),
        ("waited 5000ms: button \"Save\" expected it to contain \"Sav\" but saw \"x\"", ErrorClass::TextMismatch),
        ("waited 5000ms: button \"Save\" expected 2, counted 0", ErrorClass::CountMismatch),
        ("waited 5000ms: button \"Save\" has no aria-pressed attribute", ErrorClass::AttributeMismatch),
        ("https://app.example/x did not finish loading within 30000ms", ErrorClass::Navigation),
        ("https://app.example/x would not load: net::ERR_NAME_NOT_RESOLVED", ErrorClass::Navigation),
        ("page does NOT contain Welcome", ErrorClass::PageTextMissing),
        ("url is https://app.example/login", ErrorClass::UrlMismatch),
        ("the page refused: Cannot find context with specified id", ErrorClass::PageRefused),
        ("lost focus before it could be typed into - something else on the page took it", ErrorClass::LostFocus),
        ("the list has no option \"Annual\"", ErrorClass::NoOption),
        ("the option \"Annual\" is disabled", ErrorClass::NoOption),
        ("clicking button \"Save\" did not open a file chooser - point upload at the page's file input or the button that opens it", ErrorClass::NoFileChooser),
        ("the browser did not answer: the browser closed before answering", ErrorClass::Browser),
        ("the browser did not answer for 5000ms while waiting for button \"Save\"", ErrorClass::Browser),
        ("this action cannot run: check_text has an empty value", ErrorClass::CannotRun),
    ];
    for (detail, want) in &cases {
        assert_eq!(&classify(detail, Some(save)), want, "with the target: {detail}");
        assert_eq!(&classify(detail, None), want, "without the target: {detail}");
    }
    // A dialog the page raised is appended to the sentence, never part of
    // its class.
    assert_eq!(
        classify("waited 5000ms: button \"Save\" not found (the page showed alert \"x\" and it was accepted)", Some(save)),
        ErrorClass::NotFound
    );
    // A target whose own words read like a reason is stripped first, not
    // matched against.
    assert_eq!(classify("waited 5000ms: text \"not found\" is disabled", Some("text \"not found\"")), ErrorClass::Disabled);
}

#[test]
fn two_cases_failing_the_same_target_the_same_way_are_one_pattern() {
    let r = run(vec![
        failed_case(11, 2, "waited 5000ms: button \"Save\" not found"),
        failed_case(12, 4, "waited 5000ms: button \"Save\" not found"),
    ]);
    let scripts = vec![script(11, 2, click("Save")), script(12, 4, click("Save"))];
    let patterns = find_patterns(&r, &scripts);
    assert_eq!(patterns.len(), 1, "{patterns:?}");
    assert_eq!(patterns[0].class, ErrorClass::NotFound);
    assert_eq!(patterns[0].targets, vec!["click on button \"Save\"".to_string()]);
    assert_eq!(patterns[0].cases(), 2);
}

#[test]
fn one_overlay_covering_different_targets_in_different_cases_is_one_pattern() {
    let r = run(vec![
        failed_case(11, 2, "waited 5000ms: button \"Save\" is covered by div.modal-backdrop"),
        failed_case(12, 3, "waited 5000ms: textbox \"Name\" is covered by div.modal-backdrop"),
    ]);
    let scripts = vec![script(11, 2, click("Save")), script(12, 3, fill("Name", "secret-typed-value"))];
    let patterns = find_patterns(&r, &scripts);
    assert_eq!(patterns.len(), 1, "{patterns:?}");
    assert_eq!(patterns[0].class, ErrorClass::CoveredBy("div.modal-backdrop".into()));
    assert_eq!(
        patterns[0].targets,
        vec!["click on button \"Save\"".to_string(), "fill on textbox \"Name\"".to_string()]
    );
    let text = patterns_section(&patterns);
    assert!(text.contains("covered by div.modal-backdrop (in 2 cases)"), "{text}");
    assert!(!text.contains("secret-typed-value"), "a typed value never reaches a pattern: {text}");
}

#[test]
fn a_failure_in_one_case_only_is_no_pattern() {
    let mut one = failed_case(11, 2, "waited 5000ms: button \"Save\" not found");
    // The same failure twice in ONE case is still one case.
    one.steps.push(StepRecord {
        step_number: 3,
        outcomes: vec![ActionOutcome::failed("waited 5000ms: button \"Save\" not found")],
        screenshot: None,
    });
    let mut s = script(11, 2, click("Save"));
    s.steps.push(StepScript { step_number: 3, actions: vec![click("Save")], unchecked: None });
    let r = run(vec![one, failed_case(12, 2, "waited 5000ms: button \"Other\" not found")]);
    assert!(find_patterns(&r, &[s, script(12, 2, click("Other"))]).is_empty());
}

#[test]
fn a_different_class_on_the_same_target_is_a_different_failure() {
    let r = run(vec![
        failed_case(11, 2, "waited 5000ms: button \"Save\" not found"),
        failed_case(12, 2, "waited 5000ms: button \"Save\" is disabled"),
    ]);
    assert!(find_patterns(&r, &[script(11, 2, click("Save")), script(12, 2, click("Save"))]).is_empty());
}

/// A browser that stopped answering says nothing about the application.
#[test]
fn browser_failures_are_never_a_pattern() {
    let r = run(vec![
        failed_case(11, 2, "the browser did not answer for 5000ms while waiting for button \"Save\""),
        failed_case(12, 2, "the browser did not answer for 5000ms while waiting for button \"Save\""),
    ]);
    assert!(find_patterns(&r, &[script(11, 2, click("Save")), script(12, 2, click("Save"))]).is_empty());
}

#[test]
fn describe_failures_ends_with_the_patterns_section_when_one_repeats() {
    let r = run(vec![
        failed_case(11, 2, "waited 5000ms: button \"Save\" not found"),
        failed_case(12, 4, "waited 5000ms: button \"Save\" not found"),
    ]);
    let scripts = vec![script(11, 2, click("Save")), script(12, 4, click("Save"))];
    let text = describe_failures(&r, &scripts);
    let section = [
        "## Patterns across cases",
        "",
        "- click on button \"Save\" - not found (in 2 cases)",
        "  where: case 11 step 2 action 1, case 12 step 4 action 1",
        "",
        PATTERN_ADVICE,
    ]
    .join("\n");
    assert!(text.ends_with(&section), "{text}");
    assert!(text.starts_with("## Case 11"), "{text}");
}

#[test]
fn describe_failures_has_no_patterns_section_when_nothing_repeats() {
    let r = run(vec![
        failed_case(11, 2, "waited 5000ms: button \"Save\" not found"),
        failed_case(12, 4, "waited 5000ms: button \"Other\" not found"),
    ]);
    let text = describe_failures(&r, &[script(11, 2, click("Save")), script(12, 4, click("Other"))]);
    assert!(!text.contains("Patterns across cases"), "{text}");
    assert!(!text.contains("record_autorun_quirk"), "{text}");
}
