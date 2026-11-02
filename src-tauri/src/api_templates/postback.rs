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
    /// The answer had no body at all (an empty 400 is the application
    /// refusing the request unread).
    pub empty: bool,
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

/// Whether a failed call (`Reply::Error` or `Reply::Timeout`) got as far as
/// making the post: the page says so from the moment before it. Anything
/// before that - loading the page, a refusal - saved nothing.
pub fn reply_posted(v: &Value) -> bool {
    v.get("posted").and_then(Value::as_bool) == Some(true)
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

/// Sends one postback step from the page itself (`PostbackRequest`,
/// serialized) and answers with what happened, never with the page: field
/// NAMES, check results, capture values and a scrubbed excerpt are all that
/// come back (`Reply`). `run` names the run; the page keeps the last answer
/// of each page of the run in `window.__tcmPostback.pages` (key -> `{ html,
/// url }`), and a different `run` starts that map empty, so a browser still
/// on the page from an earlier run carries nothing from it. `carry` is a
/// `CarryMode`: `must` carries on from the held answer or refuses (`lost`),
/// `if_there` does when it is held and loads otherwise, `fresh` loads. The
/// post is what a browser would send: every `__` hidden input, every other
/// enabled control's value (unchecked boxes left out), then the template's
/// fields, then the control clicked - a submit as `name=value` with an empty
/// target, anything else as the full target and `req.argument`. The whole
/// call is aborted after `limitMs`. A postback is never retried by anyone: a
/// timeout comes back as `{ error: "timeout" }`.
pub const POSTBACK_FN: &str = r##"async function (req, limitMs, run, carry, carryLimit, answerLimit) {
  const ctrl = new AbortController();
  const timer = setTimeout(() => ctrl.abort(), limitMs || 30000);
  // Set the moment before the post is made: an error after it may have saved.
  let posted = false;
  try {
    if (!window.__tcmPostback || window.__tcmPostback.run !== run) window.__tcmPostback = { run: run, pages: {} };
    const pages = window.__tcmPostback.pages;
    // Rust's `page_key`: the path alone, ASCII lower case, no trailing "/", "/" when nothing is left.
    const keyOf = (u) => {
      const p = new URL(u, location.href).pathname.replace(/[A-Z]/g, (c) => c.toLowerCase()).replace(/\/+$/, '');
      return p === '' ? '/' : p;
    };
    const size = (s) => new Blob([s]).size;
    const parse = (html) => new DOMParser().parseFromString(html, 'text/html');
    const norm = (s) => String(s).replace(/\s+/g, ' ').trim();
    const TOKEN = /^_{0,2}requestverificationtoken$/i;
    const typeOf = (e) => (e.getAttribute('type') || '').toLowerCase();

    // Hidden: it or an ancestor has `hidden`, an inline display:none or visibility:hidden, or is
    // input[type=hidden], script, style, noscript or template. DOMParser applies no stylesheet.
    const NEVER = ['SCRIPT', 'STYLE', 'NOSCRIPT', 'TEMPLATE'];
    const selfHidden = (e) => {
      const s = e.getAttribute('style') || '';
      return e.hasAttribute('hidden') || NEVER.includes(e.tagName) ||
        (e.tagName === 'INPUT' && typeOf(e) === 'hidden') ||
        /(^|;)\s*display\s*:\s*none\b/i.test(s) || /(^|;)\s*visibility\s*:\s*hidden\b/i.test(s);
    };
    const hidden = (e) => { for (let x = e; x && x.nodeType === 1; x = x.parentElement) if (selfHidden(x)) return true; return false; };
    const textOf = (root) => {
      const out = [];
      const walk = (n) => {
        if (n.nodeType === 3) out.push(n.nodeValue);
        else if (n.nodeType === 1 && !selfHidden(n)) for (const c of n.childNodes) walk(c);
      };
      walk(root);
      return norm(out.join(' '));
    };
    // Mirrors Rust's `is_selector`; an entry that is not a valid selector is read as text.
    const isSel = (s) => s.startsWith('#') || s.startsWith('.') || s.startsWith('[') || s.includes('>');
    const shown = (doc, what) => {
      if (isSel(what)) {
        try { return Array.from(doc.querySelectorAll(what)).some((e) => !hidden(e)); } catch (e) { /* text */ }
      }
      return textOf(doc.body || doc.documentElement).includes(norm(what));
    };

    // The page to post from: the held answer, or a fresh load.
    let source = null;
    if (carry === 'must' || carry === 'if_there') {
      const held = pages[req.key];
      if (held) source = { html: held.html, url: held.url, carried: true };
      else if (carry === 'must') return { refused: 'lost' };
    }
    if (!source) {
      const g = await fetch(req.url, { credentials: 'same-origin', signal: ctrl.signal });
      if (!g.ok) return { refused: 'load_failed', status: g.status };
      const html = await g.text();
      if (size(html) > carryLimit) return { refused: 'too_large' };
      source = { html: html, url: g.url, carried: false, redirected: g.redirected };
    }
    const doc = parse(source.html);
    const sourceKey = keyOf(source.url);
    const sourceHadPassword = !!doc.querySelector('input[type="password"]');
    // A load that was sent to another page with a password field is the sign-in page; a page of
    // the application that has a password field of its own (change password) is not. A held page
    // is never the sign-in page: an answer that was is not kept.
    if (!source.carried && source.redirected && sourceKey !== req.key && sourceHadPassword) return { refused: 'signed_out' };
    const vs = doc.querySelector('input[name="__VIEWSTATE"]');
    const form = vs && (vs.form || vs.closest('form'));
    if (!form) return { refused: 'not_webforms' };

    // The control clicked: by name (equal, or ending in "$<button>"), or a
    // __doPostBack('<target>' in its href or onclick.
    const full = (n) => n === req.button || n.endsWith('$' + req.button);
    // A LinkButton on a page with validators renders WebForm_DoPostBackWithOptions(new
    // WebForm_PostBackOptions("<target>", ...)) instead; posting directly skips the client-side
    // validators, and the server validates again.
    const posting = /(?:__doPostBack\(|WebForm_PostBackOptions\()\s*\\?['"]([^'"\\]*)/;
    const found = [];
    for (const e of doc.querySelectorAll('input, button, select, textarea, a')) {
      if (e.tagName === 'INPUT' && typeOf(e) === 'hidden') continue;
      const name = e.getAttribute('name');
      if (name && full(name)) { found.push({ e: e, target: name, byName: true }); continue; }
      const m = posting.exec((e.getAttribute('href') || '') + ' ' + (e.getAttribute('onclick') || ''));
      if (m && full(m[1])) found.push({ e: e, target: m[1], byName: false });
    }
    if (found.length === 0) return { refused: 'no_button' };
    if (found.length > 1) return { refused: 'many_buttons', count: found.length };
    const hit = found[0];
    const hitType = typeOf(hit.e);
    const submit = hit.byName && ((hit.e.tagName === 'INPUT' && (hitType === 'submit' || hitType === 'image')) ||
      (hit.e.tagName === 'BUTTON' && (hitType === '' || hitType === 'submit')));

    // What a browser would send for the form as the page has it.
    const sent = [];
    const known = new Set();
    const named = Array.from(form.elements).filter((e) => e.getAttribute('name'));
    for (const e of named) {
      const name = e.getAttribute('name');
      known.add(name);
      if (!['INPUT', 'SELECT', 'TEXTAREA'].includes(e.tagName) || e.matches(':disabled')) continue;
      if (e.tagName === 'INPUT') {
        const t = typeOf(e);
        if (['submit', 'image', 'button', 'reset', 'file'].includes(t)) continue;
        if ((t === 'checkbox' || t === 'radio') && !e.checked) continue;
        sent.push([name, e.value]);
      } else if (e.tagName === 'SELECT') {
        for (const o of e.selectedOptions) if (!o.disabled) sent.push([name, o.value]);
      } else {
        // A browser sends a textarea's line breaks as CRLF.
        sent.push([name, e.value.replace(/\r?\n/g, '\r\n')]);
      }
    }
    const drop = (name) => { for (let i = sent.length - 1; i >= 0; i--) if (sent[i][0] === name) sent.splice(i, 1); };
    const unknown = [];
    for (const [k, v] of Object.entries(req.fields)) {
      if (!known.has(k)) unknown.push(k);
      const ticks = named.some((e) => e.getAttribute('name') === k && ['checkbox', 'radio'].includes(typeOf(e)));
      drop(k);
      if (!(ticks && v === '')) sent.push([k, v]);
    }
    drop('__EVENTTARGET');
    drop('__EVENTARGUMENT');
    if (submit) {
      sent.push(['__EVENTTARGET', ''], ['__EVENTARGUMENT', '']);
      if (hit.e.tagName === 'INPUT' && hitType === 'image') sent.push([hit.target + '.x', '0'], [hit.target + '.y', '0']);
      else sent.push([hit.target, hit.e.getAttribute('value') || '']);
    } else {
      sent.push(['__EVENTTARGET', hit.target], ['__EVENTARGUMENT', req.argument]);
    }
    const stateNames = [];
    const pageNames = [];
    for (const [n] of sent) {
      const list = n.startsWith('__') ? stateNames : pageNames;
      if (!list.includes(n)) list.push(n);
    }

    let body;
    if (req.files && req.files.length) {
      body = new FormData();
      for (const [k, v] of sent) body.append(k, v);
      for (const f of req.files) {
        body.append(f.field, new Blob([Uint8Array.from(atob(f.base64), (c) => c.charCodeAt(0))], { type: f.contentType }), f.name);
      }
    } else {
      body = new URLSearchParams();
      for (const [k, v] of sent) body.append(k, v);
    }
    // The form's own action, as the browser reads it from the page it is on.
    let action = new URL(form.getAttribute('action') || source.url, source.url);
    if (action.origin !== location.origin) action = new URL(req.url, location.href);
    posted = true;
    const r = await fetch(action.href, { method: 'POST', credentials: 'same-origin', headers: { 'Accept': 'text/html,application/xhtml+xml' }, body: body, signal: ctrl.signal });
    const text = await r.text();

    const finalKey = keyOf(r.url);
    const out = {
      sent: true, status: r.status, contentType: r.headers.get('content-type'), finalUrl: r.url, redirected: r.redirected,
      finalKey: finalKey, carried: source.carried, signedOut: false, tooLarge: false, partial: false, empty: text.trim() === '',
      stateNames: stateNames, pageNames: pageNames, unknownFields: unknown, aspError: null, invalidPostback: false,
      invalidField: null, checks: [], captures: {}, excerpt: '', cut: false,
    };
    if (size(text) > carryLimit) { out.tooLarge = true; return out; }
    // An UpdatePanel answer is `length|type|id|content|...`; it is no page to read or keep, and
    // its `hiddenField|__VIEWSTATE|...` entries are page state, so no excerpt is made of it.
    if (/^\d+\|/.test(text)) { out.partial = true; return out; }
    const ans = parse(text);
    // Signed out: the answer has a password field and the page posted from had none, or the post
    // was sent to a different page.
    if (ans.querySelector('input[type="password"]') && (!sourceHadPassword || (r.redirected && finalKey !== sourceKey))) {
      out.signedOut = true;
      return out;
    }
    pages[finalKey] = { html: text, url: r.url };

    // The application's own error page, and its refusal of a value the page did not offer.
    const title = norm(ans.title || '');
    let asp = null;
    if (title.includes('Runtime Error')) asp = title;
    else {
      for (const h of ans.querySelectorAll('h1, span')) {
        if ((h.textContent || '').trim().startsWith('Server Error in')) { asp = title || norm(h.textContent); break; }
      }
    }
    if (asp !== null) out.aspError = asp.slice(0, 200);
    if (text.includes('Invalid postback or callback argument') && (asp !== null || r.status >= 400)) {
      out.invalidPostback = true;
      const word = /[A-Za-z0-9_$]/;
      for (const n of pageNames) {
        for (let i = text.indexOf(n); i >= 0; i = text.indexOf(n, i + 1)) {
          if (!word.test(text[i - 1] || ' ') && !word.test(text[i + n.length] || ' ')) { out.invalidField = n; break; }
        }
        if (out.invalidField) break;
      }
    }

    const check = (kind, what, ok, count) => out.checks.push({ kind: kind, what: what, ok: ok, count: count || 0 });
    for (const w of req.shows) check('shows', w, shown(ans, w));
    for (const w of req.hides) check('hides', w, !shown(ans, w));
    if (req.rows) {
      let tables = [];
      try { tables = Array.from(ans.querySelectorAll(req.rows.table)); } catch (e) { /* none */ }
      if (tables.length !== 1) check('table', req.rows.table, false, tables.length);
      else {
        const has = norm(req.rows.has);
        check('rows', req.rows.has, Array.from(tables[0].querySelectorAll('tr')).some((tr) => !hidden(tr) && textOf(tr).includes(has)), 1);
      }
    }
    for (const [name, cap] of Object.entries(req.captures)) {
      let els;
      try { els = Array.from(ans.querySelectorAll(cap.selector)); }
      catch (e) { out.captures[name] = { count: 0, value: null, state: false, badSelector: true }; continue; }
      const c = { count: els.length, value: null, state: false, badSelector: false };
      if (els.length === 1) {
        const e = els[0];
        const nm = e.getAttribute('name') || '';
        const idv = e.getAttribute('id') || '';
        const held = TOKEN.test(nm) || TOKEN.test(idv) ||
          (['INPUT', 'SELECT', 'TEXTAREA'].includes(e.tagName) && (nm.startsWith('__') || idv.startsWith('__')));
        if (held) c.state = true;
        else if (cap.attr === null || cap.attr === undefined) c.value = (e.textContent || '').trim();
        else if (cap.attr === 'value' && e.tagName === 'SELECT') { const o = e.selectedOptions[0]; c.value = o ? o.value : null; }
        else c.value = e.getAttribute(cap.attr);
      }
      out.captures[name] = c;
    }

    // The excerpt: the answer with every state field's value taken out.
    // The anti-forgery token is an input's value or a meta tag's content, whatever its tag.
    for (const e of ans.querySelectorAll('[name], [id]')) {
      const names = [e.getAttribute('name') || '', e.getAttribute('id') || ''];
      if (names.some((n) => TOKEN.test(n))) {
        for (const a of ['value', 'content']) if (e.hasAttribute(a)) e.setAttribute(a, '(token)');
      } else if (e.hasAttribute('value') && names.some((n) => n.startsWith('__') && !/^__(eventtarget|eventargument)$/i.test(n))) {
        e.setAttribute('value', '(page state)');
      }
    }
    const html = ans.documentElement ? ans.documentElement.outerHTML : '';
    out.cut = html.length >= answerLimit;
    out.excerpt = html.slice(0, answerLimit);
    return out;
  } catch (e) {
    return { error: e && e.name === 'AbortError' ? 'timeout' : String(e), posted: posted };
  } finally {
    clearTimeout(timer);
  }
}"##;
