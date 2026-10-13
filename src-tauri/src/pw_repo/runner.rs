//! The Playwright tab's test runner: the same runs `npm run qa` makes, with
//! the same safety, from the app.
//!
//! It mirrors the repo's own `cli/`:
//! - **The tree** (`cli/inventory/`): `npx playwright test --list
//!   --reporter=json --pass-with-no-tests`, flattened and kept only under the
//!   project that owns each spec's folder (`sl/**` under SL, `ph/**` under PH,
//!   `in/**` under IN, `common-components/**` under SL). Each test carries
//!   its ADO id, from the `// ADO <id>` marker above it or, failing that,
//!   from `suites/_generated/index.json` through the raw spec with the same
//!   test title, and its result in the newest report.
//! - **The run list** (`cli/run/test-list-file.mjs`): `.qa-cli/run.list`,
//!   one `[Project] > file` line for a whole spec and one `[Project] > file >
//!   describe > title` line per test otherwise. A title holding a delimiter
//!   cannot be addressed on its own, so its whole spec runs. Written
//!   atomically; it is the only file the runner writes in the clone.
//! - **The dry count** (`cli/run/verify-count.mjs`): the same projects and
//!   run list with `--list`, counted under the owning projects. Any
//!   mismatch with what was picked refuses the run with both numbers: a
//!   `--test-list` that matches nothing exits 0 having run nothing.
//! - **The arguments** (`cli/run/build-argv.mjs`): `--project` per project,
//!   `--test-list` or `--last-failed`, the browser mode, then the options of
//!   `qa-options.json` (`cli/state/`), which is read and never written. A
//!   missing file means the repo's defaults: no option flags at all.
//!
//! Every list, count and run is a repo command (`process`), one at a time.

use super::process::{self, Launch, RunEvent, Sink};
use crate::cache::{self, keys};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

/// The run list, relative to the clone: the path `qa.mjs` writes and passes.
pub const RUN_LIST: &str = ".qa-cli/run.list";
/// The run options file at the clone's root. Read, never written.
pub const OPTIONS_FILE: &str = "qa-options.json";
/// How long a listing or a dry count may take (the repo's own limit).
pub const LIST_TIMEOUT: Duration = Duration::from_secs(120);

pub const NOTHING_PICKED: &str = "Nothing ticked.";
pub const NOTHING_FAILED: &str = "Nothing failed in the last run.";
pub const NO_TESTS: &str = "No tests found. Nothing under suites has a spec yet.";
pub const LIST_STOPPED: &str = "Listing the tests was stopped before it finished.";
pub const LIST_TIMED_OUT: &str = "Playwright did not finish listing the tests within 2 minutes, so it was stopped.";
pub const LIST_UNREADABLE: &str = "Playwright's list of the tests could not be read. The output panel shows what it said.";
pub const RUN_LIST_NOT_WRITTEN: &str = "The run list could not be written in the clone. Settings → Logs has the details.";
pub const NO_SUCH_REPORT: &str = "That report is not in the clone's reports folder.";
pub const REPORT_NOT_OPENED: &str = "The report could not be opened in your browser. Settings → Logs has the details.";

/// The refusal for a second run while one is being prepared.
pub const PREPARING_ALREADY: &str = "Another run is being prepared. Wait for it to start, or stop it, then try again.";
/// The refusal for a clone that is not the one the tab is set to.
pub const NOT_THE_CHOSEN_CLONE: &str = "That folder is not the clone chosen in the Playwright tab. Choose it again, then try again.";

/// Held from a run's listing to its start (see `run`).
static PREPARING: Mutex<()> = Mutex::new(());

/// The refusal for ticked tests the listing no longer has.
pub fn missing_ticks(missing: usize) -> String {
    let what = if missing == 1 { "1 ticked test is".to_string() } else { format!("{missing} ticked tests are") };
    format!("{what} no longer in the repo's list, so nothing was run. Use List again, then tick what to run.")
}

/// `asked` (the webview's clone path) if it is `configured` (the app's
/// clone setting), as a path; refused otherwise. Compared without
/// surrounding space, trailing separators, the kind of slash, or case on
/// Windows.
pub fn chosen_clone(asked: &str, configured: &str) -> Result<PathBuf, String> {
    let norm = |p: &str| {
        let p = p.trim().replace('\\', "/");
        let p = p.trim_end_matches('/').to_string();
        if cfg!(windows) { p.to_lowercase() } else { p }
    };
    if configured.trim().is_empty() || norm(asked) != norm(configured) {
        return Err(NOT_THE_CHOSEN_CLONE.to_string());
    }
    Ok(PathBuf::from(configured.trim()))
}

/// Builds the launch for Playwright arguments (`playwright …`): in the app
/// `Launch::repo` with npx, in the tests a fake.
pub type Starter<'a> = &'a dyn Fn(&[String]) -> Result<Launch, String>;

// ---- Types the webview sees ------------------------------------------------

/// A test's result in the newest report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub enum LastResult {
    Passed,
    Failed,
    Flaky,
    Skipped,
}

/// One listed test, with the levels of the tree it hangs from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct ListedTest {
    /// Unique and stable across listings: `["<file>", ...title path]`.
    pub id: String,
    /// `sl`, `ph`, `in`, or `common-components` for the shared tree.
    pub country: String,
    /// `self` or `admin`; empty under `common-components`.
    pub side: String,
    /// Empty under `common-components`.
    pub module: String,
    /// The feature folder, or the component under `common-components`.
    pub feature: String,
    /// The spec's file name without `.spec.ts`.
    pub spec: String,
    /// Relative to `suites/`, with `/`.
    pub file: String,
    pub title: String,
    /// The describe titles, then the test's own.
    pub title_path: Vec<String>,
    pub line: u32,
    /// The project that runs it.
    pub project: String,
    pub ado_id: Option<i32>,
    pub last_result: Option<LastResult>,
}

/// The clone's tests, in the tree's order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct TestTree {
    pub tests: Vec<ListedTest>,
    /// Every project a test belongs to, sorted.
    pub projects: Vec<String>,
    /// The report the results come from, if any.
    pub last_report: Option<String>,
}

/// What to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub enum RunAction {
    /// The ticked tests.
    Picked,
    /// Every test in the tree.
    All,
    /// What failed last time (`--last-failed`).
    Failed,
}

/// How the browser runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub enum BrowserMode {
    Hidden,
    Shown,
    /// Playwright's Inspector, paused at the start of every test.
    Debug,
}

impl BrowserMode {
    /// The word `qa` uses for it.
    pub fn word(self) -> &'static str {
        match self {
            BrowserMode::Hidden => "hidden",
            BrowserMode::Shown => "shown",
            BrowserMode::Debug => "debug",
        }
    }
}

/// What a run would do, without doing it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct RunPreview {
    /// The run list's text; none for a re-run of failures.
    pub run_list: Option<String>,
    pub command: String,
    /// How many tests it runs.
    pub expected: u32,
    pub warnings: Vec<String>,
}

/// How a run start was answered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "kind")]
pub enum RunStart {
    /// The run is going: its output comes as `PwOutput`, its end as `PwDone`.
    Started {
        run: u32,
        command: String,
        /// `N selected · N matched`, or how many failures re-run.
        summary: String,
        warnings: Vec<String>,
    },
    /// Debug pauses at every test: `count` tests need `confirm`.
    ConfirmDebug { count: u32 },
}

/// One archived run's headline numbers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct ReportStats {
    pub passed: u32,
    pub failed: u32,
    pub flaky: u32,
    pub skipped: u32,
    pub duration_ms: u32,
}

/// One `reports/<runId>/` folder holding an `index.html`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct PwReport {
    /// The folder's name: the run's local start time.
    pub run_id: String,
    /// From the sibling `<runId>.json`, when it reads.
    pub stats: Option<ReportStats>,
}

// ---- Suite paths (cli/paths/suite-path.mjs) ----------------------------------

/// Where a spec sits in the suite tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuitePath {
    Country { country: String, side: String, module: String, feature: String, stem: String },
    Common { component: String, stem: String },
}

/// The taxonomy of a refactored spec, or `None` for anything else: raw
/// `_generated/` output, `../` paths, `auth.setup.ts`.
pub fn parse_suite_path(path: &str) -> Option<SuitePath> {
    let segments: Vec<&str> = path.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    if segments.iter().any(|s| *s == ".." || *s == "_generated") {
        return None;
    }
    let stem = segments.last()?.strip_suffix(".spec.ts")?.to_string();
    let specs = segments.iter().rposition(|s| *s == "specs")?;
    if specs >= 2 && segments[specs - 2] == "common-components" && (specs == 2 || segments[specs - 3] == "suites") {
        let component = segments[specs - 1].to_string();
        return Some(SuitePath::Common { component, stem });
    }
    if specs < 4 {
        return None;
    }
    let [country, side, module, feature] = [0, 1, 2, 3].map(|i| segments[specs - 4 + i].to_string());
    if side != "self" && side != "admin" {
        return None;
    }
    Some(SuitePath::Country { country, side, module, feature, stem })
}

/// The one project that runs a spec: its country upper-cased, or SL for the
/// shared tree (the baseline project every country's shared specs run in).
pub fn owning_project(path: &SuitePath) -> String {
    match path {
        SuitePath::Country { country, .. } => country.to_uppercase(),
        SuitePath::Common { .. } => "SL".to_string(),
    }
}

// ---- The listing (cli/inventory/) ----------------------------------------------

/// The listing's argv after `npx`.
pub fn list_args() -> Vec<String> {
    ["playwright", "test", "--list", "--reporter=json", "--pass-with-no-tests"].map(String::from).to_vec()
}

/// The JSON in a `--reporter=json` run's stdout. Anything printed ahead of
/// it (a dotenv banner, which itself holds a `{`) is passed over by trying
/// each `{` in turn.
pub fn parse_list_stdout(raw: &str) -> Result<Value, String> {
    for (tries, (at, _)) in raw.match_indices('{').enumerate() {
        if tries >= 50 {
            break;
        }
        let mut de = serde_json::Deserializer::from_str(&raw[at..]);
        if let Ok(v) = Value::deserialize(&mut de) {
            if v.is_object() {
                return Ok(v);
            }
        }
    }
    Err(LIST_UNREADABLE.to_string())
}

/// One spec of a listing or a report, with the describe chain above it.
#[derive(Debug, Clone)]
struct Spec {
    file: String,
    title: String,
    title_path: Vec<String>,
    line: u32,
    /// `(project, status)` per test entry.
    tests: Vec<(String, String)>,
}

fn str_of(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

/// Every spec under the top-level file suites, each with the describe titles
/// that lead to it (the file suite's own title is not one).
fn specs_of(json: &Value) -> Vec<Spec> {
    fn collect(suite: &Value, chain: &[String], out: &mut Vec<Spec>) {
        for spec in suite.get("specs").and_then(Value::as_array).into_iter().flatten() {
            let title = str_of(spec, "title");
            let tests = spec
                .get("tests")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|t| (str_of(t, "projectName"), str_of(t, "status")))
                .collect();
            out.push(Spec {
                file: str_of(spec, "file"),
                title_path: chain.iter().cloned().chain([title.clone()]).collect(),
                title,
                line: spec.get("line").and_then(Value::as_u64).unwrap_or(0) as u32,
                tests,
            });
        }
        for child in suite.get("suites").and_then(Value::as_array).into_iter().flatten() {
            let mut next = chain.to_vec();
            next.push(str_of(child, "title"));
            collect(child, &next, out);
        }
    }
    let mut out = Vec::new();
    for file_suite in json.get("suites").and_then(Value::as_array).into_iter().flatten() {
        collect(file_suite, &[], &mut out);
    }
    out
}

/// The specs a QA engineer sees: each kept only under the project that owns
/// its folder, which drops duplicates and everything that is not a spec.
fn owned(json: &Value) -> Vec<(Spec, SuitePath, String)> {
    specs_of(json)
        .into_iter()
        .filter_map(|s| {
            let parsed = parse_suite_path(&s.file)?;
            let project = owning_project(&parsed);
            s.tests.iter().any(|(p, _)| *p == project).then_some((s, parsed, project))
        })
        .collect()
}

/// How many tests a listing holds under their owning projects: what the
/// dry count compares. Playwright's own total also counts `auth.setup.ts`.
pub fn count_matches(raw: &str) -> Result<usize, String> {
    Ok(owned(&parse_list_stdout(raw)?).len())
}

/// A test's id: its file and title path, JSON-encoded.
pub fn test_id(file: &str, title_path: &[String]) -> String {
    let parts: Vec<&str> = std::iter::once(file).chain(title_path.iter().map(String::as_str)).collect();
    serde_json::to_string(&parts).unwrap_or_default()
}

/// `rating-methods` as `Rating Methods`.
pub fn to_title(segment: &str) -> String {
    segment
        .split('-')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The tree from a listing: the owned tests, sorted country, side, module,
/// feature and spec by their shown names, and tests by line.
pub fn build_tree(clone: &Path, json: &Value) -> TestTree {
    let titles = generated_titles(clone);
    let (last_report, results) = last_results(clone);
    let mut sources: HashMap<String, Vec<String>> = HashMap::new();
    let mut tests: Vec<ListedTest> = owned(json)
        .into_iter()
        .map(|(s, parsed, project)| {
            let (country, side, module, feature, spec) = match parsed {
                SuitePath::Country { country, side, module, feature, stem } => (country, side, module, feature, stem),
                SuitePath::Common { component, stem } => {
                    ("common-components".to_string(), String::new(), String::new(), component, stem)
                }
            };
            let source = sources.entry(s.file.clone()).or_insert_with(|| {
                std::fs::read_to_string(clone.join("suites").join(&s.file))
                    .map(|t| t.lines().map(str::to_string).collect())
                    .unwrap_or_default()
            });
            let ado_id = marker_above(source, s.line).or_else(|| titles.get(&s.title).copied());
            let id = test_id(&s.file, &s.title_path);
            ListedTest {
                last_result: results.get(&id).copied(),
                id,
                country,
                side,
                module,
                feature,
                spec,
                file: s.file,
                title: s.title,
                title_path: s.title_path,
                line: s.line,
                project,
                ado_id,
            }
        })
        .collect();
    let key = |t: &ListedTest| {
        let country = if t.country == "common-components" { "Common Components".to_string() } else { t.country.to_uppercase() };
        [country, to_title(&t.side), to_title(&t.module), to_title(&t.feature), t.spec.clone()].map(|s| s.to_lowercase())
    };
    tests.sort_by(|a, b| key(a).cmp(&key(b)).then(a.line.cmp(&b.line)).then(a.title.cmp(&b.title)));
    let mut projects: Vec<String> = tests.iter().map(|t| t.project.clone()).collect::<HashSet<_>>().into_iter().collect();
    projects.sort();
    TestTree { tests, projects, last_report }
}

/// The id in a `// ADO <id>` comment in the comment lines right above line
/// `line` (1-based) of `source`, nearest first.
pub fn marker_above(source: &[String], line: u32) -> Option<i32> {
    let mut i = (line as usize).checked_sub(1)?;
    while i > 0 {
        i -= 1;
        let text = source.get(i)?.trim();
        let comment = text.strip_prefix("//")?;
        if let Some(rest) = comment.trim().strip_prefix("ADO") {
            let digits: String = rest.trim().chars().take_while(char::is_ascii_digit).collect();
            if let Ok(id) = digits.parse() {
                return Some(id);
            }
        }
    }
    None
}

/// The test titles of the raw specs `index.json` maps, to their ids. A
/// refactored test keeps its raw spec's title; a title two ids share is
/// left out.
fn generated_titles(clone: &Path) -> HashMap<String, i32> {
    let Ok(repo) = crate::pw_export::clone::open(clone) else { return HashMap::new() };
    let mut out: HashMap<String, Option<i32>> = HashMap::new();
    for (id, file) in &repo.index {
        let Ok(id) = id.parse::<i32>() else { continue };
        if file.contains(['/', '\\']) || file.contains("..") {
            continue;
        }
        let text = std::fs::read_to_string(clone.join("suites/_generated").join(file)).unwrap_or_default();
        for title in test_titles(&text) {
            out.entry(title).and_modify(|v| if *v != Some(id) { *v = None }).or_insert(Some(id));
        }
    }
    out.into_iter().filter_map(|(t, id)| id.map(|id| (t, id))).collect()
}

/// The quoted titles of `test('…'` calls in a spec's source.
pub fn test_titles(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = source;
    while let Some(at) = rest.find("test(") {
        let before_ok = rest[..at].chars().last().is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == '.'));
        let after = rest[at + 5..].trim_start();
        rest = &rest[at + 5..];
        if !before_ok {
            continue;
        }
        let Some(quote) = after.chars().next().filter(|c| matches!(c, '\'' | '"' | '`')) else { continue };
        let mut title = String::new();
        let mut chars = after[1..].chars();
        let mut closed = false;
        while let Some(c) = chars.next() {
            match c {
                '\\' => {
                    if let Some(n) = chars.next() {
                        title.push(n);
                    }
                }
                c if c == quote => {
                    closed = true;
                    break;
                }
                c => title.push(c),
            }
        }
        if closed {
            out.push(title);
        }
    }
    out
}

/// The newest report's result for each test it ran, keyed by test id, and
/// that report's run id.
fn last_results(clone: &Path) -> (Option<String>, HashMap<String, LastResult>) {
    let Some(newest) = reports(clone).into_iter().next() else { return (None, HashMap::new()) };
    let json = std::fs::read_to_string(clone.join("reports").join(format!("{}.json", newest.run_id)))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok());
    let mut out = HashMap::new();
    for (spec, _, project) in json.as_ref().map(owned).unwrap_or_default() {
        let status = spec.tests.iter().find(|(p, _)| *p == project).map(|(_, s)| s.as_str());
        let result = match status {
            Some("expected") => LastResult::Passed,
            Some("unexpected") => LastResult::Failed,
            Some("flaky") => LastResult::Flaky,
            Some("skipped") => LastResult::Skipped,
            _ => continue,
        };
        out.insert(test_id(&spec.file, &spec.title_path), result);
    }
    (Some(newest.run_id), out)
}

// ---- Options (cli/state/options-file.mjs, run-options.mjs) ------------------

/// The run options; `None` emits no flag, so the config's own value holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunOptions {
    pub retries: Option<u64>,
    pub max_failures: Option<u64>,
    pub timeout: Option<u64>,
    pub global_timeout: Option<u64>,
    pub trace: Option<String>,
}

/// `qa-options.json` as read: always usable options, and what was wrong.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OptionsRead {
    pub options: RunOptions,
    pub errors: Vec<String>,
    pub exists: bool,
}

/// Playwright's `--trace` modes.
pub const TRACE_MODES: [&str; 7] = [
    "on",
    "off",
    "on-first-retry",
    "on-all-retries",
    "retain-on-failure",
    "retain-on-first-failure",
    "retain-on-failure-and-retries",
];

/// Reads `qa-options.json` from the clone, validating each key the way the
/// repo does. Absent is the defaults; a bad file or key falls back to the
/// default for what could not be read, and says why. Never writes.
pub fn read_options(clone: &Path) -> OptionsRead {
    let path = clone.join(OPTIONS_FILE);
    let mut out = OptionsRead { exists: path.is_file(), ..Default::default() };
    if !out.exists {
        return out;
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            out.errors.push(format!("Could not read {OPTIONS_FILE}: {e}"));
            return out;
        }
    };
    let map = match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(m)) => m,
        Ok(_) => {
            out.errors.push(format!("{OPTIONS_FILE} must be a JSON object."));
            return out;
        }
        Err(e) => {
            out.errors.push(format!("{OPTIONS_FILE} is not valid JSON: {e}"));
            return out;
        }
    };
    for (key, value) in map {
        let input = match &value {
            Value::Null => continue,
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        let input = input.trim();
        let o = &mut out.options;
        let verdict: Result<(), String> = match key.as_str() {
            "trace" => {
                if input.is_empty() {
                    Ok(())
                } else if TRACE_MODES.contains(&input) {
                    o.trace = Some(input.to_string());
                    Ok(())
                } else {
                    Err(format!("\"{input}\" is not one of: {}", TRACE_MODES.join(", ")))
                }
            }
            "retries" => number(input, None, Some(Some(0))).map(|v| o.retries = v),
            // `off` and 0 both mean no limit.
            "maxFailures" => number(input, Some("off"), Some(None)).map(|v| o.max_failures = v),
            // 0 is unlimited, overriding the config's 90 s.
            "timeout" => number(input, None, Some(Some(0))).map(|v| o.timeout = v),
            // 0 restates the default (none): no flag.
            "globalTimeout" => number(input, None, Some(None)).map(|v| o.global_timeout = v),
            _ => {
                out.errors.push(format!("Unknown option \"{key}\" in {OPTIONS_FILE}, ignored."));
                continue;
            }
        };
        if let Err(e) = verdict {
            out.errors.push(format!("\"{key}\": {e}, so its default is used."));
        }
    }
    out
}

/// A whole number option: blank is `None`, `off_word` (if any) is `None`,
/// zero is `zero`.
fn number(input: &str, off_word: Option<&str>, zero: Option<Option<u64>>) -> Result<Option<u64>, String> {
    if input.is_empty() || off_word.is_some_and(|w| input.eq_ignore_ascii_case(w)) {
        return Ok(None);
    }
    if !input.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("\"{input}\" is not a whole number"));
    }
    let v: u64 = input.parse().map_err(|_| format!("\"{input}\" is too large"))?;
    Ok(if v == 0 { zero.unwrap_or(Some(0)) } else { Some(v) })
}

// ---- The run list and the arguments (cli/run/) ------------------------------

/// The lines for a selection, what they will run, and why any spec widened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestList {
    pub lines: Vec<String>,
    pub warnings: Vec<String>,
    /// Ids of the tests the lines run (a widened spec runs all of its own).
    pub expected: Vec<String>,
    /// Sorted.
    pub projects: Vec<String>,
}

const DELIM: &str = " > ";

/// The fewest lines that run `picked` out of `tests` (in tree order): one
/// line for a whole spec, else one per test with its title path.
pub fn build_test_list(tests: &[ListedTest], picked: &HashSet<String>) -> TestList {
    let mut out = TestList { lines: vec![], warnings: vec![], expected: vec![], projects: vec![] };
    let mut files: Vec<&str> = Vec::new();
    for t in tests {
        if !files.contains(&t.file.as_str()) {
            files.push(&t.file);
        }
    }
    let mut projects = HashSet::new();
    for file in files {
        let spec: Vec<&ListedTest> = tests.iter().filter(|t| t.file == file).collect();
        let selected: Vec<&ListedTest> = spec.iter().copied().filter(|t| picked.contains(&t.id)).collect();
        if selected.is_empty() {
            continue;
        }
        let project = &selected[0].project;
        projects.insert(project.clone());
        let file = file.replace('\\', "/");
        let prefix = format!("[{project}]{DELIM}");
        let unaddressable: Vec<&&ListedTest> =
            selected.iter().filter(|t| t.title_path.iter().any(|p| p.contains('>') || p.contains('\u{203A}'))).collect();
        if selected.len() == spec.len() || !unaddressable.is_empty() {
            for t in unaddressable {
                out.warnings.push(format!(
                    "\"{}\" contains a title delimiter, so it cannot be selected on its own. All of {file} runs instead.",
                    t.title
                ));
            }
            out.lines.push(format!("{prefix}{file}"));
            out.expected.extend(spec.iter().map(|t| t.id.clone()));
            continue;
        }
        for t in selected {
            out.lines.push(format!("{prefix}{file}{DELIM}{}", t.title_path.join(DELIM)));
            out.expected.push(t.id.clone());
        }
    }
    out.projects = projects.into_iter().collect();
    out.projects.sort();
    out
}

/// The run list file's text, with the header `qa` writes.
pub fn render_test_list(lines: &[String], projects: &[String], mode: BrowserMode) -> String {
    let projects = if projects.is_empty() { "(none)".to_string() } else { projects.join(", ") };
    let mut text = format!(
        "# generated by npm run qa - project {projects}, browser {}\n# {} line(s). Regenerated on every run - edits here are not preserved.\n",
        mode.word(),
        lines.len()
    );
    for l in lines {
        text.push_str(l);
        text.push('\n');
    }
    text
}

/// What a run selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target<'a> {
    TestList(&'a str),
    LastFailed,
}

/// Playwright's arguments (after `playwright`), as `build-argv.mjs` makes
/// them. A dry count (`list`) stops after the selection: no mode and no
/// option, which could hang it (`--debug`) or cut it short
/// (`--max-failures`).
pub fn run_args(projects: &[String], target: Target, list: bool, mode: BrowserMode, o: &RunOptions) -> Result<Vec<String>, String> {
    if projects.is_empty() {
        return Err(NO_TESTS.to_string());
    }
    let mut args = vec!["test".to_string()];
    args.extend(projects.iter().map(|p| format!("--project={p}")));
    match target {
        Target::LastFailed => args.push("--last-failed".into()),
        Target::TestList(path) => args.push(format!("--test-list={path}")),
    }
    if list {
        args.extend(["--list", "--reporter=json", "--pass-with-no-tests"].map(String::from));
        return Ok(args);
    }
    let debug = mode == BrowserMode::Debug;
    match mode {
        BrowserMode::Debug => args.push("--debug".into()),
        BrowserMode::Shown => args.push("--headed".into()),
        BrowserMode::Hidden => {}
    }
    if let Some(v) = o.retries {
        args.push(format!("--retries={v}"));
    }
    if let Some(v) = o.global_timeout {
        args.push(format!("--global-timeout={v}"));
    }
    if let Some(v) = &o.trace {
        args.push(format!("--trace={v}"));
    }
    // --debug already means no timeout, one failure, headed, one worker.
    if !debug {
        if let Some(v) = o.timeout {
            args.push(format!("--timeout={v}"));
        }
        match o.max_failures {
            Some(1) => args.push("-x".into()),
            Some(v) => args.push(format!("--max-failures={v}")),
            None => {}
        }
    }
    Ok(args)
}

/// `args` (after `playwright`) as `npx playwright …`.
pub fn npx_args(args: &[String]) -> Vec<String> {
    std::iter::once("playwright".to_string()).chain(args.iter().cloned()).collect()
}

/// The command line a person could paste.
pub fn describe_command(args: &[String]) -> String {
    ["npx", "playwright"]
        .iter()
        .map(|s| s.to_string())
        .chain(args.iter().map(|a| {
            if a.contains(char::is_whitespace) || a.contains('"') {
                format!("\"{}\"", a.replace('"', "\\\""))
            } else {
                a.clone()
            }
        }))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The refusal for a dry count that disagrees, naming both numbers.
pub fn explain_mismatch(expected: usize, matched: usize) -> String {
    let refusal = if matched == 0 {
        format!(
            "{expected} selected, but Playwright matched none of them. The run was stopped: a run list that matches nothing exits successfully while running no tests, so this would have looked like a pass."
        )
    } else {
        format!("{expected} selected, but Playwright matched {matched}. The run was stopped rather than silently running the wrong set.")
    };
    format!("{refusal} If the specs changed, use List again, then run again.")
}

/// How many tests failed last time (`test-results/.last-run.json`).
pub fn failed_count(clone: &Path) -> usize {
    std::fs::read_to_string(clone.join("test-results/.last-run.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.get("failedTests").and_then(Value::as_array).map(Vec::len))
        .unwrap_or(0)
}

/// A run, worked out but not started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    pub run_list: Option<String>,
    /// Playwright's arguments, after `playwright`.
    pub args: Vec<String>,
    /// For the dry count; none for a re-run of failures.
    pub dry_args: Option<Vec<String>>,
    pub expected: usize,
    pub warnings: Vec<String>,
}

/// Works out a run of `action` over `tree`.
pub fn plan(clone: &Path, tree: &TestTree, picked: &[String], action: RunAction, mode: BrowserMode, o: &RunOptions) -> Result<Planned, String> {
    if action == RunAction::Failed {
        let failed = failed_count(clone);
        if failed == 0 {
            return Err(NOTHING_FAILED.to_string());
        }
        let args = run_args(&tree.projects, Target::LastFailed, false, mode, o)?;
        return Ok(Planned { run_list: None, args, dry_args: None, expected: failed, warnings: vec![] });
    }
    if tree.tests.is_empty() {
        return Err(NO_TESTS.to_string());
    }
    let selection: HashSet<String> = match action {
        RunAction::All => tree.tests.iter().map(|t| t.id.clone()).collect(),
        _ => picked.iter().cloned().collect(),
    };
    if action == RunAction::Picked {
        // A ticked test the listing no longer has would silently not run.
        let known: HashSet<&str> = tree.tests.iter().map(|t| t.id.as_str()).collect();
        let missing = selection.iter().filter(|id| !known.contains(id.as_str())).count();
        if missing > 0 {
            return Err(missing_ticks(missing));
        }
    }
    let list = build_test_list(&tree.tests, &selection);
    if list.lines.is_empty() {
        return Err(NOTHING_PICKED.to_string());
    }
    let target = Target::TestList(RUN_LIST);
    Ok(Planned {
        run_list: Some(render_test_list(&list.lines, &list.projects, mode)),
        args: run_args(&list.projects, target, false, mode, o)?,
        dry_args: Some(run_args(&list.projects, target, true, mode, o)?),
        expected: list.expected.len(),
        warnings: list.warnings,
    })
}

// ---- Running -----------------------------------------------------------------

/// Runs `args` (after `npx`) as a repo command and returns its stdout.
/// stderr goes to `show`, so Playwright's warnings reach the output panel.
fn capture(args: &[String], show: &Sink, launch: Starter) -> Result<String, String> {
    // A listing or a dry count: short, so another short command may wait
    // for it rather than be refused.
    let launched = launch(args)?.brief();
    let (tx, rx) = mpsc::channel::<RunEvent>();
    let tx = Mutex::new(tx);
    let out: Sink = Arc::new(move |e| {
        let _ = tx.lock().unwrap_or_else(|e| e.into_inner()).send(e);
    });
    let run = process::start_split(launched, out, show.clone())?;
    let mut deadline = Instant::now() + LIST_TIMEOUT;
    let mut timed_out = false;
    let mut lines = Vec::new();
    loop {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(RunEvent::Output { line, .. }) => lines.push(line),
            Ok(RunEvent::Done { run: done, code }) => {
                // The panel hears it end too: a tab opened since knows it.
                show(RunEvent::Done { run: done, code });
                if timed_out {
                    return Err(LIST_TIMED_OUT.to_string());
                }
                let text = lines.join("\n");
                return match code {
                    None => Err(LIST_STOPPED.to_string()),
                    // A load error in one spec still lists the rest.
                    Some(_) if text.contains('{') => Ok(text),
                    Some(c) => Err(format!("Playwright could not list the tests (exit {c}). The output panel shows what it said.")),
                };
            }
            Err(mpsc::RecvTimeoutError::Timeout) if !timed_out => {
                timed_out = true;
                let _ = process::stop(run);
                // Done always follows a stop, after the output grace.
                deadline = Instant::now() + process::OUTPUT_GRACE + Duration::from_secs(5);
            }
            Err(_) => return Err(if timed_out { LIST_TIMED_OUT } else { LIST_UNREADABLE }.to_string()),
        }
    }
}

/// The clone's tests: the session's listing unless `refresh`, else a new
/// one. The ADO ids and results are read again either way.
pub fn list(clone: &Path, refresh: bool, show: &Sink, launch: Starter) -> Result<TestTree, String> {
    let key = keys::pw_list(&clone.to_string_lossy());
    let print = fingerprint(clone);
    let cached = if refresh { None } else { cache::session_fresh::<(String, Arc<Value>)>(&key, keys::PW_LIST_TTL) };
    let raw = match cached {
        Some((was, v)) if was == print => v,
        _ => {
            let v = Arc::new(parse_list_stdout(&capture(&list_args(), show, launch)?)?);
            cache::session_put(&key, (print, v.clone()));
            v
        }
    };
    Ok(build_tree(clone, &raw))
}

/// What decides what `--list` returns, as the repo's `cli/inventory/cache.mjs`
/// fingerprints it: every `*.spec.ts` under `suites/` (skipping
/// `node_modules`) and `playwright.config.ts`, each by path, size and
/// modified time, sorted. Adding, editing or deleting a spec changes it.
pub fn fingerprint(clone: &Path) -> String {
    fn stamp(file: &Path, rel: &Path) -> String {
        let name = file.strip_prefix(rel).unwrap_or(file).to_string_lossy().replace('\\', "/");
        match std::fs::metadata(file) {
            Ok(m) => {
                let at = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0);
                format!("{name}:{}:{at}", m.len())
            }
            Err(_) => format!("{name}:missing"),
        }
    }
    fn specs(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = e.path();
            if path.is_dir() {
                if e.file_name() != "node_modules" {
                    specs(&path, out);
                }
            } else if e.file_name().to_string_lossy().ends_with(".spec.ts") {
                out.push(path);
            }
        }
    }
    let suites = clone.join("suites");
    let mut found = Vec::new();
    specs(&suites, &mut found);
    let mut parts: Vec<String> = found.iter().map(|f| stamp(f, &suites)).collect();
    parts.push(stamp(&clone.join("playwright.config.ts"), clone));
    parts.sort();
    parts.join("\n")
}

/// The run list and command a run would use. Writes nothing.
pub fn preview(clone: &Path, picked: &[String], action: RunAction, mode: BrowserMode, show: &Sink, launch: Starter) -> Result<RunPreview, String> {
    let tree = list(clone, false, show, launch)?;
    let read = read_options(clone);
    let p = plan(clone, &tree, picked, action, mode, &read.options)?;
    Ok(RunPreview {
        run_list: p.run_list,
        command: describe_command(&p.args),
        expected: p.expected as u32,
        warnings: read.errors.into_iter().chain(p.warnings).collect(),
    })
}

/// Writes `.qa-cli/run.list` in the clone, atomically.
pub fn write_run_list(clone: &Path, text: &str) -> Result<(), String> {
    let path = clone.join(RUN_LIST);
    let made = path.parent().map(std::fs::create_dir_all).unwrap_or(Ok(()));
    made.map_err(|e| e.to_string()).and_then(|_| crate::ai_tools::atomic_write(&path, text)).map_err(|e| {
        crate::applog::warn(format!("pw-repo: the run list could not be written: {e}"));
        RUN_LIST_NOT_WRITTEN.to_string()
    })
}

/// Runs `action`: the run list, the dry count, then the run, its output to
/// `show`. A dry count that disagrees refuses with both numbers and starts
/// nothing more. A debug run of more than one test needs `confirm`.
pub fn run(
    clone: &Path,
    picked: &[String],
    action: RunAction,
    mode: BrowserMode,
    confirm: bool,
    show: &Sink,
    launch: Starter,
) -> Result<RunStart, String> {
    // One run prepared at a time: no second `pw_run` rewrites the run list
    // between this one's write, its dry count and its start. An `npm run qa`
    // someone runs on the same clone outside the app is not covered.
    // A run that panicked while preparing leaves nothing to protect: its
    // lock is taken as free.
    let _preparing = match PREPARING.try_lock() {
        Ok(g) => g,
        Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => return Err(PREPARING_ALREADY.to_string()),
    };
    let tree = list(clone, false, show, launch)?;
    let read = read_options(clone);
    let p = plan(clone, &tree, picked, action, mode, &read.options)?;
    if mode == BrowserMode::Debug && p.expected > 1 && !confirm {
        return Ok(RunStart::ConfirmDebug { count: p.expected as u32 });
    }
    let summary = match (&p.run_list, &p.dry_args) {
        (Some(text), Some(dry)) => {
            write_run_list(clone, text)?;
            let waits = |a: &[String]| launch(a).map(|l| l.waiting(process::SLOT_WAIT));
            let matched = count_matches(&capture(&npx_args(dry), show, &waits)?)?;
            if matched != p.expected {
                crate::applog::info(format!("pw-repo: the dry count matched {matched} of {}; the run was refused", p.expected));
                return Err(explain_mismatch(p.expected, matched));
            }
            format!("{} selected · {matched} matched", p.expected)
        }
        _ => format!("Re-running {} failed test(s).", p.expected),
    };
    // A short command (an accounts command) that took the slot after the dry
    // count is waited for rather than refusing a checked run; only this run
    // writes the run list (`PREPARING`), so what it checked is what it runs.
    let launched = launch(&npx_args(&p.args))?.waiting(process::SLOT_WAIT);
    let run = process::start(launched, show.clone())?;
    Ok(RunStart::Started {
        run,
        command: describe_command(&p.args),
        summary,
        warnings: read.errors.into_iter().chain(p.warnings).collect(),
    })
}

// ---- Reports -------------------------------------------------------------------

/// Every `reports/<runId>/` with an `index.html`, newest first (run ids are
/// local timestamps, so their names sort by date).
pub fn reports(clone: &Path) -> Vec<PwReport> {
    let root = clone.join("reports");
    let mut out: Vec<PwReport> = std::fs::read_dir(&root)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().is_dir() && e.path().join("index.html").is_file())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .map(|run_id| PwReport { stats: stats(&root.join(format!("{run_id}.json"))), run_id })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| b.run_id.cmp(&a.run_id));
    out
}

fn stats(file: &Path) -> Option<ReportStats> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()?;
    let s = v.get("stats")?;
    let n = |k: &str| s.get(k).and_then(Value::as_f64).unwrap_or(0.0).clamp(0.0, u32::MAX as f64) as u32;
    Some(ReportStats { passed: n("expected"), failed: n("unexpected"), flaky: n("flaky"), skipped: n("skipped"), duration_ms: n("duration") })
}

/// A listed report's `index.html`. Only a plain folder name under
/// `reports/` holding one.
pub fn report_index(clone: &Path, run_id: &str) -> Result<PathBuf, String> {
    let plain = !run_id.is_empty() && !run_id.contains(['/', '\\', ':']) && run_id != "." && run_id != "..";
    let index = clone.join("reports").join(run_id).join("index.html");
    if plain && index.is_file() {
        Ok(index)
    } else {
        Err(NO_SUCH_REPORT.to_string())
    }
}
