//! Per-project quirks: short, attributed notes about the application that
//! a person or an assistant can add, so the next repair does not rediscover
//! the same surprise - with the evidence later runs give for each, a cap
//! on the ACTIVE ones that names what to retire, and retirement.

use v2_lib::autorun::quirks::{
    add_quirk, apply_run_evidence, cap_refusal, delete_in, edit_in, load_quirks, quirks_path, quirks_section,
    record_in, record_run_evidence, restore_in, retire_candidates, retire_in, save_quirks, source_for_repair, source_from_run, count_saved_run, person_retired, RETIRE_APP_TOOL,
    Quirk, QuirkSource, Recorded, MAX_QUIRKS, MAX_QUIRK_CHARS, MAX_RETIRED, PERSON_NOTE, RETIRE_TOOL,
};
use v2_lib::autorun::recipe::project_slug;
use v2_lib::autorun::{CaseRecord, CaseScript, LocalRun, StepRecord, StepScript};
use v2_lib::browser::actions::{Action, ActionOutcome};
use v2_lib::browser::locator::{LocatorStep, Target};

/// A note as a test writes it: its id is derived from its time, so two
/// built in one test never share one.
fn quirk(text: &str, by: &str, at: &str) -> Quirk {
    let mut q = Quirk::new(text, by, "autorun", at.parse().unwrap_or(0));
    q.id = format!("q{at}");
    q.at = at.to_string();
    q
}

fn source(case_id: i32, steps: &[u32], class: Option<&str>) -> QuirkSource {
    QuirkSource { case_id, steps: steps.to_vec(), class: class.map(str::to_string) }
}

#[test]
fn no_file_yet_is_an_empty_list_and_a_saved_list_reads_back() {
    let dir = tempfile::tempdir().unwrap();
    assert!(load_quirks(dir.path(), "Acme", "Web").unwrap().is_empty());
    let list = vec![quirk("the search box debounces 400ms", "person", "1000"), quirk("dates render as dd/mm", "assistant", "2000")];
    save_quirks(dir.path(), "Acme", "Web", &list).unwrap();
    assert_eq!(load_quirks(dir.path(), "Acme", "Web").unwrap(), list);
}

/// A file written before ids, evidence and retirement existed loads
/// unchanged - every note active, from Auto Run, with no evidence - and
/// gains ids, the same ones on every load until a save writes them down.
#[test]
fn an_old_quirks_file_loads_unchanged_and_gets_ids() {
    let dir = tempfile::tempdir().unwrap();
    let path = quirks_path(dir.path(), "Acme", "Web");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        r#"[{"text":"the grid paginates at 50 rows","by":"person","at":"1000"},{"text":"dates render as dd/mm","by":"assistant","at":"2000"}]"#,
    )
    .unwrap();
    let first = load_quirks(dir.path(), "Acme", "Web").unwrap();
    assert_eq!(first.len(), 2);
    assert_eq!(first[0].text, "the grid paginates at 50 rows");
    assert_eq!(first[1].by, "assistant");
    for q in &first {
        assert!(q.id.starts_with('q') && q.id.len() == 7, "a short id: {}", q.id);
        assert!(q.is_active());
        assert_eq!(q.status, "active");
        assert_eq!(q.from, "autorun");
        assert!(q.sources.is_empty());
        assert_eq!((q.confirmed, q.doubted), (0, 0));
    }
    assert_ne!(first[0].id, first[1].id);
    // Stable before any save...
    assert_eq!(load_quirks(dir.path(), "Acme", "Web").unwrap(), first);
    // ...and written down by the next one.
    save_quirks(dir.path(), "Acme", "Web", &first).unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains(&format!("\"id\": \"{}\"", first[0].id)), "{raw}");
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
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].text, "the save button double-submits on a slow network");
    assert_eq!(saved[0].by, "person");
    assert_eq!(saved[0].at, "12345");
    assert!(!saved[0].id.is_empty());
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
fn save_quirks_refuses_more_active_quirks_than_the_cap() {
    let dir = tempfile::tempdir().unwrap();
    let list: Vec<Quirk> = (0..=MAX_QUIRKS).map(|i| quirk(&format!("quirk {i}"), "person", &i.to_string())).collect();
    let err = save_quirks(dir.path(), "Acme", "Web", &list).unwrap_err();
    assert!(err.contains(&MAX_QUIRKS.to_string()), "{err}");
    assert!(load_quirks(dir.path(), "Acme", "Web").unwrap().is_empty());
}

/// The cap counts ACTIVE notes only: forty active and more retired is a
/// list the file keeps.
#[test]
fn the_cap_counts_active_quirks_only() {
    let dir = tempfile::tempdir().unwrap();
    let mut list: Vec<Quirk> = (0..MAX_QUIRKS).map(|i| quirk(&format!("active {i}"), "person", &i.to_string())).collect();
    for i in 0..5 {
        let mut q = quirk(&format!("retired {i}"), "assistant", &(100 + i).to_string());
        q.status = "retired".into();
        q.retired_at = Some((200 + i).to_string());
        list.push(q);
    }
    save_quirks(dir.path(), "Acme", "Web", &list).unwrap();
    assert_eq!(load_quirks(dir.path(), "Acme", "Web").unwrap().len(), MAX_QUIRKS + 5);
}

/// Retired notes are kept for a person to restore - up to a cap of their
/// own, the oldest retirement dropped first.
#[test]
fn retired_quirks_are_kept_up_to_their_own_cap_oldest_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let list: Vec<Quirk> = (0..MAX_RETIRED + 2)
        .map(|i| {
            let mut q = quirk(&format!("retired {i}"), "assistant", &i.to_string());
            q.status = "retired".into();
            q.retired_at = Some((1000 + i).to_string());
            q
        })
        .collect();
    save_quirks(dir.path(), "Acme", "Web", &list).unwrap();
    let saved = load_quirks(dir.path(), "Acme", "Web").unwrap();
    assert_eq!(saved.len(), MAX_RETIRED);
    assert!(!saved.iter().any(|q| q.text == "retired 0" || q.text == "retired 1"), "the two oldest go");
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
    let mut q = quirk("x", "person", "1");
    q.text = "line one\nline two".into();
    let err = save_quirks(dir.path(), "Acme", "Web", &[q]).unwrap_err();
    assert!(err.contains("line"), "{err}");
}

/// The same fact twice, however capitalised or spaced, is refused rather
/// than silently written twice - retired or not.
#[test]
fn save_quirks_refuses_two_quirks_with_the_same_text() {
    let dir = tempfile::tempdir().unwrap();
    let mut second = quirk("  THE GRID   PAGINATES AT 50 ROWS", "assistant", "2");
    second.status = "retired".into();
    let list = vec![quirk("the grid paginates at 50 rows", "person", "1"), second];
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
    let mut bad = quirk("x", "person", "1");
    bad.text = "bad\nquirk".into();
    assert!(save_quirks(dir.path(), "Acme", "Web", &[bad]).is_err());
    assert_eq!(load_quirks(dir.path(), "Acme", "Web").unwrap(), vec![quirk("good", "person", "1")]);
}

// ------------------------------------------------------------ the guide line

/// Each ACTIVE note is listed with its id and who wrote it; one filed with
/// a repair says what the runs since have shown. A retired note is in no
/// guide.
#[test]
fn the_section_lists_active_notes_with_ids_attribution_and_evidence() {
    assert_eq!(quirks_section(&[]), "");
    let person = quirk("the search box debounces 400ms", "person", "1");
    let plain = quirk("dates render as dd/mm", "assistant", "2");
    let mut confirmed = quirk("the grid needs a second click", "assistant", "3");
    confirmed.sources = vec![source(7, &[2], Some("not_found"))];
    confirmed.confirmed = 3;
    confirmed.last_confirmed = Some("1790812800000".into()); // 2026-10-01
    let mut doubted = quirk("the menu opens on hover", "assistant", "4");
    doubted.sources = vec![source(8, &[1], None)];
    doubted.doubted = 2;
    let mut untested = quirk("the toast fades after 3s", "assistant", "5");
    untested.sources = vec![source(9, &[4], None)];
    let mut api = quirk("the leave handler wants a CSRF header", "assistant", "6");
    api.from = "api".into();
    let mut retired = quirk("an old note nobody needs", "assistant", "7");
    retired.status = "retired".into();

    let text = quirks_section(&[person, plain, confirmed, doubted, untested, api, retired]);
    assert!(text.starts_with("## Known quirks of this application\n\n"), "{text}");
    assert!(text.contains(RETIRE_TOOL), "{text}");
    assert!(text.contains("- [q1] (person) the search box debounces 400ms\n"), "{text}");
    assert!(text.contains("- [q2] (assistant) dates render as dd/mm\n"), "{text}");
    assert!(text.contains("- [q3] (assistant, confirmed 3x, last 2026-10-01) the grid needs a second click\n"), "{text}");
    assert!(text.contains("- [q4] (assistant, did not help 2x) the menu opens on hover\n"), "{text}");
    assert!(text.contains("- [q5] (assistant, not yet tested by a run) the toast fades after 3s\n"), "{text}");
    assert!(text.contains("- [q6] (assistant, API) the leave handler wants a CSRF header\n"), "{text}");
    assert!(!text.contains("an old note nobody needs"), "a retired note is in no guide: {text}");
}

// ------------------------------------------------------------ the cap

fn full_list() -> Vec<Quirk> {
    let mut list: Vec<Quirk> = Vec::new();
    for i in 0..MAX_QUIRKS {
        // Most are confirmed more often than doubted: not candidates.
        let mut q = quirk(&format!("note {i}"), "assistant", &(1000 + i).to_string());
        q.sources = vec![source(i as i32, &[1], None)];
        q.confirmed = 2;
        list.push(q);
    }
    // Candidates: never confirmed, or doubted more often than confirmed.
    // The newest of the four is left out: only three are named.
    list[5].confirmed = 0;
    list[5].at = "500".into();
    list[9].doubted = 5;
    list[9].at = "100".into();
    list[20].confirmed = 0;
    list[20].at = "300".into();
    list[30].confirmed = 0;
    list[30].at = "900".into();
    // A person's never-confirmed note is never a candidate.
    list[1].by = "person".into();
    list[1].confirmed = 0;
    list[1].at = "1".into();
    list
}

#[test]
fn a_full_list_refuses_one_more_and_names_the_three_best_candidates_oldest_first() {
    let mut list = full_list();
    let order: Vec<&str> = retire_candidates(&list).iter().map(|q| q.text.as_str()).collect();
    assert_eq!(order, vec!["note 9", "note 20", "note 5"]);

    let err = record_in(&mut list, "one more", "assistant", "autorun", vec![], 9999).unwrap_err();
    assert!(err.contains(RETIRE_TOOL), "the refusal names the retire tool: {err}");
    let (a, b, c) = (err.find("\"note 9\"").unwrap(), err.find("\"note 20\"").unwrap(), err.find("\"note 5\"").unwrap());
    assert!(a < b && b < c, "oldest first: {err}");
    assert!(!err.contains("\"note 30\""), "only three: {err}");
    assert!(!err.contains("\"note 1\""), "never a person's note: {err}");
    assert_eq!(list.len(), MAX_QUIRKS, "nothing was added");

    // A person adding past the cap is told the same, in their own words.
    let person = record_in(&mut list, "one more", "person", "autorun", vec![], 9999).unwrap_err();
    assert!(!person.contains(RETIRE_TOOL), "{person}");
    assert!(person.contains("\"note 9\"") && person.contains("retire one"), "{person}");
    assert_eq!(person, cap_refusal(&list, None));
}

/// Retiring one frees its place.
#[test]
fn retiring_one_makes_room_for_the_next() {
    let mut list = full_list();
    let id = list[9].id.clone();
    retire_in(&mut list, &id, Some("never helped"), None, true, 5000).unwrap();
    assert!(matches!(record_in(&mut list, "one more", "assistant", "autorun", vec![], 9999), Ok(Recorded::Added(_))));
}

// ------------------------------------------------------------ retiring

#[test]
fn the_assistant_may_not_retire_a_persons_note() {
    let mut list = vec![quirk("the grid paginates at 50 rows", "person", "1")];
    let err = retire_in(&mut list, "q1", Some("no longer true"), None, true, 10).unwrap_err();
    assert_eq!(err, PERSON_NOTE);
    assert!(list[0].is_active());
    // The person, in the app, may.
    retire_in(&mut list, "q1", None, None, false, 10).unwrap();
    assert!(!list[0].is_active());
}

#[test]
fn the_assistant_must_give_a_reason() {
    let mut list = vec![quirk("dates render as dd/mm", "assistant", "1")];
    let err = retire_in(&mut list, "q1", Some("  "), None, true, 10).unwrap_err();
    assert!(err.contains("reason"), "{err}");
    assert!(list[0].is_active());
}

#[test]
fn a_replacement_inherits_the_retired_notes_sources() {
    let mut old = quirk("the grid needs a second click", "assistant", "1");
    old.sources = vec![source(7, &[2, 3], Some("not_found"))];
    old.from = "api".into();
    let mut list = vec![old];
    let made = retire_in(
        &mut list,
        "q1",
        Some("it was the spinner, not the grid"),
        Some("a spinner covers the grid while it loads"),
        true,
        50,
    )
    .unwrap()
    .expect("a replacement");
    let Recorded::Added(new_id) = made else { panic!("a new note: {made:?}") };
    let retired = list.iter().find(|q| q.id == "q1").unwrap();
    assert_eq!(retired.status, "retired");
    assert_eq!(retired.retired_reason.as_deref(), Some("it was the spinner, not the grid"));
    assert_eq!(retired.retired_at.as_deref(), Some("50"));
    let new = list.iter().find(|q| q.id == new_id).unwrap();
    assert!(new.is_active());
    assert_eq!(new.by, "assistant");
    assert_eq!(new.from, "api");
    assert_eq!(new.sources, vec![source(7, &[2, 3], Some("not_found"))]);
}

/// Recording a line that was retired brings it back instead of copying it.
#[test]
fn a_duplicate_of_a_retired_note_reactivates_it() {
    let mut list = vec![quirk("dates render as dd/mm", "assistant", "1")];
    retire_in(&mut list, "q1", Some("gone"), None, true, 10).unwrap();
    let r = record_in(&mut list, "DATES render as dd/mm", "assistant", "autorun", vec![source(4, &[1], None)], 20).unwrap();
    assert_eq!(r, Recorded::Reactivated("q1".into(), Some("gone".into())));
    assert_eq!(list.len(), 1);
    assert!(list[0].is_active());
    assert_eq!(list[0].retired_reason, None);
    assert_eq!(list[0].sources, vec![source(4, &[1], None)]);
}

#[test]
fn restore_edit_and_delete_change_one_note() {
    let mut list = vec![quirk("dates render as dd/mm", "assistant", "1"), quirk("the grid paginates", "person", "2")];
    retire_in(&mut list, "q1", None, None, false, 10).unwrap();
    restore_in(&mut list, "q1").unwrap();
    assert!(list[0].is_active() && list[0].retired_at.is_none());

    edit_in(&mut list, "q2", "  the grid paginates at 25 rows ").unwrap();
    assert_eq!(list[1].text, "the grid paginates at 25 rows");
    assert_eq!(list[1].by, "person", "an edit keeps the author");

    delete_in(&mut list, "q1").unwrap();
    assert_eq!(list.len(), 1);
    assert!(delete_in(&mut list, "q1").is_err());
}

#[test]
fn restoring_into_a_full_list_is_refused() {
    let mut list = full_list();
    let mut retired = quirk("an old one", "assistant", "2");
    retired.id = "qold".into();
    retired.status = "retired".into();
    list.push(retired);
    let err = restore_in(&mut list, "qold").unwrap_err();
    assert!(err.contains("retire one"), "{err}");
}

// ------------------------------------------------------------ evidence

fn save_button() -> Action {
    Action::Click { selector: Target::One(LocatorStep { role: Some("button".into()), name: Some("Save".into()), ..Default::default() }) }
}

fn step(n: i32, outcomes: Vec<ActionOutcome>) -> StepRecord {
    StepRecord { step_number: n, outcomes, screenshot: None }
}

fn case(case_id: i32, steps: Vec<StepRecord>) -> CaseRecord {
    CaseRecord {
        case_id,
        title: "a case".into(),
        verdict: String::new(),
        note: String::new(),
        steps,
        proposed: String::new(),
        reason: String::new(),
        duration_ms: None,
        account: None,
    }
}

fn script_for(case_id: i32) -> CaseScript {
    CaseScript {
        case_id,
        title: "a case".into(),
        account: None,
        steps: vec![
            StepScript { step_number: 2, actions: vec![save_button()], unchecked: None },
            StepScript { step_number: 3, actions: vec![save_button()], unchecked: None },
        ],
        repairs: 0,
        last_repair: None,
    }
}

fn sourced(class: Option<&str>) -> Quirk {
    let mut q = quirk("the save button needs the form to settle", "assistant", "1");
    q.sources = vec![source(7, &[2], class)];
    q
}

#[test]
fn a_source_step_that_passed_confirms_the_note() {
    let mut list = vec![sourced(Some("not_found"))];
    let cases = vec![case(7, vec![step(2, vec![ActionOutcome::passed("clicked button \"Save\"")])])];
    assert!(apply_run_evidence(&mut list, &cases, &[script_for(7)], 1790812800000));
    assert_eq!(list[0].confirmed, 1);
    assert_eq!(list[0].last_confirmed.as_deref(), Some("1790812800000"));
    assert_eq!(list[0].doubted, 0);
}

#[test]
fn the_same_class_of_failure_again_doubts_the_note_and_another_class_does_not() {
    let mut list = vec![sourced(Some("not_found"))];
    let same = vec![case(7, vec![step(2, vec![ActionOutcome::failed("waited 5000ms: button \"Save\" not found")])])];
    assert!(apply_run_evidence(&mut list, &same, &[script_for(7)], 10));
    assert_eq!((list[0].confirmed, list[0].doubted), (0, 1));

    let other = vec![case(7, vec![step(2, vec![ActionOutcome::failed("waited 5000ms: button \"Save\" is disabled")])])];
    assert!(!apply_run_evidence(&mut list, &other, &[script_for(7)], 11));
    assert_eq!((list[0].confirmed, list[0].doubted), (0, 1));

    // With no class on file, any failure of the step counts against it.
    let mut unknown = vec![sourced(None)];
    assert!(apply_run_evidence(&mut unknown, &other, &[script_for(7)], 12));
    assert_eq!(unknown[0].doubted, 1);
}

#[test]
fn steps_that_did_not_run_change_nothing() {
    let mut list = vec![sourced(Some("not_found"))];
    let before = list.clone();
    // The case is not in this run.
    assert!(!apply_run_evidence(&mut list, &[case(8, vec![])], &[], 10));
    // The case ran, but not the step.
    assert!(!apply_run_evidence(&mut list, &[case(7, vec![step(3, vec![ActionOutcome::passed("x")])])], &[], 10));
    // The step was skipped.
    let skipped = vec![case(7, vec![step(2, vec![ActionOutcome::failed("not run: an earlier step of this case failed")])])];
    assert!(!apply_run_evidence(&mut list, &skipped, &[script_for(7)], 10));
    // The browser stopped answering: nothing about the application.
    let silent = vec![case(
        7,
        vec![step(2, vec![ActionOutcome::failed("the browser did not answer for 5000ms while waiting for button \"Save\"")])],
    )];
    assert!(!apply_run_evidence(&mut list, &silent, &[script_for(7)], 10));
    // A retired note gets no evidence at all.
    let mut retired = vec![sourced(None)];
    retired[0].status = "retired".into();
    assert!(!apply_run_evidence(&mut retired, &[case(7, vec![step(2, vec![ActionOutcome::passed("x")])])], &[], 10));
    assert_eq!(list, before);
}

/// The file is written after a run - and when it cannot be, the run is not
/// failed: nothing panics, the old file stays, and the caller is told
/// nothing was written.
#[test]
fn run_evidence_is_written_and_a_write_failure_does_not_fail_the_run() {
    let dir = tempfile::tempdir().unwrap();
    save_quirks(dir.path(), "Acme", "Web", &[sourced(Some("not_found"))]).unwrap();
    let cases = vec![case(7, vec![step(2, vec![ActionOutcome::passed("clicked")])])];
    assert!(record_run_evidence(dir.path(), "Acme", "Web", &cases, 77));
    assert_eq!(load_quirks(dir.path(), "Acme", "Web").unwrap()[0].confirmed, 1);

    // A directory where the temporary file would go: the write fails.
    let tmp = quirks_path(dir.path(), "Acme", "Web").with_extension("json.tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    assert!(!record_run_evidence(dir.path(), "Acme", "Web", &cases, 78));
    assert_eq!(load_quirks(dir.path(), "Acme", "Web").unwrap()[0].confirmed, 1, "the file is as it was");

    // No project, or no quirks file at all: nothing to do, nothing written.
    assert!(!record_run_evidence(dir.path(), "", "Web", &cases, 79));
    assert!(!record_run_evidence(dir.path(), "Acme", "Other", &cases, 79));
}

/// A repair's source is the case, its declared steps, and the class of
/// the failure among them in the newest run - read against the script
/// that ran.
#[test]
fn a_repairs_source_carries_the_class_of_the_failure_that_led_to_it() {
    let run = LocalRun {
        id: "run-1".into(),
        pbi_id: 1,
        started_at: "1".into(),
        cases: vec![case(
            7,
            vec![
                step(2, vec![ActionOutcome::passed("clicked")]),
                step(3, vec![ActionOutcome::failed("waited 5000ms: button \"Save\" is covered by div.modal")]),
            ],
        )],
        mode: "unattended".into(),
        published: None,
    };
    let s = source_for_repair(Some(&run), &script_for(7), 7, &[3, 2, 3]);
    assert_eq!(s, source(7, &[2, 3], Some("covered")));
    assert_eq!(source_for_repair(None, &script_for(7), 7, &[3]), source(7, &[3], None));
}

// ------------------------------------------------------------ review fixes

/// A line a person retired - or a note a person wrote - is the person's
/// decision: the assistant recording it again is refused, naming why it
/// was retired; the person may bring it back.
#[test]
fn the_assistant_never_brings_back_a_note_a_person_retired_or_wrote() {
    let mut list = vec![quirk("dates render as dd/mm", "assistant", "1"), quirk("the grid paginates", "person", "2")];
    retire_in(&mut list, "q1", Some("wrong - it was the locale"), None, false, 10).unwrap();
    retire_in(&mut list, "q2", None, None, false, 11).unwrap();
    assert_eq!(list[0].retired_by.as_deref(), Some("person"));

    let err = record_in(&mut list, "Dates render as dd/mm", "assistant", "autorun", vec![], 20).unwrap_err();
    assert_eq!(err, "a person retired this note (wrong - it was the locale) - ask them to restore it");
    assert_eq!(err, person_retired(Some("wrong - it was the locale")));
    let err = record_in(&mut list, "the grid paginates", "assistant", "api", vec![], 20).unwrap_err();
    assert_eq!(err, "a person retired this note - ask them to restore it");
    assert!(list.iter().all(|q| !q.is_active()), "nothing came back");

    // The person, adding the same line in the app, may.
    let back = record_in(&mut list, "dates render as dd/mm", "person", "autorun", vec![], 30).unwrap();
    assert_eq!(back, Recorded::Reactivated("q1".into(), Some("wrong - it was the locale".into())));
    assert!(list[0].is_active() && list[0].retired_by.is_none());
}

/// An API caller's refusal names the retire tool it has.
#[test]
fn a_full_list_names_the_api_retire_tool_to_an_api_caller() {
    let mut list = full_list();
    let err = record_in(&mut list, "one more", "assistant", "api", vec![], 9999).unwrap_err();
    assert!(err.contains(RETIRE_APP_TOOL), "{err}");
}

/// Candidates: runs say it did not help, then filed with a repair and
/// never confirmed, then never tied to a run ("untested") - oldest first
/// within each.
#[test]
fn candidates_rank_unhelpful_then_unconfirmed_then_untested() {
    let mut untested_old = quirk("an old standing fact", "assistant", "1");
    untested_old.from = "api".into();
    let mut unconfirmed = quirk("filed with a repair", "assistant", "5");
    unconfirmed.sources = vec![source(1, &[1], None)];
    let mut unhelpful = quirk("did not help", "assistant", "9");
    unhelpful.sources = vec![source(2, &[1], None)];
    unhelpful.doubted = 2;
    unhelpful.confirmed = 1;
    let mut helpful = quirk("confirmed often", "assistant", "3");
    helpful.sources = vec![source(3, &[1], None)];
    helpful.confirmed = 4;
    let list = vec![untested_old, unconfirmed, unhelpful, helpful];
    let order: Vec<&str> = retire_candidates(&list).iter().map(|q| q.text.as_str()).collect();
    assert_eq!(order, vec!["did not help", "filed with a repair", "an old standing fact"]);
    let text = cap_refusal(&list, Some(RETIRE_TOOL));
    assert!(text.contains("\"an old standing fact\" (untested - not tied to any run)"), "{text}");
    assert!(text.contains("\"filed with a repair\" (never confirmed by a run)"), "{text}");
}

/// One run counts at most once per source case: three passing steps of
/// one case are one confirmation; a step that failed the same way again
/// outweighs the ones that passed.
#[test]
fn one_run_counts_at_most_once_per_source_case() {
    let mut q = quirk("the form settles late", "assistant", "1");
    q.sources = vec![source(7, &[2, 3], None)];
    let mut list = vec![q];
    let both_pass = vec![case(
        7,
        vec![step(2, vec![ActionOutcome::passed("a")]), step(3, vec![ActionOutcome::passed("b")])],
    )];
    assert!(apply_run_evidence(&mut list, &both_pass, &[script_for(7)], 10));
    assert_eq!((list[0].confirmed, list[0].doubted), (1, 0));

    let mixed = vec![case(
        7,
        vec![
            step(2, vec![ActionOutcome::passed("a")]),
            step(3, vec![ActionOutcome::failed("waited 5000ms: button \"Save\" not found")]),
        ],
    )];
    assert!(apply_run_evidence(&mut list, &mixed, &[script_for(7)], 11));
    assert_eq!((list[0].confirmed, list[0].doubted), (1, 1));
}

/// `cases` on a quirk recorded on its own: each named step must have
/// failed in its case's newest run; the source keeps that failure's class.
#[test]
fn a_named_case_must_have_failed_there() {
    let run = LocalRun {
        id: "run-9".into(),
        pbi_id: 1,
        started_at: "1".into(),
        cases: vec![case(
            7,
            vec![
                step(2, vec![ActionOutcome::passed("clicked")]),
                step(3, vec![ActionOutcome::failed("waited 5000ms: button \"Save\" is covered by div.modal")]),
            ],
        )],
        mode: "unattended".into(),
        published: None,
    };
    let ok = source_from_run(Some(&run), Some(&script_for(7)), 7, &[3]).unwrap();
    assert_eq!(ok, source(7, &[3], Some("covered")));
    let err = source_from_run(Some(&run), Some(&script_for(7)), 7, &[2, 3]).unwrap_err();
    assert!(err.contains("case 7 step 2 did not fail") && err.contains("run-9"), "{err}");
    let err = source_from_run(None, None, 8, &[1]).unwrap_err();
    assert!(err.contains("no run on this machine has case 8"), "{err}");
    assert!(source_from_run(Some(&run), None, 7, &[]).is_err());
}

/// A supervised run is counted from the run file once it is saved.
#[test]
fn a_saved_run_is_counted_by_its_id() {
    let dir = tempfile::tempdir().unwrap();
    save_quirks(dir.path(), "Acme", "Web", &[sourced(None)]).unwrap();
    let run = LocalRun {
        id: "run-55".into(),
        pbi_id: 1,
        started_at: "1".into(),
        cases: vec![case(7, vec![step(2, vec![ActionOutcome::passed("clicked")])])],
        mode: String::new(),
        published: None,
    };
    v2_lib::autorun::store::save_run(dir.path(), &run).unwrap();
    assert!(count_saved_run(dir.path(), "Acme", "Web", "run-55", 5).unwrap());
    assert_eq!(load_quirks(dir.path(), "Acme", "Web").unwrap()[0].confirmed, 1);
    assert!(count_saved_run(dir.path(), "Acme", "Web", "run-56", 5).is_err());
    assert!(count_saved_run(dir.path(), "Acme", "Web", "../x", 5).is_err());
}
