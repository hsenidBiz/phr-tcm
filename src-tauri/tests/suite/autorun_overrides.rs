//! The assistant's test headers and cookies in its Auto Run browser
//! (`autorun::overrides`, the `/autorun-headers` and `/autorun-cookie*`
//! routes): guarded, named in every answer while in place, and taken away
//! by a reset - only what the assistant set.
//!
//! The browser is a scripted fake: the page is on `https://hr.example`, and
//! the cookies it holds are what `Network.getAllCookies` answers. Every
//! call it was asked is recorded, so a test sees exactly what reached the
//! browser - and that nothing reached it when a call was refused.

use crate::common::ScriptedDriver;
use serde_json::{json, Value};
use v2_lib::ai_bridge::{
    cookies_in, override_tool_in, persons_browser_not_changed, set_cookie_in, set_headers_in,
    DiscoveryBrowser, DiscoveryParts, OverrideTool,
};
use v2_lib::autorun::lease::Held;
use v2_lib::autorun::overrides::{
    protected_cookie, refused_header, undo, with_note, Overrides, COOKIE_REFUSED, REFUSED_HEADERS,
};
use v2_lib::browser::cdp::Driver;
use v2_lib::commands::autorun::{
    clear_cookies_in_for_assistant as clear_cookies_in, DiscoveryState, GuardOwner, NO_BROWSER_OPEN,
    REPLAY_RUNNING_AT_RESET,
};
use v2_lib::events::AutorunSessionChanged;

const ORIGIN: &str = "https://hr.example";
/// The sign-in's session value: it must never show.
const SESSION_VALUE: &str = "CfDJ8Kq2xYzSessionValue0099";

/// The supervised browser as the bridge sees it, with its record of test
/// changes.
struct Browser {
    d: ScriptedDriver,
    lease: Held,
    account: Option<String>,
    discovery: Option<DiscoveryState>,
    guarded_case: Option<i32>,
    guard_owner: Option<GuardOwner>,
    overrides: Overrides,
    /// The assistant opened it; otherwise the person did.
    assistant: bool,
    /// How often it was taken over (no replay carries on after).
    taken_over: usize,
}

impl DiscoveryBrowser for Browser {
    type D = ScriptedDriver;
    fn parts(&mut self) -> DiscoveryParts<'_, ScriptedDriver> {
        DiscoveryParts {
            driver: &mut self.d,
            lease: &mut self.lease,
            signed_in: &mut self.account,
            discovery: &mut self.discovery,
            guarded_case: &mut self.guarded_case,
            guard_owner: &mut self.guard_owner,
            overrides: Some(&mut self.overrides),
        }
    }
    fn close(self) {}
    fn opened_by_assistant(&mut self) -> bool {
        self.assistant
    }
    fn taken_over(&mut self) {
        self.taken_over += 1;
    }
}

/// The site's cookies: the sign-in's (`.AspNetCore.Cookies`, HttpOnly), an
/// anti-forgery one, a script-readable `culture`, and an HttpOnly cookie
/// with an innocent name; plus one of another site.
fn jar() -> Vec<Value> {
    vec![
        json!({ "name": ".AspNetCore.Cookies", "value": SESSION_VALUE, "domain": "hr.example", "path": "/", "expires": -1, "size": 40, "httpOnly": true, "secure": true, "session": true, "sameSite": "Lax" }),
        json!({ "name": "__RequestVerificationToken_L2hy", "value": "aftok-77Qx", "domain": "hr.example", "path": "/hr", "expires": -1, "size": 30, "httpOnly": false, "secure": true, "session": true }),
        json!({ "name": "culture", "value": "en-GB", "domain": ".hr.example", "path": "/", "expires": 1_900_000_000.0, "size": 12, "httpOnly": false, "secure": false, "session": false, "sameSite": "Lax" }),
        json!({ "name": "tenant", "value": "acme", "domain": "hr.example", "path": "/", "expires": -1, "size": 10, "httpOnly": true, "secure": true, "session": true }),
        json!({ "name": "elsewhere", "value": "x", "domain": "other.example", "path": "/", "expires": -1, "size": 10, "httpOnly": false, "secure": false, "session": true }),
    ]
}

/// A page on `ORIGIN` holding `jar()`.
fn page() -> ScriptedDriver {
    ScriptedDriver::new(|method, params| match method {
        "Runtime.evaluate" if params["expression"] == "location.origin" => Ok(json!({ "result": { "value": ORIGIN } })),
        "Network.getAllCookies" => Ok(json!({ "cookies": jar() })),
        "Network.setCookie" => Ok(json!({ "success": true })),
        _ => Ok(json!({})),
    })
}

fn browser(assistant: bool) -> Option<Browser> {
    Some(Browser {
        d: page(),
        lease: Held::supervised(),
        account: Some("admin".into()),
        discovery: None,
        guarded_case: None,
        guard_owner: None,
        overrides: Overrides::default(),
        assistant,
        taken_over: 0,
    })
}

fn discovering() -> Option<DiscoveryState> {
    Some(DiscoveryState {
        area: Some("Leave".into()),
        account: Some("admin".into()),
        started_at: 1,
        tried: Vec::new(),
        mapping: None,
        case_id: None,
        no_save: false,
        call_no_save: false,
        call_case_id: None,
    })
}

fn parsed(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("not JSON ({e}): {text}"))
}

fn methods(b: &Option<Browser>) -> Vec<String> {
    b.as_ref().unwrap().d.methods()
}

// ---- headers ------------------------------------------------------------------

/// cookie, authorization, proxy-authorization and host are never set as test
/// headers, in any case; one of them refuses the whole call, and the browser
/// is not asked anything.
#[tokio::test]
async fn refused_header_names() {
    assert_eq!(REFUSED_HEADERS, ["cookie", "authorization", "proxy-authorization", "host"]);
    for name in ["cookie", "Cookie", "AUTHORIZATION", "Proxy-Authorization", "Host", " host "] {
        assert!(refused_header(name), "{name}");
    }
    for name in ["X-Feature", "Accept-Language", "X-Cookie-Consent", "Hostname"] {
        assert!(!refused_header(name), "{name}");
    }
    for name in ["Cookie", "Authorization", "proxy-authorization", "HOST"] {
        let mut slot = browser(true);
        let mut headers = serde_json::Map::new();
        headers.insert("X-Feature".into(), json!("beta"));
        headers.insert(name.into(), json!("x"));
        let args = json!({ "headers": headers });
        let (status, said) = set_headers_in(&mut slot, &args).await;
        assert_eq!(status, 400, "{name}: {said}");
        assert!(said.starts_with(&format!("{name} is never set as a test header")), "{said}");
        assert!(methods(&slot).is_empty(), "{name}: the browser was asked {:?}", methods(&slot));
        assert!(slot.unwrap().overrides.is_empty());
    }
}

/// Headers are merged into one set (a name in another case replaces it, a
/// null removes it), sent to the browser whole, and named - names only.
#[tokio::test]
async fn headers_are_merged_sent_whole_and_named() {
    let mut slot = browser(true);
    let (status, said) = set_headers_in(&mut slot, &json!({ "headers": { "X-Feature": "beta", "Accept-Language": "fr" } })).await;
    assert_eq!(status, 200, "{said}");
    let v = parsed(&said);
    assert_eq!(v["changed_for_testing"], "Changed for testing: header Accept-Language, X-Feature.");
    assert!(!said.contains("beta"), "a value was echoed: {said}");

    let (status, said) = set_headers_in(&mut slot, &json!({ "headers": { "x-feature": "gamma", "Accept-Language": null } })).await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(parsed(&said)["changed_for_testing"], "Changed for testing: header x-feature.");
    let b = slot.as_ref().unwrap();
    let sent = b.d.calls_to("Network.setExtraHTTPHeaders");
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[1]["headers"], json!({ "x-feature": "gamma" }), "the set is not sent whole");

    // Removing the last one sends none, and nothing is changed.
    let (status, said) = set_headers_in(&mut slot, &json!({ "headers": { "X-FEATURE": null } })).await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(parsed(&said)["changed_for_testing"], "Nothing is changed for testing now.");
    assert_eq!(slot.as_ref().unwrap().d.calls_to("Network.setExtraHTTPHeaders")[2]["headers"], json!({}));
    assert!(slot.unwrap().overrides.is_empty());
}

/// Review Focus 4 at the bridge: headers set in a guarded browser make no
/// `Fetch` call, the guard stays on, and a save the page sends after them is
/// still stopped. (`browser_tabs` shows the same on a real connection.)
#[tokio::test]
async fn a_set_header_leaves_the_save_guard_alone() {
    let mut slot = browser(true);
    let before = {
        let b = slot.as_mut().unwrap();
        b.d.guard_saves(&[]).await.unwrap();
        b.d.saves_on_call.push(("Input.dispatchMouseEvent".into(), "POST".into(), "https://hr.example/api/Save".into()));
        b.d.calls.len()
    };
    let (status, said) = set_headers_in(&mut slot, &json!({ "headers": { "X-Tenant": "acme" } })).await;
    assert_eq!(status, 200, "{said}");
    let b = slot.as_mut().unwrap();
    let since: Vec<&str> = b.d.calls[before..].iter().map(|(m, _)| m.as_str()).collect();
    assert_eq!(since, ["Network.setExtraHTTPHeaders"], "the guard was touched");
    assert!(b.d.is_guarding_saves());
    b.d.call("Input.dispatchMouseEvent", json!({})).await.unwrap();
    assert!(b.d.take_save_blocked().is_some(), "the save went through");
}

// ---- cookies ------------------------------------------------------------------

/// Review Focus 3: the sign-in's cookies are refused - `.ASPXAUTH`, the
/// other ASP.NET sign-in and anti-forgery names, a secret-word name, and an
/// HttpOnly cookie of the site whatever its name - and nothing is set; an
/// ordinary cookie like `culture` is set on the page's own site, recorded,
/// and named.
#[tokio::test]
async fn the_sign_in_cookies_are_refused_and_culture_is_set() {
    for name in [
        ".ASPXAUTH",
        ".AspNetCore.Cookies",
        ".AspNetCore.Session",
        "ASP.NET_SessionId",
        "__RequestVerificationToken",
        "__RequestVerificationToken_L2hy",
        "session_id",
        "AuthState",
        "access_token",
        "my_password",
        "api_secret",
        "remember_cookie",
        "tenant",
    ] {
        let mut slot = browser(true);
        let (status, said) = set_cookie_in(&mut slot, &json!({ "name": name, "value": "x" })).await;
        assert_eq!(status, 403, "{name}: {said}");
        assert!(said.starts_with(&format!("{name} is the sign-in's cookie")), "{said}");
        assert!(slot.as_ref().unwrap().d.calls_to("Network.setCookie").is_empty(), "{name} was set");
        assert!(slot.unwrap().overrides.is_empty());
    }
    assert!(protected_cookie("anything", true), "an HttpOnly cookie is the sign-in's");
    assert!(!protected_cookie("culture", false) && !protected_cookie("feature_flags", false));

    let mut slot = browser(true);
    let (status, said) = set_cookie_in(&mut slot, &json!({ "name": "culture", "value": "fr-FR", "same_site": "lax" })).await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(parsed(&said)["changed_for_testing"], "Changed for testing: cookie culture.");
    let b = slot.unwrap();
    let set = b.d.calls_to("Network.setCookie");
    assert_eq!(set, vec![json!({ "name": "culture", "value": "fr-FR", "url": "https://hr.example/", "path": "/", "sameSite": "Lax" })]);
    assert_eq!(b.overrides.cookies.get("culture").map(String::as_str), Some("https://hr.example/"));
    assert_eq!(b.account.as_deref(), Some("admin"), "a cookie set changed the account");
}

/// A cookie the browser will not take is said so, and not recorded.
#[tokio::test]
async fn a_cookie_the_browser_refuses_is_not_recorded() {
    let mut slot = browser(true);
    slot.as_mut().unwrap().d = ScriptedDriver::new(|method, params| match method {
        "Runtime.evaluate" if params["expression"] == "location.origin" => Ok(json!({ "result": { "value": ORIGIN } })),
        "Network.getAllCookies" => Ok(json!({ "cookies": [] })),
        "Network.setCookie" => Ok(json!({ "success": false })),
        _ => Ok(json!({})),
    });
    let (status, said) = set_cookie_in(&mut slot, &json!({ "name": "flag", "value": "1", "same_site": "None" })).await;
    assert_eq!((status, said.as_str()), (400, COOKIE_REFUSED));
    assert!(slot.unwrap().overrides.is_empty());
}

/// Reading the site's cookies hides the sign-in's values - HttpOnly, the
/// anti-forgery cookie - and shows the rest; another site's are not listed.
/// A read works in the person's browser too.
#[tokio::test]
async fn a_cookie_value_of_the_sign_in_is_hidden_when_read() {
    let mut slot = browser(false);
    let (status, said) = cookies_in(&mut slot, &json!({})).await;
    assert_eq!(status, 200, "{said}");
    assert!(!said.contains(SESSION_VALUE), "the sign-in's value showed: {said}");
    assert!(!said.contains("aftok-77Qx"), "the anti-forgery value showed: {said}");
    let v = parsed(&said);
    assert_eq!(v["site"], "hr.example");
    let by_name = |n: &str| v["cookies"].as_array().unwrap().iter().find(|c| c["name"] == n).cloned();
    let session = by_name(".AspNetCore.Cookies").expect("the sign-in's cookie is listed");
    assert_eq!((session["value"].as_str(), session["protected"].as_bool()), (Some("(hidden)"), Some(true)));
    assert_eq!(session["http_only"], true);
    assert_eq!(session["expires"], "session");
    assert_eq!(by_name("__RequestVerificationToken_L2hy").unwrap()["value"], "(hidden)");
    assert_eq!(by_name("tenant").unwrap()["value"], "(hidden)");
    let culture = by_name("culture").unwrap();
    assert_eq!((culture["value"].as_str(), culture["protected"].as_bool()), (Some("en-GB"), Some(false)));
    assert!(by_name("elsewhere").is_none(), "another site's cookie was listed");
    assert_eq!(v["total"], 4);

    // `url` names the site instead of the page's.
    let (status, said) = cookies_in(&mut slot, &json!({ "url": "https://other.example/x?y=1" })).await;
    assert_eq!(status, 200, "{said}");
    let v = parsed(&said);
    assert_eq!(v["site"], "other.example");
    assert_eq!(v["cookies"].as_array().unwrap().len(), 1);
    assert_eq!(cookies_in(&mut slot, &json!({ "url": "ftp://x" })).await.0, 400);
}

// ---- who may act ----------------------------------------------------------------

/// The acting tools refuse (409) in the person's browser - one they opened,
/// or one holding their guard - and nothing reaches the browser. With no
/// browser they answer 404; while a replay runs, 409.
#[tokio::test]
async fn acting_tools_refuse_in_the_persons_browser() {
    let refused = persons_browser_not_changed();
    let calls = [
        (OverrideTool::SetHeaders, json!({ "headers": { "X-Feature": "beta" } })),
        (OverrideTool::SetCookie, json!({ "name": "culture", "value": "fr-FR" })),
        (OverrideTool::ClearCookies, json!({})),
    ];
    for (tool, args) in &calls {
        let mut slot = browser(false);
        let ((status, said), event) = override_tool_in(&mut slot, *tool, &args.to_string()).await;
        assert_eq!((status, said.as_str()), (409, refused.as_str()), "{tool:?}");
        assert!(event.is_none());
        assert!(methods(&slot).is_empty(), "{tool:?} reached the person's browser: {:?}", methods(&slot));
        assert_eq!(slot.as_ref().unwrap().account.as_deref(), Some("admin"));

        // The assistant's browser, but the person's guard is on it.
        let mut slot = browser(true);
        slot.as_mut().unwrap().guard_owner = Some(GuardOwner::Person);
        let ((status, _), _) = override_tool_in(&mut slot, *tool, &args.to_string()).await;
        assert_eq!(status, 409, "{tool:?}");
        assert!(methods(&slot).is_empty());

        let mut none: Option<Browser> = None;
        let ((status, said), _) = override_tool_in(&mut none, *tool, &args.to_string()).await;
        assert_eq!((status, said.as_str()), (404, NO_BROWSER_OPEN), "{tool:?}");
    }
    assert!(refused.contains("the person's Auto Run browser is open"), "{refused}");

    let _a = crate::serial::autorun();
    let replay = v2_lib::autorun::replay_to::OneReplay::claim().unwrap();
    for (tool, args) in &calls {
        let mut slot = browser(true);
        let ((status, said), _) = override_tool_in(&mut slot, *tool, &args.to_string()).await;
        assert_eq!((status, said.as_str()), (409, REPLAY_RUNNING_AT_RESET), "{tool:?}");
        assert!(methods(&slot).is_empty());
    }
    drop(replay);
}

// ---- the sign-out ---------------------------------------------------------------

/// clear_autorun_cookies is a real sign-out: every cookie of the site is
/// deleted (the sign-in's too, never another site's), the account is
/// forgotten with its cookie fingerprint, the lease is let go, the panes
/// hear nobody is signed in, the test headers go too - and a discovery
/// going keeps going.
#[tokio::test]
async fn clear_cookies_signs_out_and_lets_the_lease_go() {
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let env = v2_lib::environments::active_id(dir.path()).unwrap();
    let mut slot = browser(true);
    {
        let b = slot.as_mut().unwrap();
        b.discovery = discovering();
        b.lease.hold(dir.path(), "admin").await.unwrap();
        let print = v2_lib::browser::session::cookie_print(&mut b.d, &[ORIGIN.to_string()]).await;
        assert!(print.is_some());
        b.lease.note_sign_in(print);
        b.overrides.headers.insert("X-Feature".into(), "beta".into());
        b.overrides.cookies.insert("culture".into(), "https://hr.example/".into());
        b.d.calls.clear();
    }
    assert!(v2_lib::autorun::lease::is_held(&env, "admin"));

    let ((status, said), event) = clear_cookies_in(&mut slot).await;
    assert_eq!(status, 200, "{said}");
    let v = parsed(&said);
    assert!(v["said"].as_str().unwrap().starts_with("Signed out: deleted 4 cookies of hr.example"), "{said}");
    assert_eq!(v["changed_for_testing"], "Nothing is changed for testing now.");
    assert_eq!(event, Some(AutorunSessionChanged { opened: true, account: None }));

    let b = slot.as_ref().unwrap();
    let deleted: Vec<String> =
        b.d.calls_to("Network.deleteCookies").iter().map(|c| c["name"].as_str().unwrap().to_string()).collect();
    assert_eq!(deleted, [".AspNetCore.Cookies", "__RequestVerificationToken_L2hy", "culture", "tenant"]);
    assert_eq!(b.d.calls_to("Network.deleteCookies")[1], json!({ "name": "__RequestVerificationToken_L2hy", "domain": "hr.example", "path": "/hr" }));
    assert_eq!(b.account, None, "still signed in as an account");
    assert!(b.lease.sign_in_print().is_none(), "the cookie fingerprint was kept");
    assert_eq!(b.lease.account(), None, "the lease was kept");
    assert!(!v2_lib::autorun::lease::is_held(&env, "admin"), "the account is still held");
    assert!(b.discovery.is_some(), "the discovery was ended");
    assert!(b.overrides.is_empty(), "{:?}", b.overrides);
    assert_eq!(b.d.calls_to("Network.setExtraHTTPHeaders"), vec![json!({ "headers": {} })]);
    assert_eq!(b.taken_over, 1, "a replay could still carry on from where it stood");

    // Nobody was signed in: the panes hear nothing new.
    let ((status, _), event) = clear_cookies_in(&mut slot).await;
    assert_eq!(status, 200);
    assert_eq!(event, None);
}

// ---- the reset ------------------------------------------------------------------

/// The assistant's reset takes away its test headers and deletes the
/// cookies it set - only those: never the sign-in's or any other the page
/// set - whatever the home page then does.
#[tokio::test]
async fn reset_removes_only_what_the_assistant_set() {
    use v2_lib::commands::autorun::reset_in_for_assistant;
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    v2_lib::autorun::recipe::save_recipe(dir.path(), "acme", "Web", &crate::common::menu_recipe()).unwrap();
    let mut slot = browser(true);
    {
        let b = slot.as_mut().unwrap();
        b.overrides.headers.insert("X-Feature".into(), "beta".into());
        b.overrides.cookies.insert("culture".into(), "https://hr.example/".into());
        b.overrides.cookies.insert("feature_flags".into(), "https://hr.example/".into());
    }
    let quick = v2_lib::browser::timing::Timing {
        action_ms: 300,
        expect_ms: 300,
        nav_ms: 300,
        poll_ms: 20,
        highlight_ms: 0,
        lease_wait_ms: 300,
    };
    let (_status, said) = reset_in_for_assistant(&mut slot, dir.path(), "acme", "Web", &quick).await;
    assert!(!said.contains("could not be removed"), "{said}");
    let b = slot.as_ref().expect("the reset closed the browser");
    assert!(b.overrides.is_empty(), "{:?}", b.overrides);
    assert_eq!(b.d.calls_to("Network.setExtraHTTPHeaders"), vec![json!({ "headers": {} })]);
    let deleted = b.d.calls_to("Network.deleteCookies");
    assert_eq!(
        deleted,
        vec![
            json!({ "name": "culture", "url": "https://hr.example/" }),
            json!({ "name": "feature_flags", "url": "https://hr.example/" }),
        ],
        "the reset deleted a cookie the assistant did not set"
    );

    // A reset with nothing changed sends nothing of the kind.
    let mut slot = browser(true);
    let _ = reset_in_for_assistant(&mut slot, dir.path(), "acme", "Web", &quick).await;
    let b = slot.as_ref().unwrap();
    assert!(b.d.calls_to("Network.setExtraHTTPHeaders").is_empty() && b.d.calls_to("Network.deleteCookies").is_empty());
}

/// What could not be taken away stays recorded and named.
#[tokio::test]
async fn what_the_browser_would_not_take_away_stays_named() {
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Network.deleteCookies" => Err(v2_lib::browser::cdp::CdpError::Protocol {
            method: "Network.deleteCookies".into(),
            message: "refused by the test".into(),
        }),
        _ => Ok(json!({})),
    });
    let mut o = Overrides::default();
    o.headers.insert("X-Feature".into(), "beta".into());
    o.cookies.insert("culture".into(), "https://hr.example/".into());
    let left = undo(&mut d, &mut o).await;
    assert_eq!(left, vec!["cookie culture".to_string()]);
    assert!(o.headers.is_empty());
    assert_eq!(o.changed_line().as_deref(), Some("Changed for testing: cookie culture."));
}

// ---- the answer line ------------------------------------------------------------

/// Every try, discovery and replay answer ends with the line while anything
/// is changed: a JSON answer as its last member, any other on a line of its
/// own. Names only, headers then cookies. Nothing changed, nothing added.
#[test]
fn every_answer_ends_with_what_is_changed_for_testing() {
    let mut o = Overrides::default();
    assert_eq!(o.changed_line(), None);
    let plain = (200, json!({ "ok": true, "page": "- button" }).to_string());
    assert_eq!(with_note(plain.clone(), &o), plain);

    o.headers.insert("X-Feature".into(), "beta".into());
    o.headers.insert("Accept-Language".into(), "fr".into());
    o.cookies.insert("culture".into(), "https://hr.example/".into());
    let line = "Changed for testing: header Accept-Language, X-Feature; cookie culture.";
    assert_eq!(o.changed_line().as_deref(), Some(line));
    assert_eq!(
        o.passed_line().as_deref(),
        Some("Passed with these changes in place: header Accept-Language, X-Feature; cookie culture.")
    );

    let (status, text) = with_note(plain, &o);
    assert_eq!(status, 200);
    let v = parsed(&text);
    let last = v.as_object().unwrap().iter().last().unwrap();
    assert_eq!((last.0.as_str(), last.1), ("changed_for_testing", &json!(line)));
    assert!(!text.contains("beta") && !text.contains("fr\""), "a value was named: {text}");

    let (status, text) = with_note((409, "the case has no saved script".to_string()), &o);
    assert_eq!((status, text.as_str()), (409, format!("the case has no saved script\n{line}").as_str()));

    let mut cookies_only = Overrides::default();
    cookies_only.cookies.insert("culture".into(), "https://hr.example/".into());
    assert_eq!(cookies_only.changed_line().as_deref(), Some("Changed for testing: cookie culture."));
}
