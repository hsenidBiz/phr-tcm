//! What a script may never carry, whatever an assistant asks for: page
//! script, a fixed wait and a retry (Task 18).

use serde_json::json;
use crate::common::route_without_rules as route;
use v2_lib::ai_bridge::BridgeContext;
use v2_lib::autorun::forbidden_fixes::{forbidden_in, forbidden_kind, NO_FIXED_WAIT, NO_PAGE_SCRIPT, NO_RETRY};
use v2_lib::autorun::guide::ACTION_KINDS;
use v2_lib::autorun::store::set_root;

fn ctx() -> BridgeContext {
    BridgeContext { org: "acme".into(), project: "Web".into(), ..BridgeContext::default() }
}

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let p = std::env::temp_dir().join(format!("tcm-forbidden-{nanos}-{}", N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn page_script_kinds_are_refused_with_the_reason() {
    assert_eq!(forbidden_kind("evaluate"), Some(NO_PAGE_SCRIPT));
    assert_eq!(forbidden_kind("Evaluate"), Some(NO_PAGE_SCRIPT));
    assert_eq!(forbidden_kind("run-script"), Some(NO_PAGE_SCRIPT));
}

#[test]
fn fixed_waits_are_refused_with_the_reason() {
    for k in ["sleep", "wait", "networkidle"] {
        assert_eq!(forbidden_kind(k), Some(NO_FIXED_WAIT), "{k}");
    }
    assert_eq!(forbidden_kind("wait_for"), None);
}

#[test]
fn retries_are_refused_with_the_reason() {
    assert_eq!(forbidden_kind("retry"), Some(NO_RETRY));
    let got = forbidden_in(&json!([{ "kind": "click", "selector": { "role": "button", "name": "Save" }, "retries": 3 }]));
    assert_eq!(got, Some(format!("\"retries\" is not part of any action and never will be: {NO_RETRY}")));
    let null = forbidden_in(&json!([{ "kind": "click", "selector": "#a", "retries": null }]));
    assert_eq!(null, None);
}

#[test]
fn every_real_action_kind_passes() {
    for k in ACTION_KINDS.iter().chain(["reload", "press_key", "return_to_area"].iter()) {
        assert_eq!(forbidden_kind(k), None, "{k}");
    }
    let ok = json!([{ "kind": "fill", "selector": { "role": "textbox", "name": "retry" }, "value": "sleep" }]);
    assert_eq!(forbidden_in(&ok), None);
}

#[test]
fn a_forbidden_kind_inside_a_then_is_found() {
    let steps = json!([{
        "kind": "when_visible",
        "selector": { "role": "button", "name": "Close" },
        "then": [{ "kind": "sleep", "ms": 500 }]
    }]);
    assert_eq!(
        forbidden_in(&steps),
        Some(format!("\"sleep\" is not an action and never will be: {NO_FIXED_WAIT}"))
    );
}

#[tokio::test]
async fn a_save_with_an_evaluate_step_is_refused_naming_the_case() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.0.clone());
    let body = json!([{
        "case_id": 7,
        "steps": [{ "step": 1, "actions": [{ "kind": "evaluate", "expression": "1" }] }]
    }])
    .to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert_eq!(out, format!("case 7: \"evaluate\" is not an action and never will be: {NO_PAGE_SCRIPT}"));
}

#[tokio::test]
async fn try_autorun_action_refuses_a_sleep() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.0.clone());
    let body = json!({ "action": { "kind": "sleep", "ms": 500 }, "case_id": 7 }).to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-try", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert_eq!(out, format!("\"sleep\" is not an action and never will be: {NO_FIXED_WAIT}"));
    let one = route(&ctx(), None, "POST", "/autorun-discover-action", &body, "1.0.0").await;
    assert_eq!(one.0, 400, "{}", one.1);
    assert_eq!(one.1, out);
}

#[tokio::test]
async fn the_discovery_names_the_action_number() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.0.clone());
    let body = json!({ "actions": [
        { "kind": "click", "selector": "#a" },
        { "kind": "retry", "times": 3 }
    ] })
    .to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-discover-actions", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert_eq!(out, format!("action 2: \"retry\" is not an action and never will be: {NO_RETRY}"));
}

#[tokio::test]
async fn a_component_save_with_a_page_script_action_is_refused() {
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.0.clone());
    let body = json!({
        "name": "close_banner", "description": "d", "inputs": [],
        "actions": [{ "kind": "evaluate", "expression": "1" }]
    })
    .to_string();
    let (status, out) = route(&ctx(), None, "POST", "/autorun-component-save", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert_eq!(out, format!("\"evaluate\" is not an action and never will be: {NO_PAGE_SCRIPT}"));
}

// ---- Task 19: a repair may not let the page decide whether a failure counts ----

use v2_lib::autorun::edits::Edit;
use v2_lib::autorun::forbidden_fixes::check_repair_absorbs;
use v2_lib::autorun::quirks::QuirkClass;
use v2_lib::autorun::{CaseScript, PageErrors};

fn script(steps: serde_json::Value) -> CaseScript {
    serde_json::from_value(json!({ "case_id": 7, "title": "Save", "steps": steps })).unwrap()
}

fn save_step() -> serde_json::Value {
    json!({ "step_number": 2, "actions": [{ "kind": "click", "selector": { "role": "button", "name": "Save" } }] })
}

fn guarded_step() -> serde_json::Value {
    json!({ "step_number": 2, "actions": [
        { "kind": "when_visible", "selector": { "role": "dialog", "name": "Session warning" },
          "then": [{ "kind": "click", "selector": { "role": "button", "name": "OK" } }] },
        { "kind": "click", "selector": { "role": "button", "name": "Save" } }
    ] })
}

fn edit(quirk: Option<&str>, class: Option<QuirkClass>) -> Edit {
    Edit { case_id: 7, steps: vec![2], why: "w".into(), quirk: quirk.map(str::to_string), quirk_class: class, ..Edit::default() }
}

const NO_QUIRK_GUARD: &str = "step 2: the repair adds a when_visible - a guard runs or not as the page decides, which is how a failure gets absorbed: give the case's edit a quirk saying what the page does, with quirk_class \"app quirk\" or \"environment flake\"";

#[test]
fn a_repair_that_adds_a_guard_without_a_quirk_is_refused() {
    let old = script(json!([save_step()]));
    let new = script(json!([guarded_step()]));
    assert_eq!(check_repair_absorbs(&old, &new, Some(&edit(None, None))), vec![NO_QUIRK_GUARD.to_string()]);
    assert_eq!(check_repair_absorbs(&old, &new, None), vec![NO_QUIRK_GUARD.to_string()]);
    // A blank quirk is none; a quirk with no class is not enough.
    assert_eq!(
        check_repair_absorbs(&old, &new, Some(&edit(Some("  "), Some(QuirkClass::AppQuirk)))),
        vec![NO_QUIRK_GUARD.to_string()]
    );
    assert_eq!(
        check_repair_absorbs(&old, &new, Some(&edit(Some("A session warning may cover the form"), None))),
        vec![NO_QUIRK_GUARD.to_string()]
    );
}

#[test]
fn a_guard_with_an_app_quirk_passes() {
    let old = script(json!([save_step()]));
    let new = script(json!([guarded_step()]));
    let e = edit(Some("A session warning may cover the form"), Some(QuirkClass::AppQuirk));
    assert!(check_repair_absorbs(&old, &new, Some(&e)).is_empty());
    let e = edit(Some("The warning comes from the test box"), Some(QuirkClass::EnvironmentFlake));
    assert!(check_repair_absorbs(&old, &new, Some(&e)).is_empty());
}

#[test]
fn a_guard_around_a_product_bug_is_refused() {
    let old = script(json!([save_step()]));
    let new = script(json!([guarded_step()]));
    let e = edit(Some("Save errors out"), Some(QuirkClass::ProductBug));
    assert_eq!(
        check_repair_absorbs(&old, &new, Some(&e)),
        vec!["step 2: the repair adds a when_visible around a product bug - a real bug must surface as a failure: leave the step failing and mark it with mark_autorun_suspected_defect".to_string()]
    );
}

#[test]
fn a_repeat_while_visible_counts_as_a_guard_and_an_existing_one_is_free() {
    let rep = json!({ "step_number": 2, "actions": [
        { "kind": "repeat_while_visible", "selector": { "css": ".x .delete" }, "max": 5,
          "then": [{ "kind": "click", "selector": { "css": ".delete" } }] }
    ] });
    let old = script(json!([save_step()]));
    let new = script(json!([rep]));
    let got = check_repair_absorbs(&old, &new, None);
    assert_eq!(got.len(), 1);
    assert!(got[0].starts_with("step 2: the repair adds a repeat_while_visible - "), "{got:?}");
    assert!(check_repair_absorbs(&new, &new, None).is_empty());
}

#[test]
fn a_guard_added_to_the_clean_up_is_free() {
    let cleanup = |guard: bool| {
        let mut actions = vec![json!({ "kind": "click", "selector": { "css": "#x" } })];
        if guard {
            actions.insert(
                0,
                json!({ "kind": "when_visible", "selector": { "css": "#b" }, "then": [{ "kind": "click", "selector": { "css": "#b" } }] }),
            );
        }
        json!({ "step_number": 3, "cleanup": true, "actions": actions })
    };
    let old = script(json!([save_step(), cleanup(false)]));
    let new = script(json!([save_step(), cleanup(true)]));
    assert!(check_repair_absorbs(&old, &new, None).is_empty());
}

fn with_ignore(phrases: &[&str]) -> CaseScript {
    serde_json::from_value(json!({ "case_id": 7, "title": "Save", "steps": [save_step()], "ignore_page_errors": phrases }))
        .unwrap()
}

#[test]
fn a_new_ignored_page_error_needs_a_quirk_and_never_a_product_bug() {
    let old = with_ignore(&[]);
    let new = with_ignore(&["/api/ping"]);
    assert_eq!(
        check_repair_absorbs(&old, &new, Some(&edit(None, None))),
        vec!["the repair adds \"/api/ping\" to ignore_page_errors - a page error that fails the case is the application's: give the case's edit a quirk saying what the error is, with quirk_class \"app quirk\" or \"environment flake\", or leave it failing and tell the person".to_string()]
    );
    assert!(check_repair_absorbs(&old, &new, Some(&edit(Some("ping 503s on the test box"), Some(QuirkClass::EnvironmentFlake)))).is_empty());
    assert_eq!(
        check_repair_absorbs(&old, &new, Some(&edit(Some("ping fails"), Some(QuirkClass::ProductBug)))),
        vec!["the repair ignores \"/api/ping\", a product bug - a real bug must surface as a failure: leave it failing and tell the person".to_string()]
    );
    // A phrase already ignored (any case) is not new.
    let saved = with_ignore(&["/API/Ping"]);
    assert!(check_repair_absorbs(&saved, &new, Some(&edit(None, None))).is_empty());
}

#[test]
fn loosening_page_errors_is_refused_and_tightening_is_free() {
    let with = |p: Option<PageErrors>| {
        let mut s = script(json!([save_step()]));
        s.page_errors = p;
        s
    };
    let said = "the repair loosens page_errors - what fails the case is a person's choice, in the script editor";
    for (o, n) in [(PageErrors::Fail, Some(PageErrors::Flag)), (PageErrors::Fail, None), (PageErrors::Flag, None)] {
        assert_eq!(check_repair_absorbs(&with(Some(o)), &with(n), None), vec![said.to_string()], "{o:?}->{n:?}");
    }
    assert!(check_repair_absorbs(&with(None), &with(Some(PageErrors::Fail)), None).is_empty());
    assert!(check_repair_absorbs(&with(Some(PageErrors::Flag)), &with(Some(PageErrors::Fail)), None).is_empty());
}

#[test]
fn turning_off_a_failing_dialog_is_refused() {
    let mut old = script(json!([save_step()]));
    old.fail_on_unexpected_dialog = true;
    let mut new = old.clone();
    new.fail_on_unexpected_dialog = false;
    assert_eq!(
        check_repair_absorbs(&old, &new, None),
        vec!["the repair stops an unexpected dialog failing the case - that is a person's choice, in the script editor".to_string()]
    );
    assert!(check_repair_absorbs(&new, &old, None).is_empty());
}

#[test]
fn an_unchanged_repair_adds_nothing() {
    let s = script(json!([guarded_step()]));
    assert!(check_repair_absorbs(&s, &s, None).is_empty());
}

#[tokio::test]
async fn a_repair_adding_a_guard_without_a_quirk_is_refused_through_the_save() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let dir = TempDir::new();
    let _root = crate::serial::autorun();
    set_root(dir.0.clone());
    let steps = vec![
        v2_lib::steps_xml::Step { action: "Open".into(), expected: "".into(), shared: None },
        v2_lib::steps_xml::Step { action: "Save".into(), expected: "A confirmation appears".into(), shared: None },
    ];
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/acme/_apis/wit/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [{
            "id": 7,
            "fields": { "System.Title": "Save a rating", "Microsoft.VSTS.TCM.Steps": v2_lib::steps_xml::build_steps_xml(&steps) }
        }] })))
        .mount(&server)
        .await;
    let client = v2_lib::ado::AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let step = |n: i32, actions: serde_json::Value| json!({ "step_number": n, "actions": actions });
    let check = json!({ "kind": "expect_visible", "selector": "#t" });
    let old_body = json!([{ "case_id": 7, "title": "Save a rating", "steps": [
        step(1, json!([{ "kind": "click", "selector": "#a" }])),
        step(2, json!([{ "kind": "click", "selector": "#s" }, check.clone()])),
    ] }]);
    let mut saved: CaseScript = serde_json::from_value(old_body[0].clone()).unwrap();
    saved.organization = Some("acme".into());
    saved.project = Some("Web".into());
    v2_lib::autorun::store::save_script(&dir.0, &saved).unwrap();
    let new_scripts = json!([{ "case_id": 7, "title": "Save a rating", "steps": [
        step(1, json!([{ "kind": "click", "selector": "#a" }])),
        step(2, json!([
            { "kind": "when_visible", "selector": { "role": "dialog", "name": "Session warning" },
              "then": [{ "kind": "click", "selector": { "role": "button", "name": "OK" } }] },
            { "kind": "click", "selector": "#s" }, check
        ])),
    ] }]);
    let body = json!({ "scripts": new_scripts, "edits": [{ "case_id": 7, "steps": [2], "why": "a warning covers the form" }] })
        .to_string();
    crate::common::see_scripts(&dir.0, "acme", "Web", &body);
    let (status, out) = route(&ctx(), Some(&client), "POST", "/autorun-script", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert_eq!(out, format!("case 7: {NO_QUIRK_GUARD}"));
}

// ---- the guard is judged on the class its quirk will actually have ----

use v2_lib::autorun::forbidden_fixes::check_repair_absorbs_with;
use v2_lib::autorun::quirks::{Quirk, MAX_QUIRKS};

fn note(text: &str, class: Option<QuirkClass>) -> Quirk {
    let mut q = Quirk::new(text, "assistant", "autorun", 1);
    q.class = class;
    q
}

#[test]
fn a_guard_is_judged_on_the_class_an_active_note_with_that_text_already_has() {
    let old = script(json!([save_step()]));
    let new = script(json!([guarded_step()]));
    let text = "Save errors out on a full table";
    let e = edit(Some(text), Some(QuirkClass::AppQuirk));
    // Alone, the edit's own class passes.
    assert!(check_repair_absorbs_with(&old, &new, Some(&e), &[]).is_empty());
    // The same text is already an active product bug: its class wins, whatever the edit says.
    let quirks = [note(&format!("  {}  ", text.to_uppercase()), Some(QuirkClass::ProductBug))];
    assert_eq!(
        check_repair_absorbs_with(&old, &new, Some(&e), &quirks),
        vec!["step 2: the repair adds a when_visible around a product bug - a real bug must surface as a failure: leave the step failing and mark it with mark_autorun_suspected_defect".to_string()]
    );
    // An active note of another class keeps its class too, even against a product bug edit.
    let flake = [note(text, Some(QuirkClass::EnvironmentFlake))];
    let bug_edit = edit(Some(text), Some(QuirkClass::ProductBug));
    assert!(check_repair_absorbs_with(&old, &new, Some(&bug_edit), &flake).is_empty());
    // A note with no class takes the edit's.
    let unclassed = [note(text, None)];
    assert!(check_repair_absorbs_with(&old, &new, Some(&e), &unclassed).is_empty());
}

#[test]
fn a_guard_is_refused_when_its_quirk_cannot_be_recorded() {
    let old = script(json!([save_step()]));
    let new = script(json!([guarded_step()]));
    let full: Vec<Quirk> = (0..MAX_QUIRKS).map(|i| note(&format!("a quirk number {i}"), Some(QuirkClass::AppQuirk))).collect();
    let e = edit(Some("A session warning may cover the form"), Some(QuirkClass::AppQuirk));
    let got = check_repair_absorbs_with(&old, &new, Some(&e), &full);
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].starts_with("step 2: the repair adds a when_visible on the strength of a quirk, but that quirk cannot be recorded"), "{got:?}");
    assert!(got[0].contains("retire one"), "{got:?}");
    // A quirk already on the list needs no room.
    let mut with_it = full.clone();
    with_it[0] = note("A session warning may cover the form", Some(QuirkClass::AppQuirk));
    assert!(check_repair_absorbs_with(&old, &new, Some(&e), &with_it).is_empty());
    // And so does a script with no guard.
    assert!(check_repair_absorbs_with(&old, &old, Some(&e), &full).is_empty());
    // The same for a new ignored page error.
    let (o, n) = (with_ignore(&[]), with_ignore(&["/api/ping"]));
    let got = check_repair_absorbs_with(&o, &n, Some(&edit(Some("ping 503s"), Some(QuirkClass::AppQuirk))), &full);
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].contains("cannot be recorded"), "{got:?}");
}
