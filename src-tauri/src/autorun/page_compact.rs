//! A page answer that does not say the side menu again.
//!
//! The pages an assistant is handed (`get_autorun_page`, the answer of a
//! discovery start or action, a replay's page) each carry the application's
//! navigation landmark - its side menu, often a hundred lines - and it is the
//! same one almost every time. Each browser slot remembers the menu its last
//! answer carried; an answer whose menu is the same prints one line instead
//! (`UNCHANGED`). Only what an answer prints is compared and remembered: the
//! page that was read, and what it filed as seen, are untouched. `"page":
//! "full"` prints it whole, and a discovery start forgets what was said
//! (`forget`), so a new assistant on the slot starts with the whole page.

use super::slots::{SlotNo, SLOT_COUNT};
use std::sync::Mutex;

/// Printed in place of a navigation landmark the last answer already gave.
pub const UNCHANGED: &str = "side menu unchanged since the last answer";

/// Per slot, the navigation landmarks of the last answer that printed any.
static LAST: Mutex<[Option<String>; SLOT_COUNT]> = Mutex::new([const { None }; SLOT_COUNT]);

/// What slot `n` has been told of its menu is forgotten.
pub fn forget(n: SlotNo) {
    if let Some(slot) = LAST.lock().unwrap_or_else(|e| e.into_inner()).get_mut(n.index()) {
        *slot = None;
    }
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// The line ranges of the navigation landmarks in `lines`: the line that
/// names one (`navigation "..."`) and every line under it (indented more).
fn landmarks(lines: &[&str]) -> Vec<std::ops::Range<usize>> {
    let mut found = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.trim_start().starts_with("navigation \"") {
            let depth = indent(line);
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

/// `text` for slot `n`'s next answer: with `full`, or when the navigation
/// landmarks differ from the last answer's, as it is (and they are
/// remembered); when they are the same, one `UNCHANGED` line where the first
/// stood. Text with no navigation landmark is returned as it is and
/// changes nothing.
pub fn compact(n: SlotNo, text: &str, full: bool) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let found = landmarks(&lines);
    if found.is_empty() {
        return text.to_string();
    }
    let menu: String = found.iter().flat_map(|r| lines[r.clone()].iter().copied()).collect::<Vec<_>>().join("\n");
    let same = {
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        let Some(slot) = last.get_mut(n.index()) else { return text.to_string() };
        let same = slot.as_deref() == Some(menu.as_str());
        *slot = Some(menu);
        same
    };
    if full || !same {
        return text.to_string();
    }
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    let mut first = true;
    while i < lines.len() {
        match found.iter().find(|r| r.start == i) {
            Some(range) => {
                if first {
                    out.push(format!("{}{UNCHANGED}", " ".repeat(indent(lines[i]))));
                    first = false;
                }
                i = range.end;
            }
            None => {
                out.push(lines[i].to_string());
                i += 1;
            }
        }
    }
    let mut joined = out.join("\n");
    if text.ends_with('\n') {
        joined.push('\n');
    }
    joined
}
