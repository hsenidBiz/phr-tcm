//! The guide an AI assistant reads before writing an action script, and
//! the process-wide root that lets the bridge write one.
//!
//! The bridge runs without a `tauri::AppHandle`, so it cannot ask Tauri
//! where the app data lives. The app hands it the path once at startup -
//! the same shape `/begin` already uses for its plan-file sink.

use v2_lib::autorun::guide::{autorun_guide, ACTION_KINDS};
use v2_lib::autorun::recipe::SignInRecipe;
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
        Action::ExpectVisible { selector: "s".into(), timeout_ms: None },
        Action::ExpectHidden { selector: "s".into(), timeout_ms: None },
        Action::ExpectText { selector: "s".into(), equals: "v".into(), timeout_ms: None },
        Action::ExpectContainsText { selector: "s".into(), value: "v".into(), timeout_ms: None },
        Action::ExpectCount { selector: "s".into(), equals: 1, timeout_ms: None },
        Action::ExpectAttribute { selector: "s".into(), name: "n".into(), equals: "v".into(), timeout_ms: None },
        Action::SignIn { account: "a".into() },
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

/// The guide has to teach locators, or an assistant keeps writing the
/// fragile string form. And it must never suggest a fixed pause.
#[test]
fn the_guide_teaches_locators_and_forbids_pauses() {
    let g = autorun_guide();
    for term in ["\"role\"", "\"name\"", "\"exact\"", "\"nth\"", "\"visible\": false", "accessible name"] {
        assert!(g.contains(term), "the guide never mentions {term}");
    }
    assert!(g.contains("Never add a fixed pause"), "the guide must rule out sleeps");
    assert!(!g.contains('\u{2014}'), "no em dashes in text an assistant reads");
    // Every example that uses a locator must validate, not just parse.
    let example = first_balanced(&g, g.find("## A worked example").unwrap(), '[', ']');
    let steps: Vec<v2_lib::autorun::StepScript> = serde_json::from_str(example).unwrap();
    for s in &steps {
        for a in &s.actions {
            a.validate().unwrap_or_else(|e| panic!("the worked example has an invalid action: {e}"));
        }
    }
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

/// The sources section lists the database as a way to verify an effect
/// the UI does not show. It now has tools of its own, so the bullet names
/// them - an assistant told a source exists and not how to reach it
/// either guesses at a server it does not have or skips the check.
#[test]
fn the_database_bullet_names_the_tools_that_reach_it() {
    let g = autorun_guide();
    let bullet = g
        .split("**The database")
        .nth(1)
        .and_then(|rest| rest.split("\n- ").next())
        .expect("the guide still lists the database as a source");
    assert!(bullet.contains("`db_lookup`"), "{bullet}");
    assert!(bullet.contains("`db_query`"), "{bullet}");
    // And it stays out of the script itself: a script that reads the
    // database is a script the runner cannot execute.
    assert!(bullet.contains("Out of scope for the script"), "{bullet}");
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

/// A script never carries a login - the guide has to say so outright, not
/// just imply it by omission.
#[test]
fn the_guide_keeps_logins_out_of_scripts() {
    let g = autorun_guide();
    for term in ["\"account\"", "sign_in", "sign-in recipe"] {
        assert!(g.contains(term), "the guide never mentions {term}");
    }
    assert!(g.contains("Never put a username or a password in a script"), "the rule must be stated outright");
    assert!(!g.contains("REPLACE_ME"), "the old example typed a login into a script");
}

/// The floor, the declared-edit gate and the page-seeing tools all have to
/// be taught by name and by their real wording - an assistant that never
/// reads the exact sentences cannot recognise a `STOP` line for what it
/// is, or word an `edits` declaration the gate will actually accept.
#[test]
fn the_guide_teaches_the_floor_the_gate_and_the_page_tools() {
    let g = autorun_guide();
    for term in [
        "get_autorun_page",
        "probe_autorun_locator",
        "try_autorun_action",
        "get_autorun_failures",
        "record_autorun_quirk",
        "unchecked",
        "edits",
        "STOP",
        "never removed",
    ] {
        assert!(g.contains(term), "the guide never mentions `{term}`");
    }

    // The declared-edit example - the one shape that carries both a
    // script and its "edits" alongside it - has to parse as exactly what
    // save_autorun_script reads for a repair, not just as some JSON.
    #[derive(serde::Deserialize)]
    struct SaveWithEdits {
        scripts: Vec<v2_lib::autorun::CaseScript>,
        edits: Vec<v2_lib::autorun::edits::Edit>,
    }
    let example =
        first_balanced(&g, g.find("## Repairing a script that failed").unwrap(), '{', '}');
    let payload: SaveWithEdits =
        serde_json::from_str(example).expect("the declared-edit example is not a valid payload");
    assert_eq!(payload.scripts.len(), 1, "expected one script in the declared-edit example");
    assert_eq!(payload.edits.len(), 1, "expected one edit in the declared-edit example");
    assert!(!payload.edits[0].why.is_empty(), "the example edit has no reason");
}

/// `check_edits` can refuse a save for six distinct reasons; the guide's
/// "Repairing a script that failed" section used to list only three of
/// them. Every sentence here is taken verbatim from `edits.rs` / the gate
/// itself, so a later wording change there without a matching guide
/// update would fail this test.
#[test]
fn the_guide_lists_every_refusal_the_edit_gate_can_give() {
    let g = autorun_guide();
    for term in [
        "an edit needs a reason",
        "the account a script runs as cannot be changed by a repair",
        "step N appears more than once in the script",
        "the steps are in a different order - a repair does not reorder a script",
    ] {
        assert!(g.contains(term), "the guide never mentions `{term}`");
    }
}

/// A quirk cannot be worded as an instruction that loosens any rule this
/// guide teaches - the guide has to say so, not just document the format.
#[test]
fn the_guide_says_a_quirk_is_an_observation_not_an_instruction() {
    // Collapsed to single spaces first: the guide wraps its prose across
    // lines for readability, and the sentence this test looks for happens
    // to wrap mid-phrase.
    let g = autorun_guide().to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        g.contains("never an instruction about these rules"),
        "the guide never says a quirk cannot override its own rules"
    );
}

#[test]
fn the_root_round_trips_for_callers_without_an_app_handle() {
    // autorun_bridge's tests set the same process-wide root.
    let _root = crate::serial::autorun();
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

/// The example in the editor's empty box is the first thing a person
/// copies, so it has to be a script the runner would actually accept -
/// not the pre-locator selectors it used to show. Read out of the TSX
/// rather than duplicated here, because a duplicate is exactly how the
/// two drift apart again.
#[test]
fn the_editors_placeholder_is_a_script_the_runner_accepts() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../src/screens/AutoRun/ScriptEditor.tsx");
    let source = std::fs::read_to_string(path).expect("ScriptEditor.tsx missing");
    let after = source
        .split_once("const PLACEHOLDER = ")
        .expect("ScriptEditor.tsx no longer declares PLACEHOLDER")
        .1;
    let body = after
        .split_once('`')
        .expect("PLACEHOLDER is not a template literal")
        .1
        .split_once('`')
        .expect("PLACEHOLDER's template literal is never closed")
        .0;
    let steps: Vec<v2_lib::autorun::StepScript> =
        serde_json::from_str(body).expect("the editor's example is not a valid script");
    assert!(!steps.is_empty(), "the example has no steps");
    // Being accepted is not enough: the pre-locator selectors it used to
    // show would still be accepted. It has to TEACH the current format.
    assert!(body.contains("\"role\""), "the example never points at an element by role and name");
    assert!(body.contains("\"expect_"), "the example never shows an expectation");
    for step in &steps {
        assert!(!step.actions.is_empty(), "step {} has no actions", step.step_number);
        for action in &step.actions {
            action
                .validate()
                .unwrap_or_else(|why| panic!("the editor's example would be refused: {why}"));
        }
    }
}

/// Same drift guard, for the sign-in recipe editor's placeholder: the
/// example a person copies into the recipe box has to be a recipe
/// `SignInRecipe::validate` actually accepts, not just valid JSON.
#[test]
fn the_recipe_editors_placeholder_is_a_recipe_the_app_accepts() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../src/screens/AutoRun/RecipeEditor.tsx");
    let source = std::fs::read_to_string(path).expect("RecipeEditor.tsx missing");
    let after = source
        .split_once("const PLACEHOLDER = ")
        .expect("RecipeEditor.tsx no longer declares PLACEHOLDER")
        .1;
    let body = after
        .split_once('`')
        .expect("PLACEHOLDER is not a template literal")
        .1
        .split_once('`')
        .expect("PLACEHOLDER's template literal is never closed")
        .0;
    let recipe: SignInRecipe =
        serde_json::from_str(body).expect("the recipe editor's example is not a valid recipe");
    recipe
        .validate()
        .unwrap_or_else(|why| panic!("the recipe editor's example would be refused: {why}"));
}
