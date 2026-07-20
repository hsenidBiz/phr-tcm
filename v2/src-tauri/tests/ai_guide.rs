//! Relocated from in-file `#[cfg(test)]` blocks in `src/ai_guide.rs` and
//! `src/commands/ai_guide.rs` — `[lib] test = false` means a lib unit-test
//! binary never runs (it would miss the Common-Controls v6 manifest from
//! build.rs and crash at startup on Windows), so all tests live here as an
//! integration target that plain `cargo test` actually runs.
//!
//! `worked_example_round_trips_through_the_importer` is THE drift gate: the
//! guide's own worked example must parse through the real importer. If the
//! import format changes without updating the guide (or vice versa), this
//! fails the build.

use v2_lib::ai_guide::{build_guide_body, flavor_files, GuideFlavor, GuideOptions};
use v2_lib::commands::ai_guide::{preview_ai_guide, write_ai_guide};

fn opts() -> GuideOptions {
    GuideOptions {
        organization: "acme".into(),
        project: "Web".into(),
        area: Some("Web\\Gamma Guardians".into()),
        modules: vec!["Login".into(), "Checkout".into()],
        tags: vec!["smoke".into(), "regression".into()],
        modules_discovered: true,
        doc_paths: vec!["docs/screens/**".into(), "README.md".into()],
        conventions: "Tag UI cases with 'ui'.".into(),
        flavors: vec![GuideFlavor::Generic],
        generated_on: "2026-07-20".into(),
    }
}

fn command_opts(flavors: Vec<GuideFlavor>) -> GuideOptions {
    GuideOptions {
        organization: "acme".into(),
        project: "Web".into(),
        area: None,
        modules: vec!["Login".into()],
        tags: vec![],
        modules_discovered: true,
        doc_paths: vec![],
        conventions: String::new(),
        flavors,
        generated_on: "2026-07-20".into(),
    }
}

#[test]
fn body_contains_fixed_sections_and_custom_values() {
    let body = build_guide_body(&opts());
    // Fixed layer
    assert!(body.contains("Test Case Manager"));
    assert!(body.contains("Import File")); // workflow section
    assert!(body.contains("```json")); // worked example fence
    assert!(body.contains("\"Not Automated\"")); // statuses from VALID_STATUSES
    assert!(body.contains("\"Planned\""));
    assert!(body.contains("semicolon")); // tag separator rule
    // Custom layer
    assert!(body.contains("Login") && body.contains("Checkout"));
    assert!(body.contains("smoke"));
    assert!(body.contains("docs/screens/**"));
    assert!(body.contains("Tag UI cases with 'ui'."));
    assert!(body.contains("2026-07-20")); // snapshot stamp
    assert!(body.contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn empty_custom_inputs_omit_their_sections() {
    let mut o = opts();
    o.doc_paths.clear();
    o.conventions = String::new();
    o.modules.clear();
    o.modules_discovered = false;
    o.tags.clear();
    let body = build_guide_body(&o);
    assert!(!body.contains("## Repository documentation"));
    assert!(!body.contains("## Team conventions"));
    // Discovery failure => visible degradation note instead of a list
    assert!(body.contains("could not be discovered"));
}

#[test]
fn body_is_deterministic() {
    assert_eq!(build_guide_body(&opts()), build_guide_body(&opts()));
}

/// THE drift gate: the guide's own worked example must parse through
/// the real importer. If the import format changes without updating
/// the guide (or vice versa), this fails the build.
#[test]
fn worked_example_round_trips_through_the_importer() {
    let dir = std::env::temp_dir().join("tcm_ai_guide_test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("example-{}.json", std::process::id()));
    std::fs::write(&path, v2_lib::ai_guide::WORKED_EXAMPLE_JSON).unwrap();

    let (cases, warnings) =
        v2_lib::import_parser::parse_file(path.to_str().unwrap()).unwrap();

    assert_eq!(warnings, Vec::<String>::new(), "example must import warning-free");
    assert_eq!(cases.len(), 2, "guide's worked example must contain exactly 2 cases");

    // Create vs update semantics
    assert_eq!(cases[0].update_id, None, "case 0 has no `id` in the guide's example - it must round-trip as a CREATE");
    assert_eq!(
        cases[1].update_id,
        Some(143001),
        "case 1's `id` must round-trip as an UPDATE of that exact work item"
    );

    // Case 0 ("Login") - every field the guide's worked example advertises
    // must survive the trip through the real importer.
    assert_eq!(
        cases[0].title, "Login - valid credentials reach the dashboard",
        "case 0 title drifted from the guide's worked example"
    );
    assert_eq!(cases[0].steps.len(), 2, "case 0 must keep both of its steps");
    assert_eq!(
        cases[0].steps[0].action, "Open the sign-in page",
        "case 0 step 1 action drifted from the guide's worked example"
    );
    assert_eq!(
        cases[0].steps[0].expected, "Sign-in form is shown",
        "case 0 step 1 expected drifted from the guide's worked example"
    );
    assert_eq!(
        cases[0].steps[1].action,
        "Enter a valid username and password and submit",
        "case 0 step 2 action drifted from the guide's worked example"
    );
    assert_eq!(
        cases[0].steps[1].expected,
        "The dashboard loads and shows the signed-in user's name",
        "case 0 step 2 expected drifted from the guide's worked example"
    );
    assert_eq!(
        cases[0].tags, "smoke; login",
        "case 0 tags must round-trip as the semicolon-separated string the guide shows"
    );
    assert_eq!(
        cases[0].automation_status, "Not Automated",
        "case 0 automation_status drifted from the guide's worked example"
    );
    assert_eq!(
        cases[0].module_value, "Login",
        "case 0 module_value drifted from the guide's worked example"
    );
    assert_eq!(
        cases[0].preconditions, "A test account exists",
        "case 0 preconditions drifted from the guide's worked example - preconditions has no warning path in the importer, so this assertion is the only thing that would catch silent drift"
    );

    // Case 1 ("Checkout") - same coverage, including the update path.
    assert_eq!(
        cases[1].title, "Checkout - expired card is rejected with a clear error",
        "case 1 title drifted from the guide's worked example"
    );
    assert_eq!(cases[1].steps.len(), 1, "case 1 must keep its single step");
    assert_eq!(
        cases[1].steps[0].action, "Pay with an expired card",
        "case 1 step 1 action drifted from the guide's worked example"
    );
    assert_eq!(
        cases[1].steps[0].expected,
        "An 'expired card' error is shown; no order is created",
        "case 1 step 1 expected drifted from the guide's worked example"
    );
    assert_eq!(
        cases[1].tags, "regression; checkout",
        "case 1 tags must round-trip as the semicolon-separated string the guide shows"
    );
    assert_eq!(
        cases[1].automation_status, "Planned",
        "case 1 automation_status drifted from the guide's worked example"
    );
    assert_eq!(
        cases[1].module_value, "Checkout",
        "case 1 module_value drifted from the guide's worked example"
    );
    assert_eq!(
        cases[1].preconditions, "",
        "case 1 preconditions must round-trip as empty, matching the guide's worked example"
    );

    // Both must be submit-ready
    assert!(
        cases[0].is_valid().is_ok() && cases[1].is_valid().is_ok(),
        "both cases from the guide's worked example must pass TestCase::is_valid()"
    );

    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir(&dir);
}

#[test]
fn flavor_files_wrap_one_body_per_selected_flavor() {
    let body = "# body\ncontent";
    let all = [
        GuideFlavor::Generic,
        GuideFlavor::ClaudeSkill,
        GuideFlavor::CursorRules,
        GuideFlavor::AgentsSnippet,
    ];
    let files = flavor_files(body, &all);
    let paths: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(
        paths,
        vec![
            "AI_TEST_CASES.md",
            ".claude/skills/generate-test-cases/SKILL.md",
            ".cursor/rules/test-cases.mdc",
            "AGENTS-test-cases.md",
        ]
    );
    // Generic is the body verbatim; wrappers contain the body unchanged.
    assert_eq!(files[0].1, body);
    assert!(files[1].1.starts_with("---\nname: generate-test-cases\n"));
    assert!(
        files.iter().all(|(_, c)| c.ends_with(body)),
        "every flavor must embed the guide body verbatim and unmodified"
    );
    // Cursor frontmatter + Agents append note
    assert!(files[2].1.starts_with("---\ndescription:"));
    assert!(files[3].1.contains("append"));
}

#[test]
fn no_flavors_yields_no_files() {
    assert!(flavor_files("x", &[]).is_empty());
}

#[test]
fn write_creates_nested_flavor_files() {
    let dir = std::env::temp_dir().join(format!("tcm_ai_guide_write_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let written = write_ai_guide(
        dir.to_str().unwrap().into(),
        command_opts(vec![GuideFlavor::Generic, GuideFlavor::ClaudeSkill]),
    )
    .unwrap();

    assert_eq!(
        written,
        vec!["AI_TEST_CASES.md", ".claude/skills/generate-test-cases/SKILL.md"]
    );
    let skill = std::fs::read_to_string(
        dir.join(".claude/skills/generate-test-cases/SKILL.md"),
    )
    .unwrap();
    assert!(skill.contains("acme/Web"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn write_with_no_flavors_errors() {
    let dir = std::env::temp_dir().join(format!("tcm_ai_guide_no_flavors_{}", std::process::id()));
    assert!(write_ai_guide(dir.to_str().unwrap().into(), command_opts(vec![])).is_err());
}

#[test]
fn preview_returns_the_generic_body() {
    assert!(preview_ai_guide(command_opts(vec![])).contains("# AI guide"));
}
