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
use v2_lib::auth::{await_redirect, read_redirect, Redirect, SIGN_IN_TIMEOUT};

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
#[test]
fn a_silent_preconnect_does_not_block_the_real_redirect() {
    let (l, port) = loopback();
    let waiter = std::thread::spawn(move || {
        await_redirect(l, "s1", Duration::from_secs(10), Duration::from_millis(200))
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
    let waiter = std::thread::spawn(move || {
        await_redirect(l, "s1", Duration::from_secs(10), Duration::from_millis(200))
    });
    let page = request(port, "GET /?error=access_denied&state=s1 HTTP/1.1\r\n\r\n");
    assert!(waiter.join().unwrap().is_err());
    assert!(page.contains("Sign-in did not complete"), "{page}");
    assert!(!page.contains("You're signed in"), "{page}");
}

/// A closed browser tab used to leave sign-in pending forever.
#[test]
fn nobody_coming_back_times_out_with_a_plain_sentence() {
    let (l, _port) = loopback();
    let started = std::time::Instant::now();
    let out = await_redirect(l, "s1", Duration::from_millis(300), Duration::from_millis(200));
    assert_eq!(out, Err(SIGN_IN_TIMEOUT.to_string()));
    assert_eq!(SIGN_IN_TIMEOUT, "Sign-in timed out. Try again.");
    assert!(started.elapsed() < Duration::from_secs(3));
}
