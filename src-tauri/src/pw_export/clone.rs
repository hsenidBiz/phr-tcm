//! Reading a PHR-PLAYWRIGHT-AUTOMATION clone: which user keys and navigation
//! keys exist, what the generated index maps, and which specs are on disk.
//! Only the KEYS of `users.json` are ever read - its values hold passwords and
//! are never parsed into anything or kept.

use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

pub const NOT_A_CLONE: &str = "that folder is not a PHR-PLAYWRIGHT-AUTOMATION clone - it needs playwright.config.ts, suites/_generated/index.json, src/navigation.json and src/users/users.json";

const INDEX: &str = "suites/_generated/index.json";
const NAVIGATION: &str = "src/navigation.json";
const USERS: &str = "src/users/users.json";

#[derive(Debug, Clone)]
pub struct ClonedRepo {
    pub root: PathBuf,
    pub user_keys: Vec<String>,
    pub navigation_keys: Vec<String>,
    /// `(test case id, spec file name)` in file order.
    pub index: Vec<(String, String)>,
    pub generated_files: Vec<String>,
}

fn read_object(root: &Path, rel: &str) -> Result<Map<String, Value>, String> {
    let text = std::fs::read_to_string(root.join(rel)).map_err(|e| format!("could not read {rel}: {e}"))?;
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(m)) => Ok(m),
        Ok(_) => Err(format!("{rel} is malformed: expected a JSON object")),
        Err(e) => Err(format!("{rel} is malformed: {e}")),
    }
}

pub fn open(path: &Path) -> Result<ClonedRepo, String> {
    if ["playwright.config.ts", INDEX, NAVIGATION, USERS].iter().any(|f| !path.join(f).is_file()) {
        return Err(NOT_A_CLONE.to_string());
    }
    // Values are dropped immediately: only the keys leave this block.
    let user_keys: Vec<String> = read_object(path, USERS)?.keys().cloned().collect();

    let nav = read_object(path, NAVIGATION)?;
    let navigation_keys: Vec<String> = match nav.get("entries") {
        Some(Value::Object(m)) => m.keys().cloned().collect(),
        _ => return Err(format!("{NAVIGATION} is malformed: it has no \"entries\" object")),
    };

    let mut index = Vec::new();
    for (k, v) in read_object(path, INDEX)? {
        match v {
            Value::String(s) => index.push((k, s)),
            _ => return Err(format!("{INDEX} is malformed: the value for \"{k}\" is not a file name")),
        }
    }

    let mut generated_files = Vec::new();
    if let Ok(rd) = std::fs::read_dir(path.join("suites/_generated")) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.ends_with(".spec.ts") && e.path().is_file() {
                generated_files.push(name);
            }
        }
    }
    generated_files.sort();

    Ok(ClonedRepo { root: path.to_path_buf(), user_keys, navigation_keys, index, generated_files })
}

/// The new `index.json` text: existing pairs in order, `id` replaced in place
/// or appended; two-space indent and a trailing newline.
pub fn index_with(index: &[(String, String)], id: i32, file: &str) -> String {
    let key = id.to_string();
    let mut map = Map::new();
    let mut placed = false;
    for (k, v) in index {
        if *k == key {
            map.insert(k.clone(), Value::String(file.to_string()));
            placed = true;
        } else {
            map.insert(k.clone(), Value::String(v.clone()));
        }
    }
    if !placed {
        map.insert(key, Value::String(file.to_string()));
    }
    let mut out = serde_json::to_string_pretty(&Value::Object(map)).unwrap_or_else(|_| "{}".into());
    out.push('\n');
    out
}
