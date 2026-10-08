//! The save check: a script may name only what the app has seen on the
//! live page (`discovery_map`), so a script is never saved against a
//! guessed locator.
//!
//! Two kinds of name are allowed without a sighting: one whose words the
//! script itself typed in an earlier step (a record it just created), and
//! one a check looks for whose text the test case itself says (the
//! expected result the step is checking for). Both are whole-word matches
//! of the locator's whole name, so typing "Test" exempts no "Test
//! connection" button.

use super::discovery_map::{path_only, seen_keys, seen_paths, DiscoveryMap};
use super::edits::Edit;
use super::CaseScript;
use crate::browser::actions::Action;
use crate::browser::locator::{fold_name, LocatorStep};

/// A typed value, or a name taken from the test case, shorter than this
/// exempts nothing: two characters are inside far too many names to say
/// where they came from.
const MIN_TYPED_LEN: usize = 3;

/// The steps a changed script's check reads: the declared ones, or every
/// step (`None`) when nothing is declared, so a missing declaration can
/// never skip the check.
pub fn steps_to_check(declared: Option<&Edit>) -> Option<Vec<i32>> {
    declared.map(|e| e.steps.clone())
}

/// Is `phrase` in `text` as whole words: bounded on each side by a
/// character that is not a letter or digit, or by an end of the text?
fn has_phrase(text: &str, phrase: &str) -> bool {
    text.match_indices(phrase).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + phrase.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

fn refusal(step: i32, what: &str) -> String {
    format!(
        "Step {step}: {what} was never seen on the live app. Find it on the page first with probe_autorun_locator or discover_autorun_action, then save again."
    )
}

/// Does this action look for something rather than act on it?
fn is_check(action: &Action) -> bool {
    matches!(
        action,
        Action::WaitFor { .. }
            | Action::ExpectVisible { .. }
            | Action::ExpectHidden { .. }
            | Action::ExpectText { .. }
            | Action::ExpectContainsText { .. }
            | Action::ExpectCount { .. }
            | Action::ExpectAttribute { .. }
            | Action::ExpectFocused { .. }
            | Action::ExpectRow { .. }
            | Action::ExpectNoRow { .. }
            | Action::ExpectSorted { .. }
            | Action::ExpectRowCount { .. }
    )
}

/// The link's own words: its text or its accessible name, folded.
fn words(link: &LocatorStep) -> Vec<String> {
    [link.text.as_deref(), link.name.as_deref()]
        .into_iter()
        .flatten()
        .map(fold_name)
        .filter(|w| !w.is_empty())
        .collect()
}

/// Checks `script` against what `map` has seen, in step order: every
/// step, or only `only_steps` on a repair. `case_text` is the test case's
/// own step actions and expected results. The first failure is returned.
pub fn check_seen(
    map: &DiscoveryMap,
    script: &CaseScript,
    case_text: &[String],
    only_steps: Option<&[i32]>,
) -> Result<(), String> {
    let mut areas: Vec<&str> = script.area_name().into_iter().collect();
    for step in &script.steps {
        for action in step.actions.iter().flat_map(Action::each) {
            if let Some(a) = action.area_named() {
                areas.push(a);
            }
        }
    }
    let keys = seen_keys(map, &areas);
    let paths = seen_paths(map);
    let case_text: Vec<String> = case_text.iter().map(|t| fold_name(t)).collect();
    // Values typed by the steps before the one being checked.
    let mut typed: Vec<String> = Vec::new();

    for step in &script.steps {
        let checked = only_steps.is_none_or(|only| only.contains(&step.step_number));
        if checked {
            for action in step.actions.iter().flat_map(Action::each) {
                if let Action::Navigate { url } | Action::OpenTab { url, .. } = action {
                    let path = path_only(url);
                    if !paths.contains(&path) {
                        return Err(refusal(step.step_number, &path));
                    }
                }
                // `each` lists a `when_visible`'s own actions after it, so
                // only its own selector is taken here.
                let targets = match action {
                    Action::WhenVisible { selector, .. } => vec![selector],
                    _ => action.targets(),
                };
                for target in targets {
                    for link in target.links() {
                        if link.seen_key().is_some_and(|k| keys.contains(&k)) {
                            continue;
                        }
                        let own = words(&link);
                        // Both exceptions match the same way: the locator's
                        // whole name, of at least 3 characters, as whole
                        // words in what was typed or what the case says.
                        let named_in = |texts: &[String]| {
                            own.iter().any(|w| {
                                w.chars().count() >= MIN_TYPED_LEN && texts.iter().any(|t| has_phrase(t, w))
                            })
                        };
                        let typed_here = named_in(&typed);
                        let in_case = is_check(action) && named_in(&case_text);
                        if !typed_here && !in_case {
                            return Err(refusal(step.step_number, &target.describe()));
                        }
                    }
                }
            }
        }
        for action in step.actions.iter().flat_map(Action::each) {
            if let Some(v) = action.typed_value() {
                let v = fold_name(v);
                if v.chars().count() >= MIN_TYPED_LEN {
                    typed.push(v);
                }
            }
        }
    }
    Ok(())
}
