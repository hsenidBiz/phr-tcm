//! A divergence: where a test case and the live application disagree,
//! reported by an assistant instead of fitting a check to the page, stored
//! per project, and ruled on by a person.
//!
//! The pure rules (`divergences::report_in`, `rule_in`, `blocking`,
//! `close_repaired`), the file (`load`, `update`), the case's words
//! (`case_text`) and the route (`POST /autorun-divergence`).

use crate::common;
use v2_lib::ado::AdoClient;
use v2_lib::ai_bridge::{module_guard_for, route, BridgeContext};
use v2_lib::autorun::case_text::{fold, CaseText};
use v2_lib::autorun::divergences::{
    close_for_editor_save, close_repaired_with, note_case_text, undo_in,
    blocking, close_repaired, load, needs_ruling, open_for, path, report_in, rule_in, run_sentence, tool_answer,
    update, Divergence, DivergenceKind, DivergenceStatus, Reported, Ruling, KEEP_RULED_MS,
};
use v2_lib::autorun::store::set_root;
use v2_lib::steps_xml::{build_steps_xml, Step};
use wiremock::matchers::{method as wm_method, path as wm_path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const NOW: u64 = 1_700_000_000_123;
const DAY: u64 = 24 * 60 * 60 * 1000;

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let n = N.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("tcm-autorun-divergences-{nanos}-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn ctx() -> BridgeContext {
    BridgeContext {
        org: "acme".into(),
        project: "Web".into(),
        preconditions_ref: Some("Custom.Pre".into()),
        ..BridgeContext::default()
    }
}

/// Case 147556: preconditions say the DEF table holds exactly 4 levels,
/// step 2 expects a dropdown of Level 1 to Level 4.
fn case_147556() -> v2_lib::ado::TestCaseFull {
    let steps = vec![
        Step { action: "Open the DEF page".into(), expected: "".into(), shared: None },
        Step {
            action: "Open the level dropdown".into(),
            expected: "The dropdown lists Level 1 to Level 4".into(),
            shared: None,
        },
    ];
    v2_lib::ado::TestCaseFull {
        id: 147556,
        title: "DEF levels".into(),
        tags: String::new(),
        automation_status: "Not Automated".into(),
        steps_xml: build_steps_xml(&steps),
        step_ids: vec![],
        steps_legacy: None,
        steps,
        module_value: String::new(),
        preconditions: "The DEF table holds exactly 4 levels".into(),
        preconditions_legacy: None,
        preconditions_stored: None,
        priority: None,
    }
}

/// A signed-in client standing in for Azure DevOps: case 147556, with its
/// preconditions in the field the context names.
async fn client_with_case() -> (MockServer, AdoClient) {
    let server = MockServer::start().await;
    let case = case_147556();
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/_apis/wit/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": [{
            "id": case.id,
            "fields": {
                "System.Title": case.title,
                "Microsoft.VSTS.TCM.Steps": case.steps_xml,
                "Custom.Pre": case.preconditions,
            },
            "multilineFieldsFormat": { "Custom.Pre": "markdown" }
        }] })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    (server, client)
}

fn divergence(id: &str, case_id: i32, step: i32, kind: DivergenceKind, reported_at: u64) -> Divergence {
    Divergence {
        id: id.into(),
        case_id,
        step,
        case_says: "exactly 4 levels".into(),
        app_shows: "6 levels".into(),
        kind,
        reported_at: reported_at.to_string(),
        status: DivergenceStatus::Open,
        ruling: None,
        ruled_at: None,
        case_fp: None,
        step_action: None,
        ruled_by: None,
        said: None,
        note: None,
    }
}

fn data_report(list: &mut Vec<Divergence>) -> Reported {
    report_in(list, 147556, 2, "exactly 4 levels", "6 levels", DivergenceKind::Data, NOW).unwrap()
}

// -------------------------------------------------------------- case text

#[test]
fn fold_lowercases_collapses_and_trims_quotes() {
    assert_eq!(fold("  \u{201C}Exactly  4 *levels*.\u{201D} "), "exactly 4 levels");
}

#[test]
fn case_text_holds_words_from_steps_title_and_preconditions() {
    let text = CaseText::from_case(&case_147556());
    assert!(text.holds("exactly 4 levels"));
    assert!(text.holds("Level 4"));
    assert!(text.holds("DEF levels"));
    assert!(!text.holds("6 levels"));
    assert!(!text.holds("evel 4"), "part of a word is not a word");
    assert!(!text.holds("  "));
    assert_eq!(text.step_folded(2), "open the level dropdown\nthe dropdown lists level 1 to level 4");
    assert_eq!(text.step_folded(3), "");
    assert_eq!(text.preconditions_folded(), "the def table holds exactly 4 levels");
}

// ------------------------------------------------------------------ rules

#[test]
fn a_reported_data_divergence_is_stored_open_and_blocks_its_case() {
    let mut list = vec![];
    assert_eq!(data_report(&mut list), Reported::New("d1".into()));
    let d = blocking(&list, 147556).expect("blocks");
    assert_eq!(d.id, "d1");
    assert_eq!(d.status, DivergenceStatus::Open);
    assert_eq!(
        run_sentence(d),
        "needs a ruling: step 2: the case says \"exactly 4 levels\" but the app shows \"6 levels\" (data) - a person rules on it in Auto Run"
    );
    assert!(blocking(&list, 1).is_none());
    assert_eq!(open_for(&list, 147556).len(), 1);
    let answer = tool_answer(147556, d, false);
    assert!(answer.starts_with("Reported as d1. Case 147556 needs a ruling: its runs are Blocked with \"needs a ruling: step 2"), "{answer}");
    assert!(tool_answer(147556, d, true).starts_with("Updated as d1."));
    assert!(!answer.contains('\u{2014}') && !answer.contains('\u{2013}'));
}

#[test]
fn an_app_divergence_never_blocks() {
    let mut list = vec![];
    report_in(&mut list, 147556, 2, "exactly 4 levels", "6 levels", DivergenceKind::App, NOW).unwrap();
    assert!(blocking(&list, 147556).is_none());
    let answer = tool_answer(147556, &list[0], false);
    assert!(answer.starts_with("Reported as d1. Case 147556 is not blocked: keep the check as the case says, so the run fails at step 2"), "{answer}");
    assert!(answer.contains("mark the step with mark_autorun_suspected_defect"), "{answer}");
}

#[test]
fn the_same_divergence_reported_again_updates_it() {
    let mut list = vec![];
    data_report(&mut list);
    let again = report_in(&mut list, 147556, 2, "Exactly 4 levels", "7 levels", DivergenceKind::Data, NOW + 5).unwrap();
    assert_eq!(again, Reported::Updated("d1".into()));
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].app_shows, "7 levels");
    assert_eq!(list[0].reported_at, (NOW + 5).to_string());
    // Another kind at the same step is another divergence.
    let other = report_in(&mut list, 147556, 2, "exactly 4 levels", "6", DivergenceKind::Wording, NOW).unwrap();
    assert_eq!(other, Reported::New("d2".into()));
}

#[test]
fn a_divergence_ruled_the_case_stands_cannot_be_reported_again() {
    let mut list = vec![];
    data_report(&mut list);
    assert_eq!(rule_in(&mut list, "d1", Ruling::CaseStands, NOW + DAY).unwrap(), 147556);
    assert_eq!(list[0].status, DivergenceStatus::Ruled);
    let why = report_in(&mut list, 147556, 2, "Exactly 4 levels.", "6 LEVELS", DivergenceKind::Data, NOW + 2 * DAY)
        .unwrap_err();
    assert_eq!(
        why,
        "a person ruled on 2023-11-15 that the case stands at step 2 - keep the check as the case says; a failure there is the data's or the application's"
    );
    // A different app text is news.
    assert!(report_in(&mut list, 147556, 2, "exactly 4 levels", "8 levels", DivergenceKind::Data, NOW + 2 * DAY).is_ok());
}

#[test]
fn the_sixth_open_divergence_of_a_case_is_refused() {
    let mut list = vec![];
    for step in 1..=5 {
        report_in(&mut list, 147556, step, "exactly 4 levels", "6 levels", DivergenceKind::Data, NOW).unwrap();
    }
    let why = report_in(&mut list, 147556, 6, "exactly 4 levels", "6 levels", DivergenceKind::Data, NOW).unwrap_err();
    assert_eq!(why, "case 147556 already has 5 open divergences - wait for the person's ruling");
    // Another case is unaffected, and an update of an open one still lands.
    assert!(report_in(&mut list, 2, 1, "a", "b", DivergenceKind::Data, NOW).is_ok());
    assert_eq!(
        report_in(&mut list, 147556, 3, "exactly 4 levels", "7 levels", DivergenceKind::Data, NOW).unwrap(),
        Reported::Updated("d3".into())
    );
}

#[test]
fn a_report_is_checked_before_it_is_stored() {
    let mut list = vec![];
    let r = |list: &mut Vec<Divergence>, c, s, says: &str, shows: &str| {
        report_in(list, c, s, says, shows, DivergenceKind::Data, NOW).unwrap_err()
    };
    assert_eq!(r(&mut list, 0, 1, "a", "b"), "case_id is a test case's id");
    assert_eq!(r(&mut list, 1, 0, "a", "b"), "step is the case's step number, 1 or more");
    let blank = "a divergence needs \"case_says\" (the case's own words) and \"app_shows\" (what the page showed)";
    assert_eq!(r(&mut list, 1, 1, "  ", "b"), blank);
    assert_eq!(r(&mut list, 1, 1, "a", ""), blank);
    let one_line = "\"case_says\" and \"app_shows\" are each one line of at most 300 characters";
    assert_eq!(r(&mut list, 1, 1, &"x".repeat(301), "b"), one_line);
    assert_eq!(r(&mut list, 1, 1, "a", "two\nlines"), one_line);
    assert!(list.is_empty());
    assert!(report_in(&mut list, 1, 1, &"x".repeat(300), "b", DivergenceKind::Data, NOW).is_ok());
}

#[test]
fn rulings() {
    let mut list = vec![];
    report_in(&mut list, 147556, 2, "exactly 4 levels", "6 levels", DivergenceKind::App, NOW).unwrap();
    assert_eq!(rule_in(&mut list, "d1", Ruling::DataFixed, NOW).unwrap_err(), "Data fixed is for a data divergence");
    assert_eq!(rule_in(&mut list, "d9", Ruling::CaseStands, NOW).unwrap_err(), "no divergence d9");

    let mut list = vec![];
    data_report(&mut list);
    assert_eq!(rule_in(&mut list, "d1", Ruling::Rewrite, NOW + 1).unwrap(), 147556);
    assert_eq!(list[0].status, DivergenceStatus::AwaitingRepair);
    assert_eq!(list[0].ruling, Some(Ruling::Rewrite));
    assert_eq!(list[0].ruled_at.as_deref(), Some((NOW + 1).to_string().as_str()));
    assert_eq!(
        run_sentence(blocking(&list, 147556).unwrap()),
        "waits for a repair: step 2: the person is rewriting the case - repair the script to the case as it now reads"
    );
    assert_eq!(rule_in(&mut list, "d1", Ruling::CaseStands, NOW).unwrap_err(), "that divergence was already ruled on");
    assert_eq!(close_repaired(&mut list, 147556, &[1], NOW + 2), Vec::<i32>::new());
    assert!(blocking(&list, 147556).is_some());
    assert_eq!(close_repaired(&mut list, 147556, &[2], NOW + 2), vec![2]);
    assert!(blocking(&list, 147556).is_none());
    assert_eq!(list[0].status, DivergenceStatus::Ruled);
    assert_eq!(list[0].ruling, Some(Ruling::Rewrite));
    assert_eq!(rule_in(&mut list, "d1", Ruling::CaseStands, NOW).unwrap_err(), "that divergence was already ruled on");
}

#[test]
fn the_oldest_open_divergence_blocks_first() {
    let list = vec![
        divergence("d1", 7, 3, DivergenceKind::Data, NOW + 10),
        divergence("d2", 7, 1, DivergenceKind::Wording, NOW),
        divergence("d3", 7, 2, DivergenceKind::App, NOW - 10),
    ];
    assert_eq!(blocking(&list, 7).unwrap().id, "d2");
    assert_eq!(open_for(&list, 7).iter().map(|d| d.id.as_str()).collect::<Vec<_>>(), ["d3", "d2", "d1"]);
}

// ------------------------------------------------------------------- file

#[test]
fn old_ruled_divergences_are_pruned_on_write() {
    let dir = TempDir::new();
    let now = v2_lib::autorun::sessions::now_ms();
    let mut old_ruled = divergence("d1", 7, 1, DivergenceKind::Data, now - 200 * DAY);
    old_ruled.status = DivergenceStatus::Ruled;
    old_ruled.ruling = Some(Ruling::CaseStands);
    old_ruled.ruled_at = Some((now - KEEP_RULED_MS - DAY).to_string());
    let mut fresh_ruled = divergence("d2", 7, 2, DivergenceKind::Data, now - 10 * DAY);
    fresh_ruled.status = DivergenceStatus::Ruled;
    fresh_ruled.ruling = Some(Ruling::DataFixed);
    fresh_ruled.ruled_at = Some((now - DAY).to_string());
    let old_open = divergence("d3", 7, 3, DivergenceKind::Data, now - 200 * DAY);
    let p = path(dir.path(), "acme", "Web");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, serde_json::to_string(&vec![old_ruled, fresh_ruled, old_open]).unwrap()).unwrap();

    update(dir.path(), "acme", "Web", |_| Ok(())).unwrap();
    let ids: Vec<String> = load(dir.path(), "acme", "Web").unwrap().into_iter().map(|d| d.id).collect();
    assert_eq!(ids, ["d2", "d3"]);
}

#[test]
fn an_unreadable_divergences_file_blocks_nothing() {
    let dir = TempDir::new();
    let p = path(dir.path(), "acme", "Web");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, "{").unwrap();
    assert_eq!(load(dir.path(), "acme", "Web").unwrap_err(), "the divergences file could not be read");
    assert!(needs_ruling(dir.path(), "acme", "Web", 147556).is_none());
    let why = update(dir.path(), "acme", "Web", |list| {
        list.push(divergence("d1", 1, 1, DivergenceKind::Data, NOW));
        Ok(())
    })
    .unwrap_err();
    assert_eq!(why, "the divergences file could not be read");
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "{");
}

#[test]
fn a_missing_file_is_no_divergences_and_update_creates_it() {
    let dir = TempDir::new();
    assert!(load(dir.path(), "acme", "Web").unwrap().is_empty());
    assert!(needs_ruling(dir.path(), "acme", "Web", 1).is_none());
    update(dir.path(), "acme", "Web", |list| {
        data_report(list);
        Ok(())
    })
    .unwrap();
    assert!(needs_ruling(dir.path(), "acme", "Web", 147556).unwrap().starts_with("needs a ruling: step 2"));
    // Another project has its own file.
    assert!(needs_ruling(dir.path(), "acme", "Other", 147556).is_none());
}

#[test]
fn a_write_tells_the_screen_which_cases_changed() {
    use std::sync::{Arc, Mutex};
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let seen: Arc<Mutex<Vec<i32>>> = Arc::default();
    let sink = seen.clone();
    let mine = dir.path().to_path_buf();
    v2_lib::autorun::store::on_scripts_changed(move |root, ids| {
        if root == mine {
            sink.lock().unwrap().extend_from_slice(ids);
        }
    });
    update(dir.path(), "acme", "Web", |list| {
        data_report(list);
        Ok(())
    })
    .unwrap();
    assert_eq!(*seen.lock().unwrap(), vec![147556]);
}

// ------------------------------------------------------------------ route

fn body(case_says: &str, kind: &str) -> String {
    serde_json::json!({
        "case_id": 147556, "step": 2, "case_says": case_says, "app_shows": "6 levels", "kind": kind
    })
    .to_string()
}

async fn post(client: Option<&AdoClient>, body: &str) -> (u16, String) {
    route(&ctx(), client, "POST", "/autorun-divergence", body, "1.0.0").await
}

#[tokio::test]
async fn the_route_stores_a_divergence_quoting_the_case() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with_case().await;
    let (status, out) = post(Some(&client), &body("exactly 4 levels", "data")).await;
    assert_eq!(status, 200, "{out}");
    assert!(out.starts_with("Reported as d1. Case 147556 needs a ruling"), "{out}");
    let stored = load(dir.path(), "acme", "Web").unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!((stored[0].case_id, stored[0].step), (147556, 2));
    assert_eq!(stored[0].case_says, "exactly 4 levels");
    assert_eq!(stored[0].app_shows, "6 levels");
    assert_eq!(stored[0].kind, DivergenceKind::Data);
    // Again: updated, not duplicated.
    let (status, out) = post(Some(&client), &body("Level 4", "data")).await;
    assert_eq!(status, 200, "{out}");
    assert!(out.starts_with("Updated as d1."), "{out}");
    assert_eq!(load(dir.path(), "acme", "Web").unwrap().len(), 1);
    // A refusal of the rules is a 400 and leaves the file alone.
    let before = std::fs::read_to_string(path(dir.path(), "acme", "Web")).unwrap();
    let (status, out) = post(Some(&client), &body("exactly 4 levels", "app").replace("6 levels", "")).await;
    assert_eq!(status, 400, "{out}");
    assert!(out.starts_with("a divergence needs"), "{out}");
    assert_eq!(std::fs::read_to_string(path(dir.path(), "acme", "Web")).unwrap(), before);
}

#[tokio::test]
async fn the_route_refuses_words_the_case_does_not_hold() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with_case().await;
    let (status, out) = post(Some(&client), &body("exactly 6 levels", "data")).await;
    assert_eq!(status, 400, "{out}");
    assert_eq!(
        out,
        "\"case_says\" is the case's own words, quoted from step 2, another expected result or the preconditions - case 147556 does not hold \"exactly 6 levels\""
    );
    assert!(load(dir.path(), "acme", "Web").unwrap().is_empty());
}

#[tokio::test]
async fn the_route_refuses_a_step_the_case_has_not() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with_case().await;
    let b = serde_json::json!({
        "case_id": 147556, "step": 3, "case_says": "exactly 4 levels", "app_shows": "6", "kind": "data"
    })
    .to_string();
    let (status, out) = post(Some(&client), &b).await;
    assert_eq!((status, out.as_str()), (400, "case 147556 has 2 steps - \"step\" is one of them"));
}

#[tokio::test]
async fn the_route_without_a_sign_in_is_503() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (status, out) = post(None, &body("exactly 4 levels", "data")).await;
    assert_eq!(
        (status, out.as_str()),
        (503, "sign in to Test Case Manager first - a divergence is checked against its test case")
    );
}

#[tokio::test]
async fn a_bad_kind_names_the_three() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (status, out) = post(None, &body("exactly 4 levels", "bug")).await;
    assert_eq!(status, 400, "{out}");
    assert!(out.starts_with("that is not a divergence: "), "{out}");
    assert!(out.ends_with("kind is data, app or wording."), "{out}");
    assert!(out.contains("\"case_id\": 147556"), "{out}");
}

#[test]
fn the_route_is_auto_runs() {
    let (status, body) =
        module_guard_for("/autorun-divergence", &Default::default()).expect("refused outside Auto Run");
    assert_eq!((status, body.as_str()), (404, "Auto Run is switched off in Modules"));
    assert!(module_guard_for("/autorun-divergence", &v2_lib::modules::Module::ALL.into_iter().collect()).is_none());
}

// ------------------------------------- failures, saves and the blocked run

/// Case 7: three steps, nothing expected, one click each.
async fn client_with_case_7() -> (MockServer, AdoClient) {
    let server = MockServer::start().await;
    let steps: Vec<Step> =
        (1..=3).map(|i| Step { action: format!("Step {i}"), expected: String::new(), shared: None }).collect();
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/_apis/wit/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": [{
            "id": 7,
            "fields": { "System.Title": "Save a rating", "Microsoft.VSTS.TCM.Steps": build_steps_xml(&steps) }
        }] })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    (server, client)
}

/// Case 7's scripts, each named step's click on its own selector.
fn case_7_scripts(clicks: &[(i32, &str)]) -> serde_json::Value {
    let mut steps: Vec<serde_json::Value> = (1..=3)
        .map(|n| serde_json::json!({ "step_number": n, "actions": [{ "kind": "click", "selector": format!("#s{n}") }] }))
        .collect();
    for (n, selector) in clicks {
        steps[(*n - 1) as usize]["actions"][0]["selector"] = serde_json::json!(selector);
    }
    serde_json::json!([{ "case_id": 7, "title": "Save a rating", "steps": steps }])
}

fn see(root: &std::path::Path, body: &str) {
    let c = ctx();
    common::see_scripts(root, &c.org, &c.project, body);
}

fn failed_run_of_7() -> v2_lib::autorun::LocalRun {
    use v2_lib::autorun::{CaseRecord, LocalRun, StepRecord};
    use v2_lib::browser::actions::ActionOutcome;
    LocalRun {
        id: "run-1700000000000".to_string(),
        pbi_id: 42,
        started_at: "1700000000000".to_string(),
        cases: vec![CaseRecord {
            case_id: 7,
            title: "Save a rating".to_string(),
            verdict: "Failed".to_string(),
            note: String::new(),
            steps: vec![StepRecord {
                step_number: 2,
                outcomes: vec![ActionOutcome::failed("nothing on the page answers to #toast")],
                screenshot: None,
                downloads: vec![],
                tab: None,
                dialog: None,
                components: Vec::new(),
                duration_ms: None,
            }],
            proposed: String::new(),
            reason: String::new(),
            duration_ms: None,
            account: None,
            retried: None,
            notice: None,
            product_bugs: Vec::new(),
            page_errors_seen: 0,
            phases: None,
            cleanup: None,
        }],
        mode: String::new(),
        published: None,
        environment: None,
        resets: vec![],
        network_pauses: vec![],
    }
}

fn stored(id: &str, case_id: i32, step: i32, kind: DivergenceKind, status: DivergenceStatus, ruling: Option<Ruling>) -> Divergence {
    let ruled = status != DivergenceStatus::Open;
    Divergence {
        status,
        ruling,
        ruled_at: ruled.then(|| v2_lib::autorun::sessions::now_ms().to_string()),
        reported_at: v2_lib::autorun::sessions::now_ms().to_string(),
        ..divergence(id, case_id, step, kind, 0)
    }
}

#[tokio::test]
async fn the_failures_answer_lists_the_cases_divergences() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    v2_lib::autorun::store::save_run(dir.path(), &failed_run_of_7()).unwrap();

    // No divergence: no section.
    let (status, out) = route(&ctx(), None, "GET", "/autorun-failures?case_id=7", "", "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert!(!out.contains("## Divergences"), "{out}");

    update(dir.path(), "acme", "Web", |list| {
        list.push(stored("d1", 7, 2, DivergenceKind::Data, DivergenceStatus::Open, None));
        list.push(stored("d2", 7, 3, DivergenceKind::Wording, DivergenceStatus::AwaitingRepair, Some(Ruling::Rewrite)));
        list.push(stored("d3", 7, 1, DivergenceKind::Data, DivergenceStatus::Ruled, Some(Ruling::CaseStands)));
        // Another case's entry is not this run's business.
        list.push(stored("d4", 8, 1, DivergenceKind::Data, DivergenceStatus::Open, None));
        Ok(())
    })
    .unwrap();
    let day = v2_lib::autorun::quirks::day_of(&v2_lib::autorun::sessions::now_ms().to_string());
    let (status, out) = route(&ctx(), None, "GET", "/autorun-failures?case_id=7", "", "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    // The bounds of a repair end the answer; they are not part of the section.
    let section = out.split("## Divergences").nth(1).expect("a Divergences section");
    let section = section.split("## Bounds of a repair").next().unwrap();
    let lines: Vec<&str> = section.lines().filter(|l| l.starts_with("- ")).collect();
    assert_eq!(
        lines,
        vec![
            format!("- case 7 step 1 (d3): a person ruled on {day} that the case stands - leave the check as it is; a failure there is the data's or the application's (mark_autorun_suspected_defect, or tell the person).").as_str(),
            "- case 7 step 2 (data, d1): needs a ruling - the case says \"exactly 4 levels\", the app shows \"6 levels\". Leave the script as the case says; the person rules in Auto Run.",
            "- case 7 step 3 (rewrite, d2): the person is rewriting the case - repair that step's checks to the case as it now reads, declared in edits; the repair closes it.",
        ]
    );
    assert!(!section.contains("case 8"), "{section}");
    // The no-arguments answer carries it too.
    let (status, out) = route(&ctx(), None, "GET", "/autorun-failures", "", "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert!(out.contains("## Divergences"), "{out}");
}

#[tokio::test]
async fn a_repair_of_the_rewritten_step_closes_its_divergence() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with_case_7().await;
    let first = case_7_scripts(&[]).to_string();
    see(dir.path(), &first);
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &first, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    update(dir.path(), "acme", "Web", |list| {
        list.push(stored("d1", 7, 2, DivergenceKind::Wording, DivergenceStatus::AwaitingRepair, Some(Ruling::Rewrite)));
        Ok(())
    })
    .unwrap();

    let repair = serde_json::json!({
        "scripts": case_7_scripts(&[(2, "#s2-new")]),
        "edits": [{ "case_id": 7, "steps": [2], "why": "the case now reads differently at step 2" }],
    })
    .to_string();
    see(dir.path(), &repair);
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &repair, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert!(
        out.lines().any(|l| l == "case 7: the divergence at step 2 is closed - the script now follows the rewritten case"),
        "{out}"
    );
    let list = load(dir.path(), "acme", "Web").unwrap();
    assert_eq!(list[0].status, DivergenceStatus::Ruled);
    assert!(blocking(&list, 7).is_none());
    assert!(needs_ruling(dir.path(), "acme", "Web", 7).is_none());
}

#[tokio::test]
async fn a_save_says_the_case_needs_a_ruling() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with_case_7().await;
    update(dir.path(), "acme", "Web", |list| {
        list.push(stored("d1", 7, 2, DivergenceKind::Data, DivergenceStatus::Open, None));
        Ok(())
    })
    .unwrap();
    let body = case_7_scripts(&[]).to_string();
    see(dir.path(), &body);
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert!(
        out.lines().any(|l| l
            == "case 7: needs a ruling: step 2: the case says \"exactly 4 levels\" but the app shows \"6 levels\" (data) - a person rules on it in Auto Run - runs are Blocked until then"),
        "{out}"
    );
    // A dry run says it too, and writes nothing.
    let dry = serde_json::json!({ "scripts": case_7_scripts(&[(2, "#other")]), "edits": [{ "case_id": 7, "steps": [2], "why": "w" }], "dry_run": true }).to_string();
    see(dir.path(), &dry);
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &dry, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert!(out.contains("case 7: needs a ruling: step 2:") && out.contains("runs are Blocked until then"), "{out}");
}

#[tokio::test]
async fn a_save_for_a_case_with_an_app_divergence_says_nothing_of_a_block() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with_case_7().await;
    update(dir.path(), "acme", "Web", |list| {
        list.push(stored("d1", 7, 2, DivergenceKind::App, DivergenceStatus::Open, None));
        Ok(())
    })
    .unwrap();
    let body = case_7_scripts(&[]).to_string();
    see(dir.path(), &body);
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert!(!out.contains("Blocked"), "{out}");
}

#[test]
fn the_app_rules_a_divergence_and_loads_the_rest() {
    use v2_lib::commands::autorun_divergence::{load_at, rule_at};
    let now = v2_lib::autorun::sessions::now_ms();
    let dir = TempDir::new();
    let root = dir.path();
    let id = update(root, "org", "proj", |list| {
        report_in(list, 147556, 2, "exactly 4 levels", "6 levels", DivergenceKind::Data, now)
    })
    .unwrap();
    let Reported::New(id) = id else { panic!("a new one") };
    assert_eq!(load_at(root, "org", "proj", now).unwrap().len(), 1);
    assert!(needs_ruling(root, "org", "proj", 147556).is_some());

    let after = rule_at(root, "org", "proj", &id, Ruling::CaseStands, now + 1000).unwrap();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].status, DivergenceStatus::Ruled);
    assert!(needs_ruling(root, "org", "proj", 147556).is_none());

    // visible for seven days, then gone from the card's list
    assert_eq!(load_at(root, "org", "proj", now + 1000 + 6 * DAY).unwrap().len(), 1);
    assert!(load_at(root, "org", "proj", now + 1000 + 8 * DAY).unwrap().is_empty());
    // ruling twice is refused
    assert!(rule_at(root, "org", "proj", &id, Ruling::CaseStands, now + 2000).is_err());
}

// ------------------------------------- "Rewrite the case" must have ways out

fn awaiting(id: &str, step: i32) -> Divergence {
    Divergence {
        status: DivergenceStatus::AwaitingRepair,
        ruling: Some(Ruling::Rewrite),
        ruled_at: Some(NOW.to_string()),
        ..divergence(id, 147556, step, DivergenceKind::Data, NOW)
    }
}

fn rewritten_case() -> CaseText {
    let mut case = case_147556();
    case.steps[1].expected = "The dropdown lists Level 1 to Level 6".into();
    CaseText::from_case(&case)
}

#[test]
fn a_rewritten_case_closes_the_divergence_that_waited_for_it() {
    let was = CaseText::from_case(&case_147556());
    let mut list = vec![awaiting("d1", 2)];
    // No print yet (a ruling an earlier build saved): this read gives it one and closes nothing.
    assert!(note_case_text(&mut list, 147556, &was, NOW + 1).is_empty());
    assert_eq!(list[0].case_fp.as_deref(), Some(was.steps_fingerprint().as_str()));
    assert_eq!(list[0].step_action.as_deref(), Some("open the level dropdown"));
    assert_eq!(list[0].status, DivergenceStatus::AwaitingRepair);
    // The same text again: still waiting, and still blocking.
    assert!(note_case_text(&mut list, 147556, &was, NOW + 2).is_empty());
    assert!(blocking(&list, 147556).is_some());
    // Another case's text is not this one's.
    assert!(note_case_text(&mut list, 9, &rewritten_case(), NOW + 3).is_empty());
    // The steps text changed in Azure DevOps: closed, and the case runs again.
    assert_eq!(note_case_text(&mut list, 147556, &rewritten_case(), NOW + 4), vec![2]);
    assert_eq!(list[0].status, DivergenceStatus::Ruled);
    assert_eq!(list[0].ruled_at.as_deref(), Some((NOW + 4).to_string().as_str()));
    assert!(blocking(&list, 147556).is_none());
    // A title change alone is not a rewrite of the steps.
    let mut list = vec![awaiting("d1", 2)];
    note_case_text(&mut list, 147556, &was, NOW);
    let mut retitled = case_147556();
    retitled.title = "DEF levels (renamed)".into();
    assert!(note_case_text(&mut list, 147556, &CaseText::from_case(&retitled), NOW + 1).is_empty());
}

#[test]
fn a_ruling_saved_by_an_earlier_build_loads_with_no_print() {
    let old = r#"[{"id":"d1","case_id":7,"step":2,"case_says":"a","app_shows":"b","kind":"data","reported_at":"1","status":"awaiting_repair","ruling":"rewrite","ruled_at":"2"}]"#;
    let list: Vec<Divergence> = serde_json::from_str(old).unwrap();
    assert_eq!((list[0].case_fp.clone(), list[0].step_action.clone()), (None, None));
    // And a list without a print is written back without the keys.
    assert!(!serde_json::to_string(&list).unwrap().contains("case_fp"));
}

#[test]
fn a_repair_declaring_a_renumbered_step_closes_it_by_the_steps_action_text() {
    let was = CaseText::from_case(&case_147556());
    let mut list = vec![awaiting("d1", 2)];
    note_case_text(&mut list, 147556, &was, NOW);
    // The rewrite put a step before it: the ruled step is now step 3, and the repair declares 3.
    let now_reads = |step: i32| match step {
        3 => Some("open the level dropdown".to_string()),
        _ => Some("something else".to_string()),
    };
    assert!(close_repaired_with(&mut list, 147556, &[4], NOW + 1, &now_reads).is_empty());
    assert_eq!(list[0].status, DivergenceStatus::AwaitingRepair);
    assert_eq!(close_repaired_with(&mut list, 147556, &[3], NOW + 2, &now_reads), vec![2]);
    assert_eq!(list[0].status, DivergenceStatus::Ruled);
    // Declaring the very step still closes, as before.
    let mut list = vec![awaiting("d1", 2)];
    assert_eq!(close_repaired(&mut list, 147556, &[2], NOW), vec![2]);
}

#[test]
fn a_persons_editor_save_closes_what_waited_for_a_repair() {
    let mut list = vec![
        awaiting("d1", 2),
        Divergence { case_id: 8, ..awaiting("d2", 2) },
        divergence("d3", 147556, 3, DivergenceKind::Wording, NOW),
    ];
    assert_eq!(close_for_editor_save(&mut list, 147556, NOW + 1), vec![2]);
    assert_eq!(list[0].status, DivergenceStatus::Ruled);
    assert_eq!(list[1].status, DivergenceStatus::AwaitingRepair, "another case's");
    assert_eq!(list[2].status, DivergenceStatus::Open, "an open one still needs a ruling");
}

#[test]
fn saving_the_script_in_the_editor_unblocks_a_case_waiting_for_a_rewrite() {
    use v2_lib::commands::autorun::save_script_from_editor;
    let dir = TempDir::new();
    let root = dir.path();
    update(root, "acme", "Web", |list| {
        list.push(Divergence { case_id: 7, ..awaiting("d1", 2) });
        list.push(Divergence { case_id: 8, ..awaiting("d2", 2) });
        Ok(())
    })
    .unwrap();
    assert!(needs_ruling(root, "acme", "Web", 7).unwrap().starts_with("waits for a repair: "));
    let script: v2_lib::autorun::CaseScript = serde_json::from_value(serde_json::json!({ "case_id": 7, "title": "t", "steps": [
        { "step_number": 1, "actions": [{ "kind": "check_text", "value": "ok" }] }
    ] }))
    .unwrap();
    save_script_from_editor(root, "acme", "Web", script).unwrap();
    assert!(needs_ruling(root, "acme", "Web", 7).is_none(), "the person's save is the acceptance");
    assert!(needs_ruling(root, "acme", "Web", 8).is_some(), "another case still waits");
}

#[test]
fn undoing_a_rewrite_ruling_puts_it_back_to_needing_a_ruling() {
    use v2_lib::commands::autorun_divergence::undo_at;
    let mut list = vec![awaiting("d1", 2), divergence("d2", 147556, 3, DivergenceKind::Data, NOW)];
    list[0].case_fp = Some("abc".into());
    list[0].step_action = Some("open".into());
    assert_eq!(undo_in(&mut list, "d1"), Ok(147556));
    assert_eq!(
        (list[0].status, list[0].ruling, list[0].ruled_at.clone(), list[0].case_fp.clone(), list[0].step_action.clone()),
        (DivergenceStatus::Open, None, None, None, None)
    );
    assert!(blocking(&list, 147556).is_some());
    // Only a ruling that waits for a repair can be undone.
    assert!(undo_in(&mut list, "d1").is_err());
    assert!(undo_in(&mut list, "d2").is_err());
    assert!(undo_in(&mut list, "nope").is_err());

    // Through the app's own command body: the file changes and the list comes back.
    let dir = TempDir::new();
    update(dir.path(), "acme", "Web", |l| {
        l.push(awaiting("d1", 2));
        Ok(())
    })
    .unwrap();
    let after = undo_at(dir.path(), "acme", "Web", "d1", NOW).unwrap();
    assert_eq!(after[0].status, DivergenceStatus::Open);
    assert!(needs_ruling(dir.path(), "acme", "Web", 147556).unwrap().starts_with("needs a ruling: "));
}

#[tokio::test]
async fn reading_the_case_closes_a_rewrite_that_was_saved_and_gives_a_new_ruling_its_print() {
    use v2_lib::commands::autorun_divergence::refresh_rewritten;
    let dir = TempDir::new();
    let root = dir.path();
    let (_server, client) = client_with_case().await;
    let live = CaseText::from_case(&case_147556());
    // A ruling made while the case could not be read has no print: the read gives it one.
    update(root, "acme", "Web", |l| {
        l.push(awaiting("d1", 2));
        Ok(())
    })
    .unwrap();
    assert!(refresh_rewritten(&client, root, "acme", "Web", None).await.is_empty());
    let stored = load(root, "acme", "Web").unwrap();
    assert_eq!(stored[0].case_fp.as_deref(), Some(live.steps_fingerprint().as_str()));
    assert!(needs_ruling(root, "acme", "Web", 147556).is_some());
    // The case as the person left it differs from the print the ruling was made on.
    update(root, "acme", "Web", |l| {
        l[0].case_fp = Some(rewritten_case().steps_fingerprint());
        Ok(())
    })
    .unwrap();
    assert_eq!(refresh_rewritten(&client, root, "acme", "Web", Some(&[1])).await, vec![]);
    assert_eq!(refresh_rewritten(&client, root, "acme", "Web", Some(&[147556])).await, vec![(147556, 2)]);
    assert!(needs_ruling(root, "acme", "Web", 147556).is_none());
    // Nothing waits any more: no call is made at all.
    let none = MockServer::start().await;
    let unreachable = AdoClient::with_base_urls("tok".into(), none.uri(), none.uri());
    assert!(refresh_rewritten(&unreachable, root, "acme", "Web", None).await.is_empty());
    assert!(none.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn the_replay_route_lets_a_rewritten_case_run_again() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let (_server, client) = client_with_case().await;
    update(dir.path(), "acme", "Web", |l| {
        l.push(Divergence { case_fp: Some(rewritten_case().steps_fingerprint()), ..awaiting("d1", 2) });
        Ok(())
    })
    .unwrap();
    // The failures answer reads the case first, so the wait it describes is over.
    let (_, _) = route(&ctx(), Some(&client), "GET", "/autorun-failures?case_id=147556", "", "1.0.0").await;
    assert!(needs_ruling(dir.path(), "acme", "Web", 147556).is_none());
}

#[tokio::test]
async fn the_divergence_routes_ado_error_names_no_address_and_is_logged_raw() {
    let _log = crate::serial::log_tail();
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let server = MockServer::start().await;
    Mock::given(wm_method("GET"))
        .respond_with(ResponseTemplate::new(500).set_body_string("The service is busy"))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let (status, out) = post(Some(&client), &body("exactly 4 levels", "data")).await;
    assert_eq!(status, 502, "{out}");
    assert_eq!(out, "Azure DevOps error: The service is busy");
    assert!(!out.contains("Http") && !out.contains(&server.uri()), "{out}");
    assert!(
        v2_lib::applog::recent(200).iter().any(|l| l.message.contains("a divergence's test case could not be read")),
        "the raw error is in the log"
    );
}

// ------------------------------------------- the assistant rules, on the person's words

mod assistant {
    use super::*;
    use v2_lib::autorun::divergences::{
        failures_lines, rule_by_assistant_in, ruling_answer, RuledBy, SAID_EMPTY, SAID_TOO_LONG,
    };

    const SAID: &str = "a different cycle name is fine";

    fn pick(case_id: i32, id: &str) -> (i32, String) {
        (case_id, id.to_string())
    }

    fn open_three() -> Vec<Divergence> {
        vec![
            divergence("d1", 147556, 2, DivergenceKind::Data, NOW),
            divergence("d2", 147556, 3, DivergenceKind::Data, NOW + 1),
            divergence("d3", 147556, 4, DivergenceKind::Data, NOW + 2),
        ]
    }

    #[test]
    fn an_assistant_ruling_needs_the_persons_words() {
        let mut list = open_three();
        let before = list.clone();
        for said in ["", "   "] {
            let err = rule_by_assistant_in(&mut list, &[pick(147556, "d1")], Ruling::CaseStands, said, None, NOW).unwrap_err();
            assert_eq!(err, SAID_EMPTY);
        }
        let long = "x".repeat(301);
        let err = rule_by_assistant_in(&mut list, &[pick(147556, "d1")], Ruling::CaseStands, &long, None, NOW).unwrap_err();
        assert_eq!(err, SAID_TOO_LONG);
        let err = rule_by_assistant_in(&mut list, &[pick(147556, "d1")], Ruling::CaseStands, "a\nb", None, NOW).unwrap_err();
        assert_eq!(err, SAID_TOO_LONG);
        assert_eq!(list, before, "nothing was ruled");
        // No pick at all.
        let err = rule_by_assistant_in(&mut list, &[], Ruling::CaseStands, SAID, None, NOW).unwrap_err();
        assert_eq!(err, v2_lib::autorun::divergences::NO_PICK);
        // A note over the limit.
        let err = rule_by_assistant_in(&mut list, &[pick(147556, "d2")], Ruling::CaseStands, SAID, Some(&long), NOW).unwrap_err();
        assert_eq!(err, v2_lib::autorun::divergences::NOTE_TOO_LONG);
        assert_eq!(list, before);
        // 300 characters is allowed.
        let max = "y".repeat(300);
        assert!(rule_by_assistant_in(&mut list, &[pick(147556, "d1")], Ruling::CaseStands, &max, None, NOW).is_ok());
    }

    #[test]
    fn an_assistant_ruling_has_the_persons_effects() {
        let mut list = open_three();
        list.push(divergence("d4", 147556, 5, DivergenceKind::App, NOW + 3));
        assert_eq!(
            rule_by_assistant_in(&mut list, &[pick(147556, "d1")], Ruling::CaseStands, SAID, Some("per the chat"), NOW + 10),
            Ok(vec![147556])
        );
        assert_eq!(list[0].status, DivergenceStatus::Ruled);
        assert_eq!(list[0].ruling, Some(Ruling::CaseStands));
        assert_eq!(list[0].ruled_by, Some(RuledBy::Assistant));
        assert_eq!(list[0].said.as_deref(), Some(SAID));
        assert_eq!(list[0].note.as_deref(), Some("per the chat"));
        assert_eq!(list[0].ruled_at.as_deref(), Some((NOW + 10).to_string().as_str()));
        assert_eq!(blocking(&list, 147556).map(|d| d.id.as_str()), Some("d2"), "d1 no longer blocks");

        // The person's own rule_in gives the same fields, bar who ruled.
        let mut persons = open_three();
        rule_in(&mut persons, "d1", Ruling::CaseStands, NOW + 10).unwrap();
        assert_eq!(persons[0].ruled_by, None);
        let mut as_assistant = list[0].clone();
        as_assistant.ruled_by = None;
        as_assistant.said = None;
        as_assistant.note = None;
        assert_eq!(as_assistant, persons[0]);

        // Data fixed is for a data divergence.
        let err = rule_by_assistant_in(&mut list, &[pick(147556, "d4")], Ruling::DataFixed, SAID, None, NOW).unwrap_err();
        assert!(err.contains("Data fixed is for a data divergence"), "{err}");
        assert_eq!(list[3].status, DivergenceStatus::Open);
        assert!(rule_by_assistant_in(&mut list, &[pick(147556, "d2")], Ruling::DataFixed, SAID, None, NOW).is_ok());

        // Rewrite waits for the repair and still blocks.
        assert!(rule_by_assistant_in(&mut list, &[pick(147556, "d3")], Ruling::Rewrite, SAID, None, NOW).is_ok());
        assert_eq!(list[2].status, DivergenceStatus::AwaitingRepair);
        assert!(blocking(&list, 147556).is_some());
    }

    #[test]
    fn the_batch_form_rules_only_the_divergences_it_names() {
        let mut list = open_three();
        let picks = [pick(147556, "d1"), pick(147556, "d3"), pick(147556, "d1")];
        assert_eq!(rule_by_assistant_in(&mut list, &picks, Ruling::CaseStands, SAID, None, NOW), Ok(vec![147556]));
        assert_eq!(list[0].status, DivergenceStatus::Ruled);
        assert_eq!(list[1].status, DivergenceStatus::Open);
        assert_eq!(list[2].status, DivergenceStatus::Ruled);
        let answer = ruling_answer(&list, &picks, Ruling::CaseStands);
        assert_eq!(answer.matches("Ruled d1 ").count(), 1, "a duplicate counts once: {answer}");
        assert!(answer.contains("Ruled d3 (case 147556 step 4): the case is right, on the person's words."), "{answer}");
        assert!(answer.ends_with("The person can undo each of these on the case card in Auto Run."), "{answer}");
    }

    #[test]
    fn a_batch_with_one_divergence_that_cannot_take_the_ruling_rules_none() {
        let mut list = open_three();
        rule_in(&mut list, "d2", Ruling::CaseStands, NOW).unwrap();
        let before = list.clone();
        let err = rule_by_assistant_in(
            &mut list,
            &[pick(147556, "d1"), pick(147556, "d2"), pick(147556, "nope")],
            Ruling::CaseStands,
            SAID,
            None,
            NOW,
        )
        .unwrap_err();
        let lines: Vec<&str> = err.lines().collect();
        assert_eq!(lines.len(), 2, "{err}");
        assert!(lines[0].starts_with("d2: "), "{err}");
        assert!(lines[1].starts_with("nope: "), "{err}");
        assert_eq!(list, before, "d1 was left open");
        assert_eq!(list[0].status, DivergenceStatus::Open);
    }

    #[test]
    fn a_pick_names_its_own_case() {
        let mut list = open_three();
        let err = rule_by_assistant_in(&mut list, &[pick(147557, "d1")], Ruling::CaseStands, SAID, None, NOW).unwrap_err();
        assert_eq!(err, "d1: d1 is case 147556's divergence, not case 147557's");
        assert_eq!(list[0].status, DivergenceStatus::Open);
    }

    #[test]
    fn said_is_scrubbed_like_a_note() {
        let mut list = open_three();
        rule_by_assistant_in(
            &mut list,
            &[pick(147556, "d1")],
            Ruling::CaseStands,
            "fine, see https://hr.example.internal/x?token=abc",
            Some("also https://hr.example.internal/y?token=def"),
            NOW,
        )
        .unwrap();
        let (said, note) = (list[0].said.clone().unwrap(), list[0].note.clone().unwrap());
        for kept in [&said, &note] {
            assert!(!kept.contains("hr.example.internal") && !kept.contains("token="), "{kept}");
        }
        assert!(said.starts_with("fine, see"), "{said}");
    }

    #[test]
    fn undoing_an_assistant_ruling_puts_the_case_back_to_needing_a_ruling() {
        let mut list = open_three();
        rule_by_assistant_in(&mut list, &[pick(147556, "d1")], Ruling::CaseStands, SAID, Some("n"), NOW).unwrap();
        rule_by_assistant_in(&mut list, &[pick(147556, "d2")], Ruling::DataFixed, SAID, None, NOW).unwrap();
        assert_eq!(undo_in(&mut list, "d1"), Ok(147556));
        let d = &list[0];
        assert_eq!(
            (d.status, d.ruling, d.ruled_at.clone(), d.ruled_by, d.said.clone(), d.note.clone()),
            (DivergenceStatus::Open, None, None, None, None, None)
        );
        assert!(blocking(&list, 147556).is_some());
        assert_eq!(undo_in(&mut list, "d2"), Ok(147556), "a data_fixed ruling by the assistant is undone too");
        // A person's own ruling is still refused.
        let mut persons = open_three();
        rule_in(&mut persons, "d1", Ruling::CaseStands, NOW).unwrap();
        rule_in(&mut persons, "d2", Ruling::DataFixed, NOW).unwrap();
        for id in ["d1", "d2"] {
            assert_eq!(
                undo_in(&mut persons, id).unwrap_err(),
                "only a ruling that waits for a repair, or one the assistant made, can be undone"
            );
        }
    }

    fn checked_script(case_id: i32) -> v2_lib::autorun::CaseScript {
        serde_json::from_value(serde_json::json!({ "case_id": case_id, "title": "t", "checked": true, "steps": [
            { "step_number": 1, "actions": [{ "kind": "check_text", "value": "ok" }] }
        ] }))
        .unwrap()
    }

    #[test]
    fn an_undo_in_the_app_clears_the_scripts_checked_mark() {
        use v2_lib::autorun::store::{load_script, save_script};
        use v2_lib::commands::autorun_divergence::undo_at;
        let dir = TempDir::new();
        let root = dir.path();
        let now = v2_lib::autorun::sessions::now_ms();
        save_script(root, &checked_script(147556)).unwrap();
        update(root, "acme", "Web", |l| {
            let mut d = divergence("d1", 147556, 2, DivergenceKind::Data, NOW);
            rule_by_assistant_in(std::slice::from_mut(&mut d), &[pick(147556, "d1")], Ruling::CaseStands, SAID, None, now)?;
            l.push(d);
            Ok(())
        })
        .unwrap();
        assert!(load_script(root, 147556).unwrap().unwrap().checked);
        undo_at(root, "acme", "Web", "d1", now).unwrap();
        assert!(!load_script(root, 147556).unwrap().unwrap().checked);
    }

    #[test]
    fn an_assistant_ruling_in_the_store_clears_the_scripts_checked_mark_until_a_pass() {
        use v2_lib::autorun::store::{load_script, save_script};
        use v2_lib::commands::autorun_divergence::rule_by_assistant_at;
        let dir = TempDir::new();
        let root = dir.path();
        let now = v2_lib::autorun::sessions::now_ms();
        save_script(root, &checked_script(147556)).unwrap();
        update(root, "acme", "Web", |l| {
            l.extend(open_three());
            Ok(())
        })
        .unwrap();
        let answer =
            rule_by_assistant_at(root, "acme", "Web", &[pick(147556, "d1")], Ruling::DataFixed, SAID, None, now).unwrap();
        assert!(answer.contains("Replay the whole case with replay_autorun_to_step"), "{answer}");
        assert!(!load_script(root, 147556).unwrap().unwrap().checked);
        let kept = load(root, "acme", "Web").unwrap();
        assert_eq!(kept[0].ruled_by, Some(RuledBy::Assistant));
        // The words are not in the log.
        assert!(!v2_lib::applog::recent(200).iter().any(|l| l.message.contains(SAID)));
        // A refusal rules nothing and touches no script.
        save_script(root, &checked_script(147556)).unwrap();
        assert!(rule_by_assistant_at(root, "acme", "Web", &[pick(147556, "d1")], Ruling::CaseStands, SAID, None, now).is_err());
        assert!(load_script(root, 147556).unwrap().unwrap().checked);
    }

    #[test]
    fn a_case_stands_by_the_assistant_is_named_as_the_assistants() {
        let mut list = vec![divergence("d1", 147556, 2, DivergenceKind::Data, NOW)];
        rule_by_assistant_in(&mut list, &[pick(147556, "d1")], Ruling::CaseStands, SAID, None, NOW).unwrap();
        let err = report_in(&mut list, 147556, 2, "exactly 4 levels", "6 levels", DivergenceKind::Data, NOW + 1).unwrap_err();
        assert!(err.starts_with("the assistant ruled on "), "{err}");
        assert!(err.contains(&format!("on the person's words \"{SAID}\", that the case stands at step 2")), "{err}");
        let lines = failures_lines(&list, &[147556], NOW + 2);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].contains("the assistant ruled on") && lines[0].contains(SAID), "{}", lines[0]);
        // A person's keeps today's sentence.
        let mut persons = vec![divergence("d1", 147556, 2, DivergenceKind::Data, NOW)];
        rule_in(&mut persons, "d1", Ruling::CaseStands, NOW).unwrap();
        let err = report_in(&mut persons, 147556, 2, "exactly 4 levels", "6 levels", DivergenceKind::Data, NOW + 1).unwrap_err();
        assert!(err.starts_with("a person ruled on "), "{err}");
        assert!(failures_lines(&persons, &[147556], NOW + 2)[0].contains("a person ruled on"));
    }

    #[test]
    fn a_saved_file_from_before_the_assistant_ruled_still_loads() {
        let dir = TempDir::new();
        let p = path(dir.path(), "acme", "Web");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(
            &p,
            r#"[{"id":"d1","case_id":7,"step":2,"case_says":"a","app_shows":"b","kind":"data","reported_at":"1","status":"ruled","ruling":"case_stands","ruled_at":"2"}]"#,
        )
        .unwrap();
        let list = load(dir.path(), "acme", "Web").unwrap();
        assert_eq!((list[0].ruled_by, list[0].said.clone(), list[0].note.clone()), (None, None, None));
        assert!(!serde_json::to_string(&list[0]).unwrap().contains("ruled_by"));
    }
}
