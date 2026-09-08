# Round 8 Feedback Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the five items in round-8 feedback on the `tcm-testcases` tools: citation ordering (guide, advisories, a `normalise_citations` op), `optimize_cases` no longer trimming assertions, the missing transform ops, the copy-in that left the oldest bytes under the canonical name, merge naming its leftovers, and the fanout skill asking for a model per slice.

**Architecture:** Every change is in `v2/`. Rust logic lives in `src-tauri/src/{speccov,transform,optimize,workspace,ai_bridge,ai_tools}.rs`; the MCP tool descriptions in `src-tauri/src/mcp.rs`; one Tauri command (`copy_into_cases`) changes shape and needs `bindings.ts` regenerated; one frontend toast changes in `src/screens/ImportFile.tsx`. Every Rust test is an integration test under `src-tauri/tests/` except `speccov.rs`, which already carries an inline `mod tests` (it links no tauri) — follow the file you are in.

**Tech Stack:** Rust (regex, serde, specta), Tauri 2, React 19 + TypeScript, vitest.

**Spec:** `docs/superpowers/specs/2026-09-08-round8-feedback-design.md` — read it first; it also records two places the feedback misread the code.

## Global Constraints

- Run from `v2/` (frontend) or `v2/src-tauri/` (Rust). **One build or test command at a time** — the machine is shared.
- `src/bindings.ts` is generated: `cargo test --test bindings` regenerates it. Never hand-edit.
- No new `DELETE` HTTP call anywhere; the `ado/` scan test enforces it. Local-file deletion is also out: nothing in this plan removes a file.
- Commits: `git commit -F - <<'EOF' … EOF` from Bash, never PowerShell flags. Confirm with `git log -1`.
- Copy in tool descriptions and messages: ASCII hyphens, `Settings → Logs`-style arrows only where the codebase already uses them.
- Keep `expected_trimmed`, `expected_rewritten`, `applied`, `warnings`, `ignored` field names — callers read them.

---

### Task 1: Citation ordering — the guide says it, the advisories point at it

**Files:**
- Modify: `v2/src-tauri/src/speccov.rs` (next to `parse_citations`, ~line 222; the `cited_without_quote` push at ~765; inline tests ~1030)
- Modify: `v2/src-tauri/src/ai_bridge.rs:786-802` (advisory) and `:1385-1394` (guide paragraph)
- Test: `v2/src-tauri/tests/ai_bridge.rs` (after `a_spec_cited_case_without_quote_or_exemption_is_an_advisory_not_a_warning`, ~line 232)

**Interfaces:**
- Produces: `pub fn has_blockquote(notes: &str) -> bool` and `pub fn bare_citation_hint(notes: &str) -> &'static str` in `speccov`. Task 2 uses `has_blockquote`.

- [ ] **Step 1: Write the failing test (validate_cases advisory)**

Append to `v2/src-tauri/tests/ai_bridge.rs`:

```rust
/// Round 8 §1-§4: three of five writers, with a verbatim quote already in
/// the note, went looking for a MISSING quote - because that is what the
/// advisory said. When a blockquote exists but sits above the `Spec:`
/// line, the message has to name the position, not the absence.
#[tokio::test]
async fn a_misordered_quote_is_told_where_the_checker_reads_it() {
    let draft = serde_json::json!({
        "test_cases": [
            {
                "title": "Quote above the pointer",
                "reviewer_notes": "Checks the layout.\n\n> | Employee Details | Name, ID |\n\nSpec: S.md Report Design",
                "automation_status": "Not Automated",
                "steps": [{ "action": "Open the report.", "expected": "Four fields are shown." }]
            },
            {
                "title": "No quote anywhere",
                "reviewer_notes": "Checks the export button.\nSpec: S.md 7.7",
                "automation_status": "Not Automated",
                "steps": [{ "action": "Open the page.", "expected": "The button is shown." }]
            }
        ]
    })
    .to_string();

    let (status, body) = route(&ctx(), None, "POST", "/validate", &draft, "1.23.2").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let advisories: Vec<String> = v["advisories"]
        .as_array()
        .expect("both cases are advisories")
        .iter()
        .map(|a| a.as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(advisories.len(), 2, "{body}");
    let above = advisories.iter().find(|a| a.contains("Quote above the pointer")).unwrap();
    assert!(above.contains("not where the checker reads it"), "{above}");
    assert!(above.contains("table or code block is not a quote"), "{above}");
    let bare = advisories.iter().find(|a| a.contains("No quote anywhere")).unwrap();
    assert!(!bare.contains("not where the checker reads it"), "a bare citation keeps the plain advice: {bare}");
    assert!(bare.contains("no quotable text"), "{bare}");
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd v2/src-tauri && cargo test --test ai_bridge a_misordered_quote_is_told_where_the_checker_reads_it`
Expected: FAIL — `above` lacks "not where the checker reads it".

- [ ] **Step 3: Add the helpers to `speccov.rs`**

Immediately above `pub fn parse_citations`:

```rust
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
```

- [ ] **Step 4: Use the hint in both advisories**

`ai_bridge.rs` ~797 — replace the `advisories.push(format!(...))` body with:

```rust
                        advisories.push(format!(
                            "Test case {} ('{}') cites a Spec: section with no verbatim quote and \
                             no exemption - {}.",
                            i + 1,
                            tc.title,
                            crate::speccov::bare_citation_hint(&tc.reviewer_notes)
                        ));
```

`speccov.rs` ~765 — the `cited_without_quote.push(...)`:

```rust
                cited_without_quote.push(format!(
                    "{} — Spec: {} {} has no quote and no exemption - {}",
                    case.title,
                    spec.file,
                    spec.section,
                    bare_citation_hint(&case.reviewer_notes)
                ));
```

(`case` in that loop is the `TestCase`; if the binding there is named differently, use its `reviewer_notes`.)

- [ ] **Step 5: State the order in the guide**

`ai_bridge.rs` ~1388, in the `get_writing_guide` string. Replace exactly:

```
        Add `Out of scope: SSO` only when THIS case deliberately leaves\n\
        something out. Alongside the `Spec:` pointer, quote the source\n\
        sentence verbatim, or it must not be presented as a quote: write\n\
        `> \"<the source sentence>\"` when you can quote it, or state the\n\
        exemption in the fixed form\n\
```

with:

```
        Add `Out of scope: SSO` only when THIS case deliberately leaves\n\
        something out. The `Spec:` pointer comes FIRST, with the quote\n\
        directly beneath it as `> \"<the source sentence>\"` - the checker\n\
        reads a quote only in that position, so a quote placed above the\n\
        pointer is reported as missing. Quote verbatim or not at all: when\n\
        you cannot quote (a table, a diagram, code), state the\n\
        exemption in the fixed form\n\
```

- [ ] **Step 6: Inline speccov test for the coverage finding**

In `speccov.rs`'s `mod tests`, next to the §9 test that uses `case("Bare", "Spec: S.md 7.7")` (~line 1030), add:

```rust
    /// Round 8 §4: with a blockquote ABOVE the pointer the finding names
    /// the position; with none it keeps the plain advice.
    #[test]
    fn a_bare_citation_hint_depends_on_whether_a_quote_exists_anywhere() {
        assert!(has_blockquote("x\n> | a | b |\nSpec: S.md 1"));
        assert!(!has_blockquote("Spec: S.md 1\nplain prose"));
        assert!(bare_citation_hint("> \"q\"\nSpec: S.md 1").contains("not where the checker reads it"));
        assert!(bare_citation_hint("Spec: S.md 1").starts_with("quote the source sentence"));
    }
```

- [ ] **Step 7: Run the three suites touched**

Run: `cargo test --test ai_bridge` then `cargo test --lib speccov` (the inline tests; if `--lib` names nothing, `cargo test speccov::tests`).
Expected: all pass, including the new ones.

- [ ] **Step 8: Commit**

```bash
git add v2/src-tauri/src/speccov.rs v2/src-tauri/src/ai_bridge.rs v2/src-tauri/tests/ai_bridge.rs
git commit -F - <<'EOF'
feat(tcm): a misordered quote is reported as misplaced, not missing

Round 8 §1-§4. Three of five slice writers had a verbatim quote in the note
and went looking for a missing one, because "no verbatim quote" is what the
advisory said. Both findings now say where the checker reads a quote, and
the guide states the order once.
EOF
```

---

### Task 2: `normalise_citations` transform op

**Files:**
- Modify: `v2/src-tauri/src/transform.rs` — `Op` enum (~32), `parse_ops` (~297), the supported-ops error text (~491), `apply` (~620-760), `describe` (~795), `known_keys` (~827)
- Modify: `v2/src-tauri/src/mcp.rs:206` (transform tool description)
- Test: `v2/src-tauri/tests/transform.rs`

**Interfaces:**
- Consumes: `crate::speccov::has_blockquote`.
- Produces: `pub fn normalise_citation_notes(notes: &str) -> (String, CitationOutcome)` and `pub enum CitationOutcome { Normalised, Exempted(&'static str), Unchanged, ByHand(String) }` in `transform`.

- [ ] **Step 1: Write the failing tests**

Append to `v2/src-tauri/tests/transform.rs` (the file already has `noted(title, notes)`, `parse_ops`, `apply`):

```rust
// ---- round 8 §7.2: normalise_citations ----------------------------------

use v2_lib::transform::{normalise_citation_notes, CitationOutcome};

#[test]
fn a_quote_above_the_pointer_moves_beneath_it_in_quotation_marks() {
    let notes = "Checks the report identifies the right person.\n\n> Report Navigator resolves the context accordingly\n\nSpec: R.md General Requirements\n\nThe counterpart negative is a separate case.";
    let (out, outcome) = normalise_citation_notes(notes);
    assert!(matches!(outcome, CitationOutcome::Normalised));
    assert_eq!(
        out,
        "Checks the report identifies the right person.\n\nSpec: R.md General Requirements\n\n> \"Report Navigator resolves the context accordingly\"\n\nThe counterpart negative is a separate case."
    );
    // And the accepted form is what parse_citations reads as a quote.
    let c = v2_lib::speccov::parse_citations(&out).unwrap();
    assert_eq!(c.specs[0].quote.as_deref(), Some("Report Navigator resolves the context accordingly"));
}

#[test]
fn a_table_row_becomes_an_exemption_and_the_block_is_kept() {
    let notes = "Checks the four fields.\n\n> | Employee Details | Name, ID |\n\nSpec: R.md Report Design";
    let (out, outcome) = normalise_citation_notes(notes);
    assert!(matches!(outcome, CitationOutcome::Exempted("table/diagram")));
    assert_eq!(
        out,
        "Checks the four fields.\n\nSpec: R.md Report Design - no quotable text (table/diagram)\n\n> | Employee Details | Name, ID |"
    );
    let c = v2_lib::speccov::parse_citations(&out).unwrap();
    assert!(c.specs[0].exemption.is_some() && c.specs[0].quote.is_none());
}

#[test]
fn sql_is_code_not_prose() {
    let notes = "Spec: R.md Database Scripts\n\n> SELECT emp_id FROM perf_cycle WHERE stage = 'done'";
    let (_, outcome) = normalise_citation_notes(notes);
    assert!(matches!(outcome, CitationOutcome::Exempted("code-not-prose")));
}

#[test]
fn an_already_correct_note_is_unchanged_and_the_op_is_idempotent() {
    let good = "Checks it.\n\nSpec: R.md 7.1\n\n> \"The list refreshes.\"";
    let (out, outcome) = normalise_citation_notes(good);
    assert!(matches!(outcome, CitationOutcome::Unchanged));
    assert_eq!(out, good);
    let messy = "> The list refreshes.\nSpec: R.md 7.1";
    let (once, _) = normalise_citation_notes(messy);
    let (twice, second) = normalise_citation_notes(&once);
    assert_eq!(once, twice);
    assert!(matches!(second, CitationOutcome::Unchanged));
}

#[test]
fn two_pointers_or_two_blocks_are_left_for_a_person() {
    let (out, outcome) = normalise_citation_notes("> a\nSpec: A.md 1\nSpec: B.md 2");
    assert!(matches!(outcome, CitationOutcome::ByHand(ref why) if why.contains("2 Spec lines")));
    assert_eq!(out, "> a\nSpec: A.md 1\nSpec: B.md 2", "untouched");
    let (_, outcome) = normalise_citation_notes("> a\n\n> b\nSpec: A.md 1");
    assert!(matches!(outcome, CitationOutcome::ByHand(ref why) if why.contains("2 blockquotes")));
    let (_, outcome) = normalise_citation_notes("Spec: A.md 1\nprose only");
    assert!(matches!(outcome, CitationOutcome::Unchanged), "nothing to move");
}

#[test]
fn the_op_reports_per_case_and_respects_where() {
    let cases = vec![
        noted("Moves", "> q\nSpec: A.md 1"),
        noted("Table", "> | a |\nSpec: A.md 2"),
        noted("Fine", "Spec: A.md 3\n> \"q\""),
        noted("Skipped", "> q\nSpec: A.md 4"),
    ];
    let ops = parse_ops(&serde_json::json!([
        { "op": "normalise_citations", "where": { "title_contains": "" } }
    ]))
    .unwrap();
    let (out, report) = apply(cases, &ops);
    assert_eq!(out[0].reviewer_notes, "Spec: A.md 1\n\n> \"q\"");
    assert!(out[1].reviewer_notes.starts_with("Spec: A.md 2 - no quotable text (table/diagram)"));
    assert_eq!(out[2].reviewer_notes, "Spec: A.md 3\n> \"q\"", "already correct stays byte-identical");
    let line = &report.applied[0];
    assert!(line.contains("2 normalised") && line.contains("1 exempted") && line.contains("1 unchanged"), "{line}");
    assert!(report.warnings.iter().any(|w| w.contains("Table") && w.contains("table/diagram")), "{:?}", report.warnings);
}
```

Note the last test: `"Skipped"` is deliberately identical in shape to `"Moves"` so the counts read `2 normalised`; the `where` there is the empty filter (matches all). If `title_contains: ""` is rejected by `parse_filter`, drop the `where` key entirely.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --test transform normalise`
Expected: compile error — `normalise_citation_notes` does not exist.

- [ ] **Step 3: Implement the pure function**

Add to `transform.rs` (above `pub fn apply`):

```rust
/// What `normalise_citations` did to one note.
#[derive(Debug, Clone, PartialEq)]
pub enum CitationOutcome {
    Normalised,
    /// The block was a table or code: the pointer got the exemption form
    /// and the block was KEPT beneath it for the writer to fold into prose.
    Exempted(&'static str),
    Unchanged,
    /// More than one pointer or more than one block - refused, with why.
    ByHand(String),
}

/// Put a note's citation into the one shape the checker reads: prose,
/// `Spec:` line, `> "quote"`, remaining prose - in that order.
///
/// Round 8 §5: 66 notes needed exactly this, and the only route was
/// retyping every one. This is deliberately narrow - one pointer, one
/// blockquote run, or hands off - because a note with two of either has no
/// single right answer, and a wrong guess here corrupts a citation silently.
pub fn normalise_citation_notes(notes: &str) -> (String, CitationOutcome) {
    let lines: Vec<&str> = notes.lines().collect();
    let is_spec = |l: &str| l.trim_start().to_lowercase().starts_with("spec:");
    let is_block = |l: &str| l.trim_start().starts_with('>');

    let spec_lines: Vec<usize> = (0..lines.len()).filter(|&i| is_spec(lines[i])).collect();
    // Runs of consecutive blockquote lines, as (start, end-exclusive).
    let mut runs: Vec<(usize, usize)> = vec![];
    let mut i = 0;
    while i < lines.len() {
        if is_block(lines[i]) {
            let start = i;
            while i < lines.len() && is_block(lines[i]) {
                i += 1;
            }
            runs.push((start, i));
        } else {
            i += 1;
        }
    }

    if spec_lines.len() > 1 || runs.len() > 1 {
        return (
            notes.to_string(),
            CitationOutcome::ByHand(format!(
                "{} Spec lines / {} blockquotes - normalise by hand",
                spec_lines.len(),
                runs.len()
            )),
        );
    }
    let (Some(&spec_at), Some(&(run_start, run_end))) = (spec_lines.first(), runs.first()) else {
        return (notes.to_string(), CitationOutcome::Unchanged);
    };

    let spec_line = lines[spec_at].trim().to_string();
    let block_lines: Vec<&str> = lines[run_start..run_end].to_vec();
    let stripped: Vec<String> = block_lines
        .iter()
        .map(|l| l.trim_start().trim_start_matches('>').trim().to_string())
        .collect();
    let first = stripped.first().map(String::as_str).unwrap_or("");
    let joined = stripped.join(" ");

    let unquotable: Option<&'static str> = if first.starts_with('|') {
        Some("table/diagram")
    } else if looks_like_code(&joined) {
        Some("code-not-prose")
    } else {
        None
    };

    // Already in the accepted shape? The run must be the first non-blank
    // line after the pointer, and either the pointer carries an exemption
    // (block kept as is) or the run is one `> "..."` line.
    let next_nonblank = (spec_at + 1..lines.len()).find(|&j| !lines[j].trim().is_empty());
    let run_follows = next_nonblank == Some(run_start);
    let spec_exempt = spec_line.to_lowercase().contains("no quotable text");
    let one_quoted_line = block_lines.len() == 1 && first.starts_with('"') && first.ends_with('"') && first.len() >= 2;
    if run_follows && ((unquotable.is_some() && spec_exempt) || (unquotable.is_none() && one_quoted_line && !spec_exempt)) {
        return (notes.to_string(), CitationOutcome::Unchanged);
    }

    let (new_spec, new_block, outcome) = match unquotable {
        Some(why) => {
            let spec = if spec_exempt { spec_line.clone() } else { format!("{spec_line} - no quotable text ({why})") };
            (spec, block_lines.iter().map(|l| l.trim_end().to_string()).collect::<Vec<_>>(), CitationOutcome::Exempted(why))
        }
        None => {
            let text = joined.trim_matches(|c| c == '"' || c == '\u{201c}' || c == '\u{201d}').trim().to_string();
            (spec_line.clone(), vec![format!("> \"{text}\"")], CitationOutcome::Normalised)
        }
    };

    // Everything that is neither the pointer nor the run, split at the
    // earlier of the two: what came before stays before, the rest follows.
    let cut = spec_at.min(run_start);
    let mut before: Vec<&str> = vec![];
    let mut after: Vec<&str> = vec![];
    for (j, l) in lines.iter().enumerate() {
        if j == spec_at || (run_start..run_end).contains(&j) {
            continue;
        }
        if j < cut { before.push(l) } else { after.push(l) }
    }
    let trim_blank = |v: &[&str]| -> Vec<String> {
        let s = v.iter().position(|l| !l.trim().is_empty()).unwrap_or(v.len());
        let e = v.iter().rposition(|l| !l.trim().is_empty()).map(|p| p + 1).unwrap_or(s);
        v[s..e].iter().map(|l| l.trim_end().to_string()).collect()
    };
    let before = trim_blank(&before);
    let after = trim_blank(&after);

    let mut out: Vec<String> = vec![];
    if !before.is_empty() {
        out.extend(before);
        out.push(String::new());
    }
    out.push(new_spec);
    out.push(String::new());
    out.extend(new_block);
    if !after.is_empty() {
        out.push(String::new());
        out.extend(after);
    }
    (out.join("\n"), outcome)
}

/// A blockquote that is code rather than a sentence: SQL, a comment
/// marker, or a fence. A quote of code is not a quote of the requirement.
fn looks_like_code(s: &str) -> bool {
    let t = s.trim_start();
    if t.starts_with("--") || t.contains("```") {
        return true;
    }
    let first = t.split_whitespace().next().unwrap_or("").to_uppercase();
    matches!(
        first.as_str(),
        "SELECT" | "INSERT" | "UPDATE" | "DELETE" | "CREATE" | "ALTER" | "EXEC" | "DECLARE" | "WITH"
    )
}
```

Check the first test's expected string against this output shape (before / blank / spec / blank / quote / blank / after) — that is the accepted example from the feedback.

- [ ] **Step 4: Wire the op**

`Op` enum: add `/// Round 8 §7.2 - see normalise_citation_notes.\n    NormaliseCitations,`.
`parse_ops`: `"normalise_citations" => Op::NormaliseCitations,`.
Supported-ops error text: append `normalise_citations`.
`known_keys`: add `"normalise_citations"` to the `"remove_cases" | "dedupe"` arm.
`describe`: `Op::NormaliseCitations => "Normalised citations".to_string(),`.

In `apply`, this op edits notes, so it belongs in the `other =>` arm. Add it to the `before_snapshot` matches list, and to the `match other` body:

```rust
                        Op::NormaliseCitations => {
                            let (text, outcome) = normalise_citation_notes(&c.reviewer_notes);
                            match &outcome {
                                CitationOutcome::Normalised => normalised += 1,
                                CitationOutcome::Exempted(why) => {
                                    exempted += 1;
                                    report.warnings.push(format!(
                                        "'{}': the blockquote is a {why} - the pointer now carries the \
                                         exemption and the block was kept beneath it; fold it into \
                                         prose if you would rather.",
                                        c.title
                                    ));
                                }
                                CitationOutcome::Unchanged => unchanged += 1,
                                CitationOutcome::ByHand(why) => {
                                    by_hand += 1;
                                    report.warnings.push(format!("'{}': {why}.", c.title));
                                }
                            }
                            c.reviewer_notes = text;
                        }
```

Declare `let (mut normalised, mut exempted, mut unchanged, mut by_hand) = (0usize, 0usize, 0usize, 0usize);` next to `let mut modified`. After the per-case loop, before the `find_driven` block, add:

```rust
                if matches!(other, Op::NormaliseCitations) {
                    report.applied.push(format!(
                        "Normalised citations: {normalised} normalised, {exempted} exempted, \
                         {unchanged} unchanged, {by_hand} left for hand."
                    ));
                } else
```

so the existing `if find_driven { ... } else { ... }` becomes the tail of that chain. Do **not** add `NormaliseCitations` to `find_driven`.

- [ ] **Step 5: Tool description**

`mcp.rs:206`, inside the ops description, after the `replace_in_notes` clause add: `normalise_citations takes only where - it moves a Spec: line above its blockquote and quotes it, or writes the exemption form for a table/code block, and leaves anything with two pointers or two blocks for a person;`.

- [ ] **Step 6: Run the transform suite**

Run: `cargo test --test transform`
Expected: all pass. If `a_quote_above_the_pointer...`'s exact string differs only in blank-line placement, fix the implementation, not the test — the test is the accepted shape.

- [ ] **Step 7: Commit**

```bash
git add v2/src-tauri/src/transform.rs v2/src-tauri/src/mcp.rs v2/src-tauri/tests/transform.rs
git commit -F - <<'EOF'
feat(tcm): normalise_citations puts a note's quote where the checker reads it

Round 8 §5-§7. One pointer and one blockquote: Spec line first, the quote
beneath it in quotation marks, prose kept in order. A table row or code
gets the exemption form with the block kept. Two of either is refused by
name rather than guessed at.
EOF
```

---

### Task 3: `optimize_cases` keeps a trailing sentence that asserts something

**Files:**
- Modify: `v2/src-tauri/src/optimize.rs` — `OptimizeReport` (~112), the sentence cut in `clean_expected` (~352), the caller at ~778
- Modify: `v2/src-tauri/src/mcp.rs:187` (tool description opening)
- Test: `v2/src-tauri/tests/optimize.rs`

**Interfaces:**
- Produces: `pub fn clean_expected_keeping(raw: &str) -> (String, bool)`; `clean_expected` stays and wraps it. `OptimizeReport.assertions_kept: usize`.

- [ ] **Step 1: Write the failing tests**

Append to `tests/optimize.rs`:

```rust
// ---- round 8 §8/§12: the second sentence is often the assertion ----------

/// The eight before/after pairs the feedback measured, every one of which
/// the old one-sentence rule gutted. The retained sentence must survive.
#[test]
fn a_trailing_sentence_that_asserts_something_is_kept() {
    for (input, must_keep) in [
        ("Exactly one row is returned, for {empCompleted}. No row is returned for {empIncomplete}.", "No row is returned"),
        ("The procedure completes normally. No error, warning or message naming {empIncomplete} is raised.", "No error"),
        ("A single 'Performance Management System' group is listed. It is not duplicated.", "not duplicated"),
        ("Two rows are returned. Both carry PARM_ID '000001' and the caption 'Select Evaluation Cycle', and they differ only by REPORT_ID.", "PARM_ID"),
        ("Each is separated by a space, a hyphen, a greater-than sign and a space. The top of the hierarchy is first and the objective nearest the goal is last.", "hierarchy is first"),
        ("No rows are returned. A missing appraisee list does not fall back to every participant of the cycle.", "does not fall back"),
        ("Every row reads either Aligned or Not Aligned. No row reads Partially Aligned.", "No row reads"),
        ("They differ. Business Unit Level is business_unit_level to the procedure and @def_level to its child.", "business_unit_level"),
    ] {
        let out = clean_expected(input);
        assert!(out.contains(must_keep), "lost the assertion:\n  in:  {input}\n  out: {out}");
    }
}

/// And a gloss is still a gloss - the rule must not degrade into "keep
/// everything".
#[test]
fn a_trailing_gloss_still_goes() {
    assert_eq!(clean_expected("A confirmation appears. This proves the flow works."), "A confirmation appears.");
    assert_eq!(
        clean_expected("The order status changes to Shipped. A confirmation email is sent to the customer."),
        "The order status changes to Shipped."
    );
    assert_eq!(
        clean_expected("The appraiser is listed. That is because that appraiser sits in {unit_b}."),
        "The appraiser is listed."
    );
}

#[test]
fn the_report_counts_kept_assertions() {
    let draft = vec![case("Kept", "M", "", vec![step("Run", "One row is returned. No row is returned for {other}.")])];
    let (_out, report) = optimize(draft, None);
    assert_eq!(report.assertions_kept, 1);
    assert_eq!(report.expected_trimmed, 0, "nothing was trimmed");
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --test optimize trailing`
Expected: FAIL on the first assertion of the first test (the `No row` sentence is dropped).

- [ ] **Step 3: Implement**

In `optimize.rs`, replace the block

```rust
    if let Some(i) = sentence_break(&s) {
        s = s[..i].to_string();
    }
```

with

```rust
    let mut kept_assertion = false;
    if let Some(i) = sentence_break(&s) {
        let mut kept = s[..i].to_string();
        let mut rest = s[i + 2..].trim().to_string();
        while !rest.is_empty() {
            let (sentence, after) = match sentence_break(&rest) {
                Some(j) => (rest[..j].to_string(), rest[j + 2..].trim().to_string()),
                None => (rest.trim_end_matches('.').to_string(), String::new()),
            };
            if !carries_assertion(&kept, &sentence) {
                break;
            }
            kept = format!("{kept}. {sentence}");
            kept_assertion = true;
            rest = after;
        }
        s = kept;
    }
```

and change the function's signature/tail so it returns `(s, kept_assertion)` as `pub fn clean_expected_keeping(raw: &str) -> (String, bool)`, with

```rust
/// Reduce an expected result to the observable outcome. See
/// `clean_expected_keeping` for the one thing it will not cut.
pub fn clean_expected(raw: &str) -> String {
    clean_expected_keeping(raw).0
}
```

Add the predicate:

```rust
/// Does a later sentence still TEST something, or only explain?
///
/// Round 8 §8/§12: on two independent drafts, nine trims in ten removed the
/// sentence carrying the assertion - house style puts the observation first
/// and the discriminating detail second, so "keep the first sentence" kept
/// the half that does no testing. A sentence is an assertion when it
/// negates (the "and nothing else" half of a check) or names something the
/// kept text does not - a placeholder, a parameter, an identifier, a quoted
/// value, a number, a proper noun, an ordering. Explanations do none of
/// those.
fn carries_assertion(kept: &str, sentence: &str) -> bool {
    let lowered = sentence.to_lowercase();
    let negation = regex::Regex::new(r"(?i)\b(no|not|never|neither|none|nothing)\b|n't\b").unwrap();
    if negation.is_match(&lowered) {
        return true;
    }
    let ordering = regex::Regex::new(r"(?i)\b(first|last|before|after|top|bottom|ascending|descending|order|only)\b").unwrap();
    if ordering.is_match(&lowered) {
        return true;
    }
    let kept_lower = kept.to_lowercase();
    let mut first_word = true;
    for raw in sentence.split_whitespace() {
        let tok = raw.trim_matches(|c: char| ",.;:()".contains(c));
        if tok.is_empty() {
            continue;
        }
        let names = tok.starts_with('{')
            || tok.starts_with('@')
            || tok.contains('_')
            || tok.chars().any(|c| c.is_ascii_digit())
            || tok.starts_with(['\'', '"', '\u{2018}', '\u{201c}'])
            || (!first_word && tok.chars().next().is_some_and(|c| c.is_uppercase()));
        first_word = false;
        if names && !kept_lower.contains(&tok.to_lowercase()) {
            return true;
        }
    }
    false
}
```

Check `regex` is already a dependency of the crate (`speccov.rs` uses it); if the crate imports it as `use regex::Regex`, match that style.

At the caller (~778):

```rust
            let (cleaned_expected, kept) = clean_expected_keeping(&s.expected);
            if kept {
                report.assertions_kept += 1;
            }
```

and add to `OptimizeReport`:

```rust
    /// Trailing sentences kept because they carried an assertion - the
    /// count that used to be inside `expected_trimmed` as damage.
    pub assertions_kept: usize,
```

- [ ] **Step 4: Tool description**

`mcp.rs:187`: prepend to the `optimize_cases` description: `"Run with dry_run: true FIRST and read expected_rewritten before committing - it lists every expected result this would shorten, before and after. "`.

- [ ] **Step 5: Run the whole optimize suite**

Run: `cargo test --test optimize`
Expected: all pass, including `expected_results_keep_only_the_outcome` and `an_expected_result_that_lost_text_is_named_in_the_report` unchanged. If `a_trailing_gloss_still_goes` fails on the "confirmation email" case, the Capitalised-word rule is firing on a sentence-start word — check `first_word` handling.

- [ ] **Step 6: Commit**

```bash
git add v2/src-tauri/src/optimize.rs v2/src-tauri/src/mcp.rs v2/src-tauri/tests/optimize.rs
git commit -F - <<'EOF'
fix(tcm): optimize_cases keeps a trailing sentence that asserts something

Round 8 §8/§12. Nine trims in ten were removing the clause the case existed
to check. A later sentence now survives when it negates, orders, or names a
value the kept text does not; a gloss still goes. The report counts what
it kept, and the tool description leads with dry_run.
EOF
```

---

### Task 4: Transform gaps — preconditions, comment, honest replace reports

**Files:**
- Modify: `v2/src-tauri/src/transform.rs` (`Op`, `parse_ops`, supported list, find-empty guard ~501, `apply`, `describe`, `known_keys`)
- Modify: `v2/src-tauri/src/mcp.rs:206`
- Test: `v2/src-tauri/tests/transform.rs`

- [ ] **Step 1: Write the failing tests**

```rust
// ---- round 8 §10: the fields transform could not reach --------------------

/// The claim in §10 - "replaces only the first occurrence" - is false for
/// this code, and this pins it so the question stays settled.
#[test]
fn replace_in_ops_replace_every_occurrence_and_report_the_count() {
    let mut c = noted("Appraisee and Appraisee", "Appraisee, Appraisee, Appraisee");
    c.steps = vec![step("Appraisee opens; Appraisee saves.")];
    let cases = vec![c, noted("lowercase appraisee only", "an appraisee")];
    let ops = parse_ops(&serde_json::json!([
        { "op": "replace_in_title", "find": "Appraisee", "replace": "Employee" },
        { "op": "replace_in_notes", "find": "Appraisee", "replace": "Employee" },
        { "op": "replace_in_steps", "find": "Appraisee", "replace": "Employee" },
    ]))
    .unwrap();
    let (out, report) = apply(cases, &ops);
    assert_eq!(out[0].title, "Employee and Employee");
    assert_eq!(out[0].reviewer_notes, "Employee, Employee, Employee");
    assert_eq!(out[0].steps[0].action, "Employee opens; Employee saves.");
    assert!(report.applied[0].contains("2 occurrence(s)"), "{:?}", report.applied);
    assert!(report.applied.iter().any(|l| l.contains("3 occurrence(s)")), "{:?}", report.applied);
    // The case-variant hint: what actually left cases behind in the field.
    assert!(
        report.warnings.iter().any(|w| w.contains("different capitalisation") && w.contains("1 case")),
        "{:?}",
        report.warnings
    );
}

#[test]
fn replace_in_preconditions_and_set_comment_exist() {
    let mut c = noted("A", "n");
    c.preconditions = "Signed in as Appraisee".into();
    c.comment = "Blocked on a decision".into();
    let ops = parse_ops(&serde_json::json!([
        { "op": "replace_in_preconditions", "find": "Appraisee", "replace": "Employee" },
        { "op": "set_comment", "value": "" },
    ]))
    .unwrap();
    let (out, _) = apply(vec![c], &ops);
    assert_eq!(out[0].preconditions, "Signed in as Employee");
    assert_eq!(out[0].comment, "", "an empty value clears the comment");
    let (out, _) = apply(out, &parse_ops(&serde_json::json!([{ "op": "set_comment", "value": "Reviewed" }])).unwrap());
    assert_eq!(out[0].comment, "Reviewed");
    let err = parse_ops(&serde_json::json!([{ "op": "replace_in_preconditions", "find": "", "replace": "x" }])).unwrap_err();
    assert!(err.contains("find"), "{err}");
}

/// A blanket replace that lands inside a verbatim quote silently breaks the
/// citation contract; the diff does not show it. The report has to.
#[test]
fn a_replacement_inside_a_verbatim_quote_is_reported() {
    let cases = vec![noted("Q", "Checks it.\nSpec: S.md 1\n> \"The Appraisee list refreshes.\"")];
    let ops = parse_ops(&serde_json::json!([
        { "op": "replace_in_notes", "find": "Appraisee", "replace": "Employee" }
    ]))
    .unwrap();
    let (_, report) = apply(cases, &ops);
    assert!(
        report.warnings.iter().any(|w| w.contains("inside a verbatim quote") && w.contains("1 ")),
        "{:?}",
        report.warnings
    );
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --test transform round_8 replace_in`
Expected: the first fails on `2 occurrence(s)`; the second fails at `parse_ops` with unknown op.

- [ ] **Step 3: Implement**

`Op`: add `ReplaceInPreconditions { find: String, replace: String }` and `/// Set (or, with an empty value, clear) the local comment.\n    SetComment(String)`.
`parse_ops`: `"replace_in_preconditions" => Op::ReplaceInPreconditions { find: str_of(v, "find"), replace: str_of(v, "replace") },` and `"set_comment" => Op::SetComment(value),` (deliberately `value`, not `required_str` — empty clears).
Supported-ops error text: add both names. `known_keys`: `set_comment` joins the `"value"` arm; `replace_in_preconditions` joins the `find, replace` arm. Find-empty guard: add `| Op::ReplaceInPreconditions { find, .. }`. `describe`: two lines in the existing style. `before_snapshot` list and `find_driven` list: add `ReplaceInPreconditions`; extend the snapshot comparison with `|| before.preconditions != c.preconditions`.

In the `match other` body:

```rust
                        Op::ReplaceInPreconditions { find, replace } => {
                            occurrences += c.preconditions.matches(find.as_str()).count();
                            if !c.preconditions.contains(find.as_str()) && c.preconditions.to_lowercase().contains(&find.to_lowercase()) { variants += 1; }
                            c.preconditions = c.preconditions.replace(find.as_str(), replace);
                        }
                        Op::SetComment(v) => c.comment = v.clone(),
```

and add the same two counting lines to `ReplaceInTitle`, `ReplaceInNotes` (over `reviewer_notes`) and `ReplaceInSteps` (sum over `action` and `expected`). For `ReplaceInNotes` additionally, before replacing:

```rust
                            in_quotes += c
                                .reviewer_notes
                                .lines()
                                .filter(|l| l.trim_start().starts_with('>'))
                                .map(|l| l.matches(find.as_str()).count())
                                .sum::<usize>();
```

Declare `let (mut occurrences, mut variants, mut in_quotes) = (0usize, 0usize, 0usize);` beside `modified`. In the `find_driven` report block, change the line to

```rust
                    report.applied.push(format!(
                        "{} modified {modified} case(s), {occurrences} occurrence(s).",
                        describe(other)
                    ));
```

(for `RemoveStepMatching`/`SplitStep`, which have no `occurrences`, keep the old line — branch on `matches!(other, Op::ReplaceIn...)`). After it:

```rust
                    if variants > 0 {
                        if let Some(find) = find_of(other) {
                            report.warnings.push(format!(
                                "{variants} case(s) contain '{find}' in different capitalisation and were left alone - replace is case-sensitive."
                            ));
                        }
                    }
                    if in_quotes > 0 {
                        report.warnings.push(format!(
                            "{in_quotes} replacement(s) landed inside a verbatim quote - check_spec_coverage may now report quote_not_in_document for those cases."
                        ));
                    }
```

with a small helper `fn find_of(op: &Op) -> Option<&str>` returning the `find` of the four replace ops.

- [ ] **Step 4: Tool description**

`mcp.rs:206`: add `replace_in_preconditions` to the `{find, replace}` list and `set_comment (empty value clears)` to the `{value}` list; append `Replace ops are literal, case-sensitive and replace EVERY occurrence; the report gives the occurrence count and names cases left alone for differing capitalisation.`

- [ ] **Step 5: Run the transform suite**

Run: `cargo test --test transform`
Expected: all pass, including `replace_in_notes_edits_only_the_notes_and_counts_honestly` (its `modified 1 case(s)` substring survives the new line).

- [ ] **Step 6: Commit**

```bash
git add v2/src-tauri/src/transform.rs v2/src-tauri/src/mcp.rs v2/src-tauri/tests/transform.rs
git commit -F - <<'EOF'
feat(tcm): replace_in_preconditions, set_comment, and replace reports that count

Round 8 §10. Two fields transform could not reach, and a report that said
"modified N case(s)" for a replace that is - and always was - every
occurrence, case-sensitive. It now counts occurrences, names the cases a
capitalisation difference left alone, and says when a replacement landed
inside a verbatim quote.
EOF
```

---

### Task 5: Copy-in keeps the newest bytes under the obvious name

**Files:**
- Modify: `v2/src-tauri/src/workspace.rs:55-95` (`copy_into_cases`)
- Modify: `v2/src-tauri/src/applog.rs` (a `pub fn file_stamp()` beside `stamp()`)
- Modify: `v2/src-tauri/src/commands/workspace.rs` (`CopiedIn`)
- Modify: `v2/src/screens/ImportFile.tsx:457-465, ~500` (the copy + toast)
- Regenerate: `v2/src/bindings.ts`
- Test: `v2/src-tauri/tests/workspace.rs`, `v2/src/screens/ImportFile.test.tsx`

**Interfaces:**
- Produces: `workspace::copy_into_cases(root, source) -> Result<(PathBuf, Option<PathBuf>), String>`; command returns `CopiedIn { path: String, displaced: Option<String> }`; TS `commands.copyIntoCases(root, source)` resolves to `{ status, data: { path, displaced } }`.

- [ ] **Step 1: Rewrite the pinning test and add the new one**

In `tests/workspace.rs`, replace `a_different_file_with_the_same_name_gets_a_suffix_not_an_overwrite` with:

```rust
/// Round 8 §11. The old rule - never overwrite, newer bytes take `-2`,
/// `-3` - left the OLDEST content under the canonical name, and on an
/// id-carrying set importing the obvious file silently reverted fifteen
/// corrected work items. The picked file is the one the user wants: it
/// takes the canonical name, and what it displaces goes to `.history`.
#[test]
fn a_different_file_with_the_same_name_replaces_the_copy_and_keeps_the_old_one() {
    let root = temp_root("displace");
    let dir = ensure_cases_dir(&root).unwrap();
    std::fs::write(dir.join("login.json"), "old").unwrap();
    let src = root.join("in").join("login.json");
    std::fs::create_dir_all(src.parent().unwrap()).unwrap();
    std::fs::write(&src, "new").unwrap();

    let (copied, displaced) = copy_into_cases(&root, &src).unwrap();
    assert_eq!(copied, dir.join("login.json"), "the obvious name is the newest");
    assert_eq!(std::fs::read_to_string(&copied).unwrap(), "new");
    let displaced = displaced.expect("the old bytes were kept somewhere");
    assert!(is_inside(&dir.join(".history"), &displaced), "{}", displaced.display());
    assert!(displaced.file_name().unwrap().to_string_lossy().starts_with("login."));
    assert_eq!(std::fs::read_to_string(&displaced).unwrap(), "old", "nothing is ever lost");
    assert!(!dir.join("login-2.json").exists(), "no more numbered copies");

    // Picking it again with the same bytes: no second displacement.
    let (again, none) = copy_into_cases(&root, &src).unwrap();
    assert_eq!(again, copied);
    assert!(none.is_none());
    let _ = std::fs::remove_dir_all(&root);
}
```

Update the two other tests that destructure the return: `let copied = copy_into_cases(..).unwrap();` becomes `let (copied, _) = ...` in `a_picked_file_is_copied_in_and_picking_it_again_reuses_the_copy` and `assert_eq!(copy_into_cases(&root, &inside).unwrap().0, inside)` in `a_file_already_in_the_folder_is_not_copied`.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --test workspace`
Expected: compile error on the tuple destructure.

- [ ] **Step 3: Implement**

`applog.rs`, after `stamp()`:

```rust
/// The same clock as `stamp`, in a form a file name can carry:
/// "YYYYMMDD-HHMMSS".
pub fn file_stamp() -> String {
    let (y, mo, d, h, mi, s) = now_parts();
    format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}")
}
```

`workspace.rs`: replace `copy_into_cases` with:

```rust
/// Copy `source` into the cases folder and return where it landed, plus
/// where anything it displaced went. A file already inside is returned as
/// is; identical bytes reuse the existing file.
///
/// Different bytes under the same name REPLACE the file - the pick is the
/// user's statement of which content they want - and the previous copy
/// moves to `.history/<stem>.<stamp>.json`. Nothing is ever deleted.
///
/// Round 8 §11: this used to refuse to overwrite and write `name-2.json`,
/// `name-3.json` instead, which kept the OLDEST bytes under the obvious
/// name. On a set carrying work item ids, importing that file silently
/// reverted fifteen corrected cases in Azure DevOps. Safety was the intent;
/// the naming had it backwards.
pub fn copy_into_cases(root: &Path, source: &Path) -> Result<(PathBuf, Option<PathBuf>), String> {
    let dir = ensure_cases_dir(root)?;
    if is_inside(&dir, source) {
        return Ok((source.to_path_buf(), None));
    }
    let name = source
        .file_name()
        .ok_or_else(|| format!("not a file: {}", source.display()))?;
    let bytes =
        std::fs::read(source).map_err(|e| format!("could not read {}: {e}", source.display()))?;
    let target = dir.join(name);
    let displaced = match std::fs::read(&target) {
        Ok(existing) if existing == bytes => return Ok((target, None)),
        Ok(_) => Some(displace(&dir, &target)?),
        Err(_) => None,
    };
    std::fs::write(&target, &bytes)
        .map_err(|e| format!("could not write {}: {e}", target.display()))?;
    Ok((target, displaced))
}

/// Move `target` into `.history` under a stamped name and return the new
/// path. A clash within the same second takes `-2`, `-3`, ...
fn displace(dir: &Path, target: &Path) -> Result<PathBuf, String> {
    let history = dir.join(".history");
    std::fs::create_dir_all(&history)
        .map_err(|e| format!("could not create {}: {e}", history.display()))?;
    let stem = target.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "test-cases".into());
    let ext = target.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let stamp = crate::applog::file_stamp();
    let mut n = 1u32;
    loop {
        let file = if n == 1 { format!("{stem}.{stamp}{ext}") } else { format!("{stem}.{stamp}-{n}{ext}") };
        let candidate = history.join(file);
        if !candidate.exists() {
            std::fs::rename(target, &candidate)
                .map_err(|e| format!("could not move {} aside: {e}", target.display()))?;
            return Ok(candidate);
        }
        n += 1;
    }
}
```

`commands/workspace.rs`:

```rust
/// Where a picked file landed, and where the copy it replaced went.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct CopiedIn {
    pub path: String,
    /// Set when a different file of the same name was already there - it
    /// now lives under `.test-cases/.history`.
    pub displaced: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub fn copy_into_cases(root: String, source: String) -> Result<CopiedIn, String> {
    crate::workspace::copy_into_cases(Path::new(&root), Path::new(&source)).map(|(p, d)| CopiedIn {
        path: p.to_string_lossy().to_string(),
        displaced: d.map(|d| d.to_string_lossy().to_string()),
    })
}
```

Update the doc comment above it to match.

- [ ] **Step 4: Regenerate bindings and run the Rust side**

Run: `cargo test --test bindings` then `cargo test --test workspace`.
Expected: `bindings.ts` now has `CopiedIn`; workspace tests pass.

- [ ] **Step 5: Frontend — use the new shape and say where the old copy went**

`ImportFile.tsx` ~457:

```tsx
      let path = picked;
      let copied = false;
      let displaced: string | null = null;
      if (root && !isInsideCasesDir(root, picked)) {
        const c = await commands.copyIntoCases(root, picked);
        if (c.status === "error") throw new Error(c.error);
        path = c.data.path;
        displaced = c.data.displaced;
        copied = path !== picked;
      }
```

add `displaced` to the returned object and the `onSuccess` destructure, and change the toast tail:

```tsx
          (displaced
            ? ` - replaced the copy in ${CASES_DIR}; the previous one is in .history`
            : copied
              ? ` - copied into ${CASES_DIR}`
              : ""),
```

`ImportFile.test.tsx`: the two mocks returning a string for `copy_into_cases` return `{ path: "D:\\repo\\.test-cases\\cases.json", displaced: null }` instead. Add:

```tsx
/// Round 8 §11: a re-pick with different content replaces the copy the app
/// follows, and the toast says where the old one went.
test("a re-picked file with new content replaces the copy and says so", async () => {
  localStorage.setItem("tcm-v2-working-dir", "D:\\repo");
  mockIPC((cmd) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "plugin:dialog|open") return "C:\\Downloads\\cases.json";
    if (cmd === "copy_into_cases")
      return { path: "D:\\repo\\.test-cases\\cases.json", displaced: "D:\\repo\\.test-cases\\.history\\cases.20260908-101500.json" };
    if (cmd === "parse_import_file") return { cases: [oneCase], warnings: [] };
    if (cmd === "file_stamp") return "abc";
    if (cmd === "read_general_comment") return "";
    if (cmd === "watch_file") return null;
    if (cmd === "unwatch_all_files") return null;
    return [];
  });
  renderScreen();
  fireEvent.click(await screen.findByRole("button", { name: "Import JSON" }));
  await screen.findByText("Copied case");
  expect(await screen.findByText(/previous one is in \.history/)).toBeInTheDocument();
});
```

If the toasts in this test file are asserted through a sonner mock rather than the DOM, follow the file's existing pattern.

- [ ] **Step 6: Typecheck and run the file**

Run: `cd v2 && npx tsc --noEmit` then `npx vitest run src/screens/ImportFile.test.tsx`.
Expected: clean; all tests in the file pass.

- [ ] **Step 7: Commit**

```bash
git add v2/src-tauri/src/workspace.rs v2/src-tauri/src/applog.rs v2/src-tauri/src/commands/workspace.rs v2/src-tauri/tests/workspace.rs v2/src/bindings.ts v2/src/screens/ImportFile.tsx v2/src/screens/ImportFile.test.tsx
git commit -F - <<'EOF'
fix(v2): a re-picked file replaces the copy the app follows; the old one goes to .history

Round 8 §11. Refusing to overwrite kept the OLDEST bytes under the obvious
name and wrote the corrections to login-2.json, login-3.json - and on a set
carrying work item ids, importing the obvious file silently reverted fifteen
corrected cases. The pick now takes the canonical name; what it displaces is
kept, stamped, under .test-cases/.history. Nothing is deleted.
EOF
```

---

### Task 6: `merge_case_files` names the slices it consumed

**Files:**
- Modify: `v2/src-tauri/src/ai_bridge.rs` (the merge response, after the atomic rename ~1140-1170)
- Modify: `v2/src-tauri/src/mcp.rs:162-170` (description)
- Test: whichever test file already exercises `/merge` — run `grep -rln '"/merge"' v2/src-tauri/tests/` and extend a passing test there; if none exists, add one to `tests/ai_bridge.rs` using `route(&ctx(), None, "POST", "/merge", body, "1.23.2")` with two temp slice files.

- [ ] **Step 1: Write the failing assertion**

In the chosen test, after the merge succeeds:

```rust
    let superseded = v["superseded"].as_array().expect("the consumed slices are named");
    assert_eq!(superseded.len(), 2, "{body}");
    assert!(v["note"].as_str().unwrap_or_default().contains("safe to remove"), "{body}");
    for p in &paths {
        assert!(std::path::Path::new(p).exists(), "the tool deletes nothing: {p}");
    }
```

- [ ] **Step 2: Run to verify it fails** — `cargo test --test <file> merge`.

- [ ] **Step 3: Implement**

In the success response `serde_json::json!({ ... })` of the merge handler add:

```rust
        "superseded": req.paths,
        "note": format!(
            "The {} slice file(s) above are now superseded by {} and safe to remove from .test-cases - they are importable and id-less, and the importer will happily offer them.",
            req.paths.len(),
            req.output_path
        ),
```

`mcp.rs` description: append `The response names the slice files it consumed as superseded; remove them from .test-cases yourself - this tool deletes nothing.`

- [ ] **Step 4: Run and commit**

Run the test binary. Then:

```bash
git add v2/src-tauri/src/ai_bridge.rs v2/src-tauri/src/mcp.rs v2/src-tauri/tests/
git commit -F - <<'EOF'
feat(tcm): merge_case_files names the slices it made redundant

Round 8 §11.4. Five slice files, 106 cases, sat in the import folder next
to the 104-case draft they were merged into. The merge now says which files
are superseded; removing them stays a human action.
EOF
```

---

### Task 7: The fanout skill asks for a model per slice

**Files:**
- Modify: `v2/src-tauri/src/ai_tools.rs:191-211` (the `fanout` body)
- Test: `v2/src-tauri/tests/ai_tools.rs` (~line 84, the loop over `COMMANDS`)

- [ ] **Step 1: Write the failing test**

```rust
/// Round 8 §13: nothing in the skill mentioned model choice, so every slice
/// inherited the parent's - Opus for a layout-table transcription.
#[test]
fn the_fanout_skill_asks_for_a_model_per_slice() {
    let fanout = COMMANDS.iter().find(|c| c.stem == "fanout").unwrap();
    let md = command_markdown(fanout);
    assert!(md.contains("Choose a model per slice"), "{md}");
    assert!(md.contains("Pass `model` on each `Agent` call"), "{md}");
    assert!(md.contains("specification contradicts itself"), "{md}");
}
```

- [ ] **Step 2: Run to verify it fails** — `cargo test --test ai_tools fanout`.

- [ ] **Step 3: Add the wording**

In the `fanout` `body`, after the line ending `` `get_test_cases`.`` and its blank `""`, insert (verbatim from the feedback):

```rust
            "**Choose a model per slice; do not default the whole fan-out to one.**",
            "Pass `model` on each `Agent` call. Reserve the strongest model for slices",
            "that must reason about behaviour - recursive or multi-join SQL, ordering and",
            "null semantics, security properties, and any slice whose brief says the",
            "authority is the deployed build. A smaller model is usually enough for slices",
            "that transcribe a layout table, a registration script or a checklist into",
            "cases.",
            "",
            "State the choice and the reason in one line when you report the dispatch, so",
            "the developer can overrule it.",
            "",
            "Caution: the first thing a smaller model stops doing is noticing that the",
            "specification contradicts itself. If a slice's job includes finding",
            "divergences rather than just covering sections, keep it on the strong",
            "model.",
            "",
```

- [ ] **Step 4: Run and commit**

Run: `cargo test --test ai_tools`. Then:

```bash
git add v2/src-tauri/src/ai_tools.rs v2/src-tauri/tests/ai_tools.rs
git commit -F - <<'EOF'
docs(tcm): the fanout skill asks for a model per slice

Round 8 §13. The on-disk command regenerates through sync_commands on the
next launch or tool toggle.
EOF
```

**User action after this ships:** restart the app once (or toggle any tool in the AI Bridge tab) — `sync_commands` rewrites `~/.claude/commands/tcm/fanout.md` and the per-repo copy.

---

### Task 8: Version, changelog, and both full suites

**Files:**
- Modify: `v2/src-tauri/tauri.conf.json`, `v2/src-tauri/Cargo.toml`, `v2/src/lib/changelog.ts` — `1.24.0`
- Modify: `CLAUDE.md:18,48` — `master` → `main` (the branch was renamed on 2026-09-08)

- [ ] **Step 1: Bump and write the changelog entry** (user-facing register, like the existing entries):

```ts
  {
    version: "1.24.0",
    date: "<today>",
    items: [
      "Importing a file you had picked before, with new content, now replaces the copy the app follows instead of writing it to a numbered file beside it. The obvious name used to keep the OLDEST content - on a set carrying work item ids, importing it would have silently undone corrections in Azure DevOps. The previous copy is kept under .test-cases/.history.",
      "For assistants: optimize_cases no longer trims away a second sentence that carries the assertion; transform_cases gains normalise_citations, replace_in_preconditions and set_comment, and its replace reports now count occurrences; the citation advisories say where a misplaced quote should go; merge_case_files names the slice files it superseded.",
    ],
  },
```

- [ ] **Step 2: Run both suites, one at a time**

`cd v2/src-tauri && cargo test --tests` then `cd v2 && npx tsc --noEmit && npm test`.
Expected: green. `App.test.tsx` has a documented load flake — one timeout that passes on re-run alone is that; anything else is not.

- [ ] **Step 3: Commit**

```bash
git add v2/src-tauri/tauri.conf.json v2/src-tauri/Cargo.toml v2/src-tauri/Cargo.lock v2/src/lib/changelog.ts CLAUDE.md
git commit -F - <<'EOF'
chore(v2): 1.24.0
EOF
```

Release is the user's call: `v2/scripts/release-v2.ps1 -Version 1.24.0`.
