//! The divergence ruling words: the buttons ask whether the case or the app
//! is right. The old labels must not come back in text a person reads.

use std::path::{Path, PathBuf};

const OLD_LABELS: [&str; 3] = ["The case is right", "Rewrite the case", "Data fixed"];

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

/// Files allowed to name an old label in code. None today: a comment is
/// skipped by line, and the internal names (`case_stands`, `data_fixed`,
/// `rewrite`, the tool's `case_right`) never contain the labels.
const ALLOWED_FILES: [&str; 0] = [];

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
        if ALLOWED_FILES.contains(&rel.as_str()) {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for (n, line) in text.lines().enumerate() {
            let t = line.trim_start();
            // A comment is for the developer, not shown to a person.
            if t.starts_with("//") || t.starts_with("/*") || t.starts_with('*') {
                continue;
            }
            for label in OLD_LABELS {
                if line.contains(label) {
                    found.push(format!("{rel}:{}: {label}", n + 1));
                }
            }
        }
    }
    assert!(found.is_empty(), "old ruling labels in person-facing text:\n{}", found.join("\n"));
}
