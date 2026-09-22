//! Per-project quirks: short, attributed notes about the application that
//! a person or an assistant can add, so the next repair does not rediscover
//! the same surprise.

use v2_lib::autorun::quirks::{
    add_quirk, load_quirks, quirks_path, quirks_section, save_quirks, Quirk, MAX_QUIRKS, MAX_QUIRK_CHARS,
};
use v2_lib::autorun::recipe::project_slug;

fn quirk(text: &str, by: &str, at: &str) -> Quirk {
    Quirk { text: text.into(), by: by.into(), at: at.into() }
}

#[test]
fn no_file_yet_is_an_empty_list_and_a_saved_list_reads_back() {
    let dir = tempfile::tempdir().unwrap();
    assert!(load_quirks(dir.path(), "Acme", "Web").unwrap().is_empty());
    let list = vec![quirk("the search box debounces 400ms", "person", "1000"), quirk("dates render as dd/mm", "assistant", "2000")];
    save_quirks(dir.path(), "Acme", "Web", &list).unwrap();
    assert_eq!(load_quirks(dir.path(), "Acme", "Web").unwrap(), list);
}

/// The quirks file lives under `projects/`, named after the same slug the
/// recipe uses, with its own suffix - so the two never collide and both
/// sort together in a file listing.
#[test]
fn the_path_lives_under_projects_and_differs_between_projects() {
    let dir = tempfile::tempdir().unwrap();
    let path = quirks_path(dir.path(), "Acme", "Web");
    assert_eq!(path, dir.path().join("projects").join(format!("{}-quirks.json", project_slug("Acme", "Web"))));
    assert_ne!(quirks_path(dir.path(), "Acme", "Web"), quirks_path(dir.path(), "Acme", "Other"));
}

#[test]
fn add_quirk_appends_and_stamps_who_and_when() {
    let dir = tempfile::tempdir().unwrap();
    assert!(add_quirk(dir.path(), "Acme", "Web", "the save button double-submits on a slow network", "person", 12345).unwrap());
    let saved = load_quirks(dir.path(), "Acme", "Web").unwrap();
    assert_eq!(saved, vec![quirk("the save button double-submits on a slow network", "person", "12345")]);
}

/// A repeat of the same fact, however it is capitalised or spaced, is not
/// worth a second line - the caller gets `false` and nothing new is written.
#[test]
fn add_quirk_dedupes_case_and_whitespace_insensitively() {
    let dir = tempfile::tempdir().unwrap();
    assert!(add_quirk(dir.path(), "Acme", "Web", "the   grid  paginates at 50 rows", "person", 1).unwrap());
    assert!(!add_quirk(dir.path(), "Acme", "Web", "  THE GRID PAGINATES AT 50 ROWS  ", "assistant", 2).unwrap());
    assert_eq!(load_quirks(dir.path(), "Acme", "Web").unwrap().len(), 1);
}

#[test]
fn save_quirks_refuses_more_than_the_cap() {
    let dir = tempfile::tempdir().unwrap();
    let list: Vec<Quirk> = (0..=MAX_QUIRKS).map(|i| quirk(&format!("quirk {i}"), "person", "1")).collect();
    let err = save_quirks(dir.path(), "Acme", "Web", &list).unwrap_err();
    assert!(err.contains(&MAX_QUIRKS.to_string()), "{err}");
    assert!(load_quirks(dir.path(), "Acme", "Web").unwrap().is_empty());
}

#[test]
fn save_quirks_refuses_blank_text() {
    let dir = tempfile::tempdir().unwrap();
    let err = save_quirks(dir.path(), "Acme", "Web", &[quirk("   ", "person", "1")]).unwrap_err();
    assert!(err.contains("text"), "{err}");
}

#[test]
fn save_quirks_refuses_text_over_the_character_cap() {
    let dir = tempfile::tempdir().unwrap();
    let long = "x".repeat(MAX_QUIRK_CHARS + 1);
    let err = save_quirks(dir.path(), "Acme", "Web", &[quirk(&long, "person", "1")]).unwrap_err();
    assert!(err.contains(&MAX_QUIRK_CHARS.to_string()), "{err}");
}

#[test]
fn save_quirks_refuses_a_newline_inside_a_quirk() {
    let dir = tempfile::tempdir().unwrap();
    let err = save_quirks(dir.path(), "Acme", "Web", &[quirk("line one\nline two", "person", "1")]).unwrap_err();
    assert!(err.contains("line"), "{err}");
}

/// The same fact twice, however capitalised or spaced, is refused rather
/// than silently written twice - `linesToQuirks` on the frontend already
/// collapses a repeated line to its first occurrence, but a save that
/// bypasses that (a hand-edited call, or a future caller) must not be able
/// to persist the duplicate either.
#[test]
fn save_quirks_refuses_two_quirks_with_the_same_text() {
    let dir = tempfile::tempdir().unwrap();
    let list = vec![
        quirk("the grid paginates at 50 rows", "person", "1"),
        quirk("  THE GRID   PAGINATES AT 50 ROWS", "assistant", "2"),
    ];
    let err = save_quirks(dir.path(), "Acme", "Web", &list).unwrap_err();
    assert!(err.contains("appears twice"), "{err}");
    assert!(load_quirks(dir.path(), "Acme", "Web").unwrap().is_empty());
}

/// Mirrors `recipe::save_recipe`'s own guard and sentence: an empty
/// organization or project is refused before anything is written, rather
/// than quietly slugging to a file nobody picked.
#[test]
fn save_quirks_refuses_an_empty_organization_or_project() {
    let dir = tempfile::tempdir().unwrap();
    let err = save_quirks(dir.path(), "  ", "Web", &[quirk("x", "person", "1")]).unwrap_err();
    assert!(err.contains("organization"), "{err}");
    let err = save_quirks(dir.path(), "Acme", " ", &[quirk("x", "person", "1")]).unwrap_err();
    assert!(err.contains("project"), "{err}");
}

/// A refusal never leaves a half-written file: the earlier valid save is
/// still what loads back.
#[test]
fn a_refusal_leaves_the_earlier_save_in_place() {
    let dir = tempfile::tempdir().unwrap();
    save_quirks(dir.path(), "Acme", "Web", &[quirk("good", "person", "1")]).unwrap();
    assert!(save_quirks(dir.path(), "Acme", "Web", &[quirk("bad\nquirk", "person", "1")]).is_err());
    assert_eq!(load_quirks(dir.path(), "Acme", "Web").unwrap(), vec![quirk("good", "person", "1")]);
}

/// Behaviour change: each quirk line is now attributed, `(recorded by
/// you)` for a person and `(recorded by the assistant)` for an assistant
/// - so an assistant reading its own guide can tell its own past
/// discoveries from a person's.
#[test]
fn the_section_text_is_empty_for_no_quirks_and_a_bulleted_list_otherwise() {
    assert_eq!(quirks_section(&[]), "");
    let text = quirks_section(&[quirk("the search box debounces 400ms", "person", "1"), quirk("dates render as dd/mm", "assistant", "2")]);
    assert!(text.starts_with("## Known quirks of this application\n\n"), "{text}");
    assert!(text.contains("- the search box debounces 400ms (recorded by you)\n"), "{text}");
    assert!(text.contains("- dates render as dd/mm (recorded by the assistant)\n"), "{text}");
}
