//! The guide an AI assistant reads before writing an action script, and
//! the process-wide root that lets the bridge write one.
//!
//! The bridge runs without a `tauri::AppHandle`, so it cannot ask Tauri
//! where the app data lives. The app hands it the path once at startup -
//! the same shape `/begin` already uses for its plan-file sink.

use v2_lib::autorun::guide::{autorun_guide, ACTION_KINDS};
use v2_lib::autorun::store::{configured_root, set_root};
use v2_lib::browser::actions::Action;

/// Drift gate. An action the executor understands but the guide never
/// mentions is one the assistant will never write; an action the guide
/// advertises but serde does not produce is one it will write and the
/// runner will reject. Both directions are checked here, so adding a
/// variant to `Action` without touching the guide fails.
#[test]
fn the_guide_names_every_action_the_executor_can_run() {
    let g = autorun_guide();
    for kind in ACTION_KINDS {
        assert!(g.contains(kind), "the guide never mentions `{kind}`");
    }

    // And the names are the ones serde actually emits, not a wish list.
    let samples = vec![
        Action::Navigate { url: "u".into() },
        Action::Click { selector: "s".into() },
        Action::Fill { selector: "s".into(), value: "v".into() },
        Action::WaitFor { selector: "s".into(), timeout_ms: 1 },
        Action::CheckText { value: "v".into() },
        Action::CheckUrl { contains: "c".into() },
    ];
    let emitted: Vec<String> = samples
        .iter()
        .map(|a| serde_json::to_value(a).unwrap()["kind"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        emitted,
        ACTION_KINDS.to_vec(),
        "ACTION_KINDS has drifted from what serde emits"
    );
}

/// The assistant can read code, so the guide has to say what code is and
/// is not allowed to decide. Reading an implementation to learn WHERE a
/// button is is fine; reading it to learn what SHOULD happen turns the
/// script into a mirror of the bug it was meant to catch.
#[test]
fn the_guide_says_where_assertions_may_come_from() {
    let g = autorun_guide().to_lowercase();
    assert!(g.contains("selector"), "no selector guidance");
    assert!(g.contains("text="), "the text= selector form is undocumented");
    assert!(
        g.contains("expected result"),
        "the guide must point assertions at the case's expected result"
    );
    assert!(
        g.contains("implementation") || g.contains("source"),
        "the guide must warn about deriving assertions from the code"
    );
}

/// Extracts the first balanced `open`/`close` run starting at `from`,
/// tracking depth rather than jumping to the string's last matching
/// character - the guide carries more than one JSON example, and `find`
/// paired with `rfind` would swallow everything between the first and the
/// last regardless of what sits in between.
fn first_balanced(text: &str, from: usize, open: char, close: char) -> &str {
    let rel_start = text[from..].find(open).expect("no opening bracket found");
    let start = from + rel_start;
    let mut depth = 0i32;
    for (i, c) in text[start..].char_indices() {
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return &text[start..start + i + c.len_utf8()];
            }
        }
    }
    panic!("bracket never closes");
}

/// A worked example is what an assistant copies, so it has to be valid -
/// parseable as the very steps the runner executes.
#[test]
fn the_guides_worked_example_parses_as_real_steps() {
    let g = autorun_guide();
    let example = first_balanced(&g, g.find("## A worked example").unwrap(), '[', ']');
    let steps: Vec<v2_lib::autorun::StepScript> =
        serde_json::from_str(example).expect("the example is not a valid script");
    assert!(!steps.is_empty(), "the example has no steps");
    assert!(
        steps.iter().any(|s| !s.actions.is_empty()),
        "the example has no actions"
    );
}

/// The saving section's payload is the one shape an assistant actually
/// has to send `save_autorun_script` - a list of scripts, each carrying
/// `case_id` and `title` alongside its steps, not the bare steps array
/// from the worked example above. If this drifts from what the command
/// deserialises, the guide would be teaching the wrong shape.
#[test]
fn the_saving_example_parses_as_a_real_save_payload() {
    #[derive(serde::Deserialize)]
    struct Payload {
        scripts: Vec<v2_lib::autorun::CaseScript>,
    }
    let g = autorun_guide();
    let example = first_balanced(&g, g.find("## Saving it").unwrap(), '{', '}');
    let payload: Payload =
        serde_json::from_str(example).expect("the save example is not a valid payload");
    assert_eq!(payload.scripts.len(), 1, "expected one script in the example");
    assert!(!payload.scripts[0].title.is_empty(), "the example script has no title");
    assert!(!payload.scripts[0].steps.is_empty(), "the example script has no steps");
}

/// The tool takes `{ case_id, title, steps }` per entry, but a reader
/// could easily come away thinking it takes the bare steps array (that is
/// literally what the worked example above shows). The guide has to name
/// the actual field names somewhere, or an assistant following it
/// literally sends the wrong shape.
#[test]
fn the_guide_names_the_save_payloads_fields() {
    let g = autorun_guide();
    for term in ["case_id", "title", "scripts"] {
        assert!(g.contains(term), "the guide never mentions `{term}`");
    }
    assert!(
        g.to_lowercase().contains("timeout_ms"),
        "the guide never calls out timeout_ms as required"
    );
}

#[test]
fn the_root_round_trips_for_callers_without_an_app_handle() {
    let dir = std::env::temp_dir().join("tcm-autorun-guide-test");
    set_root(dir.clone());
    assert_eq!(configured_root(), Some(dir));
}

/// The sample lives twice on purpose: `claudedocs/` is a scratch/report
/// directory (see the repo's CLAUDE.md) that this test must not depend
/// on - someone tidying it up should not break the build - so a copy is
/// kept under `tests/fixtures/` as the one this test actually reads. The
/// `claudedocs/` copy stays for a person to open by hand.
///
/// No exact-count assertion: this pins the fixture parsing as a valid
/// bundle of real scripts, not a specific catalogue size that would break
/// every time a sample is added or removed.
#[test]
fn the_shipped_sample_bundle_parses_as_real_scripts() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/autorun-sample-scripts-pms.json"
    );
    let s = std::fs::read_to_string(path).expect("sample bundle missing");
    let v: Vec<v2_lib::autorun::CaseScript> =
        serde_json::from_str(&s).expect("the shipped sample bundle does not parse");
    assert!(!v.is_empty(), "expected at least one sample");
    assert!(v.iter().all(|c| !c.steps.is_empty()), "a sample has no steps");
}
