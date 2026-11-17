//! The divergence ruling words: the buttons ask whether the case or the app
//! is right. The old labels must not come back in text a person reads.

use std::path::{Path, PathBuf};

/// Compared in lower case: the labels and their sentence forms.
const OLD_WORDS: [&str; 7] = [
    "the case is right",
    "rewrite the case",
    "data fixed",
    "data is fixed",
    "the case is being rewritten",
    "the data was fixed",
    "the case stands",
];

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| matches!(e.to_str(), Some("rs" | "ts" | "tsx" | "md")))
        {
            out.push(path);
        }
    }
}

/// Files allowed to use an old phrase. A comment is skipped by line, and the
/// internal names (`case_stands`, `data_fixed`, `rewrite`, `case_right`) do
/// not contain the phrases. Each file here holds text only an assistant reads.
const ALLOWED_FILES: [(&str, &str); 3] = [
    ("src-tauri/src/autorun/divergences.rs", "failure answers and healer lines to the assistant, which keep internal names"),
    ("src-tauri/src/mcp.rs", "the rule_case_divergence tool description, read by the assistant"),
    ("src-tauri/src/autorun/case_values.rs", "a repair-refusal answer to the assistant"),
];

#[test]
fn no_person_facing_text_uses_the_old_ruling_labels() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = vec![];
    for dir in ["src", "src-tauri/src", "docs-site/src/content"] {
        collect(&repo.join(dir), &mut files);
    }
    assert!(files.len() > 100, "the scan found only {} files", files.len());
    let mut found = vec![];
    for path in files {
        let rel = path.strip_prefix(repo).unwrap().to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/");
        if ALLOWED_FILES.iter().any(|(f, _)| *f == rel) {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for (n, line) in text.lines().enumerate() {
            let t = line.trim_start();
            // A comment is for the developer, not shown to a person.
            if t.starts_with("//") || t.starts_with("/*") || t.starts_with('*') {
                continue;
            }
            let lower = line.to_lowercase();
            for label in OLD_WORDS {
                if lower.contains(label) {
                    found.push(format!("{rel}:{}: {label}", n + 1));
                }
            }
        }
    }
    assert!(found.is_empty(), "old ruling labels in person-facing text:\n{}", found.join("\n"));
}
