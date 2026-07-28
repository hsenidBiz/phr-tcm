//! Comments on a DRAFT: written into the case in the JSON file it came
//! from, and into a file-wide `comments` string for the set as a whole.

use v2_lib::import_parser::comments::{
    general_comment, patch_case_comment, patch_general_comment, CaseTarget,
};
use v2_lib::import_parser::{export_queue_to_html, CommentCtx, DraftFile, DraftNoteCtx};
use v2_lib::model::TestCase;

/// A file shaped like one the app exports, with a key we do not own and
/// deliberately non-alphabetical order.
const FILE: &str = r#"{
  "format": "tcm-ai-round-trip",
  "version": 1,
  "instructions": "Edit this file, do not rename it.",
  "test_cases": [
    { "id": 42, "title": "Sign in as admin", "tags": "smoke", "steps": [] },
    { "id": null, "title": "New case", "tags": "", "steps": [] }
  ]
}
"#;

fn cases(json: &str) -> Vec<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(json).unwrap()["test_cases"]
        .as_array()
        .unwrap()
        .clone()
}

#[test]
fn a_comment_lands_on_the_case_with_that_work_item_id() {
    let target = CaseTarget { id: Some(42), title: "anything at all".into() };
    let out = patch_case_comment(FILE, &target, "Step 3 needs the new dialog").unwrap();
    let out_cases = cases(&out);
    assert_eq!(out_cases[0]["comment"], "Step 3 needs the new dialog");
    // The id decides, not the title, and no other case is touched.
    assert!(out_cases[1].get("comment").is_none());
}

/// A draft case has no id, so the title is the only handle - the same rule
/// the frontend's caseKey uses.
#[test]
fn an_id_less_case_is_found_by_title_case_insensitively() {
    let target = CaseTarget { id: None, title: "  NEW CASE ".into() };
    let out = patch_case_comment(FILE, &target, "Waiting on the spec").unwrap();
    assert_eq!(cases(&out)[1]["comment"], "Waiting on the spec");
    assert!(cases(&out)[0].get("comment").is_none());
}

/// An id-less target must never be satisfied by a case that HAS an id -
/// otherwise a draft named the same as an existing case would silently
/// comment on the wrong work item.
#[test]
fn an_id_less_target_does_not_claim_an_identified_case() {
    let target = CaseTarget { id: None, title: "Sign in as admin".into() };
    assert!(patch_case_comment(FILE, &target, "x").is_err());
}

#[test]
fn a_case_that_is_not_in_this_file_is_refused_by_name() {
    let target = CaseTarget { id: None, title: "Never written".into() };
    let err = patch_case_comment(FILE, &target, "x").unwrap_err();
    assert!(err.contains("Never written"), "got {err}");
}

/// The whole point of patching rather than regenerating: a file holds keys
/// the app does not own, and an assistant is often editing it at the same
/// time. Nothing but the comment may move.
#[test]
fn everything_the_app_does_not_own_survives_a_save() {
    let out = patch_case_comment(
        FILE,
        &CaseTarget { id: Some(42), title: String::new() },
        "note",
    )
    .unwrap();

    let before: serde_json::Value = serde_json::from_str(FILE).unwrap();
    let after: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(after["instructions"], before["instructions"]);
    assert_eq!(after["format"], before["format"]);
    assert_eq!(after["version"], before["version"]);

    // Including the ORDER of the keys - a save that reshuffled the file
    // alphabetically would bury the real change in diff noise.
    let keys: Vec<&str> = after.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["format", "version", "instructions", "test_cases"]);
}

#[test]
fn clearing_a_comment_removes_the_key_rather_than_blanking_it() {
    let target = CaseTarget { id: Some(42), title: String::new() };
    let with = patch_case_comment(FILE, &target, "temporary").unwrap();
    let without = patch_case_comment(&with, &target, "   ").unwrap();
    assert!(cases(&without)[0].get("comment").is_none());
}

#[test]
fn the_whole_set_comment_round_trips() {
    assert_eq!(general_comment(FILE), "");
    let out = patch_general_comment(FILE, "Spec 3.2 is ambiguous - asked Dev").unwrap();
    assert_eq!(general_comment(&out), "Spec 3.2 is ambiguous - asked Dev");
    let cleared = patch_general_comment(&out, "").unwrap();
    assert_eq!(general_comment(&cleared), "");
    assert!(!cleared.contains("comments"));
}

/// The importer also accepts a bare list. Per-case comments still work
/// there; a comment about the whole set has nowhere to go, and says so
/// instead of quietly changing the file's shape.
#[test]
fn a_bare_list_file_takes_case_comments_but_not_a_set_comment() {
    let bare = r#"[{ "title": "Only case", "steps": [] }]"#;
    let out = patch_case_comment(
        bare,
        &CaseTarget { id: None, title: "Only case".into() },
        "fine",
    )
    .unwrap();
    assert!(out.contains("fine"));

    let err = patch_general_comment(bare, "nope").unwrap_err();
    assert!(err.contains("bare list"), "got {err}");
}

#[test]
fn a_file_that_is_not_json_is_refused_without_being_overwritten() {
    assert!(patch_general_comment("not json at all", "x").is_err());
    assert!(patch_case_comment(
        "not json at all",
        &CaseTarget { id: None, title: "t".into() },
        "x"
    )
    .is_err());
    assert_eq!(general_comment("not json at all"), "");
}

// ------------------------------------------------------------ the page

fn draft(title: &str, id: Option<i32>, comment: &str) -> TestCase {
    TestCase {
        title: title.into(),
        automation_status: "Planned".into(),
        update_id: id,
        comment: comment.into(),
        ..Default::default()
    }
}

fn tmp(name: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir()
        .join(format!("tcm-draft-{nanos}-{name}"))
        .to_string_lossy()
        .to_string()
}

fn draft_page(queue: &[TestCase], owners: Vec<String>, files: Vec<DraftFile>) -> String {
    let ctx = DraftNoteCtx { port: 4711, owners, files };
    let path = tmp("draft.html");
    export_queue_to_html(queue, &path, "", Some(CommentCtx::Draft(&ctx))).unwrap();
    let html = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    html
}

/// The gap this feature fills: on the ADO page only a case with a work item
/// id can have a note, because the store is keyed by id. A draft case has
/// no id and must still get a box.
#[test]
fn every_draft_case_gets_a_comment_box_including_the_id_less_ones() {
    let queue = vec![
        draft("Existing", Some(42), ""),
        draft("Brand new", None, "already noted"),
    ];
    let html = draft_page(
        &queue,
        vec!["C:/work/cases.json".into(), "C:/work/cases.json".into()],
        vec![],
    );
    assert_eq!(html.matches("class='note-box'").count(), 2);
    // Prefilled from the case's own comment field.
    assert!(html.contains("already noted"));
    assert!(html.contains("var NOTE_PORT=4711"));
}

#[test]
fn a_case_with_no_file_says_the_comment_stays_in_the_app() {
    let queue = vec![draft("Typed by hand", None, "")];
    let html = draft_page(&queue, vec![String::new()], vec![]);
    assert!(html.contains("Saved with the draft in the app"));
    assert!(!html.contains("Saved into this case in the JSON file"));
}

#[test]
fn the_side_panel_lists_one_box_per_watched_file() {
    let queue = vec![draft("A", None, "")];
    let html = draft_page(
        &queue,
        vec!["C:/work/login.json".into()],
        vec![
            DraftFile {
                path: "C:/work/login.json".into(),
                label: "login.json".into(),
                comment: "Spec 3.2 is ambiguous".into(),
            },
            DraftFile {
                path: "C:/work/pay.json".into(),
                label: "pay.json".into(),
                comment: String::new(),
            },
        ],
    );
    assert!(html.contains("General comments"));
    assert!(html.contains("data-file='0'"));
    assert!(html.contains("data-file='1'"));
    assert!(html.contains("Spec 3.2 is ambiguous"));
    // Collapsible without any script of its own.
    assert!(html.contains("<details class='aside' open>"));
    // The two-column shell only exists when there is a panel to put in it.
    assert!(html.contains("<div class='shell'>"));
}

#[test]
fn without_files_there_is_no_panel_and_no_second_column() {
    let html = draft_page(&[draft("A", None, "")], vec![String::new()], vec![]);
    assert!(!html.contains("General comments"));
    assert!(!html.contains("class='shell'"));
}

/// A title is attacker-controlled as far as this page is concerned - it can
/// come from a JSON file someone else wrote. It must not be able to write
/// markup into a card, nor to close the <script> element that carries the
/// list of cases the boxes address.
#[test]
fn a_hostile_title_cannot_escape_the_card_or_the_script() {
    let html = draft_page(
        &[draft("</script><img src=x onerror=alert(1)>", None, "")],
        vec!["C:/work/a.json".into()],
        vec![],
    );
    // A title reaches the page twice, and each copy has its own defence.
    //
    // In the CARD it is HTML-escaped, so it can never be markup. Checked
    // against the markup region only: the other copy lives inside a
    // <script>, where `<img` is an inert run of characters in a JS string
    // and asserting over the whole document would flag it wrongly.
    let markup = html.split("<script>").next().expect("cards precede the scripts");
    assert!(!markup.contains("<img"), "markup reached the card");
    assert!(markup.contains("&lt;img src=x"));

    // In the SCRIPT the only way out is a literal `</script`, so that is
    // what is escaped. Counting the tags proves none was smuggled in.
    assert_eq!(html.matches("</script>").count(), html.matches("<script>").count());
    assert!(html.contains(r"<\/script>"), "the JSON copy must be escaped");
}

/// Numbering is for reading the page aloud, nothing more: it counts the
/// cards, it is not stored, and it does not renumber when the search filter
/// hides some - "case 7" must mean the same thing before and after typing.
#[test]
fn cases_are_numbered_from_one_in_page_order() {
    let queue = vec![
        draft("First", Some(42), ""),
        draft("Second", None, ""),
        draft("Third", None, ""),
    ];
    let html = draft_page(&queue, vec![String::new(); 3], vec![]);
    assert!(html.contains("<span class='seq'>1</span>"));
    assert!(html.contains("<span class='seq'>2</span>"));
    assert!(html.contains("<span class='seq'>3</span>"));
    assert_eq!(html.matches("class='seq'").count(), 3);
    // The number sits beside the work item id, not instead of it.
    assert!(html.contains("<span class='seq'>1</span><span class='wid'>#42</span>"));
}
