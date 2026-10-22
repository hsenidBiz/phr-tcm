//! Matching a one-off call to a saved template, and the id, title and
//! secret check a saved call gets - the pure parts of saving what an
//! explore session sent (`api_templates::one_off`).

use serde_json::{json, Value};
use std::collections::BTreeSet;
use v2_lib::api_templates::one_off::{body_field_names, derived_id, derived_title, same_call, sends_a_secret};
use v2_lib::api_templates::{ApiTemplate, Step};

fn step(v: Value) -> Step {
    let mut v = v;
    v["name"] = json!("Request");
    serde_json::from_value(v).unwrap()
}

fn saved(effect: &str, s: Value) -> ApiTemplate {
    serde_json::from_value(json!({
        "id": "x", "title": "x", "module": "Explore", "effect": effect, "description": "x", "sources": [],
        "antiforgery": { "page": "/hr/x" }, "params": [], "steps": [ s ], "outputs": []
    }))
    .unwrap()
}

fn names(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn same_call_matches_method_path_placeholders_and_body_fields() {
    let t = saved(
        "create",
        json!({ "name": "r", "method": "POST", "path": "/hr/x/{{id}}", "query": { "handler": "Save" },
                "json": { "a": 1, "b": { "c": 2 } } }),
    );
    let s = step(json!({ "method": "POST", "path": "/HR/x/12", "query": { "handler": "save" },
                         "json": { "a": 3, "b": { "c": 4 } } }));
    assert!(same_call(&t, &s));
}

#[test]
fn a_different_handler_method_or_field_is_another_call() {
    let t = saved(
        "create",
        json!({ "name": "r", "method": "POST", "path": "/hr/x/{{id}}", "query": { "handler": "Save" },
                "json": { "a": 1, "b": { "c": 2 } } }),
    );
    let base = json!({ "method": "POST", "path": "/hr/x/12", "query": { "handler": "Save" },
                       "json": { "a": 3, "b": { "c": 4 } } });
    assert!(same_call(&t, &step(base.clone())));

    let mut publish = base.clone();
    publish["query"]["handler"] = json!("Publish");
    assert!(!same_call(&t, &step(publish)));

    let mut get = base.clone();
    get["method"] = json!("GET");
    assert!(!same_call(&t, &step(get)));

    let mut extra = base.clone();
    extra["json"]["d"] = json!(1);
    assert!(!same_call(&t, &step(extra)));

    let mut form = base.clone();
    form.as_object_mut().unwrap().remove("json");
    form["form"] = json!({ "a": "3", "b": "4" });
    assert!(!same_call(&t, &step(form)));

    let mut other_path = base.clone();
    other_path["path"] = json!("/hr/y/12");
    assert!(!same_call(&t, &step(other_path)));

    let mut other_query = base;
    other_query["query"]["page"] = json!("2");
    assert!(!same_call(&t, &step(other_query)));

    // A delete template is never "the same call".
    let del = saved("delete", json!({ "name": "r", "method": "POST", "path": "/hr/x/{{id}}", "json": { "a": 1 } }));
    assert!(!same_call(&del, &step(json!({ "method": "POST", "path": "/hr/x/1", "json": { "a": 1 } }))));
}

#[test]
fn body_field_names_go_two_levels_deep() {
    let s = step(json!({ "method": "POST", "path": "/x", "json": { "a": { "b": { "c": 1 } }, "l": [ { "x": 1 } ] } }));
    assert_eq!(body_field_names(&s), names(&["a", "a.b", "l", "l[].x"]));

    let form = step(json!({ "method": "POST", "path": "/x", "form": { "n": "1", "m": "2" }, "files": { "doc": "a.pdf" } }));
    assert_eq!(body_field_names(&form), names(&["n", "m", "doc"]));

    assert!(body_field_names(&step(json!({ "method": "GET", "path": "/x" }))).is_empty());
}

#[test]
fn derived_id_and_title() {
    let s = step(json!({ "method": "POST", "path": "/hr/pmsv10/PerformanceCycle", "query": { "handler": "Save" } }));
    let free = |_: &str| false;
    assert_eq!(derived_id(&s, &free), "post-hr-pmsv10-performancecycle-save");
    assert_eq!(derived_title(&s), "POST /hr/pmsv10/PerformanceCycle (Save)");

    let taken_once = |id: &str| id == "post-hr-pmsv10-performancecycle-save";
    assert_eq!(derived_id(&s, &taken_once), "post-hr-pmsv10-performancecycle-save-2");
    let taken_twice = |id: &str| id.ends_with("-save") || id.ends_with("-save-2");
    assert_eq!(derived_id(&s, &taken_twice), "post-hr-pmsv10-performancecycle-save-3");

    let get = step(json!({ "method": "GET", "path": "/a/{{id}}" }));
    assert_eq!(derived_id(&get, &free), "get-a-id");
    assert_eq!(derived_title(&get), "GET /a/{{id}}");

    let long = step(json!({ "method": "GET", "path": format!("/{}", "abcdefghij/".repeat(20)) }));
    assert!(derived_id(&long, &free).len() <= 90);
}

#[test]
fn a_secret_written_out_is_found_and_a_placeholder_is_not() {
    let json_secret = step(json!({ "method": "POST", "path": "/x", "json": { "Password": "x" } }));
    assert!(sends_a_secret(&json_secret));
    let nested = step(json!({ "method": "POST", "path": "/x", "json": { "user": { "apiKey": "abc" } } }));
    assert!(sends_a_secret(&nested));
    let as_param = step(json!({ "method": "POST", "path": "/x", "json": { "Password": "{{pw}}" } }));
    assert!(!sends_a_secret(&as_param));
    let form_secret = step(json!({ "method": "POST", "path": "/x", "form": { "token": "y" } }));
    assert!(sends_a_secret(&form_secret));
    let form_param = step(json!({ "method": "POST", "path": "/x", "form": { "token": "{{t}}", "name": "n" } }));
    assert!(!sends_a_secret(&form_param));
    let plain = step(json!({ "method": "POST", "path": "/x", "json": { "name": "n", "keyword": "k" } }));
    assert!(!sends_a_secret(&plain));
}
