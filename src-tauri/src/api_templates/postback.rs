//! WebForms postback steps: the pieces of the wire contract with the page
//! that need no browser. This file holds the HTML capture syntax, the
//! selector-or-text rule for `expect.html` entries and the key a carried
//! page is filed under.

use crate::autorun::nav::path_of;
use serde::Serialize;

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
