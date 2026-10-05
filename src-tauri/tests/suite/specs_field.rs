//! The `specs` list in a draft file: where the cases' specifications live,
//! read by the app for the review page's spec pane and written back by the
//! Import Test Cases tab's Attach control.

use v2_lib::import_parser::specs::{ignored_spec_entries, patch_specs, read_specs};

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

#[test]
fn a_file_saved_with_a_bom_reads_and_saves_its_specs() {
    let bom = format!("\u{feff}{FILE}");
    assert_eq!(read_specs(&bom).len(), 2, "the spec pane must not go empty on a BOM");
    assert_eq!(ignored_spec_entries(&bom), 2);
    let out = patch_specs(&bom, &["A.md".into()]).unwrap();
    assert!(!out.starts_with('\u{feff}'));
    let doc: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(doc["specs"], serde_json::json!(["A.md"]));
    assert_eq!(doc["comments"], "whole-set note");
}

// Only .md files and Azure DevOps wiki links can be specs. One rule,
// `check_spec`, decides it for every reader and writer of the list.

fn refusal(entry: &str) -> String {
    format!("{entry} cannot be a spec - only .md files and Azure DevOps wiki links can be added")
}

#[test]
fn check_spec_accepts_markdown_files_and_azure_devops_wiki_links() {
    use v2_lib::import_parser::specs::check_spec;
    for ok in [
        "Spec.md",
        "docs/Step13.MD",
        r"..\specs\Rules.markdown",
        r"C:\specs\Engine.md",
        "/home/me/specs/Engine.Markdown",
        "https://dev.azure.com/o/p/_wiki/wikis/p.wiki/12/Engine",
        "https://contoso.visualstudio.com/Web/_wiki/wikis/Web.wiki/7/Login",
        " https://dev.azure.com/o/p/_wiki/wikis/p.wiki?pagePath=%2FEngine ",
    ] {
        assert_eq!(check_spec(ok), Ok(()), "{ok}");
    }
}

#[test]
fn check_spec_refuses_everything_else_with_the_sentence() {
    use v2_lib::import_parser::specs::check_spec;
    for bad in [
        "Views/Payroll/Index.cshtml",
        "notes.txt",
        r"C:\src\PayrollController.cs",
        "spec.pdf",
        ".md",
        "Spec.md.cshtml",
        "https://example.com/spec.md",
        "https://dev.azure.com/o/p/_git/repo?path=/Spec.md",
        "http://dev.azure.com/o/p/_wiki/wikis/p.wiki/12/Engine",
        "https://dev.azure.com.evil.example/o/p/_wiki/wikis/p.wiki/12/Engine",
        "file:///C:/specs/Spec.md",
    ] {
        assert_eq!(check_spec(bad), Err(refusal(bad)), "{bad}");
    }
}

#[test]
fn the_importer_drops_a_refused_entry_and_warns_by_name() {
    let json = r#"{
      "specs": ["Spec.md", "Views/Payroll/Index.cshtml", "https://example.com/x", 7],
      "test_cases": [{ "title": "Sign in", "steps": [{"action": "Sign in", "expected": "Signed in"}] }]
    }"#;
    let parsed = v2_lib::import_parser::parse_json_text(json).unwrap();
    assert_eq!(parsed.specs, vec!["Spec.md".to_string()]);
    let w = &parsed.warnings;
    assert!(w.contains(&format!("specs: {}", refusal("Views/Payroll/Index.cshtml"))), "{w:?}");
    assert!(w.contains(&format!("specs: {}", refusal("https://example.com/x"))), "{w:?}");
    // The non-string entry keeps its own warning, unchanged.
    assert!(w.iter().any(|x| x.starts_with("specs: 1 entry ignored")), "{w:?}");
    assert_eq!(read_specs(json), vec!["Spec.md".to_string()], "the spec pane reads the same list");
    assert_eq!(
        v2_lib::import_parser::specs::refused_spec_entries(json),
        vec!["Views/Payroll/Index.cshtml".to_string(), "https://example.com/x".to_string()]
    );
}

/// The Attach control's save (`save_specs`) checks the list with
/// `check_specs` before anything else, and `patch_specs` - the write it
/// goes through - refuses on its own too, so nothing writes a refused entry.
#[test]
fn saving_a_list_with_a_refused_entry_is_refused_whole() {
    use v2_lib::import_parser::specs::check_specs;
    let list = vec!["A.md".to_string(), "Views/Index.cshtml".to_string()];
    assert_eq!(check_specs(&list), Err(refusal("Views/Index.cshtml")));
    assert_eq!(patch_specs(FILE, &list), Err(refusal("Views/Index.cshtml")));
    assert_eq!(check_specs(&["A.md".to_string(), "https://dev.azure.com/o/p/_wiki/wikis/w/1/X".to_string()]), Ok(()));
}
