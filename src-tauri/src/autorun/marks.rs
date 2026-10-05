//! Marks: the shared state a script says it changes (`changes`) or needs
//! unchanged (`needs_unchanged`), by name, for example `"cycle published"`.
//!
//! Names compare by `normalise`, so `Cycle Published` and
//! ` cycle  published ` are one name everywhere. Every save path (the
//! Script editor, an import, the assistant's save) validates them through
//! `check_saved`, by way of `nav::check_project_rules`. Repairs may change
//! them: they affect order, not safety.

use super::CaseScript;

/// The longest a name may be, in characters, once trimmed.
pub const MAX_NAME_CHARS: usize = 60;

/// The most names one list may hold.
pub const MAX_NAMES: usize = 10;

/// The comparison key of a name: trimmed, every run of whitespace inside
/// it collapsed to one space, lowercased.
pub fn normalise(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Every problem with one list, one sentence each, in order: the count
/// first, then each name.
fn list_problems(key: &str, names: &[String], out: &mut Vec<String>) {
    if names.len() > MAX_NAMES {
        out.push(format!("{key} holds more than {MAX_NAMES} names"));
    }
    let mut seen: Vec<String> = Vec::new();
    let mut reported: Vec<String> = Vec::new();
    for name in names {
        let shown = name.trim();
        if shown.is_empty() {
            out.push(format!("{key}: a name cannot be empty"));
            continue;
        }
        if shown.chars().count() > MAX_NAME_CHARS {
            out.push(format!("{key}: \"{shown}\" is longer than {MAX_NAME_CHARS} characters"));
            continue;
        }
        let k = normalise(shown);
        if seen.contains(&k) {
            if !reported.contains(&k) {
                out.push(format!("{key}: \"{shown}\" is listed twice"));
                reported.push(k);
            }
        } else {
            seen.push(k);
        }
    }
}

/// Every problem with a script's marks, one sentence each: `changes`
/// first, then `needs_unchanged`. The same name in both lists is allowed:
/// a case that needs X unchanged and then changes X is the usual shape.
pub fn check_marks(changes: &[String], needs: &[String]) -> Result<(), Vec<String>> {
    let mut out = Vec::new();
    list_problems("changes", changes, &mut out);
    list_problems("needs_unchanged", needs, &mut out);
    if out.is_empty() {
        Ok(())
    } else {
        Err(out)
    }
}

/// The save-time check every save path makes: each script's marks. A
/// refusal names the case, then every problem.
pub fn check_saved(scripts: &[CaseScript]) -> Result<(), String> {
    let refused: Vec<String> = scripts
        .iter()
        .filter_map(|s| {
            check_marks(&s.changes, &s.needs_unchanged)
                .err()
                .map(|found| format!("case {}: {}", s.case_id, found.join("; ")))
        })
        .collect();
    if refused.is_empty() {
        Ok(())
    } else {
        Err(refused.join("; "))
    }
}
