//! Warnings about how a test case's text is formatted, for `validate_cases`.
//!
//! Test case text is Markdown (see `case_markdown`): a step keeps bold,
//! italic and line breaks, the prerequisites keep full formatting. The
//! writing guide asks the assistant to use that sparingly. This is the
//! check behind the guide - it WARNS, never refuses.
//!
//! Two kinds of finding:
//!
//! - **Too much, or the wrong kind**: a heading, table, link or image
//!   (neither field has a place for them), more than three bold or italic
//!   spans in a step, a step that is mostly bold, an SQL line that is not
//!   in a code fence (its lines would run together as one paragraph in the
//!   prerequisites, and a step has no way to show it as a query).
//! - **Text Markdown would change**: `\\server` loses a backslash, a line
//!   starting `> ` loses the `>`, a `---` line becomes a rule, `[ref]: url`
//!   is swallowed as a link definition, `__init__` turns into bold `init`.
//!   The author meant the characters; the warning says to wrap them in
//!   backticks, which keeps them exactly.
//!
//! The caller decides WHICH text to look at (see `ai_bridge::validate_json`).
//!
//! The text is parsed in the converter's dialect
//! (`case_markdown::CASE_MARKDOWN_OPTIONS`), so a finding is about what the
//! text actually becomes. That dialect has no tables: a pipe-and-dashes
//! block is lines of text, and is found by its lines instead (see
//! `pipe_table_at`).

use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use regex::Regex;
use std::sync::OnceLock;

use crate::model::TestCase;

/// Bold and italic spans a step may hold before it is called out.
const MAX_SPANS_PER_STEP: usize = 3;

/// A bold run must be at least this many words to count as "mostly bold":
/// bolding the one control a step turns on is what bold is for.
const MOSTLY_BOLD_MIN_WORDS: usize = 3;

/// What was found in one text. Everything is a count or an excerpt.
#[derive(Debug, Default)]
struct Scan {
    heading: bool,
    /// A pipe table as GFM writes one (a row of cells, then a row of
    /// dashes). Neither field shows it as a table.
    table: bool,
    link: bool,
    image: bool,
    /// Bold and italic spans, `**x**`, `*x*`, `__x__` and `_x_` alike.
    spans: usize,
    /// Non-whitespace characters of text, and how many of them sit in bold.
    text_chars: usize,
    bold_chars: usize,
    bold_words: usize,
    /// SQL-looking lines outside a code fence (excerpts).
    sql: Vec<String>,
    /// Text Markdown would change (what, excerpt).
    eaten: Vec<String>,
}

impl Scan {
    fn mostly_bold(&self) -> bool {
        self.bold_words >= MOSTLY_BOLD_MIN_WORDS && self.bold_chars * 2 > self.text_chars
    }
}

fn sql_line() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        // Upper case only, as queries are written in a test case: prose
        // such as "select a leave type from the list" must not trip it.
        Regex::new(r"\b(SELECT|EXEC|UPDATE|INSERT|DELETE)\b.*\b(FROM|WHERE|SET|VALUES)\b").unwrap()
    })
}

fn escaped_punctuation() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\\[!-/:-@\[-`{-~]").unwrap())
}

fn link_definition() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^ {0,3}\[[^\]]+\]:\s*\S").unwrap())
}

/// A pipe table's delimiter row: `| --- | :-: |`, `--- | ---`.
fn table_delimiter_row() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$").unwrap())
}

/// Whether line `i` (of the lines outside fences) starts a pipe table: it
/// holds a `|`, and the line straight under it is a delimiter row with one.
fn pipe_table_at(lines: &[(&str, bool)], i: usize) -> bool {
    let Some(&(next, next_after_blank)) = lines.get(i + 1) else {
        return false;
    };
    let head = lines[i].0;
    !next_after_blank
        && head.contains('|')
        && !head.trim().is_empty()
        && next.contains('|')
        && table_delimiter_row().is_match(next)
}

/// The start of a line, short enough to quote in a warning.
fn excerpt(s: &str) -> String {
    let line = s.trim().lines().next().unwrap_or("").trim();
    let mut out: String = line.chars().take(40).collect();
    if line.chars().count() > 40 {
        out.push_str("...");
    }
    out
}

/// A line with its inline code spans blanked out: what is inside backticks
/// is kept as typed, so it is not a finding.
fn without_code_spans(line: &str) -> String {
    let b = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'`' {
            let ch = line[i..].chars().next().unwrap_or(' ');
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        let start = i;
        while i < b.len() && b[i] == b'`' {
            i += 1;
        }
        let run = i - start;
        let mut j = i;
        let mut close = None;
        while j < b.len() {
            if b[j] == b'`' {
                let s = j;
                while j < b.len() && b[j] == b'`' {
                    j += 1;
                }
                if j - s == run {
                    close = Some(j);
                    break;
                }
            } else {
                j += 1;
            }
        }
        match close {
            Some(end) => {
                out.push(' ');
                i = end;
            }
            None => out.push_str(&line[start..i]),
        }
    }
    out
}

/// The lines that are not inside a fenced code block, each with whether
/// the line above it (outside a fence) was blank or absent.
fn lines_outside_fences(text: &str) -> Vec<(&str, bool)> {
    let mut out = Vec::new();
    // The fence that is open: its character and how many it took to open.
    let mut open: Option<(char, usize)> = None;
    let mut after_blank = true;
    for line in text.lines() {
        let t = line.trim_start();
        let lead = line.len() - t.len();
        let marker = t.chars().next().filter(|c| *c == '`' || *c == '~');
        let run = marker.map(|m| t.chars().take_while(|c| *c == m).count()).unwrap_or(0);
        match open {
            Some((m, n)) => {
                if lead < 4 && Some(m) == marker && run >= n && t[run..].trim().is_empty() {
                    open = None;
                }
                after_blank = true;
            }
            None => {
                if lead < 4 && run >= 3 && marker.is_some() {
                    open = marker.map(|m| (m, run));
                    after_blank = true;
                    continue;
                }
                out.push((line, after_blank));
                after_blank = line.trim().is_empty();
            }
        }
    }
    out
}

/// The text outside fences as paragraphs, each its lines joined by a
/// space: a blank line or a fence ends one.
fn paragraphs(lines: &[(&str, bool)]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for (line, after_blank) in lines {
        if line.trim().is_empty() || *after_blank {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        }
        if !line.trim().is_empty() {
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(line.trim());
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn scan(text: &str) -> Scan {
    let mut s = Scan::default();

    // What Markdown parses: the structure the converter builds from the
    // text, in its own dialect - so an old field that happens to hold
    // pipes and dashes is read as the lines of text it is.
    let mut bold_depth = 0usize;
    let mut in_code_block = false;
    for (event, range) in Parser::new_ext(text, crate::case_markdown::CASE_MARKDOWN_OPTIONS).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { .. }) => s.heading = true,
            Event::Start(Tag::Link { .. }) => s.link = true,
            Event::Start(Tag::Image { .. }) => s.image = true,
            Event::Start(tag @ (Tag::Strong | Tag::Emphasis)) => {
                s.spans += 1;
                if matches!(tag, Tag::Strong) {
                    bold_depth += 1;
                }
                if text[range.start..].starts_with('_') {
                    s.eaten.push(format!(
                        "emphasis written with underscores (`{}` shows as bold or italic text and loses its underscores)",
                        excerpt(&text[range.clone()])
                    ));
                }
            }
            Event::End(TagEnd::Strong) => bold_depth = bold_depth.saturating_sub(1),
            Event::Start(Tag::BlockQuote(_)) => s.eaten.push(format!(
                "a line that starts with > (`{}` reads as a quote and loses the >)",
                excerpt(&text[range.clone()])
            )),
            Event::Rule => s.eaten.push(format!(
                "a line of only - * or _ (`{}` reads as a horizontal rule)",
                excerpt(&text[range.clone()])
            )),
            Event::Start(Tag::CodeBlock(_)) => in_code_block = true,
            Event::End(TagEnd::CodeBlock) => in_code_block = false,
            Event::Text(t) | Event::Code(t) if !in_code_block => {
                let chars = t.chars().filter(|c| !c.is_whitespace()).count();
                s.text_chars += chars;
                if bold_depth > 0 {
                    s.bold_chars += chars;
                    s.bold_words += t.split_whitespace().count();
                }
            }
            _ => {}
        }
    }

    // What the lines say, outside any fence.
    let lines = lines_outside_fences(text);
    for para in paragraphs(&lines) {
        // A query may run over several lines (SELECT on one, FROM on the
        // next), so it is matched on the paragraph, with inline code
        // blanked out: a query in backticks is kept as typed.
        let plain = without_code_spans(&para);
        if let Some(m) = sql_line().find(&plain) {
            s.sql.push(excerpt(&plain[m.start()..]));
        }
    }
    s.table = (0..lines.len()).any(|i| pipe_table_at(&lines, i));
    for (line, after_blank) in lines {
        if after_blank && link_definition().is_match(line) {
            s.eaten.push(format!(
                "a link definition (`{}` reads as [ref]: url and is dropped)",
                excerpt(line)
            ));
        }
        let plain = without_code_spans(line);
        if let Some(m) = escaped_punctuation().find(&plain) {
            s.eaten.push(format!(
                "a backslash before a punctuation mark (`{}` loses a backslash)",
                excerpt(&plain[m.start()..])
            ));
        }
    }
    s
}

/// The warnings for one case's text. `index` is the case's number in the
/// draft (from 1); only steps with text of their own are looked at (a
/// Shared Steps reference has none).
///
/// `new_case` is a case without an id. A case being updated holds text
/// read back from Azure DevOps, which is plain text escaped to be safe, so
/// a heading, link, image, run of emphasis or mostly-bold step can only be
/// formatting somebody typed, and those are reported for every case. An
/// SQL line outside a fence, a pipe-and-dashes table (lines of text in the
/// converter's dialect), a backslash or a leading `>` can be the plain text
/// already there, so those are reported for new cases only.
pub fn formatting_warnings(index: usize, tc: &TestCase, new_case: bool) -> Vec<String> {
    let mut places: Vec<(String, bool, Scan)> = Vec::new();
    let mut step_spans: Vec<(usize, usize)> = Vec::new();
    for (n, st) in tc.steps.iter().enumerate() {
        if st.shared.is_some() {
            continue;
        }
        let action = scan(&st.action);
        let expected = scan(&st.expected);
        let total = action.spans + expected.spans;
        if total > MAX_SPANS_PER_STEP {
            step_spans.push((n + 1, total));
        }
        places.push((format!("step {} action", n + 1), true, action));
        places.push((format!("step {} expected", n + 1), true, expected));
    }
    places.push(("preconditions".to_string(), false, scan(&tc.preconditions)));

    let head = format!("Test case {} ('{}')", index, tc.title);
    let mut out = Vec::new();
    let names = |pick: &dyn Fn(&(String, bool, Scan)) -> bool| {
        places.iter().filter(|p| pick(p)).map(|p| p.0.clone()).collect::<Vec<_>>().join(", ")
    };

    for (what, advice, pick) in [
        (
            "a heading",
            "A heading has no place in a test case. Write a plain sentence.",
            (|p: &(String, bool, Scan)| p.2.heading) as fn(&(String, bool, Scan)) -> bool,
        ),
        (
            "a link",
            "Case text does not keep links. Write the address or the name as plain text.",
            |p| p.2.link,
        ),
        (
            "an image",
            "Case text does not keep images. Describe what the tester should see.",
            |p| p.2.image,
        ),
    ] {
        let at = names(&pick);
        if !at.is_empty() {
            out.push(format!("{head}: {at} has {what}. {advice}"));
        }
    }

    for (n, total) in step_spans {
        out.push(format!(
            "{head}: step {n} has {total} bold or italic spans. Use at most {MAX_SPANS_PER_STEP}, \
             and bold only the one value or control the step turns on."
        ));
    }

    let at = names(&|p| p.1 && p.2.mostly_bold());
    if !at.is_empty() {
        out.push(format!(
            "{head}: {at} is mostly bold. Bold is for the one value or control a step turns on; \
             write the rest as plain text."
        ));
    }

    if !new_case {
        return out;
    }

    let at = names(&|p| p.2.table);
    if !at.is_empty() {
        out.push(format!(
            "{head}: {at} has a table. Case text does not show tables. Write each row as its own \
             line, or a list."
        ));
    }

    let at = places
        .iter()
        .filter(|p| !p.2.sql.is_empty())
        .map(|p| format!("{} ('{}')", p.0, p.2.sql[0]))
        .collect::<Vec<_>>()
        .join(", ");
    if !at.is_empty() {
        out.push(format!(
            "{head}: {at} has an SQL line outside a code fence. Write a lead-in sentence such as \
             \"Run the following query.\", then the query in a ```sql fence, so its lines stay \
             on their own lines."
        ));
    }

    let detail = places
        .iter()
        .flat_map(|p| p.2.eaten.iter().map(move |e| format!("{}: {e}", p.0)))
        .collect::<Vec<_>>();
    if !detail.is_empty() {
        out.push(format!(
            "{head}: Markdown would change or drop text here: {}. If you meant the characters as \
             typed, wrap them in backticks (or put the lines in a code fence), which keeps them \
             exactly.",
            detail.join("; ")
        ));
    }
    out
}
