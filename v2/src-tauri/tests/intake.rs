//! The intake that runs before any test case is written: the checklist,
//! the checks on the developer's answers, and the plan file.

use v2_lib::ai_bridge::{route, BridgeContext};
use v2_lib::intake::{job_scale, plan_markdown, plan_path, problems, questions, IntakeAnswers};

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("tcm-intake-{tag}-{nanos}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn good(dir: &std::path::Path, spec: &std::path::Path) -> IntakeAnswers {
    IntakeAnswers {
        output_path: dir.join("cases.json").to_string_lossy().to_string(),
        spec_paths: vec![spec.to_string_lossy().to_string()],
        sections: "3.1-3.4".into(),
        ordering: "tester".into(),
        reference_cases: "none".into(),
        examples_pbi: Some(144714),
        check_examples: true,
        authority: "spec-wins".into(),
        tags: "Team Assessment; regression".into(),
        module: "Performance".into(),
        automation_status: "Not Automated".into(),
        out_of_scope: "The page behind the Go button".into(),
        notes: "Atomic steps, one assertion each".into(),
    }
}

/// A spec document with exactly `sections` top-level markdown headings
/// (no AC markers, so every one of them is in-scope) padded with body
/// lines until it reaches at least `min_lines` lines total.
fn spec_with_sections(
    dir: &std::path::Path,
    name: &str,
    sections: usize,
    min_lines: usize,
) -> std::path::PathBuf {
    let path = dir.join(name);
    let mut text = String::new();
    for i in 1..=sections {
        text.push_str(&format!("## {i} Section {i}\n"));
    }
    while text.lines().count() < min_lines {
        text.push_str("body\n");
    }
    std::fs::write(&path, &text).unwrap();
    path
}

fn assert_scale(v: &serde_json::Value, expected: &str) {
    assert_eq!(v["recommendation"], expected, "got {v}");
    let lines = v["spec_lines"].as_u64().expect("spec_lines");
    let sections = v["sections_in_scope"].as_u64().expect("sections_in_scope");
    let why = v["why"].as_str().expect("why");
    // The exact phrase job_scale formats the numbers into, not a bare
    // substring check - "6".contains would false-pass on a `why` that
    // only names "68" lines, hiding a dropped number inside the other.
    let expected_phrase = format!("{lines} lines and {sections} in-scope sections");
    assert!(
        why.contains(&expected_phrase),
        "why does not name both numbers together: {why}"
    );
}

#[test]
fn the_checklist_leads_with_what_is_expensive_to_get_wrong() {
    let qs = questions();
    let fields: Vec<&str> = qs.iter().map(|q| q.field.as_str()).collect();
    assert!(fields.contains(&"output_path"));
    assert!(fields.contains(&"spec_paths"));
    assert!(fields.contains(&"authority"));

    // The ones that cannot be guessed are the required ones. `ordering`
    // joined them because the two orders are different files and only
    // the developer knows which job this one is for.
    let required: Vec<&str> = qs
        .iter()
        .filter(|q| q.required)
        .map(|q| q.field.as_str())
        .collect();
    assert_eq!(
        required,
        vec!["output_path", "spec_paths", "authority", "ordering", "reference_cases"]
    );
    // Every question explains itself - the assistant is meant to relay
    // the "why", not just the prompt.
    assert!(qs.iter().all(|q| !q.why.trim().is_empty()));
}

#[test]
fn complete_answers_pass() {
    let dir = temp_dir("ok");
    let spec = dir.join("spec.md");
    std::fs::write(&spec, "# spec").unwrap();
    assert_eq!(
        problems(&good(&dir, &spec), &["Performance".into()]),
        Vec::<String>::new()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The check that replaces a file picker: a path the assistant invented
/// is caught by name rather than discovered at the end of the job.
#[test]
fn a_spec_path_that_does_not_exist_is_named() {
    let dir = temp_dir("missing");
    let spec = dir.join("spec.md");
    std::fs::write(&spec, "# spec").unwrap();
    let mut a = good(&dir, &spec);
    a.spec_paths.push("Z:/nowhere/invented-spec.docx".into());

    let found = problems(&a, &[]);
    assert!(
        found.iter().any(|p| p.contains("invented-spec.docx")),
        "got {found:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_output_folder_that_does_not_exist_is_refused_not_created() {
    let dir = temp_dir("outdir");
    let spec = dir.join("spec.md");
    std::fs::write(&spec, "# spec").unwrap();
    let mut a = good(&dir, &spec);
    let ghost = dir.join("no-such-folder");
    a.output_path = ghost.join("cases.json").to_string_lossy().to_string();

    let found = problems(&a, &[]);
    assert!(
        found.iter().any(|p| p.contains("does not exist")),
        "got {found:?}"
    );
    assert!(
        !ghost.exists(),
        "the folder must not be created behind the developer's back"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_three_required_answers_are_each_reported_when_missing() {
    let found = problems(&IntakeAnswers::default(), &[]);
    assert!(found.iter().any(|p| p.contains("output_path is required")));
    assert!(found.iter().any(|p| p.contains("spec_paths is required")));
    assert!(found.iter().any(|p| p.contains("authority is required")));
}

#[test]
fn values_outside_their_allowed_sets_are_refused() {
    let dir = temp_dir("values");
    let spec = dir.join("spec.md");
    std::fs::write(&spec, "# spec").unwrap();

    let mut a = good(&dir, &spec);
    a.authority = "whatever".into();
    a.automation_status = "Automated".into();
    a.module = "Not A Real Module".into();
    let found = problems(&a, &["Performance".into(), "Payments".into()]);

    assert!(found.iter().any(|p| p.contains("authority 'whatever'")));
    assert!(found
        .iter()
        .any(|p| p.contains("automation_status 'Automated'")));
    assert!(found.iter().any(|p| p.contains("is not an allowed value")));

    // With no picklist available, module is not second-guessed.
    let mut b = good(&dir, &spec);
    b.module = "Anything".into();
    assert!(!problems(&b, &[])
        .iter()
        .any(|p| p.contains("allowed value")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn asking_for_a_duplicate_check_without_a_pbi_is_caught() {
    let dir = temp_dir("pbi");
    let spec = dir.join("spec.md");
    std::fs::write(&spec, "# spec").unwrap();
    let mut a = good(&dir, &spec);
    a.examples_pbi = None;
    assert!(problems(&a, &[])
        .iter()
        .any(|p| p.contains("no examples_pbi")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_plan_records_every_decision_in_the_developers_words() {
    let dir = temp_dir("plan");
    let spec = dir.join("spec.md");
    std::fs::write(&spec, "# spec").unwrap();
    let plan = plan_markdown(&good(&dir, &spec), "Manager Assessment landing page");

    assert!(plan.contains("Manager Assessment landing page"));
    assert!(plan.contains("spec.md"));
    assert!(plan.contains("3.1-3.4"));
    assert!(plan.contains("specification wins"));
    assert!(plan.contains("PBI #144714"));
    assert!(plan.contains("- The page behind the Go button"));
    assert!(plan.contains("Atomic steps"));
    // And it tells the assistant what to do before handing over.
    assert!(plan.contains("optimize_cases"));
    assert!(plan.contains("validate_cases"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// `check_spec_coverage` has to run BEFORE `optimize_cases` in the "Before
/// handing the file over" checklist - round-5 §4 says the check belongs
/// "while the draft is still in spec order", which is before optimizing it,
/// not after.
#[test]
fn the_plan_runs_coverage_before_optimize() {
    let dir = temp_dir("plan-order");
    let spec = dir.join("spec.md");
    std::fs::write(&spec, "# spec").unwrap();
    let plan = plan_markdown(&good(&dir, &spec), "Manager Assessment landing page");

    assert!(plan.contains("check_spec_coverage"), "{plan}");
    let coverage_at = plan.find("check_spec_coverage").expect("check_spec_coverage still in the plan");
    let optimize_at = plan.find("optimize_cases").expect("optimize_cases still in the plan");
    assert!(
        coverage_at < optimize_at,
        "check_spec_coverage has to precede optimize_cases in the plan: {plan}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_plan_sits_beside_the_output_file() {
    assert_eq!(
        plan_path("C:/work/login-cases.json"),
        "C:/work/login-cases-plan.md"
    );
    assert_eq!(plan_path("cases.json"), "cases-plan.md");
}

// ------------------------------------------------------------ job_scale

/// Section count dominates: lines only ever tip a recommendation upward,
/// never sideways or down, and never on their own past "choose".
#[test]
fn scale_thresholds_let_section_count_dominate() {
    let dir = temp_dir("scale-dominate");

    // 5 sections / 3000 lines -> "choose" (lines alone cannot reach fan-out).
    let five = spec_with_sections(&dir, "five.md", 5, 3000);
    let v = job_scale(&[five.to_string_lossy().to_string()], "").expect("readable");
    assert_scale(&v, "choose");

    // 20 sections / 400 lines -> "fan-out" (sections dominate over lines).
    let twenty = spec_with_sections(&dir, "twenty.md", 20, 400);
    let v = job_scale(&[twenty.to_string_lossy().to_string()], "").expect("readable");
    assert_scale(&v, "fan-out");

    // 6 sections / 500 lines -> "single-pass" (small on both axes).
    let six = spec_with_sections(&dir, "six.md", 6, 500);
    let v = job_scale(&[six.to_string_lossy().to_string()], "").expect("readable");
    assert_scale(&v, "single-pass");

    let _ = std::fs::remove_dir_all(&dir);
}

/// The section-count boundaries exactly as the plan's Global Constraints
/// state them: <8 single-pass, 8-15 choose, >15 fan-out.
#[test]
fn scale_thresholds_boundary_section_counts() {
    let dir = temp_dir("scale-boundary");

    let seven = spec_with_sections(&dir, "seven.md", 7, 200);
    assert_scale(
        &job_scale(&[seven.to_string_lossy().to_string()], "").unwrap(),
        "single-pass",
    );

    let eight = spec_with_sections(&dir, "eight.md", 8, 200);
    assert_scale(
        &job_scale(&[eight.to_string_lossy().to_string()], "").unwrap(),
        "choose",
    );

    let fifteen = spec_with_sections(&dir, "fifteen.md", 15, 200);
    assert_scale(
        &job_scale(&[fifteen.to_string_lossy().to_string()], "").unwrap(),
        "choose",
    );

    let sixteen = spec_with_sections(&dir, "sixteen.md", 16, 200);
    assert_scale(
        &job_scale(&[sixteen.to_string_lossy().to_string()], "").unwrap(),
        "fan-out",
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The >1500-line upgrade only ever fires on the "single-pass" branch -
/// a document already at "fan-out" on section count alone must never be
/// capped back down to "choose" because it also happens to be long.
#[test]
fn a_large_doc_with_many_sections_is_fan_out_not_capped_at_choose() {
    let dir = temp_dir("scale-large-fanout");
    // 20 top-level headings spread across roughly 80 filler lines each -
    // comfortably over both the section and the line threshold at once.
    let spec = spec_with_sections(&dir, "large.md", 20, 20 + 20 * 80);
    let v = job_scale(&[spec.to_string_lossy().to_string()], "").expect("readable");
    assert!(v["spec_lines"].as_u64().unwrap() > 1500, "got {v}");
    assert!(v["sections_in_scope"].as_u64().unwrap() > 15, "got {v}");
    assert_scale(&v, "fan-out");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Multi-file sizing sums across every readable spec, not just the first
/// one named.
#[test]
fn two_readable_specs_sum_their_lines_and_sections() {
    let dir = temp_dir("scale-sum");
    let a = spec_with_sections(&dir, "a.md", 5, 100);
    let b = spec_with_sections(&dir, "b.md", 4, 50);
    let v = job_scale(
        &[a.to_string_lossy().to_string(), b.to_string_lossy().to_string()],
        "",
    )
    .expect("both readable");
    assert_eq!(v["spec_lines"], serde_json::json!(150), "got {v}");
    assert_eq!(v["sections_in_scope"], serde_json::json!(9), "got {v}");
    assert_scale(&v, "choose");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Lines only tip a recommendation upward, and only from single-pass to
/// choose - they must never downgrade an already-higher recommendation and
/// must never push a recommendation past "choose" on their own.
#[test]
fn scale_lines_only_tip_upward_and_never_past_choose() {
    let dir = temp_dir("scale-lines");

    // 7 sections at exactly 1500 lines: no upgrade yet.
    let at_threshold = spec_with_sections(&dir, "at.md", 7, 1500);
    assert_scale(
        &job_scale(&[at_threshold.to_string_lossy().to_string()], "").unwrap(),
        "single-pass",
    );

    // 7 sections just over 1500 lines: upgraded to "choose".
    let over_threshold = spec_with_sections(&dir, "over.md", 7, 1501);
    assert_scale(
        &job_scale(&[over_threshold.to_string_lossy().to_string()], "").unwrap(),
        "choose",
    );

    // A "fan-out" job (20 sections) with very few lines stays "fan-out" -
    // lines never downgrade.
    let many_sections = spec_with_sections(&dir, "many.md", 20, 30);
    assert_scale(
        &job_scale(&[many_sections.to_string_lossy().to_string()], "").unwrap(),
        "fan-out",
    );

    // A "choose" job (10 sections) with a huge line count stays "choose" -
    // lines never push past it on their own.
    let mid_sections = spec_with_sections(&dir, "mid.md", 10, 5000);
    assert_scale(
        &job_scale(&[mid_sections.to_string_lossy().to_string()], "").unwrap(),
        "choose",
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// `sections_in_scope` is Task 4's own top-level count - it must not count
/// AC-marker child sections (ids like "8.2 (AC-1)"), unlike
/// `check_coverage`'s `sections_in_document`.
#[test]
fn scale_counts_top_level_sections_only_not_ac_children() {
    let dir = temp_dir("scale-ac");
    let path = dir.join("acs.md");
    // 3 top-level headings, each with two AC markers underneath - 9
    // `Inventory.sections` entries in all, but only 3 are top-level.
    std::fs::write(
        &path,
        "## 1 First\nAC-1: a\nAC-2: b\n## 2 Second\nAC-1: a\nAC-2: b\n## 3 Third\nAC-1: a\nAC-2: b\n",
    )
    .unwrap();

    let v = job_scale(&[path.to_string_lossy().to_string()], "").expect("readable");
    assert_eq!(v["sections_in_scope"], serde_json::json!(3), "got {v}");
    assert_scale(&v, "single-pass");

    let _ = std::fs::remove_dir_all(&dir);
}

/// The `sections` answer is an enumerated scope filter, same rule as
/// `check_coverage`: every comma/semicolon token must look like a section
/// id or the whole thing is read as free text that restricts nothing.
#[test]
fn scale_applies_the_same_enumerated_scope_rule_as_speccov() {
    let dir = temp_dir("scale-scope");
    let path = spec_with_sections(&dir, "spec.md", 20, 100);

    // An enumerated list restricts the count to what was actually named.
    let scoped = job_scale(&[path.to_string_lossy().to_string()], "1,2,3").expect("readable");
    assert_eq!(scoped["sections_in_scope"], serde_json::json!(3), "got {scoped}");

    // Free text restricts nothing - all 20 still count.
    let free = job_scale(
        &[path.to_string_lossy().to_string()],
        "everything except the appendix",
    )
    .expect("readable");
    assert_eq!(free["sections_in_scope"], serde_json::json!(20), "got {free}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// Sizing is advice, never a hard requirement: a spec path that cannot be
/// read as text at all (here, a folder - `spec_paths` allows those, and
/// `problems()` only checks that the path exists) sizes nothing, but a
/// path that IS readable still gets sized even if a sibling entry fails.
#[test]
fn scale_is_none_only_when_nothing_could_be_read_and_sizes_what_it_can_otherwise() {
    let dir = temp_dir("scale-unreadable");
    let folder = dir.join("not-a-file");
    std::fs::create_dir_all(&folder).unwrap();

    // Nothing readable at all -> None, never a panic or an error value.
    assert!(job_scale(&[folder.to_string_lossy().to_string()], "").is_none());

    // One unreadable entry alongside one readable one -> still sizes the
    // readable one rather than giving up entirely.
    let readable = spec_with_sections(&dir, "spec.md", 3, 50);
    let v = job_scale(
        &[folder.to_string_lossy().to_string(), readable.to_string_lossy().to_string()],
        "",
    )
    .expect("the readable entry still sizes the job");
    assert_eq!(v["sections_in_scope"], serde_json::json!(3), "got {v}");

    let _ = std::fs::remove_dir_all(&dir);
}

// ------------------------------------------------------------- routes

fn ctx() -> BridgeContext {
    BridgeContext {
        org: "acme".into(),
        project: "Web".into(),
        module_ref: None,
        preconditions_ref: None,
        disabled_tools: vec![],
        working_dir: None,
    }
}

/// Phase 1: no answers sent, so the questions come back - and the
/// assistant is told in as many words not to answer them itself.
#[tokio::test]
async fn phase_one_returns_the_questions_and_the_apps_context() {
    let (status, body) = route(&ctx(), None, "POST", "/begin?feature=Login", "", "test").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();

    assert_eq!(v["status"], "questions");
    assert_eq!(v["feature"], "Login");
    assert!(v["ask_the_developer"].as_array().unwrap().len() >= 8);
    assert_eq!(v["context"]["organization"], "acme");
    let note = v["note"].as_str().unwrap();
    assert!(note.contains("Do NOT answer them yourself"), "got: {note}");
}

/// Phase 2 with bad answers: problems come back, no plan is written, and
/// guessing past them is explicitly ruled out.
#[tokio::test]
async fn phase_two_refuses_answers_that_do_not_check_out() {
    let body_in = serde_json::json!({
        "output_path": "Z:/nowhere/at/all/cases.json",
        "spec_paths": ["Z:/nowhere/spec.md"],
        "authority": "spec",
    })
    .to_string();
    let (status, body) = route(&ctx(), None, "POST", "/begin", &body_in, "test").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();

    assert_eq!(v["status"], "needs_answers");
    assert!(v["problems"].as_array().unwrap().len() >= 2);
    assert!(v["note"].as_str().unwrap().contains("do not guess"));
    assert!(v.get("plan").is_none(), "no plan until the answers are sound");
}

/// Phase 2 with sound answers: the plan is written next to the output.
#[tokio::test]
async fn phase_two_writes_the_plan_when_the_answers_are_sound() {
    let dir = temp_dir("route");
    let spec = dir.join("spec.md");
    std::fs::write(&spec, "# spec").unwrap();
    let out = dir.join("cases.json");

    let body_in = serde_json::json!({
        "output_path": out.to_string_lossy(),
        "spec_paths": [spec.to_string_lossy()],
        "authority": "spec-wins",
        "ordering": "tester",
        "reference_cases": "none",
    })
    .to_string();
    let (status, body) =
        route(&ctx(), None, "POST", "/begin?feature=Orders", &body_in, "test").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();

    assert_eq!(v["status"], "ready");
    let written = v["plan_path"].as_str().expect("plan path");
    assert!(
        std::path::Path::new(written).is_file(),
        "plan written to disk"
    );
    assert!(std::fs::read_to_string(written).unwrap().contains("Orders"));
    // The note spells out the review gate in so many words: the developer
    // checks the plan and either names changes or says to go ahead - and
    // nothing is written until they answer.
    let note = v["note"].as_str().unwrap();
    assert!(note.contains("check the plan"), "{note}");
    assert!(note.contains("go ahead"), "{note}");
    assert!(note.contains("until they answer"), "{note}");
    let _ = std::fs::remove_dir_all(&dir);
}


/// Which order the set is FOR is the developer's call, and nothing in
/// the cases reveals it - so it is asked, and asked as a required
/// question rather than guessed at.
#[test]
fn ordering_is_asked_and_checked() {
    let asked = questions();
    let q = asked.iter().find(|q| q.field == "ordering").expect("ordering is asked");
    assert!(q.required, "an unanswered ordering would be silently guessed");
    assert!(q.ask.contains("spec") && q.ask.contains("tester"), "{}", q.ask);

    let dir = temp_dir("ordering");
    let spec = dir.join("spec.md");
    std::fs::write(&spec, "x").unwrap();

    let mut a = good(&dir, &spec);
    a.ordering = "sideways".into();
    assert!(
        problems(&a, &[]).iter().any(|p| p.contains("ordering")),
        "an unknown ordering has to be refused"
    );

    a.ordering = String::new();
    assert!(
        problems(&a, &[]).iter().any(|p| p.contains("ordering is required")),
        "a missing ordering has to be asked for"
    );

    // And the answer has to reach the plan, or asking it changed nothing.
    a.ordering = "spec".into();
    let plan = plan_markdown(&a, "Feature");
    assert!(plan.contains("document order"), "{plan}");
    assert!(plan.contains("reorder=false"), "the plan must tell it not to regroup: {plan}");

    a.ordering = "tester".into();
    let plan = plan_markdown(&a, "Feature");
    assert!(!plan.contains("reorder=false"), "{plan}");
    assert!(plan.contains("changes environment as little as possible"), "{plan}");
}

/// A ready intake carries the sizing advice alongside the plan: a 16-heading
/// spec is squarely "fan-out", and the `why` names the section count so the
/// assistant isn't just told a label without the evidence behind it.
#[tokio::test]
async fn a_ready_intake_carries_the_scale_block() {
    let dir = temp_dir("scale-route");
    let spec = spec_with_sections(&dir, "spec.md", 16, 50);
    let out = dir.join("cases.json");

    let body_in = serde_json::json!({
        "output_path": out.to_string_lossy(),
        "spec_paths": [spec.to_string_lossy()],
        "authority": "spec",
        "ordering": "tester",
        "reference_cases": "none",
    })
    .to_string();
    let (status, body) = route(&ctx(), None, "POST", "/begin?feature=Big", &body_in, "test").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();

    assert_eq!(v["status"], "ready");
    assert_eq!(v["scale"]["recommendation"], "fan-out", "got {v}");
    let why = v["scale"]["why"].as_str().expect("why");
    assert!(why.contains("16"), "why should name the section count: {why}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Sizing is advice, never a gate: an otherwise-ready intake whose only
/// `spec_paths` entry cannot be read as text (a folder - `problems()` only
/// checks that the path exists, and the question text explicitly allows
/// "a folder works too") still reaches "ready" with no plan-blocking error;
/// it just carries no `scale`.
#[tokio::test]
async fn scale_is_absent_not_an_error_when_no_spec_file_is_readable() {
    let dir = temp_dir("scale-route-none");
    let folder = dir.join("specs");
    std::fs::create_dir_all(&folder).unwrap();
    let out = dir.join("cases.json");

    let body_in = serde_json::json!({
        "output_path": out.to_string_lossy(),
        "spec_paths": [folder.to_string_lossy()],
        "authority": "spec",
        "ordering": "tester",
        "reference_cases": "none",
    })
    .to_string();
    let (status, body) = route(&ctx(), None, "POST", "/begin?feature=NoSpec", &body_in, "test").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();

    assert_eq!(v["status"], "ready", "sizing must never block an otherwise-ready intake: {v}");
    assert!(
        v.get("scale").is_none_or(|s| s.is_null()),
        "scale should be absent/null when nothing was readable: {v}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
