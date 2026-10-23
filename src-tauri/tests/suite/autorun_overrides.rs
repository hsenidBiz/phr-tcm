//! The assistant's test headers and cookies in its Auto Run browser
//! (`autorun::overrides`, the `/autorun-headers` and `/autorun-cookie*`
//! routes): guarded, named in every answer while in place, and taken away
//! by a reset - only what the assistant set.
//!
//! The browser is a scripted fake: TCM's sign-in confirmed the app's site
//! `https://hr.example` in its capture, the page shows whatever origin a
//! test says, and the cookies it holds are what `Network.getAllCookies`
//! answers. Every call it was asked is recorded, so a test sees exactly
//! what reached the browser - and that nothing reached it when a call was
//! refused.

use crate::common::ScriptedDriver;
use serde_json::{json, Value};
use v2_lib::ai_bridge::{
    cookies_in, override_tool_in, persons_browser_not_changed, set_cookie_in, set_headers_in, DiscoveryBrowser,
    DiscoveryParts, OverrideTool,
};
use v2_lib::autorun::lease::Held;
use v2_lib::autorun::overrides::{
    protected_cookie, refused_header, take_for_person, undo, with_note, Overrides, SetCookie, COOKIE_REFUSED,
    REFUSED_HEADERS, ROUTING_OVERRIDE_HEADERS, SITE_NOT_CONFIRMED, TEST_CHANGES_STUCK,
};
use v2_lib::autorun::slots::SlotNo;
use v2_lib::browser::cdp::Driver;
use v2_lib::browser::net_capture::NetCapture;
use v2_lib::commands::autorun::{
    clear_cookies_in_for_assistant as clear_cookies_in, DiscoveryState, GuardOwner, NO_BROWSER_OPEN,
    REPLAY_RUNNING_AT_RESET,
};
use v2_lib::events::AutorunSessionChanged;

const ORIGIN: &str = "https://hr.example";
/// The identity provider's page, where a sign-out may find the browser.
const IDP: &str = "https://login.idp.example";
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

/// The app's cookies: the sign-in's (`.AspNetCore.Cookies`, HttpOnly), an
/// anti-forgery one, a script-readable `culture` on the parent domain, and
/// an HttpOnly cookie with an innocent name; plus the identity provider's.
fn jar() -> Vec<Value> {
    vec![
        json!({ "name": ".AspNetCore.Cookies", "value": SESSION_VALUE, "domain": "hr.example", "path": "/", "expires": -1, "size": 40, "httpOnly": true, "secure": true, "session": true, "sameSite": "Lax" }),
        json!({ "name": "__RequestVerificationToken_L2hy", "value": "aftok-77Qx", "domain": "hr.example", "path": "/hr", "expires": -1, "size": 30, "httpOnly": false, "secure": true, "session": true }),
        json!({ "name": "culture", "value": "en-GB", "domain": ".hr.example", "path": "/", "expires": 1_900_000_000.0, "size": 12, "httpOnly": false, "secure": false, "session": false, "sameSite": "Lax" }),
        json!({ "name": "Tenant", "value": "acme", "domain": "hr.example", "path": "/", "expires": -1, "size": 10, "httpOnly": true, "secure": true, "session": true }),
        json!({ "name": "ESTSAUTH", "value": "idp-session", "domain": "login.idp.example", "path": "/", "expires": -1, "size": 10, "httpOnly": true, "secure": true, "session": true }),
        json!({ "name": "idp_lang", "value": "en", "domain": ".idp.example", "path": "/", "expires": -1, "size": 10, "httpOnly": false, "secure": true, "session": true }),
    ]
}

/// A capture whose site TCM's sign-in confirmed as `ORIGIN`.
fn confirmed() -> NetCapture {
    let mut cap = NetCapture::default();
    cap.set_app_site(Some("https://hr.example/Account/Login"));
    cap.confirm_app_site("https://hr.example/hr/home");
    cap
}

/// A page showing `origin`, holding `cookies`, whose app site is confirmed.
fn page_on(origin: &'static str, cookies: Vec<Value>) -> ScriptedDriver {
    ScriptedDriver::new(move |method, params| match method {
        "Runtime.evaluate" if params["expression"] == "location.origin" => Ok(json!({ "result": { "value": origin } })),
        "Network.getAllCookies" => Ok(json!({ "cookies": cookies.clone() })),
        "Network.setCookie" => Ok(json!({ "success": true })),
        _ => Ok(json!({})),
    })
    .with_capture(confirmed())
}

/// The app's page holding `jar()`.
fn page() -> ScriptedDriver {
    page_on(ORIGIN, jar())
}

fn browser_with(d: ScriptedDriver, assistant: bool) -> Option<Browser> {
    Some(Browser {
        d,
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

fn browser(assistant: bool) -> Option<Browser> {
    browser_with(page(), assistant)
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
        explore: None,
    })
}

fn parsed(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("not JSON ({e}): {text}"))
}

fn methods(b: &Option<Browser>) -> Vec<String> {
    b.as_ref().unwrap().d.methods()
}

/// A cookie as the browser lists it, on `domain` at `/`, readable by scripts.
fn plain(name: &str, value: &str, domain: &str) -> Value {
    json!({ "name": name, "value": value, "domain": domain, "path": "/", "expires": -1, "size": 8, "httpOnly": false, "secure": false, "session": true })
}

fn set_here(value: &str) -> SetCookie {
    SetCookie { domain: "hr.example".into(), path: "/".into(), value: value.into() }
}

// ---- headers ------------------------------------------------------------------

/// cookie, authorization, proxy-authorization and host are never set as test
/// headers, nor the method-override headers (a POST the save guard passes
/// could act as a PUT) or the URL-rewrite headers (it could reach a save
/// address the request line does not name), in any case; one of them refuses the whole call, and
/// the browser is not asked anything.
#[tokio::test]
async fn refused_header_names() {
    // A replay another module's test claims would refuse these calls.
    let _a = crate::serial::autorun();
    assert_eq!(REFUSED_HEADERS, ["cookie", "authorization", "proxy-authorization", "host"]);
    assert_eq!(
        ROUTING_OVERRIDE_HEADERS,
        ["x-http-method-override", "x-http-method", "x-method-override", "x-original-url", "x-rewrite-url"]
    );
    for name in [
        "cookie",
        "Cookie",
        "AUTHORIZATION",
        "Proxy-Authorization",
        "Host",
        " host ",
        "X-HTTP-Method-Override",
        "x-http-method",
        "X-Method-Override",
        "X-Original-URL",
        "x-original-url",
        "X-REWRITE-URL",
        " X-Rewrite-Url ",
    ] {
        assert!(refused_header(name), "{name}");
    }
    for name in ["X-Feature", "Accept-Language", "X-Cookie-Consent", "Hostname", "X-HTTP-Method-Name", "X-Original-Host", "X-Url"] {
        assert!(!refused_header(name), "{name}");
    }
    for name in [
        "Cookie",
        "Authorization",
        "proxy-authorization",
        "HOST",
        "X-HTTP-Method-Override",
        "X-HTTP-Method",
        "x-method-override",
        "X-Original-URL",
        "X-Rewrite-URL",
    ] {
        let mut slot = browser(true);
        let mut headers = serde_json::Map::new();
        headers.insert("X-Feature".into(), json!("beta"));
        headers.insert(name.into(), json!("PUT"));
        let args = json!({ "headers": headers });
        let (status, said) = set_headers_in(SlotNo::MAIN, &mut slot, &args).await;
        assert_eq!(status, 400, "{name}: {said}");
        assert!(said.starts_with(&format!("{name} is never set as a test header")), "{said}");
        assert!(methods(&slot).is_empty(), "{name}: the browser was asked {:?}", methods(&slot));
        assert!(slot.unwrap().overrides.is_empty());
    }
    let mut slot = browser(true);
    let (_, said) = set_headers_in(SlotNo::MAIN, &mut slot, &json!({ "headers": { "X-HTTP-Method-Override": "DELETE" } })).await;
    assert!(said.contains("save or delete"), "{said}");
    let mut slot = browser(true);
    let (status, said) = set_headers_in(SlotNo::MAIN, &mut slot, &json!({ "headers": { "X-Original-URL": "/Leave/Save" } })).await;
    assert_eq!(status, 400, "{said}");
    assert!(said.contains("another path or method") && said.contains("save or delete"), "{said}");
    assert!(!said.contains("/Leave/Save"), "the asked value is not echoed: {said}");
}

/// Headers are merged into one set (a name in another case replaces it, a
/// null removes it), sent to the browser whole, and named - names only.
#[tokio::test]
async fn headers_are_merged_sent_whole_and_named() {
    // A replay another module's test claims would refuse these calls.
    let _a = crate::serial::autorun();
    let mut slot = browser(true);
    let (status, said) = set_headers_in(SlotNo::MAIN, &mut slot, &json!({ "headers": { "X-Feature": "beta", "Accept-Language": "fr" } })).await;
    assert_eq!(status, 200, "{said}");
    let v = parsed(&said);
    assert_eq!(v["changed_for_testing"], "Changed for testing: header Accept-Language, X-Feature.");
    assert!(!said.contains("beta"), "a value was echoed: {said}");

    let (status, said) = set_headers_in(SlotNo::MAIN, &mut slot, &json!({ "headers": { "x-feature": "gamma", "Accept-Language": null } })).await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(parsed(&said)["changed_for_testing"], "Changed for testing: header x-feature.");
    let b = slot.as_ref().unwrap();
    let sent = b.d.calls_to("Network.setExtraHTTPHeaders");
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[1]["headers"], json!({ "x-feature": "gamma" }), "the set is not sent whole");

    // Removing the last one sends none, and nothing is changed.
    let (status, said) = set_headers_in(SlotNo::MAIN, &mut slot, &json!({ "headers": { "X-FEATURE": null } })).await;
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
    // A replay another module's test claims would refuse these calls.
    let _a = crate::serial::autorun();
    let mut slot = browser(true);
    let before = {
        let b = slot.as_mut().unwrap();
        b.d.guard_saves(&[]).await.unwrap();
        b.d.saves_on_call.push(("Input.dispatchMouseEvent".into(), "POST".into(), "https://hr.example/api/Save".into()));
        b.d.calls.len()
    };
    let (status, said) = set_headers_in(SlotNo::MAIN, &mut slot, &json!({ "headers": { "X-Tenant": "acme" } })).await;
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
/// other ASP.NET sign-in and anti-forgery names, a secret-word name (`auth`
/// as a word of its own), and an HttpOnly cookie of the site whatever its
/// name, in any case - and nothing is set; an ordinary cookie like `culture`
/// (or `author`) is set on the app's site, recorded exactly, and named.
#[tokio::test]
async fn the_sign_in_cookies_are_refused_and_culture_is_set() {
    // A replay another module's test claims would refuse these calls.
    let _a = crate::serial::autorun();
    for name in [
        ".ASPXAUTH",
        ".aspxauth",
        ".AspNetCore.Cookies",
        ".ASPNETCORE.SESSION",
        "ASP.NET_SessionId",
        "asp.net_sessionid",
        "__RequestVerificationToken",
        "__requestverificationtoken_L2hy",
        "session_id",
        "AuthState",
        "authToken",
        "AUTHTOKEN",
        "access_token",
        "my_password",
        "api_secret",
        "remember_cookie",
        // HttpOnly on the site as `Tenant`: refused in any case.
        "tenant",
        "TENANT",
    ] {
        let mut slot = browser(true);
        let (status, said) = set_cookie_in(SlotNo::MAIN, &mut slot, &json!({ "name": name, "value": "x" })).await;
        assert_eq!(status, 403, "{name}: {said}");
        assert!(said.starts_with(&format!("{name} is the sign-in's cookie")), "{said}");
        assert!(slot.as_ref().unwrap().d.calls_to("Network.setCookie").is_empty(), "{name} was set");
        assert!(slot.unwrap().overrides.is_empty());
    }
    assert!(protected_cookie("anything", true), "an HttpOnly cookie is the sign-in's");
    for name in ["culture", "feature_flags", "author", "Authority"] {
        assert!(!protected_cookie(name, false), "{name}");
    }

    let mut slot = browser(true);
    let (status, said) = set_cookie_in(SlotNo::MAIN, &mut slot, &json!({ "name": "author", "value": "kim" })).await;
    assert_eq!(status, 200, "author: {said}");

    let mut slot = browser(true);
    let (status, said) = set_cookie_in(SlotNo::MAIN, &mut slot, &json!({ "name": "culture", "value": "fr-FR", "same_site": "lax" })).await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(parsed(&said)["changed_for_testing"], "Changed for testing: cookie culture.");
    let b = slot.unwrap();
    let set = b.d.calls_to("Network.setCookie");
    assert_eq!(set, vec![json!({ "name": "culture", "value": "fr-FR", "url": "https://hr.example/", "path": "/", "sameSite": "Lax" })]);
    assert_eq!(b.overrides.cookies.get("culture"), Some(&set_here("fr-FR")));
    assert!(b.lease.ignored_in_print().contains("culture"), "the sign-in's print still counts it");
    assert_eq!(b.account.as_deref(), Some("admin"), "a cookie set changed the account");
}

/// Ruling B: a cookie is set on the app's confirmed site whatever page
/// shows - the identity provider's included - and with no confirmed site
/// nothing is set.
#[tokio::test]
async fn a_cookie_is_set_on_the_apps_confirmed_site_never_the_page_showing() {
    // A replay another module's test claims would refuse these calls.
    let _a = crate::serial::autorun();
    let mut slot = browser_with(page_on(IDP, jar()), true);
    let (status, said) = set_cookie_in(SlotNo::MAIN, &mut slot, &json!({ "name": "culture", "value": "fr-FR" })).await;
    assert_eq!(status, 200, "{said}");
    let b = slot.unwrap();
    assert_eq!(b.d.calls_to("Network.setCookie")[0]["url"], "https://hr.example/");
    assert!(!b.d.calls_to("Runtime.evaluate").iter().any(|p| p["expression"] == "location.origin"), "the page was asked");

    let unconfirmed = ScriptedDriver::new(|method, _| match method {
        "Network.getAllCookies" => Ok(json!({ "cookies": jar() })),
        _ => Ok(json!({})),
    })
    .with_capture(NetCapture::default());
    let mut slot = browser_with(unconfirmed, true);
    let (status, said) = set_cookie_in(SlotNo::MAIN, &mut slot, &json!({ "name": "culture", "value": "fr-FR" })).await;
    assert_eq!((status, said.as_str()), (409, SITE_NOT_CONFIRMED));
    assert_eq!(SITE_NOT_CONFIRMED, "The app's site is not confirmed yet: sign in through a replay or a discovery first.");
    assert!(methods(&slot).is_empty());
}

/// A cookie the browser will not take is said so, and not recorded.
#[tokio::test]
async fn a_cookie_the_browser_refuses_is_not_recorded() {
    // A replay another module's test claims would refuse these calls.
    let _a = crate::serial::autorun();
    let refusing = ScriptedDriver::new(|method, _| match method {
        "Network.getAllCookies" => Ok(json!({ "cookies": [] })),
        "Network.setCookie" => Ok(json!({ "success": false })),
        _ => Ok(json!({})),
    })
    .with_capture(confirmed());
    let mut slot = browser_with(refusing, true);
    let (status, said) = set_cookie_in(SlotNo::MAIN, &mut slot, &json!({ "name": "flag", "value": "1", "same_site": "None" })).await;
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
    assert_eq!(by_name("Tenant").unwrap()["value"], "(hidden)");
    let culture = by_name("culture").unwrap();
    assert_eq!((culture["value"].as_str(), culture["protected"].as_bool()), (Some("en-GB"), Some(false)));
    assert!(by_name("ESTSAUTH").is_none(), "another site's cookie was listed");
    assert_eq!(v["total"], 4);

    // `url` may name the confirmed site or a parent domain of it.
    let (status, said) = cookies_in(&mut slot, &json!({ "url": "https://hr.example/x?y=1" })).await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(parsed(&said)["site"], "hr.example");
    let (status, said) = cookies_in(&mut slot, &json!({ "url": "https://example/" })).await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(parsed(&said)["site"], "example");
    assert_eq!(cookies_in(&mut slot, &json!({ "url": "ftp://x" })).await.0, 400);
}

/// With no `url`, the cookies read are the app's CONFIRMED site's, not the page
/// showing (the identity provider's); with no confirmed site that is a 409.
/// A `url` outside the confirmed site and its parent domains is a 400.
#[tokio::test]
async fn cookies_are_read_for_the_confirmed_site_not_the_page_showing() {
    let mut slot = browser_with(page_on(IDP, jar()), false);
    let (status, said) = cookies_in(&mut slot, &json!({})).await;
    assert_eq!(status, 200, "{said}");
    let v = parsed(&said);
    assert_eq!(v["site"], "hr.example");
    assert!(!said.contains("idp-session") && !said.contains("idp_lang"), "the page's site was read: {said}");
    assert!(!slot.as_ref().unwrap().d.calls_to("Runtime.evaluate").iter().any(|p| p["expression"] == "location.origin"), "the page was asked");

    for outside in ["https://login.idp.example/x", "https://other.example/", "https://evilhr.example/", "https://sub.hr.example/"] {
        let (status, said) = cookies_in(&mut slot, &json!({ "url": outside })).await;
        assert_eq!(status, 400, "{outside}: {said}");
        assert!(!said.contains("idp-session"), "{said}");
    }

    let unconfirmed = ScriptedDriver::new(|method, _| match method {
        "Network.getAllCookies" => Ok(json!({ "cookies": jar() })),
        _ => Ok(json!({})),
    })
    .with_capture(NetCapture::default());
    let mut slot = browser_with(unconfirmed, false);
    for args in [json!({}), json!({ "url": "https://hr.example/" })] {
        let (status, said) = cookies_in(&mut slot, &args).await;
        assert_eq!((status, said.as_str()), (409, SITE_NOT_CONFIRMED));
    }
    assert!(slot.unwrap().d.calls_to("Network.getAllCookies").is_empty(), "cookies were read with no confirmed site");
}

/// A site that is itself a subdomain (`hr.example.com`): a `url` on its parent
/// domain is accepted and lists only the cookies set for that domain, not
/// the app host's own; a lookalike (`evil-example.com`) is refused; another
/// port or an upper-case host is the same site.
#[tokio::test]
async fn a_cookie_url_on_a_subdomain_sites_parent_is_read_and_a_lookalike_refused() {
    let cookies = vec![
        plain("app_only", "a", "hr.example.com"),
        plain("parent_wide", "p", ".example.com"),
        plain("sibling", "s", "sso.example.com"),
        plain("lookalike", "l", "evil-example.com"),
    ];
    let mut cap = NetCapture::default();
    cap.set_app_site(Some("https://hr.example.com/Account/Login"));
    cap.confirm_app_site("https://hr.example.com/hr/home");
    let d = ScriptedDriver::new(move |method, _| match method {
        "Network.getAllCookies" => Ok(json!({ "cookies": cookies.clone() })),
        _ => Ok(json!({})),
    })
    .with_capture(cap);
    let mut slot = browser_with(d, false);
    let names = |said: &str| -> Vec<String> {
        let mut n: Vec<String> =
            parsed(said)["cookies"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap().to_string()).collect();
        n.sort();
        n
    };

    let (status, said) = cookies_in(&mut slot, &json!({})).await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(parsed(&said)["site"], "hr.example.com");
    assert_eq!(names(&said), ["app_only", "parent_wide"]);

    let (status, said) = cookies_in(&mut slot, &json!({ "url": "https://example.com/" })).await;
    assert_eq!(status, 200, "a parent domain is accepted: {said}");
    assert_eq!(parsed(&said)["site"], "example.com");
    assert_eq!(names(&said), ["parent_wide"], "only that domain's cookies, not the app host's own");

    for same in ["https://hr.example.com:8443/hr/x", "HTTPS://HR.Example.COM/", "http://hr.example.com:80/"] {
        let (status, said) = cookies_in(&mut slot, &json!({ "url": same })).await;
        assert_eq!(status, 200, "{same}: {said}");
        assert_eq!(parsed(&said)["site"], "hr.example.com", "{same}");
        assert_eq!(names(&said), ["app_only", "parent_wide"], "{same}");
    }

    for outside in ["https://evil-example.com/", "https://sso.example.com/", "https://example.com.evil/", "https://xexample.com/"] {
        let (status, said) = cookies_in(&mut slot, &json!({ "url": outside })).await;
        assert_eq!(status, 400, "{outside}: {said}");
        assert!(!said.contains("lookalike") && !said.contains("sibling"), "{said}");
    }
}

// ---- the sign-in's print ---------------------------------------------------------

/// Ruling A: the sign-in's cookie print leaves out the cookies the assistant
/// set, so a test cookie never makes TCM think the sign-in changed; a
/// changed sign-in cookie still does. The print covers only the HttpOnly
/// cookies (#64), so the script-readable `culture` is never part of it; an
/// HttpOnly one a reset deleted (`Tenant`) and the assistant then set for
/// testing is left out from then on.
#[tokio::test]
async fn a_test_cookie_never_makes_the_sign_in_look_changed() {
    // A replay another module's test claims would refuse these calls.
    let _a = crate::serial::autorun();
    let origins = [ORIGIN.to_string()];
    let mut slot = browser(true);
    {
        let b = slot.as_mut().unwrap();
        let print = v2_lib::browser::session::cookie_print(&mut b.d, &origins).await;
        assert_eq!(format!("{print:?}"), "Some(CookiePrint(2 cookies))", "only the HttpOnly .AspNetCore.Cookies and Tenant");
        b.lease.note_sign_in(print);
        assert!(b.lease.same_sign_in(&mut b.d, &origins).await);
    }
    // `jar()` with `culture`, `Tenant` (None: deleted) and the session cookie as given.
    let changed = |culture: &'static str, tenant: Option<&'static str>, session: &'static str| {
        let mut cookies = jar();
        cookies[2]["value"] = json!(culture);
        cookies[0]["value"] = json!(session);
        match tenant {
            Some(t) => cookies[3]["value"] = json!(t),
            None => {
                cookies.remove(3);
            }
        }
        page_on(ORIGIN, cookies)
    };
    let lease = &slot.as_ref().unwrap().lease;
    // A script-readable cookie is not the sign-in's, set for testing or not.
    assert!(lease.same_sign_in(&mut changed("fr-FR", Some("acme"), SESSION_VALUE), &origins).await, "a script's cookie changed the sign-in");
    // Before the assistant set it, a changed or deleted Tenant is a changed sign-in.
    assert!(!lease.same_sign_in(&mut changed("en-GB", Some("globex"), SESSION_VALUE), &origins).await);
    assert!(!lease.same_sign_in(&mut changed("en-GB", None, SESSION_VALUE), &origins).await);

    let (status, said) = set_cookie_in(SlotNo::MAIN, &mut slot, &json!({ "name": "Culture", "value": "fr-FR" })).await;
    assert_eq!(status, 200, "{said}");
    let lease = &slot.as_ref().unwrap().lease;
    assert!(lease.same_sign_in(&mut changed("fr-FR", Some("acme"), SESSION_VALUE), &origins).await, "the test cookie changed the sign-in");
    let mut gone = jar();
    gone.remove(2);
    assert!(lease.same_sign_in(&mut page_on(ORIGIN, gone), &origins).await, "a reset deleting it changed the sign-in");

    // A reset deleted Tenant; the assistant set one of its own in its place.
    slot.as_mut().unwrap().d = changed("en-GB", None, SESSION_VALUE);
    let (status, said) = set_cookie_in(SlotNo::MAIN, &mut slot, &json!({ "name": "Tenant", "value": "test" })).await;
    assert_eq!(status, 200, "{said}");
    let lease = &slot.as_ref().unwrap().lease;
    assert!(lease.same_sign_in(&mut changed("en-GB", None, SESSION_VALUE), &origins).await, "the deleted test cookie changed the sign-in");
    assert!(lease.same_sign_in(&mut changed("en-GB", Some("test"), SESSION_VALUE), &origins).await, "the test cookie changed the sign-in");
    assert!(!lease.same_sign_in(&mut changed("fr-FR", Some("acme"), "another-session-value"), &origins).await, "a new sign-in went unseen");
}

// ---- #64: the print is the HttpOnly cookies ---------------------------------------

/// PeoplesHR's tab cookie: its page script writes it as pages load.
const TAB_COOKIE: &str = "my-application-browser-tab";

/// A sign-in cookie as the browser lists it: HttpOnly, on the app's host.
fn http_only(name: &str, value: &str) -> Value {
    json!({ "name": name, "value": value, "domain": "hr.example", "path": "/", "expires": -1, "size": 40, "httpOnly": true, "secure": true, "session": true })
}

/// A lease that noted the sign-in `cookies` show.
async fn signed_in_with(cookies: Vec<Value>) -> Held {
    let mut lease = Held::supervised();
    let print = v2_lib::browser::session::cookie_print(&mut page_on(ORIGIN, cookies), &[ORIGIN.to_string()]).await;
    assert!(print.is_some(), "no print taken");
    lease.note_sign_in(print);
    lease
}

/// `.ASPXAUTH` (HttpOnly) and the tab cookie (written by the page's script).
fn aspx_and_tab(auth: &str, tab: &str) -> Vec<Value> {
    vec![http_only(".ASPXAUTH", auth), plain(TAB_COOKIE, tab, "hr.example")]
}

/// The page's script rewrote its tab cookie: the sign-in is the same.
#[tokio::test]
async fn a_script_cookie_changing_keeps_the_sign_in() {
    let origins = [ORIGIN.to_string()];
    let lease = signed_in_with(aspx_and_tab("auth-admin-1", "tab-1")).await;
    assert_eq!(format!("{:?}", lease.sign_in_print()), "Some(CookiePrint(1 cookies))");
    assert!(lease.same_sign_in(&mut page_on(ORIGIN, aspx_and_tab("auth-admin-1", "tab-2")), &origins).await);
    let mut no_tab = aspx_and_tab("auth-admin-1", "tab-1");
    no_tab.remove(1);
    assert!(lease.same_sign_in(&mut page_on(ORIGIN, no_tab), &origins).await, "the tab cookie gone changed the sign-in");
}

/// A new `.ASPXAUTH` value is another sign-in.
#[tokio::test]
async fn a_new_aspxauth_is_a_new_sign_in() {
    let lease = signed_in_with(aspx_and_tab("auth-admin-1", "tab-1")).await;
    let mut d = page_on(ORIGIN, aspx_and_tab("auth-carol-1", "tab-1"));
    assert!(!lease.same_sign_in(&mut d, &[ORIGIN.to_string()]).await);
}

/// The HttpOnly sign-in cookie gone is no sign-in of the app's.
#[tokio::test]
async fn an_httponly_cookie_gone_is_a_new_sign_in() {
    let lease = signed_in_with(aspx_and_tab("auth-admin-1", "tab-1")).await;
    let mut d = page_on(ORIGIN, vec![plain(TAB_COOKIE, "tab-1", "hr.example")]);
    assert!(!lease.same_sign_in(&mut d, &[ORIGIN.to_string()]).await);
}

/// Only cookies a script could write: nothing shows whose sign-in it is,
/// so there is no print (the next replay signs in).
#[tokio::test]
async fn no_httponly_cookie_no_print() {
    let mut d = page_on(ORIGIN, vec![plain(TAB_COOKIE, "tab-1", "hr.example"), plain("culture", "en-GB", "hr.example")]);
    assert_eq!(v2_lib::browser::session::cookie_print(&mut d, &[ORIGIN.to_string()]).await, None);
}

/// What changed is said by name - in the answer and in the app log - and
/// never with a value; a browser that does not answer says nothing.
#[tokio::test]
async fn the_changed_names_are_named_never_values() {
    use v2_lib::browser::session::{changed_cookies, cookie_print};
    let _t = crate::serial::log_tail();
    let origins = [ORIGIN.to_string()];
    let before = vec![
        http_only(".ASPXAUTH", "auth-value-admin-7f3a"),
        http_only("ehrm85", "ehrm-value-admin-91c2"),
        plain(TAB_COOKIE, "tab-value-1-55d0", "hr.example"),
    ];
    let after = vec![http_only(".ASPXAUTH", "auth-value-carol-0b8e"), plain(TAB_COOKIE, "tab-value-2-6e14", "hr.example")];
    let print = cookie_print(&mut page_on(ORIGIN, before), &origins).await.expect("no print");
    let none = std::collections::BTreeSet::new();
    let changed = changed_cookies(&mut page_on(ORIGIN, after.clone()), &origins, &print, &none).await;
    assert_eq!(changed, Some(vec![".ASPXAUTH (changed)".to_string(), "ehrm85 (gone)".to_string()]));
    let ignored: std::collections::BTreeSet<String> = [".aspxauth".to_string()].into();
    let changed = changed_cookies(&mut page_on(ORIGIN, after.clone()), &origins, &print, &ignored).await;
    assert_eq!(changed, Some(vec!["ehrm85 (gone)".to_string()]), "a cookie the assistant set was named");
    let mut silent =
        ScriptedDriver::new(|method, _| Err(v2_lib::browser::cdp::CdpError::Protocol { method: method.into(), message: "no".into() }));
    assert_eq!(changed_cookies(&mut silent, &origins, &print, &none).await, None);

    let mut lease = Held::supervised();
    lease.note_sign_in(Some(print));
    assert!(!lease.same_sign_in(&mut page_on(ORIGIN, after), &origins).await);
    let lines: Vec<String> = v2_lib::applog::recent(400).into_iter().map(|l| l.message).collect();
    assert!(
        lines.iter().any(|l| l == "Auto Run: the sign-in's cookies changed: .ASPXAUTH (changed), ehrm85 (gone)"),
        "{lines:?}"
    );
    assert!(!lines.iter().any(|l| l.contains("-value-")), "a cookie value was logged");
}

// ---- who may act ----------------------------------------------------------------

/// The acting tools refuse (409) in the person's browser - one they opened,
/// or one holding their guard - and nothing reaches the browser. With no
/// browser they answer 404; while a replay runs, 409.
#[tokio::test]
async fn acting_tools_refuse_in_the_persons_browser() {
    // A replay another module's test claims would refuse these calls.
    let _a = crate::serial::autorun();
    let refused = persons_browser_not_changed();
    let calls = [
        (OverrideTool::SetHeaders, json!({ "headers": { "X-Feature": "beta" } })),
        (OverrideTool::SetCookie, json!({ "name": "culture", "value": "fr-FR" })),
        (OverrideTool::ClearCookies, json!({})),
    ];
    for (tool, args) in &calls {
        let mut slot = browser(false);
        let ((status, said), event) = override_tool_in(SlotNo::MAIN, &mut slot, *tool, &args.to_string()).await;
        assert_eq!((status, said.as_str()), (409, refused.as_str()), "{tool:?}");
        assert!(event.is_none());
        assert!(methods(&slot).is_empty(), "{tool:?} reached the person's browser: {:?}", methods(&slot));
        assert_eq!(slot.as_ref().unwrap().account.as_deref(), Some("admin"));

        // The assistant's browser, but the person's guard is on it.
        let mut slot = browser(true);
        slot.as_mut().unwrap().guard_owner = Some(GuardOwner::Person);
        let ((status, _), _) = override_tool_in(SlotNo::MAIN, &mut slot, *tool, &args.to_string()).await;
        assert_eq!(status, 409, "{tool:?}");
        assert!(methods(&slot).is_empty());

        let mut none: Option<Browser> = None;
        let ((status, said), _) = override_tool_in(SlotNo::MAIN, &mut none, *tool, &args.to_string()).await;
        assert_eq!((status, said.as_str()), (404, NO_BROWSER_OPEN), "{tool:?}");
    }
    assert!(refused.contains("the person's Auto Run browser is open"), "{refused}");

    let replay = v2_lib::autorun::replay_to::OneReplay::claim().unwrap();
    for (tool, args) in &calls {
        let mut slot = browser(true);
        let ((status, said), _) = override_tool_in(SlotNo::MAIN, &mut slot, *tool, &args.to_string()).await;
        assert_eq!((status, said.as_str()), (409, REPLAY_RUNNING_AT_RESET), "{tool:?}");
        assert!(methods(&slot).is_empty());
    }
    drop(replay);
}

/// The person taking the browser takes the assistant's test changes away
/// first; when they will not go, the person's action is refused, never run
/// with them in place.
#[tokio::test]
async fn the_person_never_runs_with_the_assistants_test_changes() {
    let mut o = Overrides::default();
    o.headers.insert("X-Feature".into(), "beta".into());
    o.cookies.insert("culture".into(), set_here("fr-FR"));
    let mut d = page_on(ORIGIN, vec![plain("culture", "fr-FR", "hr.example")]);
    assert_eq!(take_for_person(&mut d, &mut o).await, Ok(()));
    assert!(o.is_empty());
    assert_eq!(d.calls_to("Network.deleteCookies"), vec![json!({ "name": "culture", "domain": "hr.example", "path": "/" })]);

    let mut o = Overrides::default();
    o.headers.insert("X-Feature".into(), "beta".into());
    let mut stuck = ScriptedDriver::new(|method, _| match method {
        "Network.setExtraHTTPHeaders" => Err(v2_lib::browser::cdp::CdpError::Protocol {
            method: method.into(),
            message: "refused by the test".into(),
        }),
        _ => Ok(json!({})),
    });
    assert_eq!(take_for_person(&mut stuck, &mut o).await, Err(TEST_CHANGES_STUCK.to_string()));
    assert_eq!(TEST_CHANGES_STUCK, "Test changes made by the assistant could not be removed: use Release browser, then try again.");
    assert!(!o.is_empty(), "what would not go was forgotten");
    // Nothing to take away: nothing is asked.
    let mut idle = ScriptedDriver::new(|_, _| Ok(json!({})));
    assert_eq!(take_for_person(&mut idle, &mut Overrides::default()).await, Ok(()));
    assert!(idle.methods().is_empty());
}

// ---- the sign-out ---------------------------------------------------------------

/// autorun_cookies' clear is a real sign-out: every cookie of the app's
/// confirmed site is deleted (its host's and its parent domain's, the
/// sign-in's too, never the identity provider's - even with the IdP's page
/// showing), the account is forgotten with its cookie fingerprint, the lease
/// is let go, the panes hear nobody is signed in, the test headers go too -
/// and a discovery going keeps going.
#[tokio::test]
async fn clear_cookies_signs_out_and_lets_the_lease_go() {
    // A replay another module's test claims would refuse these calls.
    let _a = crate::serial::autorun();
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    let env = v2_lib::environments::active_id(dir.path()).unwrap();
    let mut slot = browser_with(page_on(IDP, jar()), true);
    {
        let b = slot.as_mut().unwrap();
        b.discovery = discovering();
        b.lease.hold(dir.path(), "admin").await.unwrap();
        let print = v2_lib::browser::session::cookie_print(&mut b.d, &[ORIGIN.to_string()]).await;
        assert!(print.is_some());
        b.lease.note_sign_in(print);
        b.overrides.headers.insert("X-Feature".into(), "beta".into());
        b.overrides.cookies.insert("culture".into(), set_here("fr-FR"));
        b.d.calls.clear();
    }
    assert!(v2_lib::autorun::lease::is_held(&env, "admin"));

    let ((status, said), event) = clear_cookies_in(SlotNo::MAIN, &mut slot).await;
    assert_eq!(status, 200, "{said}");
    let v = parsed(&said);
    assert!(v["said"].as_str().unwrap().starts_with("Signed out: deleted 4 cookies of hr.example"), "{said}");
    assert_eq!(v["changed_for_testing"], "Nothing is changed for testing now.");
    assert_eq!(event, Some(AutorunSessionChanged { opened: true, account: None }));

    let b = slot.as_ref().unwrap();
    let deleted: Vec<String> =
        b.d.calls_to("Network.deleteCookies").iter().map(|c| c["name"].as_str().unwrap().to_string()).collect();
    assert_eq!(deleted, [".AspNetCore.Cookies", "__RequestVerificationToken_L2hy", "culture", "Tenant"], "the IdP's cookies were touched");
    assert_eq!(b.d.calls_to("Network.deleteCookies")[1], json!({ "name": "__RequestVerificationToken_L2hy", "domain": "hr.example", "path": "/hr" }));
    assert_eq!(b.d.calls_to("Network.deleteCookies")[2]["domain"], ".hr.example", "the parent domain's cookie was kept");
    assert_eq!(b.account, None, "still signed in as an account");
    assert!(b.lease.sign_in_print().is_none(), "the cookie fingerprint was kept");
    assert_eq!(b.lease.account(), None, "the lease was kept");
    assert!(!v2_lib::autorun::lease::is_held(&env, "admin"), "the account is still held");
    assert!(b.discovery.is_some(), "the discovery was ended");
    assert!(b.overrides.is_empty(), "{:?}", b.overrides);
    assert_eq!(b.d.calls_to("Network.setExtraHTTPHeaders"), vec![json!({ "headers": {} })]);
    assert_eq!(b.taken_over, 1, "a replay could still carry on from where it stood");

    // Nobody was signed in: the panes hear nothing new.
    let ((status, _), event) = clear_cookies_in(SlotNo::MAIN, &mut slot).await;
    assert_eq!(status, 200);
    assert_eq!(event, None);

    // With no confirmed site nothing is deleted.
    let unconfirmed = ScriptedDriver::new(|method, _| match method {
        "Network.getAllCookies" => Ok(json!({ "cookies": jar() })),
        _ => Ok(json!({})),
    })
    .with_capture(NetCapture::default());
    let mut slot = browser_with(unconfirmed, true);
    let ((status, said), event) = clear_cookies_in(SlotNo::MAIN, &mut slot).await;
    assert_eq!((status, said.as_str(), event), (409, SITE_NOT_CONFIRMED, None));
    assert!(methods(&slot).is_empty());
    assert_eq!(slot.unwrap().account.as_deref(), Some("admin"));
}

// ---- the reset ------------------------------------------------------------------

/// The assistant's reset takes away its test headers and deletes the
/// cookies it set - only those, and only while each is still exactly what
/// it set: one the sign-in wrote over since is left alone. Never the
/// sign-in's or any other cookie the page set - whatever the home page then
/// does.
#[tokio::test]
async fn reset_removes_only_what_the_assistant_set() {
    // A replay another module's test claims would refuse these calls.
    let _a = crate::serial::autorun();
    use v2_lib::commands::autorun::reset_in_for_assistant;
    let _l = crate::serial::account_leases();
    let dir = tempfile::tempdir().unwrap();
    v2_lib::autorun::recipe::save_recipe(dir.path(), "acme", "Web", &crate::common::menu_recipe()).unwrap();
    let mut cookies = jar();
    cookies.push(plain("culture", "fr-FR", "hr.example"));
    cookies.push(plain("feature_flags", "on", "hr.example"));
    // The assistant set `theme`, and a sign-in has since written its own.
    cookies.push(plain("theme", "app-default", "hr.example"));
    let mut slot = browser_with(page_on(ORIGIN, cookies), true);
    {
        let b = slot.as_mut().unwrap();
        b.overrides.headers.insert("X-Feature".into(), "beta".into());
        b.overrides.cookies.insert("culture".into(), set_here("fr-FR"));
        b.overrides.cookies.insert("feature_flags".into(), set_here("on"));
        b.overrides.cookies.insert("theme".into(), set_here("dark"));
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
            json!({ "name": "culture", "domain": "hr.example", "path": "/" }),
            json!({ "name": "feature_flags", "domain": "hr.example", "path": "/" }),
        ],
        "the reset deleted a cookie that was not the one the assistant set"
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
        "Network.getAllCookies" => Ok(json!({ "cookies": [plain("culture", "fr-FR", "hr.example")] })),
        "Network.deleteCookies" => Err(v2_lib::browser::cdp::CdpError::Protocol {
            method: "Network.deleteCookies".into(),
            message: "refused by the test".into(),
        }),
        _ => Ok(json!({})),
    });
    let mut o = Overrides::default();
    o.headers.insert("X-Feature".into(), "beta".into());
    o.cookies.insert("culture".into(), set_here("fr-FR"));
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
    let plain_answer = (200, json!({ "ok": true, "page": "- button" }).to_string());
    assert_eq!(with_note(plain_answer.clone(), &o), plain_answer);

    o.headers.insert("X-Feature".into(), "beta".into());
    o.headers.insert("Accept-Language".into(), "fr".into());
    o.cookies.insert("culture".into(), set_here("fr-FR"));
    let line = "Changed for testing: header Accept-Language, X-Feature; cookie culture.";
    assert_eq!(o.changed_line().as_deref(), Some(line));
    assert_eq!(
        o.passed_line().as_deref(),
        Some("Passed with these changes in place: header Accept-Language, X-Feature; cookie culture.")
    );

    let (status, text) = with_note(plain_answer, &o);
    assert_eq!(status, 200);
    let v = parsed(&text);
    let last = v.as_object().unwrap().iter().last().unwrap();
    assert_eq!((last.0.as_str(), last.1), ("changed_for_testing", &json!(line)));
    assert!(!text.contains("beta") && !text.contains("fr-FR"), "a value was named: {text}");

    let (status, text) = with_note((409, "the case has no saved script".to_string()), &o);
    assert_eq!((status, text.as_str()), (409, format!("the case has no saved script\n{line}").as_str()));

    let mut cookies_only = Overrides::default();
    cookies_only.cookies.insert("culture".into(), set_here("fr-FR"));
    assert_eq!(cookies_only.changed_line().as_deref(), Some("Changed for testing: cookie culture."));
}

/// The answers take what is changed under the session lock, the moment the
/// call finishes - try, discovery start, action and batch, the assistant's
/// end of a discovery and its replay - not from the published copy
/// afterwards.
#[test]
fn the_answers_read_what_is_changed_under_the_lock() {
    let norm = |s: &str| s.replace("\r\n", "\n");
    let bridge = norm(include_str!("../../src/ai_bridge.rs"));
    let commands = norm(include_str!("../../src/commands/autorun.rs"));
    let host = norm(include_str!("../../src/commands/ai_bridge.rs"));
    let body = |text: &str, start: &str| -> String {
        let from = &text[text.find(start).unwrap_or_else(|| panic!("{start} is gone"))..];
        from[..from.find("\n}\n").unwrap()].to_string()
    };
    for f in ["async fn autorun_try(", "async fn autorun_discover_start(", "async fn autorun_discover_action(", "async fn autorun_discover_actions("] {
        assert!(body(&bridge, f).contains("*seen = Some(crate::commands::autorun::overrides_of(&slot));"), "{f}");
    }
    assert!(body(&commands, "pub(crate) async fn end_discovery(").contains("with_note(answer, &overrides_of(&slot))"));
    let replay = body(&commands, "pub(crate) async fn replay_supervised(");
    assert!(replay.contains("let changed = slot.as_ref().map(|s| s.overrides.clone()).unwrap_or_default();"), "{replay}");
    assert!(!host.contains("overrides_now"), "the replay host reads the published copy");
}
