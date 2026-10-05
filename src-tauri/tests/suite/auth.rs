use v2_lib::auth::{
    b64url, build_authorize_url, needs_refresh, pkce_pair, refresh_params, upn_from_id_token,
};

use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

#[test]
fn needs_refresh_is_false_for_fresh_token() {
    let now = Instant::now();
    assert!(!needs_refresh(Some(now + Duration::from_secs(3600)), now));
}

#[test]
fn needs_refresh_is_true_inside_early_renew_window() {
    let now = Instant::now();
    assert!(needs_refresh(Some(now + Duration::from_secs(200)), now));
}

#[test]
fn needs_refresh_is_true_without_expiry() {
    assert!(needs_refresh(None, Instant::now()));
}

#[test]
fn refresh_params_use_refresh_grant_and_public_client() {
    let params = refresh_params("rt-abc");
    let get = |k: &str| {
        params
            .iter()
            .find(|(name, _)| *name == k)
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(get("grant_type"), Some("refresh_token"));
    assert_eq!(get("refresh_token"), Some("rt-abc"));
    assert_eq!(get("client_id"), Some("04b07795-8ddb-461a-bbee-02f9e1bf7b46"));
    assert!(get("scope").unwrap().contains("offline_access"));
}

#[test]
fn pkce_verifier_meets_rfc7636() {
    let (verifier, _) = pkce_pair();
    assert!(verifier.len() >= 43 && verifier.len() <= 128);
    assert!(verifier
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-._~".contains(c)));
}

#[test]
fn pkce_challenge_is_s256_of_verifier() {
    let (verifier, challenge) = pkce_pair();
    let expected = b64url(&Sha256::digest(verifier.as_bytes()));
    assert_eq!(challenge, expected);
}

#[test]
fn pkce_pairs_are_unique() {
    assert_ne!(pkce_pair().0, pkce_pair().0);
}

#[test]
fn authorize_url_contains_required_params() {
    let url = build_authorize_url("challenge123", "http://127.0.0.1:8400", "state456");
    assert!(url
        .starts_with("https://login.microsoftonline.com/organizations/oauth2/v2.0/authorize?"));
    for needle in [
        "client_id=04b07795-8ddb-461a-bbee-02f9e1bf7b46",
        "response_type=code",
        "code_challenge=challenge123",
        "code_challenge_method=S256",
        "state=state456",
        "redirect_uri=http%3A%2F%2F127.0.0.1%3A8400",
    ] {
        assert!(url.contains(needle), "missing {needle} in {url}");
    }
}

#[test]
fn upn_extracts_preferred_username() {
    // header.payload.sig with payload {"preferred_username":"a@b.com"}
    let payload = b64url(br#"{"preferred_username":"a@b.com"}"#);
    let jwt = format!("x.{payload}.y");
    assert_eq!(upn_from_id_token(&jwt), Some("a@b.com".to_string()));
}

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use v2_lib::auth::{await_redirect, read_redirect, Loopback, Redirect, SIGN_IN_TIMEOUT};

fn loopback() -> (TcpListener, u16) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    (l, port)
}

fn request(port: u16, raw: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.write_all(raw.as_bytes()).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

#[test]
fn only_a_matching_state_with_a_code_is_a_sign_in() {
    assert_eq!(
        read_redirect("GET /?code=abc&state=s1 HTTP/1.1\r\n", "s1"),
        Redirect::Code("abc".into())
    );
    assert!(matches!(read_redirect("GET /?code=abc&state=zz HTTP/1.1", "s1"), Redirect::Refused(_)));
    let Redirect::Refused(why) =
        read_redirect("GET /?error=access_denied&state=s1 HTTP/1.1", "s1")
    else {
        panic!("a denial is not a sign-in");
    };
    assert!(why.contains("access_denied"), "{why}");
    assert!(matches!(read_redirect("GET /?state=s1 HTTP/1.1", "s1"), Redirect::Refused(_)));
    assert_eq!(read_redirect("GET /favicon.ico HTTP/1.1", "s1"), Redirect::NotTheRedirect);
    assert_eq!(read_redirect("", "s1"), Redirect::NotTheRedirect);
}

/// A browser preconnect that never sends a request used to block the one
/// read forever, so the real redirect behind it was never seen.
///
/// The per-connection budget here is a second, not the 200 ms it once was:
/// a connection that has not sent its line within it is dropped, and on a
/// loaded machine (the release gate, with the whole suite on a share of
/// the cores) this test's own thread could stall that long between
/// connecting and sending the redirect - which was dropped, and the test
/// then waited out the whole window. The app itself gives each connection
/// `LOOPBACK_READ_TIMEOUT` (10 s).
#[test]
fn a_silent_preconnect_does_not_block_the_real_redirect() {
    let (l, port) = loopback();
    let waiter = std::thread::spawn(move || {
        await_redirect(l, "s1", Duration::from_secs(10), Duration::from_secs(1), Duration::ZERO)
    });
    let _silent = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let _ = request(port, "GET /favicon.ico HTTP/1.1\r\n\r\n");
    let page = request(port, "GET /?code=abc&state=s1 HTTP/1.1\r\nHost: localhost\r\n\r\n");
    assert_eq!(waiter.join().unwrap(), Ok("abc".to_string()));
    assert!(page.contains("You're signed in"), "{page}");
}

/// "You're signed in" used to show even for a denial or a state mismatch.
#[test]
fn a_denied_sign_in_shows_a_failure_page() {
    let (l, port) = loopback();
    // Room to send the line on a loaded machine - see the preconnect test.
    let waiter = std::thread::spawn(move || {
        await_redirect(l, "s1", Duration::from_secs(10), Duration::from_secs(2), Duration::from_secs(10))
    });
    let page = request(port, "GET /?error=access_denied&state=s1 HTTP/1.1\r\n\r\n");
    assert!(waiter.join().unwrap().is_err());
    assert!(page.contains("Sign-in did not complete"), "{page}");
    assert!(!page.contains("You're signed in"), "{page}");
    // A refusal does not keep the loopback open, linger or not.
    assert!(TcpStream::connect(("127.0.0.1", port)).is_err(), "still listening after a refusal");
}

/// A closed browser tab used to leave sign-in pending forever.
#[test]
fn nobody_coming_back_times_out_with_a_plain_sentence() {
    let (l, _port) = loopback();
    let started = std::time::Instant::now();
    let out = await_redirect(l, "s1", Duration::from_millis(300), Duration::from_millis(200), Duration::ZERO);
    assert_eq!(out, Err(SIGN_IN_TIMEOUT.to_string()));
    assert_eq!(SIGN_IN_TIMEOUT, "Sign-in timed out. Try again.");
    assert!(started.elapsed() < Duration::from_secs(3));
}

/// A connection that drips one byte at a time, each inside the per-read
/// timeout, used to hold the wait for as long as it kept dripping - past
/// the sign-in window itself.
#[test]
fn a_slow_drip_connection_cannot_hold_the_wait_past_the_window() {
    let (l, port) = loopback();
    let dripper = std::thread::spawn(move || {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        for _ in 0..100 {
            if s.write_all(b"G").is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    });
    let started = std::time::Instant::now();
    let out = await_redirect(l, "s1", Duration::from_millis(600), Duration::from_millis(200), Duration::ZERO);
    assert_eq!(out, Err(SIGN_IN_TIMEOUT.to_string()));
    assert!(started.elapsed() < Duration::from_secs(2), "held for {:?}", started.elapsed());
    dripper.join().unwrap();
}

/// A request the size a real browser sends: the redirect line, then about
/// 3 KB of headers (a long Cookie line among them), all in one write.
fn browser_request(state: &str) -> String {
    let cookie: String = (0..50).map(|i| format!("c{i}={}; ", "v".repeat(48))).collect();
    [
        format!("GET /?code=abc&state={state} HTTP/1.1"),
        "Host: localhost".into(),
        "Connection: keep-alive".into(),
        "sec-ch-ua: \"Chromium\";v=\"140\", \"Not=A?Brand\";v=\"24\"".into(),
        "Upgrade-Insecure-Requests: 1".into(),
        "User-Agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36".into(),
        "Accept: text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8".into(),
        "Accept-Encoding: gzip, deflate, br, zstd".into(),
        "Accept-Language: en-GB,en;q=0.9".into(),
        format!("Cookie: {cookie}"),
        String::new(),
        String::new(),
    ]
    .join("\r\n")
}

/// The whole answer as a browser reads it: every byte up to a clean close.
/// A reset (the app dropping a socket with the browser's headers still
/// unread) is an error here, as it is "connection was reset" there.
fn browser_get(port: u16, raw: &str) -> std::io::Result<String> {
    let mut s = TcpStream::connect(("127.0.0.1", port))?;
    s.set_read_timeout(Some(Duration::from_secs(5)))?;
    s.write_all(raw.as_bytes())?;
    let mut out = Vec::new();
    s.read_to_end(&mut out)?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// The owner saw "connection was reset", then "localhost refused to
/// connect", and was signed in anyway: the loopback read only the request
/// line, answered, and dropped the socket with the rest of the headers
/// unread - which Windows answers with a reset.
#[test]
fn a_browser_sized_request_gets_the_whole_page_without_a_reset() {
    let raw = browser_request("s1");
    assert!(raw.len() > 3000, "{}", raw.len());
    let (l, port) = loopback();
    let waiter = std::thread::spawn(move || {
        await_redirect(l, "s1", Duration::from_secs(10), Duration::from_secs(2), Duration::ZERO)
    });
    let page = browser_get(port, &raw);
    assert_eq!(waiter.join().unwrap(), Ok("abc".to_string()));
    let page = page.expect("the browser's read ended in an error, not a clean close");
    assert!(page.contains("You're signed in"), "{page}");
}

/// A browser that retries the redirect (after a reset, or on reload) used
/// to find nobody listening: "localhost refused to connect".
#[test]
fn the_same_redirect_again_after_the_code_still_gets_the_page() {
    let (l, port) = loopback();
    let waiter = std::thread::spawn(move || {
        await_redirect(l, "s1", Duration::from_secs(10), Duration::from_secs(2), Duration::from_secs(10))
    });
    let first = browser_get(port, &browser_request("s1"));
    assert_eq!(waiter.join().unwrap(), Ok("abc".to_string()));
    let again = browser_get(port, &browser_request("s1"))
        .expect("the retry was refused or reset");
    assert!(again.contains("You're signed in"), "{again}");
    assert!(first.is_ok(), "{first:?}");
    // Something else in the linger gets the old answers, never the page.
    let other = browser_get(port, "GET /?code=zz&state=other HTTP/1.1\r\n\r\n").unwrap();
    assert!(other.contains("Sign-in did not complete"), "{other}");
    let favicon = browser_get(port, "GET /favicon.ico HTTP/1.1\r\n\r\n").unwrap();
    assert!(favicon.starts_with("HTTP/1.1 404"), "{favicon}");
}

/// The linger is for a browser's retry, not a port held open for good.
#[test]
fn the_linger_stops_after_its_window() {
    let (l, port) = loopback();
    let window = Duration::from_millis(800);
    let waiter = std::thread::spawn(move || {
        await_redirect(l, "s1", Duration::from_secs(10), Duration::from_secs(2), window)
    });
    browser_get(port, &browser_request("s1")).unwrap();
    let signed_in_at = Instant::now();
    assert_eq!(waiter.join().unwrap(), Ok("abc".to_string()));
    let again = browser_get(port, &browser_request("s1")).expect("refused inside the linger");
    assert!(again.contains("You're signed in"), "{again}");
    // Past the window (and one poll of the linger's accept loop).
    std::thread::sleep((window + Duration::from_millis(400)).saturating_sub(signed_in_at.elapsed()));
    match browser_get(port, &browser_request("s1")) {
        Err(_) => {}
        Ok(page) => assert!(page.is_empty(), "still answered after the linger: {page}"),
    }
}

/// A browser that tries `localhost` over IPv6 first reached nobody there.
#[test]
fn a_redirect_to_ipv6_localhost_gets_the_page() {
    if TcpListener::bind("[::1]:0").is_err() {
        eprintln!("SKIPPED a_redirect_to_ipv6_localhost_gets_the_page: this machine cannot bind [::1]");
        return;
    }
    let (lb, port) = Loopback::bind().unwrap();
    assert!(lb.has_ipv6(), "[::1]:{port} was not bound although [::1] is available");
    let waiter = std::thread::spawn(move || {
        await_redirect(lb, "s1", Duration::from_secs(10), Duration::from_secs(2), Duration::ZERO)
    });
    let mut s = TcpStream::connect(("::1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s.write_all(browser_request("s1").as_bytes()).unwrap();
    let mut page = String::new();
    s.read_to_string(&mut page).expect("the IPv6 answer ended in an error");
    assert_eq!(waiter.join().unwrap(), Ok("abc".to_string()));
    assert!(page.contains("You're signed in"), "{page}");
}

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use v2_lib::auth::{store_refreshed, AuthState, TokenSet};
use v2_lib::state::fresh_token_with;

fn expiring(access: &str, refresh: &str) -> TokenSet {
    TokenSet {
        access_token: access.into(),
        refresh_token: Some(refresh.into()),
        expires_at: None, // no expiry = needs refresh
        account: Some("a@x.com".into()),
    }
}

fn lasting(access: &str, refresh: &str) -> TokenSet {
    TokenSet {
        access_token: access.into(),
        refresh_token: Some(refresh.into()),
        expires_at: Some(Instant::now() + Duration::from_secs(3600)),
        account: Some("a@x.com".into()),
    }
}

/// Near expiry every concurrent command used to refresh on its own.
#[tokio::test]
async fn concurrent_callers_share_one_refresh() {
    let state = Arc::new(Mutex::new(AuthState { tokens: Some(expiring("old", "rt-1")) }));
    let calls = Arc::new(AtomicUsize::new(0));
    let mut handles = vec![];
    for _ in 0..5 {
        let (state, calls) = (state.clone(), calls.clone());
        handles.push(tokio::spawn(async move {
            fresh_token_with(&state, move |rt, _account| {
                let calls = calls.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(rt, "rt-1");
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    Ok(lasting("new", "rt-2"))
                }
            })
            .await
        }));
    }
    for h in handles {
        assert_eq!(h.await.unwrap().unwrap(), "new");
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1, "one refresh for five callers");
}

/// A refresh for A that finished after B signed in used to overwrite B.
#[tokio::test]
async fn a_refresh_that_finishes_after_a_new_sign_in_does_not_overwrite_it() {
    let state = Arc::new(Mutex::new(AuthState { tokens: Some(expiring("a-old", "rt-a")) }));
    let during = state.clone();
    let token = fresh_token_with(&state, move |_rt, _account| {
        let during = during.clone();
        async move {
            during.lock().unwrap().tokens = Some(lasting("b-token", "rt-b"));
            Ok(lasting("a-new", "rt-a2"))
        }
    })
    .await
    .unwrap();
    assert_eq!(token, "b-token", "the command acts as the account now signed in");
    assert_eq!(state.lock().unwrap().tokens.as_ref().unwrap().access_token, "b-token");
}

/// A FAILED refresh used to return the token read before it started - A's,
/// even though B had signed in while the refresh was out. The command must
/// act as whoever is signed in now.
#[tokio::test]
async fn a_failed_refresh_returns_the_token_of_whoever_is_signed_in_now() {
    let state = Arc::new(Mutex::new(AuthState { tokens: Some(expiring("a-old", "rt-a")) }));
    let during = state.clone();
    let token = fresh_token_with(&state, move |_rt, _account| {
        let during = during.clone();
        async move {
            during.lock().unwrap().tokens = Some(lasting("b-token", "rt-b"));
            Err("offline".to_string())
        }
    })
    .await
    .unwrap();
    assert_eq!(token, "b-token");
}

/// Signed out while the refresh was failing: no token to fall back on.
#[tokio::test]
async fn a_failed_refresh_after_a_sign_out_is_unauthorized() {
    let state = Arc::new(Mutex::new(AuthState { tokens: Some(expiring("a-old", "rt-a")) }));
    let during = state.clone();
    let out = fresh_token_with(&state, move |_rt, _account| {
        let during = during.clone();
        async move {
            during.lock().unwrap().tokens = None;
            Err("offline".to_string())
        }
    })
    .await;
    assert!(matches!(out, Err(v2_lib::ado::AdoError::Unauthorized)), "{out:?}");
}

/// Nothing changed meanwhile: the existing token is still the fallback.
#[tokio::test]
async fn a_failed_refresh_with_no_sign_in_change_keeps_the_current_token() {
    let state = Arc::new(Mutex::new(AuthState { tokens: Some(expiring("a-old", "rt-a")) }));
    let token = fresh_token_with(&state, |_rt, _account| async { Err("offline".to_string()) }).await.unwrap();
    assert_eq!(token, "a-old");
}

#[test]
fn refreshed_tokens_are_stored_only_over_the_session_they_came_from() {
    let mut s = AuthState { tokens: Some(lasting("x", "rt-1")) };
    assert!(store_refreshed(&mut s, "rt-1", lasting("y", "rt-2")));
    assert_eq!(s.tokens.as_ref().unwrap().access_token, "y");
    assert!(!store_refreshed(&mut s, "rt-1", lasting("z", "rt-3")), "rt-1 is no longer current");
    assert_eq!(s.tokens.as_ref().unwrap().access_token, "y");
    let mut signed_out = AuthState::default();
    assert!(!store_refreshed(&mut signed_out, "rt-1", lasting("z", "rt-3")));
    assert!(signed_out.tokens.is_none(), "a sign-out is not undone by a late refresh");
}
