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
    /// The check action as JSON, lowercased: what a stated count's noun is
    /// looked for in.
    pub action_json: String,
}

fn count(step: i32, n: u32, exact: bool, action_json: &str) -> CheckValue {
    CheckValue { step, kind: ValueKind::Count, value: n.to_string(), exact, action_json: action_json.to_string() }
}

fn text(step: i32, value: &str, exact: bool, action_json: &str) -> CheckValue {
    CheckValue { step, kind: ValueKind::Text, value: value.to_string(), exact, action_json: action_json.to_string() }
}

fn values_of_action(step: i32, action: &Action, out: &mut Vec<CheckValue>) {
    let json = serde_json::to_string(action).unwrap_or_default().to_lowercase();
    match action {
        Action::ExpectCount { equals, at_least, at_most, .. } | Action::ExpectRowCount { equals, at_least, at_most, .. } => {
            if let Some(n) = equals {
                out.push(count(step, *n, true, &json));
            }
            for n in [at_least, at_most].into_iter().flatten() {
                out.push(count(step, *n, false, &json));
            }
        }
        Action::ExpectOrder { texts, .. } => {
            if texts.len() >= 2 {
                out.push(count(step, texts.len() as u32, true, &json));
            }
            out.extend(texts.iter().map(|t| text(step, t, false, &json)));
        }
        Action::ExpectEach { min, equals, contains, .. } => {
            if let Some(n) = min {
                out.push(count(step, *n, false, &json));
            }
            if let Some(v) = equals {
                out.push(text(step, v, true, &json));
            }
            if let Some(v) = contains {
                out.push(text(step, v, false, &json));
            }
        }
        Action::CheckText { value } => out.push(text(step, value, false, &json)),
        Action::ExpectText { equals: Some(v), .. } => out.push(text(step, v, true, &json)),
        Action::ExpectContainsText { value, .. } => out.push(text(step, value, false, &json)),
        Action::ExpectDialog { text: t, contains, .. } => {
            if let Some(v) = t {
                out.push(text(step, v, true, &json));
            }
            if let Some(v) = contains {
                out.push(text(step, v, false, &json));
            }
        }
        Action::ExpectRow { cells, .. } | Action::ExpectNoRow { cells, .. } => {
            out.extend(cells.0.iter().map(|(_, v)| text(step, v, false, &json)));
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

const APP_SOURCE: [&str; 12] = [
    "app shows",
    "app has",
    "app lists",
    "page shows",
    "page has",
    "page lists",
    "environment has",
    "the site has",
    "was seen",
    "is seen",
    "observed",
    "currently",
];

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
            if !ex.test_prefix.is_empty() && folded.starts_with(&ex.test_prefix) {
                return true;
            }
            if ex.typed.iter().any(|t| has_phrase(&folded, t) || has_phrase(t, &folded)) {
                return true;
            }
            ex.uploads.contains(&folded) || ex.precondition_values.contains(&folded)
        }
    }
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
        .filter(|b| {
            let why = fold(&b.why);
            APP_SOURCE.iter().any(|p| why.contains(p))
        })
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
            let order_length = v.kind == ValueKind::Count && v.action_json.contains("\"texts\"");
            if v.kind == ValueKind::Count && v.exact {
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

/// The case states another count of the thing this check counts.
fn contradicts_stated_count(case: &CaseText, n: i32, v: &CheckValue) -> Option<String> {
    let mut sources: Vec<&str> = Vec::new();
    if n >= 1 {
        if let Some((action, expected)) = case.steps.get(n as usize - 1) {
            sources.push(action);
            sources.push(expected);
        }
    }
    sources.push(&case.preconditions);
    sources
        .iter()
        .flat_map(|t| stated_counts(t))
        .find(|sc| v.action_json.contains(&sc.stem))
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
