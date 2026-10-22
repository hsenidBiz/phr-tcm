//! The Playwright repo's rule files: read fresh from the clone, with the
//! compiled-in snapshot (and the line that says so) when the clone is not there.

use std::collections::BTreeSet;
use std::path::Path;
use v2_lib::autorun::pw_rules::{
    frontmatter_value, read, repo_path, section, shipped, shipped_line, snapshot, strip_frontmatter, From, RepoFile,
    Why, AGENT_FILES, GEN_TEST, READ_LIMIT, REPO_CLAUDE, SHIPPED_COPY, SNAPSHOT,
};

const PLANNER: &str = ".claude/agents/playwright-test-planner.md";

/// A fake clone: a temp folder holding `.claude/agents/`.
fn clone() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".claude/agents")).unwrap();
    dir
}

fn path_of(dir: &tempfile::TempDir) -> String {
    dir.path().to_string_lossy().into_owned()
}

fn normalised(s: &str) -> String {
    s.strip_prefix('\u{feff}').unwrap_or(s).replace("\r\n", "\n")
}

#[test]
fn the_snapshot_holds_every_file_the_roles_read() {
    for n in AGENT_FILES.iter().chain([&GEN_TEST, &REPO_CLAUDE]) {
        let rel = repo_path(n).unwrap_or_else(|| panic!("no repo path for {n}"));
        let text = snapshot(rel).unwrap_or_else(|| panic!("the snapshot lacks {rel}"));
        assert!(!text.trim().is_empty(), "{rel} is empty");
    }
    for skill in ["edit-spec", "manage-test-users", "test-data"] {
        assert!(snapshot(&format!(".claude/skills/{skill}/SKILL.md")).is_some(), "no {skill}");
    }
    assert_eq!(repo_path("spec-editor.md"), Some(".claude/agents/spec-editor.md"));
    assert_eq!(repo_path(GEN_TEST), Some(".claude/commands/gen-test.md"));
    assert_eq!(repo_path(REPO_CLAUDE), Some("CLAUDE.md"));
    assert_eq!(repo_path("nope.md"), None);
    assert_eq!(repo_path("../CLAUDE.md"), None);
}

fn files_under(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            assert_ne!(p.file_name().unwrap(), ".claude", "the snapshot must carry no .claude folder");
            files_under(&p, out);
        } else {
            out.push(p);
        }
    }
}

#[test]
fn the_snapshot_folder_holds_exactly_the_shipped_files() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("pw-rules");
    let mut found = Vec::new();
    files_under(&root, &mut found);
    let mut on_disk = BTreeSet::new();
    for p in found {
        let rel = p.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        assert_ne!(p.file_name().unwrap(), "CLAUDE.md", "{rel} would be loaded as Claude Code's own instructions");
        let key = if rel == "CLAUDE.repo.md" {
            "CLAUDE.md".to_string()
        } else if let Some(r) = rel.strip_prefix("agents/") {
            format!(".claude/agents/{r}")
        } else if let Some(r) = rel.strip_prefix("commands/") {
            format!(".claude/commands/{r}")
        } else if let Some(r) = rel.strip_prefix("skills/") {
            format!(".claude/skills/{r}")
        } else {
            panic!("an unexpected file in the snapshot folder: {rel}")
        };
        on_disk.insert(key);
    }
    let compiled: BTreeSet<String> = SNAPSHOT.iter().map(|(k, _)| k.to_string()).collect();
    assert_eq!(on_disk, compiled);
}

#[test]
fn a_live_file_is_read_fresh_each_time() {
    let dir = clone();
    let file = dir.path().join(PLANNER);
    std::fs::write(&file, "one").unwrap();
    let got = read(&path_of(&dir), PLANNER);
    assert_eq!(
        got,
        Some(RepoFile { label: "playwright-test-planner.md".into(), text: "one".into(), from: From::Live })
    );
    std::fs::write(&file, "two").unwrap();
    assert_eq!(read(&path_of(&dir), PLANNER).unwrap().text, "two");
    // The clone setting is trimmed, and a BOM and CRLF are read away.
    std::fs::write(&file, "\u{feff}a\r\nb\r\n").unwrap();
    let padded = format!("  {}  ", path_of(&dir));
    let got = read(&padded, PLANNER).unwrap();
    assert_eq!((got.text.as_str(), got.from), ("a\nb\n", From::Live));
}

#[test]
fn a_skill_is_labelled_by_its_folder() {
    let dir = clone();
    std::fs::create_dir_all(dir.path().join(".claude/skills/edit-spec")).unwrap();
    std::fs::write(dir.path().join(".claude/skills/edit-spec/SKILL.md"), "skill").unwrap();
    let got = read(&path_of(&dir), ".claude/skills/edit-spec/SKILL.md").unwrap();
    assert_eq!(got.label, "edit-spec/SKILL.md");
    assert_eq!(got.from, From::Live);
}

#[test]
fn no_clone_reads_the_shipped_copy() {
    let want = normalised(snapshot(PLANNER).unwrap());
    for clone in ["", "  "] {
        let got = read(clone, PLANNER).unwrap();
        assert_eq!(got.from, From::Shipped(Why::NoClone));
        assert_eq!(got.text, want);
        assert_eq!(got.label, "playwright-test-planner.md");
    }
}

#[test]
fn a_clone_folder_that_is_gone_reads_the_shipped_copy() {
    let dir = tempfile::tempdir().unwrap();
    let gone = dir.path().join("not-here").to_string_lossy().into_owned();
    assert_eq!(read(&gone, PLANNER).unwrap().from, From::Shipped(Why::CloneUnreadable));
    // A file where the folder should be is no folder either.
    let file = dir.path().join("a-file");
    std::fs::write(&file, "x").unwrap();
    assert_eq!(read(&file.to_string_lossy(), PLANNER).unwrap().from, From::Shipped(Why::CloneUnreadable));
}

#[test]
fn a_file_missing_from_the_clone_reads_the_shipped_copy() {
    let dir = clone();
    let got = read(&path_of(&dir), PLANNER).unwrap();
    assert_eq!(got.from, From::Shipped(Why::NotInClone));
    assert_eq!(got.text, normalised(snapshot(PLANNER).unwrap()));
}

#[test]
fn an_unreadable_file_reads_the_shipped_copy() {
    let dir = clone();
    let file = dir.path().join(PLANNER);
    let clone_path = path_of(&dir);
    std::fs::write(&file, [0xff, 0xfe, 0x00]).unwrap();
    assert_eq!(read(&clone_path, PLANNER).unwrap().from, From::Shipped(Why::NotInClone));
    std::fs::write(&file, vec![b'a'; READ_LIMIT as usize + 1]).unwrap();
    assert_eq!(read(&clone_path, PLANNER).unwrap().from, From::Shipped(Why::NotInClone));
    // At the limit it is still read.
    std::fs::write(&file, vec![b'a'; READ_LIMIT as usize]).unwrap();
    assert_eq!(read(&clone_path, PLANNER).unwrap().from, From::Live);
    std::fs::remove_file(&file).unwrap();
    std::fs::create_dir(&file).unwrap();
    assert_eq!(read(&clone_path, PLANNER).unwrap().from, From::Shipped(Why::NotInClone));
}

#[test]
fn a_file_the_snapshot_lacks_is_none_when_the_clone_lacks_it_too() {
    let dir = clone();
    assert_eq!(read(&path_of(&dir), ".claude/agents/other.md"), None);
    assert_eq!(read("", ".claude/agents/other.md"), None);
    assert_eq!(shipped(".claude/agents/other.md", Why::NotRead), None);
    let got = shipped(PLANNER, Why::NotRead).unwrap();
    assert_eq!(got.from, From::Shipped(Why::NotRead));
}

#[test]
fn a_path_that_leaves_the_repo_layout_is_never_read() {
    let dir = clone();
    std::fs::write(dir.path().join("secret.md"), "secret").unwrap();
    let clone_path = path_of(&dir);
    for rel in [
        "../secret.md",
        ".claude/../secret.md",
        ".claude/agents/../../secret.md",
        "..\\secret.md",
        "C:\\secret.md",
        "/secret.md",
        ".claude//agents/x.md",
        "",
    ] {
        assert_eq!(read(&clone_path, rel), None, "{rel:?} must not be read");
    }
}

#[test]
fn frontmatter_is_stripped_only_when_closed() {
    assert_eq!(strip_frontmatter("\u{feff}---\r\nname: x\r\ntools: a\r\n---\r\n\r\nBody\r\n"), "Body\n");
    assert_eq!(strip_frontmatter("---\nname: x\nBody\n"), "---\nname: x\nBody\n");
    assert_eq!(strip_frontmatter("Body only\n"), "Body only\n");
    assert_eq!(
        frontmatter_value("---\ndescription: 'Use: this'\n---\n", "description"),
        Some("Use: this".to_string())
    );
    assert_eq!(frontmatter_value("---\nname: \"x y\"\n---\n", "name"), Some("x y".to_string()));
    assert_eq!(frontmatter_value("---\nname: x\n---\n", "tools"), None);
    assert_eq!(frontmatter_value("name: x\n", "name"), None);
    assert_eq!(frontmatter_value("---\nname: x\n", "name"), None);
}

#[test]
fn every_shipped_agent_file_has_frontmatter_that_strips() {
    for n in AGENT_FILES {
        let text = normalised(snapshot(repo_path(n).unwrap()).unwrap());
        if text.starts_with("---\n") {
            let body = strip_frontmatter(&text);
            assert!(!body.starts_with("---"), "{n} still opens with frontmatter");
            assert!(!body.trim().is_empty(), "{n} has no body after its frontmatter");
        }
    }
}

#[test]
fn the_shipped_lines_say_why() {
    for why in [Why::NoClone, Why::CloneUnreadable, Why::NotInClone, Why::NotRead] {
        assert!(shipped_line(&why, &["spec-editor.md".to_string()]).contains(SHIPPED_COPY), "{why:?}");
    }
    assert!(shipped_line(&Why::NoClone, &[]).contains("no Playwright repo is set"));
    assert!(shipped_line(&Why::CloneUnreadable, &[]).contains("cannot be read"));
    assert!(shipped_line(&Why::NotInClone, &["spec-editor.md".to_string()]).contains("has no readable spec-editor.md"));
    assert!(shipped_line(&Why::NotInClone, &["a.md".to_string(), "b/SKILL.md".to_string()])
        .contains("has no readable a.md, b/SKILL.md."));
    assert!(shipped_line(&Why::NotRead, &[]).contains("did not answer"));
    assert_eq!(
        shipped_line(&Why::NoClone, &[]),
        "(The Playwright repo's files below are the copy shipped with this version of Test Case Manager: no Playwright repo is set on the Playwright tab.)"
    );
    for why in [Why::NoClone, Why::CloneUnreadable, Why::NotInClone, Why::NotRead] {
        let line = shipped_line(&why, &["x.md".to_string()]);
        assert!(!line.contains('\u{2014}') && !line.contains('\u{2013}'), "a dash in {line}");
    }
}

#[test]
fn a_section_names_the_file_never_its_path() {
    let dir = clone();
    std::fs::write(
        dir.path().join(PLANNER),
        "---\nname: planner\ntools: a\n---\n\n\nPlan well.\n\nTwice.\n\n",
    )
    .unwrap();
    let clone_path = path_of(&dir);
    let live = section(&read(&clone_path, PLANNER).unwrap());
    assert!(
        live.starts_with("## From the Playwright repo: playwright-test-planner.md, read from the repo just now\n\n"),
        "{live}"
    );
    assert!(!live.contains(&clone_path) && !live.contains(dir.path().to_string_lossy().as_ref()));
    assert!(!live.contains("name: planner") && !live.contains("tools: a"), "{live}");
    assert!(live.ends_with("Plan well.\n\nTwice."), "{live:?}");

    let shipped = section(&read("", PLANNER).unwrap());
    assert!(shipped.starts_with(
        "## From the Playwright repo: playwright-test-planner.md, the copy shipped with this version\n\n"
    ));
}
