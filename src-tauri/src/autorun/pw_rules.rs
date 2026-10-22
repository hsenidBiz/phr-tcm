//! The team's Playwright repo as the source of the scripting rules.
//!
//! A rule file (an agent file, the `gen-test` command, a skill, the repo's
//! own `CLAUDE.md`) is read fresh from the clone set on the Playwright tab
//! every time it is wanted. When the clone is not there, or does not hold
//! the file, the copy compiled in from `src-tauri/pw-rules/` is used
//! instead, and the answer says which it was so the text handed to a model
//! can say so in its first line.
//!
//! This module only reads. It never writes to the clone, never runs git
//! there, and never filters what the repo's files say.

use std::path::{Component, Path};

/// The six agent files the roles read, as the repo names them in `.claude/agents/`.
pub const AGENT_FILES: [&str; 6] = [
    "playwright-test-planner.md",
    "playwright-test-generator.md",
    "playwright-test-healer.md",
    "spec-editor.md",
    "test-refactorer.md",
    "doc-drift-auditor.md",
];
/// The repo's `gen-test` command, in `.claude/commands/`.
pub const GEN_TEST: &str = "gen-test.md";
/// The repo's own `CLAUDE.md`, at the root of the clone.
pub const REPO_CLAUDE: &str = "CLAUDE.md";
/// The largest rule file read from a clone, in bytes. A bigger one is not a rule file.
pub const READ_LIMIT: u64 = 262_144;
/// How TCM names the compiled-in copy to a person.
pub const SHIPPED_COPY: &str = "the copy shipped with this version of Test Case Manager";
/// The mark on a section whose file was read from the clone.
pub const LIVE_MARK: &str = "read from the repo just now";
/// The mark on a section whose file is the compiled-in copy.
pub const SHIPPED_MARK: &str = "the copy shipped with this version";

/// (path as the repo lays it out, contents): every snapshot file, through
/// `include_str!`. The snapshot folder carries no `.claude` folder and no
/// file named `CLAUDE.md`, so Claude Code working in this repository never
/// loads them as its own agents or instructions.
pub const SNAPSHOT: &[(&str, &str)] = &[
    (
        ".claude/agents/playwright-test-planner.md",
        include_str!("../../pw-rules/agents/playwright-test-planner.md"),
    ),
    (
        ".claude/agents/playwright-test-generator.md",
        include_str!("../../pw-rules/agents/playwright-test-generator.md"),
    ),
    (
        ".claude/agents/playwright-test-healer.md",
        include_str!("../../pw-rules/agents/playwright-test-healer.md"),
    ),
    (
        ".claude/agents/spec-editor.md",
        include_str!("../../pw-rules/agents/spec-editor.md"),
    ),
    (
        ".claude/agents/test-refactorer.md",
        include_str!("../../pw-rules/agents/test-refactorer.md"),
    ),
    (
        ".claude/agents/doc-drift-auditor.md",
        include_str!("../../pw-rules/agents/doc-drift-auditor.md"),
    ),
    (
        ".claude/commands/gen-test.md",
        include_str!("../../pw-rules/commands/gen-test.md"),
    ),
    (
        ".claude/skills/edit-spec/SKILL.md",
        include_str!("../../pw-rules/skills/edit-spec/SKILL.md"),
    ),
    (
        ".claude/skills/manage-test-users/SKILL.md",
        include_str!("../../pw-rules/skills/manage-test-users/SKILL.md"),
    ),
    (
        ".claude/skills/test-data/SKILL.md",
        include_str!("../../pw-rules/skills/test-data/SKILL.md"),
    ),
    ("CLAUDE.md", include_str!("../../pw-rules/CLAUDE.repo.md")),
];

/// The snapshot's copy of `rel` (a key of [`SNAPSHOT`]), exactly as compiled in.
pub fn snapshot(rel: &str) -> Option<&'static str> {
    SNAPSHOT.iter().find(|(k, _)| *k == rel).map(|(_, v)| *v)
}

/// `.claude/agents/<n>` for an [`AGENT_FILES`] name, `.claude/commands/gen-test.md`
/// for [`GEN_TEST`], `CLAUDE.md` for [`REPO_CLAUDE`]; `None` for any other name.
pub fn repo_path(name: &str) -> Option<&'static str> {
    if AGENT_FILES.contains(&name) {
        return SNAPSHOT
            .iter()
            .map(|(k, _)| *k)
            .find(|k| k.strip_prefix(".claude/agents/") == Some(name));
    }
    if name == GEN_TEST {
        return Some(".claude/commands/gen-test.md");
    }
    if name == REPO_CLAUDE {
        return Some("CLAUDE.md");
    }
    None
}

/// Why a file is the shipped copy and not the clone's.
#[derive(Debug, Clone, PartialEq)]
pub enum Why {
    /// No Playwright repo is set.
    NoClone,
    /// A folder is set but cannot be read as one.
    CloneUnreadable,
    /// The clone is a folder but holds no usable copy of the file.
    NotInClone,
    /// Test Case Manager (the app) did not answer, so the clone was not read.
    NotRead,
}

/// Where a file's text came from.
#[derive(Debug, Clone, PartialEq)]
pub enum From {
    /// Read from the clone just now.
    Live,
    /// The compiled-in copy, and why.
    Shipped(Why),
}

/// One rule file, read.
#[derive(Debug, Clone, PartialEq)]
pub struct RepoFile {
    /// The file's name, or `<skill>/SKILL.md` for a skill. Never a path.
    pub label: String,
    /// BOM dropped, `\r\n` read as `\n`, frontmatter kept.
    pub text: String,
    pub from: From,
}

/// A path as the repo lays it out and nothing else: no empty or `.`/`..`
/// part, no drive, no backslash, no leading slash.
fn plain_rel(rel: &str) -> bool {
    !rel.is_empty()
        && !rel.contains('\\')
        && !rel.contains(':')
        && Path::new(rel).components().all(|c| matches!(c, Component::Normal(_)))
        && rel.split('/').all(|p| !p.is_empty())
}

fn label_of(rel: &str) -> String {
    let parts: Vec<&str> = rel.split('/').collect();
    match parts.as_slice() {
        [.., skill, "SKILL.md"] => format!("{skill}/SKILL.md"),
        [.., last] => (*last).to_string(),
        [] => String::new(),
    }
}

/// BOM dropped, `\r\n` read as `\n`.
fn normalise(text: &str) -> String {
    text.strip_prefix('\u{feff}').unwrap_or(text).replace("\r\n", "\n")
}

/// `rel` from the clone as a text, or why not.
fn read_live(clone: &str, rel: &str) -> Result<String, Why> {
    let clone = clone.trim();
    if clone.is_empty() {
        return Err(Why::NoClone);
    }
    let root = Path::new(clone);
    if !root.is_dir() {
        return Err(Why::CloneUnreadable);
    }
    let path = root.join(rel);
    let meta = std::fs::metadata(&path).map_err(|_| Why::NotInClone)?;
    if !meta.is_file() || meta.len() > READ_LIMIT {
        return Err(Why::NotInClone);
    }
    let bytes = std::fs::read(&path).map_err(|_| Why::NotInClone)?;
    String::from_utf8(bytes).map(|t| normalise(&t)).map_err(|_| Why::NotInClone)
}

/// `rel` read now: `Live` when `clone` (trimmed) is a folder holding `rel`
/// as a regular file of at most [`READ_LIMIT`] bytes that reads as UTF-8;
/// otherwise the snapshot's copy with `NoClone` (clone empty),
/// `CloneUnreadable` (clone set but not a folder) or `NotInClone` (anything
/// else); `None` when the snapshot has no such file either, or `rel` is not
/// a plain path as the repo lays it out.
pub fn read(clone: &str, rel: &str) -> Option<RepoFile> {
    if !plain_rel(rel) {
        return None;
    }
    match read_live(clone, rel) {
        Ok(text) => Some(RepoFile { label: label_of(rel), text, from: From::Live }),
        Err(why) => shipped(rel, why),
    }
}

/// The snapshot's copy of `rel` marked `Shipped(why)`, or `None`.
pub fn shipped(rel: &str, why: Why) -> Option<RepoFile> {
    let text = normalise(snapshot(rel)?);
    Some(RepoFile { label: label_of(rel), text, from: From::Shipped(why) })
}

/// The frontmatter lines of `text` (BOM and `\r` ignored) and the lines after
/// it, when it opens with a `---` line and a later line is exactly `---`.
fn split_frontmatter(text: &str) -> Option<(Vec<&str>, Vec<&str>)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines: Vec<&str> = text.split('\n').collect();
    let bare = |l: &str| l.strip_suffix('\r').unwrap_or(l).to_string();
    if bare(lines.first()?) != "---" {
        return None;
    }
    let close = lines.iter().skip(1).position(|l| bare(l) == "---")? + 1;
    Some((lines[1..close].to_vec(), lines[close + 1..].to_vec()))
}

/// `text` without a leading `---` line through the next line that is exactly
/// `---` (BOM and `\r` ignored), leading blank lines dropped; unchanged when
/// it has no closed frontmatter.
pub fn strip_frontmatter(text: &str) -> String {
    let Some((_, rest)) = split_frontmatter(text) else {
        return text.to_string();
    };
    let first = rest.iter().position(|l| !l.trim().is_empty()).unwrap_or(rest.len());
    rest[first..].join("\n").replace("\r\n", "\n").replace('\r', "")
}

/// A `key: value` line of the frontmatter, value trimmed and unquoted
/// (`'...'` or `"..."`); `None` without one.
pub fn frontmatter_value(text: &str, key: &str) -> Option<String> {
    let (front, _) = split_frontmatter(text)?;
    for line in front {
        let Some((k, v)) = line.split_once(':') else { continue };
        if k.trim() != key {
            continue;
        }
        let v = v.trim();
        let unquoted = [('\'', '\''), ('"', '"')]
            .iter()
            .find_map(|(a, b)| v.strip_prefix(*a).and_then(|r| r.strip_suffix(*b)))
            .unwrap_or(v);
        return Some(unquoted.to_string());
    }
    None
}

/// `## From the Playwright repo: {label}, {LIVE_MARK | SHIPPED_MARK}`, a blank
/// line, and the body (frontmatter stripped, trimmed).
pub fn section(file: &RepoFile) -> String {
    let mark = match file.from {
        From::Live => LIVE_MARK,
        From::Shipped(_) => SHIPPED_MARK,
    };
    format!(
        "## From the Playwright repo: {}, {mark}\n\n{}",
        file.label,
        strip_frontmatter(&file.text).trim()
    )
}

/// The first line to put above the repo's files when any came from the
/// snapshot. `labels` names the shipped files (used for `NotInClone`).
pub fn shipped_line(why: &Why, labels: &[String]) -> String {
    match why {
        Why::NoClone => format!(
            "(The Playwright repo's files below are {SHIPPED_COPY}: no Playwright repo is set on the Playwright tab.)"
        ),
        Why::CloneUnreadable => format!(
            "(The Playwright repo's files below are {SHIPPED_COPY}: the folder set on the Playwright tab cannot be read.)"
        ),
        Why::NotInClone => format!(
            "(Some of the Playwright repo's files below are {SHIPPED_COPY}: the Playwright repo set on the Playwright tab has no readable {}.)",
            labels.join(", ")
        ),
        Why::NotRead => format!(
            "(The Playwright repo's files below are {SHIPPED_COPY}: Test Case Manager did not answer, so the Playwright repo was not read.)"
        ),
    }
}

/// The files `get_guide` topic `playwright_repo` gives, by the names the repo
/// has them: the repo's `CLAUDE.md`, the `gen-test` command, then the agent files.
pub const REPO_FILES: [&str; 8] = [
    "CLAUDE.md",
    "gen-test.md",
    "playwright-test-planner.md",
    "playwright-test-generator.md",
    "playwright-test-healer.md",
    "spec-editor.md",
    "test-refactorer.md",
    "doc-drift-auditor.md",
];

const SKILLS_DIR: &str = ".claude/skills";
const SKILL_FILE: &str = "SKILL.md";

/// A skill folder's name: letters, digits, `_` and `-` and nothing else, so a
/// name is never a path.
fn plain_skill_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn skill_rel(name: &str) -> String {
    format!("{SKILLS_DIR}/{name}/{SKILL_FILE}")
}

/// Sorted, each once: the clone's `.claude/skills/<dir>/SKILL.md` folders (when
/// the clone is a folder) and the snapshot's skills, those that read as text.
/// Only names of `[A-Za-z0-9_-]+`.
pub fn skill_names(clone: &str) -> Vec<String> {
    let mut names: Vec<String> = SNAPSHOT
        .iter()
        .filter_map(|(k, _)| k.strip_prefix(".claude/skills/")?.strip_suffix("/SKILL.md"))
        .map(str::to_string)
        .collect();
    let root = Path::new(clone.trim());
    if !clone.trim().is_empty() && root.is_dir() {
        if let Ok(entries) = std::fs::read_dir(root.join(SKILLS_DIR)) {
            for e in entries.flatten() {
                let Some(name) = e.file_name().to_str().map(str::to_string) else { continue };
                if plain_skill_name(&name) && e.path().is_dir() {
                    names.push(name);
                }
            }
        }
    }
    names.sort();
    names.dedup();
    names.retain(|n| plain_skill_name(n) && read(clone, &skill_rel(n)).is_some());
    names
}

/// What to remember when using the repo's own files in Test Case Manager.
pub fn repo_note() -> String {
    include_str!("roles/repo-tools.md").replace("\r\n", "\n")
}

/// What to remember when following the repo's `gen-test` command in Test Case Manager.
pub fn gen_test_note() -> String {
    include_str!("roles/gen-test-tools.md").replace("\r\n", "\n")
}

/// `"a", "b" and "c"`.
fn quoted_list(names: &[String]) -> String {
    let q: Vec<String> = names.iter().map(|n| format!("\"{n}\"")).collect();
    match q.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

/// Said when `skill` names no skill.
pub fn unknown_skill(asked: &str, names: &[String]) -> String {
    format!("There is no Playwright skill \"{}\": `skill` is one of {}.", asked.trim(), quoted_list(names))
}

/// Said when `file` names none of [`REPO_FILES`].
pub fn unknown_repo_file(asked: &str) -> String {
    let names: Vec<String> = REPO_FILES.iter().map(|n| (*n).to_string()).collect();
    format!("There is no Playwright repo file \"{}\": `file` is one of {}.", asked.trim(), quoted_list(&names))
}

/// The shipped line for one file read, followed by a blank line; empty when it was read live.
fn shipped_prefix(file: &RepoFile) -> String {
    match &file.from {
        From::Live => String::new(),
        From::Shipped(why) => format!("{}\n\n", shipped_line(why, &[file.label.clone()])),
    }
}

/// `None` or `""`: (200, the list). A name in [`skill_names`] (exact): (200,
/// [shipped line, blank line,] the skill's section, blank line, [`repo_note`]).
/// Anything else: (404, [`unknown_skill`]). A name is never joined to a path
/// before it matched a name from the list.
pub fn skills_answer(clone: &str, skill: Option<&str>) -> (u16, String) {
    let names = skill_names(clone);
    let asked = skill.unwrap_or("");
    if asked.is_empty() {
        let mut out = String::new();
        let root = Path::new(clone.trim());
        if clone.trim().is_empty() {
            out.push_str(&shipped_line(&Why::NoClone, &[]));
            out.push_str("\n\n");
        } else if !root.is_dir() {
            out.push_str(&shipped_line(&Why::CloneUnreadable, &[]));
            out.push_str("\n\n");
        }
        out.push_str("The Playwright repo's skills:\n");
        for n in &names {
            let Some(file) = read(clone, &skill_rel(n)) else { continue };
            let mark = match file.from {
                From::Live => LIVE_MARK,
                From::Shipped(_) => SHIPPED_MARK,
            };
            let description = frontmatter_value(&file.text, "description")
                .filter(|d| !d.is_empty())
                .unwrap_or_else(|| "no description".to_string());
            out.push_str(&format!("- {n}: {description} ({mark})\n"));
        }
        out.push_str("Ask for one with `get_guide`, topic playwright_skills and `skill` its name. In Test Case Manager a skill's steps are done with Test Case Manager's tools; never edit the Playwright clone.\n");
        return (200, out);
    }
    let Some(name) = names.iter().find(|n| n.as_str() == asked) else {
        return (404, unknown_skill(asked, &names));
    };
    match read(clone, &skill_rel(name)) {
        Some(file) => (200, format!("{}{}\n\n{}", shipped_prefix(&file), section(&file), repo_note().trim_end()) + "\n"),
        None => (404, unknown_skill(asked, &names)),
    }
}

/// `None` or `""`: the repo's `CLAUDE.md`. A [`REPO_FILES`] name (exact): (200,
/// [shipped line, blank line,] the section, blank line, the note: [`gen_test_note`]
/// for `gen-test.md`, [`repo_note`] for the rest). Anything else: (404,
/// [`unknown_repo_file`]).
pub fn repo_answer(clone: &str, file: Option<&str>) -> (u16, String) {
    let asked = file.filter(|f| !f.is_empty()).unwrap_or(REPO_CLAUDE);
    let found = REPO_FILES
        .iter()
        .find(|n| **n == asked)
        .and_then(|n| repo_path(n).and_then(|rel| read(clone, rel)).map(|f| (*n, f)));
    let Some((name, file)) = found else {
        return (404, unknown_repo_file(asked));
    };
    let note = if name == GEN_TEST { gen_test_note() } else { repo_note() };
    (200, format!("{}{}\n\n{}", shipped_prefix(&file), section(&file), note.trim_end()) + "\n")
}
