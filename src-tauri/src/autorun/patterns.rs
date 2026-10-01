//! The same failure, seen in more than one case.
//!
//! One case failing to find a button is a script to repair. Three cases
//! failing to find the same button - or three different targets all
//! covered by the same overlay - is usually the APPLICATION behaving a
//! certain way, and worth one quirk rather than three separate repairs
//! that each rediscover it. This module spots those.
//!
//! Pure: it reads a run (and the scripts that ran, for each action's
//! kind and target) and returns text. It never reads a typed value - a
//! `fill`'s value is not part of any signature or line it writes - and
//! never the outcome's own sentence either, past the error class it is
//! sorted into: what it prints is built from the script's target and a
//! fixed label, so nothing a page echoed back (a value, a file name) can
//! travel into a pattern.
//!
//! The classes are read against the very constants the browser driver
//! writes its failures with (`browser::input`, `browser::expect`,
//! `browser::actions`), never against a second copy of the wording.

use super::failures::{is_failed, stop_reason};
use super::replay::{MODULE_STEP, SIGN_IN_STEP};
use super::{CaseScript, LocalRun, StepRecord};
use crate::browser::actions::{self as act, Action};
use crate::browser::{expect as exp, input as inp};
use std::collections::{BTreeMap, BTreeSet};

/// What kind of failure an action's outcome reports, independent of the
/// target it was aimed at and of any value it quotes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ErrorClass {
    NotFound,
    MatchedMany,
    /// What was in the way, as the page described it (`div.modal`).
    CoveredBy(String),
    NotVisible,
    Offscreen,
    Disabled,
    NotEditable,
    StillMoving,
    TimedOut,
    Navigation,
    TextMismatch,
    CountMismatch,
    AttributeMismatch,
    StillVisible,
    PageTextMissing,
    UrlMismatch,
    PageRefused,
    LostFocus,
    NoOption,
    NoFileChooser,
    /// The script itself was refused before it reached the page.
    CannotRun,
    /// The browser connection, not the page. Never a pattern about the
    /// application.
    Browser,
    Other,
}

impl ErrorClass {
    /// A stable short name, saved with a quirk's source so a later run's
    /// failure can be compared with the one that led to the repair.
    pub fn key(&self) -> &'static str {
        match self {
            ErrorClass::NotFound => "not_found",
            ErrorClass::MatchedMany => "matched_many",
            ErrorClass::CoveredBy(_) => "covered",
            ErrorClass::NotVisible => "not_visible",
            ErrorClass::Offscreen => "offscreen",
            ErrorClass::Disabled => "disabled",
            ErrorClass::NotEditable => "not_editable",
            ErrorClass::StillMoving => "still_moving",
            ErrorClass::TimedOut => "timed_out",
            ErrorClass::Navigation => "navigation",
            ErrorClass::TextMismatch => "text_mismatch",
            ErrorClass::CountMismatch => "count_mismatch",
            ErrorClass::AttributeMismatch => "attribute_mismatch",
            ErrorClass::StillVisible => "still_visible",
            ErrorClass::PageTextMissing => "page_text_missing",
            ErrorClass::UrlMismatch => "url_mismatch",
            ErrorClass::PageRefused => "page_refused",
            ErrorClass::LostFocus => "lost_focus",
            ErrorClass::NoOption => "no_option",
            ErrorClass::NoFileChooser => "no_file_chooser",
            ErrorClass::CannotRun => "cannot_run",
            ErrorClass::Browser => "browser",
            ErrorClass::Other => "other",
        }
    }

    /// The class in words, for a pattern line.
    pub fn label(&self) -> String {
        match self {
            ErrorClass::NotFound => "not found".to_string(),
            ErrorClass::MatchedMany => "matched more than one element".to_string(),
            ErrorClass::CoveredBy(what) => format!("covered by {what}"),
            ErrorClass::NotVisible => "there but not visible".to_string(),
            ErrorClass::Offscreen => "outside the visible part of the page".to_string(),
            ErrorClass::Disabled => "disabled".to_string(),
            ErrorClass::NotEditable => "cannot be typed into".to_string(),
            ErrorClass::StillMoving => "still moving".to_string(),
            ErrorClass::TimedOut => "timed out while being checked".to_string(),
            ErrorClass::Navigation => "the page did not load".to_string(),
            ErrorClass::TextMismatch => "showed different text".to_string(),
            ErrorClass::CountMismatch => "a different number of elements".to_string(),
            ErrorClass::AttributeMismatch => "a different attribute".to_string(),
            ErrorClass::StillVisible => "still visible".to_string(),
            ErrorClass::PageTextMissing => "the page lacked the words".to_string(),
            ErrorClass::UrlMismatch => "a different address".to_string(),
            ErrorClass::PageRefused => "the page refused the call".to_string(),
            ErrorClass::LostFocus => "lost the focus before typing".to_string(),
            ErrorClass::NoOption => "the list had no such option".to_string(),
            ErrorClass::NoFileChooser => "no file chooser opened".to_string(),
            ErrorClass::CannotRun => "the action could not run".to_string(),
            ErrorClass::Browser => "the browser stopped answering".to_string(),
            ErrorClass::Other => "failed another way".to_string(),
        }
    }

    /// Classes that say nothing about how the APPLICATION behaves: the
    /// browser connection dropping, or a script action refused before it
    /// ever reached the page.
    fn about_the_app(&self) -> bool {
        !matches!(self, ErrorClass::Browser | ErrorClass::CannotRun)
    }
}

/// The part of a failure sentence after its target: `waited 5000ms:
/// button "Save" is covered by div.modal` -> `is covered by div.modal`.
/// Without a target (no script on disk) the sentence comes back with the
/// `waited ...: ` prefix dropped and the target still in front.
fn tail<'a>(detail: &'a str, target: Option<&str>) -> &'a str {
    let detail = match detail.find(act::DIALOG_NOTE) {
        Some(i) => &detail[..i],
        None => detail,
    };
    let Some(rest) = detail.strip_prefix("waited ") else {
        return detail;
    };
    let Some((_, after)) = rest.split_once("ms: ") else {
        return detail;
    };
    match target.and_then(|t| after.strip_prefix(t)) {
        Some(t) => t.trim_start(),
        None => after,
    }
}

/// The class of one failed action's sentence. `target` is the action's
/// own `describe()`, when the script is on disk; without it the
/// sentence's ending is read instead of its start.
pub fn classify(detail: &str, target: Option<&str>) -> ErrorClass {
    let whole = detail.trim();
    if whole.starts_with(act::BROWSER_SILENT) || whole.starts_with("the browser closed") {
        return ErrorClass::Browser;
    }
    if whole.starts_with(act::CANNOT_RUN) {
        return ErrorClass::CannotRun;
    }
    let t = tail(whole, target);
    if let Some(class) = by_start(t) {
        return class;
    }
    // Sentences that carry no target prefix of their own.
    if whole.starts_with("waited ") && whole.contains(act::NEVER_SAW) {
        return ErrorClass::NotFound;
    }
    if whole.contains(act::DID_NOT_FINISH_LOADING)
        || whole.contains(act::WOULD_NOT_LOAD)
        || whole.contains(act::ALLOWED_ORIGINS)
        || whole.starts_with("navigate needs ")
    {
        return ErrorClass::Navigation;
    }
    if whole.starts_with(act::PAGE_LACKS) {
        return ErrorClass::PageTextMissing;
    }
    if whole.starts_with(act::URL_IS) {
        return ErrorClass::UrlMismatch;
    }
    if whole.starts_with(inp::PAGE_REFUSED) {
        return ErrorClass::PageRefused;
    }
    if whole.starts_with(inp::LOST_FOCUS) {
        return ErrorClass::LostFocus;
    }
    if whole.starts_with(inp::NO_OPTION) || (whole.starts_with(inp::OPTION) && whole.ends_with(inp::DISABLED)) {
        return ErrorClass::NoOption;
    }
    if whole.contains(act::FILE_CHOOSER) {
        return ErrorClass::NoFileChooser;
    }
    // An upload's `<target> is disabled`, and any target-prefixed
    // sentence whose target could not be stripped (no script on disk).
    by_end(t).unwrap_or(ErrorClass::Other)
}

/// The reason words that a target-prefixed sentence starts with, once
/// the target is gone.
fn by_start(t: &str) -> Option<ErrorClass> {
    if let Some(rest) = t.strip_prefix(inp::MOVED_BEFORE_CLICK) {
        return Some(by_start(rest).unwrap_or(ErrorClass::Other));
    }
    if let Some(what) = t.strip_prefix(inp::COVERED_BY) {
        return Some(ErrorClass::CoveredBy(what.trim().to_string()));
    }
    if t == inp::NOT_FOUND || t == exp::NOT_ON_PAGE {
        return Some(ErrorClass::NotFound);
    }
    if t.starts_with("matched ") && t.ends_with(inp::MATCHED_MANY_TAIL) {
        return Some(ErrorClass::MatchedMany);
    }
    let exact = [
        (inp::NOT_VISIBLE, ErrorClass::NotVisible),
        (exp::HIDDEN, ErrorClass::NotVisible),
        (inp::OFFSCREEN, ErrorClass::Offscreen),
        (inp::DISABLED, ErrorClass::Disabled),
        (inp::NOT_EDITABLE, ErrorClass::NotEditable),
        (inp::STILL_MOVING, ErrorClass::StillMoving),
        (inp::STILL_LOOKING, ErrorClass::TimedOut),
        (exp::STILL_VISIBLE, ErrorClass::StillVisible),
    ];
    if let Some((_, class)) = exact.into_iter().find(|(words, _)| t == *words) {
        return Some(class);
    }
    if t.starts_with(exp::EXPECTED_TEXT) || t.starts_with(exp::EXPECTED_TO_CONTAIN) {
        return Some(ErrorClass::TextMismatch);
    }
    if t.starts_with("expected ") && t.contains(exp::COUNTED) {
        return Some(ErrorClass::CountMismatch);
    }
    if (t.starts_with(exp::HAS_NO) && t.ends_with(exp::ATTRIBUTE_TAIL)) || (t.starts_with("expected ") && t.contains("=")) {
        return Some(ErrorClass::AttributeMismatch);
    }
    None
}

/// The same reasons, found at the END of a sentence whose target is
/// still in front of them.
fn by_end(t: &str) -> Option<ErrorClass> {
    if let Some(i) = t.rfind(&format!(" {}", inp::MOVED_BEFORE_CLICK)) {
        return by_start(&t[i + 1..]);
    }
    if let Some(i) = t.rfind(&format!(" {}", inp::COVERED_BY)) {
        return Some(ErrorClass::CoveredBy(t[i + 1 + inp::COVERED_BY.len()..].trim().to_string()));
    }
    let endings = [
        (inp::NOT_FOUND, ErrorClass::NotFound),
        (exp::NOT_ON_PAGE, ErrorClass::NotFound),
        (inp::MATCHED_MANY_TAIL, ErrorClass::MatchedMany),
        (inp::NOT_VISIBLE, ErrorClass::NotVisible),
        (exp::HIDDEN, ErrorClass::NotVisible),
        (inp::OFFSCREEN, ErrorClass::Offscreen),
        (inp::DISABLED, ErrorClass::Disabled),
        (inp::NOT_EDITABLE, ErrorClass::NotEditable),
        (inp::STILL_MOVING, ErrorClass::StillMoving),
        (inp::STILL_LOOKING, ErrorClass::TimedOut),
        (exp::STILL_VISIBLE, ErrorClass::StillVisible),
        (exp::ATTRIBUTE_TAIL, ErrorClass::AttributeMismatch),
    ];
    if let Some((_, class)) = endings.into_iter().find(|(words, _)| t.ends_with(&format!(" {words}")) || t.ends_with(words)) {
        return Some(class);
    }
    for (words, class) in [
        (exp::EXPECTED_TEXT, ErrorClass::TextMismatch),
        (exp::EXPECTED_TO_CONTAIN, ErrorClass::TextMismatch),
    ] {
        if t.contains(&format!(" {words}")) {
            return Some(class);
        }
    }
    if t.contains(exp::COUNTED) && t.contains(" expected ") {
        return Some(ErrorClass::CountMismatch);
    }
    if t.contains(" expected ") && t.contains('=') && t.contains(" but saw ") {
        return Some(ErrorClass::AttributeMismatch);
    }
    None
}

/// An action's kind as a script spells it (`click`, `expect_visible`).
pub fn action_kind(action: &Action) -> String {
    serde_json::to_value(action)
        .ok()
        .and_then(|v| v.get("kind").and_then(|k| k.as_str()).map(str::to_string))
        .unwrap_or_else(|| "action".to_string())
}

/// What an action is aimed at, in words. A navigation's address loses its
/// query and fragment - a link can carry a token there, and the page and
/// path are what a pattern is about.
pub fn action_target(action: &Action) -> Option<String> {
    match action {
        Action::Click { selector }
        | Action::Fill { selector, .. }
        | Action::WaitFor { selector, .. }
        | Action::ExpectVisible { selector, .. }
        | Action::ExpectHidden { selector, .. }
        | Action::ExpectText { selector, .. }
        | Action::ExpectContainsText { selector, .. }
        | Action::ExpectCount { selector, .. }
        | Action::ExpectAttribute { selector, .. }
        | Action::Upload { selector, .. } => Some(selector.describe()),
        Action::Navigate { url } => {
            let url = url.trim();
            let end = url.find(['?', '#']).unwrap_or(url.len());
            Some(url[..end].to_string())
        }
        Action::CheckText { .. } => Some("the page text".to_string()),
        Action::CheckUrl { .. } => Some("the page address".to_string()),
        Action::SignIn { .. } => None,
    }
}

fn scripted_action(script: Option<&CaseScript>, step_number: i32, index: usize) -> Option<&Action> {
    script
        .and_then(|s| s.steps.iter().find(|st| st.step_number == step_number))
        .and_then(|st| st.actions.get(index))
}

/// One failed action, as a pattern sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct FailurePoint {
    pub step_number: i32,
    /// 1-based, as the failures report numbers them.
    pub action: usize,
    /// `None` when no script on disk has an action here.
    pub kind: Option<String>,
    pub target: Option<String>,
    pub class: ErrorClass,
}

/// Every failed action in one ordinary step (never sign-in or the module
/// screen), skipped ones left out: a `not run:` action did not fail, an
/// earlier one in the same step did.
pub fn step_failures(step: &StepRecord, script: Option<&CaseScript>) -> Vec<FailurePoint> {
    if step.step_number == SIGN_IN_STEP || step.step_number == MODULE_STEP {
        return Vec::new();
    }
    step.outcomes
        .iter()
        .enumerate()
        .filter(|(_, o)| !o.ok && !o.detail.starts_with("not run:"))
        .map(|(i, o)| {
            let action = scripted_action(script, step.step_number, i);
            let target = action.and_then(action_target);
            FailurePoint {
                step_number: step.step_number,
                action: i + 1,
                kind: action.map(action_kind),
                class: classify(&o.detail, target.as_deref()),
                target,
            }
        })
        .collect()
}

/// The class of the first failure in a step, or `None` for a step with
/// no failed action (passed, or never run).
pub fn step_failure_class(step: &StepRecord, script: Option<&CaseScript>) -> Option<ErrorClass> {
    step_failures(step, script).into_iter().next().map(|f| f.class)
}

/// Where a pattern struck.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub case_id: i32,
    pub step_number: i32,
    pub action: usize,
}

/// One failure seen in at least two different cases.
#[derive(Debug, Clone, PartialEq)]
pub struct Pattern {
    pub class: ErrorClass,
    /// `click on button "Save"` - for a covering pattern, every target
    /// the overlay blocked, in order.
    pub targets: Vec<String>,
    pub hits: Vec<Hit>,
}

impl Pattern {
    pub fn cases(&self) -> usize {
        self.hits.iter().map(|h| h.case_id).collect::<BTreeSet<_>>().len()
    }
}

/// The repeated failures in one run's failed cases. A case the assistant
/// is told to leave alone (`stop_reason`) is left out here too.
///
/// Two groupings: the same action kind on the same target failing the
/// same way; and - for an element in the way - the same covering element,
/// whatever it covered, since one overlay that blocks many different
/// targets is one fact about the application. A covered failure is
/// grouped only the second way, so it is never reported twice.
pub fn find_patterns(run: &LocalRun, scripts: &[CaseScript]) -> Vec<Pattern> {
    #[derive(PartialEq, Eq, PartialOrd, Ord)]
    enum Key {
        Covering(String),
        Target { kind: String, target: String, class: ErrorClass },
    }
    let mut groups: BTreeMap<Key, (Vec<String>, Vec<Hit>, ErrorClass)> = BTreeMap::new();
    for case in run.cases.iter().filter(|c| is_failed(c) && stop_reason(c).is_none()) {
        let script = scripts.iter().find(|s| s.case_id == case.case_id);
        for step in &case.steps {
            for f in step_failures(step, script) {
                if !f.class.about_the_app() {
                    continue;
                }
                let what = match (&f.kind, &f.target) {
                    (Some(k), Some(t)) => Some(format!("{k} on {t}")),
                    _ => None,
                };
                let key = match (&f.class, &what) {
                    (ErrorClass::CoveredBy(by), _) => Key::Covering(by.clone()),
                    (_, Some(_)) => Key::Target {
                        kind: f.kind.clone().unwrap_or_default(),
                        target: f.target.clone().unwrap_or_default(),
                        class: f.class.clone(),
                    },
                    // No script action to name: nothing to group it by.
                    (_, None) => continue,
                };
                let entry = groups.entry(key).or_insert_with(|| (Vec::new(), Vec::new(), f.class.clone()));
                if let Some(w) = what {
                    if !entry.0.contains(&w) {
                        entry.0.push(w);
                    }
                }
                entry.1.push(Hit { case_id: case.case_id, step_number: f.step_number, action: f.action });
            }
        }
    }
    let mut out: Vec<Pattern> = groups
        .into_values()
        .map(|(targets, hits, class)| Pattern { class, targets, hits })
        .filter(|p| p.cases() >= 2)
        .collect();
    out.sort_by(|a, b| b.cases().cmp(&a.cases()));
    out
}

/// What an assistant is told to do with a pattern - one line, once.
pub const PATTERN_ADVICE: &str = "If this is how the application behaves, record it with record_autorun_quirk (or as the repair's quirk) so the next script avoids it.";

/// The `## Patterns across cases` section, or nothing when there is none.
pub fn patterns_section(patterns: &[Pattern]) -> String {
    if patterns.is_empty() {
        return String::new();
    }
    let mut out = String::from("## Patterns across cases\n\n");
    for p in patterns {
        let head = match &p.class {
            ErrorClass::CoveredBy(_) => p.class.label(),
            _ => format!("{} - {}", p.targets.join("; "), p.class.label()),
        };
        out.push_str(&format!("- {head} (in {} cases)\n", p.cases()));
        if matches!(p.class, ErrorClass::CoveredBy(_)) && !p.targets.is_empty() {
            out.push_str(&format!("  blocked: {}\n", p.targets.join("; ")));
        }
        let at: Vec<String> = p
            .hits
            .iter()
            .map(|h| format!("case {} step {} action {}", h.case_id, h.step_number, h.action))
            .collect();
        out.push_str(&format!("  where: {}\n", at.join(", ")));
    }
    out.push('\n');
    out.push_str(PATTERN_ADVICE);
    out.push('\n');
    out
}
