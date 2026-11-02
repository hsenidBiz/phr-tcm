//! WebForms postback steps: the template format and its checks. See
//! `docs/superpowers/specs/2026-11-02-webforms-postback-templates-design.md`.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use v2_lib::api_templates::exec::scrub_page_state;
use v2_lib::api_templates::postback::{
    build_postback, invalid_postback, is_selector, judge, page_key, parse_html_capture, read_reply, sent_body,
    Answered, Carry, CarryMode, HtmlCapture, Refusal, Reply, Verdict, PARTIAL_UPDATE,
};
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

// ---- Task 2: the request, the carry, the reply and the judging ----

fn vars(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

fn answered(v: Value) -> Answered {
    serde_json::from_value(v).expect("an answer should deserialize")
}

/// A passing answer to `save_step`, with `edit` applied to the answer object.
fn answer(edit: impl FnOnce(&mut Value)) -> Answered {
    let mut v = json!({
        "status": 200, "finalUrl": "/Perf/CompanyEvaluation.aspx", "finalKey": "/perf/companyevaluation.aspx",
        "carried": true,
        "checks": [
            { "kind": "shows", "what": "Saved", "ok": true, "count": 1 },
            { "kind": "hides", "what": "#ValidationSummary1", "ok": true, "count": 0 }
        ],
        "captures": { "evalId": { "count": 1, "value": "42" } }
    });
    edit(&mut v);
    answered(v)
}

fn verdict_fail(a: &Answered) -> String {
    match judge(&with_step(save_step()).steps[0], a, None) {
        Verdict::Fail(s) => s,
        Verdict::Pass { detail, .. } => panic!("expected a failure, passed: {detail}"),
        Verdict::SignedOut => panic!("expected a failure, got signed out"),
    }
}

#[test]
fn a_postback_request_fills_its_placeholders() {
    let t = with_step(json!({
        "name": "open", "postback": { "page": "/Perf/E.aspx?id={{evalId}}", "button": "CmdSave", "argument": "a-{{evalId}}" },
        "form": { "txtName": "n-{{name}}" },
        "expect": { "html": { "shows": ["Hello {{name}}"], "rows": { "table": "#grid", "has": "{{name}}" } } },
        "capture": { "id": "html:#hdnEvalId@value" }
    }));
    let v = vars(&[("evalId", json!("7 8")), ("name", json!("Zed"))]);
    let r = build_postback(&t.steps[0], &v, &BTreeMap::new()).unwrap();
    assert_eq!(r.url, "/Perf/E.aspx?id=7%208");
    assert_eq!(r.key, "/perf/e.aspx");
    assert_eq!(r.button, "CmdSave");
    assert_eq!(r.argument, "a-7 8");
    assert_eq!(r.fields["txtName"], "n-Zed");
    assert_eq!(r.shows, vec!["Hello Zed"]);
    assert_eq!(r.rows.as_ref().unwrap().has, "Zed");
    assert_eq!(r.captures["id"], HtmlCapture { selector: "#hdnEvalId".into(), attr: Some("value".into()) });

    let t = with_step(json!({ "name": "open", "postback": { "page": "/Perf/{{name}}/E.aspx", "button": "b" } }));
    let err = build_postback(&t.steps[0], &vars(&[("name", json!(".."))]), &BTreeMap::new()).unwrap_err();
    assert_eq!(err, "step 'open' placeholder {{name}} in the path resolved to '..', which is not allowed");
}

#[test]
fn the_carry_says_must_only_for_a_page_this_run_carried() {
    let mut c = Carry::for_run("t");
    assert_eq!(c.mode("/perf/e.aspx", false), CarryMode::Fresh);
    c.carried_to("/perf/e.aspx");
    assert_eq!(c.mode("/perf/e.aspx", false), CarryMode::Must);
    assert_eq!(c.mode("/perf/other.aspx", false), CarryMode::Fresh);
    assert_eq!(c.mode("/perf/e.aspx", true), CarryMode::Fresh);

    let mut one = Carry::one_off();
    assert_eq!(one.run_key(), "one-off");
    assert_eq!(one.mode("/perf/e.aspx", false), CarryMode::IfThere);
    one.carried_to("/perf/e.aspx");
    assert_eq!(one.mode("/perf/e.aspx", true), CarryMode::Fresh);

    assert_ne!(Carry::for_run("t").run_key(), Carry::for_run("t").run_key());
    assert!(Carry::for_run("t").run_key().starts_with("t#"));
}

#[test]
fn replies_are_read_by_shape() {
    assert!(matches!(read_reply(&json!({ "error": "timeout" })), Some(Reply::Timeout)));
    assert!(matches!(read_reply(&json!({ "error": "boom" })), Some(Reply::Error(e)) if e == "boom"));
    for (word, why) in [
        ("lost", Refusal::Lost),
        ("not_webforms", Refusal::NotWebforms),
        ("no_button", Refusal::NoButton),
        ("many_buttons", Refusal::ManyButtons),
        ("too_large", Refusal::TooLarge),
        ("signed_out", Refusal::SignedOut),
        ("load_failed", Refusal::LoadFailed),
    ] {
        match read_reply(&json!({ "refused": word, "status": 404, "count": 3 })) {
            Some(Reply::Refused { why: w, status, count }) => {
                assert_eq!((w, status, count), (why, Some(404), Some(3)));
            }
            _ => panic!("{word} was not read as a refusal"),
        }
    }
    assert!(matches!(
        read_reply(&json!({ "refused": "lost" })),
        Some(Reply::Refused { status: None, count: None, .. })
    ));
    match read_reply(&json!({ "sent": true, "status": 200, "finalKey": "/x", "stateNames": ["__VIEWSTATE"] })) {
        Some(Reply::Answered(a)) => {
            assert_eq!(a.status, 200);
            assert_eq!(a.final_key, "/x");
            assert_eq!(a.state_names, vec!["__VIEWSTATE"]);
            assert!(!a.signed_out && a.checks.is_empty());
        }
        _ => panic!("a sent reply was not read as an answer"),
    }
    assert!(read_reply(&json!({})).is_none());
    assert!(read_reply(&json!({ "refused": "something else" })).is_none());
}

#[test]
fn judging_runs_in_order() {
    let step = &with_step(save_step()).steps[0];

    // Signed out beats everything below it.
    let a = answer(|a| {
        a["signedOut"] = json!(true);
        a["checks"][0]["ok"] = json!(false);
    });
    assert!(matches!(judge(step, &a, None), Verdict::SignedOut));

    assert_eq!(verdict_fail(&answer(|a| a["tooLarge"] = json!(true))), "the page is too large to carry");
    assert_eq!(verdict_fail(&answer(|a| a["partial"] = json!(true))), PARTIAL_UPDATE);
    assert_eq!(
        PARTIAL_UPDATE,
        "the page answered with a partial (UpdatePanel) update, which postback steps do not handle"
    );

    let asp = |edit: fn(&mut Value)| {
        answer(|a| {
            a["aspError"] = json!("Runtime Error");
            edit(a);
        })
    };
    assert_eq!(verdict_fail(&asp(|_| {})), "the application answered with an error page: Runtime Error");
    assert_eq!(
        verdict_fail(&asp(|a| {
            a["invalidPostback"] = json!(true);
            a["invalidField"] = json!("txtName");
        })),
        "the application refused a field or button value the page did not offer (txtName)"
    );
    assert_eq!(
        verdict_fail(&asp(|a| a["invalidPostback"] = json!(true))),
        "the application refused a field or button value the page did not offer"
    );
    assert_eq!(
        invalid_postback(Some("x")),
        "the application refused a field or button value the page did not offer (x)"
    );

    assert_eq!(verdict_fail(&answer(|a| a["status"] = json!(500))), "expected status 200, got 500");

    let check = |kind: &str, what: &str, count: usize| {
        verdict_fail(&answer(|a| a["checks"] = json!([{ "kind": kind, "what": what, "ok": false, "count": count }])))
    };
    assert_eq!(check("shows", "Saved", 0), "the answer does not show Saved");
    assert_eq!(check("hides", "#V", 1), "the answer shows #V");
    assert_eq!(check("table", "#grid", 2), "#grid matches 2 elements, not one");
    assert_eq!(check("rows", "#grid holds Zed", 0), "no row of #grid holds Zed");

    let cap = |c: Value| verdict_fail(&answer(|a| a["captures"] = json!({ "evalId": c })));
    assert_eq!(cap(json!({ "count": 0, "badSelector": true })), "capture evalId: #hdnEvalId is not a CSS selector");
    assert_eq!(
        cap(json!({ "count": 1, "state": true })),
        "capture evalId: #hdnEvalId is page state, which a template never captures"
    );
    assert_eq!(cap(json!({ "count": 0 })), "capture evalId: #hdnEvalId matches 0 elements, not one");
    assert_eq!(cap(json!({ "count": 2, "value": "1" })), "capture evalId: #hdnEvalId matches 2 elements, not one");

    match judge(step, &answer(|a| a["unknownFields"] = json!(["txtX"])), None) {
        Verdict::Pass { detail, captured } => {
            assert_eq!(detail, "status 200, carried on, captured evalId, not on the page: txtX");
            assert_eq!(captured["evalId"], "42");
        }
        _ => panic!("expected a pass"),
    }
    match judge(step, &answer(|a| a["carried"] = json!(false)), Some("<file a.pdf, 3 bytes>".into())) {
        Verdict::Pass { detail, .. } => {
            assert_eq!(detail, "status 200, loaded fresh, <file a.pdf, 3 bytes>, captured evalId");
        }
        _ => panic!("expected a pass"),
    }
}

#[test]
fn the_sent_body_shows_names_never_state() {
    let t = with_step(json!({
        "name": "s", "postback": { "page": "/P.aspx", "button": "b" }, "form": { "txtName": "Zed" }
    }));
    let req = build_postback(&t.steps[0], &BTreeMap::new(), &BTreeMap::new()).unwrap();
    let a = answer(|a| {
        a["stateNames"] = json!(["__VIEWSTATE"]);
        a["pageNames"] = json!(["__VIEWSTATE", "ddlType", "txtName"]);
    });
    let body: Value = serde_json::from_str(&sent_body(&req, Some(&a))).unwrap();
    assert_eq!(body, json!({ "__VIEWSTATE": "(page state)", "ddlType": "(from the page)", "txtName": "Zed" }));
    let before: Value = serde_json::from_str(&sent_body(&req, None)).unwrap();
    assert_eq!(before, json!({ "txtName": "Zed" }));
}

#[test]
fn page_state_values_are_scrubbed_from_any_html() {
    for html in [
        r#"<input type="hidden" name="__VIEWSTATE" id="__VIEWSTATE" value="abc">"#,
        "<input type='hidden' name='__VIEWSTATE' value='abc'>",
        "<input type=hidden name=__VIEWSTATE value=abc>",
        r#"<input type="hidden" id="__EVENTVALIDATION" value="abc"#,
    ] {
        let out = scrub_page_state(html);
        assert!(!out.contains("abc"), "{out}");
        assert!(out.contains("(page state)"), "{out}");
    }
    let keep = r#"<input type="hidden" name="__EVENTTARGET" value="abc"><input name="txtName" value="abc">"#;
    assert_eq!(scrub_page_state(keep), keep);
}
