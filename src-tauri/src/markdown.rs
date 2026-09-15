//! Markdown for the pages this app opens in a real browser.
//!
//! Reviewer notes are written by an assistant and read by a person doing a
//! review, so they arrive full of the things markdown is for: headings,
//! links to a spec, quoted acceptance criteria, tables mapping a
//! requirement to a case. Rendering them as plain text would defeat the
//! point of the field.
//!
//! # Why the raw-HTML events are dropped rather than escaped
//!
//! CommonMark lets a document contain literal HTML, and the obvious
//! "sanitise it afterwards" approach means writing a filter and being
//! right about every tag and attribute forever. This does not do that.
//! `Event::Html` and `Event::InlineHtml` are simply never emitted into the
//! output, so an `<img onerror=...>` in a note cannot become an element -
//! it is not filtered, it is structurally absent. Escaping the source
//! first would be worse: it would also escape the markdown.
//!
//! Autolinks are left to the caller's stylesheet; every link is rendered
//! with `rel="noreferrer noopener"` because these pages are opened from a
//! temp file and a note's link is not necessarily one the reader trusts.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

/// Escape for HTML text content and attribute values.
fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Render CommonMark (plus tables, strikethrough and task lists) to HTML,
/// with any HTML in the SOURCE discarded.
///
/// Returns an empty string for empty input so a caller can decide not to
/// draw a panel at all.
pub fn to_html(src: &str) -> String {
    if src.trim().is_empty() {
        return String::new();
    }
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    // Deliberately NOT enabling footnotes or math: neither belongs in a
    // review note, and every option is more surface to be right about.

    let mut out = String::new();
    let mut in_code_block = false;
    // What each open link actually opened, so its End emits the matching
    // close. Getting this wrong leaves a `<span>` open to the end of the
    // block and the rest of the paragraph inherits the refused-link style.
    let mut links: Vec<LinkKind> = Vec::new();
    // Header cells are `<th>`, and only the parser knows which row is which.
    let mut in_table_head = false;

    for event in Parser::new_ext(src, opts) {
        match event {
            // The whole sanitisation story, in two arms.
            Event::Html(_) | Event::InlineHtml(_) => {}

            Event::Start(tag) => match tag {
                Tag::Paragraph => out.push_str("<p>"),
                Tag::Heading { level, .. } => {
                    // Notes sit inside a page that already has an <h1> and
                    // <h2>s of its own, so a note's own headings start at
                    // <h4> - the outline stays sane and a note cannot
                    // impersonate a section of the report.
                    out.push_str(&format!("<h{}>", heading_level(level as u8)));
                }
                Tag::BlockQuote(_) => out.push_str("<blockquote>"),
                Tag::CodeBlock(_) => {
                    in_code_block = true;
                    out.push_str("<pre><code>");
                }
                Tag::List(Some(start)) => out.push_str(&format!("<ol start=\"{start}\">")),
                Tag::List(None) => out.push_str("<ul>"),
                Tag::Item => out.push_str("<li>"),
                Tag::Emphasis => out.push_str("<em>"),
                Tag::Strong => out.push_str("<strong>"),
                Tag::Strikethrough => out.push_str("<del>"),
                Tag::Link { dest_url, title, .. } => {
                    let t = if title.is_empty() {
                        String::new()
                    } else {
                        format!(" title=\"{}\"", esc(&title))
                    };
                    // href is escaped, and a javascript: or data: URL is
                    // refused outright rather than rendered as a dead link
                    // that still looks clickable. `[text]()` has nothing to
                    // refuse, so it is neither - just the text.
                    if dest_url.trim().is_empty() {
                        links.push(LinkKind::Bare);
                    } else {
                        match safe_href(&dest_url) {
                            Some(href) => {
                                links.push(LinkKind::Anchor);
                                out.push_str(&format!(
                                    "<a href=\"{}\"{t} rel=\"noreferrer noopener\" target=\"_blank\">",
                                    esc(&href)
                                ));
                            }
                            None => {
                                links.push(LinkKind::Refused);
                                out.push_str("<span class=\"md-badlink\">");
                            }
                        }
                    }
                }
                // An image in a review note would be a remote fetch from a
                // local file; the alt text is kept, the request is not.
                Tag::Image { .. } => out.push_str("<span class=\"md-noimg\">"),
                Tag::Table(_) => out.push_str("<table>"),
                Tag::TableHead => {
                    in_table_head = true;
                    out.push_str("<thead><tr>");
                }
                Tag::TableRow => out.push_str("<tr>"),
                Tag::TableCell => out.push_str(if in_table_head { "<th>" } else { "<td>" }),
                _ => {}
            },

            Event::End(tag) => match tag {
                TagEnd::Paragraph => out.push_str("</p>"),
                TagEnd::Heading(level) => {
                    out.push_str(&format!("</h{}>", heading_level(level as u8)));
                }
                TagEnd::BlockQuote(_) => out.push_str("</blockquote>"),
                TagEnd::CodeBlock => {
                    in_code_block = false;
                    out.push_str("</code></pre>");
                }
                TagEnd::List(true) => out.push_str("</ol>"),
                TagEnd::List(false) => out.push_str("</ul>"),
                TagEnd::Item => out.push_str("</li>"),
                TagEnd::Emphasis => out.push_str("</em>"),
                TagEnd::Strong => out.push_str("</strong>"),
                TagEnd::Strikethrough => out.push_str("</del>"),
                TagEnd::Link => match links.pop() {
                    Some(LinkKind::Anchor) => out.push_str("</a>"),
                    Some(LinkKind::Refused) => out.push_str("</span>"),
                    Some(LinkKind::Bare) | None => {}
                },
                TagEnd::Image => out.push_str("</span>"),
                TagEnd::Table => out.push_str("</tbody></table>"),
                TagEnd::TableHead => {
                    in_table_head = false;
                    out.push_str("</tr></thead><tbody>");
                }
                TagEnd::TableRow => out.push_str("</tr>"),
                TagEnd::TableCell => out.push_str(if in_table_head { "</th>" } else { "</td>" }),
                _ => {}
            },

            Event::Text(t) => out.push_str(&esc(&t)),
            Event::Code(t) => out.push_str(&format!("<code>{}</code>", esc(&t))),
            Event::SoftBreak => out.push(if in_code_block { '\n' } else { ' ' }),
            Event::HardBreak => out.push_str("<br>"),
            Event::Rule => out.push_str("<hr>"),
            Event::TaskListMarker(done) => out.push_str(if done {
                "<input type=\"checkbox\" checked disabled> "
            } else {
                "<input type=\"checkbox\" disabled> "
            }),
            _ => {}
        }
    }
    out
}

/// Which element a `[...]( ... )` opened, so its End can close that one.
enum LinkKind {
    /// A real `<a>`.
    Anchor,
    /// A `<span class="md-badlink">` standing in for a scheme we refuse.
    Refused,
    /// `[text]()` - no destination, so no wrapper at all.
    Bare,
}

/// A note's `# Heading` becomes `<h4>`, and everything below it shifts to
/// match, bottoming out at `<h6>`.
fn heading_level(level: u8) -> u8 {
    (level + 3).min(6)
}

/// `None` for anything that is not a plain, inert link. An allowlist, not
/// a blocklist: the schemes a review note legitimately needs are few, and
/// guessing at the ones to ban is how `javascript&colon;` gets through.
///
/// # The control characters have to go before the check, not after
///
/// A URL parser strips ASCII tab, CR and LF out of an href *before* it
/// works out the scheme. Markdown gets them in for free - pulldown-cmark
/// decodes `&Tab;` and `&#9;` in a link destination - so `java&Tab;script:`
/// arrives here spelled `java\tscript:`, which is not a valid scheme, so a
/// naive check calls it a relative path and waves it through; the browser
/// then removes the tab and runs it. That defeats the allowlist for *every*
/// scheme, not just this one.
///
/// So the controls are removed first, and the cleaned string is both what
/// gets checked and what gets returned. Checking one spelling and emitting
/// another is the bug, in whatever form it shows up.
fn safe_href(url: &str) -> Option<String> {
    let cleaned: String = url.chars().filter(|c| !c.is_control()).collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        return None;
    }
    let lower = cleaned.to_ascii_lowercase();
    // Relative links and fragments carry no scheme and are harmless.
    let has_scheme = lower
        .split_once(':')
        .is_some_and(|(scheme, _)| scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)));
    if !has_scheme {
        return Some(cleaned.to_string());
    }
    const ALLOWED: [&str; 4] = ["http://", "https://", "mailto:", "vstfs:"];
    ALLOWED
        .iter()
        .any(|p| lower.starts_with(p))
        .then(|| cleaned.to_string())
}
