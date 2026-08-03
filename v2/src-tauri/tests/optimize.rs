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
        reviewer_notes: String::new(),
        spec_order: None,
        tester_order: None,
    }
}

/// A short parenthetical is usually part of the UI string being asserted,
/// not commentary on it. `The badge reads Rejected (Edit) in red` was
/// trimmed to `Rejected` - and `Rejected (Edit)` is the literal value from
/// the spec's status-derivation table, so the trim did not shorten the
/// assertion, it falsified it: a badge reading plain `Rejected` now passes.
#[test]
fn a_short_parenthetical_is_a_ui_string_not_an_aside() {
    assert_eq!(
        clean_expected("The badge reads Rejected (Edit) in red"),
        "The badge reads Rejected (Edit) in red."
    );
    assert_eq!(clean_expected("The field is labelled Reason (optional)"), "The field is labelled Reason (optional).");
    // Three words or more is commentary, and still goes.
    assert_eq!(
        clean_expected("A count is shown (e.g. \"5 employees\")"),
        "A count is shown."
    );
    assert_eq!(
        clean_expected("The total updates (as configured in Settings)"),
        "The total updates."
    );
}

/// Declining to add an entry step is correct when the draft already walks
/// in - but it used to be invisible. A caller who passed `entry` got
/// `preamble_steps_added: 0`, no entry step and no reason, which is
/// indistinguishable from the parameter being ignored.
#[test]
fn a_declined_entry_step_says_why() {
    let c = case(
        "Approve a request",
        "",
        "",
        vec![
            Step {
                action: "In the PMS Module, open Performance Management from the main menu.".into(),
                expected: "The list is shown.".into(),
            },
            Step { action: "Click Approve.".into(), expected: "It is approved.".into() },
        ],
    );
    let (_out, report) = optimize(
        vec![c],
        Some("In the PMS Module, open Performance Management from the main menu."),
    );
    assert_eq!(report.preamble_steps_added, 0, "nothing should be prepended");
    assert!(
        report.notes.iter().any(|n| n.contains("entry step omitted")
            && n.contains("Approve a request")),
        "the report has to say the entry was declined and for which case: {:?}",
        report.notes
    );
}

/// A set written to be read against a specification is in document order
/// on purpose, and regrouping it by setup destroys the one property that
/// made it reviewable. Everything else still has to happen.
#[test]
fn reorder_false_keeps_document_order_but_still_cleans_up() {
    let mk = |title: &str, pre: &str| {
        case(
            title,
            "",
            pre,
            vec![Step { action: "Click Submit.".into(), expected: "Verify that it saves".into() }],
        )
    };
    // Deliberately alternating setups: the tester ordering would group
    // these into A,A,B and the spec ordering must not.
    let cases = vec![
        mk("First - spec 3.1", "Signed in as the manager"),
        mk("Second - spec 3.2", "Signed in as the employee"),
        mk("Third - spec 3.3", "Signed in as the manager"),
    ];

    let (kept, _) = v2_lib::optimize::optimize_with(cases.clone(), None, false);
    assert_eq!(
        kept.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(),
        vec!["First - spec 3.1", "Second - spec 3.2", "Third - spec 3.3"],
        "document order has to survive"
    );
    // The rest of the pass still ran: the expected result was trimmed.
    assert!(
        kept.iter().all(|c| c.steps.iter().all(|s| !s.expected.to_lowercase().starts_with("verify"))),
        "reorder=false must not switch the rest of the optimizer off"
    );

    // And the default still regroups, or the flag would be meaningless.
    let (grouped, _) = v2_lib::optimize::optimize_with(cases, None, true);
    assert_ne!(
        grouped.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(),
        vec!["First - spec 3.1", "Second - spec 3.2", "Third - spec 3.3"],
        "the tester ordering should have moved something"
    );
}

/// `reorder` decides only which order the ARRAY follows - it no longer
/// costs the other one. Both readings are stamped on every case, agree
/// between the two calls, and each is a complete 1..=N numbering.
#[test]
fn both_orders_are_stamped_whatever_the_array_order_is() {
    let mk = |title: &str, pre: &str| {
        case(title, "", pre, vec![Step { action: "Click.".into(), expected: String::new() }])
    };
    // Alternating setups so spec order and tester order genuinely differ.
    let draft = vec![
        mk("First", "Signed in as the manager"),
        mk("Second", "Signed in as the employee"),
        mk("Third", "Signed in as the manager"),
    ];

    let by_title = |list: &[v2_lib::model::TestCase]| {
        list.iter()
            .map(|c| (c.title.clone(), (c.spec_order, c.tester_order)))
            .collect::<std::collections::HashMap<_, _>>()
    };

    let (spec_array, _) = v2_lib::optimize::optimize_with(draft.clone(), None, false);
    let (tester_array, _) = v2_lib::optimize::optimize_with(draft, None, true);

    // The stamps are the same numbers regardless of which array order the
    // caller asked for - they describe the SET, not the file layout.
    assert_eq!(by_title(&spec_array), by_title(&tester_array));

    // spec_order follows the document; tester_order groups the managers.
    let m = by_title(&spec_array);
    assert_eq!(m["First"].0, Some(1));
    assert_eq!(m["Second"].0, Some(2));
    assert_eq!(m["Third"].0, Some(3));
    let manager_ranks = [m["First"].1.unwrap(), m["Third"].1.unwrap()];
    assert_eq!(
        (manager_ranks[0] as i64 - manager_ranks[1] as i64).abs(),
        1,
        "the two manager cases must be adjacent in the tester reading: {m:?}"
    );

    // Each reading is a full permutation of 1..=3 - a duplicate or a gap
    // would sort into nonsense.
    for pick in [0usize, 1] {
        let mut ranks: Vec<u32> = m.values().map(|v| [v.0, v.1][pick].unwrap()).collect();
        ranks.sort_unstable();
        assert_eq!(ranks, vec![1, 2, 3]);
    }

    // And the tester array really is laid out in tester_order.
    let laid_out: Vec<u32> = tester_array.iter().map(|c| c.tester_order.unwrap()).collect();
    assert_eq!(laid_out, vec![1, 2, 3], "reorder=true lays the file out in the tester reading");
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

/// validate_cases is BACK (owner decision after field feedback rated it
/// the most trustworthy tool): body-based validation works offline.
#[tokio::test]
async fn validate_runs_the_real_importer_again() {
    let ctx = BridgeContext::default();
    let good = r#"[{"title":"A","steps":[{"action":"do","expected":"ok"}]}]"#;
    let (status, body) = route(&ctx, None, "POST", "/validate", good, "test").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["cases"], 1);
    assert!(v["error"].is_null());
}

/// Large drafts validate from a local file via ?path= - no splitting, and
/// no subagent tempted to fabricate a result it could not obtain.
#[tokio::test]
async fn validate_accepts_a_file_path_for_large_drafts() {
    let dir = std::env::temp_dir().join("tcm-validate-path-test");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("draft.json");
    std::fs::write(&file, r#"[{"title":"From disk","steps":[{"action":"a","expected":"b"}]}]"#)
        .unwrap();

    let ctx = BridgeContext::default();
    let target = format!("/validate?path={}", file.to_string_lossy().replace('\\', "%5C").replace(' ', "%20"));
    let (status, body) = route(&ctx, None, "POST", &target, "", "test").await;
    let _ = std::fs::remove_file(&file);
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["cases"], 1, "body: {body}");

    // A path that does not exist is an explicit error, never a pass.
    let (_, body) = route(&ctx, None, "POST", "/validate?path=C:%5Cnowhere%5Cx.json", "", "test").await;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(!v["error"].is_null());
}

/// An empty body with no path is an explicit error - "nothing arrived"
/// must never read as "nothing wrong".
#[tokio::test]
async fn validate_refuses_an_empty_draft() {
    let ctx = BridgeContext::default();
    let (_, body) = route(&ctx, None, "POST", "/validate", "  ", "test").await;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v["error"].as_str().unwrap().contains("empty draft"));
    assert_eq!(v["cases"], 0);
}

/// dry_run: the report comes back alone, so the damage (if any) can be
/// inspected before committing to the transformed JSON.
#[tokio::test]
async fn optimize_dry_run_returns_only_the_report() {
    let ctx = BridgeContext::default();
    let draft = r#"[{"title":"A","preconditions":"User is on the Orders page",
        "steps":[{"action":"Do it","expected":"Verify that it worked"}]}]"#;
    let (status, body) = route(&ctx, None, "POST", "/optimize?dry_run=true", draft, "test").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v.get("test_cases").is_none(), "no JSON on a dry run");
    assert!(v["report"]["preconditions_rewritten"].is_array());
}

#[tokio::test]
async fn a_malformed_draft_is_a_400_with_an_explanation() {
    let ctx = BridgeContext::default();
    let (status, body) = route(&ctx, None, "POST", "/optimize", "{not json", "test").await;
    assert_eq!(status, 400);
    assert!(!body.is_empty());
}

// ------------------------------------------------- feedback regressions
// These reproduce the two failure runs from the 2026-07-27 field report
// (tcm-testcases-mcp-feedback.md). Run 1 duplicated the entry step and put
// module-open before sign-in; run 2 silently deleted the sign-in clause
// from every precondition. Neither may ever happen again.

/// Run 1: the draft's first step already IS the entry. No duplicate.
#[test]
fn an_entry_already_present_as_the_first_step_is_not_duplicated() {
    let entry = "In the PMS Module, open Performance Management from the main menu.";
    let c = case(
        "Tab renders for a manager",
        "Performance",
        "Signed in as a user who appears as manager_emp_number in a published cycle",
        vec![
            step(entry, "The module opens"),
            step("Click the My Team Assessment tab", "The tab opens"),
        ],
    );
    let (out, report) = optimize(vec![c], Some(entry));
    let c = &out[0];

    let entry_count = c
        .steps
        .iter()
        .filter(|s| s.action.trim_end_matches('.') == entry.trim_end_matches('.'))
        .count();
    assert_eq!(entry_count, 1, "steps: {:?}", c.steps.iter().map(|s| &s.action).collect::<Vec<_>>());
    assert_eq!(report.preamble_steps_added, 0);
    // And because nothing was added, the preconditions are UNTOUCHED.
    assert!(
        c.preconditions.contains("manager_emp_number"),
        "the sign-in context survived: {:?}",
        c.preconditions
    );
}

/// Run 2: the draft starts with its own navigation, so no preamble is
/// added - and the sign-in precondition must therefore survive verbatim.
/// This is the case that came back with empty preconditions in the field.
#[test]
fn preconditions_survive_when_no_preamble_is_added() {
    let c = case(
        "The tab is not rendered for a non-manager",
        "Performance",
        "Signed in as a user who is NOT a manager in any published cycle",
        vec![step("Navigate to Self Service -> My Assessments", "The page opens")],
    );
    let before = c.preconditions.clone();
    let (out, report) = optimize(
        vec![c],
        Some("In the PMS Module, open Performance Management from the main menu."),
    );

    assert_eq!(out[0].preconditions, before, "untouched, not stripped");
    assert_eq!(report.preamble_steps_added, 0);
    assert!(report.preconditions_rewritten.is_empty());
}

/// The atomic rule from the other side: when a preamble IS built, only the
/// sentences that became steps leave preconditions - sign-in stays, since
/// it defines the setup group as well as being an action.
#[test]
fn only_sentences_that_became_steps_leave_preconditions() {
    let c = case(
        "Apply a discount",
        "Payments",
        "Signed in as Admin. User is on the Payments page. Feature flag DISCOUNTS is on",
        vec![step("Enter a code", "It applies")],
    );
    let (out, report) = optimize(vec![c], None);
    let c = &out[0];

    // The navigation sentence became a step and left preconditions...
    assert!(c.steps.iter().any(|s| s.action.to_lowercase().contains("payments page")));
    assert!(!c.preconditions.to_lowercase().contains("payments page"));
    // ...the sign-in clause did NOT leave, even though it also earned a step.
    assert!(c.preconditions.contains("Signed in as Admin"), "got {:?}", c.preconditions);
    assert!(c.steps.iter().any(|s| s.action.starts_with("Sign in as Admin")));
    assert!(c.preconditions.contains("Feature flag DISCOUNTS is on"));

    // And the rewrite is visible in the report, not just implied by counts.
    assert_eq!(report.preconditions_rewritten.len(), 1);
    let rw = &report.preconditions_rewritten[0];
    assert!(rw.before.to_lowercase().contains("payments page"));
    assert!(!rw.after.to_lowercase().contains("payments page"));
}

/// Fix 3 from the report: sign in comes before a non-launch entry.
#[test]
fn a_non_launch_entry_is_placed_after_the_sign_in_step() {
    let c = case(
        "Counts every subordinate",
        "Performance",
        "Signed in as a manager with three subordinates",
        vec![step("Open the Team Members card", "Counts are shown")],
    );
    let entry = "In the PMS Module, open Performance Management from the main menu.";
    let (out, _) = optimize(vec![c], Some(entry));
    let actions: Vec<&str> = out[0].steps.iter().map(|s| s.action.as_str()).collect();

    let signin = actions.iter().position(|a| a.starts_with("Sign in")).expect("sign-in step");
    let module = actions.iter().position(|a| *a == entry).expect("entry step");
    assert!(signin < module, "sign in first, then the module: {actions:?}");
}

/// A launch entry keeps the original order: launch, then sign in.
#[test]
fn a_launch_entry_still_comes_before_sign_in() {
    let c = case(
        "T",
        "M",
        "Signed in as Admin",
        vec![step("Do the thing", "It works")],
    );
    let (out, _) = optimize(vec![c], Some("Launch the HRM portal."));
    let actions: Vec<&str> = out[0].steps.iter().map(|s| s.action.as_str()).collect();
    assert_eq!(actions[0], "Launch the HRM portal.");
    assert!(actions[1].starts_with("Sign in as Admin"));
}

/// Running the optimizer on its own output changes nothing - the
/// idempotency the duplicated preamble violated.
#[test]
fn optimizing_twice_is_a_no_op_the_second_time() {
    let draft = vec![case(
        "T",
        "Payments",
        "Signed in as Admin. User is on the Payments page",
        vec![step("Enter a code", "Verify it applies")],
    )];
    let (once, _) = optimize(draft, None);
    let (twice, report) = optimize(once.clone(), None);

    assert_eq!(report.preamble_steps_added, 0);
    assert!(report.preconditions_rewritten.is_empty());
    assert_eq!(
        once.iter().map(|c| c.steps.len()).collect::<Vec<_>>(),
        twice.iter().map(|c| c.steps.len()).collect::<Vec<_>>(),
    );
    assert_eq!(once[0].preconditions, twice[0].preconditions);
}


// ------------------------------------------------- new transform ops

#[test]
fn group_by_preconditions_is_stable_and_keeps_order() {
    let a = "Signed in as Admin";
    let v = "Signed in as Viewer";
    let draft = vec![
        case("A1", "M", a, vec![]),
        case("V1", "M", v, vec![]),
        case("A2", "M", a, vec![]),
        case("V2", "M", v, vec![]),
    ];
    let (out, report) = apply(
        draft,
        &ops(serde_json::json!([{ "op": "group_by", "value": "preconditions" }])),
    );
    assert_eq!(
        out.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(),
        vec!["A1", "A2", "V1", "V2"],
        "groups in first-appearance order, within-group order untouched"
    );
    assert!(report.applied.iter().any(|l| l.contains("2 group(s)")));
}

#[test]
fn steps_can_be_prepended_appended_and_removed() {
    let draft = vec![case("T", "M", "", vec![step("Do it", "Done")])];
    let (out, _) = apply(
        draft,
        &ops(serde_json::json!([
            { "op": "prepend_step", "action": "Launch the app.", "expected": "It opens." },
            { "op": "append_step", "action": "Log out.", "expected": "Signed out." },
        ])),
    );
    let actions: Vec<&str> = out[0].steps.iter().map(|s| s.action.as_str()).collect();
    assert_eq!(actions, vec!["Launch the app.", "Do it", "Log out."]);

    // remove_step_matching repairs a duplicated preamble.
    let (out, _) = apply(out, &ops(serde_json::json!([
        { "op": "remove_step_matching", "value": "launch the app" }
    ])));
    let actions: Vec<&str> = out[0].steps.iter().map(|s| s.action.as_str()).collect();
    assert_eq!(actions, vec!["Do it", "Log out."]);
}

#[test]
fn cases_can_be_removed_with_a_filter_and_inserted() {
    let mut dup = case("Duplicate of existing", "M", "", vec![step("a", "b")]);
    dup.tags = "drop-me".into();
    let draft = vec![case("Keep", "M", "", vec![step("a", "b")]), dup];

    let (out, report) = apply(
        draft,
        &ops(serde_json::json!([
            { "op": "remove_cases", "where": { "has_tag": "drop-me" } },
            { "op": "insert_cases", "cases": [
                { "title": "Brand new", "steps": [{ "action": "x", "expected": "y" }] }
            ]},
        ])),
    );
    assert_eq!(
        out.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(),
        vec!["Keep", "Brand new"]
    );
    assert!(report.applied.iter().any(|l| l.contains("Removed 1 case")));
}

/// The guard that keeps remove_cases from being a foot-gun.
#[test]
fn remove_cases_without_a_filter_is_refused() {
    let err = parse_ops(&serde_json::json!([{ "op": "remove_cases" }])).unwrap_err();
    assert!(err.contains("requires a \"where\" filter"), "got: {err}");
}

/// The shape defect from the field report: a transform must round-trip
/// the caller's shape, not inject an empty comment field into every case.
#[tokio::test]
async fn transforms_do_not_inject_an_empty_comment_field() {
    let ctx = BridgeContext::default();
    let body_in = serde_json::json!({
        "test_cases": r#"[{"title":"A","steps":[{"action":"a","expected":"b"}]}]"#,
        "operations": [{ "op": "add_tags", "value": "smoke" }],
    })
    .to_string();
    let (_, body) = route(&ctx, None, "POST", "/transform", &body_in, "test").await;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let case = v["test_cases"][0].as_object().unwrap();
    assert!(!case.contains_key("comment"), "keys: {:?}", case.keys().collect::<Vec<_>>());

    // A non-empty comment still round-trips - only the empty one vanishes.
    let body_in = serde_json::json!({
        "test_cases": r#"[{"title":"A","comment":"keep me","steps":[{"action":"a","expected":"b"}]}]"#,
        "operations": [{ "op": "add_tags", "value": "smoke" }],
    })
    .to_string();
    let (_, body) = route(&ctx, None, "POST", "/transform", &body_in, "test").await;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["test_cases"][0]["comment"], "keep me");
}

/// Two DIFFERENT work items are allowed to share a title. Dropping one
/// took its id with it, so the survivor would CREATE a new case and the
/// real one would never be updated - a silent loss of the caller's edit.
#[test]
fn a_title_clash_between_two_real_work_items_keeps_both() {
    let mut a = TestCase { title: "Login as admin".into(), ..Default::default() };
    a.update_id = Some(101);
    let mut b = TestCase { title: "Login as admin".into(), ..Default::default() };
    b.update_id = Some(205);

    let (out, report) = v2_lib::optimize::optimize(vec![a, b], None);
    assert_eq!(out.len(), 2, "a work item was dropped");
    assert_eq!(report.duplicates_removed, 0);
    let note = report.notes.join(" ");
    assert!(note.contains("101") && note.contains("205"), "got {note}");
}

/// A case carrying an id must not be collapsed into an id-less one either:
/// that would retarget an update into a create.
#[test]
fn an_identified_case_is_not_collapsed_into_an_id_less_one() {
    let plain = TestCase { title: "Login as admin".into(), ..Default::default() };
    let mut identified = TestCase { title: "Login as admin".into(), ..Default::default() };
    identified.update_id = Some(101);
    let (out, _) = v2_lib::optimize::optimize(vec![plain, identified], None);
    assert_eq!(out.len(), 2);
}

/// A genuine duplicate - same id, or neither carrying one - still collapses.
#[test]
fn a_real_duplicate_is_still_removed() {
    let one = TestCase { title: "Login as admin".into(), ..Default::default() };
    let two = TestCase { title: "Login  as  admin".into(), ..Default::default() };
    let (out, report) = v2_lib::optimize::optimize(vec![one, two], None);
    assert_eq!(out.len(), 1);
    assert_eq!(report.duplicates_removed, 1);
}

/// The noise strips CHAINED. "Ensure Check Number is displayed" lost
/// "ensure " and then "check " - because the subject's own first word was
/// one of the verbs - and reached a real Azure DevOps test case as "Number
/// is displayed.", where the tester can no longer tell which number.
/// Banking fields ("Check Number", "Check Date") and UI labels ("Confirm
/// button") all hit it.
#[test]
fn stripping_noise_never_eats_the_subject() {
    assert_eq!(
        clean_expected("Ensure Check Number is displayed on the receipt"),
        "Check Number is displayed on the receipt."
    );
    assert_eq!(clean_expected("Verify check is cleared"), "Check is cleared.");
    assert_eq!(clean_expected("Check that Confirm is enabled"), "Confirm is enabled.");

    // The chaining that was WANTED still works - these have a real subject
    // left after each strip.
    assert_eq!(
        clean_expected("Verify that the system should display an error"),
        "Display an error."
    );
    assert_eq!(clean_expected("Verify that the invoice is saved"), "The invoice is saved.");
}

/// `". "` is not always a sentence end. "Approx. 30 results are returned"
/// was cut at the abbreviation and became the single word "Approx".
#[test]
fn an_abbreviation_is_not_a_sentence_end() {
    assert_eq!(
        clean_expected("Approx. 30 results are returned"),
        "Approx. 30 results are returned."
    );
    assert_eq!(clean_expected("No. 5 is highlighted"), "No. 5 is highlighted.");
    // A real sentence boundary still cuts.
    assert_eq!(
        clean_expected("A confirmation appears. This proves the flow works."),
        "A confirmation appears."
    );
}

/// Shortening an expected result can DELETE an assertion - "The status
/// changes to Shipped. A confirmation email is sent." keeps only the first,
/// so nobody is ever asked to check the email. A bare counter could not
/// tell that from "added a full stop", and named no case.
#[test]
fn an_expected_result_that_lost_text_is_named_in_the_report() {
    let draft = vec![
        case("Order ships", "M", "", vec![step(
            "Ship it",
            "The order status changes to Shipped. A confirmation email is sent to the customer.",
        )]),
        // Cosmetic only - sentence case and a full stop. Must NOT be listed.
        case("Tidy only", "M", "", vec![step("Do it", "the invoice is saved")]),
    ];
    let (_out, report) = optimize(draft, None);

    let named: Vec<&str> = report.expected_rewritten.iter().map(|e| e.title.as_str()).collect();
    assert_eq!(named, vec!["Order ships"], "only material loss is listed");
    let change = &report.expected_rewritten[0];
    assert_eq!(change.step_number, 1);
    assert!(change.before.contains("confirmation email"), "the report must show what was lost");
    assert!(!change.after.contains("confirmation email"));
}

/// already_has_preamble looked only at steps[0], so a draft that opened
/// with a setup line and launched at step 2 was judged to have no preamble
/// and got a whole second one: launch, sign in and navigate, all twice.
#[test]
fn a_draft_whose_preamble_starts_at_step_two_is_not_given_another() {
    let c = case(
        "Approve a leave request",
        "Leave Management",
        "Signed in as a manager; User is on the Leave Management page",
        vec![
            step("Ensure the seed data script has run.", "Test employees exist."),
            step("Launch the application.", "The application opens."),
            step("Sign in as a manager.", "The home page is displayed."),
            step("Navigate to the Leave Management page.", "The page is displayed."),
            step("Approve the request", "It is approved"),
        ],
    );
    let (out, report) = optimize(vec![c], Some("Launch the application."));

    assert_eq!(report.preamble_steps_added, 0, "the draft already walks itself in");
    assert_eq!(out[0].steps.len(), 5, "steps: {:?}", out[0].steps);
    let launches = out[0]
        .steps
        .iter()
        .filter(|s| s.action.to_lowercase().starts_with("launch"))
        .count();
    assert_eq!(launches, 1, "the launch step was duplicated: {:?}", out[0].steps);
}

/// A case with no steps does not survive the importer - it is skipped -
/// so a remove that emptied one DELETED it from the draft, while the
/// report said the operation had been applied. Same rule the writer keeps
/// against Azure DevOps: an empty step list never stands in for a real one.
#[test]
fn removing_every_step_is_refused_rather_than_emptying_the_case() {
    let draft = vec![
        case("Keeps one", "M", "", vec![step("Open the app", "It opens"), step("Pay", "Paid")]),
        case("Loses all", "M", "", vec![step("Open the app", "It opens")]),
    ];
    let (out, report) = apply(
        draft,
        &ops(serde_json::json!([{ "op": "remove_step_matching", "value": "open the app" }])),
    );

    assert_eq!(out.len(), 2, "no case may disappear");
    assert_eq!(out[0].steps.len(), 1, "the matching step still goes when others remain");
    assert_eq!(
        out[1].steps.len(),
        1,
        "the only step was kept rather than leaving an unimportable case"
    );
    assert!(
        report.warnings.iter().any(|w| w.contains("Loses all")),
        "the case that was left alone must be named: {:?}",
        report.warnings
    );
}

/// A precondition is only navigation if it actually navigates. "On the",
/// "at the" and "from the" open any English sentence, and matching them
/// bare turned real setup into a nonsense step AND dropped it from the
/// case's setup signature - which is what decides the run order, so one
/// stray sentence reshuffled the whole sheet. Bare "launch" did the same
/// to feature-flag preconditions.
#[test]
fn a_setup_sentence_is_not_promoted_just_for_starting_with_a_preposition() {
    let setup = [
        "On the second attempt the lockout applies",
        "At the end of the billing cycle the invoice is issued",
        "From the previous run the cart holds 3 items",
        "Launch darkly flag PAY-42 is enabled",
    ];
    for pre in setup {
        let out = optimize(vec![case("T", "", pre, vec![step("do", "done")])], None).0;
        assert_eq!(
            out[0].preconditions.trim(),
            pre,
            "'{pre}' was treated as navigation and moved out of preconditions"
        );
    }

    // The forms that DO navigate still do, including the elliptical ones -
    // that is the whole reason these openers were listed.
    for pre in [
        "On the Payments page",
        "At the Orders screen",
        "Launch the HRM portal",
        "User is on the Settings tab",
    ] {
        let out = optimize(vec![case("T", "", pre, vec![step("do", "done")])], None).0;
        assert!(
            out[0].steps.len() > 1,
            "'{pre}' should have become a step, steps={:?}",
            out[0].steps
        );
    }
}

/// Lowercasing is not length-preserving, and this function used to search
/// a lowercased copy and then slice the ORIGINAL at the offsets it got
/// back. Every case here shifts the bytes: capital sharp S loses one byte
/// when lowered, capital dotted I gains one, and the Kelvin sign collapses
/// from three bytes to one. The offsets then pointed into the middle of a
/// character and `clean_expected` panicked - taking down optimize_cases,
/// which is a whole draft, over one German or Turkish word.
#[test]
fn a_multi_byte_expected_result_does_not_panic() {
    // Sharp S: 3 bytes uppercase, 2 lowercase - offsets slide LEFT.
    assert_eq!(
        clean_expected("The STRAẞE field is saved Note: trimmed"),
        "The STRAẞE field is saved."
    );
    // Dotted capital I: 2 bytes, lowercases to 3 - offsets slide RIGHT.
    assert_eq!(
        clean_expected("İstanbul is listed because the filter matches"),
        "İstanbul is listed."
    );
    // Kelvin sign: 3 bytes, lowercases to 1.
    assert_eq!(clean_expected("The K reading is shown e.g. 300"), "The K reading is shown.");

    // The same trap on the noise-prefix loop, which stripped `noise.len()`
    // bytes off the original after matching against the lowered copy.
    assert_eq!(clean_expected("VERIFY THAT ẞ is rendered"), "ẞ is rendered.");

    // And the whole point: it must not panic on anything, however odd.
    for raw in ["ẞ", "İ", "K", "verify that ẞ", " note: İ", "ẞ because İ"] {
        let _ = clean_expected(raw);
    }
}

/// The precondition move is ATOMIC: a sentence leaves preconditions only in
/// the same pass that adds it as a step. The duplicate-guard broke that by
/// scanning the WHOLE case - a mid-case "Navigate to the Payments page."
/// (there to check the result) matched the generated nav step, so the step
/// was dropped while the precondition it consumed was still removed. The
/// case lost both: no step telling the tester to go there, and no
/// precondition saying they should be.
#[test]
fn a_mid_case_navigation_is_not_mistaken_for_a_duplicate_preamble() {
    let c = case(
        "Refund a completed payment",
        "Payments",
        "Signed in as a finance admin; User is on the Payments page",
        vec![
            step("Enter 1000 in the Amount box.", "The amount is accepted"),
            step("Click Pay.", "The payment completes"),
            step("Open the payment detail.", "The detail opens"),
            step("Click Refund.", "A confirmation appears"),
            step("Confirm the refund.", "The refund is accepted"),
            step("Navigate to the Payments page.", "The list is shown"),
            step("Check the row shows Refunded.", "It shows Refunded"),
        ],
    );
    let (out, _) = optimize(vec![c], Some("Launch the application."));
    let actions: Vec<&str> = out[0].steps.iter().map(|s| s.action.as_str()).collect();

    // The tester is walked in before being told to type an amount.
    assert!(
        actions.iter().any(|a| a.to_lowercase().contains("payments page")),
        "the preamble navigation was dropped: {actions:?}"
    );
    let first_amount = actions.iter().position(|a| a.contains("Amount box")).unwrap();
    let first_nav = actions
        .iter()
        .position(|a| a.to_lowercase().contains("navigate to the payments"))
        .unwrap();
    assert!(first_nav < first_amount, "navigation must come first: {actions:?}");

    // The step at the END is still there - it checks the result and is not
    // a duplicate of the preamble.
    assert!(
        actions.last().unwrap().contains("Refunded"),
        "the case lost its closing steps: {actions:?}"
    );
    assert_eq!(
        actions.iter().filter(|a| a.to_lowercase().contains("navigate to the payments")).count(),
        2,
        "one preamble navigation and one mid-case check: {actions:?}"
    );
}

/// PLACE_WORDS was a SUBSTRING test, which is exactly backwards: "performance"
/// contains "form", "review" contains "view", "table" contains "tab". So the
/// setup sentences the marker split was added to protect were promoted after
/// all - just via a different route.
#[test]
fn a_word_that_merely_contains_a_place_word_is_not_a_place() {
    for pre in [
        "At the end of the performance review the rating is locked",
        "On the summary table the totals are frozen",
        "From the previous review cycle two goals are carried over",
    ] {
        let out = optimize(vec![case("T", "", pre, vec![step("do", "done")])], None).0;
        assert_eq!(
            out[0].preconditions.trim(),
            pre,
            "'{pre}' was promoted because a word merely CONTAINS a place word"
        );
    }

    // A real place still promotes - the whole point of the marker.
    for pre in ["On the Payments page", "At the Orders screen", "From the Reports tab"] {
        let out = optimize(vec![case("T", "", pre, vec![step("do", "done")])], None).0;
        assert!(out[0].steps.len() > 1, "'{pre}' should still become a step");
    }
}

/// EXPECTED_NOISE holds overlapping prefixes, longest first. When the
/// subject-guard refused the longer one, the loop moved on to the shorter
/// one that overlaps it - stripping the verb and leaving the connective:
/// "Verify that is shown" became "That is shown."
#[test]
fn refusing_a_prefix_does_not_fall_through_to_a_shorter_one() {
    for raw in [
        "Verify that is shown",
        "Ensure that are listed",
        "Check that was saved",
    ] {
        let out = clean_expected(raw);
        assert!(
            !out.starts_with("That ") && !out.starts_with("Are ") && !out.starts_with("Was "),
            "'{raw}' left a dangling connective: {out}"
        );
    }

    // The ordinary strips are untouched - a real subject still survives.
    assert_eq!(clean_expected("Verify that the invoice is saved"), "The invoice is saved.");
    assert_eq!(
        clean_expected("Verify that the system should display an error"),
        "Display an error."
    );
}
