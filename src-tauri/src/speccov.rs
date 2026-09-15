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
    /// The raw document text, kept for Task 2's quote-verification join -
    /// `Section`s alone don't carry body text, and a quote can appear
    /// anywhere in the document relative to its citation's section.
    pub text: String,
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
    // A single trailing lowercase letter is part of the id when whitespace
    // follows: "3.4a Entry Limit Per Role" sits between 3.4 and 3.5, and
    // without this the heading fell through to title-as-id and could not
    // be cited as "3.4a" (round 8 dogfooding). One letter only - "3.4ab"
    // is a word, not an id.
    if end < bytes.len()
        && bytes[end].is_ascii_lowercase()
        && bytes.get(end + 1).is_some_and(|b| b.is_ascii_whitespace())
    {
        end += 1;
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

    Inventory { lines: lines.len(), sections, text: text.to_string() }
}

/// Join a quote's opening fragment (the text right after the `> "` marker,
/// on the line where it started) with continuation lines until one ends in
/// the closing `"`, bounded at 5 continuation lines - the feedback's own
/// example wraps a quote across lines, and whitespace-normalisation
/// downstream still matches once the pieces are joined with a space. An
/// opener that never closes within the bound reads as no quote (`None`),
/// never as a truncated or corrupted one.
fn accumulate_quote(first_fragment: String, following: &[&str]) -> Option<String> {
    if let Some(stripped) = first_fragment.strip_suffix('"') {
        return Some(stripped.to_string());
    }
    let mut acc = first_fragment;
    for line in following.iter().take(5) {
        if let Some(stripped) = line.strip_suffix('"') {
            acc.push(' ');
            acc.push_str(stripped.trim());
            return Some(acc);
        }
        acc.push(' ');
        acc.push_str(line.trim());
    }
    None
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
/// Split "Step9 - FDP.md 3.1" into file and section. The filename may
/// contain spaces - most of a real project's specs do - so the split point
/// is the end of the first token that looks like a file extension (a dot
/// followed by letters), NOT the first space. Splitting on whitespace
/// truncated "Step9 - FDP.md" to "FDP.md" and reported a document that
/// does not exist, which zeroed `covered` on a 227-case set (round 6
/// §3.1). "3.1" cannot be mistaken for an extension: its post-dot
/// character is a digit, and extensions must start with a letter.
fn split_file_and_section(rest: &str) -> (String, String) {
    let ext_split =
        Regex::new(r"^(?s)(.+?\.[A-Za-z][A-Za-z0-9]{0,5})(?:\s+(.*))?$").unwrap();
    if let Some(caps) = ext_split.captures(rest) {
        return (
            caps[1].to_string(),
            caps.get(2).map(|m| m.as_str()).unwrap_or("").trim().to_string(),
        );
    }
    // No extension anywhere: the old first-token split, so an
    // extension-less citation keeps meaning what it always meant.
    match rest.split_once(char::is_whitespace) {
        Some((file, section)) => (file.to_string(), section.trim().to_string()),
        None => (rest.to_string(), String::new()),
    }
}

/// Is there a markdown blockquote anywhere in the note? `parse_citations`
/// only reads a quote that sits directly beneath a `Spec:` line; this
/// answers the different question "did the writer quote SOMETHING" - which
/// is what turns "no quote" into "quote in the wrong place" (round 8 §4).
pub fn has_blockquote(notes: &str) -> bool {
    notes.lines().any(|l| l.trim_start().starts_with('>'))
}

/// The sentence to add to a bare-citation finding. When a blockquote exists
/// the problem is position, and saying "no quote" sends the writer hunting
/// for one that is already there - three of five did exactly that.
pub fn bare_citation_hint(notes: &str) -> &'static str {
    if has_blockquote(notes) {
        "a quote is present but not where the checker reads it - put the `Spec:` line \
         first and the quote directly beneath it as `> \"...\"`; a table or code block is \
         not a quote: use `Spec: <file> <section> - no quotable text (table/diagram | \
         code-not-prose)`"
    } else {
        "quote the source sentence beneath the `Spec:` line as `> \"...\"`, or state the \
         exemption in the fixed form `Spec: <file> <section> - no quotable text (<why>)`"
    }
}

pub fn parse_citations(reviewer_notes: &str) -> Option<Citations> {
    // "Spec: Step10.md 7.7 (AC-3)" - the whole tail is captured and the
    // file/section split happens in `split_file_and_section`, extension-
    // aware because filenames carry spaces.
    let spec_re = Regex::new(
        r"(?im)^\s*spec:\s*(.+?)\s*$",
    )
    .unwrap();
    // Dash before "no quotable text" may be an ASCII hyphen or a typographic
    // em/en dash - reviewers paste from Word, which autocorrects "-" to "—".
    // Missing a dash form here doesn't fail loudly: the whole tail (dash and
    // reason) silently leaks into `section` and `exemption` reads as None,
    // which is exactly the misparse this feature exists to catch.
    let exemption_re = Regex::new(r"(?i)^(.*?)\s*[-\u{2013}\u{2014}]\s*no quotable text\s*\(([^)]*)\)\s*$").unwrap();
    // Same-line quote: "Spec: F.md 7.1 > "text"" - a quote marker embedded
    // directly in what `spec_re` captured as the section, rather than on its
    // own line below. Must be split off BEFORE exemption-checking, or it
    // corrupts `section` into "7.1 > "text"" (round-5 ledger M5).
    let inline_quote_re = Regex::new(r#"^(.*?)\s*>\s*"(.*)$"#).unwrap();
    // A quote's opening line, closed or not: `> "text` with no requirement
    // that the closing `"` be on this same line - a quote may wrap across
    // several lines before it closes (round-5 §7's own example wraps).
    let quote_opener_re = Regex::new(r#"^\s*>\s*"(.*)$"#).unwrap();
    let code_re = Regex::new(r"(?im)^\s*code:\s*\S+").unwrap();

    // A segment after `;` is a SECOND document pointer only when it names a
    // document (carries a file extension). Round 7 §7.2: `Summary.md 5 ...;
    // Step3-Timeline.md 3.8` silently dropped the second pointer - it
    // neither resolved nor errored, so the second document read as
    // uncovered however many cases pointed at it. A semicolon inside
    // free-text detail ("employees; managers too") stays detail.
    let names_document_re = Regex::new(r"^(?s).+?\.[A-Za-z][A-Za-z0-9]{0,5}(?:\s|$)").unwrap();

    let mut specs = vec![];
    let lines: Vec<&str> = reviewer_notes.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if let Some(caps) = spec_re.captures(line) {
            let mut pointers: Vec<String> = vec![];
            for seg in caps[1].trim().split(';') {
                let seg = seg.trim();
                match pointers.last_mut() {
                    Some(last) if !names_document_re.is_match(seg) => {
                        // Not a new pointer - reattach the split-off detail.
                        last.push_str("; ");
                        last.push_str(seg);
                    }
                    _ if seg.is_empty() => {}
                    _ => pointers.push(seg.to_string()),
                }
            }

            for (pi, pointer) in pointers.iter().enumerate() {
                let (file, mut section_raw) = split_file_and_section(pointer);
                // Tolerate a trailing period on the whole citation line.
                if let Some(stripped) = section_raw.strip_suffix('.') {
                    section_raw = stripped.to_string();
                }

                if let Some(qcaps) = inline_quote_re.captures(&section_raw) {
                    let section = normalize_section_id(qcaps[1].trim());
                    let quote = accumulate_quote(qcaps[2].to_string(), &lines[i + 1..]);
                    specs.push(SpecCitation { file, section, quote, exemption: None });
                    continue;
                }

                let (section, exemption) = match exemption_re.captures(&section_raw) {
                    Some(ecaps) => (ecaps[1].trim().to_string(), Some(ecaps[2].trim().to_string())),
                    None => (section_raw, None),
                };

                // A quote, if present, opens on the next non-blank line and
                // may wrap across several more before its closing mark. It
                // belongs to the line's FIRST pointer - the quote follows
                // the line as a whole, and the primary citation is the one
                // it substantiates.
                let quote = (pi == 0)
                    .then(|| {
                        lines[i + 1..]
                            .iter()
                            .enumerate()
                            .find(|(_, l)| !l.trim().is_empty())
                            .and_then(|(offset, opener)| {
                                quote_opener_re
                                    .captures(opener)
                                    .map(|c| accumulate_quote(c[1].to_string(), &lines[i + 2 + offset..]))
                            })
                            .flatten()
                    })
                    .flatten();

                specs.push(SpecCitation {
                    file,
                    section: normalize_section_id(&section),
                    quote,
                    exemption,
                });
            }
        }
    }

    let has_code_ref = code_re.is_match(reviewer_notes);

    if specs.is_empty() && !has_code_ref {
        None
    } else {
        Some(Citations { specs, has_code_ref })
    }
}

/// Input to [`check_coverage`]: one or more parsed [`Inventory`]s (paired
/// with the file name the citations reference), the case set to score
/// against them, and the two free-text scope fields a reviewer may set on
/// the intake form.
pub struct CoverageInput<'a> {
    pub inventories: Vec<(String, Inventory)>,
    pub cases: &'a [crate::model::TestCase],
    pub sections_scope: &'a str,
    pub out_of_scope: &'a str,
}

/// Collapse runs of whitespace (including newlines) to a single space, so a
/// quote that wraps across lines in the markdown - or in the reviewer's
/// pasted copy of it - still matches.
fn normalize_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether a quote is verbatim in the (whitespace-normalised) document -
/// including an ELIDED quote, split on `...` or `…`, whose fragments must
/// each appear in order without overlapping. The guide has always said
/// "elide with an ellipsis instead" of rewriting inside quotation marks,
/// and the checker rejected exactly that form: 33 real repairs on two
/// drafts were nothing but removing elisions to appease it (round 8 §1).
/// Ordered non-overlapping fragments are still strictly stronger than a
/// pass: every fragment must be real, and the sequence must hold, so an
/// invented quote still fails.
fn quote_matches_document(quote: &str, doc_text_norm: &str) -> bool {
    let fragments: Vec<String> = quote
        .split("...")
        .flat_map(|part| part.split('\u{2026}'))
        .map(normalize_whitespace)
        .filter(|f| !f.is_empty())
        .collect();
    if fragments.is_empty() {
        // A quote that is nothing but ellipses claims nothing.
        return false;
    }
    let mut from = 0usize;
    for frag in &fragments {
        match doc_text_norm[from..].find(frag.as_str()) {
            Some(i) => from += i + frag.len(),
            None => return false,
        }
    }
    true
}

/// The trailing path segment, lowercased, so a citation of "Step10.md"
/// matches an inventory built from "C:\specs\Step10.md" case-insensitively.
fn file_basename_lower(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_lowercase()
}

/// The sub-heading a precise citation names after its parent section.
///
/// `"5.6a Redistribution - Mode: ratio_based" -> Some("Mode: ratio_based")`.
/// The separator is the plain " - " the writing guide already uses for the
/// exemption form, and the exemption is stripped before this sees it - so
/// what is left after the last one is a heading name or nothing.
fn sub_heading_of(section: &str) -> Option<&str> {
    let (_, tail) = section.rsplit_once(" - ")?;
    let tail = tail.trim();
    (!tail.is_empty()).then_some(tail)
}

/// `"7.7 (AC-3)" -> Some("7.7")`; sections with no AC suffix have no parent.
fn parent_section_id(section_id: &str) -> Option<String> {
    let re = Regex::new(r"(?i)^(.+?)\s*\(AC-\d+\)\s*$").unwrap();
    re.captures(section_id).map(|c| c[1].trim().to_string())
}

/// A bare section-id shape: `7`, `7.7`, `7.7.1`, optionally with a trailing
/// `(AC-n)`. Used to decide whether `sections_scope` is an enumerated list
/// (every token matches) or free text (no filtering at all).
fn is_section_id_shape(s: &str) -> bool {
    // A single trailing letter is a real heading style ("3.4a Entry Limit
    // Per Role" sits between 3.4 and 3.5) - round 8 dogfooding found those
    // uncitable. A RANGE of ids ("3.1-3.6", "3.1 to 3.6") also reads as an
    // id token, so a scope list written the way people actually write one
    // stays enumerated instead of collapsing to inert free text.
    const ID: &str = r"\d+(?:\.\d+)*[a-z]?(?:\s*\(AC-\d+\))?";
    Regex::new(&format!(
        r"(?i)^{ID}(?:\s*(?:-|\u{{2013}}|\u{{2014}}|to|through)\s*{ID})?$"
    ))
    .unwrap()
    .is_match(s)
}

/// A range token's two ends, when the token is one ("3.1-3.6",
/// "3.1 to 3.6"); None for a plain id.
fn scope_range_ends(token: &str) -> Option<(String, String)> {
    let re = Regex::new(
        r"(?i)^(\d+(?:\.\d+)*[a-z]?)\s*(?:-|\u{2013}|\u{2014}|\bto\b|\bthrough\b)\s*(\d+(?:\.\d+)*[a-z]?)$",
    )
    .unwrap();
    re.captures(token)
        .map(|c| (normalize_section_id(&c[1]), normalize_section_id(&c[2])))
}

/// Whether `id` is inside an enumerated scope set that may carry range
/// tokens: an exact member, or numerically within any range member.
fn in_enumerated_scope(set: &std::collections::HashSet<String>, id: &str) -> bool {
    if set.contains(id) {
        return true;
    }
    set.iter().any(|token| {
        scope_range_ends(token).is_some_and(|(lo, hi)| {
            parts_in_range(&id_numeric_parts(id), &id_numeric_parts(&lo), &id_numeric_parts(&hi))
                // Ranges are numeric; a named heading ("Implementation")
                // never falls inside one by accident.
                && id.chars().next().is_some_and(|c| c.is_ascii_digit())
        })
    })
}

/// `sections_scope` is treated as an enumerated in-scope list only when
/// EVERY comma/semicolon-separated token looks like a section id - per the
/// plan's "not a second source of truth about scope" rule, anything else
/// (a sentence, "everything", one stray non-id token) is read as free text
/// that excludes nothing, rather than guessed at.
pub(crate) fn parse_enumerated_scope(sections_scope: &str) -> Option<std::collections::HashSet<String>> {
    let tokens: Vec<String> = sections_scope
        .split([',', ';'])
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(normalize_section_id)
        .collect();
    if tokens.is_empty() || !tokens.iter().all(|t| is_section_id_shape(t)) {
        return None;
    }
    Some(tokens.into_iter().collect())
}

/// The leading numeric components of a section id: "8.12" -> [8, 12],
/// "UAC 8.9" -> [8, 9] (the prefix word is compared separately). Empty for
/// a heading with no number at all.
fn id_numeric_parts(id: &str) -> Vec<u64> {
    let re = Regex::new(r"(\d+(?:\.\d+)*)").unwrap();
    re.captures(id)
        .map(|c| c[1].split('.').filter_map(|p| p.parse().ok()).collect())
        .unwrap_or_default()
}

/// `parts` is inside [lo, hi] when it is >= lo and, truncated to hi's
/// length, <= hi - so "sections 4 to 7" takes 4, 4.15, 5 and 7.9 alike.
fn parts_in_range(parts: &[u64], lo: &[u64], hi: &[u64]) -> bool {
    if parts.is_empty() || lo.is_empty() || hi.is_empty() {
        return false;
    }
    let trunc: Vec<u64> = parts.iter().copied().take(hi.len()).collect();
    parts >= lo && trunc.as_slice() <= hi
}

/// `out_of_scope` is free text ("7.4 is deferred to phase 2, see JIRA-99")
/// - a section is excluded only on evidence that survives prose:
///   * an id at line start or after "section(s)" / "§" that EXISTS in the
///     inventory - as before;
///   * a RANGE in any of those positions ("sections 4 to 7", "8.9-8.12"),
///     expanded against the inventory (round 6 §3.3: the bucket was
///     unreachable precisely because real scope notes are written as
///     ranges);
///   * a prefixed id ("UAC 8.9") whose prefixed form is itself a known
///     section id - the prefix is the anchor;
///   * a non-numeric heading name ("Implementation") quoted verbatim.
/// A bare number loose in prose ("phase 2") still excludes nothing.
fn parse_out_of_scope_ids(
    out_of_scope: &str,
    known_ids: &std::collections::HashSet<String>,
) -> std::collections::HashSet<String> {
    const ID: &str = r"\d+(?:\.\d+)*[a-z]?(?:\s*\(AC-\d+\))?";
    let token_re = Regex::new(&format!(
        r"(?i)(?:^|\bsections?\s+|§§?\s*|\b([A-Za-z][A-Za-z0-9_-]*)\s+)({ID})(?:\s*(?:to|through|[-\u{{2013}}\u{{2014}}])\s*({ID}))?"
    ))
    .unwrap();
    let mut ids = std::collections::HashSet::new();
    for line in out_of_scope.lines() {
        for caps in token_re.captures_iter(line) {
            let prefix = caps.get(1).map(|m| m.as_str().trim().to_string());
            let a = normalize_section_id(&caps[2]);
            let b = caps.get(3).map(|m| normalize_section_id(m.as_str()));

            // With a prefix word, the anchored family is "<prefix> <n>":
            // only ids the inventory spells that way qualify, so "phase 2"
            // stays prose while "UAC 8.9" reaches the UAC sections.
            let family_id = |n: &str| -> Option<String> {
                match &prefix {
                    None => known_ids.contains(n).then(|| n.to_string()),
                    Some(p) => {
                        let candidate = normalize_section_id(&format!("{p} {n}"));
                        known_ids
                            .iter()
                            .find(|k| k.eq_ignore_ascii_case(&candidate))
                            .cloned()
                    }
                }
            };

            match b {
                None => {
                    if let Some(id) = family_id(&a) {
                        ids.insert(id);
                    }
                }
                Some(b) => {
                    // A range: expand against the inventory. The prefix (or
                    // its absence) must match each candidate the same way a
                    // single id would.
                    let lo = id_numeric_parts(&a);
                    let hi = id_numeric_parts(&b);
                    for k in known_ids {
                        let prefix_ok = match &prefix {
                            None => k.chars().next().is_some_and(|c| c.is_ascii_digit()),
                            Some(p) => k.to_lowercase().starts_with(&format!("{} ", p.to_lowercase())),
                        };
                        if prefix_ok && parts_in_range(&id_numeric_parts(k), &lo, &hi) {
                            ids.insert(k.clone());
                        }
                    }
                }
            }
        }
    }
    // Non-numeric headings ("Implementation", "Open Items") are excluded
    // when named verbatim - there is no id to anchor on, so the full name
    // is the evidence.
    let text_lower = out_of_scope.to_lowercase();
    // A slash-list names every item in it (round 7 §8): "Audience:
    // Employees / Managers / Reviewers" must reach "Audience: Managers"
    // and "Audience: Reviewers", not only the variant that happens to
    // appear verbatim - matching one out of three made the sentence do
    // the opposite of what it said.
    let slash_match = |k_lower: &str| -> bool {
        let Some((head, tail)) = k_lower.split_once(':') else { return false };
        let head = format!("{head}:");
        let tail = tail.trim();
        if tail.is_empty() {
            return false;
        }
        text_lower
            .lines()
            .filter(|l| l.contains('/'))
            .filter_map(|l| l.find(&head).map(|at| &l[at + head.len()..]))
            .any(|after| after.split('/').take(6).any(|part| part.trim().starts_with(tail)))
    };
    for k in known_ids {
        let k_lower = k.to_lowercase();
        if k.chars().next().is_some_and(|c| !c.is_ascii_digit())
            && k.len() >= 4
            && (text_lower.contains(&k_lower) || slash_match(&k_lower))
        {
            ids.insert(k.clone());
        }
    }
    ids
}

/// Join a document inventory against a case set's `Spec:` citations into six
/// findings buckets (never a `warnings` key - see module docs and the plan's
/// "findings, not warnings" register rule). Not a hard failure and not a
/// second source of truth about scope: `sections_scope` / `out_of_scope`
/// only move sections between `uncovered` and `excluded_by_plan`, they never
/// suppress a citation or invent one.
pub fn check_coverage(input: CoverageInput) -> serde_json::Value {
    let sections_in_document: usize = input.inventories.iter().map(|(_, inv)| inv.sections.len()).sum();
    // The total reads as a per-document count when several files are
    // supplied (round 6 §3.3, cosmetic) - the breakdown says which is which.
    let sections_per_document: std::collections::BTreeMap<String, usize> = input
        .inventories
        .iter()
        .map(|(name, inv)| (name.clone(), inv.sections.len()))
        .collect();

    let known_ids: std::collections::HashSet<String> = input
        .inventories
        .iter()
        .flat_map(|(_, inv)| inv.sections.iter().map(|s| s.id.clone()))
        .collect();

    let enumerated_scope = parse_enumerated_scope(input.sections_scope);
    let out_of_scope_ids = parse_out_of_scope_ids(input.out_of_scope, &known_ids);
    let is_excluded = |id: &str| -> bool {
        enumerated_scope.as_ref().is_some_and(|set| !in_enumerated_scope(set, id))
            || out_of_scope_ids.contains(id)
    };

    // Documents keyed by basename, each paired with its display name (as
    // given by the caller) and its whitespace-normalised full text for
    // quote matching.
    let docs: Vec<(String, String, &Inventory, String)> = input
        .inventories
        .iter()
        .map(|(name, inv)| (file_basename_lower(name), name.clone(), inv, normalize_whitespace(&inv.text)))
        .collect();

    // A normalized section id is only a unique key across ALL documents by
    // coincidence - two specs can both have a "7.1". Qualify the id with
    // its file's display name whenever more than one inventory has it, so
    // "cite A.md 7.1" doesn't silently mark B.md's unrelated 7.1 as
    // covered too (reviewer-reported: `uncovered` came back empty even
    // though B's 7.1 was never cited).
    let mut id_files: std::collections::HashMap<String, std::collections::HashSet<String>> = std::collections::HashMap::new();
    for (file_key, _, inv, _) in &docs {
        for sec in &inv.sections {
            id_files.entry(sec.id.clone()).or_default().insert(file_key.clone());
        }
    }
    let qualify = |file_display: &str, id: &str| -> String {
        if id_files.get(id).is_some_and(|files| files.len() > 1) {
            format!("{file_display} {id}")
        } else {
            id.to_string()
        }
    };

    let mut covered: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
    let mut unattributed: Vec<String> = vec![];
    let mut cited_but_absent: Vec<String> = vec![];
    let mut quote_not_in_document: Vec<String> = vec![];
    let mut cited_without_quote: Vec<String> = vec![];

    for case in input.cases {
        let Some(citations) = parse_citations(&case.reviewer_notes) else {
            unattributed.push(format!("{} — no spec citation found", case.title));
            continue;
        };
        if citations.specs.is_empty() {
            // parse_citations only returns Some with empty specs when a
            // code-only citation was found (has_code_ref) - a deliberate
            // non-spec claim, not "nothing to score", but still not spec
            // coverage of any section.
            unattributed.push(format!("{} — cites code, not spec", case.title));
            continue;
        }

        for spec in &citations.specs {
            let file_key = file_basename_lower(&spec.file);
            let Some((_, doc_display, doc, doc_text_norm)) = docs.iter().find(|(base, _, _, _)| *base == file_key)
            else {
                cited_but_absent.push(format!("{} — cited by '{}', no such document", spec.section, case.title));
                continue;
            };

            // A citation of a child AC section covers that section if the
            // document actually has it; otherwise it falls back to covering
            // the parent section (if THAT exists). An AC the reviewer cited
            // but the parser didn't find as its own heading must not read
            // as cited_but_absent when the parent section it lives under is
            // right there in the inventory.
            //
            // Third fallback: a citation that BEGINS with a real heading and
            // carries a free-text locator after it - "Implementation section
            // 5 DATA view, SUPERVISOR_NAME" - resolves to that heading. In a
            // near-structureless document everything worth citing lives
            // inside one heading, and treating the locator as part of the
            // section name reported 331 accurate citations as absent (round
            // 6 §3.2). Longest heading wins, and the match must end on a
            // word boundary so "7.1" never claims a citation of "7.10".
            let resolved = doc
                .sections
                .iter()
                .find(|s| s.id == spec.section)
                .or_else(|| parent_section_id(&spec.section).and_then(|p| doc.sections.iter().find(|s| s.id == p)))
                .or_else(|| {
                    doc.sections
                        .iter()
                        .filter(|s| {
                            // .get() rather than indexing: a multi-byte
                            // character at the cut is "no match", not a panic.
                            spec.section.len() > s.id.len()
                                && spec.section
                                    .get(..s.id.len())
                                    .is_some_and(|head| head.eq_ignore_ascii_case(&s.id))
                                && spec.section
                                    .get(s.id.len()..)
                                    .is_some_and(|tail| tail.starts_with(char::is_whitespace))
                        })
                        .max_by_key(|s| s.id.len())
                })
                .or_else(|| {
                    // Round 7 §7.1, the mirror of the fallback above: the
                    // citation is an exact PREFIX of the heading - the
                    // author stopped before a parenthetical the heading
                    // carries ("(SYS-01 - SYS-05)"). Boundary discipline
                    // as ever: the heading's next character must be a word
                    // boundary, so "7.1" never claims "7.10". Only an
                    // UNAMBIGUOUS prefix resolves - two headings sharing
                    // it would make this a guess, not a match.
                    let mut hits = doc.sections.iter().filter(|s| {
                        s.id.len() > spec.section.len()
                            && s.id
                                .get(..spec.section.len())
                                .is_some_and(|head| head.eq_ignore_ascii_case(&spec.section))
                            && s.id
                                .get(spec.section.len()..)
                                .is_some_and(|tail| tail.starts_with(|c: char| c.is_whitespace() || c == '('))
                    });
                    let first = hits.next();
                    if hits.next().is_some() { None } else { first }
                });

            match resolved {
                Some(sec) => {
                    covered
                        .entry(qualify(doc_display, &sec.id))
                        .or_default()
                        .push(case.title.clone());
                    // Round 8 §15: a precise citation names the parent
                    // section AND the sub-heading beneath it. Crediting only
                    // the parent listed that sub-heading as a gap, so the
                    // more exactly a writer cited, the more holes their
                    // coverage report grew - the incentive ran backwards.
                    // Only the sub-heading actually NAMED is credited; its
                    // siblings stay uncovered, because they are.
                    if let Some(sub) = sub_heading_of(&spec.section) {
                        if let Some(child) = doc
                            .sections
                            .iter()
                            .find(|s| s.id != sec.id && s.id.eq_ignore_ascii_case(sub))
                        {
                            covered
                                .entry(qualify(doc_display, &child.id))
                                .or_default()
                                .push(case.title.clone());
                        }
                    }
                }
                None => {
                    // Name the near-miss: "nearly right" and "plain wrong"
                    // read identically otherwise, and telling them apart
                    // cost a six-probe ladder in the field (round 7 §7.1).
                    let closest = doc
                        .sections
                        .iter()
                        .map(|s| {
                            let n = s
                                .id
                                .chars()
                                .zip(spec.section.chars())
                                .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
                                .count();
                            (n, s)
                        })
                        .filter(|(n, _)| *n >= 4 && *n * 2 >= spec.section.chars().count())
                        .max_by_key(|(n, _)| *n)
                        .map(|(_, s)| format!(" (closest heading: \"{}\")", s.id));
                    cited_but_absent.push(format!(
                        "{} — cited by '{}', no such section in {}{}",
                        spec.section,
                        case.title,
                        doc_display,
                        closest.unwrap_or_default()
                    ));
                }
            }

            if let Some(quote) = &spec.quote {
                if !quote_matches_document(quote, doc_text_norm) {
                    quote_not_in_document.push(format!("{} — quoted text not found in file", case.title));
                }
            }
            // Round 7 §9: a bare `Spec:` line - no quote, no exemption -
            // was invisible HERE and reported only by validate_cases, so
            // the tool named for citation checking had a blind spot in the
            // middle of its own job. The parse already knows; report it.
            if spec.quote.is_none() && spec.exemption.is_none() {
                cited_without_quote.push(format!(
                    "{} — Spec: {} {} has no quote and no exemption - {}",
                    case.title,
                    spec.file,
                    spec.section,
                    bare_citation_hint(&case.reviewer_notes)
                ));
            }
        }
    }

    let mut uncovered: Vec<String> = vec![];
    let mut excluded_by_plan: Vec<String> = vec![];
    for (name, inv) in &input.inventories {
        // In enumerated mode a NAMED sub-heading ("On submit", "Pre-submit
        // validation") is not in the id list and read as excluded - noise
        // that buried the real exclusions when its parent ("3.6") was in
        // scope. Sections arrive in line order, so a non-id heading
        // inherits the verdict of the nearest preceding id-shaped one.
        let mut inherited_excluded: Option<bool> = None;
        for sec in &inv.sections {
            let key = qualify(name, &sec.id);
            let excluded = if is_section_id_shape(&sec.id) {
                let e = is_excluded(&sec.id);
                inherited_excluded = Some(e);
                e
            } else if enumerated_scope.is_some() {
                inherited_excluded.unwrap_or_else(|| is_excluded(&sec.id))
            } else {
                is_excluded(&sec.id)
            };
            // Evidence beats exclusion (round 7 §8): a section with
            // covering cases listed under `covered` AND under
            // `excluded_by_plan` is a contradiction the report can resolve
            // itself - the free-text scope match was over-eager, the
            // citations are concrete.
            if covered.contains_key(&key) {
                continue;
            }
            if excluded {
                excluded_by_plan.push(format!("{key} — excluded by the plan's scope"));
            } else {
                uncovered.push(key);
            }
        }
    }

    serde_json::json!({
        "sections_in_document": sections_in_document,
        "sections_per_document": sections_per_document,
        "covered": covered,
        "uncovered": uncovered,
        "unattributed": unattributed,
        "cited_but_absent": cited_but_absent,
        "quote_not_in_document": quote_not_in_document,
        "cited_without_quote": cited_without_quote,
        "excluded_by_plan": excluded_by_plan,
    })
}
