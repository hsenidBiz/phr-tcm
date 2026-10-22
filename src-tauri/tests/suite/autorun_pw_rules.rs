//! The Playwright repo's rule files: read fresh from the clone, with the
//! compiled-in snapshot (and the line that says so) when the clone is not there.

use std::collections::BTreeSet;
use std::path::Path;
use v2_lib::autorun::pw_rules::{
    frontmatter_value, read, repo_path, section, shipped, shipped_line, snapshot, strip_frontmatter, From, RepoFile,
    Why, AGENT_FILES, GEN_TEST, READ_LIMIT, REPO_CLAUDE, SHIPPED_COPY, SNAPSHOT,
};
use v2_lib::autorun::pw_rules::{
    gen_test_lead, gen_test_note, repo_answer, repo_answer_page, repo_note, skill_names, skills_answer, unknown_repo_file, unknown_skill,
    LIVE_MARK, PAGE_CHARS, REPO_FILES, SHIPPED_MARK,
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

/// A fake clone holding one skill, `alpha`.
fn clone_with_alpha() -> tempfile::TempDir {
    let dir = clone();
    let skill = dir.path().join(".claude/skills/alpha");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(skill.join("SKILL.md"), "---\nname: alpha\ndescription: Does alpha\n---\nALPHA\n").unwrap();
    dir
}

#[test]
fn the_skills_are_listed_with_their_descriptions() {
    let dir = clone_with_alpha();
    let (status, text) = skills_answer(&path_of(&dir), None);
    assert_eq!(status, 200);
    assert!(text.contains(&format!("- alpha: Does alpha ({LIVE_MARK})")), "{text}");
    let edit = text.lines().find(|l| l.starts_with("- edit-spec: ")).unwrap_or_else(|| panic!("{text}"));
    assert!(edit.ends_with(&format!("({SHIPPED_MARK})")), "{edit}");
    assert!(text.starts_with("The Playwright repo's skills:\n"), "{text}");
    assert!(text.contains("Ask for one with `get_guide`, topic playwright_skills and `skill` its name."), "{text}");
    assert_eq!(skills_answer(&path_of(&dir), Some("")), (200, text.clone()));
    assert_eq!(skill_names(&path_of(&dir)), ["alpha", "edit-spec", "manage-test-users", "test-data"]);
    // Not set: the shipped line first, every skill shipped.
    let (_, none) = skills_answer("", None);
    assert!(none.starts_with(&shipped_line(&Why::NoClone, &[])), "{none}");
    assert!(!none.contains(LIVE_MARK), "{none}");
    let missing = path_of(&dir) + "/nope";
    let (_, gone) = skills_answer(&missing, None);
    assert!(gone.starts_with(&shipped_line(&Why::CloneUnreadable, &[])), "{gone}");
}

#[test]
fn one_skill_is_given_with_the_repo_note() {
    let dir = clone_with_alpha();
    let (status, text) = skills_answer(&path_of(&dir), Some("alpha"));
    assert_eq!(status, 200);
    assert!(text.contains("## From the Playwright repo: alpha/SKILL.md, read from the repo just now"), "{text}");
    assert!(text.contains("ALPHA"), "{text}");
    assert!(text.contains(repo_note().trim_end()), "{text}");
    assert!(!text.contains("name: alpha"), "{text}");
    // A shipped skill says so first.
    let (status, text) = skills_answer(&path_of(&dir), Some("edit-spec"));
    assert_eq!(status, 200);
    assert!(text.starts_with("(Some of the Playwright repo's files below are"), "{text}");
    assert!(text.contains("edit-spec/SKILL.md"), "{text}");
}

#[test]
fn a_skill_name_with_a_path_is_refused() {
    let dir = clone_with_alpha();
    std::fs::write(dir.path().join("CLAUDE.md"), "SECRET ROOT FILE").unwrap();
    let names = skill_names(&path_of(&dir));
    for bad in ["../../CLAUDE", "edit-spec/../x", "C:\\x", "EDIT-SPEC", "alpha/", "../alpha", "alpha/SKILL.md", ".claude"] {
        let (status, text) = skills_answer(&path_of(&dir), Some(bad));
        assert_eq!(status, 404, "{bad}");
        assert_eq!(text, unknown_skill(bad, &names), "{bad}");
        assert!(text.contains("\"edit-spec\""), "{text}");
        assert!(!text.contains("SECRET"), "{text}");
    }
}

#[test]
fn a_skill_folder_that_is_no_name_is_never_listed() {
    let dir = clone_with_alpha();
    let odd = dir.path().join(".claude/skills/has space");
    std::fs::create_dir_all(&odd).unwrap();
    std::fs::write(odd.join("SKILL.md"), "x").unwrap();
    let bare = dir.path().join(".claude/skills/empty");
    std::fs::create_dir_all(&bare).unwrap();
    assert_eq!(skill_names(&path_of(&dir)), ["alpha", "edit-spec", "manage-test-users", "test-data"]);
}

#[test]
fn the_repo_file_defaults_to_claude_md() {
    for asked in [None, Some("")] {
        let (status, text) = repo_answer("", asked);
        assert_eq!(status, 200);
        assert!(text.contains("CLAUDE.md, the copy shipped"), "{text}");
        // CLAUDE.md is long: the first page opens with its page line, then the shipped line.
        let first = text.lines().next().unwrap();
        assert!(first.starts_with("(Page 1 of ") && first.contains(" of CLAUDE.md. "), "{first}");
        assert!(text.contains(&format!("\n\n{}", shipped_line(&Why::NoClone, &[]))), "{text}");
        // The note is last, so it is on the last page.
        assert!(!text.contains(repo_note().trim_end()), "{text}");
    }
}

/// How many pages CLAUDE.md answers in, read from the first page's line.
fn claude_pages(clone: &str) -> usize {
    let (_, first) = repo_answer_page(clone, None, None);
    let line = first.lines().next().unwrap();
    line.strip_prefix("(Page 1 of ").and_then(|r| r.split(' ').next()).unwrap().parse().unwrap()
}

#[test]
fn claude_md_comes_in_pages_of_at_most_40000_characters() {
    let n = claude_pages("");
    assert!(n >= 2, "the shipped CLAUDE.md should need more than one page");
    let mut joined = String::new();
    for p in 1..=n {
        let (status, text) = repo_answer_page("", Some("CLAUDE.md"), Some(&p.to_string()));
        assert_eq!(status, 200, "page {p}");
        assert!(text.chars().count() <= PAGE_CHARS, "page {p} has {} characters", text.chars().count());
        let (head, body) = text.split_once("\n\n").unwrap();
        assert!(head.starts_with(&format!("(Page {p} of {n} of CLAUDE.md. ")), "{head}");
        if p < n {
            assert!(head.contains(&format!("`page` {}.", p + 1)), "{head}");
        } else {
            assert!(head.ends_with("This is the last page.)"), "{head}");
        }
        assert!(!head.contains('\u{2014}') && !head.contains('\u{2013}'));
        joined.push_str(body);
    }
    // Nothing is lost between the pages: the note is last, and the file is whole.
    assert!(joined.trim_end().ends_with(repo_note().trim_end()), "the note must close the last page");
    let whole = normalised(snapshot("CLAUDE.md").unwrap());
    assert!(joined.contains(strip_frontmatter(&whole).trim()), "the pages together lose part of CLAUDE.md");
    // No page given is the first page.
    assert_eq!(repo_answer_page("", None, None), repo_answer_page("", None, Some("1")));
    assert_eq!(repo_answer_page("", None, None), repo_answer_page("", None, Some(" 1 ")));
}

#[test]
fn a_page_past_the_last_or_not_a_number_is_refused() {
    let n = claude_pages("");
    let (status, text) = repo_answer_page("", None, Some(&(n + 1).to_string()));
    assert_eq!((status, text.as_str()), (400, format!("CLAUDE.md has {n} pages: `page` is 1 to {n}.").as_str()));
    for bad in ["0", "-1", "x", "1.5", "2 3"] {
        let (status, text) = repo_answer_page("", None, Some(bad));
        assert_eq!(status, 400, "{bad}");
        assert!(text.starts_with("`page` is a whole number from 1"), "{text}");
    }
    // Another file is one page.
    assert_eq!(repo_answer_page("", Some("spec-editor.md"), Some("1")), repo_answer("", Some("spec-editor.md")));
    let (status, text) = repo_answer_page("", Some("spec-editor.md"), Some("2"));
    assert_eq!((status, text.as_str()), (400, "spec-editor.md has 1 page: `page` is 1."));
    assert!(!repo_answer("", Some("spec-editor.md")).1.starts_with("(Page"));
}

#[test]
fn a_long_live_claude_md_is_paged_at_line_ends_and_a_huge_line_is_cut() {
    let dir = clone();
    // Lines of 1,000 characters: 120 of them are about three pages.
    let line = "x".repeat(999) + "\n";
    std::fs::write(dir.path().join("CLAUDE.md"), line.repeat(120)).unwrap();
    let clone_path = path_of(&dir);
    let n = claude_pages(&clone_path);
    assert!(n >= 3, "{n}");
    let mut total = 0;
    for p in 1..=n {
        let (_, text) = repo_answer_page(&clone_path, None, Some(&p.to_string()));
        assert!(text.chars().count() <= PAGE_CHARS, "page {p}");
        total += text.matches(&line).count();
    }
    assert_eq!(total, 120, "every line lands on exactly one page, whole");
    // One line longer than a page is cut at the page size, losing nothing.
    std::fs::write(dir.path().join("CLAUDE.md"), "y".repeat(100_000)).unwrap();
    let n = claude_pages(&clone_path);
    let mut joined = String::new();
    for p in 1..=n {
        let (status, text) = repo_answer_page(&clone_path, None, Some(&p.to_string()));
        assert_eq!(status, 200);
        assert!(text.chars().count() <= PAGE_CHARS, "page {p}");
        joined.push_str(text.split_once("\n\n").unwrap().1);
    }
    assert!(joined.contains(&"y".repeat(100_000)), "the cut line loses characters");
}

#[test]
fn gen_test_is_labelled_with_its_repo_path() {
    let (_, text) = repo_answer("", Some("gen-test.md"));
    assert!(
        text.contains("## From the Playwright repo: .claude/commands/gen-test.md, the copy shipped with this version"),
        "{}",
        &text[..text.len().min(300)]
    );
    let dir = clone();
    std::fs::create_dir_all(dir.path().join(".claude/commands")).unwrap();
    std::fs::write(dir.path().join(".claude/commands/gen-test.md"), "LIVE GEN").unwrap();
    let (_, live) = repo_answer(&path_of(&dir), Some("gen-test.md"));
    assert!(live.contains("\n\n## From the Playwright repo: .claude/commands/gen-test.md, read from the repo just now"), "{live}");
    // No clone path comes with it.
    assert!(!live.contains(&path_of(&dir)));
    // An agent file keeps its bare name.
    assert_eq!(read("", PLANNER).unwrap().label, "playwright-test-planner.md");
}

#[test]
fn the_gen_test_answer_opens_with_tcms_rules_win_and_ends_with_the_heal_steps_mapped() {
    const LEAD: &str =
        "Where gen-test.md and TCM's rules disagree, TCM's rules win: the writer's and healer's rules come first.";
    assert_eq!(gen_test_lead().trim_end(), LEAD);
    // Ahead of the repo's text, not after 47 KB of it: first with a live file, and right after
    // the shipped line with the shipped copy.
    let dir = clone();
    std::fs::create_dir_all(dir.path().join(".claude/commands")).unwrap();
    std::fs::write(dir.path().join(".claude/commands/gen-test.md"), "LIVE GEN").unwrap();
    let (_, live) = repo_answer(&path_of(&dir), Some("gen-test.md"));
    assert!(live.starts_with(&format!("{LEAD}\n\n## From the Playwright repo: .claude/commands/gen-test.md")), "{live}");
    let (_, shipped) = repo_answer("", Some("gen-test.md"));
    let prefix = format!("{}\n\n{LEAD}\n\n## From the Playwright repo: ", shipped_line(&Why::NoClone, &[]));
    assert!(shipped.starts_with(&prefix), "{}", &shipped[..shipped.len().min(400)]);
    assert_eq!(live.matches(LEAD).count(), 1, "the sentence is said once");
    assert_eq!(shipped.matches(LEAD).count(), 1);
    // Other files do not carry it.
    assert!(!repo_answer("", Some("spec-editor.md")).1.contains(LEAD));
    assert!(!repo_answer("", None).1.contains(LEAD));
    let note = gen_test_note();
    assert!(note.starts_with("## In TCM, use these for the steps of /gen-test above"), "{note}");
    let flat = note.split_whitespace().collect::<Vec<_>>().join(" ");
    for want in [
        // handing a case to the repo's healer
        "playwright-test-healer: never dispatch that agent. Follow the healer's rules (`get_guide`, role healer) with `get_autorun_failures` and `replay_autorun_to_step`.",
        // test.fixme
        "test.fixme(): never. A stuck case stays failed: report a divergence (`report_case_divergence`) and stop.",
        // the repo's preconditions table
        "writing preconditions into the repo's table: the case's preconditions in TCM, as the writer's rules say, never the repo's files.",
        // the repo's quirk file
        "appending to docs/app-quirks.md: `record_quirk`.",
    ] {
        assert!(flat.contains(want), "the gen-test note lacks {want:?}:\n{note}");
    }
    // The mapping closes the answer, and lead-in plus mapping stay inside the caps the role notes keep.
    assert!(shipped.trim_end().ends_with(note.trim_end()));
    let both = format!("{}\n\n{}", gen_test_lead().trim_end(), note.trim_end());
    assert!(both.lines().count() <= v2_lib::autorun::roles::ROLE_MAX_LINES, "{}", both.lines().count());
    assert!(both.chars().count() <= v2_lib::autorun::roles::ROLE_MAX_CHARS, "{}", both.chars().count());
}

#[test]
fn gen_test_comes_with_its_note() {
    let (status, text) = repo_answer("", Some("gen-test.md"));
    assert_eq!(status, 200);
    assert!(text.contains(gen_test_note().trim_end()), "{text}");
    assert!(!text.contains(repo_note().trim_end()), "{text}");
    // A live file is marked live and has no shipped line.
    let dir = clone();
    std::fs::write(dir.path().join(".claude/agents/spec-editor.md"), "---\nname: x\n---\nLIVE EDITOR\n").unwrap();
    let (_, live) = repo_answer(&path_of(&dir), Some("spec-editor.md"));
    assert!(live.starts_with("## From the Playwright repo: spec-editor.md, read from the repo just now"), "{live}");
    assert!(live.contains("LIVE EDITOR") && live.contains(repo_note().trim_end()), "{live}");
}

#[test]
fn an_unknown_repo_file_is_refused() {
    for bad in ["../../.git/config", ".claude/agents/spec-editor.md", "claude.md", "C:\\x"] {
        assert_eq!(repo_answer("", Some(bad)), (404, unknown_repo_file(bad)), "{bad}");
    }
    let said = unknown_repo_file("x");
    assert!(said.starts_with("There is no Playwright repo file \"x\": `file` is one of \"CLAUDE.md\", \"gen-test.md\", "), "{said}");
    assert!(said.ends_with(" and \"doc-drift-auditor.md\"."), "{said}");
    for n in REPO_FILES {
        assert_eq!(repo_answer("", Some(n)).0, 200, "{n}");
    }
}

#[test]
fn every_repo_note_names_only_real_tools() {
    let tools = v2_lib::mcp::all_tool_names();
    for note in [repo_note(), gen_test_note(), gen_test_lead()] {
        for (i, span) in note.split('`').enumerate() {
            if i % 2 == 1 && span.contains('_') {
                assert!(tools.iter().any(|t| t == span), "a repo note backticks {span:?}, which is no tool");
            }
        }
        assert!(!note.contains('\u{2014}') && !note.contains('\u{2013}'));
    }
}
