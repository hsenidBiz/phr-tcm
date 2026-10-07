//! The per-feature test-case markdown (`suites/<seg>/test-cases/<stem>.md`).
//!
//! Pure string functions. `splice` is byte-preserving outside the section it
//! replaces: a file written by `/gen-test` (or hand-edited, or CRLF) keeps
//! every other section exactly as it was. Headings inside fenced code blocks
//! are not treated specially.

/// Everything one case section needs.
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

pub fn splice(existing: &str, id: i32, section: &str) -> String {
    let lines: Vec<&str> = existing.split_inclusive('\n').collect();
    if let Some(start) = lines.iter().position(|l| is_case_heading(l, id)) {
        let end = lines[start + 1..]
            .iter()
            .position(|l| is_h2(l))
            .map_or(lines.len(), |p| start + 1 + p);
        let mut o = String::with_capacity(existing.len() + section.len());
        o.extend(lines[..start].iter().copied());
        o.push_str(section);
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
