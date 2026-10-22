//! What a script's checks are held against: the case as written.
//!
//! A check's literal values (a count, a text) come from the case's own
//! words. A value the case does not hold needs a `because` naming where the
//! case defines it, and a count the case states differently is refused. Pure:
//! nothing here reads a file, a page or Azure DevOps.

use std::collections::BTreeSet;

use super::case_text::{fold, stated_counts, CaseText};
use super::seen_check::has_phrase;
use super::edits::GiveUp;
use super::{Because, CaseScript, Provenance, StepScript};
use crate::browser::actions::Action;
use crate::browser::locator::Target;
use crate::browser::actions::{CellMatch, HeadersSpec};
use crate::browser::pattern::literal_word_forms;

/// What kind of value a check carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    Count,
    Text,
    /// A `matches` pattern: held by the words written in it (`literal_word_forms`).
    Pattern,
    /// `true` or `false`: a checked, pressed or other boolean state.
    State,
}

/// Whole words in a step's own text that say a state is on. `on` says it only in "is on", "turned on"
/// or "switched on" (`state_of`).
pub const STATE_TRUE_WORDS: &[&str] =
    &["checked", "ticked", "selected", "enabled", "pressed", "expanded", "shown", "visible", "active"];
/// ... and off. `off` says it only in "is off", "turned off" or "switched off"; "not selected" and
/// "not checked" say it through the negation (`state_of`).
pub const STATE_FALSE_WORDS: &[&str] = &["unchecked", "unticked", "disabled", "collapsed", "hidden", "inactive"];
/// The words that lead `on` and `off` into a state.
const SWITCH_LEADS: &[&str] = &["is", "isnt", "turned", "switched"];
/// A word after `on` or `off` that makes it a place or a preposition ("on the page"), not a state.
const NOT_A_STATE_AFTER: &[&str] = &[
    "the", "a", "an", "this", "that", "these", "those", "each", "every", "its", "their", "your", "my", "our", "page",
    "screen", "top", "left", "right", "bottom",
];
/// Whole words in a step's own text that say there are none ("no <noun>" says it too).
pub const NONE_WORDS: &[&str] = &["0", "zero", "none", "nothing"];
/// ... and that there is one ("a single" says it too).
pub const ONE_WORDS: &[&str] = &["1", "one"];

/// One literal value a check uses.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckValue {
    pub step: i32,
    pub kind: ValueKind,
    /// A count as its digits.
    pub value: String,
    /// A count that must be exactly this (not a bound), or a text compared whole.
    pub exact: bool,
    /// The check action as JSON, lowercased.
    pub action_json: String,
    /// Every string VALUE of the action (never a key, never its `kind`),
    /// lowercased and joined with spaces: what a stated count's noun is
    /// looked for in.
    pub values_text: String,
    /// How a count bounds what the page shows.
    pub rel: Rel,
    /// True for the values the check has always read (a count of 2 or more, a text of 3 characters
    /// or more): a count is held against a count the case states. False for the rest (0 and 1, a
    /// number written as short text, a pattern, an attribute's or a style's value, a state, the name
    /// or text of a visible locator, a download's contents): they are held by the case or a `because`
    /// all the same, and a repair keeps one the case gives like any other.
    pub strict: bool,
    /// What a refusal calls it: "the count", "the text", "the pattern", "the state", "the attribute
    /// value" or "the style value".
    pub what: &'static str,
}

/// What a count claims about the number of things.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rel {
    /// Exactly this many.
    Equal,
    /// This many or more (`at_least`, `expect_each` `min`).
    AtLeast,
    /// This many or fewer.
    AtMost,
    /// The length of an `expect_order`: others may sit between its texts, so
    /// only a length above the stated count contradicts it.
    OrderLength,
    /// Not a count.
    NotCount,
}

/// The action's string values, never its keys or its `kind`.
fn string_values(v: &serde_json::Value, out: &mut Vec<String>) {
    match v {
        serde_json::Value::String(s) => out.push(s.to_lowercase()),
        serde_json::Value::Array(a) => a.iter().for_each(|x| string_values(x, out)),
        serde_json::Value::Object(m) => {
            for (k, x) in m {
                if k != "kind" {
                    string_values(x, out);
                }
            }
        }
        _ => {}
    }
}

struct Seen<'a> {
    json: &'a str,
    values: &'a str,
}

fn count(step: i32, n: u32, rel: Rel, s: &Seen) -> CheckValue {
    CheckValue {
        step,
        kind: ValueKind::Count,
        value: n.to_string(),
        exact: rel == Rel::Equal,
        action_json: s.json.to_string(),
        values_text: s.values.to_string(),
        rel,
        strict: n >= 2,
        what: "the count",
    }
}

/// A whole number, with a decimal part or a `%` perhaps: "5", "12.5", "3,5", "40%".
fn is_number_text(value: &str) -> bool {
    let v = value.trim();
    let v = v.strip_suffix('%').unwrap_or(v);
    let mut parts = v.splitn(2, ['.', ',']);
    let digits = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit());
    match (parts.next(), parts.next()) {
        (Some(whole), None) => digits(whole),
        (Some(whole), Some(frac)) => digits(whole) && digits(frac),
        _ => false,
    }
}

/// A text the check has always read, except a number short enough to have been exempt as short.
fn text(step: i32, value: &str, exact: bool, s: &Seen) -> CheckValue {
    CheckValue {
        step,
        kind: ValueKind::Text,
        value: value.to_string(),
        exact,
        action_json: s.json.to_string(),
        values_text: s.values.to_string(),
        rel: Rel::NotCount,
        strict: !(is_number_text(value) && value.trim().chars().count() < 3),
        what: "the text",
    }
}

/// A value of a kind the check did not read before item 39.
fn loose(step: i32, kind: ValueKind, value: &str, exact: bool, what: &'static str, s: &Seen) -> CheckValue {
    CheckValue {
        step,
        kind,
        value: value.to_string(),
        exact,
        action_json: s.json.to_string(),
        values_text: s.values.to_string(),
        rel: Rel::NotCount,
        strict: false,
        what,
    }
}

fn pattern(step: i32, value: &str, s: &Seen) -> CheckValue {
    loose(step, ValueKind::Pattern, value, false, "the pattern", s)
}

/// An attribute's value: `true` or `false` (ignoring case and spaces) is a state, any other a text.
fn attribute_value(step: i32, value: &str, s: &Seen) -> CheckValue {
    match value.trim().to_lowercase().as_str() {
        b @ ("true" | "false") => loose(step, ValueKind::State, b, true, "the state", s),
        _ => loose(step, ValueKind::Text, value, true, "the attribute value", s),
    }
}

/// The name and the text the last link of an `expect_visible` locator ends with.
fn visible_values(step: i32, selector: &Target, out: &mut Vec<CheckValue>, s: &Seen) {
    let last = match selector {
        Target::One(l) => l,
        Target::Chain(links) => match links.last() {
            Some(l) => l,
            None => return,
        },
        Target::Legacy(_) => return,
    };
    for v in [&last.text, &last.name].into_iter().flatten() {
        out.push(loose(step, ValueKind::Text, v, last.exact, "the text", s));
    }
}

fn values_of_action(step: i32, action: &Action, out: &mut Vec<CheckValue>) {
    let value = serde_json::to_value(action).unwrap_or_default();
    let json = value.to_string().to_lowercase();
    let mut strings = Vec::new();
    string_values(&value, &mut strings);
    let values = strings.join(" ");
    let s = &Seen { json: &json, values: &values };
    match action {
        Action::ExpectCount { equals, at_least, at_most, .. } | Action::ExpectRowCount { equals, at_least, at_most, .. } => {
            if let Some(n) = equals {
                out.push(count(step, *n, Rel::Equal, s));
            }
            if let Some(n) = at_least {
                out.push(count(step, *n, Rel::AtLeast, s));
            }
            if let Some(n) = at_most {
                out.push(count(step, *n, Rel::AtMost, s));
            }
        }
        Action::ExpectOrder { texts, .. } => {
            if texts.len() >= 2 {
                out.push(count(step, texts.len() as u32, Rel::OrderLength, s));
            }
            out.extend(texts.iter().map(|t| text(step, t, false, s)));
        }
        Action::ExpectEach { min, equals, contains, matches, attribute, .. } => {
            if let Some(n) = min {
                out.push(count(step, *n, Rel::AtLeast, s));
            }
            if attribute.is_none() {
                if let Some(v) = equals {
                    out.push(text(step, v, true, s));
                }
                if let Some(v) = contains {
                    out.push(text(step, v, false, s));
                }
            } else {
                if let Some(v) = equals {
                    out.push(attribute_value(step, v, s));
                }
                if let Some(v) = contains {
                    out.push(loose(step, ValueKind::Text, v, false, "the attribute value", s));
                }
            }
            if let Some(p) = matches {
                out.push(pattern(step, p, s));
            }
        }
        Action::CheckText { value } => out.push(text(step, value, false, s)),
        Action::ExpectText { equals, matches, .. } => {
            if let Some(v) = equals {
                out.push(text(step, v, true, s));
            }
            if let Some(p) = matches {
                out.push(pattern(step, p, s));
            }
        }
        Action::ExpectAttribute { equals, matches, .. } => {
            if let Some(v) = equals {
                out.push(attribute_value(step, v, s));
            }
            if let Some(p) = matches {
                out.push(pattern(step, p, s));
            }
        }
        Action::ExpectStyle { equals, contains, not_equals, .. } => {
            if let Some(v) = equals {
                out.push(loose(step, ValueKind::Text, v, true, "the style value", s));
            }
            if let Some(v) = contains {
                out.push(loose(step, ValueKind::Text, v, false, "the style value", s));
            }
            if let Some(v) = not_equals {
                out.push(loose(step, ValueKind::Text, v, true, "the style value", s));
            }
        }
        Action::ExpectChecked { checked, .. } => {
            out.push(loose(step, ValueKind::State, if *checked { "true" } else { "false" }, true, "the state", s));
        }
        Action::ExpectVisible { selector, .. } => visible_values(step, selector, out, s),
        Action::ExpectContainsText { value, .. } => out.push(text(step, value, false, s)),
        // What a downloaded file must hold. The file's name and its sheet say where to look, not what
        // is there, so they are not values.
        Action::ExpectDownload { headers, cells, contains_text, pdf, .. } => {
            let mut said: Vec<(String, bool)> = contains_text.iter().flatten().map(|v| (v.clone(), false)).collect();
            if let Some(HeadersSpec::Exact(list) | HeadersSpec::Contains(list)) = headers {
                let exact = matches!(headers, Some(HeadersSpec::Exact(_)));
                said.extend(list.iter().map(|v| (v.clone(), exact)));
            }
            said.extend(cells.iter().flatten().map(|c| (c.text.clone(), c.how == CellMatch::Exact)));
            if let Some(p) = pdf {
                said.extend(p.contains.iter().flat_map(|c| c.list()).map(|v| (v, false)));
                said.extend(p.on_page.iter().flatten().flat_map(|page| page.contains.list()).map(|v| (v, false)));
                if let Some(pages) = &p.pages {
                    for (n, rel) in [(pages.equals, Rel::Equal), (pages.at_least, Rel::AtLeast), (pages.at_most, Rel::AtMost)] {
                        if let Some(n) = n {
                            out.push(count(step, n, rel, s));
                        }
                    }
                }
            }
            out.extend(said.iter().map(|(v, exact)| loose(step, ValueKind::Text, v, *exact, "the text", s)));
        }
        Action::ExpectDialog { text: t, contains, .. } => {
            if let Some(v) = t {
                out.push(text(step, v, true, s));
            }
            if let Some(v) = contains {
                out.push(text(step, v, false, s));
            }
        }
        Action::ExpectRow { cells, .. } | Action::ExpectNoRow { cells, .. } => {
            out.extend(cells.0.iter().map(|(_, v)| text(step, v, false, s)));
        }
        Action::WhenVisible { then, .. } | Action::RepeatWhileVisible { then, .. } => {
            for a in then {
                values_of_action(step, a, out);
            }
        }
        _ => {}
    }
}

/// Every literal value the checks of `step` use, nested ones (the `then` of
/// `when_visible` and `repeat_while_visible`) included.
pub fn values_of_step(step: &StepScript) -> Vec<CheckValue> {
    let mut out = Vec::new();
    for a in &step.actions {
        values_of_action(step.step_number, a, &mut out);
    }
    out
}

/// Values a check may use without the case holding them.
#[derive(Debug, Clone, Default)]
pub struct Exemptions {
    pub test_prefix: String,
    pub typed: Vec<String>,
    pub uploads: Vec<String>,
    pub precondition_values: Vec<String>,
}

fn walk<'a>(actions: &'a [Action], f: &mut dyn FnMut(&'a Action)) {
    for a in actions {
        f(a);
        if let Action::WhenVisible { then, .. } | Action::RepeatWhileVisible { then, .. } = a {
            walk(then, f);
        }
    }
}

/// `typed`: every `fill` value and every string input of a `use_component`
/// in the whole script, folded, at least 3 chars; `uploads`: each
/// `upload.file` and its stem without extension, folded;
/// `precondition_values`: each precondition's value as a string (a number as
/// its digits).
pub fn exemptions(script: &CaseScript, test_prefix: &str) -> Exemptions {
    let mut typed: Vec<String> = Vec::new();
    let mut uploads: Vec<String> = Vec::new();
    for step in &script.steps {
        walk(&step.actions, &mut |a| match a {
            Action::Fill { value, .. } => typed.push(fold(value)),
            Action::UseComponent { inputs, .. } => {
                typed.extend(inputs.values().filter_map(|v| v.as_str()).map(fold));
            }
            Action::Upload { file, .. } => {
                uploads.push(fold(file));
                if let Some((stem, _)) = file.rsplit_once('.') {
                    uploads.push(fold(stem));
                }
            }
            _ => {}
        });
    }
    typed.retain(|t| t.chars().count() >= 3);
    let precondition_values = script
        .preconditions
        .iter()
        .map(|p| match &p.value {
            serde_json::Value::String(s) => fold(s),
            serde_json::Value::Null => String::new(),
            other => fold(&other.to_string()),
        })
        .filter(|v| !v.is_empty())
        .collect();
    Exemptions { test_prefix: fold(test_prefix), typed, uploads, precondition_values }
}

/// What the value check found.
#[derive(Debug, Clone, Default)]
pub struct ValueReport {
    pub refusals: Vec<String>,
    /// Values the case does not hold that a `because` covers, one line each.
    pub listed: Vec<String>,
    /// One per value of each step in scope that no refusal names, in script
    /// order, with where it comes from.
    pub sourced: Vec<SourcedValue>,
    pub notes: Vec<String>,
}

/// One check value and its source. `provenance` None = exempt (a bare
/// count, a short text, a placeholder or a precondition's own value).
#[derive(Debug, Clone, PartialEq)]
pub struct SourcedValue {
    pub step: i32,
    pub value: String,
    pub provenance: Option<Provenance>,
    pub why: String,
}

/// At most this many assumed or invented values are listed in a save's answer.
pub const MAX_VALUE_LINES: usize = 40;

/// The save answer's lines about check values: per case with any sourced
/// value a summary, then each assumed or invented one (at most
/// `MAX_VALUE_LINES` in all), then once a request to repeat them.
pub fn value_source_lines(sourced: &[(i32, Vec<SourcedValue>)]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut written = 0usize;
    let mut more = 0usize;
    for (case_id, values) in sourced {
        if values.is_empty() {
            continue;
        }
        let count = |p: Option<Provenance>| values.iter().filter(|v| v.provenance == p).count();
        out.push(format!(
            "case {case_id}: check values: {} from the case, {} assumed, {} invented, {} exempt",
            count(Some(Provenance::FromCase)),
            count(Some(Provenance::Assumed)),
            count(Some(Provenance::Invented)),
            count(None),
        ));
        for v in values.iter().filter(|v| matches!(v.provenance, Some(Provenance::Assumed | Provenance::Invented))) {
            if written >= MAX_VALUE_LINES {
                more += 1;
                continue;
            }
            written += 1;
            let word = v.provenance.map_or("", Provenance::word);
            out.push(format!("case {case_id}: {word}: step {} \"{}\" - {}", v.step, v.value, v.why));
        }
    }
    if more > 0 {
        out.push(format!("and {more} more assumed or invented values - get_autorun_script shows each step's because"));
    }
    if written > 0 {
        out.push("Repeat every assumed and invented value in your report before you say the work is done.".to_string());
    }
    out
}

/// A reason that opens with one of these names the app as its source.
const APP_SOURCE_START: [&str; 8] = [
    "app shows",
    "app has",
    "app lists",
    "page shows",
    "page has",
    "page lists",
    "environment has",
    "site has",
];
/// ... and one that says any of these anywhere does too.
const APP_SOURCE_ANYWHERE: [&str; 3] = ["the app shows", "the page shows", "what is on the page"];

/// Whether a because's, or a gives_up's, why names the app as its source.
pub fn names_the_app(why: &str) -> bool {
    let why = fold(why);
    let opening = why.strip_prefix("the ").unwrap_or(&why);
    APP_SOURCE_START.iter().any(|p| opening.starts_with(p)) || APP_SOURCE_ANYWHERE.iter().any(|p| why.contains(p))
}

/// Why a value needs no because.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Exempt {
    /// A bare count, a short text, a placeholder, a precondition's value.
    Bare,
    Typed,
    Upload,
    Prefix,
}

/// Why a pattern needs no because: every word written in it (some form of it) is in a value the script
/// types, or in the test name prefix, or it starts with that prefix.
fn pattern_exempt(value: &str, ex: &Exemptions) -> Option<Exempt> {
    let words = literal_word_forms(value);
    let typed = |w: &String| ex.typed.iter().any(|t| has_phrase(t, w));
    let in_prefix = |w: &String| !ex.test_prefix.is_empty() && has_phrase(&ex.test_prefix, w);
    if !words.is_empty() {
        if words.iter().all(|alts| alts.iter().any(typed)) {
            return Some(Exempt::Typed);
        }
        if words.iter().all(|alts| alts.iter().any(|w| typed(w) || in_prefix(w))) {
            return Some(Exempt::Prefix);
        }
    }
    prefixed(&fold(value.trim_start_matches('^')), &ex.test_prefix).then_some(Exempt::Prefix)
}

fn exempt_kind(v: &CheckValue, ex: &Exemptions) -> Option<Exempt> {
    let folded = fold(&v.value);
    match v.kind {
        // A bound that only asks for something to be there (`at_least` 0 or 1, `min` 1) is presence,
        // not a number the case gives; an exact count or an upper bound of 0 or 1 is compared.
        ValueKind::Count => {
            let presence = v.rel == Rel::AtLeast && matches!(v.value.as_str(), "0" | "1");
            (presence || ex.precondition_values.contains(&folded)).then_some(Exempt::Bare)
        }
        ValueKind::Pattern => {
            if v.value.contains("{{") || ex.precondition_values.contains(&folded) {
                Some(Exempt::Bare)
            } else {
                pattern_exempt(&v.value, ex)
            }
        }
        ValueKind::State => {
            (v.value.contains("{{") || ex.precondition_values.contains(&folded)).then_some(Exempt::Bare)
        }
        ValueKind::Text => {
            if (folded.chars().count() < 3 && !is_number_text(&v.value)) || v.value.contains("{{") {
                return Some(Exempt::Bare);
            }
            if ex.precondition_values.contains(&folded) {
                return Some(Exempt::Bare);
            }
            if ex.typed.iter().any(|t| has_phrase(&folded, t) || has_phrase(t, &folded)) {
                return Some(Exempt::Typed);
            }
            if ex.uploads.contains(&folded) {
                return Some(Exempt::Upload);
            }
            prefixed(&folded, &ex.test_prefix).then_some(Exempt::Prefix)
        }
    }
}

fn exempt(v: &CheckValue, ex: &Exemptions) -> bool {
    exempt_kind(v, ex).is_some()
}

/// A step's words, lowercased: runs of letters and digits, an apostrophe inside a word dropped
/// ("isn't" is "isnt").
fn words_of(step_text: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    for c in step_text.chars() {
        if c.is_alphanumeric() {
            word.push(c);
        } else if c != '\'' && !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

/// Whether the word at `i` is a negation: "not", "isn't" and the like, or "no longer" (at "longer").
fn is_negator(words: &[String], i: usize) -> bool {
    match words[i].as_str() {
        "not" | "isnt" | "arent" | "wasnt" | "werent" | "cannot" | "cant" => true,
        "longer" => i > 0 && words[i - 1] == "no",
        _ => false,
    }
}

/// A negation sits directly before the word at `i`, within 2 words ("is not selected", "is not yet selected").
fn negated(words: &[String], i: usize) -> bool {
    (1..=2).any(|d| i >= d && is_negator(words, i - d))
}

/// What the word at `i` says about a state: `Some(true)` on, `Some(false)` off, `None` nothing. A negation
/// flips it. `on` and `off` count only after "is", "turned" or "switched" (a negation may sit between) and
/// never before "the", "a", "this", "page" and the like, so "Click on the card" and "the card is on the
/// page" say nothing.
fn state_of(words: &[String], i: usize) -> Option<bool> {
    let w = words[i].as_str();
    let said = if STATE_TRUE_WORDS.contains(&w) {
        true
    } else if STATE_FALSE_WORDS.contains(&w) {
        false
    } else if w == "on" || w == "off" {
        let led = (1..=3).any(|d| i >= d && SWITCH_LEADS.contains(&words[i - d].as_str()));
        // "turned on" and "switched on" are a state whatever follows; "is on" is not when a place follows.
        let switched = i >= 1 && matches!(words[i - 1].as_str(), "turned" | "switched");
        let place = words.get(i + 1).is_some_and(|n| NOT_A_STATE_AFTER.contains(&n.as_str()));
        if !led || (place && !switched) {
            return None;
        }
        w == "on"
    } else {
        return None;
    };
    Some(said != negated(words, i))
}

/// Whether the step's own words say a state is `on` (true) or off (false).
fn step_says_state(step_text: &str, on: bool) -> bool {
    let words = words_of(step_text);
    (0..words.len()).any(|i| state_of(&words, i) == Some(on))
}

/// Whether the step's own words say there are none: "0", "zero", "none", "nothing" or "no <noun>".
fn step_says_none(step_text: &str) -> bool {
    let words = words_of(step_text);
    words.iter().enumerate().any(|(i, w)| {
        NONE_WORDS.contains(&w.as_str()) || (w == "no" && words.get(i + 1).is_some_and(|next| next != "longer"))
    })
}

/// Whether the step's own words say there is one: "1", "one" or "a single".
fn step_says_one(step_text: &str) -> bool {
    let words = words_of(step_text);
    words
        .iter()
        .enumerate()
        .any(|(i, w)| ONE_WORDS.contains(&w.as_str()) || (w == "single" && i > 0 && words[i - 1] == "a"))
}

/// Whether the case holds `v`: a strict count by `case_numbers`; a count 0 or 1, or the text "0" or
/// "1", by the step's own words for none or one (`step_says_none`, `step_says_one`); a text by
/// `CaseText::holds`; a pattern when it writes words and the case holds each, in either form an
/// optional letter or group gives it (`literal_word_forms`); `true` or `false` by the step's own words
/// for on or off (`step_says_state`).
pub fn held_by_case(v: &CheckValue, case: &CaseText) -> bool {
    held_with(v, case, &case.case_numbers())
}

fn held_with(v: &CheckValue, case: &CaseText, numbers: &BTreeSet<u32>) -> bool {
    let by_step = || match v.value.as_str() {
        "0" => step_says_none(&case.step_folded(v.step)),
        "1" => step_says_one(&case.step_folded(v.step)),
        _ => false,
    };
    match v.kind {
        ValueKind::Count if v.strict => v.value.parse::<u32>().is_ok_and(|c| numbers.contains(&c)),
        ValueKind::Count => by_step(),
        ValueKind::Text => by_step() || case.holds(&v.value),
        ValueKind::Pattern => {
            let words = literal_word_forms(&v.value);
            !words.is_empty() && words.iter().all(|alts| alts.iter().any(|w| case.holds(w)))
        }
        ValueKind::State => match v.value.as_str() {
            "true" => step_says_state(&case.step_folded(v.step), true),
            "false" => step_says_state(&case.step_folded(v.step), false),
            _ => false,
        },
    }
}

/// What a step's own words must say to hold a state the case lacks: the words, so the writer can see why.
fn state_hint(v: &CheckValue) -> String {
    if v.kind != ValueKind::State {
        return String::new();
    }
    if v.value == "true" {
        format!(
            "; a step holds true with: {}, or \"is on\", \"turned on\", \"switched on\"",
            STATE_TRUE_WORDS.join(", ")
        )
    } else {
        format!(
            "; a step holds false with: {}, \"not selected\", \"not checked\", or \"is off\", \"turned off\", \"switched off\"",
            STATE_FALSE_WORDS.join(", ")
        )
    }
}

/// The invented source of a value the script itself makes.
fn invented_by(kind: Exempt) -> &'static str {
    match kind {
        Exempt::Typed => "typed by the script",
        Exempt::Upload => "uploaded by the script",
        Exempt::Prefix => "a record named with the test name prefix",
        Exempt::Bare => "",
    }
}

/// Starts with the test prefix as a whole token: the prefix, then a space,
/// `-`, `_` or the end (a prefix that ends in punctuation needs nothing more).
fn prefixed(folded: &str, prefix: &str) -> bool {
    if prefix.is_empty() {
        return false;
    }
    let Some(rest) = folded.strip_prefix(prefix) else { return false };
    prefix.chars().next_back().is_some_and(|c| !c.is_alphanumeric())
        || rest.chars().next().is_none_or(|c| matches!(c, ' ' | '-' | '_'))
}

fn because_format_refusal(n: i32, entries: &[Because]) -> Option<String> {
    let bad = entries
        .iter()
        .any(|b| b.value.trim().is_empty() || b.why.trim().is_empty() || b.why.chars().count() > 200 || b.why.contains('\n'));
    bad.then(|| format!("step {n}: each \"because\" is {{ \"value\", \"why\" }}, the why one line of at most 200 characters"))
}

fn because_provenance_refusal(n: i32, entries: &[Because]) -> Option<String> {
    entries.iter().any(|b| b.provenance == Some(Provenance::FromCase)).then(|| {
        format!(
            "step {n}: a \"because\" is \"assumed\" or \"invented\" - from_case is what the save works out for a value the case holds, which needs no because"
        )
    })
}

fn because_app_refusals(n: i32, entries: &[Because]) -> Vec<String> {
    entries
        .iter()
        .filter(|b| names_the_app(&b.why))
        .map(|b| {
            format!(
                "step {n}: the \"because\" for \"{}\" names the app as its source - a check's value comes from the case; when the app differs, call report_case_divergence",
                b.value
            )
        })
        .collect()
}

/// Holds the literal values of the case steps in scope (`only_steps` None =
/// every case step) against the case's own words. The clean-up never.
pub fn check_case_values(
    script: &CaseScript,
    case: &CaseText,
    only_steps: Option<&[i32]>,
    test_prefix: &str,
) -> ValueReport {
    check_case_values_against(script, case, only_steps, test_prefix, None)
}

/// [`check_case_values`] for a repair of `old`: a value the old step already had (same kind,
/// same text) is not held again - only a value that is new or changed in a step in scope is.
pub fn check_case_values_against(
    script: &CaseScript,
    case: &CaseText,
    only_steps: Option<&[i32]>,
    test_prefix: &str,
    old: Option<&CaseScript>,
) -> ValueReport {
    let ex = exemptions(script, test_prefix);
    let numbers = case.case_numbers();
    let mut report = ValueReport::default();
    let push_refusal = |r: &mut ValueReport, s: String| {
        if !r.refusals.contains(&s) {
            r.refusals.push(s);
        }
    };
    for step in script.case_steps() {
        let n = step.step_number;
        if only_steps.is_some_and(|only| !only.contains(&n)) {
            continue;
        }
        if let Some(r) = because_format_refusal(n, &step.because) {
            push_refusal(&mut report, r);
        }
        if let Some(r) = because_provenance_refusal(n, &step.because) {
            push_refusal(&mut report, r);
        }
        for r in because_app_refusals(n, &step.because) {
            push_refusal(&mut report, r);
        }
        let values = values_of_step(step);
        let mut used: BTreeSet<String> = BTreeSet::new();
        for v in &values {
            used.insert(fold(&v.value));
        }
        // What the old step already had is not new (a repair of a script saved before the rule).
        let had: Vec<(ValueKind, String)> = old
            .and_then(|o| old_step(o, n))
            .map(|o| values_of_step(o).into_iter().map(|v| (v.kind, fold(&v.value))).collect())
            .unwrap_or_default();
        let mut sourced_here: BTreeSet<String> = BTreeSet::new();
        let mut source = |report: &mut ValueReport, v: &CheckValue, provenance: Option<Provenance>, why: String| {
            if sourced_here.insert(fold(&v.value)) {
                report.sourced.push(SourcedValue { step: n, value: v.value.clone(), provenance, why });
            }
        };
        for v in &values {
            // Counted and listed as before, but never refused: the old step had it.
            let had_it = had.contains(&(v.kind, fold(&v.value)));
            let exempt_as = exempt_kind(v, &ex);
            let in_case = held_with(v, case, &numbers);
            let covered = step.because.iter().find(|b| fold(&b.value) == fold(&v.value));
            if exempt_as == Some(Exempt::Bare) {
                source(&mut report, v, None, String::new());
                continue;
            }
            if in_case {
                source(&mut report, v, Some(Provenance::FromCase), String::new());
                continue;
            }
            if let Some(kind) = exempt_as {
                match covered {
                    Some(b) => source(&mut report, v, Some(b.provenance.unwrap_or(Provenance::Assumed)), b.why.trim().to_string()),
                    None => source(&mut report, v, Some(Provenance::Invented), invented_by(kind).to_string()),
                }
                continue;
            }
            // An order's own length follows from its texts, each held on its
            // own: only a stated count can contradict it.
            let order_length = v.rel == Rel::OrderLength;
            if v.kind == ValueKind::Count && v.strict && !had_it {
                if let Some(r) = contradicts_stated_count(case, n, v) {
                    push_refusal(&mut report, r);
                    continue;
                }
            }
            match covered {
                Some(b) => {
                    source(&mut report, v, Some(b.provenance.unwrap_or(Provenance::Assumed)), b.why.trim().to_string());
                    let line = format!("step {n} \"{}\" - {}", v.value, b.why.trim());
                    if !report.listed.contains(&line) {
                        report.listed.push(line);
                    }
                }
                None if order_length || had_it => {}
                None => {
                    push_refusal(
                        &mut report,
                        format!(
                            "step {n}: {} \"{}\" is not in the case - take it from the case's steps, expected results or preconditions as written, or give it a \"because\" on the step ({{ \"value\": \"{}\", \"why\": \"where the case defines it\" }}){}",
                            v.what,
                            v.value,
                            v.value,
                            state_hint(v)
                        ),
                    );
                }
            }
        }
        for b in &step.because {
            let folded = fold(&b.value);
            if !folded.is_empty() && !used.contains(&folded) {
                report.notes.push(format!(
                    "step {n}: note: \"because\" names \"{}\", which no check of the step uses",
                    b.value
                ));
            }
        }
    }
    report
}

/// Whether `text` holds the noun `stem` as a word (or its plural).
fn has_noun(text: &str, stem: &str) -> bool {
    text.split(|c: char| !c.is_alphanumeric())
        .any(|w| w == stem || w.strip_suffix('s') == Some(stem) || w.strip_suffix("es") == Some(stem))
}

/// Words that locate a thing without naming what it is.
const STRUCTURAL: &[&str] = &[
    "option", "options", "row", "rows", "listitem", "li", "tr", "td", "ul", "ol", "div", "span", "a", "item", "items",
    "cell", "cells", "combobox", "select", "list", "listbox", "table", "grid", "tbody", "gridcell", "rowgroup",
];

/// Whether the check's own values name a countable thing of their own (a
/// role, name or text such as "button", "Save", "card"): then the case
/// step's words are not used to find the noun. A bare css, or a structural
/// word only, names nothing.
fn names_own_noun(values_text: &str) -> bool {
    values_text.split_whitespace().any(|w| {
        !w.contains(|c: char| c.is_ascii_digit() || "#.[]>:=()*_".contains(c)) && !STRUCTURAL.contains(&w)
    })
}

/// A bound or count the case's stated count rules out.
fn contradicts(rel: Rel, got: u32, stated: u32) -> bool {
    match rel {
        Rel::Equal => got != stated,
        Rel::AtLeast | Rel::OrderLength => got > stated,
        Rel::AtMost => got < stated,
        Rel::NotCount => false,
    }
}

/// The case states another count of the thing this check counts. The noun
/// is looked for as a word in the check's own values and in the case step's
/// own words, never in the check's keys.
fn contradicts_stated_count(case: &CaseText, n: i32, v: &CheckValue) -> Option<String> {
    let Ok(got) = v.value.parse::<u32>() else { return None };
    let mut sources: Vec<&str> = Vec::new();
    let mut step_words = String::new();
    if n >= 1 {
        if let Some((action, expected)) = case.steps.get(n as usize - 1) {
            sources.push(action);
            sources.push(expected);
            step_words = format!("{action} {expected}").to_lowercase();
        }
    }
    sources.push(&case.preconditions);
    sources
        .iter()
        .flat_map(|t| stated_counts(t))
        .find(|sc| {
            contradicts(v.rel, got, sc.n) && (has_noun(&v.values_text, &sc.stem) || (!names_own_noun(&v.values_text) && has_noun(&step_words, &sc.stem)))
        })
        .map(|sc| {
            format!(
                "step {n}: the check counts {} but the case says {} (\"{}\") - a count comes from the case as written; if the app shows {}, call report_case_divergence and keep {}",
                v.value, sc.n, sc.phrase, v.value, sc.n
            )
        })
}

/// Every check action of the case's steps (not the clean-up), nested ones
/// included.
pub fn check_count(script: &CaseScript) -> usize {
    let mut n = 0;
    for step in script.case_steps() {
        walk(&step.actions, &mut |a| {
            if a.is_check() {
                n += 1;
            }
        });
    }
    n
}

/// The kind of every check of a step (nested ones included, exactly the
/// actions `check_count` counts), as `Action` serializes its `kind`.
pub fn check_kinds_of_step(step: &StepScript) -> Vec<String> {
    let mut kinds = Vec::new();
    walk(&step.actions, &mut |a| {
        if a.is_check() {
            kinds.push(kind_of(a));
        }
    });
    kinds
}

fn kind_of(a: &Action) -> String {
    serde_json::to_value(a)
        .ok()
        .and_then(|v| v.get("kind").and_then(|k| k.as_str().map(str::to_string)))
        .unwrap_or_default()
}

/// A step by number, the clean-up step included: its checks can be given up too.
fn old_step(script: &CaseScript, n: i32) -> Option<&StepScript> {
    script.steps.iter().find(|s| s.step_number == n)
}

/// The kinds of the checks step `n` loses: the old step's kinds minus the
/// new step's, each new one cancelling one old one of its kind.
fn removed_kinds(old: &CaseScript, new: &CaseScript, n: i32) -> Vec<String> {
    let mut removed = old_step(old, n).map(check_kinds_of_step).unwrap_or_default();
    for k in new.steps.iter().find(|s| s.step_number == n).map(check_kinds_of_step).unwrap_or_default() {
        if let Some(i) = removed.iter().position(|r| *r == k) {
            removed.remove(i);
        }
    }
    removed
}

fn gives_up_kind(g: &GiveUp) -> String {
    g.kind.trim().to_lowercase()
}

fn gives_up_format_ok(g: &GiveUp) -> bool {
    !(g.kind.trim().is_empty() || g.why.trim().is_empty() || g.why.chars().count() > 200 || g.why.contains('\n'))
}

/// Why one entry is refused, if it is (the shape's sentence is the same for
/// every entry, so the caller says it once).
fn gives_up_refusal(old: &CaseScript, declared_steps: &[i32], g: &GiveUp) -> Option<String> {
    if !gives_up_format_ok(g) {
        return Some(GIVES_UP_SHAPE.to_string());
    }
    let n = g.step;
    if names_the_app(&g.why) {
        return Some(format!(
            "step {n}: a check is never given up because of what the app shows - keep it as the case says and call report_case_divergence"
        ));
    }
    let kind = gives_up_kind(g);
    let has = declared_steps.contains(&n)
        && old_step(old, n).is_some_and(|s| check_kinds_of_step(s).iter().any(|k| *k == kind));
    (!has).then(|| {
        format!("step {n}: gives_up names a {kind} check, and the old step {n} has none - name the kind of a check the repair removes")
    })
}

const GIVES_UP_SHAPE: &str =
    "the edit's gives_up entries are { \"step\", \"kind\", \"why\" }, the why one line of at most 200 characters";

/// Whether `new` keeps the pattern `old`: the same pattern, or one that still writes each word `old` writes
/// (a widened pattern), in either form.
fn pattern_keeps(old: &str, new: &str) -> bool {
    if fold(old) == fold(new) {
        return true;
    }
    let old_words = literal_word_forms(old);
    let new_words = literal_word_forms(new);
    !old_words.is_empty() && old_words.iter().all(|o| new_words.iter().any(|n| o.iter().any(|f| n.contains(f))))
}

/// What a repair may not do to the checks: go under the script's floor
/// without naming each check it gives up (with why), or move a value away
/// from the case. `declared_steps` are the steps the edit declares.
pub fn check_repair_values(
    old: &CaseScript,
    new: &CaseScript,
    case: &CaseText,
    declared_steps: &[i32],
    gives_up: &[GiveUp],
    test_prefix: &str,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let push = |out: &mut Vec<String>, s: String| {
        if !out.contains(&s) {
            out.push(s);
        }
    };
    // 1. Each entry on its own.
    // An entry must also match a check the repair really removes: each
    // removed check is matched once.
    let mut valid: Vec<&GiveUp> = Vec::new();
    let mut removed: std::collections::BTreeMap<i32, Vec<String>> = std::collections::BTreeMap::new();
    for g in gives_up {
        if let Some(r) = gives_up_refusal(old, declared_steps, g) {
            push(&mut out, r);
            continue;
        }
        let n = g.step;
        let kind = gives_up_kind(g);
        let left = removed.entry(n).or_insert_with(|| removed_kinds(old, new, n));
        match left.iter().position(|k| *k == kind) {
            Some(i) => {
                left.remove(i);
                valid.push(g);
            }
            None => push(
                &mut out,
                format!("step {n}: gives_up names a {kind} check, and the old step {n} has none - name the kind of a check the repair removes"),
            ),
        }
    }
    // 2. The floor.
    let floor = if old.checks > 0 { old.checks as usize } else { check_count(old) };
    let kept = check_count(new);
    // A clean-up step's checks are not in the count, so giving one up lowers nothing.
    let named = valid.iter().filter(|g| old.case_steps().any(|s| s.step_number == g.step)).count();
    if kept + named < floor {
        push(
            &mut out,
            format!(
                "the repair lowers the script's checks from {floor} to {kept} - a repair keeps every check unless its edit names each one it gives up, with why, in gives_up, and the person sees it; a check the app fails is a divergence to report with report_case_divergence"
            ),
        );
    }
    // 3. Each declared step against the checks it lost.
    let mut steps: Vec<i32> = declared_steps.to_vec();
    steps.sort_unstable();
    steps.dedup();
    // Every old step, not only the declared ones: a check lost where nothing
    // was declared (the clean-up step, say) is not offset by one added elsewhere.
    let mut every: Vec<i32> = steps.iter().copied().chain(old.steps.iter().map(|s| s.step_number)).collect();
    every.sort_unstable();
    every.dedup();
    for &n in &every {
        let Some(o) = old_step(old, n) else { continue };
        let before = check_kinds_of_step(o).len();
        let after = new.steps.iter().find(|s| s.step_number == n).map_or(0, |s| check_kinds_of_step(s).len());
        let lost = before.saturating_sub(after);
        let given = valid.iter().filter(|g| g.step == n).count();
        if given < lost {
            push(
                &mut out,
                format!(
                    "step {n}: the repair removes {lost} of its checks and gives_up names {given} - add {{ \"step\": {n}, \"kind\": \"<the check's kind>\", \"why\": \"...\" }} for each one, or keep it"
                ),
            );
        } else if given > lost {
            push(
                &mut out,
                format!("step {n}: gives_up names {given} checks but the repair removes {lost} there - name only the checks it removes"),
            );
        }
    }
    // 4. Values the case gives are kept.
    let ex = exemptions(old, test_prefix);
    let numbers = case.case_numbers();
    for &n in &steps {
        let Some(o) = old_step(old, n) else { continue };
        let new_values = new.case_steps().find(|s| s.step_number == n).map(values_of_step).unwrap_or_default();
        for v in values_of_step(o) {
            // Every kind of value is kept when the case gives it: a state, a pattern, an attribute's or a
            // style's value, a visible name, a count of 0 or 1 and a short number as much as the rest. A
            // value the case does not give (one a `because` covered) a repair may change; the save then
            // holds what it wrote like any new value.
            if exempt(&v, &ex) {
                continue;
            }
            if !held_with(&v, case, &numbers) {
                continue;
            }
            let kept = match v.kind {
                ValueKind::Count => new_values.iter().any(|w| w.kind == ValueKind::Count && w.value == v.value),
                ValueKind::State => new_values.iter().any(|w| w.kind == ValueKind::State && w.value == v.value),
                ValueKind::Pattern => new_values.iter().any(|w| w.kind == ValueKind::Pattern && pattern_keeps(&v.value, &w.value)),
                ValueKind::Text => {
                    let old_fold = fold(&v.value);
                    new_values.iter().any(|w| {
                        w.kind == ValueKind::Text && {
                            let new_fold = fold(&w.value);
                            new_fold == old_fold || has_phrase(&new_fold, &old_fold)
                        }
                    })
                }
            };
            if kept {
                continue;
            }
            let held_by = serde_json::from_str::<serde_json::Value>(&v.action_json)
                .ok()
                .and_then(|j| j.get("kind").and_then(|k| k.as_str().map(str::to_string)))
                .unwrap_or_default();
            if valid.iter().any(|g| g.step == n && gives_up_kind(g) == held_by) {
                continue;
            }
            push(
                &mut out,
                format!(
                    "step {n}: the repair drops \"{}\", which the case gives - a repair keeps the case's values; if the app shows otherwise, call report_case_divergence, or ask a person to rewrite the case first",
                    v.value
                ),
            );
        }
    }
    out
}
