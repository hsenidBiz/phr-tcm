//! The optimizer and the declarative transforms - the two MCP tools that
//! reorganise a draft so the assistant doesn't have to.

use v2_lib::ai_bridge::{route, BridgeContext};
use v2_lib::model::TestCase;
use v2_lib::optimize::{clean_expected, optimize};
use v2_lib::steps_xml::Step;
use v2_lib::transform::{apply, parse_ops};

fn step(action: &str, expected: &str) -> Step {
    Step { action: action.into(), expected: expected.into() }
}

fn case(title: &str, module: &str, pre: &str, steps: Vec<Step>) -> TestCase {
    TestCase {
        title: title.into(),
        steps,
        tags: String::new(),
        automation_status: "Not Automated".into(),
        module_value: module.into(),
        preconditions: pre.into(),
        update_id: None,
        comment: String::new(),
    }
}

// ---------------------------------------------------------------- expected

#[test]
fn expected_results_keep_only_the_outcome() {
    assert_eq!(clean_expected("Verify that the invoice is saved"), "The invoice is saved.");
    assert_eq!(clean_expected("The system should display an error"), "Display an error.");
    assert_eq!(
        clean_expected("The total updates (because tax is recalculated)"),
        "The total updates."
    );
    assert_eq!(
        clean_expected("A confirmation appears. This proves the flow works."),
        "A confirmation appears."
    );
    assert_eq!(
        clean_expected("The row is removed Note: the audit log also records it"),
        "The row is removed."
    );
}

/// Trimming must never leave an empty expected - an untidy sentence beats
/// a blank one.
#[test]
fn a_bare_noise_phrase_is_left_alone_rather_than_emptied() {
    assert_eq!(clean_expected("Verify"), "Verify.");
    assert_eq!(clean_expected(""), "");
}

// ---------------------------------------------------------------- preamble

#[test]
fn navigation_moves_out_of_preconditions_and_becomes_steps() {
    let c = case(
        "Apply a discount",
        "Payments",
        "User is logged in as Admin. User is on the Payments page. Feature flag DISCOUNTS is on",
        vec![step("Enter a discount code", "Verify the discount applies")],
    );
    let (out, report) = optimize(vec![c], Some("Launch the HRM portal."));
    let c = &out[0];

    let actions: Vec<&str> = c.steps.iter().map(|s| s.action.as_str()).collect();
    assert_eq!(actions[0], "Launch the HRM portal.");
    assert!(actions[1].starts_with("Sign in as Admin"), "got {:?}", actions[1]);
    assert!(actions[2].to_lowercase().contains("payments page"), "got {:?}", actions[2]);
    assert_eq!(actions[3], "Enter a discount code");
    assert!(report.preamble_steps_added >= 3);

    // Only real setup survives in preconditions - the navigation is gone.
    assert!(!c.preconditions.to_lowercase().contains("is on the"));
    assert!(c.preconditions.contains("Feature flag DISCOUNTS is on"));
    // ...and the expected result was trimmed on the way through.
    assert_eq!(c.steps[3].expected, "The discount applies.");
}

#[test]
fn a_case_that_already_walks_in_is_not_given_a_second_preamble() {
    let c = case(
        "Already complete",
        "Payments",
        "",
        vec![step("Launch the app", "It opens"), step("Do the thing", "It works")],
    );
    let (out, report) = optimize(vec![c], None);
    assert_eq!(out[0].steps.len(), 2, "no preamble bolted on top");
    assert_eq!(report.preamble_steps_added, 0);
}

// ---------------------------------------------------------------- ordering

/// The headline behaviour: interleaved setups get grouped, so the tester
/// changes environment as few times as possible.
#[test]
fn cases_are_reordered_to_minimise_environment_switches() {
    let admin = "User is logged in as Admin";
    let viewer = "User is logged in as Viewer";
    let draft = vec![
        case("A1", "Payments", admin, vec![step("a", "b")]),
        case("V1", "Payments", viewer, vec![step("a", "b")]),
        case("A2", "Payments", admin, vec![step("a", "b")]),
        case("V2", "Payments", viewer, vec![step("a", "b")]),
        case("A3", "Payments", admin, vec![step("a", "b")]),
    ];
    let (out, report) = optimize(draft, None);

    let titles: Vec<&str> = out.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles.len(), 5);
    // Each role's cases are contiguous.
    let first_v = titles.iter().position(|t| t.starts_with('V')).unwrap();
    let last_v = titles.iter().rposition(|t| t.starts_with('V')).unwrap();
    assert_eq!(last_v - first_v, 1, "the two Viewer cases run together: {titles:?}");

    assert_eq!(report.switches_before, 4, "the draft switched on every case");
    assert_eq!(report.switches_after, 1, "one switch is the minimum for two setups");
    assert_eq!(report.groups.len(), 2);
    // The bigger group runs first - the longest uninterrupted stretch.
    assert_eq!(report.groups[0].cases, 3);
}

#[test]
fn groups_are_chained_so_each_switch_changes_as_little_as_possible() {
    // Two Payments setups and one Reports setup: the Payments pair should
    // sit next to each other rather than being split by Reports.
    let draft = vec![
        case("P-admin", "Payments", "Logged in as Admin", vec![step("a", "b")]),
        case("R-admin", "Reports", "Logged in as Admin", vec![step("a", "b")]),
        case("P-viewer", "Payments", "Logged in as Viewer", vec![step("a", "b")]),
    ];
    let (out, _) = optimize(draft, None);
    let titles: Vec<&str> = out.iter().map(|c| c.title.as_str()).collect();
    let payments: Vec<usize> = titles
        .iter()
        .enumerate()
        .filter(|(_, t)| t.starts_with("P-"))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(payments[1] - payments[0], 1, "Payments cases adjacent: {titles:?}");
}

// ---------------------------------------------------------------- tidying

#[test]
fn duplicates_empty_steps_and_stray_values_are_cleaned_up() {
    let mut dup = case("Same title", "M", "", vec![step("a", "b")]);
    dup.tags = "smoke, Smoke ; regression".into();
    dup.automation_status = "Automated".into(); // not a value ADO accepts here
    let mut with_empty = case("Other", "M", "", vec![step("a", "b"), step("  ", "  ")]);
    with_empty.tags = String::new();

    let (out, report) = optimize(
        vec![dup, case("same TITLE", "M", "", vec![step("x", "y")]), with_empty],
        None,
    );

    assert_eq!(out.len(), 2, "the case-insensitive duplicate went");
    assert_eq!(report.duplicates_removed, 1);
    assert_eq!(report.empty_steps_removed, 1);
    assert_eq!(out[0].tags, "smoke; regression", "deduped case-insensitively");
    assert_eq!(out[0].automation_status, "Not Automated");
}

// ---------------------------------------------------------------- transform

fn ops(json: serde_json::Value) -> Vec<v2_lib::transform::Operation> {
    parse_ops(&json).expect("valid ops")
}

#[test]
fn transforms_edit_only_the_cases_the_filter_names() {
    let draft = vec![
        {
            let mut c = case("Login works", "Auth", "", vec![step("a", "b")]);
            c.tags = "smoke".into();
            c
        },
        case("Payment works", "Payments", "", vec![step("a", "b")]),
    ];
    let (out, report) = apply(
        draft,
        &ops(serde_json::json!([
            { "op": "add_tags", "value": "regression", "where": { "has_tag": "smoke" } },
            { "op": "set_module", "value": "Billing", "where": { "title_contains": "payment" } },
        ])),
    );

    assert_eq!(out[0].tags, "smoke; regression");
    assert_eq!(out[1].tags, "", "the filter kept this one out of it");
    assert_eq!(out[0].module_value, "Auth");
    assert_eq!(out[1].module_value, "Billing");
    assert_eq!(report.cases_in, 2);
    assert_eq!(report.cases_out, 2);
}

#[test]
fn adding_a_tag_that_is_already_there_does_not_duplicate_it() {
    let mut c = case("T", "M", "", vec![]);
    c.tags = "Smoke".into();
    let (out, _) = apply(vec![c], &ops(serde_json::json!([{ "op": "add_tags", "value": "smoke" }])));
    assert_eq!(out[0].tags, "Smoke");
}

#[test]
fn titles_and_steps_can_be_rewritten_in_bulk() {
    let draft = vec![case("Old name", "M", "", vec![step("Click Old", "Old is gone")])];
    let (out, _) = apply(
        draft,
        &ops(serde_json::json!([
            { "op": "replace_in_title", "find": "Old", "replace": "New" },
            { "op": "prefix_title", "value": "[UI] " },
            { "op": "replace_in_steps", "find": "Old", "replace": "New" },
        ])),
    );
    assert_eq!(out[0].title, "[UI] New name");
    assert_eq!(out[0].steps[0].action, "Click New");
    assert_eq!(out[0].steps[0].expected, "New is gone");
}

#[test]
fn sorting_and_deduping_work_on_the_whole_draft() {
    let draft = vec![
        case("Zebra", "M", "", vec![]),
        case("apple", "M", "", vec![]),
        case("APPLE", "M", "", vec![]),
    ];
    let (out, report) = apply(
        draft,
        &ops(serde_json::json!([{ "op": "dedupe" }, { "op": "sort_by", "value": "title" }])),
    );
    assert_eq!(out.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(), vec!["apple", "Zebra"]);
    assert!(report.applied.iter().any(|l| l.contains("1 duplicate")));
}

/// A silently-skipped edit is worse than a rejected one.
#[test]
fn bad_operations_are_rejected_by_name() {
    assert!(parse_ops(&serde_json::json!([{ "op": "delete_everything" }]))
        .unwrap_err()
        .contains("unknown op"));
    assert!(parse_ops(&serde_json::json!([{ "op": "set_automation_status", "value": "Automated" }]))
        .unwrap_err()
        .contains("Not Automated"));
    // An empty needle would splice the replacement between every character.
    assert!(parse_ops(&serde_json::json!([{ "op": "replace_in_title", "find": "" }]))
        .unwrap_err()
        .contains("must not be empty"));
    assert!(parse_ops(&serde_json::json!({ "op": "dedupe" })).is_err(), "must be a list");
}

#[test]
fn a_filter_that_matches_nothing_says_so() {
    let (_, report) = apply(
        vec![case("A", "M", "", vec![])],
        &ops(serde_json::json!([
            { "op": "add_tags", "value": "x", "where": { "has_tag": "nope" } }
        ])),
    );
    assert!(report.applied.iter().any(|l| l.contains("nothing matched")));
}

// ---------------------------------------------------------------- routes

/// Both tools must work with no sign-in: they are pure transforms, and
/// blocking them behind a token would be pointless friction.
#[tokio::test]
async fn the_bridge_exposes_both_tools_without_a_client() {
    let ctx = BridgeContext::default();
    let draft = r#"[{"title":"A","preconditions":"User is on the Orders page",
        "steps":[{"action":"Do it","expected":"Verify that it worked"}]}]"#;

    let (status, body) = route(&ctx, None, "POST", "/optimize", draft, "test").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let steps = &v["test_cases"][0]["steps"];
    assert!(
        steps[0]["action"].as_str().unwrap().starts_with("Launch"),
        "preamble is present: {steps}"
    );
    assert_eq!(v["test_cases"][0]["steps"][2]["expected"], "It worked.");
    assert!(v["report"]["groups"].is_array());

    // transform takes the draft as a string alongside its operations.
    let body_in = serde_json::json!({
        "test_cases": draft,
        "operations": [{ "op": "add_tags", "value": "smoke" }],
    })
    .to_string();
    let (status, body) = route(&ctx, None, "POST", "/transform", &body_in, "test").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["test_cases"][0]["tags"], "smoke");
}

#[tokio::test]
async fn the_removed_validate_route_is_gone() {
    let ctx = BridgeContext::default();
    let (status, _) = route(&ctx, None, "POST", "/validate", "[]", "test").await;
    assert_eq!(status, 404, "validate_cases was removed on purpose");
}

#[tokio::test]
async fn a_malformed_draft_is_a_400_with_an_explanation() {
    let ctx = BridgeContext::default();
    let (status, body) = route(&ctx, None, "POST", "/optimize", "{not json", "test").await;
    assert_eq!(status, 400);
    assert!(!body.is_empty());
}
