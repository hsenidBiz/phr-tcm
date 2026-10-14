//! Which ADO ids the clone already has, and where.
//!
//! A case is in the repo when any one of these names it:
//! - a `## <id>` heading in a test-case file, `suites/**/test-cases/*.md`
//!   (`structure::case_headings`), or a `## Excluded <id>` one
//!   (`structure::excluded_headings`): the team left that case out on
//!   purpose, and it is never exported, not even when chosen to replace;
//! - an entry in `suites/_generated/index.json` (`pw_export::clone::open`);
//! - a `// ADO <id>` marker in a spec, `suites/**/specs/*.spec.ts`
//!   (`runner::marker_in`).
//!
//! Each place is kept as a path relative to the clone, forward slashes.
//! Whether this app exported the case comes from the Sync record
//! (`with_record`), never from the repo. Read only.

use crate::pw_export::clone::{self, ClonedRepo};
use crate::pw_repo::sync::ExportRecord;
use crate::pw_repo::{runner, structure};
use std::collections::BTreeMap;
use std::path::Path;

/// Where `index.json` is, relative to the clone.
pub const INDEX_REL: &str = "suites/_generated/index.json";

/// One id the repo has.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Found {
    /// Relative to the clone, forward slashes: test-case files first, then
    /// `index.json`, then specs, each once.
    pub places: Vec<String>,
    /// The Sync record has an export of it by this app.
    pub from_app: bool,
    /// The test-case files that have it under `## Excluded <id>`.
    pub excluded_in: Vec<String>,
    /// The test-case files that have it under a `## <id>` heading (its
    /// sections), Excluded ones not among them.
    pub sections: Vec<String>,
}

impl Found {
    pub fn excluded(&self) -> bool {
        !self.excluded_in.is_empty()
    }
}

/// Every id the repo has, by id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Presence {
    pub cases: BTreeMap<i32, Found>,
}

impl Presence {
    pub fn has(&self, id: i32) -> bool {
        self.cases.contains_key(&id)
    }

    pub fn places(&self, id: i32) -> &[String] {
        self.cases.get(&id).map(|f| f.places.as_slice()).unwrap_or(&[])
    }

    /// Where `id` is, short: "in competencies.md and specs/competencies.spec.ts".
    /// `None` when the repo does not have it.
    pub fn note(&self, id: i32) -> Option<String> {
        let places = self.places(id);
        if places.is_empty() {
            return None;
        }
        Some(in_places(places))
    }

    pub fn excluded(&self, id: i32) -> bool {
        self.cases.get(&id).is_some_and(Found::excluded)
    }

    /// Whether `id` has a section in a feature's test-case file, the files
    /// the Overview counts (`structure::feature_test_cases`).
    pub fn has_feature_section(&self, id: i32) -> bool {
        self.cases.get(&id).is_some_and(|f| f.sections.iter().any(|p| structure::feature_test_cases(p)))
    }

    /// "Excluded in the repo (in pc-cycle-setup.md)" for a case the team
    /// left out on purpose; `None` for any other.
    pub fn excluded_note(&self, id: i32) -> Option<String> {
        let found = self.cases.get(&id).filter(|f| f.excluded())?;
        Some(format!("Excluded in the repo ({})", in_places(&found.excluded_in)))
    }

    /// Marks each id the Sync record has an export of as this app's.
    pub fn with_record(mut self, record: &ExportRecord) -> Presence {
        for (id, found) in self.cases.iter_mut() {
            found.from_app = record.cases.contains_key(id);
        }
        self
    }
}

/// "in a.md, index.json and specs/a.spec.ts".
fn in_places(places: &[String]) -> String {
    let short: Vec<String> = places.iter().map(|p| short_place(p)).collect();
    let joined = match short.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => short.join(""),
    };
    format!("in {joined}")
}

/// A place as the note names it: a test-case file by its name, a spec as
/// `specs/<name>`, `index.json` as itself.
fn short_place(rel: &str) -> String {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    if rel.contains("/specs/") {
        format!("specs/{name}")
    } else {
        name.to_string()
    }
}

/// The clone at `root`, opened, then `scan_repo`.
pub fn scan(root: &Path) -> Result<Presence, String> {
    Ok(scan_repo(&clone::open(root)?))
}

/// What the clone `repo` (already opened) has.
pub fn scan_repo(repo: &ClonedRepo) -> Presence {
    let mut md: Vec<(i32, String)> = Vec::new();
    let mut excluded: Vec<(i32, String)> = Vec::new();
    let mut specs: Vec<(i32, String)> = Vec::new();
    walk(&repo.root, &repo.root.join("suites"), &mut md, &mut excluded, &mut specs);

    let mut out = Presence::default();
    let mut add = |id: i32, place: String| {
        let found = out.cases.entry(id).or_default();
        if !found.places.contains(&place) {
            found.places.push(place);
        }
    };
    for (id, place) in md.iter().chain(excluded.iter()) {
        add(*id, place.clone());
    }
    for (id, _) in &repo.index {
        if let Ok(id) = id.trim().parse::<i32>() {
            add(id, INDEX_REL.to_string());
        }
    }
    for (id, place) in specs {
        add(id, place);
    }
    for (id, place) in excluded {
        let found = out.cases.entry(id).or_default();
        if !found.excluded_in.contains(&place) {
            found.excluded_in.push(place);
        }
    }
    for (id, place) in md {
        let found = out.cases.entry(id).or_default();
        if !found.sections.contains(&place) {
            found.sections.push(place);
        }
    }
    out
}

/// Every test-case file and spec under `dir`, sorted, skipping
/// `node_modules` and dot-folders.
fn walk(
    root: &Path,
    dir: &Path,
    md: &mut Vec<(i32, String)>,
    excluded: &mut Vec<(i32, String)>,
    specs: &mut Vec<(i32, String)>,
) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).into_iter().flatten().flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    let parent = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    for e in entries {
        let path = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if name != "node_modules" && !name.starts_with('.') {
                walk(root, &path, md, excluded, specs);
            }
            continue;
        }
        let rel = || path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        if parent == "test-cases" && name.ends_with(".md") {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            for id in structure::case_headings(&text) {
                md.push((id, rel()));
            }
            for id in structure::excluded_headings(&text) {
                excluded.push((id, rel()));
            }
        } else if parent == "specs" && name.ends_with(".spec.ts") {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            for id in text.lines().filter_map(runner::marker_in) {
                specs.push((id, rel()));
            }
        }
    }
}
