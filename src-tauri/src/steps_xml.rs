//! Microsoft.VSTS.TCM.Steps XML build/parse, ported from v1
//! app/utils/xml_builder.py against tests/test_xml_builder.py as golden
//! vectors. Semantics preserved exactly:
//! - step ids start at 2; `last` == last step id
//! - empty steps list builds the single placeholder step
//! - parse strips REAL HTML markup only. v1 removed every `<...>` run, so a
//!   literal "<cycleId>" a user typed did not survive a round-trip; that was
//!   silent data loss, and `strip_tags` explains what replaced it.
//! - malformed XML parses to an empty list, never an error

use quick_xml::events::Event;
use quick_xml::Reader;
use serde::{Deserialize, Serialize};

/// Tag names Azure DevOps' rich-text editor actually emits into a step.
///
/// The list is the whole trick. ADO stores each step's HTML *escaped*
/// inside `parameterizedString`, so by the time it has been unescaped, real
/// markup (`<P>`, `<BR/>`) and text somebody typed (`<cycleId>`) look
/// exactly alike - there is no structural difference left to use. Matching
/// against what the editor can actually produce is the only thing that
/// separates them.
const HTML_TAGS: [&str; 42] = [
    "a", "b", "big", "blockquote", "br", "caption", "center", "code", "col", "colgroup", "dd",
    "div", "dl", "dt", "em", "font", "h1", "h2", "h3", "h4", "h5", "h6", "hr", "i", "img", "li",
    "ol", "p", "pre", "s", "small", "span", "strike", "strong", "sub", "sup", "table", "tbody",
    "td", "th", "thead", "tr",
];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Step {
    pub action: String,
    pub expected: String,
}

fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Build the XML string for the Microsoft.VSTS.TCM.Steps field.
pub fn build_steps_xml(steps: &[Step]) -> String {
    if steps.is_empty() {
        return "<steps id=\"0\" last=\"1\"><step id=\"2\" type=\"ActionStep\"><parameterizedString isformatted=\"true\"></parameterizedString><parameterizedString isformatted=\"true\"></parameterizedString></step></steps>".to_string();
    }
    let mut out = format!("<steps id=\"0\" last=\"{}\">", steps.len() + 1);
    for (i, step) in steps.iter().enumerate() {
        out.push_str(&format!(
            "<step id=\"{}\" type=\"ActionStep\"><parameterizedString isformatted=\"true\">{}</parameterizedString><parameterizedString isformatted=\"true\">{}</parameterizedString></step>",
            i + 2,
            escape_xml(&step.action),
            escape_xml(&step.expected),
        ));
    }
    out.push_str("</steps>");
    out
}

/// The index of the `>` that closes a real HTML tag opening at `open`, or
/// `None` when this `<` is just a less-than sign.
fn html_tag_end(c: &[char], open: usize) -> Option<usize> {
    let mut i = open + 1;
    if i < c.len() && c[i] == '/' {
        i += 1;
    }
    let name_start = i;
    while i < c.len() && c[i].is_ascii_alphanumeric() {
        i += 1;
    }
    // "a < b" and "start_date < GETUTCDATE()": no name, so no tag.
    if i == name_start {
        return None;
    }
    let name: String = c[name_start..i].iter().collect::<String>().to_ascii_lowercase();
    if !HTML_TAGS.contains(&name.as_str()) {
        return None;
    }
    // Attributes may quote a '>' (`<img alt="a>b">`), so track quoting
    // rather than scanning for the first '>'.
    let mut quote: Option<char> = None;
    while i < c.len() {
        match (quote, c[i]) {
            (Some(q), ch) if ch == q => quote = None,
            (Some(_), _) => {}
            (None, ch @ ('"' | '\'')) => quote = Some(ch),
            (None, '>') => return Some(i),
            // An unterminated `<p` followed by another `<` was never a tag.
            (None, '<') => return None,
            (None, _) => {}
        }
        i += 1;
    }
    None
}

/// Remove real HTML markup, and only that.
///
/// This used to delete every `<...>` run, which is what v1 did and what
/// this module's doc comment used to promise. It cost a developer 62
/// fragments across 20 test cases: SQL steps written as
/// `WHERE performance_cycle_id = <cycleId>` came back as
/// `WHERE performance_cycle_id =` - a query the tester cannot run, in a
/// step whose entire purpose is to run it. Silently, with the file still
/// valid and the text still reading plausibly enough to skim past.
///
/// So `<cycleId>`, `<next assessment stage>` and `a < b` now survive, while
/// `<P>` and `<BR/>` still go. A spec quote naming a real HTML element
/// (`<div>`, `<img>`, and note `<textarea>` is NOT in the list precisely
/// because a rich-text editor never emits one) is the residual ambiguity -
/// the app cannot tell that from markup, and `validate_cases` warns about
/// it instead.
fn strip_tags(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            if let Some(end) = html_tag_end(&chars, i) {
                out.push(' ');
                i = end + 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Whether `text` contains something `strip_tags` will treat as markup.
/// Used by `validate_cases` so an author quoting `<div>` from a spec finds
/// out before the round trip eats it, rather than during review.
pub fn contains_html_markup(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    (0..chars.len()).any(|i| chars[i] == '<' && html_tag_end(&chars, i).is_some())
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Parse the Steps XML into Step structs. Returns [] on empty or malformed
/// input, mirroring v1 parse_steps_xml.
pub fn parse_steps_xml(xml_str: &str) -> Vec<Step> {
    if xml_str.trim().is_empty() {
        return vec![];
    }
    let mut reader = Reader::from_str(xml_str);
    reader.config_mut().trim_text(false);

    let mut steps: Vec<Vec<String>> = vec![];
    let mut capture: Option<(String, usize)> = None; // (buffer, nested depth)

    loop {
        match reader.read_event() {
            Err(_) => return vec![], // malformed
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                let name = e.name().as_ref().to_vec();
                if let Some((buf, depth)) = capture.as_mut() {
                    *depth += 1;
                    buf.push(' ');
                    let _ = name;
                } else if name == b"step" {
                    steps.push(vec![]);
                } else if name == b"parameterizedString" && !steps.is_empty() {
                    capture = Some((String::new(), 0));
                }
            }
            Ok(Event::End(e)) => {
                if let Some((buf, depth)) = capture.as_mut() {
                    if *depth == 0 && e.name().as_ref() == b"parameterizedString" {
                        let text = collapse_ws(&strip_tags(buf));
                        if let Some(parts) = steps.last_mut() {
                            parts.push(text);
                        }
                        capture = None;
                    } else {
                        *depth = depth.saturating_sub(1);
                        buf.push(' ');
                    }
                }
            }
            Ok(Event::Empty(_)) => {
                if let Some((buf, _)) = capture.as_mut() {
                    buf.push(' ');
                }
            }
            Ok(Event::Text(t)) => {
                if let Some((buf, _)) = capture.as_mut() {
                    match t.decode() {
                        Ok(s) => buf.push_str(&s),
                        Err(_) => return vec![],
                    }
                }
            }
            // quick-xml surfaces &amp; / &lt; / &#... as separate events.
            // Resolving them here gives one unescape pass (like v1's
            // html.unescape); strip_tags then removes anything tag-shaped.
            Ok(Event::GeneralRef(r)) => {
                if let Some((buf, _)) = capture.as_mut() {
                    let name = String::from_utf8_lossy(r.as_ref()).to_string();
                    buf.push_str(&unescape_html(&format!("&{name};")));
                }
            }
            Ok(_) => {}
        }
    }
    // A dangling capture means the XML was truncated mid-element.
    if capture.is_some() {
        return vec![];
    }

    steps
        .into_iter()
        .map(|parts| Step {
            action: parts.first().cloned().unwrap_or_default(),
            expected: parts.get(1).cloned().unwrap_or_default(),
        })
        .collect()
}

/// The step ids in document order from a Steps XML blob. ADO assigns
/// arbitrary ids (not 2,3,4...) once a case has been edited in the web UI,
/// and iterationDetails must reference the REAL ids - never index math.
pub fn parse_step_ids(xml_str: &str) -> Vec<String> {
    if xml_str.trim().is_empty() {
        return vec![];
    }
    let mut reader = Reader::from_str(xml_str);
    let mut ids = vec![];
    loop {
        match reader.read_event() {
            Err(_) => return vec![],
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                if e.name().as_ref() == b"step" {
                    if let Some(id) = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == b"id")
                        .and_then(|a| String::from_utf8(a.value.to_vec()).ok())
                    {
                        ids.push(id);
                    }
                }
            }
            Ok(_) => {}
        }
    }
    ids
}

/// Flatten an ADO rich-text/HTML field to clean plain text, ported from v1
/// html_to_text: list items become bullets, block ends become line breaks,
/// all tags stripped, entities unescaped, blank runs collapsed.
pub fn html_to_text(html: &str) -> String {
    if html.trim().is_empty() {
        return String::new();
    }
    let li = regex::Regex::new(r"(?i)<\s*li[^>]*>").unwrap();
    let block = regex::Regex::new(r"(?i)<\s*(br|/p|/div|/h[1-6]|/tr)\s*/?>").unwrap();
    let tag = regex::Regex::new(r"<[^>]+>").unwrap();

    let s = li.replace_all(html, "\n\u{2022} ");
    let s = block.replace_all(&s, "\n");
    let s = tag.replace_all(&s, "");
    let s = unescape_html(&s).replace('\u{a0}', " ");

    let mut lines: Vec<String> = vec![];
    let mut blank = false;
    for ln in s.split('\n') {
        let ln = ln.trim();
        if !ln.is_empty() {
            lines.push(ln.to_string());
            blank = false;
        } else if !lines.is_empty() && !blank {
            lines.push(String::new());
            blank = true;
        }
    }
    lines.join("\n").trim().to_string()
}

/// Minimal HTML entity unescape covering what ADO fields actually contain:
/// the five XML entities, &nbsp;, and numeric references.
fn unescape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        rest = &rest[pos..];
        let end = match rest.find(';') {
            Some(e) if e <= 12 => e,
            _ => {
                out.push('&');
                rest = &rest[1..];
                continue;
            }
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            _ if entity.starts_with('#') => {
                let num = entity.trim_start_matches('#');
                let cp = if let Some(hex) = num.strip_prefix(['x', 'X']) {
                    u32::from_str_radix(hex, 16).ok()
                } else {
                    num.parse().ok()
                };
                cp.and_then(char::from_u32)
            }
            _ => None,
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}
