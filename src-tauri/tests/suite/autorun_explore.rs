//! An explore session: a discovery that signs in as a saved account, files
//! nothing in the discovery map, and keeps its own change list file under
//! `<Auto Run root>/explore/`. Its end names the saves its browser sent that
//! no change on the list names.
//!
//! The real browser is never opened here: the `_in` functions run against
//! the fake browser slot `autorun_discovery` builds.

use crate::autorun_discovery::{
    exploring, leave_page, mapped_area, opened_for_discovery, parsed, root_with_recipe_and_account, sent, signin_app,
    slot, FakeBrowser, ORG, PROJECT,
};
use crate::api_templates_runner::{answer, Rig, ORG as RUN_ORG, PROJECT as RUN_PROJECT};
use crate::autorun_cleanup::{cleanup_rig, deleted_ids, status_of};
use crate::common::quick;
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use v2_lib::ai_bridge::{
    discover_action_in, discover_start_in, end_discovery_closing, end_discovery_in, explore_changes, explore_undo_in,
    page_read_in, record_change_in, BridgeContext, API_WRITES_OFF, ENDED_FOR_UNDO, EXPLORE_NOT_MAPPING,
    EXPLORE_NOT_STARTED, READ_WITH_NO_AREA,
};
use v2_lib::autorun::cleanup::{not_this_sessions, ran_in_project};
use v2_lib::autorun::discovery_map::map_path;
use v2_lib::autorun::cleanup::EXPLORE_MADE;
use v2_lib::autorun::explore::{
    begin, change_from_args, load, mark_create_undone, mark_undone, no_such_entry, note_writes, record_change,
    safe_session_id, session_id, session_path, unaccounted, undone_by_the_tool, unknown_session, verify_create,
    Change, ChangeEntry, ExploreRun, ExploreSession, SeenWrite, BEFORE_NOT_KNOWN, CHANGES_UNREADABLE, DELETED_STAYS,
    NOT_VERIFIED, NO_EXPLORE_SESSION, SECRET_NOT_KEPT, UNRECORDED_SAVES,
};
use v2_lib::autorun::test_made;
use v2_lib::browser::net_capture::{Body, Captured};
use v2_lib::autorun::roles::{RULES_DISCOVERY, RULES_EXPLORE};
use v2_lib::autorun::store::set_root;
use v2_lib::browser::actions::Action;
use v2_lib::browser::snapshot::DEFAULT_LIMIT;
use v2_lib::commands::autorun::DiscoveryState;

/// A discovery opened for an explore session, its change list not made yet.
fn explore_opened() -> Option<DiscoveryState> {
    opened_for_discovery().map(|s| DiscoveryState { explore: Some(ExploreRun::default()), ..s })
}

/// A going explore session on the area `Leave`, whose change list `session`
/// names.
fn explore_going(session: &str) -> Option<DiscoveryState> {
    exploring("Leave")
        .map(|s| DiscoveryState { explore: Some(ExploreRun { session: Some(session.to_string()), ..ExploreRun::default() }), ..s })
}

/// Whether `id` has an explore session's shape for the account `admin`.
fn is_admin_session(id: &str) -> bool {
    let b = id.as_bytes();
    id.len() == "00000000-000000-admin".len()
        && b[..8].iter().all(u8::is_ascii_digit)
        && b[8] == b'-'
        && b[9..15].iter().all(u8::is_ascii_digit)
        && id.ends_with("-admin")
}

/// A going explore session's browser whose page sends one save when
/// clicked, and the action that clicks.
fn browser_that_saves(session: &str) -> Option<FakeBrowser> {
    let mut d = leave_page("Input.dispatchMouseEvent", "https://hr.example.internal/hr/leave/list?page=2");
    d.on_call_events
        .push(("Input.dispatchMouseEvent".into(), sent("1", "POST", "https://hr.example.internal/hr/leave/save?id=5")));
    slot(d, explore_going(session)).0
}

#[tokio::test]
async fn an_explore_start_signs_in_files_nothing_and_opens_a_change_list() {
    let dir = root_with_recipe_and_account();
    let (mut browser, closed) = slot(signin_app(true), explore_opened());

    let (status, body) =
        discover_start_in(&mut browser, dir.path(), ORG, PROJECT, "admin", Some("Leave"), &quick()).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["signed_in"], true, "{body}");
    let id = v["explore"]["session"].as_str().unwrap_or_else(|| panic!("no session in {body}")).to_string();
    assert!(is_admin_session(&id), "{id}");
    assert!(!closed.load(Ordering::SeqCst));

    let state = browser.as_ref().unwrap().discovery.as_ref().expect("the discovery is under way");
    assert_eq!(state.explore_session(), Some(id.as_str()));
    assert_eq!(state.area.as_deref(), Some("Leave"), "an area still moves the state");
    assert!(!map_path(dir.path(), ORG, PROJECT).exists(), "an explore start filed something in the map");

    let s = load(dir.path(), &id).unwrap();
    assert_eq!(s.account, "admin");
    assert_eq!((s.organization.as_str(), s.project.as_str()), (ORG, PROJECT));
    assert_eq!(s.environment, v2_lib::environments::active(dir.path()).unwrap().id);
    assert!(s.changes.is_empty() && s.writes.is_empty() && s.ended_at.is_none());
    let file = std::fs::read_to_string(session_path(dir.path(), &id)).unwrap();
    assert!(!file.contains("kim"), "a login reached the change list: {file}");
}

#[tokio::test]
async fn an_explore_start_whose_change_list_cannot_be_made_does_not_start() {
    let dir = root_with_recipe_and_account();
    // A file where the folder belongs: the change list cannot be made.
    std::fs::write(dir.path().join("explore"), "x").unwrap();
    let (mut browser, closed) = slot(signin_app(true), explore_opened());

    let (status, body) = discover_start_in(&mut browser, dir.path(), ORG, PROJECT, "admin", None, &quick()).await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body, EXPLORE_NOT_STARTED);
    assert!(closed.load(Ordering::SeqCst), "the browser was left open");
    assert!(browser.is_none());
}

#[tokio::test]
async fn an_explore_page_read_is_quiet() {
    let dir = root_with_recipe_and_account();
    let (mut browser, _) = slot(signin_app(true), explore_opened());
    let browser = browser.as_mut().unwrap();

    let (status, text) = page_read_in(browser, Some(dir.path()), ORG, PROJECT, None, DEFAULT_LIMIT).await;
    assert_eq!(status, 200, "{text}");
    assert!(text.contains("button \"Save\""), "{text}");
    assert!(!text.contains(READ_WITH_NO_AREA) && !text.contains("Recorded"), "{text}");
    assert!(!map_path(dir.path(), ORG, PROJECT).exists(), "an explore page read filed something");
}

#[tokio::test]
async fn an_explore_action_files_nothing_and_notes_its_writes() {
    let dir = root_with_recipe_and_account();
    let id = begin(dir.path(), ORG, PROJECT, "admin", 1_760_000_000_000).unwrap();
    let mut browser = browser_that_saves(&id);

    let save = Action::Click { selector: "#save".into() };
    let (status, body) =
        discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &save, None, Some("Leave Requests")).await;
    assert_eq!(status, 200, "{body}");
    let v = parsed(&body);
    assert_eq!(v["ok"], true, "{body}");
    assert_eq!(v["writes"], json!([{ "method": "POST", "path": "/hr/leave/save" }]), "{body}");
    assert!(!map_path(dir.path(), ORG, PROJECT).exists(), "an explore action filed something in the map");
    assert!(mapped_area(dir.path(), "Leave Requests").is_none());

    let s = load(dir.path(), &id).unwrap();
    assert_eq!(s.writes.len(), 1, "{:?}", s.writes);
    assert_eq!((s.writes[0].method.as_str(), s.writes[0].path.as_str()), ("POST", "/hr/leave/save"));
    let file = std::fs::read_to_string(session_path(dir.path(), &id)).unwrap();
    assert!(!file.contains("id=5"), "a query string reached the change list: {file}");
}

#[tokio::test]
async fn an_explore_action_still_works_when_the_change_list_will_not_take_its_writes() {
    let dir = root_with_recipe_and_account();
    let id = begin(dir.path(), ORG, PROJECT, "admin", 1_760_000_000_000).unwrap();
    let mut browser = browser_that_saves(&id);
    std::fs::write(session_path(dir.path(), &id), "{ not json").unwrap();

    let save = Action::Click { selector: "#save".into() };
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &save, None, None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(parsed(&body)["ok"], true, "{body}");
    // The file that did not read is left as it was.
    assert_eq!(std::fs::read_to_string(session_path(dir.path(), &id)).unwrap(), "{ not json");
}

#[tokio::test]
async fn ending_an_explore_session_names_the_saves_not_on_the_change_list() {
    let dir = root_with_recipe_and_account();
    let _g = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let save = Action::Click { selector: "#save".into() };

    let id = begin(dir.path(), ORG, PROJECT, "admin", 1_760_000_000_000).unwrap();
    let mut browser = browser_that_saves(&id);
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &save, None, None).await;
    assert_eq!(status, 200, "{body}");
    let (status, out) = end_discovery_in(&mut browser).await;
    assert_eq!(status, 200, "{out}");
    let v = parsed(&out);
    assert_eq!(v["explore"]["session"], id.as_str(), "{out}");
    assert_eq!(v["explore"]["changes"], 0, "{out}");
    assert_eq!(v["explore"]["saves_not_on_the_change_list"], json!(["POST /hr/leave/save"]), "{out}");
    assert_eq!(v["note"], UNRECORDED_SAVES, "{out}");
    assert!(load(dir.path(), &id).unwrap().ended_at.is_some());
    assert!(browser.as_ref().unwrap().discovery.is_none(), "the discovery was not ended");

    // The same when the browser is closed with it.
    let id = begin(dir.path(), ORG, PROJECT, "admin", 1_760_000_001_000).unwrap();
    let mut browser = browser_that_saves(&id);
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &save, None, None).await;
    assert_eq!(status, 200, "{body}");
    let (status, out) = end_discovery_closing(&mut browser);
    assert_eq!(status, 200, "{out}");
    let v = parsed(&out);
    assert_eq!(v["explore"]["saves_not_on_the_change_list"], json!(["POST /hr/leave/save"]), "{out}");
    assert_eq!(v["note"], UNRECORDED_SAVES, "{out}");
    assert!(load(dir.path(), &id).unwrap().ended_at.is_some());
}

#[tokio::test]
async fn ending_an_explore_session_with_every_save_listed_has_no_note() {
    let dir = root_with_recipe_and_account();
    let _g = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let id = begin(dir.path(), ORG, PROJECT, "admin", 1_760_000_000_000).unwrap();
    let mut browser = browser_that_saves(&id);

    let (status, out) = end_discovery_in(&mut browser).await;
    assert_eq!(status, 200, "{out}");
    let v = parsed(&out);
    assert_eq!(v["explore"]["saves_not_on_the_change_list"], json!([]), "{out}");
    assert!(v.get("note").is_none(), "{out}");
    // Ending again is still an answer, and the session ended once.
    let ended = load(dir.path(), &id).unwrap().ended_at;
    assert!(ended.is_some());
    let (status, _) = end_discovery_in(&mut browser).await;
    assert_eq!(status, 200);
    assert_eq!(load(dir.path(), &id).unwrap().ended_at, ended);
}

/// Review Focus 4: two sessions started in the same second as the same
/// account never share a file.
#[test]
fn two_sessions_in_the_same_second_never_share_a_file() {
    let dir = root_with_recipe_and_account();
    let ms = 1_760_000_000_000;
    let first = begin(dir.path(), ORG, PROJECT, "admin", ms).unwrap();
    let second = begin(dir.path(), ORG, PROJECT, "admin", ms).unwrap();
    assert_ne!(first, second);
    assert_eq!(second, format!("{first}-2"));
    assert_eq!(load(dir.path(), &first).unwrap().session, first);
    assert_eq!(load(dir.path(), &second).unwrap().session, second);
    let third = begin(dir.path(), ORG, PROJECT, "admin", ms).unwrap();
    assert_eq!(third, format!("{first}-3"));
}

/// Review Focus 4: an account key with characters a file name cannot hold.
#[test]
fn an_account_key_is_made_safe_for_the_file_name() {
    let dir = root_with_recipe_and_account();
    let id = session_id(1_760_000_000_000, "hr admin/../1");
    assert!(id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')), "{id}");
    assert!(!id.contains(".."), "{id}");
    assert!(safe_session_id(&id));
    for unsafe_id in ["../x", "a/b", "a\\b", "", "x..y", &"a".repeat(121)] {
        assert!(!safe_session_id(unsafe_id), "{unsafe_id}");
    }
    assert_eq!(load(dir.path(), "../x"), Err(unknown_session("../x")));
    // A key made of nothing a name can hold still makes a name.
    assert!(safe_session_id(&session_id(1_760_000_000_000, "")));
    // The file is under explore/, whatever the key was.
    let made = begin(dir.path(), ORG, PROJECT, "hr admin/../1", 1_760_000_000_000).unwrap();
    assert!(session_path(dir.path(), &made).starts_with(dir.path().join("explore")));
    assert!(session_path(dir.path(), &made).is_file());
    assert_eq!(load(dir.path(), &made).unwrap().account, "hr admin/../1");
}

fn entry(n: u32, request: Option<&str>) -> ChangeEntry {
    ChangeEntry {
        n,
        at: "2026-10-22T00:00:00.000Z".into(),
        change: Change::Created { kind: "leave".into(), id: "7".into(), name: "x".into(), verified: true },
        request: request.map(str::to_string),
        undone: false,
    }
}

fn session_with(writes: &[(&str, &str)], changes: Vec<ChangeEntry>) -> ExploreSession {
    ExploreSession {
        session: "20251009-081320-admin".into(),
        organization: ORG.into(),
        project: PROJECT.into(),
        environment: "env-0".into(),
        environment_name: "Default".into(),
        account: "admin".into(),
        started_at: "2025-10-09T08:13:20.000Z".into(),
        ended_at: None,
        changes,
        writes: writes
            .iter()
            .map(|(m, p)| SeenWrite { method: m.to_string(), path: p.to_string(), at: "t".into() })
            .collect(),
    }
}

#[test]
fn a_write_a_change_names_is_accounted_for() {
    let s = session_with(&[("POST", "/a/b/"), ("POST", "/c")], vec![entry(1, Some("post /A/b"))]);
    assert_eq!(unaccounted(&s), vec!["POST /c".to_string()]);

    // Each save is listed as it was sent, in order, once for every time it was sent.
    let s = session_with(&[("POST", "/c"), ("POST", "/d"), ("post", "/c/")], vec![entry(1, None)]);
    assert_eq!(unaccounted(&s), vec!["POST /c".to_string(), "POST /d".to_string(), "POST /c/".to_string()]);

    // A change with no request names none.
    let s = session_with(&[("POST", "/c")], vec![]);
    assert_eq!(unaccounted(&s), vec!["POST /c".to_string()]);
}

#[test]
fn a_change_list_round_trips_through_its_file() {
    let changed = ChangeEntry {
        n: 2,
        at: "2026-10-22T00:00:01.000Z".into(),
        change: Change::Changed {
            record: "leave 7".into(),
            field: "status".into(),
            before: Some(json!("Open")),
            after: None,
            hidden: false,
        },
        request: Some("POST /a".into()),
        undone: false,
    };
    let deleted = ChangeEntry {
        n: 3,
        at: "2026-10-22T00:00:02.000Z".into(),
        change: Change::Deleted { kind: "leave".into(), id: "7".into(), name: "x".into(), held: Some(json!({"a": 1})) },
        request: None,
        undone: true,
    };
    let s = session_with(&[("POST", "/a")], vec![entry(1, Some("POST /a")), changed, deleted]);
    let text = serde_json::to_string_pretty(&s).unwrap();
    assert!(text.contains("\"change\": \"created\"") && text.contains("\"change\": \"deleted\""), "{text}");
    let back: ExploreSession = serde_json::from_str(&text).unwrap();
    assert_eq!(back, s);
}

/// An explore start answers with the explore rules at its end, whatever its
/// status, and never the discovery rules; asking to map as well is refused.
#[tokio::test]
async fn an_explore_start_ends_with_the_explore_rules() {
    let dir = root_with_recipe_and_account();
    let _g = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let ctx = crate::autorun_discovery::ctx();

    let body = json!({ "account": "nobody-saved-this", "explore": true }).to_string();
    let (status, out) = v2_lib::ai_bridge::route(&ctx, None, "POST", "/autorun-discover-start", &body, "1.0.0").await;
    assert!(status >= 400, "{status}: {out}");
    assert!(out.ends_with(RULES_EXPLORE), "{out}");
    assert!(!out.contains(RULES_DISCOVERY), "{out}");

    let body = json!({ "account": "x", "explore": true, "mapping": true }).to_string();
    let answer = v2_lib::ai_bridge::route(&ctx, None, "POST", "/autorun-discover-start", &body, "1.0.0").await;
    assert_eq!(answer, (400, format!("{EXPLORE_NOT_MAPPING}\n\n{RULES_EXPLORE}")));

    // A start that does not explore keeps the discovery rules.
    let body = json!({ "account": "nobody-saved-this" }).to_string();
    let (_, out) = v2_lib::ai_bridge::route(&ctx, None, "POST", "/autorun-discover-start", &body, "1.0.0").await;
    assert!(out.ends_with(RULES_DISCOVERY) && !out.contains(RULES_EXPLORE), "{out}");
}

// ---- The change list: record, verify, read ----

/// A save the page made, answered with `body`.
fn save(method: &str, path: &str, started_ms: u64, body: Option<Body>) -> Captured {
    Captured {
        method: method.to_string(),
        url_path_query: path.to_string(),
        started_ms,
        status: Some(200),
        resp_body: body,
        ..Default::default()
    }
}

fn text(t: &str) -> Option<Body> {
    Some(Body::Text(t.to_string()))
}

fn created(kind: &str, id: &str, name: &str) -> Change {
    Change::Created { kind: kind.into(), id: id.into(), name: name.into(), verified: false }
}

fn changed(field: &str, before: Option<&str>, after: &str) -> Change {
    Change::Changed {
        record: "Cycle 12".into(),
        field: field.into(),
        before: before.map(|b| json!(b)),
        after: Some(json!(after)),
        hidden: false,
    }
}

/// A fresh session's id in a fresh root.
fn started() -> (crate::autorun_discovery::TempDir, String) {
    let dir = root_with_recipe_and_account();
    let id = begin(dir.path(), ORG, PROJECT, "admin", 1_760_000_000_000).unwrap();
    (dir, id)
}

/// Review Focus 2.
#[test]
fn a_browser_create_is_verified_only_by_a_save_this_session_made() {
    let since = 1000;
    let post = save("POST", "/hr/c?x=1", 2000, text("{\"data\":{\"Id\":274}}"));
    assert_eq!(verify_create(&[&post], since, "274", &[]), Some("POST /hr/c".to_string()));

    // The same response on a read, or on a save from before the session.
    let get = save("GET", "/hr/c", 2000, text("{\"data\":{\"Id\":274}}"));
    assert_eq!(verify_create(&[&get], since, "274", &[]), None);
    let old = save("POST", "/hr/c", 500, text("{\"data\":{\"Id\":274}}"));
    assert_eq!(verify_create(&[&old], since, "274", &[]), None);
    // A save that started exactly when the session did counts.
    let at = save("POST", "/hr/c", 1000, text("{\"Id\":274}"));
    assert!(verify_create(&[&at], since, "274", &[]).is_some());

    // A longer number is not the id.
    let longer = save("POST", "/hr/c", 2000, text("{\"Id\":1274}"));
    assert_eq!(verify_create(&[&longer], since, "274", &[]), None);
    let in_text = save("POST", "/hr/c", 2000, text("saved 1274."));
    assert_eq!(verify_create(&[&in_text], since, "274", &[]), None);

    // A number or a string, either way.
    let string = save("POST", "/hr/c", 2000, text("{\"Id\":\"274\"}"));
    assert!(verify_create(&[&string], since, "274", &[]).is_some());
    let number = save("POST", "/hr/c", 2000, text("{\"Id\":274}"));
    assert!(verify_create(&[&number], since, "274", &[]).is_some());
    let nested = save("PUT", "/hr/c", 2000, text("{\"rows\":[{\"a\":1},{\"b\":[274]}]}"));
    assert!(verify_create(&[&nested], since, "274", &[]).is_some());

    // A text body: the id as a whole word.
    let sentence = save("POST", "/hr/c", 2000, text("saved 274."));
    assert!(verify_create(&[&sentence], since, "274", &[]).is_some());

    // The id as a key, or inside a longer string, is not the id.
    let key = save("POST", "/hr/c", 2000, text("{\"274\":true}"));
    assert_eq!(verify_create(&[&key], since, "274", &[]), None);
    let inside = save("POST", "/hr/c", 2000, text("{\"msg\":\"record 274 saved\"}"));
    assert_eq!(verify_create(&[&inside], since, "274", &[]), None);

    // No body, a body that was dropped, and a save that failed verify nothing.
    let none = save("POST", "/hr/c", 2000, None);
    assert_eq!(verify_create(&[&none], since, "274", &[]), None);
    let dropped = save("POST", "/hr/c", 2000, Some(Body::Dropped));
    assert_eq!(verify_create(&[&dropped], since, "274", &[]), None);
    let mut failed = save("POST", "/hr/c", 2000, text("{\"error\":\"no record 274\"}"));
    failed.status = Some(500);
    assert_eq!(verify_create(&[&failed], since, "274", &[]), None);
    let mut failed = save("POST", "/hr/c", 2000, text("{\"Id\":274}"));
    failed.status = Some(409);
    assert_eq!(verify_create(&[&failed], since, "274", &[]), None);
    let mut ok = save("POST", "/hr/c", 2000, text("{\"Id\":274}"));
    ok.status = Some(201);
    assert!(verify_create(&[&ok], since, "274", &[]).is_some());

    // A delete never creates, whatever it answers with.
    let delete = save("DELETE", "/hr/c/274", 2000, text("{\"Id\":274}"));
    assert_eq!(verify_create(&[&delete], since, "274", &[]), None);

    // No id, no verification; the earliest matching save names the request.
    assert_eq!(verify_create(&[&number], since, "  ", &[]), None);
    let later = save("POST", "/hr/later", 3000, text("{\"Id\":274}"));
    assert_eq!(verify_create(&[&later, &number], since, "274", &[]), Some("POST /hr/c".to_string()));
}

#[test]
fn a_verified_create_goes_on_the_test_made_list_and_an_unverified_one_does_not() {
    let (dir, session) = started();
    let post = save("POST", "/hr/cycles", 2000, text("{\"data\":{\"Id\":274}}"));
    let env = v2_lib::environments::active(dir.path()).unwrap().id;

    let ok = record_change(dir.path(), &session, created("cycle", "274", "Sprint 12"), None, &[&post], 1000).unwrap();
    assert_eq!((ok.n, ok.verified, ok.note.as_deref()), (1, Some(true), None));
    let made = test_made::list(dir.path());
    assert_eq!(made.len(), 1, "{made:?}");
    let m = &made[0];
    assert_eq!((m.kind.as_str(), m.id.as_str(), m.name.as_str()), ("cycle", "274", "Sprint 12"));
    assert_eq!(m.fixture, EXPLORE_MADE);
    assert_eq!(m.run_id, session);
    assert_eq!(m.environment, env);
    assert_eq!(m.status, test_made::PRESENT);
    // The entry names the save that proved it, not what the caller said.
    let s = load(dir.path(), &session).unwrap();
    assert_eq!(s.changes[0].request.as_deref(), Some("POST /hr/cycles"));
    assert!(matches!(&s.changes[0].change, Change::Created { verified: true, .. }));

    let no = record_change(dir.path(), &session, created("cycle", "999", "Ghost"), None, &[&post], 1000).unwrap();
    assert_eq!((no.n, no.verified), (2, Some(false)));
    assert_eq!(no.note.as_deref(), Some(NOT_VERIFIED));
    assert_eq!(test_made::list(dir.path()).len(), 1, "an unverified create reached the test-made list");
    let s = load(dir.path(), &session).unwrap();
    assert!(matches!(&s.changes[1].change, Change::Created { verified: false, .. }));

    // The same create told twice is on the test-made list once.
    record_change(dir.path(), &session, created("cycle", "274", "Sprint 12"), None, &[&post], 1000).unwrap();
    assert_eq!(test_made::list(dir.path()).len(), 1);
}

#[test]
fn a_change_is_kept_in_order_with_what_it_lacks_said() {
    let (dir, session) = started();
    let first =
        record_change(dir.path(), &session, changed("Status", Some("Draft"), "Published"), None, &[], 0).unwrap();
    assert_eq!((first.n, first.verified, first.note), (1, None, None));
    let second = record_change(
        dir.path(),
        &session,
        changed("Name", None, "Sprint 13"),
        Some("POST https://hr.example.internal/hr/cycles/274?a=1".into()),
        &[],
        0,
    )
    .unwrap();
    assert_eq!(second.n, 2);
    assert_eq!(second.note.as_deref(), Some(BEFORE_NOT_KNOWN));
    let deleted = Change::Deleted {
        kind: "cycle".into(),
        id: "9".into(),
        name: "Old".into(),
        held: Some(json!({ "Name": "Old", "Password": "hunter2" })),
    };
    let third = record_change(dir.path(), &session, deleted, None, &[], 0).unwrap();
    assert_eq!((third.n, third.note.as_deref()), (3, Some(DELETED_STAYS)));

    let s = load(dir.path(), &session).unwrap();
    assert_eq!(s.changes.iter().map(|c| c.n).collect::<Vec<_>>(), [1, 2, 3]);
    // A request keeps its method and path only.
    assert_eq!(s.changes[1].request.as_deref(), Some("POST /hr/cycles/274"));
    let file = std::fs::read_to_string(session_path(dir.path(), &session)).unwrap();
    assert!(!file.contains("hunter2") && !file.contains("hr.example.internal"), "{file}");
    assert!(file.contains("\"Name\": \"Old\""), "what a delete held was not kept: {file}");
    // A delete is never put on the test-made list.
    assert!(test_made::list(dir.path()).is_empty());
}

#[test]
fn a_secret_field_keeps_neither_value() {
    let (dir, session) = started();
    let out = record_change(dir.path(), &session, changed("Password", Some("a"), "b"), None, &[], 0).unwrap();
    assert_eq!(out.note.as_deref(), Some(SECRET_NOT_KEPT));
    let s = load(dir.path(), &session).unwrap();
    match &s.changes[0].change {
        Change::Changed { before, after, hidden, field, .. } => {
            assert_eq!((before, after, *hidden), (&None, &None, true));
            assert_eq!(field, "Password");
        }
        other => panic!("{other:?}"),
    }
    let file = std::fs::read_to_string(session_path(dir.path(), &session)).unwrap();
    assert!(!file.contains("\"a\"") && !file.contains("\"b\""), "a secret reached the change list: {file}");
}

/// Review Focus 5.
#[test]
fn a_change_list_that_does_not_read_is_never_overwritten() {
    let (dir, session) = started();
    std::fs::write(session_path(dir.path(), &session), "not json").unwrap();
    let out = record_change(dir.path(), &session, changed("Status", Some("a"), "b"), None, &[], 0);
    assert_eq!(out, Err(CHANGES_UNREADABLE.to_string()));
    assert_eq!(mark_undone(dir.path(), &session, &[1]), Err(CHANGES_UNREADABLE.to_string()));
    assert_eq!(std::fs::read(session_path(dir.path(), &session)).unwrap(), b"not json");
    // A create that would have been verified left nothing on the test-made list either.
    let post = save("POST", "/hr/c", 2000, text("{\"Id\":5}"));
    let out = record_change(dir.path(), &session, created("cycle", "5", "x"), None, &[&post], 0);
    assert_eq!(out, Err(CHANGES_UNREADABLE.to_string()));
    assert!(test_made::list(dir.path()).is_empty());
    assert_eq!(std::fs::read(session_path(dir.path(), &session)).unwrap(), b"not json");
}

#[test]
fn undone_marks_entries_and_leaves_a_verified_create_to_the_tool() {
    let (dir, session) = started();
    let post = save("POST", "/hr/cycles", 2000, text("{\"Id\":274}"));
    record_change(dir.path(), &session, changed("Status", Some("Draft"), "Published"), None, &[], 0).unwrap();
    record_change(dir.path(), &session, created("cycle", "274", "Sprint"), None, &[&post], 0).unwrap();
    record_change(dir.path(), &session, created("cycle", "999", "Ghost"), None, &[], 0).unwrap();
    let undone = |n: u32| load(dir.path(), &session).unwrap().changes[n as usize - 1].undone;

    assert_eq!(mark_undone(dir.path(), &session, &[1]), Ok(()));
    assert!(undone(1));
    assert_eq!(mark_undone(dir.path(), &session, &[2]), Err(undone_by_the_tool(2)));
    assert!(!undone(2));
    assert_eq!(mark_undone(dir.path(), &session, &[99]), Err(no_such_entry(99)));
    // One bad number refuses the lot.
    assert_eq!(mark_undone(dir.path(), &session, &[3, 99]), Err(no_such_entry(99)));
    assert!(!undone(3));
    // An unverified create was only ever in the browser: the assistant sets it back by hand.
    assert_eq!(mark_undone(dir.path(), &session, &[3]), Ok(()));
    assert!(undone(3));

    // The tool marks the verified create it removed.
    assert_eq!(mark_create_undone(dir.path(), &session, "cycle", "274"), Ok(true));
    assert!(undone(2));
    assert_eq!(mark_create_undone(dir.path(), &session, "cycle", "1"), Ok(false));
    assert_eq!(mark_undone(dir.path(), "../x", &[1]), Err(unknown_session("../x")));
}

#[tokio::test]
async fn a_change_needs_the_going_explore_session() {
    let (dir, session) = started();
    let body = json!({ "change": "created", "kind": "cycle", "id": "5", "name": "x" }).to_string();

    // A discovery that is not an explore session.
    let (mut browser, _) = slot(signin_app(true), exploring("Leave"));
    assert_eq!(record_change_in(&mut browser, dir.path(), ORG, PROJECT, &body), (409, NO_EXPLORE_SESSION.to_string()));
    // An explore session whose change list was never made.
    let (mut browser, _) = slot(signin_app(true), explore_opened());
    assert_eq!(record_change_in(&mut browser, dir.path(), ORG, PROJECT, &body), (409, NO_EXPLORE_SESSION.to_string()));
    // No browser at all.
    let mut none: Option<FakeBrowser> = None;
    assert_eq!(record_change_in(&mut none, dir.path(), ORG, PROJECT, &body), (409, NO_EXPLORE_SESSION.to_string()));
    assert!(load(dir.path(), &session).unwrap().changes.is_empty());

    // The going session takes it, and says the create is not verified (the fake browser captures nothing).
    let (mut browser, _) = slot(signin_app(true), explore_going(&session));
    let (status, out) = record_change_in(&mut browser, dir.path(), ORG, PROJECT, &body);
    assert_eq!(status, 200, "{out}");
    let v = parsed(&out);
    assert_eq!((v["n"].clone(), v["verified"].clone(), v["note"].clone()), (json!(1), json!(false), json!(NOT_VERIFIED)));

    // A change that is not one is refused with its shape.
    let (status, out) = record_change_in(&mut browser, dir.path(), ORG, PROJECT, &json!({ "change": "made" }).to_string());
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("created, changed or deleted"), "{out}");

    // A change list that does not read is a conflict.
    std::fs::write(session_path(dir.path(), &session), "nope").unwrap();
    assert_eq!(record_change_in(&mut browser, dir.path(), ORG, PROJECT, &body), (409, CHANGES_UNREADABLE.to_string()));

    // Entries set back by hand name their session and need no going one.
    let (dir, session) = started();
    let (mut browser, _) = slot(signin_app(true), explore_going(&session));
    let by_hand = json!({ "change": "changed", "record": "Cycle 1", "field": "Status", "before": "A", "after": "B" });
    assert_eq!(record_change_in(&mut browser, dir.path(), ORG, PROJECT, &by_hand.to_string()).0, 200);
    let mut none: Option<FakeBrowser> = None;
    let body = json!({ "session": session, "undone": [1] }).to_string();
    let (status, out) = record_change_in(&mut none, dir.path(), ORG, PROJECT, &body);
    assert_eq!(status, 200, "{out}");
    assert!(load(dir.path(), &session).unwrap().changes[0].undone);
    let body = json!({ "session": session, "undone": [7] }).to_string();
    assert_eq!(record_change_in(&mut none, dir.path(), ORG, PROJECT, &body), (400, no_such_entry(7)));
    let body = json!({ "session": "20200101-000000-nobody", "undone": [1] }).to_string();
    assert_eq!(record_change_in(&mut none, dir.path(), ORG, PROJECT, &body).0, 404);
}

#[test]
fn the_change_lists_come_newest_first_with_the_saves_they_miss() {
    let dir = root_with_recipe_and_account();
    let older = begin(dir.path(), ORG, PROJECT, "admin", 1_760_000_000_000).unwrap();
    let newer = begin(dir.path(), ORG, PROJECT, "admin", 1_760_000_500_000).unwrap();
    begin(dir.path(), "other-org", PROJECT, "admin", 1_760_000_900_000).unwrap();
    record_change(dir.path(), &newer, changed("Status", Some("A"), "B"), None, &[], 0).unwrap();
    record_change(dir.path(), &newer, changed("Name", Some("A"), "B"), None, &[], 0).unwrap();
    mark_undone(dir.path(), &newer, &[1]).unwrap();
    note_writes(dir.path(), &newer, &[("POST".into(), "/hr/cycles?x=1".into())]).unwrap();

    let (status, out) = explore_changes(dir.path(), ORG, PROJECT, "{}");
    assert_eq!(status, 200, "{out}");
    let v = parsed(&out);
    let sessions = v["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 2, "{out}");
    assert_eq!(sessions[0]["session"], newer.as_str());
    assert_eq!(sessions[1]["session"], older.as_str());
    assert_eq!((sessions[0]["changes"].clone(), sessions[0]["undone"].clone()), (json!(2), json!(1)));
    assert_eq!(sessions[0]["account"], "admin");
    assert_eq!(sessions[0]["environment"], v2_lib::environments::active(dir.path()).unwrap().name.as_str());
    assert!(sessions[0]["started_at"].is_string() && sessions[0]["ended_at"].is_null(), "{out}");

    let (status, out) = explore_changes(dir.path(), ORG, PROJECT, &json!({ "session": newer }).to_string());
    assert_eq!(status, 200, "{out}");
    let v = parsed(&out);
    assert_eq!(v["session"], newer.as_str());
    assert_eq!(v["changes"].as_array().unwrap().len(), 2);
    assert_eq!(v["saves_not_on_the_change_list"], json!(["POST /hr/cycles"]));
    assert!(!out.contains("x=1"), "a query string reached the answer: {out}");

    assert_eq!(
        explore_changes(dir.path(), ORG, PROJECT, &json!({ "session": "../x" }).to_string()),
        (404, unknown_session("../x"))
    );
    // A change list that does not read is a conflict, and the listing skips it.
    std::fs::write(session_path(dir.path(), &older), "nope").unwrap();
    assert_eq!(
        explore_changes(dir.path(), ORG, PROJECT, &json!({ "session": older }).to_string()),
        (409, CHANGES_UNREADABLE.to_string())
    );
    let v = parsed(&explore_changes(dir.path(), ORG, PROJECT, "{}").1);
    assert_eq!(v["sessions"].as_array().unwrap().len(), 1);
}

#[test]
fn a_change_args_shape_is_said() {
    let err = change_from_args(&json!({ "change": "made" })).unwrap_err();
    assert!(err.contains("created") && err.contains("changed") && err.contains("deleted"), "{err}");
    let err = change_from_args(&json!({})).unwrap_err();
    assert!(err.contains("created, changed or deleted"), "{err}");
    let err = change_from_args(&json!({ "change": "created", "kind": "cycle", "name": "x" })).unwrap_err();
    assert!(err.contains("kind, id and name"), "{err}");
    let err = change_from_args(&json!({ "change": "changed", "record": "r", "field": "f" })).unwrap_err();
    assert!(err.contains("record, field and after"), "{err}");
    let err = change_from_args(&json!({ "change": "deleted", "kind": "cycle", "id": "1" })).unwrap_err();
    assert!(err.contains("kind, id and name"), "{err}");

    // An id is a string or a number, and a create is never verified by its caller.
    assert_eq!(
        change_from_args(&json!({ "change": "Created", "kind": "cycle", "id": 274, "name": "x", "verified": true })),
        Ok(Change::Created { kind: "cycle".into(), id: "274".into(), name: "x".into(), verified: false })
    );
    assert_eq!(
        change_from_args(&json!({ "change": "changed", "record": "r", "field": "f", "before": null, "after": 3 })),
        Ok(Change::Changed {
            record: "r".into(),
            field: "f".into(),
            before: None,
            after: Some(json!(3)),
            hidden: false
        })
    );
    assert_eq!(
        change_from_args(&json!({ "change": "deleted", "kind": "k", "id": "1", "name": "n", "held": { "a": 1 } })),
        Ok(Change::Deleted { kind: "k".into(), id: "1".into(), name: "n".into(), held: Some(json!({ "a": 1 })) })
    );
}

// ---- Undo: the bridge route into Clean up ----

/// Every process-wide thing an undo touches: the Auto Run root (an ended
/// session's change list is closed under it), the template slot, the
/// activity log, the held browsers and the account leases.
fn undo_locks() -> impl Sized {
    (
        crate::serial::autorun(),
        crate::serial::api_template_run(),
        crate::serial::activity_log(),
        crate::serial::held_browsers(),
        crate::serial::account_leases(),
    )
}

/// A cleanup rig (a proven delete template for cycles, proven as `admin`)
/// and a saved explore session of `admin` that made, and verified, a cycle
/// for each of `ids`.
fn session_that_made(responses: Vec<Value>, ids: &[&str]) -> (Rig, String, String) {
    let (r, env) = cleanup_rig(responses, &[]);
    set_root(r.root.path().to_path_buf());
    let session = made_session(&r, ids, 1_760_000_000_000);
    (r, session, env)
}

/// Another explore session in `r`'s root, started at `started_ms`, that
/// made and verified a cycle for each of `ids`.
fn made_session(r: &Rig, ids: &[&str], started_ms: u64) -> String {
    made_session_in(r, RUN_ORG, RUN_PROJECT, ids, started_ms)
}

/// `made_session`, in the organisation `org` and the project `project`.
fn made_session_in(r: &Rig, org: &str, project: &str, ids: &[&str], started_ms: u64) -> String {
    let session = begin(r.root.path(), org, project, "admin", started_ms).unwrap();
    for id in ids {
        let post = save("POST", "/hr/c?x=1", 2000, text(&format!("{{\"Id\":{id}}}")));
        let recorded =
            record_change(r.root.path(), &session, created("cycle", id, &format!("AUTOTEST {id}")), None, &[&post], 1000)
                .unwrap();
        assert_eq!(recorded.verified, Some(true), "{recorded:?}");
    }
    session
}

/// An undo as the person approved it: person_said_yes is true unless the body says otherwise.
async fn undo_in(slot: &mut Option<FakeBrowser>, r: &mut Rig, writes: bool, body: Value) -> (u16, Value) {
    let mut body = body;
    if body.is_object() && body.get("person_said_yes").is_none() {
        body["person_said_yes"] = json!(true);
    }
    let ctx = BridgeContext { org: RUN_ORG.into(), project: RUN_PROJECT.into(), api_writes: writes, ..Default::default() };
    let (status, text) = explore_undo_in(slot, &mut r.browsers, &ctx, r.root.path(), &body.to_string(), &quick()).await;
    (status, serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

/// A going explore session of `account`, its browser holding `lease_key`.
async fn exploring_as(r: &Rig, session: &str, account: &str, lease_key: &str) -> (Option<FakeBrowser>, std::sync::Arc<std::sync::atomic::AtomicBool>) {
    let state = explore_going(session).map(|s| DiscoveryState { account: Some(account.to_string()), ..s });
    let (mut browser, closed) = slot(signin_app(true), state);
    browser.as_mut().unwrap().lease.hold(r.root.path(), lease_key).await.unwrap();
    (browser, closed)
}

#[tokio::test]
async fn undo_removes_this_sessions_verified_creates_and_marks_them_undone() {
    let _locks = undo_locks();
    let (mut r, session, env) = session_that_made(vec![answer(200, json!({ "success": true }))], &["274"]);
    let mut none: Option<FakeBrowser> = None;

    let (status, v) = undo_in(&mut none, &mut r, true, json!({ "session": session, "ids": ["274"] })).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["total"], json!(1), "{v}");
    assert_eq!(v["stopped"], json!(false), "{v}");
    assert_eq!(v["results"][0]["ok"], json!(true), "{v}");
    assert_eq!(v["results"][0]["id"], json!("274"), "{v}");
    assert!(v.get("ended_session").is_none(), "{v}");
    assert!(v.get("not_marked_undone").is_none(), "{v}");
    assert_eq!(deleted_ids(&r), vec![json!("274")]);
    assert_eq!(status_of(r.root.path(), &env, "cycle", "274"), "deleted");

    let s = load(r.root.path(), &session).unwrap();
    assert_eq!(s.changes.len(), 1);
    assert!(s.changes[0].undone, "{:?}", s.changes[0]);
}

#[tokio::test]
async fn undo_refuses_an_id_the_session_did_not_verify() {
    let _locks = undo_locks();
    let (mut r, session, env) = session_that_made(vec![], &["274"]);
    made_session(&r, &["275"], 1_760_000_100_000);
    let mut none: Option<FakeBrowser> = None;

    // Another session's create, and one the record never heard of.
    for id in ["275", "999"] {
        let (status, v) = undo_in(&mut none, &mut r, true, json!({ "session": session, "ids": [id] })).await;
        assert_eq!((status, v), (400, json!(not_this_sessions(id))), "{id}");
    }
    // One id that is not the session's refuses the whole undo.
    let (status, v) = undo_in(&mut none, &mut r, true, json!({ "session": session, "ids": ["274", "275"] })).await;
    assert_eq!((status, v), (400, json!(not_this_sessions("275"))));
    assert_eq!(r.browsers.opened, 0);
    assert!(r.fetched().is_empty(), "something was sent");
    assert_eq!(status_of(r.root.path(), &env, "cycle", "274"), "present");
    assert!(!load(r.root.path(), &session).unwrap().changes[0].undone);
}

#[tokio::test]
async fn undo_ends_your_explore_session_when_it_holds_the_delete_templates_account() {
    let _locks = undo_locks();
    let (mut r, session, env) = session_that_made(vec![answer(200, json!({ "success": true }))], &["274"]);
    let (mut browser, closed) = exploring_as(&r, &session, "admin", "admin").await;

    let (status, v) = undo_in(&mut browser, &mut r, true, json!({ "session": session, "ids": ["274"] })).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["ended_session"], json!(ENDED_FOR_UNDO), "{v}");
    assert_eq!(v["results"][0]["ok"], json!(true), "{v}");
    assert!(browser.is_none(), "the session's browser is still there");
    assert!(closed.load(Ordering::SeqCst), "the session's browser was not closed");
    assert!(load(r.root.path(), &session).unwrap().ended_at.is_some());
    assert_eq!(status_of(r.root.path(), &env, "cycle", "274"), "deleted");
    assert_eq!(
        ENDED_FOR_UNDO,
        "Ended your explore session and closed its browser first: Clean up signs in as the delete template's account."
    );
}

#[tokio::test]
async fn undo_leaves_a_session_on_another_account_alone() {
    let _locks = undo_locks();
    let (mut r, session, env) = session_that_made(vec![answer(200, json!({ "success": true }))], &["274"]);
    let (mut browser, closed) = exploring_as(&r, &session, "other", "other").await;

    let (status, v) = undo_in(&mut browser, &mut r, true, json!({ "session": session, "ids": ["274"] })).await;
    assert_eq!(status, 200, "{v}");
    assert!(v.get("ended_session").is_none(), "{v}");
    assert_eq!(v["results"][0]["ok"], json!(true), "{v}");
    assert!(browser.is_some(), "the other session's browser was taken");
    assert!(!closed.load(Ordering::SeqCst));
    assert!(load(r.root.path(), &session).unwrap().ended_at.is_none());
    assert_eq!(status_of(r.root.path(), &env, "cycle", "274"), "deleted");
}

#[tokio::test]
async fn undo_needs_the_switch_and_a_saved_session() {
    let _locks = undo_locks();
    let (mut r, session, env) = session_that_made(vec![], &["274"]);
    let mut none: Option<FakeBrowser> = None;

    let body = json!({ "session": session, "ids": ["274"] });
    assert_eq!(undo_in(&mut none, &mut r, false, body).await, (400, json!(API_WRITES_OFF)));

    let missing = "20251009-000000-admin";
    let (status, v) = undo_in(&mut none, &mut r, true, json!({ "session": missing, "ids": ["274"] })).await;
    assert_eq!((status, v), (404, json!(unknown_session(missing))));

    // A body that does not read names the shape.
    for body in [json!({ "ids": ["274"] }), json!({ "session": session }), json!({ "session": session, "ids": [{}] })] {
        let (status, v) = undo_in(&mut none, &mut r, true, body).await;
        assert_eq!(status, 400, "{v}");
        assert!(v.as_str().unwrap().contains("Expected { \"session\""), "{v}");
    }
    assert_eq!(r.browsers.opened, 0);
    assert!(r.fetched().is_empty());
    assert_eq!(status_of(r.root.path(), &env, "cycle", "274"), "present");
}

/// A session from another project (or organisation) is refused before
/// anything opens: its creates would go through this project's delete
/// templates. Nothing is sent and the record is left as it was.
#[tokio::test]
async fn undo_refuses_a_session_from_another_project_and_deletes_nothing() {
    let _locks = undo_locks();
    let (mut r, _, env) = session_that_made(vec![answer(200, json!({ "success": true }))], &[]);
    let elsewhere = made_session_in(&r, RUN_ORG, "Other", &["274"], 1_760_000_200_000);
    let other_org = made_session_in(&r, "other-org", RUN_PROJECT, &["275"], 1_760_000_300_000);
    let mut none: Option<FakeBrowser> = None;

    let (status, v) = undo_in(&mut none, &mut r, true, json!({ "session": elsewhere, "ids": ["274"] })).await;
    assert_eq!((status, v), (400, json!(ran_in_project("Other"))));
    assert_eq!(ran_in_project("Other"), "this session ran in the project Other - switch to it, then undo");
    let (status, v) = undo_in(&mut none, &mut r, true, json!({ "session": other_org, "ids": ["275"] })).await;
    assert_eq!((status, v), (400, json!(ran_in_project(RUN_PROJECT))));

    assert_eq!(r.browsers.opened, 0);
    assert!(r.fetched().is_empty(), "something was sent");
    assert_eq!(status_of(r.root.path(), &env, "cycle", "274"), "present");
    assert_eq!(status_of(r.root.path(), &env, "cycle", "275"), "present");
    assert!(!load(r.root.path(), &elsewhere).unwrap().changes[0].undone);
}

// ---- The final fix wave ----

use v2_lib::ai_bridge::{end_discovery_for_replay, try_in_session, UNDO_ACCOUNT_IN_USE, UNDO_NEEDS_YES};
use v2_lib::autorun::explore::{
    handler_of, names_request, other_project, request_line, send_create_is_new, verified_creates,
};
use v2_lib::browser::net_capture::MAX_CAPTURED;

fn change_entry(n: u32, change: Change, request: Option<&str>) -> ChangeEntry {
    ChangeEntry { n, at: "2026-10-22T00:00:00.000Z".into(), change, request: request.map(str::to_string), undone: false }
}

/// C1: an edit's save answers with the id of the record it edited; it made
/// nothing, however it was labelled.
#[test]
fn an_edits_save_that_answers_with_the_records_id_is_not_a_create() {
    let floor = 1000;
    // The page showed cycle 274, then the assistant edited it.
    let read = save("GET", "/hr/cycles/274", 1500, text("{\"id\":274,\"name\":\"Old\"}"));
    let edit = save("POST", "/hr/cycles/save", 2000, text("{\"id\":274}"));
    assert_eq!(verify_create(&[&read, &edit], floor, "274", &[]), None);

    // An edit sends the id it edits: in its body, its path or its query.
    let mut by_body = save("POST", "/hr/cycles/save", 2000, text("{\"id\":274}"));
    by_body.req_body = text("{\"id\":274,\"name\":\"New\"}");
    assert_eq!(verify_create(&[&by_body], floor, "274", &[]), None);
    let mut as_form = save("POST", "/hr/cycles/save", 2000, text("{\"id\":274}"));
    as_form.req_body = text("id=274&name=New");
    assert_eq!(verify_create(&[&as_form], floor, "274", &[]), None);
    let by_path = save("POST", "/hr/cycles/274/save", 2000, text("{\"ok\":true,\"id\":274}"));
    assert_eq!(verify_create(&[&by_path], floor, "274", &[]), None);
    let by_query = save("POST", "/hr/cycles/save?id=274", 2000, text("{\"id\":274}"));
    assert_eq!(verify_create(&[&by_query], floor, "274", &[]), None);

    // A create sends other fields and is answered with the new id.
    let mut create = save("POST", "/hr/cycles/save", 2000, text("{\"id\":274}"));
    create.req_body = text("{\"name\":\"New\",\"parent\":12}");
    assert_eq!(verify_create(&[&create], floor, "274", &[]), Some("POST /hr/cycles/save".to_string()));

    // POST or PUT only: a PATCH or a DELETE never creates.
    for method in ["PATCH", "DELETE", "GET"] {
        let other = save(method, "/hr/cycles/save", 2000, text("{\"id\":274}"));
        assert_eq!(verify_create(&[&other], floor, "274", &[]), None, "{method}");
    }
    let put = save("PUT", "/hr/cycles/save", 2000, text("{\"id\":274}"));
    assert!(verify_create(&[&put], floor, "274", &[]).is_some());
    // And it must have been answered 2xx.
    let mut none_status = save("POST", "/hr/cycles/save", 2000, text("{\"id\":274}"));
    none_status.status = None;
    assert_eq!(verify_create(&[&none_status], floor, "274", &[]), None);
}

/// C1: the sign-in is before the change list, so what it answers with never counts.
#[test]
fn the_sign_ins_answer_never_verifies_a_create() {
    let floor = 1000;
    let login = save("POST", "/Account/Login", 500, text("{\"userId\":101}"));
    let other = save("POST", "/hr/employees/save", 2000, text("{\"ok\":true}"));
    assert_eq!(verify_create(&[&login, &other], floor, "101", &[]), None);
    // It does not stop a real create seen afterwards from verifying.
    let real = save("POST", "/hr/employees/save", 2500, text("{\"id\":991}"));
    assert_eq!(
        verify_create(&[&login, &other, &real], floor, "991", &[]),
        Some("POST /hr/employees/save".to_string())
    );
}

/// C1: a child's save that answers with its parent's graph verifies the
/// child that is new, not the parent the page already showed.
#[test]
fn a_save_that_answers_with_the_whole_graph_verifies_only_what_is_new() {
    let floor = 1000;
    let page = save("GET", "/hr/cycles/274", 1500, text("{\"id\":274,\"employees\":[]}"));
    let add = save("POST", "/hr/cycles/274/employees", 2000, text("{\"cycle\":274,\"employees\":[991]}"));
    assert_eq!(verify_create(&[&page, &add], floor, "274", &[]), None);
    assert_eq!(
        verify_create(&[&page, &add], floor, "991", &[]),
        Some("POST /hr/cycles/274/employees".to_string())
    );
    // Told twice, the second answer is not the first thing to carry it.
    let again = save("POST", "/hr/cycles/274/employees", 2500, text("{\"cycle\":274,\"employees\":[991,992]}"));
    assert_eq!(verify_create(&[&page, &add, &again], floor, "992", &[]), Some("POST /hr/cycles/274/employees".to_string()));
}

/// C1: where the session cannot show an id is new, it is not verified.
#[test]
fn a_create_is_not_verified_where_the_session_cannot_prove_its_id_is_new() {
    let floor = 1000;
    let post = save("POST", "/hr/c", 3000, text("{\"Id\":5}"));
    // An earlier page whose body is gone, over its cap, or not read.
    for gone in [Some(Body::Dropped), Some(Body::TooBig), Some(Body::NotText), Some(Body::OffSite), None] {
        let earlier = save("GET", "/hr/list", 2000, gone.clone());
        assert_eq!(verify_create(&[&earlier, &post], floor, "5", &[]), None, "{gone:?}");
    }
    // A request that failed, redirected or answered with nothing has no body to hold an id.
    let mut failed = save("GET", "/hr/x", 2000, None);
    failed.failed = Some("net::ERR_FAILED".into());
    failed.status = None;
    let mut redirect = save("GET", "/hr/y", 2100, None);
    redirect.status = Some(302);
    let mut empty = save("GET", "/hr/z", 2200, None);
    empty.status = Some(204);
    assert!(verify_create(&[&failed, &redirect, &empty, &post], floor, "5", &[]).is_some());
    // The save's own request body is not available.
    let mut big = post.clone();
    big.req_body = Some(Body::TooBig);
    assert_eq!(verify_create(&[&big], floor, "5", &[]), None);
    // A capture that holds as many requests as it keeps may have dropped an earlier one.
    let many: Vec<Captured> = (0..MAX_CAPTURED as u64).map(|i| save("GET", "/hr/p", 5000 + i, text("{}"))).collect();
    let late = save("POST", "/hr/c", 9000, text("{\"Id\":5}"));
    let mut all: Vec<&Captured> = many.iter().collect();
    all.push(&late);
    assert_eq!(verify_create(&all, floor, "5", &[]), None);
    // The same list, its oldest started at the floor: nothing can have been dropped before it.
    let mut from_the_start = many.clone();
    from_the_start[0].started_ms = floor;
    let mut all: Vec<&Captured> = from_the_start.iter().collect();
    all.push(&late);
    assert!(verify_create(&all, floor, "5", &[]).is_some());
}

/// C1: an id an earlier change names is not new.
#[test]
fn an_id_an_earlier_change_names_is_not_a_new_create() {
    let post = save("POST", "/hr/c", 2000, text("{\"Id\":274}"));
    let before = |change: Change, request: Option<&str>| [change_entry(1, change, request)];
    let held = Change::Deleted { kind: "cycle".into(), id: "274".into(), name: "x".into(), held: None };
    let record = Change::Changed {
        record: "Cycle 274".into(),
        field: "Status".into(),
        before: None,
        after: Some(json!("x")),
        hidden: false,
    };
    assert_eq!(verify_create(&[&post], 1000, "274", &before(created("cycle", "274", "x"), None)), None);
    assert_eq!(verify_create(&[&post], 1000, "274", &before(held, None)), None);
    assert_eq!(verify_create(&[&post], 1000, "274", &before(record.clone(), None)), None);
    assert_eq!(verify_create(&[&post], 1000, "274", &before(record, Some("POST /hr/cycles/274"))), None);
    // Another id on the list names nothing here.
    assert!(verify_create(&[&post], 1000, "274", &before(created("cycle", "999", "x"), None)).is_some());

    // Recorded twice, the second is kept as a create that is not verified.
    let (dir, session) = started();
    let first = record_change(dir.path(), &session, created("cycle", "274", "Sprint"), None, &[&post], 1000).unwrap();
    let second = record_change(dir.path(), &session, created("cycle", "274", "Sprint"), None, &[&post], 1000).unwrap();
    assert_eq!((first.verified, second.verified), (Some(true), Some(false)));
    assert_eq!(second.note.as_deref(), Some(NOT_VERIFIED));
    assert_eq!(test_made::list(dir.path()).len(), 1);
}

/// C1: the same proof for a create `send_api_request` made.
#[test]
fn a_send_create_is_new_only_when_nothing_before_it_carried_its_id() {
    let (floor, sent) = (1000, 3000);
    let new = |captured: Option<&[&Captured]>, own: bool, earlier: &[ChangeEntry]| {
        send_create_is_new(captured, floor, sent, "274", earlier, own)
    };
    let nothing: Vec<&Captured> = Vec::new();
    assert!(new(Some(&nothing), false, &[]));
    // No capture, or the call sends the id itself.
    assert!(!new(None, false, &[]));
    assert!(!new(Some(&nothing), true, &[]));
    // A page the session read after the change list began showed it.
    let shown = save("GET", "/hr/cycles", 1500, text("{\"rows\":[{\"id\":274}]}"));
    assert!(!new(Some(&[&shown]), false, &[]));
    // One from before the change list began, or the call's own answer, does not count.
    let before = save("GET", "/hr/cycles", 500, text("{\"rows\":[{\"id\":274}]}"));
    let own = save("POST", "/hr/c", 3000, text("{\"cycleId\":274}"));
    assert!(new(Some(&[&before, &own]), false, &[]));
    // A body that is not available proves nothing.
    let gone = save("GET", "/hr/cycles", 1500, Some(Body::Dropped));
    assert!(!new(Some(&[&gone]), false, &[]));
    // An earlier change names it.
    assert!(!new(Some(&nothing), false, &[change_entry(1, created("cycle", "274", "x"), None)]));
    assert!(new(Some(&nothing), false, &[change_entry(1, created("cycle", "275", "x"), None)]));
}

/// The change list gives the person what an undo would delete.
#[test]
fn the_change_list_names_each_verified_create_with_the_request_that_proved_it() {
    let (dir, session) = started();
    let post = save("POST", "/hr/cycles?x=1", 2000, text("{\"data\":{\"Id\":274}}"));
    record_change(dir.path(), &session, created("cycle", "274", "Sprint 12"), None, &[&post], 1000).unwrap();
    record_change(dir.path(), &session, created("cycle", "999", "Ghost"), None, &[], 1000).unwrap();
    record_change(dir.path(), &session, changed("Status", Some("A"), "B"), None, &[], 1000).unwrap();
    let s = load(dir.path(), &session).unwrap();
    assert_eq!(
        verified_creates(&s),
        vec![json!({ "n": 1, "kind": "cycle", "id": "274", "name": "Sprint 12", "request": "POST /hr/cycles", "undone": false })]
    );
    let (status, out) = explore_changes(dir.path(), ORG, PROJECT, &json!({ "session": session }).to_string());
    assert_eq!(status, 200, "{out}");
    assert_eq!(parsed(&out)["verified_creates"], json!(verified_creates(&s)), "{out}");
}

/// I1: the handler is part of what a save is, and one change accounts for one save.
#[test]
fn a_save_keeps_its_handler_and_one_change_accounts_for_one_save() {
    assert_eq!(request_line("post", "https://hr.example.internal/hr/x?handler=Delete&token=abc#f"), "POST /hr/x?handler=Delete");
    assert_eq!(request_line("POST", "/hr/x?HANDLER=Save"), "POST /hr/x?handler=Save");
    assert_eq!(request_line("POST", "/hr/x?handler=a%20b"), "POST /hr/x");
    assert_eq!(request_line("POST", &format!("/hr/x?handler={}", "a".repeat(41))), "POST /hr/x");
    assert_eq!(request_line("POST", "/hr/x?page=2"), "POST /hr/x");
    assert_eq!(handler_of("/x?a=1&handler=Save"), Some("Save".to_string()));
    assert_eq!(handler_of("/x?a=1"), None);

    // Two handlers on one path, one recorded.
    let s = session_with(
        &[("POST", "/hr/x?handler=Save"), ("POST", "/hr/x?handler=Delete")],
        vec![entry(1, Some("POST /hr/x?handler=Save"))],
    );
    assert_eq!(unaccounted(&s), vec!["POST /hr/x?handler=Delete".to_string()]);
    // The handler's case is ignored; a change recorded without it does not cover a save with one.
    let s = session_with(&[("POST", "/hr/x?handler=SAVE")], vec![entry(1, Some("post /hr/X?handler=save"))]);
    assert!(unaccounted(&s).is_empty());
    let s = session_with(&[("POST", "/hr/x?handler=Delete")], vec![entry(1, Some("POST /hr/x"))]);
    assert_eq!(unaccounted(&s), vec!["POST /hr/x?handler=Delete".to_string()]);

    // Two identical saves, one recorded: one is left. Three and three: none.
    let s = session_with(&[("POST", "/a"), ("POST", "/a")], vec![entry(1, Some("POST /a"))]);
    assert_eq!(unaccounted(&s), vec!["POST /a".to_string()]);
    let s = session_with(
        &[("POST", "/a"), ("POST", "/a"), ("POST", "/a")],
        vec![entry(1, Some("POST /a")), entry(2, Some("POST /a")), entry(3, Some("POST /a"))],
    );
    assert!(unaccounted(&s).is_empty());

    // Whether the list already names a request.
    assert!(names_request(&s, "POST /A/"));
    assert!(!names_request(&s, "POST /a?handler=Save"));
}

/// I1: what is noted keeps the handler and nothing else of the query, and a
/// request the assistant records keeps it too.
#[test]
fn noted_saves_and_recorded_requests_keep_the_handler_only() {
    let (dir, session) = started();
    note_writes(dir.path(), &session, &[("POST".into(), "/hr/x?handler=Publish&k=1".into())]).unwrap();
    assert_eq!(load(dir.path(), &session).unwrap().writes[0].path, "/hr/x?handler=Publish");

    record_change(
        dir.path(),
        &session,
        changed("Status", Some("A"), "B"),
        Some("POST https://hr.example.internal/hr/x?handler=Publish&k=1".into()),
        &[],
        0,
    )
    .unwrap();
    let s = load(dir.path(), &session).unwrap();
    assert_eq!(s.changes[0].request.as_deref(), Some("POST /hr/x?handler=Publish"));
    assert!(unaccounted(&s).is_empty());
    let file = std::fs::read_to_string(session_path(dir.path(), &session)).unwrap();
    assert!(!file.contains("k=1") && !file.contains("hr.example.internal"), "{file}");
}

/// I1: an action's save keeps its handler in the answer and on the session's list.
#[tokio::test]
async fn an_explore_action_notes_the_handler_of_its_save() {
    let dir = root_with_recipe_and_account();
    let _g = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let id = begin(dir.path(), ORG, PROJECT, "admin", 1_760_000_000_000).unwrap();
    let mut d = leave_page("Input.dispatchMouseEvent", "https://hr.example.internal/hr/leave/list?page=2");
    d.on_call_events.push((
        "Input.dispatchMouseEvent".into(),
        sent("1", "POST", "https://hr.example.internal/hr/leave/save?handler=Publish&id=5"),
    ));
    let mut browser = slot(d, explore_going(&id)).0;
    let save = Action::Click { selector: "#save".into() };
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &save, None, None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        parsed(&body)["writes"],
        json!([{ "method": "POST", "path": "/hr/leave/save", "handler": "Publish" }]),
        "{body}"
    );
    assert_eq!(load(dir.path(), &id).unwrap().writes[0].path, "/hr/leave/save?handler=Publish");
    let (_, out) = end_discovery_in(&mut browser).await;
    assert_eq!(parsed(&out)["explore"]["saves_not_on_the_change_list"], json!(["POST /hr/leave/save?handler=Publish"]));
}

/// M7: a try in an explore session files nothing and notes its saves.
#[tokio::test]
async fn a_try_in_an_explore_session_files_nothing_and_notes_its_saves() {
    let dir = root_with_recipe_and_account();
    let id = begin(dir.path(), ORG, PROJECT, "admin", 1_760_000_000_000).unwrap();
    let mut browser = browser_that_saves(&id);
    let b = browser.as_mut().unwrap();
    let click = Action::Click { selector: "#save".into() };
    let mut tabs_case = None;
    let (status, text) = try_in_session(
        &mut b.d,
        &mut tabs_case,
        &mut b.account,
        &mut b.lease,
        dir.path(),
        ORG,
        PROJECT,
        0,
        Some("Leave"),
        Some(&id),
        &click,
    )
    .await;
    assert_eq!(status, 200, "{text}");
    assert!(text.contains("Saves it sent: POST /hr/leave/save."), "{text}");
    assert!(!map_path(dir.path(), ORG, PROJECT).exists(), "an explore try filed something in the map");
    let s = load(dir.path(), &id).unwrap();
    assert_eq!(s.writes.len(), 1, "{:?}", s.writes);
    assert_eq!(unaccounted(&s), vec!["POST /hr/leave/save".to_string()]);
}

/// M9: a session of another project or organisation is neither read nor marked.
#[test]
fn a_session_of_another_project_is_neither_read_nor_marked() {
    let (dir, _session) = started();
    let elsewhere = begin(dir.path(), ORG, "Other", "admin", 1_760_000_900_000).unwrap();
    let other_org = begin(dir.path(), "other-org", PROJECT, "admin", 1_760_000_910_000).unwrap();
    record_change(dir.path(), &elsewhere, changed("Status", Some("A"), "B"), None, &[], 0).unwrap();
    let mut none: Option<FakeBrowser> = None;

    let there = load(dir.path(), &elsewhere).unwrap();
    let (status, out) = explore_changes(dir.path(), ORG, PROJECT, &json!({ "session": elsewhere }).to_string());
    assert_eq!((status, out), (400, other_project(&there)));
    assert_eq!(other_project(&there), "this session ran in the project Other - switch to it to read or change its list");
    let (status, out) = explore_changes(dir.path(), ORG, PROJECT, &json!({ "session": other_org }).to_string());
    assert_eq!(status, 400, "{out}");

    let body = json!({ "session": elsewhere, "undone": [1] }).to_string();
    assert_eq!(record_change_in(&mut none, dir.path(), ORG, PROJECT, &body), (400, other_project(&there)));
    assert!(!load(dir.path(), &elsewhere).unwrap().changes[0].undone);
    // From the project it ran in, it works.
    assert_eq!(record_change_in(&mut none, dir.path(), ORG, "Other", &body).0, 200);
    assert!(load(dir.path(), &elsewhere).unwrap().changes[0].undone);
}

/// M1: a replay that ends an explore session carries its end.
#[tokio::test]
async fn a_replay_that_ends_an_explore_session_carries_its_end() {
    let dir = root_with_recipe_and_account();
    let _g = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let id = begin(dir.path(), ORG, PROJECT, "admin", 1_760_000_000_000).unwrap();
    let mut browser = browser_that_saves(&id);
    let save = Action::Click { selector: "#save".into() };
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &save, None, None).await;
    assert_eq!(status, 200, "{body}");

    let ended = end_discovery_for_replay(&mut browser, dir.path()).await;
    assert!(ended.ended);
    let end = ended.explore.expect("the replay dropped the explore session's end");
    assert_eq!(end.session, id);
    assert_eq!(end.saves_not_on_the_change_list, vec!["POST /hr/leave/save".to_string()]);
    assert!(load(dir.path(), &id).unwrap().ended_at.is_some());
    // A discovery that is not an explore session has none.
    let (mut plain, _) = slot(signin_app(true), exploring("Leave"));
    assert_eq!(end_discovery_for_replay(&mut plain, dir.path()).await.explore, None);
}

/// M8 (the ruling): an undo needs the person's yes, like a delete.
#[tokio::test]
async fn undo_needs_the_persons_yes_like_a_delete() {
    let _locks = undo_locks();
    let (mut r, session, env) = session_that_made(vec![answer(200, json!({ "success": true }))], &["274"]);
    let mut none: Option<FakeBrowser> = None;
    let ctx = BridgeContext { org: RUN_ORG.into(), project: RUN_PROJECT.into(), api_writes: true, ..Default::default() };
    assert_eq!(
        UNDO_NEEDS_YES,
        "Tell the person exactly what will be deleted (each record's kind, id and name, from get_explore_changes) and wait for their yes in the chat first."
    );

    let bodies = [
        json!({ "session": session, "ids": ["274"] }),
        json!({ "session": session, "ids": ["274"], "person_said_yes": false }),
        json!({ "session": session, "ids": ["274"], "person_said_yes": "true" }),
        json!({ "session": session, "ids": ["274"], "person_said_yes": 1 }),
        json!({ "session": session, "ids": ["274"], "person_said_yes": null }),
    ];
    for body in bodies {
        let (status, text) =
            explore_undo_in(&mut none, &mut r.browsers, &ctx, r.root.path(), &body.to_string(), &quick()).await;
        assert_eq!((status, text.as_str()), (400, UNDO_NEEDS_YES), "{body}");
    }
    assert_eq!(r.browsers.opened, 0);
    assert!(r.fetched().is_empty(), "something was sent");
    assert_eq!(status_of(r.root.path(), &env, "cycle", "274"), "present");
    assert!(!load(r.root.path(), &session).unwrap().changes[0].undone);

    // With the yes it goes on, and the answer lists what it deleted.
    let (status, v) = undo_in(&mut none, &mut r, true, json!({ "session": session, "ids": ["274"], "person_said_yes": true })).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(
        v["creates"],
        json!([{ "n": 1, "kind": "cycle", "id": "274", "name": "AUTOTEST 274", "request": "POST /hr/c", "undone": false }]),
        "{v}"
    );
    assert_eq!(status_of(r.root.path(), &env, "cycle", "274"), "deleted");
}

/// M8 (Task 8 Minor 5): only the assistant's own explore session is ended for an undo.
#[tokio::test]
async fn undo_does_not_end_a_discovery_that_is_not_your_explore_session() {
    let _locks = undo_locks();
    let (mut r, session, env) = session_that_made(vec![answer(200, json!({ "success": true }))], &["274"]);
    // A plain discovery of the same account: no explore run on it.
    let (mut browser, closed) = slot(signin_app(true), exploring("Leave"));
    browser.as_mut().unwrap().lease.hold(r.root.path(), "admin").await.unwrap();

    let (status, v) = undo_in(&mut browser, &mut r, true, json!({ "session": session, "ids": ["274"] })).await;
    assert_eq!((status, v), (409, json!(UNDO_ACCOUNT_IN_USE)));
    assert!(browser.is_some(), "the other discovery's browser was taken");
    assert!(!closed.load(Ordering::SeqCst));
    assert!(browser.as_ref().unwrap().discovery.is_some());
    assert_eq!(r.browsers.opened, 0);
    assert!(r.fetched().is_empty(), "something was sent");
    assert_eq!(status_of(r.root.path(), &env, "cycle", "274"), "present");
}

/// M1: an undo that ends your explore session carries its end.
#[tokio::test]
async fn an_undo_that_ends_your_session_carries_the_saves_not_on_its_list() {
    let _locks = undo_locks();
    let (mut r, session, _env) = session_that_made(vec![answer(200, json!({ "success": true }))], &["274"]);
    note_writes(r.root.path(), &session, &[("POST".into(), "/hr/other".into())]).unwrap();
    let (mut browser, _) = exploring_as(&r, &session, "admin", "admin").await;

    let (status, v) = undo_in(&mut browser, &mut r, true, json!({ "session": session, "ids": ["274"] })).await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["ended_session"], json!(ENDED_FOR_UNDO), "{v}");
    assert_eq!(v["explore"]["session"], json!(session), "{v}");
    assert_eq!(v["explore"]["saves_not_on_the_change_list"], json!(["POST /hr/other"]), "{v}");
    assert_eq!(v["note"], json!(UNRECORDED_SAVES), "{v}");
}

/// Release on the card does not hand the person an explore session's end,
/// which is JSON naming the application's requests: it says a sentence of
/// its own and the assistant's text goes to the log.
#[tokio::test]
async fn the_cards_release_of_an_explore_session_answers_in_a_sentence() {
    use v2_lib::commands::autorun_slots::release_answer_in;
    let dir = root_with_recipe_and_account();
    let _g = crate::serial::autorun();
    set_root(dir.path().to_path_buf());
    let save = Action::Click { selector: "#save".into() };
    let id = begin(dir.path(), ORG, PROJECT, "admin", 1_760_000_000_000).unwrap();
    let mut browser = browser_that_saves(&id);
    let (status, body) = discover_action_in(&mut browser, dir.path(), ORG, PROJECT, &save, None, None).await;
    assert_eq!(status, 200, "{body}");

    let said = release_answer_in(&mut browser, "Assistant 1's browser").await;
    crate::autorun_held_browser::assert_a_persons_sentence(&said, "Assistant 1's browser");
    assert!(said.starts_with("Ended the discovery"), "{said}");
    assert!(browser.is_none(), "the explore session's browser was kept");
    assert!(load(dir.path(), &id).unwrap().ended_at.is_some(), "the session was not ended");
}
