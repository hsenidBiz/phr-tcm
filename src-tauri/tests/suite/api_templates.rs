//! The API template format and the checks a draft must pass - see design
//! doc "API templates" §4.

use serde_json::{json, Value};
use v2_lib::api_templates::exec::{parse_capture_path, placeholders, Seg};
use v2_lib::api_templates::{check, check_values, parse_draft, valid_id, ApiTemplate};

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
    for p in ["https://evil.test/x", "//evil.test/x", "/\\evil.test", "hr/x", "/hr/../x", "/hr/%2e%2e/x"] {
        let mut v = draft();
        v["steps"][0]["path"] = json!(p);
        let err = parse_draft(&v).unwrap_err();
        assert!(err.iter().any(|e| e.contains("Cycle setup")), "path {p:?}: {err:?}");
    }
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
