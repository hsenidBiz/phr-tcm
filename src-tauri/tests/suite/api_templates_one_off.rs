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

// ---- The final fix wave ----

fn proven() -> v2_lib::api_templates::Proven {
    v2_lib::api_templates::Proven {
        at: "2026-10-22 09:00:00".into(),
        origin: "https://hr.example.internal".into(),
        account: "admin".into(),
        outputs: Default::default(),
        environment: None,
        via: None,
    }
}

fn kept(t: ApiTemplate) -> v2_lib::api_templates::store::SavedTemplate {
    v2_lib::api_templates::store::SavedTemplate { template: t, runs: Vec::new() }
}

/// I4: only a proven template is ever matched.
#[test]
fn only_a_proven_template_is_matched() {
    use v2_lib::api_templates::one_off::matching;
    let call = json!({ "name": "r", "method": "GET", "path": "/hr/x" });
    let draft = saved("read", call.clone());
    let mut worked = saved("read", call);
    worked.id = "worked".into();
    worked.proven = Some(proven());
    let s = step(json!({ "method": "GET", "path": "/hr/x" }));

    assert!(matching(&[kept(draft.clone())], &s).is_none(), "an unproven draft was matched");
    let both = [kept(draft), kept(worked)];
    assert_eq!(matching(&both, &s).map(|t| t.id.as_str()), Some("worked"));
}

/// I4: an all-digit or GUID path segment is a record id and becomes a param.
#[test]
fn a_record_id_segment_becomes_an_id_param() {
    use v2_lib::api_templates::one_off::{is_id_segment, with_id_params};
    use v2_lib::api_templates::ParamType;
    for id in ["101", "0", "6f1c2d3e-4a5b-4c6d-8e7f-90a1b2c3d4e5", "6F1C2D3E-4A5B-4C6D-8E7F-90A1B2C3D4E5"] {
        assert!(is_id_segment(id), "{id}");
    }
    for not in ["", "employees", "v10", "12a", "{{id}}", "6f1c2d3e-4a5b-4c6d-8e7f", "pmsv10"] {
        assert!(!is_id_segment(not), "{not}");
    }

    let s = step(json!({ "method": "GET", "path": "/hr/pmsv10/cycles/274/employees/6f1c2d3e-4a5b-4c6d-8e7f-90a1b2c3d4e5" }));
    let (out, params) = with_id_params(&s, &[]);
    assert_eq!(out.path, "/hr/pmsv10/cycles/{{id}}/employees/{{id2}}");
    assert_eq!(params.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["id", "id2"]);
    assert_eq!((params[0].kind, params[1].kind), (ParamType::Number, ParamType::String));
    assert!(params.iter().all(|p| p.required && p.default.is_none()));

    // A name the call's params already use is not taken again.
    let own: Vec<v2_lib::api_templates::Param> =
        serde_json::from_value(json!([{ "name": "id", "type": "string", "required": true }])).unwrap();
    let (out, params) = with_id_params(&step(json!({ "method": "GET", "path": "/x/5" })), &own);
    assert_eq!(out.path, "/x/{{id2}}");
    assert_eq!(params.len(), 2);

    // No id segment: the step comes back as it was.
    let plain = step(json!({ "method": "GET", "path": "/hr/cycles" }));
    assert_eq!(with_id_params(&plain, &[]), (plain, Vec::new()));
}

/// I4: two calls for different records are the same call once generalised.
#[test]
fn calls_for_two_records_match_one_template() {
    use v2_lib::api_templates::one_off::{matching, with_id_params};
    let (generalised, params) = with_id_params(&step(json!({ "method": "GET", "path": "/api/employees/101" })), &[]);
    let mut t = saved("read", json!({ "name": "r", "method": "GET", "path": "/api/employees/101" }));
    t.steps = vec![generalised];
    t.params = params;
    t.proven = Some(proven());
    let list = [kept(t)];
    let (next, _) = with_id_params(&step(json!({ "method": "GET", "path": "/api/employees/102" })), &[]);
    assert!(matching(&list, &next).is_some());
    let (other, _) = with_id_params(&step(json!({ "method": "GET", "path": "/api/departments/102" })), &[]);
    assert!(matching(&list, &other).is_none());
}

/// I3: a secret in the query, whatever it is called, is a secret as written.
#[test]
fn a_secret_in_the_query_is_found() {
    use v2_lib::api_templates::one_off::page_sends_a_secret;
    let by_name = step(json!({ "method": "GET", "path": "/x", "query": { "apiKey": "abc" } }));
    assert!(sends_a_secret(&by_name));
    let token = step(json!({ "method": "GET", "path": "/x", "query": { "access_token": "abc" } }));
    assert!(sends_a_secret(&token));
    let opaque = step(json!({ "method": "GET", "path": "/x", "query": { "q": "Zk3qT9vLm2Xw8pRt5BnYc7DfGh1JsKe4UoIa6MlPq0WxCz" } }));
    assert!(sends_a_secret(&opaque));
    // A placeholder, and ordinary values, are not.
    let param = step(json!({ "method": "GET", "path": "/x", "query": { "apiKey": "{{key}}" } }));
    assert!(!sends_a_secret(&param));
    let plain = step(json!({ "method": "GET", "path": "/x", "query": { "handler": "Save", "page": "2", "from": "2026-01-01", "id": "6f1c2d3e-4a5b-4c6d-8e7f-90a1b2c3d4e5", "keyword": "k" } }));
    assert!(!sends_a_secret(&plain));

    // The token page's query, by the same rules.
    assert!(page_sends_a_secret("/hr/form?token=abc"));
    assert!(page_sends_a_secret("/hr/form?x=1&apiKey=abc"));
    assert!(page_sends_a_secret("/hr/form?q=Zk3qT9vLm2Xw8pRt5BnYc7DfGh1JsKe4UoIa6MlPq0WxCz"));
    assert!(!page_sends_a_secret("/hr/form"));
    assert!(!page_sends_a_secret("/hr/form?handler=Edit&id=5"));
}

/// M5: a save whose address says delete or remove is not sent as anything else.
#[test]
fn a_save_that_looks_like_a_delete_is_refused_unless_it_is_one() {
    use v2_lib::api_templates::one_off::{parse, LOOKS_LIKE_A_DELETE};
    assert_eq!(LOOKS_LIKE_A_DELETE, "This call looks like a delete: send it with effect delete and the person's yes.");
    let post = |path: &str, handler: Option<&str>, effect: &str| {
        let mut step = json!({ "method": "POST", "path": path, "form": { "x": "1" } });
        if let Some(h) = handler {
            step["query"] = json!({ "handler": h });
        }
        json!({ "step": step, "effect": effect }).to_string()
    };
    for effect in ["edit", "read"] {
        for (path, handler) in [
            ("/hr/employees/Delete", None),
            ("/hr/Employees/REMOVE/5", None),
            ("/hr/pmsv10/PerformanceCycle", Some("Delete")),
            ("/hr/pmsv10/PerformanceCycle", Some("removeRow")),
        ] {
            assert_eq!(parse(&post(path, handler, effect)).unwrap_err(), (400, LOOKS_LIKE_A_DELETE.to_string()), "{path} {handler:?}");
        }
    }
    let create = json!({
        "step": { "method": "POST", "path": "/hr/x", "query": { "handler": "Delete" }, "capture": { "i": "$.id" } },
        "effect": "create", "created": { "kind": "k", "id": "i", "name": "n" }
    });
    assert_eq!(parse(&create.to_string()).unwrap_err(), (400, LOOKS_LIKE_A_DELETE.to_string()));
    // Sent as a delete, with the person's yes, it goes on; a handler that merely resembles a word does not count.
    let yes = json!({ "step": { "method": "POST", "path": "/hr/x", "query": { "handler": "Delete" } }, "effect": "delete", "person_said_yes": true });
    assert!(parse(&yes.to_string()).is_ok());
    assert!(parse(&post("/hr/pmsv10/PerformanceCycle", Some("Save"), "edit")).is_ok());
    // A GET cannot be sent as a delete, so its address is not judged.
    assert!(parse(&json!({ "step": { "method": "GET", "path": "/hr/deleted-items" } }).to_string()).is_ok());
}

/// The call as the change list names it, and the ids the call itself sends.
#[test]
fn the_request_line_keeps_the_handler_and_the_call_knows_the_ids_it_sends() {
    use v2_lib::api_templates::one_off::{parse, request_of, request_path, sends_id};
    let one = parse(
        &json!({
            "step": { "method": "POST", "path": "/hr/c/{{cid}}", "query": { "handler": "Save", "x": "{{cid}}" },
                      "json": { "parent": { "id": 274 } } },
            "params": [{ "name": "cid", "type": "number", "required": true }],
            "values": { "cid": 12 },
            "effect": "edit"
        })
        .to_string(),
    )
    .unwrap();
    assert_eq!(request_path(&one), "/hr/c/12?handler=Save");
    assert_eq!(request_of(&one), "POST /hr/c/12?handler=Save");
    assert!(sends_id(&one, "12"), "the id in the path");
    assert!(sends_id(&one, "274"), "the id in the body");
    assert!(!sends_id(&one, "27"), "a longer number is not the id");
    assert!(!sends_id(&one, "99"));
}
