//! The one parser behind spec coverage: turns a spec document into an
//! inventory of its sections, and turns a reviewer's `reviewer_notes` free
//! text into structured citations. Pure - no I/O, no async - so coverage
//! math and sizing (later tasks) can be unit tested without a filesystem or
//! an AI call in the loop.
//!
//! Sections are read FROM the document, never taken from what the assistant
//! claims to have covered - an assistant that skipped a section would skip
//! it again while listing its own headings, silently hiding the gap.
//! Citations, by contrast, come from free text a reviewer typed, so parsing
//! is tolerant of minor formatting drift but anything that doesn't match a
//! known shape reads as `None` rather than being guessed at - unparseable
//! must never be miscounted as covered.

use regex::Regex;

pub struct Section {
    pub id: String,
    pub title: String,
    pub line: usize,
}

pub struct Inventory {
    pub lines: usize,
    pub sections: Vec<Section>,
}

/// Markdown heading: `#{1,6}` then required whitespace then the title.
fn markdown_heading(line: &str) -> Option<&str> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &line[hashes..];
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let title = rest.trim();
    if title.is_empty() {
        None
    } else {
        Some(title)
    }
}

/// A bare numbered heading such as `7.7 Copy from previous cycle` - specs
/// exported from Word have these with no markdown hashes. Anchored to
/// start-of-line only: `7.7` inside a sentence ("see section 7.7 for
/// details") must never become a section.
fn numbered_heading(line: &str) -> Option<(&str, &str)> {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 {
        return None;
    }
    let mut end = i;
    // Consume trailing `.\d+` groups: "7.7.1" etc.
    loop {
        if end < bytes.len() && bytes[end] == b'.' && end + 1 < bytes.len() && bytes[end + 1].is_ascii_digit() {
            end += 1;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
        } else {
            break;
        }
    }
    let id = &line[..end];
    let rest = &line[end..];
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let title = rest.trim();
    if title.is_empty() {
        None
    } else {
        Some((id, title))
    }
}

/// `AC-3: it does X` (case-insensitive marker, colon and rest optional).
fn ac_marker(line: &str) -> Option<String> {
    let re = Regex::new(r"(?i)^AC-(\d+)\b").unwrap();
    re.captures(line.trim_start()).map(|c| c[1].to_string())
}

/// `"7.7." -> "7.7"`: drop a trailing period specs sometimes carry over from
/// numbered-list export. Anything without a trailing `<digit>.` (e.g.
/// `"AC-3"`) is kept as-is.
pub fn normalize_section_id(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(stripped) = trimmed.strip_suffix('.') {
        if stripped.chars().next_back().is_some_and(|c| c.is_ascii_digit()) {
            return stripped.to_string();
        }
    }
    trimmed.to_string()
}

pub fn parse_inventory(text: &str) -> Inventory {
    let lines: Vec<&str> = text.lines().collect();
    let mut sections: Vec<Section> = vec![];
    let mut current_parent: Option<usize> = None; // index into sections

    for (i, raw_line) in lines.iter().enumerate() {
        let line_no = i + 1;
        if let Some(title) = markdown_heading(raw_line) {
            // A markdown heading may itself start with a number ("## 7.7 Title").
            let (id, title) = match numbered_heading(title) {
                Some((id, t)) => (normalize_section_id(id), t.to_string()),
                None => (title.to_string(), title.to_string()),
            };
            sections.push(Section { id, title, line: line_no });
            current_parent = Some(sections.len() - 1);
            continue;
        }
        if let Some((id, title)) = numbered_heading(raw_line) {
            let id = normalize_section_id(id);
            sections.push(Section { id, title: title.to_string(), line: line_no });
            current_parent = Some(sections.len() - 1);
            continue;
        }
        if let Some(n) = ac_marker(raw_line) {
            let parent_id = current_parent.map(|p| sections[p].id.clone()).unwrap_or_default();
            sections.push(Section {
                id: format!("{parent_id} (AC-{n})"),
                title: raw_line.trim().to_string(),
                line: line_no,
            });
            continue;
        }
    }

    Inventory { lines: lines.len(), sections }
}

pub struct SpecCitation {
    pub file: String,
    pub section: String,
    pub quote: Option<String>,
    pub exemption: Option<String>,
}

pub struct Citations {
    pub specs: Vec<SpecCitation>,
    pub has_code_ref: bool,
}

/// Parse `reviewer_notes` free text into structured citations.
///
/// `None` means nothing recognisable was found at all - distinct from
/// `Some(Citations { specs: vec![], .. })`, which means a code-only
/// citation was found. The distinction matters to callers (task 2+): the
/// former is "nothing to score", the latter is a deliberate non-spec claim.
pub fn parse_citations(reviewer_notes: &str) -> Option<Citations> {
    // "Spec: Step10.md 7.7 (AC-3)" - file, then section (which may itself
    // carry a trailing "(AC-n)"), then an optional quote or exemption.
    let spec_re = Regex::new(
        r"(?im)^\s*spec:\s*(\S+)\s+(.+?)\s*$",
    )
    .unwrap();
    let exemption_re = Regex::new(r"(?i)^(.*?)\s*-\s*no quotable text\s*\(([^)]*)\)\s*$").unwrap();
    let quote_re = Regex::new(r#"^\s*>\s*"(.*)"\s*$"#).unwrap();
    let code_re = Regex::new(r"(?im)^\s*code:\s*\S+").unwrap();

    let mut specs = vec![];
    let lines: Vec<&str> = reviewer_notes.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if let Some(caps) = spec_re.captures(line) {
            let file = caps[1].to_string();
            let mut section_raw = caps[2].trim().to_string();
            // Tolerate a trailing period on the whole citation line.
            if let Some(stripped) = section_raw.strip_suffix('.') {
                section_raw = stripped.to_string();
            }

            let (section, exemption) = match exemption_re.captures(&section_raw) {
                Some(ecaps) => (ecaps[1].trim().to_string(), Some(ecaps[2].trim().to_string())),
                None => (section_raw, None),
            };

            // A quote, if present, lives on the next non-blank line.
            let quote = lines[i + 1..]
                .iter()
                .find(|l| !l.trim().is_empty())
                .and_then(|l| quote_re.captures(l))
                .map(|c| c[1].to_string());

            specs.push(SpecCitation {
                file,
                section: normalize_section_id(&section),
                quote,
                exemption,
            });
        }
    }

    let has_code_ref = code_re.is_match(reviewer_notes);

    if specs.is_empty() && !has_code_ref {
        None
    } else {
        Some(Citations { specs, has_code_ref })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_headings_become_sections_with_their_line_numbers() {
        let inv = parse_inventory("# Intro\n\ntext\n\n## 7.7 Copy from previous cycle\n\nbody\n\n### 7.7.1 Empty state\n");
        assert_eq!(inv.lines, 9);
        let ids: Vec<&str> = inv.sections.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["Intro", "7.7", "7.7.1"]);
        assert_eq!(inv.sections[1].title, "Copy from previous cycle");
        assert_eq!(inv.sections[1].line, 5);
    }

    #[test]
    fn numbered_headings_without_hashes_are_found() {
        // Specs exported from Word often have bare "7.7 Title" lines.
        let inv = parse_inventory("7.7 Copy from previous cycle\nbody\n8.2 Archive\n");
        let ids: Vec<&str> = inv.sections.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["7.7", "8.2"]);
    }

    #[test]
    fn ac_markers_inside_a_section_become_child_sections() {
        let inv = parse_inventory("## 8.2 Archive\n\nAC-1: it archives\nAC-2: it restores\n");
        let ids: Vec<&str> = inv.sections.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["8.2", "8.2 (AC-1)", "8.2 (AC-2)"]);
    }

    #[test]
    fn prose_that_merely_mentions_a_number_is_not_a_section() {
        // "see section 7.7 for details" must not create a section.
        let inv = parse_inventory("Intro text mentioning 7.7 mid-sentence.\nAnd a version number 2.1 in prose.\n");
        assert!(inv.sections.is_empty(), "{:?}", inv.sections.iter().map(|s| &s.id).collect::<Vec<_>>());
    }

    #[test]
    fn a_standard_citation_parses_file_section_and_quote() {
        let c = parse_citations(
            "Checks the copy affordance.\nSpec: Step10.md 7.7 (AC-3)\n> \"Copy from previous cycle is offered only when a completed cycle exists.\"",
        ).unwrap();
        assert_eq!(c.specs.len(), 1);
        assert_eq!(c.specs[0].file, "Step10.md");
        assert_eq!(c.specs[0].section, "7.7 (AC-3)");
        assert!(c.specs[0].quote.as_deref().unwrap().starts_with("Copy from previous cycle"));
    }

    #[test]
    fn reasonable_variants_parse_and_garbage_reads_as_none() {
        // Tolerated: "Spec:" / "spec:" / extra spaces / trailing period on the section.
        assert!(parse_citations("spec:  Step10.md   7.9.").is_some());
        // Code-only citation: no spec entry, but has_code_ref is true.
        let code = parse_citations("Code: IndexModel.CanCopy").unwrap();
        assert!(code.specs.is_empty() && code.has_code_ref);
        // Nothing parseable at all.
        assert!(parse_citations("just prose with no citation").is_none());
    }

    #[test]
    fn the_fixed_exemption_form_is_recognised() {
        let c = parse_citations("Spec: Step10.md 7.9 - no quotable text (requirement is a state table)").unwrap();
        assert_eq!(c.specs[0].exemption.as_deref(), Some("requirement is a state table"));
        assert!(c.specs[0].quote.is_none());
    }
}
