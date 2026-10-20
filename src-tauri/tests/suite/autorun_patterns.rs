//! The same failure across cases: grouped by action, target and error
//! class, or by the element that covered them - and reported to an
//! assistant only when at least two different cases hit it.

use v2_lib::autorun::failures::describe_failures;
use v2_lib::autorun::patterns::{action_target, classify, find_patterns, patterns_section, ErrorClass, PATTERN_ADVICE};
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
        steps: vec![StepRecord { step_number, outcomes: vec![ActionOutcome::failed(detail)], screenshot: None, downloads: vec![], tab: None, dialog: None, components: Vec::new(), duration_ms: None }],
        proposed: "Failed".into(),
        reason: format!("step {step_number}: {detail}"),
        duration_ms: None,
        account: None,
        retried: None,
        notice: None,
        page_errors_seen: 0, phases: None, cleanup: None,
    }
}

fn script(case_id: i32, step_number: i32, action: Action) -> CaseScript {
    CaseScript {
        case_id,
        title: format!("case {case_id}"),
        account: None,
        area: None,
        steps: vec![StepScript { step_number, actions: vec![action], unchecked: None, cleanup: false }],
        repairs: 0,
        last_repair: None,
        suspected_defect: None,
        no_save: false,
        preconditions: vec![],
        precondition_repairs: vec![],
        setup: None,
        changes: vec![],
        needs_unchanged: vec![],
        needs_state: Default::default(),
        saved_at: None,
        fail_on_unexpected_dialog: false,
        page_errors: None, ignore_page_errors: vec![], organization: None, project: None, checked: false,
    }
}

fn run(cases: Vec<CaseRecord>) -> LocalRun {
    LocalRun { id: "run-1".into(), pbi_id: 1, started_at: "1".into(), cases, mode: "unattended".into(), published: None, environment: None, resets: vec![], network_pauses: vec![] }
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

/// "Cannot reach this frame" is its own class: the driver's own sentence
/// (`locator::frame_unreachable`), alone or behind the waited prefix and a
/// chain's target. Its key and label are fixed.
#[test]
fn a_frame_auto_run_cannot_reach_is_its_own_class() {
    let frame = v2_lib::browser::locator::frame_unreachable("css \"iframe#pay\"");
    let chain = "css \"iframe#pay\" >> button \"Pay\"";
    for detail in [frame.clone(), format!("waited 5000ms: {chain} {frame}"), format!("waited 5000ms: {frame}")] {
        assert_eq!(classify(&detail, Some(chain)), ErrorClass::FrameUnreachable, "with the target: {detail}");
        assert_eq!(classify(&detail, None), ErrorClass::FrameUnreachable, "without the target: {detail}");
    }
    assert_eq!(ErrorClass::FrameUnreachable.key(), "frame");
    assert_eq!(ErrorClass::FrameUnreachable.label(), "a frame Auto Run cannot reach");
}

/// It is about the application, so get_autorun_failures groups it in
/// patterns like any other such class (a browser failure never is one).
#[test]
fn two_cases_failing_on_the_same_unreachable_frame_are_one_pattern() {
    let frame = v2_lib::browser::locator::frame_unreachable("css \"iframe#pay\"");
    let r = run(vec![
        failed_case(11, 2, &format!("waited 5000ms: {frame}")),
        failed_case(12, 4, &format!("waited 5000ms: {frame}")),
    ]);
    let scripts = vec![script(11, 2, click("Pay")), script(12, 4, click("Pay"))];
    let text = describe_failures(&r, &scripts);
    assert!(text.contains("- click on button \"Pay\" - a frame Auto Run cannot reach (in 2 cases)"), "{text}");
}

#[test]
fn an_api_checks_target_drops_the_query_string_and_fragment() {
    let watch: Action = serde_json::from_value(serde_json::json!({
        "kind": "expect_response", "url_contains": " /hr/Cycle/Save?access_token=abc#top "
    }))
    .unwrap();
    assert_eq!(action_target(&watch).as_deref(), Some("/hr/Cycle/Save"));
    let ask: Action = serde_json::from_value(serde_json::json!({ "kind": "api_request", "path": "/api/me?token=abc#x" })).unwrap();
    assert_eq!(action_target(&ask).as_deref(), Some("/api/me"));
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
        downloads: vec![],
        tab: None,
        dialog: None,
        components: Vec::new(), duration_ms: None,
    });
    let mut s = script(11, 2, click("Save"));
    s.steps.push(StepScript { step_number: 3, actions: vec![click("Save")], unchecked: None, cleanup: false });
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

/// An overlay whose later classes change as it animates is one overlay:
/// grouped on its tag, id and first class only.
#[test]
fn an_overlay_is_grouped_on_its_tag_id_and_first_class() {
    use v2_lib::autorun::patterns::overlay_key;
    assert_eq!(overlay_key("div.modal.fade.show"), "div.modal");
    assert_eq!(overlay_key("div#dlg.modal.fade"), "div#dlg.modal");
    assert_eq!(overlay_key("div#spinner"), "div#spinner");
    assert_eq!(overlay_key("another element"), "another element");

    let r = run(vec![
        failed_case(11, 2, "waited 5000ms: button \"Save\" is covered by div.modal.fade"),
        failed_case(12, 3, "waited 5000ms: textbox \"Name\" is covered by div.modal.fade.show"),
    ]);
    let patterns = find_patterns(&r, &[script(11, 2, click("Save")), script(12, 3, fill("Name", "x"))]);
    assert_eq!(patterns.len(), 1, "{patterns:?}");
    assert_eq!(patterns[0].class, ErrorClass::CoveredBy("div.modal".into()));
    assert!(patterns_section(&patterns).contains("covered by div.modal (in 2 cases)"));
}

/// The advice points at the edit of every repaired case, and at `cases`.
#[test]
fn the_advice_says_how_to_tie_the_quirk_to_the_cases() {
    assert!(PATTERN_ADVICE.contains("EVERY case you repair"), "{PATTERN_ADVICE}");
    assert!(PATTERN_ADVICE.contains("`cases`"), "{PATTERN_ADVICE}");
}

// ---- components ----

#[test]
fn a_component_step_is_classified_by_its_expanded_action() {
    use crate::common::{component_case, component_run, component_script, pick_a_date, pick_a_date_file, COMPONENT_TYPED};
    use v2_lib::autorun::components::{expand, ComponentFile};
    use v2_lib::autorun::patterns::{find_patterns_with, step_failures_with};
    let missing = "waited 5000ms: #start not found";
    let run = component_run(vec![component_case(11, missing), component_case(12, missing)]);
    let scripts = [component_script(11), component_script(12)];
    let typed = expand(
        &pick_a_date(),
        serde_json::json!({ "field": { "css": "#start" }, "day": COMPONENT_TYPED }).as_object().unwrap(),
    )
    .unwrap();

    // The failure is the component's fill, not the script's third action.
    let points = step_failures_with(&run.cases[0].steps[0], Some(&scripts[0]), &pick_a_date_file());
    assert_eq!(points.len(), 2, "{points:?}");
    assert_eq!((points[0].action, points[0].kind.as_deref()), (3, Some("fill")));
    assert_eq!(points[0].target, action_target(&typed[1]));
    assert_eq!((points[1].action, points[1].kind.as_deref()), (5, Some("check_text")));

    let patterns = find_patterns_with(&run, &scripts, &pick_a_date_file());
    let p = patterns
        .iter()
        .find(|p| p.targets.iter().any(|t| t.starts_with("fill on ")))
        .unwrap_or_else(|| panic!("{patterns:?}"));
    assert_eq!(p.targets, vec![format!("fill on {}", action_target(&typed[1]).unwrap())]);
    assert_eq!(p.cases(), 2);

    // Without the components, still lined up: classed by its component.
    let points = step_failures_with(&run.cases[0].steps[0], Some(&scripts[0]), &ComponentFile::default());
    assert_eq!(points[0].kind.as_deref(), Some("use_component"));
    assert_eq!(points[0].target.as_deref(), Some("the component \"Pick a date\""));
    assert_eq!(points[1].kind.as_deref(), Some("check_text"));
}

#[test]
fn a_refusal_by_the_app_is_its_own_class() {
    let refused = r#"the app refused: "Weightage must total 100." (POST /api/eval-rules, 400, trace id 00-4bf9-00)"#;
    assert_eq!(classify(refused, None), ErrorClass::AppRefused);
    assert_eq!(classify(&format!(r#"{refused}; the page showed: "is not visible""#), None), ErrorClass::AppRefused);
    assert_eq!(ErrorClass::AppRefused.key(), "app_refused");
    assert_eq!(ErrorClass::AppRefused.label(), "the application refused the action");
    // A later failure that a refusal came before.
    let later = format!("waited 100ms and never saw css #goals - after click css #save {refused}");
    assert_eq!(classify(&later, None), ErrorClass::AppRefused);
    // The browser's own refusal is not the application's.
    assert_eq!(classify("the page refused: Cannot find node", None), ErrorClass::PageRefused);
}

#[test]
fn what_the_page_showed_never_decides_the_class() {
    let target = r#"button "Save""#;
    let base = format!("waited 5000ms: {target} is not visible");
    let noted = format!(r#"{base} - after click css #a the page showed: "is disabled""#);
    assert_eq!(classify(&noted, Some(target)), ErrorClass::NotVisible);
    assert_eq!(classify(&noted, None), classify(&base, None));
}

const REFUSED_LINE: &str = r#"the app refused: "Weightage must total 100." (POST /api/eval-rules, 400, trace id 00-4bf9-00)"#;

#[test]
fn a_blocked_save_or_an_overlay_keeps_its_class_after_a_refusal() {
    let blocked = v2_lib::browser::save_guard::blocked("POST", "https://hr.example/api/Save");
    let noted = format!("{blocked} - after click css #a {REFUSED_LINE}");
    assert_eq!(classify(&noted, None), ErrorClass::SaveBlocked);
    let target = r#"button "Save""#;
    let covered = format!("waited 5000ms: {target} is covered by div.modal.fade - after click css #a {REFUSED_LINE}");
    assert_eq!(classify(&covered, Some(target)), ErrorClass::CoveredBy("div.modal.fade".into()));
}

#[test]
fn the_note_is_found_from_the_end() {
    // The failure's own sentence quotes " - after ", and so does the page.
    let target = r#"button "Go - after review""#;
    let base = format!("waited 5000ms: {target} is not visible");
    let noted = format!(r#"{base} - after click css #a the page showed: "Saved - after review""#);
    assert_eq!(classify(&base, Some(target)), ErrorClass::NotVisible);
    assert_eq!(classify(&noted, Some(target)), ErrorClass::NotVisible);
    // The page's words never make it a refusal.
    let page = format!(r#"{base} - after click css #a the page showed: "the app refused: nothing""#);
    assert_eq!(classify(&page, Some(target)), ErrorClass::NotVisible);
}

#[test]
fn a_refusal_names_its_request() {
    use v2_lib::autorun::patterns::refused_request;
    assert_eq!(refused_request(REFUSED_LINE).as_deref(), Some("POST /api/eval-rules"));
    let later = format!("waited 100ms and never saw css #goals - after click css #save {REFUSED_LINE}");
    assert_eq!(refused_request(&later).as_deref(), Some("POST /api/eval-rules"));
    // Quoted words that look like a bracket are passed over.
    let tricky = r#"the app refused: "Bad" (input" (PUT /api/x, 409)"#;
    assert_eq!(refused_request(tricky).as_deref(), Some("PUT /api/x"));
    assert_eq!(refused_request(r#"waited 100ms and never saw css #goals"#), None);
    let shown = r#"waited 100ms and never saw css #goals - after click css #a the page showed: "x""#;
    assert_eq!(refused_request(shown), None);
}

#[test]
fn refusals_of_one_request_are_one_pattern_whatever_failed_after() {
    let run = run(vec![
        failed_case(1, 1, &format!("waited 100ms and never saw css #goals - after click css #save {REFUSED_LINE}")),
        failed_case(2, 1, &format!("page does NOT contain Saved - after click css #next {REFUSED_LINE}")),
    ]);
    let scripts = vec![
        script(1, 1, Action::WaitFor { selector: role("heading", "Goals"), timeout_ms: 100 }),
        script(2, 1, Action::CheckText { value: "Saved".into() }),
    ];
    let found = find_patterns(&run, &scripts);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].class, ErrorClass::AppRefused);
    assert_eq!(found[0].request.as_deref(), Some("POST /api/eval-rules"));
    assert_eq!(found[0].cases(), 2);
    let section = patterns_section(&found);
    assert!(section.contains("the application refused the action: POST /api/eval-rules (in 2 cases)"), "{section}");
}

/// A style check's failure is its own class, read from the check's own
/// sentence with or without the target in front; a property still
/// changing when the time ran out is something still moving. A downloaded
/// cell's mismatch (`b2 is "x", expected "y"`) is not a style.
#[test]
fn a_style_mismatch_is_its_own_class() {
    let save = "button \"Save\"";
    for (detail, want) in [
        ("waited 5000ms: button \"Save\" color is rgb(0, 0, 0), expected rgb(255, 0, 0)", ErrorClass::StyleMismatch),
        ("waited 5000ms: button \"Save\" box-shadow is none, expected it to contain 3px", ErrorClass::StyleMismatch),
        ("waited 5000ms: button \"Save\" display is none, expected anything but none", ErrorClass::StyleMismatch),
        ("waited 5000ms: button \"Save\" --brand is (empty), expected 1", ErrorClass::StyleMismatch),
        ("waited 5000ms: button \"Save\" color is still changing (rgb(9, 0, 0) so far)", ErrorClass::StillMoving),
    ] {
        assert_eq!(classify(detail, Some(save)), want, "with the target: {detail}");
        assert_eq!(classify(detail, None), want, "without the target: {detail}");
    }
    assert_ne!(classify("b2 is \"x\", expected \"y\"", None), ErrorClass::StyleMismatch);
    assert_eq!(ErrorClass::StyleMismatch.key(), "style_mismatch");
    assert_eq!(ErrorClass::StyleMismatch.label(), "a different style");
    let style: Action = serde_json::from_value(serde_json::json!({ "kind": "expect_style", "selector": { "role": "button", "name": "Save" }, "property": "color", "equals": "red" })).unwrap();
    assert_eq!(action_target(&style).as_deref(), Some(save));
}
