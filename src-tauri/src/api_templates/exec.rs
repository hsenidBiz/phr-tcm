//! Placeholder scanning and capture-path parsing shared by the template
//! checks (`api_templates::check`) and request building itself - the same
//! syntax has to agree in both places, so it lives in exactly one of them.
//!
//! Also: substitution, capture, request building and expectation checking
//! - all pure functions. Task 5's runner is the only caller that does I/O;
//! everything here just transforms values so it stays easy to test.

use super::{is_safe_relative_path, Expect, Method, Step};
use base64::Engine;
use crate::ado::endpoints::percent_encode_segment;
use regex::Regex;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// One segment of a parsed capture path: `.name`, `[N]` or `[*]` (every
/// element of an array).
#[derive(Debug, Clone, PartialEq)]
pub enum Seg {
    Key(String),
    Index(usize),
    All,
}

/// Every `{{name}}` placeholder found in `s`, in order, names only (no
/// braces, surrounding whitespace trimmed). An unterminated `{{` is left
/// alone - a literal string, not a placeholder - the caller sees it as
/// plain text with no name to check.
pub fn placeholders(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else { break };
        let name = after[..end].trim();
        if !name.is_empty() {
            out.push(name.to_string());
        }
        rest = &after[end + 2..];
    }
    out
}

/// Parses a capture path - `$.a`, `$.a[0]`, `$.a[*].id` - into segments.
/// Refused, each with a message naming the path: missing leading `$`, an
/// empty segment (so `$..a`, recursive descent, is refused rather than
/// silently accepted), an unclosed `[`, or an index that is neither `*`
/// nor all digits.
pub fn parse_capture_path(p: &str) -> Result<Vec<Seg>, String> {
    let mut chars = p.chars().peekable();
    if chars.next() != Some('$') {
        return Err(format!("capture path '{p}' must start with $"));
    }
    let mut segs = Vec::new();
    while let Some(&c) = chars.peek() {
        match c {
            '.' => {
                chars.next();
                let mut name = String::new();
                while let Some(&c2) = chars.peek() {
                    if c2 == '.' || c2 == '[' {
                        break;
                    }
                    name.push(c2);
                    chars.next();
                }
                if name.is_empty() {
                    return Err(format!("capture path '{p}' has an empty segment"));
                }
                segs.push(Seg::Key(name));
            }
            '[' => {
                chars.next();
                let mut inner = String::new();
                while let Some(&c2) = chars.peek() {
                    if c2 == ']' {
                        break;
                    }
                    inner.push(c2);
                    chars.next();
                }
                if chars.next() != Some(']') {
                    return Err(format!("capture path '{p}' has an unclosed ["));
                }
                if inner == "*" {
                    segs.push(Seg::All);
                } else if !inner.is_empty() && inner.chars().all(|c| c.is_ascii_digit()) {
                    segs.push(Seg::Index(inner.parse().expect("all-digit string parses as usize")));
                } else {
                    return Err(format!("capture path '{p}' has a bad index '[{inner}]'"));
                }
            }
            _ => return Err(format!("capture path '{p}' is malformed")),
        }
    }
    if segs.is_empty() {
        return Err(format!("capture path '{p}' names nothing after $"));
    }
    Ok(segs)
}

/// Walks `body` by `path`. `All` maps the remainder of the path over every
/// element of an array, dropping elements where it comes back `None`; if
/// nothing survives that (an empty source array, or every element
/// missing), the whole capture is `None` rather than `Some([])` - "found
/// nothing" and "found an empty list" are the same thing to a caller
/// deciding whether an output was captured.
pub fn capture(body: &Value, path: &[Seg]) -> Option<Value> {
    match path.split_first() {
        None => Some(body.clone()),
        Some((Seg::Key(k), rest)) => body.get(k).and_then(|v| capture(v, rest)),
        Some((Seg::Index(i), rest)) => body.get(*i).and_then(|v| capture(v, rest)),
        Some((Seg::All, rest)) => {
            let items = body.as_array()?;
            let out: Vec<Value> = items.iter().filter_map(|item| capture(item, rest)).collect();
            if out.is_empty() {
                None
            } else {
                Some(Value::Array(out))
            }
        }
    }
}

/// `s` is a placeholder and nothing else - `"{{name}}"`, no leading or
/// trailing text, and no nested `{{`/`}}` inside it. Returns the trimmed
/// name. This is what lets `substitute` hand back a var's real JSON type
/// instead of always stringifying it.
fn whole_placeholder(s: &str) -> Option<&str> {
    let inner = s.strip_prefix("{{")?.strip_suffix("}}")?;
    if inner.contains("{{") || inner.contains("}}") {
        return None;
    }
    let name = inner.trim();
    (!name.is_empty()).then_some(name)
}

/// Text form of a var's value for insertion into a larger string: a JSON
/// string is inserted as-is (no surrounding quotes), anything else as its
/// JSON text (`273`, `true`, `[1,2]`, ...).
fn var_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// One left-to-right pass over `s`: every `{{name}}` found in the ORIGINAL
/// string is replaced by `name`'s text form (see `var_text`); an unknown
/// name, or an empty one (`{{}}`, `{{  }}`), is left exactly as written.
/// Because the scan only ever looks at `s`, text a substitution inserts is
/// never itself rescanned for more placeholders - `{{name}}` where
/// `name`'s value contains `{{draft}}` comes back with that text intact.
pub fn substitute_str(s: &str, vars: &BTreeMap<String, Value>) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            out.push_str(rest);
            return out;
        };
        let name = after[..end].trim();
        out.push_str(&rest[..start]);
        let whole = &rest[start..start + 2 + end + 2];
        match (!name.is_empty()).then(|| vars.get(name)).flatten() {
            Some(val) => out.push_str(&var_text(val)),
            None => out.push_str(whole),
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

/// Substitutes placeholders through a JSON value, one pass (see
/// `substitute_str`). A string that IS a whole placeholder (`whole_placeholder`)
/// becomes the var's value with its real JSON type kept - a number stays a
/// number, an array stays an array; a string with a placeholder inside
/// other text becomes text via `substitute_str`. Object keys are always
/// substituted as text.
pub fn substitute(v: &Value, vars: &BTreeMap<String, Value>) -> Value {
    match v {
        Value::String(s) => match whole_placeholder(s).and_then(|name| vars.get(name)) {
            Some(val) => val.clone(),
            None => Value::String(substitute_str(s, vars)),
        },
        Value::Array(items) => Value::Array(items.iter().map(|item| substitute(item, vars)).collect()),
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, val) in map {
                out.insert(substitute_str(k, vars), substitute(val, vars));
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

/// A step's request body, ready to serialize for the JS `fetch` the
/// runner (Task 5) drives: `kind` picks the shape, `Json` carries the
/// substituted JSON value, `Form` carries substituted-as-text fields and
/// the step's files (`Step::files`), appended after the text fields.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Body {
    None,
    Json { value: Value },
    Form {
        fields: BTreeMap<String, String>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        files: Vec<FormFile>,
    },
}

/// One file a form step sends: the form field, the test file's name, its
/// content type (`test_files::content_type`), its size, and its bytes as
/// base64 - the one thing that goes to the page, which turns them back
/// into a `Blob`. The bytes are never written anywhere else: `describe` is
/// how a record or a sentence names a file, and `Debug` leaves them out.
#[derive(Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormFile {
    pub field: String,
    pub name: String,
    pub content_type: String,
    pub size: u64,
    pub base64: String,
}

impl FormFile {
    /// How a record or a sentence shows this file: `<file report.pdf, 1234 bytes>`.
    pub fn describe(&self) -> String {
        format!("<file {}, {} bytes>", self.name, self.size)
    }
}

impl std::fmt::Debug for FormFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FormFile")
            .field("field", &self.field)
            .field("name", &self.name)
            .field("content_type", &self.content_type)
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

/// A step with every placeholder resolved: ready to hand to `fetch` as-is.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BuiltRequest {
    pub method: Method,
    /// `path` followed by `?` and the encoded query string, or just the
    /// path when there is no query.
    pub url: String,
    pub body: Body,
}

/// Substitutes a step's raw `path` text one placeholder at a time: each
/// `{{name}}` is replaced by `name`'s text form (see `var_text`)
/// percent-encoded as a SINGLE path segment - every byte outside
/// `A-Za-z0-9-._~` is escaped, so a substituted value can never introduce
/// a `/`, `\`, `%`, `?` or `#` that the raw path text didn't already have.
/// A value that is exactly `.` or `..` is refused outright, before
/// encoding, naming the step and the placeholder. Text outside `{{...}}`
/// (and an unknown or empty placeholder) passes through unchanged, from
/// the raw `step.path` text - never a decoded form.
fn substitute_path(path: &str, step_name: &str, vars: &BTreeMap<String, Value>) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = path;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            out.push_str(rest);
            return Ok(out);
        };
        let name = after[..end].trim();
        out.push_str(&rest[..start]);
        let whole = &rest[start..start + 2 + end + 2];
        if name.is_empty() {
            out.push_str(whole);
        } else if let Some(val) = vars.get(name) {
            let text = var_text(val);
            if text == "." || text == ".." {
                return Err(format!(
                    "step '{step_name}' placeholder {{{{{name}}}}} in the path resolved to '{text}', which is not allowed"
                ));
            }
            out.push_str(&percent_encode_segment(&text));
        } else {
            out.push_str(whole);
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Builds the request a step describes once every placeholder is
/// resolved: the path (percent-encoded per-segment, see `substitute_path`,
/// then re-checked with `is_safe_relative_path` so an encoded `..` can't
/// slip through), the query string (substituted then percent-encoded,
/// pairs in the step's `query` map order - see `Step::query`, a
/// `BTreeMap` - joined with `&`), and the body (`json` substituted with
/// types kept, `form` substituted to text with the step's `files`, or
/// neither). `files` holds the bytes of each test file the step names, by
/// name - read by the runner, so this stays pure; a name it does not hold
/// is refused with `test_files::missing`'s sentence.
pub fn build_request(
    step: &Step,
    vars: &BTreeMap<String, Value>,
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<BuiltRequest, String> {
    let path = substitute_path(&step.path, &step.name, vars)?;
    if !is_safe_relative_path(&path) {
        return Err(format!(
            "step '{}' path is not a safe relative path on this origin once its placeholders are filled in: '{}'",
            step.name, path
        ));
    }

    let mut pairs = Vec::with_capacity(step.query.len());
    for (k, v) in &step.query {
        let key = percent_encode_segment(&substitute_str(k, vars));
        let value = percent_encode_segment(&substitute_str(v, vars));
        pairs.push(format!("{key}={value}"));
    }
    let url = if pairs.is_empty() { path } else { format!("{path}?{}", pairs.join("&")) };

    let body = if let Some(json) = &step.json {
        Body::Json { value: substitute(json, vars) }
    } else if let Some(form) = &step.form {
        let fields =
            form.iter().map(|(k, v)| (substitute_str(k, vars), substitute_str(v, vars))).collect();
        let mut attached = Vec::with_capacity(step.files.len());
        for (field, name) in &step.files {
            let Some(bytes) = files.get(name) else {
                return Err(crate::test_files::missing(name, &format!("the step \"{}\"", step.name)));
            };
            attached.push(FormFile {
                field: field.clone(),
                name: name.clone(),
                content_type: crate::test_files::content_type(name).to_string(),
                size: bytes.len() as u64,
                base64: base64::engine::general_purpose::STANDARD.encode(bytes),
            });
        }
        Body::Form { fields, files: attached }
    } else {
        Body::None
    };

    Ok(BuiltRequest { method: step.method, url, body })
}

/// Compact JSON text for an error message (`273`, `true`, `"x"`, ...).
fn compact(v: &Value) -> String {
    serde_json::to_string(v).unwrap_or_default()
}

/// Partial match: every key in `expected` must be present in `actual`
/// with an equal value; `actual` may carry extra keys. A nested object in
/// `expected` recurses the same way (also partial). Fails on the first
/// mismatching key encountered, in `expected`'s own key order.
fn partial_match(expected: &Value, actual: &Value) -> Result<(), String> {
    let Value::Object(exp_map) = expected else {
        return if expected == actual {
            Ok(())
        } else {
            Err(format!("expected {}, got {}", compact(expected), compact(actual)))
        };
    };
    for (k, exp_v) in exp_map {
        match actual.get(k) {
            None => return Err(format!("expected {k} = {}, got nothing", compact(exp_v))),
            Some(act_v) => {
                if exp_v.is_object() {
                    partial_match(exp_v, act_v)?;
                } else if exp_v != act_v {
                    return Err(format!("expected {k} = {}, got {}", compact(exp_v), compact(act_v)));
                }
            }
        }
    }
    Ok(())
}

/// Checks a step's response against its `Expect`: status first (an
/// exact-message mismatch, before the body is even parsed), then - only
/// when `e.json` is set - that the body parses as JSON at all (an HTML
/// error page fails with exactly `"the response was not JSON"`, naming no
/// parser detail) and partially matches (`partial_match`). Returns the
/// parsed body on success, or on a status/no-`json`-expectation path
/// where the body happens to parse as JSON anyway - `None` when it
/// doesn't parse and no `json` expectation asked for it to.
pub fn check_expect(e: &Expect, status: u16, body_text: &str) -> Result<Option<Value>, String> {
    if status != e.status {
        return Err(format!("expected status {}, got {status}", e.status));
    }
    let parsed: Option<Value> = serde_json::from_str(body_text).ok();
    match &e.json {
        None => Ok(parsed),
        Some(expected) => {
            let Some(actual) = &parsed else {
                return Err("the response was not JSON".to_string());
            };
            partial_match(expected, actual)?;
            Ok(parsed)
        }
    }
}

/// What an anti-forgery token is written as wherever one is taken out.
pub const TOKEN_SHOWN: &str = "(token)";

/// Opening tags (to their `>`, or to the end of a body cut off inside one).
static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<[a-z][^<>]*").unwrap());
/// A tag named for the anti-forgery token - Razor's hidden
/// `__RequestVerificationToken` input, or a `RequestVerificationToken`
/// meta tag - with any quoting.
static TOKEN_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\b(?:name|id)\s*=\s*["']?_{0,2}requestverificationtoken\b"#).unwrap()
});
/// The value a token tag carries: `value=` / `content=`, double-quoted,
/// single-quoted or bare, and a quote the 64 KB cut left open.
static TAG_VALUE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(\b(?:value|content)\s*=\s*)(?:"[^"]*"?|'[^']*'?|[^\s"'>]+)"#).unwrap()
});
/// A JSON (or script-object) member named for the token, and its string.
static TOKEN_MEMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(["']_{0,2}requestverificationtoken["']\s*:\s*)(?:"(?:[^"\\]|\\.)*"?|'(?:[^'\\]|\\.)*'?)"#,
    )
    .unwrap()
});
/// `__RequestVerificationToken=...` in form text, `RequestVerificationToken:
/// ...` in header text.
static TOKEN_PAIR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(\b_{0,2}requestverificationtoken\s*[=:]\s*)[^\s&"'<>,;]+"#).unwrap()
});

/// `text` - a request or response body - with every anti-forgery token
/// taken out: `known` (the token the runner read) wherever it appears, and
/// the value of anything named for a token, whatever it holds - a page
/// can carry a different token than the one read, and a JSON answer can
/// carry one too. Runs BEFORE `excerpt`, so a cap can never cut a token in
/// half and leave the rest showing.
pub fn scrub_tokens(text: &str, known: Option<&str>) -> String {
    let mut out = match known {
        Some(k) if !k.is_empty() => text.replace(k, TOKEN_SHOWN),
        _ => text.to_string(),
    };
    out = TAG
        .replace_all(&out, |c: &regex::Captures| {
            let tag = &c[0];
            if TOKEN_TAG.is_match(tag) {
                TAG_VALUE.replace_all(tag, format!("${{1}}\"{TOKEN_SHOWN}\"")).into_owned()
            } else {
                tag.to_string()
            }
        })
        .into_owned();
    out = TOKEN_MEMBER.replace_all(&out, format!("${{1}}\"{TOKEN_SHOWN}\"")).into_owned();
    TOKEN_PAIR.replace_all(&out, format!("${{1}}{TOKEN_SHOWN}")).into_owned()
}

/// A captured JSON value with its tokens taken out, for what a report
/// shows - strings through `scrub_tokens`, and a member named for the
/// token replaced whole.
pub fn scrub_value(v: &Value, known: Option<&str>) -> Value {
    match v {
        Value::String(s) => Value::String(scrub_tokens(s, known)),
        Value::Array(items) => Value::Array(items.iter().map(|i| scrub_value(i, known)).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, x)| {
                    let named = k.trim_start_matches('_').eq_ignore_ascii_case("requestverificationtoken");
                    let x = if named { Value::String(TOKEN_SHOWN.to_string()) } else { scrub_value(x, known) };
                    (k.clone(), x)
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

/// A logged/failure-message-safe excerpt of `text`: runs of whitespace
/// collapsed to a single space (also trims the ends), capped at 500
/// characters - the constraint shared by log lines and failure excerpts
/// alike.
pub fn excerpt(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(500).collect()
}
