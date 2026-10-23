//! Rename folder (`pw_repo::rename_folder`): what a rename would break is
//! found by reading the clone's files, a rename with breakage needs
//! confirming, the app's own export placements move with the folder, and
//! nothing in the repo is rewritten or deleted.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use v2_lib::pw_export::mapping::{self, ExportMap, Placement};
use v2_lib::pw_repo::rename_folder::{check, rename, RenameCheck};

/// A fake clone with a `performance` feature, a neighbour and the repo's own
/// files, and a folder for the app's export placements.
struct Fake {
    clone: tempfile::TempDir,
    maps: tempfile::TempDir,
}

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

fn fake() -> Fake {
    let clone = tempfile::tempdir().unwrap();
    let c = clone.path();
    write(c, "suites/sl/admin/performance/specs/p.spec.ts", "import { x } from \"../pages/x\";\n");
    write(c, "suites/sl/admin/performance/pages/x.ts", "export const x = 1;\n");
    write(c, "suites/sl/admin/performance/test-cases/p.md", "# p\n");
    write(c, "suites/sl/admin/performance/sub/specs/.gitkeep", "");
    write(c, "suites/sl/admin/other/specs/o.spec.ts", "import { y } from './y';\n");
    write(c, "src/helpers.ts", "export const h = 1;\n");
    Fake { clone, maps: tempfile::tempdir().unwrap() }
}

fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for e in fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, root, out);
            } else {
                out.insert(p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"), fs::read(&p).unwrap());
            }
        }
    }
    walk(root, root, &mut out);
    out
}

fn files_of(c: &RenameCheck) -> Vec<(String, String)> {
    c.breaks.iter().map(|b| (b.file.clone(), b.what.clone())).collect()
}

fn run_check(f: &Fake, from: &str, to: &str) -> RenameCheck {
    check(f.clone.path(), Some(f.maps.path()), from, to).unwrap()
}

fn map_file(f: &Fake) -> PathBuf {
    mapping::path(f.maps.path(), "org", "proj")
}

fn placed(path: &str) -> Placement {
    Placement { path: path.into() }
}

#[test]
fn a_clean_rename_lists_nothing_and_moves_the_folder() {
    let f = fake();
    let c = run_check(&f, "sl/admin/performance", "sl/admin/perf");
    assert!(c.breaks.is_empty(), "{:?}", c.breaks);
    assert!(c.tcm_updates.is_empty());
    let before = tree(f.clone.path());
    let done = rename(f.clone.path(), Some(f.maps.path()), "sl/admin/performance", "sl/admin/perf", false).unwrap();
    assert_eq!((done.from.as_str(), done.to.as_str(), done.placements_updated), ("sl/admin/performance", "sl/admin/perf", 0));
    let after = tree(f.clone.path());
    assert_eq!(before.len(), after.len(), "nothing is deleted or added");
    for (k, v) in &before {
        let moved = k.replace("suites/sl/admin/performance/", "suites/sl/admin/perf/");
        assert_eq!(after.get(&moved), Some(v), "{k}");
    }
    assert!(!f.clone.path().join("suites/sl/admin/performance").exists());
}

#[test]
fn every_refusal_leaves_the_clone_as_it_was() {
    let f = fake();
    let before = tree(f.clone.path());
    for (from, to, says) in [
        ("sl/admin/performance", "sl/admin/other", "is already there"),
        ("sl/admin/performance", "sl/admin/OTHER", "differs from it only in capitals"),
        ("", "sl/x", "suites folder itself"),
        ("/", "sl/x", "suites folder itself"),
        ("sl/admin/performance", "", "new name is empty"),
        ("sl/admin/performance", "sl/admin/performance", "same as the old one"),
        ("sl/admin/nothing", "sl/admin/x", "is not a folder in the clone"),
        ("sl/admin/performance", "sl/admin/_x", "starts with _ or ."),
        ("sl/admin/performance", "sl/admin/specs", "is one of the folders inside a feature"),
        ("sl/admin/performance/specs", "sl/admin/performance/x", "is one of the folders inside a feature"),
        ("sl/admin/performance", "sl/admin/performance/inner", "into itself"),
        ("sl/admin/performance", "sl/missing/performance", "no folder"),
        ("sl/admin/performance", "perf", "top level"),
    ] {
        let e = rename(f.clone.path(), Some(f.maps.path()), from, to, true).unwrap_err();
        assert!(e.contains(says), "{from} -> {to}: {e}");
        assert!(check(f.clone.path(), Some(f.maps.path()), from, to).is_err(), "{from} -> {to}");
    }
    assert_eq!(tree(f.clone.path()), before);
}

#[test]
fn a_path_that_leaves_the_suites_folder_is_refused() {
    let f = fake();
    let before = tree(f.clone.path());
    for (from, to) in [
        ("sl/admin/performance", "sl/../../outside"),
        ("sl/admin/performance", "../outside"),
        ("sl/../admin", "sl/admin/x"),
        ("sl/admin/performance", "/abs/place"),
        ("sl/admin/performance", "C:/abs/place"),
        ("sl/admin/performance", "sl\\admin\\x"),
        ("../src", "sl/x"),
    ] {
        let e = rename(f.clone.path(), Some(f.maps.path()), from, to, true).unwrap_err();
        assert!(e.contains("not a folder under suites"), "{from} -> {to}: {e}");
    }
    assert_eq!(tree(f.clone.path()), before);
    assert!(f.clone.path().join("src/helpers.ts").exists());
}

#[test]
fn a_clone_with_no_suites_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let e = rename(d.path(), None, "sl/a", "sl/b", true).unwrap_err();
    assert!(e.contains("no suites folder"), "{e}");
}

#[test]
fn a_file_outside_that_imports_into_the_folder_is_found() {
    let f = fake();
    write(
        f.clone.path(),
        "suites/sl/admin/other/specs/q.spec.ts",
        "import { x } from '../../performance/pages/x';\nconst y = require(\"../../performance/pages/x\");\n",
    );
    // A relative path that stays outside is not a break.
    write(f.clone.path(), "suites/sl/admin/other/specs/ok.spec.ts", "import { h } from '../../../../../src/helpers';\n");
    let c = run_check(&f, "sl/admin/performance", "sl/admin/perf");
    assert_eq!(
        files_of(&c),
        vec![(
            "suites/sl/admin/other/specs/q.spec.ts".to_string(),
            "Imports something inside the folder by a relative path.".to_string()
        )]
    );
}

#[test]
fn a_file_inside_that_imports_outside_is_found_when_the_path_would_stop_leading_there() {
    let f = fake();
    write(
        f.clone.path(),
        "suites/sl/admin/performance/specs/q.spec.ts",
        "import { h } from \"../../../../../src/helpers\";\nimport { x } from '../pages/x';\n",
    );
    write(f.clone.path(), "suites/sl/self/specs/.gitkeep", "");
    // Same depth: the same relative path still leads there.
    let same = run_check(&f, "sl/admin/performance", "sl/self/performance");
    assert!(same.breaks.is_empty(), "{:?}", same.breaks);
    // One level up: it no longer does.
    let up = run_check(&f, "sl/admin/performance", "sl/perf");
    assert_eq!(
        files_of(&up),
        vec![(
            "suites/sl/admin/performance/specs/q.spec.ts".to_string(),
            "Imports something outside the folder by a relative path that would stop leading there.".to_string()
        )]
    );
}

#[test]
fn json_files_are_read_for_relative_paths_too() {
    let f = fake();
    write(
        f.clone.path(),
        "suites/tsconfig.json",
        "{ \"extends\": \"./sl/admin/performance/tsconfig.base.json\", \"name\": \"./sl/admin/other\" }",
    );
    let c = run_check(&f, "sl/admin/performance", "sl/admin/perf");
    assert_eq!(files_of(&c).iter().map(|(f, _)| f.as_str()).collect::<Vec<_>>(), vec!["suites/tsconfig.json"]);
}

#[test]
fn the_config_and_package_json_are_found_by_the_folders_path() {
    let f = fake();
    write(
        f.clone.path(),
        "playwright.config.ts",
        "export default { testDir: './suites/sl/admin/performance', ignore: 'suites/sl/admin/performance-old' };\n",
    );
    write(
        f.clone.path(),
        "package.json",
        "{ \"scripts\": { \"qa\": \"playwright test suites\\\\sl\\\\admin\\\\performance\\\\specs\" } }",
    );
    // Any other file saying the same is not one the check reads for this.
    write(f.clone.path(), "src/notes.ts", "export const n = 'suites/sl/admin/performance';\n");
    let c = run_check(&f, "sl/admin/performance", "sl/admin/perf");
    let mut got = files_of(&c);
    got.sort();
    assert_eq!(
        got,
        vec![
            ("package.json".to_string(), "Mentions the folder's path.".to_string()),
            ("playwright.config.ts".to_string(), "Mentions the folder's path.".to_string()),
        ]
    );
    // A longer name beginning with the folder's is not the folder.
    write(f.clone.path(), "playwright.config.ts", "export default { testDir: './suites/sl/admin/performance-old' };\n");
    write(f.clone.path(), "package.json", "{}");
    assert!(run_check(&f, "sl/admin/performance", "sl/admin/perf").breaks.is_empty());
}

#[test]
fn dependencies_git_and_build_output_are_not_read() {
    let f = fake();
    for dir in ["node_modules/x", ".git", "dist", "build", "playwright-report"] {
        write(f.clone.path(), &format!("{dir}/z.ts"), "import a from '../suites/sl/admin/performance/pages/x';\n");
    }
    write(f.clone.path(), "package-lock.json", "{ \"a\": \"./suites/sl/admin/performance\" }");
    assert!(run_check(&f, "sl/admin/performance", "sl/admin/perf").breaks.is_empty());
}

fn save_map(f: &Fake) {
    let mut map = ExportMap::default();
    map.areas.insert("Performance".into(), placed("sl/admin/performance"));
    map.areas.insert("Other".into(), placed("sl/admin/other"));
    map.areas.insert("Sub".into(), placed("sl/admin/performance/sub"));
    map.cases.insert(7, placed("sl/admin/performance"));
    mapping::save(f.maps.path(), "org", "proj", &map).unwrap();
}

#[test]
fn the_apps_own_export_placements_are_found() {
    let f = fake();
    save_map(&f);
    let c = run_check(&f, "sl/admin/performance", "sl/admin/perf");
    assert!(c.breaks.is_empty());
    assert_eq!(
        c.tcm_updates,
        vec![
            "Export to Playwright places case 7 in this folder.".to_string(),
            "Export to Playwright places the area \"Performance\" in this folder.".to_string(),
            "Export to Playwright places the area \"Sub\" in this folder.".to_string(),
        ]
    );
}

#[test]
fn a_rename_with_breakage_is_refused_until_confirmed_and_moves_nothing() {
    let f = fake();
    save_map(&f);
    write(f.clone.path(), "suites/sl/admin/other/specs/q.spec.ts", "import { x } from '../../performance/pages/x';\n");
    let before = tree(f.clone.path());
    let maps_before = fs::read(map_file(&f)).unwrap();
    let e = rename(f.clone.path(), Some(f.maps.path()), "sl/admin/performance", "sl/admin/perf", false).unwrap_err();
    assert!(
        e.starts_with("Renaming sl/admin/performance to sl/admin/perf will break these, which you will need to fix in the repository:"),
        "{e}"
    );
    assert!(e.contains("suites/sl/admin/other/specs/q.spec.ts"), "{e}");
    assert!(!e.contains(f.clone.path().to_string_lossy().as_ref()), "the list names no full path: {e}");
    assert_eq!(tree(f.clone.path()), before);
    assert_eq!(fs::read(map_file(&f)).unwrap(), maps_before);
}

#[test]
fn a_confirmed_rename_moves_the_folder_and_the_placements_and_rewrites_no_source() {
    let f = fake();
    save_map(&f);
    write(f.clone.path(), "suites/sl/admin/other/specs/q.spec.ts", "import { x } from '../../performance/pages/x';\n");
    let before = tree(f.clone.path());
    let done = rename(f.clone.path(), Some(f.maps.path()), "sl/admin/performance", "sl/admin/perf", true).unwrap();
    assert_eq!(done.placements_updated, 3);
    assert!(f.clone.path().join("suites/sl/admin/perf/pages/x.ts").is_file());
    assert!(!f.clone.path().join("suites/sl/admin/performance").exists());
    // The importing file is exactly as it was: the app only says what breaks.
    assert_eq!(
        fs::read_to_string(f.clone.path().join("suites/sl/admin/other/specs/q.spec.ts")).unwrap(),
        "import { x } from '../../performance/pages/x';\n"
    );
    assert_eq!(before.len(), tree(f.clone.path()).len());
    let map = mapping::load(f.maps.path(), "org", "proj").unwrap();
    assert_eq!(map.areas["Performance"].path, "sl/admin/perf");
    assert_eq!(map.areas["Sub"].path, "sl/admin/perf/sub");
    assert_eq!(map.areas["Other"].path, "sl/admin/other");
    assert_eq!(map.cases[&7].path, "sl/admin/perf");
}

#[test]
fn a_rename_that_only_changes_capitals_is_allowed() {
    let f = fake();
    rename(f.clone.path(), Some(f.maps.path()), "sl/admin/other", "sl/admin/Other", false).unwrap();
    let names: Vec<String> = fs::read_dir(f.clone.path().join("suites/sl/admin"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert!(names.contains(&"Other".to_string()), "{names:?}");
    assert!(!names.contains(&"other".to_string()), "{names:?}");
}

#[test]
fn a_folder_with_no_feature_in_it_renames_too() {
    // `sl` holds no spec of its own, only folders: it is a row in the tree.
    let f = fake();
    let c = run_check(&f, "sl", "sa");
    assert!(c.breaks.is_empty(), "{:?}", c.breaks);
    rename(f.clone.path(), Some(f.maps.path()), "sl", "sa", false).unwrap();
    assert!(f.clone.path().join("suites/sa/admin/performance/pages/x.ts").is_file());
}
