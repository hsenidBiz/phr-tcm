//! The Auto Run report's analysis views: failures in clusters, a timeline per
//! failed case with its first anomaly, a stability grade, the environment's
//! history and the comparison with the run before - and the guarantees the
//! page keeps while it has a script of its own: every value escaped, no
//! secret, nothing fetched.

use std::collections::HashSet;

use serde_json::{json, Value};
use v2_lib::autorun::report::{build_smart, ReportExtras, TrendInput};
use v2_lib::autorun::report_smart::{
    clusters, compare_runs, grade, module_of, normalise_cause, normalise_target, summary_markdown, timeline, trend, Failure,
    DECIDED_BY_PERSON,
};
use v2_lib::autorun::{CaseRecord, CaseScript, LocalRun};
use v2_lib::webtheme::PagePalette;

use super::autorun_report::only_the_pages_own_scripts;

const SHOT: &str = "shot-1786000200000-000001.jpg";
const VIDEO: &str = "video-1786000200000-000009.mp4";

/// One step with one outcome.
fn step(number: i32, ms: Option<u64>, ok: bool, detail: &str) -> Value {
    let mut s = json!({ "step_number": number, "outcomes": [{ "ok": ok, "detail": detail }] });
    if let Some(ms) = ms {
        s["duration_ms"] = json!(ms);
    }
    s
}

/// A case with the given result (the person's verdict) and steps.
fn case(id: i32, verdict: &str, steps: Vec<Value>) -> Value {
    json!({ "case_id": id, "title": format!("Case {id}"), "verdict": verdict, "note": "", "steps": steps })
}

fn run(id: &str, started: u64, env: &str, cases: Vec<Value>) -> LocalRun {
    serde_json::from_value(json!({
        "id": id, "pbi_id": 42, "started_at": started.to_string(), "mode": "unattended",
        "environment": env, "cases": cases
    }))
    .unwrap()
}

fn script(id: i32, steps: Value) -> CaseScript {
    serde_json::from_value(json!({ "case_id": id, "title": format!("Case {id}"), "steps": steps })).unwrap()
}

fn passed(id: i32) -> Value {
    case(id, "Passed", vec![step(1, Some(1000), true, "ok")])
}

fn failed_at(id: i32, action_step: i32, detail: &str) -> Value {
    case(id, "Failed", vec![step(1, Some(1000), true, "ok"), step(action_step, Some(500), false, detail)])
}

fn record(v: Value) -> CaseRecord {
    serde_json::from_value(v).unwrap()
}

fn no_shots(_: &str) -> bool {
    false
}

fn all_shots(_: &str) -> bool {
    true
}

// ---------------------------------------------------------------- clusters

#[test]
fn normalising_takes_out_what_varies_and_keeps_what_does_not() {
    assert_eq!(normalise_cause("button \"Save\" not found"), normalise_cause("button \"Cancel\" not found"));
    assert_eq!(normalise_cause("button \"Save\" not found"), "button \"\u{2026}\" not found");
    assert_eq!(
        normalise_cause("timed out after 30000 ms on row 12"),
        normalise_cause("timed out after 500 ms on row 3")
    );
    assert_eq!(
        normalise_cause("record 3f2a9c10-7b1d-4e2a-9f00-123456789abc was not found"),
        normalise_cause("record 00000000-1111-2222-3333-444444444444 was not found")
    );
    // An address loses its query string, and its numbers go with the rest.
    assert_eq!(
        normalise_cause("loaded https://app.example/hr/list?token=abc123 slowly"),
        normalise_cause("loaded https://app.example/hr/list?token=zzz999 slowly")
    );
    assert!(!normalise_cause("loaded https://app.example/hr/list?token=abc123 slowly").contains("abc123"));
    // Different sentences stay different.
    assert_ne!(normalise_cause("button \"Save\" not found"), normalise_cause("button \"Save\" is disabled"));
    // An action's target stays: the Save button and the Cancel button are different places to fail.
    assert_ne!(normalise_target("click button \"Save\""), normalise_target("click button \"Cancel\""));
    assert_eq!(normalise_target("go to /hr/item/12"), normalise_target("go to /hr/item/987"));
    // A quote never closed is plain text.
    assert_eq!(normalise_cause("it said \"oops"), "it said \"oops");
}

fn click_script(id: i32, selector: &str) -> CaseScript {
    script(id, json!([{ "step_number": 1, "actions": [{ "kind": "click", "selector": selector }] }]))
}

#[test]
fn failures_with_the_same_cause_share_a_cluster_and_different_ones_do_not() {
    let r = run(
        "run-1",
        1000,
        "QA",
        vec![
            // 1 and 2: same step kind and target, same sentence once numbers and quoted values go.
            case(1, "Failed", vec![step(1, Some(10), false, "button \"Save\" not found after 5000 ms")]),
            case(2, "Failed", vec![step(1, Some(10), false, "button \"Save\" not found after 30000 ms")]),
            // 3: same target, another cause.
            case(3, "Failed", vec![step(1, Some(10), false, "button \"Save\" is disabled")]),
            // 4: same sentence as 1 but another target.
            case(4, "Failed", vec![step(1, Some(10), false, "button \"Save\" not found after 5000 ms")]),
            // 5: blocked, no step failed on its own.
            { let mut c = case(5, "Blocked", vec![]); c["note"] = json!("No test user"); c },
            passed(6),
        ],
    );
    let scripts: Vec<CaseScript> = vec![
        click_script(1, "text=Save"),
        click_script(2, "text=Save"),
        click_script(3, "text=Save"),
        click_script(4, "text=Cancel"),
    ];
    let (list, failures) = clusters(&r, &scripts, &Default::default());
    let sizes: Vec<usize> = list.iter().map(|c| c.cases.len()).collect();
    assert_eq!(sizes, vec![2, 1, 1, 1], "{list:?}");
    assert_eq!(list[0].cases, vec![1, 2], "the biggest first");
    assert_eq!(list[0].number, 1);
    assert_eq!(list[0].label(), "c1");
    // The one-case clusters keep the order their cases came in.
    assert_eq!(list[1].cases, vec![3]);
    assert_eq!(list[2].cases, vec![4]);
    assert_eq!(list[3].cases, vec![5]);
    // Blocked with nothing stopping it is clustered on what the run said.
    assert_eq!(list[3].action, "");
    assert_eq!(list[3].cause, "No test user");
    assert_eq!(failures.len(), 5, "only failed and blocked cases are failures");
    assert!(!failures.contains_key(&6));
}

#[test]
fn a_case_decided_by_the_person_without_any_words_says_so() {
    let r = run("run-1", 1000, "QA", vec![case(1, "Failed", vec![step(1, None, true, "ok")])]);
    let (list, failures) = clusters(&r, &[], &Default::default());
    assert_eq!(failures[&1].message, DECIDED_BY_PERSON);
    assert_eq!(list[0].cause, DECIDED_BY_PERSON);
}

// ---------------------------------------------------------------- timeline

fn passing_history(step2_ms: &[u64]) -> Vec<CaseRecord> {
    step2_ms
        .iter()
        .map(|ms| record(case(7, "Passed", vec![step(1, Some(1000), true, "ok"), step(2, Some(*ms), true, "ok")])))
        .collect()
}

#[test]
fn a_timeline_marks_the_failing_step_and_the_first_anomaly_is_the_first_to_fail() {
    let failed = record(case(
        7,
        "Failed",
        vec![step(1, Some(1000), true, "ok"), step(2, Some(1200), false, "button \"Save\" not found"), step(3, None, false, "not run: an earlier action of this step failed")],
    ));
    let t = timeline(&failed, &[]);
    assert_eq!(t.steps.len(), 3);
    assert_eq!(t.steps.iter().map(|s| s.ms).collect::<Vec<_>>(), vec![Some(1000), Some(1200), None]);
    assert_eq!(t.steps.iter().map(|s| s.failed).collect::<Vec<_>>(), vec![false, true, false], "a skipped step did not fail");
    assert_eq!(t.anomaly, Some(1));
    assert!(t.steps.iter().all(|s| !s.slow && s.median_ms.is_none()), "no history, nothing is slow");
}

#[test]
fn a_step_is_slow_only_past_twice_its_median_with_four_passing_runs_behind_it() {
    let slow = |ms: u64| {
        record(case(
            7,
            "Failed",
            vec![step(1, Some(1000), true, "ok"), step(2, Some(ms), true, "ok"), step(3, Some(10), false, "boom")],
        ))
    };
    let four = passing_history(&[1000, 1000, 1000, 1000]);
    let refs: Vec<&CaseRecord> = four.iter().collect();

    // Exactly twice the median is not more than twice.
    assert!(!timeline(&slow(2000), &refs).steps[1].slow);
    // Over it, the slow step comes before the failing one and is the first anomaly.
    let t = timeline(&slow(2001), &refs);
    assert!(t.steps[1].slow);
    assert_eq!(t.steps[1].median_ms, Some(1000));
    assert_eq!(t.anomaly, Some(1), "{t:?}");
    // When nothing is slow the failing step is the first anomaly.
    assert_eq!(timeline(&slow(1500), &refs).anomaly, Some(2));

    // Three passing runs are not enough history.
    let three = passing_history(&[1000, 1000, 1000]);
    let refs3: Vec<&CaseRecord> = three.iter().collect();
    let t = timeline(&slow(9000), &refs3);
    assert!(!t.steps[1].slow && t.steps[1].median_ms.is_none());
    assert_eq!(t.anomaly, Some(2));

    // The median is a median: one outlier among the passes does not move it.
    let spiky = passing_history(&[1000, 1000, 1000, 90_000, 1000]);
    let refs_s: Vec<&CaseRecord> = spiky.iter().collect();
    assert_eq!(timeline(&slow(2500), &refs_s).steps[1].median_ms, Some(1000));
    // An even count averages the middle two.
    let even = passing_history(&[1000, 2000, 3000, 4000]);
    let refs_e: Vec<&CaseRecord> = even.iter().collect();
    assert_eq!(timeline(&slow(10), &refs_e).steps[1].median_ms, Some(2500));
}

// ---------------------------------------------------------------- grade

#[test]
fn the_grade_follows_the_pass_rate_and_the_flips() {
    let p = |s: &str| -> Vec<bool> { s.chars().map(|c| c == 'P').collect() };
    // Too few judged runs to grade.
    assert_eq!(grade(&[]), None);
    assert_eq!(grade(&p("PP")), None);
    // Steady.
    assert_eq!(grade(&p("PPP")), Some('A'));
    assert_eq!(grade(&p("PPPPPPPPPP")), Some('A'));
    assert_eq!(grade(&p("PPPPPPPPPPPPPPPPPPPF")), Some('A'), "95 - 2.1");
    // 90 - 4.4 = 85.6; 80 - 4.4 = 75.6 (just inside B); 70 - 4.4 = 65.6.
    assert_eq!(grade(&p("PPPPPPPPPF")), Some('B'));
    assert_eq!(grade(&p("PPPPPPPPFF")), Some('B'));
    assert_eq!(grade(&p("PPPPPPPFFF")), Some('C'));
    // 60 - 4.4 = 55.6; 40 - 4.4 = 35.6.
    assert_eq!(grade(&p("PPPPPPFFFF")), Some('D'));
    assert_eq!(grade(&p("PPPPFFFFFF")), Some('F'));
    // Always failing scores 0.
    assert_eq!(grade(&p("FFFFF")), Some('F'));
    // The same pass rate, changing its mind every run, grades far lower than failing in a row.
    assert_eq!(grade(&p("PPPPPFFFFF")), Some('D'));
    assert_eq!(grade(&p("PFPFPFPFPF")), Some('F'));
    // Three runs: 66.7 minus the flips.
    assert_eq!(grade(&p("PPF")), Some('D'));
    assert_eq!(grade(&p("PFP")), Some('F'));
    assert_eq!(grade(&p("FPP")), Some('D'));
}

// ---------------------------------------------------------------- history

fn history_fixture() -> (LocalRun, Vec<LocalRun>) {
    // The run the report is of: 1 failed (it passed before), 2 passed (it failed before),
    // 3 failed again, 4 is new, 5 passed again, 6 is gone.
    let now = run(
        "run-3",
        3000,
        "QA",
        vec![failed_at(1, 2, "boom"), passed(2), failed_at(3, 2, "boom"), passed(4), passed(5)],
    );
    let before = run(
        "run-2",
        2000,
        "QA",
        vec![passed(1), failed_at(2, 2, "boom"), failed_at(3, 2, "boom"), passed(5), passed(6)],
    );
    let older = run("run-1", 1000, "QA", vec![passed(1), passed(2), passed(3), passed(5)]);
    // Never counted: another environment, and a run newer than the report's.
    let other_env = run("run-x", 2500, "Staging", vec![failed_at(1, 2, "boom")]);
    let newer = run("run-9", 9000, "QA", vec![failed_at(5, 2, "boom")]);
    (now.clone(), vec![newer, now, other_env, before, older])
}

#[test]
fn the_history_is_this_environments_runs_up_to_this_one_and_the_compare_is_against_the_one_before() {
    let (now, all) = history_fixture();
    let t = trend(&now, &all, &[]);
    assert_eq!(t.points.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), vec!["run-1", "run-2", "run-3"]);
    assert_eq!(t.points.iter().filter(|p| p.current).count(), 1);
    assert!(t.points[2].current);
    // 5 cases: 3 passed, 2 failed -> 60 %.
    let last = &t.points[2];
    assert_eq!((last.total, last.passed, last.failed, last.blocked, last.not_run, last.flaky), (5, 3, 2, 0, 0, 0));
    assert_eq!(last.pass_rate, Some(60));
    assert_eq!(t.points[0].pass_rate, Some(100));

    let c = t.compare.expect("a previous run");
    assert_eq!(c.previous_id, "run-2");
    assert_eq!(c.new_failures, vec![1]);
    assert_eq!(c.fixed, vec![2]);
    assert_eq!(c.still_failing, vec![3]);
    assert_eq!(c.new_cases, vec![4]);
    assert_eq!(c.missing, vec![6]);
}

#[test]
fn a_lone_run_has_a_history_of_one_and_nothing_to_compare() {
    let r = run("run-1", 1000, "QA", vec![passed(1)]);
    let t = trend(&r, &[], &[]);
    assert_eq!(t.points.len(), 1);
    assert!(t.compare.is_none());
    assert_eq!(t.per_case[&1].strip, vec!['P']);
    assert_eq!(t.per_case[&1].grade, None);
}

#[test]
fn the_history_keeps_the_newest_twenty_runs_and_each_cases_strip_is_oldest_first() {
    let runs: Vec<LocalRun> = (1..=25u64)
        .map(|i| {
            let case = if i == 25 { failed_at(1, 2, "boom") } else { passed(1) };
            run(&format!("run-{i}"), i * 1000, "QA", vec![case])
        })
        .collect();
    let now = runs[24].clone();
    let t = trend(&now, &runs, &[]);
    assert_eq!(t.points.len(), 20);
    assert_eq!(t.points[0].id, "run-6");
    assert!(t.points[19].current);
    let strip: String = t.per_case[&1].strip.iter().collect();
    assert_eq!(strip, format!("{}F", "P".repeat(19)));
    assert_eq!(t.per_case[&1].grade, Some('A'));
}

#[test]
fn flaky_is_counted_the_way_the_app_counts_it() {
    let (now, all) = history_fixture();
    // Case 3 is marked flaky: its failure is counted as flaky, not failed, in every run.
    let t = trend(&now, &all, &[3]);
    let last = &t.points[2];
    assert_eq!((last.failed, last.flaky), (1, 1));
    assert_eq!(last.pass_rate, Some(60), "a flaky failure is still not a pass");
    assert_eq!(t.points[1].flaky, 1);
    // ... and is neither a new, fixed nor still-failing case in the comparison.
    let c = t.compare.unwrap();
    assert_eq!(c.still_failing, Vec::<i32>::new());
    assert_eq!(c.new_failures, vec![1]);
    assert!(t.per_case[&3].marked && t.per_case[&3].flaky());

    // The report's own summary agrees with the history point for the same run.
    let mut marked = script(3, json!([]));
    marked.flaky = Some(v2_lib::autorun::FlakyMark { marked_at: "1".into() });
    assert_eq!(v2_lib::autorun::report::flaky_count(&now, &[marked]), last.flaky);

    // Passing and failing among its recent runs makes a case flaky without any mark;
    // the same case failing in a row does not.
    let t = trend(&now, &all, &[]);
    assert!(t.per_case[&1].alternates, "case 1 went P, P, F... passed and failed lately");
    // Only the cases of this run are reported on; case 6 is not in it.
    assert!(!t.per_case.contains_key(&6));
    let alternating = trend(&now, &all, &[]);
    assert!(!alternating.per_case[&5].alternates, "case 5 only ever passed");
}

#[test]
fn comparing_two_runs_directly_ignores_flaky_marked_failures() {
    let before = run("a", 1, "QA", vec![passed(1), failed_at(2, 2, "x")]);
    let now = run("b", 2, "QA", vec![failed_at(1, 2, "x"), passed(2)]);
    let marked: HashSet<i32> = [1].into_iter().collect();
    let c = compare_runs(&before, &now, &marked);
    assert!(c.new_failures.is_empty(), "case 1 is a flaky failure, not a new failure");
    assert_eq!(c.fixed, vec![2]);
}

// ---------------------------------------------------------------- the rest of the text

#[test]
fn the_module_is_read_from_the_trip_the_run_made() {
    let went = record(case(1, "Passed", vec![step(-1, None, true, "Go to Leave (Group Menu Items turned off)"), step(1, None, true, "ok")]));
    assert_eq!(module_of(&went).as_deref(), Some("Leave"));
    let plain = record(case(1, "Passed", vec![step(-1, None, true, "Go to Time & Attendance")]));
    assert_eq!(module_of(&plain).as_deref(), Some("Time & Attendance"));
    let failed = record(case(1, "Failed", vec![step(-1, None, false, "could not find the menu")]));
    assert_eq!(module_of(&failed), None);
    assert_eq!(module_of(&record(passed(1))), None);
}

#[test]
fn the_failure_summary_has_what_a_ticket_needs_and_nothing_more() {
    let c = record(failed_at(201, 2, "button \"Save\" not found"));
    let f = Failure {
        step_number: Some(2),
        step: "Step 2".into(),
        action: "click text=Save".into(),
        message: "button \"Save\" not found".into(),
    };
    let md = summary_markdown(&c, &f, Some("QA"), "2 Oct 2026, 09:30", &[SHOT.to_string()], Some(VIDEO));
    assert!(md.starts_with("### #201 Case 201 - Failed\n"), "{md}");
    assert!(md.contains("- Environment: QA\n"));
    assert!(md.contains("- Ran: 2 Oct 2026, 09:30\n"));
    assert!(md.contains("- Failing step: Step 2 - click text=Save\n"));
    assert!(md.contains("- Error: button \"Save\" not found\n"));
    assert!(md.contains(&format!("- Screenshots: {SHOT}\n")));
    assert!(md.contains(&format!("- Video: {VIDEO}\n")));
    // No step failed: no step line; no pictures: no pictures line.
    let none = Failure { step_number: None, step: String::new(), action: String::new(), message: "decided".into() };
    let md = summary_markdown(&c, &none, None, "now", &[], None);
    assert!(!md.contains("Failing step") && !md.contains("Screenshots") && !md.contains("Environment"), "{md}");
}

// ---------------------------------------------------------------- the page

fn page(now: &LocalRun, all: &[LocalRun], scripts: &[CaseScript], exists: &dyn Fn(&str) -> bool) -> String {
    build_smart(
        now,
        scripts,
        &Default::default(),
        "2 Oct 2026, 09:30",
        exists,
        &|_| None,
        &ReportExtras { divergences: &[] },
        &TrendInput { runs: all, flaky_marked: &[] },
        &PagePalette::default(),
    )
}

/// The JSON the page's script reads.
fn data_of(html: &str) -> Value {
    let open = "<script type=\"application/json\" id=\"report-data\">";
    let at = html.find(open).expect("the data blob") + open.len();
    let end = html[at..].find("</script>").unwrap();
    serde_json::from_str(&html[at..at + end]).expect("the blob is JSON")
}

#[test]
fn the_page_has_every_view_the_clusters_the_timelines_and_the_history() {
    let (now, all) = history_fixture();
    let html = page(&now, &all, &[], &no_shots);
    for view in ["cases", "insights", "history", "gallery"] {
        assert!(html.contains(&format!("id=\"view-{view}\"")), "{view}");
    }
    for nav in ["failures", "flaky", "all", "insights", "history", "gallery"] {
        assert!(html.contains(&format!("data-nav=\"{nav}\"")), "{nav}");
    }
    // Something failed, so the page opens on Failures; the sidebar counts them.
    assert!(html.contains("data-start=\"failures\""));
    assert!(html.contains("<span class=\"rail__label\">Failures</span><span class=\"rail__count\">2</span>"));
    // The two failures share a cause: one group, a readable title, a real example error line.
    assert_eq!(html.matches("class=\"group\"").count() >= 1, true);
    assert!(html.contains("<p class=\"grp__note\">boom</p>"), "{html}");
    assert!(html.contains("Copy failure summary") && html.contains("Copy AI prompt"));
    assert!(html.contains("Possible first anomaly") && html.contains("class=\"timeline__bar\" data-failed"), "{html}");
    for (title, n) in [("New failures", 1), ("Fixed", 1), ("Still failing", 1), ("New cases", 1), ("Not in this run", 1)] {
        assert!(html.contains(&format!("<h2 class=\"panel__title\">{title}</h2><span class=\"panel__meta\">{n}</span>")), "{title}");
    }
    assert!(html.contains("<svg"), "the charts are inline SVG");
    // Every case still has its steps, notes and the rest in its detail.
    assert_eq!(html.matches("<article class=\"dt detail\"").count(), 5);
    assert!(html.contains("<span class=\"step__title\">Step 2</span>"));
    // The filter reads these.
    assert!(html.contains("data-status=\"failed\"") && html.contains("data-grade=\"A\"") && html.contains("data-cause=\"c1 "));
    only_the_pages_own_scripts(&html);
}

#[test]
fn a_case_title_cannot_break_out_of_the_data_blob_or_the_page() {
    let (mut now, all) = history_fixture();
    let evil = "</script><img src=x onerror=alert(1)><!-- <script>alert(2)</script> \u{2028}&";
    now.cases[0].title = evil.to_string();
    let html = page(&now, &all, &[], &all_shots);
    only_the_pages_own_scripts(&html);
    assert!(!html.contains("<img src=x"));
    // The blob parses and carries the title as text.
    let data = data_of(&html);
    let summary = data["sum"]["1"].as_str().expect("case 1 failed");
    assert!(summary.contains("</script>") && summary.contains("<img src=x onerror=alert(1)>"));
    // It is written with no raw `<`, `>` or `&` at all.
    let open = "<script type=\"application/json\" id=\"report-data\">";
    let at = html.find(open).unwrap() + open.len();
    let blob = &html[at..at + html[at..].find("</script>").unwrap()];
    assert!(!blob.contains('<') && !blob.contains('>') && !blob.contains('&') && !blob.contains('\u{2028}'), "{blob}");
}

#[test]
fn no_secret_reaches_the_page_or_its_data() {
    let (now, all) = history_fixture();
    let typed = script(
        1,
        json!([{ "step_number": 1, "actions": [{ "kind": "fill", "selector": "#password", "value": "hunter2-typed-secret" }] },
                { "step_number": 2, "actions": [{ "kind": "navigate", "url": "https://app.example/x?token=s3cr3t-token#frag" }] }]),
    );
    let mut now = now;
    now.cases[0].steps[1].outcomes[0].detail = "loaded https://app.example/x?token=s3cr3t-token#frag".into();
    let html = page(&now, &all, &[typed], &all_shots);
    for secret in ["hunter2-typed-secret", "s3cr3t-token", "#frag"] {
        assert!(!html.contains(secret), "{secret} leaked");
    }
    assert!(html.contains("loaded https://app.example/x"), "the address itself is still there");
}

/// Every value of every `src=` and `href=` in the page.
fn links(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    for attr in ["src=\"", "href=\""] {
        for (at, _) in html.match_indices(attr) {
            let rest = &html[at + attr.len()..];
            out.push(rest[..rest.find('"').unwrap()].to_string());
        }
    }
    out
}

#[test]
fn nothing_is_fetched_only_relative_pictures_and_anchors_are_linked() {
    let (mut now, all) = history_fixture();
    now.cases[0].steps[1].outcomes[0].screenshot = Some(SHOT.to_string());
    now.cases[0].video = Some(VIDEO.to_string());
    let html = page(&now, &all, &[], &all_shots);
    let found = links(&html);
    assert!(found.len() > 5);
    for l in &found {
        assert!(l.starts_with("../shots/") || l.starts_with('#'), "an outside link: {l}");
    }
    assert!(found.iter().any(|l| l == &format!("../shots/{SHOT}")));
    assert!(!html.contains("http://") && !html.contains("https://"), "no address at all in a page with none in its run");
    assert!(!html.contains("url(") && !html.contains("@import"), "the style loads nothing");
    // The page's own script reaches nothing on the network either.
    let js = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("web/autorun-report.js")).unwrap();
    for banned in ["fetch(", "XMLHttpRequest", "WebSocket", "import(", "sendBeacon", "eval(", "new Function"] {
        assert!(!js.contains(banned), "{banned}");
    }
}

#[test]
fn the_gallery_holds_the_pictures_and_videos_that_are_there_and_safe() {
    let (mut now, all) = history_fixture();
    now.cases[0].steps[1].outcomes[0].screenshot = Some(SHOT.to_string());
    now.cases[0].steps[1].screenshot = Some("../runs/x.jpg".to_string());
    now.cases[0].video = Some(VIDEO.to_string());
    now.cases[2].video = Some("video-1-2.mp4".to_string());
    let only: HashSet<&str> = [SHOT, VIDEO].into_iter().collect();
    let html = page(&now, &all, &[], &|n| only.contains(n));
    let data = data_of(&html);
    let g = data["gallery"].as_array().unwrap();
    let names: Vec<&str> = g.iter().map(|i| i["n"].as_str().unwrap()).collect();
    assert_eq!(names, vec![SHOT, VIDEO]);
    assert_eq!(g[0]["c"], 1);
    assert_eq!(g[1]["k"], 1, "a video says so");
    assert!(html.contains("data-gtab=\"all\">All <span class=\"tab__count\">2</span>"));
    // The summary to copy names the failing step's pictures and the video.
    let md = data["sum"]["1"].as_str().unwrap();
    assert!(md.contains(SHOT) && md.contains(VIDEO), "{md}");

    let empty = page(&now, &all, &[], &no_shots);
    assert!(empty.contains("No pictures or videos were kept for this run."));
}

fn web_file(name: &str) -> String {
    std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("web").join(name)).unwrap()
}

#[test]
fn print_is_always_the_light_palette_and_lays_the_report_out_in_order() {
    let (now, all) = history_fixture();
    let html = page(&now, &all, &[], &no_shots);
    // The tokens end with a print block that puts the light values on every mode.
    let dark = html.find(":root[data-mode=\"dark\"]{color-scheme:dark;").expect("the dark block");
    let print = html.find("@media print{:root,:root[data-mode=\"light\"],:root[data-mode=\"dark\"],:root:not([data-mode])").expect("the print block");
    assert!(dark < print, "the print block comes last, so it wins");
    let block = &html[print..html[print..].find("}}").map(|e| print + e).unwrap()];
    assert!(block.contains("color-scheme:light") && block.contains("--bg:#ffffff") && block.contains("--text:#171717"), "{block}");
    assert!(!block.contains("--bg:#111111"));
    // The chrome is left off paper; the failed cases are shown in full.
    let css = web_file("autorun-report.css");
    let printed = &css[css.find("@media print").unwrap()..];
    for hidden in [".topbar", ".rail", ".pane__list", ".btns", ".tabs", "#view-gallery"] {
        assert!(printed.contains(hidden), "{hidden} is hidden in print");
    }
    assert!(printed.contains(".dt[data-print=\"1\"]"));
    assert!(printed.contains("print-color-adjust: exact"), "the bars and marks print");
    // And the page says which cases that is.
    assert!(article_of_page(&html, 1).contains("data-print=\"1\"") && article_of_page(&html, 2).contains("data-print=\"0\""));
}

fn article_of_page(html: &str, id: i32) -> &str {
    super::autorun_report::article_of(html, id)
}

/// The stylelint rules of the design system the page follows, by hand: no colour, gradient or
/// effect of its own outside the token file, and only the scale's shapes, type, shadows and
/// transitions.
#[test]
fn the_pages_stylesheet_follows_the_design_systems_rules() {
    let css = web_file("autorun-report.css");
    let mut plain = String::new();
    let mut rest = css.as_str();
    while let Some(at) = rest.find("/*") {
        plain.push_str(&rest[..at]);
        rest = &rest[rest[at..].find("*/").map(|e| at + e + 2).unwrap()..];
    }
    plain.push_str(rest);
    for banned in ["rgb(", "rgba(", "hsl(", "hsla(", "hwb(", "lab(", "lch(", "oklab(", "oklch(", "color(", "color-mix(", "gradient(", "backdrop-filter", "text-shadow", "letter-spacing", "uppercase"] {
        assert!(!plain.contains(banned), "{banned}");
    }
    let chars: Vec<char> = plain.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if *c == '#' {
            let word: String = chars[i + 1..].iter().take_while(|c| c.is_ascii_alphanumeric()).collect();
            assert!(!((3..=8).contains(&word.len()) && word.chars().all(|c| c.is_ascii_hexdigit())), "a hex colour #{word}");
        }
    }
    let flat = plain.replace(['{', '}'], ";");
    let mut checked = 0;
    for decl in flat.split(';') {
        let Some((prop, value)) = decl.split_once(':') else { continue };
        let (prop, value) = (prop.trim(), value.trim());
        match prop {
            "filter" => panic!("filter is not allowed"),
            "box-shadow" => assert!(value == "none" || value == "var(--elevation)", "box-shadow: {value}"),
            "font-family" => assert!(value == "inherit" || value == "var(--font-sans)" || value == "var(--font-mono)", "{value}"),
            "font-size" => assert!(value == "inherit" || value.starts_with("var(--text-"), "font-size: {value}"),
            "transition" => assert!(
                value == "none" || value == "transform var(--duration-fast) var(--ease)" || value == "opacity var(--duration-fast) var(--ease)",
                "transition: {value}"
            ),
            p if p == "border-radius" || p.ends_with("-radius") => {
                for v in value.split_whitespace() {
                    assert!(matches!(v, "0" | "50%" | "var(--radius-sm)" | "var(--radius-md)"), "{p}: {v}");
                }
            }
            "color" | "background" | "background-color" | "fill" | "stroke" | "border-color" => {
                for w in value.split(|c: char| !c.is_ascii_alphabetic() && c != '-').filter(|w| !w.is_empty()) {
                    // Words inside var(--name) are names, not colours.
                    let in_var = w.starts_with("--") || w == "var";
                    assert!(in_var || matches!(w, "transparent" | "none" | "inherit" | "currentColor"), "{prop}: {value}");
                }
            }
            _ => continue,
        }
        checked += 1;
    }
    assert!(checked > 40, "the scan looked at {checked} declarations");
    // Box shadows only on overlays: the dialog, the lightbox, the narrow sidebar.
    assert_eq!(plain.matches("var(--elevation)").count(), 3);
    // The tokens are where the colours live.
    assert!(web_file("autorun-report-tokens.css").contains("--fail-fg:#cf222e"));
}

#[test]
fn a_hostile_palette_value_is_dropped_and_a_plain_one_is_the_accent() {
    use v2_lib::webtheme::safe_color;
    for ok in ["#1764d8", "#fff", "#ffffff80", "rgb(10, 20, 30)", "rgba(10,20,30,0.5)", "hsl(210 50% 40%)", "hsla(210, 50%, 40%, 0.8)"] {
        assert!(safe_color(ok), "{ok}");
    }
    for bad in ["red", "red}</style><script>alert(1)</script>", "url(http://x/y)", "#12", "#gggggg", "rgb(1,2,3);x", "rgb(1,2,3)}", "var(--x)", "expression(1)", "#fff\"", ""] {
        assert!(!safe_color(bad), "{bad}");
    }
    let (now, all) = history_fixture();
    let mut hostile = PagePalette::default();
    hostile.light.accent = "red}</style><script>alert(1)</script>".into();
    hostile.dark.accent = "url(http://x/y)".into();
    hostile.light.bg = "}</style><script>alert(2)</script>".into();
    let run_it = |pal: &PagePalette| {
        build_smart(&now, &[], &Default::default(), "x", &no_shots, &|_| None, &ReportExtras { divergences: &[] }, &TrendInput { runs: &all, flaky_marked: &[] }, pal)
    };
    let html = run_it(&hostile);
    assert!(!html.contains("alert(1)") && !html.contains("alert(2)") && !html.contains("url(http"), "dropped");
    only_the_pages_own_scripts(&html);
    assert!(html.contains("--accent-l:#1764d8"), "the default accent stays");
    assert!(!html.contains(":root{--accent-l:"), "no override was written");
    // A plain accent is the one thing taken from the palette.
    let mut good = PagePalette::default();
    good.light.accent = "#0a7d42".into();
    good.dark.accent = "rgb(10, 200, 100)".into();
    let html = run_it(&good);
    assert!(html.contains(":root{--accent-l:#0a7d42;--accent-d:rgb(10, 200, 100);}"), "{}", &html[..html.find("</style>").unwrap()]);
    // The neutrals and status colours are the page's own whatever the app wears.
    assert!(html.contains("--fail-fg:#cf222e"));
}

#[test]
fn a_big_run_with_a_full_history_stays_a_page_that_opens() {
    let cases = |fail: bool| -> Vec<Value> {
        (1..=500)
            .map(|id| {
                let steps: Vec<Value> = (1..=5).map(|n| step(n, Some(900 + n as u64), true, "ok")).collect();
                let mut c = case(id, if fail && id % 7 == 0 { "Failed" } else { "Passed" }, steps);
                if fail && id % 7 == 0 {
                    c["steps"][4] = step(5, Some(100), false, &format!("button \"Save\" not found after {id} ms"));
                }
                c
            })
            .collect()
    };
    let runs: Vec<LocalRun> = (1..=21u64).map(|i| run(&format!("run-{i}"), i * 1000, "QA", cases(i % 3 == 0))).collect();
    let now = runs[20].clone();
    let html = page(&now, &runs, &[], &no_shots);
    assert!(html.len() < 8_000_000, "{} bytes", html.len());
    only_the_pages_own_scripts(&html);
    // 71 failed cases (every seventh of 500) share one cause once the numbers are gone,
    // and only the newest 20 of the 21 runs are history.
    assert!(html.contains("id=\"list-failures\" data-title=\"Failures\" data-count=\"71\""));
    assert_eq!(html.matches("class=\"group\"").count(), 1, "one group of failures");
    assert_eq!(data_of(&html)["sum"].as_object().unwrap().len(), 71);
}

// ---------------------------------------------------------------- fix round 1

/// The number in the top bar's count of `tone` (`fail`, `pass`, `flaky`, `blocked`, `skip`).
fn top_count(html: &str, tone: &str) -> Option<usize> {
    let marker = format!("<span class=\"count__value tone-{tone}\">");
    let at = html.find(&marker)? + marker.len();
    html[at..html[at..].find('<').map(|e| at + e)?].parse().ok()
}

#[test]
fn the_counts_add_up_and_a_flaky_failure_counts_once() {
    let r = run(
        "run-1",
        1000,
        "QA",
        vec![
            passed(1),
            failed_at(2, 2, "boom"),
            failed_at(3, 2, "bang"),
            { let mut c = case(4, "Blocked", vec![]); c["note"] = json!("No test user"); c },
            case(5, "", vec![]),
            passed(6),
        ],
    );
    // Case 2 is marked flaky: its failure is a flaky one, and counted as that and nothing else.
    let mut marked = script(2, json!([]));
    marked.flaky = Some(v2_lib::autorun::FlakyMark { marked_at: "1".into() });
    let html = build_smart(
        &r,
        &[marked],
        &Default::default(),
        "x",
        &no_shots,
        &|_| None,
        &ReportExtras { divergences: &[] },
        &TrendInput { runs: &[], flaky_marked: &[2] },
        &PagePalette::default(),
    );
    let (failed, passed, flaky, blocked, not_run) = (
        top_count(&html, "fail").unwrap(),
        top_count(&html, "pass").unwrap(),
        top_count(&html, "flaky").unwrap(),
        top_count(&html, "blocked").unwrap(),
        top_count(&html, "skip").unwrap(),
    );
    assert_eq!((failed, passed, flaky, blocked, not_run), (1, 2, 1, 1, 1));
    assert_eq!(failed + passed + flaky + blocked + not_run, r.cases.len(), "every case counted once");
    // The sidebar and the lists agree: Failures is the failed and blocked, Flaky the flaky one.
    assert!(html.contains("<span class=\"rail__label\">Failures</span><span class=\"rail__count\">2</span>"));
    assert!(html.contains("<span class=\"rail__label\">Flaky</span><span class=\"rail__count\">1</span>"));
    assert!(html.contains("<span class=\"rail__label\">All cases</span><span class=\"rail__count\">6</span>"));
    // The History row for this run says the same, and so does the old summary table.
    assert!(html.contains("<td class=\"tone-fail\">1</td><td class=\"tone-flaky\">1</td>"), "the history row");
    assert!(html.contains("<td class=\"b-failed\">1</td><td class=\"b-blocked\">1</td><td class=\"b-notrun\">1</td><td class=\"b-flaky\">1</td>"));
    // A flaky failure is `flaky` to the filter, as it is to the counts.
    assert!(html.contains("data-id=\"2\" data-status=\"flaky\""));
    assert!(html.contains("data-id=\"3\" data-status=\"failed\""));
}

#[test]
fn history_and_compare_are_of_the_same_pbi_even_with_another_pbi_between() {
    let run_of_pbi = |id: &str, t: u64, pbi: i32, cases: Vec<Value>| {
        let mut r = run(id, t, "QA", cases);
        r.pbi_id = pbi;
        r
    };
    let a1 = run_of_pbi("a1", 1000, 42, vec![passed(1), passed(2)]);
    let b1 = run_of_pbi("b1", 2000, 43, vec![failed_at(90, 2, "boom"), passed(91)]);
    let a2 = run_of_pbi("a2", 3000, 42, vec![failed_at(1, 2, "boom"), passed(2)]);
    let b2 = run_of_pbi("b2", 3500, 43, vec![passed(90)]);
    let a3 = run_of_pbi("a3", 4000, 42, vec![failed_at(1, 2, "boom"), failed_at(2, 2, "bang"), passed(3)]);
    let all = vec![b2, a3.clone(), a1, b1.clone(), a2];

    let t = trend(&a3, &all, &[]);
    assert_eq!(t.points.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), vec!["a1", "a2", "a3"], "no run of PBI 43");
    let c = t.compare.expect("a previous run of this PBI");
    assert_eq!(c.previous_id, "a2", "the previous run of this PBI, not the one made just before");
    assert_eq!(c.still_failing, vec![1]);
    assert_eq!(c.new_failures, vec![2]);
    assert_eq!(c.new_cases, vec![3]);
    assert!(c.missing.is_empty() && c.fixed.is_empty(), "{c:?}");

    // The page says so, and for a PBI's first run says that instead of listing everything as new.
    let html = page(&a3, &all, &[], &no_shots);
    assert!(html.contains("3 recorded runs of PBI #42 in QA"), "{html}");
    let first = page(&b1, &all, &[], &no_shots);
    assert!(first.contains("This is the first run of PBI #43 in QA."), "{first}");
    assert!(first.contains("1 recorded run of PBI #43 in QA"));
}

#[test]
fn a_cases_own_history_is_not_cut_by_the_environments_newest_twenty_runs() {
    // Case 101 (PBI 42) went pass, fail, pass, fail, pass, fail in six runs, and has not run since;
    // twenty-five runs of another PBI were made in the same environment after that.
    let mut runs: Vec<LocalRun> = Vec::new();
    for i in 1..=6u64 {
        let c = if i % 2 == 1 { passed(101) } else { failed_at(101, 2, "boom") };
        runs.push(run(&format!("a{i}"), i * 1000, "QA", vec![c]));
    }
    for i in 1..=25u64 {
        let mut r = run(&format!("b{i}"), 10_000 + i * 1000, "QA", vec![passed(900 + i as i32)]);
        r.pbi_id = 43;
        runs.push(r);
    }
    let mut now = run("a7", 100_000, "QA", vec![passed(101)]);
    now.pbi_id = 42;
    let t = trend(&now, &runs, &[]);
    let c = &t.per_case[&101];
    assert!(c.alternates, "the Flaky list would list it, so the report marks it");
    assert_eq!(c.strip.iter().collect::<String>(), "PFPFPFP");
    assert!(c.grade.is_some());
    // The app's own flaky list, over the same runs, agrees.
    let mut env_runs = runs.clone();
    env_runs.push(now.clone());
    let app = v2_lib::autorun::flaky::flaky_cases(&env_runs, v2_lib::autorun::flaky::FLAKY_WINDOW);
    assert!(app.iter().any(|f| f.case_id == 101));
    // Its slow-step baseline is its own earlier passes, whatever PBI the runs between were of.
    assert_eq!(t.passes[&101].len(), 3, "the three earlier passes");
    // The run-level charts are this PBI's newest runs only.
    assert_eq!(t.points.iter().map(|p| p.id.as_str()).collect::<Vec<_>>().len(), 7);
}

#[test]
fn a_cluster_has_a_readable_title_and_a_real_error_line() {
    let r = run(
        "run-1",
        1000,
        "QA",
        vec![
            case(1, "Failed", vec![step(1, Some(10), false, "button \"Apply\" not found after 5000 ms")]),
            case(2, "Failed", vec![step(1, Some(10), false, "button \"Apply\" not found after 7 ms")]),
        ],
    );
    let scripts = vec![click_script(1, "text=Apply"), click_script(2, "text=Apply")];
    let (list, _) = clusters(&r, &scripts, &Default::default());
    assert_eq!(list.len(), 1);
    let c = &list[0];
    assert!(c.title.starts_with("Click ") && c.title.contains("Apply") && c.title.contains("not found"), "{}", c.title);
    assert_eq!(c.example, "button \"Apply\" not found after 5000 ms", "the first case's own line, not normalised");
    assert_eq!(c.label(), "c1");
}

#[test]
fn an_escaped_quote_inside_a_quoted_value_belongs_to_the_value() {
    // The browser's messages are Debug-formatted, so an inner quote arrives as \".
    assert_eq!(
        normalise_cause("expected text \"say \\\"hi\\\"\" not found"),
        normalise_cause("expected text \"other\" not found")
    );
    assert_eq!(normalise_cause("expected text \"say \\\"hi\\\"\" not found"), "expected text \"\u{2026}\" not found");
}

#[test]
fn the_timeline_says_where_the_anomaly_began() {
    let failed = record(case(
        7,
        "Failed",
        vec![step(1, Some(1000), true, "ok"), step(2, Some(3000), true, "ok"), step(3, Some(500), false, "boom")],
    ));
    let t = timeline(&failed, &[]);
    assert_eq!(t.total_ms, 4500);
    assert_eq!(t.anomaly, Some(2));
    assert_eq!(t.anomaly_at_ms, 4000, "the failing step began after the two before it");
}

#[test]
fn a_failed_step_is_always_the_failure_colour_and_slow_is_an_extra_note() {
    let (now, all) = history_fixture();
    let html = page(&now, &all, &[], &no_shots);
    // Only a step that did not fail is amber; a failed step carries only the failure flag.
    assert!(html.contains("class=\"timeline__bar\" data-failed"));
    assert!(!html.contains("data-failed data-slow") && !html.contains("data-slow data-failed"));
}
