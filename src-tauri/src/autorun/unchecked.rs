//! What a script leaves unchecked, counted: every expectation a step's
//! `unchecked` names, in the order the case runs. Pure - the card, the run
//! report and the Playwright export all read this one list.

use super::{CaseScript, Unchecked};

/// One expectation the script leaves unchecked.
#[derive(Debug, Clone, PartialEq)]
pub struct UncheckedExpectation {
    pub step: i32,
    pub item: String,
    pub why: String,
    pub stand_in: Option<String>,
}

/// The item of a step whose `unchecked` is one sentence: the step checks
/// nothing, so its whole expected result is the one expectation.
pub const WHOLE_STEP_ITEM: &str = "the step's expected result";

/// The most entries one step's `unchecked` list may hold.
pub const MAX_ITEMS: usize = 20;

fn collapsed(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Every expectation the script leaves unchecked, case steps in run order,
/// never the clean-up: a sentence is one (item `WHOLE_STEP_ITEM`), a list is
/// one per entry. Item and why with their whitespace collapsed; a blank
/// sentence or entry is left out.
pub fn expectations(script: &CaseScript) -> Vec<UncheckedExpectation> {
    let mut out = Vec::new();
    for step in script.case_steps() {
        let step_number = step.step_number;
        match &step.unchecked {
            None => {}
            Some(Unchecked::Step(sentence)) => {
                let why = collapsed(sentence);
                if !why.is_empty() {
                    out.push(UncheckedExpectation { step: step_number, item: WHOLE_STEP_ITEM.to_string(), why, stand_in: None });
                }
            }
            Some(Unchecked::Items(items)) => {
                for entry in items {
                    let item = collapsed(&entry.item);
                    if item.is_empty() {
                        continue;
                    }
                    let stand_in = entry.stand_in.as_deref().map(collapsed).filter(|s| !s.is_empty());
                    out.push(UncheckedExpectation { step: step_number, item, why: collapsed(&entry.why), stand_in });
                }
            }
        }
    }
    out
}

/// "1 expectation unchecked" / "N expectations unchecked".
pub fn count_words(n: usize) -> String {
    if n == 1 {
        "1 expectation unchecked".to_string()
    } else {
        format!("{n} expectations unchecked")
    }
}

/// "// TODO unchecked: no lock icon - why", plus " (stand-in: expect_hidden)"
/// when it has one: the line Export to Playwright writes under the step.
pub fn todo_line(e: &UncheckedExpectation) -> String {
    let mut s = format!("// TODO unchecked: {} - {}", e.item, e.why);
    if let Some(kind) = &e.stand_in {
        s.push_str(&format!(" (stand-in: {kind})"));
    }
    s
}

/// "step 3: no lock icon - why", plus " (stand-in: expect_hidden)" when it has one.
pub fn line(e: &UncheckedExpectation) -> String {
    let mut s = format!("step {}: {} - {}", e.step, e.item, e.why);
    if let Some(kind) = &e.stand_in {
        s.push_str(&format!(" (stand-in: {kind})"));
    }
    s
}
