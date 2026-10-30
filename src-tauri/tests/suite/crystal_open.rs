//! The Crystal Reports screen's Open report button: `crystal_open_file`
//! makes a picked file the open report through the same `crystal::open` the
//! assistant uses, and a refusal on screen names no path.

use std::path::Path;

use v2_lib::commands::crystal as screen;
use v2_lib::crystal;
use v2_lib::modules::Module;

use crate::serial;

const SAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../crystal-reports/Sample Report");
const SAMPLE: &str = "PMSV10_GoalAlignCascade.rpt";

fn copy_sample(dir: &Path, name: &str) -> String {
    let to = dir.join(name);
    std::fs::copy(Path::new(SAMPLES).join(name), &to).expect("the sample copies");
    to.to_string_lossy().into_owned()
}

#[test]
fn the_screen_opens_a_picked_file_as_the_open_report() {
    let _lock = serial::crystal();
    crystal::close();
    let dir = tempfile::tempdir().unwrap();
    let path = copy_sample(dir.path(), SAMPLE);

    screen::open_file_with(true, &path).expect("a sample opens");
    assert_eq!(crystal::current_path().as_deref(), Some(path.as_str()));
    let shown = screen::current_with(true).unwrap().expect("the screen sees it");
    assert_eq!(shown.preview.path, path);
    crystal::close();
}

#[test]
fn opening_another_report_replaces_the_open_one() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let a = copy_sample(dir.path(), SAMPLE);
    let other = dir.path().join("other");
    std::fs::create_dir(&other).unwrap();
    let b = copy_sample(&other, SAMPLE);

    crystal::open(&a).unwrap();
    screen::open_file_with(true, &b).unwrap();
    assert_eq!(crystal::current_path().as_deref(), Some(b.as_str()));
    crystal::close();
}

#[test]
fn a_bad_pick_is_refused_without_its_path() {
    let _lock = serial::crystal();
    crystal::close();
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().to_string_lossy().into_owned();
    let txt = dir.path().join("notes.txt");
    std::fs::write(&txt, "x").unwrap();

    let said = screen::open_file_with(true, &txt.to_string_lossy()).unwrap_err();
    assert!(said.contains("notes.txt"), "{said}");
    assert!(!said.contains(&folder), "{said}");

    let gone = dir.path().join("gone.rpt");
    let said = screen::open_file_with(true, &gone.to_string_lossy()).unwrap_err();
    assert!(said.contains("gone.rpt"), "{said}");
    assert!(!said.contains(&folder), "{said}");
    assert!(crystal::current_path().is_none());
}

#[test]
fn open_file_is_refused_while_the_module_is_off() {
    let _e = serial::extras();
    let _lock = serial::crystal();
    crystal::close();
    let dir = tempfile::tempdir().unwrap();
    let path = copy_sample(dir.path(), SAMPLE);

    assert_eq!(screen::open_file_with(false, &path).unwrap_err(), Module::CrystalReports.off_sentence());
    assert!(crystal::current_path().is_none());
}
