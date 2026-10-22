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
