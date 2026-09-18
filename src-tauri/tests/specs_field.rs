//! The `specs` list in a draft file: where the cases' specifications live,
//! read by the app for the review page's spec pane and written back by the
//! Import File tab's Attach control.

use v2_lib::import_parser::specs::{patch_specs, read_specs};

const FILE: &str = r#"{
  "format": "azure-devops-test-cases",
  "version": 1,
  "instructions": "Edit this file.",
  "specs": ["Step13-CalculationEngine.md", "https://dev.azure.com/o/p/_wiki/wikis/p.wiki/12/Engine", 7, ""],
  "comments": "whole-set note",
  "test_cases": [{ "id": 42, "title": "Sign in", "steps": [{"action": "Sign in", "expected": "Signed in"}] }],
  "unknown_key": { "kept": true }
}
"#;

#[test]
fn read_specs_keeps_order_and_ignores_non_strings_and_blanks() {
    assert_eq!(
        read_specs(FILE),
        vec![
            "Step13-CalculationEngine.md".to_string(),
            "https://dev.azure.com/o/p/_wiki/wikis/p.wiki/12/Engine".to_string(),
        ]
    );
    assert!(read_specs("[]").is_empty(), "a bare list has no specs");
    assert!(read_specs("not json").is_empty(), "unreadable means none, never an error");
    assert!(read_specs(r#"{"specs": "one.md"}"#).is_empty(), "a string is not a list");
}

#[test]
fn patch_specs_writes_the_list_and_keeps_everything_else() {
    let out = patch_specs(FILE, &["A.md".into(), "B.md".into()]).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(doc["specs"], serde_json::json!(["A.md", "B.md"]));
    assert_eq!(doc["comments"], "whole-set note");
    assert_eq!(doc["unknown_key"]["kept"], true);
    assert_eq!(doc["test_cases"][0]["id"], 42);
    // Key order is preserved: specs stays where it was, before comments.
    assert!(out.find("\"specs\"").unwrap() < out.find("\"comments\"").unwrap(), "{out}");
    assert!(out.ends_with('\n'));
}

#[test]
fn patch_specs_with_an_empty_list_removes_the_key() {
    let out = patch_specs(FILE, &[]).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(doc.get("specs").is_none());
}

#[test]
fn patch_specs_refuses_a_bare_list_with_the_same_message_as_a_comment() {
    let err = patch_specs(r#"[{"title": "T", "steps": []}]"#, &["A.md".into()]).unwrap_err();
    assert!(err.contains("bare list"), "{err}");
}

#[test]
fn parse_file_returns_the_specs_beside_the_cases() {
    let dir = std::env::temp_dir().join("tcm-v2-specs-field-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{}-cases.json", std::process::id()));
    std::fs::write(&path, FILE).unwrap();
    let parsed = v2_lib::import_parser::parse_file(&path.to_string_lossy()).unwrap();
    assert_eq!(parsed.cases.len(), 1);
    assert_eq!(parsed.specs, vec!["Step13-CalculationEngine.md".to_string(), "https://dev.azure.com/o/p/_wiki/wikis/p.wiki/12/Engine".to_string()]);
    assert!(
        parsed.warnings.iter().any(|w| w.contains("specs") && w.contains("ignored")),
        "the non-string entry is named: {:?}",
        parsed.warnings
    );
}

#[test]
fn the_export_instructions_name_the_field() {
    let json = v2_lib::import_parser::queue_to_json_string(&[]).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(doc["instructions"].as_str().unwrap().contains("'specs'"), "{}", doc["instructions"]);
}
