//! What a script may never carry, whatever an assistant asks for: page
//! script, a fixed wait, a retry around a step (Task 18), and a repair that
//! lets the page decide whether a failure counts (Task 19). The Playwright
//! pipeline forbids each in its own code; here no action does any of them,
//! and asking for one is refused with the reason, so the rule holds whatever
//! tools wrote the script.

pub const NO_PAGE_SCRIPT: &str = "a script never runs page script: it acts and checks through the page as a person does, and page script that makes a step pass hides what a person would see";
pub const NO_FIXED_WAIT: &str = "a script never waits a fixed time: wait for what the step needs with wait_for, or give the check a longer timeout_ms";
pub const NO_RETRY: &str = "a script never retries a step: a failure that comes and goes is an environment flake to report (2 green of 3 is a flake, not a fix), and a real bug must surface as a failure";

const PAGE_SCRIPT_KINDS: &[&str] = &[
    "evaluate",
    "eval",
    "evaluate_all",
    "evaluate_handle",
    "run_script",
    "execute_script",
    "script",
    "javascript",
    "js",
    "run_js",
    "run_code",
];
const FIXED_WAIT_KINDS: &[&str] = &[
    "wait",
    "sleep",
    "pause",
    "delay",
    "wait_for_timeout",
    "wait_timeout",
    "wait_ms",
    "timeout",
    "networkidle",
    "network_idle",
    "wait_for_network_idle",
    "wait_for_load_state",
];
const RETRY_KINDS: &[&str] = &["retry", "retries", "to_pass", "retry_until", "until_pass", "repeat_until", "try_again"];
const RETRY_KEYS: &[&str] = &["retry", "retries", "to_pass"];

/// The reason a `kind` is refused, or None for any other kind. Compared
/// trimmed, lowercased, with `-` and spaces read as `_`.
pub fn forbidden_kind(kind: &str) -> Option<&'static str> {
    let k = kind.trim().to_lowercase().replace(['-', ' '], "_");
    let k = k.as_str();
    if PAGE_SCRIPT_KINDS.contains(&k) {
        Some(NO_PAGE_SCRIPT)
    } else if FIXED_WAIT_KINDS.contains(&k) {
        Some(NO_FIXED_WAIT)
    } else if RETRY_KINDS.contains(&k) {
        Some(NO_RETRY)
    } else {
        None
    }
}

/// The first forbidden kind or key in `v`, depth first through every array
/// and object (a `then` included): an object whose "kind" is a string
/// `forbidden_kind` refuses, else an object holding a non-null retry key.
pub fn forbidden_in(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::Array(items) => items.iter().find_map(forbidden_in),
        serde_json::Value::Object(map) => {
            if let Some(kind) = map.get("kind").and_then(|k| k.as_str()) {
                if let Some(reason) = forbidden_kind(kind) {
                    return Some(format!("\"{kind}\" is not an action and never will be: {reason}"));
                }
            }
            for key in RETRY_KEYS {
                if map.get(*key).is_some_and(|x| !x.is_null()) {
                    return Some(format!("\"{key}\" is not part of any action and never will be: {NO_RETRY}"));
                }
            }
            map.values().find_map(forbidden_in)
        }
        _ => None,
    }
}

/// What a repair from `old` to `new` adds that would let a failure pass unseen. `edit` is the case's
/// declared edit. Empty when it adds nothing of the kind. Each line once, in this order.
pub fn check_repair_absorbs(
    old: &crate::autorun::CaseScript,
    new: &crate::autorun::CaseScript,
    edit: Option<&crate::autorun::edits::Edit>,
) -> Vec<String> {
    use crate::autorun::quirks::QuirkClass;
    use crate::autorun::PageErrors;
    use crate::browser::actions::Action;
    let quirk_given = edit.is_some_and(|e| e.quirk.as_deref().is_some_and(|q| !q.trim().is_empty()));
    let class = edit.and_then(|e| e.quirk_class);
    let is_guard = |a: &&Action| matches!(a, Action::WhenVisible { .. } | Action::RepeatWhileVisible { .. });
    let mut out: Vec<String> = Vec::new();

    // 1. A new guard, per step (the clean-up is free).
    for step in new.steps.iter().filter(|s| !s.cleanup) {
        let now = step.actions.iter().filter(is_guard).count();
        let before = old
            .steps
            .iter()
            .find(|s| s.step_number == step.step_number)
            .map_or(0, |s| s.actions.iter().filter(is_guard).count());
        if now <= before {
            continue;
        }
        let n = step.step_number;
        let kind = step.actions.iter().filter(is_guard).nth(before).map(|a| a.kind()).unwrap_or_default();
        if class == Some(QuirkClass::ProductBug) {
            out.push(format!("step {n}: the repair adds a {kind} around a product bug - a real bug must surface as a failure: leave the step failing and mark it with mark_autorun_suspected_defect"));
        } else if !quirk_given || class.is_none() {
            out.push(format!("step {n}: the repair adds a {kind} - a guard runs or not as the page decides, which is how a failure gets absorbed: give the case's edit a quirk saying what the page does, with quirk_class \"app quirk\" or \"environment flake\""));
        }
    }

    // 2. A new ignored page error.
    for phrase in &new.ignore_page_errors {
        if old.ignore_page_errors.iter().any(|p| p.to_lowercase() == phrase.to_lowercase()) {
            continue;
        }
        if class == Some(QuirkClass::ProductBug) {
            out.push(format!("the repair ignores \"{phrase}\", a product bug - a real bug must surface as a failure: leave it failing and tell the person"));
        } else if !quirk_given || class.is_none() {
            out.push(format!("the repair adds \"{phrase}\" to ignore_page_errors - a page error that fails the case is the application's: give the case's edit a quirk saying what the error is, with quirk_class \"app quirk\" or \"environment flake\", or leave it failing and tell the person"));
        }
    }

    // 3. Looser page errors.
    if matches!(
        (old.page_errors, new.page_errors),
        (Some(PageErrors::Fail), Some(PageErrors::Flag) | None) | (Some(PageErrors::Flag), None)
    ) {
        out.push("the repair loosens page_errors - what fails the case is a person's choice, in the script editor".to_string());
    }

    // 4. A dialog that no longer fails.
    if old.fail_on_unexpected_dialog && !new.fail_on_unexpected_dialog {
        out.push("the repair stops an unexpected dialog failing the case - that is a person's choice, in the script editor".to_string());
    }
    out
}
