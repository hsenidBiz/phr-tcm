//! Playwright export: the preview (what can be exported and why not) and the
//! all-or-nothing write into the clone.

use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use v2_lib::autorun::accounts::{save_accounts_for, Account};
use v2_lib::autorun::{nav, recipe, store, CaseScript, LocalRun};
use v2_lib::pw_export::export::{preview_with, write_with, ExportResult};
use v2_lib::pw_export::mapping::{save, ExportMap, Placement};
use v2_lib::pw_export::test_case::CaseDoc;

const ORG: &str = "org";
const PROJECT: &str = "proj";
const REAL_PASSWORD: &str = "S3cretPw!";
const CLONE_PASSWORD: &str = "clone-pass-9";
const MD_REL: &str = "suites/sl/admin/performance/proficiency-levels/test-cases/proficiency-levels.md";

struct Fx {
    root: tempfile::TempDir,
    clone: tempfile::TempDir,
    /// Each case's Module, as the Auto Run screen passes it.
    modules: std::cell::RefCell<BTreeMap<i32, String>>,
}

fn write(p: PathBuf, s: &str) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, s).unwrap();
}

fn placement() -> Placement {
    Placement { side: "admin".into(), module: "performance".into(), feature: "proficiency-levels".into() }
}

impl Fx {
    fn new() -> Fx {
        let fx = Fx {
            root: tempfile::tempdir().unwrap(),
            clone: tempfile::tempdir().unwrap(),
            modules: Default::default(),
        };
        let c = fx.clone.path();
        write(c.join("playwright.config.ts"), "export default {};");
        write(c.join("suites/_generated/index.json"), "{\n  \"1\": \"old.spec.ts\"\n}\n");
        write(c.join("suites/_generated/old.spec.ts"), "// old\n");
        write(c.join("src/navigation.json"), "{ \"entries\": { \"performance/proficiency-levels\": {} } }");
        write(
            c.join("src/users/users.json"),
            &format!("{{\"REPO_KEY\": {{\"username\": \"u\", \"password\": \"{CLONE_PASSWORD}\"}}}}"),
        );

        let r = fx.root.path();
        let env_id = v2_lib::environments::active_id(r).unwrap();
        let acct = |key: &str, user: &str| Account {
            key: key.into(),
            label: key.into(),
            username: user.into(),
            password: REAL_PASSWORD.into(),
        };
        save_accounts_for(
            r,
            &env_id,
            &[acct("admin", "hr.admin"), acct("other", "hr.other"), acct("ghost", "hr.ghost")],
        )
        .unwrap();

        let mut rc = recipe::builtin_recipe();
        rc.start_url = "https://app.example/login".into();
        recipe::save_recipe(r, ORG, PROJECT, &rc).unwrap();

        let navjson = json!({ "modules": [
            { "area": "Definition Wizard", "module": "Performance", "clicks": ["a.dw"], "arrived": "/dw", "recorded": "2026-01-01T00:00:00Z" },
            { "area": "Unplaced", "module": "Performance", "clicks": ["a.up"], "arrived": "/up", "recorded": "2026-01-01T00:00:00Z" }
        ]});
        write(nav::nav_path(r, ORG, PROJECT), &navjson.to_string());

        let mut map = ExportMap::default();
        map.areas.insert("Definition Wizard".into(), placement());
        let mut accts = BTreeMap::new();
        accts.insert("admin".to_string(), "REPO_KEY".to_string());
        accts.insert("other".to_string(), "MISSING_KEY".to_string());
        map.accounts.insert(env_id, accts);
        save(r, ORG, PROJECT, &map).unwrap();
        fx
    }

    fn script(&self, id: i32, title: &str, account: &str, area: &str, actions: serde_json::Value) {
        let s: CaseScript = serde_json::from_value(json!({
            "case_id": id, "title": title, "account": account, "area": area,
            "steps": [{ "step_number": 1, "actions": actions }]
        }))
        .unwrap();
        store::save_script(self.root.path(), &s).unwrap();
    }

    fn click_script(&self, id: i32, title: &str, account: &str, area: &str) {
        self.script(id, title, account, area, json!([{ "kind": "click", "selector": "#go" }]));
    }

    fn good(&self, id: i32, title: &str) {
        self.click_script(id, title, "admin", "Definition Wizard");
        self.run(&format!("r{id}"), "5", id, "Passed");
    }

    fn run(&self, run_id: &str, started: &str, case: i32, verdict: &str) {
        let run: LocalRun = serde_json::from_value(json!({
            "id": run_id, "pbi_id": 1, "started_at": started,
            "cases": [{ "case_id": case, "title": "t", "verdict": verdict, "note": "", "steps": [] }]
        }))
        .unwrap();
        store::save_run(self.root.path(), &run).unwrap();
    }

    fn clone_path(&self) -> String {
        self.clone.path().to_string_lossy().to_string()
    }

    fn read_clone(&self, rel: &str) -> String {
        fs::read_to_string(self.clone.path().join(rel)).unwrap()
    }

    fn exists(&self, rel: &str) -> bool {
        self.clone.path().join(rel).exists()
    }
}

fn doc(id: i32, title: &str) -> CaseDoc {
    CaseDoc {
        id,
        title: title.into(),
        state: "Ready".into(),
        area_path: "P\\A".into(),
        iteration_path: "P\\I".into(),
        project: PROJECT.into(),
        module: "Performance".into(),
        tags: "t1".into(),
        preconditions: "pre".into(),
        steps: vec![("Click go".into(), "It goes".into())],
        side: String::new(),
        navigation_captured: false,
        feature_title: String::new(),
    }
}

fn docs(list: &[(i32, &str)]) -> BTreeMap<i32, CaseDoc> {
    list.iter().map(|(i, t)| (*i, doc(*i, t))).collect()
}

fn export(fx: &Fx, ids: &[i32], d: &BTreeMap<i32, CaseDoc>) -> Result<ExportResult, String> {
    write_with(fx.root.path(), ORG, PROJECT, ids, &fx.clone_path(), &fx.modules.borrow(), d)
}

#[test]
fn preview_lists_each_reason() {
    let fx = Fx::new();
    // 10: no script.
    // 11: newest run Failed (an older run passed).
    fx.click_script(11, "Eleven", "admin", "Definition Wizard");
    fx.run("a", "1", 11, "Passed");
    fx.run("b", "2", 11, "Failed");
    // 12: never reviewed.
    fx.click_script(12, "Twelve", "admin", "Definition Wizard");
    fx.run("c", "3", 12, "");
    // 13: area recorded but not placed.
    fx.click_script(13, "Thirteen", "admin", "Unplaced");
    fx.run("d", "3", 13, "Passed");
    // 14: account not mapped.
    fx.click_script(14, "Fourteen", "ghost", "Definition Wizard");
    fx.run("e", "3", 14, "Passed");
    // 15: account mapped to a key the clone lacks.
    fx.click_script(15, "Fifteen", "other", "Definition Wizard");
    fx.run("f", "3", 15, "Passed");
    // 16: cannot translate.
    fx.script(
        16,
        "Sixteen",
        "admin",
        "Definition Wizard",
        json!([{ "kind": "expect_response", "url_contains": "x", "json": { "rows": [{ "a": 1 }] } }]),
    );
    fx.run("g", "3", 16, "Passed");
    // 17: exportable.
    fx.good(17, "Seventeen");
    // 18: area not recorded.
    fx.click_script(18, "Eighteen", "admin", "Nowhere");
    fx.run("h", "3", 18, "Passed");

    let ids = [10, 11, 12, 13, 14, 15, 16, 17, 18];
    let p = preview_with(fx.root.path(), ORG, PROJECT, &ids, &fx.clone_path(), &fx.modules.borrow()).unwrap();
    assert!(p.clone_ok, "{:?}", p.clone_problem);
    assert!(!p.environment.is_empty());
    assert_eq!(p.user_keys, vec!["REPO_KEY"]);
    assert!(p.areas.contains(&"Definition Wizard".to_string()));
    assert!(p.accounts.contains(&"admin".to_string()));
    let by: BTreeMap<i32, _> = p.cases.iter().map(|c| (c.case_id, c)).collect();
    let reason = |id: i32| by[&id].reason.clone().unwrap_or_default();

    assert!(!by[&10].exportable);
    assert!(reason(10).contains("no script"), "{}", reason(10));
    assert_eq!(reason(11), "latest run: Failed");
    assert_eq!(reason(12), "latest run: not reviewed");
    assert!(reason(13).contains("Unplaced"), "{}", reason(13));
    assert!(reason(14).contains("ghost"), "{}", reason(14));
    assert!(by[&14].add_user_command.is_none());
    assert!(reason(15).contains("MISSING_KEY"), "{}", reason(15));
    let cmd = by[&15].add_user_command.clone().unwrap();
    assert_eq!(cmd, "npm run users -- add \"MISSING_KEY\" --username \"hr.other\" --password <password> --apply");
    assert!(!cmd.contains(REAL_PASSWORD));
    assert!(!reason(16).is_empty() && !by[&16].exportable);
    assert!(reason(18).contains("Nowhere"), "{}", reason(18));

    let ok = by[&17];
    assert!(ok.exportable, "{:?}", ok.reason);
    assert_eq!(ok.reason, None);
    assert_eq!(ok.seg.as_deref(), Some("sl/admin/performance/proficiency-levels"));
    assert_eq!(ok.user_key.as_deref(), Some("REPO_KEY"));
    let json = serde_json::to_string(&p).unwrap();
    assert!(!json.contains(REAL_PASSWORD) && !json.contains(CLONE_PASSWORD));
}

#[test]
fn write_creates_raw_spec_index_and_section() {
    let fx = Fx::new();
    fx.good(20, "Pasting newlines is sanitized");
    let r = export(&fx, &[20], &docs(&[(20, "Pasting newlines is sanitized")])).unwrap();
    assert_eq!(r.cases, vec![(20, "pasting-newlines-is-sanitized.spec.ts".to_string())]);
    assert!(r.files.contains(&"suites/_generated/pasting-newlines-is-sanitized.spec.ts".to_string()));
    assert!(r.files.contains(&"suites/_generated/index.json".to_string()));
    assert!(r.files.contains(&MD_REL.to_string()));
    assert!(r.missing_navigation.is_empty());

    let spec = fx.read_clone("suites/_generated/pasting-newlines-is-sanitized.spec.ts");
    assert!(
        spec.starts_with(&format!(
            "// spec: suites/sl/admin/performance/proficiency-levels/test-cases/proficiency-levels.md\n// seed: suites/_generated/seed.spec.ts\n\n"
        )),
        "{spec}"
    );
    assert!(spec.contains("test.describe('Proficiency Levels'"));
    assert!(!spec.contains(CLONE_PASSWORD) && !spec.contains(REAL_PASSWORD));
    assert_eq!(
        fx.read_clone("suites/_generated/index.json"),
        "{\n  \"1\": \"old.spec.ts\",\n  \"20\": \"pasting-newlines-is-sanitized.spec.ts\"\n}\n"
    );

    let md = fx.read_clone(MD_REL);
    assert!(md.starts_with("# Test Case Set: Proficiency Levels"));
    assert!(md.contains("**User:** REPO_KEY"));
    assert!(md.contains("## 20 \u{2014} Pasting newlines is sanitized"));
    assert!(!fx.exists("suites/_generated/index.json.tcm-export-tmp"));
}

#[test]
fn several_cases_land_in_one_feature_file_with_unique_names() {
    let fx = Fx::new();
    fx.good(30, "Same title");
    fx.good(31, "Same title");
    let r = export(&fx, &[30, 31], &docs(&[(30, "Same title"), (31, "Same title")])).unwrap();
    assert_eq!(r.cases, vec![(30, "same-title.spec.ts".to_string()), (31, "same-title-2.spec.ts".to_string())]);
    let md = fx.read_clone(MD_REL);
    assert!(md.contains("## 30 \u{2014} Same title") && md.contains("## 31 \u{2014} Same title"));
    assert_eq!(md.matches("**User:**").count(), 1);
}

#[test]
fn reexport_reuses_the_file_and_replaces_the_section() {
    let fx = Fx::new();
    fx.good(40, "First title");
    export(&fx, &[40], &docs(&[(40, "First title")])).unwrap();
    let mut d = docs(&[(40, "First title")]);
    d.get_mut(&40).unwrap().steps = vec![("Brand new action".into(), "x".into())];
    let r = export(&fx, &[40], &d).unwrap();
    assert_eq!(r.cases, vec![(40, "first-title.spec.ts".to_string())]);
    let md = fx.read_clone(MD_REL);
    assert_eq!(md.matches("## 40 \u{2014}").count(), 1);
    assert!(md.contains("Brand new action") && !md.contains("Click go"));
    assert_eq!(fx.read_clone("suites/_generated/index.json").matches("\"40\"").count(), 1);
    assert!(!fx.exists("suites/_generated/first-title-2.spec.ts"));
}

#[test]
fn other_sections_and_index_entries_are_untouched() {
    let fx = Fx::new();
    let existing =
        "# Test Case Set: Proficiency Levels\n\n**User:** SOMEONE_ELSE\n\n## 7 \u{2014} Hand written\n\n**Assertion floor:** 3\n\nbody\n\n";
    write(fx.clone.path().join(MD_REL), existing);
    fx.good(50, "Fifty");
    export(&fx, &[50], &docs(&[(50, "Fifty")])).unwrap();
    let md = fx.read_clone(MD_REL);
    assert!(md.starts_with(existing), "{md}");
    assert!(md.contains("**User:** SOMEONE_ELSE") && !md.contains("**User:** REPO_KEY"));
    assert!(md.contains("## 50 \u{2014} Fifty"));
    assert!(fx.read_clone("suites/_generated/index.json").starts_with("{\n  \"1\": \"old.spec.ts\",\n"));
    assert_eq!(fx.read_clone("suites/_generated/old.spec.ts"), "// old\n");
}

#[test]
fn missing_navigation_is_reported_not_written() {
    let fx = Fx::new();
    let nav_path = fx.clone.path().join("src/navigation.json");
    write(nav_path.clone(), "{ \"entries\": { \"admin/users\": {} } }");
    let before = fs::read(&nav_path).unwrap();
    fx.good(60, "Sixty");
    fx.good(61, "Sixty one");
    let r = export(&fx, &[60, 61], &docs(&[(60, "Sixty"), (61, "Sixty one")])).unwrap();
    assert_eq!(r.missing_navigation, vec!["performance/proficiency-levels"]);
    assert_eq!(fs::read(&nav_path).unwrap(), before);
}

#[test]
fn users_json_is_never_written() {
    let fx = Fx::new();
    let users = fx.clone.path().join("src/users/users.json");
    let before = fs::read(&users).unwrap();
    fx.good(70, "Seventy");
    export(&fx, &[70], &docs(&[(70, "Seventy")])).unwrap();
    assert_eq!(fs::read(&users).unwrap(), before);
}

#[test]
fn an_unexportable_case_refuses_the_whole_export() {
    let fx = Fx::new();
    fx.good(80, "Eighty");
    let before = fx.read_clone("suites/_generated/index.json");
    let e = export(&fx, &[80, 81], &docs(&[(80, "Eighty"), (81, "x")])).unwrap_err();
    assert!(e.contains("81"), "{e}");
    assert_eq!(fx.read_clone("suites/_generated/index.json"), before);
    assert!(!fx.exists("suites/_generated/eighty.spec.ts"));
    assert!(!fx.exists("suites/sl"));
}

fn stray_temps(p: &Path, out: &mut Vec<String>) {
    for e in fs::read_dir(p).unwrap().flatten() {
        let n = e.file_name().to_string_lossy().to_string();
        if n.ends_with(".tcm-export-tmp") {
            out.push(n);
        }
        if e.path().is_dir() {
            stray_temps(&e.path(), out);
        }
    }
}

#[test]
fn a_failed_write_changes_nothing() {
    let fx = Fx::new();
    fx.good(90, "Ninety");
    // The test-case file's target is a directory: its rename must fail.
    fs::create_dir_all(fx.clone.path().join(MD_REL)).unwrap();
    let before = fx.read_clone("suites/_generated/index.json");
    assert!(export(&fx, &[90], &docs(&[(90, "Ninety")])).is_err());
    assert!(!fx.exists("suites/_generated/ninety.spec.ts"));
    assert_eq!(fx.read_clone("suites/_generated/index.json"), before);
    let mut stray = vec![];
    stray_temps(fx.clone.path(), &mut stray);
    assert!(stray.is_empty(), "{stray:?}");
}

#[test]
fn directories_created_for_a_failed_write_are_removed() {
    let fx = Fx::new();
    fx.good(91, "Ninety one");
    // The raw spec's temp file cannot be created (a directory sits at its
    // path), after the test-case file's directories and temp were made.
    fs::create_dir_all(fx.clone.path().join("suites/_generated/ninety-one.spec.ts.tcm-export-tmp")).unwrap();
    assert!(export(&fx, &[91], &docs(&[(91, "Ninety one")])).is_err());
    assert!(!fx.exists("suites/sl"), "created dirs left behind");
}

fn preview_of(fx: &Fx, id: i32) -> v2_lib::pw_export::export::PreviewCase {
    preview_with(fx.root.path(), ORG, PROJECT, &[id], &fx.clone_path(), &fx.modules.borrow()).unwrap().cases.remove(0)
}

#[test]
fn a_return_to_area_name_resolves_like_auto_run_in_preview_and_write() {
    let fx = Fx::new();
    fx.script(
        100,
        "Hundred",
        "admin",
        "Definition Wizard",
        json!([{ "kind": "return_to_area", "area": "  definition   wizard " }]),
    );
    fx.run("r100", "5", 100, "Passed");
    let pc = preview_of(&fx, 100);
    // Only an exact-after-normalisation match may pass; whichever way
    // find_area rules, the preview and the write must agree.
    let wrote = export(&fx, &[100], &docs(&[(100, "Hundred")]));
    assert_eq!(pc.exportable, wrote.is_ok(), "{:?} vs {:?}", pc.reason, wrote.as_ref().err());
    fx.script(
        101,
        "Hundred one",
        "admin",
        "Definition Wizard",
        json!([{ "kind": "return_to_area", "area": "DEFINITION WIZARD" }]),
    );
    fx.run("r101", "5", 101, "Passed");
    let pc = preview_of(&fx, 101);
    assert!(pc.exportable, "{:?}", pc.reason);
    assert!(export(&fx, &[101], &docs(&[(101, "Hundred one")])).is_ok());
    fx.script(102, "x", "admin", "Definition Wizard", json!([{ "kind": "return_to_area", "area": "Nope" }]));
    fx.run("r102", "5", 102, "Passed");
    assert!(preview_of(&fx, 102).reason.unwrap().contains("Nope"));
}

#[test]
fn an_untranslatable_sign_in_step_shows_in_the_preview() {
    let fx = Fx::new();
    let mut rc = recipe::builtin_recipe();
    rc.start_url = "https://app.example/login".into();
    rc.after_sign_in = serde_json::from_value(json!([
        { "kind": "expect_row_count", "table": "table.grid", "at_least": 1 }
    ]))
    .unwrap();
    recipe::save_recipe(fx.root.path(), ORG, PROJECT, &rc).unwrap();
    fx.good(110, "One ten");
    let pc = preview_of(&fx, 110);
    assert!(!pc.exportable && pc.reason.is_some(), "{:?}", pc);
    assert!(export(&fx, &[110], &docs(&[(110, "One ten")])).is_err());
}

#[test]
fn an_index_entry_that_leaves_the_folder_is_refused() {
    let fx = Fx::new();
    write(
        fx.clone.path().join("suites/_generated/index.json"),
        "{
  \"123\": \"../../evil.spec.ts\"
}
",
    );
    fx.good(123, "One two three");
    let pc = preview_of(&fx, 123);
    assert!(!pc.exportable && pc.reason.unwrap().contains("../../evil.spec.ts"));
    assert!(export(&fx, &[123], &docs(&[(123, "One two three")])).is_err());
    assert!(!fx.clone.path().join("evil.spec.ts").exists());
    assert!(!fx.clone.path().parent().unwrap().join("evil.spec.ts").exists());
    assert!(!fx.exists("suites/sl"));
}

#[test]
fn two_cases_resolving_to_one_file_are_refused_and_fresh_names_avoid_the_index() {
    let fx = Fx::new();
    write(
        fx.clone.path().join("suites/_generated/index.json"),
        "{
  \"130\": \"shared.spec.ts\",
  \"131\": \"shared.spec.ts\",
  \"9\": \"fresh.spec.ts\"
}
",
    );
    fx.good(130, "A");
    fx.good(131, "B");
    let e = export(&fx, &[130, 131], &docs(&[(130, "A"), (131, "B")])).unwrap_err();
    assert!(e.contains("130") && e.contains("131"), "{e}");
    assert!(!fx.exists("suites/sl"));
    // "fresh" is only in the index (no file on disk): a new case must not take it.
    fx.good(132, "Fresh");
    let r = export(&fx, &[132], &docs(&[(132, "Fresh")])).unwrap();
    assert_eq!(r.cases, vec![(132, "fresh-2.spec.ts".to_string())]);
}

#[test]
fn an_unreadable_accounts_file_is_an_error_not_a_clone_problem() {
    let fx = Fx::new();
    let env_id = v2_lib::environments::active_id(fx.root.path()).unwrap();
    write(v2_lib::autorun::accounts::accounts_path_for(fx.root.path(), &env_id), "not json");
    let e = preview_with(fx.root.path(), ORG, PROJECT, &[1], &fx.clone_path(), &BTreeMap::new());
    assert!(e.is_err());
}

fn spec_of(fx: &Fx, r: &ExportResult, id: i32) -> String {
    let file = &r.cases.iter().find(|(i, _)| *i == id).unwrap().1;
    fx.read_clone(&format!("suites/_generated/{file}"))
}

#[test]
fn a_script_with_no_area_goes_where_auto_run_takes_it_by_the_cases_module() {
    let fx = Fx::new();
    // Named like an area: that area, as an unattended run picks it.
    fx.click_script(140, "One forty", "admin", "");
    fx.run("r140", "5", 140, "Passed");
    fx.modules.borrow_mut().insert(140, " definition wizard ".into());
    let pc = preview_of(&fx, 140);
    assert!(pc.exportable, "{:?}", pc.reason);
    let r = export(&fx, &[140], &docs(&[(140, "One forty")])).unwrap();
    assert!(spec_of(&fx, &r, 140).contains("cur.locator('a.dw')"));

    // A Module with several areas and none named like it: Auto Run's own refusal.
    fx.click_script(141, "One forty one", "admin", "");
    fx.run("r141", "5", 141, "Passed");
    fx.modules.borrow_mut().insert(141, "Performance".into());
    let want = v2_lib::autorun::nav::route_for(
        &v2_lib::autorun::nav::load_nav(fx.root.path(), ORG, PROJECT).unwrap(),
        None,
        Some("Performance"),
        Some("admin"),
    )
    .unwrap_err();
    assert_eq!(preview_of(&fx, 141).reason.as_deref(), Some(want.as_str()));
    assert!(export(&fx, &[141], &docs(&[(141, "One forty one")])).unwrap_err().contains(&want));

    // No Module at all.
    fx.click_script(142, "One forty two", "admin", "");
    fx.run("r142", "5", 142, "Passed");
    assert_eq!(preview_of(&fx, 142).reason.as_deref(), Some(v2_lib::autorun::nav::NO_MODULE));
}

#[test]
fn a_project_with_no_areas_exports_with_no_menu_clicks_placed_by_module() {
    let fx = Fx::new();
    write(nav::nav_path(fx.root.path(), ORG, PROJECT), &json!({ "modules": [] }).to_string());
    fx.click_script(150, "One fifty", "admin", "");
    fx.run("r150", "5", 150, "Passed");
    fx.modules.borrow_mut().insert(150, "Performance".into());

    // The Module is what the dialog places, and it is not placed yet.
    let p = preview_with(fx.root.path(), ORG, PROJECT, &[150], &fx.clone_path(), &fx.modules.borrow()).unwrap();
    assert_eq!(p.areas, vec!["Performance".to_string()]);
    assert!(p.cases[0].reason.as_deref().unwrap_or("").contains("\"Performance\" is not placed"), "{:?}", p.cases[0]);

    let mut map = v2_lib::pw_export::mapping::load(fx.root.path(), ORG, PROJECT).unwrap();
    map.areas.insert("Performance".into(), placement());
    save(fx.root.path(), ORG, PROJECT, &map).unwrap();
    let pc = preview_of(&fx, 150);
    assert!(pc.exportable, "{:?}", pc.reason);
    let r = export(&fx, &[150], &docs(&[(150, "One fifty")])).unwrap();
    let spec = spec_of(&fx, &r, 150);
    // Home, the recipe's steps, then straight into step 1: no area clicks.
    assert!(!spec.contains("a.dw") && !spec.contains("a.up"), "{spec}");
    assert!(spec.contains("await cur.locator('#go').first().click();"), "{spec}");

    // A bare return_to_area has nowhere to go - as in Auto Run.
    fx.script(151, "One fifty one", "admin", "", json!([{ "kind": "return_to_area" }]));
    fx.run("r151", "5", 151, "Passed");
    fx.modules.borrow_mut().insert(151, "Performance".into());
    assert_eq!(preview_of(&fx, 151).reason.as_deref(), Some(v2_lib::autorun::runner::NO_AREA_IN_RUN));

    // No Module and no areas: nothing says where it goes.
    fx.click_script(152, "One fifty two", "admin", "");
    fx.run("r152", "5", 152, "Passed");
    let why = preview_of(&fx, 152).reason.unwrap();
    assert!(why.contains("no areas recorded") && why.contains("Module"), "{why}");
}

#[test]
fn an_area_not_placed_yet_comes_with_a_suggestion_from_its_name() {
    let fx = Fx::new();
    fx.good(160, "One sixty");
    let p = preview_with(fx.root.path(), ORG, PROJECT, &[160], &fx.clone_path(), &fx.modules.borrow()).unwrap();
    // "Definition Wizard" is placed already; only "Unplaced" is guessed.
    assert_eq!(p.suggested.len(), 1, "{:?}", p.suggested);
    assert_eq!(
        p.suggested.get("Unplaced"),
        Some(&Placement { side: "admin".into(), module: "performance".into(), feature: "unplaced".into() })
    );
    // A guess is never saved by the preview.
    let saved = v2_lib::pw_export::mapping::load(fx.root.path(), ORG, PROJECT).unwrap();
    assert!(!saved.areas.contains_key("Unplaced"));
}

#[test]
fn the_seed_spec_is_never_a_raw_specs_name() {
    let fx = Fx::new();
    // A title that kebabs to "seed" takes the next free name instead.
    fx.good(170, "Seed");
    let r = export(&fx, &[170], &docs(&[(170, "Seed")])).unwrap();
    assert_eq!(r.cases, vec![(170, "seed-2.spec.ts".to_string())]);
    assert_eq!(r.user_keys, vec![(170, "REPO_KEY".to_string())]);
    assert!(!fx.exists("suites/_generated/seed.spec.ts"));
    // An index entry that points at it, in any case, refuses the case.
    write(fx.clone.path().join("suites/_generated/index.json"), "{\n  \"171\": \"Seed.Spec.ts\"\n}\n");
    fx.good(171, "One seventy one");
    let pc = preview_of(&fx, 171);
    assert!(!pc.exportable && pc.reason.as_deref().unwrap_or("").contains("seed spec"), "{:?}", pc.reason);
    assert!(export(&fx, &[171], &docs(&[(171, "One seventy one")])).is_err());
    assert!(!fx.exists("suites/_generated/Seed.Spec.ts"));
}
