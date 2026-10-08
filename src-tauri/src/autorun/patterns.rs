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

use super::api_checks as api;
use super::components::{ran_actions, ComponentFile, Ran};
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
    /// A chain had to pass through a frame Auto Run cannot enter: one from
    /// another site, a sandboxed one, or one with no page yet
    /// (`locator::frame_unreachable`). About the application, so it can
    /// carry quirks.
    FrameUnreachable,
    /// A request the page made, or the answer to it, was not what the
    /// case expected (`expect_response`).
    Api,
    /// The page tried to send a save while a no-save script ran, and the
    /// browser stopped it (`browser::save_guard`).
    SaveBlocked,
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
            ErrorClass::FrameUnreachable => "frame",
            ErrorClass::Api => "api",
            ErrorClass::SaveBlocked => "no-save",
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
            ErrorClass::FrameUnreachable => "a frame Auto Run cannot reach".to_string(),
            ErrorClass::Api => "the server was asked or answered differently".to_string(),
            ErrorClass::SaveBlocked => "the page tried to save, and the script must not".to_string(),
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
    // The page's own errors, said after the step's last words.
    let detail = match detail.find(crate::browser::page_errors::NOTE) {
        Some(i) => &detail[..i],
        None => detail,
    };
    // A dialog nobody expected, said after the sentence it was read during
    // (` (a confirm dialog was accepted: "...")`): the page's words, which
    // could read as anything.
    let detail = match detail.find(crate::browser::dialogs::WAS_ACCEPTED).and_then(|w| detail[..w].rfind(" (a")) {
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
    // The guard's own sentence, before anything that reads an ending: it
    // ends with a path, which could read as anything.
    if crate::browser::save_guard::is_blocked(whole) {
        return ErrorClass::SaveBlocked;
    }
    // Before anything that reads a sentence's ending: an API check's
    // failure can end with an excerpt of whatever the server answered.
    if is_api_check(whole) {
        return ErrorClass::Api;
    }
    // The frame's own sentence can stand alone or follow a chain's target,
    // so it is found anywhere in the sentence, before any reading of its
    // start or end.
    if whole.contains(crate::browser::locator::FRAME_UNREACHABLE) {
        return ErrorClass::FrameUnreachable;
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
    // An `expect_dialog`'s own sentences: no dialog is nothing found; the
    // wrong words are text that did not match.
    if whole.starts_with(crate::browser::dialogs::NO_DIALOG) {
        return ErrorClass::NotFound;
    }
    if whole.starts_with("the dialog said \"") {
        return ErrorClass::TextMismatch;
    }
    // The table checks' own sentences (`browser::table`).
    if whole.starts_with("the table has no column \"") {
        return ErrorClass::NotFound;
    }
    if whole.starts_with("no row has ")
        || whole.starts_with("a row has ")
        || whole.contains(" order - row ")
    {
        return ErrorClass::TextMismatch;
    }
    if whole.starts_with("the table has ") && whole.contains(" rows, not ") {
        return ErrorClass::CountMismatch;
    }
    if whole.starts_with(crate::browser::drag::DID_NOT_FINISH) {
        return ErrorClass::TimedOut;
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

/// An `expect_response` or `api_request` failure (`autorun::api_checks`):
/// one of its own sentences, read without the body excerpt that may follow
/// it.
fn is_api_check(whole: &str) -> bool {
    if whole.starts_with(api::NO_REQUEST)
        || whole.starts_with(api::RESPONSE_TO)
        || whole.starts_with(api::BODY_GONE)
        || whole.starts_with(api::BODY_UNREADABLE)
    {
        return true;
    }
    // `<METHOD> /<path> <what happened>`.
    let sentence = whole.split(api::BODY_BEGAN).next().unwrap_or(whole);
    let Some((method, rest)) = sentence.split_once(' ') else {
        return false;
    };
    !method.is_empty()
        && method.chars().all(|c| c.is_ascii_uppercase())
        && rest.starts_with('/')
        && (rest.contains(api::NOT_FINISHED)
            || rest.ends_with(api::CANCELLED)
            || rest.contains(api::NET_FAILED)
            || rest.contains(api::ANSWERED)
            || rest.contains(api::REDIRECTED))
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
        | Action::ExpectFocused { selector, .. }
        | Action::Upload { selector, .. }
        | Action::WhenVisible { selector, .. } => Some(selector.describe()),
        // Each acts on the page as a whole; a key is pressed on whatever
        // has the focus.
        Action::UseComponent { component, .. } => Some(format!("the component \"{}\"", component.trim())),
        Action::Reload => Some("the page".to_string()),
        Action::ExpireSession => Some("the session".to_string()),
        Action::ReturnToArea { .. } => Some(match action.area_named() {
            Some(area) => format!("the {area} area"),
            None => "the case's area".to_string(),
        }),
        Action::PressKey { key, .. } => Some(format!("the {} key", key.trim())),
        Action::ExpectDialog { .. } => Some("a dialog".to_string()),
        Action::ExpectRow { table, .. }
        | Action::ExpectNoRow { table, .. }
        | Action::ExpectSorted { table, .. }
        | Action::ExpectRowCount { table, .. } => Some(table.describe()),
        // What is picked up: a drag that cannot start is about it.
        Action::Drag { from, .. } => Some(from.describe()),
        Action::ExpectDownload { name, .. } => Some(format!("the download \"{}\"", name.trim())),
        // A tab by the name the script gave it.
        Action::ExpectTab { name, .. }
        | Action::OpenTab { name, .. }
        | Action::SwitchTab { name }
        | Action::CloseTab { name }
        | Action::ExpectTabClosed { name, .. } => Some(format!("the \"{name}\" tab")),
        Action::Navigate { url } => {
            let url = url.trim();
            let end = url.find(['?', '#']).unwrap_or(url.len());
            Some(url[..end].to_string())
        }
        Action::CheckText { .. } => Some("the page text".to_string()),
        Action::CheckUrl { .. } => Some("the page address".to_string()),
        Action::SignIn { .. } => None,
        // An address fragment or a path: its query string can carry a
        // token, so it is dropped here as a navigation's is.
        Action::ExpectResponse { url_contains: address, .. } | Action::ApiRequest { path: address, .. } => {
            let address = address.trim();
            let end = address.find(['?', '#']).unwrap_or(address.len());
            Some(address[..end].to_string())
        }
    }
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
    step_failures_with(step, script, &ComponentFile::default())
}

/// [`step_failures`], with the project's components: an action a
/// component ran is read as it expands from them, so it is classed by its
/// own kind and target. One that no longer expands is classed by its
/// component.
pub fn step_failures_with(step: &StepRecord, script: Option<&CaseScript>, components: &ComponentFile) -> Vec<FailurePoint> {
    if step.step_number == SIGN_IN_STEP || step.step_number == MODULE_STEP {
        return Vec::new();
    }
    let ran = match script.and_then(|s| s.steps.iter().find(|st| st.step_number == step.step_number)) {
        Some(st) => ran_actions(&st.actions, &step.outcomes, components, &step.components),
        None => Vec::new(),
    };
    step.outcomes
        .iter()
        .enumerate()
        .filter(|(_, o)| !o.ok && !o.detail.starts_with("not run:"))
        .map(|(i, o)| {
            let (kind, target) = match ran.get(i) {
                Some(Ran { action: Some(a), .. }) => (Some(action_kind(a)), action_target(a)),
                Some(Ran { action: None, component: Some(c) }) => {
                    (Some("use_component".to_string()), Some(format!("the component \"{}\"", c.trim())))
                }
                _ => (None, None),
            };
            FailurePoint {
                step_number: step.step_number,
                action: i + 1,
                kind,
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

/// What a covering element is grouped by: its tag, id and FIRST class
/// only. An overlay's later classes come and go as it animates
/// (`div.modal.fade` and `div.modal.fade.show` are one overlay), so the
/// whole list would split one fact into several patterns.
pub fn overlay_key(covering: &str) -> String {
    let mut parts = covering.trim().splitn(3, '.');
    let head = parts.next().unwrap_or_default();
    match parts.next() {
        Some(first) if !first.is_empty() => format!("{head}.{first}"),
        _ => head.to_string(),
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
    find_patterns_with(run, scripts, &ComponentFile::default())
}

/// [`find_patterns`], with the project's components (`step_failures_with`).
pub fn find_patterns_with(run: &LocalRun, scripts: &[CaseScript], components: &ComponentFile) -> Vec<Pattern> {
    #[derive(PartialEq, Eq, PartialOrd, Ord)]
    enum Key {
        Covering(String),
        Target { kind: String, target: String, class: ErrorClass },
    }
    let mut groups: BTreeMap<Key, (Vec<String>, Vec<Hit>, ErrorClass)> = BTreeMap::new();
    for case in run.cases.iter().filter(|c| is_failed(c) && stop_reason(c).is_none()) {
        let script = scripts.iter().find(|s| s.case_id == case.case_id);
        for step in &case.steps {
            for f in step_failures_with(step, script, components) {
                if !f.class.about_the_app() {
                    continue;
                }
                let what = match (&f.kind, &f.target) {
                    (Some(k), Some(t)) => Some(format!("{k} on {t}")),
                    _ => None,
                };
                let key = match (&f.class, &what) {
                    (ErrorClass::CoveredBy(by), _) => Key::Covering(overlay_key(by)),
                    (_, Some(_)) => Key::Target {
                        kind: f.kind.clone().unwrap_or_default(),
                        target: f.target.clone().unwrap_or_default(),
                        class: f.class.clone(),
                    },
                    // No script action to name: nothing to group it by.
                    (_, None) => continue,
                };
                let class = match &f.class {
                    ErrorClass::CoveredBy(by) => ErrorClass::CoveredBy(overlay_key(by)),
                    other => other.clone(),
                };
                let entry = groups.entry(key).or_insert_with(|| (Vec::new(), Vec::new(), class));
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
pub const PATTERN_ADVICE: &str = "If this is how the application behaves, put the same quirk on the edit of EVERY case you repair for it, so later runs can tell whether it helped - or record it once with record_autorun_quirk and `cases` naming those cases and steps. Either way the next script avoids it.";

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
