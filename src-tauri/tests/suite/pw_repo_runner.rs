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
    parse_list_stdout, parse_suite_path, plan, read_options, render_test_list, report_index, reports, run_args,
    test_titles, BrowserMode, LastResult, ListedTest, RunAction, RunOptions, RunStart, SuitePath, Target, NOTHING_FAILED,
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
        (create.country.as_str(), create.side.as_str(), create.module.as_str(), create.feature.as_str(), create.spec.as_str()),
        ("sl", "admin", "performance", "rating-methods", "rating-methods")
    );
    assert_eq!(create.file, RM);
    assert_eq!(create.title_path, ["Rating Methods", "User can create a new rating method"]);
    assert_eq!(create.line, 5);
    assert_eq!(create.project, "SL");
    assert_eq!(create.id, format!("[\"{RM}\",\"Rating Methods\",\"User can create a new rating method\"]"));

    // The shared tree has no side or module and runs under SL.
    let shared = find(&tree, "finds an employee by name");
    assert_eq!(
        (shared.country.as_str(), shared.side.as_str(), shared.module.as_str(), shared.feature.as_str()),
        ("common-components", "", "", "employee-search")
    );
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

#[test]
fn suite_paths_read_as_the_repo_reads_them() {
    assert_eq!(
        parse_suite_path(RM),
        Some(SuitePath::Country {
            country: "sl".into(),
            side: "admin".into(),
            module: "performance".into(),
            feature: "rating-methods".into(),
            stem: "rating-methods".into()
        })
    );
    assert_eq!(
        parse_suite_path("common-components/employee-search/specs/x.spec.ts"),
        Some(SuitePath::Common { component: "employee-search".into(), stem: "x".into() })
    );
    // A country module that happens to be called common-components keeps
    // its country.
    assert!(matches!(
        parse_suite_path("sl/admin/common-components/search/specs/x.spec.ts"),
        Some(SuitePath::Country { .. })
    ));
    for not_a_spec in [
        "_generated/x.spec.ts",
        "../src/users/registry.test.ts",
        "auth.setup.ts",
        "sl/elsewhere/performance/x/specs/x.spec.ts",
        "sl/admin/performance/x/x.spec.ts",
    ] {
        assert_eq!(parse_suite_path(not_a_spec), None, "{not_a_spec}");
    }
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
    assert_eq!(
        plan(d.path(), &tree, &["[\"gone\"]".to_string()], RunAction::Picked, BrowserMode::Hidden, &none).unwrap_err(),
        NOTHING_PICKED
    );

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
