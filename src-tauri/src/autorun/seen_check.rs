//! The save check: a script may name only what the app has seen on the
//! live page (`discovery_map`), so a script is never saved against a
//! guessed locator.
//!
//! Two kinds of name are allowed without a sighting. One is a locator
//! whose text or name contains a value the script typed in an earlier
//! step (a record it just created): typing "AutoTest Leave 7" exempts a
//! row "AutoTest Leave 7 Pending", never a "Leave" button. The other is a
//! name a check looks for that the test case itself says (the expected
//! result the step is checking for). Both match whole words only, and a
//! typed value shorter than 3 characters exempts nothing.
//!
//! A step that uses a component must name one the project has and give it
//! every input, each of its kind. Every locator of the component an input
//! goes into (a target input, or a text input written into a locator) is
//! checked like any other, as it runs; the component's fixed locators are
//! not, as they were checked when the component was saved.

use super::components::{expand, find, not_saved, Component, ComponentFile};
use super::discovery_map::{page_path, path_only, seen_keys, seen_paths, DiscoveryMap};
use super::edits::Edit;
use super::CaseScript;
use crate::browser::actions::Action;
use crate::browser::locator::{fold_name, LocatorStep, SeenKey, Target};
use std::collections::HashSet;

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

/// One locator, or one page path, a script names that the map has never
/// seen: the step that names it and how it reads (`Target::describe`, or
/// the path). Or a use of a component the step cannot make: then
/// `refused` says why, without the step ("Pick a date needs day"), and
/// `locator` is the component's name as the script writes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unseen {
    pub step: i32,
    pub locator: String,
    pub refused: Option<String>,
}

impl Unseen {
    /// The sentence a save refuses this with.
    fn refusal(&self) -> String {
        match &self.refused {
            Some(why) => format!("Step {}: {why}", self.step),
            None => refusal(self.step, &self.locator),
        }
    }
}

/// Checks `script` against what `map` has seen, in step order: every
/// step, or only `only_steps` on a repair. `case_text` is the test case's
/// own step actions and expected results; `components` are the project's,
/// for the steps that use one. The first failure is returned.
pub fn check_seen(
    map: &DiscoveryMap,
    components: &ComponentFile,
    script: &CaseScript,
    case_text: &[String],
    only_steps: Option<&[i32]>,
) -> Result<(), String> {
    match scan(map, components, script, case_text, only_steps, true).into_iter().next() {
        Some(u) => Err(u.refusal()),
        None => Ok(()),
    }
}

/// [`check_seen`], but every failure in step order rather than the first:
/// an import names all of them at once.
pub fn check_seen_all(
    map: &DiscoveryMap,
    components: &ComponentFile,
    script: &CaseScript,
    case_text: &[String],
    only_steps: Option<&[i32]>,
) -> Vec<Unseen> {
    scan(map, components, script, case_text, only_steps, false)
}

/// Does this script use a component anywhere? Its checks need the
/// project's components file only then.
pub fn uses_components(script: &CaseScript) -> bool {
    script
        .steps
        .iter()
        .flat_map(|s| s.actions.iter())
        .flat_map(Action::each)
        .any(|a| matches!(a, Action::UseComponent { .. }))
}

/// The targets an action's own check reads: a `when_visible`'s own
/// selector only, as `each` lists its guarded actions after it.
fn own_targets(action: &Action) -> Vec<&Target> {
    match action {
        Action::WhenVisible { selector, .. } => vec![selector],
        _ => action.targets(),
    }
}

/// A locator a component's inputs went into, as it runs: the links an
/// input put there (a fixed link it already had is not re-checked; that
/// was done when the component was saved), whether its action is a check,
/// and what the component typed before it.
struct InputLocator {
    target: Target,
    links: Vec<LocatorStep>,
    check: bool,
    typed_before: Vec<String>,
}

/// Every locator of `c` an input changed, from its `expanded` actions
/// (`expand`'s, which pairs one for one with `c.actions`): a target
/// input's locator, or a fixed one a text input was written into.
fn input_locators(c: &Component, expanded: &[Action]) -> Vec<InputLocator> {
    let mut out = Vec::new();
    let mut typed_before: Vec<String> = Vec::new();
    for (written, ran) in c.actions.iter().zip(expanded) {
        for (w, r) in written.each().into_iter().zip(ran.each()) {
            for (wt, rt) in own_targets(w).into_iter().zip(own_targets(r)) {
                if wt == rt {
                    continue;
                }
                let fixed = wt.links();
                let links: Vec<LocatorStep> = rt.links().into_iter().filter(|l| !fixed.contains(l)).collect();
                out.push(InputLocator { target: rt.clone(), links, check: is_check(r), typed_before: typed_before.clone() });
            }
            if let Some(v) = r.typed_value() {
                let v = fold_name(v);
                if v.chars().count() >= MIN_TYPED_LEN {
                    typed_before.push(v);
                }
            }
        }
    }
    out
}

/// The actions a `use_component` the project can expand runs as, for what
/// they type and the areas they go to; nothing for any other action, or
/// for a use that cannot be expanded (its own step refuses that).
fn as_run(components: &ComponentFile, action: &Action) -> Vec<Action> {
    match action {
        Action::UseComponent { component, inputs } => {
            find(components, component).and_then(|c| expand(c, inputs).ok()).unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

/// Is this link neither on the map nor exempt?
fn link_unseen(link: &LocatorStep, keys: &HashSet<SeenKey>, typed: &[String], case_text: &[String], check: bool) -> bool {
    if link.seen_key().is_some_and(|k| keys.contains(&k)) {
        return false;
    }
    let own = words(link);
    // A value typed earlier (already at least 3 characters) as whole words
    // inside the locator's text or name: the record the script created.
    let typed_here = typed.iter().any(|t| own.iter().any(|w| has_phrase(w, t)));
    // The locator's whole name, of at least 3 characters, as whole words in
    // what the case says.
    let in_case = check
        && own
            .iter()
            .any(|w| w.chars().count() >= MIN_TYPED_LEN && case_text.iter().any(|t| has_phrase(t, w)));
    !typed_here && !in_case
}

/// The failures of the check, stopping at the first when `first_only`.
fn scan(
    map: &DiscoveryMap,
    components: &ComponentFile,
    script: &CaseScript,
    case_text: &[String],
    only_steps: Option<&[i32]>,
    first_only: bool,
) -> Vec<Unseen> {
    let mut unseen: Vec<Unseen> = Vec::new();
    let mut areas: Vec<String> = script.area_name().into_iter().map(str::to_string).collect();
    for step in &script.steps {
        for action in step.actions.iter().flat_map(Action::each) {
            let ran = as_run(components, action);
            for a in std::iter::once(action).chain(ran.iter().flat_map(Action::each)) {
                if let Some(a) = a.area_named() {
                    areas.push(a.to_string());
                }
            }
        }
    }
    let areas: Vec<&str> = areas.iter().map(String::as_str).collect();
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
                    // Compared as the map files a page; named as written.
                    let path = path_only(url);
                    if !paths.contains(&page_path(url)) {
                        unseen.push(Unseen { step: step.step_number, locator: path, refused: None });
                        if first_only {
                            return unseen;
                        }
                    }
                }
                // The locators the script names: the action's own, each
                // with whether it is only looked for and what was typed
                // before it; or every locator of a component its inputs
                // went into.
                let mut named: Vec<InputLocator> = own_targets(action)
                    .into_iter()
                    .map(|t| InputLocator {
                        target: t.clone(),
                        links: t.links(),
                        check: is_check(action),
                        typed_before: Vec::new(),
                    })
                    .collect();
                if let Action::UseComponent { component, inputs } = action {
                    let used = find(components, component)
                        .ok_or_else(|| not_saved(component))
                        .and_then(|c| expand(c, inputs).map(|ex| (c, ex)));
                    let (c, expanded) = match used {
                        Ok(used) => used,
                        Err(why) => {
                            unseen.push(Unseen {
                                step: step.step_number,
                                locator: component.clone(),
                                refused: Some(why),
                            });
                            if first_only {
                                return unseen;
                            }
                            continue;
                        }
                    };
                    named.extend(input_locators(c, &expanded));
                }
                for n in named {
                    let typed: Vec<String> = typed.iter().cloned().chain(n.typed_before).collect();
                    // One line per target, however many of its links are
                    // unseen.
                    if n.links.iter().any(|l| link_unseen(l, &keys, &typed, &case_text, n.check)) {
                        unseen.push(Unseen { step: step.step_number, locator: n.target.describe(), refused: None });
                        if first_only {
                            return unseen;
                        }
                    }
                }
            }
        }
        for action in step.actions.iter().flat_map(Action::each) {
            // A component types what its actions type, its text inputs
            // put in.
            let ran = as_run(components, action);
            for a in std::iter::once(action).chain(ran.iter().flat_map(Action::each)) {
                if let Some(v) = a.typed_value() {
                    let v = fold_name(v);
                    if v.chars().count() >= MIN_TYPED_LEN {
                        typed.push(v);
                    }
                }
            }
        }
    }
    unseen
}

/// Does this link carry a `{{x}}` text placeholder anywhere?
fn holds_text_placeholder(link: &LocatorStep) -> bool {
    serde_json::to_string(link).is_ok_and(|s| s.contains("{{"))
}

/// A component's own locators and the pages it goes to, checked against
/// what `map` has seen in `area` (and in any area its actions return to),
/// in order; the first unseen one is refused, named by the component's
/// action. Two kinds are exempt, as only a script knows them: a link a
/// target input fills (`{"input": ...}`), and a whole locator a text input
/// is written into (`{{x}}`). A script's save checks both as they expand.
pub fn check_component_seen(map: &DiscoveryMap, area: Option<&str>, actions: &[Action]) -> Result<(), String> {
    let mut areas: Vec<&str> = area.into_iter().collect();
    areas.extend(actions.iter().flat_map(Action::each).filter_map(Action::area_named));
    let keys = seen_keys(map, &areas);
    let paths = seen_paths(map);
    let refused = |i: usize, what: &str| {
        format!(
            "Action {}: {what} was never seen on the live app. Find it on the page first with probe_autorun_locator or discover_autorun_action, then save again.",
            i + 1
        )
    };
    // Values typed by the component's earlier actions.
    let mut typed: Vec<String> = Vec::new();
    for (i, action) in actions.iter().enumerate() {
        for a in action.each() {
            if let Action::Navigate { url } | Action::OpenTab { url, .. } = a {
                if !paths.contains(&page_path(url)) {
                    return Err(refused(i, &path_only(url)));
                }
            }
            for t in own_targets(a) {
                let links = t.links();
                if links.iter().any(holds_text_placeholder) {
                    continue;
                }
                let check = is_check(a);
                if links.iter().filter(|l| l.input.is_none()).any(|l| link_unseen(l, &keys, &typed, &[], check)) {
                    return Err(refused(i, &t.describe()));
                }
            }
            if let Some(v) = a.typed_value() {
                let v = fold_name(v);
                if v.chars().count() >= MIN_TYPED_LEN {
                    typed.push(v);
                }
            }
        }
    }
    Ok(())
}
