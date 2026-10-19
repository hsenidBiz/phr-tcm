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
//! One rule (`secret_name`) decides a name is a secret's, whether it is a
//! header's (the capture store), a JSON key, a form field, a multipart
//! part or a query parameter, so the rules cannot drift apart.
//!
//! A body is judged by what it IS, never by the content type it claims: a
//! JSON body sent as a form, a form sent as `text/plain`. Beyond names, two
//! things are hidden wherever they sit: a JWT-looking value (an OIDC answer
//! can carry one under an innocent key) and any long opaque run of token
//! characters (an echoed session cookie, a token under any name). So are an
//! authenticator's `otpauth://` URI and its base32 key, grouped or not.

use crate::api_templates::exec::scrub_tokens;
use crate::autorun::api_checks::{hide_members_by, REDACTED, SECRET_WORDS};
use crate::browser::net_capture::HIDDEN;
use regex::{Captures, Regex};
use serde_json::Value;
use std::sync::LazyLock;

/// Words a name carries a secret under, matched anywhere in the name once
/// its separators are taken out (beyond `api_checks`' own).
const MORE_WORDS: [&str; 13] = [
    "credential", "signature", "jwt", "bearer", "csrf", "xsrf", "passwd", "pwd", "passphrase", "samlresponse",
    "samlrequest", "relaystate", "codeverifier",
];

/// Words a name carries a secret under only as a word of its own (`otp` is
/// inside `footprint`): an authenticator's shared key, a one-time code, a
/// recovery code. `twoFactor` counts written as one word or two.
const SEGMENT_WORDS: [&str; 7] = ["shared", "authenticator", "totp", "otp", "mfa", "twofactor", "recovery"];

/// What may stand before a final `key` word to make a compound: `api key`,
/// `subscription-key`, `x-functions-key`... (`sortKey`, `primaryKey`,
/// `foreignKey` and `keyword` are not secrets).
const KEY_PREFIXES: [&str; 14] = [
    "api", "access", "private", "secret", "client", "subscription", "function", "functions", "master", "host", "auth",
    "signing", "encryption", "license",
];

/// The most nesting followed: a value that is a URL whose query holds a URL
/// ... Past it a value that would nest further is hidden whole.
const MAX_NESTING: u8 = 3;

/// The words of a name, lower case, split on anything that is not a letter
/// or digit and on camelCase boundaries: `isAuthorized` -> is, authorized;
/// `x-api-key` -> x, api, key; `APIKey` -> api, key.
fn segments(name: &str) -> Vec<String> {
    let chars: Vec<char> = name.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_alphanumeric() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            continue;
        }
        if !cur.is_empty() {
            let prev = chars[i - 1];
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            let boundary = c.is_uppercase()
                && (prev.is_lowercase() || prev.is_ascii_digit() || (prev.is_uppercase() && next_lower));
            if boundary {
                out.push(std::mem::take(&mut cur));
            }
        }
        cur.extend(c.to_lowercase());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Whether a name - a header, a JSON key, a form field, a part, a query
/// parameter - sounds like it holds a secret.
///
/// Most words count anywhere in the name, in any case, with the separators
/// ignored (`access_token`, `Set-Cookie`, `Input.Password`). `auth` and `key`
/// are too common inside other words to count that way: `auth` only as a
/// word of its own (`auth`, `authToken`, `x-auth-user`; never `author` or
/// `isAuthorized`), `key` only in a compound (`apiKey`, `x-api-key`,
/// `private_key`; never `keyword` or `sortKey`).
pub fn secret_name(name: &str) -> bool {
    let plain: String = name.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect();
    if SECRET_WORDS.iter().chain(MORE_WORDS.iter()).any(|w| plain.contains(w)) {
        return true;
    }
    if name.to_ascii_lowercase().starts_with("x-auth") {
        return true;
    }
    let words = segments(name);
    if words.iter().any(|w| w == "auth" || w == "authtoken") {
        return true;
    }
    if words.iter().any(|w| SEGMENT_WORDS.contains(&w.as_str()))
        || words.windows(2).any(|p| p[0] == "two" && p[1] == "factor")
    {
        return true;
    }
    const WHOLE: [&str; 12] = [
        "apikey", "accesskey", "privatekey", "secretkey", "clientkey", "encryptionkey", "signingkey", "masterkey",
        "subscriptionkey", "functionkey", "hostkey", "authkey",
    ];
    let compound = |w: &[String]| {
        w.last().is_some_and(|l| WHOLE.contains(&l.as_str()))
            || (w.len() >= 2 && w[w.len() - 1] == "key" && KEY_PREFIXES.contains(&w[w.len() - 2].as_str()))
    };
    if words.iter().any(|w| WHOLE.contains(&w.as_str())) || words == ["key"] || compound(&words) {
        return true;
    }
    // `apiKeyValue`, `x-api-key-id`: the key's value or id.
    words.len() >= 2 && matches!(words[words.len() - 1].as_str(), "value" | "id") && compound(&words[..words.len() - 1])
}

/// Any JSON member in text that does not parse: group 1 the escape, group 2
/// the name. Which are secret is `secret_name`'s say.
static ANY_MEMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(\\?)["']([^"'\\]*)\\?["']\s*:\s*"#).unwrap());

/// `name = "value"`, `name: 'value'`, `name=value`, GraphQL's
/// `password:\"x\"`: the name a bare identifier (checked with
/// `secret_name`), the value quoted - escaped quotes too - or a run up to a
/// separator.
static ASSIGN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"([\w$.\-]+)(\s*[:=]\s*)(\\"(?:[^"\\]|\\[^"])*\\"|"(?:[^"\\\r\n]|\\.)*"|'(?:[^'\\\r\n]|\\.)*'|\\"[^"\\]*\\?|"[^"\\\r\n]*|[^\s&;,"'<>)}\[\]\\]+)"#,
    )
    .unwrap()
});

/// A line that is `name: value` or `name=value` (`text/plain` forms, header
/// blocks, properties): the whole rest of the line is the value.
static LINE_FIELD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^([ \t]*)([\w$.\-\[\]]+)([ \t]*[:=][ \t]*)([^\r\n]*)").unwrap());

/// An authorization line or value, whatever its scheme: `Basic dXNl...`.
static AUTH_HEADER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)(authorization\s*[:=]\s*)[^\r\n"'&;,<>\[\]]+"#).unwrap());

/// An element's opening tag.
static OPEN_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<([\w:.\-]+)(?:\s[^>]*[^/>])?>").unwrap());
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

/// A part's `name`, in its `Content-Disposition` line (not `filename`)...
static PART_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)(?:^|[;\s])name\s*=\s*"?([^";\r\n]*)"#).unwrap());
/// ...or its RFC 5987 form: `name*=UTF-8''password`.
static PART_NAME_EXT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)(?:^|[;\s])name\*\s*=\s*"?[^'";\r\n]*'[^'";\r\n]*'([^";\r\n]*)"#).unwrap());

/// `Bearer <token>`, also with an encoded space.
static BEARER_VALUE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bbearer(?:\s|%20|\+)+[\w\-.~+/%]+=*").unwrap());

/// A JWT whose parts may be broken across lines: a JOSE header (`eyJ`) and
/// two more parts, a line break allowed inside a part or beside a dot.
static JOSE_SPLIT: LazyLock<Regex> = LazyLock::new(|| {
    let seg = r"[A-Za-z0-9_\-]+(?:[ \t]*\r?\n[ \t]*[A-Za-z0-9_\-]+)*";
    let dot = r"(?:[ \t]*\r?\n[ \t]*)?\.(?:[ \t]*\r?\n[ \t]*)?";
    Regex::new(&format!(r"eyJ{seg}{dot}{seg}{dot}{seg}")).unwrap()
});
/// Three or more base64url parts joined by dots.
static DOTTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z0-9_\-]+(?:\.[A-Za-z0-9_\-]+){2,}").unwrap());
/// A long run of characters a token is written with (`hex` tokens start at 32).
static LONG_RUN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Za-z0-9_\-+/=.%]{32,}").unwrap());

/// A JWT-looking run is hidden only when longer than this.
const JWT_MIN_LEN: usize = 40;

/// Hides the JWT-looking runs of `text`: a JOSE-headed token (`eyJ`, the
/// base64 of `{"`), line breaks and all, and three or more base64url parts
/// joined by dots, over 40 characters, that mix upper case, lower case and
/// digits, with two parts of 16 or more characters - so a long dotted host or
/// class name stays and a token under any key does not.
fn hide_jwts(text: &str) -> String {
    let text = JOSE_SPLIT.replace_all(text, |c: &Captures| {
        let run = &c[0];
        if run.chars().filter(|c| !c.is_whitespace()).count() > JWT_MIN_LEN {
            REDACTED.to_string()
        } else {
            run.to_string()
        }
    });
    DOTTED
        .replace_all(&text, |c: &Captures| {
            let run = &c[0];
            let mixed = run.chars().any(|c| c.is_ascii_uppercase())
                && run.chars().any(|c| c.is_ascii_lowercase())
                && run.chars().any(|c| c.is_ascii_digit());
            let long_parts = run.split('.').filter(|p| p.len() >= 16).count();
            if run.len() > JWT_MIN_LEN && (run.starts_with("eyJ") || (mixed && long_parts >= 2)) {
                REDACTED.to_string()
            } else {
                run.to_string()
            }
        })
        .into_owned()
}

/// A token-shaped value of this length (or longer) is hidden: one path
/// segment, one value.
const OPAQUE_MIN: usize = 40;
/// Pure hex is a token (a key, a digest) from this length.
const HEX_MIN: usize = 32;

/// A letter and a digit both: what a cookie value has and a word lacks.
fn opaque(text: &str) -> bool {
    text.chars().any(|c| c.is_ascii_digit()) && text.chars().any(|c| c.is_ascii_alphabetic())
}

/// Whether `text` is shaped like a token, not merely long: lower case, upper
/// case and a digit all in it; or a base64 marker (`+`, `/`, `=` padding) in
/// text with both cases; or pure hex of 32 or more with a letter. It must be
/// `min` characters or more, hex excepted. All-caps business codes, lower
/// case slugs, GUIDs, numbers and dates are not token-shaped.
fn token_shaped(text: &str, min: usize) -> bool {
    let n = text.chars().count();
    if n >= HEX_MIN
        && text.chars().all(|c| c.is_ascii_hexdigit())
        && text.chars().any(|c| c.is_ascii_alphabetic())
    {
        return true;
    }
    if n < min {
        return false;
    }
    let upper = text.chars().any(|c| c.is_ascii_uppercase());
    let lower = text.chars().any(|c| c.is_ascii_lowercase());
    let digit = text.chars().any(|c| c.is_ascii_digit());
    let marker = text.contains('+') || text.contains('/') || text.ends_with('=');
    (upper && lower && digit) || (marker && upper && lower)
}

/// `token_shaped` for one decoded path segment or value. Dots separate names
/// (`Company.Module.SomeLongTypeName2026`), so each part is judged alone. A
/// value that starts with `/` is an address (an encoded `ReturnUrl`): each
/// of its segments is judged alone.
fn token_segment(decoded: &str, min: usize) -> bool {
    let judge = |part: &str| part.split('.').any(|p| token_shaped(p, min));
    if decoded.starts_with('/') {
        decoded.split('/').any(judge)
    } else {
        judge(decoded)
    }
}

/// `run` (a run of token characters, 32 or more) with the token-shaped parts
/// hidden. After a short `name=` the rest is the value. The rest is split on
/// `/` and each segment is percent-decoded (tokens are written with `%2F` and
/// `%3D`) and judged alone: paths of short words and numbers stay, a token
/// segment is hidden. A token a path split into short pieces (with `%2F`, or
/// literal slashes beside a `+` or `=` padding) is caught by judging the
/// decoded whole. Text with spaces is not a token.
fn hide_run(run: &str) -> String {
    let (prefix, rest) = match run.find('=') {
        Some(i)
            if i > 0
                && i <= 32
                && i + 1 < run.len()
                && run[..i].chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')) =>
        {
            (&run[..=i], &run[i + 1..])
        }
        _ => ("", run),
    };
    let decode = |t: &str| String::from_utf8_lossy(&urlencoding::decode_binary(t.as_bytes())).into_owned();
    let plain = decode(rest);
    if plain.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return run.to_string();
    }
    let mut changed = false;
    let shown: Vec<String> = rest
        .split('/')
        .map(|seg| {
            if token_segment(&decode(seg), OPAQUE_MIN) {
                changed = true;
                HIDDEN.to_string()
            } else {
                seg.to_string()
            }
        })
        .collect();
    if changed {
        return format!("{prefix}{}", shown.join("/"));
    }
    let split_marker = rest.to_ascii_lowercase().contains("%2f") || plain.contains('+') || plain.ends_with('=');
    if split_marker && !plain.starts_with('/') && token_shaped(&plain, OPAQUE_MIN) {
        return format!("{prefix}{HIDDEN}");
    }
    run.to_string()
}

/// Hides every opaque token (`hide_run`) in `text`: an echoed session cookie,
/// a reset token in a link, a token under an innocent name.
fn hide_long_runs(text: &str) -> String {
    LONG_RUN.replace_all(text, |c: &Captures| hide_run(&c[0])).into_owned()
}

/// A cookie or session label, then within 80 characters an opaque value of 16
/// or more: `ASP.NET_SessionId=5kbsz4dpbtzs1x4aeuvq2o4t`. Shorter than a
/// token the long-run rule catches, so the label gives it away.
static COOKIE_VALUE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(ASP\.NET_SessionId|\.ASPXAUTH|\.AspNetCore\.[\w.\-]*|Set-Cookie|\bCookie\s*:)([^\r\n]{0,80}?)([A-Za-z0-9][A-Za-z0-9_\-+/=.%]{15,})",
    )
    .unwrap()
});

fn hide_cookie_values(text: &str) -> String {
    COOKIE_VALUE
        .replace_all(text, |c: &Captures| {
            if opaque(&c[3]) {
                format!("{}{}{HIDDEN}", &c[1], &c[2])
            } else {
                c[0].to_string()
            }
        })
        .into_owned()
}

fn hide_bearer(text: &str) -> String {
    BEARER_VALUE.replace_all(text, format!("Bearer {REDACTED}").as_str()).into_owned()
}

/// An `otpauth://` URI (what an authenticator's QR code holds: the shared
/// key, the account, the issuer), raw, JSON-escaped (`\/`) or
/// percent-encoded - the encoded one ends at the next `&`.
static OTPAUTH_URI: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)otpauth:(?:\\/|[^\s"'<>\\])*|otpauth%3a[^\s"'<>\\&]*"#).unwrap()
});

/// A run that may be a base32 key (A-Z or a-z, and 2-7): whole, or in
/// space-separated groups of 4 as an authenticator page shows it, the last
/// group shorter. `base32_key` judges it.
static BASE32_RUN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"\b(?:[A-Z2-7]{4}(?: [A-Z2-7]{4})+(?: [A-Z2-7]{1,3})?",
        r"|[a-z2-7]{4}(?: [a-z2-7]{4})+(?: [a-z2-7]{1,3})?",
        r"|[A-Z2-7]{16,}|[a-z2-7]{16,})\b",
    ))
    .unwrap()
});

/// A base32 key is this long, its groups joined.
const BASE32_MIN: usize = 16;

/// Whether `run` is an authenticator's base32 key: 16 or more characters
/// with the groups joined, at least 2 digits (a word has none) and more
/// letters than digits, and no group of 4 all digits (a phone number
/// written in groups). An all-caps word, a sentence and a business code
/// with `0`, `1`, `8` or `9` in it are not.
fn base32_key(run: &str) -> bool {
    if run.split(' ').any(|g| g.len() == 4 && g.chars().all(|c| c.is_ascii_digit())) {
        return false;
    }
    let joined: String = run.chars().filter(|c| *c != ' ').collect();
    let digits = joined.chars().filter(|c| c.is_ascii_digit()).count();
    joined.len() >= BASE32_MIN && digits >= 2 && joined.len() - digits > digits
}

/// Hides `otpauth://` URIs whole and base32 keys (`base32_key`).
fn hide_authenticator_keys(text: &str) -> String {
    let text = OTPAUTH_URI.replace_all(text, REDACTED);
    BASE32_RUN
        .replace_all(&text, |c: &Captures| if base32_key(&c[0]) { REDACTED.to_string() } else { c[0].to_string() })
        .into_owned()
}

/// What every redacted text ends with: bearer values, JWTs, long opaque
/// runs, `otpauth://` URIs and base32 keys hidden wherever they sit.
fn finish(text: String) -> String {
    hide_authenticator_keys(&hide_long_runs(&hide_cookie_values(&hide_jwts(&hide_bearer(&text)))))
}

/// A `code` value that is a token and not a business code: token-shaped
/// (`token_shaped`) and 20 or more token characters (a password-reset code).
fn opaque_code(value: &str) -> bool {
    value.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+' | '/' | '=' | '.' | '%'))
        && value.chars().count() >= 20
        && token_shaped(value, 20)
}

/// A percent-decoded component (`+` a space); bytes that are not UTF-8 are
/// replaced, so a name cannot hide a word behind them.
fn decoded(component: &str) -> String {
    let spaced = component.replace('+', " ");
    String::from_utf8_lossy(&urlencoding::decode_binary(spaced.as_bytes())).into_owned()
}

fn plain_looking(head: &str) -> bool {
    !head.chars().any(|c| c.is_whitespace() || matches!(c, '"' | '{' | '}' | '<' | '>'))
}

/// `...?a=1`, `/cb#access_token=x`: an address (or the tail of one) with a
/// query or a fragment carrying fields.
fn address_like(value: &str) -> bool {
    let Some(at) = value.find(['?', '#']) else { return false };
    plain_looking(&value[..at]) && value[at + 1..].contains('=')
}

/// `a=1&b=2`, in one line, every field a name and a value.
fn looks_like_form(text: &str) -> bool {
    let t = text.trim();
    t.contains('=')
        && !t.contains('\n')
        && t.split('&').all(|seg| {
            seg.is_empty() || seg.split_once('=').is_some_and(|(k, _)| !k.is_empty() && plain_looking(k) && !k.contains('\''))
        })
}

/// A value found inside another (a form field's, a query parameter's, a
/// JSON string's) with the secrets in it hidden: JSON redacted by key, an
/// address with a query or fragment redacted by parameter, a form redacted
/// by field, other text by its fields and members. `None` when it holds
/// nothing to hide.
fn redact_text_value(value: &str, depth: u8) -> Option<String> {
    let value = value.trim_start_matches('\u{feff}');
    let trimmed = value.trim();
    let json = (trimmed.starts_with('{') || trimmed.starts_with('['))
        .then(|| serde_json::from_str::<Value>(trimmed).ok())
        .flatten();
    let address = address_like(value);
    let form = !address && looks_like_form(value);
    if depth >= MAX_NESTING && (json.is_some() || address || form) {
        return Some(REDACTED.to_string());
    }
    let hidden = if let Some(v) = &json {
        let hidden = redact_value(v, depth + 1);
        if hidden == *v {
            return None;
        }
        serde_json::to_string(&hidden).unwrap_or_default()
    } else if address {
        path_query_at(value, depth + 1)
    } else if form {
        form_at(value, depth + 1)
    } else if value.contains(['=', ':', '"', '<']) {
        page_text(value)
    } else {
        return None;
    };
    (hidden != value).then_some(hidden)
}

/// `v` with every value under a secret key hidden, at any depth; a string
/// holding JSON, an address, a form or members is redacted inside. In an
/// OAuth object (`code` beside `state`) the code goes too.
fn redact_value(v: &Value, depth: u8) -> Value {
    match v {
        Value::Object(map) => {
            let has = |name: &str| map.keys().any(|k| k.eq_ignore_ascii_case(name));
            let oauth = has("state") && has("code");
            Value::Object(
                map.iter()
                    .map(|(k, x)| {
                        let code = k.eq_ignore_ascii_case("code");
                        let hide = secret_name(k)
                            || (oauth && code)
                            || (code && x.as_str().is_some_and(opaque_code));
                        let x = if hide { Value::String(REDACTED.to_string()) } else { redact_value(x, depth) };
                        (k.clone(), x)
                    })
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.iter().map(|i| redact_value(i, depth)).collect()),
        Value::String(s) => match redact_text_value(s, depth) {
            Some(hidden) => Value::String(hidden),
            None => v.clone(),
        },
        other => other.clone(),
    }
}

/// Fields `a=1&b=2` with a secret field's value hidden and a value that
/// holds a secret inside redacted. Every other field is left as written. In
/// an OAuth form (`code` beside `state`) the code is a secret too.
fn form_at(text: &str, depth: u8) -> String {
    let name_of = |pair: &str| decoded(pair.split_once('=').map_or("", |(n, _)| n)).to_lowercase();
    let names: Vec<String> = text.split('&').map(name_of).collect();
    let oauth = names.iter().any(|n| n == "state") && names.iter().any(|n| n == "code");
    text.split('&')
        .zip(&names)
        .map(|(pair, name)| field_at(pair, depth, oauth && name == "code"))
        .collect::<Vec<_>>()
        .join("&")
}

fn field_at(pair: &str, depth: u8, force: bool) -> String {
    let Some((name, value)) = pair.split_once('=') else { return pair.to_string() };
    let plain_value = decoded(value);
    let opaque_code_value = decoded(name).eq_ignore_ascii_case("code") && opaque_code(&plain_value);
    if force || opaque_code_value || secret_name(&decoded(name)) {
        return format!("{name}={REDACTED}");
    }
    match redact_text_value(&plain_value, depth) {
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
/// members redacted inside. The path stays as it is - except a token in it.
pub fn redact_path_query(pq: &str) -> String {
    finish(path_query_at(pq, 0))
}

/// One field (a HAR `params` entry, say): its value hidden when `name` is a
/// secret's, otherwise redacted inside.
pub fn redact_field(name: &str, value: &str) -> String {
    if secret_name(name) {
        return REDACTED.to_string();
    }
    finish(redact_text_value(value, 0).unwrap_or_else(|| value.to_string()))
}

/// A header's value: hidden when the name is a secret's, otherwise redacted
/// as any text (an address in it loses its secret parameters).
pub fn redact_header(name: &str, value: &str) -> String {
    if secret_name(name) || name.eq_ignore_ascii_case("authorization") {
        return HIDDEN.to_string();
    }
    redact_text(value)
}

/// Free text (a failure, a tab name, an id) with the secrets in it hidden.
pub fn redact_text(text: &str) -> String {
    finish(redact_text_value(text, 0).unwrap_or_else(|| text.to_string()))
}

/// JSON text with every secret key's value hidden, at any depth: `None` when
/// `body` does not parse. Left exactly as written when there is nothing to
/// hide.
fn json_text(body: &str) -> Option<String> {
    let v = serde_json::from_str::<Value>(body).ok()?;
    let hidden = redact_value(&v, 0);
    Some(if hidden == v { body.to_string() } else { serde_json::to_string(&hidden).unwrap_or_default() })
}

/// Elements named like a secret (`<password>x</password>`) with their
/// content hidden.
fn hide_elements(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    while let Some(open) = OPEN_TAG.captures_at(text, pos) {
        let (Some(whole), Some(name)) = (open.get(0), open.get(1)) else { break };
        out.push_str(&text[pos..whole.end()]);
        pos = whole.end();
        if secret_name(name.as_str()) {
            if let Some(rel) = text[pos..].find(&format!("</{}", name.as_str())) {
                out.push_str(REDACTED);
                pos += rel;
            }
        }
    }
    out.push_str(&text[pos..]);
    out
}

/// A page, a script, XML, a header block or any other text: anti-forgery
/// tokens, the value of a secret field in a form, a meta tag, an element, a
/// `name: value` line or an assignment (`apiKey = '...'`, GraphQL's
/// `password:"x"`), and JSON members inside it.
fn page_text(body: &str) -> String {
    let text = scrub_tokens(&hide_bearer(body), None);
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
    let text = hide_elements(&text);
    let text = AUTH_HEADER.replace_all(&text, format!("${{1}}{REDACTED}").as_str()).into_owned();
    let text = LINE_FIELD
        .replace_all(&text, |c: &Captures| {
            if secret_name(&c[2]) && !c[4].is_empty() {
                format!("{}{}{}{REDACTED}", &c[1], &c[2], &c[3])
            } else {
                c[0].to_string()
            }
        })
        .into_owned();
    let text = hide_assignments(&text, 0);
    hide_members_by(&text, &ANY_MEMBER, secret_name)
}

/// Assignments (`name = value`) whose name is a secret's have the value
/// hidden. A quoted value of any other name is looked into: an attribute
/// `href="/cb#id_token=x"` holds one.
fn hide_assignments(text: &str, depth: u8) -> String {
    ASSIGN
        .replace_all(text, |c: &Captures| {
            let value = &c[3];
            if !secret_name(&c[1]) {
                let quote = value.chars().next().filter(|q| matches!(q, '"' | '\''));
                return match quote {
                    Some(q) if depth < MAX_NESTING && value.len() > 2 && value.ends_with(q) => {
                        let inner = &value[1..value.len() - 1];
                        format!("{}{}{q}{}{q}", &c[1], &c[2], hide_assignments(inner, depth + 1))
                    }
                    _ => c[0].to_string(),
                };
            }
            let shown = if value.starts_with("\\\"") {
                format!("\\\"{REDACTED}\\\"")
            } else if value.starts_with('"') {
                format!("\"{REDACTED}\"")
            } else if value.starts_with('\'') {
                format!("'{REDACTED}'")
            } else {
                REDACTED.to_string()
            };
            format!("{}{}{shown}", &c[1], &c[2])
        })
        .into_owned()
}

/// The name of a multipart part, from its `Content-Disposition` line.
fn part_name(line: &str) -> Option<String> {
    if let Some(n) = PART_NAME_EXT.captures(line) {
        return Some(decoded(&n[1]));
    }
    PART_NAME.captures(line).map(|n| n[1].to_string())
}

#[derive(Clone, Copy)]
enum At {
    Preamble,
    Headers { secret: bool },
    Content { secret: bool, hidden: bool },
    Epilogue,
}

/// A multipart body with the content of every part named like a secret
/// replaced by `[redacted]` (the headers and the delimiters stay), and all
/// other text - the other parts, the preamble and the epilogue - redacted as
/// any text. The boundary is the mime's `boundary=` or, failing that, the
/// body's first delimiter line; a delimiter may carry trailing whitespace.
/// `None` when the body is not multipart: no delimiter line, and neither a
/// multipart mime nor a `Content-Disposition` line.
fn multipart_text(mime: &str, body: &str) -> Option<String> {
    let from_mime = mime.to_ascii_lowercase().find("boundary=").map(|at| {
        mime[at + "boundary=".len()..].split(';').next().unwrap_or("").trim().trim_matches('"').to_string()
    });
    let from_mime = from_mime.filter(|b| !b.is_empty());
    let boundary = from_mime.clone().or_else(|| {
        body.lines().find_map(|l| l.trim_end().strip_prefix("--").filter(|b| !b.is_empty()).map(str::to_string))
    })?;
    if from_mime.is_none() && !body.to_ascii_lowercase().contains("content-disposition") {
        return None;
    }
    let open = format!("--{boundary}");
    let close = format!("--{boundary}--");
    if !body.lines().any(|l| l.trim_end() == open || l.trim_end() == close) {
        return None;
    }

    let mut at = At::Preamble;
    let mut out = String::with_capacity(body.len());
    for line in body.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\r', '\n']);
        let delimiter = bare.trim_end();
        if delimiter == open || delimiter == close {
            out.push_str(line);
            at = if delimiter == close { At::Epilogue } else { At::Headers { secret: false } };
            continue;
        }
        at = match at {
            At::Preamble | At::Epilogue => {
                out.push_str(&page_text(line));
                at
            }
            At::Headers { secret } => {
                out.push_str(line);
                if bare.is_empty() {
                    At::Content { secret, hidden: false }
                } else {
                    let named = part_name(bare).is_some_and(|n| secret_name(&n));
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

/// `body` with every secret hidden, whatever its length. The body is judged
/// by what it is, whatever `mime` says: JSON by key, a form by field,
/// multipart by part name, anything else as text (pages, scripts, header
/// blocks, `name: value` lines, assignments). A JWT, a bearer value or a long
/// opaque run is hidden wherever it sits. Nothing is cut, and an address
/// inside the body is kept (a secret query parameter in it is hidden).
/// `mime` only helps find a multipart boundary.
pub fn redact_body(mime: Option<&str>, body: &str) -> String {
    let body = body.trim_start_matches('\u{feff}');
    let trimmed = body.trim();
    let json = (trimmed.starts_with('{') || trimmed.starts_with('[')).then(|| json_text(body)).flatten();
    let hidden = if let Some(json) = json {
        json
    } else if let Some(parts) = multipart_text(mime.unwrap_or(""), body) {
        parts
    } else if looks_like_form(trimmed) {
        form_at(body, 0)
    } else {
        page_text(body)
    };
    finish(hidden)
}
