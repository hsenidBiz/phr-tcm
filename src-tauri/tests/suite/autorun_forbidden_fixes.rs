//! What a script may never carry, whatever an assistant asks for: page
//! script, a fixed wait and a retry (Task 18).

use serde_json::json;
use v2_lib::ai_bridge::{route, BridgeContext};
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
