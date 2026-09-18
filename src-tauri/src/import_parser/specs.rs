//! The `specs` list: where the cases' specifications live - file paths
//! (absolute, or relative to the JSON file) and Azure DevOps wiki URLs.
//! Read for the review page's spec pane; written by the Import File tab's
//! Attach control. Same read-patch-write discipline as `comments.rs`: the
//! app owns this key, and nothing else in the file.

use serde_json::Value;

/// The `specs` list from an already-parsed document: the entries in order
/// (non-blank strings only), and how many entries were dropped for not
/// being one - for the importer's warning. A document with no `specs` key,
/// or one that is not an array, simply has none and nothing ignored.
pub fn specs_from_value(doc: &Value) -> (Vec<String>, usize) {
    let entries = doc.get("specs").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut kept = vec![];
    let mut ignored = 0;
    for v in &entries {
        match v.as_str().map(str::trim).filter(|s| !s.is_empty()) {
            Some(s) => kept.push(s.to_string()),
            None => ignored += 1,
        }
    }
    (kept, ignored)
}

/// The list as written, in order; entries that are not non-blank strings
/// are dropped (the importer warns about them). A file that is not an
/// object, or does not parse, simply has none.
pub fn read_specs(json: &str) -> Vec<String> {
    let doc = serde_json::from_str::<Value>(json).unwrap_or(Value::Null);
    specs_from_value(&doc).0
}

/// How many entries `read_specs` would drop - for the importer's warning.
pub fn ignored_spec_entries(json: &str) -> usize {
    let doc = serde_json::from_str::<Value>(json).unwrap_or(Value::Null);
    specs_from_value(&doc).1
}

/// Replace the list, keeping every other key where it was. An empty list
/// removes the key: a file with no specs should not say `"specs": []`.
pub fn patch_specs(json: &str, specs: &[String]) -> Result<String, String> {
    let mut doc = super::comments::document(json)?;
    let obj = doc.as_object_mut().ok_or(
        "this file is a bare list of test cases, so there is nowhere to put its specs - \
         re-export it from the app to get the full format",
    )?;
    if specs.is_empty() {
        obj.remove("specs");
    } else {
        obj.insert("specs".into(), Value::Array(specs.iter().map(|s| Value::String(s.clone())).collect()));
    }
    super::comments::render(&doc)
}
