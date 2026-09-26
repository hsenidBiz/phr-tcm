//! A signed-in browser, written down and put back.

use crate::common;

use common::ScriptedDriver;
use serde_json::json;
use v2_lib::autorun::sessions::{forget_session, load_fresh_session, save_session};
use v2_lib::browser::session::{
    capture, clear, cookie_belongs, host_of, restore, seed_script, unseed, OriginStorage, SavedSession,
};

#[test]
fn a_cookie_belongs_to_a_host_by_the_usual_domain_rule() {
    assert_eq!(host_of("https://hr.example.internal:8443").as_deref(), Some("hr.example.internal"));
    assert_eq!(host_of("http://127.0.0.1:8080").as_deref(), Some("127.0.0.1"));
    assert_eq!(host_of("file://"), None);
    assert!(cookie_belongs("hr.example.internal", "hr.example.internal"));
    assert!(cookie_belongs(".example.internal", "hr.example.internal"));
    assert!(cookie_belongs("Example.Internal", "hr.example.internal"));
    assert!(!cookie_belongs("evil-example.internal", "hr.example.internal"));
    assert!(!cookie_belongs("internal.evil", "hr.example.internal"));
    assert!(cookie_belongs("127.0.0.1", "127.0.0.1"));
}

#[tokio::test]
async fn capture_keeps_this_applications_cookies_and_the_pages_local_storage() {
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Network.getAllCookies" => Ok(json!({ "cookies": [
            { "name": "sid", "value": "abc", "domain": "hr.example.internal", "path": "/", "httpOnly": true, "secure": true, "session": true, "sameSite": "Lax" },
            { "name": "track", "value": "x", "domain": ".ads.example", "path": "/", "session": false, "expires": 1900000000.0 }
        ] })),
        "Runtime.evaluate" => Ok(json!({ "result": { "value": {
            "origin": "https://hr.example.internal", "entries": [["token", "t\"1"], ["theme", "dark"]]
        } } })),
        other => panic!("unexpected {other}"),
    });
    let saved = capture(&mut d, &["https://hr.example.internal".to_string()], 42).await.unwrap();
    assert_eq!(saved.saved_at_ms, 42);
    assert_eq!(saved.cookies.len(), 1, "the advertising cookie is not this application's");
    assert_eq!(saved.cookies[0]["name"], "sid");
    assert_eq!(
        saved.local_storage,
        vec![OriginStorage {
            origin: "https://hr.example.internal".into(),
            entries: vec![("token".into(), "t\"1".into()), ("theme".into(), "dark".into())],
        }]
    );
}

#[tokio::test]
async fn local_storage_from_another_origin_is_not_kept() {
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Network.getAllCookies" => Ok(json!({ "cookies": [] })),
        _ => Ok(json!({ "result": { "value": { "origin": "https://sso.other", "entries": [["k", "v"]] } } })),
    });
    let saved = capture(&mut d, &["https://hr.example.internal".to_string()], 1).await.unwrap();
    assert!(saved.local_storage.is_empty());
}

/// The one place captured data becomes JavaScript source. It must survive
/// quotes, backslashes and newlines, and must only act on its own origin.
#[test]
fn the_seed_script_embeds_data_as_json_and_checks_the_origin() {
    let nasty = "a\"b\\c\n</script>";
    let js = seed_script(&[OriginStorage { origin: "https://hr.example.internal".into(), entries: vec![("k".into(), nasty.into())] }]);
    let encoded = serde_json::to_string(nasty).unwrap();
    assert!(js.contains(&encoded), "the value must appear exactly as serde_json writes it: {js}");
    assert!(js.contains("location.origin"), "{js}");
    assert!(js.contains("getItem"), "it must not overwrite what the page already has: {js}");
    assert!(seed_script(&[]).is_empty(), "nothing to seed is no script at all");
}

#[tokio::test]
async fn restore_sets_cookies_in_the_protocols_shape_and_seeds_storage() {
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Page.addScriptToEvaluateOnNewDocument" => Ok(json!({ "identifier": "7" })),
        _ => Ok(json!({})),
    });
    let saved = SavedSession {
        saved_at_ms: 1,
        cookies: vec![
            json!({ "name": "sid", "value": "abc", "domain": "hr.example.internal", "path": "/", "httpOnly": true, "secure": true, "session": true, "sameSite": "Lax", "size": 6, "priority": "Medium" }),
            json!({ "name": "keep", "value": "1", "domain": "hr.example.internal", "path": "/", "session": false, "expires": 1900000000.5 }),
        ],
        local_storage: vec![OriginStorage { origin: "https://hr.example.internal".into(), entries: vec![("token".into(), "t1".into())] }],
    };
    let ids = restore(&mut d, &saved).await.unwrap();
    assert_eq!(ids, vec!["7".to_string()]);
    let sent = &d.calls_to("Network.setCookies")[0]["cookies"];
    assert_eq!(sent[0], json!({ "name": "sid", "value": "abc", "domain": "hr.example.internal", "path": "/", "httpOnly": true, "secure": true, "sameSite": "Lax" }),
        "a session cookie carries no expiry, and read-only fields like size are not sent back");
    assert_eq!(sent[1]["expires"], 1900000000.5);
    assert!(d.calls_to("Page.addScriptToEvaluateOnNewDocument")[0]["source"].as_str().unwrap().contains("t1"));

    unseed(&mut d, &ids).await;
    assert_eq!(d.calls_to("Page.removeScriptToEvaluateOnNewDocument")[0]["identifier"], "7");
}

#[tokio::test]
async fn an_empty_session_sends_nothing() {
    let mut d = ScriptedDriver::new(|m, _| panic!("unexpected {m}"));
    let ids = restore(&mut d, &SavedSession { saved_at_ms: 1, cookies: vec![], local_storage: vec![] }).await.unwrap();
    assert!(ids.is_empty());
}

#[tokio::test]
async fn clear_wipes_cookies_and_each_web_origins_storage() {
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({})));
    clear(&mut d, &["https://hr.example.internal".to_string(), "file://".to_string()]).await.unwrap();
    assert_eq!(d.calls_to("Network.clearBrowserCookies").len(), 1);
    let cleared = d.calls_to("Storage.clearDataForOrigin");
    assert_eq!(cleared.len(), 1, "file:// has no storage origin to clear");
    assert_eq!(cleared[0]["origin"], "https://hr.example.internal");
    assert_eq!(cleared[0]["storageTypes"], "local_storage");
}

#[test]
fn a_session_file_is_trusted_only_while_it_is_fresh() {
    let dir = tempfile::tempdir().unwrap();
    let s = SavedSession { saved_at_ms: 1_000_000, cookies: vec![json!({ "name": "sid" })], local_storage: vec![] };
    save_session(dir.path(), "admin", &s).unwrap();
    let minute = 60_000;
    assert_eq!(load_fresh_session(dir.path(), "admin", 10, 1_000_000 + 9 * minute), Some(s.clone()));
    assert_eq!(load_fresh_session(dir.path(), "admin", 10, 1_000_000 + 11 * minute), None);
    assert_eq!(load_fresh_session(dir.path(), "nobody", 10, 1_000_000), None);
    // A clock that went backwards must not make an old file look fresh forever.
    assert_eq!(load_fresh_session(dir.path(), "admin", 10, 500_000), None);
    assert!(save_session(dir.path(), "../escape", &s).is_err());
    forget_session(dir.path(), "admin");
    assert_eq!(load_fresh_session(dir.path(), "admin", 10, 1_000_000), None);
}
