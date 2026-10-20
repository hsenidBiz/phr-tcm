//! `repeat_while_visible` in a case script: reset a list to a known state on
//! every run - delete every goal group a previous run left behind - while
//! never touching the baseline rows other data depends on (`except`).
//!
//! Each pass looks once, without waiting, for the visible matches of
//! `selector`, sets aside every match whose row holds a protected text, marks
//! the FIRST other match's row with `data-autorun-row`, and runs `then`. A
//! `then` locator that starts with `{ "css": "[data-autorun-row]" }` is
//! resolved inside that row only, so a protected row - first on the page or
//! not - is never the one acted on. The loop stops when no unprotected match
//! is left, and fails when `max` passes did not clear them.

use crate::common;

use common::{quick, ScriptedDriver};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use v2_lib::autorun::components::ComponentFile;
use v2_lib::autorun::discovery_map::{AreaMap, DiscoveryMap, PageMap, SeenElement};
use v2_lib::autorun::runner::{run_step, AFTER_ACTION_FAILED};
use v2_lib::autorun::seen_check::check_seen;
use v2_lib::autorun::{CaseScript, StepScript};
use v2_lib::browser::actions::{execute_with, Action};
use v2_lib::browser::locator::{LocatorStep, Target, CSS_JS, VISIBLE_JS};
use v2_lib::browser::repeat::{KEPT_JS, MARK_JS, ROW_SCOPE, UNMARK_JS};

fn action(v: Value) -> Action {
    serde_json::from_value(v).expect("the test wrote an action that does not parse")
}

fn refusal(v: Value) -> String {
    action(v).validate().expect_err("this repeat_while_visible should have been refused")
}

fn step(actions: Vec<Action>) -> StepScript {
    StepScript { step_number: 1, actions, unchecked: None, cleanup: false }
}

/// The guide's example: delete every leftover goal group but the two the
/// other modules' data hangs off.
fn reset_goal_groups(max: u32) -> Value {
    json!({
        "kind": "repeat_while_visible",
        "selector": { "css": ".delete" },
        "max": max,
        "except": ["Customer Focus", "Operational Excellence"],
        "then": [
            { "kind": "click", "selector": [ { "css": ROW_SCOPE }, { "css": ".delete" } ] },
            { "kind": "click", "selector": { "css": "#confirm" } }
        ]
    })
}

// ---- the page --------------------------------------------------------------

/// A list of goal groups, each row (`row-i`) holding its name and a delete
/// icon (`del-i`, matched by `.delete`) and a `.noop` button that does
/// nothing. Clicking a row's delete icon arms the shared `#confirm` button;
/// clicking that removes the armed row. `#broken` never matches. Every
/// click is logged by the element it landed on.
struct Rows {
    names: Vec<&'static str>,
    alive: Vec<bool>,
    marked: Option<usize>,
    armed: Option<usize>,
    clicks: Vec<String>,
}

type Shared = Arc<Mutex<Rows>>;

fn ids(list: Vec<String>) -> Value {
    json!({ "result": { "objectId": format!("arr|{}", list.join(",")) } })
}

/// Where the probe says an element is: the click lands there, and the
/// mouse event's x says which element was clicked.
fn x_of(id: &str) -> f64 {
    if let Some(i) = id.strip_prefix("del-") {
        return 100.0 + i.parse::<f64>().unwrap();
    }
    if let Some(i) = id.strip_prefix("noop-") {
        return 200.0 + i.parse::<f64>().unwrap();
    }
    999.0
}

fn index(id: &str) -> Option<usize> {
    id.split('-').nth(1).and_then(|n| n.parse().ok())
}

fn goal_groups(names: Vec<&'static str>) -> (ScriptedDriver, Shared) {
    let n = names.len();
    let rows = Arc::new(Mutex::new(Rows { names, alive: vec![true; n], marked: None, armed: None, clicks: vec![] }));
    let page = rows.clone();
    let d = ScriptedDriver::new(move |method, params| {
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        let on = params["objectId"].as_str().unwrap_or("").to_string();
        let mut r = page.lock().unwrap();
        Ok(match method {
            "Runtime.evaluate" if params["expression"] == "document" => json!({ "result": { "objectId": "doc" } }),
            "Runtime.evaluate" => json!({ "result": { "value": { "origin": "https://hr.example.internal", "entries": [] } } }),
            "Runtime.callFunctionOn" if f == CSS_JS => {
                let sel = params["arguments"][0]["value"].as_str().unwrap_or("").to_string();
                let alive: Vec<usize> = (0..r.names.len()).filter(|i| r.alive[*i]).collect();
                let found: Vec<String> = match (on.as_str(), sel.as_str()) {
                    ("doc", ".delete") => alive.iter().map(|i| format!("del-{i}")).collect(),
                    ("doc", s) if s == ROW_SCOPE => {
                        r.marked.filter(|i| r.alive[*i]).map(|i| format!("row-{i}")).into_iter().collect()
                    }
                    ("doc", "#confirm") => vec!["confirm".to_string()],
                    (row, ".delete") if row.starts_with("row-") => {
                        index(row).filter(|i| r.alive[*i]).map(|i| format!("del-{i}")).into_iter().collect()
                    }
                    (row, ".noop") if row.starts_with("row-") => {
                        index(row).filter(|i| r.alive[*i]).map(|i| format!("noop-{i}")).into_iter().collect()
                    }
                    _ => vec![],
                };
                ids(found)
            }
            "Runtime.getProperties" => {
                let list = on.strip_prefix("arr|").unwrap_or("");
                let props: Vec<Value> = list
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .enumerate()
                    .map(|(i, id)| json!({ "name": i.to_string(), "value": { "objectId": id } }))
                    .collect();
                json!({ "result": props })
            }
            // The row of a delete icon, and the protected text it holds:
            // exact, ignoring case, as the real function reads it.
            "Runtime.callFunctionOn" if f == KEPT_JS => {
                let except: Vec<String> = params["arguments"][0]["value"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                    .unwrap_or_default();
                let name = index(&on).map(|i| r.names[i]).unwrap_or("");
                let kept = except.iter().find(|e| e.trim().eq_ignore_ascii_case(name)).cloned();
                json!({ "result": { "value": { "row": true, "kept": kept } } })
            }
            "Runtime.callFunctionOn" if f == MARK_JS => {
                r.marked = index(&on);
                json!({ "result": { "value": true } })
            }
            "Runtime.callFunctionOn" if f == UNMARK_JS => {
                r.marked = None;
                json!({ "result": { "value": true } })
            }
            "Runtime.callFunctionOn" if f == v2_lib::browser::input::PROBE_JS => {
                let mut p = common::ready_probe();
                p["x"] = json!(x_of(&on));
                json!({ "result": { "value": p } })
            }
            "Runtime.callFunctionOn" if f == VISIBLE_JS => {
                let seen = index(&on).is_none_or(|i| r.alive.get(i).copied().unwrap_or(true));
                json!({ "result": { "value": seen } })
            }
            "Runtime.callFunctionOn" if f == v2_lib::browser::input::HAS_FOCUS_JS => json!({ "result": { "value": true } }),
            "Runtime.callFunctionOn" => json!({ "result": { "value": "text" } }),
            "Input.dispatchMouseEvent" if params["type"] == "mouseReleased" => {
                let x = params["x"].as_f64().unwrap_or(0.0);
                if (100.0..200.0).contains(&x) {
                    let i = (x - 100.0) as usize;
                    let said = format!("delete {}", r.names[i]);
                    r.clicks.push(said);
                    r.armed = Some(i);
                } else if (200.0..300.0).contains(&x) {
                    let i = (x - 200.0) as usize;
                    let said = format!("noop {}", r.names[i]);
                    r.clicks.push(said);
                } else {
                    r.clicks.push("confirm".to_string());
                    if let Some(i) = r.armed.take() {
                        r.alive[i] = false;
                    }
                }
                json!({})
            }
            _ => json!({}),
        })
    });
    (d, rows)
}

fn left(rows: &Shared) -> Vec<&'static str> {
    let r = rows.lock().unwrap();
    (0..r.names.len()).filter(|i| r.alive[*i]).map(|i| r.names[i]).collect()
}

fn clicks(rows: &Shared) -> Vec<String> {
    rows.lock().unwrap().clicks.clone()
}

async fn run(d: &mut ScriptedDriver, actions: Vec<Action>) -> Vec<v2_lib::browser::actions::ActionOutcome> {
    let dir = tempfile::tempdir().unwrap();
    let mut acc: Option<String> = None;
    run_step(d, dir.path(), "Acme", "Web", &step(actions), &quick(), &mut acc).await.unwrap()
}

// ---- the loop ---------------------------------------------------------------

#[tokio::test]
async fn it_loops_until_none_are_left() {
    let (mut d, rows) = goal_groups(vec!["Old A", "Old B", "Old C"]);
    let a = action(json!({
        "kind": "repeat_while_visible", "selector": { "css": ".delete" }, "max": 10,
        "then": [
            { "kind": "click", "selector": [ { "css": ROW_SCOPE }, { "css": ".delete" } ] },
            { "kind": "click", "selector": { "css": "#confirm" } }
        ]
    }));
    let out = run(&mut d, vec![a]).await;
    assert_eq!(out.len(), 1, "one outcome per action, as always");
    assert!(out[0].ok, "{:?}", out[0]);
    assert!(left(&rows).is_empty(), "{:?}", left(&rows));
    assert!(out[0].detail.contains("3 passes"), "{}", out[0].detail);
    assert!(out[0].detail.contains("none left"), "{}", out[0].detail);
    assert_eq!(
        clicks(&rows),
        vec!["delete Old A", "confirm", "delete Old B", "confirm", "delete Old C", "confirm"]
    );
}

#[tokio::test]
async fn it_never_acts_on_a_protected_row_even_when_that_row_is_first() {
    let (mut d, rows) = goal_groups(vec!["Customer Focus", "Old A", "operational excellence", "Old B"]);
    let out = run(&mut d, vec![action(reset_goal_groups(10))]).await;
    assert!(out[0].ok, "{:?}", out[0]);
    assert_eq!(left(&rows), vec!["Customer Focus", "operational excellence"]);
    let c = clicks(&rows);
    assert!(!c.iter().any(|c| c.contains("Customer Focus") || c.contains("operational")), "{c:?}");
    assert_eq!(c, vec!["delete Old A", "confirm", "delete Old B", "confirm"]);
    assert!(out[0].detail.contains("2 passes"), "{}", out[0].detail);
    assert!(
        out[0].detail.contains("kept 2 protected (Customer Focus, Operational Excellence)"),
        "{}",
        out[0].detail
    );
}

#[tokio::test]
async fn it_stops_when_only_protected_rows_remain() {
    let (mut d, rows) = goal_groups(vec!["Customer Focus", "Operational Excellence"]);
    let after = action(json!({ "kind": "click", "selector": { "css": "#confirm" } }));
    let out = run(&mut d, vec![action(reset_goal_groups(5)), after]).await;
    assert!(out[0].ok, "{:?}", out[0]);
    assert!(out[0].detail.contains("0 passes"), "{}", out[0].detail);
    assert!(out[0].detail.contains("kept 2 protected"), "{}", out[0].detail);
    assert!(out[1].ok, "the step carries on: {:?}", out[1]);
    assert_eq!(left(&rows).len(), 2);
    assert_eq!(clicks(&rows), vec!["confirm"], "only the step's own action clicked");
}

#[tokio::test]
async fn it_fails_at_the_max_cap_with_a_clear_sentence() {
    let (mut d, rows) = goal_groups(vec!["Old A", "Old B", "Customer Focus"]);
    // The pass clicks something that removes nothing: the rows stay.
    let a = action(json!({
        "kind": "repeat_while_visible", "selector": { "css": ".delete" }, "max": 2, "except": ["Customer Focus"],
        "then": [ { "kind": "click", "selector": [ { "css": ROW_SCOPE }, { "css": ".noop" } ] } ]
    }));
    let after = action(json!({ "kind": "click", "selector": { "css": "#confirm" } }));
    let out = run(&mut d, vec![a, after]).await;
    assert!(!out[0].ok, "{:?}", out[0]);
    assert!(out[0].detail.contains("still 2 rows after max 2 passes"), "{}", out[0].detail);
    assert_eq!(out[1].detail, AFTER_ACTION_FAILED, "the step stops");
    assert_eq!(clicks(&rows), vec!["noop Old A", "noop Old A"], "max passes, no more");
}

#[tokio::test]
async fn a_then_failure_stops_the_step() {
    let (mut d, rows) = goal_groups(vec!["Old A", "Old B"]);
    let a = action(json!({
        "kind": "repeat_while_visible", "selector": { "css": ".delete" }, "max": 10,
        "then": [
            { "kind": "click", "selector": [ { "css": ROW_SCOPE }, { "css": ".delete" } ] },
            { "kind": "click", "selector": { "css": "#broken" } }
        ]
    }));
    let after = action(json!({ "kind": "click", "selector": { "css": "#confirm" } }));
    let out = run(&mut d, vec![a, after]).await;
    assert!(!out[0].ok, "{:?}", out[0]);
    assert!(out[0].detail.contains("pass 1"), "{}", out[0].detail);
    assert!(out[0].detail.contains("#broken"), "{}", out[0].detail);
    assert_eq!(out[1].detail, AFTER_ACTION_FAILED);
    assert_eq!(clicks(&rows), vec!["delete Old A"], "nothing after the failure");
    assert_eq!(left(&rows), vec!["Old A", "Old B"]);
}

#[tokio::test]
async fn the_executor_hands_it_to_the_runner() {
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({})));
    let out = execute_with(&mut d, &action(reset_goal_groups(3)), &quick()).await;
    assert!(!out.ok);
    assert_eq!(out.detail, "repeat_while_visible is carried out by the runner");
}

// ---- validation ---------------------------------------------------------------

#[test]
fn the_guide_example_is_accepted_and_round_trips() {
    let a = action(reset_goal_groups(20));
    assert_eq!(a.validate(), Ok(()));
    assert!(!a.is_check(), "a reset tidies up; it checks nothing");
    assert_eq!(serde_json::to_value(&a).unwrap(), reset_goal_groups(20));
}

#[test]
fn max_is_required_and_bounded() {
    let mut no_max = reset_goal_groups(1);
    no_max.as_object_mut().unwrap().remove("max");
    assert_eq!(
        refusal(no_max),
        "repeat_while_visible needs max - the most passes it may make, 1 to 200"
    );
    assert_eq!(refusal(reset_goal_groups(0)), "repeat_while_visible max is 1 to 200, not 0");
    assert_eq!(refusal(reset_goal_groups(201)), "repeat_while_visible max is 1 to 200, not 201");
    assert_eq!(action(reset_goal_groups(200)).validate(), Ok(()));
}

#[test]
fn then_and_except_are_checked() {
    let mut empty = reset_goal_groups(5);
    empty["then"] = json!([]);
    assert_eq!(refusal(empty), "repeat_while_visible has nothing in \"then\" - say what to do on each pass");

    let mut blank = reset_goal_groups(5);
    blank["except"] = json!(["Customer Focus", "  "]);
    assert_eq!(refusal(blank), "repeat_while_visible except 2 is empty - give the protected text");

    // A then that never acts inside the row could act on a protected one.
    let mut unscoped = reset_goal_groups(5);
    unscoped["then"] = json!([{ "kind": "click", "selector": { "css": "#confirm" } }]);
    assert_eq!(
        refusal(unscoped),
        "repeat_while_visible with except needs a then action on the row it is working on - start its locator with { \"css\": \"[data-autorun-row]\" }"
    );

    // The loop's own selector, page-wide, is the first match: maybe protected.
    let mut page_wide = reset_goal_groups(5);
    page_wide["then"] = json!([
        { "kind": "click", "selector": [ { "css": ROW_SCOPE }, { "css": ".delete" } ] },
        { "kind": "click", "selector": { "css": ".delete" } }
    ]);
    assert_eq!(
        refusal(page_wide),
        "repeat_while_visible then 2 points at the loop's own selector across the whole page, which can be a protected row - start its locator with { \"css\": \"[data-autorun-row]\" }"
    );

    // A selector string cannot list its matches: it takes one.
    let mut legacy = reset_goal_groups(5);
    legacy["selector"] = json!(".delete");
    assert_eq!(
        refusal(legacy),
        "repeat_while_visible needs a locator object for selector, not a selector string"
    );

    // then holds plain actions, as when_visible's does.
    let mut nested = reset_goal_groups(5);
    nested["then"] = json!([reset_goal_groups(2)]);
    assert_eq!(refusal(nested), "repeat_while_visible \"then\" cannot hold another repeat_while_visible");
    let mut check = reset_goal_groups(5);
    check["then"] = json!([
        { "kind": "click", "selector": [ { "css": ROW_SCOPE }, { "css": ".delete" } ] },
        { "kind": "expect_visible", "selector": { "css": "#x" } }
    ]);
    assert_eq!(
        refusal(check),
        "repeat_while_visible \"then\" cannot hold expect_visible - a pass tidies up and checks nothing, so put the check after the loop"
    );
}

#[test]
fn a_sign_in_recipe_cannot_hold_it() {
    let recipe: v2_lib::autorun::recipe::SignInRecipe = serde_json::from_value(json!({
        "start_url": "https://hr.example.internal/",
        "steps": [ { "kind": "click", "selector": { "css": "#go" } } ],
        "after_sign_in": [ reset_goal_groups(3) ],
        "signed_in": { "css": "#marker" }
    }))
    .unwrap();
    let why = recipe.validate().unwrap_err();
    assert!(why.contains("a sign-in recipe cannot contain repeat_while_visible - it belongs in a case script"), "{why}");
}

// ---- the seen check ---------------------------------------------------------------

fn css(c: &str) -> LocatorStep {
    LocatorStep { css: Some(c.to_string()), ..LocatorStep::default() }
}

/// Each link seen in "Goals" on /goals, matching `matched` elements.
fn seen(links: &[(LocatorStep, u32)]) -> DiscoveryMap {
    let elements = links
        .iter()
        .map(|(l, matched)| SeenElement {
            key: l.seen_key().unwrap(),
            locator: Target::One(l.clone()),
            role: String::new(),
            name: String::new(),
            kind: "other".to_string(),
            required: false,
            seen_at: 0,
            matched: *matched,
            all: None,
        })
        .collect();
    DiscoveryMap {
        areas: vec![AreaMap {
            area: "Goals".to_string(),
            pages: vec![PageMap { path: "/goals".to_string(), title: String::new(), elements }],
            ..AreaMap::default()
        }],
    }
}

fn goals_script(actions: Value) -> CaseScript {
    serde_json::from_value(json!({ "case_id": 7, "title": "T", "area": "Goals",
                                   "steps": [{ "step_number": 1, "actions": actions }] }))
    .unwrap()
}

#[test]
fn the_seen_check_reads_its_selector_and_then_like_when_visible() {
    let s = goals_script(json!([reset_goal_groups(10)]));
    // The delete icon shows on every row, so it was seen matching several;
    // the row link itself is the loop's own and never on the map.
    let map = seen(&[(css(".delete"), 4), (css("#confirm"), 1)]);
    let none = ComponentFile::default();
    assert_eq!(check_seen(&map, &none, &s, &[], None), Ok(()));

    // A then locator never seen is refused, as any.
    let no_confirm = seen(&[(css(".delete"), 4)]);
    let why = check_seen(&no_confirm, &none, &s, &[], None).unwrap_err();
    assert!(why.contains("#confirm"), "{why}");

    // The loop's own selector never seen is refused too.
    let no_delete = seen(&[(css("#confirm"), 1)]);
    let why = check_seen(&no_delete, &none, &s, &[], None).unwrap_err();
    assert!(why.contains(".delete"), "{why}");
}

// ---- Playwright export ---------------------------------------------------------------

fn export(actions: Value) -> Result<String, String> {
    let script: CaseScript =
        serde_json::from_value(json!({ "case_id": 1, "title": "A case", "steps": [{ "step_number": 1, "actions": actions }] }))
            .unwrap();
    let area = vec![Target::One(css("a.area"))];
    let by_name = std::collections::BTreeMap::new();
    let texts = std::collections::BTreeMap::new();
    let origins = vec!["https://app.example".to_string()];
    v2_lib::pw_export::translate::raw_spec(&v2_lib::pw_export::translate::RawSpecInput {
        script: &script,
        md_path: "suites/sl/admin/m/f/test-cases/f.md".into(),
        feature_title: "F".into(),
        after_sign_in: &[],
        area_clicks: &area,
        area_clicks_by_name: &by_name,
        step_texts: &texts,
        origins: &origins,
    })
    .map_err(|e| e.0)
}

#[test]
fn the_export_writes_a_bounded_loop_with_the_same_except_rule() {
    let spec = export(json!([reset_goal_groups(20)])).unwrap().replace("\r\n", "\n");
    // The same bounded loop, the same protected texts, the same row rule.
    assert!(spec.contains("for (let pass = 0; ; pass++) {"), "{spec}");
    assert!(spec.contains("[\"Customer Focus\",\"Operational Excellence\"]"), "{spec}");
    assert!(spec.contains("'still ' + left + ' rows after max 20 passes'"), "{spec}");
    assert!(spec.contains("data-autorun-row"), "{spec}");
    assert!(spec.contains("tr, [role=row], li"), "{spec}");
    // The then actions act inside the marked row.
    assert!(spec.contains("cur.locator('[data-autorun-row]')"), "{spec}");
    assert!(spec.contains(".locator('.delete')"), "{spec}");
    assert!(spec.contains("cur.locator('#confirm')"), "{spec}");
    // The check function refuses nothing it can write.
    let script: CaseScript = goals_script(json!([reset_goal_groups(20)]));
    assert_eq!(v2_lib::pw_export::translate::check(&script, &["https://app.example".to_string()]), Ok(()));
}

// ---- the guide ---------------------------------------------------------------

#[test]
fn the_guide_teaches_it_with_the_goal_group_example_and_its_safety_rules() {
    let g = v2_lib::autorun::guide::autorun_guide();
    let section = g.split_once("## Resetting a list to a known state").expect("no section").1;
    let section = section.split("\n## ").next().unwrap();
    for term in ["Customer Focus", "Operational Excellence", "[data-autorun-row]", "max", "1 to 200",
                 "still 3 rows after max 20 passes", "tr", "[role=row]", "data-*-id", "exactly one of the `except` texts"] {
        assert!(section.contains(term), "the section never says {term:?}");
    }
    let at = section.find("{ \"kind\": \"repeat_while_visible\"").expect("no example");
    let mut depth = 0;
    let mut end = at;
    for (i, c) in section[at..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = at + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    let example = action(serde_json::from_str(&section[at..end]).expect("the example is not JSON"));
    assert_eq!(example.validate(), Ok(()));
}
