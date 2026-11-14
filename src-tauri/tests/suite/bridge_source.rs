//! The AI bridge's source as ONE text, for the source-scan tripwires.
//!
//! `src/ai_bridge/` is a module directory (`mod.rs` holds the route match,
//! the sub-modules hold what the arms call). The scans here were written
//! when the bridge was one file, and what they look for - a route arm, the
//! body of one function, a forbidden word - is the same wherever it now
//! sits, so they read the whole directory as the one text it used to be:
//! `mod.rs` first, then the other files by name, line endings as `\n`.

pub fn ai_bridge_source() -> String {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("ai_bridge");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|n| n.ends_with(".rs") && n != "mod.rs")
        .collect();
    names.sort();
    names.insert(0, "mod.rs".to_string());
    let mut text = String::new();
    for name in names {
        let part = std::fs::read_to_string(dir.join(&name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        text.push_str(&part.replace("\r\n", "\n"));
        text.push('\n');
    }
    text
}
