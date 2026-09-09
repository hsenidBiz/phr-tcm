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
    fn a_quote_wrapping_across_lines_is_accumulated_and_matches_the_document() {
        // The feedback's own §7 example wraps the quote across two lines -
        // the closing `"` isn't on the same line as the opening one.
        let notes = "Spec: Step10.md 7.7 (AC-3)\n\
                     > \"Copy from previous cycle is offered only when a completed\n\
                        cycle exists for the same appraisal type.\"";
        let c = parse_citations(notes).unwrap();
        assert_eq!(
            c.specs[0].quote.as_deref(),
            Some("Copy from previous cycle is offered only when a completed cycle exists for the same appraisal type.")
        );
        let inv = parse_inventory(
            "## 7.7 Copy from previous cycle\n\n\
             Copy from previous cycle is offered only when a completed\n\
             cycle exists for the same appraisal type.\n",
        );
        let cases = vec![case("Copy offered", notes)];
        let v = check_coverage(CoverageInput {
            inventories: vec![("Step10.md".into(), inv)],
            cases: &cases,
            sections_scope: "",
            out_of_scope: "",
        });
        assert_eq!(v["quote_not_in_document"], serde_json::json!(Vec::<String>::new()), "{v}");
    }

    // ---- round 7 §§7-9 --------------------------------------------------

    /// §7.1: a citation that is an exact PREFIX of the real heading
    /// resolves - `5. Cycle Stage Level Notifications` reaches the heading
    /// `5. Cycle Stage Level Notifications (SYS-01 -> SYS-05)`. Round 6
    /// §3.2 made trailing detail AFTER a complete heading resolve; the
    /// mirror (an incomplete heading) cost a six-probe ladder.
    #[test]
    fn a_citation_that_prefixes_the_heading_resolves() {
        let inv = parse_inventory("## 5. Cycle Stage Level Notifications (SYS-01 - SYS-05)\n\nbody\n");
        let cases = vec![case("Send To", "Spec: Summary.md 5. Cycle Stage Level Notifications")];
        let v = check_coverage(CoverageInput {
            inventories: vec![("Summary.md".into(), inv)],
            cases: &cases,
            sections_scope: "",
            out_of_scope: "",
        });
        assert_eq!(v["cited_but_absent"].as_array().unwrap().len(), 0, "{v}");
        assert_eq!(v["covered"].as_object().unwrap().len(), 1, "{v}");
    }

    /// The prefix rule must not let `7.1` claim `7.10` - the boundary after
    /// the citation has to be a real word boundary in the heading.
    #[test]
    fn a_numeric_prefix_does_not_claim_a_longer_section_number() {
        let inv = parse_inventory("## 7.10 Archive rules\n\nbody\n");
        let cases = vec![case("Archive", "Spec: S.md 7.1")];
        let v = check_coverage(CoverageInput {
            inventories: vec![("S.md".into(), inv)],
            cases: &cases,
            sections_scope: "",
            out_of_scope: "",
        });
        assert_eq!(v["cited_but_absent"].as_array().unwrap().len(), 1, "{v}");
    }

    /// §7.1's other half: when a section is nearly right, the error names
    /// the closest heading instead of leaving the author to probe for it.
    #[test]
    fn a_near_miss_names_the_closest_heading() {
        let inv = parse_inventory("## 5. Cycle Stage Level Notifications (SYS-01 - SYS-05)\n\nbody\n");
        // A typo ("Notifcations") - not a prefix, but close.
        let cases = vec![case("Send To", "Spec: Summary.md 5. Cycle Stage Level Notifcations")];
        let v = check_coverage(CoverageInput {
            inventories: vec![("Summary.md".into(), inv)],
            cases: &cases,
            sections_scope: "",
            out_of_scope: "",
        });
        let absent = v["cited_but_absent"].as_array().unwrap();
        assert_eq!(absent.len(), 1, "{v}");
        assert!(
            absent[0].as_str().unwrap().contains("closest heading"),
            "the near-miss should be named: {v}"
        );
    }

    /// §7.2: a `;` citation names two documents - both pointers resolve,
    /// not just the first. Half-honoured was the one misleading option.
    #[test]
    fn a_semicolon_citation_resolves_both_documents() {
        let a = parse_inventory("## 5. Notifications\n\nbody\n");
        let b = parse_inventory("## 3.8 Send To\n\nbody\n");
        let cases = vec![case(
            "Send To audience",
            "Spec: Summary.md 5. Notifications; Step3-Timeline.md 3.8 Send To",
        )];
        let v = check_coverage(CoverageInput {
            inventories: vec![("Summary.md".into(), a), ("Step3-Timeline.md".into(), b)],
            cases: &cases,
            sections_scope: "",
            out_of_scope: "",
        });
        assert_eq!(v["cited_but_absent"].as_array().unwrap().len(), 0, "{v}");
        assert_eq!(v["covered"].as_object().unwrap().len(), 2, "both documents covered: {v}");
        assert_eq!(v["uncovered"].as_array().unwrap().len(), 0, "{v}");
    }

    /// A `;` inside free-text detail (no document after it) must NOT be
    /// split into a phantom second pointer.
    #[test]
    fn a_semicolon_inside_detail_is_not_a_second_pointer() {
        let inv = parse_inventory("## 5. Notifications\n\nbody\n");
        let cases = vec![case("Send To", "Spec: Summary.md 5. Notifications - employees; managers too")];
        let v = check_coverage(CoverageInput {
            inventories: vec![("Summary.md".into(), inv)],
            cases: &cases,
            sections_scope: "",
            out_of_scope: "",
        });
        assert_eq!(v["cited_but_absent"].as_array().unwrap().len(), 0, "{v}");
        assert_eq!(v["covered"].as_object().unwrap().len(), 1, "{v}");
    }

    /// §8: a section with covering cases is never ALSO excluded - the
    /// evidence wins over an over-eager scope match. The Alerts run listed
    /// the same Implementation section under `covered` (11 cases) and
    /// `excluded_by_plan` at once.
    #[test]
    fn covering_evidence_beats_an_out_of_scope_match() {
        let inv = parse_inventory("## Implementation\n\nbody\n");
        let cases = vec![case("Impl check", "Spec: S.md Implementation")];
        let v = check_coverage(CoverageInput {
            inventories: vec![("S.md".into(), inv)],
            cases: &cases,
            sections_scope: "",
            out_of_scope: "covered through their parent Implementation section",
        });
        assert_eq!(v["covered"].as_object().unwrap().len(), 1, "{v}");
        assert_eq!(
            v["excluded_by_plan"].as_array().unwrap().len(),
            0,
            "a covered section cannot be excluded too: {v}"
        );
    }

    /// §8: a slash-list names every item in it. "Audience: Employees /
    /// Managers / Reviewers" excluded only the first and left the other two
    /// uncovered - the sentence achieved its own opposite.
    #[test]
    fn a_slash_list_excludes_every_named_section() {
        let inv = parse_inventory(
            "## Audience: Employees\n\nbody\n\n## Audience: Managers\n\nbody\n\n## Audience: Reviewers\n\nbody\n",
        );
        let cases: Vec<crate::model::TestCase> = vec![];
        let v = check_coverage(CoverageInput {
            inventories: vec![("S.md".into(), inv)],
            cases: &cases,
            sections_scope: "",
            out_of_scope: "the Audience: Employees / Managers / Reviewers subsections contain only SQL",
        });
        assert_eq!(v["excluded_by_plan"].as_array().unwrap().len(), 3, "{v}");
        assert_eq!(v["uncovered"].as_array().unwrap().len(), 0, "{v}");
    }

    /// §9: a bare `Spec:` line - no quote, no exemption - is a finding of
    /// THIS tool, not something only validate_cases mentions. The Alerts
    /// draft passed with both citation lists empty while 14 cases carried
    /// exactly this.
    #[test]
    fn a_citation_with_no_quote_and_no_exemption_is_reported() {
        let inv = parse_inventory("## 7.7 Copy\n\nCopy body text.\n");
        let cases = vec![
            case("Bare", "Spec: S.md 7.7"),
            case("Quoted", "Spec: S.md 7.7\n> \"Copy body text.\""),
            case("Exempt", "Spec: S.md 7.7 - no quotable text (state table)"),
        ];
        let v = check_coverage(CoverageInput {
            inventories: vec![("S.md".into(), inv)],
            cases: &cases,
            sections_scope: "",
            out_of_scope: "",
        });
        let bare = v["cited_without_quote"].as_array().expect("fourth list present");
        assert_eq!(bare.len(), 1, "{v}");
        assert!(bare[0].as_str().unwrap().contains("Bare"), "{v}");
    }

    #[test]
    fn a_same_line_quote_does_not_corrupt_the_section() {
        // Round-5 ledger M5: "Spec: F.md 7.1 > "q"" used to leave `section`
        // as the corrupted "7.1 > "q"" instead of splitting off the quote.
        let c = parse_citations("Spec: F.md 7.1 > \"q\"").unwrap();
        assert_eq!(c.specs[0].section, "7.1");
        assert_eq!(c.specs[0].quote.as_deref(), Some("q"));
    }

    #[test]
    fn an_unterminated_quote_gives_up_after_five_lines_without_corrupting_anything() {
        let notes = "Spec: Step10.md 7.9\n\
                     > \"This quote never closes\n\
                     line 2\nline 3\nline 4\nline 5\nline 6";
        let c = parse_citations(notes).unwrap();
        assert_eq!(c.specs[0].section, "7.9", "section must stay intact even when the quote never resolves");
        assert_eq!(c.specs[0].quote, None, "an opener with no close inside the bound must read as no quote");
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

    #[test]
    fn the_exemption_form_accepts_any_dash() {
        // Reviewers paste from Word, which autocorrects "-" to an em/en
        // dash. Missing a form here used to leak the dash and reason text
        // into `section` and read `exemption` as None - a silent misparse.
        for dash in ["-", "\u{2013}", "\u{2014}"] {
            let notes = format!("Spec: Step10.md 7.9 {dash} no quotable text (state table)");
            let c = parse_citations(&notes).unwrap_or_else(|| panic!("dash {dash:?} should parse"));
            assert_eq!(c.specs[0].section, "7.9", "dash {dash:?} leaked into section");
            assert_eq!(c.specs[0].exemption.as_deref(), Some("state table"), "dash {dash:?}");
        }
    }

    fn case(title: &str, notes: &str) -> crate::model::TestCase {
        crate::model::TestCase {
            title: title.to_string(),
            reviewer_notes: notes.to_string(),
            automation_status: "Not Automated".into(),
            ..Default::default()
        }
    }

    #[test]
    fn covered_and_uncovered_split_on_citations() {
        let inv = parse_inventory("## 7.1 List\n## 7.4 Export\n## 7.7 Copy\n");
        let cases = vec![case("List loads", "Spec: S.md 7.1"), case("Copy offered", "Spec: S.md 7.7")];
        let v = check_coverage(CoverageInput {
            inventories: vec![("S.md".into(), inv)],
            cases: &cases,
            sections_scope: "",
            out_of_scope: "",
        });
        assert_eq!(v["sections_in_document"], serde_json::json!(3));
        assert_eq!(v["uncovered"], serde_json::json!(["7.4"]));
        assert_eq!(v["covered"]["7.1"], serde_json::json!(["List loads"]));
        assert_eq!(v["covered"]["7.7"], serde_json::json!(["Copy offered"]));
    }

    #[test]
    fn a_case_without_a_parseable_citation_is_unattributed_not_a_gap() {
        let inv = parse_inventory("## 7.1 List\n## 7.4 Export\n");
        let cases = vec![
            case("Undocumented case", "just prose, no Spec: line"),
            case("Cites a ghost section", "Spec: S.md 9.9"),
        ];
        let v = check_coverage(CoverageInput {
            inventories: vec![("S.md".into(), inv)],
            cases: &cases,
            sections_scope: "",
            out_of_scope: "",
        });
        assert_eq!(v["unattributed"], serde_json::json!(["Undocumented case — no spec citation found"]));
        assert_eq!(
            v["cited_but_absent"],
            serde_json::json!(["9.9 — cited by 'Cites a ghost section', no such section in S.md"])
        );
        // A ghost citation must never be miscounted as coverage.
        assert_eq!(v["covered"].as_object().unwrap().len(), 0);
    }

    #[test]
    fn a_code_only_citation_is_unattributed_as_a_deliberate_non_spec_claim() {
        // Distinct from "nothing to score": has_code_ref is a claim the
        // reviewer made on purpose, but it still covers no spec section.
        let cases = vec![case("Backed by code only", "Code: IndexModel.CanCopy")];
        let v = check_coverage(CoverageInput {
            inventories: vec![],
            cases: &cases,
            sections_scope: "",
            out_of_scope: "",
        });
        assert_eq!(v["unattributed"], serde_json::json!(["Backed by code only — cites code, not spec"]));
    }

    #[test]
    fn a_quote_that_is_not_in_the_document_is_reported() {
        let inv = parse_inventory("## 7.1 List\n\nThe list refreshes automatically\nwhen data changes.\n");
        let cases = vec![
            case(
                "Verbatim",
                "Spec: S.md 7.1\n> \"The list refreshes automatically when data changes.\"",
            ),
            case("Paraphrase", "Spec: S.md 7.1\n> \"The list updates itself instantly.\""),
        ];
        let v = check_coverage(CoverageInput {
            inventories: vec![("S.md".into(), inv)],
            cases: &cases,
            sections_scope: "",
            out_of_scope: "",
        });
        // Found (whitespace-normalised across the line wrap) -> not reported.
        assert_eq!(v["quote_not_in_document"], serde_json::json!(["Paraphrase — quoted text not found in file"]));
    }

    #[test]
    fn an_ac_citation_falls_back_to_its_parent_section() {
        // Doc only has AC-1 under 8.2; the case cites AC-3, which the
        // parser never saw as its own heading. It must still resolve to the
        // parent section 8.2 - not read as cited_but_absent - because the
        // parent the citation lives under really is in the inventory.
        let inv = parse_inventory("## 8.2 Archive\n\nAC-1: it archives\n");
        let cases = vec![case("Archives on schedule", "Spec: S.md 8.2 (AC-3)")];
        let v = check_coverage(CoverageInput {
            inventories: vec![("S.md".into(), inv)],
            cases: &cases,
            sections_scope: "",
            out_of_scope: "",
        });
        assert_eq!(v["covered"]["8.2"], serde_json::json!(["Archives on schedule"]));
        assert_eq!(v["cited_but_absent"], serde_json::json!(Vec::<String>::new()));
    }

    #[test]
    fn ac_children_stay_reported_when_the_parent_is_covered() {
        // A covered parent must not silence its AC children - round-5 §3's
        // own example lists "8.2 (AC-2)" in `uncovered` even though 8.2
        // itself is covered. Finer-grained gaps are the spec'd reading, not
        // a bug: pin the behaviour so it isn't "fixed" away later.
        let inv = parse_inventory("## 8.2 Archive\n\nAC-1: it archives\nAC-2: it restores\n");
        let cases = vec![case("Archive parent behaviour", "Spec: S.md 8.2")];
        let v = check_coverage(CoverageInput {
            inventories: vec![("S.md".into(), inv)],
            cases: &cases,
            sections_scope: "",
            out_of_scope: "",
        });
        assert_eq!(v["covered"]["8.2"], serde_json::json!(["Archive parent behaviour"]));
        let uncovered: Vec<&str> = v["uncovered"].as_array().unwrap().iter().map(|s| s.as_str().unwrap()).collect();
        assert!(uncovered.contains(&"8.2 (AC-1)"), "{uncovered:?}");
        assert!(uncovered.contains(&"8.2 (AC-2)"), "{uncovered:?}");
    }

    #[test]
    fn a_citation_naming_an_unknown_file_is_cited_but_absent() {
        let inv = parse_inventory("## 7.1 List\n");
        let cases = vec![case("Wrong file", "Spec: Nope.md 7.1")];
        let v = check_coverage(CoverageInput {
            inventories: vec![("S.md".into(), inv)],
            cases: &cases,
            sections_scope: "",
            out_of_scope: "",
        });
        assert_eq!(v["cited_but_absent"], serde_json::json!(["7.1 — cited by 'Wrong file', no such document"]));
    }

    #[test]
    fn a_citation_matches_the_inventory_file_by_trailing_path_segment_case_insensitively() {
        let inv = parse_inventory("## 7.1 List\n");
        let cases = vec![case("List loads", "Spec: s.MD 7.1")];
        let v = check_coverage(CoverageInput {
            inventories: vec![(r"C:\specs\S.md".into(), inv)],
            cases: &cases,
            sections_scope: "",
            out_of_scope: "",
        });
        assert_eq!(v["covered"]["7.1"], serde_json::json!(["List loads"]));
    }

    #[test]
    fn the_same_section_id_in_two_files_is_tracked_per_file() {
        // Two documents each happen to have a "7.1". Citing only A.md's must
        // not silently mark B.md's unrelated 7.1 as covered too.
        let inv_a = parse_inventory("## 7.1 List\n");
        let inv_b = parse_inventory("## 7.1 List\n");
        let cases = vec![case("A's list loads", "Spec: A.md 7.1")];
        let v = check_coverage(CoverageInput {
            inventories: vec![("A.md".into(), inv_a), ("B.md".into(), inv_b)],
            cases: &cases,
            sections_scope: "",
            out_of_scope: "",
        });
        assert_eq!(v["covered"]["A.md 7.1"], serde_json::json!(["A's list loads"]));
        assert_eq!(v["uncovered"], serde_json::json!(["B.md 7.1"]));
    }

    #[test]
    fn plan_scope_moves_sections_to_excluded_not_uncovered() {
        let doc = || parse_inventory("## 7.1 List\n## 7.4 Export\n## 7.7 Copy\n");
        let cases = vec![case("List loads", "Spec: S.md 7.1")];

        // An enumerated list filters: 7.4 is excluded, not uncovered.
        let v = check_coverage(CoverageInput {
            inventories: vec![("S.md".into(), doc())],
            cases: &cases,
            sections_scope: "7.1, 7.7",
            out_of_scope: "",
        });
        assert_eq!(v["excluded_by_plan"], serde_json::json!(["7.4 — excluded by the plan's scope"]));
        assert_eq!(v["uncovered"], serde_json::json!(["7.7"]));

        // Free text excludes nothing - not a second source of truth.
        let v2 = check_coverage(CoverageInput {
            inventories: vec![("S.md".into(), doc())],
            cases: &cases,
            sections_scope: "everything",
            out_of_scope: "",
        });
        assert_eq!(v2["excluded_by_plan"], serde_json::json!(Vec::<String>::new()));
        assert_eq!(v2["uncovered"], serde_json::json!(["7.4", "7.7"]));

        // out_of_scope naming a section id excludes it too.
        let v3 = check_coverage(CoverageInput {
            inventories: vec![("S.md".into(), doc())],
            cases: &cases,
            sections_scope: "",
            out_of_scope: "7.4 is deferred to phase 2",
        });
        assert_eq!(v3["excluded_by_plan"], serde_json::json!(["7.4 — excluded by the plan's scope"]));
        assert_eq!(v3["uncovered"], serde_json::json!(["7.7"]));
    }

    #[test]
    fn out_of_scope_prose_numbers_do_not_get_harvested_as_sections() {
        // "phase 2" and the "99" tail of "JIRA-99" both look like section-id
        // tokens in isolation. Only "7.4" - at line start, and an id that
        // actually exists in the document - may be excluded; section "2"
        // must stay honestly uncovered rather than being silently swallowed.
        let inv = parse_inventory("## 2 Overview\n## 7.4 Export\n## 7.7 Copy\n");
        let cases: Vec<crate::model::TestCase> = vec![];
        let v = check_coverage(CoverageInput {
            inventories: vec![("S.md".into(), inv)],
            cases: &cases,
            sections_scope: "",
            out_of_scope: "7.4 is deferred to phase 2, see JIRA-99",
        });
        assert_eq!(v["excluded_by_plan"], serde_json::json!(["7.4 — excluded by the plan's scope"]), "{v}");
        let uncovered: Vec<&str> = v["uncovered"].as_array().unwrap().iter().map(|s| s.as_str().unwrap()).collect();
        assert!(uncovered.contains(&"2"), "section 2 must stay honestly uncovered: {uncovered:?}");
        assert!(uncovered.contains(&"7.7"), "{uncovered:?}");
    }

    #[test]
    fn the_report_has_exactly_the_listed_keys_and_never_a_warnings_key() {
        let inv = parse_inventory("## 7.1 List\n");
        let cases = vec![case("List loads", "Spec: S.md 7.1")];
        let v = check_coverage(CoverageInput {
            inventories: vec![("S.md".into(), inv)],
            cases: &cases,
            sections_scope: "",
            out_of_scope: "",
        });
        let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        let mut expected = vec![
            "sections_in_document",
            "sections_per_document",
            "covered",
            "uncovered",
            "unattributed",
            "cited_but_absent",
            "quote_not_in_document",
            "cited_without_quote",
            "excluded_by_plan",
        ];
        expected.sort_unstable();
        assert_eq!(keys, expected);
        assert!(!v.as_object().unwrap().contains_key("warnings"));
    }

    // ---- round 6 §3.1: filenames with spaces --------------------------

    #[test]
    fn a_spec_filename_containing_spaces_resolves() {
        // The blocker: whitespace-splitting truncated "Step9 - FDP.md" to
        // "FDP.md" and zeroed `covered` on a 227-case set.
        let c = parse_citations("Spec: Step9 - FDP.md 3.1").unwrap();
        assert_eq!(c.specs[0].file, "Step9 - FDP.md");
        assert_eq!(c.specs[0].section, "3.1");

        let c = parse_citations("Spec: UC & UACs.md 8.9").unwrap();
        assert_eq!(c.specs[0].file, "UC & UACs.md");

        // The space-free case must not regress...
        let c = parse_citations("Spec: Step9FDP.md 3.1").unwrap();
        assert_eq!(c.specs[0].file, "Step9FDP.md");
        assert_eq!(c.specs[0].section, "3.1");
        // ...and "3.1" is never mistaken for an extension: a section id's
        // post-dot character is a digit, extensions start with a letter.
        let c = parse_citations("Spec: v1.2 spec.md 3.1").unwrap();
        assert_eq!(c.specs[0].file, "v1.2 spec.md");
    }

    #[test]
    fn a_spaced_filename_still_carries_its_quote_and_exemption() {
        let c = parse_citations("Spec: Step9 - FDP.md 3.1 > \"the exact text\"").unwrap();
        assert_eq!(c.specs[0].file, "Step9 - FDP.md");
        assert_eq!(c.specs[0].section, "3.1");
        assert!(c.specs[0].quote.as_deref().is_some_and(|q| q.contains("the exact text")));

        let c =
            parse_citations("Spec: UC & UACs.md 3.1 - no quotable text (a table)").unwrap();
        assert_eq!(c.specs[0].file, "UC & UACs.md");
        assert_eq!(c.specs[0].exemption.as_deref(), Some("a table"));
    }

    // ---- round 6 §3.2: heading + free-text locator --------------------

    #[test]
    fn a_heading_plus_locator_resolves_to_the_heading() {
        // A near-structureless document: everything citable lives inside
        // one heading, and the locator after it is detail, not a section
        // name - 331 accurate citations read as absent before this.
        let inv = parse_inventory("## Implementation\nbody\n## Notes\nmore\n");
        let cases = vec![case(
            "Greets the Supervisor by Their Own Name",
            "Spec: A.md Implementation section 5 DATA view, SUPERVISOR_NAME",
        )];
        let v = check_coverage(CoverageInput {
            inventories: vec![("A.md".into(), inv)],
            cases: &cases,
            sections_scope: "",
            out_of_scope: "",
        });
        assert!(v["covered"].get("Implementation").is_some(), "{v}");
        assert_eq!(v["cited_but_absent"].as_array().unwrap().len(), 0, "{v}");
    }

    #[test]
    fn the_heading_prefix_match_requires_a_word_boundary() {
        // "7.10" must never resolve to section "7.1" just because the
        // characters line up.
        let inv = parse_inventory("## 7.1 Alpha\nbody\n");
        let cases = vec![case("X", "Spec: A.md 7.10")];
        let v = check_coverage(CoverageInput {
            inventories: vec![("A.md".into(), inv)],
            cases: &cases,
            sections_scope: "",
            out_of_scope: "",
        });
        assert_eq!(v["cited_but_absent"].as_array().unwrap().len(), 1, "{v}");
        assert!(v["covered"].as_object().unwrap().is_empty(), "{v}");
    }

    // ---- round 6 §3.3: out_of_scope reaches excluded_by_plan ----------

    #[test]
    fn out_of_scope_ranges_land_in_excluded_by_plan() {
        let inv = parse_inventory("## 3 Keep\na\n## 4 A\nb\n## 4.15 B\nc\n## 5 C\nd\n## 7 D\ne\n## 8 Keep too\nf\n");
        let cases: Vec<crate::model::TestCase> = vec![];
        let v = check_coverage(CoverageInput {
            inventories: vec![("F.md".into(), inv)],
            cases: &cases,
            sections_scope: "",
            out_of_scope: "sections 4 to 7 are reference only for this batch",
        });
        let excluded = v["excluded_by_plan"].as_array().unwrap();
        for id in ["4 ", "4.15 ", "5 ", "7 "] {
            assert!(
                excluded.iter().any(|e| e.as_str().unwrap().starts_with(id)),
                "{id} missing from {v}"
            );
        }
        let uncovered: Vec<&str> =
            v["uncovered"].as_array().unwrap().iter().map(|u| u.as_str().unwrap()).collect();
        assert_eq!(uncovered, vec!["3", "8"], "{v}");
    }

    #[test]
    fn a_prefixed_range_excludes_the_prefixed_family() {
        let inv = parse_inventory("## UAC 8.9\na\n## UAC 8.10\nb\n## UAC 8.13\nc\n");
        let cases: Vec<crate::model::TestCase> = vec![];
        let v = check_coverage(CoverageInput {
            inventories: vec![("U.md".into(), inv)],
            cases: &cases,
            sections_scope: "",
            out_of_scope: "UAC 8.9 to 8.12 belong to the Review step",
        });
        let excluded = v["excluded_by_plan"].as_array().unwrap();
        assert_eq!(excluded.len(), 2, "{v}"); // 8.9 and 8.10; 8.13 stays
        let uncovered = v["uncovered"].as_array().unwrap();
        assert_eq!(uncovered.len(), 1, "{v}");
        assert_eq!(uncovered[0], "UAC 8.13", "{v}");
    }

    #[test]
    fn a_bare_number_in_prose_still_excludes_nothing() {
        let inv = parse_inventory("## 2 Real section\nbody\n");
        let cases: Vec<crate::model::TestCase> = vec![];
        let v = check_coverage(CoverageInput {
            inventories: vec![("F.md".into(), inv)],
            cases: &cases,
            sections_scope: "",
            out_of_scope: "deferred to phase 2 of the project",
        });
        assert!(v["excluded_by_plan"].as_array().unwrap().is_empty(), "{v}");
    }

    #[test]
    fn a_named_heading_in_out_of_scope_is_excluded() {
        let inv = parse_inventory("## Implementation\na\n## Open Items\nb\n");
        let cases: Vec<crate::model::TestCase> = vec![];
        let v = check_coverage(CoverageInput {
            inventories: vec![("F.md".into(), inv)],
            cases: &cases,
            sections_scope: "",
            out_of_scope: "Open Items is tracked separately",
        });
        let excluded = v["excluded_by_plan"].as_array().unwrap();
        assert_eq!(excluded.len(), 1, "{v}");
        assert!(excluded[0].as_str().unwrap().starts_with("Open Items"), "{v}");
    }

    #[test]
    fn the_per_document_breakdown_names_each_file() {
        let a = parse_inventory("## 1 A\nx\n## 2 B\ny\n");
        let b = parse_inventory("## 1 C\nz\n");
        let cases: Vec<crate::model::TestCase> = vec![];
        let v = check_coverage(CoverageInput {
            inventories: vec![("A.md".into(), a), ("B.md".into(), b)],
            cases: &cases,
            sections_scope: "",
            out_of_scope: "",
        });
        assert_eq!(v["sections_in_document"], 3, "{v}");
        assert_eq!(v["sections_per_document"]["A.md"], 2, "{v}");
        assert_eq!(v["sections_per_document"]["B.md"], 1, "{v}");
    }

    /// Round 8 §1: the guide tells authors to elide mid-quote with `...`,
    /// so the checker must honour an elided quote - every fragment
    /// verbatim, in document order - instead of failing it wholesale.
    #[test]
    fn an_elided_quote_matches_when_its_fragments_appear_in_order() {
        let doc = "## 1 Links\n\nAction links - every template links back into the system with a deep URL, not generic landing pages.\n";
        let run = |notes: &str| {
            let cases = vec![case("Elided", notes)];
            let v = check_coverage(CoverageInput {
                inventories: vec![("A.md".into(), parse_inventory(doc))],
                cases: &cases,
                sections_scope: "",
                out_of_scope: "",
            });
            v["quote_not_in_document"].as_array().unwrap().len()
        };
        // Three-dot and U+2026 elisions both pass...
        assert_eq!(run("Spec: A.md 1\n> \"Action links ... not generic landing pages.\""), 0);
        assert_eq!(run("Spec: A.md 1\n> \"Action links \u{2026} not generic landing pages.\""), 0);
        // ...an invented fragment still fails...
        assert_eq!(run("Spec: A.md 1\n> \"Action links ... the moon on a stick.\""), 1);
        // ...and so do real fragments in the WRONG order.
        assert_eq!(run("Spec: A.md 1\n> \"not generic landing pages ... Action links\""), 1);
    }

    /// Round 8 dogfooding: a scope list written "3.1-3.6, 3.10" must stay
    /// enumerated, with the range admitting its numeric members.
    #[test]
    fn an_enumerated_scope_accepts_ranges() {
        let inv = parse_inventory("## 3.1 A\nx\n## 3.4 B\nx\n## 3.6 C\nx\n## 3.7 D\nx\n");
        let cases: Vec<crate::model::TestCase> = vec![];
        let v = check_coverage(CoverageInput {
            inventories: vec![("F.md".into(), inv)],
            cases: &cases,
            sections_scope: "3.1-3.6, 3.10",
            out_of_scope: "",
        });
        let uncovered: Vec<&str> =
            v["uncovered"].as_array().unwrap().iter().map(|u| u.as_str().unwrap()).collect();
        assert_eq!(uncovered, vec!["3.1", "3.4", "3.6"], "{v}");
        assert!(
            v["excluded_by_plan"].as_array().unwrap().iter().any(|e| e.as_str().unwrap().starts_with("3.7")),
            "{v}"
        );
    }

    /// Round 8 dogfooding: "3.4a Entry Limit Per Role" is a real heading
    /// style; its id must parse as its own section and be citable.
    #[test]
    fn a_letter_suffixed_heading_is_a_real_citable_id() {
        let doc = "### 3.4a Entry Limit Per Role\nbody\n### 3.5 Next\nx\n";
        let inv = parse_inventory(doc);
        let ids: Vec<&str> = inv.sections.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["3.4a", "3.5"]);

        let cases = vec![case("Limit", "Spec: F.md 3.4a\n> \"body\"")];
        let v = check_coverage(CoverageInput {
            inventories: vec![("F.md".into(), parse_inventory(doc))],
            cases: &cases,
            sections_scope: "",
            out_of_scope: "",
        });
        assert!(v["covered"].get("3.4a").is_some(), "{v}");
        assert!(v["cited_but_absent"].as_array().unwrap().is_empty(), "{v}");
    }

    /// Round 8 dogfooding: in enumerated mode a NAMED sub-heading inherits
    /// the verdict of the id-shaped heading above it - "On submit" under an
    /// in-scope 3.6 is uncovered work, not an exclusion.
    #[test]
    fn named_sub_headings_inherit_their_parents_scope_in_enumerated_mode() {
        let inv = parse_inventory(
            "## 3.6 Submit\n#### Pre-submit validation\nx\n#### On submit\nx\n## 3.7 Actions\n#### Buttons\nx\n",
        );
        let cases: Vec<crate::model::TestCase> = vec![];
        let v = check_coverage(CoverageInput {
            inventories: vec![("F.md".into(), inv)],
            cases: &cases,
            sections_scope: "3.6",
            out_of_scope: "",
        });
        let uncovered: Vec<&str> =
            v["uncovered"].as_array().unwrap().iter().map(|u| u.as_str().unwrap()).collect();
        assert_eq!(uncovered, vec!["3.6", "Pre-submit validation", "On submit"], "{v}");
        let excluded = v["excluded_by_plan"].as_array().unwrap();
        assert!(excluded.iter().any(|e| e.as_str().unwrap().starts_with("3.7")), "{v}");
        assert!(excluded.iter().any(|e| e.as_str().unwrap().starts_with("Buttons")), "{v}");
    }
}
