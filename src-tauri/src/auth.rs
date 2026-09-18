//! Microsoft Entra ID sign-in via OAuth2 authorization-code + PKCE (RFC 7636)
//! against the well-known Azure CLI public client — no app registration, no
//! PATs, no client secret. Tokens live in [`AuthState`] in Rust memory only
//! and must never be returned over IPC (enforced by tests/bindings.rs).

use base64::Engine;
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

const CLIENT_ID: &str = "04b07795-8ddb-461a-bbee-02f9e1bf7b46"; // Azure CLI public client
const AUTHORITY: &str = "https://login.microsoftonline.com/organizations";
const SCOPE: &str = "499b84ac-1321-427f-aa17-267ca6975798/.default offline_access openid profile";
const TOKEN_URL: &str = "https://login.microsoftonline.com/organizations/oauth2/v2.0/token";

/// How long the browser has to come back with the redirect.
pub const SIGN_IN_WINDOW: Duration = Duration::from_secs(300);
/// How long one loopback connection may take to send its request line.
pub const LOOPBACK_READ_TIMEOUT: Duration = Duration::from_secs(10);
/// What a sign-in nobody finished says.
pub const SIGN_IN_TIMEOUT: &str = "Sign-in timed out. Try again.";

/// Refresh this long before the access token actually expires.
const EARLY_RENEW: Duration = Duration::from_secs(300);

pub fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// RFC 7636: verifier is 43-128 chars of unreserved charset; challenge = b64url(sha256(verifier)).
pub fn pkce_pair() -> (String, String) {
    let random: [u8; 32] = rand::random();
    let verifier = b64url(&random);
    let challenge = b64url(&Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

/// The full token set, Rust-side only. Deliberately does NOT derive
/// specta::Type / Serialize: nothing here may ever cross IPC.
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<Instant>,
    pub account: Option<String>,
}

#[derive(Default)]
pub struct AuthState {
    pub tokens: Option<TokenSet>,
}

/// True when the access token is missing an expiry or within EARLY_RENEW of it.
pub fn needs_refresh(expires_at: Option<Instant>, now: Instant) -> bool {
    match expires_at {
        None => true,
        Some(at) => now + EARLY_RENEW >= at,
    }
}

/// Store refreshed tokens only if the session they were refreshed FROM is
/// still the current one. A refresh for account A that finishes after the
/// user signed in as B (or signed out) must not replace B's tokens.
pub fn store_refreshed(state: &mut AuthState, sent_refresh_token: &str, fresh: TokenSet) -> bool {
    let still_current =
        state.tokens.as_ref().and_then(|t| t.refresh_token.as_deref()) == Some(sent_refresh_token);
    if still_current {
        state.tokens = Some(fresh);
    }
    still_current
}

pub fn build_authorize_url(challenge: &str, redirect_uri: &str, state: &str) -> String {
    format!(
        "{AUTHORITY}/oauth2/v2.0/authorize?client_id={CLIENT_ID}&response_type=code&redirect_uri={}&scope={}&code_challenge={challenge}&code_challenge_method=S256&state={state}",
        urlencoding::encode(redirect_uri),
        urlencoding::encode(SCOPE),
    )
}

#[derive(serde::Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    id_token: Option<String>,
}

impl TokenResponse {
    fn into_token_set(self, fallback_account: Option<String>) -> TokenSet {
        let account = self
            .id_token
            .as_deref()
            .and_then(upn_from_id_token)
            .or(fallback_account);
        TokenSet {
            access_token: self.access_token,
            refresh_token: self.refresh_token,
            expires_at: self
                .expires_in
                .map(|secs| Instant::now() + Duration::from_secs(secs)),
            account,
        }
    }
}

/// Extract the UPN ("preferred_username") from an id_token without signature
/// verification — display only, never used for authorization decisions.
pub fn upn_from_id_token(id_token: &str) -> Option<String> {
    let payload = id_token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    claims["preferred_username"].as_str().map(String::from)
}

/// Form params for the refresh_token grant (pure, for tests).
pub fn refresh_params(refresh_token: &str) -> Vec<(&'static str, String)> {
    vec![
        ("client_id", CLIENT_ID.to_string()),
        ("grant_type", "refresh_token".to_string()),
        ("refresh_token", refresh_token.to_string()),
        ("scope", SCOPE.to_string()),
    ]
}

/// A transport failure as one of the no-URL sentences (`ado/transport.rs`).
/// reqwest's Display names the login endpoint, which nobody can act on.
fn sign_in_network_error(e: &reqwest::Error) -> String {
    match crate::ado::network_error(e) {
        crate::ado::AdoError::Network(m) => m,
        other => other.to_string(),
    }
}

async fn post_token_endpoint(url: &str, params: &[(&str, String)]) -> Result<TokenResponse, String> {
    // Entra's real token endpoint is never loopback; only tests point this
    // at a wiremock `MockServer`, so the same pooling hazard `ado/mod.rs`
    // documents for `AdoClient` applies here too.
    let client = if crate::ado::is_loopback_base(url) {
        crate::ado::unpooled_loopback_client()
    } else {
        crate::ado::http_client()
    };
    let resp = client
        .post(url)
        .form(params)
        .send()
        .await
        .map_err(|e| {
            crate::applog::warn(format!("token endpoint request failed: {e}"));
            sign_in_network_error(&e)
        })?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        crate::applog::warn(format!(
            "token endpoint returned {status}: {}",
            body.chars().take(600).collect::<String>()
        ));
        // Entra's `error` code (invalid_grant, ...) - never `error_uri` or
        // the description, which carry URLs and trace ids.
        let code: String = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v["error"].as_str().map(str::to_string))
            .unwrap_or_else(|| status.as_u16().to_string())
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .take(40)
            .collect();
        return Err(format!(
            "Microsoft sign-in refused the request ({code}). Sign in again - Settings → Logs has the details."
        ));
    }
    resp.json().await.map_err(|e| {
        crate::applog::warn(format!("token endpoint response could not be read: {e}"));
        sign_in_network_error(&e)
    })
}

/// `refresh` against a given token endpoint - the seam the no-URL tests use.
pub async fn refresh_at(
    token_url: &str,
    refresh_token: &str,
    account: Option<String>,
) -> Result<TokenSet, String> {
    let params = refresh_params(refresh_token);
    let tokens = post_token_endpoint(token_url, &params).await?;
    Ok(tokens.into_token_set(account))
}

/// Exchange a refresh token for a new token set (silent renew).
pub async fn refresh(refresh_token: &str, account: Option<String>) -> Result<TokenSet, String> {
    refresh_at(TOKEN_URL, refresh_token, account).await
}

async fn exchange_code(
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<TokenResponse, String> {
    let params = [
        ("client_id", CLIENT_ID.to_string()),
        ("grant_type", "authorization_code".to_string()),
        ("code", code.to_string()),
        ("redirect_uri", redirect_uri.to_string()),
        ("code_verifier", verifier.to_string()),
        ("scope", SCOPE.to_string()),
    ];
    post_token_endpoint(TOKEN_URL, &params).await
}

/// The one page a user sees outside the app - make it feel like ours.
const SIGNED_IN_PAGE: &str = r##"<!DOCTYPE html><html lang="en"><head><meta charset="utf-8">
<title>Signed in - Test Case Manager</title>
<style>
  * { margin:0; box-sizing:border-box; }
  body { min-height:100vh; display:flex; align-items:center; justify-content:center;
         font-family:'Segoe UI',system-ui,sans-serif; color:#f8fafc;
         background:radial-gradient(1200px 600px at 20% -10%, #1e3a5f 0%, #0f172a 55%, #0a0f1e 100%); }
  .card { text-align:center; padding:56px 64px; border:1px solid rgba(148,163,184,.25);
          border-radius:20px; background:rgba(30,41,59,.55); backdrop-filter:blur(8px);
          box-shadow:0 24px 80px rgba(0,0,0,.45); animation:pop .5s cubic-bezier(.2,.9,.3,1.2) both; }
  .mark { width:84px; height:84px; margin:0 auto 20px; border-radius:22px;
          background:linear-gradient(135deg,#2aa5e0,#1565c0); display:flex;
          align-items:center; justify-content:center; box-shadow:0 10px 30px rgba(21,101,192,.5); }
  .mark svg { width:44px; height:44px; stroke:#fff; fill:none; stroke-width:2;
              stroke-linecap:round; stroke-linejoin:round; }
  h1 { font-size:26px; font-weight:600; margin-bottom:8px; }
  p  { color:#94a3b8; font-size:15px; line-height:1.6; }
  .check { display:inline-flex; align-items:center; gap:8px; margin-top:22px;
           padding:8px 18px; border-radius:999px; font-size:13.5px; font-weight:600;
           color:#4ade80; background:rgba(34,197,94,.12); border:1px solid rgba(74,222,128,.35);
           animation:fade .6s .25s both; }
  @keyframes pop  { from { opacity:0; transform:translateY(14px) scale(.97); } }
  @keyframes fade { from { opacity:0; } }
</style></head><body>
<div class="card">
  <div class="mark"><svg viewBox="0 0 24 24"><path d="M10 2v7.31L4.29 19.7A2 2 0 0 0 6.05 22h11.9a2 2 0 0 0 1.76-2.3L14 9.31V2"/><path d="M8.5 2h7"/><path d="M7 16h10"/></svg></div>
  <h1>You're signed in</h1>
  <p>Head back to <b>Test Case Manager</b> - it already has your session.<br>This tab can be closed.</p>
  <span class="check">&#10003;&nbsp;Authentication complete</span>
</div>
<script>setTimeout(function(){ try { window.close(); } catch(e){} }, 2500);</script>
</body></html>"##;

/// Shown when the browser came back without a usable answer.
const FAILED_PAGE: &str = r##"<!DOCTYPE html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Sign-in did not complete - Test Case Manager</title>
<style>
  * { margin:0; box-sizing:border-box; }
  body { min-height:100vh; display:flex; align-items:center; justify-content:center;
         font-family:'Segoe UI',system-ui,sans-serif; color:#f8fafc; background:#0f172a; }
  .card { text-align:center; padding:48px 56px; border:1px solid rgba(148,163,184,.25);
          border-radius:20px; background:rgba(30,41,59,.55); }
  h1 { font-size:24px; font-weight:600; margin-bottom:8px; }
  p  { color:#94a3b8; font-size:15px; line-height:1.6; }
</style></head><body>
<div class="card">
  <h1>Sign-in did not complete</h1>
  <p>Go back to <b>Test Case Manager</b> and sign in again.<br>This tab can be closed.</p>
</div>
</body></html>"##;

/// What the loopback made of one request line.
#[derive(Debug, PartialEq, Eq)]
pub enum Redirect {
    /// The authorization code, state checked.
    Code(String),
    /// The browser came back, but not with a sign-in: denied, wrong state,
    /// no code. The sentence is for the user and names no URL.
    Refused(String),
    /// Not the redirect at all (favicon, a preconnect that sent nothing).
    NotTheRedirect,
}

pub fn read_redirect(request_line: &str, expected_state: &str) -> Redirect {
    let Some(target) = request_line
        .strip_prefix("GET ")
        .and_then(|r| r.split_whitespace().next())
    else {
        return Redirect::NotTheRedirect;
    };
    let Some(query) = target.strip_prefix("/?") else {
        return Redirect::NotTheRedirect;
    };
    let (mut code, mut state, mut error) = (None, None, None);
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("code", v)) => code = Some(v.to_string()),
            Some(("state", v)) => state = Some(v.to_string()),
            Some(("error", v)) => error = Some(v.to_string()),
            _ => {}
        }
    }
    if state.as_deref() != Some(expected_state) {
        return Redirect::Refused("The sign-in reply did not belong to this attempt. Try again.".into());
    }
    if let Some(e) = error {
        let e: String = e.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').take(40).collect();
        return Redirect::Refused(format!("Sign-in was not completed ({e}). Try again."));
    }
    match code {
        Some(c) if !c.is_empty() => Redirect::Code(c),
        _ => Redirect::Refused("The sign-in reply carried no authorization code. Try again.".into()),
    }
}

fn respond(stream: &mut std::net::TcpStream, status: &str, body: &str) {
    use std::io::Write;
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

/// Wait on the loopback for the browser's redirect, for at most `window`.
///
/// Non-blocking accept so the deadline is real (a closed browser tab used
/// to leave sign-in pending forever, leaking a thread and a port). Each
/// connection gets `read_timeout` to send its request line; one that sends
/// nothing (a browser preconnect) or something else (a favicon) is dropped
/// and the wait goes on, so it cannot hide the real redirect behind it.
pub fn await_redirect(
    listener: std::net::TcpListener,
    expected_state: &str,
    window: Duration,
    read_timeout: Duration,
) -> Result<String, String> {
    use std::io::{BufRead, BufReader, ErrorKind, Read};
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + window;
    loop {
        if Instant::now() >= deadline {
            return Err(SIGN_IN_TIMEOUT.into());
        }
        let mut stream = match listener.accept() {
            Ok((s, _)) => s,
            Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::Interrupted => {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(e) => return Err(e.to_string()),
        };
        // An accepted socket inherits the listener's non-blocking mode on
        // Windows; the read below must block (up to its timeout).
        if stream.set_nonblocking(false).is_err() {
            continue;
        }
        let _ = stream.set_read_timeout(Some(read_timeout));
        let _ = stream.set_write_timeout(Some(read_timeout));
        let Ok(reader) = stream.try_clone() else { continue };
        let mut line = String::new();
        if BufReader::new(reader.take(8192)).read_line(&mut line).is_err() {
            continue; // said nothing in time: drop it, keep waiting
        }
        match read_redirect(&line, expected_state) {
            Redirect::NotTheRedirect => respond(&mut stream, "404 Not Found", ""),
            Redirect::Code(code) => {
                respond(&mut stream, "200 OK", SIGNED_IN_PAGE);
                return Ok(code);
            }
            Redirect::Refused(why) => {
                respond(&mut stream, "200 OK", FAILED_PAGE);
                return Err(why);
            }
        }
    }
}

/// Runs the interactive flow: opens the system browser at the authorize URL,
/// waits for the loopback redirect, exchanges the code.
pub async fn sign_in_interactive(open_url: impl Fn(&str)) -> Result<TokenSet, String> {
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    // Entra ID only allows arbitrary ports on "http://localhost" (RFC 8252
    // loopback), NOT on the literal 127.0.0.1 - using the IP form fails with
    // AADSTS50011 against the Azure CLI app registration. The socket still
    // binds to 127.0.0.1; localhost resolves there for the browser redirect.
    let redirect_uri = format!("http://localhost:{port}");
    let (verifier, challenge) = pkce_pair();
    let state = b64url(&rand::random::<[u8; 16]>());
    open_url(&build_authorize_url(&challenge, &redirect_uri, &state));

    let code = tokio::task::spawn_blocking(move || {
        await_redirect(listener, &state, SIGN_IN_WINDOW, LOOPBACK_READ_TIMEOUT)
    })
    .await
    .map_err(|e| e.to_string())??;

    let tokens = exchange_code(&code, &verifier, &redirect_uri).await?;
    Ok(tokens.into_token_set(None))
}
