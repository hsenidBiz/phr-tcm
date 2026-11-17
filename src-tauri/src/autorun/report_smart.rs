//! What the run report works out beyond listing a run: which failures share
//! a cause, where a failed case's time went, how a case has fared in this
//! environment's recent runs, and how this run compares with the one before.
//!
//! Everything here is a pure reading of run records already on this machine
//! (nothing is asked of Azure DevOps or the network), and nothing here holds
//! a password, a typed value, a cookie or a header: it only re-orders and
//! counts the sentences the run recorded, which the report already shows.
//!
//! The counting rules are the app's own: a case's bucket is
//! [`report::bucket`] (held to `tests/fixtures/verdict_buckets.json`), and a
//! failure of a case whose script is marked flaky counts as flaky, as Past
//! runs does. What makes a case "flaky in its recent runs" is
//! [`flaky::flaky_cases_with_marks`], not a second definition.

use std::collections::{HashMap, HashSet};

use super::components::ComponentFile;
use super::flaky::{flaky_cases_of, FLAKY_WINDOW};
use super::replay::MODULE_STEP;
use super::report::{case_bucket, counts, scrub_urls, step_label, stopped_action, stopping_point};
use super::{CaseRecord, CaseScript, LocalRun, StepRecord};

/// How many runs of one environment the trends look at, the run itself
/// included.
pub const HISTORY_RUNS: usize = 20;

/// A step is only called slow when the case has passed at least this many
/// times in the environment's history, and the step has at least this many
/// timings in those passes.
pub const MIN_PASSES_FOR_SLOW: usize = 4;

/// A step is slow when it took more than this many times its median.
pub const SLOW_FACTOR: u64 = 2;

/// What a case with no failing step and no recorded sentence is said to
/// have failed on: its result came from the person reviewing the run.
pub const DECIDED_BY_PERSON: &str = "decided by the person reviewing the run";

// ----------------------------------------------------------------------
// Failures grouped by cause
// ----------------------------------------------------------------------

/// Where a failed or blocked case stopped, in words.
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    /// The step's number (`MODULE_STEP`, `SIGN_IN_STEP` for the runner's
    /// own); `None` when no step failed on its own.
    pub step_number: Option<i32>,
    /// "Step 2", "Sign in"; empty when no step failed on its own.
    pub step: String,
    /// The action that stopped it, in words (never a typed value).
    pub action: String,
    /// The one sentence that says why, with addresses stripped of their
    /// query strings.
    pub message: String,
}

/// Where `case` stopped and why. The sentence is the failing action's own,
/// else the run's reason, else the case's note, else that a person decided.
pub fn failure_of(case: &CaseRecord, script: Option<&CaseScript>, components: &ComponentFile) -> Failure {
    match stopping_point(case) {
        Some((step, index, outcome)) => Failure {
            step_number: Some(step.step_number),
            step: step_label(step.step_number),
            action: stopped_action(case, step, index, script, components),
            message: scrub_urls(outcome.detail.trim()),
        },
        None => {
            let said = [case.reason.trim(), case.note.trim()].into_iter().find(|s| !s.is_empty());
            Failure {
                step_number: None,
                step: String::new(),
                action: String::new(),
                message: said.map(scrub_urls).unwrap_or_else(|| DECIDED_BY_PERSON.to_string()),
            }
        }
    }
}

/// A sentence with what varies from case to case taken out, so two cases
/// that failed the same way read the same: addresses lose their query,
/// quoted values become `"…"`, and any number or identifier becomes `#`.
pub fn normalise_cause(text: &str) -> String {
    normalise(text, false)
}

/// [`normalise_cause`] for an action's words: the quoted names stay (they
/// are the target - "click the Save button" and "click the Cancel button"
/// are different places to fail), numbers and identifiers still go.
pub fn normalise_target(text: &str) -> String {
    normalise(text, true)
}

fn normalise(text: &str, keep_quoted: bool) -> String {
    let scrubbed = scrub_urls(text);
    let mut quoted_out = String::with_capacity(scrubbed.len());
    let mut in_quote = false;
    let mut quote = String::new();
    let mut chars = scrubbed.chars().peekable();
    while let Some(c) = chars.next() {
        // The browser's messages are Debug-formatted: an inner quote is `\"`.
        if in_quote && c == '\\' && chars.peek() == Some(&'"') {
            quote.push(c);
            quote.push('"');
            chars.next();
        } else if c == '"' {
            if in_quote {
                if keep_quoted {
                    quoted_out.push_str(&quote);
                } else {
                    quoted_out.push('\u{2026}');
                }
                quoted_out.push('"');
                quote.clear();
            } else {
                quoted_out.push('"');
            }
            in_quote = !in_quote;
        } else if in_quote {
            quote.push(c);
        } else {
            quoted_out.push(c);
        }
    }
    // A quote never closed is plain text again.
    if in_quote {
        quoted_out.push_str(&quote);
    }
    let words: Vec<String> = quoted_out.split_whitespace().map(hide_numbers).collect();
    words.join(" ")
}

/// One whitespace-free chunk with its numbers and identifiers hidden.
fn hide_numbers(word: &str) -> String {
    let core = word.trim_matches(|c: char| !c.is_alphanumeric());
    let identifier = core.len() >= 8
        && core.chars().any(|c| c.is_ascii_digit())
        && core.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    if identifier {
        return word.replacen(core, "#", 1);
    }
    let mut out = String::with_capacity(word.len());
    let mut prev_digit = false;
    for c in word.chars() {
        if c.is_ascii_digit() {
            if !prev_digit {
                out.push('#');
            }
            prev_digit = true;
        } else {
            out.push(c);
            prev_digit = false;
        }
    }
    out
}

/// Failed and Blocked cases that stopped the same way: the same action in
/// the same place and the same sentence once what varies is taken out.
#[derive(Debug, Clone, PartialEq)]
pub struct Cluster {
    /// 1-based, in the order clusters are listed (`C1`, `C2`...).
    pub number: usize,
    /// The shared action, normalised; empty when no step failed on its own.
    pub action: String,
    /// The shared sentence, normalised.
    pub cause: String,
    /// The cases, in the order the run had them.
    pub cases: Vec<i32>,
    /// What a reader calls it: the failing action and the start of the
    /// sentence, "Click button \"Apply\": button \"...\" not found".
    pub title: String,
    /// A real error line from the first of its cases, not normalised.
    pub example: String,
}

impl Cluster {
    /// The short id the filter's `cause:` also takes (`c1`).
    pub fn label(&self) -> String {
        format!("c{}", self.number)
    }
}

/// `text` cut to `max` characters with an ellipsis, on a character boundary.
pub fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{}\u{2026}", cut.trim_end())
}

fn readable_title(action: &str, cause: &str) -> String {
    let cause = shorten(cause, 60);
    let mut chars = action.chars();
    let action: String = match chars.next() {
        Some(f) => f.to_uppercase().chain(chars).collect(),
        None => String::new(),
    };
    if action.is_empty() {
        cause
    } else {
        format!("{}: {cause}", shorten(&action, 60))
    }
}

/// Every Failed and Blocked case of `run` in clusters, the biggest cluster
/// first (ties: the one whose first case came first in the run), and for
/// each case the failure it was clustered on.
pub fn clusters(
    run: &LocalRun,
    scripts: &[CaseScript],
    components: &ComponentFile,
) -> (Vec<Cluster>, HashMap<i32, Failure>) {
    let mut failures: HashMap<i32, Failure> = HashMap::new();
    let mut groups: Vec<(String, String, Vec<i32>)> = Vec::new();
    for case in &run.cases {
        if !matches!(case_bucket(case), "Failed" | "Blocked") {
            continue;
        }
        let script = scripts.iter().find(|s| s.case_id == case.case_id);
        let failure = failure_of(case, script, components);
        let action = if failure.step_number.is_some() { normalise_target(&failure.action) } else { String::new() };
        let cause = normalise_cause(&failure.message);
        match groups.iter_mut().find(|(a, c, _)| *a == action && *c == cause) {
            Some((_, _, ids)) => ids.push(case.case_id),
            None => groups.push((action, cause, vec![case.case_id])),
        }
        failures.insert(case.case_id, failure);
    }
    // Stable: equal sizes keep the order their first case came in.
    groups.sort_by(|a, b| b.2.len().cmp(&a.2.len()));
    let out = groups
        .into_iter()
        .enumerate()
        .map(|(i, (action, cause, cases))| {
            let example = failures
                .get(&cases[0])
                .map(|f| shorten(f.message.lines().next().unwrap_or("").trim(), 200))
                .unwrap_or_default();
            Cluster { number: i + 1, title: readable_title(&action, &cause), action, cause, cases, example }
        })
        .collect();
    (out, failures)
}

// ----------------------------------------------------------------------
// Timeline of one case
// ----------------------------------------------------------------------

/// One step of a failed case's timeline.
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineStep {
    pub label: String,
    /// How long the step took; `None` when the run did not time it.
    pub ms: Option<u64>,
    /// The step is where the case stopped.
    pub failed: bool,
    /// More than [`SLOW_FACTOR`] times the median of its passing runs.
    pub slow: bool,
    /// That median, when there was enough history to have one.
    pub median_ms: Option<u64>,
}

/// A case's steps with their durations and where it first went wrong.
#[derive(Debug, Clone, PartialEq)]
pub struct Timeline {
    pub steps: Vec<TimelineStep>,
    /// The index into `steps` of the first anomaly: the first step that
    /// failed, or the first step that ran more than twice its median in
    /// this environment's history - whichever comes first.
    pub anomaly: Option<usize>,
    /// All the steps' durations added up, milliseconds.
    pub total_ms: u64,
    /// How long into the case the anomaly step began, milliseconds.
    pub anomaly_at_ms: u64,
}

fn step_failed(step: &StepRecord) -> bool {
    step.outcomes.iter().any(|o| !o.ok && !o.detail.starts_with("not run:"))
}

fn median(values: &mut [u64]) -> u64 {
    values.sort_unstable();
    let n = values.len();
    if n % 2 == 1 {
        values[n / 2]
    } else {
        (values[n / 2 - 1] + values[n / 2]) / 2
    }
}

/// `case`'s timeline. `passes` are this case's records from earlier runs in
/// the same environment that came to Passed: with at least
/// [`MIN_PASSES_FOR_SLOW`] of them, and as many timings of a step, a step
/// that took more than [`SLOW_FACTOR`] times the median of those timings is
/// slow. With less history, only a failing step is an anomaly.
pub fn timeline(case: &CaseRecord, passes: &[&CaseRecord]) -> Timeline {
    let enough = passes.len() >= MIN_PASSES_FOR_SLOW;
    let mut steps = Vec::with_capacity(case.steps.len());
    for step in &case.steps {
        let median_ms = if enough {
            let mut seen: Vec<u64> = passes
                .iter()
                .filter_map(|p| p.steps.iter().find(|s| s.step_number == step.step_number))
                .filter_map(|s| s.duration_ms)
                .collect();
            (seen.len() >= MIN_PASSES_FOR_SLOW).then(|| median(&mut seen))
        } else {
            None
        };
        let slow = matches!((step.duration_ms, median_ms), (Some(ms), Some(m)) if m > 0 && ms > m * SLOW_FACTOR);
        steps.push(TimelineStep {
            label: step_label(step.step_number),
            ms: step.duration_ms,
            failed: step_failed(step),
            slow,
            median_ms,
        });
    }
    let anomaly = steps.iter().position(|s| s.failed || s.slow);
    let total_ms = steps.iter().filter_map(|s| s.ms).sum();
    let anomaly_at_ms = anomaly.map_or(0, |i| steps[..i].iter().filter_map(|s| s.ms).sum());
    Timeline { steps, anomaly, total_ms, anomaly_at_ms }
}

// ----------------------------------------------------------------------
// Grade
// ----------------------------------------------------------------------

/// How few judged runs a case needs before it is graded at all: one pass
/// is no track record.
pub const MIN_JUDGED_FOR_GRADE: usize = 3;

/// A stability grade, A (steady) to F (unreliable), from a case's judged
/// outcomes in this environment, oldest first (`true` is a pass; Blocked
/// and not-run results are not judged and not passed in).
///
/// ```text
/// pass_rate = passes / judged
/// flip_rate = flips / (judged - 1)     a flip is a pass next to a fail
/// score     = 100 * pass_rate - 40 * flip_rate      (never below 0)
/// A: score >= 90   B: >= 75   C: >= 60   D: >= 40   F: below that
/// ```
///
/// A case that always passes scores 100; always failing, 0; passing every
/// run but one at the very end, out of 10 judged runs, about 86 (B); alternating, 10 (F). The
/// flips are what separate a case that failed twice in a row from one that
/// keeps changing its mind. `None` for fewer than [`MIN_JUDGED_FOR_GRADE`].
pub fn grade(judged: &[bool]) -> Option<char> {
    if judged.len() < MIN_JUDGED_FOR_GRADE {
        return None;
    }
    let n = judged.len() as f64;
    let passes = judged.iter().filter(|p| **p).count() as f64;
    let flips = judged.windows(2).filter(|w| w[0] != w[1]).count() as f64;
    let score = (100.0 * passes / n - 40.0 * flips / (n - 1.0)).max(0.0);
    Some(match score {
        s if s >= 90.0 => 'A',
        s if s >= 75.0 => 'B',
        s if s >= 60.0 => 'C',
        s if s >= 40.0 => 'D',
        _ => 'F',
    })
}

// ----------------------------------------------------------------------
// History and compare
// ----------------------------------------------------------------------

/// One run in the history charts.
#[derive(Debug, Clone, PartialEq)]
pub struct RunPoint {
    pub id: String,
    /// The run's start, epoch milliseconds as the run stored it.
    pub started_at: String,
    pub total: usize,
    pub passed: usize,
    /// Failed, not counting flaky failures.
    pub failed: usize,
    pub blocked: usize,
    pub not_run: usize,
    /// Failed cases whose script is marked flaky.
    pub flaky: usize,
    /// Passed over every case with a result (Passed, Failed, Blocked), as a
    /// percentage; `None` when none has one.
    pub pass_rate: Option<u32>,
    /// The cases' durations added up, milliseconds; 0 when none was timed.
    pub duration_ms: u64,
    /// The run the report is of.
    pub current: bool,
}

/// What a case's recent runs say about it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CaseTrend {
    /// One letter per run the case was in, oldest first, at most
    /// [`HISTORY_RUNS`]: `P`assed, `F`ailed, `B`locked, `N`ot run.
    pub strip: Vec<char>,
    pub grade: Option<char>,
    /// How many of those runs were judged (Passed or Failed).
    pub judged: usize,
    /// How many times a judged run differed from the one before it.
    pub flips: usize,
    /// The case passed and failed among its newest [`FLAKY_WINDOW`] runs in
    /// this environment (`flaky::flaky_cases_of`, as the Flaky list does).
    pub alternates: bool,
    /// The person marked the case's script flaky.
    pub marked: bool,
}

impl CaseTrend {
    /// Flaky in either sense.
    pub fn flaky(&self) -> bool {
        self.alternates || self.marked
    }

    /// How often the case changed its mind between judged runs, 0 to 100.
    pub fn flip_percent(&self) -> u32 {
        if self.judged < 2 {
            return 0;
        }
        ((self.flips as f64 / (self.judged - 1) as f64) * 100.0).round() as u32
    }
}

/// This run set against the one before it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Compare {
    pub previous_id: String,
    pub previous_started_at: String,
    /// Failing now, and in the previous run it was not.
    pub new_failures: Vec<i32>,
    /// Failing in the previous run, Passed now.
    pub fixed: Vec<i32>,
    pub still_failing: Vec<i32>,
    /// In this run and not in the previous one.
    pub new_cases: Vec<i32>,
    /// In the previous run and not in this one.
    pub missing: Vec<i32>,
    /// A title for every case named above (the newest run's own).
    pub titles: HashMap<i32, String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Trend<'a> {
    /// Runs of this PBI in this environment, oldest first, the report's run
    /// last, at most [`HISTORY_RUNS`].
    pub points: Vec<RunPoint>,
    pub per_case: HashMap<i32, CaseTrend>,
    /// Against the previous run of this PBI in this environment; `None` when
    /// this is the first.
    pub compare: Option<Compare>,
    /// Each earlier passing record of a case in this environment, newest
    /// first - what the timeline's "slow" is judged against.
    pub passes: HashMap<i32, Vec<&'a CaseRecord>>,
}

fn started(run: &LocalRun) -> u64 {
    run.started_at.trim().parse().unwrap_or(0)
}

/// A failed case that is not a flaky failure: what "failing" means when
/// runs are compared (a flaky failure is counted as flaky, as the summary
/// does).
fn failing(case: &CaseRecord, marked: &HashSet<i32>) -> bool {
    case_bucket(case) == "Failed" && !marked.contains(&case.case_id)
}

fn case_duration(case: &CaseRecord) -> u64 {
    match case.duration_ms {
        Some(ms) => ms.max(0) as u64,
        None => case.steps.iter().chain(case.cleanup.iter()).filter_map(|s| s.duration_ms).sum(),
    }
}

/// How long `case` took, milliseconds; 0 when nothing was timed.
pub fn duration_of(case: &CaseRecord) -> u64 {
    case_duration(case)
}

fn point(run: &LocalRun, marked: &HashSet<i32>, current: bool) -> RunPoint {
    let c = counts(run);
    let flaky = run.cases.iter().filter(|x| case_bucket(x) == "Failed" && marked.contains(&x.case_id)).count();
    let judged = c[0] + c[1] + c[2];
    RunPoint {
        id: run.id.clone(),
        started_at: run.started_at.clone(),
        total: run.cases.len(),
        passed: c[0],
        failed: c[1] - flaky,
        blocked: c[2],
        not_run: c[3],
        flaky,
        pass_rate: (judged > 0).then(|| ((c[0] as f64 / judged as f64) * 100.0).round() as u32),
        duration_ms: run.cases.iter().map(case_duration).sum(),
        current,
    }
}

/// The trends for `run` from every run on this machine (`runs`; `run`
/// itself may be among them), all in `run`'s environment and none newer.
///
/// Two windows, on purpose. The run-level history (`points`, `compare`) is
/// the newest [`HISTORY_RUNS`] runs of this PBI - another PBI's runs are a
/// different suite and would only muddy the charts and the comparison. A
/// case's own history (strip, grade, flaky marker, the runs its slow-step
/// baseline comes from) is that case's own newest [`HISTORY_RUNS`] runs in
/// the environment, whatever PBI made them, and its flaky marker is the
/// app's own (`flaky::flaky_cases_of` over every run of the environment), so
/// the report and the Flaky list never disagree.
///
/// `marked` is the ids of the cases whose script is marked flaky.
pub fn trend<'a>(run: &'a LocalRun, runs: &'a [LocalRun], marked: &[i32]) -> Trend<'a> {
    let marked_set: HashSet<i32> = marked.iter().copied().collect();
    let mine = started(run);
    let mut env: Vec<&LocalRun> = runs
        .iter()
        .filter(|r| r.id != run.id && r.environment == run.environment && started(r) <= mine)
        .collect();
    env.sort_by(|a, b| started(a).cmp(&started(b)).then_with(|| a.started_at.cmp(&b.started_at)));
    env.push(run);

    // Run level: this PBI only.
    let mut same_pbi: Vec<&LocalRun> = env.iter().copied().filter(|r| r.pbi_id == run.pbi_id).collect();
    if same_pbi.len() > HISTORY_RUNS {
        same_pbi.drain(..same_pbi.len() - HISTORY_RUNS);
    }
    let points: Vec<RunPoint> = same_pbi.iter().map(|r| point(r, &marked_set, r.id == run.id)).collect();

    // Case level: the app's own flaky definition over the whole environment.
    let alternating: HashSet<i32> = flaky_cases_of(&env, FLAKY_WINDOW, &[])
        .into_iter()
        .filter(|f| f.environment == run.environment && f.pattern.iter().any(|p| *p) && f.pattern.iter().any(|p| !*p))
        .map(|f| f.case_id)
        .collect();

    let mut seen: HashMap<i32, Vec<(&LocalRun, &CaseRecord)>> = HashMap::new();
    for r in &env {
        for case in &r.cases {
            seen.entry(case.case_id).or_default().push((r, case));
        }
    }
    let mut per_case: HashMap<i32, CaseTrend> = HashMap::new();
    let mut passes: HashMap<i32, Vec<&CaseRecord>> = HashMap::new();
    for (id, mut list) in seen {
        // Only the cases in this run are reported on.
        if !run.cases.iter().any(|c| c.case_id == id) {
            continue;
        }
        if list.len() > HISTORY_RUNS {
            list.drain(..list.len() - HISTORY_RUNS);
        }
        let mut t = CaseTrend::default();
        let mut judged: Vec<bool> = Vec::new();
        for (r, case) in &list {
            let b = case_bucket(case);
            t.strip.push(match b {
                "Passed" => 'P',
                "Failed" => 'F',
                "Blocked" => 'B',
                _ => 'N',
            });
            if matches!(b, "Passed" | "Failed") {
                judged.push(b == "Passed");
            }
            if b == "Passed" && r.id != run.id {
                passes.entry(id).or_default().push(case);
            }
        }
        t.judged = judged.len();
        t.flips = judged.windows(2).filter(|w| w[0] != w[1]).count();
        t.grade = grade(&judged);
        t.alternates = alternating.contains(&id);
        t.marked = marked_set.contains(&id);
        per_case.insert(id, t);
    }
    for list in passes.values_mut() {
        list.reverse();
    }

    let compare = (same_pbi.len() >= 2).then(|| compare_runs(same_pbi[same_pbi.len() - 2], run, &marked_set));
    Trend { points, per_case, compare, passes }
}

/// `now` against `before`.
pub fn compare_runs(before: &LocalRun, now: &LocalRun, marked: &HashSet<i32>) -> Compare {
    let was: HashMap<i32, &CaseRecord> = before.cases.iter().map(|c| (c.case_id, c)).collect();
    let is: HashMap<i32, &CaseRecord> = now.cases.iter().map(|c| (c.case_id, c)).collect();
    let mut out = Compare {
        previous_id: before.id.clone(),
        previous_started_at: before.started_at.clone(),
        ..Compare::default()
    };
    for case in &before.cases {
        out.titles.insert(case.case_id, case.title.clone());
    }
    for case in &now.cases {
        out.titles.insert(case.case_id, case.title.clone());
        match was.get(&case.case_id) {
            None => out.new_cases.push(case.case_id),
            Some(old) => {
                let (f_now, f_was) = (failing(case, marked), failing(old, marked));
                if f_now && f_was {
                    out.still_failing.push(case.case_id);
                } else if f_now {
                    out.new_failures.push(case.case_id);
                } else if f_was && case_bucket(case) == "Passed" {
                    out.fixed.push(case.case_id);
                }
            }
        }
    }
    for case in &before.cases {
        if !is.contains_key(&case.case_id) {
            out.missing.push(case.case_id);
        }
    }
    for list in [&mut out.new_failures, &mut out.fixed, &mut out.still_failing, &mut out.new_cases, &mut out.missing] {
        list.sort_unstable();
        list.dedup();
    }
    out
}

// ----------------------------------------------------------------------
// Insights
// ----------------------------------------------------------------------

/// The run's health: three scores out of 100 and their mean as a letter.
#[derive(Debug, Clone, PartialEq)]
pub struct Health {
    pub score: u32,
    pub letter: char,
    /// This run's pass rate.
    pub pass: u32,
    /// 100 less the share of this run's cases that are flaky.
    pub stability: u32,
    /// 100 when this run took no longer than the median of the previous
    /// runs of this PBI, less one point per percent over that.
    pub speed: u32,
}

/// The letter for a score, with the bands the grade uses.
pub fn letter_for(score: f64) -> char {
    match score {
        s if s >= 90.0 => 'A',
        s if s >= 75.0 => 'B',
        s if s >= 60.0 => 'C',
        s if s >= 40.0 => 'D',
        _ => 'F',
    }
}

/// How healthy the run is, from `points` (this PBI's runs, the report's
/// last) and how many of its `total` cases are `flaky_cases` flaky. `None`
/// when the run has no result at all.
pub fn health(points: &[RunPoint], flaky_cases: usize, total: usize) -> Option<Health> {
    let now = points.last()?;
    let pass = now.pass_rate?;
    let stability = if total == 0 { 100 } else { 100 - ((flaky_cases as f64 / total as f64) * 100.0).round() as u32 };
    let mut before: Vec<u64> = points[..points.len() - 1].iter().map(|p| p.duration_ms).filter(|d| *d > 0).collect();
    let speed = if before.is_empty() || now.duration_ms == 0 {
        100
    } else {
        let m = median(&mut before) as f64;
        let over = (now.duration_ms as f64 / m - 1.0) * 100.0;
        (100.0 - over).clamp(0.0, 100.0).round() as u32
    };
    let score = (f64::from(pass) + f64::from(stability) + f64::from(speed)) / 3.0;
    Some(Health { score: score.round() as u32, letter: letter_for(score), pass, stability, speed })
}

/// The `n` cases of `run` that took longest, slowest first: (case id, ms).
pub fn slowest(run: &LocalRun, n: usize) -> Vec<(i32, u64)> {
    let mut all: Vec<(i32, u64)> = run.cases.iter().map(|c| (c.case_id, case_duration(c))).filter(|(_, ms)| *ms > 0).collect();
    all.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    all.truncate(n);
    all
}

/// The `n` cases of the run that change their mind most between judged
/// runs, most first: (case id, percent of runs it flipped on). Only the
/// cases that count as flaky and flipped at least once.
pub fn most_flaky(per_case: &HashMap<i32, CaseTrend>, order: &[i32], n: usize) -> Vec<(i32, u32)> {
    let mut all: Vec<(i32, u32)> = order
        .iter()
        .filter_map(|id| per_case.get(id).map(|t| (*id, t)))
        .filter(|(_, t)| t.flaky() && t.flips > 0)
        .map(|(id, t)| (id, t.flip_percent()))
        .collect();
    // Stable: equal percentages keep the run's order.
    all.sort_by(|a, b| b.1.cmp(&a.1));
    all.truncate(n);
    all
}

// ----------------------------------------------------------------------
// The module a case ran in, and the text to copy
// ----------------------------------------------------------------------

/// The module the run took `case` to: the run's own "Go to X" line from
/// its trip to the module (the case's Module is not kept with the run).
/// `None` when the case never got there.
pub fn module_of(case: &CaseRecord) -> Option<String> {
    let step = case.steps.iter().find(|s| s.step_number == MODULE_STEP)?;
    let first = step.outcomes.first().filter(|o| o.ok)?;
    let rest = first.detail.trim().strip_prefix("Go to ")?;
    let name = rest.split(" (").next().unwrap_or(rest).trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// A title or sentence on one line, for a Markdown list.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The failure summary a person pastes into a ticket or a chat, as
/// Markdown: the case, where and when it ran, the failing step, the
/// sentence the run recorded, and the names of the pictures and video.
/// Never a value a script typed or anything the run did not already show.
pub fn summary_markdown(
    case: &CaseRecord,
    failure: &Failure,
    environment: Option<&str>,
    ran_at: &str,
    pictures: &[String],
    video: Option<&str>,
) -> String {
    let mut s = format!("### #{} {} - {}\n", case.case_id, one_line(&case.title), case_bucket(case));
    if let Some(env) = environment.map(str::trim).filter(|e| !e.is_empty()) {
        s.push_str(&format!("- Environment: {}\n", one_line(env)));
    }
    s.push_str(&format!("- Ran: {}\n", one_line(ran_at)));
    if failure.step_number.is_some() {
        s.push_str(&format!("- Failing step: {} - {}\n", failure.step, one_line(&failure.action)));
    }
    s.push_str(&format!("- Error: {}\n", one_line(&failure.message)));
    if !pictures.is_empty() {
        s.push_str(&format!("- Screenshots: {}\n", pictures.join(", ")));
    }
    if let Some(v) = video {
        s.push_str(&format!("- Video: {v}\n"));
    }
    s
}
