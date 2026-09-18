//! `both_branches_reason`: flag a case that merges two branches, and stay
//! quiet on the ordinary shapes.

use v2_lib::branchcheck::both_branches_reason;
use v2_lib::model::TestCase;
use v2_lib::steps_xml::Step;

fn case(steps: &[(&str, &str)]) -> TestCase {
    TestCase {
        title: "T".into(),
        steps: steps
            .iter()
            .map(|(a, e)| Step { action: (*a).into(), expected: (*e).into(), shared: None })
            .collect(),
        automation_status: "Not Automated".into(),
        ..Default::default()
    }
}

#[test]
fn a_setting_turned_off_mid_run_is_two_cases() {
    let c = case(&[
        ("Sign in as the manager.", "The dashboard is shown."),
        ("Open the appraisal.", "The form is shown."),
        ("Expand the goal row.", "An Actions and Measures section is shown."),
        ("Turn off the action_measure_enabled setting.", "The setting is saved."),
        ("Expand the goal row again.", "No Actions and Measures section is shown."),
    ]);
    let why = both_branches_reason(&c).expect("should flag");
    assert!(why.contains("step 4"), "{why}");
    assert!(why.contains("setting"), "{why}");
}

#[test]
fn contradictory_expectations_in_one_case_are_flagged() {
    let c = case(&[
        ("Sign in.", "The dashboard is shown."),
        ("Open a revised goal.", "The revision tag is shown on the goal."),
        ("Open a goal that was not changed.", "No revision tag is shown on the goal."),
    ]);
    let why = both_branches_reason(&c).expect("should flag");
    assert!(why.contains("absence"), "{why}");
}

/// The cost of a false positive is an author second-guessing a correct
/// case, so the ordinary shapes must stay quiet.
#[test]
fn a_straightforward_case_is_not_flagged() {
    let single = case(&[
        ("Sign in as the manager.", "The dashboard is shown."),
        ("Open the appraisal.", "The form is shown."),
        ("Click Submit.", "A confirmation is shown."),
    ]);
    assert_eq!(both_branches_reason(&single), None);

    // Setup at the top is setup, not a mid-run reconfiguration.
    let configured_upfront = case(&[
        ("Turn on the action_measure_enabled setting.", "The setting is saved."),
        ("Sign in as the manager.", "The dashboard is shown."),
        ("Expand the goal row.", "An Actions and Measures section is shown."),
    ]);
    assert_eq!(both_branches_reason(&configured_upfront), None);

    // A negative-only case denies things without ever asserting them.
    let negative_only = case(&[
        ("Sign in as the employee.", "The dashboard is shown."),
        ("Open the appraisal.", "No Revise Goal Plan button is shown."),
    ]);
    assert_eq!(both_branches_reason(&negative_only), None);

    // "Disable the Submit button" is an assertion about a control, not
    // a settings change.
    let disabled_control = case(&[
        ("Sign in.", "The dashboard is shown."),
        ("Open the form.", "The form is shown."),
        ("Clear the required field.", "The Submit button is disabled."),
    ]);
    assert_eq!(both_branches_reason(&disabled_control), None);
}

/// Round 3's three worked false positives, pinned verbatim. The old
/// rule paired an absence with the presence of a DIFFERENT element
/// because both mentioned the row they live in: 15 warnings on a
/// clean 63-case set, 0 actionable.
#[test]
fn absence_of_one_element_next_to_presence_of_others_is_not_a_merge() {
    let c = case(&[
        ("Sign in as the manager.", "The dashboard is shown."),
        ("Open the assessment.", "The wizard is shown."),
        (
            "Expand the goal row.",
            "The row expands and no Actions & Measures section is rendered inside it.",
        ),
        (
            "Read the expanded row.",
            "The description, target date and KPI details are shown as normal.",
        ),
    ]);
    assert_eq!(both_branches_reason(&c), None);
}

/// One claim stated from both sides for clarity is one claim.
/// Splitting it would produce two cases neither of which asserts it.
#[test]
fn a_single_assertion_phrased_as_a_contrast_is_not_a_merge() {
    let c = case(&[
        ("Sign in.", "The dashboard is shown."),
        ("Open the objectives group.", "The group is shown."),
        ("Read the objective row.", "A rating and comment control is offered once per objective."),
        (
            "Expand the key results.",
            "The key results carry no rating controls of their own - rating is captured at \
             the objective level only.",
        ),
    ]);
    assert_eq!(both_branches_reason(&c), None);
}

/// The navigation preamble every case carries must not pair with the
/// first real assertion after it - that flags a large fraction of any
/// well-formed set.
#[test]
fn the_navigation_preamble_is_not_the_positive_branch() {
    let c = case(&[
        ("Sign in as the manager.", "The dashboard is shown."),
        ("Open the assessment for the stage.", "The assessment list is shown."),
        (
            "Click the Goals stage.",
            "The Assessment Wizard opens on the Goals step for that stage.",
        ),
        (
            "Leave one goal unrated and click Continue.",
            "Continue is blocked and the Goals step indicator is not green.",
        ),
    ]);
    assert_eq!(both_branches_reason(&c), None);
}

/// A title that already names its branch is an author who has ALREADY
/// split - warning them to split again is backwards.
#[test]
fn a_title_declaring_its_branch_is_never_asked_to_split_again() {
    let c = TestCase {
        title: "Delete Is Not Offered to the Employee on an Attachment Uploaded by the Manager"
            .into(),
        steps: [
            ("Sign in as the employee.", "The dashboard is shown."),
            ("Open the attachments list.", "The manager's file and the employee's own file are both listed."),
            ("Read the manager's file row.", "No Delete control is shown on the manager's file."),
            ("Read the employee's own file row.", "A Delete control is shown on the employee's own file."),
        ]
        .iter()
        .map(|(a, e)| Step { action: (*a).into(), expected: (*e).into(), shared: None })
        .collect(),
        automation_status: "Not Automated".into(),
        ..Default::default()
    };
    // Deliberately checks both files in one list - co-location is what
    // catches Delete being rendered per-section rather than per-row.
    assert_eq!(both_branches_reason(&c), None);
}

/// And the tightening must not have killed the real catch: the same
/// element asserted present and absent, under a neutral title.
#[test]
fn the_same_element_present_and_absent_is_still_flagged() {
    let c = case(&[
        ("Sign in.", "The dashboard is shown."),
        ("Open the wizard on a content step.", "The Reject button is shown in the footer."),
        ("Move to the goal-planning step.", "The Reject button is not shown in the footer."),
    ]);
    assert!(both_branches_reason(&c).is_some());
}
