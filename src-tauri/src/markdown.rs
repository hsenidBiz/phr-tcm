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
use std::ops::Range;

use crate::case_markdown::{Field, CASE_MARKDOWN_OPTIONS};

/// What a Markdown list item becomes in a step, which has no lists.
const BULLET: &str = "\u{2022} ";

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
///
/// Notes sit inside a page that already has an `<h1>` and `<h2>`s of its
/// own, so a note's own headings start at `<h4>` - the outline stays sane
/// and a note cannot impersonate a section of the report.
pub fn to_html(src: &str) -> String {
    render(src, 3)
}

/// The same renderer, for a document that IS the page - a spec pane tab,
/// not a note embedded in one - so its own `# Heading` stays `<h1>`.
pub fn to_html_standalone(src: &str) -> String {
    render(src, 0)
}

fn render(src: &str, heading_base: u8) -> String {
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
                    out.push_str(&format!("<h{}>", heading_level(level as u8, heading_base)));
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
                    out.push_str(&format!("</h{}>", heading_level(level as u8, heading_base)));
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

// ---------------------------------------------------------------------------
// Test case text
// ---------------------------------------------------------------------------

/// A step's action or expected result, or the List of Prerequisites, as the
/// HTML the review page shows: the same output as the app's own screens
/// (`renderCaseMarkdown` in `src/lib/markdown.ts`), so a case reads the same
/// wherever it is opened.
///
/// The dialect is the converter's (`case_markdown::CASE_MARKDOWN_OPTIONS`),
/// not the one above: plain CommonMark with no tables, strikethrough or task
/// lists, because Azure DevOps shows none of them and an old case holding a
/// `~~` or a `|` must not look different. Bold, italic, inline code and
/// fenced blocks show; headings, links, images, quotes and rules show as
/// their plain text, as they do in Azure DevOps. A single newline is a line
/// break. Raw HTML never reaches the output as markup: a tag Azure DevOps'
/// editor could have produced is dropped and its text kept, and anything else
/// in angle brackets (`<cycleId>`) is a typed placeholder, shown as typed.
///
/// - A step is lines, as Azure DevOps shows it: one `<br>` per line break,
///   so a blank line is two and three blank lines stay three. It has no
///   paragraphs and no lists - a list item is a "• item" (or "1. item")
///   line - and a fenced block stays a block, the one place the page looks
///   richer than the step (its lines are the same).
/// - The prerequisites are paragraphs and real lists.
///
/// Returns an empty string for empty input.
pub fn case_text_to_html(src: &str, field: Field) -> String {
    match field {
        Field::Step => step_text_to_html(src),
        Field::Prereq => prereq_text_to_html(src),
    }
}

/// Raw HTML met inline, as what it shows: nothing for a real tag, the text
/// as typed for anything else.
fn inline_html(text: &str) -> String {
    if crate::steps_xml::contains_html_markup(text) {
        String::new()
    } else {
        text_lines(text)
    }
}

/// A raw HTML block, as what it shows: its text with the real tags out.
fn html_block_text(text: &str) -> String {
    let kept = crate::steps_xml::strip_tags(text);
    text_lines(kept.trim())
}

/// Escaped text with its line breaks kept.
fn text_lines(text: &str) -> String {
    esc(text).replace("\r\n", "<br>").replace('\n', "<br>")
}

fn prereq_text_to_html(src: &str) -> String {
    let mut out = String::new();
    // A raw HTML block arrives line by line; it is one paragraph of text.
    let mut html_block: Option<String> = None;
    for event in Parser::new_ext(src, CASE_MARKDOWN_OPTIONS) {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph | Tag::Heading { .. } => out.push_str("<p>"),
                Tag::CodeBlock(_) => out.push_str("<pre><code>"),
                Tag::HtmlBlock => html_block = Some(String::new()),
                Tag::List(Some(start)) => out.push_str(&format!("<ol start=\"{start}\">")),
                Tag::List(None) => out.push_str("<ul>"),
                Tag::Item => out.push_str("<li>"),
                Tag::Emphasis => out.push_str("<em>"),
                Tag::Strong => out.push_str("<strong>"),
                // Quotes, links and images are their contents: the link text
                // and the image's alt text arrive as text events.
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph | TagEnd::Heading(_) => out.push_str("</p>\n"),
                TagEnd::CodeBlock => out.push_str("</code></pre>\n"),
                TagEnd::HtmlBlock => {
                    if let Some(buf) = html_block.take() {
                        let text = html_block_text(&buf);
                        if !text.is_empty() {
                            out.push_str(&format!("<p>{text}</p>\n"));
                        }
                    }
                }
                TagEnd::List(true) => out.push_str("</ol>\n"),
                TagEnd::List(false) => out.push_str("</ul>\n"),
                TagEnd::Item => out.push_str("</li>"),
                TagEnd::Emphasis => out.push_str("</em>"),
                TagEnd::Strong => out.push_str("</strong>"),
                _ => {}
            },
            Event::Text(t) => out.push_str(&esc(&t)),
            Event::Code(t) => out.push_str(&format!("<code>{}</code>", esc(&t))),
            Event::Html(t) => match html_block.as_mut() {
                Some(buf) => buf.push_str(&t),
                None => out.push_str(&inline_html(&t)),
            },
            Event::InlineHtml(t) => out.push_str(&inline_html(&t)),
            Event::SoftBreak | Event::HardBreak => out.push_str("<br>"),
            _ => {}
        }
    }
    out
}

/// One visual piece of a step: a line (some inline HTML) or a code block,
/// with where it sits in the source so the blank lines between two pieces
/// can be counted.
struct StepPiece {
    html: String,
    code: bool,
    start: usize,
    end: usize,
}

/// What `step_text_to_html` is in the middle of.
#[derive(Default)]
struct StepWalk {
    pieces: Vec<StepPiece>,
    /// The line being built: its HTML and where it starts and ends.
    line: Option<(String, usize, usize)>,
    /// Open lists: whether ordered, the next number, the delimiter typed.
    lists: Vec<(bool, u64, char)>,
    /// The marker of an item whose first content has not arrived yet.
    marker: Option<(String, usize)>,
    /// A fenced or indented block being collected.
    code: Option<String>,
    /// A raw HTML block being collected.
    html: Option<String>,
}

impl StepWalk {
    /// Inline content joins the current line, which an open item's marker
    /// starts.
    fn inline(&mut self, html: &str, range: &Range<usize>) {
        if self.line.is_none() {
            let (lead, start) = match self.marker.take() {
                Some((marker, start)) => (marker, start),
                None => (String::new(), range.start),
            };
            self.line = Some((lead, start, range.start));
        }
        if let Some((line, _, end)) = self.line.as_mut() {
            line.push_str(html);
            *end = (*end).max(range.end);
        }
    }

    /// An item that turned out to hold no text of its own (a code block or a
    /// nested list follows) still shows its marker.
    fn lone_marker(&mut self) {
        if let Some((marker, start)) = self.marker.take() {
            self.pieces.push(StepPiece { html: marker, code: false, start, end: start });
        }
    }

    fn flush(&mut self) {
        if let Some((html, start, end)) = self.line.take() {
            self.pieces.push(StepPiece { html, code: false, start, end });
        }
    }
}

fn step_text_to_html(src: &str) -> String {
    let mut w = StepWalk::default();
    for (event, range) in Parser::new_ext(src, CASE_MARKDOWN_OPTIONS).into_offset_iter() {
        match event {
            Event::Start(tag) => match tag {
                Tag::List(start) => {
                    w.flush();
                    w.lone_marker();
                    let delim = if start.is_some() { ordered_delimiter(src, range.start) } else { '.' };
                    w.lists.push((start.is_some(), start.unwrap_or(1), delim));
                }
                Tag::Item => {
                    w.flush();
                    w.lone_marker();
                    let marker = match w.lists.last_mut() {
                        Some((true, next, delim)) => {
                            let m = format!("{next}{delim} ");
                            *next += 1;
                            m
                        }
                        _ => BULLET.to_string(),
                    };
                    w.marker = Some((esc(&marker), range.start));
                }
                Tag::CodeBlock(_) => {
                    w.flush();
                    w.lone_marker();
                    w.code = Some(String::new());
                }
                Tag::HtmlBlock => {
                    w.flush();
                    w.html = Some(String::new());
                }
                Tag::Emphasis => w.inline("<em>", &range),
                Tag::Strong => w.inline("<strong>", &range),
                // Paragraphs, headings, quotes, links and images are just
                // their contents.
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph | TagEnd::Heading(_) => w.flush(),
                TagEnd::Item => {
                    w.flush();
                    w.lone_marker();
                }
                TagEnd::List(_) => {
                    w.lists.pop();
                }
                TagEnd::CodeBlock => {
                    if let Some(text) = w.code.take() {
                        w.pieces.push(StepPiece {
                            html: format!("<pre><code>{}</code></pre>", esc(&text)),
                            code: true,
                            start: range.start,
                            end: range.end,
                        });
                    }
                }
                TagEnd::HtmlBlock => {
                    if let Some(buf) = w.html.take() {
                        let html = html_block_text(&buf);
                        if !html.is_empty() {
                            w.pieces.push(StepPiece { html, code: false, start: range.start, end: range.end });
                        }
                    }
                }
                TagEnd::Emphasis => w.inline("</em>", &range),
                TagEnd::Strong => w.inline("</strong>", &range),
                _ => {}
            },
            Event::Text(t) => match w.code.as_mut() {
                Some(code) => code.push_str(&t),
                None => w.inline(&esc(&t), &range),
            },
            Event::Code(t) => w.inline(&format!("<code>{}</code>", esc(&t)), &range),
            Event::Html(t) => match w.html.as_mut() {
                Some(buf) => buf.push_str(&t),
                None => w.inline(&inline_html(&t), &range),
            },
            Event::InlineHtml(t) => w.inline(&inline_html(&t), &range),
            Event::SoftBreak | Event::HardBreak => w.inline("<br>", &range),
            _ => {}
        }
    }
    w.flush();
    w.lone_marker();

    let mut html = String::new();
    let mut prev: Option<&StepPiece> = None;
    for piece in &w.pieces {
        if let Some(p) = prev {
            let blank = blank_lines(src, p.end, piece.start);
            // After a block a <br> is a whole line; before one, the first
            // <br> only ends the line it follows.
            let breaks = if p.code {
                blank
            } else if piece.code {
                if blank > 0 { blank + 1 } else { 0 }
            } else {
                1 + blank
            };
            html.push_str(&"<br>".repeat(breaks));
        }
        html.push_str(&piece.html);
        prev = Some(piece);
    }
    html
}

/// The `.` or `)` a person typed after an ordered list's first number.
fn ordered_delimiter(src: &str, at: usize) -> char {
    src[at..]
        .trim_start()
        .chars()
        .find(|c| !c.is_ascii_digit())
        .filter(|c| *c == ')')
        .unwrap_or('.')
}

/// How many blank lines lie in the source between where one piece ends and
/// the next starts.
fn blank_lines(src: &str, prev_end: usize, next_start: usize) -> usize {
    if next_start <= prev_end {
        return 0;
    }
    let breaks = src[prev_end..next_start].matches('\n').count();
    // A piece's range may or may not take in its own line ending.
    if src[..prev_end].ends_with('\n') { breaks } else { breaks.saturating_sub(1) }
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

/// A `# Heading` becomes `<h{1 + base}>`, and everything below it shifts to
/// match, bottoming out at `<h6>`.
fn heading_level(level: u8, base: u8) -> u8 {
    (level + base).min(6)
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
