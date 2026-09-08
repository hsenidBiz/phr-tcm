//! The round-5 Part II fixes to `transform_cases`: honest counts, named
//! keys, positions that exist, a draft that can travel as a path, and a
//! report that echoes what it ignored.

use v2_lib::ai_bridge::{route, BridgeContext};
use v2_lib::model::TestCase;
use v2_lib::steps_xml::Step;
use v2_lib::transform::{apply, parse_ops, parse_ops_full};

fn ctx() -> BridgeContext {
    BridgeContext::default()
}

fn step(action: &str) -> Step {
    Step { action: action.into(), expected: "It happens.".into() }
}

fn case(title: &str, steps: Vec<Step>) -> TestCase {
    TestCase { title: title.into(), steps, ..Default::default() }
}

struct TempDir(std::path::PathBuf);
impl TempDir {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir()
            .join(format!("tcm-transform-{nanos}-{}", N.fetch_add(1, Ordering::SeqCst)));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ---- §12: an op that matched nothing must not read like one that worked

#[test]
fn a_find_that_matches_nothing_says_so_instead_of_counting_the_filter() {
    let cases = vec![case("A", vec![step("Expand the row.")]), case("B", vec![step("Save.")])];
    let ops = parse_ops(&serde_json::json!([
        { "op": "replace_in_steps", "find": "Expand", "replace": "Open" },
        { "op": "replace_in_steps", "find": "ZZZ_NOT_PRESENT_ZZZ", "replace": "Open" },
    ]))
    .unwrap();
    let (_, report) = apply(cases, &ops);

    // The working op reports the case it CHANGED (1), not the two the
    // absent filter selected; the no-op reports zero plus the check-it
    // line - round 5's probe showed both lines identical at "2 case(s)".
    assert!(
        report.applied[0].contains("modified 1 case(s)"),
        "{:?}",
        report.applied
    );
    assert!(report.applied[1].contains("modified 0 case(s)"), "{:?}", report.applied);
    assert!(
        report.applied.iter().any(|l| l.contains("the find matched no text")),
        "the silent no-op line is missing: {:?}",
        report.applied
    );
}

#[test]
fn remove_step_matching_gets_the_same_no_match_honesty_as_remove_cases() {
    let cases = vec![case("A", vec![step("Step one."), step("Step two.")])];
    let ops = parse_ops(&serde_json::json!([
        { "op": "remove_step_matching", "value": "ZZZ_MATCHES_NOTHING" },
    ]))
    .unwrap();
    let (out, report) = apply(cases, &ops);
    assert_eq!(out[0].steps.len(), 2, "steps must be untouched");
    assert!(report.applied[0].contains("modified 0 case(s)"), "{:?}", report.applied);
    assert!(report.applied.iter().any(|l| l.contains("the find matched no text")));
}

// ---- §11: the missing-key error names the accepted keys, and `action`
// ---- is a legal alias because the schema's field list makes it look right

#[test]
fn remove_step_matching_accepts_action_and_its_error_names_every_key() {
    let ok = parse_ops(&serde_json::json!([
        { "op": "remove_step_matching", "action": "Open the page." },
    ]));
    assert!(ok.is_ok(), "the schema advertises `action`; refusing it is a trap: {ok:?}");

    let err = parse_ops(&serde_json::json!([{ "op": "remove_step_matching" }])).unwrap_err();
    for key in ["value", "find", "action"] {
        assert!(err.contains(key), "the error must name '{key}': {err}");
    }
}

// ---- §13: insert at a position, and the direct split_step op ------------

#[test]
fn insert_cases_lands_where_it_was_asked_not_at_the_end() {
    let cases = vec![case("Alpha", vec![step("s")]), case("Zulu", vec![step("s")])];
    let ops = parse_ops(&serde_json::json!([
        { "op": "insert_cases", "after": "Alpha",
          "cases": [{ "title": "Target", "steps": [{ "action": "s", "expected": "e" }] }] },
    ]))
    .unwrap();
    let (out, report) = apply(cases, &ops);
    let titles: Vec<&str> = out.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles, vec!["Alpha", "Target", "Zulu"], "spec order is the point");
    assert!(report.applied[0].contains("at index 1"), "{:?}", report.applied);
}

#[test]
fn insert_cases_at_index_and_before_work_and_a_missed_fragment_warns() {
    let cases = vec![case("Alpha", vec![step("s")]), case("Zulu", vec![step("s")])];
    let ops = parse_ops(&serde_json::json!([
        { "op": "insert_cases", "at_index": 0,
          "cases": [{ "title": "First", "steps": [{ "action": "s", "expected": "e" }] }] },
        { "op": "insert_cases", "before": "NO SUCH TITLE",
          "cases": [{ "title": "Lost", "steps": [{ "action": "s", "expected": "e" }] }] },
    ]))
    .unwrap();
    let (out, report) = apply(cases, &ops);
    assert_eq!(out[0].title, "First");
    // The missed fragment appends rather than dropping the case, and SAYS so.
    assert_eq!(out.last().unwrap().title, "Lost");
    assert!(
        report.warnings.iter().any(|w| w.contains("NO SUCH TITLE")),
        "{:?}",
        report.warnings
    );
}

#[test]
fn the_old_position_spellings_fail_loudly_naming_the_real_ones() {
    // `index`/`at`/`position` used to be swallowed whole (§13/§15).
    let err = parse_ops(&serde_json::json!([
        { "op": "insert_cases", "index": 1,
          "cases": [{ "title": "T", "steps": [{ "action": "s", "expected": "e" }] }] },
    ]))
    .unwrap_err();
    assert!(err.contains("at_index"), "the error must point at the real key: {err}");
}

#[test]
fn split_step_replaces_the_matching_step_in_place() {
    // The 18-case edit from round 5, as ONE operation instead of 5-8.
    let cases = vec![case(
        "No Approval Actions Are Offered",
        vec![
            step("Sign in."),
            step("Navigate to Reviews and click the tab."),
            step("Check the summary."),
        ],
    )];
    let ops = parse_ops(&serde_json::json!([
        { "op": "split_step", "find": "Navigate to Reviews and click",
          "into": [
              { "action": "Navigate to Reviews.", "expected": "The hub opens." },
              { "action": "Click the tab.", "expected": "The tab opens." },
          ] },
    ]))
    .unwrap();
    let (out, report) = apply(cases, &ops);
    let actions: Vec<&str> = out[0].steps.iter().map(|s| s.action.as_str()).collect();
    assert_eq!(
        actions,
        vec!["Sign in.", "Navigate to Reviews.", "Click the tab.", "Check the summary."],
        "the split lands at the matched step's own index"
    );
    assert!(report.applied[0].contains("modified 1 case(s)"), "{:?}", report.applied);
}

// ---- §15: echo what you ignored ----------------------------------------

#[test]
fn unknown_operation_keys_and_dropped_case_fields_are_echoed() {
    let (_, ignored) = parse_ops_full(&serde_json::json!([
        // `find` is not read by set_tags - the §15 class in miniature.
        { "op": "set_tags", "value": "smoke", "find": "oops" },
        // The exact §15 evidence payload: author/priority/notes/step_number
        // are silently deleted by the rebuild - now they are named.
        { "op": "insert_cases", "cases": [{
            "title": "Probe One", "author": "avin", "priority": 2,
            "steps": [{ "action": "Step A.", "expected": "A happens.",
                        "notes": "step note", "step_number": 1 }],
            "reviewer_notes": "Kept."
        }] },
    ]))
    .unwrap();
    assert!(
        ignored.iter().any(|l| l.contains("\"find\"") && l.contains("set_tags")),
        "{ignored:?}"
    );
    let dropped = ignored.iter().find(|l| l.contains("cases[0]")).expect("dropped-fields line");
    for k in ["author", "priority", "steps.notes", "steps.step_number"] {
        assert!(dropped.contains(k), "'{k}' missing from: {dropped}");
    }
}

// ---- §10: the draft travels as a path, and both-sources is refused ------

#[tokio::test]
async fn a_draft_can_be_transformed_from_a_path_and_written_back_in_place() {
    let dir = TempDir::new();
    let path = dir.0.join("draft.json");
    std::fs::write(
        &path,
        serde_json::json!({ "test_cases": [
            { "title": "A", "steps": [{ "action": "Expand the row.", "expected": "Opens." }] }
        ]})
        .to_string(),
    )
    .unwrap();

    let body = serde_json::json!({
        "path": path.to_string_lossy(),
        "in_place": true,
        "operations": [ { "op": "replace_in_steps", "find": "Expand", "replace": "Open" } ],
    })
    .to_string();
    let (status, out) = route(&ctx(), None, "POST", "/transform", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");

    // No JSON echo - the write-back IS the result - and the file changed.
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(v.get("test_cases").is_none(), "in_place must not echo the draft: {out}");
    assert_eq!(v["cases"], 1);
    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert!(on_disk.contains("Open the row."), "the file was not rewritten: {on_disk}");
    assert!(!std::path::Path::new(&format!("{}.tmp", path.display())).exists());
}

#[tokio::test]
async fn path_and_inline_json_together_is_a_400_not_a_silent_preference() {
    let dir = TempDir::new();
    let path = dir.0.join("draft.json");
    std::fs::write(&path, "[]").unwrap();
    let body = serde_json::json!({
        "path": path.to_string_lossy(),
        "test_cases": "[]",
        "operations": [],
    })
    .to_string();
    let (status, out) = route(&ctx(), None, "POST", "/transform", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("not both"), "{out}");
}

#[tokio::test]
async fn in_place_without_a_path_is_refused() {
    let body = serde_json::json!({
        "test_cases": "[{\"title\":\"A\",\"steps\":[{\"action\":\"s\",\"expected\":\"e\"}]}]",
        "in_place": true,
        "operations": [],
    })
    .to_string();
    let (status, out) = route(&ctx(), None, "POST", "/transform", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
}

#[tokio::test]
async fn unknown_request_arguments_surface_in_the_reports_ignored_list() {
    let body = serde_json::json!({
        "test_cases": "[{\"title\":\"A\",\"steps\":[{\"action\":\"s\",\"expected\":\"e\"}]}]",
        "dry_run": true, // transform has no dry_run - it must not vanish
        "operations": [],
    })
    .to_string();
    let (status, out) = route(&ctx(), None, "POST", "/transform", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let ignored = v["report"]["ignored"].as_array().unwrap();
    assert!(
        ignored.iter().any(|l| l.as_str().unwrap_or("").contains("dry_run")),
        "{out}"
    );
}

// ---- round 6 §1: reviewer_notes are finally editable --------------------

fn noted(title: &str, notes: &str) -> TestCase {
    TestCase { reviewer_notes: notes.into(), ..case(title, vec![step("s")]) }
}

#[test]
fn replace_in_notes_edits_only_the_notes_and_counts_honestly() {
    // The §1 probe, inverted: the find-string planted in every field, and
    // only reviewer_notes may change.
    let mut c = noted("Has FIND in title: FIND", "Spec: FIND section 5");
    c.preconditions = "FIND".into();
    c.tags = "FIND".into();
    c.steps = vec![step("FIND the row.")];
    let cases = vec![c, noted("Untouched", "nothing to see")];
    let ops = parse_ops(&serde_json::json!([
        { "op": "replace_in_notes", "find": "FIND", "replace": "Step9 - FDP.md" },
    ]))
    .unwrap();
    let (out, report) = apply(cases, &ops);
    assert_eq!(out[0].reviewer_notes, "Spec: Step9 - FDP.md section 5");
    assert_eq!(out[0].title, "Has FIND in title: FIND", "title untouched");
    assert_eq!(out[0].preconditions, "FIND", "preconditions untouched");
    assert_eq!(out[0].tags, "FIND", "tags untouched");
    assert_eq!(out[0].steps[0].action, "FIND the row.", "steps untouched");
    assert!(report.applied[0].contains("modified 1 case(s)"), "{:?}", report.applied);
}

#[test]
fn set_reviewer_notes_overwrites_and_requires_a_value() {
    let cases = vec![noted("A", "old")];
    let ops = parse_ops(&serde_json::json!([
        { "op": "set_reviewer_notes", "value": "new note" },
    ]))
    .unwrap();
    let (out, _) = apply(cases, &ops);
    assert_eq!(out[0].reviewer_notes, "new note");

    let err = parse_ops(&serde_json::json!([{ "op": "set_reviewer_notes" }])).unwrap_err();
    assert!(err.contains("value"), "{err}");
}

// ---- round 6 §5: where.at_index addresses one of two title twins --------

#[test]
fn at_index_addresses_exactly_one_of_two_identical_cases() {
    // The fan-out collision: two legitimately different cases whose titles
    // converged. Every text filter matches both; the index picks one.
    let cases = vec![
        case("Alerts - No Notification When Cycle Inactive", vec![step("a")]),
        case("Alerts - No Notification When Cycle Inactive", vec![step("b")]),
    ];
    let ops = parse_ops(&serde_json::json!([
        { "op": "suffix_title", "value": " (SYS_03)", "where": { "at_index": 1 } },
    ]))
    .unwrap();
    let (out, report) = apply(cases, &ops);
    assert_eq!(out[0].title, "Alerts - No Notification When Cycle Inactive");
    assert_eq!(out[1].title, "Alerts - No Notification When Cycle Inactive (SYS_03)");
    assert!(report.applied[0].contains("applied to 1 case(s)"), "{:?}", report.applied);
}

#[test]
fn at_index_composes_with_text_filters_and_validates_its_type() {
    // AND semantics: index 0's module is not "Auth", so nothing matches.
    let mut c = case("A", vec![step("s")]);
    c.module_value = "Payments".into();
    let ops = parse_ops(&serde_json::json!([
        { "op": "set_tags", "value": "x", "where": { "at_index": 0, "module_is": "Auth" } },
    ]))
    .unwrap();
    let (out, report) = apply(vec![c], &ops);
    assert_eq!(out[0].tags, "");
    assert!(
        report.applied.iter().any(|l| l.contains("nothing matched")),
        "{:?}",
        report.applied
    );

    let err = parse_ops(&serde_json::json!([
        { "op": "set_tags", "value": "x", "where": { "at_index": "one" } },
    ]))
    .unwrap_err();
    assert!(err.contains("non-negative integer"), "{err}");
}

#[test]
fn remove_cases_accepts_an_index_only_filter() {
    let cases = vec![case("Keep", vec![step("a")]), case("Drop", vec![step("b")])];
    let ops = parse_ops(&serde_json::json!([
        { "op": "remove_cases", "where": { "at_index": 1 } },
    ]))
    .unwrap();
    let (out, _) = apply(cases, &ops);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].title, "Keep");
}

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
fn a_two_line_quote_above_the_pointer_is_joined_with_a_space() {
    let notes = "Checks the report identifies the right person.\n\n> first half\n> second half\n\nSpec: R.md General Requirements";
    let (out, outcome) = normalise_citation_notes(notes);
    assert!(matches!(outcome, CitationOutcome::Normalised));
    assert_eq!(
        out,
        "Checks the report identifies the right person.\n\nSpec: R.md General Requirements\n\n> \"first half second half\""
    );
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
fn the_op_reports_per_case() {
    let cases = vec![
        noted("Moves", "> q\nSpec: A.md 1"),
        noted("Table", "> | a |\nSpec: A.md 2"),
        noted("Fine", "Spec: A.md 3\n> \"q\""),
        noted("Skipped", "> q\nSpec: A.md 4"),
    ];
    let ops = parse_ops(&serde_json::json!([
        { "op": "normalise_citations" }
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

/// Review of round 8 §10: `replace_in_steps` counted `variants` once per
/// offending STEP, not once per case, so a case with two case-variant-only
/// steps was reported as "2 case(s)" - contradicting the warning's own
/// text. A case with several such steps is still one case.
#[test]
fn replace_in_steps_counts_a_case_with_several_variant_steps_once() {
    let mut two_step_variant = case("Two-step case", vec![
        step("appraisee opens the form."),
        step("appraisee saves the form."),
    ]);
    two_step_variant.steps[0].expected = "It happens.".into();
    let exact_hit = case("Exact case", vec![step("Appraisee opens the form.")]);
    let cases = vec![two_step_variant, exact_hit];
    let ops = parse_ops(&serde_json::json!([
        { "op": "replace_in_steps", "find": "Appraisee", "replace": "Employee" },
    ]))
    .unwrap();
    let (_out, report) = apply(cases, &ops);
    let variant_warnings: Vec<&String> = report
        .warnings
        .iter()
        .filter(|w| w.contains("different capitalisation"))
        .collect();
    assert_eq!(variant_warnings.len(), 1, "{:?}", report.warnings);
    assert!(variant_warnings[0].contains("1 case"), "{:?}", variant_warnings);
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
