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
const ALL: [&str; 6] = [
    "PMSV10_GoalAlignCascade.rpt",
    "PMSV10_GoalAlignCascadeSS.rpt",
    "PMSV10_IndDetailedEval.rpt",
    "PMSV10_IndDetailedEvalSS.rpt",
    "PMSV10_IndPerfOverview.rpt",
    "PMSV10_IndPerfOverviewSS.rpt",
];

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

#[test]
fn text_holding_fields_is_found_but_never_replaced() {
    let _lock = serial::crystal();
    for sample in ALL {
        let dir = tempfile::tempdir().unwrap();
        let path = copy_sample(dir.path(), sample);
        let ir = crystal::open(&path).unwrap();
        let Some(locked) = objects(&ir).iter().find(|o| {
            o["text"]["editable"] == false && o["text"]["display"].as_str().is_some_and(|t| !t.trim().is_empty())
        }) else {
            continue;
        };
        let shown = display(locked);
        let hits = crystal::find(&shown, true).unwrap();
        let hit = hits.iter().find(|h| h.object_id == locked["id"].as_str().unwrap()).expect("listed");
        assert!(!hit.replaceable, "{sample}");

        let out = crystal::replace(&shown, "X", true, false).unwrap();
        assert!(out.skipped_locked >= 1, "{sample}: {out:?}");
        let now = crystal::read(&ReadFilter::default()).unwrap();
        let same = objects(&now).iter().find(|o| o["name"] == locked["name"] && o["kind"] == locked["kind"]);
        let same = same.expect("still there");
        assert_eq!(same["text"]["display"], locked["text"]["display"], "{sample}");
        crystal::close();
        return;
    }
    eprintln!("no sample has text that holds fields; the locked-text rule is untested here");
    crystal::close();
}

#[test]
fn a_replace_reaches_subreport_text_and_a_dry_run_names_the_subreport() {
    let _lock = serial::crystal();
    for sample in ALL {
        let dir = tempfile::tempdir().unwrap();
        let path = copy_sample(dir.path(), sample);
        crystal::open(&path).unwrap();
        let reread = rpt_core::open_bytes(&std::fs::read(&path).unwrap()).unwrap();
        for sub in &reread.report().subreports {
            let Ok(sir) = rpt_core::subedit::subreport_ir(&reread, &sub.name) else { continue };
            let Some(o) =
                sir.objects.iter().find(|o| o.text.as_ref().is_some_and(|t| t.editable && t.display.trim().len() >= 3))
            else {
                continue;
            };
            let shown = o.text.as_ref().unwrap().display.clone();
            let hits = crystal::find(&shown, true).unwrap();
            assert!(hits.iter().any(|h| h.subreport.as_deref() == Some(sub.name.as_str())), "{sample}");
            let out = crystal::replace(&shown, "Sub text changed", true, true).unwrap();
            assert!(out.changes.iter().any(|c| c.subreport.as_deref() == Some(sub.name.as_str())), "{out:?}");
            crystal::close();
            return;
        }
    }
    eprintln!("no sample has a subreport with plain text; subreport replace is untested here");
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
    assert_eq!(screen::replace_with(false, "a", "b", false, true).unwrap_err(), off);
    let text = editable_text(&ir);
    assert!(!screen::find_with(true, &display(&text), true).unwrap().is_empty());
    let out = screen::replace_with(true, &display(&text), "Typed on the screen", true, true).unwrap();
    assert!(!out.applied && out.replaced >= 1);
    crystal::close();
}
