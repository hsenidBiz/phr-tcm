//! Recording a sign-in recipe: what the page reports becomes steps and a
//! signed-in check, the review's field choices make the recipe, and the
//! recipe is saved only after it has signed in on its own.

use crate::common;

use common::{account, quick, stateful_app, ScriptedDriver, PASSWORD};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use v2_lib::autorun::recipe::{load_recipe, save_recipe, RecipeStep, SignInRecipe, PASSWORD as PASSWORD_SLOT, USERNAME};
use v2_lib::autorun::recorder::{AxLink, Ended, BROWSER_CLOSED, CANCELLED, HELD_JS};
use v2_lib::autorun::sessions::{now_ms, save_session};
use v2_lib::autorun::signin_recorder::{
    arm, capture, check_failure, css_escape, css_string, field_css, field_locator_from_ax, field_locator_from_hints,
    finish, next_report, Captured, Draft, FieldChoice, FieldHints, FieldRole, Report, Seen, Step, BINDING,
    CHECK_BROWSER_SILENT, CHECK_START_DID_NOT_OPEN, LISTENER_JS, NO_MARKER, NO_STEPS, NO_SUBMIT, PICK_OFF_JS,
    PICK_ON_JS, UNREADABLE_CLICK, UNREADABLE_FIELD,
};
use v2_lib::browser::actions::Action;
use v2_lib::browser::cdp::Event;
use v2_lib::browser::locator::{LocatorStep, Target};
use v2_lib::browser::session::SavedSession;
use v2_lib::commands::autorun::close_autorun_browsers;
use v2_lib::commands::autorun_record::{auto_run_record_cancel, recording_is_going, RecorderClaim};
use v2_lib::commands::autorun_record_signin::{
    auto_run_record_sign_in_pick, auto_run_record_sign_in_stop, build_recipe, check_sign_in, current_draft,
    forget_draft, keep_draft, listen, open_the_recording, sign_in_recording_is_open, Listening, SignInFor,
    NO_DRAFT, OTHER_PROJECT,
};
use v2_lib::events::RecordingEvent;

fn exact_role(role: &str, name: &str) -> Target {
    Target::One(LocatorStep { role: Some(role.into()), name: Some(name.into()), exact: true, ..LocatorStep::default() })
}

fn css(s: &str) -> Target {
    Target::One(LocatorStep { css: Some(s.into()), ..LocatorStep::default() })
}

fn node(role: &str, name: &str) -> AxLink {
    AxLink { role: role.into(), name: name.into(), ignored: false }
}

fn hints(tag: &str, password: bool, id: &str, name: &str, placeholder: &str) -> FieldHints {
    FieldHints { tag: tag.into(), password, id: id.into(), name: name.into(), placeholder: placeholder.into() }
}

/// A page's report, the way the binding delivers it.
fn report(payload: Value) -> Event {
    Event { method: "Runtime.bindingCalled".into(), params: json!({ "name": BINDING, "payload": payload.to_string() }) }
}

fn field_report(i: u32, password: bool, id: &str) -> Event {
    report(json!({ "ev": "field", "doc": "d1", "i": i, "tag": "input", "password": password, "id": id, "name": "", "placeholder": "" }))
}

fn click_report(ev: &str, i: u32, text: &str) -> Event {
    report(json!({ "ev": ev, "doc": "d1", "i": i, "tag": "button", "role": "", "label": "", "text": text }))
}

/// The accessibility tree around held element `i` (backend 50 + i): one
/// node with this role and name, inside the page.
fn tree(i: usize, role: &str, name: &str) -> Value {
    json!({ "nodes": [
        { "nodeId": "1", "role": { "value": "RootWebArea" }, "name": { "value": "Sign in" }, "childIds": ["2"] },
        { "nodeId": "2", "role": { "value": role }, "name": { "value": name }, "parentId": "1", "backendDOMNodeId": 50 + i }
    ] })
}

/// A page holding these elements: `Some(tree)` still there, `None` gone.
/// Every `Runtime.evaluate` expression is kept in `evals`.
fn page_with(held: Vec<Option<Value>>, evals: Arc<std::sync::Mutex<Vec<String>>>) -> ScriptedDriver {
    page_hooked(held, evals, |_| {})
}

/// `page_with`, calling `on_eval` with each expression it is asked to run.
fn page_hooked(
    held: Vec<Option<Value>>,
    evals: Arc<std::sync::Mutex<Vec<String>>>,
    mut on_eval: impl FnMut(&str) + Send + 'static,
) -> ScriptedDriver {
    let mut last_i = 0usize;
    ScriptedDriver::new(move |method, params| {
        Ok(match method {
            "Runtime.evaluate" if params["expression"] == "document" => json!({ "result": { "objectId": "doc" } }),
            "Runtime.evaluate" => {
                let expression = params["expression"].as_str().unwrap_or("");
                evals.lock().unwrap().push(expression.to_string());
                on_eval(expression);
                json!({ "result": { "value": null } })
            }
            "Runtime.callFunctionOn" if params["functionDeclaration"] == HELD_JS => {
                last_i = params["arguments"][1]["value"].as_u64().unwrap_or(0) as usize;
                json!({ "result": { "objectId": "arr" } })
            }
            "Runtime.getProperties" => match held.get(last_i) {
                Some(Some(_)) => json!({ "result": [{ "name": "0", "value": { "objectId": format!("el{last_i}") } }] }),
                _ => json!({ "result": [] }),
            },
            "DOM.describeNode" => {
                let i: i64 = params["objectId"].as_str().unwrap_or("el0")[2..].parse().unwrap_or(0);
                json!({ "node": { "backendNodeId": 50 + i } })
            }
            "Accessibility.getPartialAXTree" => {
                let i = (params["backendNodeId"].as_i64().unwrap_or(50) - 50) as usize;
                held.get(i).cloned().flatten().unwrap_or(json!({ "nodes": [] }))
            }
            _ => json!({}),
        })
    })
}

fn no_evals() -> Arc<std::sync::Mutex<Vec<String>>> {
    Arc::new(std::sync::Mutex::new(vec![]))
}

// ---- the listener ----------------------------------------------------------

/// The listener sits on the page where the password is typed. It never
/// reads a field's value, reports only trusted events, and stops a click
/// only in pick mode.
#[test]
fn the_listener_never_reads_what_was_typed_and_stops_only_the_picked_click() {
    assert!(!LISTENER_JS.contains(".value"), "the listener must never read a value");
    assert!(!LISTENER_JS.contains("'value'") && !LISTENER_JS.contains("\"value\""), "nor a value attribute");
    assert!(LISTENER_JS.contains(BINDING));
    for ev in ["'input'", "'change'", "'focusout'", "'keydown'", "'click'"] {
        assert!(LISTENER_JS.contains(&format!("addEventListener({ev}")), "listens for {ev}");
    }
    assert_eq!(LISTENER_JS.matches("isTrusted").count(), 4, "every handler ignores events a script made");
    assert_eq!(LISTENER_JS.matches("preventDefault").count(), 1);
    assert_eq!(LISTENER_JS.matches("stopImmediatePropagation").count(), 1);
    let pick = LISTENER_JS.find("if (window.__tcmRecPick)").expect("pick mode");
    let stop = LISTENER_JS.find("preventDefault").unwrap();
    assert!(pick < stop && stop - pick < 120, "only the picked click is stopped");
    assert!(LISTENER_JS.contains("button[type=submit],input[type=submit],button:not([type])"), "Enter's button");
    assert_eq!(PICK_ON_JS, "window.__tcmRecPick = true");
    assert_eq!(PICK_OFF_JS, "window.__tcmRecPick = false");
}

#[tokio::test]
async fn arming_adds_the_binding_and_the_listener_to_every_new_document() {
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({})));
    arm(&mut d).await.unwrap();
    assert_eq!(d.methods(), vec!["Runtime.enable", "Runtime.addBinding", "Page.addScriptToEvaluateOnNewDocument"]);
    assert_eq!(d.calls_to("Runtime.addBinding")[0]["name"], BINDING);
    assert_eq!(d.calls_to("Page.addScriptToEvaluateOnNewDocument")[0]["source"], LISTENER_JS);
}

#[tokio::test]
async fn each_kind_of_report_is_read_back_and_another_binding_is_ignored() {
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({})));
    d.events.push_back(Event { method: "Runtime.bindingCalled".into(), params: json!({ "name": "__tcmRecordClick", "payload": "{}" }) });
    d.events.push_back(field_report(0, true, "pw"));
    d.events.push_back(click_report("click", 1, "Next"));
    d.events.push_back(click_report("marker", 2, "Sign out"));
    d.events.push_back(report(json!({ "ev": "no_submit", "doc": "d1" })));
    let wait = Duration::from_millis(10);
    assert_eq!(next_report(&mut d, wait).await.unwrap(), None);
    match next_report(&mut d, wait).await.unwrap() {
        Some(Report::Field(f)) => {
            assert_eq!((f.doc.as_str(), f.i), ("d1", 0));
            assert_eq!(f.hints, hints("input", true, "pw", "", ""));
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(next_report(&mut d, wait).await.unwrap(), Some(Report::Click(c)) if c.i == 1 && c.hints.text == "Next"));
    assert!(matches!(next_report(&mut d, wait).await.unwrap(), Some(Report::Marker(c)) if c.i == 2));
    assert_eq!(next_report(&mut d, wait).await.unwrap(), Some(Report::NoSubmit {}));
    assert_eq!(next_report(&mut d, wait).await.unwrap(), None, "nothing more: a timeout is not an error");
}

// ---- locating a field ------------------------------------------------------

#[test]
fn a_field_is_named_by_its_own_accessible_role_and_name() {
    assert_eq!(field_locator_from_ax(&[node("textbox", "  Email \n address "), node("form", "")]), Some(exact_role("textbox", "Email address")));
    assert_eq!(field_locator_from_ax(&[node("searchbox", "Find")]), Some(exact_role("searchbox", "Find")));
    let ignored = [AxLink { role: "generic".into(), name: String::new(), ignored: true }, node("textbox", "Password")];
    assert_eq!(field_locator_from_ax(&ignored), Some(exact_role("textbox", "Password")));
    assert_eq!(field_locator_from_ax(&[node("textbox", "  ")]), None, "a field with no name");
    assert_eq!(field_locator_from_ax(&[node("generic", "Email"), node("textbox", "Email")]), None, "only the field's own node");
    assert_eq!(field_locator_from_ax(&[]), None);
}

#[test]
fn a_fields_css_is_its_id_else_its_name_else_the_password_field_else_its_placeholder() {
    assert_eq!(field_css(&hints("input", true, "pw", "password", "Password")), Some("#pw".into()));
    assert_eq!(field_css(&hints("INPUT", true, "", "password", "Password")), Some("input[name=\"password\"]".into()));
    assert_eq!(field_css(&hints("input", true, " ", "", "Password")), Some("input[type=\"password\"]".into()));
    assert_eq!(field_css(&hints("textarea", false, "", "", "Say why")), Some("textarea[placeholder=\"Say why\"]".into()));
    assert_eq!(field_css(&hints("", false, "", "", "Email")), Some("input[placeholder=\"Email\"]".into()));
    assert_eq!(field_css(&hints("input", false, "", "", "")), None);
    assert_eq!(field_locator_from_hints(&hints("input", false, "user", "", "")), Some(css("#user")));
    assert_eq!(field_locator_from_hints(&hints("input", false, "", "", "")), None);
}

#[test]
fn ids_and_attribute_values_are_escaped_the_way_css_reads_them() {
    assert_eq!(css_escape("user"), "user");
    assert_eq!(css_escape("1st"), "\\31 st");
    assert_eq!(css_escape("-1"), "-\\31 ");
    assert_eq!(css_escape("-"), "\\-");
    assert_eq!(css_escape("a.b:c[0]"), "a\\.b\\:c\\[0\\]");
    assert_eq!(css_escape("a b"), "a\\ b");
    assert_eq!(css_escape("é_x-2"), "é_x-2");
    assert_eq!(css_escape("a\u{1}b"), "a\\1 b");
    assert_eq!(css_escape("a\0"), "a\u{FFFD}");
    assert_eq!(css_string("say \"hi\" \\ now"), "\"say \\\"hi\\\" \\\\ now\"");
    assert_eq!(css_string("a\nb"), "\"a\\a b\"");
    assert_eq!(field_css(&hints("input", false, "login:user", "", "")), Some("#login\\:user".into()));
    assert_eq!(field_css(&hints("input", false, "", "a\"b", "")), Some("input[name=\"a\\\"b\"]".into()));
}

// ---- the recording ---------------------------------------------------------

#[tokio::test]
async fn a_recording_keeps_fields_and_clicks_in_order_and_the_picked_check() {
    let evals = no_evals();
    // 0: the username field (named by the tree); 1: the password field,
    // gone by the time it is asked about (named by its id); 2: the Next
    // button, gone (named by its words); 3: the signed-in check.
    let mut d = page_with(
        vec![Some(tree(0, "textbox", "Email")), None, None, Some(tree(3, "button", "Sign out"))],
        evals.clone(),
    );
    d.events.push_back(field_report(0, false, "email"));
    d.events.push_back(field_report(0, false, "email"));
    d.events.push_back(field_report(1, true, "pw"));
    d.events.push_back(click_report("click", 2, "Next"));
    d.events.push_back(report(json!({ "ev": "no_submit", "doc": "d1" })));
    d.events.push_back(click_report("marker", 3, "Sign out"));
    let (stop, cancel, pick) = (AtomicBool::new(true), AtomicBool::new(false), AtomicBool::new(true));
    let mut seen: Vec<String> = vec![];
    let captured = capture(&mut d, &stop, &cancel, &pick, &mut |s| {
        seen.push(match s {
            Seen::Step(n, Step::Click(t)) => format!("{n} click {}", t.describe()),
            Seen::Step(n, Step::Field { target, password }) => format!("{n} field {} {password}", target.describe()),
            Seen::Marker(t) => format!("marker {}", t.describe()),
            Seen::Unreadable(why) => why.to_string(),
        })
    })
    .await;
    assert_eq!(
        captured.steps,
        vec![
            Step::Field { target: exact_role("textbox", "Email"), password: false },
            Step::Field { target: css("#pw"), password: true },
            Step::Click(Target::One(LocatorStep { text: Some("Next".into()), exact: true, ..LocatorStep::default() })),
        ],
        "a field reported twice in a row is one step"
    );
    assert_eq!(captured.marker, Some(exact_role("button", "Sign out")));
    assert_eq!(captured.ended, Ended::Stopped { href: String::new() });
    assert_eq!(
        seen,
        vec![
            "1 field textbox \"Email\" false".to_string(),
            "2 field #pw true".into(),
            "3 click text \"Next\"".into(),
            NO_SUBMIT.into(),
            "marker button \"Sign out\"".into(),
        ]
    );
    assert!(!pick.load(Ordering::SeqCst), "a marker ends pick mode");
    assert!(evals.lock().unwrap().contains(&PICK_OFF_JS.to_string()), "and turns it off on the page");
    let (steps, marker) = finish(captured).unwrap();
    assert_eq!((steps.len(), marker.is_some()), (3, true));
}

#[tokio::test]
async fn pick_mode_is_put_back_on_the_page_until_a_marker_arrives() {
    let evals = no_evals();
    let stop = Arc::new(AtomicBool::new(false));
    // Stop is pressed once pick mode has been put back on the page once.
    let stop_after = stop.clone();
    let mut d = page_hooked(vec![], evals.clone(), move |expression| {
        if expression == PICK_ON_JS {
            stop_after.store(true, Ordering::SeqCst);
        }
    });
    d.events.push_back(click_report("click", 0, "Next"));
    let pick = AtomicBool::new(true);
    let captured = capture(&mut d, &stop, &AtomicBool::new(false), &pick, &mut |_| {}).await;
    assert_eq!(captured.steps.len(), 1);
    assert_eq!(captured.marker, None);
    assert!(pick.load(Ordering::SeqCst), "still waiting for the pick");
    assert_eq!(evals.lock().unwrap().iter().filter(|e| *e == PICK_ON_JS).count(), 1);

    // Without pick mode nothing is put on the page.
    let evals = no_evals();
    let mut d = page_with(vec![], evals.clone());
    let captured = capture(&mut d, &AtomicBool::new(true), &AtomicBool::new(false), &AtomicBool::new(false), &mut |_| {}).await;
    assert!(captured.steps.is_empty());
    assert!(evals.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_click_or_field_that_cannot_be_named_is_a_note_not_a_step() {
    let mut d = page_with(vec![None, None], no_evals());
    d.events.push_back(click_report("click", 0, ""));
    d.events.push_back(field_report(1, false, ""));
    let mut notes: Vec<String> = vec![];
    let captured = capture(&mut d, &AtomicBool::new(true), &AtomicBool::new(false), &AtomicBool::new(false), &mut |s| {
        if let Seen::Unreadable(why) = s {
            notes.push(why.to_string());
        }
    })
    .await;
    assert!(captured.steps.is_empty());
    assert_eq!(notes, vec![UNREADABLE_CLICK.to_string(), UNREADABLE_FIELD.to_string()]);
    assert_eq!(finish(captured).unwrap_err(), NO_STEPS);
}

#[tokio::test]
async fn a_closed_browser_or_a_cancel_ends_the_recording_with_nothing() {
    let mut d = page_with(vec![Some(tree(0, "textbox", "Email"))], no_evals());
    d.closed_when_drained = true;
    d.events.push_back(field_report(0, false, "email"));
    let mut heard: Vec<RecordingEvent> = vec![];
    let captured = listen(&mut d, &AtomicBool::new(false), &AtomicBool::new(false), &AtomicBool::new(false), &mut |e| heard.push(e)).await;
    assert_eq!(captured.ended, Ended::Closed);
    assert_eq!(heard.len(), 2, "{heard:?}");
    assert_eq!((heard[0].kind.as_str(), heard[0].index, heard[0].readable.as_str()), ("field", 1, "textbox \"Email\""));
    assert_eq!((heard[1].kind.as_str(), heard[1].detail.as_str()), ("closed", BROWSER_CLOSED));
    assert_eq!(finish(captured).unwrap_err(), BROWSER_CLOSED);

    let mut d = page_with(vec![], no_evals());
    d.events.push_back(click_report("click", 0, "Next"));
    let captured = capture(&mut d, &AtomicBool::new(false), &AtomicBool::new(true), &AtomicBool::new(false), &mut |_| {}).await;
    assert_eq!(captured, Captured { steps: vec![], marker: None, ended: Ended::Cancelled });
    assert_eq!(finish(captured).unwrap_err(), CANCELLED);
}

#[tokio::test]
async fn every_event_carries_locator_words_and_says_which_field_is_the_password() {
    let mut d = page_with(vec![None, Some(tree(1, "textbox", "Password")), None, Some(tree(3, "link", "Kim"))], no_evals());
    d.events.push_back(field_report(0, false, "user"));
    d.events.push_back(field_report(1, true, "pw"));
    d.events.push_back(click_report("click", 2, "Sign in"));
    d.events.push_back(click_report("marker", 3, "Kim"));
    let mut heard: Vec<RecordingEvent> = vec![];
    listen(&mut d, &AtomicBool::new(true), &AtomicBool::new(false), &AtomicBool::new(true), &mut |e| heard.push(e)).await;
    let rows: Vec<(String, u32, String, bool)> =
        heard.iter().map(|e| (e.kind.clone(), e.index, e.readable.clone(), e.password)).collect();
    assert_eq!(
        rows,
        vec![
            ("field".to_string(), 1, "#user".to_string(), false),
            ("field".into(), 2, "textbox \"Password\"".into(), true),
            ("click".into(), 3, "text \"Sign in\"".into(), false),
            ("marker".into(), 0, "link \"Kim\"".into(), false),
        ]
    );
    for e in &heard {
        assert!(!format!("{e:?}").contains(PASSWORD), "{e:?}");
    }
}

// ---- from a draft to a recipe ----------------------------------------------

fn draft() -> Draft {
    Draft {
        organization: "acme".into(),
        project: "Web".into(),
        start_url: "https://hr.example.internal/".into(),
        steps: vec![
            Step::Field { target: css("#user"), password: false },
            Step::Click(exact_role("button", "Next")),
            Step::Field { target: css("#pass"), password: true },
            Step::Field { target: css("#domain"), password: false },
            Step::Click(css("#go")),
        ],
        marker: Some(css("#marker")),
    }
}

fn choice(role: FieldRole, text: &str) -> FieldChoice {
    FieldChoice { role, text: text.into() }
}

fn roles() -> Vec<FieldChoice> {
    vec![choice(FieldRole::Username, ""), choice(FieldRole::Password, ""), choice(FieldRole::Text, "CORP")]
}

fn fill(target: Target, value: &str) -> RecipeStep {
    RecipeStep::Do(Action::Fill { selector: target, value: value.into() })
}

fn click(target: Target) -> RecipeStep {
    RecipeStep::Do(Action::Click { selector: target })
}

#[test]
fn a_draft_becomes_a_recipe_with_placeholders_fixed_text_and_the_default_settings() {
    let recipe = draft().recipe(&roles(), None).unwrap();
    assert_eq!(
        recipe,
        SignInRecipe {
            start_url: "https://hr.example.internal/".into(),
            steps: vec![
                fill(css("#user"), USERNAME),
                click(exact_role("button", "Next")),
                fill(css("#pass"), PASSWORD_SLOT),
                fill(css("#domain"), "CORP"),
                click(css("#go")),
            ],
            after_sign_in: vec![],
            signed_in: css("#marker"),
            allowed_origins: vec![],
            session_minutes: 480,
        }
    );
}

#[test]
fn a_draft_keeps_the_projects_after_sign_in_sites_and_session_length() {
    let existing: SignInRecipe = serde_json::from_value(json!({
        "start_url": "https://old.example.internal/",
        "steps": [ { "kind": "click", "selector": { "css": "#old" } } ],
        "after_sign_in": [ { "kind": "click", "selector": { "css": "#menu" } } ],
        "signed_in": { "css": "#old-marker" },
        "allowed_origins": ["https://sso.example.internal"],
        "session_minutes": 90
    }))
    .unwrap();
    let recipe = draft().recipe(&roles(), Some(&existing)).unwrap();
    assert_eq!(recipe.start_url, "https://hr.example.internal/", "the recording's own start address");
    assert_eq!(recipe.signed_in, css("#marker"), "and its own check");
    assert_eq!(recipe.steps.len(), 5);
    assert_eq!(recipe.after_sign_in, existing.after_sign_in);
    assert_eq!(recipe.allowed_origins, existing.allowed_origins);
    assert_eq!(recipe.session_minutes, 90);
}

#[test]
fn a_draft_is_refused_without_a_check_steps_or_usable_fixed_text() {
    let mut no_marker = draft();
    no_marker.marker = None;
    assert_eq!(no_marker.recipe(&roles(), None).unwrap_err(), NO_MARKER);

    let mut no_steps = draft();
    no_steps.steps.clear();
    assert_eq!(no_steps.recipe(&[], None).unwrap_err(), NO_STEPS);

    let blank = vec![choice(FieldRole::Username, ""), choice(FieldRole::Password, ""), choice(FieldRole::Text, "  ")];
    assert_eq!(draft().recipe(&blank, None).unwrap_err(), "step 4: type the fixed text, or choose Username or Password");

    for slot in [USERNAME, PASSWORD_SLOT] {
        let placeholder =
            vec![choice(FieldRole::Username, ""), choice(FieldRole::Password, ""), choice(FieldRole::Text, &format!("x{slot}"))];
        let err = draft().recipe(&placeholder, None).unwrap_err();
        assert!(err.starts_with("step 4: fixed text cannot hold {{username}} or {{password}}"), "{err}");
    }

    let err = draft().recipe(&roles()[..2], None).unwrap_err();
    assert_eq!(err, "the field choices do not match the recording - record it again");

    let mut bad_start = draft();
    bad_start.start_url = "hr.example.internal".into();
    assert_eq!(bad_start.recipe(&roles(), None).unwrap_err(), "the start address must be a full http or https address");
}

#[test]
fn the_draft_the_dialog_sees_is_words_only() {
    let view = draft().view();
    let kinds: Vec<(&str, &str, bool)> =
        view.steps.iter().map(|s| (s.kind.as_str(), s.readable.as_str(), s.password)).collect();
    assert_eq!(
        kinds,
        vec![
            ("field", "#user", false),
            ("click", "button \"Next\"", false),
            ("field", "#pass", true),
            ("field", "#domain", false),
            ("click", "#go", false),
        ]
    );
    assert_eq!(view.marker, "#marker");
    let mut none = draft();
    none.marker = None;
    assert_eq!(none.view().marker, "");
}

#[test]
fn a_check_that_fails_says_which_part_and_never_the_address() {
    assert_eq!(check_failure("anything", true), CHECK_BROWSER_SILENT);
    assert_eq!(check_failure("the sign-in page did not open: net::ERR at https://hr.example.internal/", false), CHECK_START_DID_NOT_OPEN);
    let after = check_failure("signed in as Kim, but after_sign_in step 2 stopped: no element matched #menu", false);
    assert_eq!(
        after,
        "the recorded sign-in worked, but the recipe's after-sign-in steps did not - nothing was saved. After-sign-in step 2 stopped: no element matched #menu"
    );
    let marker = check_failure(
        "the recipe ran, but link \"Kim\" never appeared - check the username and password for \"admin\", and the recipe's signed_in locator",
        false,
    );
    assert!(marker.starts_with("the recorded steps ran, but the signed-in check never appeared"), "{marker}");
    assert!(marker.ends_with("(link \"Kim\")"), "{marker}");
    assert_eq!(
        check_failure("sign-in stopped at step 2: no element matched #pass", false),
        "the check did not sign in - nothing was saved: sign-in stopped at step 2: no element matched #pass"
    );
}

// ---- the check -------------------------------------------------------------

/// The recipe `stateful_app` answers to, as a recording of it makes it.
fn recorded_recipe() -> SignInRecipe {
    let d = Draft {
        organization: "acme".into(),
        project: "Web".into(),
        start_url: "https://hr.example.internal/".into(),
        steps: vec![
            Step::Field { target: css("#user"), password: false },
            Step::Field { target: css("#pass"), password: true },
            Step::Click(css("#go")),
        ],
        marker: Some(css("#marker")),
    };
    d.recipe(&[choice(FieldRole::Username, ""), choice(FieldRole::Password, "")], None).unwrap()
}

#[tokio::test]
async fn the_check_signs_in_through_the_recorded_steps_never_a_saved_session() {
    let dir = tempfile::tempdir().unwrap();
    let saved = SavedSession {
        saved_at_ms: now_ms(),
        cookies: vec![json!({ "name": "sid", "value": "abc", "domain": "hr.example.internal", "path": "/", "session": true })],
        local_storage: vec![],
    };
    save_session(dir.path(), "admin", &saved).unwrap();
    // A saved session this app would accept: the check must not use it.
    let (mut d, state) = stateful_app(true, None);
    check_sign_in(&mut d, dir.path(), &recorded_recipe(), &account(), &quick()).await.unwrap();
    assert!(!state.restored.load(Ordering::SeqCst), "no saved session was put back");
    assert!(state.typed_password.load(Ordering::SeqCst), "the password was typed");
    assert!(d.calls_to("Network.setCookies").is_empty());
}

#[tokio::test]
async fn a_check_whose_step_finds_nothing_says_which_step_and_never_the_password() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, _state) = stateful_app(false, Some("#pass"));
    let err = check_sign_in(&mut d, dir.path(), &recorded_recipe(), &account(), &quick()).await.unwrap_err();
    assert!(err.starts_with("the check did not sign in - nothing was saved: sign-in stopped at step 2"), "{err}");
    assert!(!err.contains(PASSWORD) && !err.contains("://"), "{err}");
}

// ---- the kept draft, and the recorder's one slot ---------------------------

/// Everything that touches the process-wide draft or the recorder's slot,
/// in one test under the suite's autorun lock.
#[tokio::test]
async fn the_draft_is_kept_per_project_and_a_sign_in_recording_shares_the_recorders_cancel() {
    let _claims = crate::serial::autorun();
    let dir = tempfile::tempdir().unwrap();

    forget_draft();
    assert_eq!(build_recipe(dir.path(), "acme", "Web", &roles()).unwrap_err(), NO_DRAFT);
    keep_draft(draft());
    assert_eq!(build_recipe(dir.path(), "acme", "Other", &roles()).unwrap_err(), OTHER_PROJECT);
    let fresh = build_recipe(dir.path(), "acme", "Web", &roles()).unwrap();
    assert_eq!((fresh.session_minutes, fresh.after_sign_in.len()), (480, 0), "defaults without a recipe");

    // With a recipe saved, its other settings are kept.
    let mut saved = common::recipe();
    saved.after_sign_in = vec![click(css("#menu"))];
    saved.session_minutes = 60;
    save_recipe(dir.path(), "acme", "Web", &saved).unwrap();
    let kept = build_recipe(dir.path(), "acme", "Web", &roles()).unwrap();
    assert_eq!((kept.session_minutes, kept.after_sign_in.clone()), (60, saved.after_sign_in.clone()));
    assert_eq!(load_recipe(dir.path(), "acme", "Web").unwrap(), Some(saved), "building saves nothing");
    assert!(current_draft().is_some(), "and the draft is still there for another try");
    forget_draft();

    // Pick and Stop with nothing open say so.
    assert!(auto_run_record_sign_in_pick().await.is_err());
    assert!(auto_run_record_sign_in_stop().await.is_err());

    // A Cancel pressed while Start is still opening wins: nothing opens,
    // the recording browser is closed and the recorder is free.
    let claim = RecorderClaim::claim().expect("nothing is recording");
    auto_run_record_cancel().await.unwrap();
    let (closed, spawned) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
    let err = open_the_recording(claim, about(), fake_recording(ClosesOnDrop(closed.clone()), spawned.clone()))
        .await
        .unwrap_err();
    assert_eq!(err, CANCELLED);
    assert!(!spawned.load(Ordering::SeqCst) && closed.load(Ordering::SeqCst));
    assert!(!recording_is_going() && !sign_in_recording_is_open().await);

    // Once open, it holds the recorder; Pick reaches it; Cancel ends it.
    let claim = RecorderClaim::claim().expect("free again");
    let (closed, picked) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
    open_the_recording(claim, about(), fake_recording_seeing_pick(ClosesOnDrop(closed.clone()), picked.clone()))
        .await
        .expect("nothing cancelled this one");
    assert!(recording_is_going() && sign_in_recording_is_open().await);
    assert!(RecorderClaim::claim().is_none(), "a module recording has to wait");
    auto_run_record_sign_in_pick().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !picked.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the recording heard the pick");
    auto_run_record_cancel().await.unwrap();
    assert!(closed.load(Ordering::SeqCst), "its browser is closed");
    assert!(!recording_is_going() && !sign_in_recording_is_open().await);

    // Stop hands back the draft in words and keeps the locators.
    let claim = RecorderClaim::claim().expect("free again");
    open_the_recording(claim, about(), move |claim, stop, _cancel, _pick| -> Listening {
        tokio::spawn(async move {
            while !stop.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            drop(claim);
            let d = draft();
            Captured { steps: d.steps, marker: d.marker, ended: Ended::Stopped { href: String::new() } }
        })
    })
    .await
    .unwrap();
    let view = auto_run_record_sign_in_stop().await.unwrap();
    assert_eq!(view, draft().view());
    let kept = current_draft().expect("kept for the save");
    assert_eq!((kept.organization.as_str(), kept.project.as_str(), kept.steps.len()), ("acme", "Web", 5));
    assert!(!recording_is_going());
    forget_draft();

    // The app exiting ends an open sign-in recording the way Cancel does.
    let claim = RecorderClaim::claim().expect("free again");
    let (closed, spawned) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
    open_the_recording(claim, about(), fake_recording(ClosesOnDrop(closed.clone()), spawned.clone())).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), close_autorun_browsers()).await.expect("closing on exit is bounded");
    assert!(closed.load(Ordering::SeqCst));
    assert!(!recording_is_going() && !sign_in_recording_is_open().await);
}

fn about() -> SignInFor {
    SignInFor { organization: "acme".into(), project: "Web".into(), start_url: "https://hr.example.internal/".into() }
}

/// Stands in for the recording browser: says when it has been closed.
struct ClosesOnDrop(Arc<AtomicBool>);

impl Drop for ClosesOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// The listening task, as Start spawns it, minus the page: it owns the
/// browser and the claim, and waits for Cancel.
fn fake_recording(
    browser: ClosesOnDrop,
    spawned: Arc<AtomicBool>,
) -> impl FnOnce(RecorderClaim, Arc<AtomicBool>, Arc<AtomicBool>, Arc<AtomicBool>) -> Listening {
    move |claim, _stop, cancel, _pick| {
        spawned.store(true, Ordering::SeqCst);
        tokio::spawn(async move {
            while !cancel.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            drop(browser);
            drop(claim);
            Captured { steps: vec![], marker: None, ended: Ended::Cancelled }
        })
    }
}

/// The same, noting when pick mode is asked for.
fn fake_recording_seeing_pick(
    browser: ClosesOnDrop,
    picked: Arc<AtomicBool>,
) -> impl FnOnce(RecorderClaim, Arc<AtomicBool>, Arc<AtomicBool>, Arc<AtomicBool>) -> Listening {
    move |claim, _stop, cancel, pick| {
        tokio::spawn(async move {
            while !cancel.load(Ordering::SeqCst) {
                if pick.load(Ordering::SeqCst) {
                    picked.store(true, Ordering::SeqCst);
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            drop(browser);
            drop(claim);
            Captured { steps: vec![], marker: None, ended: Ended::Cancelled }
        })
    }
}
