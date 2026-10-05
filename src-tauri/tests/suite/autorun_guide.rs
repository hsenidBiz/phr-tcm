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
        Action::Upload { selector: "s".into(), file: "f.pdf".into() },
        Action::ExpectResponse { method: None, url_contains: "/x".into(), status: 200, json: None, timeout_ms: None, stray: Default::default() },
        Action::ApiRequest { path: "/api/x".into(), query: Default::default(), expect: Default::default(), stray: Default::default() },
        Action::WhenVisible { selector: "s".into(), within_ms: None, then: vec![Action::Click { selector: "s".into() }] },
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

/// The two API checks have limits an assistant cannot guess: what each one
/// can look at, what it assumes when a field is left out, and where its
/// facts may come from. The guide must say all of it, and must send the
/// assistant to the proven API templates as REFERENCE only.
#[test]
fn the_guide_teaches_the_api_checks_and_their_limits() {
    let g = autorun_guide();
    assert!(g.contains("## Checking the API"), "the guide has no API section");
    let section = g.split_once("## Checking the API").unwrap().1;
    let section = section.split("\n## ").next().unwrap();
    for term in [
        "expect_response",
        "api_request",
        "GET only",
        "since the step began",
        "url_contains",
        "/PerformanceCycle/Save",
        "never a full address",
        "status",
        "200",
        "only the fields you list",
        "list_api_templates",
        "reference only",
        "never run a template from a script",
        "expected result",
    ] {
        assert!(section.contains(term), "the API section never says {term:?}");
    }
    // A host never matches and is refused: the assistant has to know why.
    assert!(section.contains("host"), "the API section never mentions the host");
    assert!(!section.contains('\u{2014}'), "no em dashes in text an assistant reads");
    // Imported templates are labelled Unproven: only the proven ones are proof.
    assert!(section.contains("the ones marked proven"), "the guide must send the assistant to the PROVEN templates");
    assert!(!section.contains("were proven against this site"), "not every saved template is proven");
    let flat = section.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    // A sign-in page or app shell answered 200 with no redirect passes a
    // status-only check: the guide has to say so and advise `json`.
    assert!(flat.contains("no redirect"), "the guide never says a 200 sign-in page without a redirect passes");
    assert!(flat.contains("give `json` whenever the answer must be data"), "the guide never advises json for data");
    // A tried expect_response is a step of its own.
    assert!(flat.contains("sees only the requests the page makes while it waits"), "the guide never explains a tried check");
    // Every example line in the section is a real action, so a copied one
    // runs, and is indented like the guide's other examples.
    let mut seen = 0;
    for line in section.lines().filter(|l| l.trim_start().starts_with("{ \"kind\"")) {
        assert!(line.starts_with("    { \"kind\""), "an example line is not indented: {line}");
        let action: Action = serde_json::from_str(line.trim()).unwrap_or_else(|e| panic!("{line}: {e}"));
        action.validate().unwrap_or_else(|e| panic!("{line} is refused: {e}"));
        seen += 1;
    }
    assert!(seen >= 2, "the API section should show both kinds as examples");
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
        "retire_autorun_quirk",
        "Patterns across cases",
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
    // The documented shape is the top-level list, said in words too: an
    // `edits` put inside each script is what used to go missing.
    let flat = g.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        flat.contains("`edits` is a TOP-LEVEL list beside `scripts`, as above - never inside a script - with one entry per case you are changing"),
        "the guide never says where `edits` goes"
    );
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

/// Every `STOP:` line `failures::stop_reason` can give is listed in the
/// guide in its own words, and no count is given for them: a count went
/// stale once already when the module-path line was added.
#[test]
fn the_guide_lists_every_stop_line_and_gives_no_count() {
    let g = autorun_guide();
    for line in [
        "the sign-in failed - fix the account or the recipe in the app, not the script",
        "the browser stopped answering - rerun before changing anything",
        "the run could not take this case to its module screen - fix the module path or the case's Module in the app, not the script",
        "the person marked this case Blocked - a missing precondition is not a script defect",
    ] {
        assert!(g.contains(&format!("`STOP: {line}`")), "the guide never lists `STOP: {line}`");
    }
    let flat = g.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    for count in ["three of its lines", "none of those three", "three `stop:`", "the three stop"] {
        assert!(!flat.contains(count), "the guide still counts its STOP lines: `{count}`");
    }
}

/// What to do when the application, not the script, is wrong. The mark is
/// the outcome for a failure the script cannot fix, so the guide has to say
/// when to use it and what it is not.
#[test]
fn the_guide_teaches_marking_a_suspected_application_defect() {
    let g = autorun_guide();
    let repair = &g[g.find("## Repairing a script that failed").unwrap()..];
    let start = repair.find("### When the application is wrong").expect("the subsection is missing");
    let section = repair[start..].split("\n## ").next().unwrap();
    let flat = section.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    assert!(section.contains("mark_autorun_suspected_defect"), "{flat}");
    assert!(flat.contains("the script is not changed"), "{flat}");
    assert!(flat.contains("not a repair"), "{flat}");
    assert!(flat.contains("clears"), "{flat}");
    assert!(flat.contains("unattended runs label a failure at that step"), "{flat}");
    assert!(flat.contains("refused unless that step failed in the case's newest run"), "{flat}");
    assert!(flat.contains("refused when the failure is one of the `stop:` lines"), "{flat}");
    assert!(!section.contains('\u{2014}'), "no em dashes in text an assistant reads");
}

/// Which model does which part of the work is the assistant's own
/// judgement; the guide gives tiers and examples, never a vendor or a rule.
#[test]
fn the_guide_leaves_the_choice_of_model_to_the_assistant() {
    let g = autorun_guide();
    let start = g.find("## Choosing a model for the work").expect("the section is missing");
    let section = g[start..].split("\n## ").next().unwrap();
    let flat = section.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    assert!(flat.contains("your own judgement"), "{flat}");
    assert!(flat.contains("token"), "{flat}");
    for tool in ["get_autorun_failures", "get_autorun_page", "probe_autorun_locator", "list_api_templates"] {
        assert!(section.contains(tool), "the section never names `{tool}`");
    }
    assert!(!section.contains('\u{2014}'), "no em dashes in text an assistant reads");
    for vendor in ["claude", "opus", "sonnet", "haiku", "gpt", "gemini", "anthropic", "openai"] {
        assert!(!flat.contains(vendor), "the section names a vendor or model: {vendor}");
    }
    // Near the top: before the actions are taught.
    assert!(start < g.find("## The actions").unwrap(), "the section belongs near the top");
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

/// The built-in recipe restricts navigation too: the guide must not tell an
/// assistant that a project without its own recipe can go anywhere.
#[test]
fn the_guide_says_navigate_is_held_to_the_site_address_with_either_recipe() {
    let flat = autorun_guide().split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        flat.contains(
            "`navigate` is held to the site address and its allowed sites, whether the sign-in is the \
             project's own recipe or the built-in one. Only with no site address and no saved recipe is it \
             unrestricted."
        ),
        "{flat}"
    );
    assert!(!flat.contains("A project with no recipe saved yet has no such restriction"), "{flat}");
}

/// `when_visible` dismisses what may or may not show up. The guide's own
/// examples - the cookie banner and "Another active session" - are what an
/// assistant copies, so each must validate as a script action.
#[test]
fn the_guide_teaches_when_visible_with_its_two_examples() {
    let g = autorun_guide();
    assert!(g.contains("## Dismissing what may not show up"), "the guide has no when_visible section");
    let section = g.split_once("## Dismissing what may not show up").unwrap().1;
    let section = section.split("
## ").next().unwrap();
    for term in ["not shown, skipped", "2000", "10000", "Another active session", "cookie", "expected-result floor"] {
        assert!(section.contains(term), "the when_visible section never mentions {term}");
    }
    let mut from = 0;
    let mut seen = 0;
    while let Some(at) = section[from..].find("{ \"kind\": \"when_visible\"") {
        let example = first_balanced(section, from + at, '{', '}');
        let a: Action = serde_json::from_str(example).unwrap_or_else(|e| panic!("{e}: {example}"));
        a.validate().unwrap_or_else(|e| panic!("the guide's example is refused: {e}: {example}"));
        seen += 1;
        from += at + example.len();
    }
    assert!(seen >= 2, "the section has {seen} when_visible examples, wants the cookie banner and the session prompt");
}

/// Spec 9: "anywhere on the page" now includes same-origin frames, and the
/// guide says which frames are not read.
#[test]
fn the_guide_says_which_frames_check_text_reads() {
    let g = autorun_guide();
    let line = g.lines().find(|l| l.contains("\"kind\": \"check_text\"")).expect("no check_text line");
    assert!(line.contains("same-origin frames included"), "{line}");
    assert!(line.contains("a frame holding a page from another site is not searched"), "{line}");
}
