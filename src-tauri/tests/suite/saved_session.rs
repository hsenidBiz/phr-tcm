//! Stay signed in (`saved_session`): the refresh token kept in Credential
//! Manager between launches. Driven against the in-memory store - never the
//! real vault of whoever runs the suite.

use std::sync::Mutex;

use v2_lib::auth::{is_refusal, AuthState, TokenSet, SIGN_IN_REFUSED, SIGN_IN_UNAVAILABLE};
use v2_lib::db::{MemoryStore, SecretStore};
use v2_lib::saved_session::{forget, keep_current, load, resume_with, save, Resume, MAX_PARTS, PART};

const HEAD: &str = "tcm-v2/auth/session";

fn part(i: usize) -> String {
    format!("{HEAD}/{i}")
}

/// A refresh token long enough to need three parts, so the splitting is
/// what is under test, not a token that happens to fit in one entry.
fn long_token(seed: char) -> String {
    std::iter::repeat(seed).take(PART * 2 + 17).collect()
}

fn tokens(access: &str, refresh: Option<&str>, account: Option<&str>) -> TokenSet {
    TokenSet {
        access_token: access.into(),
        refresh_token: refresh.map(str::to_string),
        expires_at: None,
        account: account.map(str::to_string),
    }
}

fn signed_in(refresh: Option<&str>, account: Option<&str>) -> Mutex<AuthState> {
    Mutex::new(AuthState { tokens: Some(tokens("at", refresh, account)) })
}

#[test]
fn a_kept_session_reads_back_whole_across_its_parts() {
    let store = MemoryStore::default();
    let rt = long_token('a');
    save(&store, &rt, Some("avin@example.com")).unwrap();
    // Three parts, none over the entry limit.
    for i in 0..3 {
        let p = store.get(&part(i)).unwrap().expect("part");
        assert!(p.len() <= PART, "part {i} is {} long", p.len());
    }
    assert_eq!(store.get(&part(3)).unwrap(), None);
    let saved = load(&store).unwrap().expect("kept");
    assert_eq!(saved.refresh_token, rt);
    assert_eq!(saved.account.as_deref(), Some("avin@example.com"));
}

#[test]
fn nothing_kept_reads_as_none() {
    assert!(load(&MemoryStore::default()).unwrap().is_none());
}

#[test]
fn a_shorter_token_leaves_none_of_the_longer_one_behind() {
    let store = MemoryStore::default();
    save(&store, &long_token('a'), None).unwrap();
    save(&store, "short", None).unwrap();
    assert_eq!(store.get(&part(1)).unwrap(), None);
    assert_eq!(store.get(&part(2)).unwrap(), None);
    assert_eq!(load(&store).unwrap().expect("kept").refresh_token, "short");
}

#[test]
fn a_session_missing_a_part_is_no_session() {
    let store = MemoryStore::default();
    save(&store, &long_token('a'), None).unwrap();
    store.remove(&part(1)).unwrap();
    assert!(load(&store).unwrap().is_none());
}

#[test]
fn an_unreadable_head_is_no_session() {
    let store = MemoryStore::default();
    save(&store, "rt", None).unwrap();
    store.put(HEAD, "not json").unwrap();
    assert!(load(&store).unwrap().is_none());
    store.put(HEAD, r#"{"parts":999,"account":null}"#).unwrap();
    assert!(load(&store).unwrap().is_none());
}

#[test]
fn forget_removes_every_entry() {
    let store = MemoryStore::default();
    save(&store, &long_token('a'), Some("a")).unwrap();
    forget(&store).unwrap();
    assert_eq!(store.get(HEAD).unwrap(), None);
    for i in 0..MAX_PARTS {
        assert_eq!(store.get(&part(i)).unwrap(), None, "part {i}");
    }
    // And forgetting nothing is fine.
    forget(&store).unwrap();
}

#[test]
fn an_empty_or_oversized_token_is_not_kept() {
    let store = MemoryStore::default();
    assert!(save(&store, "", None).is_err());
    let huge: String = std::iter::repeat('x').take(PART * MAX_PARTS + 1).collect();
    assert!(save(&store, &huge, None).is_err());
    assert!(load(&store).unwrap().is_none());
}

#[test]
fn keep_current_keeps_the_signed_in_session_when_on() {
    let store = MemoryStore::default();
    keep_current(&store, &signed_in(Some("rt-1"), Some("a@x")), true);
    let saved = load(&store).unwrap().expect("kept");
    assert_eq!(saved.refresh_token, "rt-1");
    assert_eq!(saved.account.as_deref(), Some("a@x"));
}

#[test]
fn keep_current_forgets_when_off_or_nobody_is_signed_in() {
    let store = MemoryStore::default();
    save(&store, "old", None).unwrap();
    keep_current(&store, &signed_in(Some("rt-1"), None), false);
    assert!(load(&store).unwrap().is_none(), "off keeps nothing");

    save(&store, "old", None).unwrap();
    keep_current(&store, &Mutex::new(AuthState::default()), true);
    assert!(load(&store).unwrap().is_none(), "signed out keeps nothing");

    save(&store, "old", None).unwrap();
    keep_current(&store, &signed_in(None, None), true);
    assert!(load(&store).unwrap().is_none(), "no refresh token keeps nothing");
}

#[tokio::test]
async fn resume_with_nothing_kept_never_asks_microsoft() {
    let store = MemoryStore::default();
    let out = resume_with(&store, |_, _| async { panic!("nothing was kept") }).await;
    assert!(matches!(out, Resume::Nothing));
}

#[tokio::test]
async fn resume_sends_the_kept_token_and_account_and_hands_back_the_new_set() {
    let store = MemoryStore::default();
    let rt = long_token('r');
    save(&store, &rt, Some("a@x")).unwrap();
    let sent = rt.clone();
    let out = resume_with(&store, move |got, account| async move {
        assert_eq!(got, sent);
        assert_eq!(account.as_deref(), Some("a@x"));
        Ok(tokens("fresh", Some("rt-2"), Some("a@x")))
    })
    .await;
    match out {
        Resume::Resumed(t) => {
            assert_eq!(t.access_token, "fresh");
            assert_eq!(t.refresh_token.as_deref(), Some("rt-2"));
        }
        _ => panic!("expected the session to resume"),
    }
    // Storing the result (and its rotated token) is the caller's decision.
    assert_eq!(load(&store).unwrap().expect("kept").refresh_token, rt);
}

#[tokio::test]
async fn a_refused_session_is_forgotten() {
    let store = MemoryStore::default();
    save(&store, "rt", None).unwrap();
    let refusal = format!("{SIGN_IN_REFUSED} (invalid_grant). Sign in again - Settings → Logs has the details.");
    let out = resume_with(&store, move |_, _| async move { Err(refusal) }).await;
    assert!(matches!(out, Resume::Refused));
    assert!(load(&store).unwrap().is_none());
}

#[tokio::test]
async fn an_unreachable_or_troubled_microsoft_keeps_the_session_for_next_time() {
    for err in [
        v2_lib::auth::SIGN_IN_NET_UNREACHABLE.to_string(),
        v2_lib::auth::SIGN_IN_NET_TIMEOUT.to_string(),
        format!("{SIGN_IN_UNAVAILABLE} (temporarily_unavailable). Try again in a few minutes."),
    ] {
        let store = MemoryStore::default();
        save(&store, "rt", None).unwrap();
        let out = resume_with(&store, move |_, _| async move { Err(err) }).await;
        assert!(matches!(out, Resume::Unreachable));
        assert!(load(&store).unwrap().is_some());
    }
}

#[test]
fn only_a_refusal_counts_as_one() {
    assert!(is_refusal(&format!("{SIGN_IN_REFUSED} (invalid_grant). Sign in again.")));
    assert!(!is_refusal(v2_lib::auth::SIGN_IN_NET_UNREACHABLE));
    assert!(!is_refusal(&format!("{SIGN_IN_UNAVAILABLE} (503).")));
}

/// The real token endpoint's answers, through the real parsing: a 400 is a
/// verdict on the token, a 503 is Microsoft's side and must not cost
/// anyone their kept sign-in.
#[tokio::test]
async fn the_token_endpoint_tells_a_refusal_from_trouble() {
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    for (status, body, refused) in [
        (400, serde_json::json!({ "error": "invalid_grant" }), true),
        (400, serde_json::json!({ "error": "interaction_required" }), true),
        (503, serde_json::json!({ "error": "temporarily_unavailable" }), false),
        (429, serde_json::json!({}), false),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .mount(&server)
            .await;
        let err = match v2_lib::auth::refresh_at(&format!("{}/token", server.uri()), "rt", None).await {
            Ok(_) => panic!("{status} is not a success"),
            Err(e) => e,
        };
        assert_eq!(is_refusal(&err), refused, "{status}: {err}");
        assert!(!err.contains("http://") && !err.contains("https://"), "{err}");
        assert!(err.contains("Logs"), "{err}");
    }
}

#[test]
fn after_sign_out_the_browser_asks_which_account() {
    let url = v2_lib::auth::build_authorize_url("c", "http://localhost:1", "s");
    assert!(!url.contains("prompt="), "{url}");
    let choosing = v2_lib::auth::choosing_account(url.clone());
    assert!(choosing.starts_with(&url));
    assert!(choosing.ends_with("&prompt=select_account"), "{choosing}");
}
