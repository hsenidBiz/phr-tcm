//! The AI Bridge tab's writing style (writing-style.json): the store, its
//! limits, and the starting style a machine gets when it has none. How the
//! guide carries it is pinned in ai_bridge.rs.

use v2_lib::writing_style::{
    get_or_create, load, refusal, save, WritingStyle, DEFAULT_RISK_TIERED, EMPTY, FILE, MAX_BYTES, TOO_LARGE,
};

fn dir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn style(enabled: bool, text: &str) -> WritingStyle {
    WritingStyle { enabled, text: text.into() }
}

#[test]
fn a_saved_style_reads_back_the_same() {
    let d = dir();
    for s in [style(true, "## Mine\nOne case per screen.\n"), style(false, ""), style(false, "kept while off")] {
        save(d.path(), &s).unwrap();
        assert_eq!(load(d.path()), Some(s.clone()));
    }
    // One JSON file, and no temp file left beside it.
    let names: Vec<String> =
        std::fs::read_dir(d.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, vec![FILE.to_string()]);
    assert_eq!(FILE, "writing-style.json");
}

#[test]
fn a_save_creates_a_missing_folder() {
    let d = dir();
    let inner = d.path().join("not-yet");
    save(&inner, &style(true, "rules")).unwrap();
    assert_eq!(load(&inner), Some(style(true, "rules")));
}

#[test]
fn text_over_64_kb_is_refused() {
    let d = dir();
    assert_eq!(MAX_BYTES, 65_536);
    let at_limit = "a".repeat(MAX_BYTES);
    save(d.path(), &style(true, &at_limit)).unwrap();

    let over = "a".repeat(MAX_BYTES + 1);
    assert_eq!(save(d.path(), &style(true, &over)), Err(TOO_LARGE.to_string()));
    assert_eq!(TOO_LARGE, "the writing style is larger than 64 KB");
    // Counted in UTF-8 bytes, not characters: 21,846 three-byte characters
    // are 65,538 bytes.
    let wide = "\u{20ac}".repeat(21_846);
    assert_eq!(refusal(&style(false, &wide)), Some(TOO_LARGE));
    // The refused save left the last good one in place.
    assert_eq!(load(d.path()), Some(style(true, &at_limit)));
}

#[test]
fn empty_text_is_refused_only_while_enabled() {
    let d = dir();
    for blank in ["", "   ", "\n\t \r\n"] {
        assert_eq!(save(d.path(), &style(true, blank)), Err(EMPTY.to_string()), "{blank:?}");
        assert!(save(d.path(), &style(false, blank)).is_ok(), "{blank:?}");
    }
    assert_eq!(EMPTY, "the writing style is empty");
}

#[test]
fn a_missing_file_is_created_with_the_starting_style_switched_off() {
    let d = dir();
    assert_eq!(load(d.path()), None);
    let got = get_or_create(d.path());
    assert_eq!(got, style(false, DEFAULT_RISK_TIERED));
    assert!(d.path().join(FILE).exists(), "the file is written");
    assert_eq!(load(d.path()), Some(style(false, DEFAULT_RISK_TIERED)));

    // An existing file is never replaced by the starting style.
    save(d.path(), &style(true, "mine")).unwrap();
    assert_eq!(get_or_create(d.path()), style(true, "mine"));
}

#[test]
fn an_unreadable_file_is_left_alone() {
    let d = dir();
    std::fs::write(d.path().join(FILE), "{ not json").unwrap();
    assert_eq!(load(d.path()), None);
    assert_eq!(get_or_create(d.path()), style(false, DEFAULT_RISK_TIERED));
    assert_eq!(std::fs::read_to_string(d.path().join(FILE)).unwrap(), "{ not json");
}

/// The starting style is the old risk-tiered trial, converted: the tiers,
/// the budget, the regression review labels and its exact question.
#[test]
fn the_default_text_carries_the_old_trial_rules() {
    let text = DEFAULT_RISK_TIERED.replace("\r\n", "\n");
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    for needle in [
        "### Tier every scenario",
        "T1 - Critical",
        "T2 - Core",
        "T3 - Low",
        "Tier each SCENARIO, not the whole story.",
        "Equivalence partitioning: one case per partition, not per value.",
        "T1 25, T2 12, T3 5",
        "exactly one of `T1`, `T2`, `T3`",
        "## Edge cases, the tiered way",
        "## Scenario list before drafting",
        "## Closing summary",
        "## Regression suite review (trial rules)",
        "`KEEP_REGRESSION`",
        "`ADD_REGRESSION`",
        "`REMOVE_REGRESSION`",
        "Proposed final = Current - Remove + Add and Keep = Current - Remove",
        "\"Would you like me to apply these Regression tag changes?\"",
        "follow the rules and say which rule the request conflicts with",
    ] {
        assert!(flat.contains(needle), "missing {needle:?}");
    }
    // The two workflow steps are sections now, not numbered steps.
    assert!(!text.contains("1.5. Before drafting"));
    assert!(!text.contains("6. End with a summary"));
    assert!(text.len() <= MAX_BYTES);
}

/// Upload .md reads a Markdown file for the editor: only .md and .markdown,
/// no larger than a save accepts, and never a path in the error.
#[test]
fn upload_reads_only_markdown_files_within_the_limit() {
    use v2_lib::writing_style::read_markdown;
    let d = dir();
    let md = d.path().join("policy.md");
    std::fs::write(&md, "\u{feff}## Policy\nOne case per screen.\n").unwrap();
    assert_eq!(read_markdown(&md).unwrap(), "## Policy\nOne case per screen.\n");

    let upper = d.path().join("policy.MARKDOWN");
    std::fs::write(&upper, "text").unwrap();
    assert_eq!(read_markdown(&upper).unwrap(), "text");

    let txt = d.path().join("policy.txt");
    std::fs::write(&txt, "text").unwrap();
    assert_eq!(read_markdown(&txt), Err("choose a .md or .markdown file".to_string()));

    let big = d.path().join("big.md");
    std::fs::write(&big, "a".repeat(MAX_BYTES + 1)).unwrap();
    assert_eq!(read_markdown(&big), Err(TOO_LARGE.to_string()));

    let missing = d.path().join("gone.md");
    let err = read_markdown(&missing).unwrap_err();
    assert!(!err.contains("gone.md") && !err.contains(&*d.path().to_string_lossy()), "{err}");
}

/// The guide carries the text, but the log never does: only that it changed
/// and its size.
#[test]
fn the_source_never_logs_the_text() {
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/writing_style.rs"))
        .unwrap()
        .replace("\r\n", "\n");
    let logs: Vec<&str> = src.split("applog::").skip(1).map(|rest| rest.split(");").next().unwrap_or(rest)).collect();
    assert!(!logs.is_empty());
    for call in logs {
        assert!(!call.contains(".text)") && !call.contains(".text,") && !call.contains("{text"), "a log names the text: {call}");
    }
    assert!(src.contains("style.text.len()"), "the save logs the size");
}
