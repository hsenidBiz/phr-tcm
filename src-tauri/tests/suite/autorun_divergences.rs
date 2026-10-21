//! A divergence: where a test case and the live application disagree,
//! reported by an assistant instead of fitting a check to the page, stored
//! per project, and ruled on by a person.
//!
//! The pure rules (`divergences::report_in`, `rule_in`, `blocking`,
//! `close_repaired`), the file (`load`, `update`), the case's words
//! (`case_text`) and the route (`POST /autorun-divergence`).

use v2_lib::ado::AdoClient;
use v2_lib::ai_bridge::{module_guard_for, route, BridgeContext};
use v2_lib::autorun::case_text::{fold, CaseText};
use v2_lib::autorun::divergences::{
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
