//! The `specs` list: where the cases' specifications live - markdown files
//! (absolute, or relative to the JSON file) and Azure DevOps wiki URLs.
//! Read for the review page's spec pane; written by the Import Test Cases tab's
//! Attach control. Same read-patch-write discipline as `comments.rs`: the
//! app owns this key, and nothing else in the file.
//!
//! What an entry may be is one rule, `check_spec`, and every reader and
//! writer of the list goes through it: a `.md` (or `.markdown`) file, or an
//! `https://` Azure DevOps wiki link. Anything else (source code such as
//! `.cshtml`, text, PDFs, an ordinary web page) is refused with one sentence,
//! so whoever tried, the person or an assistant, is told why.

use serde_json::Value;

/// The sentence a refused entry gets, wherever it was refused.
fn refusal(entry: &str) -> String {
    format!("{entry} cannot be a spec - only .md files and Azure DevOps wiki links can be added")
}

/// Whether `entry` may be listed in `specs`. Ok for a `.md` or `.markdown`
/// file (the extension in any case, the path absolute or relative), or for
/// an `https://` URL on dev.azure.com or a *.visualstudio.com host whose
/// path holds `/_wiki/wikis/`. Err with the sentence for anything else.
pub fn check_spec(entry: &str) -> Result<(), String> {
    let e = entry.trim();
    let allowed = if e.contains("://") { is_wiki_link(e) } else { is_markdown_file(e) };
    if allowed {
        Ok(())
    } else {
        Err(refusal(e))
    }
}

/// Every entry of a list through `check_spec`: the first refusal, if any.
pub fn check_specs(entries: &[String]) -> Result<(), String> {
    entries.iter().try_for_each(|e| check_spec(e))
}

/// The file name's extension, read the same on every platform: a path
/// written on Windows uses `\`, and `Path` on another OS would not split it.
fn is_markdown_file(path: &str) -> bool {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => {
            ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown")
        }
        _ => false,
    }
}

/// An `https://` Azure DevOps wiki page link. Plain http is refused: the
/// page is fetched with the person's token, which never goes out unencrypted.
fn is_wiki_link(url: &str) -> bool {
    let Some(rest) = url.get(..8).filter(|s| s.eq_ignore_ascii_case("https://")).map(|_| &url[8..]) else {
        return false;
    };
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    // No credentials in a link, and the port is not part of the host name.
    if authority.contains('@') {
        return false;
    }
    let host = authority.split(':').next().unwrap_or("").to_ascii_lowercase();
    let azure = host == "dev.azure.com"
        || host.strip_suffix(".visualstudio.com").is_some_and(|org| !org.is_empty() && !org.contains('.'));
    // The path alone: a query string naming `/_wiki/wikis/` does not make
    // some other page a wiki page.
    let path = path.split(['?', '#']).next().unwrap_or("");
    azure && format!("/{path}").contains("/_wiki/wikis/")
}

/// The `specs` list as read from a document.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SpecsRead {
    /// The entries `check_spec` allows, in order.
    pub kept: Vec<String>,
    /// How many entries were not non-blank strings.
    pub ignored: usize,
    /// The entries `check_spec` refused, as written (trimmed), in order.
    pub refused: Vec<String>,
}

/// The `specs` list from an already-parsed document. A document with no
/// `specs` key, or one that is not an array, simply has none and nothing
/// ignored or refused.
pub fn specs_from_value(doc: &Value) -> SpecsRead {
    let entries = doc.get("specs").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut out = SpecsRead::default();
    for v in &entries {
        match v.as_str().map(str::trim).filter(|s| !s.is_empty()) {
            Some(s) if check_spec(s).is_ok() => out.kept.push(s.to_string()),
            Some(s) => out.refused.push(s.to_string()),
            None => out.ignored += 1,
        }
    }
    out
}

fn read(json: &str) -> SpecsRead {
    let doc = serde_json::from_str::<Value>(super::strip_bom(json)).unwrap_or(Value::Null);
    specs_from_value(&doc)
}

/// The list as written, in order; entries that are not non-blank strings,
/// or that `check_spec` refuses, are dropped (the importer warns about
/// both). A file that is not an object, or does not parse, simply has none.
pub fn read_specs(json: &str) -> Vec<String> {
    read(json).kept
}

/// How many entries `read_specs` would drop for not being non-blank
/// strings - for the importer's warning.
pub fn ignored_spec_entries(json: &str) -> usize {
    read(json).ignored
}

/// The entries `read_specs` would drop because `check_spec` refuses them.
pub fn refused_spec_entries(json: &str) -> Vec<String> {
    read(json).refused
}

/// The importer's warning for one refused entry.
pub fn refused_warning(entry: &str) -> String {
    format!("specs: {}", refusal(entry.trim()))
}

/// Replace the list, keeping every other key where it was. An empty list
/// removes the key: a file with no specs should not say `"specs": []`.
/// Refused outright, writing nothing, when any entry is not allowed.
pub fn patch_specs(json: &str, specs: &[String]) -> Result<String, String> {
    check_specs(specs)?;
    let mut doc = super::comments::document(json)?;
    let obj = doc.as_object_mut().ok_or(
        "this file is a bare list of test cases, so there is nowhere to put its specs - \
         re-export it from the app to get the full format",
    )?;
    if specs.is_empty() {
        obj.remove("specs");
    } else {
        obj.insert("specs".into(), Value::Array(specs.iter().map(|s| Value::String(s.clone())).collect()));
    }
    super::comments::render(&doc)
}

/// A draft's text with every refused `specs` entry taken out, for a tool
/// that writes a file back. The text comes back untouched when nothing was
/// refused, so a clean file is never reformatted by this.
pub fn without_refused_specs(json: &str) -> Result<String, String> {
    let r = read(json);
    if r.refused.is_empty() {
        return Ok(json.to_string());
    }
    patch_specs(json, &r.kept)
}
