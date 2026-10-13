//! The per-feature test-case markdown (`suites/<seg>/test-cases/<stem>.md`).
//!
//! Pure string functions. `splice` is byte-preserving outside the section it
//! replaces: a file written by `/gen-test` (or hand-edited, or CRLF) keeps
//! every other section exactly as it was. Headings inside fenced code blocks
//! are not treated specially.

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
    pub side: String,
    pub navigation_captured: bool,
    pub feature_title: String,
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
    o.push_str(&format!("\n**Side:** {} \u{2014} mapped in TCM.\n\n", one_line(&doc.side)));
    if doc.navigation_captured {
        o.push_str("**Navigation:** captured in src/navigation.json.\n\n");
    } else {
        o.push_str(
            "**Navigation:** not yet captured - capture it in this repo before refactoring.\n\n",
        );
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
    o.push_str("\n### Cleanup / Postconditions\n\n| # | Action |\n|---|--------|\n\n");
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

/// A paragraph starting at `lines[at]`, with the blank lines after it.
fn paragraph_from(lines: &[&str], at: usize) -> usize {
    let mut end = at + 1;
    while end < lines.len() && !is_blank(lines[end]) && !lines[end].starts_with('#') {
        end += 1;
    }
    while end < lines.len() && is_blank(lines[end]) {
        end += 1;
    }
    end
}

/// The blocks of a case section the repo writes itself, which a re-export
/// keeps exactly as they are (byte for byte), in the repo's own places:
/// - `**Also:** ...`, the line `/gen-test --also` writes under the heading;
/// - `**Added by engineer (not from ADO):**` and everything after it up to
///   the next heading: the steps an engineer added, and their notes;
/// - an `**Assertion floor:** ...` line that `npm run spec:floor` appended.
///
/// A `## Amendment` section is a level-two heading of its own, so it is
/// never inside a case section and `splice` leaves it where it is.
#[derive(Default)]
struct LocalBlocks {
    also: String,
    engineer: String,
    floor: String,
}

fn local_blocks(old: &[&str]) -> LocalBlocks {
    let mut out = LocalBlocks::default();
    let mut i = 0;
    while i < old.len() {
        let line = old[i].trim_start();
        let end = if line.starts_with("**Also:**") {
            let end = paragraph_from(old, i);
            out.also.extend(old[i..end].iter().copied());
            end
        } else if line.starts_with("**Added by engineer") {
            let mut end = i + 1;
            while end < old.len() && !old[end].starts_with('#') {
                end += 1;
            }
            out.engineer.extend(old[i..end].iter().copied());
            end
        } else if line.starts_with("**Assertion floor:**") {
            let end = paragraph_from(old, i);
            out.floor.extend(old[i..end].iter().copied());
            end
        } else {
            i + 1
        };
        i = end;
    }
    out
}

/// `section` (as `section()` writes it) with the old section's own blocks
/// put back: Also under the heading, the engineer's steps before Cleanup,
/// the assertion floor at the end.
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
    let cleanup = lines.iter().position(|l| l.starts_with("### Cleanup")).unwrap_or(lines.len());
    let mut o = String::with_capacity(section.len() + local.also.len() + local.engineer.len() + local.floor.len());
    o.extend(lines[..under_heading].iter().copied());
    o.push_str(&local.also);
    o.extend(lines[under_heading..cleanup.max(under_heading)].iter().copied());
    o.push_str(&local.engineer);
    o.extend(lines[cleanup.max(under_heading)..].iter().copied());
    o.push_str(&local.floor);
    o
}

/// `existing` with case `id`'s section replaced by `section`, or `section`
/// appended when the file has none. Everything outside that section is kept
/// byte for byte, and so are the repo's own blocks inside it
/// (`local_blocks`).
pub fn splice(existing: &str, id: i32, section: &str) -> String {
    let lines: Vec<&str> = existing.split_inclusive('\n').collect();
    if let Some(start) = lines.iter().position(|l| is_case_heading(l, id)) {
        let end = lines[start + 1..]
            .iter()
            .position(|l| is_h2(l))
            .map_or(lines.len(), |p| start + 1 + p);
        let section = with_local_blocks(section, &lines[start..end]);
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
