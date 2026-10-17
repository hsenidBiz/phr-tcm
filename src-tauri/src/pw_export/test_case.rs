//! The per-feature test-case markdown (`suites/<seg>/test-cases/<stem>.md`).
//!
//! Pure string functions. `splice` is byte-preserving outside the section it
//! replaces: a file written by `/gen-test` (or hand-edited, or CRLF) keeps
//! every other section exactly as it was. Headings inside fenced code blocks
//! are not treated specially.

use super::handover::Handover;

/// Everything one case section needs.
#[derive(Clone)]
pub struct CaseDoc {
    pub id: i32,
    pub title: String,
    pub state: String,
    pub area_path: String,
    pub iteration_path: String,
    pub project: String,
    pub module: String,
    pub tags: String,
    pub preconditions: String,
    pub steps: Vec<(String, String)>,
    pub folder: String,
    pub navigation_captured: bool,
    pub feature_title: String,
    /// What TCM knows that ADO does not: the export fills it from the
    /// script; read from ADO alone it is empty.
    pub handover: Handover,
}

/// A table cell: `|` escaped, every newline flavour as `<br>`.
fn cell(s: &str) -> String {
    s.replace('|', "\\|")
        .replace("\r\n", "<br>")
        .replace(['\r', '\n'], "<br>")
}

/// Single-line text: newlines become spaces.
fn one_line(s: &str) -> String {
    s.replace("\r\n", " ").replace(['\r', '\n'], " ")
}

/// What every table row TCM writes from its own knowledge ends with: the
/// only rows a re-export replaces (`with_repo_rows`).
pub const FROM_TCM: &str = " (from TCM)";

/// A cell of a row TCM writes: one line, `|` escaped, marked `FROM_TCM`.
fn tcm_cell(s: &str) -> String {
    format!("{}{FROM_TCM}", cell(one_line(s).trim()))
}

/// A code fence longer than any backtick run in `body`.
fn fence_for(body: &str) -> String {
    let (mut longest, mut run) = (0usize, 0usize);
    for c in body.chars() {
        if c == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    "`".repeat(longest.max(2) + 1)
}

pub fn section(doc: &CaseDoc) -> String {
    let mut o = String::new();
    o.push_str(&format!("## {} \u{2014} {}\n\n", doc.id, one_line(&doc.title)));
    o.push_str("### Metadata\n\n| Field | Value |\n|-------|-------|\n");
    let rows: [(&str, String); 8] = [
        ("ID", doc.id.to_string()),
        ("Title", doc.title.clone()),
        ("State", doc.state.clone()),
        ("Area Path", doc.area_path.clone()),
        ("Iteration", doc.iteration_path.clone()),
        ("Project", doc.project.clone()),
        ("Module", doc.module.clone()),
        ("Tags", doc.tags.clone()),
    ];
    for (k, v) in rows {
        o.push_str(&format!("| {} | {} |\n", k, cell(&v)));
    }
    o.push_str(&format!("\n**Folder:** {} \u{2014} mapped in TCM.\n\n", one_line(&doc.folder)));
    if doc.navigation_captured {
        o.push_str("**Navigation:** captured in src/navigation.json.\n\n");
    } else {
        o.push_str(
            "**Navigation:** not yet captured - capture it in this repo before refactoring.\n\n",
        );
    }
    if !doc.handover.from_tcm.is_empty() {
        o.push_str("**From TCM:**\n\n");
        for line in &doc.handover.from_tcm {
            o.push_str(&format!("- {}\n", one_line(line).trim()));
        }
        o.push('\n');
    }
    o.push_str("### Preconditions\n\n| # | Condition |\n|---|-----------|\n");
    let mut n = 0;
    for line in doc.preconditions.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        n += 1;
        o.push_str(&format!("| {} | {} |\n", n, cell(line)));
    }
    for row in &doc.handover.preconditions {
        n += 1;
        o.push_str(&format!("| {} | {} |\n", n, tcm_cell(row)));
    }
    o.push_str(
        "\n### Test Data & Validation\n\n| Name | Value / Rule | Source | Expected After Save | Notes |\n|------|--------------|--------|---------------------|-------|\n",
    );
    let nav = one_line(&doc.feature_title);
    let fence = fence_for(&nav);
    o.push_str(&format!("\n### Navigation Path\n\n{fence}text\n{nav}\n{fence}\n"));
    o.push_str("\n### Test Steps\n\n| Step | Action | Expected Result |\n|------|--------|-----------------|\n");
    for (i, (action, expected)) in doc.steps.iter().enumerate() {
        o.push_str(&format!("| {} | {} | {} |\n", i + 1, cell(action), cell(expected)));
    }
    o.push_str("\n### Cleanup / Postconditions\n\n| # | Action |\n|---|--------|\n");
    for (i, row) in doc.handover.cleanup.iter().enumerate() {
        o.push_str(&format!("| {} | {} |\n", i + 1, tcm_cell(row)));
    }
    o.push('\n');
    o
}

pub fn new_file(feature_title: &str, stem: &str, user_key: &str) -> String {
    format!(
        "# Test Case Set: {}\n\nCovers the ADO test cases below. Each `##` section is one `test()` in\n`specs/{stem}.spec.ts`; the ids here are what the linter's\n`assertion-count` rule resolves to raw counterparts via\n`suites/_generated/index.json`.\n\n**User:** {}  <!-- from-tcm -->\n\n",
        one_line(feature_title),
        one_line(user_key),
    )
}

/// Does `line` start a level-2 heading (`##` then whitespace)?
fn is_h2(line: &str) -> bool {
    line.strip_prefix("##")
        .and_then(|r| r.chars().next())
        .is_some_and(char::is_whitespace)
}

/// Is `line` the heading of case `id` (`^##\s+<id>\s`)?
fn is_case_heading(line: &str, id: i32) -> bool {
    let Some(rest) = line.strip_prefix("##") else { return false };
    let rest_trim = rest.trim_start_matches(char::is_whitespace);
    if rest_trim.len() == rest.len() {
        return false;
    }
    let digits = rest_trim.len() - rest_trim.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 || rest_trim[..digits].parse::<i64>().ok() != Some(i64::from(id)) {
        return false;
    }
    rest_trim[digits..].chars().next().is_some_and(char::is_whitespace)
}

fn is_blank(line: &str) -> bool {
    line.trim().is_empty()
}

/// A markdown heading: one to six `#` at the line's start, then a space or
/// a tab. `#11 deletes it` or `#hashtag` is text, not a heading.
fn is_heading(line: &str) -> bool {
    let hashes = line.len() - line.trim_start_matches('#').len();
    (1..=6).contains(&hashes) && line[hashes..].starts_with([' ', '\t'])
}

/// A code fence's opening or closing line.
fn is_fence(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("```") || t.starts_with("~~~")
}

/// A paragraph starting at `lines[at]`, with the blank lines after it.
fn paragraph_from(lines: &[&str], at: usize) -> usize {
    let mut end = at + 1;
    while end < lines.len() && !is_blank(lines[end]) && !is_heading(lines[end]) {
        end += 1;
    }
    while end < lines.len() && is_blank(lines[end]) {
        end += 1;
    }
    end
}

/// From `lines[at]` up to the next heading outside a code fence.
fn block_from(lines: &[&str], at: usize) -> usize {
    let mut fenced = is_fence(lines[at]);
    let mut end = at + 1;
    while end < lines.len() && (fenced || !is_heading(lines[end])) {
        if is_fence(lines[end]) {
            fenced = !fenced;
        }
        end += 1;
    }
    end
}

/// The blocks of a case section the repo writes itself, which a re-export
/// keeps exactly as they are (byte for byte):
/// - `**Also:** ...`, the line `/gen-test --also` writes under the heading;
/// - `**Added by engineer (not from ADO):**` and everything after it up to
///   the next markdown heading outside a code fence: the steps an engineer
///   added, and their notes. Put back before Cleanup, or after it when the
///   engineer had it after Cleanup;
/// - an `**Assertion floor:** ...` paragraph that `npm run spec:floor`
///   appended, put back at the section's end.
///
/// A `## Amendment` section is a level-two heading of its own, so it is
/// never inside a case section and `splice` leaves it where it is.
#[derive(Default)]
struct LocalBlocks {
    also: String,
    engineer: String,
    engineer_after_cleanup: bool,
    floor: String,
}

fn is_cleanup(line: &str) -> bool {
    line.starts_with("### Cleanup")
}

fn local_blocks(old: &[&str]) -> LocalBlocks {
    let mut out = LocalBlocks::default();
    let mut past_cleanup = false;
    let mut i = 0;
    while i < old.len() {
        let line = old[i].trim_start();
        let end = if line.starts_with("**Also:**") {
            let end = paragraph_from(old, i);
            out.also.extend(old[i..end].iter().copied());
            end
        } else if line.starts_with("**Added by engineer") {
            let end = block_from(old, i);
            out.engineer.extend(old[i..end].iter().copied());
            out.engineer_after_cleanup = past_cleanup;
            end
        } else if line.starts_with("**Assertion floor:**") {
            let end = paragraph_from(old, i);
            out.floor.extend(old[i..end].iter().copied());
            end
        } else {
            past_cleanup |= is_cleanup(old[i]);
            i + 1
        };
        i = end;
    }
    out
}

/// `section` (as `section()` writes it) with the old section's own blocks
/// put back (`LocalBlocks`).
fn with_local_blocks(section: &str, old: &[&str]) -> String {
    let local = local_blocks(old);
    if local.also.is_empty() && local.engineer.is_empty() && local.floor.is_empty() {
        return section.to_string();
    }
    let lines: Vec<&str> = section.split_inclusive('\n').collect();
    // After the heading and the blank lines under it.
    let mut under_heading = 1.min(lines.len());
    while under_heading < lines.len() && is_blank(lines[under_heading]) {
        under_heading += 1;
    }
    let cleanup = lines.iter().position(|l| is_cleanup(l)).unwrap_or(lines.len()).max(under_heading);
    let mut o = String::with_capacity(section.len() + local.also.len() + local.engineer.len() + local.floor.len());
    o.extend(lines[..under_heading].iter().copied());
    o.push_str(&local.also);
    o.extend(lines[under_heading..cleanup].iter().copied());
    if !local.engineer_after_cleanup {
        o.push_str(&local.engineer);
    }
    o.extend(lines[cleanup..].iter().copied());
    if local.engineer_after_cleanup {
        o.push_str(&local.engineer);
    }
    o.push_str(&local.floor);
    o
}

/// The tables whose rows the repo may add to, and whether a table's first
/// column is the row number TCM renumbers (`with_repo_rows`).
const REPO_ROW_TABLES: [(&str, bool); 3] =
    [("### Preconditions", true), ("### Test Data", false), ("### Cleanup", true)];

fn is_row(line: &str) -> bool {
    line.trim_start().starts_with('|')
}

/// A table's `|---|---|` line.
fn is_separator(line: &str) -> bool {
    let t = line.trim();
    t.contains('-') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ' | '\t'))
}

/// The data rows (header and separator left out) of the table right under
/// the first heading outside a code fence that starts with `heading`, as a
/// range of `lines`. `None` when there is no such heading.
fn table_rows(lines: &[&str], heading: &str) -> Option<std::ops::Range<usize>> {
    let mut fenced = false;
    let mut at = None;
    for (i, l) in lines.iter().enumerate() {
        if is_fence(l) {
            fenced = !fenced;
        } else if !fenced && l.starts_with(heading) {
            at = Some(i);
            break;
        }
    }
    let mut first = at? + 1;
    while first < lines.len() && is_blank(lines[first]) {
        first += 1;
    }
    let mut end = first;
    while end < lines.len() && is_row(lines[end]) {
        end += 1;
    }
    first = (first + 1).min(end);
    if first < end && is_separator(lines[first]) {
        first += 1;
    }
    Some(first..end)
}

/// What a row says after its number cell when `numbered`, else the whole
/// row, as written (escapes kept, line ending and outer spaces dropped).
fn row_text(line: &str, numbered: bool) -> &str {
    let t = line.trim();
    if !numbered {
        return t;
    }
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = match t.strip_suffix('|') {
        Some(r) if !r.ends_with('\\') => r,
        _ => t,
    };
    t.split_once('|').map_or(t, |(_, rest)| rest).trim()
}

/// `section` with the rows the repo added to the old section's tables put
/// back: every row that does not end `FROM_TCM` and is not one of the ADO
/// rows `section` writes anyway, after the ADO rows and before TCM's own,
/// renumbered. TCM's old rows are dropped - `section` has today's.
fn with_repo_rows(section: &str, old: &[&str]) -> String {
    let mut lines: Vec<String> = section.split_inclusive('\n').map(str::to_string).collect();
    for (heading, numbered) in REPO_ROW_TABLES {
        let Some(old_rows) = table_rows(old, heading) else { continue };
        let kept: Vec<&str> = old[old_rows]
            .iter()
            .map(|l| row_text(l, numbered))
            .filter(|t| !t.is_empty() && !t.ends_with(FROM_TCM))
            .collect();
        let ours: Vec<&str> = lines.iter().map(String::as_str).collect();
        let Some(new_rows) = table_rows(&ours, heading) else { continue };
        let (ado, tcm): (Vec<&str>, Vec<&str>) =
            ours[new_rows.clone()].iter().map(|l| row_text(l, numbered)).partition(|t| !t.ends_with(FROM_TCM));
        let kept: Vec<&str> = kept.into_iter().filter(|t| !ado.contains(t)).collect();
        if kept.is_empty() {
            continue;
        }
        let rows: Vec<String> = ado
            .iter()
            .chain(&kept)
            .chain(&tcm)
            .enumerate()
            .map(|(i, t)| if numbered { format!("| {} | {t} |\n", i + 1) } else { format!("{t}\n") })
            .collect();
        lines.splice(new_rows, rows);
    }
    lines.concat()
}

/// `existing` with case `id`'s section replaced by `section`, or `section`
/// appended when the file has none. Everything outside that section is kept
/// byte for byte, and so are the repo's own blocks inside it
/// (`local_blocks`) and the rows it added to its tables (`with_repo_rows`).
/// The old `**From TCM:**` block is TCM's, so `section`'s replaces it whole.
pub fn splice(existing: &str, id: i32, section: &str) -> String {
    let lines: Vec<&str> = existing.split_inclusive('\n').collect();
    if let Some(start) = lines.iter().position(|l| is_case_heading(l, id)) {
        let end = lines[start + 1..]
            .iter()
            .position(|l| is_h2(l))
            .map_or(lines.len(), |p| start + 1 + p);
        let old = &lines[start..end];
        let section = with_local_blocks(&with_repo_rows(section, old), old);
        let mut o = String::with_capacity(existing.len() + section.len());
        o.extend(lines[..start].iter().copied());
        o.push_str(&section);
        o.extend(lines[end..].iter().copied());
        return o;
    }
    let mut o = String::from(existing);
    if !o.is_empty() {
        let blank_end = o.ends_with("\n\n") || o.ends_with("\n\r\n");
        if !blank_end {
            if !o.ends_with('\n') {
                o.push('\n');
            }
            o.push('\n');
        }
    }
    o.push_str(section);
    o
}
