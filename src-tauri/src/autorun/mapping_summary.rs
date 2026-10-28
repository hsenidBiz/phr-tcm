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
    crate::ai_tools::atomic_write(&path, &text)?;
    super::data_changed::notify(root, crate::events::AutorunDataKind::MappingSummary);
    Ok(())
}

/// `run` as a summary. A screen met more than once, in one list or in
/// several (names compared by `module_key`), appears once: in the list of
/// its last outcome (`MappingRun::outcomes`), with that list's last entry
/// for it. A screen not reached that another outcome, before or after it,
/// saved under another name (`saved_elsewhere`) is in no list. Every name
/// and reason has its addresses taken out.
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
        if last.get(&key) == Some(&MappingOutcome::Unreached) && saved_elsewhere(&run.outcomes, i) {
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
/// updated or found unchanged - by an outcome under another name, before
/// or after it in the run: one that arrived on the same address path, or,
/// when `i` has no address path, one with the same menu path. Nothing
/// known about `i` matches nothing. A save under the same name earlier in
/// the run does not count: then the screen's last outcome is that it was
/// not reached.
fn saved_elsewhere(outcomes: &[(String, MappingOutcome, Option<MappingScreen>)], i: usize) -> bool {
    let Some((name, _, Some(missed))) = outcomes.get(i) else { return false };
    let key = module_key(name);
    outcomes.iter().filter(|(other, _, _)| module_key(other) != key).any(|(_, outcome, screen)| {
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
    // The word before, lower-cased, for `bare_host_path`: `version 1.0.0.0`.
    let mut before = String::new();
    let flush = |word: &mut String, before: &mut String, out: &mut String| {
        if !word.is_empty() {
            out.push_str(&clean_word(word, before));
            *before = word.trim_matches(|c: char| !c.is_alphanumeric()).to_ascii_lowercase();
            word.clear();
        }
    };
    for c in text.chars() {
        if c.is_whitespace() {
            flush(&mut word, &mut before, &mut out);
            out.push(c);
        } else {
            word.push(c);
        }
    }
    flush(&mut word, &mut before, &mut out);
    out
}

/// One word of `without_addresses`, with the quotes or brackets around an
/// address kept. `before` is the word ahead of it, lower-cased.
fn clean_word(word: &str, before: &str) -> String {
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
    } else if let Some(path) = bare_host_path(core, before) {
        path
    } else {
        core.to_string()
    };
    format!("{}{cleaned}{tail}", &word[..lead])
}

/// Last labels that name a file far more often than a site, so a word such
/// as `Index.cshtml.cs:40` (a code location) or `report.pdf/` is left alone.
const FILE_EXTENSIONS: &[&str] = &[
    "cs", "rs", "ts", "tsx", "js", "jsx", "mjs", "json", "py", "rb", "go", "java", "php", "cpp", "sh", "md", "txt", "log",
    "html", "htm", "css", "scss", "xml", "yml", "yaml", "toml", "ini", "sql", "csv", "cshtml", "razor", "pdf", "png", "jpg",
    "jpeg", "gif", "svg", "docx", "xlsx", "zip", "rpt", "aspx", "ascx", "ashx", "asmx", "axd", "master", "config", "vb", "kt",
    "vue", "swift", "dart",
];

/// The path of a host written without a scheme or `www.`, or `None` when
/// `core` is not one. It is one only when it is an IPv4 address, or a
/// dot-separated name whose last label looks like a top-level domain (two
/// to 24 letters) AND which is followed by a `/` path or a `:port`. So
/// `hr.example.com/x`, `hr.example.com:8080` and `10.0.0.5:8080/hr` are
/// hidden, while a version (`v2.2.3`), an abbreviation (`e.g.`), a file
/// name (`cv.txt`) and a code location (`Index.cshtml.cs:40`) are words.
///
/// Four numbers with dots are an IPv4 address when they carry a `:port` or a
/// `/path`, or when each is 0-255 and the word before is not `version`, `ver`
/// or `v` (so `version 1.0.0.0` stays; `v7.2.0.15` is no address at all).
fn bare_host_path(core: &str, before: &str) -> Option<String> {
    let (authority, rest) = match core.find('/') {
        Some(i) => (&core[..i], Some(&core[i..])),
        None => (core, None),
    };
    let (host, has_port) = match authority.rsplit_once(':') {
        Some((h, port)) if !port.is_empty() && port.len() <= 5 && port.bytes().all(|b| b.is_ascii_digit()) => (h, true),
        Some(_) => return None,
        None => (authority, false),
    };
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 || labels.iter().any(|l| l.is_empty() || !l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')) {
        return None;
    }
    let ipv4 = labels.len() == 4
        && labels.iter().all(|l| (1..=3).contains(&l.len()) && l.bytes().all(|b| b.is_ascii_digit()))
        && (rest.is_some() || has_port || (labels.iter().all(|l| l.parse::<u16>().is_ok_and(|n| n <= 255)) && !matches!(before, "version" | "ver" | "v")));
    let last = labels[labels.len() - 1];
    let tld = (2..=24).contains(&last.len())
        && last.bytes().all(|b| b.is_ascii_alphabetic())
        && !FILE_EXTENSIONS.contains(&last.to_ascii_lowercase().as_str());
    if !(ipv4 || (tld && (rest.is_some() || has_port))) {
        return None;
    }
    let path = rest.unwrap_or("/").split(['?', '#']).next().unwrap_or("");
    Some(if path.is_empty() { "/".to_string() } else { path.to_string() })
}
