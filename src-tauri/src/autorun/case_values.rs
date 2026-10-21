//! What a script's checks are held against: the case as written.
//!
//! A check's literal values (a count, a text) come from the case's own
//! words. A value the case does not hold needs a `because` naming where the
//! case defines it, and a count the case states differently is refused. Pure:
//! nothing here reads a file, a page or Azure DevOps.

use std::collections::BTreeSet;

use super::case_text::{fold, stated_counts, CaseText};
use super::seen_check::has_phrase;
use super::{Because, CaseScript, StepScript};
use crate::browser::actions::Action;

/// What kind of value a check carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    Count,
    Text,
}

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
    }
}

fn text(step: i32, value: &str, exact: bool, s: &Seen) -> CheckValue {
    CheckValue {
        step,
        kind: ValueKind::Text,
        value: value.to_string(),
        exact,
        action_json: s.json.to_string(),
        values_text: s.values.to_string(),
        rel: Rel::NotCount,
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
        Action::ExpectEach { min, equals, contains, attribute, .. } => {
            if let Some(n) = min {
                out.push(count(step, *n, Rel::AtLeast, s));
            }
            // An attribute's value is not the case's words, as for `expect_attribute`.
            if attribute.is_none() {
                if let Some(v) = equals {
                    out.push(text(step, v, true, s));
                }
                if let Some(v) = contains {
                    out.push(text(step, v, false, s));
                }
            }
        }
        Action::CheckText { value } => out.push(text(step, value, false, s)),
        Action::ExpectText { equals: Some(v), .. } => out.push(text(step, v, true, s)),
        Action::ExpectContainsText { value, .. } => out.push(text(step, value, false, s)),
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
    pub notes: Vec<String>,
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

fn names_the_app(why: &str) -> bool {
    let why = fold(why);
    let opening = why.strip_prefix("the ").unwrap_or(&why);
    APP_SOURCE_START.iter().any(|p| opening.starts_with(p)) || APP_SOURCE_ANYWHERE.iter().any(|p| why.contains(p))
}

fn exempt(v: &CheckValue, ex: &Exemptions) -> bool {
    let folded = fold(&v.value);
    match v.kind {
        ValueKind::Count => {
            if matches!(v.value.as_str(), "0" | "1") {
                return true;
            }
            ex.precondition_values.contains(&folded)
        }
        ValueKind::Text => {
            if folded.chars().count() < 3 || v.value.contains("{{") {
                return true;
            }
            if prefixed(&folded, &ex.test_prefix) {
                return true;
            }
            if ex.typed.iter().any(|t| has_phrase(&folded, t) || has_phrase(t, &folded)) {
                return true;
            }
            ex.uploads.contains(&folded) || ex.precondition_values.contains(&folded)
        }
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
        for r in because_app_refusals(n, &step.because) {
            push_refusal(&mut report, r);
        }
        let values = values_of_step(step);
        let mut used: BTreeSet<String> = BTreeSet::new();
        for v in &values {
            used.insert(fold(&v.value));
        }
        for v in &values {
            if exempt(v, &ex) {
                continue;
            }
            let in_case = match v.kind {
                ValueKind::Count => v.value.parse::<u32>().is_ok_and(|c| numbers.contains(&c)),
                ValueKind::Text => case.holds(&v.value),
            };
            if in_case {
                continue;
            }
            // An order's own length follows from its texts, each held on its
            // own: only a stated count can contradict it.
            let order_length = v.rel == Rel::OrderLength;
            if v.kind == ValueKind::Count {
                if let Some(r) = contradicts_stated_count(case, n, v) {
                    push_refusal(&mut report, r);
                    continue;
                }
            }
            let covered = step.because.iter().find(|b| fold(&b.value) == fold(&v.value));
            match covered {
                Some(b) => {
                    let line = format!("step {n} \"{}\" - {}", v.value, b.why.trim());
                    if !report.listed.contains(&line) {
                        report.listed.push(line);
                    }
                }
                None if order_length => {}
                None => {
                    let what = if v.kind == ValueKind::Count { "the count" } else { "the text" };
                    push_refusal(
                        &mut report,
                        format!(
                            "step {n}: {what} \"{}\" is not in the case - take it from the case's steps, expected results or preconditions as written, or give it a \"because\" on the step ({{ \"value\": \"{}\", \"why\": \"where the case defines it\" }})",
                            v.value, v.value
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
            contradicts(v.rel, got, sc.n) && (has_noun(&v.values_text, &sc.stem) || has_noun(&step_words, &sc.stem))
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
