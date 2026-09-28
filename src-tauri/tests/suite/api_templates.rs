//! The API template format and the checks a draft must pass - see design
//! doc "API templates" §4.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use v2_lib::api_templates::exec::{
    build_request, capture, check_expect, excerpt, parse_capture_path, placeholders, substitute,
    substitute_str, Body, Seg,
};
use v2_lib::api_templates::{check, check_values, parse_draft, valid_id, ApiTemplate, Expect, Method, Step};

/// Builds a `BTreeMap<String, Value>` from name/value pairs - shorthand for
/// the `vars` argument `substitute`, `substitute_str` and `build_request`
/// all take.
fn btree(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

/// Mirrors the design doc's §4 example exactly, minus `proven` (which only
/// the app may write).
fn draft() -> Value {
    json!({
        "id": "pms-create-draft-cycle",
        "title": "Create a draft performance cycle",
        "module": "PMS / Performance Cycle",
        "effect": "create",
        "description": "Cycle setup + evaluation rules; leaves the cycle in Draft.",
        "sources": ["Pages/PerformanceCycle/Index.CycleSetup.cshtml.cs:95"],
        "antiforgery": { "page": "/hr/pmsv10/performancecycle?mode=create" },
        "params": [
            { "name": "cycleName", "type": "string", "required": true,
              "description": "Shown in the cycle list" },
            { "name": "startDate", "type": "date", "required": true },
            { "name": "ratingMethodId", "type": "number", "required": true,
              "lookup": "SELECT ... WHERE name = @ratingMethod" }
        ],
        "steps": [
            { "name": "Cycle setup", "method": "POST",
              "path": "/hr/pmsv10/performancecycle", "query": { "handler": "SaveProgress" },
              "form": { "CycleName": "{{cycleName}}", "StartDate": "{{startDate}}" },
              "expect": { "status": 200, "json": { "success": true } },
              "capture": { "cycleId": "$.cycleId" } },
            { "name": "Evaluation rules", "method": "POST",
              "path": "/hr/pmsv10/performancecycle", "query": { "handler": "SaveEvalRulesProgress" },
              "form": { "CycleId": "{{cycleId}}", "RatingMethodId": "{{ratingMethodId}}" },
              "expect": { "status": 200, "json": { "success": true } } }
        ],
        "outputs": ["cycleId"]
    })
}

/// Parses without running `check` - for building a deliberately-invalid
/// `ApiTemplate` to hand straight to `check`/`check_values`, where
/// `parse_draft` would just bail out early with the first problem.
fn parsed(v: &Value) -> ApiTemplate {
    serde_json::from_value(v.clone()).expect("test fixture should deserialize")
}

#[test]
fn the_spec_example_parses_and_checks_clean() {
    assert!(parse_draft(&draft()).is_ok());
}

#[test]
fn an_unknown_field_is_refused_at_any_level() {
    let mut v = draft();
    v["extra"] = json!("nope");
    assert!(parse_draft(&v).is_err());

    let mut v = draft();
    v["steps"][0]["headers"] = json!({ "X-Test": "1" });
    assert!(parse_draft(&v).is_err());
}

#[test]
fn a_draft_carrying_proven_is_refused() {
    let mut v = draft();
    v["proven"] = json!({
        "at": "2026-09-28T10:14:00Z",
        "origin": "https://hrmmainphdev01.phrsandbox.dev",
        "account": "hr.admin",
        "outputs": { "cycleId": 273 }
    });
    let err = parse_draft(&v).unwrap_err();
    assert!(err.iter().any(|e| e.contains("proven is written by the app")), "{err:?}");
}

#[test]
fn ids_are_lowercase_filename_safe_and_short() {
    assert!(valid_id("pms-create_draft-cycle"));
    for bad in ["", "Pms-x", "a/b", "a..b", "a b", &"x".repeat(101)] {
        assert!(!valid_id(bad), "{bad:?} should not be a valid id");
    }
}

#[test]
fn only_relative_paths_on_the_same_origin() {
    for p in [
        "https://evil.test/x",
        "//evil.test/x",
        "/\\evil.test",
        "hr/x",
        "/hr/../x",
        "/hr/%2e%2e/x",
        // An encoded slash inside a ".." segment still decodes to a real
        // ".." segment - splitting the RAW path on '/' before decoding
        // (the earlier, wrong approach) would miss all of these.
        "/a/..%2fb",
        "/a/..%2Fb",
        "/a/%2e%2e%2fb",
        "/a/..%5cb",
        // Double-encoded: one decode pass looks like the inert `%2e%2e`;
        // only a second pass reveals "..".
        "/hr/%252e%252e/x",
    ] {
        let mut v = draft();
        v["steps"][0]["path"] = json!(p);
        let err = parse_draft(&v).unwrap_err();
        assert!(err.iter().any(|e| e.contains("Cycle setup")), "path {p:?}: {err:?}");
    }

    // An ordinary encoded character that isn't part of a traversal attempt
    // is still accepted.
    let mut v = draft();
    v["steps"][0]["path"] = json!("/hr/a%20b");
    assert!(parse_draft(&v).is_ok());
}

#[test]
fn a_dotdot_encoded_many_layers_deep_is_still_refused() {
    // level_1 = "..", encoded once. Each further layer re-encodes every
    // '%' as '%25', wrapping the previous layer's encoding once more - so
    // `it` ends up "..", percent-encoded 9 times over.
    let mut it = "%2e%2e".to_string();
    for _ in 0..8 {
        it = it.replace('%', "%25");
    }
    let mut v = draft();
    v["steps"][0]["path"] = json!(format!("/hr/{it}/x"));
    let err = parse_draft(&v).unwrap_err();
    assert!(err.iter().any(|e| e.contains("Cycle setup")), "{err:?}");

    // An ordinary encoded character is still accepted, unaffected by the
    // deeper decode loop.
    let mut v = draft();
    v["steps"][0]["path"] = json!("/hr/a%20b");
    assert!(parse_draft(&v).is_ok());
}

#[test]
fn a_step_has_at_most_one_body() {
    let mut v = draft();
    // step 0 already has a "form" body; adding "json" gives it two.
    v["steps"][0]["json"] = json!({ "x": 1 });
    let err = parse_draft(&v).unwrap_err();
    assert!(
        err.iter().any(|e| e.contains("Cycle setup") && e.contains("json") && e.contains("form")),
        "{err:?}"
    );
}

#[test]
fn a_placeholder_must_name_a_param_or_an_earlier_capture() {
    // step 0 (Cycle setup) uses {{nextId}}, but only step 1 (Evaluation
    // rules), which comes AFTER it, captures it - too late.
    let mut v = draft();
    v["steps"][0]["query"]["hint"] = json!("{{nextId}}");
    v["steps"][1]["capture"]["nextId"] = json!("$.nextId");
    let err = parse_draft(&v).unwrap_err();
    assert!(err.iter().any(|e| e.contains("Cycle setup") && e.contains("nextId")), "{err:?}");

    // {{nope}} names nothing declared or captured, wherever it shows up.
    let refused = |edit: &dyn Fn(&mut Value)| {
        let mut v = draft();
        edit(&mut v);
        let err = parse_draft(&v).unwrap_err();
        assert!(err.iter().any(|e| e.contains("nope")), "{err:?}");
    };
    refused(&|v| v["steps"][0]["path"] = json!("/hr/pmsv10/performancecycle/{{nope}}"));
    refused(&|v| v["steps"][0]["query"]["extra"] = json!("{{nope}}"));
    refused(&|v| {
        v["steps"][0].as_object_mut().unwrap().remove("form");
        v["steps"][0]["json"] = json!({ "a": "{{nope}}" });
    });
    refused(&|v| {
        v["steps"][0].as_object_mut().unwrap().remove("form");
        v["steps"][0]["json"] = json!({ "{{nope}}": 1 });
    });
    refused(&|v| v["steps"][0]["form"]["CycleName"] = json!("{{nope}}"));
}

#[test]
fn capture_paths_are_checked() {
    for good in ["$.a", "$.a[0]", "$.a[*].id"] {
        let mut v = draft();
        v["steps"][0]["capture"] = json!({ "cycleId": good });
        assert!(parse_draft(&v).is_ok(), "{good} should be a valid capture path");
    }
    for bad in ["a.b", "$..a", "$.a[x]"] {
        let mut v = draft();
        v["steps"][0]["capture"] = json!({ "cycleId": bad });
        let err = parse_draft(&v).unwrap_err();
        assert!(err.iter().any(|e| e.contains("Cycle setup")), "{bad}: {err:?}");
    }
}

#[test]
fn outputs_must_be_captured_somewhere() {
    let mut v = draft();
    v["outputs"] = json!(["nope"]);
    let err = parse_draft(&v).unwrap_err();
    assert!(err.iter().any(|e| e.contains("nope")), "{err:?}");
}

#[test]
fn all_problems_come_back_together() {
    let mut v = draft();
    v["id"] = json!("Bad Id!");
    v["steps"][0]["path"] = json!("hr/x");
    v["outputs"] = json!(["cycleId", "nope"]);
    let t = parsed(&v);
    let problems = check(&t);
    assert_eq!(problems.len(), 3, "{problems:?}");
}

#[test]
fn values_are_checked_against_the_params() {
    let mut v = draft();
    v["params"].as_array_mut().unwrap().push(json!({ "name": "isActive", "type": "boolean" }));
    v["params"].as_array_mut().unwrap().push(json!({ "name": "tags", "type": "list" }));
    let t = parsed(&v);

    let base = || {
        let mut m = serde_json::Map::new();
        m.insert("cycleName".to_string(), json!("Q1 2027"));
        m.insert("startDate".to_string(), json!("2027-01-01"));
        m.insert("ratingMethodId".to_string(), json!(5));
        m
    };

    let mut m = base();
    m.remove("startDate");
    assert!(check_values(&t, &m).iter().any(|e| e.contains("startDate")), "{:?}", check_values(&t, &m));

    let mut m = base();
    m.insert("extra".to_string(), json!("nope"));
    assert!(check_values(&t, &m).iter().any(|e| e.contains("extra")), "{:?}", check_values(&t, &m));

    let mut m = base();
    m.insert("ratingMethodId".to_string(), json!("33"));
    assert!(check_values(&t, &m).iter().any(|e| e.contains("ratingMethodId")), "{:?}", check_values(&t, &m));

    let mut m = base();
    m.insert("startDate".to_string(), json!("2026-02-30"));
    assert!(check_values(&t, &m).iter().any(|e| e.contains("startDate")), "{:?}", check_values(&t, &m));

    let mut m = base();
    m.insert("startDate".to_string(), json!("28/09/2026"));
    assert!(check_values(&t, &m).iter().any(|e| e.contains("startDate")), "{:?}", check_values(&t, &m));

    let mut m = base();
    m.insert("isActive".to_string(), json!("yes"));
    assert!(check_values(&t, &m).iter().any(|e| e.contains("isActive")), "{:?}", check_values(&t, &m));

    let mut m = base();
    m.insert("tags".to_string(), json!({}));
    assert!(check_values(&t, &m).iter().any(|e| e.contains("tags")), "{:?}", check_values(&t, &m));
}

#[test]
fn placeholder_scanning_finds_names_in_order() {
    assert_eq!(placeholders("no placeholders here"), Vec::<String>::new());
    assert_eq!(placeholders("{{a}} and {{b}}"), vec!["a".to_string(), "b".to_string()]);
    assert_eq!(placeholders("unterminated {{oops"), Vec::<String>::new());
}

#[test]
fn capture_path_parsing_produces_segments() {
    assert_eq!(parse_capture_path("$.a[0]").unwrap(), vec![Seg::Key("a".to_string()), Seg::Index(0)]);
    assert_eq!(
        parse_capture_path("$.a[*].id").unwrap(),
        vec![Seg::Key("a".to_string()), Seg::All, Seg::Key("id".to_string())]
    );
    assert!(parse_capture_path("a.b").is_err());
    assert!(parse_capture_path("$..a").is_err());
    assert!(parse_capture_path("$.a[x]").is_err());
}

// --- Task 3: requests, placeholders, captures and expectations ---

/// A minimal step with the given path, no query/body, default expect.
fn plain_step(path: &str) -> Step {
    Step {
        name: "Step".to_string(),
        method: Method::Get,
        path: path.to_string(),
        query: BTreeMap::new(),
        json: None,
        form: None,
        expect: Expect::default(),
        capture: BTreeMap::new(),
    }
}

#[test]
fn a_whole_value_placeholder_keeps_its_type() {
    let vars = btree(&[("cycleId", json!(273)), ("ids", json!([1, 2]))]);
    assert_eq!(substitute(&json!({"a":"{{cycleId}}","b":"{{ids}}"}), &vars), json!({"a":273,"b":[1,2]}));
}

#[test]
fn a_placeholder_inside_text_is_text() {
    assert_eq!(substitute_str("cycle {{cycleId}}!", &btree(&[("cycleId", json!(273))])), "cycle 273!");
}

#[test]
fn substituted_values_are_never_substituted_again() {
    // Review Focus 1
    let vars = btree(&[("name", json!("Q3 {{draft}}")), ("draft", json!("X"))]);
    assert_eq!(substitute_str("{{name}}", &vars), "Q3 {{draft}}");
}

#[test]
fn query_values_are_substituted_and_encoded() {
    // Review Focus 3
    let mut step = plain_step("/hr/pmsv10/performancecycle");
    step.query.insert("handler".to_string(), "Step".to_string());
    step.query.insert("stepKey".to_string(), "{{k}}".to_string());
    let vars = btree(&[("k", json!("a b&c"))]);
    let req = build_request(&step, &vars).unwrap();
    assert_eq!(req.url, "/hr/pmsv10/performancecycle?handler=Step&stepKey=a%20b%26c");
}

#[test]
fn capture_reads_keys_indexes_and_every_element() {
    let body = json!({"cycleId":274,"stages":[{"stageId":"s1"},{"stageId":"s2"}]});
    assert_eq!(capture(&body, &parse_capture_path("$.cycleId").unwrap()), Some(json!(274)));
    assert_eq!(capture(&body, &parse_capture_path("$.stages[1].stageId").unwrap()), Some(json!("s2")));
    assert_eq!(
        capture(&body, &parse_capture_path("$.stages[*].stageId").unwrap()),
        Some(json!(["s1", "s2"]))
    );
    assert_eq!(capture(&body, &parse_capture_path("$.nope").unwrap()), None);
}

#[test]
fn capture_all_over_an_empty_array_is_none() {
    let body = json!({"stages": []});
    assert_eq!(capture(&body, &parse_capture_path("$.stages[*].stageId").unwrap()), None);
}

#[test]
fn expect_is_a_partial_match() {
    let e = Expect { status: 200, json: Some(json!({"success": true})) };
    assert!(check_expect(&e, 200, r#"{"success":true,"cycleId":1}"#).is_ok());
}

#[test]
fn an_html_answer_fails_a_json_expectation_cleanly() {
    // Review Focus 2
    let e = Expect { status: 200, json: Some(json!({"success": true})) };
    assert_eq!(check_expect(&e, 200, "<!DOCTYPE html><html>").unwrap_err(), "the response was not JSON");
}

#[test]
fn a_body_that_is_json_but_not_the_expected_shape_fails() {
    let e = Expect { status: 200, json: Some(json!({"success": true})) };
    let err = check_expect(&e, 200, r#"{"success":false}"#).unwrap_err();
    assert_eq!(err, "expected success = true, got false");
}

#[test]
fn a_status_mismatch_is_reported_before_the_body_is_even_parsed() {
    let e = Expect { status: 200, json: Some(json!({"success": true})) };
    let err = check_expect(&e, 400, "not json at all").unwrap_err();
    assert_eq!(err, "expected status 200, got 400");
}

#[test]
fn no_json_expectation_still_returns_a_parsed_body_when_there_is_one() {
    let e = Expect { status: 200, json: None };
    assert_eq!(check_expect(&e, 200, r#"{"a":1}"#).unwrap(), Some(json!({"a":1})));
    assert_eq!(check_expect(&e, 200, "plain text").unwrap(), None);
}

#[test]
fn excerpts_are_capped_at_500() {
    assert!(excerpt(&"x".repeat(900)).chars().count() <= 500);
}

#[test]
fn excerpt_collapses_whitespace() {
    assert_eq!(excerpt("a\n\n  b\t\tc"), "a b c");
}

// --- R5: path placeholders are percent-encoded as one segment ---

#[test]
fn a_path_placeholder_value_is_percent_encoded_as_one_segment() {
    let step = plain_step("/hr/pmsv10/step/{{seg}}");
    let vars = btree(&[("seg", json!("no slash"))]);
    let req = build_request(&step, &vars).unwrap();
    assert_eq!(req.url, "/hr/pmsv10/step/no%20slash");
}

#[test]
fn a_path_placeholder_cannot_smuggle_a_slash_past_encoding() {
    // "../admin" becomes the single encoded segment "..%2Fadmin"; decoding
    // that back still reveals a ".." segment, so the post-substitution
    // safety re-check refuses the built request rather than letting the
    // encoded slash reach the origin as a real path separator.
    let step = plain_step("/hr/{{seg}}");
    let vars = btree(&[("seg", json!("../admin"))]);
    let err = build_request(&step, &vars).unwrap_err();
    assert!(err.contains("Step"), "{err}");
}

#[test]
fn a_path_placeholder_value_of_exactly_dotdot_is_refused() {
    let step = plain_step("/hr/{{seg}}/x");
    let vars = btree(&[("seg", json!(".."))]);
    let err = build_request(&step, &vars).unwrap_err();
    assert!(err.contains("Step") && err.contains(".."), "{err}");
}

#[test]
fn a_path_placeholder_value_of_exactly_dot_is_refused() {
    let step = plain_step("/hr/{{seg}}/x");
    let vars = btree(&[("seg", json!("."))]);
    let err = build_request(&step, &vars).unwrap_err();
    assert!(err.contains("Step"), "{err}");
}

#[test]
fn a_json_body_is_substituted_and_typed() {
    let mut step = plain_step("/hr/pmsv10/thing");
    step.json = Some(json!({"cycleId": "{{cycleId}}", "note": "for {{cycleId}}"}));
    let vars = btree(&[("cycleId", json!(273))]);
    let req = build_request(&step, &vars).unwrap();
    match req.body {
        Body::Json { value } => assert_eq!(value, json!({"cycleId": 273, "note": "for 273"})),
        other => panic!("expected a json body, got {other:?}"),
    }
}

#[test]
fn a_form_body_is_substituted_as_text() {
    let mut step = plain_step("/hr/pmsv10/thing");
    let mut form = BTreeMap::new();
    form.insert("CycleId".to_string(), "{{cycleId}}".to_string());
    step.form = Some(form);
    let vars = btree(&[("cycleId", json!(273))]);
    let req = build_request(&step, &vars).unwrap();
    match req.body {
        Body::Form { fields } => assert_eq!(fields.get("CycleId"), Some(&"273".to_string())),
        other => panic!("expected a form body, got {other:?}"),
    }
}

#[test]
fn a_step_with_no_body_builds_body_none() {
    let step = plain_step("/hr/pmsv10/thing");
    let req = build_request(&step, &BTreeMap::new()).unwrap();
    assert!(matches!(req.body, Body::None));
}
