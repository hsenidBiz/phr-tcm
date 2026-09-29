//! The API template format and the checks a draft must pass - see design
//! doc "API templates" §4.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use v2_lib::api_templates::exec::{
    build_request, capture, check_expect, excerpt, parse_capture_path, placeholders, scrub_tokens, scrub_value,
    substitute, substitute_str, Body, Seg,
};
use v2_lib::api_templates::store::{self, RunRecord};
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
    // Keys too: `build_request` fills placeholders in query and form keys.
    refused(&|v| v["steps"][0]["query"]["{{nope}}"] = json!("x"));
    refused(&|v| v["steps"][0]["form"]["{{nope}}"] = json!("x"));
}

/// A query string belongs in `query`: a `?` (or a `#`) in the path would
/// have `build_request` append a second `?`. Refused raw and encoded.
#[test]
fn a_path_carries_no_query_or_fragment() {
    for p in ["/hr/x?handler=Save", "/hr/x#top", "/hr/x%3Fhandler=Save", "/hr/x%23top", "/hr/x%253Fa=1"] {
        let mut v = draft();
        v["steps"][0]["path"] = json!(p);
        let err = parse_draft(&v).unwrap_err();
        assert!(
            err.iter().any(|e| e.contains("Cycle setup") && e.contains("query parameters go in query")),
            "path {p:?}: {err:?}"
        );
    }
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

/// Every form an anti-forgery token takes in a body loses its value: the
/// token the runner read anywhere, and anything named for a token in
/// either attribute order, either quote, JSON, form or header text - and
/// one the 64 KB cut left without its closing quote.
#[test]
fn scrub_takes_every_anti_forgery_token_out() {
    let cases = [
        (r#"<input name="__RequestVerificationToken" type="hidden" value="AAA1" />"#, "AAA1"),
        (r#"<input type="hidden" value="AAA2" name="__RequestVerificationToken">"#, "AAA2"),
        (r#"<INPUT TYPE='hidden' VALUE='AAA3' NAME='__RequestVerificationToken'>"#, "AAA3"),
        (r#"<input name=__RequestVerificationToken value=AAA4>"#, "AAA4"),
        (r#"<meta name="RequestVerificationToken" content="AAA5">"#, "AAA5"),
        (r#"{"ok":true,"RequestVerificationToken":"AAA6"}"#, "AAA6"),
        (r#"{"__RequestVerificationToken" : "AAA7\"x"}"#, "AAA7"),
        ("CycleName=x&__RequestVerificationToken=AAA8&b=1", "AAA8"),
        ("RequestVerificationToken: AAA9", "AAA9"),
        (r#"<p>ok</p><input name="__RequestVerificationToken" type="hidden" value="AAB0"#, "AAB0"),
        ("the page said known-tok-1 twice: known-tok-1", "known-tok-1"),
    ];
    for (text, secret) in cases {
        let out = scrub_tokens(text, Some("known-tok-1"));
        assert!(!out.contains(secret), "{secret} survived in {out}");
        assert!(out.contains("(token)"), "nothing marked where the token was: {out}");
    }

    // Everything else is left as it was.
    let plain = r#"<input name="CycleName" value="FY27"> {"success":false,"message":"bad"}"#;
    assert_eq!(scrub_tokens(plain, Some("known-tok-1")), plain);
    assert_eq!(scrub_tokens(plain, None), plain);
    assert_eq!(scrub_tokens(plain, Some("")), plain);

    // A captured value: strings scrubbed, a member named for a token replaced.
    let v = json!({ "id": 1, "echo": "known-tok-1", "RequestVerificationToken": 5, "list": ["known-tok-1"] });
    assert_eq!(
        scrub_value(&v, Some("known-tok-1")),
        json!({ "id": 1, "echo": "(token)", "RequestVerificationToken": "(token)", "list": ["(token)"] })
    );
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

// --- Task 4: the template store and its run history ---

fn run_record(ok: bool) -> RunRecord {
    RunRecord {
        at: "2026-09-28T10:14:00Z".to_string(),
        mode: "run".to_string(),
        account: "hr.admin".to_string(),
        ok,
        failed_step: if ok { None } else { Some("Evaluation rules".to_string()) },
        detail: if ok { None } else { Some("400 ...".to_string()) },
        outputs: btree(&[("cycleId", json!(274))]),
    }
}

#[test]
fn save_then_load_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let t = parsed(&draft());
    store::save(dir.path(), "Org", "Proj", &t).unwrap();
    let loaded = store::load(dir.path(), "Org", "Proj", &t.id).unwrap();
    assert_eq!(loaded, Some(t));
}

#[test]
fn list_is_grouped_by_module_then_title() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = parsed(&draft());
    a.id = "zzz-template".to_string();
    a.module = "PMS / A".to_string();
    a.title = "Z title".to_string();
    let mut b = parsed(&draft());
    b.id = "aaa-template".to_string();
    b.module = "PMS / A".to_string();
    b.title = "A title".to_string();
    let mut c = parsed(&draft());
    c.id = "mmm-template".to_string();
    c.module = "PMS / B".to_string();
    c.title = "A title".to_string();
    for t in [&a, &b, &c] {
        store::save(dir.path(), "Org", "Proj", t).unwrap();
    }
    let listed = store::list(dir.path(), "Org", "Proj").unwrap();
    let ids: Vec<&str> = listed.iter().map(|s| s.template.id.as_str()).collect();
    assert_eq!(ids, vec!["aaa-template", "zzz-template", "mmm-template"]);
}

#[test]
fn a_broken_file_is_skipped_not_fatal() {
    let dir = tempfile::tempdir().unwrap();
    let t = parsed(&draft());
    store::save(dir.path(), "Org", "Proj", &t).unwrap();
    let broken_path = store::templates_dir(dir.path(), "Org", "Proj").join("broken.json");
    std::fs::write(&broken_path, "not json").unwrap();
    let listed = store::list(dir.path(), "Org", "Proj").unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].template.id, t.id);
}

#[test]
fn run_history_keeps_the_newest_twenty() {
    let dir = tempfile::tempdir().unwrap();
    let t = parsed(&draft());
    store::save(dir.path(), "Org", "Proj", &t).unwrap();
    for i in 0..25 {
        let mut r = run_record(true);
        r.at = format!("2026-09-28T10:{i:02}:00Z");
        store::append_run(dir.path(), "Org", "Proj", &t.id, r).unwrap();
    }
    let listed = store::list(dir.path(), "Org", "Proj").unwrap();
    assert_eq!(listed[0].runs.len(), 20);
    assert_eq!(listed[0].runs[0].at, "2026-09-28T10:24:00Z");
    assert_eq!(listed[0].runs[19].at, "2026-09-28T10:05:00Z");
}

/// A history written before runs said whether they were a prove or a run
/// held runs only: it still loads, every line a run.
#[test]
fn a_history_written_without_modes_loads_as_runs() {
    let dir = tempfile::tempdir().unwrap();
    let t = parsed(&draft());
    store::save(dir.path(), "Org", "Proj", &t).unwrap();
    let old = json!([
        { "at": "2026-09-28 11:00:00", "account": "hr.admin", "ok": true, "outputs": { "cycleId": 274 } },
        { "at": "2026-09-28 10:00:00", "account": "hr.admin", "ok": false,
          "failed_step": "Evaluation rules", "detail": "400", "outputs": {} }
    ]);
    let file = store::templates_dir(dir.path(), "Org", "Proj").join(format!("{}.runs.json", t.id));
    std::fs::write(&file, old.to_string()).unwrap();
    let runs = &store::list(dir.path(), "Org", "Proj").unwrap()[0].runs;
    assert_eq!(runs.len(), 2, "{runs:?}");
    assert!(runs.iter().all(|r| r.mode == "run"), "{runs:?}");

    // And a new line added to it keeps its own mode, the old lines theirs.
    let mut proved = run_record(true);
    proved.mode = "prove".to_string();
    store::append_run(dir.path(), "Org", "Proj", &t.id, proved).unwrap();
    let runs = &store::list(dir.path(), "Org", "Proj").unwrap()[0].runs;
    let modes: Vec<&str> = runs.iter().map(|r| r.mode.as_str()).collect();
    assert_eq!(modes, ["prove", "run", "run"]);
}

#[test]
fn remove_takes_the_template_and_its_history() {
    let dir = tempfile::tempdir().unwrap();
    let t = parsed(&draft());
    store::save(dir.path(), "Org", "Proj", &t).unwrap();
    store::append_run(dir.path(), "Org", "Proj", &t.id, run_record(true)).unwrap();
    store::remove(dir.path(), "Org", "Proj", &t.id).unwrap();
    assert_eq!(store::load(dir.path(), "Org", "Proj", &t.id).unwrap(), None);
    let tdir = store::templates_dir(dir.path(), "Org", "Proj");
    assert!(!tdir.join(format!("{}.json", t.id)).exists());
    assert!(!tdir.join(format!("{}.runs.json", t.id)).exists());

    // Missing id is not an error.
    store::remove(dir.path(), "Org", "Proj", "nope-nothing-here").unwrap();
}

#[test]
fn an_invalid_id_never_reaches_the_disk() {
    let dir = tempfile::tempdir().unwrap();
    let mut t = parsed(&draft());
    t.id = "../x".to_string();
    assert!(store::save(dir.path(), "Org", "Proj", &t).is_err());
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        0,
        "save with an invalid id must not touch the disk"
    );
}

/// Cookie paths are case-sensitive. A hosted server kept its anti-forgery
/// cookie on `/hr/pmsv10` while a template called `/hr/PMSV10/...`, so every
/// save went without it and came back an empty 400. These are the rules
/// the runner uses to send such a step in the cookie's letter case instead.
mod cookie_case {
    use serde_json::json;
    use v2_lib::api_templates::cookies::{case_blind_cookies, in_cookie_case, jar_cookies, lost_by_adapting};

    fn jar() -> Vec<v2_lib::api_templates::cookies::JarCookie> {
        jar_cookies(&json!([
            { "name": ".AspNetCore.Antiforgery.X", "value": "VALUE-1", "domain": "hr.example.internal", "path": "/hr/pmsv10" },
            { "name": "sid", "value": "VALUE-2", "domain": ".example.internal", "path": "/" },
            { "name": "elsewhere", "value": "VALUE-3", "domain": "other.internal", "path": "/hr/pmsv10" },
            { "name": "partial", "value": "VALUE-4", "domain": "hr.example.internal", "path": "/hr/pm" },
            { "name": "trailing", "value": "VALUE-5", "domain": "hr.example.internal", "path": "/Reports/" }
        ]))
    }

    fn names(path: &str) -> Vec<String> {
        let jar = jar();
        case_blind_cookies(&jar, "hr.example.internal", path).iter().map(|c| c.name.clone()).collect()
    }

    #[test]
    fn a_cookie_whose_path_differs_only_in_letter_case_is_found() {
        assert_eq!(names("/hr/PMSV10/PerformanceCycle"), vec![".AspNetCore.Antiforgery.X".to_string()]);
    }

    #[test]
    fn the_same_case_finds_nothing() {
        assert!(names("/hr/pmsv10/performancecycle").is_empty(), "{:?}", names("/hr/pmsv10/performancecycle"));
    }

    #[test]
    fn only_whole_segments_on_this_host_count() {
        // "/hr/pm" is not a whole segment of "/hr/PMSV10"; "other.internal"
        // is not this host; "/" covers every path in any case.
        let found = names("/hr/PMSV10/x");
        assert!(!found.contains(&"partial".to_string()), "{found:?}");
        assert!(!found.contains(&"elsewhere".to_string()), "{found:?}");
        assert!(!found.contains(&"sid".to_string()), "{found:?}");
        // A path ending in "/" covers what follows it.
        assert_eq!(names("/reports/y"), vec!["trailing".to_string()]);
    }

    /// A cookie the browser holds WITHOUT a leading dot is host-only (RFC
    /// 6265 §5.3): it goes to that exact host, never to its subdomains.
    #[test]
    fn a_host_only_cookie_on_a_parent_host_does_not_count() {
        let jar = jar_cookies(&json!([
            { "name": "parent-host-only", "value": "v", "domain": "example.internal", "path": "/hr/pmsv10" },
            { "name": "parent-domain", "value": "v", "domain": ".example.internal", "path": "/hr/pmsv10" }
        ]));
        let found: Vec<String> =
            case_blind_cookies(&jar, "hr.example.internal", "/hr/PMSV10/x").iter().map(|c| c.name.clone()).collect();
        assert_eq!(found, vec!["parent-domain".to_string()]);
    }

    /// Adapting must not trade one cookie for another: when the path as
    /// written already carries a cookie (on a path other than "/") that the
    /// adapted path would not, the path is left alone.
    #[test]
    fn a_cookie_the_written_path_already_carries_is_not_given_up() {
        let jar = jar_cookies(&json!([
            { "name": "upper", "value": "v", "domain": "hr.example.internal", "path": "/hr/PMSV10" },
            { "name": "lower", "value": "v", "domain": "hr.example.internal", "path": "/hr/pmsv10" },
            { "name": "root", "value": "v", "domain": "hr.example.internal", "path": "/" }
        ]));
        let lost = lost_by_adapting(&jar, "hr.example.internal", "/hr/PMSV10/x", "/hr/pmsv10/x");
        assert_eq!(lost.map(|c| c.name.as_str()), Some("upper"));
        // "/" covers both, so on its own it is never a reason to hold back.
        let only_root = jar_cookies(&json!([{ "name": "root", "value": "v", "domain": "hr.example.internal", "path": "/" }]));
        assert!(lost_by_adapting(&only_root, "hr.example.internal", "/hr/PMSV10/x", "/hr/pmsv10/x").is_none());
    }

    #[test]
    fn the_adapted_path_takes_the_cookies_letter_case() {
        assert_eq!(in_cookie_case("/hr/PMSV10/PerformanceCycle", "/hr/pmsv10"), "/hr/pmsv10/PerformanceCycle");
        assert_eq!(in_cookie_case("/reports/y", "/Reports/"), "/Reports/y");
    }

    #[test]
    fn a_jar_cookie_keeps_no_value() {
        let text = format!("{:?}", jar());
        assert!(!text.contains("VALUE-"), "a cookie value was kept: {text}");
    }
}

/// The guide tells the assistant to write paths in the application's own
/// letter case, and why.
#[test]
fn the_guide_says_paths_keep_the_applications_letter_case() {
    let text = v2_lib::api_templates::guide::text(&[], None);
    assert!(text.contains("letter case"), "{text}");
    assert!(text.contains("case-sensitive"), "{text}");
    assert!(text.contains("/hr/pmsv10"), "{text}");
}

/// The guide teaches flows: the two tools, the order of work (map the
/// wizard before any template), and that a stage may be optional.
#[test]
fn the_guide_explains_flows() {
    let text = v2_lib::api_templates::guide::text(&[], None);
    let lower = text.to_lowercase();
    assert!(text.contains("save_api_flow"), "no save_api_flow");
    assert!(text.contains("get_api_flow_progress"), "no get_api_flow_progress");
    assert!(lower.contains("map the wizard first"), "no order of work");
    assert!(lower.contains("optional"), "no optional stages");
    assert!(text.contains("## Flows"), "no Flows section");

    // The Flows section, one line: the guide wraps its sentences.
    let flows = text[text.find("## Flows").unwrap()..text.find("## When a run fails").unwrap()]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    // Proving or running on a flow needs a database, and reading switched on.
    assert!(flows.contains("Proving or running a template on a flow needs a database chosen on the AI Bridge tab"), "{flows}");
    assert!(flows.contains("Company database (read)"), "no reading switch: {flows}");
    // A prove on a flow is saved only once its own stage reads as done.
    assert!(flows.contains("saved only if its own stage's check reads done afterwards"), "{flows}");
    // A number subject must be captured as a JSON number.
    assert!(flows.contains("captured as a JSON number"), "{flows}");
    // A check that always returns one row would always read as done.
    assert!(flows.contains("SELECT COUNT(*)"), "{flows}");
    assert!(flows.contains("SELECT CASE WHEN EXISTS"), "{flows}");
    assert!(flows.contains("always return one row"), "{flows}");
    // One large flow per kind of record, placing every template found for it;
    // the save answer's list of templates on no flow is to be worked to empty.
    assert!(flows.contains("One large flow per kind of record"), "{flows}");
    assert!(flows.contains("places EVERY template you have discovered"), "{flows}");
    assert!(flows.contains("not_on_a_flow"), "{flows}");
    assert!(flows.contains("a row only when the stage is done"), "{flows}");
}

/// The guide warns that an account can be signed in in one place at a
/// time, and what that looks like when it goes wrong.
#[test]
fn the_guide_says_an_account_is_signed_in_in_one_place_at_a_time() {
    let text = v2_lib::api_templates::guide::text(&[], None);
    assert!(text.contains("one place at a time"), "{text}");
    assert!(text.contains("Continue here"), "{text}");
    assert!(text.contains("empty 400"), "{text}");
    // ...and that an empty 400 is tried again, three times, before a step fails.
    assert!(text.contains("up to three more times"), "{text}");
    assert!(text.contains("each with a fresh token"), "{text}");
    assert!(text.contains("all 4 tries"), "{text}");
}

/// The guide hands the assistant this project's account KEYS and the
/// recipe's origin - so it never has to guess a host or an account - and
/// nothing else about an account: the guide is built from the keys alone,
/// here the ones loaded from a real accounts file whose entry carries a
/// username and a password.
#[test]
fn the_guide_names_the_accounts_and_origin_but_no_password() {
    use v2_lib::api_templates::guide;
    let dir = tempfile::tempdir().unwrap();
    let mut second = crate::common::account();
    second.key = "hr.supervisor".into();
    second.username = "supervisor.login".into();
    v2_lib::autorun::accounts::save_accounts(dir.path(), &[crate::common::account(), second]).unwrap();
    let keys: Vec<String> =
        v2_lib::autorun::accounts::load_accounts(dir.path()).unwrap().into_iter().map(|a| a.key).collect();

    let text = guide::text(&keys, Some("https://hr.example.internal"));
    assert!(text.contains("\"admin\""), "{text}");
    assert!(text.contains("\"hr.supervisor\""), "{text}");
    assert!(text.contains("https://hr.example.internal"), "{text}");
    assert!(!text.contains(crate::common::PASSWORD), "a password reached the guide");
    assert!(!text.contains("supervisor.login"), "a username reached the guide");
    // The format and the workflow are there too.
    for part in ["antiforgery", "{{", "$.", "db_query", "prove_api_template", "run_api_template", "replace"] {
        assert!(text.contains(part), "the guide never mentions {part}");
    }

    // Nothing set up yet: it says what the person has to do, rather than
    // naming no accounts and no host in silence.
    let bare = guide::text(&[], None);
    assert!(bare.contains("Auto Run"), "{bare}");
    assert!(!bare.contains("https://"), "{bare}");
}

// ---- the tab's data: flows in the overview, removing a flow --------------

fn a_flow() -> v2_lib::api_templates::flow::Flow {
    serde_json::from_value(json!({
        "id": "pms-performance-cycle",
        "title": "Performance cycle wizard",
        "module": "PMS / Performance Cycle",
        "subject": { "name": "cycleId", "type": "number" },
        "stages": [
            { "id": "setup", "title": "Cycle setup", "creates": true,
              "check": "SELECT 1 FROM t WHERE cycle_id = {{cycleId}}" }
        ]
    }))
    .expect("fixture should deserialize")
}

#[test]
fn the_overview_carries_flows() {
    use v2_lib::api_templates::flow_store;
    use v2_lib::commands::api_templates::overview_at;
    let dir = tempfile::tempdir().unwrap();
    store::save(dir.path(), "Org", "Proj", &parsed(&draft())).unwrap();
    flow_store::save(dir.path(), "Org", "Proj", &a_flow()).unwrap();

    let overview = overview_at(dir.path(), "Org", "Proj").unwrap();
    assert_eq!(overview.templates.len(), 1);
    assert_eq!(overview.flows, vec![a_flow()]);
    assert_eq!(overview.origin, None, "no sign-in recipe was saved");
}

#[test]
fn the_overview_still_answers_when_the_flows_cannot_be_listed() {
    use v2_lib::api_templates::flow_store;
    use v2_lib::commands::api_templates::overview_at;
    let dir = tempfile::tempdir().unwrap();
    store::save(dir.path(), "Org", "Proj", &parsed(&draft())).unwrap();
    // A file where the flows directory should be: reading it as a
    // directory fails with something other than "not found".
    let flows_dir = flow_store::flows_dir(dir.path(), "Org", "Proj");
    std::fs::create_dir_all(flows_dir.parent().unwrap()).unwrap();
    std::fs::write(&flows_dir, "not a directory").unwrap();
    assert!(flow_store::list(dir.path(), "Org", "Proj").is_err(), "the premise: listing must fail here");

    let overview = overview_at(dir.path(), "Org", "Proj").unwrap();
    assert_eq!(overview.templates.len(), 1, "the templates must survive");
    assert!(overview.flows.is_empty());
}

#[test]
fn removing_a_flow_is_refused_where_auto_run_is_not_offered() {
    use v2_lib::api_templates::flow_store;
    use v2_lib::commands::api_templates::remove_flow_at;
    let dir = tempfile::tempdir().unwrap();
    flow_store::save(dir.path(), "Org", "Proj", &a_flow()).unwrap();

    let err = remove_flow_at(false, dir.path(), "Org", "Proj", "pms-performance-cycle").unwrap_err();
    assert_eq!(err, "not available in this build");
    assert!(
        flow_store::load(dir.path(), "Org", "Proj", "pms-performance-cycle").unwrap().is_some(),
        "a refused removal must leave the flow where it is"
    );
}

#[test]
fn removing_a_flow_removes_only_that_flow() {
    use v2_lib::api_templates::flow_store;
    use v2_lib::commands::api_templates::remove_flow_at;
    let dir = tempfile::tempdir().unwrap();
    let mut other = a_flow();
    other.id = "other-flow".to_string();
    flow_store::save(dir.path(), "Org", "Proj", &a_flow()).unwrap();
    flow_store::save(dir.path(), "Org", "Proj", &other).unwrap();

    remove_flow_at(true, dir.path(), "Org", "Proj", "pms-performance-cycle").unwrap();
    assert_eq!(flow_store::load(dir.path(), "Org", "Proj", "pms-performance-cycle").unwrap(), None);
    assert!(flow_store::load(dir.path(), "Org", "Proj", "other-flow").unwrap().is_some());
    assert!(remove_flow_at(true, dir.path(), "Org", "Proj", "../escape").is_err(), "a bad id is refused");
}
