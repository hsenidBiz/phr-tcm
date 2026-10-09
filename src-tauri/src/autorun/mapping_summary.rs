//! The last mapping run's summary, kept per project and shown in the
//! Discovery dialog (spec 2026-10-09 section 3): when it ran, the modules
//! it mapped, the screens it added, updated, found unchanged and could not
//! reach, and how many saves its guard blocked.
//!
//! Saved beside the project's other files as `<slug>-mapping.json`, written
//! atomically. It holds area names, menu click names and reasons only:
//! never a host, a query string or a whole address.

use super::nav::module_key;
use super::recipe::project_slug;
use crate::commands::autorun::{MappingOutcome, MappingRun, MappingScreen};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A screen whose menu path the run changed.
#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, PartialEq)]
pub struct UpdatedArea {
    pub name: String,
    /// The menu path before, as click names joined with ", then ".
    pub old_path: String,
    /// The menu path now. The same as `old_path` when only the page the
    /// clicks arrive on changed.
    pub new_path: String,
}

/// A screen the run could not reach, and why.
#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, PartialEq)]
pub struct UnreachedArea {
    pub name: String,
    pub reason: String,
}

/// What one mapping run did. Each screen is in exactly one list: the list
/// of the last thing that happened to it in the run.
#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, PartialEq)]
pub struct MappingSummary {
    /// When the run started, milliseconds since the epoch; a JavaScript
    /// number holds it exactly.
    #[specta(type = f64)]
    pub ran_at: u64,
    pub modules: Vec<String>,
    pub added: Vec<String>,
    pub updated: Vec<UpdatedArea>,
    pub unchanged: Vec<String>,
    pub unreached: Vec<UnreachedArea>,
    /// How many save requests the run's guard blocked.
    pub blocked_writes: u32,
}

/// `projects/<slug>-mapping.json` under the Auto Run folder.
pub fn summary_path(root: &Path, org: &str, project: &str) -> PathBuf {
    root.join("projects").join(format!("{}-mapping.json", project_slug(org, project)))
}

/// The project's last mapping summary, or `None` when no run has ended.
pub fn load_summary(root: &Path, org: &str, project: &str) -> Result<Option<MappingSummary>, String> {
    let path = summary_path(root, org, project);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("The last menu mapping could not be read: {e}")),
    };
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|e| format!("The last menu mapping could not be read ({e}); the next mapping run replaces it."))
}

/// Keep `summary` as the project's last, replacing the one before.
pub fn save_summary(root: &Path, org: &str, project: &str, summary: &MappingSummary) -> Result<(), String> {
    let path = summary_path(root, org, project);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(summary).map_err(|e| e.to_string())?;
    crate::ai_tools::atomic_write(&path, &text)
}

/// `run` as a summary. A screen met more than once, in one list or in
/// several (names compared by `module_key`), appears once: in the list of
/// its last outcome (`MappingRun::outcomes`), with that list's last entry
/// for it. A screen not reached that a later outcome saved under another
/// name (`saved_later`) is in no list. Every name and reason has its
/// addresses taken out.
pub fn summarize(run: &MappingRun) -> MappingSummary {
    let mut last: HashMap<String, MappingOutcome> = HashMap::new();
    let mut at: HashMap<String, usize> = HashMap::new();
    for (i, (name, outcome, _)) in run.outcomes.iter().enumerate() {
        last.insert(module_key(name), *outcome);
        at.insert(module_key(name), i);
    }
    // A name the run's lists hold but its outcomes never named: the last
    // list it is in, in the order a screen's outcomes can come.
    let mut fallback: HashMap<String, MappingOutcome> = HashMap::new();
    let mut fall = |name: &str, outcome| {
        let key = module_key(name);
        if !last.contains_key(&key) {
            fallback.insert(key, outcome);
        }
    };
    run.added.iter().for_each(|n| fall(n, MappingOutcome::Added));
    run.updated.iter().for_each(|(n, _, _)| fall(n, MappingOutcome::Updated));
    run.unchanged.iter().for_each(|n| fall(n, MappingOutcome::Unchanged));
    run.unreached.iter().for_each(|(n, _)| fall(n, MappingOutcome::Unreached));
    last.extend(fallback);
    for (key, i) in at {
        if last.get(&key) == Some(&MappingOutcome::Unreached) && saved_later(&run.outcomes, i) {
            last.remove(&key);
        }
    }

    let clean = |s: &str| without_addresses(s.trim());
    MappingSummary {
        ran_at: run.started_at,
        modules: run.modules.iter().map(|m| clean(m)).collect(),
        added: kept(&run.added, |n| n, MappingOutcome::Added, &last).into_iter().map(|n| clean(n)).collect(),
        updated: kept(&run.updated, |(n, _, _)| n, MappingOutcome::Updated, &last)
            .into_iter()
            .map(|(n, old, new)| UpdatedArea { name: clean(n), old_path: clean(old), new_path: clean(new) })
            .collect(),
        unchanged: kept(&run.unchanged, |n| n, MappingOutcome::Unchanged, &last).into_iter().map(|n| clean(n)).collect(),
        unreached: kept(&run.unreached, |(n, _)| n, MappingOutcome::Unreached, &last)
            .into_iter()
            .map(|(n, why)| UnreachedArea { name: clean(n), reason: clean(why) })
            .collect(),
        blocked_writes: run.blocked_writes,
    }
}

/// Whether the screen outcome `i` could not reach was saved - added,
/// updated or found unchanged - by an outcome after it, under any name:
/// one that arrived on the same address path, or, when `i` has no address
/// path, one with the same menu path. Nothing known about `i` matches
/// nothing.
fn saved_later(outcomes: &[(String, MappingOutcome, Option<MappingScreen>)], i: usize) -> bool {
    let Some(Some(missed)) = outcomes.get(i).map(|o| o.2.as_ref()) else { return false };
    outcomes[i + 1..].iter().any(|(_, outcome, screen)| {
        let saved = matches!(outcome, MappingOutcome::Added | MappingOutcome::Updated | MappingOutcome::Unchanged);
        saved
            && screen.as_ref().is_some_and(|s| match &missed.arrived {
                Some(arrived) => s.arrived.as_ref() == Some(arrived),
                None => !missed.menu.is_empty() && s.menu == missed.menu,
            })
    })
}

/// The entries of `items` whose name's last outcome is `outcome`, each
/// name once, at its last entry in `items`.
fn kept<'a, T>(
    items: &'a [T],
    name: impl Fn(&'a T) -> &'a String,
    outcome: MappingOutcome,
    last: &HashMap<String, MappingOutcome>,
) -> Vec<&'a T> {
    let keys: Vec<String> = items.iter().map(|it| module_key(name(it))).collect();
    items
        .iter()
        .enumerate()
        .filter(|(i, _)| last.get(&keys[*i]) == Some(&outcome) && !keys[i + 1..].contains(&keys[*i]))
        .map(|(_, it)| it)
        .collect()
}

/// One log line for an ended run: its counts and its screens' names, with
/// no path, reason or address.
pub fn log_line(summary: &MappingSummary) -> String {
    let names = |list: &[String]| if list.is_empty() { "none".to_string() } else { list.join(", ") };
    let updated: Vec<String> = summary.updated.iter().map(|u| u.name.clone()).collect();
    let unreached: Vec<String> = summary.unreached.iter().map(|u| u.name.clone()).collect();
    format!(
        "Auto Run mapping run ended: {} added ({}), {} updated ({}), {} unchanged ({}), {} not reached ({}), {} saves blocked",
        summary.added.len(),
        names(&summary.added),
        summary.updated.len(),
        names(&updated),
        summary.unchanged.len(),
        names(&summary.unchanged),
        summary.unreached.len(),
        names(&unreached),
        summary.blocked_writes,
    )
}

/// `text` with every address in it reduced to its path: no scheme, host,
/// query string or fragment. A bare path keeps no query string or fragment
/// either. A reason a replay gave ("could not open https://host/?t=x")
/// can carry one, and the summary is shown and logged.
pub fn without_addresses(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if !word.is_empty() {
            out.push_str(&clean_word(word));
            word.clear();
        }
    };
    for c in text.chars() {
        if c.is_whitespace() {
            flush(&mut word, &mut out);
            out.push(c);
        } else {
            word.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// One word of `without_addresses`, with the quotes or brackets around an
/// address kept.
fn clean_word(word: &str) -> String {
    const AROUND: &[char] = &['"', '\'', '(', ')', '[', ']', '<', '>', '{', '}', ',', ';', '.', ':', '!'];
    let lead = word.len() - word.trim_start_matches(AROUND).len();
    let core = word.trim_start_matches(AROUND).trim_end_matches(AROUND);
    if core.is_empty() {
        return word.to_string();
    }
    let tail = &word[lead + core.len()..];
    let cleaned = if core.contains("://") || core.to_ascii_lowercase().starts_with("www.") {
        let path = crate::autorun::discovery_map::path_only(core);
        if core.contains("://") {
            path
        } else {
            // `www.host/x` has no scheme for `path_only` to find.
            match path.find('/') {
                Some(i) => path[i..].to_string(),
                None => "/".to_string(),
            }
        }
    } else if core.starts_with('/') {
        core.split(['?', '#']).next().unwrap_or("").to_string()
    } else {
        core.to_string()
    };
    format!("{}{cleaned}{tail}", &word[..lead])
}
