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
                    // that still looks clickable.
                    match safe_href(&dest_url) {
                        Some(href) => out.push_str(&format!(
                            "<a href=\"{}\"{t} rel=\"noreferrer noopener\" target=\"_blank\">",
                            esc(&href)
                        )),
                        None => out.push_str("<span class=\"md-badlink\">"),
                    }
                }
                // An image in a review note would be a remote fetch from a
                // local file; the alt text is kept, the request is not.
                Tag::Image { .. } => out.push_str("<span class=\"md-noimg\">"),
                Tag::Table(_) => out.push_str("<table>"),
                Tag::TableHead => out.push_str("<thead><tr>"),
                Tag::TableRow => out.push_str("<tr>"),
                Tag::TableCell => out.push_str("<td>"),
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
                TagEnd::Link => out.push_str("</a>"),
                TagEnd::Image => out.push_str("</span>"),
                TagEnd::Table => out.push_str("</table>"),
                TagEnd::TableHead => out.push_str("</tr></thead><tbody>"),
                TagEnd::TableRow => out.push_str("</tr>"),
                TagEnd::TableCell => out.push_str("</td>"),
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

/// A note's `# Heading` becomes `<h4>`, and everything below it shifts to
/// match, bottoming out at `<h6>`.
fn heading_level(level: u8) -> u8 {
    (level + 3).min(6)
}

/// `None` for anything that is not a plain, inert link. An allowlist, not
/// a blocklist: the schemes a review note legitimately needs are few, and
/// guessing at the ones to ban is how `javascript&colon;` gets through.
fn safe_href(url: &str) -> Option<String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    // Relative links and fragments carry no scheme and are harmless.
    let has_scheme = lower
        .split_once(':')
        .is_some_and(|(scheme, _)| scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)));
    if !has_scheme {
        return Some(trimmed.to_string());
    }
    const ALLOWED: [&str; 4] = ["http://", "https://", "mailto:", "vstfs:"];
    ALLOWED
        .iter()
        .any(|p| lower.starts_with(p))
        .then(|| trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_the_things_a_review_note_is_made_of() {
        let html = to_html(
            "## Spec link\n\nSee [AC 3](https://example.invalid/spec#ac3).\n\n\
             - covers **login**\n- covers `POST /session`\n\n\
             | Req | Case |\n|---|---|\n| AC1 | yes |\n",
        );
        assert!(html.contains("<h5>Spec link</h5>"), "headings shift under the page's own: {html}");
        assert!(html.contains(r#"<a href="https://example.invalid/spec#ac3""#));
        assert!(html.contains("rel=\"noreferrer noopener\""));
        assert!(html.contains("<strong>login</strong>"));
        assert!(html.contains("<code>POST /session</code>"));
        assert!(html.contains("<table>") && html.contains("<td>AC1</td>"));
    }

    /// The reason this module exists rather than a string filter.
    #[test]
    fn html_in_the_source_cannot_become_html_in_the_output() {
        let html = to_html(
            "<img src=x onerror=\"alert(1)\">\n\nliteral <b>bold</b> inline\n\n\
             <script>alert(2)</script>\n",
        );
        assert!(!html.contains("<img"), "{html}");
        assert!(!html.contains("<script"), "{html}");
        assert!(!html.contains("onerror"), "{html}");
        assert!(!html.contains("<b>"), "an inline tag is not markdown either: {html}");
        // The words survive; only the markup is gone.
        assert!(html.contains("literal"));
    }

    #[test]
    fn text_is_escaped_so_it_cannot_close_a_tag() {
        let html = to_html("a < b && c > d, \"quoted\"");
        assert!(html.contains("a &lt; b &amp;&amp; c &gt; d"), "{html}");
    }

    #[test]
    fn only_inert_link_schemes_survive() {
        for (src, expect_link) in [
            ("[x](https://ok.invalid)", true),
            ("[x](http://ok.invalid)", true),
            ("[x](mailto:a@b.invalid)", true),
            ("[x](./relative/page.md)", true),
            ("[x](#anchor)", true),
            ("[x](javascript:alert(1))", false),
            ("[x](JaVaScRiPt:alert(1))", false),
            ("[x](data:text/html;base64,PHNjcmlwdD4=)", false),
            ("[x](vbscript:msgbox)", false),
        ] {
            let html = to_html(src);
            assert_eq!(
                html.contains("<a href="),
                expect_link,
                "{src} produced {html}"
            );
            assert!(!html.contains("javascript:"), "{src} produced {html}");
        }
    }

    /// A remote image in a page opened from a temp file is a network call
    /// nobody asked for. The alt text stays.
    #[test]
    fn images_are_not_fetched() {
        let html = to_html("![a diagram](https://tracker.invalid/pixel.png)");
        assert!(!html.contains("<img"), "{html}");
        assert!(html.contains("a diagram"), "{html}");
    }

    #[test]
    fn empty_input_renders_nothing_at_all() {
        assert_eq!(to_html(""), "");
        assert_eq!(to_html("   \n  \n"), "");
    }

    #[test]
    fn a_code_block_keeps_its_line_breaks_and_escapes_its_contents() {
        let html = to_html("```\nif (a < b) {\n  go();\n}\n```");
        assert!(html.contains("<pre><code>"));
        assert!(html.contains("if (a &lt; b) {\n"), "{html}");
    }
}
