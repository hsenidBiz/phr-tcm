//! A page answer that does not say a side menu again.
//!
//! The pages an assistant is handed (`get_autorun_page`, the answer of a
//! discovery start or action, a replay's page) each carry the application's
//! navigation landmarks - its side menu, often a hundred lines - and they are
//! the same almost every time. Each browser slot remembers, per assistant, the
//! landmarks its last answer carried; an answer whose landmark is the same
//! prints one line naming it instead (`unchanged_line`). A landmark inside
//! `main` (a breadcrumb, tabs, pagination) is part of the page and always
//! printed.
//!
//! The collapse happens while the page is rendered, before the line limit
//! cuts it (`collapse`, called by `browser::snapshot`), so the lines it saves
//! show more of the page. It reads what the answer's reader already has from
//! a task-local (`scoped`); no scope, no collapse. What the read files as seen
//! is untouched: a collapsed line keeps its record. `"page": "full"` collapses
//! nothing, and every event that may hand the slot to another reader
//! (`forget`) starts the slot over.

use super::slots::{SlotNo, SLOT_COUNT};
use std::ops::Range;
use std::sync::Mutex;

tokio::task_local! {
    /// The text of each landmark block this answer's reader already has.
    static KNOWN: Vec<String>;
}

/// The end of the line printed in place of a landmark already given.
pub const UNCHANGED: &str = "unchanged since the last answer";

/// What is printed in place of landmark `name`, indented as it was.
pub fn unchanged_line(indent: usize, name: &str) -> String {
    format!("{}navigation \"{name}\" {UNCHANGED}", " ".repeat(indent))
}

/// One landmark a reader was given: its name and its lines, joined.
#[derive(Clone, Debug, PartialEq)]
struct Block {
    name: String,
    text: String,
}

/// Per slot and reader (`kept::owner_of`), the landmarks of the last answer
/// that had any.
static LAST: Mutex<Vec<(usize, String, Vec<Block>)>> = Mutex::new(Vec::new());

/// Everything slot `n` has been told is forgotten, whoever the reader.
pub fn forget(n: SlotNo) {
    LAST.lock().unwrap_or_else(|e| e.into_inner()).retain(|(slot, _, _)| *slot != n.index());
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn is_landmark(line: &str) -> bool {
    line.trim_start().starts_with("navigation \"")
}

/// The name in a `navigation "name" ...` line.
fn name_of(line: &str) -> String {
    let rest = line.trim_start().strip_prefix("navigation \"").unwrap_or("");
    rest.split('"').next().unwrap_or("").to_string()
}

/// Is `line` the line printed in place of a landmark (`unchanged_line`)?
fn is_unchanged(line: &str) -> bool {
    is_landmark(line) && line.ends_with(UNCHANGED)
}

/// The line ranges of the navigation landmarks in `lines` that are not
/// inside `main`: the line that names one and every line under it (indented
/// more).
fn landmarks(lines: &[&str]) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    let mut mains: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.trim().is_empty() {
            i += 1;
            continue;
        }
        let depth = indent(line);
        while mains.last().is_some_and(|m| *m >= depth) {
            mains.pop();
        }
        if line.trim_start().starts_with("main ") {
            mains.push(depth);
        }
        if is_landmark(line) && mains.is_empty() {
            let mut end = i + 1;
            while end < lines.len() && !lines[end].trim().is_empty() && indent(lines[end]) > depth {
                end += 1;
            }
            found.push(i..end);
            i = end;
        } else {
            i += 1;
        }
    }
    found
}

/// What the reader of slot `n` (`owner`) already has: the landmark blocks of
/// its last answer, or none when it asks for the whole page (`full`).
pub fn known_for(n: SlotNo, owner: &str, full: bool) -> Vec<String> {
    if full {
        return Vec::new();
    }
    let last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    last.iter()
        .find(|(slot, who, _)| *slot == n.index() && who == owner)
        .map(|(_, _, blocks)| blocks.iter().map(|b| b.text.clone()).collect())
        .unwrap_or_default()
}

/// Run `fut` (a read of the page) with `known` as what its reader has.
pub async fn scoped<F: std::future::Future>(known: Vec<String>, fut: F) -> F::Output {
    KNOWN.scope(known, fut).await
}

/// The printed lines of a page, each with the data it files, with every
/// landmark block the reader already has (`scoped`) reduced to one line that
/// names it. The block's other lines stay in the list with an empty text:
/// they are not printed or counted against the line limit, but what they file
/// as seen is kept.
pub fn collapse<T>(mut lines: Vec<(String, T)>) -> Vec<(String, T)> {
    let known = KNOWN.try_with(|k| k.clone()).unwrap_or_default();
    if known.is_empty() {
        return lines;
    }
    let texts: Vec<&str> = lines.iter().map(|(t, _)| t.as_str()).collect();
    let mut changes = Vec::new();
    for range in landmarks(&texts) {
        if known.contains(&texts[range.clone()].join("\n")) {
            changes.push((range.clone(), unchanged_line(indent(texts[range.start]), &name_of(texts[range.start]))));
        }
    }
    for (range, line) in changes {
        lines[range.start].0 = line;
        for l in &mut lines[range.start + 1..range.end] {
            l.0 = String::new();
        }
    }
    lines
}

/// What was printed to slot `n`'s reader `owner` is remembered: the landmarks
/// of `text`, the ones named by an `unchanged_line` among them as the reader
/// was given them before. Text with no landmark changes nothing.
pub fn remember(n: SlotNo, owner: &str, text: &str) {
    let lines: Vec<&str> = text.lines().collect();
    let mut mine: Vec<Block> = Vec::new();
    let before = {
        let last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        last.iter().find(|(slot, who, _)| *slot == n.index() && who == owner).map(|(_, _, b)| b.clone()).unwrap_or_default()
    };
    let mut found = false;
    for range in landmarks(&lines) {
        found = true;
        let first = lines[range.start];
        let name = name_of(first);
        if is_unchanged(first) {
            if let Some(prev) = before.iter().find(|b| b.name == name) {
                mine.push(prev.clone());
            }
        } else {
            mine.push(Block { name, text: lines[range].join("\n") });
        }
    }
    if !found {
        return;
    }
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    last.retain(|(slot, who, _)| !(*slot == n.index() && who == owner));
    if last.len() >= 4 * SLOT_COUNT {
        last.remove(0);
    }
    last.push((n.index(), owner.to_string(), mine));
}
