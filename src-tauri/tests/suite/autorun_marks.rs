//! Auto Run marks: the shared state a script says it changes (`changes`)
//! or needs unchanged (`needs_unchanged`), compared by `normalise` and
//! validated on every save. See the design "Auto Run reset phases" §1.

use serde_json::{json, Value};
use v2_lib::autorun::marks::{check_marks, normalise};
use v2_lib::autorun::{store, CaseScript};

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn script_with(case_id: i32, changes: Value, needs: Value) -> Value {
    json!({ "case_id": case_id, "title": "t", "changes": changes, "needs_unchanged": needs, "steps": [
        { "step_number": 1, "actions": [{ "kind": "check_text", "value": "ok" }] }
    ] })
}

// ------------------------------------------------------------ comparison

/// Review Focus 1: names that differ only in case or spacing are one name.
#[test]
fn names_compare_trimmed_case_insensitively_with_inner_spaces_collapsed() {
    assert_eq!(normalise("Cycle Published"), "cycle published");
    assert_eq!(normalise(" cycle published "), "cycle published");
    assert_eq!(normalise("cycle   published"), "cycle published");
    assert_eq!(normalise("\tCYCLE\u{20}\u{20}Published\n"), "cycle published");
    assert_eq!(normalise("Cycle Published"), normalise(" cycle  published "));
    assert_eq!(normalise("Appraisal submitted for A001"), "appraisal submitted for a001");
    assert_eq!(normalise("   "), "");
    assert_ne!(normalise("cycle published"), normalise("cycle-published"));
}

// ------------------------------------------------------------ refusals

#[test]
fn marks_within_the_limits_are_accepted() {
    assert_eq!(check_marks(&[], &[]), Ok(()));
    let ten: Vec<String> = (1..=10).map(|n| format!("change {n}")).collect();
    assert_eq!(check_marks(&ten, &ten), Ok(()));
    let sixty = "a".repeat(60);
    assert_eq!(check_marks(&[sixty.clone()], &[sixty]), Ok(()));
    // Counted after trimming: the spaces around a name are not part of it.
    assert_eq!(check_marks(&[format!("  {}  ", "b".repeat(60))], &[]), Ok(()));
}

/// The usual shape: a case that needs X unchanged and then changes X.
#[test]
fn the_same_name_in_both_lists_is_allowed() {
    assert_eq!(check_marks(&names(&["cycle published"]), &names(&["Cycle Published"])), Ok(()));
}

#[test]
fn a_name_longer_than_60_characters_is_refused_in_the_specs_words() {
    let long = "x".repeat(61);
    assert_eq!(
        check_marks(&[long.clone()], &[]),
        Err(vec![format!("changes: \"{long}\" is longer than 60 characters")])
    );
    assert_eq!(
        check_marks(&[], &[long.clone()]),
        Err(vec![format!("needs_unchanged: \"{long}\" is longer than 60 characters")])
    );
    // Characters, not bytes.
    assert_eq!(check_marks(&["é".repeat(60)], &[]), Ok(()));
}

#[test]
fn more_than_ten_names_is_refused_in_the_specs_words() {
    let eleven: Vec<String> = (1..=11).map(|n| format!("change {n}")).collect();
    assert_eq!(check_marks(&eleven, &[]), Err(vec!["changes holds more than 10 names".to_string()]));
    assert_eq!(check_marks(&[], &eleven), Err(vec!["needs_unchanged holds more than 10 names".to_string()]));
}

#[test]
fn an_empty_name_is_refused_in_the_specs_words() {
    assert_eq!(check_marks(&names(&[""]), &[]), Err(vec!["changes: a name cannot be empty".to_string()]));
    assert_eq!(
        check_marks(&[], &names(&["   "])),
        Err(vec!["needs_unchanged: a name cannot be empty".to_string()])
    );
}

/// Ruled for this task: one list holding the same name twice (as
/// `normalise` compares them) is refused, naming it once.
#[test]
fn the_same_name_twice_in_one_list_is_refused() {
    assert_eq!(
        check_marks(&names(&["cycle published", " Cycle  Published ", "CYCLE PUBLISHED"]), &[]),
        Err(vec!["changes: \"Cycle  Published\" is listed twice".to_string()])
    );
    assert_eq!(
        check_marks(&[], &names(&["a", "b", "A"])),
        Err(vec!["needs_unchanged: \"A\" is listed twice".to_string()])
    );
}

#[test]
fn every_problem_is_named_one_sentence_each() {
    let mut changes: Vec<String> = (1..=11).map(|n| format!("change {n}")).collect();
    changes.push(String::new());
    let needs = vec!["y".repeat(61), "ok".to_string(), "OK".to_string()];
    assert_eq!(
        check_marks(&changes, &needs),
        Err(vec![
            "changes holds more than 10 names".to_string(),
            "changes: a name cannot be empty".to_string(),
            format!("needs_unchanged: \"{}\" is longer than 60 characters", "y".repeat(61)),
            "needs_unchanged: \"OK\" is listed twice".to_string(),
        ])
    );
}

// ------------------------------------------------------------ on a save

/// Every door a script comes in by - the editor, a file import and the
/// assistant's save - refuses a mark that breaks the rules, and writes
/// nothing.
#[tokio::test]
async fn every_save_path_refuses_an_invalid_mark() {
    use v2_lib::ai_bridge::{route, BridgeContext};
    use v2_lib::autorun::store::{load_script, set_root};
    use v2_lib::commands::autorun::{import_scripts_from_path, save_script_from_editor};

    let _root = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("data");
    let long = "z".repeat(61);
    let bad_changes = json!([long]);
    let bad_needs = json!(["", "cycle published"]);
    let expected = |id: i32| {
        format!(
            "case {id}: changes: \"{long}\" is longer than 60 characters; needs_unchanged: a name cannot be empty"
        )
    };

    // The editor.
    let script: CaseScript = serde_json::from_value(script_with(7, bad_changes.clone(), bad_needs.clone())).unwrap();
    assert_eq!(save_script_from_editor(&root, "acme", "Web", script).unwrap_err(), expected(7));
    assert!(load_script(&root, 7).unwrap().is_none());

    // A file import: all or nothing.
    let file = dir.path().join("bundle.json");
    std::fs::write(
        &file,
        json!([
            script_with(8, json!(["cycle published"]), json!([])),
            script_with(9, bad_changes.clone(), bad_needs.clone())
        ])
        .to_string(),
    )
    .unwrap();
    assert_eq!(import_scripts_from_path(&root, "acme", "Web", file.to_str().unwrap()).unwrap_err(), expected(9));
    assert!(load_script(&root, 8).unwrap().is_none());

    // The assistant's save, refused before it needs Azure DevOps.
    set_root(root.clone());
    let ctx = BridgeContext { org: "acme".into(), project: "Web".into(), ..BridgeContext::default() };
    let body = json!([script_with(10, bad_changes.clone(), bad_needs.clone())]).to_string();
    let (status, out) = route(&ctx, None, "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!((status, out), (400, expected(10)));
    assert!(load_script(&root, 10).unwrap().is_none());

    // Good marks save from the editor, kept as written.
    let good: CaseScript =
        serde_json::from_value(script_with(11, json!(["Cycle published"]), json!(["cycle published"]))).unwrap();
    save_script_from_editor(&root, "acme", "Web", good).unwrap();
    let back = load_script(&root, 11).unwrap().unwrap();
    assert_eq!(back.changes, vec!["Cycle published".to_string()]);
    assert_eq!(back.needs_unchanged, vec!["cycle published".to_string()]);
}

// ------------------------------------------------------- old script files

/// A script file from before marks loads with none, and saves byte for
/// byte as it was: neither key appears just because the type knows them.
#[test]
fn an_old_script_loads_unchanged_and_saves_byte_identical() {
    let old = "{\n  \"case_id\": 1,\n  \"title\": \"t\",\n  \"steps\": [\n    {\n      \"step_number\": 1,\n      \"actions\": [\n        {\n          \"kind\": \"check_text\",\n          \"value\": \"ok\"\n        }\n      ]\n    }\n  ]\n}";
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("scripts").join("case-1.json");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, old).unwrap();

    let sc = store::load_script(dir.path(), 1).unwrap().unwrap();
    assert!(sc.changes.is_empty() && sc.needs_unchanged.is_empty());
    store::save_script(dir.path(), &sc).unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), old);

    // With some, both lists are written and read back.
    let with: CaseScript =
        serde_json::from_value(script_with(2, json!(["cycle published"]), json!(["appraisal submitted for A001"])))
            .unwrap();
    let text = serde_json::to_string(&with).unwrap();
    assert!(text.contains("\"changes\":[\"cycle published\"]"), "{text}");
    assert!(text.contains("\"needs_unchanged\":[\"appraisal submitted for A001\"]"), "{text}");
    store::save_script(dir.path(), &with).unwrap();
    assert_eq!(store::load_script(dir.path(), 2).unwrap().unwrap(), with);
}
