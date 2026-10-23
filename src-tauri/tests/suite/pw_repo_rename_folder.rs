//! Rename folder (`pw_repo::rename_folder`): what a rename would break is
//! found by reading the clone's files, a rename with breakage needs
//! confirming, the app's own export placements move with the folder, and
//! nothing in the repo is rewritten or deleted.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use v2_lib::pw_export::mapping::{self, ExportMap, Placement};
use v2_lib::pw_repo::rename_folder::{check, rename, RenameCheck, CAPITALS_ONLY, SHARED_REFUSED};

/// A fake clone with a `performance` feature, a neighbour and the repo's own
/// files, and a folder for the app's export placements.
struct Fake {
    clone: tempfile::TempDir,
    maps: tempfile::TempDir,
    /// The Sync records: a project with one for this clone belongs to it.
    sync: tempfile::TempDir,
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
    Fake { clone, maps: tempfile::tempdir().unwrap(), sync: tempfile::tempdir().unwrap() }
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
    check(f.clone.path(), Some(f.maps.path()), Some(f.sync.path()), from, to).unwrap()
}

fn map_file(f: &Fake) -> PathBuf {
    mapping::path(f.maps.path(), "org", "proj")
}

/// The project `org`/`proj` has exported to this clone.
fn belongs(f: &Fake, org: &str, proj: &str) {
    let rec = v2_lib::pw_repo::sync::record_path(f.sync.path(), &f.clone.path().to_string_lossy(), org, proj);
    fs::write(rec, "{}").unwrap();
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
    let done = rename(f.clone.path(), Some(f.maps.path()), Some(f.sync.path()), "sl/admin/performance", "sl/admin/perf", false).unwrap();
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
        let e = rename(f.clone.path(), Some(f.maps.path()), Some(f.sync.path()), from, to, true).unwrap_err();
        assert!(e.contains(says), "{from} -> {to}: {e}");
        assert!(check(f.clone.path(), Some(f.maps.path()), Some(f.sync.path()), from, to).is_err(), "{from} -> {to}");
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
        let e = rename(f.clone.path(), Some(f.maps.path()), Some(f.sync.path()), from, to, true).unwrap_err();
        assert!(e.contains("not a folder under suites"), "{from} -> {to}: {e}");
    }
    assert_eq!(tree(f.clone.path()), before);
    assert!(f.clone.path().join("src/helpers.ts").exists());
}

#[test]
fn a_clone_with_no_suites_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let e = rename(d.path(), None, None, "sl/a", "sl/b", true).unwrap_err();
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
    belongs(f, "org", "proj");
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
    let e = rename(f.clone.path(), Some(f.maps.path()), Some(f.sync.path()), "sl/admin/performance", "sl/admin/perf", false).unwrap_err();
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
    let done = rename(f.clone.path(), Some(f.maps.path()), Some(f.sync.path()), "sl/admin/performance", "sl/admin/perf", true).unwrap();
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
fn a_rename_that_only_changes_capitals_is_allowed_once_confirmed() {
    let f = fake();
    let e = rename(f.clone.path(), Some(f.maps.path()), Some(f.sync.path()), "sl/admin/other", "sl/admin/Other", false)
        .unwrap_err();
    assert!(e.contains(CAPITALS_ONLY), "{e}");
    rename(f.clone.path(), Some(f.maps.path()), Some(f.sync.path()), "sl/admin/other", "sl/admin/Other", true).unwrap();
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
    rename(f.clone.path(), Some(f.maps.path()), Some(f.sync.path()), "sl", "sa", false).unwrap();
    assert!(f.clone.path().join("suites/sa/admin/performance/pages/x.ts").is_file());
}

// ---- the review wave ---------------------------------------------------------

const ALIAS_TSCONFIG: &str = r#"{
  // the repo's own aliases
  "extends": "./tsconfig.base.json",
  "compilerOptions": { "strict": true, },
}"#;
const ALIAS_BASE: &str = r#"{
  /* paths are relative to this file */
  "compilerOptions": {
    "paths": {
      "@common/*": ["suites/common-components/*"],
      "@fixtures/*": ["src/fixtures/*"],
    },
  },
}"#;

fn with_common(f: &Fake) {
    let c = f.clone.path();
    write(c, "suites/common-components/employee-search/pages/employee-search.page.ts", "export class P {}\n");
    write(c, "suites/common-components/employee-search/specs/.gitkeep", "");
    write(c, "suites/common-components/other-thing/pages/o.page.ts", "export class O {}\n");
    write(c, "src/fixtures/test.ts", "export const test = 1;\n");
    write(c, "tsconfig.json", ALIAS_TSCONFIG);
    write(c, "tsconfig.base.json", ALIAS_BASE);
}

fn rename_ok(f: &Fake, from: &str, to: &str, confirmed: bool) -> Result<v2_lib::pw_repo::rename_folder::RenameDone, String> {
    rename(f.clone.path(), Some(f.maps.path()), Some(f.sync.path()), from, to, confirmed)
}

#[test]
fn an_import_through_a_tsconfig_alias_into_the_folder_is_found() {
    let f = fake();
    with_common(&f);
    write(
        f.clone.path(),
        "suites/sl/admin/performance/specs/q.spec.ts",
        "import { P } from '@common/employee-search/pages/employee-search.page';\nimport { test } from '@fixtures/test';\nimport { O } from '@common/other-thing/pages/o.page';\n",
    );
    let c = run_check(&f, "common-components/employee-search", "common-components/employee-lookup");
    assert_eq!(
        files_of(&c),
        vec![(
            "suites/sl/admin/performance/specs/q.spec.ts".to_string(),
            "Imports something inside the folder through a path alias.".to_string()
        )]
    );
    // The other component is reached by the other import only.
    let c = run_check(&f, "common-components/other-thing", "common-components/other-thang");
    assert_eq!(c.breaks.len(), 1, "{:?}", c.breaks);
    // A folder no alias reaches has nothing to list.
    assert!(run_check(&f, "sl/admin/other", "sl/admin/another").breaks.is_empty());
}

#[test]
fn a_paths_entry_that_points_into_the_folder_is_found() {
    let f = fake();
    with_common(&f);
    write(
        f.clone.path(),
        "tsconfig.json",
        r#"{ "compilerOptions": { "baseUrl": ".", "paths": { "@emp/*": ["suites/common-components/employee-search/*"] } } }"#,
    );
    let c = run_check(&f, "common-components/employee-search", "common-components/employee-lookup");
    assert_eq!(
        files_of(&c),
        vec![("tsconfig.json".to_string(), "Has a path alias that points into the folder.".to_string())]
    );
}

#[test]
fn a_bare_import_from_the_base_url_is_found() {
    let f = fake();
    write(f.clone.path(), "tsconfig.json", r#"{ "compilerOptions": { "baseUrl": "." } }"#);
    write(f.clone.path(), "src/helpers2.ts", "import { x } from 'suites/sl/admin/performance/pages/x';\n");
    let c = run_check(&f, "sl/admin/performance", "sl/admin/perf");
    assert!(files_of(&c).iter().any(|(file, _)| file == "src/helpers2.ts"), "{:?}", c.breaks);
}

#[test]
fn navigation_entries_the_specs_find_by_folder_names_are_listed() {
    let f = fake();
    write(
        f.clone.path(),
        "src/navigation.json",
        r#"{ "entries": { "admin/performance": { "side": "admin" }, "admin/other": {} } }"#,
    );
    // The feature's own folder renamed: its key changes.
    let c = run_check(&f, "sl/admin/performance", "sl/admin/perf");
    assert_eq!(
        files_of(&c),
        vec![(
            "src/navigation.json".to_string(),
            "Has the navigation entry \"admin/performance\", which the specs find by their folder names.".to_string()
        )]
    );
    // A parent renamed: the key of each feature under it changes too.
    let c = run_check(&f, "sl/admin", "sl/hr");
    assert_eq!(c.breaks.len(), 2, "{:?}", c.breaks);
    // Nothing named in the file: nothing listed.
    write(f.clone.path(), "src/navigation.json", r#"{ "entries": { "x/y": {} } }"#);
    assert!(run_check(&f, "sl/admin/performance", "sl/admin/perf").breaks.is_empty());
    // A value that names the path.
    write(
        f.clone.path(),
        "src/navigation.json",
        r#"{ "entries": { "x/y": { "from": "suites/sl/admin/performance/specs" } } }"#,
    );
    let c = run_check(&f, "sl/admin/performance", "sl/admin/perf");
    assert_eq!(files_of(&c), vec![("src/navigation.json".to_string(), "Mentions the folder's path.".to_string())]);
    // And the file is never rewritten.
    let before = fs::read(f.clone.path().join("src/navigation.json")).unwrap();
    rename_ok(&f, "sl/admin/performance", "sl/admin/perf", true).unwrap();
    assert_eq!(fs::read(f.clone.path().join("src/navigation.json")).unwrap(), before);
}

#[test]
fn the_shared_tree_itself_cannot_be_renamed_or_made() {
    let f = fake();
    with_common(&f);
    let before = tree(f.clone.path());
    for (from, to) in [
        ("common-components", "shared"),
        ("Common-Components", "shared"),
        ("sl", "common-components"),
        ("sl", "Common-components"),
    ] {
        assert_eq!(rename_ok(&f, from, to, true).unwrap_err(), SHARED_REFUSED, "{from} -> {to}");
        assert!(check(f.clone.path(), Some(f.maps.path()), Some(f.sync.path()), from, to).is_err());
    }
    assert_eq!(tree(f.clone.path()), before);
}

#[test]
fn a_common_component_in_the_export_settings_is_reported_and_follows_the_rename() {
    let f = fake();
    with_common(&f);
    let mut map = ExportMap::default();
    map.components.insert("Employee Search".into(), "employee-search".into());
    map.components.insert("Other".into(), "other-thing".into());
    mapping::save(f.maps.path(), "org", "proj", &map).unwrap();
    belongs(&f, "org", "proj");
    let c = run_check(&f, "common-components/employee-search", "common-components/employee-lookup");
    assert!(c.breaks.is_empty(), "{:?}", c.breaks);
    assert_eq!(
        c.tcm_updates,
        vec!["Export to Playwright maps the common component \"Employee Search\" to this folder.".to_string()]
    );
    let done = rename_ok(&f, "common-components/employee-search", "common-components/employee-lookup", false).unwrap();
    assert_eq!((done.placements_updated, done.warning), (1, None));
    let map = mapping::load(f.maps.path(), "org", "proj").unwrap();
    assert_eq!(map.components["Employee Search"], "employee-lookup");
    assert_eq!(map.components["Other"], "other-thing");
}

#[test]
fn a_component_moved_out_of_the_shared_tree_is_listed() {
    let f = fake();
    with_common(&f);
    let mut map = ExportMap::default();
    map.components.insert("Employee Search".into(), "employee-search".into());
    mapping::save(f.maps.path(), "org", "proj", &map).unwrap();
    belongs(&f, "org", "proj");
    let c = run_check(&f, "common-components/employee-search", "sl/employee-search");
    assert_eq!(c.breaks.len(), 1, "{:?}", c.breaks);
    assert!(c.breaks[0].file.ends_with(".pwexport.json"));
}

#[test]
fn an_export_settings_file_that_cannot_be_read_is_listed_and_named_after_the_move() {
    let f = fake();
    save_map(&f);
    fs::write(map_file(&f), "{ not json").unwrap();
    let name = map_file(&f).file_name().unwrap().to_string_lossy().to_string();
    let c = run_check(&f, "sl/admin/performance", "sl/admin/perf");
    assert_eq!(
        files_of(&c),
        vec![(name.clone(), format!("Could not update TCM's export settings in {name}; fix them by hand."))]
    );
    // Unconfirmed: nothing moves.
    assert!(rename_ok(&f, "sl/admin/performance", "sl/admin/perf", false).is_err());
    assert!(f.clone.path().join("suites/sl/admin/performance").exists());
    // Confirmed: the folder moves and the answer says what was not updated.
    let done = rename_ok(&f, "sl/admin/performance", "sl/admin/perf", true).unwrap();
    assert!(f.clone.path().join("suites/sl/admin/perf").exists());
    let w = done.warning.unwrap();
    assert!(w.contains("The folder was renamed") && w.contains(&name), "{w}");
    assert!(!w.contains(f.maps.path().to_string_lossy().as_ref()), "{w}");
}

#[test]
fn a_rename_of_capitals_alone_always_asks_and_lists_imports_spelled_the_old_way() {
    let f = fake();
    write(
        f.clone.path(),
        "suites/sl/admin/performance/specs/old.spec.ts",
        "import { o } from '../../other/pages/o';\nimport { z } from './z';\n",
    );
    write(f.clone.path(), "suites/sl/admin/performance/specs/same.spec.ts", "import { z } from './z';\n");
    let c = run_check(&f, "sl/admin/other", "sl/admin/Other");
    assert_eq!(
        files_of(&c),
        vec![
            ("suites/sl/admin/other".to_string(), CAPITALS_ONLY.to_string()),
            (
                "suites/sl/admin/performance/specs/old.spec.ts".to_string(),
                "Imports something inside the folder spelled with its old capitals.".to_string()
            ),
        ]
    );
    // The note alone for a folder nobody imports.
    let c = run_check(&f, "sl/admin/performance", "sl/admin/Performance");
    assert_eq!(files_of(&c), vec![("suites/sl/admin/performance".to_string(), CAPITALS_ONLY.to_string())]);
    let e = rename_ok(&f, "sl/admin/performance", "sl/admin/Performance", false).unwrap_err();
    assert!(e.contains(CAPITALS_ONLY), "{e}");
}

#[test]
fn a_string_holding_the_folders_path_anywhere_is_listed() {
    let f = fake();
    write(f.clone.path(), "src/a.ts", "const a = require.resolve('./suites/sl/admin/performance/pages/x');\n");
    write(
        f.clone.path(),
        "src/b.ts",
        "const b = path.join(__dirname, \"..\\\\suites\\\\sl\\\\admin\\\\performance\\\\data\");\n",
    );
    write(f.clone.path(), "src/c.ts", "const c = { storageState: `suites/sl/admin/performance/.auth/s.json` };\n");
    // The path inside a longer name, or in a comment: not the folder.
    write(f.clone.path(), "src/d.ts", "const d = 'suites/sl/admin/performance-old/x';\n// suites/sl/admin/performance\n");
    let c = run_check(&f, "sl/admin/performance", "sl/admin/perf");
    let mut got = files_of(&c);
    got.sort();
    assert_eq!(
        got,
        ["src/a.ts", "src/b.ts", "src/c.ts"]
            .iter()
            .map(|n| (n.to_string(), "Mentions the folder's path.".to_string()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn an_escaped_slash_path_in_a_config_is_found() {
    let f = fake();
    write(
        f.clone.path(),
        "playwright.config.ts",
        "export default { testMatch: /suites\\/sl\\/admin\\/performance\\//, };\n",
    );
    let c = run_check(&f, "sl/admin/performance", "sl/admin/perf");
    assert_eq!(files_of(&c), vec![("playwright.config.ts".to_string(), "Mentions the folder's path.".to_string())]);
}

#[test]
fn secrets_sessions_and_report_output_are_never_read() {
    let f = fake();
    let says = "{ \"a\": \"suites/sl/admin/performance/x\", \"b\": \"../../performance/x\" }";
    for p in [
        "src/users/users.json",
        "src/users/other.json",
        ".auth/state.json",
        "suites/.auth/state.json",
        "allure-results/r.json",
        "blob-report/r.json",
        "test-results/r.json",
        "playwright-report/r.json",
        "src/secrets.json",
        "src/api-token.json",
    ] {
        write(f.clone.path(), p, says);
    }
    assert!(run_check(&f, "sl/admin/performance", "sl/admin/perf").breaks.is_empty());
    // A file of the same kind elsewhere is read.
    write(f.clone.path(), "src/ok.json", says);
    assert_eq!(run_check(&f, "sl/admin/performance", "sl/admin/perf").breaks.len(), 1);
}

#[test]
fn a_name_clash_in_non_ascii_capitals_is_refused_before_the_move() {
    let f = fake();
    write(f.clone.path(), "suites/sl/admin/\u{c9}cole/specs/.gitkeep", "");
    let e = rename_ok(&f, "sl/admin/other", "sl/admin/\u{e9}cole", true).unwrap_err();
    assert!(e.contains("differs from it only in capitals"), "{e}");
    assert!(f.clone.path().join("suites/sl/admin/other").exists());
}

#[test]
fn only_export_settings_that_belong_to_this_clone_are_moved() {
    let f = fake();
    save_map(&f);
    // A second project, exported to some other clone: no record for this one.
    let mut other = ExportMap::default();
    other.areas.insert("Elsewhere".into(), placed("sl/admin/performance"));
    mapping::save(f.maps.path(), "org", "other-proj", &other).unwrap();
    let c = run_check(&f, "sl/admin/performance", "sl/admin/perf");
    assert_eq!(c.tcm_updates.len(), 3);
    let before = fs::read(mapping::path(f.maps.path(), "org", "other-proj")).unwrap();
    let done = rename_ok(&f, "sl/admin/performance", "sl/admin/perf", false).unwrap();
    assert_eq!(done.placements_updated, 3);
    assert_eq!(fs::read(mapping::path(f.maps.path(), "org", "other-proj")).unwrap(), before);
    assert_eq!(mapping::load(f.maps.path(), "org", "proj").unwrap().areas["Performance"].path, "sl/admin/perf");
}

#[test]
fn the_rename_holds_the_run_slot_and_frees_it_on_every_path() {
    use v2_lib::pw_repo::process;
    let _slot = crate::serial::pw_repo();
    assert!(process::running_now().is_none());
    {
        let held = process::hold("Renaming a folder").unwrap();
        assert_eq!(process::running_now().map(|(_, l, _)| l), Some("Renaming a folder".to_string()));
        // Nothing else takes it, a second hold included.
        let e = process::hold("Renaming a folder").unwrap_err();
        assert!(e.contains("Renaming a folder is still running"), "{e}");
        drop(held);
    }
    assert!(process::running_now().is_none());
    // Freed on an early return too.
    let early = || -> Result<(), String> {
        let _h = process::hold("Renaming a folder")?;
        Err("refused".into())
    };
    assert!(early().is_err());
    assert!(process::running_now().is_none());
}
