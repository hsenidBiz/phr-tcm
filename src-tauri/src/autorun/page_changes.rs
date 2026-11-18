//! What changed on a page since the reader's last answer.
//!
//! Each browser slot remembers, per assistant, the address and the whole
//! printed lines of the last page it gave. The next read of the same page can
//! then say only what differs (`diff`, `render`): the lines that are new or
//! gone, each under its unchanged ancestors (the nearest lines above it with
//! less indentation, as `get_autorun_page`'s query prints them). This module
//! is pure: it knows nothing of how a page is read or when a diff is used.

use super::slots::{SlotNo, SLOT_COUNT};
use std::sync::Mutex;

/// Said when the page is exactly what the reader already has.
pub const UNCHANGED_PAGE: &str = "page: unchanged since your last answer";

/// The first line of a changes answer.
pub const CHANGES_HEADER: &str = "page: changes since your last answer (\"page\": \"whole\" for the page)";

/// Above this many lines on either side (after the common head and tail are
/// set aside) the diff marks every remaining line instead of aligning them.
const MAX_SIDE: usize = 4000;

/// One line of a changes answer.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    /// `'+'` new, `'-'` gone, `' '` an unchanged ancestor shown for context.
    pub marker: char,
    pub line: String,
    /// For a `'+'` line, its index among the new lines.
    pub new_index: Option<usize>,
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// Indexes of the ancestors of line `i` of `lines`, outermost first.
fn ancestors(lines: &[String], i: usize) -> Vec<usize> {
    let mut need = indent(&lines[i]);
    let mut found = Vec::new();
    for j in (0..i).rev() {
        if need == 0 {
            break;
        }
        if lines[j].trim().is_empty() {
            continue;
        }
        let d = indent(&lines[j]);
        if d < need {
            found.push(j);
            need = d;
        }
    }
    found.reverse();
    found
}

#[derive(Clone, Copy)]
enum Op {
    Gone(usize),
    New(usize),
}

/// The lines gone from `old` and new in `new`, in page order, gone before new
/// within a run.
fn align(old: &[String], new: &[String]) -> Vec<Op> {
    let mut head = 0;
    while head < old.len() && head < new.len() && old[head] == new[head] {
        head += 1;
    }
    let (mut oe, mut ne) = (old.len(), new.len());
    while oe > head && ne > head && old[oe - 1] == new[ne - 1] {
        oe -= 1;
        ne -= 1;
    }
    let (n, m) = (oe - head, ne - head);
    let mut ops = Vec::new();
    if n > MAX_SIDE || m > MAX_SIDE {
        ops.extend((head..oe).map(Op::Gone));
        ops.extend((head..ne).map(Op::New));
        return ops;
    }
    // lcs[i][j]: the longest common run of old[head+i..oe] and new[head+j..ne].
    let w = m + 1;
    let mut lcs = vec![0u16; (n + 1) * w];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i * w + j] = if old[head + i] == new[head + j] {
                lcs[(i + 1) * w + j + 1] + 1
            } else {
                lcs[(i + 1) * w + j].max(lcs[i * w + j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && old[head + i] == new[head + j] {
            i += 1;
            j += 1;
        } else if i < n && (j == m || lcs[(i + 1) * w + j] >= lcs[i * w + j + 1]) {
            ops.push(Op::Gone(head + i));
            i += 1;
        } else {
            ops.push(Op::New(head + j));
            j += 1;
        }
    }
    ops
}

/// A line as printed: which page and line it came from, and its text.
struct Shown {
    gone: bool,
    index: usize,
    text: String,
}

/// The changed lines between `old` and `new`, each under its unmarked
/// ancestors (each ancestor once), in page order. Empty when nothing changed.
pub fn diff(old: &[String], new: &[String]) -> Vec<Change> {
    let mut out = Vec::new();
    // The chain of lines the last change was printed under, itself included.
    let mut chain: Vec<Shown> = Vec::new();
    for op in align(old, new) {
        let (gone, index, lines) = match op {
            Op::Gone(i) => (true, i, old),
            Op::New(j) => (false, j, new),
        };
        let mut want: Vec<Shown> = ancestors(lines, index)
            .into_iter()
            .map(|a| Shown { gone, index: a, text: lines[a].clone() })
            .collect();
        // Skip what the previous change already printed: the same line, or
        // the same text at the same depth when it came from the other page.
        let mut same = 0;
        while same < want.len()
            && same < chain.len()
            && ((chain[same].gone == want[same].gone && chain[same].index == want[same].index)
                || (chain[same].gone != want[same].gone && chain[same].text == want[same].text))
        {
            same += 1;
        }
        for a in &want[same..] {
            out.push(Change { marker: ' ', line: a.text.clone(), new_index: None });
        }
        out.push(Change {
            marker: if gone { '-' } else { '+' },
            line: lines[index].clone(),
            new_index: if gone { None } else { Some(index) },
        });
        want.push(Shown { gone, index, text: lines[index].clone() });
        chain = want;
    }
    out
}

/// A changes answer: `CHANGES_HEADER`, then each change on a line of its own,
/// `+` or `-` in front of the line, an ancestor with a space so it lines up.
pub fn render(changes: &[Change]) -> String {
    let mut lines = vec![CHANGES_HEADER.to_string()];
    lines.extend(changes.iter().map(|c| format!("{}{}", c.marker, c.line)));
    lines.join("\n")
}

fn bare(address: &str) -> &str {
    address.split('#').next().unwrap_or(address)
}

/// Do `a` and `b` name the same page once each loses its `#fragment`?
pub fn same_address(a: &str, b: &str) -> bool {
    bare(a) == bare(b)
}

/// Per slot and reader, the address and whole lines of the last page given.
static LAST: Mutex<Vec<(usize, String, String, Vec<String>)>> = Mutex::new(Vec::new());

/// The address (fragment removed) and whole lines last given to slot `n`'s
/// reader `owner`.
pub fn remembered(n: SlotNo, owner: &str) -> Option<(String, Vec<String>)> {
    let last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    last.iter()
        .find(|(slot, who, _, _)| *slot == n.index() && who == owner)
        .map(|(_, _, address, lines)| (address.clone(), lines.clone()))
}

/// Slot `n`'s reader `owner` was given `lines` of the page at `address`.
pub fn remember_whole(n: SlotNo, owner: &str, address: &str, lines: Vec<String>) {
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    last.retain(|(slot, who, _, _)| !(*slot == n.index() && who == owner));
    if last.len() >= 4 * SLOT_COUNT {
        last.remove(0);
    }
    last.push((n.index(), owner.to_string(), bare(address).to_string(), lines));
}

/// Everything slot `n` has been told is forgotten, whoever the reader.
pub fn forget(n: SlotNo) {
    LAST.lock().unwrap_or_else(|e| e.into_inner()).retain(|(slot, _, _, _)| *slot != n.index());
}
