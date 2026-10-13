//! Finish in the repo: the prompt a person pastes into Claude Code opened in
//! their clone, so the repo's own `/gen-test` finishes what an export began.
//!
//! An export does `/gen-test`'s first three steps' work (each case's raw
//! spec in `suites/_generated/`, its `index.json` entry and its section in
//! the feature's test-case file) but never runs a raw spec. Steps 4 (run
//! and heal the raw spec), 5 (refactor), 6 (verify) and 7 (run summary) are
//! the repo's, and `.claude/commands/gen-test.md` has no "start at step 4"
//! mode: a bare `/gen-test <id>` would fetch the ticket and
//! generate a new raw spec over the export. So the prompt names the steps
//! and the files instead.
//!
//! The prompt is fixed text plus values checked here only: the case ids,
//! the `<seg>` and file paths read from the clone, and a spec stem that is
//! one of the feature's own or matches `STEM_RULE`. One prompt per `<seg>`,
//! since `/gen-test` takes one `<seg>` per invocation. The app only builds
//! this text; it never starts Claude Code or anything else for it. Read only.

use crate::pw_export::clone::ClonedRepo;
use crate::pw_repo::presence::{self, INDEX_REL};
use crate::pw_repo::sync::ExportRecord;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;

/// What a typed spec stem must look like, said in the refusal.
pub const STEM_RULE: &str = "lower-case letters, digits and hyphens, starting with a letter or digit";

/// The person's spec choice for one `<seg>`: `stem` is the spec to put the
/// cases in, or `None` for one spec per case. A `<seg>` with no choice gets
/// `default_stem`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, specta::Type)]
pub struct HandoffChoice {
    pub seg: String,
    pub stem: Option<String>,
}

/// One prompt: the cases of one `<seg>`, in the order they were asked for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct HandoffCard {
    pub seg: String,
    pub case_ids: Vec<i32>,
    /// The stems of the feature's `specs/*.spec.ts` that `valid_stem` allows, sorted.
    pub stems: Vec<String>,
    /// The spec the prompt names, or `None` for one spec per case.
    pub stem: Option<String>,
    pub prompt: String,
}

/// Whether `stem` is a spec name the prompt may carry: `^[a-z0-9][a-z0-9-]*$`.
pub fn valid_stem(stem: &str) -> bool {
    let mut bytes = stem.bytes();
    matches!(bytes.next(), Some(b) if b.is_ascii_lowercase() || b.is_ascii_digit())
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The default: the feature's one spec when it has exactly one, else one
/// spec per case.
pub fn default_stem(stems: &[String]) -> Option<String> {
    match stems {
        [only] => Some(only.clone()),
        _ => None,
    }
}

/// "101", "101 and 102", "101, 102 and 103".
fn and_list(items: &[String]) -> String {
    match items.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => items.join(""),
    }
}

fn ticks(paths: &[String]) -> String {
    paths.iter().map(|p| format!("`{p}`")).collect::<Vec<_>>().join(", ")
}

/// The prompt for the cases `ids` of `seg`: `raw_specs` and `sections` are
/// relative to the clone, forward slashes; `stem` is the spec to append to,
/// or `None` for one spec per case. Pure: the values are checked by `cards`.
pub fn prompt(seg: &str, ids: &[i32], raw_specs: &[String], sections: &[String], stem: Option<&str>) -> String {
    let id_list: Vec<String> = ids.iter().map(i32::to_string).collect();
    let several = ids.len() > 1;
    let cases = if several { "cases" } else { "case" };
    let whose = if several { "each case's" } else { "the case's" };
    let section = if sections.len() > 1 { "test-case sections" } else { "test-case section" };
    let spec_line = match stem {
        Some(stem) => format!(
            "Put {} in `suites/{seg}/specs/{stem}.spec.ts` (append; the --spec contract).",
            if several { "them" } else { "it" }
        ),
        None => "One spec per case (no --spec).".to_string(),
    };
    let step4 = if several {
        "In step 4, run those raw specs and heal them in place if they fail; never regenerate them from the tickets."
    } else {
        "In step 4, run that raw spec and heal it in place if it fails; never regenerate it from the ticket."
    };
    format!(
        "Run steps 4, 5, 6 and 7 of .claude/commands/gen-test.md for ADO {cases} {} in {seg}. \
         Steps 1-3 are already done: the Test Case Manager exported {whose} raw spec ({}) and its {section} ({}). \
         {step4} \
         Do not fetch the tickets and do not run git. \
         {spec_line} \
         The tests are independent (no --chain). \
         Stop and report on any stop-and-report condition the command defines.",
        and_list(&id_list),
        ticks(raw_specs),
        ticks(sections),
    )
}

/// A name the prompt can carry as it is: no backtick, no control character.
fn carriable(text: &str) -> bool {
    !text.is_empty() && !text.chars().any(|c| c == '`' || c.is_control())
}

/// `<seg>` of a test-case file `suites/<seg>/test-cases/<name>.md`:
/// `<country>/<side>/<module>/<feature>`.
fn seg_of(md: &str) -> Option<String> {
    let rest = md.strip_prefix("suites/")?;
    let (seg, name) = rest.rsplit_once("/test-cases/")?;
    let parts: Vec<&str> = seg.split('/').collect();
    (parts.len() == 4 && parts.iter().all(|p| !p.is_empty()) && !name.contains('/')).then(|| seg.to_string())
}

/// The stems of `suites/<seg>/specs/*.spec.ts` that `valid_stem` allows, sorted.
pub fn stems_of(root: &Path, seg: &str) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(root.join("suites").join(seg).join("specs"))
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().is_file())
                .filter_map(|e| e.file_name().to_string_lossy().strip_suffix(".spec.ts").map(str::to_string))
                .filter(|s| valid_stem(s))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// One case, checked: its `<seg>`, raw spec and section, relative to the clone.
struct Checked {
    id: i32,
    seg: String,
    raw: String,
    section: String,
}

/// Case `id` is one this app exported (`written` by an export this run, or
/// on the Sync `record`) and the clone still has its raw spec (an
/// `index.json` entry whose file is there) and one section; else why not.
fn check(repo: &ClonedRepo, found: &presence::Presence, record: &ExportRecord, written: &BTreeSet<i32>, id: i32) -> Result<Checked, String> {
    if !written.contains(&id) && !record.cases.contains_key(&id) {
        return Err(format!("Case {id} was not exported by this app, so there is nothing to finish for it here."));
    }
    let key = id.to_string();
    let file = repo
        .index
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, f)| f.clone())
        .ok_or_else(|| format!("Case {id} has no entry in {INDEX_REL}. Export it again first."))?;
    if !repo.generated_files.contains(&file) {
        return Err(format!("Case {id}'s raw spec suites/_generated/{file} is not in the clone. Export it again first."));
    }
    if found.excluded(id) {
        return Err(format!("Case {id} is excluded in the repo, so it is not finished there."));
    }
    let mds: Vec<&String> = found.places(id).iter().filter(|p| p.contains("/test-cases/")).collect();
    let section = match mds.as_slice() {
        [] => return Err(format!("Case {id} has no section in a test-case file. Export it again first.")),
        [one] => (*one).clone(),
        several => {
            // The file this app wrote it to, when the record says which.
            let recorded = record.cases.get(&id).map(|e| e.files.as_slice()).unwrap_or(&[]);
            match several.iter().find(|p| recorded.contains(p)) {
                Some(p) => (*p).clone(),
                None => {
                    let names: Vec<String> = several.iter().map(|p| p.to_string()).collect();
                    return Err(format!("Case {id} has a section in more than one test-case file: {}.", names.join(", ")));
                }
            }
        }
    };
    let raw = format!("suites/_generated/{file}");
    let seg = seg_of(&section).ok_or_else(|| format!("Case {id}'s test-case file {section} is not in a feature folder."))?;
    if !carriable(&raw) || !carriable(&section) {
        return Err(format!("Case {id}'s files have a name the prompt cannot carry."));
    }
    Ok(Checked { id, seg, raw, section })
}

/// The prompts for `case_ids` in the clone `repo` (already opened), one per
/// `<seg>` in the order its first case was asked for. Refuses the whole
/// lot, with a sentence, if any case or chosen stem does not check out.
pub fn cards(
    repo: &ClonedRepo,
    record: &ExportRecord,
    written: &BTreeSet<i32>,
    case_ids: &[i32],
    choices: &[HandoffChoice],
) -> Result<Vec<HandoffCard>, String> {
    if case_ids.is_empty() {
        return Err("There are no exported cases to finish.".into());
    }
    let found = presence::scan_repo(repo);
    let mut groups: Vec<(String, Vec<Checked>)> = Vec::new();
    let mut seen: BTreeSet<i32> = BTreeSet::new();
    for &id in case_ids {
        if !seen.insert(id) {
            continue;
        }
        let c = check(repo, &found, record, written, id)?;
        match groups.iter_mut().find(|(s, _)| *s == c.seg) {
            Some((_, list)) => list.push(c),
            None => groups.push((c.seg.clone(), vec![c])),
        }
    }

    let mut out = Vec::with_capacity(groups.len());
    for (seg, list) in groups {
        let stems = stems_of(&repo.root, &seg);
        let stem = match choices.iter().find(|c| c.seg == seg) {
            Some(HandoffChoice { stem: Some(s), .. }) => {
                let s = s.trim();
                if !valid_stem(s) {
                    return Err(format!("\"{s}\" cannot be a spec name: use {STEM_RULE}."));
                }
                Some(s.to_string())
            }
            Some(HandoffChoice { stem: None, .. }) => None,
            None => default_stem(&stems),
        };
        let ids: Vec<i32> = list.iter().map(|c| c.id).collect();
        let raw: Vec<String> = list.iter().map(|c| c.raw.clone()).collect();
        let mut sections: Vec<String> = Vec::new();
        for c in &list {
            if !sections.contains(&c.section) {
                sections.push(c.section.clone());
            }
        }
        let prompt = prompt(&seg, &ids, &raw, &sections, stem.as_deref());
        out.push(HandoffCard { seg, case_ids: ids, stems, stem, prompt });
    }
    Ok(out)
}

/// The clone at `root`, opened, then `cards`.
pub fn cards_at(
    root: &Path,
    record: &ExportRecord,
    written: &BTreeSet<i32>,
    case_ids: &[i32],
    choices: &[HandoffChoice],
) -> Result<Vec<HandoffCard>, String> {
    cards(&crate::pw_export::clone::open(root)?, record, written, case_ids, choices)
}
