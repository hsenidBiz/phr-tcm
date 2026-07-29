//! Writing comments back into the JSON file a draft was imported from.
//!
//! The file is PATCHED, never regenerated. An assistant may be editing the
//! same file, and it holds keys we do not own - `instructions`, whatever a
//! future format adds, whatever the author put there by hand. Reading the
//! document, changing one value and writing it back leaves all of that
//! alone; rebuilding it from the queue would quietly drop it and reorder
//! what survived. (Key order survives because serde_json is built with
//! `preserve_order` - without it every save would reshuffle the file
//! alphabetically and every diff would be noise.)
//!
//! Two things are stored here:
//!
//! - a **per-case** comment, in that case's `comment` field - the same
//!   field the queue card edits and the importer already round-trips;
//! - a **file-wide** comment in a top-level `comments` string, for the
//!   notes that belong to the set rather than to any one case.
//!
//! Both are removed rather than blanked when the text is emptied, so a file
//! that never had a comment goes back to not having one.

use serde_json::Value;

/// Which case in the file a comment belongs to.
///
/// Deliberately the same identity rule as the frontend's `caseKey`: a work
/// item id when there is one, the trimmed lower-cased title otherwise.
/// Position is not used - the queue holds cases from several files and from
/// Manual Entry, so a queue index means nothing inside one file.
#[derive(Debug, Clone, PartialEq)]
pub struct CaseTarget {
    pub id: Option<i32>,
    pub title: String,
}

impl CaseTarget {
    fn matches(&self, case: &Value) -> bool {
        if let Some(id) = self.id {
            return case.get("id").and_then(Value::as_i64) == Some(id as i64);
        }
        // An id-less case in the file must not be claimed by an id-ful
        // target's title, and vice versa - that is what the id branch above
        // already guarantees. Here both sides are id-less.
        if case.get("id").and_then(Value::as_i64).is_some() {
            return false;
        }
        let title = case.get("title").and_then(Value::as_str).unwrap_or("");
        title.trim().eq_ignore_ascii_case(self.title.trim())
    }
}

fn document(json: &str) -> Result<Value, String> {
    serde_json::from_str(json).map_err(|e| format!("not valid JSON: {e}"))
}

/// The cases, whether the file is the wrapper shape or the bare list the
/// importer also accepts.
fn cases_of(doc: &mut Value) -> Result<&mut Vec<Value>, String> {
    if doc.is_array() {
        return Ok(doc.as_array_mut().expect("checked"));
    }
    doc.get_mut("test_cases")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| "the file has no test_cases list".into())
}

/// The whole document back as text, in the same pretty shape the exporter
/// writes so a comment save and a fresh export look identical on disk.
fn render(doc: &Value) -> Result<String, String> {
    let mut text = serde_json::to_string_pretty(doc).map_err(|e| e.to_string())?;
    text.push('\n');
    Ok(text)
}

/// Set (or clear) one case's comment. Errors when the case is not in this
/// file - better a visible "not saved" in the page than a comment written
/// against the wrong case.
pub fn patch_case_comment(json: &str, target: &CaseTarget, text: &str) -> Result<String, String> {
    let mut doc = document(json)?;
    let cases = cases_of(&mut doc)?;
    // Not `find`. A draft case has no id, so its title is its whole
    // identity, and a file may hold the same title twice - the importer
    // warns about that and still imports both. Taking the first match wrote
    // the comment onto the wrong case and reported it saved. The payload
    // from the page carries no way to tell the two apart, so the only
    // honest answer is to say which one is ambiguous.
    let hits: Vec<usize> = cases
        .iter()
        .enumerate()
        .filter(|(_, c)| target.matches(c))
        .map(|(i, _)| i)
        .collect();
    let idx = match hits.as_slice() {
        [only] => *only,
        [] => return Err(format!("'{}' is not in this file", target.title)),
        many => {
            return Err(format!(
                "'{}' appears {} times in this file, so there is no way to tell which one this \
                 comment is about - give them different titles and try again. (The queue card \
                 writes through this same path, so it cannot get round it either.)",
                target.title,
                many.len()
            ))
        }
    };
    let obj = cases[idx]
        .as_object_mut()
        .ok_or("a test case in this file is not an object")?;
    if text.trim().is_empty() {
        obj.remove("comment");
    } else {
        obj.insert("comment".into(), Value::String(text.to_string()));
    }
    render(&doc)
}

/// Set (or clear) the file-wide comment.
pub fn patch_general_comment(json: &str, text: &str) -> Result<String, String> {
    let mut doc = document(json)?;
    let obj = doc.as_object_mut().ok_or(
        "this file is a bare list of test cases, so there is nowhere to put a comment about \
         the whole set - re-export it from the app to get the full format",
    )?;
    if text.trim().is_empty() {
        obj.remove("comments");
    } else {
        obj.insert("comments".into(), Value::String(text.to_string()));
    }
    render(&doc)
}

/// The file-wide comment, empty when the file has none. A file that is
/// missing or unreadable simply has no comment - the caller is prefilling a
/// text box, not validating the file.
pub fn general_comment(json: &str) -> String {
    serde_json::from_str::<Value>(json)
        .ok()
        .and_then(|d| d.get("comments").and_then(Value::as_str).map(String::from))
        .unwrap_or_default()
}
