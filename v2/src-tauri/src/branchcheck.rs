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

/// Words that carry no subject matter, so two sentences sharing only these
/// are not talking about the same thing.
const STOPWORDS: [&str; 24] = [
    "the", "and", "are", "with", "that", "this", "from", "have", "been", "will", "when", "then",
    "shown", "displayed", "rendered", "visible", "appears", "section", "value", "field", "page",
    "user", "there", "which",
];

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

/// How many content words two expectations share.
fn overlap(a: &[String], b: &[String]) -> usize {
    a.iter().filter(|w| b.contains(w)).count()
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

    // Signal 2: one expectation asserts a thing and another denies it.
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
            if i == j || negated(pos) {
                continue;
            }
            if overlap(&neg_words, &topic_words(pos)) >= 2 {
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
}
