//! Per-project quirks: short, attributed notes about the application that
//! a person or an assistant has learned the hard way, so the next script -
//! written by either - does not rediscover the same surprise.
//!
//! Saved beside the project's sign-in recipe, under its own file: the two
//! are edited from the same dialog but saved by separate commands, so a
//! recipe the app refuses never takes an already-typed quirks list down
//! with it.

use super::recipe::project_slug;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Quirk {
    pub text: String,
    /// "person" or "assistant"
    pub by: String,
    /// Epoch milliseconds as a string.
    pub at: String,
}

/// How many quirks a project keeps. Past this, the oldest ones are no
/// longer worth the reading time - a person is expected to prune the list
/// rather than let it grow without bound.
pub const MAX_QUIRKS: usize = 40;

/// One quirk is one line, read at a glance.
pub const MAX_QUIRK_CHARS: usize = 300;

pub fn quirks_path(root: &Path, org: &str, project: &str) -> PathBuf {
    root.join("projects").join(format!("{}-quirks.json", project_slug(org, project)))
}

pub fn load_quirks(root: &Path, org: &str, project: &str) -> Result<Vec<Quirk>, String> {
    match std::fs::read_to_string(quirks_path(root, org, project)) {
        Ok(s) => {
            let s = s.strip_prefix('\u{feff}').unwrap_or(&s);
            serde_json::from_str(s).map_err(|e| format!("the project quirks file is not readable: {e}"))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(e) => Err(e.to_string()),
    }
}

fn validate(quirks: &[Quirk]) -> Result<(), String> {
    if quirks.len() > MAX_QUIRKS {
        return Err(format!(
            "a project keeps at most {MAX_QUIRKS} quirks - remove one before adding another"
        ));
    }
    let mut seen: Vec<String> = Vec::new();
    for q in quirks {
        if q.text.trim().is_empty() {
            return Err("a quirk needs some text".to_string());
        }
        if q.text.chars().count() > MAX_QUIRK_CHARS {
            return Err(format!("a quirk is at most {MAX_QUIRK_CHARS} characters"));
        }
        if q.text.contains('\n') {
            return Err("a quirk is one line - remove the line break".to_string());
        }
        // Case- and whitespace-insensitive, the same rule `add_quirk` uses
        // to skip a repeat - a two-line box saved straight through must not
        // be able to write the fact twice.
        let norm = normalized(q.text.trim());
        if seen.contains(&norm) {
            return Err(format!("quirk \"{}\" appears twice", q.text.trim()));
        }
        seen.push(norm);
    }
    Ok(())
}

/// Replaces the whole list. Validated first, then written to a temporary
/// file and renamed, so a reader never sees a half-written list and a
/// refused save leaves the earlier one exactly as it was.
pub fn save_quirks(root: &Path, org: &str, project: &str, quirks: &[Quirk]) -> Result<(), String> {
    validate(quirks)?;
    // `slug_part` (behind `project_slug`, via `quirks_path`) never reads as
    // empty any more, so - as with `recipe::save_recipe` - the real refusal,
    // nothing was typed at all, has to be checked on the raw names here.
    if org.trim().is_empty() || project.trim().is_empty() {
        return Err("pick an organization and a project first".to_string());
    }
    let path = quirks_path(root, org, project);
    std::fs::create_dir_all(path.parent().expect("projects folder")).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(quirks).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.to_string());
    }
    Ok(())
}

/// Collapse runs of whitespace to a single space and lower-case, so "the
/// grid  paginates" and "THE GRID PAGINATES" compare equal regardless of
/// case or how the spacing landed.
fn normalized(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Appends one quirk, unless an equal one (case- and whitespace-insensitive)
/// is already on the list, in which case nothing is written and this
/// returns `false`.
pub fn add_quirk(
    root: &Path,
    org: &str,
    project: &str,
    text: &str,
    by: &str,
    now_ms: u64,
) -> Result<bool, String> {
    let mut existing = load_quirks(root, org, project)?;
    let trimmed = text.trim();
    if existing.iter().any(|q| normalized(&q.text) == normalized(trimmed)) {
        return Ok(false);
    }
    existing.push(Quirk { text: trimmed.to_string(), by: by.to_string(), at: now_ms.to_string() });
    save_quirks(root, org, project, &existing)?;
    Ok(true)
}

/// A Markdown section for an assistant's guide: the running list, or
/// nothing at all when there is nothing to say yet. Each line is
/// attributed - `by` is never shown as-is (a raw "person" or "assistant"
/// reads oddly mid-sentence), so an assistant reading its own guide can
/// tell which quirks it recorded itself and which a person already knew.
pub fn quirks_section(quirks: &[Quirk]) -> String {
    if quirks.is_empty() {
        return String::new();
    }
    let mut out = String::from("## Known quirks of this application\n\n");
    for q in quirks {
        let attribution = if q.by == "assistant" { "recorded by the assistant" } else { "recorded by you" };
        out.push_str(&format!("- {} ({attribution})\n", q.text));
    }
    out
}
