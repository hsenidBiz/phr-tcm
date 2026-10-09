//! A run's recipe and areas file (`autorun::run_files`): each step looks at
//! them, but within one run a file is read once, and again only when its
//! modified time or size changed.
//!
//! Every test works in its own temporary folder and touches no
//! process-wide state, so none takes a `serial` lock.

use crate::common;
use serde_json::json;
use std::path::Path;
use std::time::{Duration, SystemTime};
use v2_lib::autorun::lease::Held;
use v2_lib::autorun::nav::{nav_path, save_nav, NavFile};
use v2_lib::autorun::recipe::save_recipe;
use v2_lib::autorun::run_files::RunFiles;
use v2_lib::autorun::runner::{run_step_in_run, AreaRoute, InRun, NEEDS_SCRIPT_AREA};
use v2_lib::autorun::StepScript;
use v2_lib::browser::timing::Timing;

const ORG: &str = "acme";
const PROJECT: &str = "PMS";

fn quick() -> Timing {
    Timing { action_ms: 300, expect_ms: 300, nav_ms: 300, poll_ms: 20, highlight_ms: 0, lease_wait_ms: 300 }
}

/// A project with a saved recipe, an environments file and an areas file.
fn project(root: &Path) {
    v2_lib::environments::load_or_init(root, None).unwrap();
    save_recipe(root, ORG, PROJECT, &common::recipe()).unwrap();
    save_nav(root, ORG, PROJECT, &NavFile::default()).unwrap();
}

/// One step of a run that shares `files`, with nothing to do: only the
/// files it looks at before its actions matter here.
async fn one_step(root: &Path, files: &RunFiles) {
    let step: StepScript = serde_json::from_value(json!({ "step_number": 1, "actions": [] })).unwrap();
    let mut d = common::FakePage::default().driver();
    let mut account = None;
    let mut lease = Held::supervised();
    let mut run = InRun { files: Some(files), ..Default::default() };
    let area = AreaRoute::Unknown(NEEDS_SCRIPT_AREA);
    run_step_in_run(&mut d, root, ORG, PROJECT, &step, &quick(), &mut account, &mut lease, None, area, &mut run)
        .await
        .unwrap();
}

#[tokio::test]
async fn the_recipe_is_read_once_per_run() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let files = RunFiles::default();
    for _ in 0..4 {
        one_step(dir.path(), &files).await;
    }
    assert_eq!(files.reads(), 2, "the recipe and the areas file, once each, for four steps");
    // What the run keeps is what is on disk.
    assert_eq!(files.recipe(dir.path(), ORG, PROJECT).unwrap(), Some(common::recipe()));
    assert_eq!(files.reads(), 2);

    // A second run starts afresh: nothing carries over.
    let next = RunFiles::default();
    one_step(dir.path(), &next).await;
    assert_eq!(next.reads(), 2);
}

/// The supervised run's promise: a recipe or an areas file changed between
/// two steps is what the next step sees - a new size, or only a new
/// modified time.
#[tokio::test]
async fn a_recipe_changed_between_steps_is_read_again() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root);
    let files = RunFiles::default();
    one_step(root, &files).await;
    assert_eq!(files.reads(), 2);

    // A new size: another start address.
    let mut changed = common::recipe();
    changed.start_url = "https://hr.example.internal/other/".to_string();
    save_recipe(root, ORG, PROJECT, &changed).unwrap();
    one_step(root, &files).await;
    assert_eq!(files.reads(), 3, "the changed recipe was not read again");
    assert_eq!(files.recipe(root, ORG, PROJECT).unwrap().map(|r| r.start_url), Some(changed.start_url.clone()));
    assert_eq!(files.reads(), 3, "read again although nothing changed");

    // The areas switch flipped between two steps.
    save_nav(root, ORG, PROJECT, &NavFile { direct_urls: false, ..NavFile::default() }).unwrap();
    one_step(root, &files).await;
    assert_eq!(files.reads(), 4, "the changed areas file was not read again");
    assert!(!files.nav(root, ORG, PROJECT).unwrap().direct_urls);

    // The same size, only a new modified time: still read again.
    let path = nav_path(root, ORG, PROJECT);
    let later = SystemTime::now() + Duration::from_secs(60);
    std::fs::OpenOptions::new().write(true).open(&path).unwrap().set_modified(later).unwrap();
    one_step(root, &files).await;
    assert_eq!(files.reads(), 5, "a file with a new modified time was not read again");

    // A file that goes away is read again too, and reads as absent.
    std::fs::remove_file(&path).unwrap();
    one_step(root, &files).await;
    assert_eq!(files.reads(), 6);
    assert!(files.nav(root, ORG, PROJECT).unwrap().direct_urls, "a missing areas file reads as the default");
    assert_eq!(files.reads(), 6);
}

/// A failed read is never kept: the next step reads again and says why.
#[tokio::test]
async fn an_unreadable_file_is_read_again_and_fails_each_step() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root);
    std::fs::write(nav_path(root, ORG, PROJECT), "{ not json").unwrap();
    let files = RunFiles::default();
    let err = files.nav(root, ORG, PROJECT).unwrap_err();
    assert!(err.starts_with("the areas file is not readable"), "{err}");
    assert!(files.nav(root, ORG, PROJECT).is_err());
    assert_eq!(files.reads(), 2, "a failed read was kept");
}
