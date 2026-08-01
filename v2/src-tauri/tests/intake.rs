//! The intake that runs before any test case is written: the checklist,
//! the checks on the developer's answers, and the plan file.

use v2_lib::ai_bridge::{route, BridgeContext};
use v2_lib::intake::{plan_markdown, plan_path, problems, questions, IntakeAnswers};

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
    assert_eq!(required, vec!["output_path", "spec_paths", "authority", "ordering"]);
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

#[test]
fn the_plan_sits_beside_the_output_file() {
    assert_eq!(
        plan_path("C:/work/login-cases.json"),
        "C:/work/login-cases-plan.md"
    );
    assert_eq!(plan_path("cases.json"), "cases-plan.md");
}

// ------------------------------------------------------------- routes

fn ctx() -> BridgeContext {
    BridgeContext {
        org: "acme".into(),
        project: "Web".into(),
        module_ref: None,
        preconditions_ref: None,
        disabled_tools: vec![],
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
    assert!(v["note"].as_str().unwrap().contains("get their agreement"));
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
