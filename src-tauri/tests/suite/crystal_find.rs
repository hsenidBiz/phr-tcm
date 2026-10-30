//! The hidden module's find and replace: hits across the report and its
//! subreports, a dry run that lists and saves nothing, and a replace that is
//! saved like any edit and never touches text that holds fields.

use std::path::Path;

use serde_json::Value;
use v2_lib::commands::crystal as screen;
use v2_lib::crystal::{self, ReadFilter};

use crate::serial;

const SAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../crystal-reports/Sample Report");
const SAMPLE: &str = "PMSV10_GoalAlignCascade.rpt";

fn copy_sample(dir: &Path, name: &str) -> String {
    let to = dir.join(name);
    std::fs::copy(Path::new(SAMPLES).join(name), &to).expect("the sample copies");
    to.to_string_lossy().into_owned()
}

fn objects(ir: &Value) -> &Vec<Value> {
    ir["objects"].as_array().expect("the IR lists its objects")
}

/// A plain text object with a few visible letters in it.
fn editable_text(ir: &Value) -> Value {
    objects(ir)
        .iter()
        .find(|o| {
            o["text"]["editable"] == true && o["text"]["display"].as_str().is_some_and(|t| t.trim().len() >= 3)
        })
        .cloned()
        .expect("the sample has a plain text object")
}

fn display(o: &Value) -> String {
    o["text"]["display"].as_str().unwrap().to_string()
}

fn open_sample(dir: &Path) -> (String, Value) {
    let path = copy_sample(dir, SAMPLE);
    let ir = crystal::open(&path).unwrap();
    (path, ir)
}

#[test]
fn find_lists_text_and_field_hits() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let (_, ir) = open_sample(dir.path());

    let text = editable_text(&ir);
    let hits = crystal::find(&display(&text), true).unwrap();
    let hit = hits.iter().find(|h| h.object_id == text["id"].as_str().unwrap()).expect("the text is a hit");
    assert_eq!((hit.matched.as_str(), hit.replaceable, hit.subreport.clone()), ("text", true, None));
    assert_eq!(hit.name, text["name"].as_str().unwrap());

    let field = objects(&ir).iter().find(|o| o["field"].is_object()).expect("a field object");
    let source = field["field"]["data_source"].as_str().unwrap();
    let hits = crystal::find(source, true).unwrap();
    let hit = hits.iter().find(|h| h.object_id == field["id"].as_str().unwrap()).expect("the field is a hit");
    assert_eq!((hit.matched.as_str(), hit.replaceable, hit.text.as_str()), ("field", false, source));

    assert_eq!(crystal::find("", false).unwrap_err(), "Give the text to find.");
    crystal::close();
}

#[test]
fn case_is_ignored_unless_asked() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let (_, ir) = open_sample(dir.path());
    let text = editable_text(&ir);
    let id = text["id"].as_str().unwrap();
    let wrong_case: String =
        display(&text).chars().map(|c| if c.is_ascii_lowercase() { c.to_ascii_uppercase() } else { c.to_ascii_lowercase() }).collect();
    assert_ne!(wrong_case, display(&text), "the sample text has letters to flip");
    assert!(crystal::find(&wrong_case, false).unwrap().iter().any(|h| h.object_id == id));
    assert!(crystal::find(&wrong_case, true).unwrap().iter().all(|h| h.object_id != id));
    crystal::close();
}

#[test]
fn a_dry_run_lists_changes_and_changes_nothing() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let (path, ir) = open_sample(dir.path());
    let text = editable_text(&ir);
    let old = display(&text);
    let before = std::fs::read(&path).unwrap();

    let out = crystal::replace(&old, "Replaced by a dry run", true, true).unwrap();
    assert!(!out.applied);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let change = out.changes.iter().find(|c| c.object_id == text["id"].as_str().unwrap()).expect("listed");
    assert_eq!((change.before.as_str(), change.after.as_str()), (old.as_str(), "Replaced by a dry run"));
    assert_eq!(change.subreport, None);
    assert!(out.replaced >= 1);
    crystal::close();
}

#[test]
fn a_replace_is_saved_like_any_edit() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let (path, ir) = open_sample(dir.path());
    let text = editable_text(&ir);

    let out = crystal::replace(&display(&text), "Brand new words", true, false).unwrap();
    assert!(out.applied && out.replaced >= 1, "{out:?}");

    // The open report agrees, and so does the file read back fresh.
    let now = crystal::read(&ReadFilter::default()).unwrap();
    assert!(objects(&now).iter().any(|o| o["text"]["display"] == "Brand new words"));
    crystal::open(&path).unwrap();
    let reread = crystal::read(&ReadFilter::default()).unwrap();
    assert!(objects(&reread).iter().any(|o| o["text"]["display"] == "Brand new words"));
    crystal::close();
}

/// The object's record without its id (a replace may renumber ids).
fn record(o: &Value) -> Value {
    let mut o = o.clone();
    o.as_object_mut().unwrap().remove("id");
    o
}

/// A tracked fixture with both what the six samples lack: a text object that holds
/// fields, and a subreport with plain text.
const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../crystal-reports/tests/fixtures/reports/benbrahim777/Top5USAwithSub.rpt"
);

fn copy_fixture(dir: &Path) -> String {
    let to = dir.join("Top5USAwithSub.rpt");
    std::fs::copy(FIXTURE, &to).unwrap_or_else(|e| panic!("the fixture {FIXTURE} is missing: {e}"));
    to.to_string_lossy().into_owned()
}

#[test]
fn text_holding_fields_is_found_but_never_replaced() {
    use rpt_core::rpt_reader::model::ReportObjectKind;
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let path = copy_fixture(dir.path());
    let ir = crystal::open(&path).unwrap();

    // The report's own model says which text objects hold fields.
    let rpt = rpt_core::open_bytes(&std::fs::read(&path).unwrap()).unwrap();
    let holders: Vec<String> = rpt
        .report()
        .report_definition
        .areas
        .iter()
        .flat_map(|a| a.sections.iter())
        .flat_map(|s| s.objects.iter())
        .filter(|o| matches!(&o.kind, ReportObjectKind::Text(t) if !t.embedded_fields.is_empty() && t.display.trim().len() >= 3))
        .map(|o| o.name.clone())
        .collect();
    let all = objects(&ir);
    let locked = holders
        .iter()
        .find_map(|n| {
            let same: Vec<&Value> = all.iter().filter(|o| &o["name"] == n).collect();
            (same.len() == 1).then(|| same[0].clone())
        })
        .unwrap_or_else(|| panic!("the fixture has no text object that holds fields (found {holders:?}); this test would prove nothing"));
    assert_eq!(locked["text"]["editable"], false);
    assert!(!locked["field"].is_object(), "a text object, not a field object");

    let shown = display(&locked);
    let hits = crystal::find(&shown, true).unwrap();
    let hit = hits.iter().find(|h| h.object_id == locked["id"].as_str().unwrap()).expect("listed");
    assert!(!hit.replaceable && hit.matched == "text");

    let out = crystal::replace(&shown, "X", true, false).unwrap();
    assert!(out.skipped_locked >= 1, "{out:?}");
    assert!(out.changes.iter().all(|c| c.object_id != locked["id"].as_str().unwrap()), "{out:?}");
    let now = crystal::read(&ReadFilter::default()).unwrap();
    let same = objects(&now).iter().find(|o| o["name"] == locked["name"]);
    assert_eq!(record(same.expect("still there")), record(&locked));
    // Read back from the file: the object is as it was.
    crystal::open(&path).unwrap();
    let reread = crystal::read(&ReadFilter::default()).unwrap();
    let same = objects(&reread).iter().find(|o| o["name"] == locked["name"]);
    assert_eq!(record(same.expect("still there")), record(&locked));
    crystal::close();
}

/// The fixture's subreport with a plain text object: (path, subreport, its text).
fn sample_with_subreport_text(dir: &Path) -> (String, String, String) {
    let path = copy_fixture(dir);
    let rpt = rpt_core::open_bytes(&std::fs::read(&path).unwrap()).unwrap();
    for sub in &rpt.report().subreports {
        let Ok(sir) = rpt_core::subedit::subreport_ir(&rpt, &sub.name) else { continue };
        let found =
            sir.objects.iter().find(|o| o.text.as_ref().is_some_and(|t| t.editable && t.display.trim().len() >= 3));
        if let Some(o) = found {
            return (path, sub.name.clone(), o.text.as_ref().unwrap().display.clone());
        }
    }
    panic!("the fixture has no subreport with plain text; these tests would prove nothing");
}

#[test]
fn a_replace_in_subreport_text_is_saved_and_reads_back() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let (path, sub, shown) = sample_with_subreport_text(dir.path());
    crystal::open(&path).unwrap();
    let main_before = crystal::read(&ReadFilter::default()).unwrap();

    let out = crystal::replace(&shown, "Sub text changed", true, false).unwrap();
    assert!(out.applied && out.changes.iter().any(|c| c.subreport.as_deref() == Some(sub.as_str())), "{out:?}");

    // The file read back fresh holds the new text in that subreport.
    let after = rpt_core::open_bytes(&std::fs::read(&path).unwrap()).unwrap();
    let sub_after = rpt_core::subedit::subreport_ir(&after, &sub).expect("the subreport still opens");
    assert!(
        sub_after.objects.iter().any(|o| o.text.as_ref().is_some_and(|t| t.display.contains("Sub text changed"))),
        "the new text is not in the subreport"
    );
    assert!(
        !sub_after.objects.iter().any(|o| o.text.as_ref().is_some_and(|t| t.editable && t.display == shown)),
        "the old text is still there"
    );
    // Main-report objects that did not hold the text are unchanged.
    crystal::open(&path).unwrap();
    let main_after = crystal::read(&ReadFilter::default()).unwrap();
    let kept = |ir: &Value| -> Vec<Value> {
        objects(ir)
            .iter()
            .filter(|o| !o["text"]["display"].as_str().is_some_and(|t| t.contains(&shown)))
            .map(record)
            .collect()
    };
    assert_eq!(kept(&main_after), kept(&main_before));
    crystal::close();
}

#[test]
fn a_dry_run_names_the_subreport() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let (path, sub, shown) = sample_with_subreport_text(dir.path());
    let before = std::fs::read(&path).unwrap();
    crystal::open(&path).unwrap();
    let hits = crystal::find(&shown, true).unwrap();
    assert!(hits.iter().any(|h| h.subreport.as_deref() == Some(sub.as_str())));
    let out = crystal::replace(&shown, "Sub text changed", true, true).unwrap();
    assert!(!out.applied && out.changes.iter().any(|c| c.subreport.as_deref() == Some(sub.as_str())), "{out:?}");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    crystal::close();
}

#[test]
fn a_replace_that_expects_another_count_applies_nothing() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let (path, ir) = open_sample(dir.path());
    let text = editable_text(&ir);
    let before = std::fs::read(&path).unwrap();
    let dry = crystal::replace(&display(&text), "Reviewed", true, true).unwrap();

    let said = crystal::replace_expecting(&display(&text), "Reviewed", true, false, Some(dry.replaced + 1)).unwrap_err();
    assert!(said.contains("changed since") && said.contains("review"), "{said}");
    assert_eq!(std::fs::read(&path).unwrap(), before);

    let out = crystal::replace_expecting(&display(&text), "Reviewed", true, false, Some(dry.replaced)).unwrap();
    assert!(out.applied && out.replaced == dry.replaced, "{out:?}");
    crystal::close();
}

#[test]
fn a_replace_that_changes_nothing_does_not_rewrite_the_file() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let (path, ir) = open_sample(dir.path());
    let text = editable_text(&ir);
    let shown = display(&text);
    let before = std::fs::read(&path).unwrap();
    // Replacing the text with itself is no change.
    let out = crystal::replace(&shown, &shown, true, false).unwrap();
    assert_eq!((out.replaced, out.applied, out.changes.len()), (0, false, 0), "{out:?}");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    crystal::close();
}

#[test]
fn no_match_saves_nothing() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let (path, _) = open_sample(dir.path());
    let before = std::fs::read(&path).unwrap();
    let out = crystal::replace("zz-no-such-text-zz", "x", false, false).unwrap();
    assert_eq!((out.replaced, out.applied, out.changes.len()), (0, false, 0));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(crystal::replace("", "x", false, false).unwrap_err(), "Give the text to find.");
    crystal::close();
}

#[test]
fn a_replace_that_cannot_save_changes_nothing() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let (path, ir) = open_sample(dir.path());
    let text = editable_text(&ir);
    let before = std::fs::read(&path).unwrap();
    // A read-only file does not stop a rename over it on every platform, so make the
    // folder refuse the temp file instead: a directory sits where the temp file goes.
    let temp = dir.path().join(format!(".{SAMPLE}.{}.saving", std::process::id()));
    std::fs::create_dir(&temp).unwrap();

    let said = crystal::replace(&display(&text), "Nope", true, false).unwrap_err();
    assert!(said.contains("Could not save"), "{said}");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let now = crystal::read(&ReadFilter::default()).unwrap();
    assert!(objects(&now).iter().any(|o| o["id"] == text["id"] && o["text"]["display"] == text["text"]["display"]));
    std::fs::remove_dir(&temp).unwrap();
    crystal::close();
}

#[test]
fn the_screen_commands_follow_the_module_switch() {
    let _e = serial::extras();
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let (_, ir) = open_sample(dir.path());
    let off = v2_lib::modules::Module::CrystalReports.off_sentence();
    assert_eq!(screen::find_with(false, "a", false).unwrap_err(), off);
    assert_eq!(screen::replace_with(false, "a", "b", false, true, None).unwrap_err(), off);
    let text = editable_text(&ir);
    assert!(!screen::find_with(true, &display(&text), true).unwrap().is_empty());
    let out = screen::replace_with(true, &display(&text), "Typed on the screen", true, true, None).unwrap();
    assert!(!out.applied && out.replaced >= 1);
    crystal::close();
}
