//! Spec coverage: section inventories, citation parsing and the coverage
//! report built from them.

use v2_lib::speccov::{check_coverage, parse_citations, parse_inventory, CoverageInput};

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
    let cases: Vec<v2_lib::model::TestCase> = vec![];
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

fn case(title: &str, notes: &str) -> v2_lib::model::TestCase {
    v2_lib::model::TestCase {
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
    let cases: Vec<v2_lib::model::TestCase> = vec![];
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
    let cases: Vec<v2_lib::model::TestCase> = vec![];
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
    let cases: Vec<v2_lib::model::TestCase> = vec![];
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
    let cases: Vec<v2_lib::model::TestCase> = vec![];
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
    let cases: Vec<v2_lib::model::TestCase> = vec![];
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
    let cases: Vec<v2_lib::model::TestCase> = vec![];
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
    let cases: Vec<v2_lib::model::TestCase> = vec![];
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
    let cases: Vec<v2_lib::model::TestCase> = vec![];
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
