//! The expected-result floor: every way an Auto Run script falls short of
//! its test case's expected results. Pure - no browser, no filesystem.

use super::CaseScript;
use crate::steps_xml::Step;

/// One expected result of the test case, by step position (1-based).
pub struct Expected {
    pub step_number: i32,
    pub expected: String,
    /// The case step is a Shared Steps reference: its steps live in another
    /// work item, and a script writes no step for it (guide.rs).
    pub shared: bool,
}

/// The case's expected results, by position. Empty ones are kept: they are
/// the "no expectation" case, which the floor treats differently from a
/// missing step.
pub fn expected_of(steps: &[Step]) -> Vec<Expected> {
    steps
        .iter()
        .enumerate()
        .map(|(i, s)| Expected {
            step_number: i as i32 + 1,
            expected: s.expected.trim().to_string(),
            shared: s.shared.is_some(),
        })
        .collect()
}

fn truncated(s: &str) -> String {
    s.chars().take(60).collect()
}

/// The script step numbers that land on a Shared Steps row of the case,
/// sorted, each once. A script writes no step for such a row; one that does
/// was numbered as if the case had no Shared Steps - how every script saved
/// before 1.25.23 was numbered - so its later steps, and their results, sit
/// a row off. Shared by the save (rule 5 below) and the send (publish.rs).
pub fn steps_on_shared_rows(script_steps: &[i32], shared_rows: &[i32]) -> Vec<i32> {
    let mut hit: Vec<i32> = script_steps.iter().copied().filter(|n| shared_rows.contains(n)).collect();
    hit.sort_unstable();
    hit.dedup();
    hit
}

/// What rule 5 says about script step `n`.
pub fn on_shared_row(n: i32) -> String {
    format!(
        "step {n} is a Shared Steps entry in the case, which a script writes no step for - if this script was numbered without its Shared Steps, add one to every step number from {n} on"
    )
}

/// Every way this script falls short of its case. Empty means it holds.
/// Rules 1-2 are driven by the CASE (only speak about a step position it
/// has an opinion on). Rules 3-4 are driven by the SCRIPT: a step with
/// `unchecked` set is judged wherever it sits, including step 0 and any
/// step past the case's count, where "no case step there" is rule 4. Rule 5
/// refuses a script step on a Shared Steps row.
pub fn check_floor(script: &CaseScript, expected: &[Expected]) -> Vec<String> {
    let mut out: Vec<(i32, String)> = Vec::new();

    for e in expected {
        let want = e.expected.trim();
        let n = e.step_number;
        if want.is_empty() {
            continue;
        }
        match script.steps.iter().find(|s| s.step_number == n) {
            None => {
                out.push((n, format!("step {n} expects \"{}\" but the script has no step {n}", truncated(want))));
            }
            Some(step) => {
                let has_check = step.actions.iter().any(|a| a.is_check());
                if step.unchecked.is_none() && !has_check {
                    out.push((
                        n,
                        format!(
                            "step {n} expects \"{}\" but the script checks nothing there - add an expect_ action, or say why in \"unchecked\"",
                            truncated(want)
                        ),
                    ));
                }
            }
        }
    }

    for step in &script.steps {
        if step.unchecked.is_none() {
            continue;
        }
        let n = step.step_number;
        let has_check = step.actions.iter().any(|a| a.is_check());
        if has_check {
            out.push((n, format!("step {n} says it is unchecked but has a check - drop one or the other")));
            continue;
        }
        let want_here = expected.iter().find(|e| e.step_number == n).is_some_and(|e| !e.expected.trim().is_empty());
        if !want_here {
            out.push((n, format!("step {n} says it is unchecked but the case expects nothing there")));
        }
    }

    let shared_rows: Vec<i32> = expected.iter().filter(|e| e.shared).map(|e| e.step_number).collect();
    let numbers: Vec<i32> = script.steps.iter().map(|s| s.step_number).collect();
    for n in steps_on_shared_rows(&numbers, &shared_rows) {
        out.push((n, on_shared_row(n)));
    }

    out.sort();
    out.dedup();
    out.into_iter().map(|(_, s)| s).collect()
}
