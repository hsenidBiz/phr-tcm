//! The open Crystal Reports report (`crystal`): open, read, edit and save,
//! compare, check, new and preview, against copies of the vendored samples.
//!
//! Every test works on a copy in its own temp dir - the samples beside the
//! bundle are never written - and takes the `crystal` lock, because the
//! open report is one process-wide slot.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use v2_lib::crystal::{self, ReadFilter};

use crate::serial;

const SAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../crystal-reports/Sample Report");
const SAMPLE: &str = "PMSV10_GoalAlignCascade.rpt";

/// A copy of `name` from the samples in `dir`, as a path string.
fn copy_sample(dir: &Path, name: &str) -> String {
    let to = dir.join(name);
    std::fs::copy(Path::new(SAMPLES).join(name), &to).expect("the sample copies");
    to.to_string_lossy().into_owned()
}

fn objects(ir: &Value) -> &Vec<Value> {
    ir["objects"].as_array().expect("the IR lists its objects")
}

/// A Text object `set_text` can rewrite, whose name picks it out alone.
fn editable_text(ir: &Value) -> Value {
    let all = objects(ir);
    all.iter()
        .find(|o| {
            let name = o["name"].as_str().unwrap_or("");
            o["kind"] == "Text"
                && o["text"]["editable"] == true
                && !name.is_empty()
                && all.iter().filter(|p| p["name"].as_str().unwrap_or("").eq_ignore_ascii_case(name)).count() == 1
        })
        .cloned()
        .expect("the sample has a plain text object with a unique name")
}

fn object_named<'a>(ir: &'a Value, name: &str) -> &'a Value {
    objects(ir).iter().find(|o| o["name"] == name).expect("the object is in the IR")
}

/// `$.report_definition.areas[0].sections[1].objects[2]` out of a compare
/// line: the object a difference belongs to.
fn object_path(line: &str) -> Option<&str> {
    let at = line.find(".objects[")?;
    let close = line[at..].find(']')?;
    Some(&line[..at + close + 1])
}

#[test]
fn opening_returns_the_ir_and_reading_can_narrow_it() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let path = copy_sample(dir.path(), SAMPLE);

    let ir = crystal::open(&path).expect("a sample opens");
    assert_eq!(ir["format"], "crpt-ir/1");
    assert_eq!(crystal::current_path().as_deref(), Some(path.as_str()));

    let whole = crystal::read(&ReadFilter::default()).unwrap();
    assert_eq!(objects(&whole).len(), objects(&ir).len());

    let first = objects(&ir).iter().find(|o| !o["name"].as_str().unwrap_or("").is_empty()).unwrap().clone();
    let name = first["name"].as_str().unwrap().to_string();
    let one = crystal::read(&ReadFilter { section: None, object: Some(name.clone()) }).unwrap();
    assert_eq!(objects(&one).len(), 1, "{one}");
    assert_eq!(objects(&one)[0]["name"], name.as_str());
    let by_id = crystal::read(&ReadFilter { section: None, object: first["id"].as_str().map(String::from) }).unwrap();
    assert_eq!(objects(&by_id)[0]["id"], first["id"]);

    let section = first["section"].as_str().unwrap().to_string();
    let narrowed = crystal::read(&ReadFilter { section: Some(section.clone()), object: None }).unwrap();
    let areas = narrowed["areas"].as_array().unwrap();
    assert_eq!(areas.len(), 1);
    let sections = areas[0]["sections"].as_array().unwrap();
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0]["name"], section.as_str());
    let ids: Vec<&Value> = sections[0]["object_ids"].as_array().unwrap().iter().collect();
    assert_eq!(objects(&narrowed).len(), ids.len());
    assert!(objects(&narrowed).iter().all(|o| o["section"] == section.as_str()));

    let err = crystal::read(&ReadFilter { section: None, object: Some("no-such".into()) }).unwrap_err();
    assert!(err.contains("no-such"), "{err}");
    crystal::close();
    assert_eq!(crystal::current_path(), None);
    assert!(crystal::read(&ReadFilter::default()).is_err());
}

#[test]
fn an_edit_is_saved_to_disk_and_only_that_changes() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let path = copy_sample(dir.path(), SAMPLE);
    let original = dir.path().join("original.rpt");
    std::fs::copy(&path, &original).unwrap();

    let ir = crystal::open(&path).unwrap();
    let target = editable_text(&ir);
    let name = target["name"].as_str().unwrap();
    let after =
        crystal::edit(Some(json!([{ "op": "set_text", "object": { "name": name }, "text": "Edited by TCM" }])), None)
            .expect("the edit is accepted");
    assert_eq!(object_named(&after, name)["text"]["text"], "Edited by TCM");

    // The file on disk is what changed: read independently of the open report.
    let lines = crystal::compare(original.to_str().unwrap(), &path).unwrap();
    assert!(!lines.is_empty(), "the saved file differs from the original");
    let paths: std::collections::BTreeSet<Option<&str>> = lines.iter().map(|l| object_path(l)).collect();
    assert_eq!(paths.len(), 1, "every difference is in one object: {lines:#?}");
    assert!(paths.iter().next().unwrap().is_some(), "{lines:#?}");
    assert!(lines.iter().any(|l| l.contains("Edited by TCM")), "{lines:#?}");

    // A plan (the camelCase vocabulary) is the other way in, and saves the same way.
    let after = crystal::edit(
        None,
        Some(
            json!({ "planVersion": 1, "operations": [{ "action": "setFontSize", "target": name, "fontSizePt": 17 }] }),
        ),
    )
    .expect("the plan is accepted");
    assert_eq!(object_named(&after, name)["text"]["font"]["size_pt"], 17.0);
    let reread = rpt_core::open(&path).unwrap();
    let reread = rpt_core::build_ir(&reread, &rpt_core::locate::locate(&reread), &path);
    let reread = serde_json::to_value(&reread).unwrap();
    assert_eq!(object_named(&reread, name)["text"]["font"]["size_pt"], 17.0);
    assert_eq!(object_named(&reread, name)["text"]["text"], "Edited by TCM");

    let both = crystal::edit(Some(json!([])), Some(json!({ "operations": [] }))).unwrap_err();
    assert!(both.contains("not both"), "{both}");
    let neither = crystal::edit(None, None).unwrap_err();
    assert!(neither.contains("ops") && neither.contains("plan"), "{neither}");
    crystal::close();
}

#[test]
fn a_refused_batch_leaves_the_file_and_reloads() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let path = copy_sample(dir.path(), SAMPLE);
    let ir = crystal::open(&path).unwrap();
    let name = editable_text(&ir)["name"].as_str().unwrap().to_string();
    let before = std::fs::read(&path).unwrap();

    let err = crystal::edit(
        Some(json!([
            { "op": "set_text", "object": { "name": name }, "text": "Never saved" },
            { "op": "set_text", "object": { "name": "no-such" }, "text": "x" },
        ])),
        None,
    )
    .unwrap_err();
    assert!(err.ends_with("The report was read again from disk: nothing in this batch was saved."), "{err}");
    assert_eq!(std::fs::read(&path).unwrap(), before, "the file is byte for byte as it was");
    let now = crystal::read(&ReadFilter::default()).unwrap();
    assert_ne!(object_named(&now, &name)["text"]["text"], "Never saved");
    crystal::close();
}

#[test]
fn a_locked_or_missing_file_is_refused_in_a_sentence() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();

    let missing = dir.path().join("Missing Report.rpt");
    let err = crystal::open(missing.to_str().unwrap()).unwrap_err();
    assert!(err.contains("Missing Report.rpt"), "{err}");

    let text = dir.path().join("notes.txt");
    std::fs::write(&text, "not a report").unwrap();
    let err = crystal::open(text.to_str().unwrap()).unwrap_err();
    assert!(err.contains("notes.txt") && err.contains("not a .rpt file"), "{err}");

    let path = copy_sample(dir.path(), SAMPLE);
    let ir = crystal::open(&path).unwrap();
    let name = editable_text(&ir)["name"].as_str().unwrap().to_string();
    let before = std::fs::read(&path).unwrap();
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&path, perms.clone()).unwrap();

    let result = crystal::edit(Some(json!([{ "op": "set_text", "object": { "name": name }, "text": "Locked" }])), None);
    let unchanged = std::fs::read(&path).unwrap() == before;
    let still = crystal::read(&ReadFilter::default()).map(|ir| object_named(&ir, &name)["text"]["text"].clone());
    let leftovers: Vec<PathBuf> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| *p != text && p.to_string_lossy() != path)
        .collect();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    std::fs::set_permissions(&path, perms).unwrap();

    let err = result.unwrap_err();
    assert!(err.contains(SAMPLE), "{err}");
    assert!(unchanged, "a read-only file is left as it was");
    assert_ne!(still.unwrap(), "Locked", "the open report keeps its old state");
    assert!(leftovers.is_empty(), "no half-written file is left behind: {leftovers:?}");
    crystal::close();
}

#[test]
fn new_refuses_an_existing_file_and_opens_a_blank_one() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let existing = copy_sample(dir.path(), SAMPLE);
    let before = std::fs::read(&existing).unwrap();
    let err = crystal::new_report(&existing, Default::default()).unwrap_err();
    assert!(err.contains(SAMPLE), "{err}");
    assert_eq!(std::fs::read(&existing).unwrap(), before);

    let fresh = dir.path().join("Fresh.rpt");
    let opts = rpt_core::blank::NewReportOptions { title: Some("Fresh report".into()), ..Default::default() };
    let ir = crystal::new_report(fresh.to_str().unwrap(), opts).expect("a blank report is created");
    assert_eq!(ir["format"], "crpt-ir/1");
    assert!(fresh.is_file());
    assert_eq!(crystal::current_path().as_deref(), fresh.to_str());
    assert_eq!(ir["summary"]["title"], "Fresh report");
    crystal::close();
}

#[test]
fn check_reports_no_errors_on_a_sample() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let path = copy_sample(dir.path(), SAMPLE);

    let report = crystal::check(Some(&path)).unwrap();
    assert_eq!(report["errors"], json!([]), "{report}");
    assert!(report["warnings"].is_array());

    crystal::close();
    assert!(crystal::check(None).is_err(), "with nothing open and no path there is nothing to check");
    crystal::open(&path).unwrap();
    assert_eq!(crystal::check(None).unwrap()["errors"], json!([]));
    crystal::close();
}

#[test]
fn a_saved_preview_has_pages() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let path = copy_sample(dir.path(), SAMPLE);

    crystal::open(&path).unwrap();
    // Opening already rendered one, so the screen always has something to show.
    let (auto, pdf) = crystal::last_pdf().expect("opening renders a preview");
    assert!(auto.page_count > 0 && pdf.starts_with(b"%PDF"), "{auto:?}");

    let info = crystal::preview_saved().unwrap();
    assert_eq!(info.path, path);
    assert!(info.page_count > 0);
    assert_eq!(info.live, None);
    let (stored, pdf) = crystal::last_pdf().unwrap();
    assert_eq!(stored.page_count, info.page_count);
    assert!(pdf.starts_with(b"%PDF"));

    crystal::close();
    assert!(crystal::last_pdf().is_none());
    assert!(crystal::preview_saved().is_err());
}

#[test]
fn the_guide_names_every_tool_and_the_units() {
    let guide = crystal::guide();
    for tool in [
        "open_report",
        "read_report",
        "edit_report",
        "compare_reports",
        "check_report",
        "new_report",
        "preview_report",
        "preview_report_live",
        "get_report_guide",
    ] {
        assert!(guide.contains(tool), "the guide names {tool}");
    }
    for term in ["twips", "1440", "#RRGGBB", "git"] {
        assert!(guide.contains(term), "the guide mentions {term}");
    }
}
