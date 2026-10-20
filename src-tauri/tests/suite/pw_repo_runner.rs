//! The Playwright tab's test runner (`pw_repo::runner`): the tree from a
//! saved `--list --reporter=json` output, the run list `qa` writes, the
//! arguments for every action and mode, `qa-options.json`, the reports, and
//! the dry count that refuses a run whose count disagrees.
//!
//! Playwright never runs here. The tests that go through `runner::run` get
//! a fake starter: cmd.exe typing a saved listing for a list or a dry count,
//! and echoing for the run itself. They hold `serial::held_browsers` then
//! `serial::pw_repo`: there is one run slot for the whole process.

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use v2_lib::pw_repo::process::{Launch, RunEvent, Sink};
use v2_lib::pw_repo::runner::{
    self, build_test_list, build_tree, count_matches, describe_command, explain_mismatch, list_args, marker_above,
    parse_list_stdout, plan, read_options, render_test_list, report_index, reports, run_args,
    test_titles, BrowserMode, LastResult, ListedTest, RunAction, RunOptions, RunStart, Target, NOTHING_FAILED,
    NOTHING_PICKED, NO_SUCH_REPORT, RUN_LIST,
};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn saved(name: &str) -> String {
    fs::read_to_string(fixtures().join("pw_list").join(name)).unwrap()
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap().flatten() {
        let dest = to.join(e.file_name());
        if e.path().is_dir() {
            copy_dir(&e.path(), &dest);
        } else {
            fs::copy(e.path(), dest).unwrap();
        }
    }
}

/// A copy of the fixture clone, with `node_modules`.
fn clone() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    copy_dir(&fixtures().join("pw_repo"), d.path());
    fs::create_dir_all(d.path().join("node_modules/@playwright/test")).unwrap();
    d
}

fn tree_of(clone: &Path) -> runner::TestTree {
    build_tree(clone, &parse_list_stdout(&saved("list.json")).unwrap())
}

fn titles(tree: &runner::TestTree) -> Vec<&str> {
    tree.tests.iter().map(|t| t.title.as_str()).collect()
}

fn find<'a>(tree: &'a runner::TestTree, title: &str) -> &'a ListedTest {
    tree.tests.iter().find(|t| t.title == title).unwrap()
}

const RM: &str = "sl/admin/performance/rating-methods/specs/rating-methods.spec.ts";

// ---- The tree ---------------------------------------------------------------

#[test]
fn the_tree_comes_from_a_saved_listing() {
    let d = clone();
    let tree = tree_of(d.path());
    // Each spec once, under the project that owns its folder: no PH or IN
    // duplicates, no unit file, no raw generated spec, no auth setup.
    assert_eq!(
        titles(&tree),
        [
            "finds an employee by name",
            "User can create a new rating method",
            "User can delete a rating method",
            "the app shell loads",
            "the app shell loads again",
        ]
    );
    assert_eq!(tree.projects, ["SL"]);
    assert_eq!(tree.last_report, None);

    let create = find(&tree, "User can create a new rating method");
    assert_eq!(
        (create.path.clone(), create.spec.as_str()),
        (vec!["sl".to_string(), "admin".into(), "performance".into(), "rating-methods".into()], "rating-methods")
    );
    assert_eq!(create.file, RM);
    assert_eq!(create.title_path, ["Rating Methods", "User can create a new rating method"]);
    assert_eq!(create.line, 5);
    assert_eq!(create.project, "SL");
    assert_eq!(create.id, format!("[\"{RM}\",\"Rating Methods\",\"User can create a new rating method\"]"));

    // The shared tree has no side or module and runs under SL.
    let shared = find(&tree, "finds an employee by name");
    assert_eq!(shared.path, ["common-components", "employee-search"]);
    assert_eq!(shared.project, "SL");
}

#[test]
fn a_banner_ahead_of_the_json_is_passed_over() {
    let raw = format!("[dotenv@17.2.0] injecting env (0) from .env -- tip: {{ quiet: true }}\n{}", saved("list.json"));
    assert_eq!(parse_list_stdout(&raw).unwrap(), parse_list_stdout(&saved("list.json")).unwrap());
    assert!(parse_list_stdout("Error: no config").is_err());
    // The dry count counts owned tests, never Playwright's total (which
    // includes the setup project).
    assert_eq!(count_matches(&saved("dry-two.json")).unwrap(), 2);
    assert_eq!(count_matches(&saved("dry-one.json")).unwrap(), 1);
    assert_eq!(count_matches(&saved("list.json")).unwrap(), 5);
}

/// A saved-listing shape holding one spec at `file`, run under `projects`.
fn listing_of(file: &str, projects: &[&str]) -> serde_json::Value {
    let tests: Vec<_> = projects
        .iter()
        .map(|p| serde_json::json!({"projectId": p, "projectName": p, "status": "skipped", "results": []}))
        .collect();
    serde_json::json!({"suites": [{
        "title": file, "file": file, "line": 0, "column": 0, "specs": [{
            "title": "picks a competency", "file": file, "line": 5, "column": 3, "tests": tests
        }]
    }]})
}

const GROUPED: &str = "sl/admin/performance/definition-wizard/competencies/specs/competencies.spec.ts";

fn path_of(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|p| p.to_string()).collect()
}

#[test]
fn a_grouped_spec_lists_with_its_whole_path() {
    let d = clone();
    let json = listing_of(GROUPED, &["SL", "PH"]);
    let tree = build_tree(d.path(), &json);
    assert_eq!(tree.tests.len(), 1);
    let t = &tree.tests[0];
    assert_eq!(t.path, path_of(&["sl", "admin", "performance", "definition-wizard", "competencies"]));
    assert_eq!((t.spec.as_str(), t.project.as_str()), ("competencies", "SL"));
    assert_eq!(t.ado_id, Some(135300));
    assert_eq!(count_matches(&json.to_string()).unwrap(), 1);
}

#[test]
fn a_backslash_path_is_located() {
    let d = clone();
    let json = listing_of(&GROUPED.replace('/', "\\"), &["SL"]);
    let tree = build_tree(d.path(), &json);
    assert_eq!(tree.tests[0].path, path_of(&["sl", "admin", "performance", "definition-wizard", "competencies"]));
    assert_eq!(count_matches(&json.to_string()).unwrap(), 1);
}

#[test]
fn any_folder_depth_is_kept_and_only_non_specs_are_dropped() {
    let d = clone();
    // No side check any more: a spec under sl/elsewhere is a spec.
    let kept = listing_of("sl/elsewhere/performance/x/specs/x.spec.ts", &["SL"]);
    assert_eq!(build_tree(d.path(), &kept).tests[0].path, path_of(&["sl", "elsewhere", "performance", "x"]));
    // A project other than the owner's does not count.
    assert_eq!(count_matches(&listing_of(GROUPED, &["PH"]).to_string()).unwrap(), 0);
    for not_a_spec in ["_generated/x.spec.ts", "../src/users/registry.test.ts", "auth.setup.ts", "sl/admin/performance/x/x.spec.ts"] {
        assert_eq!(count_matches(&listing_of(not_a_spec, &["SL"]).to_string()).unwrap(), 0, "{not_a_spec}");
    }
}

#[test]
fn tests_sort_by_the_shown_names_level_by_level() {
    let d = clone();
    let mut json = listing_of("sl/admin/performance/rating-methods/specs/r.spec.ts", &["SL"]);
    let more = [GROUPED, "common-components/employee-search/specs/e.spec.ts", "sl/admin/performance/definition-wizard/competencies/specs/a.spec.ts"];
    for f in more {
        let one = listing_of(f, &["SL"]);
        json["suites"].as_array_mut().unwrap().push(one["suites"][0].clone());
    }
    let tree = build_tree(d.path(), &json);
    let order: Vec<String> = tree.tests.iter().map(|t| format!("{}/{}", t.path.join("/"), t.spec)).collect();
    assert_eq!(
        order,
        [
            "common-components/employee-search/e",
            "sl/admin/performance/definition-wizard/competencies/a",
            "sl/admin/performance/definition-wizard/competencies/competencies",
            "sl/admin/performance/rating-methods/r",
        ]
    );
}

#[test]
fn ado_ids_come_from_markers_and_from_the_index() {
    let d = clone();
    let tree = tree_of(d.path());
    assert_eq!(find(&tree, "User can create a new rating method").ado_id, Some(135152));
    assert_eq!(find(&tree, "User can delete a rating method").ado_id, Some(135179));
    assert_eq!(find(&tree, "the app shell loads").ado_id, None);

    // No marker: the raw spec index.json maps, whose test has the same title.
    fs::write(d.path().join("suites/_generated/shell.spec.ts"), "test('the app shell loads', async () => {});\n").unwrap();
    // A title two ids share names neither.
    fs::write(d.path().join("suites/_generated/again-a.spec.ts"), "test(\"the app shell loads again\", async () => {});\n").unwrap();
    fs::write(d.path().join("suites/_generated/again-b.spec.ts"), "test(`the app shell loads again`, async () => {});\n").unwrap();
    fs::write(
        d.path().join("suites/_generated/index.json"),
        r#"{ "135152": "verify-create-rating-method.spec.ts", "135179": "delete-rating-method.spec.ts",
             "135300": "shell.spec.ts", "135301": "again-a.spec.ts", "135302": "again-b.spec.ts" }"#,
    )
    .unwrap();
    let tree = tree_of(d.path());
    assert_eq!(find(&tree, "the app shell loads").ado_id, Some(135300));
    assert_eq!(find(&tree, "the app shell loads again").ado_id, None);
    // A marker wins over the index.
    assert_eq!(find(&tree, "User can create a new rating method").ado_id, Some(135152));
}

#[test]
fn a_marker_is_the_comment_right_above_the_test() {
    let src: Vec<String> = [
        "test.describe('x', () => {",
        "  // ADO 135270",
        "  // the account it runs as",
        "  test('a', async () => {});",
        "",
        "  test('b', async () => {});",
        "  // ADO   99",
        "  test('c', async () => {});",
    ]
    .map(String::from)
    .to_vec();
    assert_eq!(marker_above(&src, 4), Some(135270));
    assert_eq!(marker_above(&src, 6), None, "a blank line ends the comment above");
    assert_eq!(marker_above(&src, 8), Some(99));
    assert_eq!(marker_above(&src, 1), None);
    assert_eq!(marker_above(&src, 0), None);
    assert_eq!(
        test_titles("test('it\\'s here', f); latest('no'); test.describe('no', f); test( \"two\", f)"),
        ["it's here", "two"]
    );
}

#[test]
fn last_results_come_from_the_newest_report() {
    let d = clone();
    let reports_dir = d.path().join("reports");
    for run in ["2026-10-12T16-00-00", "2026-10-13T09-14-00"] {
        fs::create_dir_all(reports_dir.join(run)).unwrap();
        fs::write(reports_dir.join(run).join("index.html"), "<html></html>").unwrap();
    }
    fs::write(reports_dir.join("2026-10-13T09-14-00.json"), saved("report.json")).unwrap();
    // The older run passed everything: it is not the last result.
    fs::write(reports_dir.join("2026-10-12T16-00-00.json"), saved("report.json").replace("\"unexpected\"", "\"expected\"")).unwrap();
    // A run that died before its report has no index.html: not listed.
    fs::create_dir_all(reports_dir.join("2026-10-13T10-00-00")).unwrap();

    let tree = tree_of(d.path());
    assert_eq!(tree.last_report.as_deref(), Some("2026-10-13T09-14-00"));
    assert_eq!(find(&tree, "User can create a new rating method").last_result, Some(LastResult::Passed));
    assert_eq!(find(&tree, "User can delete a rating method").last_result, Some(LastResult::Failed));
    assert_eq!(find(&tree, "the app shell loads").last_result, Some(LastResult::Flaky));
    assert_eq!(find(&tree, "the app shell loads again").last_result, None);

    let listed = reports(d.path());
    assert_eq!(listed.iter().map(|r| r.run_id.as_str()).collect::<Vec<_>>(), ["2026-10-13T09-14-00", "2026-10-12T16-00-00"]);
    let stats = listed[0].stats.as_ref().unwrap();
    assert_eq!((stats.passed, stats.failed, stats.flaky, stats.skipped, stats.duration_ms), (1, 1, 1, 0, 15003));
}

#[test]
fn a_reports_stats_are_read_once_until_it_changes() {
    let d = clone();
    let reports_dir = d.path().join("reports");
    let run = "2026-10-13T09-14-00";
    fs::create_dir_all(reports_dir.join(run)).unwrap();
    fs::write(reports_dir.join(run).join("index.html"), "<html></html>").unwrap();
    let file = reports_dir.join(format!("{run}.json"));
    fs::write(&file, saved("report.json")).unwrap();
    let at = fs::metadata(&file).unwrap().modified().unwrap();
    let passed = |d: &Path| reports(d)[0].stats.as_ref().map(|s| s.passed);
    assert_eq!(passed(d.path()), Some(1));

    // The same modified time and size: the kept stats, not the file read
    // again.
    let changed = saved("report.json").replace("\"expected\": 1", "\"expected\": 7");
    assert_ne!(changed, saved("report.json"), "the fixture has \"expected\": 1");
    fs::write(&file, &changed).unwrap();
    fs::File::options().write(true).open(&file).unwrap().set_modified(at).unwrap();
    assert_eq!(passed(d.path()), Some(1));

    // The same modified time, a new size: read again.
    fs::write(&file, saved("report.json").replace("\"expected\": 1", "\"expected\": 12")).unwrap();
    fs::File::options().write(true).open(&file).unwrap().set_modified(at).unwrap();
    assert_eq!(passed(d.path()), Some(12));

    // A new modified time: read again.
    fs::write(&file, &changed).unwrap();
    fs::File::options().write(true).open(&file).unwrap().set_modified(at + Duration::from_secs(5)).unwrap();
    assert_eq!(passed(d.path()), Some(7));

    // Only the stats are read: a report with no stats has none, and a count
    // that is not a number is 0.
    fs::write(&file, r#"{ "suites": [{ "title": "x" }], "config": { "a": [1, 2, 3] } }"#).unwrap();
    fs::File::options().write(true).open(&file).unwrap().set_modified(at + Duration::from_secs(10)).unwrap();
    assert_eq!(reports(d.path())[0].stats, None);
    fs::write(&file, r#"{ "stats": { "expected": 2.9, "unexpected": "many", "duration": 12.5 }, "errors": [] }"#).unwrap();
    fs::File::options().write(true).open(&file).unwrap().set_modified(at + Duration::from_secs(15)).unwrap();
    let s = reports(d.path())[0].stats.clone().unwrap();
    assert_eq!((s.passed, s.failed, s.flaky, s.skipped, s.duration_ms), (2, 0, 0, 0, 12));
}

#[test]
fn a_report_opens_only_by_a_listed_plain_name() {
    let d = clone();
    fs::create_dir_all(d.path().join("reports/2026-10-13T09-14-00")).unwrap();
    fs::write(d.path().join("reports/2026-10-13T09-14-00/index.html"), "").unwrap();
    assert_eq!(
        report_index(d.path(), "2026-10-13T09-14-00").unwrap(),
        d.path().join("reports").join("2026-10-13T09-14-00").join("index.html")
    );
    for bad in ["", ".", "..", "../reports/2026-10-13T09-14-00", "2026-10-13T09-14-00/../2026-10-13T09-14-00", "C:\\x", "missing"] {
        assert_eq!(report_index(d.path(), bad).unwrap_err(), NO_SUCH_REPORT, "{bad:?}");
    }
    assert!(reports(&d.path().join("nowhere")).is_empty());
}

// ---- The run list ------------------------------------------------------------

fn ids(tree: &runner::TestTree, titles: &[&str]) -> HashSet<String> {
    titles.iter().map(|t| find(tree, t).id.clone()).collect()
}

#[test]
fn the_run_list_is_what_qa_writes() {
    let d = clone();
    let tree = tree_of(d.path());

    // A whole spec is one line; one test of a spec names its title path.
    let l = build_test_list(&tree.tests, &ids(&tree, &["User can create a new rating method", "User can delete a rating method"]));
    assert_eq!(l.lines, [format!("[SL] > {RM}")]);
    assert_eq!(l.expected.len(), 2);
    let l = build_test_list(&tree.tests, &ids(&tree, &["User can delete a rating method", "finds an employee by name"]));
    assert_eq!(
        l.lines,
        [
            "[SL] > common-components/employee-search/specs/employee-search.spec.ts".to_string(),
            format!("[SL] > {RM} > Rating Methods > User can delete a rating method"),
        ]
    );
    assert_eq!(l.projects, ["SL"]);
    assert!(l.warnings.is_empty());
    assert!(build_test_list(&tree.tests, &HashSet::new()).lines.is_empty());

    // A title holding a delimiter cannot be addressed: its whole spec runs,
    // and the count expects all of it.
    let mut tests = tree.tests.clone();
    let delete = tests.iter_mut().find(|t| t.title == "User can delete a rating method").unwrap();
    delete.title = "Save > Close".into();
    delete.title_path = vec!["Rating Methods".into(), "Save > Close".into()];
    let mut ph = tests[3].clone();
    ph.project = "PH".into();
    ph.file = "ph/self/leave/apply/specs/apply.spec.ts".into();
    ph.id = "ph".into();
    tests.push(ph);
    let picked: HashSet<String> = [tests[2].id.clone(), "ph".to_string()].into();
    let l = build_test_list(&tests, &picked);
    assert_eq!(l.lines, [format!("[SL] > {RM}"), "[PH] > ph/self/leave/apply/specs/apply.spec.ts".to_string()]);
    assert_eq!(l.expected.len(), 3);
    assert_eq!(l.projects, ["PH", "SL"]);
    assert_eq!(l.warnings.len(), 1);
    assert!(l.warnings[0].contains("\"Save > Close\" contains a title delimiter"), "{}", l.warnings[0]);

    // The header qa writes, then one line each.
    assert_eq!(
        render_test_list(&l.lines, &l.projects, BrowserMode::Shown),
        format!(
            "# generated by npm run qa - project PH, SL, browser shown\n# 2 line(s). Regenerated on every run - edits here are not preserved.\n[SL] > {RM}\n[PH] > ph/self/leave/apply/specs/apply.spec.ts\n"
        )
    );
}

// ---- The arguments -----------------------------------------------------------

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn the_arguments_for_every_action_and_mode() {
    let none = RunOptions::default();
    let list = Target::TestList(RUN_LIST);
    let sl = strings(&["SL"]);
    let base = ["test", "--project=SL", "--test-list=.qa-cli/run.list"];
    assert_eq!(run_args(&sl, list, false, BrowserMode::Hidden, &none).unwrap(), strings(&base));
    assert_eq!(run_args(&sl, list, false, BrowserMode::Shown, &none).unwrap(), strings(&[&base[..], &["--headed"]].concat()));
    assert_eq!(run_args(&sl, list, false, BrowserMode::Debug, &none).unwrap(), strings(&[&base[..], &["--debug"]].concat()));
    assert_eq!(
        run_args(&strings(&["PH", "SL"]), Target::LastFailed, false, BrowserMode::Hidden, &none).unwrap(),
        strings(&["test", "--project=PH", "--project=SL", "--last-failed"])
    );
    assert!(run_args(&[], list, false, BrowserMode::Hidden, &none).is_err(), "no project would run every test three times");

    let all = RunOptions {
        retries: Some(2),
        max_failures: Some(5),
        timeout: Some(0),
        global_timeout: Some(900000),
        trace: Some("on".into()),
    };
    assert_eq!(
        run_args(&sl, list, false, BrowserMode::Shown, &all).unwrap(),
        strings(&[&base[..], &["--headed", "--retries=2", "--global-timeout=900000", "--trace=on", "--timeout=0", "--max-failures=5"]].concat())
    );
    // Debug already means no timeout and one failure: those are not passed.
    assert_eq!(
        run_args(&sl, list, false, BrowserMode::Debug, &all).unwrap(),
        strings(&[&base[..], &["--debug", "--retries=2", "--global-timeout=900000", "--trace=on"]].concat())
    );
    let one = RunOptions { max_failures: Some(1), ..Default::default() };
    assert_eq!(run_args(&sl, list, false, BrowserMode::Hidden, &one).unwrap(), strings(&[&base[..], &["-x"]].concat()));
    // The dry count: the same selection, listed, with no mode or option.
    for mode in [BrowserMode::Hidden, BrowserMode::Shown, BrowserMode::Debug] {
        assert_eq!(
            run_args(&sl, list, true, mode, &all).unwrap(),
            strings(&[&base[..], &["--list", "--reporter=json", "--pass-with-no-tests"]].concat())
        );
    }
    assert_eq!(list_args(), strings(&["playwright", "test", "--list", "--reporter=json", "--pass-with-no-tests"]));
    assert_eq!(
        describe_command(&strings(&["test", "--grep", "a title"])),
        "npx playwright test --grep \"a title\""
    );
}

#[test]
fn every_action_plans_as_qa_would() {
    let d = clone();
    let tree = tree_of(d.path());
    let none = RunOptions::default();

    let all = plan(d.path(), &tree, &[], RunAction::All, BrowserMode::Hidden, &none).unwrap();
    assert_eq!(all.expected, 5);
    assert_eq!(all.args, strings(&["test", "--project=SL", "--test-list=.qa-cli/run.list"]));
    assert_eq!(
        all.dry_args.unwrap(),
        strings(&["test", "--project=SL", "--test-list=.qa-cli/run.list", "--list", "--reporter=json", "--pass-with-no-tests"])
    );
    assert!(all.run_list.unwrap().ends_with(
        "[SL] > sl/self/smoke/framework-smoke/specs/framework-smoke.spec.ts\n[SL] > sl/self/smoke/framework-smoke/specs/framework-smoke-again.spec.ts\n"
    ));

    let picked = vec![find(&tree, "the app shell loads").id.clone()];
    let one = plan(d.path(), &tree, &picked, RunAction::Picked, BrowserMode::Debug, &none).unwrap();
    assert_eq!(one.expected, 1);
    assert_eq!(one.args.last().unwrap(), "--debug");
    assert_eq!(plan(d.path(), &tree, &[], RunAction::Picked, BrowserMode::Hidden, &none).unwrap_err(), NOTHING_PICKED);
    // A ticked test the listing no longer has refuses the run, naming how
    // many, even beside ones it still has.
    let gone = "[\"gone\"]".to_string();
    let e = plan(d.path(), &tree, &[gone.clone()], RunAction::Picked, BrowserMode::Hidden, &none).unwrap_err();
    assert_eq!(e, runner::missing_ticks(1));
    assert!(e.starts_with("1 ticked test is no longer in the repo's list") && e.contains("Use Refresh above the list"), "{e}");
    let two = vec![gone, "[\"also gone\"]".to_string(), find(&tree, "the app shell loads").id.clone()];
    let e = plan(d.path(), &tree, &two, RunAction::Picked, BrowserMode::Hidden, &none).unwrap_err();
    assert!(e.starts_with("2 ticked tests are no longer"), "{e}");
    // Run all is not about ticks.
    assert!(plan(d.path(), &tree, &two, RunAction::All, BrowserMode::Hidden, &none).is_ok());

    // Re-run failures: --last-failed under every project that has tests.
    assert_eq!(plan(d.path(), &tree, &[], RunAction::Failed, BrowserMode::Hidden, &none).unwrap_err(), NOTHING_FAILED);
    fs::create_dir_all(d.path().join("test-results")).unwrap();
    fs::write(d.path().join("test-results/.last-run.json"), r#"{ "status": "failed", "failedTests": ["a-1", "b-2"] }"#).unwrap();
    let failed = plan(d.path(), &tree, &[], RunAction::Failed, BrowserMode::Shown, &none).unwrap();
    assert_eq!(failed.expected, 2);
    assert_eq!(failed.args, strings(&["test", "--project=SL", "--last-failed", "--headed"]));
    assert_eq!((failed.run_list, failed.dry_args), (None, None));
}

// ---- qa-options.json -------------------------------------------------------------

#[test]
fn qa_options_are_read_as_the_repo_reads_them() {
    let d = clone();
    // Missing: the repo's defaults, which pass no flag at all.
    let read = read_options(d.path());
    assert_eq!((read.options, read.errors.len(), read.exists), (RunOptions::default(), 0, false));

    fs::write(
        d.path().join("qa-options.json"),
        r#"{ "retries": 2, "maxFailures": "off", "timeout": "0", "globalTimeout": 0, "trace": "retain-on-failure", "colour": 1 }"#,
    )
    .unwrap();
    let read = read_options(d.path());
    assert!(read.exists);
    assert_eq!(
        read.options,
        RunOptions { retries: Some(2), max_failures: None, timeout: Some(0), global_timeout: None, trace: Some("retain-on-failure".into()) }
    );
    assert_eq!(read.errors, ["Unknown option \"colour\" in qa-options.json, ignored."]);

    // A bad value falls back to its default and says so; the rest still read.
    fs::write(d.path().join("qa-options.json"), r#"{ "retries": -1, "maxFailures": 1, "trace": "always", "timeout": null }"#).unwrap();
    let read = read_options(d.path());
    assert_eq!(read.options, RunOptions { max_failures: Some(1), ..Default::default() });
    assert_eq!(read.errors.len(), 2, "{:?}", read.errors);

    fs::write(d.path().join("qa-options.json"), "{ not json").unwrap();
    let read = read_options(d.path());
    assert_eq!(read.options, RunOptions::default());
    assert!(read.errors[0].starts_with("qa-options.json is not valid JSON"));
}

// ---- Running, through a fake starter -------------------------------------------

fn collect() -> (Sink, Receiver<RunEvent>) {
    let (tx, rx) = channel();
    let tx = Mutex::new(tx);
    let sink: Sink = Arc::new(move |e| {
        let _ = tx.lock().unwrap().send(e);
    });
    (sink, rx)
}

fn cmd() -> PathBuf {
    std::env::var_os("ComSpec").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows\System32\cmd.exe"))
}

/// A starter that records every argv it is asked for, types `list.json`
/// for the listing and `dry` for a dry count, and echoes for a run.
struct Fake {
    dir: tempfile::TempDir,
    asked: Mutex<Vec<Vec<String>>>,
}

impl Fake {
    fn new(dry: &str) -> Fake {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("list.json"), saved("list.json")).unwrap();
        fs::write(dir.path().join("dry.json"), saved(dry)).unwrap();
        Fake { dir, asked: Mutex::new(vec![]) }
    }

    fn launch(&self, args: &[String]) -> Result<Launch, String> {
        assert_eq!(args[0], "playwright", "only playwright is run");
        self.asked.lock().unwrap().push(args.to_vec());
        let typed = |name: &str| {
            let path = self.dir.path().join(name).into_os_string();
            vec![OsString::from("/D"), OsString::from("/C"), OsString::from("type"), path]
        };
        let a = if args == list_args() {
            typed("list.json")
        } else if args.iter().any(|a| a == "--list") {
            typed("dry.json")
        } else {
            ["/D", "/C", "echo", "ran"].map(OsString::from).to_vec()
        };
        Ok(Launch::test_program(cmd(), a, self.dir.path().to_path_buf(), "npx playwright test"))
    }

    fn runs(&self) -> Vec<Vec<String>> {
        self.asked.lock().unwrap().iter().filter(|a| !a.iter().any(|x| x == "--list")).cloned().collect()
    }
}

/// Every file under `dir`, relative, sorted.
fn files(dir: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for e in fs::read_dir(dir).unwrap().flatten() {
            if e.path().is_dir() {
                walk(root, &e.path(), out);
            } else {
                out.push(e.path().strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut out = vec![];
    walk(dir, dir, &mut out);
    out.sort();
    out
}

fn rating_methods(tree: &runner::TestTree) -> Vec<String> {
    tree.tests.iter().filter(|t| t.file == RM).map(|t| t.id.clone()).collect()
}

#[cfg(windows)]
#[test]
fn a_count_mismatch_refuses_to_run() {
    let _held_browsers = crate::serial::held_browsers();
    let _slot = crate::serial::pw_repo();
    let d = clone();
    // Playwright matches one of the two picked tests.
    let fake = Fake::new("dry-one.json");
    let (show, _rx) = collect();
    let picked = rating_methods(&tree_of(d.path()));

    let e = runner::run(d.path(), &picked, RunAction::Picked, BrowserMode::Hidden, false, &show, &|a| fake.launch(a)).unwrap_err();
    assert_eq!(e, explain_mismatch(2, 1));
    assert!(e.starts_with("2 selected, but Playwright matched 1."), "{e}");
    assert!(fake.runs().is_empty(), "nothing but the listing and the dry count started: {:?}", fake.asked.lock().unwrap());
    // The dry count named the run list it was checking.
    let asked = fake.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 2);
    assert_eq!(
        asked[1],
        strings(&["playwright", "test", "--project=SL", "--test-list=.qa-cli/run.list", "--list", "--reporter=json", "--pass-with-no-tests"])
    );
    assert_eq!(
        fs::read_to_string(d.path().join(RUN_LIST)).unwrap(),
        format!("# generated by npm run qa - project SL, browser hidden\n# 1 line(s). Regenerated on every run - edits here are not preserved.\n[SL] > {RM}\n")
    );
    assert_eq!(v2_lib::pw_repo::process::running(), None);

    // Matching none says so, and refuses too.
    let fake = Fake::new("dry-two.json");
    fs::write(fake.dir.path().join("dry.json"), saved("dry-two.json").replace("\"SL\"", "\"XX\"")).unwrap();
    let e = runner::run(d.path(), &picked, RunAction::Picked, BrowserMode::Hidden, false, &show, &|a| fake.launch(a)).unwrap_err();
    assert_eq!(e, explain_mismatch(2, 0));
    assert!(fake.runs().is_empty());
}

#[cfg(windows)]
#[test]
fn a_matching_count_runs_and_writes_only_the_run_list() {
    let _held_browsers = crate::serial::held_browsers();
    let _slot = crate::serial::pw_repo();
    let d = clone();
    let options = r#"{ "retries": 1, "colour": true }"#;
    fs::write(d.path().join("qa-options.json"), options).unwrap();
    let before = files(d.path());
    let fake = Fake::new("dry-two.json");
    let (show, rx) = collect();
    let picked = rating_methods(&tree_of(d.path()));

    let started = runner::run(d.path(), &picked, RunAction::Picked, BrowserMode::Shown, false, &show, &|a| fake.launch(a)).unwrap();
    let RunStart::Started { run, command, summary, warnings } = started else { panic!("{started:?}") };
    assert_eq!(command, "npx playwright test --project=SL --test-list=.qa-cli/run.list --headed --retries=1");
    assert_eq!(summary, "2 selected · 2 matched");
    assert_eq!(warnings, ["Unknown option \"colour\" in qa-options.json, ignored."]);
    assert_eq!(
        fake.runs(),
        [strings(&["playwright", "test", "--project=SL", "--test-list=.qa-cli/run.list", "--headed", "--retries=1"])]
    );
    // The run's own output reaches the panel, then its end.
    let mut lines = vec![];
    loop {
        match rx.recv_timeout(Duration::from_secs(20)).unwrap() {
            RunEvent::Output { run: r, line } if r == run => lines.push(line),
            RunEvent::Done { run: r, code } if r == run => {
                assert_eq!(code, Some(0));
                break;
            }
            _ => {}
        }
    }
    assert_eq!(lines, ["ran"]);

    // qa-options.json is read, never written; the run list is the one file
    // the runner added.
    assert_eq!(fs::read_to_string(d.path().join("qa-options.json")).unwrap(), options);
    let mut expected = before;
    expected.push(".qa-cli/run.list".to_string());
    expected.sort();
    assert_eq!(files(d.path()), expected);

    // The listing was the session's: a second run lists nothing again.
    let fake2 = Fake::new("dry-two.json");
    let again = runner::run(d.path(), &picked, RunAction::Picked, BrowserMode::Hidden, false, &show, &|a| fake2.launch(a)).unwrap();
    let RunStart::Started { run: run2, .. } = again else { panic!() };
    assert!(fake2.asked.lock().unwrap().iter().all(|a| *a != list_args()));
    while !matches!(rx.recv_timeout(Duration::from_secs(20)).unwrap(), RunEvent::Done { run: r, .. } if r == run2) {}
}

#[cfg(windows)]
#[test]
fn a_debug_run_of_more_than_one_test_needs_confirm() {
    let _held_browsers = crate::serial::held_browsers();
    let _slot = crate::serial::pw_repo();
    let d = clone();
    let fake = Fake::new("dry-two.json");
    let (show, rx) = collect();
    let picked = rating_methods(&tree_of(d.path()));

    let asked = runner::run(d.path(), &picked, RunAction::Picked, BrowserMode::Debug, false, &show, &|a| fake.launch(a)).unwrap();
    assert_eq!(asked, RunStart::ConfirmDebug { count: 2 });
    assert!(fake.runs().is_empty());
    assert!(!d.path().join(RUN_LIST).exists(), "nothing written before the answer");

    // Re-running two failures in debug asks too.
    fs::create_dir_all(d.path().join("test-results")).unwrap();
    fs::write(d.path().join("test-results/.last-run.json"), r#"{ "failedTests": ["a", "b"] }"#).unwrap();
    let asked = runner::run(d.path(), &[], RunAction::Failed, BrowserMode::Debug, false, &show, &|a| fake.launch(a)).unwrap();
    assert_eq!(asked, RunStart::ConfirmDebug { count: 2 });
    assert!(fake.runs().is_empty());

    // Confirmed, it runs.
    let started = runner::run(d.path(), &picked, RunAction::Picked, BrowserMode::Debug, true, &show, &|a| fake.launch(a)).unwrap();
    let RunStart::Started { run, command, .. } = started else { panic!("{started:?}") };
    assert!(command.ends_with("--debug"), "{command}");
    while !matches!(rx.recv_timeout(Duration::from_secs(20)).unwrap(), RunEvent::Done { run: r, .. } if r == run) {}
}

#[test]
fn the_runner_writes_only_through_one_atomic_write() {
    // Source tripwire: the runner writes one file, through atomic_write,
    // and never writes qa-options.json.
    let src = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/pw_repo/runner.rs")).unwrap().replace("\r\n", "\n");
    assert_eq!(src.matches("atomic_write(").count(), 1);
    assert!(!src.contains("fs::write("));
    assert!(!src.contains("File::create"));
}

// ---- Fix round: a stale listing, one run prepared at a time, the clone ------

#[test]
fn the_fingerprint_follows_the_specs_and_the_config() {
    let d = clone();
    let first = runner::fingerprint(d.path());
    assert_eq!(runner::fingerprint(d.path()), first, "nothing changed");
    // Files that are not specs do not count.
    fs::write(d.path().join("suites/sl/admin/performance/rating-methods/data/extra.json"), "{}").unwrap();
    fs::create_dir_all(d.path().join("node_modules/x/suites")).unwrap();
    assert_eq!(runner::fingerprint(d.path()), first);
    // A new spec does.
    let added = d.path().join("suites/sl/self/smoke/framework-smoke/specs/added.spec.ts");
    fs::write(&added, "test('added', async () => {});\n").unwrap();
    let second = runner::fingerprint(d.path());
    assert_ne!(second, first);
    // So does an edited config.
    fs::write(d.path().join("playwright.config.ts"), "export default { changed: true };\n").unwrap();
    assert_ne!(runner::fingerprint(d.path()), second);
}

#[test]
fn only_the_chosen_clone_is_used() {
    assert_eq!(runner::chosen_clone(r" C:\repo\ ", r"C:\repo").unwrap(), PathBuf::from(r"C:\repo"));
    assert_eq!(runner::chosen_clone("C:/repo", r"C:\repo").unwrap(), PathBuf::from(r"C:\repo"));
    for (asked, configured) in [(r"C:\other", r"C:\repo"), (r"C:\repo", ""), ("", ""), (r"C:\repo\..\x", r"C:\repo")] {
        assert_eq!(runner::chosen_clone(asked, configured).unwrap_err(), runner::NOT_THE_CHOSEN_CLONE, "{asked:?} {configured:?}");
    }
}

#[test]
fn a_mismatch_says_to_list_again() {
    assert!(explain_mismatch(3, 2).ends_with("If the specs changed, use Refresh above the list, then run again."));
    assert!(explain_mismatch(3, 0).ends_with("If the specs changed, use Refresh above the list, then run again."));
}

#[cfg(windows)]
#[test]
fn a_spec_added_after_listing_is_listed_again() {
    let _held_browsers = crate::serial::held_browsers();
    let _slot = crate::serial::pw_repo();
    let d = clone();
    let fake = Fake::new("dry-two.json");
    let (show, rx) = collect();
    let picked = rating_methods(&tree_of(d.path()));
    let wait = |run| while !matches!(rx.recv_timeout(Duration::from_secs(20)).unwrap(), RunEvent::Done { run: r, .. } if r == run) {};

    let RunStart::Started { run, .. } =
        runner::run(d.path(), &picked, RunAction::Picked, BrowserMode::Hidden, false, &show, &|a| fake.launch(a)).unwrap()
    else {
        panic!()
    };
    wait(run);

    // A spec is added after the listing: the next run lists again, so Run
    // all includes it.
    let file = "sl/self/smoke/framework-smoke/specs/added.spec.ts";
    fs::write(d.path().join("suites").join(file), "test('added later', async () => {});\n").unwrap();
    let mut listing: serde_json::Value = serde_json::from_str(&saved("list.json")).unwrap();
    let suite = serde_json::json!({
        "title": file.replace('/', "\\"), "file": file, "column": 0, "line": 0,
        "specs": [{ "title": "added later", "ok": true, "tags": [], "id": "a1-1", "file": file, "line": 1, "column": 1,
                    "tests": [{ "projectId": "SL", "projectName": "SL", "results": [], "status": "skipped" }] }]
    });
    listing["suites"].as_array_mut().unwrap().push(suite);
    let listing = serde_json::to_string_pretty(&listing).unwrap();
    let fake = Fake::new("dry-two.json");
    fs::write(fake.dir.path().join("list.json"), &listing).unwrap();
    fs::write(fake.dir.path().join("dry.json"), &listing).unwrap();

    let RunStart::Started { run, summary, .. } =
        runner::run(d.path(), &[], RunAction::All, BrowserMode::Hidden, false, &show, &|a| fake.launch(a)).unwrap()
    else {
        panic!()
    };
    assert_eq!(summary, "6 selected · 6 matched");
    assert!(fake.asked.lock().unwrap().iter().any(|a| *a == list_args()), "it listed again");
    assert!(fs::read_to_string(d.path().join(RUN_LIST)).unwrap().contains(file));
    wait(run);
}

#[cfg(windows)]
#[test]
fn a_second_run_cannot_rewrite_the_run_list_while_one_is_prepared() {
    let _held_browsers = crate::serial::held_browsers();
    let _slot = crate::serial::pw_repo();
    let d = clone();
    let other = clone();
    let fake = Fake::new("dry-two.json");
    let (show, rx) = collect();
    let picked = rating_methods(&tree_of(d.path()));
    let second: Mutex<Option<Result<RunStart, String>>> = Mutex::new(None);

    // While the first run is being prepared (its dry count starting), a
    // second one is asked for.
    let launch = |a: &[String]| {
        if a.iter().any(|x| x.starts_with("--test-list")) && second.lock().unwrap().is_none() {
            let r = runner::run(other.path(), &[], RunAction::All, BrowserMode::Hidden, false, &show, &|b| fake.launch(b));
            *second.lock().unwrap() = Some(r);
        }
        fake.launch(a)
    };
    let RunStart::Started { run, .. } =
        runner::run(d.path(), &picked, RunAction::Picked, BrowserMode::Hidden, false, &show, &launch).unwrap()
    else {
        panic!()
    };
    assert_eq!(second.lock().unwrap().clone().unwrap().unwrap_err(), runner::PREPARING_ALREADY);
    assert!(!other.path().join(RUN_LIST).exists());
    while !matches!(rx.recv_timeout(Duration::from_secs(20)).unwrap(), RunEvent::Done { run: r, .. } if r == run) {}
}

#[cfg(windows)]
#[test]
fn a_run_that_panicked_while_preparing_does_not_lock_out_the_next() {
    let _held_browsers = crate::serial::held_browsers();
    let _slot = crate::serial::pw_repo();
    let d = clone();
    let fake = Fake::new("dry-two.json");
    let (show, rx) = collect();
    let picked = rating_methods(&tree_of(d.path()));
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runner::run(d.path(), &picked, RunAction::Picked, BrowserMode::Hidden, false, &show, &|_| panic!("a starter that failed"))
    }));
    assert!(panicked.is_err());
    let RunStart::Started { run, .. } =
        runner::run(d.path(), &picked, RunAction::Picked, BrowserMode::Hidden, false, &show, &|a| fake.launch(a)).unwrap()
    else {
        panic!()
    };
    while !matches!(rx.recv_timeout(Duration::from_secs(20)).unwrap(), RunEvent::Done { run: r, .. } if r == run) {}
}

#[cfg(windows)]
#[test]
fn a_short_command_that_takes_the_slot_after_the_dry_count_is_waited_for() {
    let _held_browsers = crate::serial::held_browsers();
    let _slot = crate::serial::pw_repo();
    let d = clone();
    let fake = Fake::new("dry-two.json");
    let (show, rx) = collect();
    let (brief_sink, brief_rx) = collect();
    let picked = rating_methods(&tree_of(d.path()));
    let took: Mutex<Option<u32>> = Mutex::new(None);

    // Between the dry count's end and the run's start, an accounts command
    // (short, about two seconds) takes the slot.
    let launch = |a: &[String]| {
        let real = !a.iter().any(|x| x == "--list");
        if real && took.lock().unwrap().is_none() {
            let ping = ["/D", "/C", "ping", "-n", "3", "127.0.0.1"].map(OsString::from).to_vec();
            let accounts = Launch::test_program(cmd(), ping, fake.dir.path().to_path_buf(), "npm run users").brief();
            *took.lock().unwrap() = Some(v2_lib::pw_repo::process::start(accounts, brief_sink.clone()).unwrap());
        }
        fake.launch(a)
    };
    let RunStart::Started { run, .. } =
        runner::run(d.path(), &picked, RunAction::Picked, BrowserMode::Hidden, false, &show, &launch).unwrap()
    else {
        panic!()
    };
    // The run was not refused: it waited for the accounts command, which
    // ended (the slot holds one command at a time).
    let accounts = took.lock().unwrap().unwrap();
    assert!(run > accounts);
    while !matches!(brief_rx.recv_timeout(Duration::from_secs(20)).unwrap(), RunEvent::Done { run: r, .. } if r == accounts) {}
    // A short command waits at least as long as a listing may take.
    assert!(v2_lib::pw_repo::process::SLOT_WAIT >= runner::LIST_TIMEOUT + Duration::from_secs(10));
    while !matches!(rx.recv_timeout(Duration::from_secs(20)).unwrap(), RunEvent::Done { run: r, .. } if r == run) {}
}

/// A listing Playwright could not load (two tests with one title, a syntax
/// error) has no tests and says why in `errors`: the tree carries the first
/// line of each, with where it is, so the screen can show them instead of
/// "no tests".
#[test]
fn errors_from_loading_the_tests_are_carried_on_the_tree() {
    let c = clone();
    let at = c.path().join("cli").join("data").join("roundtrip.test.mjs");
    let json = serde_json::json!({
        "suites": [],
        "errors": [
            {
                "message": "Error: duplicate test title \"competencies: parse then emit is byte-identical\", first declared in ../cli/data/roundtrip.test.mjs:25\n\n    at somewhere (x.js:1:1)",
                "location": { "file": at.to_string_lossy(), "line": 30, "column": 3 }
            },
            { "message": "\u{1b}[31mSyntaxError: Unexpected token\u{1b}[39m\nmore" },
        ]
    });
    let tree = build_tree(c.path(), &json);
    assert!(tree.tests.is_empty());
    assert_eq!(
        tree.load_errors,
        vec![
            "Error: duplicate test title \"competencies: parse then emit is byte-identical\", first declared in ../cli/data/roundtrip.test.mjs:25 (cli/data/roundtrip.test.mjs:30)".to_string(),
            "SyntaxError: Unexpected token".to_string(),
        ]
    );
}

#[test]
fn a_clean_listing_has_no_load_errors() {
    let c = clone();
    assert!(tree_of(c.path()).load_errors.is_empty());
}


// ---- The kept listing -----------------------------------------------------------
//
// The durable cache keeps one listing (every clone's key shares a prefix),
// so each test here holds the run slot's locks: no other test puts or
// evicts a listing in the middle of it.

fn kept_key(d: &Path) -> String {
    v2_lib::cache::keys::pw_list(&d.to_string_lossy())
}

/// What a listing is kept as: the fingerprint's hash and its suites and
/// errors.
fn keep(d: &Path, json: &serde_json::Value) {
    let print = runner::fingerprint_hash(&runner::fingerprint(d));
    v2_lib::cache::put(&kept_key(d), &(print, runner::kept_listing(json)));
}

#[test]
fn a_listing_kept_on_disk_is_used_while_the_clone_is_unchanged() {
    let _held_browsers = crate::serial::held_browsers();
    let _slot = crate::serial::pw_repo();
    let json = parse_list_stdout(&saved("list.json")).unwrap();
    let (show, _rx) = collect();

    // As after a restart: only the durable entry is there.
    let d = clone();
    keep(d.path(), &json);
    let never = |_: &[String]| -> Result<Launch, String> { panic!("nothing is listed while the kept listing holds") };
    assert_eq!(runner::list(d.path(), false, &show, &never).unwrap().tests.len(), 5);
    // Read once from disk, then from memory.
    assert_eq!(runner::list(d.path(), false, &show, &never).unwrap().tests.len(), 5);

    // The Runner's Refresh never uses it.
    let refused = |_: &[String]| -> Result<Launch, String> { Err("listed again".into()) };
    let d = clone();
    keep(d.path(), &json);
    assert_eq!(runner::list(d.path(), true, &show, &refused).unwrap_err(), "listed again");
}

#[test]
fn a_kept_listing_of_a_changed_clone_is_forgotten() {
    let _held_browsers = crate::serial::held_browsers();
    let _slot = crate::serial::pw_repo();
    let json = parse_list_stdout(&saved("list.json")).unwrap();
    let (show, _rx) = collect();
    let d = clone();
    keep(d.path(), &json);
    fs::write(d.path().join("suites").join(RM), "// changed\n").unwrap();
    let refused = |_: &[String]| -> Result<Launch, String> { Err("listed again".into()) };
    assert_eq!(runner::list(d.path(), false, &show, &refused).unwrap_err(), "listed again");
    assert!(v2_lib::cache::get::<serde_json::Value>(&kept_key(d.path())).is_none(), "forgotten, not left on disk");
}

#[cfg(windows)]
#[test]
fn a_new_listing_is_kept_as_its_suites_and_errors_with_a_hashed_fingerprint() {
    let _held_browsers = crate::serial::held_browsers();
    let _slot = crate::serial::pw_repo();
    let d = clone();
    let fake = Fake::new("dry-two.json");
    let (show, _rx) = collect();
    assert_eq!(runner::list(d.path(), true, &show, &|a| fake.launch(a)).unwrap().tests.len(), 5);
    let (print, json) = v2_lib::cache::get::<(String, serde_json::Value)>(&kept_key(d.path())).expect("kept on disk");
    assert_eq!(print, runner::fingerprint_hash(&runner::fingerprint(d.path())));
    assert_eq!(print.len(), 64, "a SHA-256, not the fingerprint itself");
    let mut keys: Vec<&str> = json.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort();
    assert_eq!(keys, ["errors", "suites"]);
    let full = parse_list_stdout(&saved("list.json")).unwrap();
    assert!(full.get("config").is_some(), "the fixture has a config to leave out");
    assert_eq!(json["suites"], full["suites"]);
}

#[cfg(windows)]
#[test]
fn a_listing_over_the_cap_is_kept_in_memory_only() {
    let _held_browsers = crate::serial::held_browsers();
    let _slot = crate::serial::pw_repo();
    let d = clone();
    let fake = Fake::new("dry-two.json");
    let mut big = parse_list_stdout(&saved("list.json")).unwrap();
    // Many short lines: the output is read a line at a time.
    let one = serde_json::json!({ "message": "x".repeat(60) });
    let n = v2_lib::cache::keys::PW_LIST_PERSIST_MAX / 60 + 1;
    big["errors"] = serde_json::Value::Array(vec![one; n]);
    fs::write(fake.dir.path().join("list.json"), serde_json::to_string_pretty(&big).unwrap()).unwrap();
    let (show, _rx) = collect();
    runner::list(d.path(), true, &show, &|a| fake.launch(a)).unwrap();
    assert!(v2_lib::cache::get::<serde_json::Value>(&kept_key(d.path())).is_none(), "not on disk");
    // This session still has it: no second listing.
    let never = |_: &[String]| -> Result<Launch, String> { panic!("listed again") };
    runner::list(d.path(), false, &show, &never).unwrap();
}

#[cfg(windows)]
#[test]
fn the_disk_keeps_one_clones_listing() {
    let _held_browsers = crate::serial::held_browsers();
    let _slot = crate::serial::pw_repo();
    let (one, two) = (clone(), clone());
    let (show, _rx) = collect();
    for d in [&one, &two] {
        let fake = Fake::new("dry-two.json");
        runner::list(d.path(), true, &show, &|a| fake.launch(a)).unwrap();
    }
    assert!(v2_lib::cache::get::<serde_json::Value>(&kept_key(one.path())).is_none());
    assert!(v2_lib::cache::get::<serde_json::Value>(&kept_key(two.path())).is_some());
}
