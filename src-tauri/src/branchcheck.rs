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
