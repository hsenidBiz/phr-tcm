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
use crate::common::quick;
use serde_json::json;
use std::sync::atomic::Ordering;
use v2_lib::ai_bridge::{
    discover_action_in, discover_start_in, end_discovery_closing, end_discovery_in, page_read_in, EXPLORE_NOT_MAPPING,
    EXPLORE_NOT_STARTED, READ_WITH_NO_AREA,
};
use v2_lib::autorun::discovery_map::map_path;
use v2_lib::autorun::explore::{
    begin, load, safe_session_id, session_id, session_path, unaccounted, unknown_session, Change, ChangeEntry,
    ExploreRun, ExploreSession, SeenWrite, UNRECORDED_SAVES,
};
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
        .map(|s| DiscoveryState { explore: Some(ExploreRun { session: Some(session.to_string()) }), ..s })
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

    // Each save is named once, in the order it was sent.
    let s = session_with(&[("POST", "/c"), ("POST", "/d"), ("post", "/c/")], vec![entry(1, None)]);
    assert_eq!(unaccounted(&s), vec!["POST /c".to_string(), "POST /d".to_string()]);

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
