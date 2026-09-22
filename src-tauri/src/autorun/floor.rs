//! The expected-result floor: every way an Auto Run script falls short of
//! its test case's expected results. Pure - no browser, no filesystem.

use super::CaseScript;
use crate::steps_xml::Step;

/// One expected result of the test case, by step position (1-based).
pub struct Expected {
    pub step_number: i32,
    pub expected: String,
}

/// The case's expected results, by position. Empty ones are kept: they are
/// the "no expectation" case, which the floor treats differently from a
/// missing step.
pub fn expected_of(steps: &[Step]) -> Vec<Expected> {
    steps
        .iter()
        .enumerate()
        .map(|(i, s)| Expected { step_number: i as i32 + 1, expected: s.expected.trim().to_string() })
        .collect()
}

fn truncated(s: &str) -> String {
    s.chars().take(60).collect()
}

/// Every way this script falls short of its case. Empty means it holds.
/// Rules 1-2 are driven by the CASE (only speak about a step position it
/// has an opinion on). Rules 3-4 are driven by the SCRIPT: a step with
/// `unchecked` set is judged wherever it sits, including step 0 and any
/// step past the case's count, where "no case step there" is rule 4.
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

    out.sort();
    out.dedup();
    out.into_iter().map(|(_, s)| s).collect()
}
