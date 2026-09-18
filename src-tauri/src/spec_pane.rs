//! The review page's spec pane: each entry of a draft file's `specs` list
//! becomes one rendered document. A file on disk is read and rendered with
//! the same sanitised markdown the notes use; a wiki page is fetched with
//! the signed-in user's token and cached, then rendered the same way. Every
//! failure is a document with an `error` - the page shows the message in
//! that tab and the rest of the review is unaffected.

use std::path::{Path, PathBuf};

use crate::ado::{AdoClient, AdoError};
use crate::import_parser::esc;

#[derive(Debug, Clone, PartialEq)]
pub enum SpecSource {
    File(PathBuf),
    Wiki(String),
}

/// Whether a `specs` entry names a wiki page rather than a file: an
/// `http://` or `https://` URL, after trimming. Shared by `SpecSource::
/// from_entry` and the render path's token-fetch gate, so the two never
/// drift apart on what counts as "needs a sign-in token".
pub fn is_wiki_entry(entry: &str) -> bool {
    let e = entry.trim_start();
    e.starts_with("http://") || e.starts_with("https://")
}

impl SpecSource {
    /// An `http(s)://` entry is a wiki page (a non-wiki URL still goes that
    /// way and fails with a clear message); anything else is a file, joined
    /// to `base_dir` when relative.
    pub fn from_entry(entry: &str, base_dir: &Path) -> SpecSource {
        let e = entry.trim();
        if is_wiki_entry(e) {
            return SpecSource::Wiki(e.to_string());
        }
        let p = Path::new(e);
        SpecSource::File(if p.is_absolute() { p.to_path_buf() } else { base_dir.join(p) })
    }

    fn key(&self) -> String {
        match self {
            SpecSource::File(p) => format!("file:{}", p.to_string_lossy().to_lowercase().replace('\\', "/")),
            SpecSource::Wiki(u) => format!("wiki:{}", u.trim_end_matches('/').to_lowercase()),
        }
    }
}

/// One tab of the pane.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct SpecDoc {
    pub title: String,
    /// "file" | "wiki"
    pub kind: String,
    /// The path or URL as resolved - what "Open in Azure DevOps" links to.
    pub source: String,
    pub html: String,
    pub error: Option<String>,
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.to_string_lossy().to_string())
}

/// The first `# ` heading's text, if the document opens with one.
fn first_heading(md: &str) -> Option<String> {
    md.lines()
        .map(str::trim)
        .find(|l| l.starts_with("# "))
        .map(|l| l.trim_start_matches('#').trim().to_string())
        .filter(|t| !t.is_empty())
}

pub fn render_file(path: &Path) -> SpecDoc {
    let name = file_name(path);
    let source = path.to_string_lossy().to_string();
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if ext != "md" && ext != "markdown" && ext != "txt" {
        return SpecDoc { title: name, kind: "file".into(), source, html: String::new(), error: Some("Not a text spec".into()) };
    }
    match std::fs::read_to_string(path) {
        Err(e) => SpecDoc {
            title: name.clone(),
            kind: "file".into(),
            source,
            html: String::new(),
            error: Some(format!("Could not read {name}: {}", e.kind())),
        },
        Ok(text) => {
            let text = text.strip_prefix('\u{feff}').unwrap_or(&text).to_string();
            if ext == "txt" {
                return SpecDoc { title: name, kind: "file".into(), source, html: format!("<pre>{}</pre>", esc(&text)), error: None };
            }
            SpecDoc {
                title: first_heading(&text).unwrap_or(name),
                kind: "file".into(),
                source,
                html: crate::markdown::to_html_standalone(&text),
                error: None,
            }
        }
    }
}

/// A wiki page's title from its path: the last segment, `-` read as space.
pub fn wiki_title(page_path: &str) -> String {
    page_path.trim_end_matches('/').rsplit('/').next().unwrap_or(page_path).replace('-', " ").trim().to_string()
}

/// Azure wiki markdown, made renderable off-line: the `[[_TOC_]]` macro
/// goes, and an image whose target is not an absolute URL (an attachment,
/// which needs the user's session the page cannot carry) becomes a link
/// to the page itself, labelled as the image.
pub fn prepare_wiki_markdown(content: &str, page_url: &str) -> String {
    let mut out = String::with_capacity(content.len());
    for line in content.lines() {
        if line.trim().eq_ignore_ascii_case("[[_TOC_]]") {
            continue;
        }
        let mut rest = line;
        let mut built = String::new();
        while let Some(start) = rest.find("![") {
            let after = &rest[start + 2..];
            // Malformed image (no `](`): stop rewriting here. `rest` still
            // holds the unconsumed text, including the `![` itself, and is
            // appended after the loop as plain text - left as written, never
            // dropped.
            let Some(close) = after.find("](") else { break };
            let alt = &after[..close];
            let tail = &after[close + 2..];
            // Malformed image (no closing `)`): same as above - stop here
            // and let `rest` (including the `![`) fall through unrewritten.
            let Some(end) = tail.find(')') else { break };
            let target = tail[..end].trim();
            built.push_str(&rest[..start]);
            if target.starts_with("http://") || target.starts_with("https://") {
                built.push_str(&rest[start..start + 2 + close + 2 + end + 1]);
            } else {
                built.push_str(&format!("[{alt} (image)]({page_url})"));
            }
            rest = &tail[end + 1..];
        }
        built.push_str(rest);
        out.push_str(&built);
        out.push('\n');
    }
    out
}

fn wiki_error(url: &str, reason: String) -> SpecDoc {
    SpecDoc { title: wiki_title(url), kind: "wiki".into(), source: url.to_string(), html: String::new(), error: Some(reason) }
}

pub async fn render_wiki(client: &AdoClient, url: &str) -> SpecDoc {
    let Some((org, project)) = crate::ado::endpoints::wiki_url_org_project(url) else {
        return wiki_error(url, "Could not fetch this wiki page: the link does not name an Azure DevOps wiki page.".into());
    };
    let key = crate::cache::keys::wiki_page(url);
    let page: Option<(String, String)> = crate::cache::fresh(&key, crate::cache::keys::WIKI_PAGE_TTL_MS);
    let (path, content) = match page {
        Some(p) => p,
        None => match client.get_wiki_page(&org, &project, "", url).await {
            Ok(p) => {
                crate::cache::put(&key, &(p.path.clone(), p.content.clone()));
                (p.path, p.content)
            }
            Err(e) => return wiki_error(url, format!("Could not fetch this wiki page: {}", user_sentence(&e))),
        },
    };
    SpecDoc {
        title: wiki_title(&path),
        kind: "wiki".into(),
        source: url.to_string(),
        html: crate::markdown::to_html_standalone(&prepare_wiki_markdown(&content, url)),
        error: None,
    }
}

/// The client's Display is already the user-facing sentence for transport
/// errors (see ado/transport.rs); the two status cases get their own words
/// so no case ever names a URL.
fn user_sentence(e: &AdoError) -> String {
    match e {
        AdoError::NotFound => "the page was not found, or you do not have access to it.".into(),
        AdoError::Unauthorized | AdoError::Forbidden => "Azure DevOps refused the request - sign in again.".into(),
        AdoError::RateLimited { .. } => "Azure DevOps is rate-limiting requests right now - try again shortly.".into(),
        AdoError::Http { .. } | AdoError::Network(_) => e.to_string(),
    }
}

/// Every `specs` entry of every file, paired with that file's directory
/// (relative entries resolve against it). Files with no specs add nothing.
pub fn spec_entries(files: &[crate::import_parser::DraftFile]) -> Vec<(String, PathBuf)> {
    files
        .iter()
        .flat_map(|f| {
            let base = Path::new(&f.path).parent().map(Path::to_path_buf).unwrap_or_default();
            f.specs.iter().map(move |s| (s.clone(), base.clone()))
        })
        .collect()
}

/// Every entry from every file, in order, rendered once each (the same
/// resolved source named by two files is one tab). `entries` pairs each
/// spec string with the directory of the file that named it.
pub async fn render_all(entries: &[(String, PathBuf)], client: Option<&AdoClient>) -> Vec<SpecDoc> {
    let mut seen = std::collections::HashSet::new();
    let mut docs = vec![];
    for (entry, base) in entries {
        let src = SpecSource::from_entry(entry, base);
        if !seen.insert(src.key()) {
            continue;
        }
        docs.push(match &src {
            SpecSource::File(p) => render_file(p),
            SpecSource::Wiki(u) => match client {
                Some(c) => render_wiki(c, u).await,
                None => wiki_error(u, "Could not fetch this wiki page: not signed in.".into()),
            },
        });
    }
    docs
}
