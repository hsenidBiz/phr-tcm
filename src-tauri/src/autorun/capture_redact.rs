//! What the assistant may read of a captured request: its address, query
//! and body with every secret hidden, and nothing else cut.
//!
//! Built on `api_checks`' redaction (its secret words, its walk over JSON
//! text that does not parse) but for a different job. A failure quotes 500
//! characters with the addresses shortened; a template author needs the
//! whole body, addresses included, in pages. So nothing here cuts length
//! or shortens an address - only a value that holds a secret is replaced,
//! by `[redacted]`, and the body's shape stays readable.
//!
//! One list of words (`secret_name`) decides a name is a secret's, whether
//! it is a header's (the capture store), a JSON key, a form field, a
//! multipart part or a query parameter, so the rules cannot drift apart.
//! Separately, a JWT-looking value is hidden wherever it appears: an OIDC
//! answer can carry one under an innocent key.

use crate::api_templates::exec::scrub_tokens;
use crate::autorun::api_checks::{hide_members_by, BEARER, REDACTED, SECRET_WORDS};
use regex::{Captures, Regex};
use serde_json::Value;
use std::sync::LazyLock;

/// The words a name carries a secret under beyond `api_checks`' own:
/// keys, signatures, credentials, bearer and anti-forgery values, and the
/// short names a password goes by.
const MORE_WORDS: [&str; 11] =
    ["auth", "key", "credential", "signature", "jwt", "bearer", "csrf", "xsrf", "passwd", "pwd", "passphrase"];

/// The most nesting followed: a value that is a URL whose query holds a URL
/// ... Past it a value that would nest further is hidden whole.
const MAX_NESTING: u8 = 3;

fn words() -> impl Iterator<Item = &'static str> {
    SECRET_WORDS.iter().chain(MORE_WORDS.iter()).copied()
}

/// Whether a name - a header, a JSON key, a form field, a part, a query
/// parameter - sounds like it holds a secret: any word of the list, in any
/// case, anywhere in the name.
pub fn secret_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    words().any(|w| name.contains(w))
}

fn alternatives() -> String {
    words().collect::<Vec<_>>().join("|")
}

/// A JSON member named like a secret in text that does not parse; the
/// shape `api_checks`' `SECRET_KEY` has, with this list's words.
static SECRET_MEMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r#"(?i)(\\?)"[^"\\]*(?:{})[^"\\]*\\?"\s*:\s*"#, alternatives())).unwrap()
});

/// `name = "value"` or `name: 'value'` in a script, the name a bare
/// identifier with a secret word in it.
static SCRIPT_ASSIGN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r#"(?i)(\b[\w$.\-]*(?:{})[\w$.\-]*\s*[:=]\s*)("(?:[^"\\\r\n]|\\.)*"|'(?:[^'\\\r\n]|\\.)*')"#,
        alternatives()
    ))
    .unwrap()
});

/// An element named like a secret: `<password>x</password>`.
static XML_ELEMENT: LazyLock<Regex> = LazyLock::new(|| {
    let w = alternatives();
    Regex::new(&format!(
        r"(?is)(<[\w:.\-]*(?:{w})[\w:.\-]*(?:\s[^>]*[^/>])?>)(.*?)(</[\w:.\-]*(?:{w})[\w:.\-]*\s*>)"
    ))
    .unwrap()
});

/// An `<input>` or `<meta>` tag.
static FIELD_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<(?:input|meta)\b[^>]*>").unwrap());
/// What names a tag: `name`, `id` or `property`.
static TAG_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)\b(?:name|id|property)\s*=\s*["']?([^"'\s>]*)"#).unwrap());
static PASSWORD_TYPE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)\btype\s*=\s*["']?password\b"#).unwrap());
/// What such a tag holds: `value` or `content`.
static TAG_VALUE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)(\b(?:value|content)\s*=\s*)("[^"]*"|'[^']*')"#).unwrap());

/// A part's `name`, in its `Content-Disposition` line (not `filename`).
static PART_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)(?:^|[;\s])name\s*=\s*"?([^";\r\n]*)"#).unwrap());

/// Three or more base64url parts joined by dots.
static DOTTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z0-9_\-]+(?:\.[A-Za-z0-9_\-]+){2,}").unwrap());

/// A run is taken for a JWT only when it is longer than this.
const JWT_MIN_LEN: usize = 40;

/// Hides the JWT-looking runs of `text`: three or more base64url parts
/// joined by dots, over 40 characters, and either a JOSE header (`eyJ`, the
/// base64 of `{"`) or a mix of upper case, lower case and digits - so a long
/// dotted host or class name stays and a token under any key does not.
fn hide_jwts(text: &str) -> String {
    DOTTED
        .replace_all(text, |c: &Captures| {
            let run = &c[0];
            let mixed = run.chars().any(|c| c.is_ascii_uppercase())
                && run.chars().any(|c| c.is_ascii_lowercase())
                && run.chars().any(|c| c.is_ascii_digit());
            if run.len() > JWT_MIN_LEN && (run.starts_with("eyJ") || mixed) {
                REDACTED.to_string()
            } else {
                run.to_string()
            }
        })
        .into_owned()
}

/// What every redacted text ends with: bearer values and JWTs hidden
/// wherever they sit.
fn finish(text: String) -> String {
    let text = BEARER.replace_all(&text, format!("Bearer {REDACTED}").as_str());
    hide_jwts(&text)
}

fn decoded(component: &str) -> String {
    let spaced = component.replace('+', " ");
    urlencoding::decode(&spaced).map(|c| c.into_owned()).unwrap_or(spaced)
}

/// A value found inside another (a form field's, a query parameter's, a
/// JSON string's) with the secrets in it hidden: JSON redacted by key, an
/// address with a query redacted by parameter, other text member by
/// member. `None` when it holds nothing to hide.
fn redact_text_value(value: &str, depth: u8) -> Option<String> {
    let trimmed = value.trim();
    let json = (trimmed.starts_with('{') || trimmed.starts_with('['))
        .then(|| serde_json::from_str::<Value>(trimmed).ok())
        .flatten();
    let address = (value.starts_with('/') || value.contains("://")) && value.contains('?');
    if depth >= MAX_NESTING && (json.is_some() || address) {
        return Some(REDACTED.to_string());
    }
    if let Some(v) = json {
        let hidden = redact_value(&v, depth + 1);
        return (hidden != v).then(|| serde_json::to_string(&hidden).unwrap_or_default());
    }
    if address {
        let hidden = path_query_at(value, depth + 1);
        return (hidden != value).then_some(hidden);
    }
    if value.contains('"') {
        let hidden = hide_members_by(value, &SECRET_MEMBER);
        return (hidden != value).then_some(hidden);
    }
    None
}

/// `v` with every value under a secret key hidden, at any depth; a string
/// holding JSON, an address or members is redacted inside.
fn redact_value(v: &Value, depth: u8) -> Value {
    match v {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, x)| {
                    let x = if secret_name(k) { Value::String(REDACTED.to_string()) } else { redact_value(x, depth) };
                    (k.clone(), x)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(|i| redact_value(i, depth)).collect()),
        Value::String(s) => match redact_text_value(s, depth) {
            Some(hidden) => Value::String(hidden),
            None => v.clone(),
        },
        other => other.clone(),
    }
}

/// Fields `a=1&b=2` with a secret field's value hidden and a value that
/// holds a secret inside redacted. Every other field is left as written.
fn form_at(text: &str, depth: u8) -> String {
    text.split('&').map(|pair| field_at(pair, depth)).collect::<Vec<_>>().join("&")
}

fn field_at(pair: &str, depth: u8) -> String {
    let Some((name, value)) = pair.split_once('=') else { return pair.to_string() };
    if secret_name(&decoded(name)) {
        return format!("{name}={REDACTED}");
    }
    match redact_text_value(&decoded(value), depth) {
        Some(hidden) => format!("{name}={}", urlencoding::encode(&hidden)),
        None => pair.to_string(),
    }
}

fn path_query_at(pq: &str, depth: u8) -> String {
    let (head, fragment) = match pq.split_once('#') {
        Some((h, f)) => (h, Some(f)),
        None => (pq, None),
    };
    let (path, query) = match head.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (head, None),
    };
    let mut out = path.to_string();
    if let Some(q) = query {
        out.push('?');
        out.push_str(&form_at(q, depth));
    }
    if let Some(f) = fragment {
        out.push('#');
        out.push_str(&if f.contains('=') { form_at(f, depth) } else { f.to_string() });
    }
    out
}

/// An address's path and query (`/a/b?x=1`) with every query parameter named
/// like a secret hidden, and a parameter that holds an address, JSON or
/// members redacted inside. The path stays as it is - except a JWT in it.
pub fn redact_path_query(pq: &str) -> String {
    finish(path_query_at(pq, 0))
}

/// JSON text with every secret key's value hidden, at any depth. Left exactly
/// as written when there is nothing to hide; text that does not parse (a
/// body cut short) is redacted member by member.
fn json_text(body: &str) -> String {
    match serde_json::from_str::<Value>(body) {
        Ok(v) => {
            let hidden = redact_value(&v, 0);
            if hidden == v {
                body.to_string()
            } else {
                serde_json::to_string(&hidden).unwrap_or_default()
            }
        }
        Err(_) => page_text(body),
    }
}

/// A page, a script, XML or any other text: anti-forgery tokens, the value
/// of a secret field in a form, a meta tag, an element or a script's
/// assignment, and JSON members inside it.
fn page_text(body: &str) -> String {
    let text = scrub_tokens(body, None);
    let text = FIELD_TAG
        .replace_all(&text, |c: &Captures| {
            let tag = &c[0];
            let named = TAG_NAME.captures_iter(tag).any(|n| secret_name(&n[1]));
            if named || PASSWORD_TYPE.is_match(tag) {
                TAG_VALUE.replace_all(tag, format!("${{1}}\"{REDACTED}\"").as_str()).into_owned()
            } else {
                tag.to_string()
            }
        })
        .into_owned();
    let text = XML_ELEMENT.replace_all(&text, format!("${{1}}{REDACTED}${{3}}").as_str()).into_owned();
    let text = SCRIPT_ASSIGN.replace_all(&text, format!("${{1}}\"{REDACTED}\"").as_str()).into_owned();
    hide_members_by(&text, &SECRET_MEMBER)
}

/// A multipart body with the content of every part named like a secret
/// replaced by `[redacted]` (the headers and the delimiters stay), and the
/// other parts' text redacted member by member. The boundary is the mime's
/// `boundary=` or, failing that, the body's first delimiter line. `None`
/// when there is no boundary to find.
fn multipart_text(mime: &str, body: &str) -> Option<String> {
    let from_mime = mime.to_ascii_lowercase().find("boundary=").map(|at| {
        mime[at + "boundary=".len()..].split(';').next().unwrap_or("").trim().trim_matches('"').to_string()
    });
    let boundary = from_mime.filter(|b| !b.is_empty()).or_else(|| {
        body.lines().find_map(|l| l.trim_end().strip_prefix("--").filter(|b| !b.is_empty()).map(str::to_string))
    })?;
    let open = format!("--{boundary}");
    let close = format!("--{boundary}--");

    enum At {
        Preamble,
        Headers { secret: bool },
        Content { secret: bool, hidden: bool },
    }
    let mut at = At::Preamble;
    let mut out = String::with_capacity(body.len());
    for line in body.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\r', '\n']);
        if bare == open || bare == close {
            out.push_str(line);
            at = if bare == close { At::Preamble } else { At::Headers { secret: false } };
            continue;
        }
        at = match at {
            At::Preamble => {
                out.push_str(line);
                At::Preamble
            }
            At::Headers { secret } => {
                out.push_str(line);
                if bare.is_empty() {
                    At::Content { secret, hidden: false }
                } else {
                    let named = PART_NAME.captures(bare).is_some_and(|n| secret_name(&n[1]));
                    At::Headers { secret: secret || named }
                }
            }
            At::Content { secret: true, hidden } => {
                if !hidden {
                    out.push_str(REDACTED);
                    out.push_str(&line[bare.len()..]);
                }
                At::Content { secret: true, hidden: true }
            }
            At::Content { secret: false, hidden } => {
                out.push_str(&page_text(line));
                At::Content { secret: false, hidden }
            }
        };
    }
    Some(out)
}

/// `body` with every secret hidden, whatever its length: JSON by key, form
/// fields and multipart parts by name, pages and scripts by the value of a
/// secret field, and a JWT or bearer value wherever it sits. Nothing is
/// cut, and an address inside the body is kept (a secret query parameter in
/// it is hidden). `mime` is the body's content type; without one the body
/// is judged by its look.
pub fn redact_body(mime: Option<&str>, body: &str) -> String {
    let mime = mime.unwrap_or("").trim();
    let kind = mime.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    let trimmed = body.trim();
    let hidden = if kind.starts_with("multipart/")
        || (kind.is_empty() && trimmed.starts_with("--") && body.to_ascii_lowercase().contains("content-disposition"))
    {
        multipart_text(mime, body).unwrap_or_else(|| page_text(body))
    } else if kind.contains("x-www-form-urlencoded") {
        form_at(body, 0)
    } else if kind.contains("json") {
        json_text(body)
    } else if kind.is_empty() && (trimmed.starts_with('{') || trimmed.starts_with('[')) {
        json_text(body)
    } else if kind.is_empty() && looks_like_form(trimmed) {
        form_at(body, 0)
    } else {
        page_text(body)
    };
    finish(hidden)
}

/// `a=1&b=2`: no space or line break, at least one `=`.
fn looks_like_form(text: &str) -> bool {
    text.contains('=') && !text.chars().any(|c| c.is_whitespace() || c == '<' || c == '"')
}
