//! The runner running WebForms postback steps: what it hands the page, what
//! it keeps between steps, and what it will and will not say about the
//! answer. The page is `api_templates_runner`'s `App`, which answers
//! `POSTBACK_FN` from a script; the in-page function itself is proven
//! against a real browser in `api_postback_live`.

use crate::api_templates_runner::{answer, page_reply, prove, request, rig, Rig, PAGE, QUICK_PAUSES, TOKEN};
use crate::common::{activity_records, quick};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::time::Duration;
use v2_lib::api_templates::postback::{MAY_HAVE_SAVED, PAGE_LOST};
use v2_lib::api_templates::runner::{
    run_in_page, run_template_within, Mode, RunReport, TokenPage, RUN_LIMIT, SENT_ELSEWHERE,
};
use v2_lib::api_templates::ApiTemplate;

pub(crate) fn step(name: &str, page: &str, extra: Value) -> Value {
    let mut s = json!({ "name": name, "postback": { "page": page, "button": "btnSave" } });
    for (k, v) in extra.as_object().expect("an object") {
        s[k] = v.clone();
    }
    s
}

pub(crate) fn template(steps: Vec<Value>) -> ApiTemplate {
    serde_json::from_value(json!({
        "id": "pb-eval",
        "title": "Create an evaluation",
        "module": "PMS / Evaluation",
        "effect": "create",
        "description": "A WebForms evaluation.",
        "sources": [],
        "antiforgery": { "page": PAGE },
        "params": [],
        "steps": steps,
        "outputs": []
    }))
    .unwrap()
}

async fn run(r: &mut Rig, t: ApiTemplate) -> RunReport {
    let req = request(t, prove());
    run_template_within(&mut r.browsers, r.root.path(), &req, &quick(), RUN_LIMIT, &QUICK_PAUSES).await
}

fn replies(r: &Rig, list: Vec<Value>) {
    r.script.lock().unwrap().postbacks = VecDeque::from(list);
}

fn posted(r: &Rig) -> Vec<Vec<Value>> {
    r.script.lock().unwrap().posted.clone()
}

fn last_detail(report: &RunReport) -> String {
    report.steps.last().expect("a step").detail.clone()
}

fn one_step() -> ApiTemplate {
    template(vec![step("Save", "/Perf/List.aspx", json!({}))])
}

#[tokio::test]
async fn a_postback_template_runs_with_no_token_on_its_page() {
    let _act = crate::serial::activity_log();
    let mut r = rig(vec![], None);
    r.script.lock().unwrap().token = None;
    replies(&r, vec![page_reply(200, json!({}))]);
    let report = run(&mut r, one_step()).await;
    assert!(report.ok, "{report:?}");
    assert_eq!(r.sign_ins(), 1, "a second sign-in was made for a token that was not needed");
    assert!(r.fetched().is_empty());
    assert_eq!(report.steps[0].handler.as_deref(), Some("btnSave"));
    assert_eq!(report.steps[0].detail, "status 200, carried on");
}

#[tokio::test]
async fn postbacks_to_one_page_carry_and_another_loads_fresh() {
    let _act = crate::serial::activity_log();
    let mut r = rig(vec![], None);
    replies(
        &r,
        vec![
            page_reply(200, json!({ "carried": false })),
            page_reply(200, json!({})),
            page_reply(200, json!({ "carried": false, "finalKey": "/perf/eval.aspx" })),
            page_reply(200, json!({ "carried": false })),
        ],
    );
    let t = template(vec![
        step("A", "/Perf/List.aspx", json!({})),
        step("A again", "/Perf/List.aspx?x=1", json!({})),
        step("B", "/Perf/Eval.aspx", json!({})),
        step(
            "A forced",
            "/Perf/List.aspx",
            json!({ "postback": { "page": "/Perf/List.aspx", "button": "btnSave", "fresh": true } }),
        ),
    ]);
    let report = run(&mut r, t).await;
    assert!(report.ok, "{report:?}");
    let sent = posted(&r);
    let modes: Vec<_> = sent.iter().map(|a| a[3].clone()).collect();
    assert_eq!(modes, vec![json!("fresh"), json!("must"), json!("fresh"), json!("fresh")]);
    assert!(sent.iter().all(|a| a[2] == sent[0][2]), "one run key for the whole run: {sent:?}");
    assert_eq!(sent[0].len(), 6);
    assert_eq!(sent[0][1], json!(30_000));
}

#[tokio::test]
async fn a_redirect_carries_under_the_final_path() {
    let _act = crate::serial::activity_log();
    let mut r = rig(vec![], None);
    replies(
        &r,
        vec![
            page_reply(200, json!({ "carried": false, "redirected": true, "finalKey": "/perf/eval.aspx" })),
            page_reply(200, json!({})),
        ],
    );
    let t = template(vec![
        step("Save", "/Perf/List.aspx", json!({})),
        step("Then", "/Perf/Eval.aspx?id=7", json!({})),
    ]);
    let report = run(&mut r, t).await;
    assert!(report.ok, "{report:?}");
    let sent = posted(&r);
    assert_eq!(sent[0][3], json!("fresh"));
    assert_eq!(sent[1][3], json!("must"));
}

#[tokio::test]
async fn each_run_has_its_own_carry_key() {
    let _act = crate::serial::activity_log();
    let mut keys = vec![];
    for _ in 0..2 {
        let mut r = rig(vec![], None);
        replies(&r, vec![page_reply(200, json!({}))]);
        let report = run(&mut r, one_step()).await;
        assert!(report.ok, "{report:?}");
        keys.push(posted(&r)[0][2].clone());
    }
    assert_ne!(keys[0], keys[1]);
}

#[tokio::test]
async fn a_lost_page_fails_the_step() {
    let _act = crate::serial::activity_log();
    let mut r = rig(vec![], None);
    replies(&r, vec![page_reply(200, json!({})), json!({ "refused": "lost" })]);
    let t = template(vec![step("A", "/Perf/List.aspx", json!({})), step("B", "/Perf/List.aspx", json!({}))]);
    let report = run(&mut r, t).await;
    assert!(!report.ok);
    assert_eq!(report.failed.as_deref(), Some("B"));
    assert_eq!(last_detail(&report), PAGE_LOST);
}

#[tokio::test]
async fn a_postback_is_never_sent_twice() {
    let _act = crate::serial::activity_log();
    // An empty 400, with every retry pause available.
    let mut r = rig(vec![], None);
    replies(&r, vec![page_reply(400, json!({ "empty": true, "excerpt": "" })), page_reply(200, json!({}))]);
    let report = run(&mut r, one_step()).await;
    assert!(!report.ok);
    assert_eq!(posted(&r).len(), 1, "the postback was sent again");
    assert!(last_detail(&report).ends_with(MAY_HAVE_SAVED), "{}", last_detail(&report));
    assert!(last_detail(&report).contains("an empty 400"), "{}", last_detail(&report));

    // A timeout once the post went out.
    let mut r = rig(vec![], None);
    replies(&r, vec![json!({ "error": "timeout", "posted": true }), page_reply(200, json!({}))]);
    let report = run(&mut r, one_step()).await;
    assert_eq!(posted(&r).len(), 1);
    assert_eq!(
        last_detail(&report),
        "the step took longer than 30 seconds - a postback may already have saved, so it was not sent again"
    );

    // A timeout before it did (loading the page): nothing may have saved.
    let mut r = rig(vec![], None);
    replies(&r, vec![json!({ "error": "timeout" })]);
    let report = run(&mut r, one_step()).await;
    assert_eq!(last_detail(&report), "the step took longer than 30 seconds");
    assert_eq!(posted(&r).len(), 1);
}

#[tokio::test]
async fn a_stuck_page_is_not_sent_to_again() {
    let _act = crate::serial::activity_log();
    let mut r = rig(vec![], None);
    r.script.lock().unwrap().hang_fetch = true;
    let req = request(one_step(), prove());
    let report =
        run_template_within(&mut r.browsers, r.root.path(), &req, &quick(), Duration::from_millis(800), &QUICK_PAUSES)
            .await;
    assert!(!report.ok);
    assert_eq!(posted(&r).len(), 1);
}

#[tokio::test]
async fn a_postback_that_lands_on_sign_in_ends_the_session() {
    let _act = crate::serial::activity_log();
    // Answered by the sign-in page: the post went out, so it says so. Refused before it was sent: it did not.
    let cases = [
        (page_reply(200, json!({ "signedOut": true })), format!("{SENT_ELSEWHERE}{MAY_HAVE_SAVED}")),
        (json!({ "refused": "signed_out" }), SENT_ELSEWHERE.to_string()),
    ];
    for (reply, expected) in cases {
        let mut r = rig(vec![], None);
        replies(&r, vec![reply]);
        let report = run(&mut r, one_step()).await;
        assert!(!report.ok);
        assert_eq!(last_detail(&report), expected);
        assert_eq!(r.browsers.closed, 1, "the browser was kept");
    }
}

#[tokio::test]
async fn an_error_page_and_an_invalid_postback_fail_with_their_sentences() {
    let _act = crate::serial::activity_log();
    let mut r = rig(vec![], None);
    replies(&r, vec![page_reply(500, json!({ "aspError": "Runtime Error" }))]);
    let report = run(&mut r, one_step()).await;
    assert!(
        last_detail(&report).starts_with("the application answered with an error page: Runtime Error"),
        "{}",
        last_detail(&report)
    );
    assert_eq!(report.steps.last().unwrap().status, Some(500));

    let mut r = rig(vec![], None);
    replies(&r, vec![page_reply(500, json!({ "invalidPostback": true, "invalidField": "txtX" }))]);
    let report = run(&mut r, one_step()).await;
    assert!(
        last_detail(&report).starts_with("the application refused a field or button value the page did not offer (txtX)"),
        "{}",
        last_detail(&report)
    );
}

#[tokio::test]
async fn html_checks_and_captures_judge_the_answer() {
    let _act = crate::serial::activity_log();
    let expecting = json!({ "expect": { "html": { "shows": ["Saved"] } } });

    let mut r = rig(vec![], None);
    replies(
        &r,
        vec![page_reply(200, json!({ "checks": [{ "kind": "shows", "what": "Saved", "ok": false, "count": 0 }] }))],
    );
    let report = run(&mut r, template(vec![step("Save", "/Perf/List.aspx", expecting)])).await;
    assert_eq!(report.failed.as_deref(), Some("Save"));
    assert!(last_detail(&report).starts_with("the answer does not show Saved"), "{}", last_detail(&report));

    let capturing = json!({ "capture": { "evalId": "html:#hdnEvalId@value" } });
    let mut r = rig(vec![], None);
    replies(&r, vec![page_reply(200, json!({ "captures": { "evalId": { "count": 2, "value": "7" } } }))]);
    let report = run(&mut r, template(vec![step("Save", "/Perf/List.aspx", capturing.clone())])).await;
    assert!(
        last_detail(&report).starts_with("capture evalId: #hdnEvalId matches 2 elements, not one"),
        "{}",
        last_detail(&report)
    );

    // A good capture is an output and fills the next step's placeholder.
    let mut r = rig(vec![], None);
    replies(
        &r,
        vec![
            page_reply(200, json!({ "captures": { "evalId": { "count": 1, "value": "7" } } })),
            page_reply(200, json!({})),
        ],
    );
    let mut t = template(vec![
        step("Save", "/Perf/List.aspx", capturing),
        step("Use", "/Perf/List.aspx", json!({ "form": { "hdnId": "{{evalId}}" } })),
    ]);
    t.outputs = vec!["evalId".into()];
    let report = run(&mut r, t).await;
    assert!(report.ok, "{report:?}");
    assert_eq!(report.outputs["evalId"], json!("7"));
    assert_eq!(report.steps[0].detail, "status 200, carried on, captured evalId");
    assert_eq!(posted(&r)[1][0]["fields"]["hdnId"], json!("7"));
}

#[tokio::test]
async fn no_page_state_reaches_a_report_record_or_log() {
    let _log = crate::serial::log_tail();
    let _act = crate::serial::activity_log();
    let activity = tempfile::tempdir().unwrap();
    v2_lib::activity_log::init(activity.path().to_path_buf());

    let echoing = "<html><body><input type=\"hidden\" name=\"__VIEWSTATE\" value=\"SECRETSTATE\">\
                   <input type=\"hidden\" name=\"__EVENTVALIDATION\" value=\"SECRETVALID\">oops</body></html>";
    let mut r = rig(vec![], None);
    replies(
        &r,
        vec![page_reply(
            200,
            json!({ "excerpt": echoing, "checks": [{ "kind": "shows", "what": "Saved", "ok": false, "count": 0 }] }),
        )],
    );
    let expecting = json!({ "expect": { "html": { "shows": ["Saved"] } } });
    let report = run(&mut r, template(vec![step("Save", "/Perf/List.aspx", expecting)])).await;
    assert!(!report.ok);
    let shown = serde_json::to_string(&report).unwrap() + &report.message();
    assert!(shown.contains("(page state)"), "the failure shows no excerpt: {shown}");

    let records = activity_records(activity.path(), "api");
    let step_records: Vec<_> = records.iter().filter(|r| r.get("step").is_some()).collect();
    assert_eq!(step_records.len(), 1, "{records:?}");
    assert_eq!(step_records[0]["method"], "POST");
    assert_eq!(step_records[0]["handler"], "btnSave");
    assert_eq!(step_records[0]["postback"]["button"], "btnSave");
    assert!(
        step_records[0]["request"].as_str().unwrap().contains("\"__VIEWSTATE\":\"(page state)\""),
        "{}",
        step_records[0]["request"]
    );
    let mut everything = shown;
    for rec in &records {
        everything.push_str(&rec.to_string());
    }
    for line in v2_lib::applog::recent(400) {
        everything.push_str(&line.message);
    }
    for secret in ["SECRETSTATE", "SECRETVALID"] {
        assert!(!everything.contains(secret), "{secret} reached a report, record or log");
    }
}

#[tokio::test]
async fn a_failure_names_no_host_and_no_token() {
    let _log = crate::serial::log_tail();
    let _act = crate::serial::activity_log();
    let mut r = rig(vec![], None);
    replies(
        &r,
        vec![json!({ "error": format!("TypeError: Failed to fetch https://login.idp.example.org/sso?code=1 {TOKEN}") })],
    );
    let report = run(&mut r, one_step()).await;
    assert!(!report.ok);
    assert_eq!(last_detail(&report), "the request did not complete - see Settings, Logs");
    let mut everything = serde_json::to_string(&report).unwrap();
    for line in v2_lib::applog::recent(400) {
        everything.push_str(&line.message);
    }
    assert!(!everything.contains("idp.example.org"), "a host reached a report or log");
    assert!(!everything.contains(TOKEN), "the token reached a report or log");

    // The same once the post had gone out says the save may have happened.
    let mut r = rig(vec![], None);
    replies(&r, vec![json!({ "error": "TypeError: Failed to fetch", "posted": true })]);
    let report = run(&mut r, one_step()).await;
    assert!(last_detail(&report).ends_with(MAY_HAVE_SAVED), "{}", last_detail(&report));
}

#[tokio::test]
async fn a_mixed_template_still_sends_the_token_with_its_json_steps() {
    let _act = crate::serial::activity_log();
    let mut r = rig(vec![answer(200, json!({ "success": true }))], None);
    replies(&r, vec![page_reply(200, json!({}))]);
    let t = template(vec![
        json!({ "name": "Json", "method": "POST", "path": "/hr/pmsv10/performancecycle",
                "query": { "handler": "SaveProgress" }, "form": { "A": "1" },
                "expect": { "status": 200, "json": { "success": true } } }),
        step("Save", "/Perf/List.aspx", json!({})),
    ]);
    let report = run(&mut r, t).await;
    assert!(report.ok, "{report:?}");
    assert_eq!(r.fetched()[0][1], json!(TOKEN));
    assert_eq!(posted(&r)[0].len(), 6, "the postback got a token argument");
    assert!(!serde_json::to_string(&posted(&r)[0]).unwrap().contains(TOKEN));
}

#[tokio::test]
async fn a_one_off_postback_carries_if_the_browser_holds_the_page() {
    let _act = crate::serial::activity_log();
    let mut r = rig(vec![], None);
    r.script.lock().unwrap().token = None;
    r.script.lock().unwrap().hrefs = VecDeque::from([format!("https://hr.example.internal{PAGE}")]);
    replies(
        &r,
        vec![page_reply(
            200,
            json!({ "carried": true, "excerpt": "<input name=\"__VIEWSTATE\" value=\"SECRETSTATE\">" }),
        )],
    );
    let req = request(one_step(), Mode::Explore);
    let mut app = r.browsers.next.take().unwrap();
    let out = run_in_page(&mut app, r.root.path(), &req, &quick(), TokenPage::Here, RUN_LIMIT, &QUICK_PAUSES).await;
    assert!(out.report.ok, "{:?}", out.report);
    assert_eq!(out.postback_carried, Some(true));
    assert_eq!(posted(&r)[0][3], json!("if_there"));
    let body = out.answer.unwrap().body;
    assert!(body.contains("(page state)") && !body.contains("SECRETSTATE"), "{body}");
}
