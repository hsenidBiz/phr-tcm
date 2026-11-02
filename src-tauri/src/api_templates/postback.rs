//! WebForms postback steps: the pieces of the wire contract with the page
//! that need no browser. This file holds the HTML capture syntax, the
//! selector-or-text rule for `expect.html` entries and the key a carried
//! page is filed under.

use super::exec::{self, substitute_str};
use super::runner::SENT_ELSEWHERE;
use super::{is_safe_relative_path, RowCheck, Step};
use crate::ado::endpoints::percent_encode_segment;
use crate::autorun::nav::path_of;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};

/// What an `html:` capture reads: the first element a selector matches, and
/// one of its attributes - or its text when there is none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HtmlCapture {
    pub selector: String,
    pub attr: Option<String>,
}

/// Reads an `html:<selector>[@attribute]` capture path. `None` when `path`
/// is not an `html:` capture at all (a JSON path is the caller's to parse).
/// The attribute is what follows the LAST `@`, when that is a valid
/// attribute name and the `@` is not inside `[...]` - `a[href@x]` is one
/// selector with no attribute.
pub fn parse_html_capture(path: &str) -> Option<Result<HtmlCapture, String>> {
    let rest = path.strip_prefix("html:")?;
    let mut selector = rest;
    let mut attr = None;
    if let Some(at) = rest.rfind('@') {
        let before = &rest[..at];
        let open = before.matches('[').count() > before.matches(']').count();
        let name = &rest[at + 1..];
        if !open && is_attribute_name(name) {
            selector = before;
            attr = Some(name.to_string());
        }
    }
    if selector.trim().is_empty() {
        return Some(Err("capture path 'html:' names no selector".to_string()));
    }
    Some(Ok(HtmlCapture { selector: selector.trim().to_string(), attr }))
}

/// `^[A-Za-z_:][-A-Za-z0-9_:.]*$`
fn is_attribute_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c == ':' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.'))
}

/// Whether an `expect.html` entry is a selector rather than text on the
/// page.
pub fn is_selector(s: &str) -> bool {
    s.starts_with(['#', '.', '[']) || s.contains('>')
}

/// The key a page is carried under: its path alone (no host, query or
/// fragment), lower case, without a trailing `/`; `/` when that leaves
/// nothing.
pub fn page_key(url: &str) -> String {
    let p = path_of(url).to_ascii_lowercase();
    let trimmed = p.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        trimmed.to_string()
    }
}

/// What a page-state field reads as wherever a sent body is shown.
pub const PAGE_STATE_SHOWN: &str = "(page state)";
/// What a field the page filled in (and the template did not set) reads as.
pub const FROM_THE_PAGE: &str = "(from the page)";
/// The most page a run carries from one step to the next.
pub const CARRY_LIMIT_BYTES: u64 = 8 * 1024 * 1024;
/// The carry key of a one-off call: it has no run of its own.
pub const ONE_OFF_CARRY: &str = "one-off";

pub const PAGE_LOST: &str = "the page this step carries on from was lost - run the template again";
pub const TOO_LARGE: &str = "the page is too large to carry";
/// Appended to a timeout or an empty-400 sentence: a postback is never
/// retried, because the first one may have been read.
pub const MAY_HAVE_SAVED: &str = " - a postback may already have saved, so it was not sent again";
pub const PARTIAL_UPDATE: &str =
    "the page answered with a partial (UpdatePanel) update, which postback steps do not handle";
pub const POSTBACK_NOT_SAVED: &str = "not saved as a template: a postback tried here may carry on from the call before it - prove a template with all its postback steps";

pub fn not_webforms(page: &str) -> String {
    format!("{page} is not a WebForms page - use a normal step.")
}

pub fn no_button(button: &str) -> String {
    format!("the page has no control named {button} - nothing was sent")
}

pub fn many_buttons(button: &str, n: usize) -> String {
    format!("{n} controls on the page match {button} - name it in full - nothing was sent")
}

pub fn page_did_not_open(page: &str, status: u16) -> String {
    format!("the page {page} did not open (status {status}) - nothing was sent")
}

pub fn error_page(title: &str) -> String {
    format!("the application answered with an error page: {title}")
}

/// The application's "invalid postback or callback argument" refusal; the
/// field is named when the page's answer says which one.
pub fn invalid_postback(field: Option<&str>) -> String {
    let base = "the application refused a field or button value the page did not offer";
    match field {
        Some(f) => format!("{base} ({f})"),
        None => base.to_string(),
    }
}

/// Everything the page needs to post one step. Placeholders are filled;
/// nothing here is page state - the page supplies that itself.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostbackRequest {
    pub url: String,
    pub key: String,
    pub button: String,
    pub argument: String,
    pub fields: BTreeMap<String, String>,
    pub files: Vec<exec::FormFile>,
    pub shows: Vec<String>,
    pub hides: Vec<String>,
    pub rows: Option<RowCheck>,
    pub captures: BTreeMap<String, HtmlCapture>,
}

/// `s` with each `{{name}}` replaced by its value percent-encoded as one
/// query component; an unknown placeholder is left as written.
fn substitute_encoded(s: &str, vars: &BTreeMap<String, Value>) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else { break };
        let name = after[..end].trim();
        out.push_str(&rest[..start]);
        match (!name.is_empty()).then(|| vars.get(name)).flatten() {
            Some(v) => out.push_str(&percent_encode_segment(&exec::var_text(v))),
            None => out.push_str(&rest[start..start + 2 + end + 2]),
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

/// Builds the request a postback step describes once every placeholder is
/// filled: the page address (its path part checked like any step's path, its
/// query part percent-encoded), the button and argument, the form values,
/// the checks and the captures, plus the step's files.
pub fn build_postback(
    step: &Step,
    vars: &BTreeMap<String, Value>,
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<PostbackRequest, String> {
    let Some(pb) = &step.postback else {
        return Err(format!("step '{}' is not a postback step", step.name));
    };
    let (path_part, query_part) = match pb.page.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (pb.page.as_str(), None),
    };
    let path = exec::substitute_path(path_part, &step.name, vars)?;
    if !is_safe_relative_path(&path) {
        return Err(format!(
            "step '{}' page is not a safe relative path on this origin once its placeholders are filled in: '{}'",
            step.name, path
        ));
    }
    let url = match query_part {
        Some(q) => format!("{path}?{}", substitute_encoded(q, vars)),
        None => path,
    };

    // `check` refuses a `__` key as written; a placeholder must not get one
    // past it at run time.
    let mut fields = BTreeMap::new();
    for (k, v) in step.form.iter().flatten() {
        let key = substitute_str(k, vars);
        if key.starts_with("__") {
            return Err(format!(
                "step '{}' form field '{key}' is page state, which a template never sets",
                step.name
            ));
        }
        fields.insert(key, substitute_str(v, vars));
    }
    let html = step.expect.html.as_ref();
    let fill = |list: Option<&Vec<String>>| -> Vec<String> {
        list.map(|l| l.iter().map(|s| substitute_str(s, vars)).collect()).unwrap_or_default()
    };
    let mut captures = BTreeMap::new();
    for (name, path) in &step.capture {
        match parse_html_capture(path) {
            Some(Ok(c)) => {
                captures.insert(name.clone(), c);
            }
            Some(Err(e)) => return Err(format!("step '{}' capture '{name}': {e}", step.name)),
            None => {}
        }
    }
    Ok(PostbackRequest {
        key: page_key(&url),
        url,
        button: pb.button.clone(),
        argument: substitute_str(&pb.argument, vars),
        fields,
        files: exec::attach_files(step, files)?,
        shows: fill(html.map(|h| &h.shows)),
        hides: fill(html.map(|h| &h.hides)),
        rows: html.and_then(|h| h.rows.as_ref()).map(|r| RowCheck {
            table: r.table.clone(),
            has: substitute_str(&r.has, vars),
        }),
        captures,
    })
}

/// How the page is to be got for a step.
#[derive(Serialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum CarryMode {
    /// Carry on from the answer of the step before; refuse if it is gone.
    Must,
    /// Load the page again, whatever was carried.
    Fresh,
    /// Carry on when the browser still holds the page, else load it (a
    /// one-off call, which has no run to say).
    IfThere,
}

/// Which pages a run has carried - names only; the pages themselves stay
/// in the browser, under this run's key.
pub struct Carry {
    key: String,
    one_off: bool,
    carried: HashSet<String>,
}

static RUNS: AtomicU64 = AtomicU64::new(0);

impl Carry {
    /// A new run's carry. Its key is unique to the run, so a browser still
    /// on a page from an earlier run carries nothing into this one.
    pub fn for_run(template_id: &str) -> Carry {
        let n = RUNS.fetch_add(1, Ordering::Relaxed);
        Carry { key: format!("{template_id}#{n}"), one_off: false, carried: HashSet::new() }
    }

    pub fn one_off() -> Carry {
        Carry { key: ONE_OFF_CARRY.to_string(), one_off: true, carried: HashSet::new() }
    }

    pub fn run_key(&self) -> &str {
        &self.key
    }

    pub fn mode(&self, key: &str, fresh: bool) -> CarryMode {
        if fresh {
            CarryMode::Fresh
        } else if self.one_off {
            CarryMode::IfThere
        } else if self.carried.contains(key) {
            CarryMode::Must
        } else {
            CarryMode::Fresh
        }
    }

    /// The step's answer is now the page held under `final_key`.
    pub fn carried_to(&mut self, final_key: &str) {
        self.carried.insert(final_key.to_string());
    }
}

/// Why the page would not send a step at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Lost,
    NotWebforms,
    NoButton,
    ManyButtons,
    TooLarge,
    SignedOut,
    LoadFailed,
}

/// What the in-page function handed back.
#[derive(Debug)]
pub enum Reply {
    Error(String),
    Timeout,
    Refused { why: Refusal, status: Option<u16>, count: Option<usize> },
    Answered(Answered),
}

/// The page's account of a sent postback: HTML and page state never come
/// back, only field names, check results, capture values and an
/// already-scrubbed excerpt.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Answered {
    pub status: u16,
    pub content_type: Option<String>,
    pub final_url: String,
    pub redirected: bool,
    pub final_key: String,
    pub carried: bool,
    pub signed_out: bool,
    pub too_large: bool,
    pub partial: bool,
    pub state_names: Vec<String>,
    pub page_names: Vec<String>,
    pub unknown_fields: Vec<String>,
    pub asp_error: Option<String>,
    pub invalid_postback: bool,
    pub invalid_field: Option<String>,
    pub checks: Vec<CheckResult>,
    pub captures: BTreeMap<String, CaptureResult>,
    pub excerpt: String,
    pub cut: bool,
}

/// One `expect.html` check as the page judged it. `kind` is `shows`,
/// `hides`, `rows` or `table` (the selector of a row check matched other
/// than once); `what` is the entry as written. A `rows` failure is worded
/// from the step's own row check, not from `what`, and any other `kind` is
/// a contract error.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CheckResult {
    pub kind: String,
    pub what: String,
    pub ok: bool,
    pub count: usize,
}

/// One `html:` capture as the page read it. `value` is `Some("")` when the
/// element has the attribute (or text) but it is empty, and `None` when the
/// attribute is absent - the page sends `null` for that.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CaptureResult {
    pub count: usize,
    pub value: Option<String>,
    pub state: bool,
    pub bad_selector: bool,
}

/// Reads the in-page function's reply by its shape; `None` for anything
/// that is not one of the three.
pub fn read_reply(v: &Value) -> Option<Reply> {
    if let Some(e) = v.get("error").and_then(Value::as_str) {
        return Some(if e == "timeout" { Reply::Timeout } else { Reply::Error(e.to_string()) });
    }
    if let Some(r) = v.get("refused").and_then(Value::as_str) {
        let why = match r {
            "lost" => Refusal::Lost,
            "not_webforms" => Refusal::NotWebforms,
            "no_button" => Refusal::NoButton,
            "many_buttons" => Refusal::ManyButtons,
            "too_large" => Refusal::TooLarge,
            "signed_out" => Refusal::SignedOut,
            "load_failed" => Refusal::LoadFailed,
            _ => return None,
        };
        let status = v.get("status").and_then(Value::as_u64).and_then(|n| u16::try_from(n).ok());
        let count = v.get("count").and_then(Value::as_u64).map(|n| n as usize);
        return Some(Reply::Refused { why, status, count });
    }
    if v.get("sent").and_then(Value::as_bool) == Some(true) {
        return serde_json::from_value(v.clone()).ok().map(Reply::Answered);
    }
    None
}

/// The sentence for a refusal.
pub fn refusal_sentence(
    why: Refusal,
    page: &str,
    button: &str,
    status: Option<u16>,
    count: Option<usize>,
) -> String {
    match why {
        Refusal::Lost => PAGE_LOST.to_string(),
        Refusal::NotWebforms => not_webforms(page),
        Refusal::NoButton => no_button(button),
        Refusal::ManyButtons => many_buttons(button, count.unwrap_or(0)),
        Refusal::TooLarge => TOO_LARGE.to_string(),
        Refusal::SignedOut => SENT_ELSEWHERE.to_string(),
        Refusal::LoadFailed => page_did_not_open(page, status.unwrap_or(0)),
    }
}

/// How one postback step came out.
#[derive(Debug)]
pub enum Verdict {
    Pass { detail: String, captured: BTreeMap<String, String> },
    SignedOut,
    Fail(String),
}

/// Judges the page's account of a step against the step, in a fixed order:
/// signed out, too large, a partial update, the application's own error
/// page, the status, each check, each capture. `req` is the request that
/// was sent (its row check carries the filled-in text); `sent_files` is how
/// the files that went are named for the detail.
pub fn judge(step: &Step, req: &PostbackRequest, a: &Answered, sent_files: Option<String>) -> Verdict {
    if a.signed_out {
        return Verdict::SignedOut;
    }
    if a.too_large {
        return Verdict::Fail(TOO_LARGE.to_string());
    }
    if a.partial {
        return Verdict::Fail(PARTIAL_UPDATE.to_string());
    }
    // The application refusing a field stands on its own: its answer need
    // not carry an error page's title.
    if a.invalid_postback {
        return Verdict::Fail(invalid_postback(a.invalid_field.as_deref()));
    }
    if let Some(title) = &a.asp_error {
        return Verdict::Fail(error_page(title));
    }
    if a.status != step.expect.status {
        return Verdict::Fail(format!("expected status {}, got {}", step.expect.status, a.status));
    }
    if let Some(c) = a.checks.iter().find(|c| !c.ok) {
        return Verdict::Fail(match c.kind.as_str() {
            "shows" => format!("the answer does not show {}", c.what),
            "hides" => format!("the answer shows {}", c.what),
            "table" => format!("{} matches {} elements, not one", c.what, c.count),
            "rows" => match &req.rows {
                Some(r) => format!("no row of {} holds {}", r.table, r.has),
                None => format!("the page reported a row check this step does not have: {}", c.what),
            },
            other => format!("the page reported a check of a kind this version does not know: {other}"),
        });
    }
    let mut captured = BTreeMap::new();
    for (name, path) in &step.capture {
        let Some(Ok(declared)) = parse_html_capture(path) else { continue };
        let selector = &declared.selector;
        let Some(r) = a.captures.get(name) else {
            return Verdict::Fail(format!("capture {name}: {selector} was not reported by the page"));
        };
        if r.bad_selector {
            return Verdict::Fail(format!("capture {name}: {selector} is not a CSS selector"));
        }
        if r.state {
            return Verdict::Fail(format!(
                "capture {name}: {selector} is page state, which a template never captures"
            ));
        }
        if r.count != 1 {
            return Verdict::Fail(format!("capture {name}: {selector} matches {} elements, not one", r.count));
        }
        let Some(value) = &r.value else {
            let what = declared.attr.as_deref().unwrap_or("text");
            return Verdict::Fail(format!("capture {name}: {selector} has no {what}"));
        };
        captured.insert(name.clone(), value.clone());
    }

    let mut detail = format!("status {}, {}", a.status, if a.carried { "carried on" } else { "loaded fresh" });
    if let Some(f) = sent_files {
        detail.push_str(&format!(", {f}"));
    }
    if !captured.is_empty() {
        detail.push_str(&format!(", captured {}", captured.keys().cloned().collect::<Vec<_>>().join(", ")));
    }
    if !a.unknown_fields.is_empty() {
        detail.push_str(&format!(", not on the page: {}", a.unknown_fields.join(", ")));
    }
    Verdict::Pass { detail, captured }
}

/// The body a postback sent, for a record or a sentence: a JSON object of
/// field name to value, where the template's own values show as written and
/// every other field shows only that it was there - page state as
/// `(page state)`, anything else the page filled in as `(from the page)`.
pub fn sent_body(req: &PostbackRequest, a: Option<&Answered>) -> String {
    let mut body = serde_json::Map::new();
    if let Some(a) = a {
        for name in &a.page_names {
            body.insert(name.clone(), Value::String(FROM_THE_PAGE.to_string()));
        }
        for name in &a.state_names {
            body.insert(name.clone(), Value::String(PAGE_STATE_SHOWN.to_string()));
        }
    }
    for (name, value) in &req.fields {
        body.insert(name.clone(), Value::String(value.clone()));
    }
    for f in &req.files {
        body.insert(f.field.clone(), Value::String(f.describe()));
    }
    Value::Object(body).to_string()
}
