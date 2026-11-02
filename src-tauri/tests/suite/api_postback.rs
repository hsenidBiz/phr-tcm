//! WebForms postback steps: the template format and its checks. See
//! `docs/superpowers/specs/2026-11-02-webforms-postback-templates-design.md`.

use serde_json::{json, Value};
use v2_lib::api_templates::postback::{is_selector, page_key, parse_html_capture, HtmlCapture};
use v2_lib::api_templates::{check, delete_shape_ok, parse_draft, ApiTemplate, Method};

/// A template written before postbacks existed.
fn old() -> Value {
    json!({
        "id": "pms-create-draft-cycle",
        "title": "Create a draft performance cycle",
        "module": "PMS / Performance Cycle",
        "effect": "create",
        "description": "Cycle setup; leaves the cycle in Draft.",
        "sources": ["Pages/PerformanceCycle/Index.CycleSetup.cshtml.cs:95"],
        "antiforgery": { "page": "/hr/pmsv10/performancecycle?mode=create" },
        "params": [ { "name": "cycleName", "type": "string", "required": true } ],
        "steps": [
            { "name": "Cycle setup", "method": "POST",
              "path": "/hr/pmsv10/performancecycle", "query": { "handler": "SaveProgress" },
              "form": { "CycleName": "{{cycleName}}" },
              "expect": { "status": 200, "json": { "success": true } },
              "capture": { "cycleId": "$.cycleId" } }
        ],
        "outputs": ["cycleId"]
    })
}

fn parsed(v: &Value) -> ApiTemplate {
    serde_json::from_value(v.clone()).expect("test fixture should deserialize")
}

/// The format's example postback step.
fn save_step() -> Value {
    json!({
        "name": "save the evaluation",
        "postback": { "page": "/Perf/CompanyEvaluation.aspx", "button": "CmdSave" },
        "form": { "txtEvalName": "{{name}}", "dtStartDate": "{{start}}", "chkGoal$0": "on" },
        "expect": { "status": 200, "html": { "shows": ["Saved"], "hides": ["#ValidationSummary1"] } },
        "capture": { "evalId": "html:#hdnEvalId@value" }
    })
}

/// A template around one step, with params `name` and `start`.
fn with_step(step: Value) -> ApiTemplate {
    let mut v = old();
    v["params"] = json!([
        { "name": "name", "type": "string", "required": true },
        { "name": "start", "type": "date", "required": true }
    ]);
    v["steps"] = json!([step]);
    v["outputs"] = json!([]);
    parsed(&v)
}

/// The problems of a template whose only step is `step`.
fn problems(step: Value) -> Vec<String> {
    check(&with_step(step))
}

/// `save_step` with `edit` applied to the step object.
fn tweaked(edit: impl FnOnce(&mut Value)) -> Value {
    let mut s = save_step();
    edit(&mut s);
    s
}

fn has(p: &[String], sentence: &str) -> bool {
    p.iter().any(|x| x == sentence)
}

#[test]
fn a_template_saved_before_postbacks_writes_back_unchanged() {
    // Exactly what the app has always written: `description`, `lookup` and
    // `json` as null when absent, keys in struct order - and nothing new
    // (no `postback: null`, no `html: null`).
    let written = |files: bool| {
        let mut step = json!({
            "name": "Cycle setup", "method": "POST",
            "path": "/hr/pmsv10/performancecycle", "query": { "handler": "SaveProgress" },
            "json": null,
            "form": { "CycleName": "{{cycleName}}" }
        });
        if files {
            step["files"] = json!({ "Attachment": "sample.pdf" });
        }
        step["expect"] = json!({ "status": 200, "json": { "success": true } });
        step["capture"] = json!({ "cycleId": "$.cycleId" });
        let mut v = old();
        v["params"] = json!([{
            "name": "cycleName", "type": "string", "required": true, "description": null, "lookup": null
        }]);
        v["steps"] = json!([step]);
        v
    };
    for files in [false, true] {
        let v = written(files);
        let back = serde_json::to_string_pretty(&parsed(&v)).unwrap();
        assert_eq!(back, serde_json::to_string_pretty(&v).unwrap());
        assert!(!back.contains("postback") && !back.contains("\"html\""), "{back}");
    }
}

#[test]
fn the_spec_example_parses_and_checks_clean() {
    let t = with_step(save_step());
    assert!(check(&t).is_empty(), "{:?}", check(&t));
    let step = &t.steps[0];
    assert_eq!(step.method(), Method::Post);
    assert_eq!(step.address(), "/Perf/CompanyEvaluation.aspx");
    assert!(step.is_postback());
    let out = serde_json::to_value(step).unwrap();
    assert!(out.get("method").is_none() && out.get("path").is_none(), "{out}");
    assert!(parse_draft(&serde_json::to_value(&t).unwrap()).is_ok());
}

#[test]
fn a_postback_step_refuses_what_does_not_apply() {
    let sentence = "step 'save the evaluation' is a postback - leave out method, path, query and json (the page goes in postback.page)";
    for (key, value) in [
        ("method", json!("POST")),
        ("path", json!("/x")),
        ("query", json!({ "a": "b" })),
        ("json", json!({ "a": 1 })),
    ] {
        let p = problems(tweaked(|s| s[key] = value));
        assert_eq!(p.iter().filter(|x| x.as_str() == sentence).count(), 1, "{key}: {p:?}");
    }
    // Several at once are still said once.
    let p = problems(tweaked(|s| {
        s["method"] = json!("POST");
        s["path"] = json!("/x");
    }));
    assert_eq!(p.iter().filter(|x| x.as_str() == sentence).count(), 1, "{p:?}");
}

#[test]
fn a_normal_step_needs_a_method() {
    let p = problems(json!({ "name": "x", "path": "/hr/x" }));
    assert!(has(&p, "step 'x' needs a method: GET or POST"), "{p:?}");
}

#[test]
fn postback_page_and_button_rules() {
    for bad in ["//evil", "/a/../b", "/a#b"] {
        let p = problems(tweaked(|s| {
            s["name"] = json!("x");
            s["postback"]["page"] = json!(bad);
        }));
        let want = format!("step 'x' has a postback page that is not a safe relative path on this origin: '{bad}'");
        assert!(has(&p, &want), "{bad}: {p:?}");
    }

    // A query on the page is allowed; its placeholder must be known.
    let with_query = |placeholder: &str| {
        let mut v = old();
        v["params"] = json!([{ "name": "name", "type": "string", "required": true }]);
        v["steps"] = json!([
            { "name": "first", "postback": { "page": "/Perf/E.aspx", "button": "CmdSave" },
              "capture": { "evalId": "html:#hdnEvalId@value" } },
            { "name": "second",
              "postback": { "page": format!("/Perf/E.aspx?id={{{{{placeholder}}}}}"), "button": "CmdSave" } }
        ]);
        v["outputs"] = json!([]);
        check(&parsed(&v))
    };
    assert!(with_query("evalId").is_empty(), "{:?}", with_query("evalId"));
    let p = with_query("nope");
    assert!(p.iter().any(|x| x.contains("placeholder {{nope}}")), "{p:?}");

    let want = "step 'x' needs postback.button: the name of the control it clicks, written out (no placeholder)";
    for bad in ["", "{{b}}"] {
        let p = problems(tweaked(|s| {
            s["name"] = json!("x");
            s["postback"]["button"] = json!(bad);
        }));
        assert!(has(&p, want), "{bad:?}: {p:?}");
    }

    // The argument may use a placeholder, and that placeholder is counted.
    let p = problems(tweaked(|s| s["postback"]["argument"] = json!("Edit${{name}}")));
    assert!(p.is_empty(), "{p:?}");
    let p = problems(tweaked(|s| s["postback"]["argument"] = json!("Edit${{ghost}}")));
    assert!(p.iter().any(|x| x.contains("placeholder {{ghost}}")), "{p:?}");
}

#[test]
fn page_state_is_never_the_templates() {
    let p = problems(tweaked(|s| {
        s["name"] = json!("x");
        s["form"]["__EVENTTARGET"] = json!("CmdSave");
    }));
    assert!(has(&p, "step 'x' sets __EVENTTARGET, which is page state - the runner carries it"), "{p:?}");

    let p = problems(tweaked(|s| {
        s["name"] = json!("x");
        s["capture"] = json!({ "v": "html:input[name=__VIEWSTATE]@value" });
    }));
    assert!(
        has(&p, "step 'x' capture 'v' reads page state - the runner carries page state, a template never captures it"),
        "{p:?}"
    );
}

#[test]
fn html_belongs_to_postbacks() {
    let p = problems(tweaked(|s| {
        s["name"] = json!("x");
        s["expect"]["json"] = json!({ "ok": true });
    }));
    assert!(
        has(&p, "step 'x' is a postback, which answers with HTML - check it with expect.html, not expect.json"),
        "{p:?}"
    );

    let p = problems(tweaked(|s| {
        s["name"] = json!("x");
        s["capture"] = json!({ "id": "$.id" });
    }));
    assert!(
        has(&p, "step 'x' capture 'id' reads JSON, but a postback answers with HTML - use html:<selector>"),
        "{p:?}"
    );

    let only = "step 'x' uses expect.html or an html: capture, which only a postback step has";
    let p = problems(json!({ "name": "x", "method": "GET", "path": "/hr/x", "expect": { "html": { "shows": ["a"] } } }));
    assert!(has(&p, only), "{p:?}");
    let p = problems(json!({ "name": "x", "method": "GET", "path": "/hr/x", "capture": { "a": "html:#a" } }));
    assert!(has(&p, only), "{p:?}");

    let empty = "step 'x' has an empty expect.html entry";
    for html in [
        json!({ "shows": [""] }),
        json!({ "hides": ["  "] }),
        json!({ "rows": { "table": "", "has": "a" } }),
        json!({ "rows": { "table": "#t", "has": "" } }),
    ] {
        let p = problems(tweaked(|s| {
            s["name"] = json!("x");
            s["expect"]["html"] = html.clone();
        }));
        assert!(has(&p, empty), "{html}: {p:?}");
    }
}

#[test]
fn placeholders_in_html_checks_are_checked() {
    let run = |html: Value| problems(tweaked(|s| s["expect"]["html"] = html));
    assert!(run(json!({ "shows": ["{{name}}"] })).is_empty());
    assert!(run(json!({ "rows": { "table": "#grid", "has": "{{name}}" } })).is_empty());
    let p = run(json!({ "shows": ["{{ghost}}"] }));
    assert!(p.iter().any(|x| x.contains("placeholder {{ghost}}")), "{p:?}");
    let p = run(json!({ "rows": { "table": "#grid", "has": "{{ghost}}" } }));
    assert!(p.iter().any(|x| x.contains("placeholder {{ghost}}")), "{p:?}");
}

#[test]
fn html_captures_parse() {
    let cap = |s: &str, a: Option<&str>| Some(Ok(HtmlCapture { selector: s.to_string(), attr: a.map(String::from) }));
    assert_eq!(parse_html_capture("html:#a"), cap("#a", None));
    assert_eq!(parse_html_capture("html:#a@value"), cap("#a", Some("value")));
    assert_eq!(parse_html_capture("html:a[href@x]"), cap("a[href@x]", None));
    assert_eq!(parse_html_capture("html:a[href@x]@title"), cap("a[href@x]", Some("title")));
    assert_eq!(parse_html_capture("html:#a@data-id"), cap("#a", Some("data-id")));
    let none = Some(Err("capture path 'html:' names no selector".to_string()));
    assert_eq!(parse_html_capture("html:"), none);
    assert_eq!(parse_html_capture("html:@value"), none);
    assert_eq!(parse_html_capture("$.a"), None);
}

#[test]
fn selector_or_text() {
    for s in ["#x", ".y", "[name=z]", "table > tr"] {
        assert!(is_selector(s), "{s}");
    }
    for s in ["Saved", "Item 1.5"] {
        assert!(!is_selector(s), "{s}");
    }
}

#[test]
fn a_page_is_filed_by_its_path_alone() {
    assert_eq!(page_key("/Perf/E.aspx?id=3#top"), "/perf/e.aspx");
    assert_eq!(page_key("https://host.example/Perf/E.aspx/"), "/perf/e.aspx");
    assert_eq!(page_key("https://host.example"), "/");
    assert_eq!(page_key("/"), "/");
}

#[test]
fn a_delete_template_of_postbacks_still_needs_id_in_every_post() {
    let delete = |page: &str| {
        parsed(&json!({
            "id": "remove-evaluation", "title": "Delete an evaluation", "module": "PMS", "effect": "delete",
            "description": "d", "sources": ["x:1"], "antiforgery": { "page": "/hr/x" },
            "deletes_kind": "evaluation",
            "params": [ { "name": "id", "type": "string", "required": true } ],
            "steps": [ { "name": "Delete", "postback": { "page": page, "button": "CmdDelete" } } ],
            "outputs": []
        }))
    };
    assert!(delete_shape_ok(&delete("/Perf/E.aspx?id={{id}}")));
    assert!(!delete_shape_ok(&delete("/Perf/E.aspx?id=7")));
}
