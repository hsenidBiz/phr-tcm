//! "One branch per case" - detecting when a positive and its negative have
//! been written as a single test case.
//!
//! # Why this is worth warning about
//!
//! A negative folded into the positive case is COVERED but not VISIBLE. In
//! a 54-case set, six negatives were written as an extra step inside their
//! positive: expand a row and see the section, turn the setting off, expand
//! again and see it gone. The coverage is real, and an auditor reading
//! titles against a 619-case backlog undercounts it every time, because no
//! title says the negative was tested. The developer who prompted this
//! asked directly whether a particular negative existed; it did, as step 5
//! of something else.
//!
//! Splitting them also surfaces things a merged case hides. One title
//! claimed a button was hidden "from the Manager and the Reviewer" while
//! only ever signing in as the manager - the title was simply wrong. And an
//! all-goals-rated negative used one unrated goal out of two; rewritten
//! standalone with four of five rated, it now catches an implementation
//! that greens on any rating, which the original would have passed.
//!
//! # Why these are warnings
//!
//! There are legitimate exceptions - a transition that genuinely IS the
//! behaviour under test, such as a badge that must change when a value
//! changes. So the wording leaves the judgement with the author.

use crate::model::TestCase;

/// Words that carry no subject matter, so two sentences sharing only these
/// are not talking about the same thing. "step"/"steps" joined the list in
/// round 3: every case in a wizard-shaped set says "the Goals step", so the
/// word pairs a navigation preamble with whatever assertion follows it.
const STOPWORDS: [&str; 26] = [
    "the", "and", "are", "with", "that", "this", "from", "have", "been", "will", "when", "then",
    "shown", "displayed", "rendered", "visible", "appears", "section", "value", "field", "page",
    "user", "there", "which", "step", "steps",
];

/// Steps at the very start are setup, not a mid-run environment change.
const SETUP_STEPS: usize = 2;

/// Phrases that mean "this step changes how the system is configured".
///
/// Deliberately narrow. A case that reconfigures the system half way
/// through is testing two environments in one run, which is nearly always a
/// merged positive and negative - and it also breaks the run-sheet ordering
/// `optimize_cases` builds, since the case leaves the environment different
/// from how it found it.
const CONFIG_CHANGE: [&str; 8] = [
    "turn on", "turn off", "switch on", "switch off", "toggle ", "untick", "uncheck", "disable",
];

/// `disable`/`enable` need a configuration-shaped object; "disable the
/// button" is usually the assertion, not a settings change.
const CONFIG_OBJECT: [&str; 7] = [
    "setting", "option", "toggle", "configuration", "checkbox", "flag", "preference",
];

fn is_config_change(action: &str) -> bool {
    let a = action.to_lowercase();
    if ["turn on", "turn off", "switch on", "switch off", "untick"]
        .iter()
        .any(|p| a.contains(p))
    {
        return true;
    }
    let verbish = CONFIG_CHANGE.iter().any(|p| a.contains(p)) || a.contains("enable");
    verbish && CONFIG_OBJECT.iter().any(|o| a.contains(o))
}

fn negated(text: &str) -> bool {
    let t = text.to_lowercase();
    [
        " not ", "n't", "no ", " never ", "hidden", "absent", "disappears", "is gone", "does not",
        "cannot", "without",
    ]
    .iter()
    .any(|m| t.contains(m))
        || t.starts_with("no ")
}

/// Content words, lowercased, minus stopwords and short tokens.
fn topic_words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 3 && !STOPWORDS.contains(w))
        .map(|w| w.to_string())
        .collect()
}

/// How many DISTINCT content words two expectations share. Distinct, or a
/// word repeated on one side counts double: "rating ... rating" against a
/// side that says "rating" once scored 2 of the 3-word threshold by
/// itself, flagging a contrast that shares only two subjects.
fn overlap(a: &[String], b: &[String]) -> usize {
    let mut seen: Vec<&String> = Vec::new();
    for w in a {
        if b.contains(w) && !seen.contains(&w) {
            seen.push(w);
        }
    }
    seen.len()
}

/// Adjacent content-word pairs - the cheap stand-in for a noun phrase.
fn bigrams(words: &[String]) -> Vec<(&str, &str)> {
    words.windows(2).map(|w| (w[0].as_str(), w[1].as_str())).collect()
}

/// Whether two expectations are about the SAME THING, not merely wordy in
/// the same register. Round 3 measured the old 2-shared-words rule at 15
/// warnings on a clean 63-case set, 0 actionable: absence of one element
/// was being paired with presence of a DIFFERENT element because both
/// mentioned the row they live in. Sharing a two-word phrase ("revision
/// tag" with "revision tag") - or three content words, for a reworded
/// phrase - is what "the same thing" looks like in step prose.
fn same_subject(a: &[String], b: &[String]) -> bool {
    let pairs = bigrams(a);
    bigrams(b).iter().any(|p| pairs.contains(p)) || overlap(a, b) >= 3
}

/// An arrival line - "The Assessment Wizard opens on the Goals step" - is
/// the navigation preamble reporting where the case now stands, not an
/// assertion a negative can contradict. Every case in a set carries one,
/// so pairing against it flags a large fraction of any well-formed set.
fn arrival(text: &str) -> bool {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w == "opens")
}

/// A title that already names its branch - "... Is Not Offered ...",
/// "... Without ...", "... Hidden ..." - is an author declaring this the
/// single negative case, usually because it was ALREADY split. Warning it
/// to split again is backwards.
fn title_declares_branch(title: &str) -> bool {
    let t = format!(" {} ", title.to_lowercase());
    [" not ", " no ", " never ", " without ", " hidden ", " cannot "]
        .iter()
        .any(|m| t.contains(m))
}

/// Why a case looks like it holds both branches, or `None` when it does not.
///
/// Returns the reason so the warning can name the evidence - a bare
/// "consider splitting" gives the author nothing to check.
pub fn both_branches_reason(c: &TestCase) -> Option<String> {
    // Signal 1: the environment changes half way through.
    for (i, s) in c.steps.iter().enumerate().skip(SETUP_STEPS) {
        if is_config_change(&s.action) {
            return Some(format!(
                "step {} changes a setting mid-run, so the case tests two configurations in one \
                 run",
                i + 1
            ));
        }
    }

    // Signal 2: one expectation asserts a thing and another denies THE SAME
    // thing. Skipped wholesale for a title that already declares its branch
    // - that author has split, and this warning would ask them to split the
    // split.
    if title_declares_branch(&c.title) {
        return None;
    }
    let expectations: Vec<(usize, &str)> = c
        .steps
        .iter()
        .enumerate()
        .map(|(i, s)| (i, s.expected.trim()))
        .filter(|(_, e)| !e.is_empty())
        .collect();
    for (i, neg) in &expectations {
        if !negated(neg) {
            continue;
        }
        let neg_words = topic_words(neg);
        if neg_words.len() < 2 {
            continue;
        }
        for (j, pos) in &expectations {
            if i == j || negated(pos) || arrival(pos) {
                continue;
            }
            if same_subject(&neg_words, &topic_words(pos)) {
                return Some(format!(
                    "step {} expects something that step {} expects the absence of",
                    j + 1,
                    i + 1
                ));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steps_xml::Step;

    fn case(steps: &[(&str, &str)]) -> TestCase {
        TestCase {
            title: "T".into(),
            steps: steps
                .iter()
                .map(|(a, e)| Step { action: (*a).into(), expected: (*e).into() })
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
            .map(|(a, e)| Step { action: (*a).into(), expected: (*e).into() })
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
}
