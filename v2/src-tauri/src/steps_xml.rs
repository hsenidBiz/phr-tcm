//! Microsoft.VSTS.TCM.Steps XML build/parse, ported from v1
//! app/utils/xml_builder.py against tests/test_xml_builder.py as golden
//! vectors. Semantics preserved exactly:
//! - step ids start at 2; `last` == last step id
//! - empty steps list builds the single placeholder step
//! - parse strips anything tag-shaped (a literal "<placeholder>" typed by a
//!   user does not survive a round-trip - same as v1)
//! - malformed XML parses to an empty list, never an error

use quick_xml::events::Event;
use quick_xml::Reader;
use serde::{Deserialize, Serialize};

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

/// Replace every `<...>` run with a space (v1 strips tags after unescaping,
/// so even user-typed angle-bracket text is removed).
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
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
