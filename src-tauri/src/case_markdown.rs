//! Test case text: Markdown inside TCM, the HTML each Azure DevOps field
//! supports outside.
//!
//! A step's action and expected result, and the List of Prerequisites, are
//! Markdown everywhere in TCM - the import JSON, the queue, the editors,
//! the assistant's tools. Azure DevOps keeps HTML, and not the same HTML
//! for both: a step's editor offers bold, italic and underline only, so a
//! step is written as `<B>`, `<I>` and `<BR/>` and nothing else; the
//! prerequisites field is a full rich-text field and gets paragraphs,
//! lists and code. This module is both directions for both fields.
//!
//! # One shape in the middle
//!
//! Every conversion goes through the same small model - lines of styled
//! runs, grouped into paragraphs, lists and code blocks - and both sides
//! are normalised into it the same way (whitespace collapsed as HTML
//! collapses it, edge spaces moved outside bold and italic). So "did this
//! survive the round trip" is a comparison of two values, not of two
//! strings that may legitimately differ (`<B>` vs `<STRONG>`, `<DIV>` vs
//! `<P>`).
//!
//! # Reading back without inventing formatting
//!
//! The hard direction is HTML to Markdown for text that holds no
//! formatting. Every case written so far is plain text, and a plain
//! `SELECT COUNT(*) FROM t` has to come back as exactly that - not as
//! `COUNT(\*)`, and never as italics. So nothing is escaped by rule.
//! The text is first written out with no escapes at all and read back
//! through the very parser that will read it later; if that reproduces the
//! model, it is the answer. Only when it does not (two `(*)` on a line DO
//! pair up as emphasis; a line that starts `# ` IS a heading) is every
//! punctuation mark escaped, and then each escape is taken away again,
//! last first, wherever the text still reads back the same without it.
//! What remains is only what would otherwise be read as Markdown.
//!
//! # Raw HTML
//!
//! As in `markdown.rs`, raw HTML in the Markdown never reaches the output
//! as markup. A tag Azure DevOps' editor could have produced (the list in
//! `steps_xml::HTML_TAGS`) is dropped and its text kept; anything else in
//! angle brackets - `<cycleId>`, `<next assessment stage>` - is a
//! placeholder somebody typed, and is kept as escaped text, for the reason
//! `steps_xml::strip_tags` gives.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::ops::Range;

use crate::html_escape::esc as escape_html;
use crate::steps_xml::{
    collapse_ws, contains_html_markup, html_tag_end, html_to_text, strip_tags, unescape_html,
};

/// What a Markdown list item becomes in a step, which has no lists.
const BULLET: &str = "\u{2022} ";

/// The Markdown dialect of test case text: plain CommonMark - no tables,
/// strikethrough, task lists, footnotes or smart punctuation.
///
/// Every place TCM shows case text renders it with exactly these options,
/// so what a person sees in TCM is what this module writes to Azure DevOps.
/// A renderer with tables switched on would show a pipe-table as a table
/// in TCM while the step in Azure DevOps holds its text; one with
/// strikethrough would show `~~x~~` struck through while Azure DevOps shows
/// the tildes. Neither field can hold those, so neither dialect may
/// pretend to.
pub const CASE_MARKDOWN_OPTIONS: Options = Options::empty();

/// Which field a text belongs to: a step's action or expected result, or
/// the List of Prerequisites. They take different HTML.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Step,
    Prereq,
}

// ---------------------------------------------------------------------------
// The model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Style {
    bold: bool,
    italic: bool,
    /// Inline code. Only the prerequisites keep it; a step shows its text.
    code: bool,
}

impl Style {
    /// What a space between two runs shares with both of them.
    fn meet(self, other: Style) -> Style {
        Style {
            bold: self.bold && other.bold,
            italic: self.italic && other.italic,
            code: self.code && other.code,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Run {
    text: String,
    style: Style,
}

impl Run {
    fn plain(text: impl Into<String>) -> Run {
        Run { text: text.into(), style: Style::default() }
    }
}

/// One visual line. Empty is a blank line.
type Line = Vec<Run>;

#[derive(Debug, Clone, PartialEq)]
enum Block {
    /// Lines shown one under the other, as one paragraph.
    Para(Vec<Line>),
    List { ordered: bool, start: u64, items: Vec<Vec<Block>> },
    /// A code block's lines, verbatim.
    Code(Vec<String>),
}

// ---------------------------------------------------------------------------
// The public surface
// ---------------------------------------------------------------------------

/// A step's Markdown as the HTML a step field takes: escaped text, `<B>`,
/// `<I>` and `<BR/>` - one `<BR/>` per line break, so a blank line is two.
/// A list item becomes a "• item" (or "1. item") line, a fenced block its
/// lines, inline code its text; headings, links, images, quotes and raw
/// HTML become their plain text.
pub fn step_md_to_html(md: &str) -> String {
    step_html(&step_md_lines(md))
}

/// The prerequisites' Markdown as rich-text HTML: `<p>`, `<br>`,
/// `<strong>`, `<em>`, `<ul>`/`<ol>`/`<li>`, `<code>` and `<pre><code>`.
/// Headings, links, images and quotes become their plain text; raw HTML is
/// dropped (see the module notes).
pub fn prereq_md_to_html(md: &str) -> String {
    blocks_html(&prereq_md_blocks(md))
}

/// A step's HTML, as Azure DevOps holds it, back to Markdown. Lists read
/// back as the "• " lines a step shows, a `<pre>` as plain lines, `<U>` as
/// plain text. Plain text comes back exactly as written.
pub fn step_html_to_md(html: &str) -> String {
    protect(&step_html_lines(html), ser_step, step_md_lines, plainer_lines)
}

/// The prerequisites' HTML back to Markdown: lists stay lists, a `<pre>`
/// becomes a fence. Plain text comes back exactly as written.
pub fn prereq_html_to_md(html: &str) -> String {
    let blocks = normalize_blocks(html_blocks(html), true);
    protect(&blocks, ser_prereq, prereq_md_blocks, plainer_blocks)
}

/// Whether `new_md` leaves a field that holds `current_html` as it is:
/// equal to that HTML read as Markdown, or equal to what TCM's reader
/// made of it before Markdown (`legacy_text`).
///
/// The second is every copy saved before this change - a queue draft, a
/// cache, an exported file. It holds `Save` where Azure DevOps holds
/// `<B>Save</B>`, one line where it holds three, `COUNT(*)` where the
/// Markdown is `COUNT(\*)`; none of that is an edit, and none of it may
/// rewrite the field. An edit that adds bold or a line break differs from
/// both.
pub fn unchanged(new_md: &str, current_html: &str, field: Field) -> bool {
    let read = match field {
        Field::Step => step_html_to_md(current_html),
        Field::Prereq => prereq_html_to_md(current_html),
    };
    new_md == read || new_md == legacy_text(current_html, field)
}

/// A field's HTML as TCM read it before case text was Markdown: for a
/// step, entities undone, real tags stripped and all whitespace collapsed
/// (as `steps_xml::parse_steps_xml` did); for the prerequisites,
/// `steps_xml::html_to_text`.
pub fn legacy_text(html: &str, field: Field) -> String {
    match field {
        Field::Step => collapse_ws(&strip_tags(&unescape_html(html))),
        Field::Prereq => html_to_text(html),
    }
}

// ---------------------------------------------------------------------------
// Escaping only what needs it
// ---------------------------------------------------------------------------

/// The Markdown for `model`, with the fewest escapes that make it read
/// back as `model`.
///
/// `ser(model, esc)` writes the model, escaping the `i`th punctuation mark
/// of its text when `esc[i]` (missing entries are false), and says how many
/// marks there are. `parse` reads Markdown back into the model.
///
/// When even escaping everything does not reproduce the model, it holds
/// emphasis Markdown cannot write (`normalize_line` removes the kinds that
/// are known; this is the net under it). `plainer(model, what_it_read_as)`
/// then takes the emphasis off the lines that came back wrong and the
/// plainer model is written instead - losing a bold, never leaving its
/// asterisks in Azure DevOps.
fn protect<T: PartialEq>(
    model: &T,
    ser: fn(&T, &[bool]) -> (String, usize),
    parse: fn(&str) -> T,
    plainer: fn(&T, &T) -> T,
) -> String {
    let (bare, marks) = ser(model, &[]);
    if parse(&bare) == *model {
        return bare;
    }
    let mut esc = vec![true; marks];
    let target = parse(&ser(model, &esc).0);
    if target != *model {
        let simpler = plainer(model, &target);
        if simpler != *model {
            return protect(&simpler, ser, parse, plainer);
        }
    }
    for i in (0..marks).rev() {
        esc[i] = false;
        if parse(&ser(model, &esc).0) != target {
            esc[i] = true;
        }
    }
    ser(model, &esc).0
}

/// A line without bold or italic.
fn unemphasised(line: &Line) -> Line {
    let plain: Line = line
        .iter()
        .map(|r| Run { style: Style { bold: false, italic: false, ..r.style }, ..r.clone() })
        .collect();
    normalize_line(&plain)
}

/// `model` with the emphasis taken off every line that `read` does not
/// reproduce.
fn plainer_lines(model: &Vec<Line>, read: &Vec<Line>) -> Vec<Line> {
    model
        .iter()
        .enumerate()
        .map(|(i, l)| if read.get(i) == Some(l) { l.clone() } else { unemphasised(l) })
        .collect()
}

/// The same for blocks, line by line where the two still line up and
/// wholesale where they do not.
fn plainer_blocks(model: &Vec<Block>, read: &Vec<Block>) -> Vec<Block> {
    fn all(b: &Block) -> Block {
        match b {
            Block::Para(lines) => Block::Para(lines.iter().map(unemphasised).collect()),
            Block::Code(lines) => Block::Code(lines.clone()),
            Block::List { ordered, start, items } => Block::List {
                ordered: *ordered,
                start: *start,
                items: items.iter().map(|it| it.iter().map(all).collect()).collect(),
            },
        }
    }
    model
        .iter()
        .enumerate()
        .map(|(i, b)| match (b, read.get(i)) {
            (Block::Para(lines), Some(Block::Para(got))) => Block::Para(plainer_lines(lines, got)),
            (
                Block::List { ordered, start, items },
                Some(Block::List { ordered: o, start: s, items: got }),
            ) if ordered == o && start == s => Block::List {
                ordered: *ordered,
                start: *start,
                items: items
                    .iter()
                    .enumerate()
                    .map(|(k, it)| plainer_blocks(it, got.get(k).unwrap_or(&vec![])))
                    .collect(),
            },
            (b, Some(got)) if b == got => b.clone(),
            (b, _) => all(b),
        })
        .collect()
}

/// Markdown being written, counting the punctuation marks of the text so
/// each can be escaped or not.
struct Out<'a> {
    md: String,
    esc: &'a [bool],
    marks: usize,
}

impl<'a> Out<'a> {
    fn new(esc: &'a [bool]) -> Self {
        Out { md: String::new(), esc, marks: 0 }
    }

    fn raw(&mut self, s: &str) {
        self.md.push_str(s);
    }

    /// Text, with a backslash before each punctuation mark chosen for one.
    /// CommonMark lets a backslash escape any ASCII punctuation, and only
    /// that, so those are the only candidates.
    fn text(&mut self, t: &str) {
        for c in t.chars() {
            if c.is_ascii_punctuation() {
                if self.esc.get(self.marks).copied().unwrap_or(false) {
                    self.md.push('\\');
                }
                self.marks += 1;
            }
            self.md.push(c);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Bold,
    Italic,
}

impl Mark {
    fn delim(self) -> &'static str {
        match self {
            Mark::Bold => "**",
            Mark::Italic => "*",
        }
    }

    fn on(self, s: Style) -> bool {
        match self {
            Mark::Bold => s.bold,
            Mark::Italic => s.italic,
        }
    }
}

/// How emphasis opens and closes across a line's runs, as a stack: what
/// run `i` closes (innermost first) and then opens (outermost first).
/// Marks that open together nest so the one that lasts longer is outside.
///
/// A mark that ends while one opened AFTER it carries on - `*a **b*** **c**`,
/// overlapping rather than nested - would have to be closed and reopened at
/// that point, which Markdown cannot always parse; `normalize_line` removes
/// every such overlap first, and `Change::conflict` is how it finds them.
fn emphasis_steps(line: &Line) -> Vec<Change> {
    let mut steps = vec![];
    let mut open: Vec<(Mark, usize)> = vec![];
    for (i, run) in line.iter().enumerate() {
        let mut step = Change::default();
        if let Some(p) = open.iter().position(|(m, _)| !m.on(run.style)) {
            step.conflict = open[p + 1..]
                .iter()
                .filter(|(m, _)| m.on(run.style))
                .map(|&(m, at)| (m, at))
                .collect();
            step.close = open[p..].iter().rev().map(|(m, _)| *m).collect();
            open.truncate(p);
        }
        let lasts = |m: Mark| line[i..].iter().take_while(|r| m.on(r.style)).count();
        let mut new: Vec<Mark> = [Mark::Bold, Mark::Italic]
            .into_iter()
            .filter(|m| m.on(run.style) && !open.iter().any(|(o, _)| o == m))
            .collect();
        new.sort_by_key(|m| std::cmp::Reverse(lasts(*m)));
        for m in &new {
            open.push((*m, i));
        }
        step.open = new;
        steps.push(step);
    }
    steps
}

#[derive(Default)]
struct Change {
    close: Vec<Mark>,
    open: Vec<Mark>,
    /// Marks closed here only to be reopened at once, with the run each
    /// was first opened at.
    conflict: Vec<(Mark, usize)>,
}

/// One line of runs as Markdown; every style closes at the end of the
/// line, so a line break inside bold is written `**a**` / `**b**`.
fn ser_line(o: &mut Out, line: &Line) {
    let mut open: Vec<Mark> = vec![];
    for (run, step) in line.iter().zip(emphasis_steps(line)) {
        for m in step.close {
            o.raw(m.delim());
            open.retain(|x| *x != m);
        }
        for m in step.open {
            o.raw(m.delim());
            open.push(m);
        }
        if run.style.code {
            o.raw(&code_span(&run.text));
        } else {
            o.text(&run.text);
        }
    }
    for m in open.iter().rev() {
        o.raw(m.delim());
    }
}

/// The longest run of backticks in `s`.
fn longest_backticks(s: &str) -> usize {
    let (mut best, mut cur) = (0, 0);
    for c in s.chars() {
        if c == '`' {
            cur += 1;
            best = best.max(cur);
        } else {
            cur = 0;
        }
    }
    best
}

/// Inline code: fenced by more backticks than it holds, and padded with a
/// space when it starts or ends with one (CommonMark strips that space).
fn code_span(t: &str) -> String {
    let fence = "`".repeat(longest_backticks(t) + 1);
    let pad = if t.starts_with('`') || t.ends_with('`') { " " } else { "" };
    format!("{fence}{pad}{t}{pad}{fence}")
}

fn ser_step(lines: &Vec<Line>, esc: &[bool]) -> (String, usize) {
    let mut o = Out::new(esc);
    for (k, line) in lines.iter().enumerate() {
        if k > 0 {
            o.raw("\n");
        }
        ser_line(&mut o, line);
    }
    (o.md, o.marks)
}

fn ser_prereq(blocks: &Vec<Block>, esc: &[bool]) -> (String, usize) {
    let mut o = Out::new(esc);
    ser_blocks(&mut o, blocks, "", "");
    let md = o.md.trim_end_matches('\n').to_string();
    (md, o.marks)
}

/// Whether two blocks in a row need a blank line between them. A list
/// (other than one numbered from past 1) and a fence may follow a
/// paragraph directly, and a paragraph a fence - so "Accounts:" over its
/// list reads back as it was typed. Anything else would run together.
fn needs_blank(prev: &Block, next: &Block) -> bool {
    match (prev, next) {
        (Block::Para(_), Block::List { ordered, start, .. }) => *ordered && *start != 1,
        (Block::Para(_), Block::Code(_)) | (Block::Code(_), Block::Para(_)) => false,
        _ => true,
    }
}

/// Blocks as Markdown. `first` prefixes the first line, `cont` every other
/// one - the indentation that keeps a list item's later lines inside it.
fn ser_blocks(o: &mut Out, blocks: &[Block], first: &str, cont: &str) {
    for (i, b) in blocks.iter().enumerate() {
        let lead = if i == 0 { first } else { cont };
        if i > 0 && needs_blank(&blocks[i - 1], b) {
            o.raw("\n");
        }
        match b {
            Block::Para(lines) => {
                for (k, line) in lines.iter().enumerate() {
                    o.raw(if k == 0 { lead } else { cont });
                    ser_line(o, line);
                    o.raw("\n");
                }
            }
            Block::Code(lines) => {
                let longest = lines.iter().map(|l| longest_backticks(l)).max().unwrap_or(0);
                let fence = "`".repeat((longest + 1).max(3));
                o.raw(lead);
                o.raw(&fence);
                o.raw("\n");
                for l in lines {
                    o.raw(cont);
                    o.raw(l);
                    o.raw("\n");
                }
                o.raw(cont);
                o.raw(&fence);
                o.raw("\n");
            }
            Block::List { ordered, start, items } => {
                for (k, item) in items.iter().enumerate() {
                    let marker = if *ordered {
                        format!("{}. ", start + k as u64)
                    } else {
                        "- ".to_string()
                    };
                    let first_line = format!("{}{marker}", if k == 0 { lead } else { cont });
                    let rest = format!("{cont}{}", " ".repeat(marker.len()));
                    ser_blocks(o, item, &first_line, &rest);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Markdown -> model
// ---------------------------------------------------------------------------

/// Raw HTML met in Markdown, as the text it leaves: a real tag leaves
/// nothing, anything else in angle brackets is kept as typed.
fn inline_html_text(h: &str) -> Option<&str> {
    (!contains_html_markup(h)).then_some(h)
}

/// Where `range`'s text stops, before the line ending it closes with.
fn without_newline(src: &str, range: Range<usize>) -> usize {
    let bytes = src.as_bytes();
    let mut end = range.end;
    while end > range.start && matches!(bytes[end - 1], b'\n' | b'\r') {
        end -= 1;
    }
    end
}

/// A step's lines from its Markdown.
///
/// A step keeps every line break exactly, so the line structure is read
/// from the SOURCE between one piece of content and the next, not from the
/// parse: one line break, plus one more for each blank line in between.
/// That keeps three blank lines as three, makes a soft break a line break
/// (as it is in a step), and lets a fence's own lines vanish without
/// leaving a gap.
fn step_md_lines(md: &str) -> Vec<Line> {
    let mut acc = StepLines { src: md, lines: vec![], cur: vec![], prev_end: None };
    let (mut bold, mut italic) = (0usize, 0usize);
    let mut in_code = false;
    let mut lists: Vec<Option<u64>> = vec![];
    for (ev, r) in Parser::new_ext(md, CASE_MARKDOWN_OPTIONS).into_offset_iter() {
        let style = Style { bold: bold > 0, italic: italic > 0, code: false };
        match ev {
            Event::Start(Tag::Strong) => bold += 1,
            Event::End(TagEnd::Strong) => bold = bold.saturating_sub(1),
            Event::Start(Tag::Emphasis) => italic += 1,
            Event::End(TagEnd::Emphasis) => italic = italic.saturating_sub(1),
            Event::Start(Tag::List(start)) => lists.push(start),
            Event::End(TagEnd::List(_)) => {
                lists.pop();
            }
            Event::Start(Tag::Item) => {
                let prefix = match lists.last_mut() {
                    Some(Some(n)) => {
                        let p = format!("{n}{} ", ordered_delimiter(md, r.start));
                        *n += 1;
                        p
                    }
                    _ => BULLET.to_string(),
                };
                acc.content(&prefix, Style::default(), r.start..r.start);
            }
            Event::Start(Tag::CodeBlock(_)) => in_code = true,
            Event::End(TagEnd::CodeBlock) => in_code = false,
            Event::Text(t) if in_code => acc.code_text(&t, r),
            Event::Text(t) | Event::Code(t) => acc.content(&t, style, r),
            Event::InlineHtml(h) => {
                if let Some(t) = inline_html_text(&h) {
                    acc.content(t, style, r);
                }
            }
            Event::Html(h) => {
                let kept = strip_tags(&h);
                let kept = kept.trim();
                if !kept.is_empty() {
                    let end = without_newline(md, r.clone());
                    acc.content(kept, style, r.start..end);
                }
            }
            _ => {}
        }
    }
    acc.newline();
    finish_step_lines(acc.lines)
}

/// The `.` or `)` a person typed after an ordered item's number, read from
/// the source at the item: `1) Login` stays `1) Login` in a step.
fn ordered_delimiter(src: &str, at: usize) -> char {
    src[at..]
        .trim_start()
        .chars()
        .find(|c| !c.is_ascii_digit())
        .filter(|c| *c == ')')
        .unwrap_or('.')
}

struct StepLines<'a> {
    src: &'a str,
    lines: Vec<Line>,
    cur: Line,
    /// Where the last content ended in the source.
    prev_end: Option<usize>,
}

impl StepLines<'_> {
    fn newline(&mut self) {
        self.lines.push(std::mem::take(&mut self.cur));
    }

    /// Break lines as the source does between the last content and `at`.
    fn break_to(&mut self, at: usize) {
        let Some(prev) = self.prev_end else { return };
        if prev >= at {
            return;
        }
        let gap = &self.src[prev..at];
        if !gap.contains('\n') {
            return;
        }
        let parts: Vec<&str> = gap.split('\n').collect();
        let blanks = parts[1..parts.len() - 1].iter().filter(|p| p.trim().is_empty()).count();
        self.newline();
        for _ in 0..blanks {
            self.newline();
        }
    }

    fn content(&mut self, text: &str, style: Style, r: Range<usize>) {
        self.break_to(r.start);
        self.cur.push(Run { text: text.to_string(), style });
        self.prev_end = Some(r.end);
    }

    /// A code block's text: its own line breaks are lines; the one it ends
    /// with is left for the next content to measure from.
    fn code_text(&mut self, text: &str, r: Range<usize>) {
        self.break_to(r.start);
        let (body, ends_line) = match text.strip_suffix('\n') {
            Some(b) => (b, true),
            None => (text, false),
        };
        for (i, seg) in body.split('\n').enumerate() {
            if i > 0 {
                self.newline();
            }
            self.cur.push(Run::plain(seg.trim_end_matches('\r')));
        }
        self.prev_end = Some(if ends_line { without_newline(self.src, r) } else { r.end });
    }
}

/// The prerequisites' blocks from their Markdown.
fn prereq_md_blocks(md: &str) -> Vec<Block> {
    let mut b = Builder::new();
    let (mut bold, mut italic) = (0usize, 0usize);
    let mut code: Option<String> = None;
    for ev in Parser::new_ext(md, CASE_MARKDOWN_OPTIONS) {
        let style = Style { bold: bold > 0, italic: italic > 0, code: false };
        match ev {
            Event::Start(Tag::Strong) => bold += 1,
            Event::End(TagEnd::Strong) => bold = bold.saturating_sub(1),
            Event::Start(Tag::Emphasis) => italic += 1,
            Event::End(TagEnd::Emphasis) => italic = italic.saturating_sub(1),
            Event::Start(Tag::Paragraph | Tag::Heading { .. } | Tag::BlockQuote(_))
            | Event::End(
                TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::BlockQuote(_) | TagEnd::HtmlBlock,
            )
            | Event::Rule => b.para_break(),
            Event::Start(Tag::List(start)) => b.open_list(start.is_some(), start.unwrap_or(1)),
            Event::End(TagEnd::List(_)) => b.close_list(),
            Event::Start(Tag::Item) => b.open_item(),
            Event::End(TagEnd::Item) => b.close_item(),
            Event::Start(Tag::CodeBlock(_)) => code = Some(String::new()),
            Event::End(TagEnd::CodeBlock) => {
                if let Some(buf) = code.take() {
                    b.code(buf.split('\n').map(str::to_string).collect());
                }
            }
            Event::Text(t) => match code.as_mut() {
                Some(buf) => buf.push_str(&t),
                None => b.text(&t, style),
            },
            Event::Code(t) => b.text(&t, Style { code: true, ..style }),
            Event::InlineHtml(h) => {
                if let Some(t) = inline_html_text(&h) {
                    b.text(t, style);
                }
            }
            Event::Html(h) => {
                let kept = strip_tags(&h);
                if !kept.trim().is_empty() {
                    b.text(kept.trim(), style);
                }
                b.soft_break();
            }
            Event::SoftBreak | Event::HardBreak => b.newline(),
            _ => {}
        }
    }
    normalize_blocks(b.finish(), true)
}

// ---------------------------------------------------------------------------
// Building blocks (from Markdown events or HTML tags)
// ---------------------------------------------------------------------------

/// Where lines collect: the document, or one list item.
#[derive(Default)]
struct Container {
    blocks: Vec<Block>,
    lines: Vec<Line>,
    cur: Line,
    /// The current line exists although it holds nothing visible - a
    /// `<div>&nbsp;</div>`, which Azure DevOps uses for a blank line.
    touched: bool,
    /// A paragraph ended: the next text starts after a blank line.
    gap: bool,
}

impl Container {
    fn has_cur(&self) -> bool {
        self.touched || !self.cur.is_empty()
    }

    fn newline(&mut self) {
        self.lines.push(std::mem::take(&mut self.cur));
        self.touched = false;
    }

    fn soft_break(&mut self) {
        if self.has_cur() {
            self.newline();
        }
    }

    fn para_break(&mut self) {
        self.soft_break();
        self.gap = true;
    }

    fn apply_gap(&mut self) {
        if self.gap {
            let last_has_text = self
                .lines
                .last()
                .is_some_and(|l| l.iter().any(|r| !r.text.trim().is_empty()));
            if self.cur.is_empty() && last_has_text {
                self.lines.push(vec![]);
            }
            self.gap = false;
        }
    }

    fn text(&mut self, t: &str, style: Style) {
        if t.trim().is_empty() {
            if self.has_cur() {
                self.cur.push(Run { text: t.to_string(), style });
            }
            return;
        }
        self.apply_gap();
        self.cur.push(Run { text: t.to_string(), style });
    }

    fn touch(&mut self) {
        self.apply_gap();
        self.touched = true;
    }

    fn flush(&mut self) {
        self.soft_break();
        if !self.lines.is_empty() {
            self.blocks.push(Block::Para(std::mem::take(&mut self.lines)));
        }
        self.gap = false;
    }
}

enum Frame {
    Box(Container),
    List { ordered: bool, start: u64, items: Vec<Vec<Block>> },
}

/// Blocks from a stream of "text here, a line break, a list starts" -
/// forgiving about the unclosed `<li>` and the list item with no list that
/// real HTML has.
struct Builder {
    stack: Vec<Frame>,
}

impl Builder {
    fn new() -> Self {
        Builder { stack: vec![Frame::Box(Container::default())] }
    }

    fn top_is_list(&self) -> bool {
        matches!(self.stack.last(), Some(Frame::List { .. }))
    }

    fn in_item(&self) -> bool {
        let n = self.stack.len();
        n >= 2
            && matches!(self.stack[n - 1], Frame::Box(_))
            && matches!(self.stack[n - 2], Frame::List { .. })
    }

    /// The container text goes into. Text directly inside a list is an
    /// item of its own.
    fn cont(&mut self) -> &mut Container {
        if self.top_is_list() {
            self.open_item();
        }
        match self.stack.last_mut() {
            Some(Frame::Box(c)) => c,
            _ => unreachable!("open_item leaves a container on top"),
        }
    }

    /// The container on top, unless that is a list (where a line break or
    /// a space means nothing).
    fn top_box(&mut self) -> Option<&mut Container> {
        match self.stack.last_mut() {
            Some(Frame::Box(c)) => Some(c),
            _ => None,
        }
    }

    fn has_cur(&mut self) -> bool {
        self.top_box().is_some_and(|c| c.has_cur())
    }

    fn text(&mut self, t: &str, style: Style) {
        if t.trim().is_empty() {
            if let Some(c) = self.top_box() {
                c.text(t, style);
            }
            return;
        }
        self.cont().text(t, style);
    }

    fn newline(&mut self) {
        if let Some(c) = self.top_box() {
            c.newline();
        }
    }

    fn soft_break(&mut self) {
        if let Some(c) = self.top_box() {
            c.soft_break();
        }
    }

    fn para_break(&mut self) {
        if let Some(c) = self.top_box() {
            c.para_break();
        }
    }

    fn touch(&mut self) {
        if let Some(c) = self.top_box() {
            c.touch();
        }
    }

    fn open_list(&mut self, ordered: bool, start: u64) {
        self.cont().flush();
        self.stack.push(Frame::List { ordered, start, items: vec![] });
    }

    fn open_item(&mut self) {
        if self.in_item() {
            self.close_item();
        }
        if !self.top_is_list() {
            self.open_list(false, 1);
        }
        self.stack.push(Frame::Box(Container::default()));
    }

    fn close_item(&mut self) {
        if !self.in_item() {
            return;
        }
        if let Some(Frame::Box(mut c)) = self.stack.pop() {
            c.flush();
            if let Some(Frame::List { items, .. }) = self.stack.last_mut() {
                items.push(c.blocks);
            }
        }
    }

    fn close_list(&mut self) {
        if self.in_item() {
            self.close_item();
        }
        if !self.top_is_list() {
            return;
        }
        if let Some(Frame::List { ordered, start, items }) = self.stack.pop() {
            let c = self.cont();
            c.flush();
            c.blocks.push(Block::List { ordered, start, items });
        }
    }

    fn code(&mut self, lines: Vec<String>) {
        let c = self.cont();
        c.flush();
        c.blocks.push(Block::Code(lines));
    }

    fn finish(mut self) -> Vec<Block> {
        while self.stack.len() > 1 {
            if self.in_item() {
                self.close_item();
            } else if self.top_is_list() {
                self.close_list();
            } else {
                break;
            }
        }
        match self.stack.pop() {
            Some(Frame::Box(mut c)) => {
                c.flush();
                c.blocks
            }
            _ => vec![],
        }
    }
}

// ---------------------------------------------------------------------------
// HTML -> model
// ---------------------------------------------------------------------------

/// Blocks from Azure DevOps HTML, upper or lower case. A tag is only what
/// `steps_xml` counts as one, so a typed `<cycleId>` (stored unescaped by
/// older writers) stays text.
fn html_blocks(html: &str) -> Vec<Block> {
    let chars: Vec<char> = html.chars().collect();
    let mut w = Walker { b: Builder::new(), bold: 0, italic: 0, code: 0, pre: None };
    let mut text = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            if chars[i..].starts_with(&['<', '!', '-', '-']) {
                w.text(&std::mem::take(&mut text));
                let close = (i + 4..chars.len().saturating_sub(2))
                    .find(|&j| chars[j..j + 3] == ['-', '-', '>']);
                i = close.map_or(chars.len(), |j| j + 3);
                continue;
            }
            if let Some(end) = html_tag_end(&chars, i) {
                w.text(&std::mem::take(&mut text));
                let tag: String = chars[i..=end].iter().collect();
                w.tag(&tag);
                i = end + 1;
                continue;
            }
        }
        text.push(chars[i]);
        i += 1;
    }
    w.text(&text);
    w.finish()
}

struct Walker {
    b: Builder,
    bold: usize,
    italic: usize,
    code: usize,
    /// Inside a `<pre>`: its text, verbatim.
    pre: Option<String>,
}

fn count(n: &mut usize, closing: bool, self_closing: bool) {
    if self_closing {
        return;
    }
    if closing {
        *n = n.saturating_sub(1);
    } else {
        *n += 1;
    }
}

/// An `<ol start="N">`'s N.
fn list_start(tag: &str) -> u64 {
    let lower = tag.to_ascii_lowercase();
    let Some(at) = lower.find("start") else { return 1 };
    let rest = lower[at + 5..].trim_start();
    let Some(rest) = rest.strip_prefix('=') else { return 1 };
    let digits: String = rest
        .trim_start()
        .trim_start_matches(['"', '\''])
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().unwrap_or(1)
}

impl Walker {
    fn style(&self) -> Style {
        Style { bold: self.bold > 0, italic: self.italic > 0, code: self.code > 0 }
    }

    fn tag(&mut self, tag: &str) {
        let inner = tag.trim_start_matches('<').trim_end_matches('>');
        let closing = inner.starts_with('/');
        let inner = inner.trim_start_matches('/');
        let self_closing = inner.trim_end().ends_with('/');
        let name: String = inner
            .chars()
            .take_while(char::is_ascii_alphanumeric)
            .collect::<String>()
            .to_ascii_lowercase();

        if let Some(buf) = self.pre.as_mut() {
            match name.as_str() {
                "br" => buf.push('\n'),
                "pre" if closing => {
                    let buf = self.pre.take().unwrap_or_default();
                    self.b.code(buf.split('\n').map(str::to_string).collect());
                }
                _ => {}
            }
            return;
        }

        match name.as_str() {
            "b" | "strong" => count(&mut self.bold, closing, self_closing),
            "i" | "em" => count(&mut self.italic, closing, self_closing),
            "code" => count(&mut self.code, closing, self_closing),
            "br" => self.b.newline(),
            "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote" => self.b.para_break(),
            "div" | "tr" | "table" | "tbody" | "thead" | "caption" | "dl" | "dt" | "dd"
            | "center" | "hr" => self.b.soft_break(),
            "td" | "th" => {
                if !closing && self.b.has_cur() {
                    self.b.text(" ", Style::default());
                }
            }
            "ul" | "ol" => {
                if closing {
                    self.b.close_list();
                } else if !self_closing {
                    self.b.open_list(name == "ol", list_start(tag));
                }
            }
            "li" => {
                if closing {
                    self.b.close_item();
                } else {
                    self.b.open_item();
                }
            }
            "pre" => {
                if !closing && !self_closing {
                    self.b.soft_break();
                    self.pre = Some(String::new());
                }
            }
            // Underline has no Markdown form; links, spans and fonts are
            // their text; an image leaves nothing.
            _ => {}
        }
    }

    /// A run of text between tags, entities still in it.
    ///
    /// Whitespace is HTML's: any run of it - a raw newline included - is
    /// one space, because that is what Azure DevOps shows. Only `<BR>`,
    /// `<P>`, `<DIV>` and `<li>` break a line. A chunk that is ONLY
    /// whitespace is the indentation between tags and at most a space.
    fn text(&mut self, raw: &str) {
        if raw.is_empty() {
            return;
        }
        let s = unescape_html(raw).replace('\r', "");
        if let Some(buf) = self.pre.as_mut() {
            buf.push_str(&s);
            return;
        }
        let style = self.style();
        if s.chars().all(char::is_whitespace) {
            if s.contains('\u{a0}') {
                self.b.touch();
            }
            if self.b.has_cur() {
                self.b.text(" ", style);
            }
            return;
        }
        self.b.text(&s.replace('\u{a0}', " "), style);
    }

    fn finish(mut self) -> Vec<Block> {
        if let Some(buf) = self.pre.take() {
            self.b.code(buf.split('\n').map(str::to_string).collect());
        }
        self.b.finish()
    }
}

/// A step's lines from its HTML: a step has no blocks, so lists become
/// "• " lines and a `<pre>` its lines.
fn step_html_lines(html: &str) -> Vec<Line> {
    let mut lines = vec![];
    flatten(&html_blocks(html), &mut lines);
    finish_step_lines(lines)
}

fn flatten(blocks: &[Block], out: &mut Vec<Line>) {
    for b in blocks {
        match b {
            Block::Para(lines) => out.extend(lines.iter().cloned()),
            Block::Code(lines) => out.extend(lines.iter().map(|l| vec![Run::plain(l.as_str())])),
            Block::List { ordered, start, items } => {
                for (k, item) in items.iter().enumerate() {
                    let prefix = if *ordered {
                        format!("{}. ", start + k as u64)
                    } else {
                        BULLET.to_string()
                    };
                    let mut sub = vec![];
                    flatten(item, &mut sub);
                    if sub.is_empty() {
                        sub.push(vec![]);
                    }
                    sub[0].insert(0, Run::plain(prefix));
                    out.extend(sub);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Normalising, so both directions meet
// ---------------------------------------------------------------------------

/// One line as HTML shows it, and as Markdown can write it: whitespace
/// collapsed and trimmed; a space keeping only the styles BOTH its
/// neighbours have too (so `<B>Save </B>now` is `**Save** now`, never
/// `**Save **now`); punctuation unglued from a styled edge (`unglue`);
/// overlapping styles made to nest (`unoverlap`); equal neighbours merged.
///
/// Both directions normalise through here, which is what makes the result
/// of one the exact input of the other.
fn normalize_line(line: &[Run]) -> Line {
    let mut chars: Vec<(char, Style)> = vec![];
    for r in line {
        for c in r.text.chars() {
            let c = if c.is_whitespace() { ' ' } else { c };
            if c == ' ' && chars.last().map_or(true, |(p, _)| *p == ' ') {
                continue;
            }
            chars.push((c, r.style));
        }
    }
    while chars.last().is_some_and(|(c, _)| *c == ' ') {
        chars.pop();
    }
    // Each pass only ever takes a style off a character, so this ends; the
    // bound is a guard, not a limit anything reaches.
    for _ in 0..64 {
        loop {
            // No leading, trailing or doubled spaces are left, so every
            // space has a neighbour on each side.
            for i in 0..chars.len() {
                if chars[i].0 == ' ' {
                    let s = chars[i].1.meet(chars[i - 1].1).meet(chars[i + 1].1);
                    chars[i].1 = s;
                }
            }
            let bold = unglue(&mut chars, |s| &mut s.bold);
            let italic = unglue(&mut chars, |s| &mut s.italic);
            let bare_bold = drop_bare(&mut chars, |s| &mut s.bold);
            let bare_italic = drop_bare(&mut chars, |s| &mut s.italic);
            if !bold && !italic && !bare_bold && !bare_italic {
                break;
            }
        }
        let mut runs = group(&chars);
        if !unoverlap(&mut runs) {
            return runs;
        }
        chars = runs
            .iter()
            .flat_map(|r| r.text.chars().map(move |c| (c, r.style)))
            .collect();
    }
    group(&chars)
}

fn group(chars: &[(char, Style)]) -> Line {
    let mut out: Line = vec![];
    for &(c, style) in chars {
        match out.last_mut() {
            Some(r) if r.style == style => r.text.push(c),
            _ => out.push(Run { text: c.to_string(), style }),
        }
    }
    out
}

/// Remove the first overlap between bold and italic, if there is one, and
/// say so. Where a style ends while one opened inside it carries on, the
/// inner one has to close and reopen at that point - `***` then `**` with
/// nothing between, which Markdown reads as one run of five. If the next
/// text starts with a space, that space loses its styles and the closing
/// and opening fall on either side of it (`*note **this*** **now**`).
/// Glued to text, there is no Markdown for it at all, and the inner style
/// is dropped where the two overlapped.
fn unoverlap(runs: &mut Line) -> bool {
    let steps = emphasis_steps(runs);
    let Some((i, step)) = steps.iter().enumerate().find(|(_, s)| !s.conflict.is_empty()) else {
        return false;
    };
    if let Some(rest) = runs[i].text.strip_prefix(' ') {
        let rest = rest.to_string();
        runs[i].text = rest;
        runs.insert(i, Run::plain(" "));
        runs.retain(|r| !r.text.is_empty());
    } else {
        for &(m, from) in &step.conflict {
            for r in &mut runs[from..i] {
                match m {
                    Mark::Bold => r.style.bold = false,
                    Mark::Italic => r.style.italic = false,
                }
            }
        }
    }
    true
}

/// Punctuation (or inline code, which starts with a backtick) for the
/// purpose of CommonMark's flanking rules - anything that is neither a
/// letter, a digit nor a space.
fn punct_like(c: char, s: Style) -> bool {
    s.code || !(c.is_alphanumeric() || c.is_whitespace())
}

/// A letter or digit outside code.
fn word_char(c: char, s: Style) -> bool {
    !s.code && c.is_alphanumeric()
}

/// Take one style off any punctuation at the edge of a styled stretch
/// that is glued to a letter outside it. `a<B>(x)</B>` cannot be written
/// in Markdown - `a**(x)**` is not bold, because a `**` between a letter
/// and a bracket can only close - so it is `a(**x)**`: the bracket loses
/// its bold, the text keeps it, and no asterisk ever reaches Azure DevOps.
/// Says whether anything changed.
fn unglue(chars: &mut [(char, Style)], flag: impl Fn(&mut Style) -> &mut bool) -> bool {
    let mut changed = false;
    let n = chars.len();
    let mut i = 0;
    while i < n {
        if !*flag(&mut chars[i].1) {
            i += 1;
            continue;
        }
        let mut end = i;
        while end < n && *flag(&mut chars[end].1) {
            end += 1;
        }
        let (mut a, mut b) = (i, end);
        while a < b && a > 0 && punct_like(chars[a].0, chars[a].1) && word_char(chars[a - 1].0, chars[a - 1].1) {
            *flag(&mut chars[a].1) = false;
            a += 1;
            changed = true;
        }
        while b > a && b < n && punct_like(chars[b - 1].0, chars[b - 1].1) && word_char(chars[b].0, chars[b].1) {
            *flag(&mut chars[b - 1].1) = false;
            b -= 1;
            changed = true;
        }
        i = end;
    }
    changed
}

/// Take a style off any stretch of it that holds no letter or digit. A
/// bold apostrophe looks like any other, and between two punctuation
/// marks (`'**'**,`) CommonMark cannot tell which way such a `**` faces.
/// Says whether anything changed.
fn drop_bare(chars: &mut [(char, Style)], flag: impl Fn(&mut Style) -> &mut bool) -> bool {
    let mut changed = false;
    let n = chars.len();
    let mut i = 0;
    while i < n {
        if !*flag(&mut chars[i].1) {
            i += 1;
            continue;
        }
        let mut end = i;
        while end < n && *flag(&mut chars[end].1) {
            end += 1;
        }
        if !chars[i..end].iter().any(|(c, _)| c.is_alphanumeric()) {
            for ch in &mut chars[i..end] {
                *flag(&mut ch.1) = false;
            }
            changed = true;
        }
        i = end;
    }
    changed
}

/// A step's lines, normalised: no inline code (a step shows its text), no
/// blank lines at either end.
fn finish_step_lines(lines: Vec<Line>) -> Vec<Line> {
    let lines: Vec<Line> = lines
        .into_iter()
        .map(|l| {
            let plain: Line = l
                .into_iter()
                .map(|r| Run { style: Style { code: false, ..r.style }, ..r })
                .collect();
            normalize_line(&plain)
        })
        .collect();
    let first = lines.iter().position(|l| !l.is_empty());
    let last = lines.iter().rposition(|l| !l.is_empty());
    match (first, last) {
        (Some(a), Some(b)) => lines[a..=b].to_vec(),
        _ => vec![],
    }
}

/// Blocks, normalised: a blank line parts paragraphs (when `split`), empty
/// items, lists and code blocks go, and two lists of the same kind in a
/// row are one list - Markdown cannot write them apart.
fn normalize_blocks(blocks: Vec<Block>, split: bool) -> Vec<Block> {
    let mut out: Vec<Block> = vec![];
    for b in blocks {
        match b {
            Block::Para(lines) => {
                let lines: Vec<Line> = lines.iter().map(|l| normalize_line(l)).collect();
                if split {
                    for chunk in lines.split(|l| l.is_empty()) {
                        if !chunk.is_empty() {
                            out.push(Block::Para(chunk.to_vec()));
                        }
                    }
                } else if lines.iter().any(|l| !l.is_empty()) {
                    out.push(Block::Para(lines));
                }
            }
            Block::List { ordered, start, items } => {
                let items: Vec<Vec<Block>> = items
                    .into_iter()
                    .map(|it| normalize_blocks(it, split))
                    .filter(|it| !it.is_empty())
                    .collect();
                if items.is_empty() {
                    continue;
                }
                if let Some(Block::List { ordered: o, items: prev, .. }) = out.last_mut() {
                    if *o == ordered {
                        prev.extend(items);
                        continue;
                    }
                }
                out.push(Block::List { ordered, start, items });
            }
            Block::Code(lines) => {
                let mut lines: Vec<String> =
                    lines.into_iter().map(|l| l.trim_end_matches('\r').to_string()).collect();
                while lines.first().is_some_and(|l| l.trim().is_empty()) {
                    lines.remove(0);
                }
                while lines.last().is_some_and(|l| l.trim().is_empty()) {
                    lines.pop();
                }
                if !lines.is_empty() {
                    out.push(Block::Code(lines));
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Model -> HTML
// ---------------------------------------------------------------------------

fn step_html(lines: &[Line]) -> String {
    lines
        .iter()
        .map(|line| {
            line.iter()
                .map(|r| {
                    let mut t = escape_html(&r.text);
                    if r.style.italic {
                        t = format!("<I>{t}</I>");
                    }
                    if r.style.bold {
                        t = format!("<B>{t}</B>");
                    }
                    t
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("<BR/>")
}

fn lines_html(lines: &[Line]) -> String {
    lines
        .iter()
        .map(|line| {
            line.iter()
                .map(|r| {
                    let mut t = escape_html(&r.text);
                    if r.style.code {
                        t = format!("<code>{t}</code>");
                    }
                    if r.style.italic {
                        t = format!("<em>{t}</em>");
                    }
                    if r.style.bold {
                        t = format!("<strong>{t}</strong>");
                    }
                    t
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("<br>")
}

fn blocks_html(blocks: &[Block]) -> String {
    blocks.iter().map(block_html).collect()
}

fn block_html(b: &Block) -> String {
    match b {
        Block::Para(lines) => format!("<p>{}</p>", lines_html(lines)),
        Block::Code(lines) => format!("<pre><code>{}</code></pre>", escape_html(&lines.join("\n"))),
        Block::List { ordered, start, items } => {
            let (open, close) = match (*ordered, *start) {
                (false, _) => ("<ul>".to_string(), "</ul>"),
                (true, 1) => ("<ol>".to_string(), "</ol>"),
                (true, n) => (format!("<ol start=\"{n}\">"), "</ol>"),
            };
            let items: String = items.iter().map(|it| format!("<li>{}</li>", item_html(it))).collect();
            format!("{open}{items}{close}")
        }
    }
}

/// A list item's content. Its one paragraph needs no `<p>` of its own (the
/// item is the paragraph); with two or more, each gets one so they stay
/// apart.
fn item_html(item: &[Block]) -> String {
    let paras = item.iter().filter(|b| matches!(b, Block::Para(_))).count();
    item.iter()
        .map(|b| match b {
            Block::Para(lines) if paras == 1 => lines_html(lines),
            other => block_html(other),
        })
        .collect()
}
