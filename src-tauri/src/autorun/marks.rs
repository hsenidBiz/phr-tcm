//! Marks: the shared state a script says it changes (`changes`) or needs
//! unchanged (`needs_unchanged`), by name, for example `"cycle published"`.
//!
//! Names compare by `normalise`, so `Cycle Published` and
//! ` cycle  published ` are one name everywhere. Every save path (the
//! Script editor, an import, the assistant's save) validates them through
//! `check_saved`, by way of `nav::check_project_rules`. Repairs may change
//! them: they affect order, not safety.
//!
//! Settings (`needs_state`) are the same kind of mark with a value: a
//! setting a case needs at one value. `check_state` holds them to the
//! same rules.

use super::CaseScript;
use std::collections::BTreeMap;

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

/// Every problem with a script's settings (`needs_state`), one sentence
/// each: the count first, then each setting in the map's order. A name or
/// a value is held to the same rules as a mark's name. Two spellings of
/// one setting (by `normalise`) that need different values contradict
/// each other and are refused; the same value twice is allowed.
pub fn check_state(state: &BTreeMap<String, String>) -> Result<(), Vec<String>> {
    const KEY: &str = "needs_state";
    let mut out = Vec::new();
    if state.len() > MAX_NAMES {
        out.push(format!("{KEY} holds more than {MAX_NAMES} settings"));
    }
    // Each setting's key, with its first spelling and value as written.
    let mut seen: Vec<(String, String, String)> = Vec::new();
    let mut reported: Vec<String> = Vec::new();
    for (name, value) in state {
        let (shown, wanted) = (name.trim(), value.trim());
        if shown.is_empty() {
            out.push(format!("{KEY}: a setting's name cannot be empty"));
            continue;
        }
        if shown.chars().count() > MAX_NAME_CHARS {
            out.push(format!("{KEY}: \"{shown}\" is longer than {MAX_NAME_CHARS} characters"));
            continue;
        }
        if wanted.is_empty() {
            out.push(format!("{KEY}: \"{shown}\" needs a value"));
            continue;
        }
        if wanted.chars().count() > MAX_NAME_CHARS {
            out.push(format!("{KEY}: the value for \"{shown}\" is longer than {MAX_NAME_CHARS} characters"));
            continue;
        }
        let k = normalise(shown);
        match seen.iter().find(|(key, _, _)| *key == k) {
            Some((_, first, was)) if normalise(was) != normalise(wanted) => {
                if !reported.contains(&k) {
                    out.push(format!(
                        "{KEY}: \"{first}\" cannot need both \"{was}\" and \"{wanted}\" - pick one value"
                    ));
                    reported.push(k);
                }
            }
            Some(_) => {}
            None => seen.push((k, shown.to_string(), wanted.to_string())),
        }
    }
    if out.is_empty() {
        Ok(())
    } else {
        Err(out)
    }
}

/// The save-time check every save path makes: each script's marks, then
/// its settings. A refusal names the case, then every problem.
pub fn check_saved(scripts: &[CaseScript]) -> Result<(), String> {
    let refused: Vec<String> = scripts
        .iter()
        .filter_map(|s| {
            let mut found = check_marks(&s.changes, &s.needs_unchanged).err().unwrap_or_default();
            found.extend(check_state(&s.needs_state).err().unwrap_or_default());
            (!found.is_empty()).then(|| format!("case {}: {}", s.case_id, found.join("; ")))
        })
        .collect();
    if refused.is_empty() {
        Ok(())
    } else {
        Err(refused.join("; "))
    }
}
